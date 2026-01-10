//! The relevance leg — RFC 03 acceptance criterion 3 (plan 06 Phase 3).
//!
//! The criterion, verbatim: *"A filter-first stage-1 vector query returns results whose relevance is
//! at least equal to the HNSW path on the same corpus, measured on a fixed query set, with no
//! starvation as co-resident user count rises from 1 to 1,000."*
//!
//! Three decisions carry the measurement.
//!
//! **Both strategies run against one store holding one corpus.** That is what `recall_vector_only`'s
//! strategy parameter exists for (plan 06 Phase 1). Timing two strategies against two stores would
//! measure the data rather than the strategy, and the `corpus_digest` on every row is what lets a
//! reader confirm afterwards that it did not happen.
//!
//! **The baseline is filter-first, not the HNSW path.** `FilterFirstExact` filters before it caps, so
//! it scores every visible row and returns the true top-k of what the caller can read — it *is* the
//! ground truth for this corpus, and cannot be graded against an approximation of itself. Grading
//! both strategies against it turns "at least equal to the HNSW path" into a number: the HNSW row's
//! overlap is the fraction of the true top-k the index actually found, and the filter-first row is
//! the 1.0 reference it is read against. A reference row that is not 1.0 would mean the baseline
//! itself is unstable, which is worth seeing.
//!
//! **Starvation is a fill ratio, not a count.** `returned / min(stage1_k, visible)` — a ratio to what
//! was *available*. The failure mode RISK-013 names is the post-filter discarding the caller's rows
//! after the index walk, which collapses this while the raw returned count still looks plausible.
//!
//! No row carries a verdict. Whether a relevance delta is acceptable is a user decision, and a
//! boolean in the report would hide it behind a number.

use crate::bench::corpus;
use crate::bench::report::{RelevanceLeg, StrategyLabel};
use crate::bench::rig::Rig;
use crate::bench::{BenchConfig, BenchError};
use crate::store::VectorStrategy;

/// The two strategies under comparison, baseline first.
///
/// Order matters: [`VectorStrategy::FilterFirstExact`] is the exact one and is measured first at
/// every point, so the approximate arm always has a baseline to be graded against.
const STRATEGIES: [VectorStrategy; 2] = [
    VectorStrategy::FilterFirstExact,
    VectorStrategy::PostFilteredAnn,
];

/// Measure one co-resident point: build the corpus at `co_residents`, then run both strategies over
/// the fixed query set against that one store.
///
/// Returns one row per strategy, in [`STRATEGIES`] order.
///
/// **`rig` must be a freshly opened store, not one a previous point has already used.** Fact ids and
/// embeddings are a function of the index alone, so rebuilding in place rewrites only ownership and
/// looks like the cheap way to sweep — but it is measurably wrong. Sweeping `1,10` and then `10,1`
/// both put the higher post-filtered overlap on whichever point ran *first*, while the exact arm sat
/// at 1.0 in every position: the approximate arm degrades with the number of rewrites the HNSW index
/// has absorbed, and the sweep axis was measuring rebuild count rather than co-residency. The caller
/// gives each point its own directory (FU-005) and opens them sequentially, because SurrealKV holds
/// an exclusive directory lock for the process lifetime.
pub async fn measure_point(
    rig: &Rig,
    cfg: &BenchConfig,
    tenant: &str,
    total: u32,
    visible: u32,
    co_residents: u32,
) -> Result<Vec<RelevanceLeg>, BenchError> {
    let corpus = corpus::build(rig, tenant, total, visible, co_residents).await?;
    let queries = corpus::query_set(rig.embed_dim(), cfg.queries, corpus.total);
    let ctx = corpus::reader_ctx(tenant, corpus::OWNER);

    // The reference top-k per query, from the exact arm. Collected before the strategy loop so the
    // approximate arm is graded against a complete baseline rather than one being built beside it.
    let mut baselines: Vec<Vec<String>> = Vec::with_capacity(queries.len());
    for qv in &queries {
        baselines.push(
            run(rig, &ctx, qv, cfg.stage1_k, STRATEGIES[0])
                .await?
                .into_iter()
                .collect(),
        );
    }

    let mut rows = Vec::with_capacity(STRATEGIES.len());

    for strategy in STRATEGIES {
        // Every query is run once up front to collect the relevance measures, and again under
        // `Timing` to collect the latency. Separating them keeps a `Vec` allocation per result out of
        // the timed section, which at a 1,000-fact visible set is not negligible next to the query.
        let mut overlaps = Vec::with_capacity(queries.len());
        let mut correlations = Vec::with_capacity(queries.len());
        let mut fills = Vec::with_capacity(queries.len());

        for (j, qv) in queries.iter().enumerate() {
            let ids = run(rig, &ctx, qv, cfg.stage1_k, strategy).await?;
            let reference = &baselines[j];
            overlaps.push(overlap(&ids, reference));
            correlations.push(rank_correlation(&ids, reference));
            fills.push(fill_ratio(ids.len(), cfg.stage1_k, corpus.visible));
        }

        let timing = rig
            .time_query_set(
                &ctx,
                &queries,
                cfg.stage1_k,
                strategy,
                cfg.warmup_reps,
                cfg.measure_reps,
            )
            .await?;

        rows.push(RelevanceLeg {
            co_residents: corpus.co_residents,
            visible: corpus.visible,
            total: corpus.total,
            strategy: StrategyLabel::of(strategy),
            queries: queries.len() as u32,
            overlap: mean(&overlaps),
            rank_correlation: mean(&correlations),
            fill_ratio: mean(&fills),
            timing,
            corpus_digest: corpus.digest.clone(),
        });
    }

    for row in &rows {
        tracing::info!(
            target: "memory-bench",
            co_residents = row.co_residents,
            strategy = %row.strategy,
            overlap = row.overlap,
            fill_ratio = row.fill_ratio,
            p50_millis = row.timing.p50_millis,
            "bench.relevance.point"
        );
    }
    Ok(rows)
}

