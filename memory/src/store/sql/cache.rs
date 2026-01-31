//! The per-scope in-memory search cache of the SQL storage (ordin change M-8 g; ordin SA-READ-10): for
//! every fact of a tenant, the columns the stage-1 filters read and the decoded embedding, grouped by
//! collection, so a vector run and the collections-in-play step (9′.1) are answered from memory without
//! reading a row.
//!
//! The cache is coherent only while the `SqlStorage` that owns it is the one writer of its tables — the
//! embedded deployment it exists for. A write by anything else (another process, another
//! `SqlStorage` over the same database, a hand-run statement) is not seen until the tenant is reloaded.
//! Every write path of the storage that changes a filter column or an embedding of a fact, or removes a
//! fact, calls [`SearchCache::forget`] or [`SearchCache::forget_tenant`] after it commits.
// The scope is the tenant (the SQL storage's equivalent of a SurrealDB namespace); an entry is
// reloaded after every write that sets, clears or removes an embedding or changes a filter column
// (`put_fact`, `set_fact_embedding`, `end_validity`,
// `supersede`, `keep_fact`, `hard_delete`, `erase_entity`) and a tenant's entries are dropped by
// `erase_tenant`, `erase_scope` and `erase_collection`; the cache is unbounded, its size at the
// 100,000-fact soft limit being ordin's R-108. It is coherent only while this `SqlStorage` is the one
// writer of its tables, which is the embedded deployment it exists for.
// C-91 (3 × 20,000 facts, 4 sub-queries, p95 ≤ 200 ms) is not met while every vector run
// reads its collection's rows to apply the read filter, so the cache holds, beside each vector, the
// columns the stage-1 filters read — owner user, owner team, visibility, memory class, validity
// interval, collection — for every fact of the tenant, with or without an embedding. A tenant is read
// whole on its first recall; afterwards only the facts a write named since are re-read, by id, before
// the next recall (the whole tenant instead once they exceed both 2,000 and a quarter of it). The
// entity filter (`filters.entity`) is not cached: its fact ids are read from `mem_fact_entity` once
// per recall, the junction rows `merge_entities` rewrites. Timestamps are compared at the precision the dialect stores
// (milliseconds in SQLite, microseconds in PostgreSQL), as the SQL predicate compared them.
// The graph and link runs of step 9″ have no budget they can meet while each one reads and
// sorts every fact naming a popular entity (ordin C-93, 40 ms p95), and a statement cannot both walk the
// newest facts and stay bounded when the seeds belong to another collection. So the cache also holds
// each fact's `ingested_at` and `entities`, and those runs scan it as a vector run does, over the same
// snapshot; `mem_fact.entities` and the `mem_fact_entity` rows name the same entities on every write
// path. They inherit the single-writer rule above; `merge_entities` joins the invalidating writes.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};

use crate::types::domain::{MemoryClass, Visibility};
use crate::types::ports::StoreError;

/// A cached fact's embedding.
#[derive(Clone)]
pub(super) enum Embedding {
    /// `embedding IS NULL`.
    Absent,
    /// A decoded embedding of `MEMORY_EMBED_DIM` values and its squared norm, summed in index order
    /// exactly as the cosine of the exact scan sums it.
    Vector { values: Arc<[f32]>, norm_sq: f64 },
    /// A stored embedding of this many values, not `MEMORY_EMBED_DIM`: a vector run whose filter
    /// admits the fact fails, as decoding it did before the cache held it.
    WrongDim(usize),
}

/// The filter columns and embedding of one fact.
#[derive(Clone)]
pub(super) struct CachedFact {
    pub owner_user: Arc<str>,
    pub owner_team: Option<Arc<str>>,
    pub visibility: Visibility,
    pub memory_class: MemoryClass,
    /// At the dialect's stored precision.
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    /// At the dialect's stored precision.
    pub ingested_at: DateTime<Utc>,
    /// The entities the fact names, in stored order.
    pub entities: Arc<[Arc<str>]>,
    pub embedding: Embedding,
}

/// One fact as a load read it.
pub(super) struct LoadedFact {
    pub id: String,
    pub collection: String,
    pub fact: CachedFact,
}

/// One collection's facts, by fact id. Shared with in-flight recalls; a refresh copies it only while a
/// recall still holds it.
pub(super) type Slab = Arc<HashMap<Arc<str>, CachedFact>>;

/// A tenant's facts by collection, in label (byte) order.
pub(super) type Snapshot = BTreeMap<String, Slab>;

