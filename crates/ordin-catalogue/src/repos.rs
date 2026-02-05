//! Repositories: start-up, reconciliation with the profile, the fetch loop, and Settings › Agent
//! repositories (CatalogueService.md §A, §B, §B2, §C, §J2, §K, §L2).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_agent_model::{RepositoryRef, SourceScope, BUNDLED_REPOSITORY_ID};
use ordin_format_plugins::FormatId;
use ordin_organisation_profile::{AgentCatalogueSection, AgentRepositoryEntry, RepositoryFormat};
use ordin_projects::{GithubError, RepoPermission};
use ordin_types::domain::agents::{
    AgentFormat, AgentRepository, AgentRepositoryState, PartKind, RepositorySource,
};
use ordin_types::time::Timestamp;
use ordin_types::AgentRepositoryId;
use sqlx::Row;
use tokio_util::sync::CancellationToken;

use crate::db::{all_repos, enum_text, now_text, parse_timestamp, read_repos, repo_by_id, RepoRow};
use crate::error::{CatalogueError, ALL_CODES};
use crate::git::{self, GitFailure};
use crate::ingest::{IngestEnd, TreeFrom};
use crate::service::{repo_ref_of, Backoff, CatalogueService, Probe};
use crate::types::{
    AddRepositoryInput, AgentRepositoryRef, CatalogueNotice, FetchOutcome, FetchResult,
    ProbeRepositoryInput, RenameRepositoryInput, RepositoryProbe, RepositorySummary,
    RepositorySyncOutcome, RepositorySyncReport, RepositorySyncResult,
};

const FETCH_INTERVAL: Duration = Duration::from_secs(15 * 60);
const PROBE_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// The most repositories read besides the bundled catalogue (Configuration, *Index limits*; A-42).
const MAX_REPOSITORIES: i64 = 10;

/// A code of the Error Table as a `&'static str`.
pub(crate) fn static_code(code: &str) -> &'static str {
    ALL_CODES
        .iter()
        .find(|c| **c == code)
        .copied()
        .unwrap_or("CAT_CHECK_FAILED")
}

/// How a failure to obtain credentials reads to the fetch.
pub(crate) fn credential_failure(error: GithubError) -> GitFailure {
    match error {
        GithubError::SignInNeeded => GitFailure::SignIn,
        GithubError::Unreachable | GithubError::RateLimited { .. } => {
            GitFailure::Unreachable("credentials unavailable".to_owned())
        }
        other => GitFailure::Other(format!("{other:?}")),
    }
}

