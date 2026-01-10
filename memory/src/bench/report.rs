//! The re-pointed report (plan 06 Phase 2) — the shape every leg writes into, fixed before any leg
//! exists to write into it.
//!
//! The ordering is deliberate. RFC 03 keeps this component's report *shape* and replaces its legs
//! (`docs/wip/rfc/03-scalability-first-architecture.md`), and three legs landing in three phases is
//! three chances to invent three shapes. Pinning it once, up front, is what keeps the relevance,
//! latency and co-residency rows comparable to each other and to a rerun.
//!
//! Two properties are load-bearing rather than decorative:
//!
//! * **Every row carries the rig, not only the number.** `engine`, `engine_version` and
//!   [`HostDisk`] sit at the top of the report, and every row carries its own [`Timing`] including
//!   `min_max_ratio`. The defect that forced the re-pointing (FU-024) was a 22x spread between two
//!   runs of one nominal configuration; a median-only row can reproduce that and look steady doing
//!   it. RFC 03 open question 4c may also move the embedded backend from SurrealKV to RocksDB, and a
//!   figure quoted without its engine outlives the engine it was true of (RISK-002).
//! * **No verdict fields.** The report records measures and the thresholds they are read against; it
//!   does not decide whether a relevance delta or a latency figure is acceptable. A boolean would
//!   hide a user decision behind a number.
//!
//! The store URL reaches the report by **scheme only**, unchanged from the C11 spec: a remote URL may
//! carry credentials in its authority, so the full value is never written to a file or a log.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bench::rig::Timing;
use crate::bench::BenchError;
use crate::store::VectorStrategy;

/// Which retrieval strategy a row was measured under.
///
/// A report-owned label rather than `store::VectorStrategy` itself, which is `pub(crate)` (rule C12)
/// and would not be nameable from the report's readers. The two spellings are kept in step by
/// [`StrategyLabel::of`], which is the only way one is produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StrategyLabel {
    /// Walk the HNSW index for K approximate neighbours, then apply the scope filter.
    PostFilteredAnn,
    /// Apply the scope filter first, then score every surviving row exactly.
    FilterFirstExact,
}

impl StrategyLabel {
    /// The label for the strategy a leg actually ran.
    // Consumed by the relevance, latency and co-residency legs (plan 06 Phases 3-5); until those
    // land its only caller is the label test, which a non-test build cannot see.
    #[allow(dead_code)]
    pub(crate) fn of(strategy: VectorStrategy) -> Self {
        match strategy {
            VectorStrategy::PostFilteredAnn => Self::PostFilteredAnn,
            VectorStrategy::FilterFirstExact => Self::FilterFirstExact,
        }
    }

    /// The configured spelling, for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PostFilteredAnn => "post-filtered-ann",
            Self::FilterFirstExact => "filter-first-exact",
        }
    }
}

impl std::fmt::Display for StrategyLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the report can say about the storage underneath the measurement.
///
/// `label` is operator-supplied (`MEMORY_BENCH_HOST_DISK`) and defaults to `unspecified`. That is not
/// laziness: a process can read the path it wrote to, but it cannot tell an NVMe device from a SATA
/// SSD from a network volume, and an absolute latency figure is dominated by exactly that difference
/// (RISK-003). Recording an operator's description beside an unlabelled default is honest; deriving a
/// plausible one would put a fabricated fact next to a measured one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDisk {
    /// `std::env::consts::OS`.
    pub os: String,
    /// `std::env::consts::ARCH`.
    pub arch: String,
    /// `MEMORY_BENCH_HOST_DISK` verbatim, or `unspecified`.
    pub label: String,
}

impl HostDisk {
    /// The host as this process can see it, with `label` supplied by the caller.
    pub fn describe(label: impl Into<String>) -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            label: label.into(),
        }
    }
}

