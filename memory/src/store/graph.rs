//! Graph logic every storage applies identically (memory-store amendment January 2026, ADR-021): the
//! entity-page cursor, the neighbour walk's selection rule, and the bookkeeping of which facts the
//! graph and link runs reached and through what — so the SurrealDB store and the SQL storage differ
//! only in the statements they issue (ADR-101: one behaviour on every backend).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::text::normalise_name;
use crate::types::domain::{AliasFolder, Entity, Fact};
use crate::types::ports::{Neighbour, StoreError};

/// Collections a recall with no filter may have in play (SA-COLL-02).
pub(crate) const MAX_COLLECTIONS_IN_PLAY: usize = 32;
/// `list_entities` page bound (step 20d).
pub(crate) const ENTITY_PAGE_MAX: u32 = 100;
/// `entity_detail` bound (step 20e).
pub(crate) const ENTITY_DETAIL_MAX: u32 = 200;
/// `fact_versions` bound (step 20f).
pub(crate) const FACT_VERSIONS_MAX: u32 = 50;
/// `list_entities` search bound, in characters (step 20d).
pub(crate) const SEARCH_MAX_CHARS: usize = 200;
/// Matched entities a recall seeds the graph with (SA-GRAPH-10).
pub(crate) const MAX_SEEDS: u32 = 8;
/// Relationships a candidate names in `via` (step 9″.4).
pub(crate) const MAX_VIA: usize = 4;

/// The `list_entities` keyset: `(canonical_name, id)` of the page's last entity, base64url(JSON).
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct EntityCursor {
    pub n: String,
    pub id: String,
}

impl EntityCursor {
    pub(crate) fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).unwrap_or_default())
    }

    /// Decode a cursor `encode` produced; anything else is out of range ("range" selects
    /// `VAL_OUT_OF_RANGE`).
    pub(crate) fn decode(raw: &str) -> Result<Self, StoreError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(raw.as_bytes())
            .map_err(|_| StoreError::Validation("cursor out of range".into()))?;
        serde_json::from_slice(&bytes)
            .map_err(|_| StoreError::Validation("cursor out of range".into()))
    }
}

/// Refuse a `limit` outside `1..=max` ("range" selects `VAL_OUT_OF_RANGE`).
pub(crate) fn check_limit(limit: u32, max: u32, what: &str) -> Result<(), StoreError> {
    if limit == 0 || limit > max {
        return Err(StoreError::Validation(format!("{what} out of range")));
    }
    Ok(())
}

/// The normalised `search` of `list_entities`, refusing one outside 1..=200 characters.
pub(crate) fn normalised_search(search: Option<&str>) -> Result<Option<String>, StoreError> {
    match search {
        None => Ok(None),
        Some(s) => {
            let n = s.chars().count();
            if n == 0 || n > SEARCH_MAX_CHARS {
                return Err(StoreError::Validation("search out of range".into()));
            }
            Ok(Some(crate::text::normalise_name(s)))
        }
    }
}

/// Validate a relationship's graph fields (`put_relationship`, ADR-021): the normalised kind 1..=128
/// characters and `replaces` absent or `relationship:<uuid>` (`VAL_INVALID_BODY`).
pub(crate) fn validate_relationship_graph_fields(
    kind: &str,
    replaces: Option<&str>,
) -> Result<(), StoreError> {
    let k = crate::text::normalise_kind(kind);
    if k.is_empty() || k.chars().count() > 128 {
        return Err(StoreError::Validation("kind length".into()));
    }
    if let Some(r) = replaces {
        let ok = r
            .strip_prefix("relationship:")
            .is_some_and(|u| uuid::Uuid::parse_str(u).is_ok());
        if !ok {
            return Err(StoreError::Validation("replaces".into()));
        }
    }
    Ok(())
}

/// What an entity's erasure removes besides facts and links: ids of queued jobs, of dead-lettered
/// jobs and of held records.
#[derive(Default)]
pub(crate) struct PendingWrites {
    pub jobs: Vec<String>,
    pub dead: Vec<String>,
    pub held: Vec<String>,
}

impl PendingWrites {
    /// Every id, for the proof's digest.
    pub(crate) fn all(&self) -> impl Iterator<Item = String> + '_ {
        self.jobs
            .iter()
            .chain(&self.dead)
            .chain(&self.held)
            .cloned()
    }
}

