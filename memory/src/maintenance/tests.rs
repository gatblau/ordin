//! Unit tests for the pure, I/O-free maintenance cores (ADR-010 algorithmic-core unit tests). Each
//! shared path is exercised with at least three table-driven cases.

use super::*;
use crate::types::domain::{MemoryClass, Visibility};

/// Build a minimal fact for the contradiction-detection table tests.
fn fact(
    id: &str,
    entities: &[&str],
    subject: &str,
    predicate: &str,
    object: &str,
    valid_from: DateTime<Utc>,
    confidence: f64,
) -> Fact {
    Fact {
        collection: String::new(),
        id: id.into(),
        content: serde_json::json!({
            "subject": subject, "predicate": predicate, "object": object
        }),
        entities: entities.iter().map(|e| e.to_string()).collect(),
        source_id: None,
        memory_class: MemoryClass::Semantic,
        fact_class: None,
        visibility: Visibility::UserPrivate,
        owner: ScopeRef {
            tenant: "acme".into(),
            team: None,
            user: "u-1".into(),
        },
        valid_from,
        valid_to: None,
        ingested_at: valid_from,
        confidence,
        salience: 0.5,
        stability: 1.0,
        pii_review: false,
        supersedes: None,
        superseded_by: None,
        derived_from: vec![],
        last_recalled_at: None,
        single_valued: false,
        changed: false,
        triple_key: None,
    }
}

fn dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

#[test]
fn retrievability_cases() {
    // (delta_secs, stability, k, expected_approx)
    let cases = [
        // Spec example: 10 days, s=1, k=10 -> exp(-86400) underflows to ~0.
        (864000.0_f64, 1.0_f64, 10.0_f64, 0.0_f64),
        // No elapsed time -> R = 1.0.
        (0.0, 1.0, 10.0, 1.0),
        // Clock skew (negative delta) clamps to 0 -> R = 1.0.
        (-500.0, 1.0, 10.0, 1.0),
        // One stability-window elapsed (delta = s*k) -> exp(-1) ~= 0.3679.
        (10.0, 1.0, 10.0, std::f64::consts::E.recip()),
    ];
    for (delta, s, k, expected) in cases {
        let r = retrievability(delta, s, k);
        assert!(
            (r - expected).abs() < 1e-4,
            "retrievability({delta},{s},{k}) = {r}, expected ~{expected}"
        );
    }
    // Zero stability must not divide-by-zero / NaN.
    let r0 = retrievability(100.0, 0.0, 10.0);
    assert!(
        r0.is_finite() && (0.0..=1.0).contains(&r0),
        "zero stability gave {r0}"
    );
}

#[test]
fn is_prune_candidate_cases() {
    let floor = 0.3;
    let prune_r = 0.05;
    // (r, salience, expected)
    let cases = [
        // Low R, low salience -> prune.
        (0.0, 0.1, true),
        // Low R, high salience -> survives disuse (ADR-006).
        (0.0, 0.9, false),
        // High R, low salience -> not yet a prune candidate.
        (0.5, 0.1, false),
        // Salience exactly at the floor -> not below the floor -> survives.
        (0.0, 0.3, false),
        // R exactly at the prune threshold -> not strictly below -> survives.
        (0.05, 0.1, false),
    ];
    for (r, salience, expected) in cases {
        assert_eq!(
            is_prune_candidate(r, salience, prune_r, floor),
            expected,
            "is_prune_candidate(r={r}, sal={salience})"
        );
    }
}

#[test]
fn reinforce_raises_stability_and_resets_clock() {
    let now = dt("2026-01-10T12:00:00.000Z");
    // (stability, gain, expected_stability)
    let cases = [
        (1.0_f64, 0.5_f64, 1.5_f64),
        (2.0, 0.5, 3.0),
        (0.5, 1.0, 1.0),
    ];
    for (s, gain, expected) in cases {
        let (new_s, when) = reinforce(s, gain, now);
        assert!(
            (new_s - expected).abs() < 1e-9,
            "reinforce({s},{gain}) = {new_s}"
        );
        assert!(new_s >= s, "stability must not fall");
        assert_eq!(when, now, "decay clock must reset to now");
    }
}