/// The predicates of step 9′.3 a vector run and the collections step apply, as SQL applied them:
/// [`super::search_read_filter`], [`super::validity_clause`] and [`super::meta_clause`].
pub(super) struct SearchFilter {
    pub user: String,
    pub teams: HashSet<String>,
    /// `filters.valid_at` at the dialect's stored precision.
    pub valid_at: Option<DateTime<Utc>>,
    pub memory_class: Option<MemoryClass>,
    pub visibility: Option<Visibility>,
    /// The fact ids naming `filters.entity`, when it is set.
    pub entity_facts: Option<HashSet<String>>,
}

impl SearchFilter {
    /// Whether the fact `id` passes the read filter (without the grant arm), the validity predicate
    /// and the metadata predicates. The tenant predicate is the cache key.
    pub fn admits(&self, id: &str, f: &CachedFact) -> bool {
        let in_scope = *f.owner_user == *self.user
            || (f.visibility == Visibility::TeamShared
                && f.owner_team
                    .as_deref()
                    .is_some_and(|t| self.teams.contains(t)))
            || f.visibility == Visibility::TenantShared;
        if !in_scope {
            return false;
        }
        let valid = match self.valid_at {
            Some(at) => f.valid_from <= at && f.valid_to.is_none_or(|to| to > at),
            None => f.valid_to.is_none(),
        };
        valid
            && self.memory_class.is_none_or(|mc| f.memory_class == mc)
            && self.visibility.is_none_or(|v| f.visibility == v)
            && self
                .entity_facts
                .as_ref()
                .is_none_or(|ids| ids.contains(id))
    }
}

/// The collections of `snapshot` holding at least one fact `filter` admits, in label order (step 9′.1).
pub(super) fn collections_in_play(snapshot: &Snapshot, filter: &SearchFilter) -> Vec<String> {
    snapshot
        .iter()
        .filter(|(_, slab)| slab.iter().any(|(id, f)| filter.admits(id, f)))
        .map(|(c, _)| c.clone())
        .collect()
}

/// One vector run over `slab`: every fact `filter` admits that has an embedding, scored exactly by
/// cosine against `query` (whose squared norm is `query_norm_sq`), clamped to [0, 1], best first with
/// ties by fact id, at most `k`. The cosine is the exact scan's: `dot / (√‖q‖² · √‖v‖²)`, each sum taken
/// in index order in `f64`, 0 when either norm is 0 — the same value, bit for bit, the scan computed.
pub(super) fn vector_run(
    slab: &HashMap<Arc<str>, CachedFact>,
    filter: &SearchFilter,
    query: &[f32],
    query_norm_sq: f64,
    embed_dim: u32,
    k: usize,
) -> Result<Vec<(String, f64)>, StoreError> {
    let mut scored: Vec<(&Arc<str>, f64)> = Vec::new();
    for (id, f) in slab {
        let values = match &f.embedding {
            Embedding::Absent => continue,
            Embedding::WrongDim(n) => {
                if filter.admits(id, f) {
                    return Err(StoreError::Internal(format!(
                        "stored embedding dimension {n} != MEMORY_EMBED_DIM {embed_dim}"
                    )));
                }
                continue;
            }
            Embedding::Vector { values, norm_sq } => (values, *norm_sq),
        };
        if !filter.admits(id, f) {
            continue;
        }
        let (v, norm_sq) = values;
        let mut dot = 0.0f64;
        for (x, y) in query.iter().zip(v.iter()) {
            dot += f64::from(*x) * f64::from(*y);
        }
        let sim = if query_norm_sq == 0.0 || norm_sq == 0.0 {
            0.0
        } else {
            dot / (query_norm_sq.sqrt() * norm_sq.sqrt())
        };
        scored.push((id, sim.clamp(0.0, 1.0)));
    }
    let k = k.max(1);
    let order =
        |a: &(&Arc<str>, f64), b: &(&Arc<str>, f64)| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0));
    // The order is total (ids are unique), so selecting the best `k` before sorting keeps exactly the
    // facts, and the order, a full sort and cut would.
    if scored.len() > k {
        scored.select_nth_unstable_by(k - 1, order);
        scored.truncate(k);
    }
    scored.sort_by(order);
    Ok(scored
        .into_iter()
        .map(|(id, s)| (id.to_string(), s))
        .collect())
}