/// The result of one clone-or-fetch-and-ingest of one repository.
pub(crate) struct FetchRun {
    pub result: FetchResult,
    pub commit: Option<String>,
    /// The Error Table code that makes `sync_profile_repositories` report `Failed`.
    pub code: Option<&'static str>,
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn valid_repository(repository: &str) -> bool {
    let mut parts = repository.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    // A part of `.` or `..` would climb out of the clones folder, and clone removal would then
    // delete the wrong directory.
    [owner, name].iter().all(|part| {
        (1..=100).contains(&part.len())
            && *part != "."
            && *part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
    })
}

fn format_of(format: RepositoryFormat) -> AgentFormat {
    match format {
        RepositoryFormat::Ordin => AgentFormat::Ordin,
        RepositoryFormat::Apm => AgentFormat::Apm,
    }
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl CatalogueService {
    fn section(&self) -> AgentCatalogueSection {
        self.inner
            .deps
            .profile
            .current()
            .document
            .agent_catalogue
            .clone()
    }

    /// §A: index the bundled catalogue, reconcile with the profile, start the loops.
    pub async fn start(&self, shutdown: CancellationToken) -> Result<(), CatalogueError> {
        self.pool()?;
        self.index_bundled().await?;
        self.reconcile().await?;
        let service = self.clone();
        let stop = shutdown.clone();
        tokio::spawn(async move {
            let mut changes = service.inner.deps.profile.subscribe();
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    changed = changes.changed() => {
                        if changed.is_err() { break; }
                        if let Err(error) = service.reconcile().await {
                            tracing::warn!(component = "catalogue", code = error.code, "catalogue.reconcile_failed");
                        }
                    }
                }
            }
        });
        let service = self.clone();
        tokio::spawn(async move {
            loop {
                service.tick().await;
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = tokio::time::sleep(FETCH_INTERVAL) => {}
                }
            }
        });
        Ok(())
    }

    /// §A1.
    async fn index_bundled(&self) -> Result<(), CatalogueError> {
        let pool = self.pool()?;
        let folder = self.inner.config.bundled_catalogue.clone();
        sqlx::query(
            "INSERT OR IGNORE INTO agent_repositories (id, display_name, host, repository, protected_branch, \
             format, format_version, source, signed_merges_required, person_can_publish, person_can_curate, \
             last_fetched_commit, profile_key, clone_path, state, added_at, ordin_version) \
             VALUES (?1,'ordin','bundled','ordin/catalogue','bundled','ordin','unknown','profile',0,0,0,NULL,NULL,?2,'cloned',?3,NULL)",
        )
        .bind(BUNDLED_REPOSITORY_ID.to_string())
        .bind(folder.to_string_lossy().into_owned())
        .bind(now_text())
        .execute(pool)
        .await?;
        let stored = sqlx::query(
            "SELECT ordin_version, bundled_digest FROM agent_repositories WHERE id = ?1",
        )
        .bind(BUNDLED_REPOSITORY_ID.to_string())
        .fetch_one(pool)
        .await?;
        let stored_version: Option<String> = stored.try_get("ordin_version")?;
        let stored_digest: Option<String> = stored.try_get("bundled_digest")?;
        let digest = {
            let folder = folder.clone();
            tokio::task::spawn_blocking(move || bundled_digest(&folder))
                .await
                .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?
        };
        // CAT-BND-1: the same version with the same files is not read again; other files are.
        // spec: ordin-desktop/components/CatalogueService#CAT-BND-1
        if stored_version.as_deref() == Some(self.inner.config.ordin_version.as_str())
            && stored_digest == digest
        {
            return Ok(());
        }
        let row = self.repo(BUNDLED_REPOSITORY_ID).await?;
        let commit = format!("ordin-{}", self.inner.config.ordin_version);
        match self.ingest(&row, &commit, TreeFrom::Folder(folder)).await? {
            IngestEnd::Ingested {
                offered,
                not_offered,
                ..
            } => {
                sqlx::query("UPDATE agent_repositories SET bundled_digest = ?2 WHERE id = ?1")
                    .bind(BUNDLED_REPOSITORY_ID.to_string())
                    .bind(digest)
                    .execute(pool)
                    .await?;
                tracing::info!(
                    component = "catalogue",
                    parts = offered + not_offered,
                    ordin_version = self.inner.config.ordin_version.as_str(),
                    "catalogue.bundled_indexed"
                );
                Ok(())
            }
            IngestEnd::Refused(code) => Err(CatalogueError::permanent(
                static_code(&code),
                "the bundled catalogue was refused",
            )),
            IngestEnd::WithdrawnUnreadable => Err(CatalogueError::permanent(
                "CAT_WITHDRAWN_LIST_UNREADABLE",
                "the bundled catalogue",
            )),
        }
    }

    /// §B.
    // spec: ordin-desktop/components/CatalogueService#ADR-086
    pub(crate) async fn reconcile(&self) -> Result<(), CatalogueError> {
        let _guard = self.inner.reconcile_lock.lock().await;
        let pool = self.pool()?;
        let section = self.section();
        let mut rows = all_repos(pool).await?;
        let mut named: Vec<AgentRepositoryId> = Vec::new();
        for entry in &section.repositories {
            if let Some(reason) = self.entry_invalid(entry) {
                tracing::warn!(
                    component = "catalogue",
                    reason_code = reason,
                    "catalogue.profile_entry_invalid"
                );
                continue;
            }
            let found = rows
                .iter()
                .find(|r| {
                    r.is_read()
                        && !r.is_bundled()
                        && same(&r.host, &entry.host)
                        && same(&r.repository, &entry.repository)
                })
                .cloned()
                .or_else(|| {
                    rows.iter()
                        .find(|r| {
                            r.state == AgentRepositoryState::Dropped
                                && same(&r.host, &entry.host)
                                && same(&r.repository, &entry.repository)
                        })
                        .cloned()
                });
            match found {
                Some(row) => {
                    let revived = row.state == AgentRepositoryState::Dropped;
                    sqlx::query(
                        "UPDATE agent_repositories SET display_name = ?2, profile_key = ?3, protected_branch = ?4, \
                         format = ?5, source = 'profile', signed_merges_required = ?6 \
                         WHERE id = ?1",
                    )
                    .bind(row.id.to_string())
                    .bind(&entry.display_name)
                    .bind(&entry.key)
                    .bind(&entry.protected_branch)
                    .bind(enum_text(&format_of(entry.format)))
                    .bind(entry.signed_merges_required as i64)
                    .execute(pool)
                    .await?;
                    if revived {
                        sqlx::query("UPDATE agent_repositories SET state = 'named', last_fetched_commit = NULL WHERE id = ?1")
                            .bind(row.id.to_string())
                            .execute(pool)
                            .await?;
                    }
                    named.push(row.id);
                }
                None => {
                    let id = AgentRepositoryId::new();
                    let clone = self.clone_path(&entry.host, &entry.repository);
                    sqlx::query(
                        "INSERT INTO agent_repositories (id, display_name, host, repository, protected_branch, \
                         format, format_version, source, signed_merges_required, person_can_publish, \
                         person_can_curate, last_fetched_commit, profile_key, clone_path, state, added_at) \
                         VALUES (?1,?2,?3,?4,?5,?6,'unknown','profile',?7,0,0,NULL,?8,?9,'named',?10)",
                    )
                    .bind(id.to_string())
                    .bind(&entry.display_name)
                    .bind(&entry.host)
                    .bind(&entry.repository)
                    .bind(&entry.protected_branch)
                    .bind(enum_text(&format_of(entry.format)))
                    .bind(entry.signed_merges_required as i64)
                    .bind(&entry.key)
                    .bind(clone.to_string_lossy().into_owned())
                    .bind(now_text())
                    .execute(pool)
                    .await?;
                    named.push(id);
                }
            }
            rows = all_repos(pool).await?;
        }
        for row in rows
            .iter()
            .filter(|r| r.source == RepositorySource::Profile && !r.is_bundled() && r.is_read())
        {
            if !named.contains(&row.id) {
                self.drop_repository(row, AgentRepositoryState::Dropped)
                    .await?;
            }
        }
        Ok(())
    }

    fn entry_invalid(&self, entry: &AgentRepositoryEntry) -> Option<&'static str> {
        if entry.key == "ordin" {
            return Some("CAT_PROFILE_KEY_RESERVED");
        }
        if !valid_host(&entry.host) {
            return Some("AREPO_HOST_UNSUPPORTED");
        }
        if !valid_repository(&entry.repository) {
            return Some("AREPO_NAME_INVALID");
        }
        if !git::valid_branch(&entry.protected_branch) {
            return Some("AREPO_BRANCH_INVALID");
        }
        None
    }

    /// §B3 and §K4: the repository's index rows and clone leave; the row stays in `state`.
    async fn drop_repository(
        &self,
        row: &RepoRow,
        state: AgentRepositoryState,
    ) -> Result<(), CatalogueError> {
        let pool = self.pool()?;
        let repo = row.id.to_string();
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let borrowed = sqlx::query(
            "SELECT 1 FROM part_dependencies d JOIN parts p ON p.id = d.part_id \
             JOIN parts q ON q.id = d.uses_part_id \
             WHERE q.agent_repository_id = ?1 AND p.agent_repository_id != ?1 LIMIT 1",
        )
        .bind(&repo)
        .fetch_optional(&mut *tx)
        .await?
        .is_some();
        let owned = "(SELECT id FROM parts WHERE agent_repository_id = ?1)";
        for sql in [
            format!("DELETE FROM part_search WHERE part_id IN {owned}"),
            "DELETE FROM lifecycles WHERE agent_repository_id = ?1".to_owned(),
            "DELETE FROM vocabulary_terms WHERE agent_repository_id = ?1".to_owned(),
            format!(
                "DELETE FROM part_dependencies WHERE part_id IN {owned} OR uses_part_id IN {owned}"
            ),
            format!("DELETE FROM part_index WHERE part_id IN {owned}"),
            "DELETE FROM parts WHERE agent_repository_id = ?1".to_owned(),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&repo)
                .execute(&mut *tx)
                .await?;
        }
        if !borrowed {
            sqlx::query("DELETE FROM withdrawn_list_entries WHERE agent_repository_id = ?1")
                .bind(&repo)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "UPDATE agent_repositories SET state = ?2, last_fetched_commit = NULL WHERE id = ?1",
        )
        .bind(&repo)
        .bind(enum_text(&state))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let clone = PathBuf::from(&row.clone_path);
        if clone.exists() {
            let _ = tokio::fs::remove_dir_all(&clone).await;
        }
        tracing::info!(
            component = "catalogue",
            repository_id = row.id.to_string().as_str(),
            source = enum_text(&row.source).as_str(),
            "catalogue.repository_dropped"
        );
        self.notify(CatalogueNotice::RepositoryDropped {
            agent_repository_id: row.id,
        });
        Ok(())
    }

    /// §B2.
    pub async fn sync_profile_repositories(&self) -> Result<RepositorySyncReport, CatalogueError> {
        self.reconcile().await?;
        let rows: Vec<RepoRow> = self
            .profile_rows()
            .await?
            .into_iter()
            .filter(|r| r.is_read())
            .collect();
        let mut results = Vec::new();
        for row in rows {
            let lock = self.lock_for(row.id);
            let _held = lock.lock().await;
            let Some(row) = repo_by_id(self.pool()?, row.id).await? else {
                continue;
            };
            let mut outcome_row = row.clone();
            let mut repaired = false;
            if row.state == AgentRepositoryState::Cloned
                && !git::clone_is_intact(&PathBuf::from(&row.clone_path))
            {
                let _ = tokio::fs::remove_dir_all(&row.clone_path).await;
                sqlx::query("UPDATE agent_repositories SET state = 'named', last_fetched_commit = NULL WHERE id = ?1")
                    .bind(row.id.to_string())
                    .execute(self.pool()?)
                    .await?;
                tracing::info!(
                    component = "catalogue",
                    repository_id = row.id.to_string().as_str(),
                    "catalogue.clone_repaired"
                );
                outcome_row.state = AgentRepositoryState::Named;
                outcome_row.last_fetched_commit = None;
                repaired = true;
            }
            let was_named = outcome_row.state == AgentRepositoryState::Named;
            let run = self.fetch_and_ingest(&outcome_row).await;
            let outcome = match run.code {
                Some(code) => RepositorySyncOutcome::Failed {
                    code: code.to_owned(),
                },
                None if repaired => RepositorySyncOutcome::Repaired,
                None if was_named => RepositorySyncOutcome::Cloned,
                None => RepositorySyncOutcome::Fetched,
            };
            results.push(RepositorySyncResult {
                display_name: row.display_name.clone(),
                outcome,
            });
        }
        Ok(RepositorySyncReport { results })
    }

    /// Profile rows in the profile's order, then any other profile row.
    pub(crate) async fn profile_rows(&self) -> Result<Vec<RepoRow>, CatalogueError> {
        let section = self.section();
        let mut rows: Vec<RepoRow> = all_repos(self.pool()?)
            .await?
            .into_iter()
            .filter(|r| r.source == RepositorySource::Profile && !r.is_bundled())
            .collect();
        let position = |row: &RepoRow| {
            section
                .repositories
                .iter()
                .position(|e| same(&e.host, &row.host) && same(&e.repository, &row.repository))
                .unwrap_or(usize::MAX)
        };
        rows.sort_by_key(position);
        Ok(rows)
    }

    /// One tick of the fetch loop (§C).
    async fn tick(&self) {
        let Ok(rows) = read_repos(match self.pool() {
            Ok(pool) => pool,
            Err(_) => return,
        })
        .await
        else {
            return;
        };
        for row in rows.into_iter().filter(|r| !r.is_bundled()) {
            {
                let mut backoff = self
                    .inner
                    .backoff
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let entry = backoff.entry(row.id).or_default();
                if entry.skip_ticks > 0 {
                    entry.skip_ticks -= 1;
                    continue;
                }
            }
            let lock = self.lock_for(row.id);
            let _held = lock.lock().await;
            let Ok(Some(row)) = repo_by_id(self.pool().expect("pool"), row.id).await else {
                continue;
            };
            if !row.is_read() {
                continue;
            }
            let run = self.fetch_and_ingest(&row).await;
            let mut backoff = self
                .inner
                .backoff
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let entry = backoff.entry(row.id).or_default();
            if run.result == FetchResult::Unreachable {
                entry.streak += 1;
                if entry.streak >= 3 {
                    // 15 minutes doubled for each further failure, at most 60 minutes.
                    let doublings = (entry.streak - 2).min(2);
                    entry.skip_ticks = (1u32 << doublings) - 1;
                }
            } else {
                *entry = Backoff::default();
            }
        }
    }

    /// §C1–§C5 for one row; the caller holds the repository's lock.
    pub(crate) async fn fetch_and_ingest(&self, row: &RepoRow) -> FetchRun {
        let started = Instant::now();
        let run = self.fetch_run(row).await;
        let at = self
            .record_fetch(row.id, &run.result, run.commit.as_deref())
            .await;
        tracing::info!(
            component = "catalogue",
            repository_id = row.id.to_string().as_str(),
            result = enum_text_of(&run.result).as_str(),
            commit = run.commit.as_deref().unwrap_or(""),
            duration_ms = started.elapsed().as_millis() as u64,
            "catalogue.fetch"
        );
        if let (FetchResult::Ingested { .. }, Some(at), Some(commit)) =
            (&run.result, at, &run.commit)
        {
            self.spawn_health(row.id, at, commit.clone());
        }
        run
    }

    // spec: ordin-desktop/components/CatalogueService#ADR-084
    // spec: ordin-desktop/components/CatalogueService#R-78
    async fn fetch_run(&self, row: &RepoRow) -> FetchRun {
        let refused = |code: &'static str| FetchRun {
            result: FetchResult::CommitRefused {
                reason_code: code.to_owned(),
            },
            commit: None,
            code: Some(code),
        };
        let github = Arc::clone(&self.inner.deps.github);
        let handle = tokio::runtime::Handle::current();
        let (host, repository, branch) = (
            row.host.clone(),
            row.repository.clone(),
            row.protected_branch.clone(),
        );
        let path = PathBuf::from(&row.clone_path);
        let fetched = tokio::task::spawn_blocking(move || {
            let credentials = || {
                handle
                    .block_on(github.git_credentials(&host))
                    .map(|c| (c.username, c.token.expose().to_owned()))
                    .map_err(credential_failure)
            };
            git::fetch_branch(
                &path,
                &git::remote_url(&host, &repository),
                &branch,
                &credentials,
            )
        })
        .await;
        let head = match fetched {
            Ok(Ok(head)) => head,
            Ok(Err(GitFailure::SignIn)) => {
                return FetchRun {
                    result: FetchResult::SignInNeeded,
                    commit: None,
                    code: Some("CAT_SIGN_IN_NEEDED"),
                }
            }
            Ok(Err(GitFailure::Unreachable(_))) => {
                return FetchRun {
                    result: FetchResult::Unreachable,
                    commit: None,
                    code: Some("CAT_UNREACHABLE"),
                }
            }
            Ok(Err(GitFailure::Other(_))) | Err(_) => return refused("CAT_FETCH_FAILED"),
        };
        if row.last_fetched_commit.as_deref() == Some(head.as_str()) {
            return FetchRun {
                result: FetchResult::Unchanged,
                commit: Some(head),
                code: None,
            };
        }
        if row.signed_merges_required == Some(true) {
            match self
                .inner
                .deps
                .github
                .commit_verified(&row.host, &row.repository, &head)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(
                        component = "catalogue",
                        repository_id = row.id.to_string().as_str(),
                        commit = head.to_string().as_str(),
                        "catalogue.merge_unsigned"
                    );
                    return FetchRun {
                        result: FetchResult::MergeUnsigned,
                        commit: Some(head),
                        code: None,
                    };
                }
                Err(GithubError::SignInNeeded) => {
                    return FetchRun {
                        result: FetchResult::SignInNeeded,
                        commit: None,
                        code: Some("CAT_SIGN_IN_NEEDED"),
                    }
                }
                Err(GithubError::Unreachable) => {
                    return FetchRun {
                        result: FetchResult::Unreachable,
                        commit: None,
                        code: Some("CAT_UNREACHABLE"),
                    }
                }
                Err(_) => return refused("CAT_FETCH_FAILED"),
            }
        }
        match self
            .ingest(row, &head, TreeFrom::Commit(head.clone()))
            .await
        {
            Ok(IngestEnd::Ingested {
                offered,
                not_offered,
                health,
            }) => {
                self.remember_health(row.id, health);
                self.refresh_permission(row).await;
                FetchRun {
                    result: FetchResult::Ingested {
                        offered,
                        not_offered,
                    },
                    commit: Some(head),
                    code: None,
                }
            }
            Ok(IngestEnd::WithdrawnUnreadable) => FetchRun {
                result: FetchResult::WithdrawnListUnreadable,
                commit: Some(head),
                code: None,
            },
            Ok(IngestEnd::Refused(code)) => FetchRun {
                result: FetchResult::CommitRefused { reason_code: code },
                commit: Some(head),
                code: None,
            },
            Err(error) => FetchRun {
                result: FetchResult::CommitRefused {
                    reason_code: error.code.to_owned(),
                },
                commit: Some(head),
                code: Some(error.code),
            },
        }
    }

    /// Keeps what the profile row's permission says (best effort; a failure changes nothing).
    async fn refresh_permission(&self, row: &RepoRow) {
        if row.source != RepositorySource::Profile {
            return;
        }
        if let Ok(permission) = self
            .inner
            .deps
            .github
            .repository_permission(&row.host, &row.repository)
            .await
        {
            let can = permission >= RepoPermission::Write;
            if let Ok(pool) = self.pool() {
                let _ = sqlx::query("UPDATE agent_repositories SET person_can_publish = ?2, person_can_curate = ?2 WHERE id = ?1")
                    .bind(row.id.to_string())
                    .bind(can as i64)
                    .execute(pool)
                    .await;
            }
        }
    }

    /// Records one outcome in `repository_fetches` and returns its time.
    async fn record_fetch(
        &self,
        id: AgentRepositoryId,
        result: &FetchResult,
        commit: Option<&str>,
    ) -> Option<String> {
        let pool = self.pool().ok()?;
        let text = serde_json::to_string(result).ok()?;
        let mut at = Timestamp::now();
        for _ in 0..50 {
            let done = sqlx::query(
                "INSERT INTO repository_fetches (agent_repository_id, at, result, \"commit\", health) \
                 VALUES (?1,?2,?3,?4,NULL)",
            )
            .bind(id.to_string())
            .bind(at.to_wire())
            .bind(&text)
            .bind(commit)
            .execute(pool)
            .await;
            match done {
                Ok(_) => return Some(at.to_wire()),
                Err(_) => {
                    at = Timestamp::from_unix_millis(at.unix_millis() + 1).ok()?;
                }
            }
        }
        None
    }

    // ---- Settings › Agent repositories ----

    pub async fn list_repositories(&self) -> Result<Vec<RepositorySummary>, CatalogueError> {
        let pool = self.pool()?;
        let mut out = Vec::new();
        for row in read_repos(pool).await? {
            let count: i64 = sqlx::query(
                "SELECT COUNT(*) AS n FROM parts p JOIN part_index i ON i.part_id = p.id \
                 WHERE p.agent_repository_id = ?1 AND p.kind = 'WorkspaceProfile' AND i.at_head = 1 AND i.offered = 1",
            )
            .bind(row.id.to_string())
            .fetch_one(pool)
            .await?
            .try_get("n")?;
            let last = sqlx::query(
                "SELECT at, result, \"commit\" AS commit_sha FROM repository_fetches \
                 WHERE agent_repository_id = ?1 ORDER BY at DESC LIMIT 1",
            )
            .bind(row.id.to_string())
            .fetch_optional(pool)
            .await?;
            let last_fetch = match last {
                Some(r) => {
                    let result: String = r.try_get("result")?;
                    let at: String = r.try_get("at")?;
                    serde_json::from_str::<FetchResult>(&result).ok().map(
                        |result| -> Result<FetchOutcome, CatalogueError> {
                            Ok(FetchOutcome {
                                agent_repository_id: row.id,
                                result,
                                commit: r.try_get("commit_sha")?,
                                at: parse_timestamp(&at)?,
                            })
                        },
                    )
                }
                None => None,
            }
            .transpose()?;
            out.push(RepositorySummary {
                repository: row.public(),
                agent_count: count as u32,
                set_by_company: row.source == RepositorySource::Profile,
                last_fetch,
            });
        }
        Ok(out)
    }

    pub async fn fetch_now(
        &self,
        id: Option<AgentRepositoryId>,
    ) -> Result<Vec<FetchOutcome>, CatalogueError> {
        let rows: Vec<RepoRow> = match id {
            Some(id) => {
                let row = self.repo(id).await?;
                if !row.is_read() || row.is_bundled() {
                    return Err(CatalogueError::repository_unknown());
                }
                vec![row]
            }
            None => read_repos(self.pool()?)
                .await?
                .into_iter()
                .filter(|r| !r.is_bundled())
                .collect(),
        };
        let mut out = Vec::new();
        for row in rows {
            let lock = self.lock_for(row.id);
            let _held = lock.lock().await;
            let Some(row) = repo_by_id(self.pool()?, row.id).await? else {
                continue;
            };
            let run = self.fetch_and_ingest(&row).await;
            out.push(FetchOutcome {
                agent_repository_id: row.id,
                result: run.result,
                commit: run.commit,
                at: Timestamp::now(),
            });
        }
        Ok(out)
    }

    /// §K1.
    pub async fn probe_repository(
        &self,
        input: ProbeRepositoryInput,
    ) -> Result<RepositoryProbe, CatalogueError> {
        if input.web_address.len() > 2048 {
            return Err(CatalogueError::name_invalid(
                "the address is longer than 2,048 bytes",
            ));
        }
        let url = url::Url::parse(input.web_address.trim())
            .map_err(|_| CatalogueError::name_invalid("not an address"))?;
        if url.scheme() != "https" {
            return Err(CatalogueError::name_invalid("not an https address"));
        }
        let host = url
            .host_str()
            .ok_or_else(|| CatalogueError::name_invalid("no host"))?
            .to_ascii_lowercase();
        let segments: Vec<&str> = url.path().trim_matches('/').split('/').collect();
        let [owner, name] = segments.as_slice() else {
            return Err(CatalogueError::name_invalid("not owner/name"));
        };
        let name = name.strip_suffix(".git").unwrap_or(name);
        let repository = format!("{owner}/{name}");
        if !valid_repository(&repository) || !valid_host(&host) {
            return Err(CatalogueError::name_invalid("not owner/name"));
        }
        let section = self.section();
        if host != "github.com" && !section.repositories.iter().any(|e| same(&e.host, &host)) {
            return Err(CatalogueError::permanent(
                "AREPO_HOST_UNSUPPORTED",
                "only GitHub hosts can be added",
            ));
        }
        let rows = all_repos(self.pool()?).await?;
        if rows
            .iter()
            .any(|r| r.is_read() && same(&r.host, &host) && same(&r.repository, &repository))
        {
            return Err(CatalogueError::permanent(
                "AREPO_DUPLICATE",
                "the repository is already read",
            ));
        }
        if !section.person_may_add {
            return Err(CatalogueError::add_disabled());
        }
        let github = &self.inner.deps.github;
        let permission = github
            .repository_permission(&host, &repository)
            .await
            .map_err(github_error)?;
        if permission == RepoPermission::None {
            return Err(CatalogueError::permanent(
                "CAT_NO_READ_ACCESS",
                "no read access",
            ));
        }
        let branch = github
            .default_branch(&host, &repository)
            .await
            .map_err(github_error)?;
        let probe_id = uuid::Uuid::now_v7();
        let path = self
            .inner
            .config
            .root
            .join("probes")
            .join(probe_id.to_string());
        if let Some(parent) = path.parent() {
            git::create_private_folder(parent)
                .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?;
        }
        let handle = tokio::runtime::Handle::current();
        let (github2, host2, repo2, branch2, path2) = (
            Arc::clone(github),
            host.clone(),
            repository.clone(),
            branch.clone(),
            path.clone(),
        );
        let fetched = tokio::task::spawn_blocking(move || {
            let credentials = || {
                handle
                    .block_on(github2.git_credentials(&host2))
                    .map(|c| (c.username, c.token.expose().to_owned()))
                    .map_err(credential_failure)
            };
            let head = git::fetch_branch(
                &path2,
                &git::remote_url(&host2, &repo2),
                &branch2,
                &credentials,
            )?;
            git::read_commit_tree(&path2, &head).map_err(|e| match e {
                git::TreeFailure::TooLarge => GitFailure::Other("tree too large".to_owned()),
                git::TreeFailure::Other(m) => GitFailure::Other(m),
            })
        })
        .await
        .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?;
        let cleanup = |path: &PathBuf| {
            let _ = std::fs::remove_dir_all(path);
        };
        let files = match fetched {
            Ok(files) => files,
            Err(failure) => {
                cleanup(&path);
                return Err(git_failure_error(failure));
            }
        };
        let detected = self
            .inner
            .deps
            .formats
            .detect(&ordin_format_plugins::SourceTree(files));
        let (format, agent_count) = match detected {
            Ok(found) => match found
                .iter()
                .find_map(|d| d.format.as_repository_format().map(|f| (f, d.agent_count)))
            {
                Some(found) => found,
                None => {
                    cleanup(&path);
                    return Err(CatalogueError::permanent(
                        "CAT_FORMAT_UNKNOWN",
                        "no agent format recognised",
                    ));
                }
            },
            Err(_) => {
                cleanup(&path);
                return Err(CatalogueError::permanent(
                    "CAT_FORMAT_UNKNOWN",
                    "no agent format recognised",
                ));
            }
        };
        let can_publish = permission >= RepoPermission::Write;
        let expires_at = Timestamp::from_unix_millis(
            Timestamp::now().unix_millis() + PROBE_LIFETIME.as_millis() as i64,
        )
        .map_err(|_| CatalogueError::store("the clock is out of range"))?;
        self.inner
            .probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                probe_id,
                Probe {
                    host: host.clone(),
                    repository: repository.clone(),
                    branch,
                    format,
                    path: path.clone(),
                    can_publish,
                    expires: Instant::now() + PROBE_LIFETIME,
                },
            );
        let service = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(PROBE_LIFETIME).await;
            service.expire_probe(probe_id).await;
        });
        Ok(RepositoryProbe {
            probe_id,
            host,
            repository,
            suggested_name: name.to_owned(),
            format,
            agent_count,
            person_can_publish: can_publish,
            expires_at,
        })
    }

    async fn expire_probe(&self, id: uuid::Uuid) {
        let probe = self
            .inner
            .probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        if let Some(probe) = probe {
            let _ = tokio::fs::remove_dir_all(&probe.path).await;
        }
    }

    /// §K2.
    pub async fn add_repository(
        &self,
        input: AddRepositoryInput,
    ) -> Result<AgentRepository, CatalogueError> {
        let probe = {
            let probes = self
                .inner
                .probes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            probes.get(&input.probe_id).cloned()
        };
        let Some(probe) = probe.filter(|p| p.expires > Instant::now()) else {
            self.expire_probe(input.probe_id).await;
            return Err(CatalogueError::permanent(
                "CAT_PROBE_EXPIRED",
                "the probe is unknown or expired",
            ));
        };
        let name = input.display_name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
            return Err(CatalogueError::permanent(
                "CAT_NAME_INVALID",
                "the display name is not 1-120 characters",
            ));
        }
        if !self.section().person_may_add {
            return Err(CatalogueError::add_disabled());
        }
        // spec: ordin-desktop/components/CatalogueService#CAT-LIM-1
        // Profile and person rows count alike; the probe is left in place so the person can add it
        // after removing one of their own.
        let read: i64 = sqlx::query(
            "SELECT COUNT(*) AS n FROM agent_repositories \
             WHERE state IN ('named','cloned') AND id <> ?1",
        )
        .bind(BUNDLED_REPOSITORY_ID.to_string())
        .fetch_one(self.pool()?)
        .await?
        .try_get("n")?;
        if read >= MAX_REPOSITORIES {
            return Err(CatalogueError::permanent(
                "CAT_REPOSITORY_LIMIT",
                format!("{read} agent repositories are already read"),
            ));
        }
        let clone = self.clone_path(&probe.host, &probe.repository);
        if clone.exists() {
            let _ = tokio::fs::remove_dir_all(&clone).await;
        }
        if let Some(parent) = clone.parent() {
            git::create_private_folder(parent)
                .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?;
        }
        tokio::fs::rename(&probe.path, &clone)
            .await
            .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?;
        self.inner
            .probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&input.probe_id);
        let id = AgentRepositoryId::new();
        sqlx::query(
            "INSERT INTO agent_repositories (id, display_name, host, repository, protected_branch, format, \
             format_version, source, signed_merges_required, person_can_publish, person_can_curate, \
             last_fetched_commit, profile_key, clone_path, state, added_at) \
             VALUES (?1,?2,?3,?4,?5,?6,'unknown','person',NULL,?7,?7,NULL,NULL,?8,'cloned',?9)",
        )
        .bind(id.to_string())
        .bind(&name)
        .bind(&probe.host)
        .bind(&probe.repository)
        .bind(&probe.branch)
        .bind(enum_text(&probe.format))
        .bind(probe.can_publish as i64)
        .bind(clone.to_string_lossy().into_owned())
        .bind(now_text())
        .execute(self.pool()?)
        .await?;
        tracing::info!(
            component = "catalogue",
            repository_id = id.to_string().as_str(),
            source = "person",
            "catalogue.repository_added"
        );
        let lock = self.lock_for(id);
        let _held = lock.lock().await;
        let row = self.repo(id).await?;
        let run = self.fetch_and_ingest(&row).await;
        if let Some(code) = run.code {
            tracing::warn!(
                component = "catalogue",
                repository_id = id.to_string().as_str(),
                code,
                "catalogue.add_ingest_incomplete"
            );
        }
        Ok(self.repo(id).await?.public())
    }

    pub async fn rename_repository(
        &self,
        input: RenameRepositoryInput,
    ) -> Result<AgentRepository, CatalogueError> {
        let row = self.repo(input.id).await?;
        if row.source != RepositorySource::Person || !row.is_read() {
            return Err(CatalogueError::permanent(
                "AREPO_REMOVE_NOT_OWN",
                "the repository is set by the company",
            ));
        }
        let name = input.display_name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
            return Err(CatalogueError::permanent(
                "CAT_NAME_INVALID",
                "the display name is not 1-120 characters",
            ));
        }
        sqlx::query("UPDATE agent_repositories SET display_name = ?2 WHERE id = ?1")
            .bind(input.id.to_string())
            .bind(&name)
            .execute(self.pool()?)
            .await?;
        Ok(self.repo(input.id).await?.public())
    }

    pub async fn remove_repository(&self, id: AgentRepositoryId) -> Result<(), CatalogueError> {
        let row = self.repo(id).await?;
        if row.source != RepositorySource::Person {
            return Err(CatalogueError::permanent(
                "AREPO_REMOVE_NOT_OWN",
                "the repository is set by the company",
            ));
        }
        let lock = self.lock_for(id);
        let _held = lock.lock().await;
        self.drop_repository(&row, AgentRepositoryState::Removed)
            .await?;
        tracing::info!(
            component = "catalogue",
            repository_id = id.to_string().as_str(),
            source = "person",
            "catalogue.repository_removed"
        );
        Ok(())
    }

    // ---- lookups for the Package Catalogue (§J2) and §L2 ----

    pub async fn repository_by_ref(
        &self,
        reference: &AgentRepositoryRef,
    ) -> Result<Option<AgentRepository>, CatalogueError> {
        let rows = read_repos(self.pool()?).await?;
        let found = match reference {
            AgentRepositoryRef::ProfileKey(key) if key == "ordin" => {
                rows.into_iter().find(|r| r.is_bundled())
            }
            AgentRepositoryRef::ProfileKey(key) => rows
                .into_iter()
                .find(|r| r.profile_key.as_deref() == Some(key.as_str())),
            AgentRepositoryRef::Address(address) => {
                let url = url::Url::parse(address)
                    .map_err(|_| CatalogueError::name_invalid("not an address"))?;
                let host = url
                    .host_str()
                    .ok_or_else(|| CatalogueError::name_invalid("no host"))?;
                let path = url.path().trim_matches('/');
                let path = path.strip_suffix(".git").unwrap_or(path);
                if !valid_repository(path) {
                    return Err(CatalogueError::name_invalid("not owner/name"));
                }
                rows.into_iter()
                    .find(|r| !r.is_bundled() && same(&r.host, host) && same(&r.repository, path))
            }
        };
        Ok(found.map(|r| r.public()))
    }

    pub async fn repository_ref(
        &self,
        id: AgentRepositoryId,
    ) -> Result<AgentRepositoryRef, CatalogueError> {
        let row = self.repo(id).await?;
        if row.is_bundled() {
            return Ok(AgentRepositoryRef::ProfileKey("ordin".to_owned()));
        }
        Ok(match row.profile_key {
            Some(key) => AgentRepositoryRef::ProfileKey(key),
            None => AgentRepositoryRef::Address(format!("https://{}/{}", row.host, row.repository)),
        })
    }

    /// §L2.
    // spec: ordin-desktop/components/CatalogueService#ADR-090
    pub async fn source_scope(&self, home: RepositoryRef) -> Result<SourceScope, CatalogueError> {
        if let RepositoryRef::Repository(id) = &home {
            let row = repo_by_id(self.pool()?, *id).await?;
            if !row.is_some_and(|r| r.is_read()) {
                return Err(CatalogueError::repository_unknown());
            }
        }
        let home_id = match &home {
            RepositoryRef::Repository(id) => *id,
            RepositoryRef::Bundled => BUNDLED_REPOSITORY_ID,
        };
        let profile = self
            .profile_rows()
            .await?
            .into_iter()
            .filter(|r| r.is_read() && r.id != home_id)
            .map(|r| RepositoryRef::Repository(r.id))
            .collect();
        Ok(SourceScope {
            home,
            profile,
            include_bundled: true,
        })
    }

    /// The ids of the profile repositories in profile order (for citations).
    pub(crate) async fn profile_row_ids(&self) -> Result<Vec<String>, CatalogueError> {
        Ok(self
            .profile_rows()
            .await?
            .into_iter()
            .filter(|r| r.is_read())
            .map(|r| r.id.to_string())
            .collect())
    }
}

