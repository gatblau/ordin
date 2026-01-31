//! Curation reads (Parts of [display name], ADR-091), lifecycles for the Handover Service, and
//! the health report (§H, §N, §F).

use std::collections::BTreeMap;
use std::path::PathBuf;

use ordin_agent_model::kind_info;
use ordin_projects::GithubError;
use ordin_types::domain::agents::{
    Lifecycle, LifecycleStage, PartKind, PartStatus, VocabularyTerm, VocabularyTermState,
    WithdrawnListEntry,
};
use ordin_types::time::Timestamp;
use ordin_types::{AgentRepositoryId, HandoverPlaceId, LifecycleId, PartId, VocabularyTermId};
use regex::Regex;
use sqlx::Row;

use crate::db::{enum_from, enum_text, parse_timestamp, visible_parts_of, PartRecord};
use crate::error::CatalogueError;
use crate::library::PARTLY_MIN;
use crate::repos::github_error;
use crate::service::CatalogueService;
use crate::types::{
    axis_from_text, HandoverPlaceType, HealthLine, HealthReport, KindLine, LifecyclePlace,
    PullRequestStatus, RepositoryLayout, ReviewerInfo,
};

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredHealth {
    computed_at: String,
    lines: Vec<HealthLine>,
}

impl CatalogueService {
    pub(crate) fn remember_health(&self, id: AgentRepositoryId, lines: Vec<HealthLine>) {
        self.inner
            .pending_health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, lines);
    }

    /// Spawns the report of one ingest in the background; it blocks nothing (§D9).
    pub(crate) fn spawn_health(&self, id: AgentRepositoryId, at: String, commit: String) {
        let service = self.clone();
        tokio::spawn(async move {
            let ingest_lines = service
                .inner
                .pending_health
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&id)
                .cloned()
                .unwrap_or_default();
            match service.compute_health(id, ingest_lines).await {
                Ok(lines) => {
                    let stored = StoredHealth {
                        computed_at: Timestamp::now().to_wire(),
                        lines,
                    };
                    if let (Ok(pool), Ok(text)) = (service.pool(), serde_json::to_string(&stored)) {
                        let _ = sqlx::query("UPDATE repository_fetches SET health = ?3 WHERE agent_repository_id = ?1 AND at = ?2")
                            .bind(id.to_string())
                            .bind(at)
                            .bind(text)
                            .execute(pool)
                            .await;
                    }
                    service
                        .inner
                        .pending_health
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&id);
                }
                Err(error) => tracing::warn!(
                    component = "catalogue",
                    repository_id = %id,
                    commit = %commit,
                    code = error.code,
                    "catalogue.health_failed"
                ),
            }
        });
    }

    /// §H.
    pub(crate) async fn compute_health(
        &self,
        id: AgentRepositoryId,
        mut lines: Vec<HealthLine>,
    ) -> Result<Vec<HealthLine>, CatalogueError> {
        let pool = self.pool()?;
        let parts = visible_parts_of(pool, id).await?;
        let repo = id.to_string();
        // Unused parts: no incoming edge, and not an agent.
        for record in &parts {
            if record.part.kind == PartKind::WorkspaceProfile {
                continue;
            }
            let used =
                sqlx::query("SELECT 1 FROM part_dependencies WHERE uses_part_id = ?1 LIMIT 1")
                    .bind(record.part.id.to_string())
                    .fetch_optional(pool)
                    .await?
                    .is_some();
            if !used {
                lines.push(HealthLine::UnusedPart {
                    part_id: record.part.id,
                });
            }
        }
        // Look-alikes per kind.
        for record in &parts {
            let text = if record.summary.is_empty() {
                record.part.name.clone()
            } else {
                record.summary.clone()
            };
            if text.trim().is_empty() {
                continue;
            }
            let similar = self
                .similar_parts(record.part.kind, &text, Some(record.part.id))
                .await?;
            for other in similar.into_iter().take(3) {
                lines.push(HealthLine::LookAlike {
                    part_id: record.part.id,
                    similar_to: other.part.id,
                });
            }
        }
        // Shared identifiers across read repositories.
        let shared = sqlx::query(
            "SELECT DISTINCT p.identifier AS identifier, q.agent_repository_id AS other \
             FROM parts p JOIN part_index i ON i.part_id = p.id \
             JOIN parts q ON q.identifier = p.identifier AND q.agent_repository_id != p.agent_repository_id \
             JOIN part_index j ON j.part_id = q.id \
             JOIN agent_repositories r ON r.id = q.agent_repository_id \
             WHERE p.agent_repository_id = ?1 AND i.at_head = 1 AND j.at_head = 1 AND r.state IN ('named','cloned')",
        )
        .bind(&repo)
        .fetch_all(pool)
        .await?;
        for row in shared {
            let other: String = row.try_get("other")?;
            if let Ok(other) = uuid::Uuid::parse_str(&other) {
                lines.push(HealthLine::SharedIdentifier {
                    identifier: row.try_get("identifier")?,
                    other_repository_id: AgentRepositoryId(other),
                });
            }
        }
        // Parts used while deprecated or withdrawn.
        for record in &parts {
            let deprecated = record.part.status == PartStatus::Deprecated;
            if !(deprecated || record.part.withdrawn) {
                continue;
            }
            let users: Vec<PartId> = self.direct_users(record.part.id).await?;
            if users.is_empty() {
                continue;
            }
            if record.part.withdrawn {
                lines.push(HealthLine::UsedWhileWithdrawn {
                    part_id: record.part.id,
                    used_by: users.clone(),
                });
            }
            if deprecated {
                lines.push(HealthLine::UsedWhileDeprecated {
                    part_id: record.part.id,
                    used_by: users,
                });
            }
        }
        // Active terms no part cites.
        let terms = sqlx::query(
            "SELECT t.id AS id FROM vocabulary_terms t WHERE t.agent_repository_id = ?1 AND t.state = 'active' \
             AND NOT EXISTS (SELECT 1 FROM parts p JOIN part_index i ON i.part_id = p.id \
                             WHERE i.at_head = 1 AND instr(p.vocabulary_terms_cited, t.id) > 0)",
        )
        .bind(&repo)
        .fetch_all(pool)
        .await?;
        for term in terms {
            let text: String = term.try_get("id")?;
            if let Ok(term_id) = VocabularyTermId::parse(&text) {
                lines.push(HealthLine::UncitedTerm {
                    vocabulary_term_id: term_id,
                });
            }
        }
        let _ = PARTLY_MIN;
        Ok(lines)
    }

    async fn direct_users(&self, id: PartId) -> Result<Vec<PartId>, CatalogueError> {
        let rows = sqlx::query(
            "SELECT d.part_id AS id FROM part_dependencies d JOIN part_index i ON i.part_id = d.part_id \
             WHERE d.uses_part_id = ?1 AND i.at_head = 1 AND i.offered = 1",
        )
        .bind(id.to_string())
        .fetch_all(self.pool()?)
        .await?;
        rows.iter()
            .map(|r| {
                let text: String = r.try_get("id")?;
                PartId::parse(&text)
                    .map_err(|_| CatalogueError::store("a stored part id is not readable"))
            })
            .collect()
    }

    /// The report of the last ingest, computed now when none is stored.
    // spec: ordin-desktop/components/CatalogueService#ADR-091
    pub async fn health(&self, id: AgentRepositoryId) -> Result<HealthReport, CatalogueError> {
        let row = self.repo(id).await?;
        let stored = sqlx::query(
            "SELECT health, \"commit\" AS commit_sha FROM repository_fetches \
             WHERE agent_repository_id = ?1 AND health IS NOT NULL ORDER BY at DESC LIMIT 1",
        )
        .bind(id.to_string())
        .fetch_optional(self.pool()?)
        .await?;
        let commit = row.last_fetched_commit.clone().unwrap_or_default();
        if let Some(stored) = stored {
            let text: String = stored.try_get("health")?;
            if let Ok(parsed) = serde_json::from_str::<StoredHealth>(&text) {
                return Ok(HealthReport {
                    agent_repository_id: id,
                    computed_at: parse_timestamp(&parsed.computed_at)?,
                    commit: stored
                        .try_get::<Option<String>, _>("commit_sha")?
                        .unwrap_or(commit),
                    lines: parsed.lines,
                });
            }
        }
        let waiting = self
            .inner
            .pending_health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let lines = self.compute_health(id, waiting).await?;
        Ok(HealthReport {
            agent_repository_id: id,
            computed_at: Timestamp::now(),
            commit,
            lines,
        })
    }

    pub async fn withdrawn_list(
        &self,
        id: AgentRepositoryId,
    ) -> Result<Vec<WithdrawnListEntry>, CatalogueError> {
        self.repo(id).await?;
        self.withdrawn_entries_of(id).await
    }

    pub async fn vocabulary(
        &self,
        id: AgentRepositoryId,
    ) -> Result<Vec<VocabularyTerm>, CatalogueError> {
        self.repo(id).await?;
        let rows = sqlx::query(
            "SELECT id, vocabulary_part_id, axis, code, label, description, state FROM vocabulary_terms \
             WHERE agent_repository_id = ?1 ORDER BY axis, code",
        )
        .bind(id.to_string())
        .fetch_all(self.pool()?)
        .await?;
        let corrupt = || CatalogueError::store("a stored term is not readable");
        let mut out = Vec::new();
        for row in rows {
            let term: String = row.try_get("id")?;
            let part: String = row.try_get("vocabulary_part_id")?;
            let axis: String = row.try_get("axis")?;
            let state: String = row.try_get("state")?;
            out.push(VocabularyTerm {
                id: VocabularyTermId::parse(&term).map_err(|_| corrupt())?,
                agent_repository_id: id,
                vocabulary_part_id: PartId::parse(&part).map_err(|_| corrupt())?,
                axis: axis_from_text(&axis).ok_or_else(corrupt)?,
                code: row.try_get("code")?,
                label: row.try_get("label")?,
                description: row.try_get("description")?,
                state: match state.as_str() {
                    "active" => VocabularyTermState::Active,
                    _ => VocabularyTermState::Retired,
                },
                replacement_code: None,
            });
        }
        Ok(out)
    }

    pub async fn layout(&self, id: AgentRepositoryId) -> Result<RepositoryLayout, CatalogueError> {
        let row = self.repo(id).await?;
        let parts = visible_parts_of(self.pool()?, id).await?;
        let mut by_kind: BTreeMap<PartKind, Vec<&PartRecord>> = BTreeMap::new();
        for record in &parts {
            by_kind.entry(record.part.kind).or_default().push(record);
        }
        let mut kinds = Vec::new();
        for (kind, records) in by_kind {
            let first = &records[0].part.file_path;
            let directory = match kind_info(kind).kind_dir {
                Some(dir) => first
                    .find(&format!("/{dir}/"))
                    .map(|at| first[..at + 1 + dir.len()].to_owned())
                    .unwrap_or_else(|| parent(first)),
                None => parent(first),
            };
            let readme_path = if directory.is_empty() {
                "README.md".to_owned()
            } else {
                format!("{directory}/README.md")
            };
            let commit = records[0].part.commit.clone();
            let readme = self
                .read_file(&row, &commit, &readme_path)
                .await
                .ok()
                .flatten()
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            kinds.push(KindLine {
                kind,
                count: records.len() as u32,
                directory,
                readme,
            });
        }
        Ok(RepositoryLayout { kinds })
    }

    /// §N: who reviews a change to these paths.
    // spec: ordin-desktop/components/CatalogueService#C-67
    pub async fn reviewers_for(
        &self,
        id: AgentRepositoryId,
        paths: Vec<String>,
    ) -> Result<ReviewerInfo, CatalogueError> {
        let row = self.repo(id).await?;
        let rules = match self
            .inner
            .deps
            .github
            .branch_review_rules(&row.host, &row.repository, &row.protected_branch)
            .await
        {
            Ok(rules) => rules,
            // C-67 fallback: no admin right to read the rules.
            Err(GithubError::NotFound) => {
                return Ok(ReviewerInfo {
                    readable: false,
                    review_required: None,
                    reviewers: Vec::new(),
                })
            }
            Err(error) => return Err(github_error(error)),
        };
        let commit = row.last_fetched_commit.clone().unwrap_or_default();
        let mut owners_file: Option<String> = None;
        for candidate in [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"] {
            if let Ok(Some(bytes)) = self.read_file(&row, &commit, candidate).await {
                owners_file = Some(String::from_utf8_lossy(&bytes).into_owned());
                break;
            }
        }
        let mut reviewers: Vec<String> = Vec::new();
        if let Some(text) = owners_file {
            let rules = parse_codeowners(&text);
            for path in &paths {
                if let Some(owners) = rules
                    .iter()
                    .rev()
                    .find(|(matcher, _)| matcher.is_match(path))
                    .map(|(_, o)| o)
                {
                    for owner in owners {
                        if !reviewers.contains(owner) {
                            reviewers.push(owner.clone());
                        }
                    }
                }
            }
        }
        Ok(ReviewerInfo {
            readable: true,
            review_required: Some(rules.review_required),
            reviewers,
        })
    }

    pub async fn pull_request_status(
        &self,
        id: AgentRepositoryId,
        number: u64,
    ) -> Result<PullRequestStatus, CatalogueError> {
        let row = self.repo(id).await?;
        let github = &self.inner.deps.github;
        let view = github
            .pull_request(&row.host, &row.repository, number)
            .await
            .map_err(github_error)?;
        let merged_signed = match (&view.merge_commit, row.signed_merges_required) {
            (Some(commit), Some(true))
                if view.state == ordin_types::domain::agents::PullRequestState::Merged =>
            {
                Some(
                    github
                        .commit_verified(&row.host, &row.repository, commit)
                        .await
                        .map_err(github_error)?,
                )
            }
            _ => None,
        };
        Ok(PullRequestStatus {
            state: view.state,
            review: view.review,
            change_requests: view.change_requests,
            merged_signed,
        })
    }

    // ---- lifecycles ----

    async fn lifecycle_from(
        &self,
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<Lifecycle, CatalogueError> {
        let corrupt = || CatalogueError::store("a stored lifecycle is not readable");
        let id: String = row.try_get("id")?;
        let part: String = row.try_get("part_id")?;
        let repo: String = row.try_get("agent_repository_id")?;
        let stages: String = row.try_get("stages")?;
        let status: String = row.try_get("status")?;
        Ok(Lifecycle {
            id: LifecycleId::parse(&id).map_err(|_| corrupt())?,
            part_id: PartId::parse(&part).map_err(|_| corrupt())?,
            agent_repository_id: AgentRepositoryId::parse(&repo).map_err(|_| corrupt())?,
            identifier: row.try_get("identifier")?,
            name: row.try_get("name")?,
            stages: serde_json::from_str::<Vec<LifecycleStage>>(&stages).map_err(|_| corrupt())?,
            status: enum_from::<PartStatus>(&status).ok_or_else(corrupt)?,
            version: row.try_get("version")?,
        })
    }

    const LIFECYCLE_SELECT: &'static str = "SELECT l.id, l.part_id, l.agent_repository_id, l.identifier, \
        l.name, l.stages, l.status, l.version FROM lifecycles l JOIN part_index i ON i.part_id = l.part_id \
        JOIN agent_repositories r ON r.id = l.agent_repository_id \
        WHERE i.at_head = 1 AND i.offered = 1 AND r.state IN ('named','cloned')";

    pub async fn lifecycles(
        &self,
        id: Option<AgentRepositoryId>,
    ) -> Result<Vec<Lifecycle>, CatalogueError> {
        let mut sql = Self::LIFECYCLE_SELECT.to_owned();
        if id.is_some() {
            sql.push_str(" AND l.agent_repository_id = ?1");
        }
        sql.push_str(" ORDER BY l.name, l.identifier, l.version");
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        if let Some(id) = id {
            query = query.bind(id.to_string());
        }
        let rows = query.fetch_all(self.pool()?).await?;
        let mut out = Vec::new();
        for row in &rows {
            out.push(self.lifecycle_from(row).await?);
        }
        Ok(out)
    }

    pub async fn lifecycle(&self, id: LifecycleId) -> Result<Lifecycle, CatalogueError> {
        let sql = format!("{} AND l.id = ?1", Self::LIFECYCLE_SELECT);
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id.to_string())
            .fetch_optional(self.pool()?)
            .await?
            .ok_or_else(|| {
                CatalogueError::permanent(
                    "CAT_LIFECYCLE_UNKNOWN",
                    "no lifecycle has this identifier",
                )
            })?;
        self.lifecycle_from(&row).await
    }

    pub async fn lifecycle_place(
        &self,
        id: HandoverPlaceId,
    ) -> Result<LifecyclePlace, CatalogueError> {
        let row = sqlx::query("SELECT place_type, address FROM lifecycle_places WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(self.pool()?)
            .await?
            .ok_or_else(|| {
                CatalogueError::permanent("CAT_LIFECYCLE_UNKNOWN", "no place has this identifier")
            })?;
        let place_type: String = row.try_get("place_type")?;
        Ok(LifecyclePlace {
            id,
            place_type: enum_from::<HandoverPlaceType>(&place_type)
                .ok_or_else(|| CatalogueError::store("a stored place type is not readable"))?,
            address: row.try_get("address")?,
        })
    }
}

