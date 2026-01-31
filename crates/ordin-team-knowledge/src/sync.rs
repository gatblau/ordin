//! Checking (§D), getting (§F), sharing (§E) and what follows a send (§G).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Instant;

use git2::Oid;
use ordin_knowledge::{TeamPageImport, TeamPageImported};
use ordin_memory_service::{TeamFact, TeamImportBatch, TeamImportResult, TeamTerm};
use ordin_push::{PushNotice, TeamShareProposal};
use ordin_types::domain::TeamAccess;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::{KnowledgePageId, MemoryFactId, TeamShareId, WorkspaceId};

use crate::error::TeamError;
use crate::files::{self, ItemKind, PathKind};
use crate::git;
use crate::model::{GetReport, ShareProposed, TeamAvailable, TeamCounts};
use crate::service::{
    sha256_hex, Local, TeamKnowledgeService, Tip, IMPORT_BATCH, MAX_MOVES, SOFT_FOLDER_FACTS,
};
use crate::store::{self, Binding, Planned, Record, ShareScope};
use crate::text;

/// What §F step 3 decided for one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Take {
    Remote,
    BothChanged,
}

/// A share's change set (§E step 5 with the team workspace's files).
#[derive(Debug, Default)]
pub(crate) struct Plan {
    /// Path relative to the folder → new bytes, `None` = delete.
    pub changes: BTreeMap<String, Option<Vec<u8>>>,
    pub counts: TeamCounts,
    pub changed_titles: Vec<String>,
    pub new_titles: Vec<String>,
    pub held_for_check: u32,
    /// Items counted in `counts` or a team workspace file: a share needs at least one.
    pub counted: usize,
    /// TW-B step 3: the subjects of the workspace's changed settings.
    pub workspace_subjects: Vec<String>,
    pub description_changed: bool,
    /// TW-F step 3.
    pub activity: usize,
    /// TW-G step 2: the results' file names, in path order.
    pub results: Vec<String>,
}

