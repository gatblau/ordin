//! The Agent Model's `PartSource`, implemented over the index (CatalogueService.md §L), and the
//! lookups the ingest and the other reads share.

use std::path::PathBuf;

use async_trait::async_trait;
use ordin_agent_model::{
    overlay_order, parse_manifest, ModelPart, PartFiles, PartKind, PartSource, PartSourceError,
    RepositoryRef, Resolution, SourceScope, SourcedPart, VocabularyAxis, VocabularySet,
    WithdrawnListEntry,
};
use ordin_types::num::Sha256Hex;
use ordin_types::{AgentRepositoryId, WithdrawnListEntryId};
use sqlx::Row;

use crate::db::{
    all_repos, enum_from, latest_non_draft, parse_timestamp, part_by_id, part_from_row, uses_of,
    visible_parts_of, visible_versions, PartRecord, RepoRow, PART_SELECT,
};
use crate::error::CatalogueError;
use crate::git;
use crate::service::{repo_ref_of, CatalogueService};
use crate::types::axis_text;

/// The answer of one lookup through a scope, with the repositories that hold the part.
pub(crate) enum Hit {
    Found(Box<PartRecord>, Box<RepoRow>),
    NotFound,
    Ambiguous(Vec<RepoRow>),
}

impl From<CatalogueError> for PartSourceError {
    fn from(_: CatalogueError) -> Self {
        PartSourceError::Unavailable { retryable: true }
    }
}

fn log_error(identifier: &str, error: &PartSourceError) {
    tracing::warn!(
        component = "catalogue",
        identifier,
        error = ?error,
        "catalogue.part_source_error"
    );
}