pub(crate) fn github_error(error: GithubError) -> CatalogueError {
    match error {
        GithubError::SignInNeeded => CatalogueError::sign_in_needed(),
        GithubError::NotFound => CatalogueError::permanent("CAT_NO_READ_ACCESS", "no read access"),
        GithubError::Unreachable | GithubError::RateLimited { .. } => {
            CatalogueError::unreachable("GitHub did not answer")
        }
        other => CatalogueError::fetch_failed(format!("{other:?}")),
    }
}

fn git_failure_error(failure: GitFailure) -> CatalogueError {
    match failure {
        GitFailure::SignIn => CatalogueError::sign_in_needed(),
        GitFailure::Unreachable(detail) => CatalogueError::unreachable(detail),
        GitFailure::Other(detail) => CatalogueError::fetch_failed(detail),
    }
}

fn enum_text_of(result: &FetchResult) -> String {
    serde_json::to_value(result)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str().map(str::to_owned)))
        .unwrap_or_default()
}

#[allow(dead_code)]
fn _kinds(_: PartKind, _: FormatId) {
    let _ = repo_ref_of;
}

/// CAT-BND-1: the lower-case hex SHA-256 of the bundled catalogue's files as the ingest reads them,
/// in path order, each as its path, a zero byte, its length (8 bytes, little-endian) and its bytes;
/// `None` when the folder cannot be read.
// spec: ordin-desktop/components/CatalogueService#CAT-BND-1
pub(crate) fn bundled_digest(folder: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let files = crate::git::read_folder(folder).ok()?;
    let mut hash = Sha256::new();
    for (path, bytes) in &files {
        hash.update(path.as_bytes());
        hash.update([0u8]);
        hash.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
        hash.update(bytes);
    }
    Some(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod bundled_digest_tests {
    use super::bundled_digest;

    // CAT-BND-1: the digest follows the files, not the time they were written; no folder, no digest.
    // spec: ordin-desktop/components/CatalogueService#CAT-BND-1
    #[test]
    fn the_digest_changes_with_the_files_only() {
        let root = std::env::temp_dir().join(format!("cat-bnd-{}", uuid::Uuid::now_v7()));
        assert_eq!(bundled_digest(&root), None);
        std::fs::create_dir_all(root.join("models/a")).unwrap();
        std::fs::write(root.join("models/a/one.md"), b"one").unwrap();
        let first = bundled_digest(&root).expect("a digest");
        assert_eq!(first.len(), 64);
        std::fs::write(root.join("models/a/one.md"), b"one").unwrap();
        assert_eq!(bundled_digest(&root).as_deref(), Some(first.as_str()));
        std::fs::write(root.join("models/a/two.md"), b"two").unwrap();
        let second = bundled_digest(&root).expect("a digest");
        assert_ne!(first, second);
        std::fs::write(root.join("models/a/two.md"), b"tw0").unwrap();
        assert_ne!(bundled_digest(&root).as_deref(), Some(second.as_str()));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
