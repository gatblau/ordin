//! C6 — Retrieval Engine. The synchronous read path of `recall` (ADR-004: no LLM call, NFR-P1).
//!
//! A fixed pipeline turns an authenticated, scoped [`RecallRequest`] into a bounded, ranked set of
//! facts with confidence, an opaque pagination cursor, and — when the request opts in via
//! `include_provenance` — each sourced fact's `origin_ref` + `modification_marker` so the agent can
//! check source freshness itself (ADR-014) — or an explicit abstention. It performs exactly two
//! read-path model inferences (query embedding and a cross-encoder rerank, ADR-012/ADR-005), each with
//! its own SA-LAT-01 sub-budget inside NFR-P2 (whole-path p95 ≤ 200 ms). It owns no persistence:
//! candidate retrieval, scope/`valid_at` filtering, and source loading are delegated to the Memory
//! Store (C1). `memory` performs no source-change check (ADR-014). This component is pure orchestration
//! plus the ranking, gating, and cursor arithmetic.
//!
//! Pipeline (`recall`): input-guard + cursor-decode → (optional reformulation) → embed → stage-1
//! multi-signal recall → rerank → recency weight → gate/abstain → pagination window → conditional
//! provenance attach → build outcome. Each external step degrades per the spec rather than blocking the
//! response: embed and stage-1 fail fast (a typed error); rerank degrades to stage-1 order; provenance
//! attach is best-effort (a source-load miss leaves that fact's `source` unset). Scope is taken only
//! from `ctx` (built by C3), never from the request body;
//! tenant isolation is structural (a different tenant is a different namespace). Provider keys are
//! env-only and never logged; fact `content` is never logged — only counts and scores.

use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{AppError, ValidationKind};
use crate::maintenance::{is_fading, DecayConfig};
use crate::types::api::{
    RankedFact, RecallLink, RecallRequest, RecallResponse, SourceProvenance, SubQuery, SubQueryKind,
};
use crate::types::domain::Fact;
use crate::types::ports::{
    EmbeddingClient, RecallFilters as StoreFilters, RerankClient, StageOneQuery, StageOneSearch,
    StoragePort,
};
use crate::types::scope::ScopeContext;

/// The resolved C6 configuration (Phase 2D keys owned by C6 unless noted).
#[derive(Clone, Copy)]
pub struct RetrievalConfig {
    /// `MEMORY_EMBED_DIM` — query-vector length must equal this (and the store's index dim).
    pub embed_dim: u32,
    /// `MEMORY_STAGE1_K` — stage-1 candidate fan-out fed to rerank (SA-RERANK-01).
    pub stage1_k: u16,
    /// `MEMORY_RESULT_CAP_MAX` — hard upper bound on `result_cap` (SA-CAP-01).
    pub result_cap_max: u8,
    /// `MEMORY_ABSTAIN_THRESHOLD` — top-final score below which recall abstains (SA-GATE-01).
    pub abstain_threshold: f64,
    /// `MEMORY_RECENCY_WEIGHT` — recency boost weight `w` (SA-RECENCY-01).
    pub recency_weight: f64,
    /// `MEMORY_RECENCY_TAU_DAYS` — recency decay constant `τ` in days.
    pub recency_tau_days: f64,
    /// `MEMORY_REFORMULATION_ENABLED` — A/B flag gating the optional reformulation step (default off).
    pub reformulation_enabled: bool,
    /// `MEMORY_GRAPH_HOPS` — link-expansion depth, 0..=2 (ADR-021).
    pub graph_hops: u8,
    /// `MEMORY_GRAPH_FANOUT` — neighbours followed per hop, 1..=256.
    pub graph_fanout: u32,
    /// `MEMORY_GRAPH_CANDIDATE_CAP` — facts the graph and link lists may contribute, 1..=1000.
    pub graph_cap: u32,
    /// `MEMORY_TRUST_WEIGHT` — weight of source trust in the final score, 0..=0.5 (SA-GRAPH-08).
    pub trust_weight: f64,
    /// The decay curve `fading` is read against (C7, SA-GRAPH-07).
    pub decay: DecayConfig,
    /// `MEMORY_RECALL_TIME_LIMIT_SECS` — overall time limit of one recall (SA-COLL-06).
    pub time_limit: std::time::Duration,
}