/// Refuse a recall with no collection filter and more collections in play than the cap: dropping some
/// to fit would hide facts without saying so.
// spec: components/memory-store#SA-COLL-02
pub(crate) fn refuse_too_many_collections(
    filtered: bool,
    in_play: usize,
) -> Result<(), StoreError> {
    if !filtered && in_play > MAX_COLLECTIONS_IN_PLAY {
        return Err(StoreError::TooManyCollections {
            in_play: u32::try_from(in_play).unwrap_or(u32::MAX),
        });
    }
    Ok(())
}

/// `keep` with `merge`'s canonical name and aliases folded in (step 19d). A name only private facts carry
/// stays private on `keep` (`merge_shared`: some fact naming `merge` is not private).
pub(crate) fn fold_aliases(keep: &Entity, merge: &Entity, merge_shared: bool) -> Entity {
    let mut out = keep.clone();
    let had: HashSet<String> = keep.aliases.iter().map(|a| normalise_name(a)).collect();
    // `merge`'s canonical name was stated in its own folder, and has no record of its own.
    let canonical = AliasFolder {
        alias: normalise_name(&merge.canonical_name),
        team: merge.owner.team.clone(),
        collection: merge.collection.clone(),
    };
    for f in merge.alias_folders.iter().chain([&canonical]) {
        let known = keep.alias_folders.iter().any(|k| k.alias == f.alias);
        if (known || !had.contains(&f.alias)) && !out.alias_folders.contains(f) {
            out.alias_folders.push(f.clone());
        }
    }
    for a in merge
        .aliases
        .iter()
        .chain(std::iter::once(&merge.canonical_name))
    {
        if !out.aliases.contains(a) && a != &keep.canonical_name {
            out.aliases.push(a.clone());
            if !merge_shared || merge.private_aliases.contains(a) {
                out.private_aliases.push(a.clone());
            }
        }
    }
    out
}

/// The caller's ids of `facts`' entities and sources, each once: what `hard_delete` may also remove.
pub(crate) fn delete_candidates(facts: &[Fact]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut owners: BTreeSet<&str> = BTreeSet::new();
    let mut entities: BTreeSet<&str> = BTreeSet::new();
    let mut sources: BTreeSet<&str> = BTreeSet::new();
    for f in facts {
        owners.insert(&f.owner.user);
        entities.extend(f.entities.iter().map(String::as_str));
        sources.extend(f.source_id.as_deref());
    }
    let list = |s: BTreeSet<&str>| s.into_iter().map(str::to_string).collect();
    (list(owners), list(entities), list(sources))
}

/// Take one team's folder off `entity`'s aliases, or all of its folders when `collection` is `None`,
/// removing each alias no other folder stated. Returns whether anything changed.
pub(crate) fn drop_alias_folder(entity: &mut Entity, team: &str, collection: Option<&str>) -> bool {
    let of_folder = |f: &AliasFolder| {
        f.team.as_deref() == Some(team) && collection.is_none_or(|c| f.collection == c)
    };
    let mut gone: HashSet<String> = entity
        .alias_folders
        .iter()
        .filter(|f| of_folder(f))
        .map(|f| f.alias.clone())
        .collect();
    if gone.is_empty() {
        return false;
    }
    entity.alias_folders.retain(|f| !of_folder(f));
    for f in &entity.alias_folders {
        gone.remove(&f.alias);
    }
    entity
        .aliases
        .retain(|a| !gone.contains(&normalise_name(a)));
    entity
        .private_aliases
        .retain(|a| !gone.contains(&normalise_name(a)));
    true
}

/// Refuse a merge that would publish `keep`'s name: shared facts naming `merge` move to `keep`, and a
/// reader of them sees its canonical name. The merge the other way keeps the private name private.
// The fold guards the names that move to `keep`; this guards the name `keep` already has.
pub(crate) fn refuse_name_exposure(
    keep_private: bool,
    keep_shared: bool,
    merge_shared: bool,
) -> Result<(), StoreError> {
    if merge_shared && keep_private && !keep_shared {
        return Err(StoreError::ScopeForbidden);
    }
    Ok(())
}

