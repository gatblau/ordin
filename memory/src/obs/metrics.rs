//! X4 — Metrics. The four quality layers (task usage, memory quality, efficiency, governance),
//! exported for scrape at `GET /metrics`.
//!
//! Phase 1 ships a minimal, dependency-free catalogue: the metric **names** match the X4 catalogue
//! exactly so the contract is fixed; counters/gauges are atomics and the Prometheus exposition is
//! rendered by hand. Components increment via the injected [`Metrics`] handle in later phases. Label
//! cardinality is bounded — no tenant id, user id, or fact id is ever a label value.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A single registered counter, rendered in the exposition by its catalogue name.
#[derive(Default)]
struct Counter {
    value: AtomicU64,
}

impl Counter {
    fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }
    fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// The metric catalogue handles, injected into components. Names are the X4 catalogue names.
///
/// Phase 1 registers the catalogue and exposes it; per-label series and histograms are refined as
/// each component lands. Unused handles carry `#[allow(dead_code)]` to keep clippy clean until the
/// owning component wires them in.
#[allow(dead_code)]
#[derive(Default)]
pub struct Metrics {
    // --- Task usage ---
    recall_requests_total: Counter,
    recall_abstain_total: Counter,
    // --- Memory quality ---
    memory_contradictions_superseded_total: Counter,
    // `memory_facts_stale_pending_refresh_total` was here. It counted facts awaiting a re-read from
    // their source — a loop ADR-014 removed when `memory` became a passive store that reaches out to
    // nothing. Nothing ever incremented it, so it exported a permanent zero under a name that
    // described a working refresh path. A reassuring zero for a subsystem that does not exist is
    // worse than an absent series.
    memory_facts_total: Counter, // gauge, modelled as a settable counter for Phase 1
    // --- Efficiency / governance counters (histograms/gauges arrive with their owners) ---
    writes_rejected_total: Counter,
    writes_quarantined_total: Counter,
    deletions_total: Counter,
    auth_decisions_total: Counter,
    // --- Write authority by fact class (ADR-019) ---
    // Split by refusal reason rather than carried on one counter, because "your token may not
    // assert this class" and "not for that source" are different misconfigurations at the token
    // issuer. The class label itself is never a label value: the vocabulary belongs to the caller
    // and is unbounded, so it would make this service's cardinality a function of somebody else's
    // domain model.
    write_authority_refusals_class_total: Counter,
    write_authority_refusals_source_total: Counter,
    /// Writes admitted **only** because the token carried no authority claim. This is what makes an
    /// absent control visible rather than silent: a deployment that believes the class rule is in
    /// force alarms on this being non-zero. A silent safe default and a silent unsafe default fail
    /// the same way, and the noise is the mitigation, not the default.
    write_authority_absent_total: Counter,
    // --- Self-metric ---
    metrics_scrape_total: Counter,
}

impl Metrics {
    /// Build the catalogue. Held behind an `Arc` and injected.
    pub fn new() -> Arc<Metrics> {
        Arc::new(Metrics::default())
    }

    /// Render the Prometheus text exposition for the registered catalogue. Carries no fact content
    /// and no identifiers. Increments the self-metric `metrics_scrape_total`.
    pub fn render(&self) -> String {
        self.metrics_scrape_total.inc();
        let lines = [
            ("recall_requests_total", self.recall_requests_total.get()),
            ("recall_abstain_total", self.recall_abstain_total.get()),
            (
                "memory_contradictions_superseded_total",
                self.memory_contradictions_superseded_total.get(),
            ),
            ("memory_facts_total", self.memory_facts_total.get()),
            ("writes_rejected_total", self.writes_rejected_total.get()),
            (
                "writes_quarantined_total",
                self.writes_quarantined_total.get(),
            ),
            ("deletions_total", self.deletions_total.get()),
            ("auth_decisions_total", self.auth_decisions_total.get()),
            (
                "memory_write_authority_absent_total",
                self.write_authority_absent_total.get(),
            ),
            ("metrics_scrape_total", self.metrics_scrape_total.get()),
        ];
        let mut out = String::new();
        for (name, value) in lines {
            out.push_str(&format!("# TYPE {name} counter\n{name} {value}\n"));
        }
        // `memory_write_authority_refusals_total` is one family with a `reason` label, so its
        // `# TYPE` line is emitted once for the family and the labelled series follow it. A `# TYPE`
        // carrying a label selector is not valid exposition and a scraper rejects the whole body.
        out.push_str("# TYPE memory_write_authority_refusals_total counter\n");
        for (reason, value) in [
            ("class", self.write_authority_refusals_class_total.get()),
            ("source", self.write_authority_refusals_source_total.get()),
        ] {
            out.push_str(&format!(
                "memory_write_authority_refusals_total{{reason=\"{reason}\"}} {value}\n"
            ));
        }
        out
    }