impl RetrievalConfig {
    /// Project the resolved [`Config`] onto the C6 subset.
    pub fn from_config(c: &Config) -> Self {
        Self {
            embed_dim: c.embed_dim,
            stage1_k: c.stage1_k,
            result_cap_max: c.result_cap_max,
            abstain_threshold: c.abstain_threshold,
            recency_weight: c.recency_weight,
            recency_tau_days: c.recency_tau_days,
            reformulation_enabled: c.reformulation_enabled,
            graph_hops: c.graph_hops,
            graph_fanout: c.graph_fanout,
            graph_cap: c.graph_cap,
            trust_weight: c.trust_weight,
            decay: DecayConfig {
                decay_k: c.decay_k,
                prune_retrievability: c.prune_retrievability,
            },
            time_limit: std::time::Duration::from_secs(u64::from(c.recall_time_limit_secs)),
        }
    }
}

/// What C6 returns to C8: the response payload plus the `Meta` fields C8 serialises.
pub struct RecallOutcome {
    /// `{ facts: Vec<RankedFact> }`.
    pub response: RecallResponse,
    /// Opaque cursor; `None` on the last page or on abstain.
    pub next_cursor: Option<String>,
    /// `true` => `facts` is empty by gating, not by absence.
    pub abstained: bool,
}

/// The opaque pagination cursor payload — `(final_score, fact_id)` of the last emitted fact
/// (SA-PAGE-01). Encoded as base64url(JSON); never persisted.
#[derive(Serialize, Deserialize, Debug)]
struct Cursor {
    /// Final score of the last emitted fact.
    s: f64,
    /// Id of the last emitted fact.
    id: String,
}

impl Cursor {
    fn encode(&self) -> Result<String, AppError> {
        let json = serde_json::to_vec(self).map_err(|_| AppError::Internal)?;
        Ok(URL_SAFE_NO_PAD.encode(json))
    }

    fn decode(raw: &str) -> Result<Cursor, AppError> {
        let bytes = URL_SAFE_NO_PAD.decode(raw.as_bytes()).map_err(|_| {
            AppError::Validation(ValidationKind::OutOfRange, "undecodable cursor".into())
        })?;
        serde_json::from_slice(&bytes).map_err(|_| {
            AppError::Validation(ValidationKind::OutOfRange, "malformed cursor".into())
        })
    }
}

/// A candidate carried through ranking: the fact, the final score assigned in the recency step, and
/// the links that reached it (step 10″).
struct Scored {
    fact: Fact,
    final_score: f64,
    graph_hop: Option<u8>,
    via: Vec<String>,
}

/// Links `RecallResponse` carries at most (step 10″).
const MAX_RECALL_LINKS: usize = 20;

/// The synchronous read path. Holds the injected store/provider seams and the resolved config;
/// construct once and share via `Arc`. Freshness is agent-side (ADR-014): no freshness seam.
pub struct RetrievalEngine {
    store: Arc<dyn StoragePort>,
    embedder: Arc<dyn EmbeddingClient>,
    reranker: Arc<dyn RerankClient>,
    config: RetrievalConfig,
}

impl RetrievalEngine {
    pub fn new(
        store: Arc<dyn StoragePort>,
        embedder: Arc<dyn EmbeddingClient>,
        reranker: Arc<dyn RerankClient>,
        config: RetrievalConfig,
    ) -> Self {
        Self {
            store,
            embedder,
            reranker,
            config,
        }
    }

    /// The synchronous read path. `ctx` is the authenticated scope (never derived from `req`); `req`
    /// is the validated recall request. Returns the ranked facts plus the `Meta` fields C8 serialises.
    /// Past `time_limit`, in any step, the recall ends with `AppError::RecallTimeout`.
    // spec: phase-1-analysis#SA-COLL-06
    pub async fn recall(
        &self,
        ctx: &ScopeContext,
        req: &RecallRequest,
    ) -> Result<RecallOutcome, AppError> {
        match tokio::time::timeout(self.config.time_limit, self.run_recall(ctx, req)).await {
            Ok(outcome) => outcome,
            Err(_) => {
                tracing::warn!(
                    target: "memory",
                    correlation_id = %ctx.correlation_id,
                    "retrieval.time_limit"
                );
                Err(AppError::RecallTimeout)
            }
        }
    }