/// A provisioning ceiling, carried over from the C11 spec unchanged.
///
/// A ceiling is not a failure: it exits 0 and is reported, because finding one is what the instrument
/// is for. `None` on every re-pointed leg so far — the legs that could hit one provisioned tens of
/// thousands of namespaces, and RFC 03 withdrew them — but the field stays because an engine refusal
/// during a large corpus build is the same event and belongs in the same place.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CeilingHit {
    /// 0-based index of the first item that failed.
    pub failed_at_index: u32,
    /// The `StoreError` rendered verbatim, untruncated.
    pub engine_error: String,
}

/// One point of the relevance leg — RFC 03 acceptance criterion 3, filled in plan 06 Phase 3.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelevanceLeg {
    /// Distinct co-resident owner ids the invisible remainder was spread across.
    pub co_residents: u32,
    /// Facts visible to the measured caller.
    pub visible: u32,
    /// Facts in the namespace.
    pub total: u32,
    pub strategy: StrategyLabel,
    /// Queries in the fixed query set this point averaged over.
    pub queries: u32,
    /// Mean `|returned n baseline| / |baseline|` across the query set, [0,1].
    pub overlap: f64,
    /// Mean rank correlation against the baseline ordering, [-1,1].
    pub rank_correlation: f64,
    /// The starvation measure: `returned_visible / min(stage1_k, visible)`, [0,1].
    ///
    /// A ratio to what was *available*, not a raw count. The failure mode RISK-013 names is the
    /// post-filter discarding the caller's rows after the index walk, which shows up as this
    /// collapsing while the raw count stays plausible.
    pub fill_ratio: f64,
    pub timing: Timing,
    /// The corpus this point ran against, so two rows can be shown to have measured the same data.
    pub corpus_digest: String,
}

/// One point of the visible-fraction latency leg — RFC 03 acceptance criterion 4, filled in plan 06
/// Phase 4.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatencyLeg {
    /// `visible / total`.
    pub visible_fraction: f64,
    pub visible: u32,
    pub total: u32,
    pub strategy: StrategyLabel,
    /// `returned / min(stage1_k, visible)`, as the relevance leg measures it.
    ///
    /// On a *latency* row because a timing without it can be read exactly backwards. The post-filtered
    /// arm gets faster as the visible fraction falls — not because the index walk gets cheaper, but
    /// because it finds and returns almost nothing, so there is less to resolve. A crossover derived
    /// from timings alone would record the point at which the index path "wins" while it is in fact
    /// starving. The plan says as much in its own rationale: a latency win on a strategy that returns
    /// the wrong rows is not a win.
    pub fill_ratio: f64,
    pub timing: Timing,
    pub corpus_digest: String,
}

/// Where the two strategies swap places, as an **interval on the sampling grid** — never a point
/// estimate the grid cannot support.
///
/// RFC 03 acceptance criterion 4 asks for the crossover to be "measured and recorded". It was already
/// known to sit "somewhere between 12.5% and 5%" without being pinned, and a grid of six fractions
/// cannot locate it more finely than the two fractions either side of it. Reporting the bracketing
/// pair and the grid it was sampled on is the honest form; interpolating a single number between two
/// samples would state a precision nobody measured.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Crossover {
    /// The larger fraction of the bracketing pair — at and above it, the index path was faster.
    /// `None` when no adjacent pair on the grid changed winner.
    pub upper_fraction: Option<f64>,
    /// The smaller fraction of the bracketing pair — at and below it, filter-first was faster.
    pub lower_fraction: Option<f64>,
    /// The fractions actually sampled, descending. Recorded even when the bounds are `None`: a run
    /// that never crossed still says *where it looked*, and a reader can tell a grid too coarse to
    /// catch the crossing from a grid on which no crossing exists.
    pub grid: Vec<f64>,
    /// How many adjacent pairs on the grid changed winner.
    ///
    /// One is the clean case the bounds describe. More than one means the bounds span several
    /// crossings — noise, or a genuinely non-monotonic result — and a reader who sees only the
    /// interval would read a widened bracket as a single confident one.
    pub transitions: u32,
}

