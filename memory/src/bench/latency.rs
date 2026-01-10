//! The absolute-latency leg — RFC 03 acceptance criterion 4 (plan 06 Phase 4).
//!
//! This is the leg FU-024 was raised for. The criterion: *"Stage-1 vector latency stays inside NFR-P3
//! (≤ 50 ms) for a filtered set of 1,000 facts, and the crossover point at which HNSW becomes faster
//! is measured and recorded."*
//!
//! The visible set is **pinned at one consumer's memory** and the namespace around it is sized to hit
//! each fraction on the grid. That is the whole design: an absolute claim about a 1,000-fact filtered
//! set has to be measured at 1,000 facts, at every fraction, or the number moves with the caller's own
//! corpus rather than with the thing under test.
//!
//! Two things this leg deliberately does not do.
//!
//! **It reports no verdict against NFR-P3.** The threshold travels on the report as context
//! ([`crate::bench::report::NFR_P3_MILLIS`]) beside the measured percentiles. Whether a figure taken
//! on this rig, on this operator's disk, clears a deployment budget is a judgement about the rig as
//! much as about the number — RISK-003 — and a boolean would hide it.
//!
//! **It reports the crossover as an interval, never a point.** RFC 03 already located it "somewhere
//! between 12.5% and 5%" without pinning it, and a six-fraction grid cannot do better than name the
//! two samples either side. Interpolating between them would state a precision nobody measured
//! (RISK-004).

use crate::bench::corpus;
use crate::bench::report::{Crossover, LatencyLeg, StrategyLabel};
use crate::bench::rig::Rig;
use crate::bench::{BenchConfig, BenchError};
use crate::store::VectorStrategy;

/// The two strategies under comparison. Same order as the relevance leg, so a reader comparing the
/// two legs' rows is comparing like with like.
const STRATEGIES: [VectorStrategy; 2] = [
    VectorStrategy::FilterFirstExact,
    VectorStrategy::PostFilteredAnn,
];

/// The namespace size that puts `visible` facts at `fraction` of it.
///
/// Rounded up and floored at `visible`, so a fraction of 1.0 — or any fraction that would round the
/// namespace below the visible set — gives a corpus the caller can see entirely rather than one that
/// silently claims more visible facts than it holds.
pub fn total_for(visible: u32, fraction: f64) -> u32 {
    if fraction <= 0.0 {
        return visible;
    }
    let total = (visible as f64 / fraction).ceil();
    if total >= u32::MAX as f64 {
        u32::MAX
    } else {
        (total as u32).max(visible)
    }
}

/// How many co-resident users hold the invisible remainder at this size.
///
/// One co-resident per visible-set-worth of remainder: co-residents carry about as much memory as the
/// measured caller does, which is the shape a consumer deployment actually has. It matters to the
/// measurement rather than being decoration — `fact_owner_user` is the index the filter-first arm
/// stands on, and its selectivity is a function of how many distinct owners the namespace holds. A
/// single co-resident owning ninety-nine thousand facts would give that index a cardinality no
/// deployment would.
pub fn co_residents_for(visible: u32, total: u32) -> u32 {
    ((total.saturating_sub(visible)) / visible.max(1)).max(1)
}

/// Measure one visible fraction: build a corpus sized to it with the visible set pinned, then time
/// both strategies over the fixed query set.
///
/// `rig` must be a freshly opened store — see the relevance leg for why a reused one measures the
/// number of rewrites the index has absorbed rather than the variable being swept.
pub async fn measure_point(
    rig: &Rig,
    cfg: &BenchConfig,
    tenant: &str,
    visible: u32,
    fraction: f64,
) -> Result<Vec<LatencyLeg>, BenchError> {
    let total = total_for(visible, fraction);
    let co_residents = co_residents_for(visible, total);
    let built = corpus::build(rig, tenant, total, visible, co_residents).await?;
    let queries = corpus::query_set(rig.embed_dim(), cfg.queries, built.total);
    let ctx = corpus::reader_ctx(tenant, corpus::OWNER);

    let mut rows = Vec::with_capacity(STRATEGIES.len());
    for strategy in STRATEGIES {
        // One untimed pass for the fill ratio, before the timed one. It costs a query set against a
        // corpus that took minutes to build, and without it a reader cannot tell a strategy that got
        // faster from one that stopped finding anything.
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
            // The *achieved* fraction, not the requested one: `total_for` rounds, and at small
            // corpora the two differ by enough to matter to whoever reads the grid.
            visible_fraction = built.visible_fraction(),
            strategy = %StrategyLabel::of(strategy),
            visible = built.visible,
            total = built.total,
            p50_millis = timing.p50_millis,
            p95_millis = timing.p95_millis,
            min_max_ratio = timing.min_max_ratio,
            fill_ratio,
            "bench.latency.point"
        );
        rows.push(LatencyLeg {
            visible_fraction: built.visible_fraction(),
            visible: built.visible,
            total: built.total,
            strategy: StrategyLabel::of(strategy),
            fill_ratio,
            timing,
            corpus_digest: built.digest.clone(),
        });
    }
    Ok(rows)
}