/// The counted kinds of §D step 4 and §E step 5.
fn count(counts: &mut TeamCounts, kind: ItemKind, change: Change) {
    match (kind, change) {
        (ItemKind::Fact, Change::New) => counts.new_facts += 1,
        (ItemKind::Fact, Change::Changed) => counts.changed_facts += 1,
        (ItemKind::Fact, Change::Removed) => counts.removed_facts += 1,
        (ItemKind::Page, Change::New) => counts.new_pages += 1,
        (ItemKind::Page, Change::Changed) => counts.changed_pages += 1,
        (ItemKind::Page, Change::Removed) => counts.removed_pages += 1,
        _ => {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    New,
    Changed,
    Removed,
}

fn oid_text(oid: Oid) -> String {
    oid.to_string()
}

/// The item paths of a tip (terms excluded: they have no record, TK-TR-1).
fn item_paths(tip: &Tip) -> BTreeMap<String, (ItemKind, String, Oid)> {
    tip.files
        .iter()
        .filter_map(|(path, oid)| match files::classify(path) {
            Some(PathKind::Item { kind, stem }) if kind != ItemKind::Term => {
                Some((path.clone(), (kind, stem, *oid)))
            }
            _ => None,
        })
        .collect()
}

/// "8 new facts and 1 page from Priya Shah and Sam Okafor": ≤ 5 names, then "and N others".
fn from_list(names: BTreeSet<String>) -> Vec<String> {
    let all: Vec<String> = names.into_iter().collect();
    if all.len() <= 5 {
        return all;
    }
    let mut out: Vec<String> = all[..5].to_vec();
    out.push(format!("and {} others", all.len() - 5));
    out
}

impl TeamKnowledgeService {
    // ---- §D ----

    // spec: ordin-desktop/components/TeamKnowledgeService#SA-TEAM-07
    /// §D: reads only. The caller holds the workspace's lock.
    pub(crate) async fn check(&self, ws: WorkspaceId) -> Result<(), TeamError> {
        let started = Instant::now();
        let pool = self.pool()?.clone();
        let mut b = self.bound(ws).await?;
        let tip = self.fetch_folder(&b).await?;
        self.recheck_access(&mut b).await;
        self.settle_shares(ws, &b, &tip).await?;
        let tip_text = oid_text(tip.commit);
        if b.last_checked_commit.as_deref() == Some(tip_text.as_str()) {
            store::touch_checked(&pool, ws).await?;
            return Ok(());
        }
        match self.folder_format(ws, &tip).await? {
            Err(()) => {
                let blob = tip
                    .files
                    .get(files::FORMAT_FILE)
                    .map(|o| o.to_string())
                    .unwrap_or_default();
                store::put_problem(&pool, ws, files::FORMAT_FILE, "TEAM_FILE_UNREADABLE", &blob)
                    .await?;
            }
            Ok(Some(n)) if n > files::FOLDER_FORMAT => {
                let blob = tip
                    .files
                    .get(files::FORMAT_FILE)
                    .map(|o| o.to_string())
                    .unwrap_or_default();
                store::put_problem(&pool, ws, files::FORMAT_FILE, "TEAM_FORMAT_NEWER", &blob)
                    .await?;
            }
            _ => {
                store::clear_problem(&pool, ws, files::FORMAT_FILE).await?;
            }
        }
        let records = store::records(&pool, ws).await?;
        let remote = item_paths(&tip);
        let mut counts = TeamCounts::default();
        let mut sharers_wanted = Vec::new();
        for (path, (kind, stem, oid)) in &remote {
            match records.get(path) {
                None => {
                    count(&mut counts, *kind, Change::New);
                    sharers_wanted.push((path.clone(), *kind, stem.clone(), *oid));
                }
                Some(r) if r.blob != oid_text(*oid) => {
                    count(&mut counts, *kind, Change::Changed);
                    sharers_wanted.push((path.clone(), *kind, stem.clone(), *oid));
                }
                _ => {}
            }
        }
        for (path, r) in &records {
            if !remote.contains_key(path) {
                if let Some(kind) = ItemKind::parse(&r.kind) {
                    count(&mut counts, kind, Change::Removed);
                }
            }
        }
        // `from`: the shared_by of the new and changed fact and page files only.
        let wanted: Vec<(String, Oid, u64)> = sharers_wanted
            .iter()
            .filter(|(_, k, _, _)| matches!(k, ItemKind::Fact | ItemKind::Page))
            .map(|(p, k, _, o)| {
                let max = if *k == ItemKind::Page {
                    files::PAGE_MAX_BYTES
                } else {
                    files::ITEM_MAX_BYTES
                };
                (p.clone(), *o, max as u64)
            })
            .collect();
        let blobs = self.read_blobs(self.clone_path(ws), wanted).await?;
        let mut names = BTreeSet::new();
        for (path, kind, stem, _) in &sharers_wanted {
            if let Some(Some(bytes)) = blobs.get(path) {
                if let Some(name) = files::sharer_of(*kind, bytes, stem) {
                    names.insert(name);
                }
            }
        }
        let facts = remote
            .values()
            .filter(|(k, _, _)| *k == ItemKind::Fact)
            .count();
        let folder_large = facts > SOFT_FOLDER_FACTS;
        if folder_large {
            tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), facts, "team.folder_large");
        }
        // §D step 5: what a share would send, without writing.
        let (to_share, held) = if b.access == TeamAccess::ReadWrite {
            let name = self.person_name().await;
            match self.export_local(ws, &name).await {
                Ok(local) => {
                    let plan = self.item_plan(&local, &tip, &records);
                    (Some(plan.counts), local.held_for_check)
                }
                Err(_) => (b.to_share, b.held_for_check),
            }
        } else {
            (None, b.held_for_check)
        };
        let available = (!counts.is_empty()).then(|| TeamAvailable {
            counts,
            from: from_list(names),
        });
        let became =
            available.is_some() && b.available.as_ref().is_none_or(|a| a.counts.is_empty());
        store::store_check(
            &pool,
            ws,
            available.as_ref(),
            to_share.as_ref(),
            held,
            &tip_text,
            folder_large,
        )
        .await?;
        tracing::info!(
            target: "ordin::team_knowledge",
            workspace_id = ws.to_string().as_str(),
            new = counts.new_facts + counts.new_pages,
            changed = counts.changed_facts + counts.changed_pages,
            removed = counts.removed_facts + counts.removed_pages,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "team.checked"
        );
        if became {
            self.changed(ws).await;
        }
        Ok(())
    }

    // ---- §F ----

    // spec: ordin-desktop/components/TeamKnowledgeService#TK-TR-5
    /// §F with the team workspace's additions (TW-D, TW-F step 4, TW-G step 5). The caller holds
    /// the workspace's lock. Returns the report and the tip it read.
    pub(crate) async fn get_now(&self, ws: WorkspaceId) -> Result<(GetReport, Tip), TeamError> {
        let started = Instant::now();
        let pool = self.pool()?.clone();
        let mut b = self.bound(ws).await?;
        let tip = self.fetch_folder(&b).await?;
        self.recheck_access(&mut b).await;
        match self.folder_format(ws, &tip).await? {
            Ok(Some(n)) if n > files::FOLDER_FORMAT => {
                return Err(TeamError::permanent("TEAM_FORMAT_NEWER"));
            }
            Err(()) => {
                let blob = tip
                    .files
                    .get(files::FORMAT_FILE)
                    .map(|o| o.to_string())
                    .unwrap_or_default();
                store::put_problem(&pool, ws, files::FORMAT_FILE, "TEAM_FILE_UNREADABLE", &blob)
                    .await?;
            }
            _ => {
                store::clear_problem(&pool, ws, files::FORMAT_FILE).await?;
            }
        }
        let name = self.person_name().await;
        let local = self.export_local(ws, &name).await?;
        let records = store::records(&pool, ws).await?;
        let remote = item_paths(&tip);
        let path = self.clone_path(ws);

        // Step 3: classify.
        let mut takes: BTreeMap<String, (ItemKind, String, Oid, Take)> = BTreeMap::new();
        let mut removals: Vec<(String, ItemKind, String, bool)> = Vec::new();
        for (p, (kind, stem, oid)) in &remote {
            let record = records.get(p);
            if record.is_some_and(|r| r.blob == oid_text(*oid)) {
                continue;
            }
            let local_unchanged = match (local.files.get(p), record) {
                (None, _) => true,
                (Some(l), Some(r)) => sha256_hex(&l.bytes) == r.local_sha256,
                // No record: this computer never synced the item; the remote file is taken.
                (Some(_), None) => true,
            };
            let take = if local_unchanged {
                Take::Remote
            } else {
                Take::BothChanged
            };
            takes.insert(p.clone(), (*kind, stem.clone(), *oid, take));
        }
        for (p, r) in &records {
            if remote.contains_key(p) {
                continue;
            }
            let Some(kind) = ItemKind::parse(&r.kind) else {
                continue;
            };
            let unchanged = local
                .files
                .get(p)
                .is_none_or(|l| sha256_hex(&l.bytes) == r.local_sha256);
            removals.push((p.clone(), kind, r.item_id.clone(), unchanged));
        }

        // The blobs to read: every new or changed item, and the entities and sources the taken facts
        // name, and every term file of the tip (TK-TR-2).
        let mut wanted: Vec<(String, Oid, u64)> = takes
            .iter()
            .map(|(p, (k, _, o, _))| {
                let max = if *k == ItemKind::Page {
                    files::PAGE_MAX_BYTES
                } else {
                    files::ITEM_MAX_BYTES
                };
                (p.clone(), *o, max as u64)
            })
            .collect();
        for (p, oid) in &tip.files {
            if let Some(PathKind::Item { kind, .. }) = files::classify(p) {
                if matches!(kind, ItemKind::Term | ItemKind::Entity | ItemKind::Source)
                    && !takes.contains_key(p)
                {
                    wanted.push((p.clone(), *oid, files::ITEM_MAX_BYTES as u64));
                }
            }
        }
        let blobs = self.read_blobs(path.clone(), wanted).await?;
        let bytes_of = |p: &str| blobs.get(p).cloned().flatten();

        let mut report = GetReport {
            counts: TeamCounts::default(),
            not_taken: 0,
            skipped_files: 0,
            commit: oid_text(tip.commit),
        };
        let problem = |p: &str, oid: Oid| (p.to_owned(), oid_text(oid));
        let mut unreadable: Vec<(String, String)> = Vec::new();

        // Terms of the tip (TK-TR-2).
        let mut tip_terms: HashMap<String, TeamTerm> = HashMap::new();
        let mut bad_terms: BTreeSet<String> = BTreeSet::new();
        for (p, oid) in &tip.files {
            if let Some(PathKind::Item {
                kind: ItemKind::Term,
                stem,
            }) = files::classify(p)
            {
                match bytes_of(p).as_deref().map(|b| files::parse_term(b, &stem)) {
                    Some(Ok(term)) => {
                        tip_terms.insert(stem, term);
                    }
                    _ => {
                        bad_terms.insert(stem);
                        unreadable.push(problem(p, *oid));
                    }
                }
            }
        }
        let local_terms: BTreeSet<String> = local
            .files
            .values()
            .filter(|f| f.kind == ItemKind::Term)
            .map(|f| f.item_id.clone())
            .collect();

        // Entities and sources of the tip, read as needed.
        let mut entities = HashMap::new();
        let mut sources = HashMap::new();
        for (p, oid) in &tip.files {
            match files::classify(p) {
                Some(PathKind::Item {
                    kind: ItemKind::Entity,
                    stem,
                }) => {
                    match bytes_of(p)
                        .as_deref()
                        .map(|b| files::parse_entity(b, &stem))
                    {
                        Some(Ok(e)) => {
                            entities.insert(e.id.clone(), (p.clone(), e));
                        }
                        _ if takes.contains_key(p) => unreadable.push(problem(p, *oid)),
                        _ => {}
                    }
                }
                Some(PathKind::Item {
                    kind: ItemKind::Source,
                    stem,
                }) => {
                    match bytes_of(p)
                        .as_deref()
                        .map(|b| files::parse_source(b, &stem))
                    {
                        Some(Ok(s)) => {
                            sources.insert(s.id.clone(), (p.clone(), s));
                        }
                        _ if takes.contains_key(p) => unreadable.push(problem(p, *oid)),
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Parse the taken facts, relationships and pages.
        let mut facts: Vec<(String, TeamFact, Take)> = Vec::new();
        let mut relationships = Vec::new();
        let mut pages: Vec<(String, KnowledgePageId, Vec<u8>, Take, bool)> = Vec::new();
        for (p, (kind, stem, oid, take)) in &takes {
            let Some(bytes) = bytes_of(p) else {
                unreadable.push(problem(p, *oid));
                continue;
            };
            match kind {
                ItemKind::Fact => match files::parse_fact(&bytes, stem) {
                    Ok(fact) => {
                        let missing_word = fact.triple.as_ref().is_some_and(|t| {
                            bad_terms.contains(&t.predicate)
                                || (!tip_terms.contains_key(&t.predicate)
                                    && !local_terms.contains(&t.predicate))
                        });
                        if missing_word {
                            unreadable.push(problem(p, *oid));
                            continue;
                        }
                        let fact = if *take == Take::BothChanged {
                            match local.facts.get(p) {
                                Some(mine) => merge_fact(mine, fact),
                                None => fact,
                            }
                        } else {
                            fact
                        };
                        facts.push((p.clone(), fact, *take));
                    }
                    Err(()) => unreadable.push(problem(p, *oid)),
                },
                ItemKind::Relationship => match files::parse_relationship(&bytes, stem) {
                    Ok(r) => relationships.push((p.clone(), r)),
                    Err(()) => unreadable.push(problem(p, *oid)),
                },
                ItemKind::Page => match files::parse_page(&bytes, stem) {
                    Ok((page_id, _)) => {
                        let is_new = !records.contains_key(p);
                        pages.push((p.clone(), page_id, bytes, *take, is_new));
                    }
                    Err(()) => unreadable.push(problem(p, *oid)),
                },
                // Entities and sources travel inside the fact batches.
                _ => {}
            }
        }
        let unreadable_paths: BTreeSet<String> =
            unreadable.iter().map(|(p, _)| p.clone()).collect();
        for (p, blob) in &unreadable {
            store::put_problem(&pool, ws, p, "TEAM_FILE_UNREADABLE", blob).await?;
        }
        report.skipped_files = unreadable_paths.len() as u32;

        // Step 4: the import, sources and entities with the facts that name them.
        let mut recorded: Vec<String> = Vec::new();
        let terms_for = |fs: &[(String, TeamFact, Take)]| -> Vec<TeamTerm> {
            let keys: BTreeSet<&str> = fs
                .iter()
                .filter_map(|(_, f, _)| f.triple.as_ref().map(|t| t.predicate.as_str()))
                .collect();
            keys.into_iter()
                .filter_map(|k| tip_terms.get(k).cloned())
                .collect()
        };
        let changed_entities: Vec<&str> = takes
            .iter()
            .filter(|(p, (k, _, _, _))| *k == ItemKind::Entity && !unreadable_paths.contains(*p))
            .filter_map(|(p, _)| {
                entities
                    .values()
                    .find(|(ep, _)| ep == p)
                    .map(|(_, e)| e.id.as_str())
            })
            .collect();
        let changed_sources: Vec<&str> = takes
            .iter()
            .filter(|(p, (k, _, _, _))| *k == ItemKind::Source && !unreadable_paths.contains(*p))
            .filter_map(|(p, _)| {
                sources
                    .values()
                    .find(|(sp, _)| sp == p)
                    .map(|(_, s)| s.id.as_str())
            })
            .collect();
        let chunks: Vec<&[(String, TeamFact, Take)]> = if facts.is_empty() {
            Vec::new()
        } else {
            facts.chunks(IMPORT_BATCH).collect()
        };
        let mut relationships_sent = false;
        let mut batches: Vec<(TeamImportBatch, Vec<String>)> = Vec::new();
        for (n, chunk) in chunks.iter().enumerate() {
            let mut named_entities: BTreeSet<String> = BTreeSet::new();
            let mut named_sources: BTreeSet<String> = BTreeSet::new();
            for (_, f, _) in chunk.iter() {
                named_entities.extend(f.entities.iter().cloned());
                if let Some(s) = &f.source_id {
                    named_sources.insert(s.clone());
                }
            }
            if n == 0 {
                named_entities.extend(changed_entities.iter().map(|s| (*s).to_owned()));
                named_sources.extend(changed_sources.iter().map(|s| (*s).to_owned()));
            }
            let mut carried = Vec::new();
            let batch_entities = named_entities
                .iter()
                .filter_map(|id| entities.get(id))
                .map(|(p, e)| {
                    carried.push(p.clone());
                    e.clone()
                })
                .collect();
            let batch_sources = named_sources
                .iter()
                .filter_map(|id| sources.get(id))
                .map(|(p, s)| {
                    carried.push(p.clone());
                    s.clone()
                })
                .collect();
            let batch_relationships = if n == 0 {
                relationships_sent = true;
                relationships
                    .iter()
                    .map(|(p, r)| {
                        carried.push(p.clone());
                        r.clone()
                    })
                    .collect()
            } else {
                Vec::new()
            };
            batches.push((
                TeamImportBatch {
                    facts: chunk.iter().map(|(_, f, _)| f.clone()).collect(),
                    terms: terms_for(chunk),
                    entities: batch_entities,
                    sources: batch_sources,
                    relationships: batch_relationships,
                    read_folder: None,
                },
                carried,
            ));
        }
        if !relationships_sent && !relationships.is_empty() {
            // ADR-111: a changed relationship alone travels in a batch with no facts.
            let carried = relationships.iter().map(|(p, _)| p.clone()).collect();
            batches.push((
                TeamImportBatch {
                    facts: Vec::new(),
                    terms: Vec::new(),
                    entities: Vec::new(),
                    sources: Vec::new(),
                    relationships: relationships.iter().map(|(_, r)| r.clone()).collect(),
                    read_folder: None,
                },
                carried,
            ));
        }
        let fact_paths: HashMap<String, (String, Take)> = facts
            .iter()
            .map(|(p, f, t)| (f.id.to_string(), (p.clone(), *t)))
            .collect();
        for (batch, carried) in batches {
            let mut outcome = self.deps.memory.import_team(ws, batch.clone()).await;
            if matches!(&outcome, Err(e) if e.retryable()) {
                // Step 5: retried once after 2 s.
                self.wait(std::time::Duration::from_secs(2)).await;
                outcome = self.deps.memory.import_team(ws, batch).await;
            }
            let outcomes = outcome
                .map_err(|e| TeamError::per_cause("TEAM_MEMORY_UNAVAILABLE", e.retryable()))?;
            for p in carried {
                if let Some(oid) = tip.files.get(&p) {
                    self.record_taken(&pool, ws, &p, *oid, &local).await?;
                    recorded.push(p);
                }
            }
            for o in outcomes {
                let Some((p, take)) = fact_paths.get(&o.item_id) else {
                    continue;
                };
                let oid = tip.files[p];
                match o.result {
                    TeamImportResult::Kept | TeamImportResult::Unchanged => {
                        self.record_taken(&pool, ws, p, oid, &local).await?;
                        store::clear_problem(&pool, ws, p).await?;
                        recorded.push(p.clone());
                        let change = if records.contains_key(p) {
                            Change::Changed
                        } else {
                            Change::New
                        };
                        let _ = take;
                        count(&mut report.counts, ItemKind::Fact, change);
                    }
                    TeamImportResult::NotTaken { .. } => {
                        // TK-TR-5: every NotTaken is drawn as TEAM_FACT_NOT_TAKEN.
                        store::put_problem(&pool, ws, p, "TEAM_FACT_NOT_TAKEN", &oid_text(oid))
                            .await?;
                        report.not_taken += 1;
                    }
                }
            }
        }

        // Step 6: pages.
        for (p, page_id, bytes, take, is_new) in pages {
            let imported = self
                .deps
                .pages
                .import_team_page(
                    ws,
                    TeamPageImport {
                        page_id,
                        bytes,
                        both_changed: take == Take::BothChanged,
                    },
                )
                .await;
            let oid = tip.files[&p];
            match imported {
                Ok(
                    TeamPageImported::Written { .. }
                    | TeamPageImported::KeptLocal { .. }
                    | TeamPageImported::Unchanged,
                ) => {
                    self.record_taken(&pool, ws, &p, oid, &local).await?;
                    store::clear_problem(&pool, ws, &p).await?;
                    recorded.push(p.clone());
                    let change = if is_new { Change::New } else { Change::Changed };
                    count(&mut report.counts, ItemKind::Page, change);
                }
                Err(e) => {
                    store::put_problem(&pool, ws, &p, e.code(), &oid_text(oid)).await?;
                }
            }
        }

        // Removed by the team.
        for (p, kind, item_id, unchanged) in removals {
            if unchanged {
                match kind {
                    ItemKind::Fact => {
                        if let Ok(id) = MemoryFactId::parse(&item_id) {
                            self.deps
                                .memory
                                .forget_team_fact(ws, id)
                                .await
                                .map_err(|e| {
                                    TeamError::per_cause("TEAM_MEMORY_UNAVAILABLE", e.retryable())
                                })?;
                        }
                        count(&mut report.counts, ItemKind::Fact, Change::Removed);
                    }
                    ItemKind::Page => {
                        if let Ok(id) = KnowledgePageId::parse(&item_id) {
                            if let Err(e) = self.deps.pages.remove_team_page(ws, id).await {
                                store::put_problem(&pool, ws, &p, e.code(), "").await?;
                                continue;
                            }
                        }
                        count(&mut report.counts, ItemKind::Page, Change::Removed);
                    }
                    // SA-TEAM-AMEM-02: a removed relationship's local link is kept; entities and
                    // sources leave with the facts that name them.
                    _ => {}
                }
            }
            store::drop_record(&pool, ws, &p).await?;
            store::clear_problem(&pool, ws, &p).await?;
        }
        // A problem whose path left the folder is cleared.
        for (problem, _) in store::problems(&pool, ws).await? {
            let known = tip.files.contains_key(&problem.path)
                || problem.path.starts_with("results/")
                || problem.code.starts_with("TEAM_WS_");
            if !known {
                store::clear_problem(&pool, ws, &problem.path).await?;
            }
        }

        // The local digests of what was taken, as the workspace now renders it.
        if !recorded.is_empty() {
            if let Ok(after) = self.export_local(ws, &name).await {
                for p in &recorded {
                    if let (Some(l), Some(oid)) = (after.files.get(p), tip.files.get(p)) {
                        let kind = l.kind;
                        store::put_record(
                            &pool,
                            ws,
                            p,
                            &Record {
                                kind: kind.as_str().to_owned(),
                                item_id: l.item_id.clone(),
                                blob: oid_text(*oid),
                                local_sha256: sha256_hex(&l.bytes),
                            },
                        )
                        .await?;
                    }
                }
            }
        }

        // The team workspace's description, activity and results (TW-D, TW-F step 4, TW-G step 5).
        if self.team_row(ws).await?.is_some() {
            self.get_team_parts(ws, &b, &tip).await?;
        }

        // Step 7.
        store::store_got(&pool, ws, &report.commit).await?;
        self.journal(
            "got",
            ws,
            "Team knowledge was brought into this workspace.",
            serde_json::json!({ "workspace_id": ws, "counts": report.counts, "not_taken": report.not_taken, "skipped_files": report.skipped_files }),
        )
        .await;
        tracing::info!(
            target: "ordin::team_knowledge",
            workspace_id = ws.to_string().as_str(),
            imported = recorded.len(),
            not_taken = report.not_taken,
            skipped = report.skipped_files,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "team.got"
        );
        store::set_last_get(&pool, ws, &report).await?;
        self.changed(ws).await;
        Ok((report, tip))
    }

    /// The record of a taken path: its remote blob, and for now the digest of the remote bytes; the
    /// digest of the workspace's own rendering replaces it once the get has written everything.
    async fn record_taken(
        &self,
        pool: &sqlx::SqlitePool,
        ws: WorkspaceId,
        p: &str,
        oid: Oid,
        local: &Local,
    ) -> Result<(), TeamError> {
        let Some(PathKind::Item { kind, stem }) = files::classify(p) else {
            return Ok(());
        };
        let item_id = match kind {
            ItemKind::Entity => format!("entity:{stem}"),
            ItemKind::Source => format!("source:{stem}"),
            ItemKind::Relationship => format!("relationship:{stem}"),
            _ => stem,
        };
        let digest = local
            .files
            .get(p)
            .map(|l| sha256_hex(&l.bytes))
            .unwrap_or_else(|| "0".repeat(64));
        store::put_record(
            pool,
            ws,
            p,
            &Record {
                kind: kind.as_str().to_owned(),
                item_id,
                blob: oid_text(oid),
                local_sha256: digest,
            },
        )
        .await
    }

    // ---- §E ----

    /// §E step 5 for the knowledge items: the comparison with the tip, path by path.
    pub(crate) fn item_plan(
        &self,
        local: &Local,
        tip: &Tip,
        records: &HashMap<String, Record>,
    ) -> Plan {
        let mut plan = Plan {
            held_for_check: local.held_for_check,
            ..Plan::default()
        };
        for (p, l) in &local.files {
            let remote = tip.files.get(p);
            if l.kind == ItemKind::Term {
                // SPEC-GAP-9: TK-TR-1 rewrites a term file "when its label or meaning changed here", but
                // `team_knowledge_items` has no `term` kind, so a change here cannot be told from a
                // colleague's. A term file is written when absent or different from the tip; never deleted.
                let differs = match remote {
                    None => true,
                    Some(oid) => {
                        Oid::hash_object(git2::ObjectType::Blob, &l.bytes).ok() != Some(*oid)
                    }
                };
                if differs {
                    plan.changes.insert(p.clone(), Some(l.bytes.clone()));
                }
                continue;
            }
            let record = records.get(p);
            match remote {
                None => {
                    plan.changes.insert(p.clone(), Some(l.bytes.clone()));
                    count(&mut plan.counts, l.kind, Change::New);
                    plan.counted += 1;
                    if l.kind == ItemKind::Page {
                        plan.new_titles.extend(l.title.clone());
                    }
                }
                Some(oid) => {
                    let same =
                        Oid::hash_object(git2::ObjectType::Blob, &l.bytes).ok() == Some(*oid);
                    if !same && record.is_some_and(|r| r.blob == oid_text(*oid)) {
                        plan.changes.insert(p.clone(), Some(l.bytes.clone()));
                        count(&mut plan.counts, l.kind, Change::Changed);
                        plan.counted += 1;
                        if l.kind == ItemKind::Page {
                            plan.changed_titles.extend(l.title.clone());
                        }
                    }
                }
            }
        }
        for (p, r) in records {
            if local.files.contains_key(p) {
                continue;
            }
            if let Some(oid) = tip.files.get(p) {
                if r.blob == oid_text(*oid) {
                    plan.changes.insert(p.clone(), None);
                    if let Some(kind) = ItemKind::parse(&r.kind) {
                        count(&mut plan.counts, kind, Change::Removed);
                        plan.counted += 1;
                    }
                }
            }
        }
        if plan.counted > 0 {
            let format_bytes = files::format_file_bytes();
            let same = tip.files.get(files::FORMAT_FILE).is_some_and(|oid| {
                Oid::hash_object(git2::ObjectType::Blob, &format_bytes).ok() == Some(*oid)
            });
            if !same {
                plan.changes
                    .insert(files::FORMAT_FILE.to_owned(), Some(format_bytes));
            }
        }
        plan
    }

    /// §E (with TW-B, TW-F step 3 and TW-G step 2 by scope). The caller holds the workspace's lock.
    /// `Ok(None)`: a results share with nothing to send.
    pub(crate) async fn share_now(
        &self,
        ws: WorkspaceId,
        scope: ShareScope,
    ) -> Result<Option<ShareProposed>, TeamError> {
        let pool = self.pool()?.clone();
        if scope == ShareScope::All {
            self.share_refusals(ws).await?;
        } else {
            let b = self.bound(ws).await?;
            if b.access == TeamAccess::ReadOnly {
                return Err(TeamError::permanent("TEAM_READ_ONLY"));
            }
            if store::open_share(&pool, ws).await?.is_some() {
                return Err(TeamError::permanent("TEAM_SHARE_PENDING"));
            }
        }
        // Step 2: get first.
        let (_, tip) = self.get_now(ws).await?;
        let b = self.bound(ws).await?;
        let name = self.person_name().await;
        let (plan, ()) = self.plan_share(ws, &b, &tip, scope, &name, None).await?;
        if plan.changes.is_empty()
            || (plan.counted == 0
                && plan.activity == 0
                && plan.results.is_empty()
                && !plan.description_changed)
        {
            if scope == ShareScope::Results {
                return Ok(None);
            }
            return Err(TeamError::permanent("TEAM_NOTHING_TO_SHARE"));
        }
        let login = self.login_for(&b, scope).await?;
        let (commit, planned) = self
            .build_commit(ws, &b, &tip, &plan, &name, login.as_deref())
            .await?;
        // Step 7: review.
        let rules = self
            .deps
            .repos
            .branch_review_rules(&b.host, &b.repository, &b.branch)
            .await
            .map_err(|_| TeamError::of("TEAM_REMOTE_UNREACHABLE"))?;
        let share_id = TeamShareId::new();
        let review_branch = rules.review_required.then(|| text::review_branch(share_id));
        // Step 8: the card.
        let workspace_name = self
            .active_workspace(ws)
            .await
            .map(|w| w.name)
            .unwrap_or_default();
        let card = match scope {
            ShareScope::All => text::share_card(&b, &plan),
            ShareScope::Results => text::results_card(&b, &plan, &workspace_name),
            ShareScope::Workspace => {
                let preview = self.share_preview_lines(ws, &b).await?;
                text::workspace_card(&b, &plan, &workspace_name, &preview)
            }
        };
        let share = store::Share {
            id: share_id,
            workspace_id: ws,
            state: "proposed".to_owned(),
            base_commit: oid_text(tip.commit),
            commit_id: commit.clone(),
            pull_request: None,
            counts: plan.counts,
            planned,
            moves: 0,
            scope,
        };
        store::insert_share(&pool, &share).await?;
        let (title, body) = text::request_text(&name, &plan);
        let proposal = TeamShareProposal {
            share_id,
            workspace_id: ws,
            clone_path: self.clone_path(ws),
            commit,
            host: b.host.clone(),
            repository: b.repository.clone(),
            branch: b.branch.clone(),
            review_branch,
            title,
            body,
            card,
        };
        let approval = match self.deps.push.propose_team_share(proposal).await {
            Ok(a) => a,
            Err(e) => {
                store::set_share_state(&pool, share_id, "failed", Some("TEAM_PUSH_FAILED")).await?;
                return Err(TeamError::per_cause("TEAM_PUSH_FAILED", e.retryable));
            }
        };
        store::set_share_approval(&pool, share_id, approval).await?;
        let proposed = ShareProposed {
            share_id,
            approval_id: approval,
            counts: plan.counts,
            held_for_check: plan.held_for_check,
        };
        self.journal(
            "share_proposed",
            ws,
            "A share of team knowledge waits for approval.",
            serde_json::json!({ "workspace_id": ws, "share_id": share_id, "counts": plan.counts }),
        )
        .await;
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), share_id = share_id.to_string().as_str(), items = plan.changes.len(), "team.share_proposed");
        store::set_last_share(&pool, ws, &proposed).await?;
        Ok(Some(proposed))
    }

    /// The change set of a share of `scope` against `tip`. `only`: the paths an approved card
    /// covered (§G step 3), so a rebuild sends no item the person did not see.
    pub(crate) async fn plan_share(
        &self,
        ws: WorkspaceId,
        b: &Binding,
        tip: &Tip,
        scope: ShareScope,
        name: &str,
        only: Option<&BTreeSet<String>>,
    ) -> Result<(Plan, ()), TeamError> {
        let pool = self.pool()?.clone();
        let records = store::records(&pool, ws).await?;
        let mut plan = if scope == ShareScope::Results {
            Plan::default()
        } else {
            let local = self.export_local(ws, name).await?;
            self.item_plan(&local, tip, &records)
        };
        if self.team_row(ws).await?.is_some() {
            self.plan_team_parts(ws, b, tip, scope, &mut plan).await?;
        }
        if let Some(only) = only {
            plan.changes.retain(|p, _| only.contains(p));
        }
        Ok((plan, ()))
    }

    /// The login of SA-TEAM-TW-02 for a share from a team workspace; None otherwise.
    async fn login_for(&self, b: &Binding, scope: ShareScope) -> Result<Option<String>, TeamError> {
        if scope == ShareScope::All && self.team_row(b.workspace_id).await?.is_none() {
            return Ok(None);
        }
        self.login(&b.host).await.map(Some)
    }

    /// §E step 6 (and TW-B step 5's author): writes the blobs and the commit in the clone.
    pub(crate) async fn build_commit(
        &self,
        ws: WorkspaceId,
        b: &Binding,
        tip: &Tip,
        plan: &Plan,
        name: &str,
        login: Option<&str>,
    ) -> Result<(String, Vec<Planned>), TeamError> {
        let email = match self.deps.repos.person().await {
            Ok(p) => p.email,
            Err(_) => None,
        };
        let author = text::author_name(name, login);
        let message = text::commit_message(name, plan);
        let changes: Vec<(String, Option<Vec<u8>>)> = plan
            .changes
            .iter()
            .map(|(p, v)| (p.clone(), v.clone()))
            .collect();
        let path = self.clone_path(ws);
        let folder = b.folder.clone();
        let parent = tip.commit;
        let built = tokio::task::spawn_blocking(move || {
            let repo = git2::Repository::open_bare(&path).map_err(|_| git::GitFailure::Disk)?;
            // SA-TEAM-03: the person's email, else the clone's user.email, else a reserved name.
            let email = email
                .or_else(|| {
                    repo.config()
                        .ok()
                        .and_then(|c| c.get_string("user.email").ok())
                        .filter(|e| !e.trim().is_empty())
                })
                .unwrap_or_else(|| "team-knowledge@ordin.invalid".to_owned());
            let signature =
                git2::Signature::now(&author, &email).map_err(|_| git::GitFailure::Disk)?;
            let mut blobs = Vec::with_capacity(changes.len());
            let mut planned = Vec::with_capacity(changes.len());
            for (p, bytes) in &changes {
                let oid = match bytes {
                    Some(b) => Some(git::write_blob(&repo, b)?),
                    None => None,
                };
                planned.push(Planned {
                    path: p.clone(),
                    blob: oid.map(|o| o.to_string()),
                });
                blobs.push((p.clone(), oid));
            }
            let commit = git::commit(&repo, parent, &folder, &blobs, &signature, &message)?;
            Ok::<_, git::GitFailure>((commit.to_string(), planned))
        })
        .await
        .map_err(|_| TeamError::permanent("TEAM_CLONE_FAILED"))?
        .map_err(|_| TeamError::permanent("TEAM_CLONE_FAILED"))?;
        Ok(built)
    }

    // ---- §G ----

    pub(crate) async fn on_notice(&self, notice: PushNotice) {
        let share_id = match &notice {
            PushNotice::TeamShareSent { share_id, .. }
            | PushNotice::TeamShareMoved { share_id }
            | PushNotice::TeamShareFailed { share_id, .. }
            | PushNotice::TeamShareRejected { share_id } => *share_id,
            _ => return,
        };
        let Ok(pool) = self.pool().cloned() else {
            return;
        };
        let Ok(Some(share)) = store::share(&pool, share_id).await else {
            return;
        };
        let ws = share.workspace_id;
        let guard = self.workspace_lock(ws);
        let _held = guard.lock().await;
        let outcome = match notice {
            PushNotice::TeamShareSent {
                pull_request: None, ..
            } => self.share_sent(&share).await,
            PushNotice::TeamShareSent {
                pull_request: Some(pr),
                ..
            } => {
                async {
                    store::set_share_pull_request(&pool, share_id, &pr).await?;
                    store::set_share_state(&pool, share_id, "waiting_review", None).await?;
                    self.team_share_waiting(&share).await?;
                    self.changed(ws).await;
                    Ok(())
                }
                .await
            }
            PushNotice::TeamShareMoved { .. } => self.share_moved(&share).await,
            PushNotice::TeamShareFailed { code, .. } => {
                let mapped = match code {
                    "PUSH_SIGN_IN_NEEDED" => "TEAM_SIGN_IN_NEEDED",
                    "PUSH_FORBIDDEN" | "PUSH_NO_REVIEW_RIGHT" => "TEAM_READ_ONLY",
                    "PUSH_ORIGIN_UNREACHABLE" => "TEAM_REMOTE_UNREACHABLE",
                    "PUSH_OUTCOME_UNKNOWN" => "TEAM_SHARE_UNKNOWN",
                    _ => "TEAM_PUSH_FAILED",
                };
                tracing::warn!(target: "ordin::team_knowledge", share_id = share_id.to_string().as_str(), code = mapped, "team.share_failed");
                let r = store::set_share_state(&pool, share_id, "failed", Some(mapped)).await;
                if mapped == "TEAM_READ_ONLY" {
                    if let Ok(access) = self.access_of_ws(ws).await {
                        let _ = store::set_access(&pool, ws, access).await;
                    }
                }
                let _ = store::set_last_error(&pool, ws, Some(mapped)).await;
                self.team_share_failed(&share, mapped).await;
                self.changed(ws).await;
                r
            }
            PushNotice::TeamShareRejected { .. } => {
                let r = store::set_share_state(&pool, share_id, "rejected", None).await;
                self.team_share_rejected(&share).await;
                self.changed(ws).await;
                r
            }
            _ => Ok(()),
        };
        if let Err(e) = outcome {
            tracing::warn!(target: "ordin::team_knowledge", share_id = share_id.to_string().as_str(), code = e.code, "team.share_notice_failed");
        }
    }

    async fn access_of_ws(&self, ws: WorkspaceId) -> Result<TeamAccess, TeamError> {
        let b = self.bound(ws).await?;
        self.access_of(&b.host, &b.repository).await
    }

    /// §G step 1: the share is `done`; each planned path's record takes its new blob.
    pub(crate) async fn share_sent(&self, share: &store::Share) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        let ws = share.workspace_id;
        store::set_share_state(&pool, share.id, "done", None).await?;
        self.write_sent_records(share).await?;
        store::set_shared_at(&pool, ws).await?;
        self.journal(
            "shared",
            ws,
            "Team knowledge was shared.",
            serde_json::json!({ "workspace_id": ws, "share_id": share.id, "counts": share.counts }),
        )
        .await;
        tracing::info!(target: "ordin::team_knowledge", share_id = share.id.to_string().as_str(), "team.share_done");
        self.changed(ws).await;
        Ok(())
    }

    /// The records of a share that reached the branch.
    async fn write_sent_records(&self, share: &store::Share) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        let ws = share.workspace_id;
        let wanted: Vec<(String, Oid, u64)> = share
            .planned
            .iter()
            .filter_map(|p| {
                let oid = Oid::from_str(p.blob.as_deref()?).ok()?;
                Some((p.path.clone(), oid, files::TEAM_RESULT_MAX_BYTES))
            })
            .collect();
        let blobs = self
            .read_blobs(self.clone_path(ws), wanted)
            .await
            .unwrap_or_default();
        for p in &share.planned {
            match files::classify(&p.path) {
                Some(PathKind::Item { kind, stem }) if kind != ItemKind::Term => match &p.blob {
                    Some(blob) => {
                        let digest = blobs
                            .get(&p.path)
                            .and_then(|b| b.as_deref().map(sha256_hex))
                            .unwrap_or_else(|| "0".repeat(64));
                        let item_id = match kind {
                            ItemKind::Entity => format!("entity:{stem}"),
                            ItemKind::Source => format!("source:{stem}"),
                            ItemKind::Relationship => format!("relationship:{stem}"),
                            _ => stem,
                        };
                        store::put_record(
                            &pool,
                            ws,
                            &p.path,
                            &Record {
                                kind: kind.as_str().to_owned(),
                                item_id,
                                blob: blob.clone(),
                                local_sha256: digest,
                            },
                        )
                        .await?;
                    }
                    None => store::drop_record(&pool, ws, &p.path).await?,
                },
                _ => {}
            }
        }
        self.team_share_sent(share, &blobs).await?;
        Ok(())
    }

    /// §G step 3: the branch moved after the card; get, rebuild on the new tip, resend.
    async fn share_moved(&self, share: &store::Share) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        let ws = share.workspace_id;
        let moves = share.moves + 1;
        tracing::info!(target: "ordin::team_knowledge", share_id = share.id.to_string().as_str(), moves, "team.share_moved");
        if moves > MAX_MOVES {
            store::set_share_state(&pool, share.id, "failed", Some("TEAM_SHARE_RACE")).await?;
            store::set_last_error(&pool, ws, Some("TEAM_SHARE_RACE")).await?;
            self.changed(ws).await;
            return Ok(());
        }
        let (_, tip) = self.get_now(ws).await?;
        let b = self.bound(ws).await?;
        let name = self.person_name().await;
        let approved: BTreeSet<String> = share.planned.iter().map(|p| p.path.clone()).collect();
        let (plan, _) = self
            .plan_share(ws, &b, &tip, share.scope, &name, Some(&approved))
            .await?;
        let login = if self.team_row(ws).await?.is_some() {
            Some(self.login(&b.host).await?)
        } else {
            None
        };
        let (commit, planned) = self
            .build_commit(ws, &b, &tip, &plan, &name, login.as_deref())
            .await?;
        store::set_share_rebuilt(
            &pool,
            share.id,
            &oid_text(tip.commit),
            &commit,
            &planned,
            moves,
        )
        .await?;
        if let Err(e) = self.deps.push.resend_team_share(share.id, commit).await {
            store::set_share_state(&pool, share.id, "failed", Some("TEAM_PUSH_FAILED")).await?;
            return Err(TeamError::per_cause("TEAM_PUSH_FAILED", e.retryable));
        }
        Ok(())
    }

    /// §G steps 2 and 4: a check settles a share waiting for review (the branch holds every planned
    /// blob) and a share whose outcome was unknown (the branch contains its commit).
    async fn settle_shares(
        &self,
        ws: WorkspaceId,
        b: &Binding,
        tip: &Tip,
    ) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        let shares =
            store::shares_in(&pool, &["waiting_review", "approved", "sending", "failed"]).await?;
        for share in shares.into_iter().filter(|s| s.workspace_id == ws) {
            let holds = share.planned.iter().all(|p| {
                let remote = tip.files.get(&p.path).map(|o| o.to_string());
                remote == p.blob
            });
            let contains = {
                let path = self.clone_path(ws);
                let commit = share.commit_id.clone();
                let tip_oid = tip.commit;
                tokio::task::spawn_blocking(move || {
                    let repo = git2::Repository::open_bare(&path).ok()?;
                    let commit = Oid::from_str(&commit).ok()?;
                    Some(git::contains(&repo, tip_oid, commit))
                })
                .await
                .ok()
                .flatten()
                .unwrap_or(false)
            };
            let settle = match share.state.as_str() {
                "waiting_review" => holds,
                "approved" | "sending" => contains,
                "failed" => contains && share_unknown(&pool, share.id).await,
                _ => false,
            };
            if settle {
                let _ = b;
                self.share_sent(&share).await?;
            }
        }
        Ok(())
    }
}