/// One point of the co-residency leg — the visible set pinned while the namespace grows underneath
/// it, filled in plan 06 Phase 5.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoResidencyLeg {
    /// Facts in the namespace at this point.
    pub namespace_total: u32,
    /// Facts visible to the measured caller — identical at every point, which is the whole design of
    /// this leg: a latency difference cannot then be attributed to the caller's own corpus growing.
    pub visible: u32,
    pub co_residents: u32,
    pub strategy: StrategyLabel,
    /// `returned / min(stage1_k, visible)`, as the relevance and latency legs measure it.
    ///
    /// On this row for the same reason it is on a latency row, and the reason applies here whether or
    /// not the leg's axis is called a fraction: pinning the visible set while the namespace grows
    /// *lowers* the visible fraction at every step, which is precisely the regime in which the
    /// post-filtered arm's latency falls because it finds and returns almost nothing. Without this
    /// field a reader would see the index path overtake filter-first at the largest namespace and
    /// record it as the scaling result the leg exists to produce (FU-012).
    pub fill_ratio: f64,
    pub timing: Timing,
    pub corpus_digest: String,
}

/// The run report written to `MEMORY_BENCH_REPORT_PATH`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchReport {
    /// The storage engine the measurement ran against, as far as a client can know it: the endpoint
    /// scheme for an embedded store (`surrealkv`, `mem`), and `remote-<scheme>` for a server whose
    /// backing engine the client cannot observe.
    pub engine: String,
    /// The engine's own version string, asked of the engine rather than assumed from the crate
    /// version. `unknown` when the endpoint would not answer — an unanswered probe must not fail a
    /// run, but it must also not be quietly filled in.
    pub engine_version: String,
    pub host_disk: HostDisk,
    pub embed_dim: u32,
    /// Scheme only; the full URL is never written to the report.
    pub store_url_scheme: String,
    pub relevance: Vec<RelevanceLeg>,
    pub latency: Vec<LatencyLeg>,
    /// Where the two strategies swap places, on the grid the latency leg sampled. `None` until that
    /// leg runs.
    pub crossover: Option<Crossover>,
    /// NFR-P3's stage-1 vector budget in milliseconds, recorded **as context** so a reader has the
    /// threshold beside the measurement without the report deciding anything. There is deliberately
    /// no field saying whether a row met it: that is a user's call, and a boolean would hide it.
    pub nfr_p3_millis: f64,
    pub co_residency: Vec<CoResidencyLeg>,
    /// `Some(_)` only when the engine refused before a leg's population was complete.
    pub ceiling: Option<CeilingHit>,
}

impl BenchReport {
    /// A report with the rig named and no legs run yet.
    ///
    /// Every run writes this much even when no leg produces a row, because a run that measured
    /// nothing and a run whose report never got written are different events, and an empty file
    /// cannot tell them apart.
    pub fn new(
        engine: impl Into<String>,
        engine_version: impl Into<String>,
        host_disk: HostDisk,
        embed_dim: u32,
        store_url_scheme: impl Into<String>,
    ) -> Self {
        Self {
            engine: engine.into(),
            engine_version: engine_version.into(),
            host_disk,
            embed_dim,
            store_url_scheme: store_url_scheme.into(),
            relevance: Vec::new(),
            latency: Vec::new(),
            crossover: None,
            nfr_p3_millis: NFR_P3_MILLIS,
            co_residency: Vec::new(),
            ceiling: None,
        }
    }