/// The maintenance-worker amendment's Scenario Outline "The pure comparison" (ADR-022), and the
/// cases that keep two facts apart.
// spec: components/maintenance-worker#SA-TRIPLE-09
#[test]
fn facts_conflict_cases() {
    let t = dt("2026-01-10T12:00:00.000Z");
    let a = fact("fact:a", &["entity:e1"], "s", "p", "x", t, 0.5);
    let y = fact("fact:b", &["entity:e1"], "s", "p", "y", t, 0.5);
    let same = fact("fact:c", &["entity:e1"], "s", "p", "x", t, 0.5);
    let mut other_owner = y.clone();
    other_owner.owner.user = "u-2".into();
    // SPEC-GAP-1: the outline's row `collection "tk:a" | collection ""` reads `false`; the rule text
    // ("without its visibility and collection tests") and SA-TRIPLE-09 give `true`, tested here.
    let mut other_folder = y.clone();
    other_folder.collection = "tk:a".into();
    let mut other_visibility = y.clone();
    other_visibility.visibility = Visibility::TeamShared;
    let no_shared_entity = fact("fact:d", &["entity:e2"], "s", "p", "y", t, 0.5);
    let other_predicate = fact("fact:e", &["entity:e1"], "s", "q", "y", t, 0.5);
    let cases: [(&Fact, &Fact, bool); 7] = [
        (&a, &y, true),
        (&a, &same, false),
        (&a, &other_owner, false),
        (&other_folder, &a, true),
        (&a, &other_visibility, true),
        (&a, &no_shared_entity, false),
        (&a, &other_predicate, false),
    ];
    for (i, (x, z, want)) in cases.iter().enumerate() {
        assert_eq!(facts_conflict(x, z), *want, "case {i}");
        assert_eq!(facts_conflict(z, x), *want, "case {i} reversed");
    }
}

/// A triple written after `0011_triples` has an object that is not a string, so the duty's comparison
/// never matches it (it is compared at the write instead, SA-TRIPLE-09).
#[test]
fn a_triple_object_is_not_compared_by_the_duty() {
    let t = dt("2026-01-10T12:00:00.000Z");
    let mut a = fact("fact:a", &["entity:e1"], "s", "p", "x", t, 0.5);
    let mut b = a.clone();
    b.id = "fact:b".into();
    a.content = serde_json::json!({"subject": "s", "predicate": "p", "object": {"kind": "entity", "name": "x"}});
    b.content = serde_json::json!({"subject": "s", "predicate": "p", "object": {"kind": "entity", "name": "y"}});
    assert!(!facts_conflict(&a, &b));
}

// --- SA-GRAPH-07: fading -----------------------------------------------------------------------

fn decay() -> DecayConfig {
    DecayConfig {
        decay_k: 864_000.0,
        prune_retrievability: 0.05,
    }
}

fn episodic(last_used_days_ago: Option<i64>, ingested_days_ago: i64, now: DateTime<Utc>) -> Fact {
    let mut f = fact("fact:f", &["entity:e"], "s", "p", "o", now, 0.9);
    f.memory_class = MemoryClass::Episodic;
    f.ingested_at = now - chrono::Duration::days(ingested_days_ago);
    f.last_recalled_at = last_used_days_ago.map(|d| now - chrono::Duration::days(d));
    f
}

#[test]
fn fading_follows_the_last_fifth_of_the_decay_curve() {
    let now = dt("2026-06-01T00:00:00.000Z");
    // (last recalled days ago, ingested days ago, expected fading)
    let cases = [
        (Some(25), 100, true),
        (Some(23), 100, false),
        (None, 26, true),
        (None, 10, false),
    ];
    for (last, ingested, want) in cases {
        let f = episodic(last, ingested, now);
        assert_eq!(is_fading(&f, now, &decay()), want, "{last:?} {ingested}");
    }
    let f = episodic(Some(25), 100, now);
    assert_eq!(fading_since(&f), now - chrono::Duration::days(25));
    let f = episodic(None, 26, now);
    assert_eq!(fading_since(&f), f.ingested_at);
}

#[test]
fn semantic_consolidated_and_ended_facts_never_fade() {
    let now = dt("2026-06-01T00:00:00.000Z");
    for class in [MemoryClass::Semantic, MemoryClass::Consolidated] {
        let mut f = episodic(None, 400, now);
        f.memory_class = class;
        assert!(!is_fading(&f, now, &decay()));
    }
    let mut f = episodic(None, 400, now);
    f.valid_to = Some(now);
    assert!(!is_fading(&f, now, &decay()));
}
