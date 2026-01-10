//! The corpus every re-pointed leg measures against (plan 06 Phase 2).
//!
//! One builder, shared by the relevance, latency and co-residency legs, because three builders would
//! make the three legs incomparable — and comparing them is the entire point of RFC 03 acceptance
//! criteria 3 and 4.
//!
//! The corpus is one namespace holding `total` user-private facts, of which `visible` belong to the
//! measured caller [`OWNER`] and the remainder is spread evenly across `co_residents` distinct owner
//! ids. That last parameter is what turns a *visible fraction* into a *co-resident user count*
//! without changing the corpus size, which is the difference between the leg RFC 03 asks for and a
//! re-parameterisation of the one it replaces.
//!
//! **Determinism is a correctness property here, not a convenience.** Embeddings are derived from the
//! fact's index by perturbing a fixed unit vector, exactly as the in-repo `co_residency_cost`
//! measurement does (`src/store/mod.rs`), so a rerun measures the same corpus and a difference
//! between two runs is attributable to the strategy rather than to the data. [`Corpus::digest`]
//! records a fingerprint of every `(id, owner, embedding)` triple written, so two rows in a report
//! can be *shown* to have measured the same corpus instead of assumed to have.

use sha2::{Digest, Sha256};

use crate::bench::rig::Rig;
use crate::bench::BenchError;
use crate::types::domain::{Fact, MemoryClass, Visibility};
use crate::types::ports::{MemoryStore, RecallFilters, StageOneQuery, StoreError};
use crate::types::scope::{OpSet, ScopeContext, ScopeRef};

/// The caller every leg measures from — the "one consumer" whose memory is held fixed while the
/// namespace grows around them.
pub const OWNER: &str = "u-me";

/// The embedding model version stamped on every generated fact.
const MODEL_VERSION: &str = "bench-corpus-v1";

/// A built corpus, and everything a report row needs to say which corpus it measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Corpus {
    pub tenant: String,
    /// The measured caller; always [`OWNER`].
    pub owner: String,
    /// Facts in the namespace.
    pub total: u32,
    /// Facts owned by [`OWNER`], and so visible to the measured caller.
    pub visible: u32,
    /// Distinct co-resident owner ids the invisible remainder is spread across.
    pub co_residents: u32,
    /// `total - visible`.
    pub co_resident_facts: u32,
    pub embed_dim: u32,
    /// SHA-256 over every `(id, owner, embedding)` triple written, in write order, hex-encoded.
    pub digest: String,
}

impl Corpus {
    /// `visible / total`, or `0.0` for an empty corpus.
    pub fn visible_fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.visible as f64 / self.total as f64
        }
    }
}

/// The `n` co-resident owner ids, in a fixed order.
///
/// Zero-padded and generated from the index alone, so the same `n` yields the same ids on every run
/// and a report row naming a co-resident count names a reproducible population rather than a size.
pub fn co_resident_users(n: u32) -> Vec<String> {
    (0..n).map(|i| format!("u-co{i:07}")).collect()
}

/// The fixed query vector every leg queries with: the unit vector the corpus is perturbed away from.
///
/// Shared rather than re-declared per leg. A query vector that differed between the relevance leg and
/// the latency leg would make their rows describe different searches while looking like one series.
pub fn query_vector(embed_dim: u32) -> Vec<f32> {
    let mut v = vec![0.0_f32; embed_dim.max(2) as usize];
    v[0] = 1.0;
    v
}

/// The fixed query set: `count` deterministic vectors placed evenly through the corpus's own
/// embedding span, so each query lands in a different part of the ranking.
///
/// Evenly through the span rather than all at the origin: [`query_vector`] alone would have all
/// `count` queries return the same neighbourhood, and a "fixed query set" of twenty identical
/// searches measures one search twenty times. `total` is the corpus size the span is taken from, so
/// query `j` sits at roughly `j / count` of the way through it.
pub fn query_set(embed_dim: u32, count: u32, total: u32) -> Vec<Vec<f32>> {
    let count = count.max(1);
    (0..count)
        .map(|j| {
            let at = (j as u64 * total as u64) / count as u64;
            embedding(embed_dim, at as u32, total)
        })
        .collect()
}

