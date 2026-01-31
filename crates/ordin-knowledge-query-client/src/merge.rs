//! `merge_hits`: reciprocal rank fusion and the ordering rule (KnowledgeQueryClient.md Internal
//! Logic M). Pure and deterministic, so the front door and the desktop order results identically.

use std::cmp::Ordering;
use std::collections::HashMap;

use ordin_types::wire::knowledge_query::{
    KnowledgeHit, KnowledgeHitKind, PageStatus, PageTier, SharedIdentity,
};
use sha2::{Digest, Sha256};

use crate::types::RRF_K;

/// The identity of a hit across lists: `(repository, path, commit, section, sha256(statement))`.
type HitKey = (String, String, String, Option<String>, [u8; 32]);

struct Scored {
    hit: KnowledgeHit,
    score: f64,
    last_list: usize,
}

/// Merges the ranked lists of several agents into one ranked list of at most `limit` hits.
///
/// 1. A hit that fails its field bounds is dropped (logged `kq_hit_dropped`, DEBUG).
/// 2. Each hit scores `sum 1/(60 + rank)` over the lists it appears in; the hit kept for a key is the
///    one from the earliest list in input order.
/// 3. When at least one hit is not `Deprecated`, every `Deprecated` hit is dropped.
/// 4. Sorted by score (six decimal places), then tier, then page before document, then
///    `(agent, repository, path, section)`.
/// 5. Hits sharing a `shared_identity` are made adjacent.
/// 6. Truncated to `limit`.
// spec: ordin-desktop/components/KnowledgeQueryClient#SA-KQC-07
pub fn merge_hits(lists: Vec<(String, Vec<KnowledgeHit>)>, limit: u8) -> Vec<KnowledgeHit> {
    let mut order: Vec<Scored> = Vec::new();
    let mut index: HashMap<HitKey, usize> = HashMap::new();
    for (list_no, (source, hits)) in lists.into_iter().enumerate() {
        for (position, hit) in hits.into_iter().enumerate() {
            if let Some(field) = out_of_bounds(&hit) {
                tracing::debug!(
                    component = "knowledge_query_client",
                    agent = source.as_str(),
                    field,
                    "kq_hit_dropped"
                );
                continue;
            }
            let share = 1.0 / (RRF_K + (position + 1) as f64);
            let key = key_of(&hit);
            match index.get(&key) {
                Some(&at) => {
                    let entry = &mut order[at];
                    if entry.last_list != list_no {
                        entry.score += share;
                        entry.last_list = list_no;
                    }
                }
                None => {
                    index.insert(key, order.len());
                    order.push(Scored {
                        hit,
                        score: share,
                        last_list: list_no,
                    });
                }
            }
        }
    }

    if order.iter().any(|s| s.hit.status != PageStatus::Deprecated) {
        order.retain(|s| s.hit.status != PageStatus::Deprecated);
    }

    order.sort_by(compare);
    let sorted: Vec<KnowledgeHit> = order.into_iter().map(|s| s.hit).collect();

    let mut placed = vec![false; sorted.len()];
    let mut sequence: Vec<usize> = Vec::with_capacity(sorted.len());
    for at in 0..sorted.len() {
        if placed[at] {
            continue;
        }
        placed[at] = true;
        sequence.push(at);
        if let Some(identity) = sorted[at].shared_identity.as_ref() {
            for later in (at + 1)..sorted.len() {
                if !placed[later] && same_identity(sorted[later].shared_identity.as_ref(), identity)
                {
                    placed[later] = true;
                    sequence.push(later);
                }
            }
        }
    }
    let mut slots: Vec<Option<KnowledgeHit>> = sorted.into_iter().map(Some).collect();
    sequence
        .into_iter()
        .take(usize::from(limit))
        .filter_map(|at| slots[at].take())
        .collect()
}

fn same_identity(other: Option<&SharedIdentity>, identity: &SharedIdentity) -> bool {
    other == Some(identity)
}

fn key_of(hit: &KnowledgeHit) -> HitKey {
    let digest: [u8; 32] = Sha256::digest(hit.statement.as_bytes()).into();
    (
        hit.repository.clone(),
        hit.path.clone(),
        hit.commit.clone(),
        hit.section.clone(),
        digest,
    )
}

/// The first field of a hit outside its bounds (Shared Context, `KnowledgeHit`), if any.
fn out_of_bounds(hit: &KnowledgeHit) -> Option<&'static str> {
    let statement = hit.statement.chars().count();
    if !(1..=4_000).contains(&statement) {
        return Some("statement");
    }
    let path = hit.path.chars().count();
    if !(1..=1_024).contains(&path) {
        return Some("path");
    }
    let hex = hit
        .commit
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !matches!(hit.commit.len(), 40 | 64) || !hex {
        return Some("commit");
    }
    if hit.things.len() > 16 {
        return Some("things");
    }
    None
}

fn micro(score: f64) -> i64 {
    (score * 1_000_000.0).round() as i64
}

fn tier_rank(tier: Option<PageTier>) -> u8 {
    match tier {
        Some(PageTier::HumanReviewed) => 3,
        Some(PageTier::MachineConfirmed) => 2,
        Some(PageTier::Unverified) => 1,
        None => 0,
    }
}

fn kind_rank(kind: KnowledgeHitKind) -> u8 {
    match kind {
        KnowledgeHitKind::Page => 0,
        KnowledgeHitKind::Document => 1,
    }
}

fn compare(a: &Scored, b: &Scored) -> Ordering {
    micro(b.score)
        .cmp(&micro(a.score))
        .then_with(|| tier_rank(b.hit.tier).cmp(&tier_rank(a.hit.tier)))
        .then_with(|| kind_rank(a.hit.kind).cmp(&kind_rank(b.hit.kind)))
        .then_with(|| a.hit.agent.cmp(&b.hit.agent))
        .then_with(|| a.hit.repository.cmp(&b.hit.repository))
        .then_with(|| a.hit.path.cmp(&b.hit.path))
        .then_with(|| a.hit.section.cmp(&b.hit.section))
}
