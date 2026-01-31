//! The collection leg (tenancy-benchmark amendment January 2026, ADR-020): whether a small
//! collection's best fact reaches the stage-1 candidates beside a large one, whether an exact rare
//! term keeps its place, whether a collection filter returns a full page, and how long stage-1 takes
//! with four typed sub-queries over several collections — each beside its **baseline**, the same
//! corpus and queries through the as-built merge (a map keyed by fact id, cut at `stage1_k` in key
//! order, no collections).
//!
//! The leg stays on C1 (`MemoryStore::recall`) with no model provider: synthetic embeddings make the
//! fairness geometry exact, and the order the exact-term run is read against is the fused order — the
//! final order whenever the reranker is unavailable (C6 step 5′). **No verdict is written**, as for
//! every leg.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bench::rig::{Rig, Timing};
use crate::bench::{BenchConfig, BenchError};
use crate::store::surreal::{Store, VectorStrategy};
use crate::types::api::SubQueryKind;
use crate::types::domain::{Fact, MemoryClass, Visibility};
use crate::types::placement::MemoryProfile;
use crate::types::ports::{MemoryStore, RecallFilters, StageOneQuery, StageOneSearch, StoreError};
use crate::types::scope::{OpSet, ScopeContext, ScopeRef};

/// The large and the small fairness collections (SA-BENCH-COLL-01).
const LARGE: &str = "tk:a";
const SMALL: &str = "tk:b";
/// The fairness queries (SA-BENCH-COLL-01: 20 fixed queries).
const FAIRNESS_QUERIES: u32 = 20;
/// The filtered run's `stage1_k` (step 4).
const FILTERED_K: u16 = 50;
/// The leg's one team scope; every fact is team-shared within it.
const TEAM: &str = "ws:bench";
/// The reader, who also owns every fact: the rig runs under `MemoryProfile::Individual`, whose read
/// filter is the owner equality.
const READER: &str = "u-me";
/// The embedding model version stamped on every generated fact.
const MODEL_VERSION: &str = "bench-collections-v1";
/// The most checks a fixture file holds, and its largest size (SA-BENCH-COLL-02).
const FIXTURE_MAX_CHECKS: usize = 100;
const FIXTURE_MAX_BYTES: u64 = 256 * 1024;

