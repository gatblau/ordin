//! `merge_hits` (Internal Logic M), table-driven.

use ordin_types::wire::knowledge_query::{
    KnowledgeHit, KnowledgeHitKind, PageStatus, PageTier, SharedIdentity,
};

use super::harness::{hit, COMMIT};
use crate::merge_hits;

fn lists(one: Vec<KnowledgeHit>) -> Vec<(String, Vec<KnowledgeHit>)> {
    vec![("a".to_owned(), one)]
}

#[test]
fn rank_fusion_sums_the_shares_of_every_list_a_hit_appears_in() {
    let shared = hit("p1", "x.md", "shared fact");
    let only_first = hit("p1", "y.md", "only in the first list");
    let only_second = hit("p2", "z.md", "only in the second list");
    let merged = merge_hits(
        vec![
            ("p1".to_owned(), vec![only_first.clone(), shared.clone()]),
            ("p2".to_owned(), vec![shared.clone(), only_second.clone()]),
        ],
        20,
    );
    // shared: 1/62 + 1/61; only_first: 1/61; only_second: 1/62.
    let order: Vec<&str> = merged.iter().map(|h| h.statement.as_str()).collect();
    assert_eq!(
        order,
        [
            "shared fact",
            "only in the first list",
            "only in the second list"
        ]
    );
}

#[test]
fn the_kept_hit_is_the_one_from_the_earliest_list() {
    let mut first = hit("p1", "x.md", "same statement");
    first.agent = "from-first".to_owned();
    let mut second = first.clone();
    second.agent = "from-second".to_owned();
    let merged = merge_hits(
        vec![
            ("p1".to_owned(), vec![first]),
            ("p2".to_owned(), vec![second]),
        ],
        20,
    );
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].agent, "from-first");
}

#[test]
fn a_hit_repeated_inside_one_list_scores_once() {
    let one = hit("p1", "x.md", "twice");
    let other = hit("p1", "y.md", "other");
    let merged = merge_hits(lists(vec![one.clone(), other.clone(), one.clone()]), 20);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].statement, "twice");
}

#[test]
fn deprecated_hits_are_kept_when_nothing_else_matches() {
    let mut only = hit("p1", "x.md", "old");
    only.status = PageStatus::Deprecated;
    assert_eq!(merge_hits(lists(vec![only.clone()]), 20).len(), 1);
    let stable = hit("p1", "y.md", "current");
    let merged = merge_hits(lists(vec![only, stable]), 20);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].statement, "current");
}

#[test]
fn hits_outside_their_bounds_are_dropped() {
    let mut empty_statement = hit("p1", "a.md", "x");
    empty_statement.statement = String::new();
    let mut long_statement = hit("p1", "b.md", "x");
    long_statement.statement = "s".repeat(4_001);
    let mut empty_path = hit("p1", "c.md", "c");
    empty_path.path = String::new();
    let mut long_path = hit("p1", "d.md", "d");
    long_path.path = "p".repeat(1_025);
    let mut short_commit = hit("p1", "e.md", "e");
    short_commit.commit = "abc".to_owned();
    let mut upper_commit = hit("p1", "f.md", "f");
    upper_commit.commit = COMMIT.to_uppercase();
    let mut many_things = hit("p1", "g.md", "g");
    many_things.things = (0..17).map(|n| n.to_string()).collect();
    let mut sixty_four = hit("p1", "h.md", "h");
    sixty_four.commit = "a".repeat(64);
    let good = hit("p1", "i.md", "good");
    let merged = merge_hits(
        lists(vec![
            empty_statement,
            long_statement,
            empty_path,
            long_path,
            short_commit,
            upper_commit,
            many_things,
            sixty_four,
            good,
        ]),
        20,
    );
    let kept: Vec<&str> = merged.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(kept, ["h.md", "i.md"]);
}

#[test]
fn the_limit_truncates_the_result() {
    let hits: Vec<KnowledgeHit> = (0..5)
        .map(|n| hit("p1", &format!("{n}.md"), &format!("fact {n}")))
        .collect();
    assert_eq!(merge_hits(lists(hits.clone()), 3).len(), 3);
    assert_eq!(merge_hits(lists(hits), 0).len(), 0);
}

#[test]
fn equal_scores_order_by_tier_then_kind_then_names() {
    let mut a = hit("agent-b", "p.md", "one");
    a.tier = Some(PageTier::MachineConfirmed);
    let mut b = hit("agent-a", "p.md", "two");
    b.tier = Some(PageTier::MachineConfirmed);
    let mut c = hit("agent-a", "p.md", "three");
    c.tier = Some(PageTier::MachineConfirmed);
    c.kind = KnowledgeHitKind::Document;
    c.section = Some("S".to_owned());
    // Three single-hit lists: every hit has rank 1 and the same score.
    let merged = merge_hits(
        vec![
            ("1".to_owned(), vec![c.clone()]),
            ("2".to_owned(), vec![a.clone()]),
            ("3".to_owned(), vec![b.clone()]),
        ],
        20,
    );
    let order: Vec<&str> = merged.iter().map(|h| h.statement.as_str()).collect();
    // Pages before the document; between the two pages the agent name decides.
    assert_eq!(order, ["two", "one", "three"]);
}

#[test]
fn a_better_rank_beats_a_better_tier() {
    let mut low = hit("p1", "a.md", "high rank, low trust");
    low.tier = None;
    low.kind = KnowledgeHitKind::Document;
    let mut high = hit("p1", "b.md", "low rank, high trust");
    high.tier = Some(PageTier::HumanReviewed);
    let merged = merge_hits(lists(vec![low, high]), 20);
    assert_eq!(merged[0].statement, "high rank, low trust");
}

#[test]
fn hits_of_one_shared_identity_become_adjacent() {
    let identity = SharedIdentity {
        glossary: "git.example.org/glossary@main".to_owned(),
        concept_path: "suppliers/borealis-ltd.md".to_owned(),
    };
    let mut first = hit("p1", "a.md", "first about Borealis");
    first.shared_identity = Some(identity.clone());
    let middle = hit("p1", "b.md", "about something else");
    let other = hit("p1", "c.md", "yet another");
    let mut last = hit("p2", "d.md", "second about Borealis");
    last.shared_identity = Some(identity);
    let merged = merge_hits(
        vec![
            ("p1".to_owned(), vec![first, middle, other]),
            (
                "p2".to_owned(),
                vec![
                    hit("p2", "z.md", "p2 lead"),
                    hit("p2", "y.md", "p2 next"),
                    hit("p2", "x.md", "p2 third"),
                    last,
                ],
            ),
        ],
        20,
    );
    let at: Vec<usize> = merged
        .iter()
        .enumerate()
        .filter(|(_, h)| h.shared_identity.is_some())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(at.len(), 2);
    assert_eq!(at[1], at[0] + 1);
}