/// One graph or link run of step 9″ over `slab`: every fact `filter` admits naming any of `entities`,
/// newest `ingested_at` first, ties by fact id, at most `k`, each with its entities.
pub(super) fn entity_run(
    slab: &HashMap<Arc<str>, CachedFact>,
    filter: &SearchFilter,
    entities: &HashSet<&str>,
    k: usize,
) -> Vec<(String, Vec<String>)> {
    let mut hits: Vec<(&Arc<str>, &CachedFact)> = slab
        .iter()
        .filter(|(id, f)| {
            f.entities.iter().any(|e| entities.contains(&**e)) && filter.admits(id, f)
        })
        .collect();
    let k = k.max(1);
    let order = |a: &(&Arc<str>, &CachedFact), b: &(&Arc<str>, &CachedFact)| {
        b.1.ingested_at
            .cmp(&a.1.ingested_at)
            .then_with(|| a.0.cmp(b.0))
    };
    // As in `vector_run`: the order is total, so selecting first keeps what a full sort would.
    if hits.len() > k {
        hits.select_nth_unstable_by(k - 1, order);
        hits.truncate(k);
    }
    hits.sort_by(order);
    hits.into_iter()
        .map(|(id, f)| {
            let names = f.entities.iter().map(|e| e.to_string()).collect();
            (id.to_string(), names)
        })
        .collect()
}

/// The squared norm of `v`, summed in index order in `f64` (the exact scan's `‖v‖²`).
pub(super) fn norm_sq(v: &[f32]) -> f64 {
    let mut n = 0.0f64;
    for x in v {
        let x = f64::from(*x);
        n += x * x;
    }
    n
}

/// What a recall must do before it can read a tenant's facts from the cache.
pub(super) enum Plan {
    /// The cache is current: read this snapshot.
    Fresh(Snapshot),
    /// Read the tenant's facts — every one when `ids` is `None`, else those ids — then
    /// [`SearchCache::install`] them with this ticket.
    Load {
        ids: Option<Vec<String>>,
        ticket: Ticket,
    },
}

/// A load's claim on a tenant entry: where the entry stood when the load began. Dropping it without
/// installing (a failed or cancelled load) releases the claim.
pub(super) struct Ticket {
    cache: Arc<SearchCache>,
    tenant: String,
    epoch: u64,
    /// The write sequence the load began after: every write numbered at or below it committed before
    /// the load read anything.
    seq: u64,
    full: bool,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if self.full {
            let mut guard = self.cache.write();
            if let Some(t) = guard.get_mut(&self.tenant) {
                t.loading = t.loading.saturating_sub(1);
            }
        }
    }
}

/// One tenant's entry.
#[derive(Default)]
struct TenantFacts {
    /// Whether `collections` holds every fact of the tenant (apart from those in `dirty`).
    loaded: bool,
    /// Bumped by [`SearchCache::forget_tenant`]: a load that began before it installs nothing.
    epoch: u64,
    /// Full loads in flight. While one is, or once loaded, writes are recorded in `dirty`.
    loading: u32,
    collections: BTreeMap<String, Slab>,
    /// Fact id → its collection, for moving or removing a fact a write named.
    collection_of: HashMap<Arc<str>, String>,
    /// Facts written since they were last read, with the write's sequence number.
    dirty: HashMap<String, u64>,
    /// The last write sequence number handed out.
    seq: u64,
    /// The sequence the installed facts are current as of; a full load that began earlier would
    /// overwrite newer facts and is refused.
    installed_seq: u64,
}

impl TenantFacts {
    fn fact_count(&self) -> usize {
        self.collection_of.len()
    }

    fn remove(&mut self, id: &str) {
        if let Some(c) = self.collection_of.remove(id) {
            if let Some(slab) = self.collections.get_mut(&c) {
                Arc::make_mut(slab).remove(id);
                if slab.is_empty() {
                    self.collections.remove(&c);
                }
            }
        }
    }

    fn insert(&mut self, row: LoadedFact) {
        let id: Arc<str> = Arc::from(row.id);
        self.remove(&id);
        let slab = self.collections.entry(row.collection.clone()).or_default();
        Arc::make_mut(slab).insert(id.clone(), row.fact);
        self.collection_of.insert(id, row.collection);
    }
}

/// Up to this many written facts are always re-read by id; above it, and above a quarter of the
/// tenant, the tenant is read whole instead.
const PARTIAL_RELOAD_MAX: usize = 2_000;