    async fn run_recall(
        &self,
        ctx: &ScopeContext,
        req: &RecallRequest,
    ) -> Result<RecallOutcome, AppError> {
        // Step 1 — input guard and pagination decode. Step 1′: exactly one of a non-empty `query` and
        // a non-empty `searches`, 1–4 sub-queries of 1–4,096 characters, and a well-formed collection
        // filter (SA-SUBQ-01, ADR-020).
        let searches = validate_request_shape(req)?;
        if req.result_cap == 0 || req.result_cap > self.config.result_cap_max {
            return Err(AppError::Validation(
                ValidationKind::OutOfRange,
                format!("result_cap={}", req.result_cap),
            ));
        }
        let effective_cap = req.result_cap.min(self.config.result_cap_max) as usize;
        let cursor = match &req.cursor {
            Some(raw) => Some(Cursor::decode(raw)?),
            None => None,
        };

        // Step 2 — optional query reformulation (A/B-gated, off by default per ADR-012 / good-mem §7.3).
        // The rerank query is the plain `query`, or else the first sub-query's text (SA-SUBQ-03).
        // spec: components/retrieval-engine#SA-SUBQ-03
        let effective_query = match searches.first() {
            Some(first) => first.query.clone(),
            None => req.query.clone(),
        };
        tracing::debug!(
            target: "memory",
            correlation_id = %ctx.correlation_id,
            query_len = effective_query.len(),
            searches = searches.len(),
            collections = req.filters.collections.as_ref().map_or(0, Vec::len),
            result_cap_effective = effective_cap,
            cursor_present = cursor.is_some(),
            reformulation_enabled = self.config.reformulation_enabled,
            "retrieval.input"
        );

        // Step 3 — embed the query (read-path inference #1). Fail fast on any provider error. Step 3′:
        // a typed request embeds every `vec` sub-query in one call, and none when it has none.
        let (query_vector, keyword_terms_plain, stage_searches) = if searches.is_empty() {
            let query_vector = self.embed_one(&effective_query).await?;
            (query_vector, keyword_terms(&effective_query), Vec::new())
        } else {
            (Vec::new(), Vec::new(), self.typed_searches(searches).await?)
        };

        // Step 4 — stage-1 multi-signal recall (store applies the read filter using ctx). Fail fast.
        // Step 4′: a typed request hands C1 one search per sub-query; the collection filter passes
        // through.
        let stage_query = StageOneQuery {
            query_vector,
            keyword_terms: keyword_terms_plain,
            searches: stage_searches,
            filters: StoreFilters {
                memory_class: req.filters.memory_class,
                visibility: req.filters.visibility,
                entity: req.filters.entity.clone(),
                valid_at: req.filters.valid_at,
                collections: req.filters.collections.clone(),
            },
            scope: ctx.clone(),
            stage1_k: self.config.stage1_k,
            // Step 4″ — the graph settings, and the texts entity phrases are cut from.
            graph_hops: self.config.graph_hops,
            graph_fanout: self.config.graph_fanout,
            graph_cap: self.config.graph_cap,
            phrase_texts: if searches.is_empty() {
                vec![req.query.clone()]
            } else {
                searches.iter().map(|s| s.query.clone()).collect()
            },
        };
        let candidates = self.store.recall(ctx, &stage_query).await?;
        tracing::debug!(
            target: "memory",
            correlation_id = %ctx.correlation_id,
            stage1_candidates = candidates.len(),
            "retrieval.stage1"
        );
        if candidates.is_empty() {
            // No candidates -> abstain with an empty page (step 7 outcome, reached early).
            return Ok(RecallOutcome {
                response: RecallResponse {
                    facts: vec![],
                    links: vec![],
                },
                next_cursor: None,
                abstained: true,
            });
        }

        // Step 5 — cross-encoder rerank (inference #2). Degrade to stage-1 order on any provider error.
        let docs: Vec<String> = candidates
            .iter()
            .map(|c| c.fact.content.to_string())
            .collect();
        let rerank_scores = match self.reranker.rerank(&effective_query, &docs).await {
            Ok(scores) => Some(scores),
            Err(e) => {
                tracing::warn!(
                    target: "memory",
                    correlation_id = %ctx.correlation_id,
                    rerank_skipped = true,
                    detail = %e,
                    "retrieval.rerank_degraded"
                );
                None
            }
        };

        // Step 6 — recency weighting; assign the final score and sort (final desc, id asc).
        let now = Utc::now();
        let mut scored: Vec<Scored> = candidates
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                // Step 5′: a candidate with no aligned score, and every candidate on the degraded
                // path, takes its fused_norm — so the degraded order is C1's fused order.
                let rerank_score = match &rerank_scores {
                    Some(scores) => scores
                        .get(i)
                        .copied()
                        .unwrap_or(c.fused_norm)
                        .clamp(0.0, 1.0),
                    None => c.fused_norm,
                };
                let age_days =
                    ((now - c.fact.ingested_at).num_seconds() as f64 / 86_400.0).max(0.0);
                // Step 6′ — the trust factor (SA-GRAPH-08).
                // spec: components/retrieval-engine#SA-GRAPH-08
                let final_score = recency_final_score(
                    rerank_score,
                    age_days,
                    self.config.recency_weight,
                    self.config.recency_tau_days,
                ) * trust_factor(self.config.trust_weight, c.source_trust);
                Scored {
                    fact: c.fact,
                    final_score,
                    graph_hop: c.graph_hop,
                    via: c.via,
                }
            })
            .collect();
        scored.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.fact.id.cmp(&b.fact.id))
        });

        // Step 7 — retrieval gating / abstain.
        let top_final = scored.first().map(|s| s.final_score).unwrap_or(0.0);
        if top_final < self.config.abstain_threshold {
            tracing::debug!(
                target: "memory",
                correlation_id = %ctx.correlation_id,
                top_final_score = top_final,
                abstained = true,
                "retrieval.gate"
            );
            return Ok(RecallOutcome {
                response: RecallResponse {
                    facts: vec![],
                    links: vec![],
                },
                next_cursor: None,
                abstained: true,
            });
        }

        // Step 8 — pagination window: drop everything not strictly after the cursor, then truncate.
        let surviving_after_cursor: Vec<Scored> = match &cursor {
            // The cursor's score travels as JSON, and serde_json's default float parser
            // is not correctly rounded, so a score can come back one ULP away from the one encoded
            // (seen once the trust factor of step 6′ made scores like 1.08675 common). Scores within
            // a relative 1e-12 of the cursor's are the cursor's own score; the form is unchanged.
            Some(cur) => scored
                .into_iter()
                .filter(|s| {
                    let same = (s.final_score - cur.s).abs() <= 1e-12 * cur.s.abs().max(1.0);
                    (!same && s.final_score < cur.s) || (same && s.fact.id > cur.id)
                })
                .collect(),
            None => scored,
        };
        let remaining = surviving_after_cursor.len();
        let page: Vec<Scored> = surviving_after_cursor
            .into_iter()
            .take(effective_cap)
            .collect();
        let has_more = remaining > page.len();

        // Step 9 — conditional provenance attach (SA-PROV-01, ADR-014). When the caller opts in,
        // surface each sourced fact's origin_ref + modification_marker so the agent can run its own
        // source-freshness check. Best-effort: a get_source miss/error simply omits that fact's source.
        let mut provenance: std::collections::HashMap<String, SourceProvenance> =
            std::collections::HashMap::new();
        if req.include_provenance {
            for s in &page {
                if let Some(sid) = &s.fact.source_id {
                    if let Ok(Some(src)) = self.store.get_source(ctx, sid).await {
                        provenance.insert(
                            s.fact.id.clone(),
                            SourceProvenance {
                                origin_ref: src.origin_ref,
                                modification_marker: src.modification_marker,
                            },
                        );
                    }
                }
            }
        }

        // Step 10 — build the outcome.
        let next_cursor = if has_more {
            page.last()
                .map(|last| {
                    Cursor {
                        s: last.final_score,
                        id: last.fact.id.clone(),
                    }
                    .encode()
                })
                .transpose()?
        } else {
            None
        };
        // Step 10″ — the links that led to the page's facts.
        let links = self.recall_links(ctx, &page).await;
        let facts: Vec<RankedFact> = page
            .into_iter()
            .map(|s| {
                let source = provenance.remove(&s.fact.id);
                RankedFact {
                    // SA-GRAPH-07: a signal to the reader, not a ranking input.
                    fading: is_fading(&s.fact, now, &self.config.decay),
                    fact: s.fact,
                    score: s.final_score.clamp(0.0, 1.0),
                    source,
                }
            })
            .collect();
        tracing::debug!(
            target: "memory",
            correlation_id = %ctx.correlation_id,
            facts_returned = facts.len(),
            next_cursor_present = next_cursor.is_some(),
            "retrieval.outcome"
        );
        Ok(RecallOutcome {
            response: RecallResponse { facts, links },
            next_cursor,
            abstained: false,
        })
    }

    /// Step 10″'s `links`: for each page fact reached at hop ≥ 1, each relationship of its `via`
    /// (read-filtered; a miss is skipped) with both endpoints' canonical names, grouped by relationship
    /// in the order of its first reached fact, at most 20. Store errors are absorbed: the fact is still
    /// returned without its link.
    async fn recall_links(&self, ctx: &ScopeContext, page: &[Scored]) -> Vec<RecallLink> {
        let mut links: Vec<RecallLink> = Vec::new();
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut names: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();
        let mut partial = false;
        for s in page {
            if s.graph_hop.unwrap_or(0) == 0 {
                continue;
            }
            for rid in &s.via {
                if let Some(&i) = index.get(rid) {
                    if !links[i].reached.contains(&s.fact.id) {
                        links[i].reached.push(s.fact.id.clone());
                    }
                    continue;
                }
                if links.len() >= MAX_RECALL_LINKS {
                    continue;
                }
                let rel = match self.store.get_relationship(ctx, rid).await {
                    Ok(Some(r)) => r,
                    Ok(None) => continue,
                    Err(_) => {
                        partial = true;
                        continue;
                    }
                };
                let mut ends: [Option<String>; 2] = [None, None];
                for (slot, id) in [&rel.from, &rel.to].into_iter().enumerate() {
                    if !names.contains_key(id) {
                        let n = match self.store.get_entity(ctx, id).await {
                            Ok(e) => e.map(|e| e.canonical_name),
                            Err(_) => None,
                        };
                        names.insert(id.clone(), n);
                    }
                    ends[slot] = names.get(id).cloned().flatten();
                }
                let [from, to] = ends;
                let (Some(from_name), Some(to_name)) = (from, to) else {
                    partial = true;
                    continue;
                };
                index.insert(rid.clone(), links.len());
                links.push(RecallLink {
                    relationship: rel,
                    from_name,
                    to_name,
                    reached: vec![s.fact.id.clone()],
                });
            }
        }
        if partial {
            tracing::warn!(target: "memory", correlation_id = %ctx.correlation_id, "retrieval.links_partial");
        }
        links
    }
}