    /// Count a fact-class authority refusal, by reason (`"class"` or `"source"`).
    // spec: components/service-layer#SA-AUTHZ-01
    pub fn inc_write_authority_refusal(&self, reason: &str) {
        match reason {
            "source" => self.write_authority_refusals_source_total.inc(),
            _ => self.write_authority_refusals_class_total.inc(),
        }
    }

    /// Count a write admitted only because the token carried no authority claim.
    // spec: phase-1-analysis#SA-AUTHZ-02
    pub fn inc_write_authority_absent(&self) {
        self.write_authority_absent_total.inc();
    }

    /// Increment the request counter (task-usage layer). Wired by C6/C8 in later phases.
    #[allow(dead_code)]
    pub fn inc_recall_requests(&self) {
        self.recall_requests_total.inc();
    }

    /// Increment the abstain counter. Wired by C6 in a later phase.
    #[allow(dead_code)]
    pub fn inc_recall_abstain(&self) {
        self.recall_abstain_total.inc();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The refusal counter is one family with a `reason` label, so the exposition must carry exactly
    /// one `# TYPE` line for it and two series beneath. A `# TYPE` line carrying a label selector is
    /// invalid and makes a scraper reject the entire body — which would take every other metric in
    /// this catalogue down with it, not just this one.
    #[test]
    fn refusal_family_renders_one_type_line_and_two_labelled_series() {
        let m = Metrics::new();
        m.inc_write_authority_refusal("class");
        m.inc_write_authority_refusal("source");
        m.inc_write_authority_refusal("source");
        let out = m.render();

        assert_eq!(
            out.matches("# TYPE memory_write_authority_refusals_total counter")
                .count(),
            1,
            "exactly one TYPE line for the family:\n{out}"
        );
        assert!(
            out.contains("memory_write_authority_refusals_total{reason=\"class\"} 1"),
            "class series:\n{out}"
        );
        assert!(
            out.contains("memory_write_authority_refusals_total{reason=\"source\"} 2"),
            "source series:\n{out}"
        );
        // No TYPE line anywhere may carry a label selector.
        for line in out.lines().filter(|l| l.starts_with("# TYPE")) {
            assert!(!line.contains('{'), "TYPE line must not carry labels: {line}");
        }
    }

    /// The absent-claim counter is what makes a missing control visible, so it is a plain unlabelled
    /// series a deployment can alert on directly.
    #[test]
    fn absent_counter_is_rendered_and_counts() {
        let m = Metrics::new();
        assert!(m.render().contains("memory_write_authority_absent_total 0"));
        m.inc_write_authority_absent();
        assert!(m.render().contains("memory_write_authority_absent_total 1"));
    }

    /// An unrecognised reason falls to the class bucket rather than being dropped: a refusal that
    /// increments nothing is a refusal an operator cannot see.
    #[test]
    fn unknown_refusal_reason_still_counts() {
        let m = Metrics::new();
        m.inc_write_authority_refusal("something-else");
        assert!(m
            .render()
            .contains("memory_write_authority_refusals_total{reason=\"class\"} 1"));
    }
}