/// One relationship a neighbour hop considers, after the read filter and validity.
pub(crate) struct HopEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub confidence: f64,
    pub replaces: Option<String>,
}

impl HopEdge {
    /// The order a hop follows edges in: confidence descending, then id.
    pub(crate) fn rank(a: &Self, b: &Self) -> std::cmp::Ordering {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.id.cmp(&b.id))
    }
}

/// Steps 2–3 of `neighbours` for one hop: of `edges` (touching `frontier`), keep those SA-GRAPH-05 lets
/// the caller follow — a readable, valid asserting fact (`asserted_readable`), or no asserting fact at
/// all and `replaces` set (`has_asserted` says which have any asserting row) — order them by confidence
/// descending then id, and take at most `fanout` distinct other endpoints not yet in `seen`. Returns
/// `(endpoint, via)` in that order.
// spec: components/memory-store#SA-GRAPH-05
// A team relationship imported with no asserting fact has no `replaces`, so it is stored, listed and
// exported, but not followed.
pub(crate) fn pick_hop(
    frontier: &HashSet<String>,
    mut edges: Vec<HopEdge>,
    has_asserted: &HashSet<String>,
    asserted_readable: &HashSet<String>,
    seen: &HashSet<String>,
    fanout: u32,
) -> Vec<(String, String)> {
    edges.retain(|e| {
        asserted_readable.contains(&e.id) || (!has_asserted.contains(&e.id) && e.replaces.is_some())
    });
    edges.sort_by(HopEdge::rank);
    let mut out: Vec<(String, String)> = Vec::new();
    let mut taken: HashSet<String> = HashSet::new();
    for e in edges {
        if out.len() as u32 >= fanout {
            break;
        }
        for (here, there) in [(&e.from, &e.to), (&e.to, &e.from)] {
            if out.len() as u32 >= fanout {
                break;
            }
            if frontier.contains(here) && !seen.contains(there) && taken.insert(there.clone()) {
                out.push((there.clone(), e.id.clone()));
            }
        }
    }
    out
}

/// What the graph and link runs of one recall reached: each entity's lowest hop and one relationship
/// that reached it (from `neighbours`), and the seeds (hop 0).
pub(crate) struct Reach {
    pub seeds: Vec<String>,
    /// Reached entity → (hop, via), seeds excluded.
    pub reached: HashMap<String, (u8, String)>,
}

impl Reach {
    pub(crate) fn new(seeds: Vec<String>, neighbours: Vec<Neighbour>) -> Self {
        let seed_set: HashSet<&String> = seeds.iter().collect();
        let mut reached = HashMap::new();
        for n in neighbours {
            if seed_set.contains(&n.entity_id) {
                continue;
            }
            reached
                .entry(n.entity_id)
                .and_modify(|cur: &mut (u8, String)| {
                    if n.hop < cur.0 {
                        *cur = (n.hop, n.via.clone());
                    }
                })
                .or_insert((n.hop, n.via));
        }
        Self { seeds, reached }
    }

    /// The reached entities at exactly `hop`, sorted for a stable statement.
    pub(crate) fn at_hop(&self, hop: u8) -> Vec<String> {
        let mut v: Vec<String> = self
            .reached
            .iter()
            .filter(|(_, (h, _))| *h == hop)
            .map(|(e, _)| e.clone())
            .collect();
        v.sort();
        v
    }

    /// The relationships that reached a fact naming `entities` at link hop ≥ 1, at most 4, in the
    /// order of the hop then the entity id.
    pub(crate) fn via_of(&self, entities: &[String]) -> Vec<String> {
        let mut hits: BTreeMap<(u8, &String), &String> = BTreeMap::new();
        for e in entities {
            if let Some((h, via)) = self.reached.get(e) {
                hits.insert((*h, e), via);
            }
        }
        let mut out: Vec<String> = Vec::new();
        for via in hits.into_values() {
            if !out.contains(via) {
                out.push(via.clone());
            }
            if out.len() == MAX_VIA {
                break;
            }
        }
        out
    }
}

/// The graph bookkeeping a recall carries to its candidates: each fact's lowest hop and its `via`.
#[derive(Default)]
pub(crate) struct Reached {
    pub hop: HashMap<String, u8>,
    pub via: HashMap<String, Vec<String>>,
}