async fn share_unknown(pool: &sqlx::SqlitePool, id: TeamShareId) -> bool {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT result_code FROM team_knowledge_shares WHERE id = ?1",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()
    .as_deref()
        == Some("TEAM_SHARE_UNKNOWN")
}

// spec: ordin-desktop/components/TeamKnowledgeService#SA-TEAM-06
/// §F step 3, both changed: `valid_to` = the earlier of two non-null values (null only when both
/// are), `superseded_by` from the side whose `valid_to` was taken (the remote side when equal), every
/// other field from the remote side.
pub(crate) fn merge_fact(mine: &TeamFact, theirs: TeamFact) -> TeamFact {
    let mut merged = theirs;
    match (mine.valid_to, merged.valid_to) {
        (Some(m), Some(r)) if m < r => {
            merged.valid_to = Some(m);
            merged.superseded_by = mine.superseded_by.clone();
        }
        (Some(m), None) => {
            merged.valid_to = Some(m);
            merged.superseded_by = mine.superseded_by.clone();
        }
        _ => {}
    }
    merged
}

/// The card's affected items for a share of knowledge (§E step 8).
pub(crate) fn knowledge_affects(plan: &Plan) -> Vec<AffectedItem> {
    text::count_labels(&plan.counts, &plan.changed_titles)
        .into_iter()
        .map(|label| AffectedItem::Record { label })
        .collect()
}