/// The embedding for the fact at `index` of a corpus of `total`: [`query_vector`] fanned out on the
/// second component by `index / total`, so the corpus spans a fixed angular arc whatever its size and
/// every fact sits at a distinguishable, reproducible angle from the query.
///
/// **The perturbation is a fraction of the corpus, not a fixed step.** A fixed step (the in-repo
/// `co_residency_cost` measurement uses `1e-6`) is only distinguishable at that measurement's scale:
/// at 60 facts it moves the second component by `6e-5`, the cosine distance by under `2e-9`, and
/// SurrealDB scores the embedding array in `f32` — so every distance collapses to exactly `0.0`, every
/// fact ties, and the returned order is arbitrary. A leg whose rank correlation is computed over ties
/// is measuring the engine's tie-breaking. Scaling by `index / total` puts the corpus on the arc from
/// 0 to 45° at any size, which separates every rank in `f32` with room to spare.
pub fn embedding(embed_dim: u32, index: u32, total: u32) -> Vec<f32> {
    let mut v = query_vector(embed_dim);
    v[1] = index as f32 / total.max(1) as f32;
    v
}

/// The fact id at `index`.
pub fn fact_id(index: u32) -> String {
    format!("fact:bench{index:07}")
}

/// How many of the first `index` facts belong to [`OWNER`], under an even spread of `visible` across
/// `total`.
///
/// Integer Bresenham. `u64` throughout because `index * visible` reaches 10^10 at the operator-sized
/// populations and would silently wrap in `u32`.
fn visible_before(index: u32, visible: u32, total: u32) -> u32 {
    if total == 0 {
        return 0;
    }
    ((index as u64 * visible as u64) / total as u64) as u32
}

/// Whether the fact at `index` belongs to [`OWNER`].
fn is_visible(index: u32, visible: u32, total: u32) -> bool {
    visible_before(index + 1, visible, total) > visible_before(index, visible, total)
}

/// The owner of the fact at `index` under a corpus of `total` facts, `visible` of them the caller's,
/// with the remainder spread across `co_residents` co-resident users.
///
/// **The caller's facts are spread evenly through the whole index range, not taken from the front.**
/// That is the difference between a corpus that can measure post-filter starvation and one that
/// cannot. Embeddings are perturbed monotonically by index (see [`embedding`]), so index order *is*
/// rank order from the fixed query: giving [`OWNER`] the first `visible` indices would make the
/// caller's facts precisely the nearest neighbours, the HNSW walk would return them whatever the
/// visible fraction, and the fill ratio would read 1.0 at every point. A leg that reports 1.0
/// everywhere has not been shown to measure anything (RFC 03 acceptance criterion 3).
///
/// An even spread is Bresenham rather than a modulus, so the count is exactly `visible` for every
/// `(visible, total)` pair rather than only for those that divide. The invisible remainder is then
/// dealt round-robin by its own ordinal, so the co-residents hold within one fact of each other and
/// no single co-resident dominates the invisible set at any co-resident count.
fn owner_of(index: u32, visible: u32, total: u32, co_residents: &[String]) -> String {
    if co_residents.is_empty() || is_visible(index, visible, total) {
        OWNER.to_string()
    } else {
        let invisible_ordinal = index - visible_before(index, visible, total);
        co_residents[(invisible_ordinal as usize) % co_residents.len()].clone()
    }
}

/// A write context for `tenant`.
///
/// The corpus writer is not the measured caller: `set_fact_embedding` selects a namespace by
/// `ctx.tenant` and applies no ownership predicate, so one writer populates every owner's facts.
/// Reads are a different matter — a leg measures through the *caller's* context and gets the caller's
/// read filter, which is exactly the property `visible` is supposed to mean.
pub fn writer_ctx(tenant: &str) -> ScopeContext {
    ScopeContext {
        tenant: tenant.to_string(),
        teams: vec![],
        user: OWNER.to_string(),
        token_jti: "bench".into(),
        allowed_ops: OpSet {
            read: true,
            write: true,
            forget: true,
        },
        // The harness never calls `Service::remember`, so the fact-class authority check is not on
        // its path; `None` here is "no claim", not "no authority".
        client: None,
        fact_authority: None,
        correlation_id: "bench-corpus".into(),
    }
}