impl Reached {
    /// Record that `fact` was reached at `hop` (0 for the graph run) through `via`.
    pub(crate) fn note(&mut self, fact: &str, hop: u8, via: Vec<String>) {
        let h = self.hop.entry(fact.to_string()).or_insert(hop);
        if hop < *h {
            *h = hop;
        }
        if !via.is_empty() {
            let v = self.via.entry(fact.to_string()).or_default();
            for r in via {
                if v.len() < MAX_VIA && !v.contains(&r) {
                    v.push(r);
                }
            }
        }
    }
}

/// The SHA-256 digest an entity erasure proof carries: over the newline-joined sorted removed ids.
pub(crate) fn digest_ids(mut ids: Vec<String>) -> String {
    use sha2::{Digest, Sha256};
    ids.sort();
    ids.dedup();
    let digest = Sha256::digest(ids.join("\n").as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The deterministic key of one `relationship_fact` row, so `link_fact` is an upsert and a replay
/// reaches the same row.
#[cfg_attr(not(feature = "surreal"), allow(dead_code))]
pub(crate) fn link_row_key(relationship_id: &str, fact_id: &str, role: &str) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("{relationship_id}\n{fact_id}\n{role}").as_bytes(),
    )
    .to_string()
}

/// Step 20f over any `MemoryStore`: follow `supersedes` back and `superseded_by` forward from the fact,
/// each hop read-filtered, stopping at `limit` or a missing or unreadable link; oldest first.
pub(crate) async fn versions_through<S: crate::types::ports::MemoryStore + ?Sized>(
    store: &S,
    ctx: &crate::types::scope::ScopeContext,
    fact_id: &str,
    limit: u32,
) -> Result<Vec<crate::types::domain::Fact>, StoreError> {
    let Some(start) = store.get_fact(ctx, fact_id).await? else {
        return Ok(Vec::new());
    };
    let limit = limit as usize;
    let mut seen: HashSet<String> = HashSet::from([start.id.clone()]);
    let mut back: Vec<crate::types::domain::Fact> = Vec::new();
    let mut cur = start.supersedes.clone();
    while let Some(id) = cur {
        if back.len() + 1 >= limit || !seen.insert(id.clone()) {
            break;
        }
        match store.get_fact(ctx, &id).await? {
            Some(f) => {
                cur = f.supersedes.clone();
                back.push(f);
            }
            None => break,
        }
    }
    back.reverse();
    let mut fwd_from = start.superseded_by.clone();
    back.push(start);
    while let Some(id) = fwd_from {
        if back.len() >= limit || !seen.insert(id.clone()) {
            break;
        }
        match store.get_fact(ctx, &id).await? {
            Some(f) => {
                fwd_from = f.superseded_by.clone();
                back.push(f);
            }
            None => break,
        }
    }
    Ok(back)
}

/// Steps 19d′ for `merge_entities`: given the relationships touching keep or merge before the
/// repoint, the ids to delete (self-loops and the later of two identical live links) and the
/// `relationship_fact` rows to move `(from_rel, to_rel)`.
// Links fold within one owner, so a caller's facts never move onto a link it cannot read; two owners'
// rows stay two.
pub(crate) fn merge_plan(
    rels: &[crate::types::domain::Relationship],
    keep_id: &str,
    merge_id: &str,
) -> (Vec<String>, Vec<(String, String)>) {
    let point = |e: &str| {
        if e == merge_id {
            keep_id.to_string()
        } else {
            e.to_string()
        }
    };
    let mut drop: Vec<String> = Vec::new();
    let mut moves: Vec<(String, String)> = Vec::new();
    let mut live: BTreeMap<_, Vec<&crate::types::domain::Relationship>> = BTreeMap::new();
    for r in rels {
        let (f, t) = (point(&r.from), point(&r.to));
        if f == t {
            drop.push(r.id.clone());
            continue;
        }
        if r.valid_to.is_none() {
            live.entry((r.owner.user.clone(), f, r.kind.clone(), t))
                .or_default()
                .push(r);
        }
    }
    for (_, mut group) in live {
        if group.len() < 2 {
            continue;
        }
        group.sort_by(|a, b| {
            a.valid_from
                .cmp(&b.valid_from)
                .then_with(|| a.id.cmp(&b.id))
        });
        let keeper = group[0].id.clone();
        for other in &group[1..] {
            moves.push((other.id.clone(), keeper.clone()));
            drop.push(other.id.clone());
        }
    }
    (drop, moves)
}

