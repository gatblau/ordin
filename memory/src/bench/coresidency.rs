//! The co-residency leg — per-user latency as the namespace grows underneath a fixed visible set
//! (plan 06 Phase 5).
//!
//! This is the second measurement RFC 03 names, and it is a **distinct** measurement from the
//! visible-fraction leg rather than a re-parameterisation of it. The latency leg varies the fraction
//! at a fixed visible set, which means the namespace size and the fraction move together and a
//! reader cannot tell which of the two a difference is attributable to. This leg pins one consumer's
//! memory at `MEMORY_BENCH_VISIBLE` and grows only the namespace, so the cost attributable to
//! co-residency is separated from the cost attributable to the caller's own corpus.
//!
//! That separation is exactly the Stage 2 question: whether filter-first's cost tracks the *visible
//! set* — flat as co-residents arrive — while the index walk's tracks the *whole namespace*. Filter-
//! first was flat at ~4.5 ms across a twentyfold change in namespace size on the visible-fraction
//! leg, which is the shape RFC 03 predicts; here the fraction is not co-moving with the size, so a
//! flat filter-first line means what it appears to mean.
//!
//! **The visible fraction still falls, and every row says so.** Pinning the visible set while the
//! namespace grows is arithmetically a falling fraction — 1,000 of 2,000 facts is 50%, 1,000 of
//! 100,000 is 1% — which is exactly the regime the visible-fraction leg found the post-filtered arm
//! getting *faster* in, because it finds and returns almost nothing (FU-012). A row carrying only a
//! timing would let the index path be recorded as overtaking filter-first at the largest namespace,
//! which is the scaling result this leg exists to produce and would be a starvation artefact. So
//! every row carries `fill_ratio`, measured by an untimed pass over the query set, as the latency leg
//! carries it. What separates this leg from that one is which variable is the grid — sizes here,
//! fractions there — and what that buys is a sweep spaced by namespace size, up to a size the
//! fraction grid never reaches.
//!
//! Everything else is the discipline the previous three phases built and is deliberately unchanged:
//! one fresh rig per point, the shared corpus builder, the shared query set, the same two strategies
//! in the same order, `Timing` with warm-up and repetition control, and no verdict on any row.

use crate::bench::corpus;
use crate::bench::latency::co_residents_for;
use crate::bench::report::{CoResidencyLeg, StrategyLabel};
use crate::bench::rig::Rig;
use crate::bench::{BenchConfig, BenchError};
use crate::store::VectorStrategy;

/// The two strategies under comparison, in the same order as the relevance and latency legs, so a
/// reader comparing rows across the three legs is comparing like with like.
const STRATEGIES: [VectorStrategy; 2] = [
    VectorStrategy::FilterFirstExact,
    VectorStrategy::PostFilteredAnn,
];

/// Measure one namespace size: build a corpus of `namespace_total` facts with the visible set pinned
/// at `cfg.visible`, then time both strategies over the fixed query set.
///
/// `namespace_total` is floored at `cfg.visible` by [`corpus::build`], which clamps `visible` to
/// `total` — a size below the pinned visible set cannot hold it, and silently shrinking the visible
/// set instead would break the one invariant this leg exists to maintain.
///
/// The co-resident count is derived the way the latency leg derives it ([`co_residents_for`]): one
/// co-resident per visible-set-worth of remainder, so co-residents carry about as much memory as the
/// measured caller. It is derived rather than configured because it is not this leg's variable — the
/// namespace size is — and a co-resident count held fixed while the namespace grew would give
/// `fact_owner_user` a cardinality no deployment has, which is the index the filter-first arm stands
/// on.
///
/// **`rig` must be a freshly opened store.** Rebuilding a corpus in place across points measures the
/// number of rewrites the HNSW index has absorbed rather than the variable being swept (plan 06
/// Phase 3), and SurrealKV holds an exclusive directory lock for the process lifetime either way
/// (FU-005).
pub async fn measure_point(
    rig: &Rig,
    cfg: &BenchConfig,
    tenant: &str,
    namespace_total: u32,
) -> Result<Vec<CoResidencyLeg>, BenchError> {
    let co_residents = co_residents_for(cfg.visible, namespace_total);
    let built = corpus::build(rig, tenant, namespace_total, cfg.visible, co_residents).await?;
    let queries = corpus::query_set(rig.embed_dim(), cfg.queries, built.total);
    let ctx = corpus::reader_ctx(tenant, corpus::OWNER);

    let mut rows = Vec::with_capacity(STRATEGIES.len());
    for strategy in STRATEGIES {
        // One untimed pass for the fill ratio before the timed one, exactly as the latency leg takes
        // it: without it a reader cannot tell a strategy that got faster from one that stopped
        // finding anything, and at the largest namespace sizes those are the two candidate readings.
        let mut fills = Vec::with_capacity(queries.len());
        for qv in &queries {
            let ids = crate::bench::relevance::run(rig, &ctx, qv, cfg.stage1_k, strategy).await?;
            fills.push(crate::bench::relevance::fill_ratio(
                ids.len(),
                cfg.stage1_k,
                built.visible,
            ));
        }
        let fill_ratio = if fills.is_empty() {
            1.0
        } else {
            fills.iter().sum::<f64>() / fills.len() as f64
        };

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
        tracing::info!(
            target: "memory-bench",
            namespace_total = built.total,
            visible = built.visible,
            co_residents = built.co_residents,
            strategy = %StrategyLabel::of(strategy),
            p50_millis = timing.p50_millis,
            p95_millis = timing.p95_millis,
            min_max_ratio = timing.min_max_ratio,
            fill_ratio,
            "bench.coresidency.point"
        );
        rows.push(CoResidencyLeg {
            namespace_total: built.total,
            visible: built.visible,
            co_residents: built.co_residents,
            strategy: StrategyLabel::of(strategy),
            fill_ratio,
            timing,
            corpus_digest: built.digest.clone(),
        });
    }

    if let Some(winner) = faster_at(&rows, built.total) {
        // The winner's own fill ratio travels with it, so the log line cannot be quoted as a scaling
        // result when it is a starvation artefact.
        let winner_fill = rows
            .iter()
            .find(|r| r.strategy == winner)
            .map(|r| r.fill_ratio)
            .unwrap_or(1.0);
        tracing::info!(
            target: "memory-bench",
            namespace_total = built.total,
            winner = %winner,
            winner_fill_ratio = winner_fill,
            "bench.coresidency.faster"
        );
    }
    Ok(rows)
}