/// The stage-1 query every leg issues.
///
/// One definition rather than one per leg. No keyword terms, no entity filter and no `valid_at`: the
/// vector signal is the one under comparison, and a metadata predicate would change what both
/// strategies select while looking like it changed neither. Two legs building this separately is the
/// same drift hazard RISK-001 names for the seam itself.
pub fn stage_one(qv: &[f32], ctx: &ScopeContext, stage1_k: u16) -> StageOneQuery {
    StageOneQuery {
        query_vector: qv.to_vec(),
        keyword_terms: vec![],
        filters: RecallFilters::default(),
        scope: ctx.clone(),
        stage1_k,
    }
}

/// A read context for `user` in `tenant` — the scope a leg measures from.
pub fn reader_ctx(tenant: &str, user: &str) -> ScopeContext {
    ScopeContext {
        user: user.to_string(),
        correlation_id: format!("bench-read-{user}"),
        ..writer_ctx(tenant)
    }
}

/// Build the corpus into `rig`'s store.
///
/// `visible` is clamped to `total`, because a corpus claiming more visible facts than it holds would
/// silently make every fill ratio in the relevance leg exceed 1.0 and read as an absence of
/// starvation.
///
/// The plan's signature was `build(rig, tenant, total, visible)`. `co_residents` is the fifth
/// parameter because there is otherwise no way to say what the invisible remainder is spread across,
/// and that count is the knob Phase 3 sweeps from 1 to 1,000 — leaving it implicit would have made
/// [`co_resident_users`] unreachable from the only function that can use it.
pub async fn build(
    rig: &Rig,
    tenant: &str,
    total: u32,
    visible: u32,
    co_residents: u32,
) -> Result<Corpus, BenchError> {
    let embed_dim = rig.embed_dim();
    let visible = visible.min(total);
    // A corpus with invisible facts and nobody to own them would report a `co_resident_facts` count
    // that no owner id backs, so the count is raised to one rather than the corpus quietly becoming
    // entirely the caller's.
    let co_residents = if visible < total {
        co_residents.max(1)
    } else {
        co_residents
    };
    let users = co_resident_users(co_residents);
    let ctx = writer_ctx(tenant);
    let store = rig.store();

    let mut hasher = Sha256::new();
    hasher.update(tenant.as_bytes());
    hasher.update(total.to_le_bytes());
    hasher.update(visible.to_le_bytes());
    hasher.update(co_residents.to_le_bytes());
    hasher.update(embed_dim.to_le_bytes());

    for i in 0..total {
        let id = fact_id(i);
        let owner = owner_of(i, visible, total, &users);
        let vector = embedding(embed_dim, i, total);

        let f = fact(&id, tenant, &owner);
        with_conflict_retry(|| store.put_fact(&f))
            .await
            .map_err(BenchError::Store)?;
        with_conflict_retry(|| store.set_fact_embedding(&ctx, &id, &vector, MODEL_VERSION))
            .await
            .map_err(BenchError::Store)?;

        hasher.update(id.as_bytes());
        hasher.update(owner.as_bytes());
        for c in &vector {
            hasher.update(c.to_le_bytes());
        }

        // Progress at a coarse interval: the operator-sized populations take minutes, and a build
        // that prints nothing is indistinguishable from a build that has hung.
        if (i + 1) % 5_000 == 0 {
            tracing::info!(target: "memory-bench", written = i + 1, total, "bench.corpus.progress");
        }
    }

    let corpus = Corpus {
        tenant: tenant.to_string(),
        owner: OWNER.to_string(),
        total,
        visible,
        co_residents,
        co_resident_facts: total - visible,
        embed_dim,
        digest: format!("{:x}", hasher.finalize()),
    };
    tracing::info!(
        target: "memory-bench",
        total = corpus.total,
        visible = corpus.visible,
        co_residents = corpus.co_residents,
        digest = %corpus.digest,
        "bench.corpus.built"
    );
    Ok(corpus)
}