/// Refuse a merge whose plan would delete a relationship the caller does not own (M-1). The repoint
/// only changes references, so another user's facts and links may follow it; a deletion may not.
// A self-loop or duplicate of another user refuses the whole merge with `ScopeForbidden`, before a
// change; maintenance scope passes.
pub(crate) fn refuse_merge_of_others(
    ctx: &crate::types::scope::ScopeContext,
    rels: &[crate::types::domain::Relationship],
    drop: &[String],
) -> Result<(), StoreError> {
    rels.iter()
        .filter(|r| drop.contains(&r.id))
        .try_for_each(|r| crate::store::owner_gate(ctx, &r.owner.user))
}

// --- Triples and conflicts (memory-store amendment ADR-022) -------------------------------------------

/// `live_same_subject_predicate` bound (step 20t).
pub(crate) const SAME_SUBJECT_PREDICATE_MAX: u32 = 64;
/// `conflicts_of` bound (step 20v).
pub(crate) const CONFLICT_IDS_MAX: usize = 100;

/// Step 7″'s checks on a fact's `TripleKey` before it is written: a set predicate needs a subject
/// entity, and an entity object needs its `object_entity` (`VAL_INVALID_BODY`, "triple").
// spec: components/memory-store#SA-TRIPLE-01
pub(crate) fn check_triple_key(f: &Fact) -> Result<(), StoreError> {
    let Some(k) = &f.triple_key else {
        return Ok(());
    };
    let bad = k.subject_entity.is_empty()
        || k.predicate.is_empty()
        || (k.object_kind == crate::types::domain::ObjectKind::Entity
            && k.object_entity.as_deref().is_none_or(str::is_empty));
    if bad {
        return Err(StoreError::Validation("triple".into()));
    }
    Ok(())
}

/// Take a stored row's triple fields out of it and build the fact's `TripleKey`; `None` when the row
/// has no predicate (a passage, an earlier fact). Nulls of the two flags are dropped so the row
/// decodes with the flags' defaults.
#[cfg(feature = "surreal")]
pub(crate) fn take_triple_key(
    obj: &mut serde_json::Map<String, serde_json::Value>,
) -> Option<crate::types::domain::TripleKey> {
    let mut take = |k: &str| match obj.remove(k) {
        Some(serde_json::Value::String(s)) => Some(s),
        _ => None,
    };
    let subject_entity = take("subject_entity");
    let predicate = take("predicate");
    let object_kind = take("object_kind");
    let object_value = take("object_value");
    let object_unit = take("object_unit");
    let object_entity = take("object_entity");
    let author = take("author");
    for flag in ["single_valued", "changed"] {
        if obj.get(flag).is_some_and(|v| !v.is_boolean()) {
            obj.remove(flag);
        }
    }
    let predicate = predicate?;
    Some(crate::types::domain::TripleKey {
        subject_entity: subject_entity.unwrap_or_default(),
        predicate,
        object_kind: object_kind
            .as_deref()
            .and_then(crate::types::domain::ObjectKind::parse)
            .unwrap_or_default(),
        object_value: object_value.unwrap_or_default(),
        object_unit,
        object_entity,
        author,
    })
}

/// Step 20u's pair, ordered so that `fact_a < fact_b` in byte order; the same fact twice is refused.
// spec: components/memory-store#SA-TRIPLE-08
pub(crate) fn conflict_pair<'a>(a: &'a str, b: &'a str) -> Result<(&'a str, &'a str), StoreError> {
    if a == b {
        return Err(StoreError::Validation("conflict".into()));
    }
    Ok(if a.as_bytes() < b.as_bytes() {
        (a, b)
    } else {
        (b, a)
    })
}

/// Whether two facts have the same owner (tenant, team, user): a conflict never crosses owners.
pub(crate) fn same_owner(a: &Fact, b: &Fact) -> bool {
    a.owner.tenant == b.owner.tenant && a.owner.team == b.owner.team && a.owner.user == b.owner.user
}