/// Derive the crossover from a completed set of latency rows.
///
/// The winner at each fraction is decided on **p50**, not p95. At the default nine measurement
/// repetitions a nearest-rank p95 *is* the slowest sample, so a crossover derived from it would be
/// decided by one unlucky observation per point. NFR-P3's own p95 is still on every row; it is the
/// threshold's statistic, not the comparison's.
///
/// A transition is an adjacent pair on the descending grid whose winner differs. With one, the bounds
/// are that pair. With several — noise, or a genuinely non-monotonic result — the bounds span all of
/// them and `transitions` says so, because a widened bracket reported alone reads like a confident
/// narrow one. With none, both bounds are `None` and the grid is still recorded: a run that did not
/// cross has still said where it looked.
pub fn crossover_from(rows: &[LatencyLeg]) -> Crossover {
    let mut points: Vec<(f64, Option<StrategyLabel>)> = Vec::new();
    let mut fractions: Vec<f64> = rows.iter().map(|r| r.visible_fraction).collect();
    fractions.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    fractions.dedup();

    for f in &fractions {
        let at: Vec<&LatencyLeg> = rows
            .iter()
            .filter(|r| r.visible_fraction == *f)
            .collect();
        let winner = at
            .iter()
            .min_by(|a, b| {
                a.timing
                    .p50_millis
                    .partial_cmp(&b.timing.p50_millis)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            // A fraction measured for only one strategy has no winner to report — a comparison needs
            // two arms, and treating a lone row as a win would invent a transition.
            .filter(|_| at.len() >= 2)
            .map(|r| r.strategy);
        points.push((*f, winner));
    }

    let mut transitions = 0u32;
    let mut upper: Option<f64> = None;
    let mut lower: Option<f64> = None;
    for pair in points.windows(2) {
        let (hi_f, hi_w) = pair[0];
        let (lo_f, lo_w) = pair[1];
        match (hi_w, lo_w) {
            (Some(a), Some(b)) if a != b => {
                transitions += 1;
                upper = Some(upper.map_or(hi_f, |u: f64| u.max(hi_f)));
                lower = Some(lower.map_or(lo_f, |l: f64| l.min(lo_f)));
            }
            _ => {}
        }
    }

    Crossover {
        upper_fraction: upper,
        lower_fraction: lower,
        grid: fractions,
        transitions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::rig::Timing;

    fn row(fraction: f64, strategy: StrategyLabel, p50: f64) -> LatencyLeg {
        LatencyLeg {
            visible_fraction: fraction,
            visible: 1000,
            total: (1000.0 / fraction) as u32,
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

    /// The visible set is pinned and the namespace is sized around it, so the fraction is the only
    /// thing that moves between points.
    #[test]
    fn the_namespace_is_sized_to_the_fraction_and_never_below_the_visible_set() {
        assert_eq!(total_for(1000, 1.0), 1000);
        assert_eq!(total_for(1000, 0.5), 2000);
        assert_eq!(total_for(1000, 0.125), 8000);
        assert_eq!(total_for(1000, 0.01), 100_000);
        // A fraction above 1.0 cannot mean fewer facts than the caller owns.
        assert_eq!(total_for(1000, 2.0), 1000);
        assert_eq!(total_for(1000, 0.0), 1000);
        assert_eq!(total_for(1000, -1.0), 1000);
        // Rounding is up, so the achieved fraction never exceeds the requested one.
        assert_eq!(total_for(1000, 0.3), 3334);
    }

    /// Co-residents hold about as much memory as the measured caller, because `fact_owner_user`'s
    /// selectivity — which the filter-first arm stands on — is a function of the distinct owner count.
    #[test]
    fn co_residents_scale_with_the_namespace() {
        assert_eq!(co_residents_for(1000, 1000), 1);
        assert_eq!(co_residents_for(1000, 2000), 1);
        assert_eq!(co_residents_for(1000, 8000), 7);
        assert_eq!(co_residents_for(1000, 100_000), 99);
        // A zero visible set divides by one rather than by zero: every fact is invisible, and one
        // co-resident per fact is the shape that follows from "co-residents hold as much as the
        // caller" when the caller holds nothing.
        assert_eq!(co_residents_for(0, 100), 100);
    }

    /// One clean transition: the bounds are the pair that bracket it, and the grid travels with them.
    #[test]
    fn a_single_crossing_is_reported_as_the_pair_that_brackets_it() {
        use StrategyLabel::{FilterFirstExact as FF, PostFilteredAnn as ANN};
        let rows = vec![
            row(0.5, FF, 20.0),
            row(0.5, ANN, 5.0), // index wins at a high fraction
            row(0.125, FF, 6.0),
            row(0.125, ANN, 4.0), // still the index
            row(0.05, FF, 3.0),
            row(0.05, ANN, 8.0), // filter-first takes over
            row(0.01, FF, 1.0),
            row(0.01, ANN, 9.0),
        ];
        let c = crossover_from(&rows);
        assert_eq!(c.transitions, 1);
        assert_eq!(c.upper_fraction, Some(0.125));
        assert_eq!(c.lower_fraction, Some(0.05));
        assert_eq!(c.grid, vec![0.5, 0.125, 0.05, 0.01]);
    }

    /// A grid on which one strategy wins everywhere records `null` bounds and keeps the grid — the
    /// run still said where it looked, and an invented point would be worse than no point.
    #[test]
    fn a_grid_that_never_crosses_records_no_bounds_and_keeps_the_grid() {
        use StrategyLabel::{FilterFirstExact as FF, PostFilteredAnn as ANN};
        let rows = vec![
            row(0.5, FF, 2.0),
            row(0.5, ANN, 9.0),
            row(0.05, FF, 1.0),
            row(0.05, ANN, 8.0),
        ];
        let c = crossover_from(&rows);
        assert_eq!(c.transitions, 0);
        assert_eq!(c.upper_fraction, None);
        assert_eq!(c.lower_fraction, None);
        assert_eq!(c.grid, vec![0.5, 0.05]);
    }

    /// Several crossings widen the bracket, and `transitions` is what stops the widened bracket being
    /// read as one confident interval.
    #[test]
    fn several_crossings_widen_the_bracket_and_are_counted() {
        use StrategyLabel::{FilterFirstExact as FF, PostFilteredAnn as ANN};
        let rows = vec![
            row(0.5, FF, 9.0),
            row(0.5, ANN, 2.0),
            row(0.25, FF, 2.0),
            row(0.25, ANN, 9.0),
            row(0.125, FF, 9.0),
            row(0.125, ANN, 2.0),
            row(0.05, FF, 2.0),
            row(0.05, ANN, 9.0),
        ];
        let c = crossover_from(&rows);
        assert_eq!(c.transitions, 3);
        assert_eq!(c.upper_fraction, Some(0.5));
        assert_eq!(c.lower_fraction, Some(0.05));
    }

    /// A fraction measured for only one strategy has no winner: a comparison needs two arms, and
    /// treating a lone row as a win would invent a transition out of a missing measurement.
    #[test]
    fn a_fraction_with_one_arm_cannot_produce_a_transition() {
        use StrategyLabel::{FilterFirstExact as FF, PostFilteredAnn as ANN};
        let rows = vec![row(0.5, ANN, 2.0), row(0.05, FF, 1.0)];
        let c = crossover_from(&rows);
        assert_eq!(c.transitions, 0);
        assert_eq!(c.grid, vec![0.5, 0.05]);
    }

    #[test]
    fn an_empty_leg_has_an_empty_grid_and_no_bounds() {
        let c = crossover_from(&[]);
        assert_eq!(c.transitions, 0);
        assert!(c.grid.is_empty());
        assert_eq!(c.upper_fraction, None);
    }
}