/// The collection leg's report (`BenchReport.collections`).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct CollectionLegReport {
    /// Fairness run.
    pub fairness: FairnessRow,
    /// Exact-term run.
    pub exact_term: ExactTermRow,
    /// Filtered run.
    pub filtered: FilteredRow,
    /// Budget run, stage-1 share: 4 sub-queries, `MEMORY_BENCH_COLLECTION_LOAD` collections.
    pub stage1_timing: Timing,
    /// `None` = `MEMORY_BENCH_FIXTURE_DIR` unset.
    pub fixture: Option<Vec<FixtureRow>>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct FairnessRow {
    pub queries: u32,
    pub answer_in_candidates: u32,
    pub baseline_answer_in_candidates: u32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ExactTermRow {
    pub queries: u32,
    pub in_top3: u32,
    pub baseline_in_top3: u32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct FilteredRow {
    pub requested: u32,
    pub returned_min: u32,
    pub returned_mean: f64,
    pub baseline_returned_min: u32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct FixtureRow {
    pub collection: String,
    pub checks: u32,
    pub precision_at_5: f64,
    pub recall_at_5: f64,
    pub mrr: f64,
}

/// Validate `MEMORY_BENCH_COLLECTION_SIZES`: a larger size, then a smaller one.
pub(crate) fn validate_sizes(sizes: &[u32]) -> Result<(u32, u32), BenchError> {
    match sizes {
        [large, small, ..] if small < large => Ok((*large, *small)),
        _ => Err(BenchError::FixtureUnreadable {
            path: "MEMORY_BENCH_COLLECTION_SIZES".into(),
            reason: "MEMORY_BENCH_COLLECTION_SIZES needs a larger size then a smaller one".into(),
        }),
    }
}

/// Run the leg: rigs under `cfg.rig_dir/collections` (and `…/collections-load` for the budget run),
/// facts in `tenant`. The fixture files are read and checked **before** anything is built, so an
/// unreadable one measures nothing for any collection.
pub(crate) async fn run(
    cfg: &BenchConfig,
    tenant: &str,
) -> Result<CollectionLegReport, BenchError> {
    let (large, small) = validate_sizes(&cfg.collection_sizes)?;
    let fixtures = match &cfg.fixture_dir {
        Some(dir) => Some(read_fixtures(dir)?),
        None => None,
    };
    let ctx = reader(tenant);
    let k = cfg.stage1_k.max(1);

    // Steps 1–4 and 6 share one rig.
    let rig = Rig::open_surrealkv(
        cfg.rig_dir.join("collections"),
        cfg.embed_dim,
        MemoryProfile::Individual,
    )
    .await?;
    let result = async {
        let corpus = fairness_corpus(&rig, tenant, large, small).await?;
        let fairness = fairness(&rig, &ctx, &corpus, k).await?;
        let exact_term = exact_term(&rig, &ctx, tenant, &corpus, k).await?;
        let filtered = filtered(&rig, &ctx, &corpus).await?;
        let fixture = match &fixtures {
            Some(files) => Some(fixture_rows(&rig, &ctx, tenant, files, k).await?),
            None => {
                tracing::info!(target: "memory-bench", "bench.collections.fixture.skipped");
                None
            }
        };
        Ok::<_, BenchError>((fairness, exact_term, filtered, fixture))
    }
    .await;
    finish_rig(rig, cfg.retain);
    let (fairness, exact_term, filtered, fixture) = result?;

    // Step 5 — a fresh rig for the budget run.
    let rig = Rig::open_surrealkv(
        cfg.rig_dir.join("collections-load"),
        cfg.embed_dim,
        MemoryProfile::Individual,
    )
    .await?;
    let timing = budget(&rig, &ctx, tenant, cfg, large).await;
    finish_rig(rig, cfg.retain);
    let stage1_timing = timing?;

    Ok(CollectionLegReport {
        fairness,
        exact_term,
        filtered,
        stage1_timing,
        fixture,
    })
}

/// Step 7 — discard a rig unless `MEMORY_BENCH_RETAIN`.
fn finish_rig(rig: Rig, retain: bool) {
    if retain {
        tracing::info!(target: "memory-bench", "bench.rig.retained");
    } else if let Err(e) = rig.discard() {
        tracing::warn!(target: "memory-bench", error = %e, "bench.rig.discard_failed");
    }
}

fn reader(tenant: &str) -> ScopeContext {
    ScopeContext {
        tenant: tenant.to_string(),
        teams: vec![TEAM.to_string()],
        user: READER.to_string(),
        token_jti: "bench".into(),
        allowed_ops: OpSet {
            read: true,
            write: true,
            forget: false,
        },
        client: None,
        fact_authority: None,
        correlation_id: "bench-collections".into(),
    }
}

fn fact(id: &str, tenant: &str, collection: &str, content: serde_json::Value) -> Fact {
    let at = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap_or_else(chrono::Utc::now);
    Fact {
        id: id.to_string(),
        content,
        entities: vec!["entity:bench".into()],
        source_id: None,
        memory_class: MemoryClass::Semantic,
        fact_class: None,
        visibility: Visibility::TeamShared,
        owner: ScopeRef {
            tenant: tenant.to_string(),
            team: Some(TEAM.to_string()),
            user: READER.to_string(),
        },
        valid_from: at,
        valid_to: None,
        ingested_at: at,
        confidence: 0.9,
        salience: 0.7,
        stability: 1.0,
        pii_review: false,
        supersedes: None,
        superseded_by: None,
        derived_from: vec![],
        last_recalled_at: None,
        collection: collection.to_string(),
        single_valued: false,
        changed: false,
        triple_key: None,
    }
}

async fn put(store: &Store, ctx: &ScopeContext, f: &Fact, v: &[f32]) -> Result<(), BenchError> {
    crate::bench::corpus::with_conflict_retry(|| store.put_fact(f)).await?;
    crate::bench::corpus::with_conflict_retry(|| {
        store.set_fact_embedding(ctx, &f.id, v, MODEL_VERSION)
    })
    .await?;
    Ok(())
}

/// The axis query `j` lies along: one of axes `1..dim`, so query vectors are orthogonal to each other
/// wherever the dimension allows.
fn query_axis(dim: u32, j: u32) -> usize {
    1 + (j % (dim.max(2) - 1)) as usize
}

/// The unit query vector of query `j`.
fn query_vector(dim: u32, j: u32) -> Vec<f32> {
    let mut v = vec![0.0f32; dim.max(2) as usize];
    v[query_axis(dim, j)] = 1.0;
    v
}

/// A unit vector at cosine `c` to query `j`'s vector (the remainder on axis 0, which no query uses).
fn at_cosine(dim: u32, j: u32, c: f64) -> Vec<f32> {
    let mut v = vec![0.0f32; dim.max(2) as usize];
    v[query_axis(dim, j)] = c as f32;
    v[0] = (1.0 - c * c).max(0.0).sqrt() as f32;
    v
}

/// The full fact id of a store key or id.
fn full_id(id: &str) -> String {
    if id.starts_with("fact:") {
        id.to_string()
    } else {
        format!("fact:{id}")
    }
}

/// SA-BENCH-COLL-01's corpus and the answer of each fairness query.
struct FairnessCorpus {
    /// The `tk:b` answer fact of query `j`.
    answers: Vec<String>,
}

/// Build SA-BENCH-COLL-01's corpus: `large` facts in `tk:a` dealt round-robin to the 20 queries at
/// cosines 0.95 down to 0.50, and `small` facts in `tk:b`, of which fact `j` is query `j`'s answer —
/// the nearest of `tk:b` to that query, placed by construction just below `tk:a`'s
/// `min(100, m / 2)`-th nearest (`m` = `tk:a` facts per query), so between the 60th and the 200th
/// nearest overall at the default sizes.
async fn fairness_corpus(
    rig: &Rig,
    tenant: &str,
    large: u32,
    small: u32,
) -> Result<FairnessCorpus, BenchError> {
    let dim = rig.embed_dim();
    let store = rig.store();
    let ctx = reader(tenant);
    let per_query = (large / FAIRNESS_QUERIES).max(1);
    for i in 0..large {
        let j = i % FAIRNESS_QUERIES;
        let r = i / FAIRNESS_QUERIES;
        let c = 0.95 - 0.45 * f64::from(r) / f64::from(per_query);
        let f = fact(
            &format!("fact:coll-a{i:07}"),
            tenant,
            LARGE,
            serde_json::json!({"subject": "orders", "object": format!("a{i}")}),
        );
        put(store, &ctx, &f, &at_cosine(dim, j, c)).await?;
        if (i + 1) % 5_000 == 0 {
            tracing::info!(target: "memory-bench", written = i + 1, large, "bench.collections.corpus");
        }
    }
    let target = (per_query / 2).clamp(1, 100);
    let answer_cos = 0.95 - 0.45 * (f64::from(target) - 0.5) / f64::from(per_query);
    let mut answers = Vec::new();
    for i in 0..small {
        let j = i % FAIRNESS_QUERIES;
        let c = if i < FAIRNESS_QUERIES {
            answer_cos
        } else {
            answer_cos - 0.01 - 0.3 * f64::from(i) / f64::from(small)
        };
        let id = format!("fact:coll-b{i:07}");
        let f = fact(
            &id,
            tenant,
            SMALL,
            serde_json::json!({"subject": "ledger", "object": format!("b{i}")}),
        );
        put(store, &ctx, &f, &at_cosine(dim, j, c)).await?;
        if i < FAIRNESS_QUERIES {
            answers.push(id);
        }
    }
    Ok(FairnessCorpus { answers })
}

fn plain(ctx: &ScopeContext, vector: Vec<f32>, terms: Vec<String>, k: u16) -> StageOneQuery {
    StageOneQuery {
        graph_hops: 0,
        graph_fanout: 32,
        graph_cap: 200,
        phrase_texts: vec![],
        query_vector: vector,
        keyword_terms: terms,
        searches: vec![],
        filters: RecallFilters::default(),
        scope: ctx.clone(),
        stage1_k: k,
    }
}

/// The as-built merge, reproduced as the baseline: the vector and keyword signals with no collection
/// predicate, merged into a map keyed by fact id, cut at `stage1_k` in key order — the candidate ids
/// in that order.
async fn baseline(
    store: &Store,
    ctx: &ScopeContext,
    q: &StageOneQuery,
) -> Result<Vec<String>, StoreError> {
    let mut scores: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    if !q.query_vector.is_empty() {
        let strategy = VectorStrategy::for_profile(MemoryProfile::Individual);
        for (id, s) in store.recall_vector_only(ctx, q, strategy).await? {
            scores.entry(id).or_default().0 = s;
        }
    }
    if !q.keyword_terms.is_empty() {
        for (id, s) in store.recall_keyword_only(ctx, q).await? {
            scores.entry(id).or_default().1 = s;
        }
    }
    Ok(scores
        .into_keys()
        .take(usize::from(q.stage1_k))
        .map(|id| full_id(&id))
        .collect())
}

/// Step 2 — fairness.
async fn fairness(
    rig: &Rig,
    ctx: &ScopeContext,
    corpus: &FairnessCorpus,
    k: u16,
) -> Result<FairnessRow, BenchError> {
    let dim = rig.embed_dim();
    let store = rig.store();
    let (mut found, mut base_found) = (0, 0);
    for (j, answer) in corpus.answers.iter().enumerate() {
        let q = plain(ctx, query_vector(dim, j as u32), vec![], k);
        if store
            .recall(ctx, &q)
            .await?
            .iter()
            .any(|c| &c.fact_id == answer)
        {
            found += 1;
        }
        if baseline(store, ctx, &q).await?.contains(answer) {
            base_found += 1;
        }
    }
    let row = FairnessRow {
        queries: corpus.answers.len() as u32,
        answer_in_candidates: found,
        baseline_answer_in_candidates: base_found,
    };
    tracing::info!(
        target: "memory-bench",
        queries = row.queries,
        answer_in_candidates = row.answer_in_candidates,
        baseline_answer_in_candidates = row.baseline_answer_in_candidates,
        "bench.collections.fairness"
    );
    Ok(row)
}

/// Step 3 — exact term: 20 facts added to `tk:a`, each the only holder of one `zq-NNNN` token, at a
/// middling cosine to its query; per token a plain query (vector and the token) and a `lex`
/// sub-query of it — 40 queries — each counted when the holder is within the first 3 candidates by
/// `fused_score`. The baseline is read by its own stage-1 order (fact-id key order).
async fn exact_term(
    rig: &Rig,
    ctx: &ScopeContext,
    tenant: &str,
    corpus: &FairnessCorpus,
    k: u16,
) -> Result<ExactTermRow, BenchError> {
    let dim = rig.embed_dim();
    let store = rig.store();
    let n = corpus.answers.len() as u32;
    let mut holders = Vec::new();
    for j in 0..n {
        let token = format!("zq-{:04}", 7000 + j);
        let id = format!("fact:coll-x{j:07}");
        let f = fact(
            &id,
            tenant,
            LARGE,
            serde_json::json!({"subject": "ticket", "object": token}),
        );
        put(store, ctx, &f, &at_cosine(dim, j, 0.6)).await?;
        holders.push((j, token, id));
    }
    let (mut queries, mut in_top3, mut base_top3) = (0u32, 0u32, 0u32);
    for (j, token, id) in &holders {
        let plain_q = plain(ctx, query_vector(dim, *j), vec![token.clone()], k);
        let mut lex_q = plain(ctx, vec![], vec![], k);
        lex_q.searches = vec![StageOneSearch {
            kind: SubQueryKind::Lex,
            vector: vec![],
            terms: vec![token.clone()],
            weight: 2.0,
        }];
        // The as-built merge had no typed sub-query: the lex run's baseline is the keyword signal.
        let lex_base = plain(ctx, vec![], vec![token.clone()], k);
        for (q, base_q) in [(&plain_q, &plain_q), (&lex_q, &lex_base)] {
            queries += 1;
            let got = store.recall(ctx, q).await?;
            if got.iter().take(3).any(|c| &c.fact_id == id) {
                in_top3 += 1;
            }
            if baseline(store, ctx, base_q)
                .await?
                .iter()
                .take(3)
                .any(|c| c == id)
            {
                base_top3 += 1;
            }
        }
    }
    let row = ExactTermRow {
        queries,
        in_top3,
        baseline_in_top3: base_top3,
    };
    tracing::info!(
        target: "memory-bench",
        queries = row.queries,
        in_top3 = row.in_top3,
        baseline_in_top3 = row.baseline_in_top3,
        "bench.collections.exact_term"
    );
    Ok(row)
}

/// Step 4 — filtered: each fairness query restricted to `tk:b` at `stage1_k = 50`; the baseline is
/// the unfiltered as-built top-K with the `tk:b` facts kept.
async fn filtered(
    rig: &Rig,
    ctx: &ScopeContext,
    corpus: &FairnessCorpus,
) -> Result<FilteredRow, BenchError> {
    let dim = rig.embed_dim();
    let store = rig.store();
    let mut returned = Vec::new();
    let mut base_returned = Vec::new();
    for j in 0..corpus.answers.len() as u32 {
        let mut q = plain(ctx, query_vector(dim, j), vec![], FILTERED_K);
        q.filters.collections = Some(vec![SMALL.to_string()]);
        returned.push(store.recall(ctx, &q).await?.len() as u32);
        q.filters.collections = None;
        let mut kept = 0u32;
        for id in baseline(store, ctx, &q).await? {
            if store
                .get_fact(ctx, &id)
                .await?
                .is_some_and(|f| f.collection == SMALL)
            {
                kept += 1;
            }
        }
        base_returned.push(kept);
    }
    let row = FilteredRow {
        requested: u32::from(FILTERED_K),
        returned_min: returned.iter().copied().min().unwrap_or(0),
        returned_mean: if returned.is_empty() {
            0.0
        } else {
            f64::from(returned.iter().sum::<u32>()) / returned.len() as f64
        },
        baseline_returned_min: base_returned.iter().copied().min().unwrap_or(0),
    };
    tracing::info!(
        target: "memory-bench",
        requested = row.requested,
        returned_min = row.returned_min,
        returned_mean = row.returned_mean,
        baseline_returned_min = row.baseline_returned_min,
        "bench.collections.filtered"
    );
    Ok(row)
}

/// Step 5 — the stage-1 share of the budget run: `MEMORY_BENCH_COLLECTION_LOAD` collections of
/// `size` facts; queries of 4 sub-queries (2 `lex`, 2 `vec`, synthetic vectors), timed over
/// `MEMORY_BENCH_REPS × MEMORY_BENCH_QUERIES` samples after `MEMORY_BENCH_WARMUP`. The as-built path
/// — a plain query through the baseline merge plus its `get_fact` resolution — is timed beside it
/// and logged, not reported (the report row is the measurement the budget is read against).
async fn budget(
    rig: &Rig,
    ctx: &ScopeContext,
    tenant: &str,
    cfg: &BenchConfig,
    size: u32,
) -> Result<Timing, BenchError> {
    let dim = rig.embed_dim();
    let store = rig.store();
    let topics = 100u32;
    for c in 0..cfg.collection_load {
        let label = format!("tk:load{c}");
        for i in 0..size {
            let j = i % FAIRNESS_QUERIES;
            let cos = 0.5 + 0.45 * f64::from((i.wrapping_mul(7919)) % 1000) / 1000.0;
            let f = fact(
                &format!("fact:load{c}-{i:07}"),
                tenant,
                &label,
                serde_json::json!({"subject": format!("topic{}", i % topics), "object": format!("item {i}")}),
            );
            put(store, ctx, &f, &at_cosine(dim, j, cos)).await?;
            if (i + 1) % 5_000 == 0 {
                tracing::info!(target: "memory-bench", collection = c, written = i + 1, size, "bench.collections.load");
            }
        }
    }
    let k = cfg.stage1_k.max(1);
    let queries: Vec<StageOneQuery> = (0..cfg.queries.max(1))
        .map(|j| {
            let lex = |t: u32| StageOneSearch {
                kind: SubQueryKind::Lex,
                vector: vec![],
                terms: vec![format!("topic{}", t % topics)],
                weight: 1.0,
            };
            let vec_ = |q: u32| StageOneSearch {
                kind: SubQueryKind::Vec,
                vector: query_vector(dim, q),
                terms: vec![],
                weight: 1.0,
            };
            let mut searches = vec![vec_(j), lex(j), vec_(j + 1), lex(j + 50)];
            searches[0].weight = 2.0;
            let mut q = plain(ctx, vec![], vec![], k);
            q.searches = searches;
            q
        })
        .collect();
    let next = std::cell::Cell::new(0usize);
    let failures = std::cell::Cell::new(0u32);
    let reps = cfg.measure_reps.max(1) * queries.len() as u32;
    let timing = Timing::measure(cfg.warmup_reps, reps, || {
        let q = &queries[next.get() % queries.len()];
        next.set(next.get() + 1);
        let failures = &failures;
        async move {
            if store.recall(ctx, q).await.is_err() {
                failures.set(failures.get() + 1);
            }
        }
    })
    .await;
    if failures.get() > 0 {
        // Surface the real error rather than report percentiles of a query that never ran.
        store.recall(ctx, &queries[0]).await?;
    }

    // The as-built baseline: a plain query (one vector, the topic term) through the fact-id merge and
    // its per-candidate resolution.
    let plain_queries: Vec<StageOneQuery> = (0..cfg.queries.max(1))
        .map(|j| {
            plain(
                ctx,
                query_vector(dim, j),
                vec![format!("topic{}", j % topics)],
                k,
            )
        })
        .collect();
    let next = std::cell::Cell::new(0usize);
    let base = Timing::measure(cfg.warmup_reps, reps, || {
        let q = &plain_queries[next.get() % plain_queries.len()];
        next.set(next.get() + 1);
        async move {
            if let Ok(ids) = baseline(store, ctx, q).await {
                for id in ids {
                    let _ = store.get_fact(ctx, &id).await;
                }
            }
        }
    })
    .await;
    tracing::info!(
        target: "memory-bench",
        collections = cfg.collection_load,
        size,
        p50_millis = timing.p50_millis,
        p95_millis = timing.p95_millis,
        p99_millis = timing.p99_millis,
        baseline_p50_millis = base.p50_millis,
        baseline_p95_millis = base.p95_millis,
        "bench.collections.timing"
    );
    Ok(timing)
}

// --- Step 6: per-collection fixtures (SA-BENCH-COLL-02) ------------------------------------------

/// One check of a fixture file.
#[derive(Deserialize, Clone)]
struct Check {
    query: String,
    #[serde(rename = "type")]
    kind: Option<SubQueryKind>,
    expect: Vec<String>,
}

#[derive(Deserialize)]
struct ChecksFile {
    format: u32,
    checks: Vec<Check>,
}

/// One collection's fixture, read and checked.
struct FixtureFiles {
    collection: String,
    checks: Vec<Check>,
    facts: Vec<(Fact, Vec<f32>)>,
}

fn unreadable(path: &Path, reason: impl Into<String>) -> BenchError {
    BenchError::FixtureUnreadable {
        path: path.display().to_string(),
        reason: reason.into(),
    }
}

/// Read every `<collection>.checks.json` in `dir` with its `<collection>.facts.jsonl`, refusing the
/// first file that does not parse or breaks SA-BENCH-COLL-02.
fn read_fixtures(dir: &Path) -> Result<Vec<FixtureFiles>, BenchError> {
    let entries = std::fs::read_dir(dir).map_err(|e| unreadable(dir, e.to_string()))?;
    let mut checks_files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".checks.json"))
        })
        .collect();
    checks_files.sort();
    let mut out = Vec::new();
    for path in checks_files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let collection = name.trim_end_matches(".checks.json").to_string();
        if !crate::types::domain::is_valid_collection(&collection) {
            return Err(unreadable(&path, "the file name is not a collection label"));
        }
        let size = std::fs::metadata(&path)
            .map_err(|e| unreadable(&path, e.to_string()))?
            .len();
        if size > FIXTURE_MAX_BYTES {
            return Err(unreadable(&path, "larger than 256 KiB"));
        }
        let raw = std::fs::read(&path).map_err(|e| unreadable(&path, e.to_string()))?;
        let file: ChecksFile =
            serde_json::from_slice(&raw).map_err(|e| unreadable(&path, e.to_string()))?;
        if file.format != 1 {
            return Err(unreadable(
                &path,
                format!("format {} is not 1", file.format),
            ));
        }
        if file.checks.len() > FIXTURE_MAX_CHECKS {
            return Err(unreadable(&path, "more than 100 checks"));
        }
        for c in &file.checks {
            let bad_ids = c.expect.is_empty()
                || c.expect.len() > 10
                || c.expect.iter().any(|id| {
                    id.strip_prefix("fact:")
                        .and_then(|u| uuid::Uuid::parse_str(u).ok())
                        .is_none_or(|u| u.get_version_num() != 7)
                });
            if bad_ids || c.query.trim().is_empty() {
                return Err(unreadable(&path, "a check breaks SA-BENCH-COLL-02"));
            }
        }
        let facts_path = dir.join(format!("{collection}.facts.jsonl"));
        let body = std::fs::read_to_string(&facts_path)
            .map_err(|e| unreadable(&facts_path, e.to_string()))?;
        let mut facts = Vec::new();
        for line in body.lines().filter(|l| !l.trim().is_empty()) {
            let mut value: serde_json::Value =
                serde_json::from_str(line).map_err(|e| unreadable(&facts_path, e.to_string()))?;
            let embedding: Vec<f32> = value
                .as_object_mut()
                .and_then(|o| o.remove("embedding"))
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| unreadable(&facts_path, e.to_string()))?
                .ok_or_else(|| unreadable(&facts_path, "a fact has no embedding"))?;
            let fact: Fact = serde_json::from_value(value)
                .map_err(|e| unreadable(&facts_path, e.to_string()))?;
            facts.push((fact, embedding));
        }
        out.push(FixtureFiles {
            collection,
            checks: file.checks,
            facts,
        });
    }
    Ok(out)
}