/// Step 20v's bound on `ids`.
pub(crate) fn check_conflict_ids(ids: &[String]) -> Result<(), StoreError> {
    if ids.is_empty() || ids.len() > CONFLICT_IDS_MAX {
        return Err(StoreError::Validation("ids out of range".into()));
    }
    Ok(())
}

/// Step 20v's result from the stored pairs `(fact_a, fact_b, recorded_at)` and the readable live
/// facts: one item per requested id on a row whose other fact is readable and live, ordered by
/// `fact`, then `with`.
// spec: phase-1-analysis#SA-TRIPLE-13
pub(crate) fn conflicts_for(
    ids: &[String],
    rows: &[(String, String, chrono::DateTime<chrono::Utc>)],
    readable_live: &HashSet<String>,
) -> Vec<crate::types::ports::FactConflict> {
    let asked: HashSet<&String> = ids.iter().collect();
    let mut out: BTreeMap<(String, String), chrono::DateTime<chrono::Utc>> = BTreeMap::new();
    for (a, b, at) in rows {
        for (fact, with) in [(a, b), (b, a)] {
            if asked.contains(fact) && readable_live.contains(with) {
                out.insert((fact.clone(), with.clone()), *at);
            }
        }
    }
    out.into_iter()
        .map(
            |((fact, with), recorded_at)| crate::types::ports::FactConflict {
                fact,
                with,
                recorded_at,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(id: &str, from: &str, to: &str, c: f64) -> HopEdge {
        HopEdge {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            confidence: c,
            replaces: None,
        }
    }

    #[test]
    fn a_hop_follows_readable_links_by_confidence_up_to_the_fanout() {
        let frontier: HashSet<String> = ["a".to_string()].into();
        let readable: HashSet<String> = ["r1", "r2", "r3"].iter().map(|s| s.to_string()).collect();
        let edges = vec![
            edge("r1", "a", "b", 0.5),
            edge("r2", "c", "a", 0.9),
            edge("r3", "a", "d", 0.7),
            edge("r4", "a", "e", 1.0),
        ];
        let got = pick_hop(&frontier, edges, &readable, &readable, &HashSet::new(), 2);
        assert_eq!(
            got,
            vec![("c".to_string(), "r2".into()), ("d".into(), "r3".into())]
        );
    }

    #[test]
    fn a_persons_link_with_no_asserting_fact_is_followed() {
        let frontier: HashSet<String> = ["a".to_string()].into();
        let mut e = edge("r1", "a", "b", 1.0);
        e.replaces = Some("relationship:x".into());
        let got = pick_hop(
            &frontier,
            vec![e, edge("r2", "a", "c", 1.0)],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            8,
        );
        assert_eq!(got, vec![("b".to_string(), "r1".into())]);
    }

    #[test]
    fn reach_keeps_the_lowest_hop_and_via_is_bounded() {
        let r = Reach::new(
            vec!["s".into()],
            vec![
                Neighbour {
                    entity_id: "x".into(),
                    hop: 2,
                    via: "r2".into(),
                },
                Neighbour {
                    entity_id: "x".into(),
                    hop: 1,
                    via: "r1".into(),
                },
                Neighbour {
                    entity_id: "s".into(),
                    hop: 1,
                    via: "r0".into(),
                },
            ],
        );
        assert_eq!(r.at_hop(1), vec!["x".to_string()]);
        assert!(r.at_hop(2).is_empty());
        assert_eq!(r.via_of(&["x".into(), "s".into()]), vec!["r1".to_string()]);
    }

    #[test]
    fn the_entity_cursor_round_trips_and_garbage_is_out_of_range() {
        let c = EntityCursor {
            n: "borealis".into(),
            id: "entity:1".into(),
        };
        assert_eq!(EntityCursor::decode(&c.encode()).unwrap(), c);
        assert!(EntityCursor::decode("not base64!")
            .unwrap_err()
            .to_string()
            .contains("range"));
    }

    #[test]
    fn replaces_must_be_a_relationship_uuid() {
        assert!(validate_relationship_graph_fields("supplies", None).is_ok());
        assert!(validate_relationship_graph_fields(
            "supplies",
            Some("relationship:0192e7a0-0000-7000-8000-000000000001")
        )
        .is_ok());
        assert!(validate_relationship_graph_fields("supplies", Some("fact:x")).is_err());
        assert!(validate_relationship_graph_fields("   ", None).is_err());
    }
}