/// Attempts a conflict-shaped write makes before giving up.
const CONFLICT_ATTEMPTS: u32 = 5;

/// Run `op`, retrying while it fails with a conflict the engine says is retryable.
///
/// SurrealKV returns `Transaction conflict: Transaction write conflict. This transaction can be
/// retried` on a foreground write that raced the engine's own background work — observed
/// intermittently part-way through a corpus build, on a workload with exactly one writer. It aborted
/// the whole leg, which at the operator-sized populations is tens of minutes of writing thrown away
/// for a condition the engine describes as transient in the message itself.
///
/// Matched on the message text because `map_db_err` folds every non-timeout engine error into
/// `StoreError::Unavailable(String)`; there is no typed conflict variant to match on, and adding one
/// is a C1 change this leg has no business making. The match is narrow — a non-conflict failure is
/// returned on the first attempt, so a genuinely broken store still fails fast.
///
/// Backoff is exponential and **without jitter**: jitter decorrelates competing clients, and this
/// workload has one writer.
async fn with_conflict_retry<F, Fut>(mut op: F) -> Result<(), StoreError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), StoreError>>,
{
    let mut delay_millis = 10u64;
    for attempt in 1..=CONFLICT_ATTEMPTS {
        match op().await {
            Ok(()) => return Ok(()),
            Err(e) if attempt < CONFLICT_ATTEMPTS && is_retryable_conflict(&e) => {
                tracing::debug!(
                    target: "memory-bench",
                    attempt,
                    delay_millis,
                    "bench.corpus.write_conflict_retry"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_millis)).await;
                delay_millis *= 2;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("the loop returns on the final attempt")
}

/// Whether `e` is the engine's own retryable write conflict.
fn is_retryable_conflict(e: &StoreError) -> bool {
    match e {
        StoreError::Unavailable(msg) => {
            let m = msg.to_lowercase();
            m.contains("conflict") && m.contains("retried")
        }
        _ => false,
    }
}

/// One generated fact. Deliberately uniform apart from id and owner: the corpus varies exactly two
/// things, so any measured difference is attributable to one of them.
fn fact(id: &str, tenant: &str, owner: &str) -> Fact {
    let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap_or_else(chrono::Utc::now);
    Fact {
        id: id.to_string(),
        content: serde_json::json!({"subject": "s", "predicate": "p", "object": "o"}),
        entities: vec!["entity:bench".into()],
        source_id: None,
        memory_class: MemoryClass::Semantic,
        // The benchmark measures retrieval cost, not authorisation: an unclassed corpus keeps the
        // rows uniform and the class off the measurement.
        fact_class: None,
        // User-private throughout: the visible fraction is then decided by ownership alone. A
        // tenant-shared fact would be visible to every co-resident under the collective read filter
        // and would make `visible` mean nothing.
        visibility: Visibility::UserPrivate,
        owner: ScopeRef {
            tenant: tenant.to_string(),
            team: None,
            user: owner.to_string(),
        },
        // A fixed timestamp rather than `now`, so two builds of the same parameters are identical
        // rows and not merely identically-shaped ones.
        valid_from: now,
        valid_to: None,
        ingested_at: now,
        confidence: 0.9,
        salience: 0.7,
        stability: 1.0,
        pii_review: false,
        supersedes: None,
        superseded_by: None,
        derived_from: vec![],
        last_recalled_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the engine's own retryable conflict is retried. A store that is genuinely broken fails on
    /// the first attempt rather than after five rounds of backoff.
    #[test]
    fn only_a_retryable_conflict_is_retryable() {
        assert!(is_retryable_conflict(&StoreError::Unavailable(
            "Transaction conflict: Transaction write conflict. This transaction can be retried"
                .into()
        )));
        assert!(!is_retryable_conflict(&StoreError::Unavailable(
            "connection refused".into()
        )));
        assert!(!is_retryable_conflict(&StoreError::Unavailable(
            "conflict".into()
        )));
        assert!(!is_retryable_conflict(&StoreError::Timeout));
    }

    /// A conflict that clears is retried through; a failure that is not a conflict is returned at
    /// once, and a conflict that never clears gives up rather than looping forever.
    #[tokio::test]
    async fn a_conflict_is_retried_and_anything_else_is_not() {
        let conflict = || {
            StoreError::Unavailable(
                "Transaction conflict: Transaction write conflict. This transaction can be retried"
                    .into(),
            )
        };
        let n = std::cell::Cell::new(0u32);
        with_conflict_retry(|| {
            n.set(n.get() + 1);
            let fail = n.get() < 3;
            async move {
                if fail {
                    Err(conflict())
                } else {
                    Ok(())
                }
            }
        })
        .await
        .expect("a conflict that clears must be retried through");
        assert_eq!(n.get(), 3);

        let m = std::cell::Cell::new(0u32);
        let err = with_conflict_retry(|| {
            m.set(m.get() + 1);
            async { Err(StoreError::Timeout) }
        })
        .await
        .expect_err("a non-conflict failure is not retried");
        assert!(matches!(err, StoreError::Timeout));
        assert_eq!(m.get(), 1, "a non-conflict failure must not be retried");

        let k = std::cell::Cell::new(0u32);
        with_conflict_retry(|| {
            k.set(k.get() + 1);
            async move { Err(conflict()) }
        })
        .await
        .expect_err("a conflict that never clears must give up");
        assert_eq!(k.get(), CONFLICT_ATTEMPTS);
    }

    #[test]
    fn co_resident_users_are_distinct_ordered_and_reproducible() {
        assert!(co_resident_users(0).is_empty());
        let a = co_resident_users(3);
        assert_eq!(a, vec!["u-co0000000", "u-co0000001", "u-co0000002"]);
        assert_eq!(a, co_resident_users(3));
        let many = co_resident_users(1000);
        assert_eq!(
            many.iter().collect::<std::collections::BTreeSet<_>>().len(),
            1000
        );
    }

    /// The invisible remainder is dealt round-robin, so no co-resident holds more than one fact more
    /// than another. A skewed deal would make a co-resident count describe a population that does not
    /// behave like one.
    #[test]
    fn the_invisible_remainder_is_dealt_evenly_across_co_residents() {
        let users = co_resident_users(3);
        let mut counts = std::collections::BTreeMap::new();
        for i in 0..10u32 {
            *counts.entry(owner_of(i, 4, 10, &users)).or_insert(0u32) += 1;
        }
        assert_eq!(counts[OWNER], 4, "exactly `visible` facts are the caller's");
        let co: Vec<u32> = users.iter().map(|u| counts[u]).collect();
        assert_eq!(co.iter().sum::<u32>(), 6);
        assert!(
            co.iter().max().unwrap() - co.iter().min().unwrap() <= 1,
            "uneven deal: {co:?}"
        );
    }

    /// The count is exactly `visible` for every `(visible, total)` pair, not only for those that
    /// divide — a corpus holding 999 of a claimed 1,000 visible facts would shift every fill ratio by
    /// an amount nobody could see.
    #[test]
    fn exactly_visible_facts_are_the_callers_at_every_ratio() {
        for (visible, total) in [(1u32, 7u32), (3, 7), (7, 7), (1000, 20000), (999, 1001)] {
            let n = (0..total).filter(|&i| is_visible(i, visible, total)).count();
            assert_eq!(n as u32, visible, "visible={visible} total={total}");
        }
    }

    /// The caller's facts are spread through the index range rather than taken from the front, and
    /// index order is rank order. Without this the HNSW walk would return the caller's facts whatever
    /// the visible fraction and the starvation measure would read 1.0 everywhere.
    #[test]
    fn the_callers_facts_are_spread_through_the_ranking() {
        let visible = 10u32;
        let total = 100u32;
        let owned: Vec<u32> = (0..total)
            .filter(|&i| is_visible(i, visible, total))
            .collect();
        assert_eq!(owned.len(), visible as usize);
        assert!(
            *owned.last().unwrap() >= total - total / visible,
            "the caller's last fact is at {}, still near the front of {total}",
            owned.last().unwrap()
        );
        // No two consecutive indices, at a 10% fraction — the front block the old builder produced.
        assert!(
            owned.windows(2).all(|w| w[1] - w[0] > 1),
            "the caller's facts are contiguous: {owned:?}"
        );
    }

    /// With no co-residents at all every fact is the caller's, rather than the remainder falling into
    /// an index that does not exist.
    #[test]
    fn a_corpus_with_no_co_residents_is_entirely_the_callers() {
        for i in 0..5u32 {
            assert_eq!(owner_of(i, 2, 5, &[]), OWNER);
        }
    }

    /// Every fact sits at its own angle from the query, in index order, so a rank is a rank and not a
    /// tie-break. This is what makes the relevance leg's rank correlation meaningful.
    #[test]
    fn embeddings_are_distinct_ordered_and_reproducible() {
        assert_eq!(embedding(8, 3, 100), embedding(8, 3, 100));
        assert_ne!(embedding(8, 3, 100), embedding(8, 4, 100));
        assert_eq!(query_vector(8)[0], 1.0);
        assert_eq!(query_vector(8)[1], 0.0);
        for i in 0..10u32 {
            assert!(embedding(8, i + 1, 100)[1] > embedding(8, i, 100)[1]);
            assert_eq!(embedding(8, i, 100).len(), 8);
        }
    }

    /// The corpus spans the same angular arc at any size, and consecutive ranks stay separable in
    /// `f32` — the precision the engine actually scores the embedding array at. A corpus whose
    /// consecutive facts differ by less than `f32` epsilon returns ties, and a rank correlation over
    /// ties measures the engine's tie-breaking rather than the strategy.
    #[test]
    fn consecutive_facts_stay_separable_in_f32_at_every_corpus_size() {
        for total in [60u32, 1_000, 20_000, 100_000] {
            let a = embedding(8, 0, total);
            let b = embedding(8, 1, total);
            let last = embedding(8, total - 1, total);
            assert!(
                b[1] - a[1] > f32::EPSILON,
                "total={total}: consecutive facts differ by {}, under f32 epsilon",
                b[1] - a[1]
            );
            // The last fact is at index `total - 1`, so it lands one step short of the full arc.
            let step = 1.0 / total as f32;
            assert!(
                (last[1] - (1.0 - step)).abs() < step,
                "total={total}: the corpus must span the same arc at any size, ends at {}",
                last[1]
            );
        }
    }

    /// The query set spans the corpus rather than clustering at its origin, so twenty queries are
    /// twenty different searches and not one search run twenty times.
    #[test]
    fn the_query_set_spans_the_corpus() {
        let qs = query_set(8, 5, 100);
        assert_eq!(qs.len(), 5);
        assert_eq!(qs, query_set(8, 5, 100), "the query set is fixed across runs");
        assert_eq!(qs[0], query_vector(8), "the first query is the corpus anchor");
        for w in qs.windows(2) {
            assert!(w[1][1] > w[0][1], "queries must be distinct and ordered");
        }
        assert_eq!(qs.last().unwrap(), &embedding(8, 80, 100));
        assert_eq!(query_set(8, 0, 100).len(), 1, "a zero-query set still measures once");
    }

    /// A reader context differs from the writer's in exactly the user it reads as — the read filter
    /// is what a leg is measuring, so nothing else may vary with it.
    #[test]
    fn a_reader_context_changes_only_the_caller() {
        let w = writer_ctx("acme");
        let r = reader_ctx("acme", "u-co0000001");
        assert_eq!(r.tenant, w.tenant);
        assert_eq!(r.teams, w.teams);
        assert_eq!(r.allowed_ops.read, w.allowed_ops.read);
        assert_eq!(r.user, "u-co0000001");
        assert_ne!(r.user, w.user);
    }
}
