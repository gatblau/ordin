//! Ingest of one commit, all or nothing (CatalogueService.md §D, §D0, §E, §F): the tree is read
//! through the format plug-in, checked part by part, its references resolved, and everything is
//! written in one transaction.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use ordin_agent_model::{
    markdown_body, relation_kind, ModelPart, PartKind, PartStatus, StagedPart, Urn, VocabularySet,
};
use ordin_checker::{CheckSubject, RepositoryCheck, Severity};
use ordin_format_plugins::{FormatError, FormatId, ReadRequest, SourceTree};
use ordin_types::domain::handover::HandoverPlaceType;
use ordin_types::{HandoverPlaceId, LifecycleId, PartId, VocabularyTermId, WithdrawnListEntryId};
use sqlx::{Row, Sqlite, Transaction};

use crate::db::{enum_text, now_text, RepoRow};
use crate::error::CatalogueError;
use crate::git::{self, TreeFailure};
use crate::lifecycle::{read_journey, LifeNote, LifecycleDraft, Reading};
use crate::partsource::Hit;
use crate::service::{repo_ref_of, CatalogueService};
use crate::types::{axis_text, CatalogueNotice, HealthLine};
use crate::withdrawn::{read_list, FileEntry};

/// Where the tree of a commit comes from.
pub(crate) enum TreeFrom {
    Commit(String),
    Folder(PathBuf),
}

/// How an ingest ended.
pub(crate) enum IngestEnd {
    Ingested {
        offered: u32,
        not_offered: u32,
        health: Vec<HealthLine>,
    },
    /// The commit is refused; the reason is a code of the Error Table.
    Refused(String),
    WithdrawnUnreadable,
}

/// One part read from the commit, with everything the index row needs (§D0).
struct Candidate {
    model: ModelPart,
    id: PartId,
    status: PartStatus,
    offered: bool,
    reason: Option<String>,
    summary: String,
    area: Option<String>,
    audience: Vec<String>,
    publisher: String,
    refs: Vec<(Option<PartKind>, String)>,
    files: Vec<String>,
    body: String,
    uses: Vec<PartId>,
    lifecycle: Option<(String, LifecycleDraft)>,
}

impl Candidate {
    fn identifier(&self) -> &str {
        &self.model.manifest.metadata.id
    }

    fn version(&self) -> &str {
        &self.model.manifest.metadata.version
    }

    fn mark(&mut self, reason: &str) {
        if self.offered {
            self.offered = false;
            self.reason = Some(reason.to_owned());
        }
    }
}

/// The first sentence of a description: up to the first `. ` or `.\n`, at most 300 characters.
fn first_sentence(text: &str) -> String {
    let cut = [". ", ".\n"]
        .iter()
        .filter_map(|m| text.find(m).map(|at| at + 1))
        .min()
        .unwrap_or(text.len());
    text[..cut].trim().chars().take(300).collect()
}

/// A publisher segment for minted identifiers: `[a-z][a-z0-9-]*`.
fn publisher_of(repository: &str) -> String {
    let owner = repository.split('/').next().unwrap_or("");
    let mut text: String = owner
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if !text.starts_with(|c: char| c.is_ascii_lowercase()) {
        text.insert(0, 'x');
    }
    text
}

fn valid_term(code: &str, label: &str, description: &str) -> Option<&'static str> {
    let code_ok = !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !code_ok {
        return Some("VOCAB_CODE_INVALID");
    }
    if label.is_empty() || label.chars().count() > 120 {
        return Some("VOCAB_LABEL_INVALID");
    }
    if description.chars().count() > 1000 {
        return Some("VOCAB_DESCRIPTION_TOO_LONG");
    }
    None
}