    /// Serialise to `path`, pretty-printed so a reviewer reads it without a tool.
    ///
    /// A serialisation failure and an IO failure both surface as [`BenchError::ReportWrite`] carrying
    /// the path: from an operator's side they are one event — the numbers did not reach the disk.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), BenchError> {
        let path = path.as_ref();
        let body = serde_json::to_string_pretty(self).map_err(|e| BenchError::ReportWrite {
            path: path.to_path_buf(),
            source: std::io::Error::other(e),
        })?;
        std::fs::write(path, body).map_err(|source| BenchError::ReportWrite {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// NFR-P3: the stage-1 vector signal's latency budget, in milliseconds. Context for the latency leg's
/// rows, never a verdict on them.
pub const NFR_P3_MILLIS: f64 = 50.0;

/// The engine named by an endpoint scheme.
///
/// `mem` and `surrealkv` are engines this process links and runs in-process, so the scheme names the
/// engine exactly. A `ws` or `http` endpoint is a server whose storage backend the client never sees,
/// so it is recorded as remote rather than guessed — the point of the field is that a figure is never
/// quoted against an engine nobody checked.
pub fn engine_from_scheme(scheme: &str) -> String {
    match scheme {
        "surrealkv" | "mem" | "rocksdb" | "tikv" | "fdb" => scheme.to_string(),
        "" => "unknown".to_string(),
        other => format!("remote-{other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing() -> Timing {
        Timing {
            warmup_reps: 3,
            measure_reps: 9,
            p50_millis: 4.0,
            p95_millis: 7.0,
            p99_millis: 9.0,
            min_millis: 3.0,
            max_millis: 9.0,
            min_max_ratio: 3.0,
        }
    }

    /// The two strategy spellings must not drift: the label is derived from the store's own enum,
    /// never written out by hand at a call site.
    #[test]
    fn every_strategy_has_exactly_one_label() {
        assert_eq!(
            StrategyLabel::of(VectorStrategy::PostFilteredAnn),
            StrategyLabel::PostFilteredAnn
        );
        assert_eq!(
            StrategyLabel::of(VectorStrategy::FilterFirstExact),
            StrategyLabel::FilterFirstExact
        );
        assert_eq!(
            StrategyLabel::PostFilteredAnn.as_str(),
            "post-filtered-ann"
        );
        assert_eq!(
            StrategyLabel::FilterFirstExact.as_str(),
            "filter-first-exact"
        );
    }

    /// An embedded scheme names its engine; a remote one names only that it is remote, because the
    /// client cannot see the server's storage backend and a guess here would be quoted as a fact.
    #[test]
    fn only_an_embedded_scheme_names_its_engine() {
        assert_eq!(engine_from_scheme("surrealkv"), "surrealkv");
        assert_eq!(engine_from_scheme("mem"), "mem");
        assert_eq!(engine_from_scheme("ws"), "remote-ws");
        assert_eq!(engine_from_scheme("https"), "remote-https");
        assert_eq!(engine_from_scheme(""), "unknown");
    }

    /// A round trip through the file the operator actually reads, not through an in-memory value:
    /// the report is only useful if it survives serialisation, and every row's spread survives with
    /// it.
    #[test]
    fn a_written_report_round_trips_with_its_spread_intact() {
        let dir = std::env::temp_dir().join(format!("memory-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("bench-report.json");

        let mut r = BenchReport::new(
            "surrealkv",
            "3.1.5",
            HostDisk::describe("unspecified"),
            8,
            "surrealkv",
        );
        r.latency.push(LatencyLeg {
            visible_fraction: 0.5,
            visible: 1000,
            total: 2000,
            strategy: StrategyLabel::FilterFirstExact,
            fill_ratio: 1.0,
            timing: timing(),
            corpus_digest: "abc".into(),
        });
        r.write(&path).expect("write the report");

        let back: BenchReport =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read back"))
                .expect("parse the report");
        assert_eq!(back, r);
        assert_eq!(back.latency[0].timing.min_max_ratio, 3.0);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The report refuses to be written where it cannot be written, rather than reporting a success
    /// nobody can read.
    #[test]
    fn an_unwritable_path_is_a_report_write_error() {
        let r = BenchReport::new("mem", "unknown", HostDisk::default(), 8, "mem");
        let path = std::env::temp_dir().join("memory-no-such-dir-1234567/report.json");
        match r.write(&path) {
            Err(BenchError::ReportWrite { path: p, .. }) => assert_eq!(p, path),
            other => panic!("expected ReportWrite, got {other:?}"),
        }
    }
}