/// One query through the measurement seam, reduced to the returned ids in rank order.
pub(crate) async fn run(
    rig: &Rig,
    ctx: &crate::types::scope::ScopeContext,
    qv: &[f32],
    stage1_k: u16,
    strategy: VectorStrategy,
) -> Result<Vec<String>, BenchError> {
    let returned = rig
        .store()
        .recall_vector_only(ctx, &corpus::stage_one(qv, ctx, stage1_k), strategy)
        .await
        .map_err(BenchError::Store)?;
    Ok(returned.into_iter().map(|(fid, _)| fid).collect())
}

/// `|returned ∩ reference| / |reference|`, in `[0,1]`. `1.0` against an empty reference — nothing was
/// available to miss, which is not a relevance failure.
fn overlap(returned: &[String], reference: &[String]) -> f64 {
    if reference.is_empty() {
        return 1.0;
    }
    let r: std::collections::BTreeSet<&String> = reference.iter().collect();
    let hits = returned.iter().filter(|id| r.contains(id)).count();
    hits as f64 / reference.len() as f64
}

/// Spearman's ρ between the two orderings, over the ids present in **both**, in `[-1,1]`.
///
/// `0.0` when fewer than two ids are shared: with one point or none there is no ordering to correlate,
/// and any other value would be invented. `overlap` is reported on the same row and is what
/// distinguishes "the orders disagree" from "there was nothing to compare" — a reader needs both
/// numbers, which is why neither is folded into the other.
fn rank_correlation(returned: &[String], reference: &[String]) -> f64 {
    let rank_in = |list: &[String], id: &String| list.iter().position(|x| x == id);
    let shared: Vec<(usize, usize)> = returned
        .iter()
        .filter_map(|id| Some((rank_in(returned, id)?, rank_in(reference, id)?)))
        .collect();
    let n = shared.len();
    if n < 2 {
        return 0.0;
    }
    // Ranks are re-derived within the shared subset, so removing ids the other side missed does not
    // leave gaps that read as disagreement.
    let mut by_returned: Vec<usize> = (0..n).collect();
    by_returned.sort_by_key(|&i| shared[i].0);
    let mut by_reference: Vec<usize> = (0..n).collect();
    by_reference.sort_by_key(|&i| shared[i].1);
    let mut ra = vec![0usize; n];
    let mut rb = vec![0usize; n];
    for (rank, &i) in by_returned.iter().enumerate() {
        ra[i] = rank;
    }
    for (rank, &i) in by_reference.iter().enumerate() {
        rb[i] = rank;
    }
    let d2: f64 = (0..n)
        .map(|i| {
            let d = ra[i] as f64 - rb[i] as f64;
            d * d
        })
        .sum();
    let nf = n as f64;
    1.0 - (6.0 * d2) / (nf * (nf * nf - 1.0))
}