/// Step 6′'s trust factor: `1 + MEMORY_TRUST_WEIGHT · (source_trust − 0.5)` (SA-GRAPH-08).
fn trust_factor(weight: f64, source_trust: f64) -> f64 {
    1.0 + weight * (source_trust - 0.5)
}

impl RetrievalEngine {
    /// Step 3 for a plain request: one embedding call for the query.
    async fn embed_one(&self, query: &str) -> Result<Vec<f32>, AppError> {
        let mut embeddings = self.embedder.embed(&[query.to_string()]).await?;
        if embeddings.is_empty() {
            return Err(AppError::Provider(
                crate::types::ports::ProviderError::Malformed("empty embedding response".into()),
            ));
        }
        let query_vector = embeddings.swap_remove(0);
        if query_vector.len() != self.config.embed_dim as usize {
            // A dim mismatch is a misconfiguration startup should have caught (SA-EMBED-01).
            return Err(AppError::Internal);
        }
        Ok(query_vector)
    }

    /// Steps 3′ and 4′: embed the `vec` sub-queries' texts in one call (element `i` is the vector of
    /// the `i`-th `vec` sub-query), then build one search per sub-query in request order, the first
    /// weighted 2.0 and every other 1.0 (SA-SUBQ-02).
    // spec: components/retrieval-engine#SA-SUBQ-02
    async fn typed_searches(&self, searches: &[SubQuery]) -> Result<Vec<StageOneSearch>, AppError> {
        let texts: Vec<String> = searches
            .iter()
            .filter(|s| s.kind == SubQueryKind::Vec)
            .map(|s| s.query.clone())
            .collect();
        let mut vectors = if texts.is_empty() {
            Vec::new()
        } else {
            self.embedder.embed(&texts).await?
        };
        if vectors.len() != texts.len() {
            return Err(AppError::Provider(
                crate::types::ports::ProviderError::Malformed("embedding response count".into()),
            ));
        }
        if vectors
            .iter()
            .any(|v| v.len() != self.config.embed_dim as usize)
        {
            return Err(AppError::Internal);
        }
        vectors.reverse();
        Ok(searches
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let weight = if i == 0 { 2.0 } else { 1.0 };
                match s.kind {
                    SubQueryKind::Vec => StageOneSearch {
                        kind: SubQueryKind::Vec,
                        vector: vectors.pop().unwrap_or_default(),
                        terms: Vec::new(),
                        weight,
                    },
                    SubQueryKind::Lex => StageOneSearch {
                        kind: SubQueryKind::Lex,
                        vector: Vec::new(),
                        terms: keyword_terms(&s.query),
                        weight,
                    },
                }
            })
            .collect())
    }
}