/// Step 6 — load each fixture's facts into the collection it names, run every check (plain or typed
/// as `type` says) with no collection filter, and compute precision at 5, recall at 5 and mean
/// reciprocal rank per collection.
// The checks carry query *texts*, but the leg runs with no model provider and the format carries no
// query vector. A `lex` check runs as a `lex` sub-query; a `vec` check and a
// plain one (`type: null`) cannot be embedded here and run on the text's keyword terms alone. The
// fixture facts' owner, visibility and collection are the leg's own (its one team scope), so the
// checks read them through the reader's filter.
async fn fixture_rows(
    rig: &Rig,
    ctx: &ScopeContext,
    tenant: &str,
    files: &[FixtureFiles],
    k: u16,
) -> Result<Vec<FixtureRow>, BenchError> {
    let store = rig.store();
    let template = fact("fact:template", tenant, "", serde_json::json!({}));
    for file in files {
        for (f, v) in &file.facts {
            let mut f = f.clone();
            f.owner = template.owner.clone();
            f.visibility = template.visibility;
            f.collection = file.collection.clone();
            put(store, ctx, &f, v).await?;
        }
    }
    let mut rows = Vec::new();
    for file in files {
        let (mut p5, mut r5, mut rr) = (0.0, 0.0, 0.0);
        for c in &file.checks {
            let terms = crate::retrieval::keyword_terms(&c.query);
            let mut q = plain(ctx, vec![], vec![], k);
            match c.kind {
                Some(SubQueryKind::Lex) => {
                    q.searches = vec![StageOneSearch {
                        kind: SubQueryKind::Lex,
                        vector: vec![],
                        terms,
                        weight: 2.0,
                    }]
                }
                _ => q.keyword_terms = terms,
            }
            let got: Vec<String> = store
                .recall(ctx, &q)
                .await?
                .into_iter()
                .map(|c| c.fact_id)
                .collect();
            let hits5 = got
                .iter()
                .take(5)
                .filter(|id| c.expect.contains(id))
                .count() as f64;
            p5 += hits5 / 5.0;
            r5 += hits5 / c.expect.len() as f64;
            rr += got
                .iter()
                .position(|id| c.expect.contains(id))
                .map_or(0.0, |p| 1.0 / (p + 1) as f64);
        }
        let n = file.checks.len().max(1) as f64;
        let row = FixtureRow {
            collection: file.collection.clone(),
            checks: file.checks.len() as u32,
            precision_at_5: p5 / n,
            recall_at_5: r5 / n,
            mrr: rr / n,
        };
        tracing::info!(
            target: "memory-bench",
            checks = row.checks,
            precision_at_5 = row.precision_at_5,
            recall_at_5 = row.recall_at_5,
            mrr = row.mrr,
            "bench.collections.fixture"
        );
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_need_a_larger_then_a_smaller_one() {
        assert_eq!(validate_sizes(&[20_000, 200]).unwrap(), (20_000, 200));
        for bad in [&[][..], &[200][..], &[200, 200][..], &[200, 20_000][..]] {
            let err = validate_sizes(bad).unwrap_err();
            assert_eq!(err.exit_code(), 2);
            assert_eq!(
                err.to_string(),
                "MEMORY_BENCH_COLLECTION_SIZES needs a larger size then a smaller one"
            );
        }
    }

    #[test]
    fn query_vectors_are_orthogonal_and_cosines_exact() {
        let q0 = query_vector(32, 0);
        let q1 = query_vector(32, 1);
        assert_eq!(q0.iter().zip(&q1).map(|(a, b)| a * b).sum::<f32>(), 0.0);
        let v = at_cosine(32, 3, 0.62);
        let dot: f32 = v.iter().zip(&query_vector(32, 3)).map(|(a, b)| a * b).sum();
        assert!((dot - 0.62).abs() < 1e-6);
        let norm: f32 = v.iter().map(|x| x * x).sum();
        assert!((norm - 1.0).abs() < 1e-5);
    }
}