impl CatalogueService {
    /// The rows of each step of the resolution order: home, the profile repositories together,
    /// the bundled catalogue. A repository that is not read is left out.
    pub(crate) async fn steps_of(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<Vec<RepoRow>>, CatalogueError> {
        let mut steps = Vec::new();
        let mut one = |rows: Vec<RepoRow>| steps.push(rows);
        let mut home = Vec::new();
        if let Ok(row) = self.repo_of(&scope.home).await {
            if row.is_read() {
                home.push(row);
            }
        }
        one(home);
        let mut profile = Vec::new();
        for reference in &scope.profile {
            if let Ok(row) = self.repo_of(reference).await {
                if row.is_read() {
                    profile.push(row);
                }
            }
        }
        one(profile);
        if scope.include_bundled {
            let mut bundled = Vec::new();
            if let Ok(row) = self.repo(ordin_agent_model::BUNDLED_REPOSITORY_ID).await {
                bundled.push(row);
            }
            one(bundled);
        }
        Ok(steps)
    }

    /// Every distinct repository of the scope, in order.
    pub(crate) async fn scope_rows(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<RepoRow>, CatalogueError> {
        let mut rows: Vec<RepoRow> = Vec::new();
        for step in self.steps_of(scope).await? {
            for row in step {
                if rows.iter().all(|r| r.id != row.id) {
                    rows.push(row);
                }
            }
        }
        Ok(rows)
    }

    /// §L2: the first step with any hit decides.
    pub(crate) async fn lookup_steps(
        &self,
        steps: &[Vec<RepoRow>],
        kind: Option<PartKind>,
        id: &str,
        version: Option<&str>,
    ) -> Result<Hit, CatalogueError> {
        let pool = self.pool()?;
        for step in steps {
            let mut hits: Vec<(PartRecord, RepoRow)> = Vec::new();
            for row in step {
                let records = visible_versions(pool, row.id, kind, id).await?;
                let mut by_kind: std::collections::BTreeMap<PartKind, Vec<PartRecord>> =
                    std::collections::BTreeMap::new();
                for record in records {
                    by_kind.entry(record.part.kind).or_default().push(record);
                }
                for (_, group) in by_kind {
                    let chosen = match version {
                        Some(version) => group.into_iter().find(|r| r.part.version == version),
                        None => latest_non_draft(group),
                    };
                    if let Some(record) = chosen {
                        hits.push((record, row.clone()));
                    }
                }
            }
            match hits.len() {
                0 => continue,
                1 => {
                    let (record, row) = hits.remove(0);
                    return Ok(Hit::Found(Box::new(record), Box::new(row)));
                }
                _ => {
                    let mut rows: Vec<RepoRow> = Vec::new();
                    for (_, row) in hits {
                        if rows.iter().all(|r| r.id != row.id) {
                            rows.push(row);
                        }
                    }
                    return Ok(Hit::Ambiguous(rows));
                }
            }
        }
        Ok(Hit::NotFound)
    }

    pub(crate) async fn lookup(
        &self,
        scope: &SourceScope,
        kind: Option<PartKind>,
        id: &str,
        version: Option<&str>,
    ) -> Result<Hit, CatalogueError> {
        let steps = self.steps_of(scope).await?;
        self.lookup_steps(&steps, kind, id, version).await
    }

    /// The file of a part at a commit; `None` when the commit or the file is not there.
    pub(crate) async fn read_file(
        &self,
        row: &RepoRow,
        commit: &str,
        file: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        if row.is_bundled() {
            let path = self.bundled_path(file);
            return match tokio::fs::read(&path).await {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.to_string()),
            };
        }
        let clone = PathBuf::from(&row.clone_path);
        let (commit, file) = (commit.to_owned(), file.to_owned());
        tokio::task::spawn_blocking(move || git::read_blob_at(&clone, &commit, &file))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Where a bundled part's file is: the folder holds `models/` itself, or is that folder.
    pub(crate) fn bundled_path(&self, file: &str) -> PathBuf {
        let root = &self.inner.config.bundled_catalogue;
        if root.join("models").is_dir() {
            root.join(file)
        } else {
            root.join(file.strip_prefix("models/").unwrap_or(file))
        }
    }

    /// A `SourcedPart` from a record, parsing the manifest at the part's commit.
    pub(crate) async fn sourced(
        &self,
        row: &RepoRow,
        record: &PartRecord,
    ) -> Result<SourcedPart, PartSourceError> {
        let identifier = record.part.identifier.clone();
        let bytes = self
            .read_file(row, &record.part.commit, &record.part.file_path)
            .await
            .map_err(|_| PartSourceError::Unavailable { retryable: true })?
            .ok_or_else(|| PartSourceError::Corrupt {
                part_identifier: identifier.clone(),
            })?;
        let manifest = parse_manifest(&record.part.file_path, &bytes)
            .ok()
            .flatten()
            .ok_or(PartSourceError::Corrupt {
                part_identifier: identifier,
            })?;
        Ok(SourcedPart {
            part: record.part.clone(),
            repository: repo_ref_of(row),
            repository_label: row.label(),
            repository_display_name: row.shown_as(),
            manifest,
        })
    }

    async fn sourced_all(
        &self,
        records: Vec<(PartRecord, RepoRow)>,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        let mut out = Vec::with_capacity(records.len());
        for (record, row) in records {
            out.push(self.sourced(&row, &record).await?);
        }
        Ok(out)
    }

    /// The `ModelPart` of a sourced part, for the Vocabulary set.
    async fn model_part(&self, part: &SourcedPart) -> Result<ModelPart, PartSourceError> {
        let files = self.files(part).await?;
        Ok(ModelPart {
            kind: part.part.kind,
            manifest: part.manifest.clone(),
            manifest_path: part.part.file_path.clone(),
            files,
            content_digest: part.part.content_digest.clone(),
        })
    }

    /// Fetches the branch once, without ingesting (a commit missing from the clone, §L4).
    async fn refetch(&self, row: &RepoRow) {
        let github = std::sync::Arc::clone(&self.inner.deps.github);
        let handle = tokio::runtime::Handle::current();
        let (host, repository, branch) = (
            row.host.clone(),
            row.repository.clone(),
            row.protected_branch.clone(),
        );
        let path = PathBuf::from(&row.clone_path);
        let _ = tokio::task::spawn_blocking(move || {
            let credentials = || {
                handle
                    .block_on(github.git_credentials(&host))
                    .map(|c| (c.username, c.token.expose().to_owned()))
                    .map_err(crate::repos::credential_failure)
            };
            git::fetch_branch(
                &path,
                &git::remote_url(&host, &repository),
                &branch,
                &credentials,
            )
        })
        .await;
    }

    async fn resolution(&self, hit: Hit) -> Result<Resolution, PartSourceError> {
        match hit {
            Hit::NotFound => Ok(Resolution::NotFound),
            Hit::Ambiguous(rows) => Ok(Resolution::Ambiguous {
                repositories: rows.iter().map(RepoRow::label).collect(),
            }),
            Hit::Found(record, row) => Ok(Resolution::Found(self.sourced(&row, &record).await?)),
        }
    }

    /// Parts of one repository by kind set, latest non-draft per identifier.
    pub(crate) async fn latest_of_kinds(
        &self,
        row: &RepoRow,
        kinds: &[PartKind],
    ) -> Result<Vec<PartRecord>, CatalogueError> {
        let all = visible_parts_of(self.pool()?, row.id).await?;
        let mut by_id: std::collections::BTreeMap<(PartKind, String), Vec<PartRecord>> =
            std::collections::BTreeMap::new();
        for record in all.into_iter().filter(|r| kinds.contains(&r.part.kind)) {
            by_id
                .entry((record.part.kind, record.part.identifier.clone()))
                .or_default()
                .push(record);
        }
        Ok(by_id.into_values().filter_map(latest_non_draft).collect())
    }

    /// Stored withdrawn entries of one repository.
    pub(crate) async fn withdrawn_entries_of(
        &self,
        repository: AgentRepositoryId,
    ) -> Result<Vec<WithdrawnListEntry>, CatalogueError> {
        let rows = sqlx::query(
            "SELECT id, part_kind, part_identifier, part_version, part_content_digest, reason, \
             withdrawn_at, replacement_identifier, replacement_version, removed_by_hand_seen_at, \
             read_at FROM withdrawn_list_entries WHERE agent_repository_id = ?1 \
             ORDER BY part_kind, part_identifier, IFNULL(part_version, '')",
        )
        .bind(repository.to_string())
        .fetch_all(self.pool()?)
        .await?;
        let corrupt = || CatalogueError::store("a stored withdrawn entry is not readable");
        let mut out = Vec::new();
        for row in rows {
            let id: String = row.try_get("id")?;
            let kind: String = row.try_get("part_kind")?;
            let digest: Option<String> = row.try_get("part_content_digest")?;
            let withdrawn_at: String = row.try_get("withdrawn_at")?;
            let seen: Option<String> = row.try_get("removed_by_hand_seen_at")?;
            let read_at: String = row.try_get("read_at")?;
            out.push(WithdrawnListEntry {
                id: WithdrawnListEntryId(uuid::Uuid::parse_str(&id).map_err(|_| corrupt())?),
                agent_repository_id: repository,
                part_kind: enum_from(&kind).ok_or_else(corrupt)?,
                part_identifier: row.try_get("part_identifier")?,
                part_version: row.try_get("part_version")?,
                part_content_digest: match digest {
                    Some(d) => Some(Sha256Hex::parse(&d).map_err(|_| corrupt())?),
                    None => None,
                },
                reason: row.try_get("reason")?,
                withdrawn_at: parse_timestamp(&withdrawn_at)?,
                replacement_identifier: row.try_get("replacement_identifier")?,
                replacement_version: row.try_get("replacement_version")?,
                removed_by_hand_seen_at: match seen {
                    Some(s) => Some(parse_timestamp(&s)?),
                    None => None,
                },
                read_at: parse_timestamp(&read_at)?,
            });
        }
        Ok(out)
    }

    /// The reverse closure of a part over `part_dependencies`: every visible head part that uses it,
    /// directly or through other parts, depth at most 32 (§J).
    pub(crate) async fn reverse_closure(
        &self,
        id: ordin_types::PartId,
    ) -> Result<Vec<(PartRecord, RepoRow)>, CatalogueError> {
        let sql = format!(
            "WITH RECURSIVE up(id, depth) AS (\
               SELECT part_id, 1 FROM part_dependencies WHERE uses_part_id = ?1 \
               UNION \
               SELECT d.part_id, up.depth + 1 FROM part_dependencies d JOIN up ON d.uses_part_id = up.id \
               WHERE up.depth < 32) \
             {PART_SELECT} WHERE p.id IN (SELECT id FROM up) AND i.at_head = 1 AND i.offered = 1"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id.to_string())
            .fetch_all(self.pool()?)
            .await?;
        let repos = all_repos(self.pool()?).await?;
        let mut out = Vec::new();
        for row in &rows {
            let mut record = part_from_row(row)?;
            record.part.uses = uses_of(self.pool()?, record.part.id).await?;
            if let Some(repo) = repos
                .iter()
                .find(|r| r.id == record.part.agent_repository_id)
            {
                out.push((record, repo.clone()));
            }
        }
        out.sort_by(|(a, ar), (b, br)| {
            (ar.label(), a.part.kind, &a.part.identifier, &a.part.version).cmp(&(
                br.label(),
                b.part.kind,
                &b.part.identifier,
                &b.part.version,
            ))
        });
        Ok(out)
    }
}

impl CatalogueService {
    /// The visible Vocabulary parts of one repository as `ModelPart`s, one per axis (the first wins).
    pub(crate) async fn vocabulary_models_of(
        &self,
        row: &RepoRow,
    ) -> Result<Vec<(VocabularyAxis, ModelPart)>, PartSourceError> {
        let records = visible_parts_of(self.pool()?, row.id).await?;
        let mut taken = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for record in records
            .into_iter()
            .filter(|r| r.part.kind == PartKind::Vocabulary)
        {
            let sourced = self.sourced(row, &record).await?;
            if let Ok(Some(body)) = sourced.manifest.vocabulary() {
                if taken.insert(axis_text(body.axis)) {
                    out.push((body.axis, self.model_part(&sourced).await?));
                }
            }
        }
        Ok(out)
    }
}

// spec: ordin-desktop/components/CatalogueService#SA-RECON-01
#[async_trait]
impl PartSource for CatalogueService {
    async fn resolve(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, PartSourceError> {
        let result = async {
            let hit = self.lookup(scope, Some(kind), id, version).await?;
            if let Hit::Ambiguous(rows) = &hit {
                tracing::warn!(
                    component = "catalogue",
                    identifier = id,
                    error = "ambiguous",
                    repositories = rows.len(),
                    "catalogue.part_source_error"
                );
            }
            self.resolution(hit).await
        }
        .await;
        if let Err(error) = &result {
            log_error(id, error);
        }
        result
    }

    async fn resolve_any_kind(
        &self,
        scope: &SourceScope,
        id: &str,
    ) -> Result<Resolution, PartSourceError> {
        let result = async {
            let hit = self.lookup(scope, None, id, None).await?;
            self.resolution(hit).await
        }
        .await;
        if let Err(error) = &result {
            log_error(id, error);
        }
        result
    }

    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError> {
        let identifier = part.part.identifier.clone();
        let result = async {
            let record =
                part_by_id(self.pool()?, part.part.id)
                    .await?
                    .ok_or(PartSourceError::Corrupt {
                        part_identifier: identifier.clone(),
                    })?;
            let row = self.repo(part.part.agent_repository_id).await?;
            let dir = record
                .part
                .file_path
                .rsplit_once('/')
                .map(|(dir, _)| dir.to_owned())
                .unwrap_or_default();
            let mut refetched = row.is_bundled();
            loop {
                let mut files = std::collections::BTreeMap::new();
                let mut missing = false;
                for path in &record.files {
                    match self
                        .read_file(&row, &record.part.commit, path)
                        .await
                        .map_err(|_| PartSourceError::Unavailable { retryable: true })?
                    {
                        Some(bytes) => {
                            let key = if dir.is_empty() {
                                path.clone()
                            } else {
                                path.strip_prefix(&format!("{dir}/"))
                                    .unwrap_or(path)
                                    .to_owned()
                            };
                            files.insert(key, bytes);
                        }
                        None => {
                            missing = true;
                            break;
                        }
                    }
                }
                if !missing {
                    return Ok(PartFiles(files));
                }
                if refetched {
                    return Err(PartSourceError::Corrupt {
                        part_identifier: identifier.clone(),
                    });
                }
                refetched = true;
                self.refetch(&row).await;
            }
        }
        .await;
        if let Err(error) = &result {
            log_error(&identifier, error);
        }
        result
    }

    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError> {
        let mut kinds = vec![PartKind::PracticePack];
        kinds.extend(overlay_order().iter().copied());
        let mut seen = std::collections::BTreeSet::new();
        let mut picked = Vec::new();
        for row in self.scope_rows(scope).await? {
            for record in self.latest_of_kinds(&row, &kinds).await? {
                if seen.insert((record.part.kind, record.part.identifier.clone())) {
                    picked.push((record, row.clone()));
                }
            }
        }
        self.sourced_all(picked).await
    }

    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        let mut out = Vec::new();
        for row in self.scope_rows(scope).await? {
            if !row.is_bundled() && row.last_fetched_commit.is_none() {
                return Err(PartSourceError::WithdrawnListUnreadable {
                    repository: row.label(),
                });
            }
            out.extend(self.withdrawn_entries_of(row.id).await?);
        }
        Ok(out)
    }

    async fn repository_parts(
        &self,
        repository: &RepositoryRef,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        let Ok(row) = self.repo_of(repository).await else {
            return Ok(Vec::new());
        };
        let records = visible_parts_of(self.pool()?, row.id).await?;
        let pairs = records.into_iter().map(|r| (r, row.clone())).collect();
        self.sourced_all(pairs).await
    }

    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError> {
        let mut taken: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();
        let mut parts: Vec<ModelPart> = Vec::new();
        for row in self.scope_rows(scope).await? {
            for (axis, model) in self.vocabulary_models_of(&row).await? {
                if taken.insert(axis_text(axis)) {
                    parts.push(model);
                }
            }
        }
        let refs: Vec<&ModelPart> = parts.iter().collect();
        VocabularySet::from_parts(&refs)
            .map_err(|_| PartSourceError::Unavailable { retryable: false })
    }

    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError> {
        let users: Vec<(PartRecord, RepoRow)> = self
            .reverse_closure(part.part.id)
            .await?
            .into_iter()
            .filter(|(record, _)| record.part.kind == PartKind::WorkspaceProfile)
            .collect();
        self.sourced_all(users).await
    }

    async fn citing_parts(
        &self,
        repository: &RepositoryRef,
        axis: VocabularyAxis,
        code: &str,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        let Ok(row) = self.repo_of(repository).await else {
            return Ok(Vec::new());
        };
        let term = sqlx::query(
            "SELECT id FROM vocabulary_terms WHERE agent_repository_id = ?1 AND axis = ?2 AND code = ?3",
        )
        .bind(row.id.to_string())
        .bind(axis_text(axis))
        .bind(code)
        .fetch_optional(self.pool()?)
        .await
        .map_err(CatalogueError::from)?;
        let Some(term) = term else {
            return Ok(Vec::new());
        };
        let term_id: String = term.try_get("id").map_err(CatalogueError::from)?;
        let sql = format!(
            "{PART_SELECT} WHERE i.at_head = 1 AND i.offered = 1 AND instr(p.vocabulary_terms_cited, ?1) > 0"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(term_id)
            .fetch_all(self.pool()?)
            .await
            .map_err(CatalogueError::from)?;
        let repos = all_repos(self.pool()?).await?;
        let mut pairs: Vec<(PartRecord, RepoRow)> = Vec::new();
        for row in &rows {
            let mut record = part_from_row(row)?;
            record.part.uses = uses_of(self.pool()?, record.part.id).await?;
            if let Some(repo) = repos
                .iter()
                .find(|r| r.id == record.part.agent_repository_id)
            {
                pairs.push((record, repo.clone()));
            }
        }
        pairs.sort_by(|(a, ar), (b, br)| {
            (ar.label(), a.part.kind, &a.part.identifier).cmp(&(
                br.label(),
                b.part.kind,
                &b.part.identifier,
            ))
        });
        self.sourced_all(pairs).await
    }
}