/// Steps 1 and 1′ — the request's shape: exactly one of a non-empty `query` (1–4,096 characters) and
/// a non-empty `searches` (1–4 sub-queries of 1–4,096 characters each), and `filters.collections`,
/// when present, 1–32 distinct labels each matching SA-COLL-01. Returns the sub-queries (empty for a
/// plain request). Every breach is `VAL_OUT_OF_RANGE`.
// spec: components/retrieval-engine#SA-SUBQ-01
fn validate_request_shape(req: &RecallRequest) -> Result<&[SubQuery], AppError> {
    let out_of_range = |what: &str| AppError::Validation(ValidationKind::OutOfRange, what.into());
    let searches: &[SubQuery] = req.searches.as_deref().unwrap_or(&[]);
    let has_query = !req.query.trim().is_empty();
    match (has_query, searches.is_empty()) {
        (true, false) | (false, true) => {
            return Err(out_of_range("exactly one of query and searches"));
        }
        (true, true) => {
            if req.query.chars().count() > 4096 {
                return Err(out_of_range("query length out of range"));
            }
        }
        (false, false) => {
            if searches.len() > 4 {
                return Err(out_of_range("searches count out of range"));
            }
            if searches
                .iter()
                .any(|s| s.query.trim().is_empty() || s.query.chars().count() > 4096)
            {
                return Err(out_of_range("sub-query length out of range"));
            }
        }
    }
    if let Some(cs) = &req.filters.collections {
        let mut seen = std::collections::HashSet::new();
        if cs.is_empty()
            || cs.len() > 32
            || cs
                .iter()
                .any(|c| !crate::types::domain::is_valid_collection(c) || !seen.insert(c.as_str()))
        {
            return Err(out_of_range("filters.collections out of range"));
        }
    }
    Ok(searches)
}

/// Apply the SA-RECENCY-01 recency boost: `final = rerank · (1 + w·exp(−age_days/τ))`. `age_days` is
/// floored at 0 by the caller. Pure arithmetic, factored out for table-driven testing.
fn recency_final_score(rerank_score: f64, age_days: f64, w: f64, tau: f64) -> f64 {
    let recency_boost = w * (-age_days / tau).exp();
    rerank_score * (1.0 + recency_boost)
}

/// Extract BM25 keyword terms from the query: split on non-alphanumerics, lowercase, drop empties,
/// de-duplicate (order-preserving). The store's `memory_text` analyzer does the real matching; this
/// only feeds candidate terms (C6 *Gaps* — keyword tokenisation follows C1's analyzer).
pub(crate) fn keyword_terms(query: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

#[cfg(test)]
mod tests;