/// How many times a recall retries a load another load or an erasure overtook, before it reads the
/// tenant once more and uses that read without caching it.
const LOAD_ATTEMPTS: usize = 4;

/// The cache: tenant → its facts.
#[derive(Default)]
pub(super) struct SearchCache {
    by_tenant: RwLock<HashMap<String, TenantFacts>>,
}

impl SearchCache {
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, TenantFacts>> {
        self.by_tenant.write().unwrap_or_else(|p| p.into_inner())
    }

    /// What a recall over `tenant` must do first.
    pub fn plan(self: &Arc<Self>, tenant: &str) -> Plan {
        {
            let guard = self.by_tenant.read().unwrap_or_else(|p| p.into_inner());
            if let Some(t) = guard.get(tenant) {
                if t.loaded && t.dirty.is_empty() {
                    return Plan::Fresh(t.collections.clone());
                }
            }
        }
        let mut guard = self.write();
        let t = guard.entry(tenant.to_string()).or_default();
        if t.loaded && t.dirty.is_empty() {
            return Plan::Fresh(t.collections.clone());
        }
        let full = !t.loaded || t.dirty.len() > (t.fact_count() / 4).max(PARTIAL_RELOAD_MAX);
        if full {
            t.loading += 1;
        }
        let ids = (!full).then(|| t.dirty.keys().cloned().collect());
        Plan::Load {
            ids,
            ticket: Ticket {
                cache: self.clone(),
                tenant: tenant.to_string(),
                epoch: t.epoch,
                seq: t.seq,
                full,
            },
        }
    }

    /// Install what the load `ticket` read: `rows` are the tenant's facts (every one for a full load,
    /// those of the requested ids that still exist otherwise). Returns the snapshot to read, or `None`
    /// when the load was overtaken (an erasure, or a newer full load) and must be retried.
    pub fn install(
        &self,
        ticket: Ticket,
        rows: Vec<LoadedFact>,
        requested: Option<&[String]>,
    ) -> Option<Snapshot> {
        let mut guard = self.write();
        let t = guard.get_mut(&ticket.tenant)?;
        if t.epoch != ticket.epoch {
            return None;
        }
        match requested {
            None => {
                if ticket.seq < t.installed_seq {
                    return None;
                }
                let mut fresh = TenantFacts {
                    loaded: true,
                    epoch: t.epoch,
                    loading: t.loading,
                    seq: t.seq,
                    installed_seq: ticket.seq,
                    dirty: std::mem::take(&mut t.dirty),
                    ..Default::default()
                };
                // Writes recorded at or before the load began are in what it read.
                fresh.dirty.retain(|_, s| *s > ticket.seq);
                let mut by_collection: BTreeMap<String, HashMap<Arc<str>, CachedFact>> =
                    BTreeMap::new();
                for row in rows {
                    let id: Arc<str> = Arc::from(row.id);
                    fresh
                        .collection_of
                        .insert(id.clone(), row.collection.clone());
                    by_collection
                        .entry(row.collection)
                        .or_default()
                        .insert(id, row.fact);
                }
                fresh.collections = by_collection
                    .into_iter()
                    .map(|(c, m)| (c, Arc::new(m)))
                    .collect();
                *t = fresh;
            }
            Some(ids) => {
                let mut read: HashMap<String, LoadedFact> =
                    rows.into_iter().map(|r| (r.id.clone(), r)).collect();
                for id in ids {
                    // Applied only while the write that named it is still the latest: a later write
                    // keeps it dirty for the next recall, and a load that already applied it was at
                    // least as recent as this one.
                    match t.dirty.get(id) {
                        Some(s) if *s <= ticket.seq => {}
                        _ => continue,
                    }
                    t.dirty.remove(id);
                    match read.remove(id) {
                        Some(row) => t.insert(row),
                        None => t.remove(id),
                    }
                }
                t.installed_seq = t.installed_seq.max(ticket.seq);
            }
        }
        Some(t.collections.clone())
    }

    /// Record that a write changed or removed the facts `ids` of `tenant`. Called after it commits.
    pub fn forget(&self, tenant: &str, ids: &[String]) {
        let mut guard = self.write();
        let Some(t) = guard.get_mut(tenant) else {
            return;
        };
        if !t.loaded && t.loading == 0 {
            // Nothing cached and no load in flight: the next load reads the write.
            return;
        }
        for id in ids {
            t.seq += 1;
            let s = t.seq;
            t.dirty.insert(id.clone(), s);
        }
    }