fn parent(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(d, _)| d.to_owned())
        .unwrap_or_default()
}

/// One `CODEOWNERS` pattern as a regular expression over a '/'-separated path.
fn pattern_regex(pattern: &str) -> Option<Regex> {
    let anchored = pattern.starts_with('/') || pattern.trim_end_matches('/').contains('/');
    let directory = pattern.ends_with('/');
    let body = pattern.trim_matches('/');
    let mut out = String::from(if anchored { "^" } else { "(^|.*/)" });
    let chars: Vec<char> = body.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        match chars[at] {
            '*' if chars.get(at + 1) == Some(&'*') => {
                out.push_str(".*");
                at += 2;
                if chars.get(at) == Some(&'/') {
                    at += 1;
                }
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        at += 1;
    }
    out.push_str(if directory { "/.*$" } else { "(/.*)?$" });
    Regex::new(&out).ok()
}

fn parse_codeowners(text: &str) -> Vec<(Regex, Vec<String>)> {
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or("").trim();
            let mut words = line.split_whitespace();
            let pattern = words.next()?;
            let owners: Vec<String> = words.map(str::to_owned).collect();
            Some((pattern_regex(pattern)?, owners))
        })
        .collect()
}

#[allow(dead_code)]
fn _unused(_: PathBuf) {
    let _ = enum_text::<PartKind>;
}