impl CatalogueService {
    /// §D. `commit` is the 40-hex commit, or `ordin-<version>` for the bundled catalogue.
    // spec: ordin-desktop/components/CatalogueService#SA-RECON-02
    // spec: ordin-desktop/components/CatalogueService#SA-RECON-03
    // spec: ordin-desktop/components/CatalogueService#ADR-091
    pub(crate) async fn ingest(
        &self,
        row: &RepoRow,
        commit: &str,
        from: TreeFrom,
    ) -> Result<IngestEnd, CatalogueError> {
        let started = Instant::now();
        let pool = self.pool()?.clone();
        let git_backed = matches!(from, TreeFrom::Commit(_));

        // §D1 and the read of the tree.
        let files = match from {
            TreeFrom::Commit(ref sha) => {
                let clone = PathBuf::from(&row.clone_path);
                let (branch, sha) = (row.protected_branch.clone(), sha.clone());
                let checkout = {
                    let (clone, branch, sha) = (clone.clone(), branch.clone(), sha.clone());
                    tokio::task::spawn_blocking(move || git::checkout_commit(&clone, &branch, &sha))
                        .await
                        .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?
                };
                if let Err(message) = checkout {
                    return Err(CatalogueError::fetch_failed(message));
                }
                let read = tokio::task::spawn_blocking(move || git::read_commit_tree(&clone, &sha))
                    .await
                    .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?;
                match read {
                    Ok(files) => files,
                    Err(TreeFailure::TooLarge) => {
                        return Ok(IngestEnd::Refused("CAT_REPOSITORY_TOO_LARGE".to_owned()))
                    }
                    Err(TreeFailure::Other(message)) => {
                        return Err(CatalogueError::fetch_failed(message))
                    }
                }
            }
            TreeFrom::Folder(ref root) => {
                let root = root.clone();
                let mut files = tokio::task::spawn_blocking(move || git::read_folder(&root))
                    .await
                    .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?
                    .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?;
                if !files.keys().any(|k| k.starts_with("models/")) {
                    files = files
                        .into_iter()
                        .map(|(k, v)| (format!("models/{k}"), v))
                        .collect();
                }
                files
            }
        };

        // §D2: the withdrawn list.
        let list: Vec<FileEntry> = if row.is_bundled() {
            Vec::new()
        } else {
            match files.get(ordin_agent_model::WITHDRAWN_LIST_FILE_NAME) {
                None => Vec::new(),
                Some(bytes) => match read_list(bytes) {
                    Ok(list) => list,
                    Err(code) => {
                        tracing::warn!(
                            component = "catalogue",
                            repository_id = %row.id,
                            commit,
                            reason_code = code,
                            "catalogue.withdrawn_unreadable"
                        );
                        return Ok(IngestEnd::WithdrawnUnreadable);
                    }
                },
            }
        };

        // §D3: detect, then read in two passes.
        let tree = SourceTree(files);
        let format = FormatId::from(row.format);
        let detection = match self.inner.deps.formats.detect(&tree) {
            Ok(found) => found.into_iter().find(|d| d.format == format),
            Err(FormatError::TreeTooLarge { .. }) => {
                return Ok(IngestEnd::Refused("CAT_REPOSITORY_TOO_LARGE".to_owned()))
            }
            Err(_) => None,
        };
        let Some(detection) = detection else {
            return Ok(IngestEnd::Refused("CAT_FORMAT_CHANGED".to_owned()));
        };
        let plugin = self.inner.deps.formats.get(format);
        let publisher = publisher_of(&row.repository);
        let first = match plugin.read(
            &tree,
            &ReadRequest {
                publisher: publisher.clone(),
                vocabulary: VocabularySet::default(),
                source_label: row.display_name.clone(),
            },
        ) {
            Ok(outcome) => outcome,
            Err(_) => return Ok(IngestEnd::Refused("CAT_FORMAT_CHANGED".to_owned())),
        };
        let vocabulary_parts: Vec<&ModelPart> = first
            .parts
            .iter()
            .filter(|p| p.kind == PartKind::Vocabulary)
            .collect();
        let Ok(vocabulary) = VocabularySet::from_parts(&vocabulary_parts) else {
            return Ok(IngestEnd::Refused("VOCAB_CODE_INVALID".to_owned()));
        };
        let outcome = match plugin.read(
            &tree,
            &ReadRequest {
                publisher,
                vocabulary,
                source_label: row.display_name.clone(),
            },
        ) {
            Ok(outcome) => outcome,
            Err(_) => return Ok(IngestEnd::Refused("CAT_FORMAT_CHANGED".to_owned())),
        };
        for item in &outcome.mapping_report.items {
            tracing::debug!(
                component = "catalogue",
                reason = item.reason.code(),
                "catalogue.mapping_item"
            );
        }
        let mapped: BTreeMap<String, &'static str> = outcome
            .mapping_report
            .items
            .iter()
            .filter_map(|i| i.part_identifier.clone().map(|id| (id, i.reason.code())))
            .collect();

        // §D4.
        if outcome.parts.len() > 2000 {
            return Ok(IngestEnd::Refused("CAT_REPOSITORY_TOO_LARGE".to_owned()));
        }

        // §D3a and §D5: index rows and the 2C rules.
        let existing = self.existing_part_ids(row).await?;
        let mut candidates: Vec<Candidate> = Vec::with_capacity(outcome.parts.len());
        let mut keys = BTreeSet::new();
        for model in outcome.parts {
            let metadata = &model.manifest.metadata;
            if !keys.insert((model.kind, metadata.id.clone(), metadata.version.clone())) {
                return Ok(IngestEnd::Refused("PART_DUPLICATE".to_owned()));
            }
            let id = existing
                .get(&(model.kind, metadata.id.clone(), metadata.version.clone()))
                .copied()
                .unwrap_or_default();
            let (status, status_error) = match model.manifest.status_or_default() {
                Ok(status) => (status, false),
                Err(_) => (PartStatus::Approved, true),
            };
            let manifest_name = model
                .manifest_path
                .rsplit('/')
                .next()
                .unwrap_or(&model.manifest_path)
                .to_owned();
            let body = model
                .files
                .0
                .get(&manifest_name)
                .and_then(|bytes| markdown_body(&model.manifest_path, bytes))
                .unwrap_or_default();
            let dir = model
                .manifest_path
                .rsplit_once('/')
                .map(|(d, _)| d.to_owned())
                .unwrap_or_default();
            let files = model
                .files
                .0
                .keys()
                .map(|k| {
                    if dir.is_empty() {
                        k.clone()
                    } else {
                        format!("{dir}/{k}")
                    }
                })
                .collect();
            let refs = model
                .manifest
                .references()
                .into_iter()
                .map(|r| (relation_kind(&r.relation), r.id))
                .collect();
            let publisher = Urn::parse(&metadata.id)
                .map(|u| u.publisher)
                .unwrap_or_else(|| metadata.owner.clone());
            let mut candidate = Candidate {
                id,
                status,
                offered: true,
                reason: None,
                summary: first_sentence(&metadata.description),
                area: (!metadata.category.is_empty()).then(|| metadata.category.clone()),
                audience: metadata.audience.roles.clone(),
                publisher,
                refs,
                files,
                body,
                uses: Vec::new(),
                lifecycle: None,
                model,
            };
            if status_error
                || ordin_agent_model::compare_versions(candidate.version(), candidate.version())
                    .is_err()
            {
                candidate.mark("PART_VERSION_INVALID");
            }
            if let Some(reason) = mapped.get(candidate.identifier()) {
                candidate.mark(reason);
            }
            if candidate.model.kind == PartKind::Vocabulary {
                if let Ok(Some(body)) = candidate.model.manifest.vocabulary() {
                    let mut codes = BTreeSet::new();
                    for term in &body.terms {
                        let rule =
                            valid_term(&term.code, &term.label, &term.description).or_else(|| {
                                (!codes.insert(term.code.clone())).then_some("VOCAB_CODE_INVALID")
                            });
                        if let Some(rule) = rule {
                            candidate.mark(rule);
                            break;
                        }
                    }
                }
            }
            candidates.push(candidate);
        }
        // `supersedes` names a part of the same repository and kind.
        let known: BTreeSet<(PartKind, String)> = candidates
            .iter()
            .map(|c| (c.model.kind, c.identifier().to_owned()))
            .chain(existing.keys().map(|(k, id, _)| (*k, id.clone())))
            .collect();
        for candidate in &mut candidates {
            if let Some(target) = candidate.model.manifest.metadata.supersedes.clone() {
                if !known.contains(&(candidate.model.kind, target)) {
                    candidate.mark("PART_SUPERSEDES_INVALID");
                }
            }
        }

        // §D6: the Checker, over every read part.
        let home = repo_ref_of(row);
        let label = row.label();
        let scope = self.source_scope(home.clone()).await?;
        let request = RepositoryCheck {
            repository: home.clone(),
            repository_display_name: row.display_name.clone(),
            scope: scope.clone(),
            changes: candidates
                .iter()
                .map(|c| StagedPart {
                    part: c.model.clone(),
                    repository: home.clone(),
                    repository_label: label.clone(),
                })
                .collect(),
            deletions: Vec::new(),
            source: None,
        };
        let subject = CheckSubject::RepositoryRead {
            repository: label.clone(),
            commit: commit.to_owned(),
        };
        let report = match self
            .inner
            .deps
            .checker
            .check_repository(&request, subject)
            .await
        {
            Ok(report) => report,
            Err(error) => {
                tracing::warn!(
                    component = "catalogue",
                    code = error.code(),
                    "catalogue.check_unavailable"
                );
                return Ok(IngestEnd::Refused("CAT_CHECK_UNAVAILABLE".to_owned()));
            }
        };
        for finding in report
            .findings
            .iter()
            // A malformed `slackChannel` is a Failure on a read, but the lifecycle stays offered and the
            // channel is dropped (spec §F), so that one finding does not withhold the part.
            .filter(|f| {
                f.severity == Severity::Failure && f.code != "lifecycle_slack_channel_invalid"
            })
        {
            match &finding.part_identifier {
                Some(id) => {
                    for candidate in candidates.iter_mut().filter(|c| c.identifier() == id) {
                        candidate.mark(&finding.code);
                    }
                }
                None => return Ok(IngestEnd::Refused("CAT_CHECK_FAILED".to_owned())),
            }
        }

        // §F: lifecycles.
        let mut health: Vec<HealthLine> = Vec::new();
        for candidate in candidates
            .iter_mut()
            .filter(|c| c.offered && c.model.kind == PartKind::Journey)
        {
            let name = candidate.model.manifest.metadata.name.clone();
            match read_journey(
                &candidate.model.manifest,
                &name,
                &candidate.model.manifest.metadata.id,
            ) {
                Reading::NotALifecycle => {}
                Reading::Offered(draft) => {
                    for note in &draft.notes {
                        if let LifeNote::SlackChannelIgnored { stage } = note {
                            health.push(HealthLine::LifecycleSlackChannelIgnored {
                                lifecycle: name.clone(),
                                stage: stage.clone(),
                            });
                        }
                    }
                    candidate.lifecycle = Some((name, draft));
                }
                Reading::NotOffered { code, notes } => {
                    for note in notes {
                        match note {
                            LifeNote::PlaceInvalid { stage } => {
                                health.push(HealthLine::LifecyclePlaceInvalid {
                                    lifecycle: name.clone(),
                                    stage,
                                })
                            }
                            LifeNote::SeamLoop => health.push(HealthLine::JourneyNotOffered {
                                part_id: candidate.id,
                            }),
                            LifeNote::SlackChannelIgnored { .. } => {}
                        }
                    }
                    candidate.mark(code);
                }
            }
        }

        // §D7: references become dependency edges.
        let steps = self.steps_of(&scope).await?;
        let outer_steps = if steps.len() > 1 {
            steps[1..].to_vec()
        } else {
            Vec::new()
        };
        let mut by_key: HashMap<(PartKind, String), Vec<usize>> = HashMap::new();
        let mut by_id: HashMap<String, Vec<usize>> = HashMap::new();
        for (at, c) in candidates.iter().enumerate() {
            by_key
                .entry((c.model.kind, c.identifier().to_owned()))
                .or_default()
                .push(at);
            by_id.entry(c.identifier().to_owned()).or_default().push(at);
        }
        let in_commit = |pool: &[usize], candidates: &[Candidate]| -> Option<usize> {
            pool.iter()
                .copied()
                .filter(|&at| candidates[at].status != PartStatus::Draft)
                .max_by(|&a, &b| {
                    crate::db::version_order(candidates[a].version(), candidates[b].version())
                })
        };
        let mut edges: Vec<(usize, Edge)> = Vec::new();
        let mut broken: Vec<(usize, String)> = Vec::new();
        let mut shared: Vec<(usize, String, ordin_types::AgentRepositoryId)> = Vec::new();
        for at in 0..candidates.len() {
            let refs = candidates[at].refs.clone();
            for (kind, id) in refs {
                let local_pool: Option<Vec<usize>> = match kind {
                    Some(kind) => by_key.get(&(kind, id.clone())).cloned(),
                    None => by_id.get(&id).cloned(),
                };
                if let Some(pool) = local_pool {
                    let kinds: BTreeSet<PartKind> =
                        pool.iter().map(|&p| candidates[p].model.kind).collect();
                    if kinds.len() == 1 {
                        if let Some(target) = in_commit(&pool, &candidates) {
                            edges.push((at, Edge::Local(target)));
                            continue;
                        }
                    }
                }
                match self.lookup_steps(&outer_steps, kind, &id, None).await? {
                    Hit::Found(record, _) => edges.push((at, Edge::Stored(record.part.id))),
                    Hit::NotFound => broken.push((at, id)),
                    Hit::Ambiguous(rows) => {
                        let other = rows.first().map(|r| r.id).unwrap_or(row.id);
                        shared.push((at, id, other));
                    }
                }
            }
        }
        for (at, id) in &broken {
            health.push(HealthLine::BrokenBorrowedReference {
                part_id: candidates[*at].id,
                reference: id.clone(),
            });
            candidates[*at].mark("BROKEN_BORROWED_REFERENCE");
        }
        for (at, id, other) in &shared {
            health.push(HealthLine::SharedIdentifier {
                identifier: id.clone(),
                other_repository_id: *other,
            });
            candidates[*at].mark("SHARED_IDENTIFIER");
        }
        // Any part whose `uses` closure holds a part that is not offered is not offered either.
        loop {
            let mut changed = false;
            for (at, edge) in &edges {
                if let Edge::Local(target) = edge {
                    if !candidates[*target].offered && candidates[*at].offered {
                        candidates[*at].mark("USES_NOT_OFFERED_PART");
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for (at, edge) in &edges {
            let target = match edge {
                Edge::Local(t) => candidates[*t].id,
                Edge::Stored(id) => *id,
            };
            if !candidates[*at].uses.contains(&target) {
                candidates[*at].uses.push(target);
            }
        }

        // §D8: one transaction.
        let offered = candidates.iter().filter(|c| c.offered).count() as u32;
        let not_offered = candidates.len() as u32 - offered;
        let result = self
            .write_commit(
                row,
                commit,
                git_backed,
                &detection.declared_version,
                &candidates,
                &list,
            )
            .await;
        let newly = match result {
            Ok(newly) => newly,
            Err(error) => {
                tracing::error!(
                    component = "catalogue",
                    repository_id = %row.id,
                    commit,
                    code = error.code,
                    "catalogue.ingest_failed"
                );
                return Err(error);
            }
        };
        // §D9.
        self.notify(CatalogueNotice::RepositoryUpdated {
            agent_repository_id: row.id,
            commit: commit.to_owned(),
            newly_withdrawn: newly,
        });
        let _ = pool;
        tracing::info!(
            component = "catalogue",
            repository_id = %row.id,
            commit,
            offered,
            not_offered,
            duration_ms = started.elapsed().as_millis() as u64,
            "catalogue.ingested"
        );
        Ok(IngestEnd::Ingested {
            offered,
            not_offered,
            health,
        })
    }

    /// The existing (kind, identifier, version) → part id of one repository.
    async fn existing_part_ids(
        &self,
        row: &RepoRow,
    ) -> Result<HashMap<(PartKind, String, String), PartId>, CatalogueError> {
        let rows = sqlx::query(
            "SELECT id, kind, identifier, version FROM parts WHERE agent_repository_id = ?1",
        )
        .bind(row.id.to_string())
        .fetch_all(self.pool()?)
        .await?;
        let mut out = HashMap::new();
        for r in rows {
            let id: String = r.try_get("id")?;
            let kind: String = r.try_get("kind")?;
            if let (Ok(id), Some(kind)) = (
                uuid::Uuid::parse_str(&id),
                crate::db::enum_from::<PartKind>(&kind),
            ) {
                out.insert(
                    (kind, r.try_get("identifier")?, r.try_get("version")?),
                    PartId(id),
                );
            }
        }
        Ok(out)
    }

    /// §D8 in one transaction; any SQL error rolls the whole commit back (`CAT_STORE_FAILED`).
    async fn write_commit(
        &self,
        row: &RepoRow,
        commit: &str,
        git_backed: bool,
        format_version: &str,
        candidates: &[Candidate],
        list: &[FileEntry],
    ) -> Result<Vec<WithdrawnListEntryId>, CatalogueError> {
        let profile_ids = self.profile_row_ids().await?;
        let mut tx = self.pool()?.begin_with("BEGIN IMMEDIATE").await?;
        let repo = row.id.to_string();
        let now = now_text();
        sqlx::query("UPDATE part_index SET at_head = 0 WHERE part_id IN (SELECT id FROM parts WHERE agent_repository_id = ?1)")
            .bind(&repo)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM part_search WHERE part_id IN (SELECT id FROM parts WHERE agent_repository_id = ?1)")
            .bind(&repo)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM part_dependencies WHERE part_id IN (SELECT id FROM parts WHERE agent_repository_id = ?1)")
            .bind(&repo)
            .execute(&mut *tx)
            .await?;
        let stored_commit = if git_backed {
            commit.to_owned()
        } else {
            format!("ordin-{}", self.inner.config.ordin_version)
        };
        for c in candidates {
            let metadata = &c.model.manifest.metadata;
            sqlx::query(
                "INSERT INTO parts (id, agent_repository_id, kind, identifier, name, version, status, \
                 supersedes, owner, file_path, \"commit\", content_digest, vocabulary_terms_cited, withdrawn) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'[]',0) \
                 ON CONFLICT(agent_repository_id, kind, identifier, version) DO UPDATE SET \
                 name = excluded.name, status = excluded.status, supersedes = excluded.supersedes, \
                 owner = excluded.owner, file_path = excluded.file_path, \"commit\" = excluded.\"commit\", \
                 content_digest = excluded.content_digest",
            )
            .bind(c.id.to_string())
            .bind(&repo)
            .bind(enum_text(&c.model.kind))
            .bind(&metadata.id)
            .bind(&metadata.name)
            .bind(&metadata.version)
            .bind(enum_text(&c.status))
            .bind(&metadata.supersedes)
            .bind(&metadata.owner)
            .bind(&c.model.manifest_path)
            .bind(&stored_commit)
            .bind(&c.model.content_digest.0)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO part_index (part_id, at_head, offered, not_offered_reason, summary, area, \
                 audience, publisher, files) VALUES (?1,1,?2,?3,?4,?5,?6,?7,?8) \
                 ON CONFLICT(part_id) DO UPDATE SET at_head = 1, offered = excluded.offered, \
                 not_offered_reason = excluded.not_offered_reason, summary = excluded.summary, \
                 area = excluded.area, audience = excluded.audience, publisher = excluded.publisher, \
                 files = excluded.files",
            )
            .bind(c.id.to_string())
            .bind(c.offered as i64)
            .bind(&c.reason)
            .bind(&c.summary)
            .bind(&c.area)
            .bind(json(&c.audience))
            .bind(&c.publisher)
            .bind(json(&c.files))
            .execute(&mut *tx)
            .await?;
            if c.offered {
                sqlx::query(
                    "INSERT INTO part_search (name, summary, body, part_id) VALUES (?1,?2,?3,?4)",
                )
                .bind(&metadata.name)
                .bind(&c.summary)
                .bind(&c.body)
                .bind(c.id.to_string())
                .execute(&mut *tx)
                .await?;
            }
        }
        for c in candidates.iter().filter(|c| c.offered) {
            for used in &c.uses {
                sqlx::query("INSERT OR IGNORE INTO part_dependencies (part_id, uses_part_id) VALUES (?1,?2)")
                    .bind(c.id.to_string())
                    .bind(used.to_string())
                    .execute(&mut *tx)
                    .await?;
            }
        }
        let newly = self.write_withdrawn(&mut tx, row, list, &now).await?;
        self.write_vocabulary(&mut tx, row, candidates).await?;
        self.write_lifecycles(&mut tx, row, candidates).await?;
        relink_citations(&mut tx, &profile_ids).await?;
        if git_backed {
            sqlx::query("UPDATE agent_repositories SET last_fetched_commit = ?2, state = 'cloned', format_version = ?3 WHERE id = ?1")
                .bind(&repo)
                .bind(commit)
                .bind(format_version)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query("UPDATE agent_repositories SET ordin_version = ?2, state = 'cloned', format_version = ?3 WHERE id = ?1")
                .bind(&repo)
                .bind(&self.inner.config.ordin_version)
                .bind(format_version)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(newly)
    }

    async fn write_withdrawn(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        row: &RepoRow,
        list: &[FileEntry],
        now: &str,
    ) -> Result<Vec<WithdrawnListEntryId>, CatalogueError> {
        let repo = row.id.to_string();
        let mut newly = Vec::new();
        for entry in list {
            let id = WithdrawnListEntryId::new();
            let done = sqlx::query(
                "INSERT OR IGNORE INTO withdrawn_list_entries (id, agent_repository_id, part_kind, \
                 part_identifier, part_version, part_content_digest, reason, withdrawn_at, \
                 replacement_identifier, replacement_version, read_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            )
            .bind(id.to_string())
            .bind(&repo)
            .bind(enum_text(&entry.kind))
            .bind(&entry.identifier)
            .bind(&entry.version)
            .bind(entry.digest.as_ref().map(|d| d.0.clone()))
            .bind(&entry.reason)
            .bind(entry.withdrawn_at.to_wire())
            .bind(entry.replacement.as_ref().map(|r| r.0.clone()))
            .bind(entry.replacement.as_ref().map(|r| r.1.clone()))
            .bind(now)
            .execute(&mut **tx)
            .await?;
            if done.rows_affected() == 1 {
                newly.push(id);
            }
        }
        // Entries the file no longer lists stay in force; seeing them gone is recorded once.
        let in_file: BTreeSet<(PartKind, String, String)> =
            list.iter().map(FileEntry::key).collect();
        let stored = sqlx::query(
            "SELECT id, part_kind, part_identifier, IFNULL(part_version, '') AS part_version, \
             removed_by_hand_seen_at FROM withdrawn_list_entries WHERE agent_repository_id = ?1",
        )
        .bind(&repo)
        .fetch_all(&mut **tx)
        .await?;
        for entry in stored {
            let seen: Option<String> = entry.try_get("removed_by_hand_seen_at")?;
            let kind: String = entry.try_get("part_kind")?;
            let Some(kind) = crate::db::enum_from::<PartKind>(&kind) else {
                continue;
            };
            let key = (
                kind,
                entry.try_get::<String, _>("part_identifier")?,
                entry.try_get::<String, _>("part_version")?,
            );
            if seen.is_none() && !in_file.contains(&key) {
                let id: String = entry.try_get("id")?;
                sqlx::query(
                    "UPDATE withdrawn_list_entries SET removed_by_hand_seen_at = ?2 WHERE id = ?1",
                )
                .bind(&id)
                .bind(now)
                .execute(&mut **tx)
                .await?;
                tracing::info!(
                    component = "catalogue",
                    repository_id = %row.id,
                    entry_id = %id,
                    "catalogue.withdrawn_entry_removed_by_hand"
                );
            }
            sqlx::query(
                "UPDATE parts SET withdrawn = 1 WHERE agent_repository_id = ?1 AND kind = ?2 \
                 AND identifier = ?3 AND (?4 = '' OR version = ?4)",
            )
            .bind(&repo)
            .bind(enum_text(&key.0))
            .bind(&key.1)
            .bind(&key.2)
            .execute(&mut **tx)
            .await?;
        }
        Ok(newly)
    }

    async fn write_vocabulary(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        row: &RepoRow,
        candidates: &[Candidate],
    ) -> Result<(), CatalogueError> {
        let repo = row.id.to_string();
        let mut listed: BTreeSet<(String, String)> = BTreeSet::new();
        for c in candidates
            .iter()
            .filter(|c| c.offered && c.model.kind == PartKind::Vocabulary)
        {
            let Ok(Some(body)) = c.model.manifest.vocabulary() else {
                continue;
            };
            let axis = axis_text(body.axis);
            for term in body.terms {
                listed.insert((axis.to_owned(), term.code.clone()));
                sqlx::query(
                    "INSERT INTO vocabulary_terms (id, agent_repository_id, vocabulary_part_id, axis, code, \
                     label, description, state) VALUES (?1,?2,?3,?4,?5,?6,?7,'active') \
                     ON CONFLICT(agent_repository_id, axis, code) DO UPDATE SET \
                     vocabulary_part_id = excluded.vocabulary_part_id, label = excluded.label, \
                     description = excluded.description, state = 'active'",
                )
                .bind(VocabularyTermId::new().to_string())
                .bind(&repo)
                .bind(c.id.to_string())
                .bind(axis)
                .bind(&term.code)
                .bind(&term.label)
                .bind(&term.description)
                .execute(&mut **tx)
                .await?;
            }
        }
        let stored = sqlx::query(
            "SELECT id, axis, code FROM vocabulary_terms WHERE agent_repository_id = ?1",
        )
        .bind(&repo)
        .fetch_all(&mut **tx)
        .await?;
        for term in stored {
            let key = (
                term.try_get::<String, _>("axis")?,
                term.try_get::<String, _>("code")?,
            );
            if !listed.contains(&key) {
                let id: String = term.try_get("id")?;
                sqlx::query("UPDATE vocabulary_terms SET state = 'retired' WHERE id = ?1")
                    .bind(id)
                    .execute(&mut **tx)
                    .await?;
            }
        }
        Ok(())
    }

    async fn write_lifecycles(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        row: &RepoRow,
        candidates: &[Candidate],
    ) -> Result<(), CatalogueError> {
        let repo = row.id.to_string();
        let mut current = Vec::new();
        for c in candidates.iter().filter(|c| c.offered) {
            let Some((name, draft)) = &c.lifecycle else {
                continue;
            };
            let mut stages = Vec::with_capacity(draft.stages.len());
            for stage in &draft.stages {
                let place = place_id(tx, &stage.place.0, &stage.place.1).await?;
                let mut stage = stage.stage.clone();
                stage.handover_place_id = place;
                stages.push(stage);
            }
            let part = c.id.to_string();
            current.push(part.clone());
            sqlx::query(
                "INSERT INTO lifecycles (id, part_id, agent_repository_id, identifier, name, stages, status, version) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(part_id) DO UPDATE SET \
                 identifier = excluded.identifier, name = excluded.name, stages = excluded.stages, \
                 status = excluded.status, version = excluded.version",
            )
            .bind(LifecycleId::new().to_string())
            .bind(&part)
            .bind(&repo)
            .bind(c.identifier())
            .bind(name)
            .bind(json(&stages))
            .bind(enum_text(&c.status))
            .bind(c.version())
            .execute(&mut **tx)
            .await?;
        }
        let stored = sqlx::query("SELECT part_id FROM lifecycles WHERE agent_repository_id = ?1")
            .bind(&repo)
            .fetch_all(&mut **tx)
            .await?;
        for entry in stored {
            let part: String = entry.try_get("part_id")?;
            if !current.contains(&part) {
                sqlx::query("DELETE FROM lifecycles WHERE part_id = ?1")
                    .bind(part)
                    .execute(&mut **tx)
                    .await?;
            }
        }
        Ok(())
    }
}

enum Edge {
    Local(usize),
    Stored(PartId),
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".to_owned())
}

/// One `HandoverPlaceId` per distinct (place type, address), minted once.
async fn place_id(
    tx: &mut Transaction<'_, Sqlite>,
    place_type: &HandoverPlaceType,
    address: &str,
) -> Result<HandoverPlaceId, CatalogueError> {
    let existing =
        sqlx::query("SELECT id FROM lifecycle_places WHERE address = ?1 AND place_type = ?2")
            .bind(address)
            .bind(enum_text(place_type))
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(found) = existing {
        let id: String = found.try_get("id")?;
        return HandoverPlaceId::parse(&id)
            .map_err(|_| CatalogueError::store("a stored place id is not readable"));
    }
    let id = HandoverPlaceId::new();
    sqlx::query("INSERT INTO lifecycle_places (id, place_type, address) VALUES (?1,?2,?3)")
        .bind(id.to_string())
        .bind(enum_text(place_type))
        .bind(address)
        .execute(&mut **tx)
        .await?;
    Ok(id)
}

/// Recomputes `vocabulary_terms_cited` of every head part from its category and audience roles:
/// the part's own repository's term first, then the other profile repositories in profile order,
/// then the bundled catalogue.
async fn relink_citations(
    tx: &mut Transaction<'_, Sqlite>,
    profile: &[String],
) -> Result<(), CatalogueError> {
    let terms = sqlx::query("SELECT id, agent_repository_id, axis, code FROM vocabulary_terms")
        .fetch_all(&mut **tx)
        .await?;
    if terms.is_empty() {
        return Ok(());
    }
    let mut by_key: HashMap<(String, String, String), String> = HashMap::new();
    for t in &terms {
        by_key.insert(
            (
                t.try_get("agent_repository_id")?,
                t.try_get("axis")?,
                t.try_get("code")?,
            ),
            t.try_get("id")?,
        );
    }
    let bundled = ordin_agent_model::BUNDLED_REPOSITORY_ID.to_string();
    let parts = sqlx::query(
        "SELECT p.id, p.agent_repository_id, p.vocabulary_terms_cited, i.area, i.audience \
         FROM parts p JOIN part_index i ON i.part_id = p.id WHERE i.at_head = 1",
    )
    .fetch_all(&mut **tx)
    .await?;
    for part in parts {
        let own: String = part.try_get("agent_repository_id")?;
        let area: Option<String> = part.try_get("area")?;
        let audience: String = part.try_get("audience")?;
        let roles: Vec<String> = serde_json::from_str(&audience).unwrap_or_default();
        let mut wanted: Vec<(&str, String)> = Vec::new();
        if let Some(area) = area {
            wanted.push(("category", area));
        }
        wanted.extend(roles.into_iter().map(|r| ("audience-role", r)));
        let order: Vec<&String> = std::iter::once(&own)
            .chain(profile.iter().filter(|p| **p != own))
            .chain(std::iter::once(&bundled).filter(|b| **b != own))
            .collect();
        let mut ids: Vec<String> = Vec::new();
        for (axis, code) in wanted {
            if let Some(id) = order
                .iter()
                .find_map(|repo| by_key.get(&((*repo).clone(), axis.to_owned(), code.clone())))
            {
                ids.push(id.clone());
            }
        }
        let text = json(&ids);
        let stored: String = part.try_get("vocabulary_terms_cited")?;
        if stored != text {
            let id: String = part.try_get("id")?;
            sqlx::query("UPDATE parts SET vocabulary_terms_cited = ?2 WHERE id = ?1")
                .bind(id)
                .bind(text)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}