/// `returned / min(stage1_k, visible)` — the starvation measure, in `[0,1]`.
///
/// The denominator is what was *available* to return, not what was asked for: a caller who owns three
/// facts and asks for ten has not been starved by receiving three. Clamped rather than allowed to
/// exceed 1.0, so an unexpected over-return is visible as a saturated ratio rather than as a number
/// that reads like an especially good result.
pub(crate) fn fill_ratio(returned: usize, stage1_k: u16, visible: u32) -> f64 {
    let available = (stage1_k as u32).min(visible);
    if available == 0 {
        return 1.0;
    }
    (returned as f64 / available as f64).clamp(0.0, 1.0)
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

/// A convenience the report's own shape does not provide: which strategy won a point, for the log
/// line and the runbook. Not a verdict — it names the larger number, it does not judge it.
pub fn better_overlap(rows: &[RelevanceLeg]) -> Option<StrategyLabel> {
    rows.iter()
        .max_by(|a, b| {
            a.overlap
                .partial_cmp(&b.overlap)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|r| r.strategy)
}

/// The corpus a point was measured against, for a caller that needs to assert two points shared one.
pub fn digest_of(rows: &[RelevanceLeg]) -> Option<&str> {
    rows.first().map(|r| r.corpus_digest.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn overlap_is_the_fraction_of_the_reference_that_was_found() {
        let reference = ids(&["a", "b", "c", "d"]);
        assert_eq!(overlap(&ids(&["a", "b", "c", "d"]), &reference), 1.0);
        assert_eq!(overlap(&ids(&["a", "b"]), &reference), 0.5);
        assert_eq!(overlap(&ids(&["x", "y"]), &reference), 0.0);
        // Order does not enter overlap — that is what rank correlation is for.
        assert_eq!(overlap(&ids(&["d", "c", "b", "a"]), &reference), 1.0);
        // Nothing was available to miss.
        assert_eq!(overlap(&ids(&[]), &[]), 1.0);
    }

    #[test]
    fn rank_correlation_is_one_when_the_orders_agree_and_minus_one_when_reversed() {
        let reference = ids(&["a", "b", "c", "d"]);
        assert_eq!(rank_correlation(&ids(&["a", "b", "c", "d"]), &reference), 1.0);
        assert_eq!(
            rank_correlation(&ids(&["d", "c", "b", "a"]), &reference),
            -1.0
        );
        // Fewer than two shared ids: no ordering exists to correlate, and any other value would be
        // invented. `overlap` on the same row is what says why.
        assert_eq!(rank_correlation(&ids(&["a"]), &reference), 0.0);
        assert_eq!(rank_correlation(&ids(&["x", "y"]), &reference), 0.0);
    }

    /// Ids the other side never returned are dropped before ranking, rather than leaving gaps that
    /// would read as disagreement between two orders that in fact agree on everything they share.
    #[test]
    fn rank_correlation_ignores_ids_the_other_side_missed() {
        let reference = ids(&["a", "b", "c", "d"]);
        assert_eq!(rank_correlation(&ids(&["a", "x", "b", "y"]), &reference), 1.0);
    }

    /// The denominator is what was available, not what was asked for: a caller who owns three facts
    /// and asks for ten has not been starved by receiving three.
    #[test]
    fn the_fill_ratio_measures_against_what_was_available() {
        assert_eq!(fill_ratio(10, 10, 1000), 1.0);
        assert_eq!(fill_ratio(2, 10, 1000), 0.2);
        assert_eq!(fill_ratio(3, 10, 3), 1.0, "three of three is not starvation");
        assert_eq!(fill_ratio(0, 10, 1000), 0.0);
        assert_eq!(fill_ratio(5, 10, 0), 1.0, "nothing was available to return");
        assert_eq!(fill_ratio(20, 10, 1000), 1.0, "an over-return saturates");
    }

    /// The exact arm is measured first at every point, so the approximate arm always has a reference
    /// to be graded against rather than being graded against nothing.
    #[test]
    fn the_exact_strategy_is_the_baseline() {
        assert_eq!(STRATEGIES[0], VectorStrategy::FilterFirstExact);
        assert_eq!(STRATEGIES[1], VectorStrategy::PostFilteredAnn);
    }
}