/// Which strategy was faster at `namespace_total`, on p50.
///
/// A convenience the report's own shape does not provide, in the same spirit as
/// [`crate::bench::relevance::better_overlap`]: it **names the smaller number**, it does not judge
/// it. Deliberately not a field on [`CoResidencyLeg`] — the report records measures and never a
/// verdict, and a `winner` on the row would read as one while hiding the margin that decides whether
/// it means anything.
///
/// **Never read without the fill ratios on the same rows.** The visible fraction falls as the
/// namespace grows, so the arm this names at the largest size may be the one that stopped returning
/// anything rather than the one that scaled (FU-012). The function names the faster arm because that
/// is a fact about the timings; whether being faster meant anything is what the fill ratio beside it
/// decides.
///
/// `None` when the size was not measured for both strategies: a comparison needs two arms, and
/// naming a lone row the winner would report a race nobody ran. p50 rather than p95 for the reason
/// the crossover uses it — at low repetition counts a nearest-rank p95 *is* the slowest sample, so a
/// comparison decided on it is decided by one unlucky observation.
pub fn faster_at(rows: &[CoResidencyLeg], namespace_total: u32) -> Option<StrategyLabel> {
    let at: Vec<&CoResidencyLeg> = rows
        .iter()
        .filter(|r| r.namespace_total == namespace_total)
        .collect();
    if at.len() < 2 {
        return None;
    }
    at.iter()
        .min_by(|a, b| {
            a.timing
                .p50_millis
                .partial_cmp(&b.timing.p50_millis)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|r| r.strategy)
}

/// The namespace sizes present in a completed leg, ascending — the grid the rows were sampled on.
///
/// Taken from the rows rather than from the configuration, so a leg that stopped early reports the
/// sizes it actually reached instead of the ones it was asked for.
pub fn sizes_measured(rows: &[CoResidencyLeg]) -> Vec<u32> {
    let mut sizes: Vec<u32> = rows.iter().map(|r| r.namespace_total).collect();
    sizes.sort_unstable();
    sizes.dedup();
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::rig::Timing;

    fn row(namespace_total: u32, strategy: StrategyLabel, p50: f64) -> CoResidencyLeg {
        CoResidencyLeg {
            namespace_total,
            visible: 1000,
            co_residents: co_residents_for(1000, namespace_total),
            strategy,
            fill_ratio: 1.0,
            timing: Timing {
                warmup_reps: 3,
                measure_reps: 9,
                p50_millis: p50,
                p95_millis: p50 * 1.2,
                p99_millis: p50 * 1.3,
                min_millis: p50 * 0.9,
                max_millis: p50 * 1.4,
                min_max_ratio: 1.55,
            },
            corpus_digest: "d".into(),
        }
    }

    /// The exact arm is measured first at every size, matching the relevance and latency legs, so a
    /// reader comparing the three legs' rows is comparing like with like.
    #[test]
    fn the_exact_arm_is_measured_first() {
        assert_eq!(STRATEGIES[0], VectorStrategy::FilterFirstExact);
        assert_eq!(STRATEGIES[1], VectorStrategy::PostFilteredAnn);
    }

    /// Naming the faster arm is naming the smaller p50, at the size asked for and no other.
    #[test]
    fn the_faster_arm_is_named_per_size() {
        use StrategyLabel::{FilterFirstExact as FF, PostFilteredAnn as ANN};
        let rows = vec![
            row(2000, FF, 4.5),
            row(2000, ANN, 9.0),
            row(20000, FF, 4.6),
            row(20000, ANN, 2.0),
        ];
        assert_eq!(faster_at(&rows, 2000), Some(FF));
        assert_eq!(faster_at(&rows, 20000), Some(ANN));
        // A size nobody measured has no winner, rather than falling back to the nearest one.
        assert_eq!(faster_at(&rows, 8000), None);
    }

    /// A size measured for one strategy has no winner: a comparison needs two arms, and naming a
    /// lone row the winner would report a race nobody ran.
    #[test]
    fn one_arm_at_a_size_names_no_winner() {
        let rows = vec![row(2000, StrategyLabel::FilterFirstExact, 4.5)];
        assert_eq!(faster_at(&rows, 2000), None);
        assert_eq!(faster_at(&[], 2000), None);
    }

    /// The grid comes from the rows, so a leg that stopped early reports where it actually looked.
    #[test]
    fn the_grid_is_the_sizes_actually_measured() {
        use StrategyLabel::{FilterFirstExact as FF, PostFilteredAnn as ANN};
        let rows = vec![
            row(8000, FF, 4.5),
            row(8000, ANN, 9.0),
            row(2000, FF, 4.4),
            row(2000, ANN, 6.0),
        ];
        assert_eq!(sizes_measured(&rows), vec![2000, 8000]);
        assert!(sizes_measured(&[]).is_empty());
    }
}