    /// Drop everything cached for `tenant`; a load in flight installs nothing.
    pub fn forget_tenant(&self, tenant: &str) {
        let mut guard = self.write();
        if let Some(t) = guard.get_mut(tenant) {
            *t = TenantFacts {
                epoch: t.epoch + 1,
                loading: t.loading,
                seq: t.seq,
                ..Default::default()
            };
        }
    }

    /// A snapshot of `rows` that is not cached — the last resort of a recall whose loads kept being
    /// overtaken.
    pub fn uncached(rows: Vec<LoadedFact>) -> Snapshot {
        let mut by_collection: BTreeMap<String, HashMap<Arc<str>, CachedFact>> = BTreeMap::new();
        for row in rows {
            by_collection
                .entry(row.collection)
                .or_default()
                .insert(Arc::from(row.id), row.fact);
        }
        by_collection
            .into_iter()
            .map(|(c, m)| (c, Arc::new(m)))
            .collect()
    }

    pub const fn load_attempts() -> usize {
        LOAD_ATTEMPTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(user: &str, vis: Visibility, v: &[f32]) -> CachedFact {
        CachedFact {
            owner_user: Arc::from(user),
            owner_team: Some(Arc::from("t")),
            visibility: vis,
            memory_class: MemoryClass::Semantic,
            valid_from: DateTime::<Utc>::MIN_UTC,
            valid_to: None,
            ingested_at: DateTime::<Utc>::MIN_UTC,
            entities: Arc::from([Arc::<str>::from("e1")]),
            embedding: Embedding::Vector {
                values: Arc::from(v),
                norm_sq: norm_sq(v),
            },
        }
    }

    fn row(id: &str, coll: &str, user: &str) -> LoadedFact {
        LoadedFact {
            id: id.into(),
            collection: coll.into(),
            fact: fact(user, Visibility::UserPrivate, &[1.0, 0.0]),
        }
    }

    fn filter(user: &str) -> SearchFilter {
        SearchFilter {
            user: user.into(),
            teams: HashSet::from(["t".to_string()]),
            valid_at: None,
            memory_class: None,
            visibility: None,
            entity_facts: None,
        }
    }

    fn snapshot_ids(s: &Snapshot) -> Vec<String> {
        let mut out: Vec<String> = s
            .values()
            .flat_map(|m| m.keys().map(|k| k.to_string()))
            .collect();
        out.sort();
        out
    }

    fn load(cache: &Arc<SearchCache>) -> (Option<Vec<String>>, Ticket) {
        match cache.plan("acme") {
            Plan::Load { ids, ticket } => (ids, ticket),
            Plan::Fresh(_) => panic!("expected a load"),
        }
    }

    #[test]
    fn the_cosine_matches_the_scan_and_ties_go_by_id() {
        let q = [0.6f32, 0.8];
        let mut slab = HashMap::new();
        for id in ["b", "a", "c"] {
            slab.insert(
                Arc::<str>::from(id),
                fact("u", Visibility::UserPrivate, &[3.0, 4.0]),
            );
        }
        slab.insert(
            Arc::<str>::from("z"),
            fact("other", Visibility::UserPrivate, &[3.0, 4.0]),
        );
        let out = vector_run(&slab, &filter("u"), &q, norm_sq(&q), 2, 2).unwrap();
        let got: Vec<&str> = out.iter().map(|(i, _)| i.as_str()).collect();
        assert_eq!(
            got,
            vec!["a", "b"],
            "another user's private fact is not admitted"
        );
        let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
        for (x, y) in q.iter().zip([3.0f32, 4.0].iter()) {
            let (x, y) = (f64::from(*x), f64::from(*y));
            dot += x * y;
            na += x * x;
            nb += y * y;
        }
        assert_eq!(out[0].1, (dot / (na.sqrt() * nb.sqrt())).clamp(0.0, 1.0));
    }

    #[test]
    fn an_entity_run_takes_the_newest_admitted_facts_naming_an_entity() {
        let mut slab = HashMap::new();
        let at = |secs| DateTime::<Utc>::from_timestamp(secs, 0).unwrap();
        for (id, user, secs, entity) in [
            ("old", "u", 1, "e1"),
            ("tied-b", "u", 2, "e1"),
            ("tied-a", "u", 2, "e2"),
            ("elsewhere", "u", 3, "e9"),
            ("unreadable", "other", 4, "e1"),
        ] {
            let mut f = fact(user, Visibility::UserPrivate, &[1.0, 0.0]);
            f.ingested_at = at(secs);
            f.entities = Arc::from([Arc::<str>::from(entity)]);
            slab.insert(Arc::<str>::from(id), f);
        }
        let wanted = HashSet::from(["e1", "e2"]);
        let got = entity_run(&slab, &filter("u"), &wanted, 2);
        assert_eq!(
            got,
            vec![
                ("tied-a".to_string(), vec!["e2".to_string()]),
                ("tied-b".to_string(), vec!["e1".to_string()]),
            ]
        );
    }

    #[test]
    fn a_wrong_dimension_fails_only_when_admitted() {
        let mut slab = HashMap::new();
        let mut bad = fact("other", Visibility::UserPrivate, &[1.0, 0.0]);
        bad.embedding = Embedding::WrongDim(3);
        slab.insert(Arc::<str>::from("x"), bad);
        let q = [1.0f32, 0.0];
        assert!(vector_run(&slab, &filter("u"), &q, 1.0, 2, 5)
            .unwrap()
            .is_empty());
        let err = vector_run(&slab, &filter("other"), &q, 1.0, 2, 5).unwrap_err();
        assert!(err.to_string().contains("stored embedding dimension 3"));
    }

    #[test]
    fn a_write_during_a_full_load_is_reread_by_the_next_recall() {
        let cache = Arc::new(SearchCache::default());
        let (ids, ticket) = load(&cache);
        assert!(ids.is_none());
        // A write commits while the load is reading.
        cache.forget("acme", &["f1".into()]);
        let snap = cache
            .install(ticket, vec![row("f1", "c", "u")], None)
            .unwrap();
        assert_eq!(snapshot_ids(&snap), vec!["f1"]);
        let (ids, ticket) = load(&cache);
        assert_eq!(ids, Some(vec!["f1".to_string()]));
        let snap = cache
            .install(ticket, vec![row("f1", "d", "u")], Some(&["f1".into()]))
            .unwrap();
        assert_eq!(snap.keys().cloned().collect::<Vec<_>>(), vec!["d"]);
        assert!(matches!(cache.plan("acme"), Plan::Fresh(_)));
    }

    #[test]
    fn an_older_full_load_never_overwrites_a_newer_one() {
        let cache = Arc::new(SearchCache::default());
        let (_, older) = load(&cache);
        cache.forget("acme", &["f1".into()]);
        let (_, newer) = load(&cache);
        cache
            .install(newer, vec![row("f2", "c", "u")], None)
            .unwrap();
        assert!(cache
            .install(older, vec![row("f1", "c", "u")], None)
            .is_none());
        let Plan::Fresh(snap) = cache.plan("acme") else {
            panic!("the newer load is installed");
        };
        assert_eq!(snapshot_ids(&snap), vec!["f2"]);
    }

    #[test]
    fn a_partial_load_skips_a_fact_written_again_since() {
        let cache = Arc::new(SearchCache::default());
        let (_, t) = load(&cache);
        cache
            .install(t, vec![row("f1", "c", "u"), row("f2", "c", "u")], None)
            .unwrap();
        cache.forget("acme", &["f1".into()]);
        let (ids, t) = load(&cache);
        assert_eq!(ids, Some(vec!["f1".to_string()]));
        // A second write of f1 lands before the first reload installs.
        cache.forget("acme", &["f1".into()]);
        cache.install(t, vec![], Some(&["f1".into()])).unwrap();
        let (ids, t) = load(&cache);
        assert_eq!(ids, Some(vec!["f1".to_string()]), "still dirty");
        let snap = cache.install(t, vec![], Some(&["f1".into()])).unwrap();
        assert_eq!(snapshot_ids(&snap), vec!["f2"]);
    }

    #[test]
    fn an_erasure_during_a_load_installs_nothing() {
        let cache = Arc::new(SearchCache::default());
        let (_, t) = load(&cache);
        cache.forget_tenant("acme");
        assert!(cache.install(t, vec![row("f1", "c", "u")], None).is_none());
        assert!(matches!(cache.plan("acme"), Plan::Load { ids: None, .. }));
    }

    #[test]
    fn an_idle_unloaded_tenant_records_no_writes() {
        let cache = Arc::new(SearchCache::default());
        {
            let (_, _t) = load(&cache);
        }
        cache.forget("acme", &["f1".into()]);
        let guard = cache.by_tenant.read().unwrap();
        assert!(guard.get("acme").unwrap().dirty.is_empty());
    }
}
