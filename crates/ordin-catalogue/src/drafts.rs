//! Draft worktrees and the draft `PartSource` (CatalogueService.md §M, §M2).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use ordin_agent_model::{
    overlay_order, ModelPart, PartFiles, PartKind, PartSource, PartSourceError, RepositoryRef,
    Resolution, SourceScope, SourcedPart, VocabularyAxis, VocabularySet, WithdrawnListEntry,
};
use ordin_format_plugins::{FormatId, ReadRequest, SourceTree};
use ordin_types::domain::agents::{Part, PartStatus};
use ordin_types::{AgentRepositoryId, PartId};

use crate::db::{latest_non_draft, version_order, PartRecord, RepoRow};
use crate::error::CatalogueError;
use crate::git::{self, WorktreeFailure};
use crate::service::{repo_ref_of, CatalogueService};
use crate::types::{axis_text, DraftWorktree, DraftWorktreeInput};

const ZERO_COMMIT: &str = "0000000000000000000000000000000000000000";

fn slug(branch: &str) -> String {
    branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

impl CatalogueService {
    /// §M.
    pub async fn open_draft_worktree(
        &self,
        input: DraftWorktreeInput,
    ) -> Result<DraftWorktree, CatalogueError> {
        let row = self.repo(input.agent_repository_id).await?;
        if !row.person_can_publish {
            return Err(CatalogueError::permanent(
                "CAT_NO_REVIEW_RIGHT",
                "the person cannot publish to this repository",
            ));
        }
        if !git::valid_branch(&input.branch) {
            return Err(CatalogueError::permanent(
                "AREPO_BRANCH_INVALID",
                "not a valid branch name",
            ));
        }
        let name = slug(&input.branch);
        let path = self
            .inner
            .config
            .root
            .join("worktrees")
            .join(row.id.to_string())
            .join(&name);
        let (clone, protected, branch) = (
            PathBuf::from(&row.clone_path),
            row.protected_branch.clone(),
            input.branch.clone(),
        );
        let at = path.clone();
        let base = tokio::task::spawn_blocking(move || {
            git::add_worktree(&clone, &protected, &branch, &name, &at)
        })
        .await
        .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?;
        match base {
            Ok(base_commit) => Ok(DraftWorktree { path, base_commit }),
            Err(WorktreeFailure::BranchExists) => Err(CatalogueError::permanent(
                "CAT_BRANCH_EXISTS",
                "the branch already exists",
            )),
            Err(WorktreeFailure::Other(detail)) => Err(CatalogueError::fetch_failed(detail)),
        }
    }

    pub async fn close_draft_worktree(
        &self,
        branch: &str,
        id: AgentRepositoryId,
    ) -> Result<(), CatalogueError> {
        if !git::valid_branch(branch) {
            return Err(CatalogueError::permanent(
                "AREPO_BRANCH_INVALID",
                "the branch name is not valid",
            ));
        }
        let row = self.repo(id).await?;
        let name = slug(branch);
        let path = self
            .inner
            .config
            .root
            .join("worktrees")
            .join(row.id.to_string())
            .join(&name);
        let clone = PathBuf::from(&row.clone_path);
        tokio::task::spawn_blocking(move || git::remove_worktree(&clone, &name, &path))
            .await
            .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?
            .map_err(CatalogueError::fetch_failed)
    }

    /// §M2.
    pub async fn draft_source(
        &self,
        id: AgentRepositoryId,
        worktree: PathBuf,
    ) -> Result<Arc<dyn PartSource>, CatalogueError> {
        let row = self.repo(id).await?;
        let models = self.read_worktree(&row, &worktree).await?;
        let label = row.label();
        let display = row.shown_as();
        let parts = models
            .into_iter()
            .map(|model| {
                let metadata = &model.manifest.metadata;
                let part = Part {
                    id: PartId::new(),
                    agent_repository_id: id,
                    kind: model.kind,
                    identifier: metadata.id.clone(),
                    name: metadata.name.clone(),
                    version: metadata.version.clone(),
                    status: model
                        .manifest
                        .status_or_default()
                        .unwrap_or(PartStatus::Approved),
                    supersedes: metadata.supersedes.clone(),
                    owner: metadata.owner.clone(),
                    file_path: model.manifest_path.clone(),
                    commit: ZERO_COMMIT.to_owned(),
                    content_digest: model.content_digest.clone(),
                    vocabulary_terms_cited: Vec::new(),
                    uses: Vec::new(),
                    withdrawn: false,
                };
                let sourced = SourcedPart {
                    part,
                    repository: RepositoryRef::Repository(id),
                    repository_label: label.clone(),
                    repository_display_name: display.clone(),
                    manifest: model.manifest.clone(),
                };
                (sourced, model)
            })
            .collect();
        Ok(Arc::new(DraftSource {
            service: self.clone(),
            repository: id,
            row,
            parts,
        }))
    }

    async fn read_worktree(
        &self,
        row: &RepoRow,
        path: &Path,
    ) -> Result<Vec<ModelPart>, CatalogueError> {
        let folder = path.to_path_buf();
        let files = tokio::task::spawn_blocking(move || git::read_folder(&folder))
            .await
            .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?
            .map_err(|e| CatalogueError::fetch_failed(e.to_string()))?;
        let tree = SourceTree(files);
        let plugin = self.inner.deps.formats.get(FormatId::from(row.format));
        let request = |vocabulary| ReadRequest {
            publisher: row
                .repository
                .split('/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase(),
            vocabulary,
            source_label: row.display_name.clone(),
        };
        let first = plugin
            .read(&tree, &request(VocabularySet::default()))
            .map_err(|e| CatalogueError::permanent("CAT_FORMAT_CHANGED", e.to_string()))?;
        let vocabulary_parts: Vec<&ModelPart> = first
            .parts
            .iter()
            .filter(|p| p.kind == PartKind::Vocabulary)
            .collect();
        let vocabulary = VocabularySet::from_parts(&vocabulary_parts)
            .map_err(|e| CatalogueError::permanent("VOCAB_CODE_INVALID", e.to_string()))?;
        let outcome = plugin
            .read(&tree, &request(vocabulary))
            .map_err(|e| CatalogueError::permanent("CAT_FORMAT_CHANGED", e.to_string()))?;
        Ok(outcome.parts)
    }
}

/// The catalogue with one repository's head replaced by a worktree.
struct DraftSource {
    service: CatalogueService,
    repository: AgentRepositoryId,
    row: RepoRow,
    parts: Vec<(SourcedPart, ModelPart)>,
}

impl DraftSource {
    fn mine(&self, row: &RepoRow) -> bool {
        row.id == self.repository
    }

    /// The draft's parts of one kind and identifier, or of any kind.
    fn candidates(&self, kind: Option<PartKind>, id: &str) -> Vec<&SourcedPart> {
        self.parts
            .iter()
            .map(|(s, _)| s)
            .filter(|s| s.part.identifier == id && kind.is_none_or(|k| s.part.kind == k))
            .collect()
    }

    /// Worktree parts are visible whatever their status.
    fn pick(candidates: Vec<&SourcedPart>, version: Option<&str>) -> Option<SourcedPart> {
        match version {
            Some(version) => candidates
                .into_iter()
                .find(|s| s.part.version == version)
                .cloned(),
            None => candidates
                .into_iter()
                .max_by(|a, b| version_order(&a.part.version, &b.part.version))
                .cloned(),
        }
    }

    async fn lookup(
        &self,
        scope: &SourceScope,
        kind: Option<PartKind>,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, PartSourceError> {
        for step in self.service.steps_of(scope).await? {
            let mut hits: Vec<SourcedPart> = Vec::new();
            for row in &step {
                if self.mine(row) {
                    let mut by_kind: BTreeMap<PartKind, Vec<&SourcedPart>> = BTreeMap::new();
                    for c in self.candidates(kind, id) {
                        by_kind.entry(c.part.kind).or_default().push(c);
                    }
                    hits.extend(
                        by_kind
                            .into_values()
                            .filter_map(|group| Self::pick(group, version)),
                    );
                } else {
                    let records =
                        crate::db::visible_versions(self.service.pool()?, row.id, kind, id).await?;
                    let mut by_kind: BTreeMap<PartKind, Vec<PartRecord>> = BTreeMap::new();
                    for r in records {
                        by_kind.entry(r.part.kind).or_default().push(r);
                    }
                    for group in by_kind.into_values() {
                        let chosen = match version {
                            Some(version) => group.into_iter().find(|r| r.part.version == version),
                            None => latest_non_draft(group),
                        };
                        if let Some(record) = chosen {
                            hits.push(self.service.sourced(row, &record).await?);
                        }
                    }
                }
            }
            match hits.len() {
                0 => continue,
                1 => return Ok(Resolution::Found(hits.remove(0))),
                _ => {
                    let mut labels: Vec<String> =
                        hits.iter().map(|h| h.repository_label.clone()).collect();
                    labels.dedup();
                    return Ok(Resolution::Ambiguous {
                        repositories: labels,
                    });
                }
            }
        }
        Ok(Resolution::NotFound)
    }

    fn model_of(&self, part: &SourcedPart) -> Option<&ModelPart> {
        self.parts
            .iter()
            .find(|(s, _)| {
                s.part.id == part.part.id
                    || (s.part.kind == part.part.kind
                        && s.part.identifier == part.part.identifier
                        && s.part.version == part.part.version)
            })
            .map(|(_, m)| m)
    }

    /// Identity of a part for closure checks: kind, identifier, version.
    fn key(part: &SourcedPart) -> (PartKind, String, String) {
        (
            part.part.kind,
            part.part.identifier.clone(),
            part.part.version.clone(),
        )
    }
}

#[async_trait]
impl PartSource for DraftSource {
    async fn resolve(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, PartSourceError> {
        self.lookup(scope, Some(kind), id, version).await
    }

    async fn resolve_any_kind(
        &self,
        scope: &SourceScope,
        id: &str,
    ) -> Result<Resolution, PartSourceError> {
        self.lookup(scope, None, id, None).await
    }

    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError> {
        if part.part.commit == ZERO_COMMIT && part.part.agent_repository_id == self.repository {
            return self
                .model_of(part)
                .map(|m| m.files.clone())
                .ok_or(PartSourceError::Corrupt {
                    part_identifier: part.part.identifier.clone(),
                });
        }
        self.service.files(part).await
    }

    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError> {
        let mut kinds = vec![PartKind::PracticePack];
        kinds.extend(overlay_order().iter().copied());
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for row in self.service.scope_rows(scope).await? {
            if self.mine(&row) {
                let mut by_id: BTreeMap<(PartKind, String), Vec<&SourcedPart>> = BTreeMap::new();
                for (s, _) in &self.parts {
                    if kinds.contains(&s.part.kind) {
                        by_id
                            .entry((s.part.kind, s.part.identifier.clone()))
                            .or_default()
                            .push(s);
                    }
                }
                for (key, group) in by_id {
                    let latest = group
                        .into_iter()
                        .filter(|s| s.part.status != PartStatus::Draft)
                        .max_by(|a, b| version_order(&a.part.version, &b.part.version));
                    if let Some(latest) = latest {
                        if seen.insert(key) {
                            out.push(latest.clone());
                        }
                    }
                }
            } else {
                for record in self.service.latest_of_kinds(&row, &kinds).await? {
                    if seen.insert((record.part.kind, record.part.identifier.clone())) {
                        out.push(self.service.sourced(&row, &record).await?);
                    }
                }
            }
        }
        Ok(out)
    }

    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        self.service.withdrawn(scope).await
    }

    async fn repository_parts(
        &self,
        repository: &RepositoryRef,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        if *repository == repo_ref_of(&self.row) {
            let mut parts: Vec<SourcedPart> = self.parts.iter().map(|(s, _)| s.clone()).collect();
            parts.sort_by(|a, b| {
                (a.part.kind, &a.part.identifier)
                    .cmp(&(b.part.kind, &b.part.identifier))
                    .then_with(|| version_order(&a.part.version, &b.part.version))
            });
            return Ok(parts);
        }
        self.service.repository_parts(repository).await
    }

    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError> {
        let mut taken: BTreeSet<&'static str> = BTreeSet::new();
        let mut models: Vec<ModelPart> = Vec::new();
        for row in self.service.scope_rows(scope).await? {
            let mine: Vec<(VocabularyAxis, ModelPart)> = if self.mine(&row) {
                let mut out = Vec::new();
                let mut here = BTreeSet::new();
                for (_, model) in self
                    .parts
                    .iter()
                    .filter(|(s, _)| s.part.kind == PartKind::Vocabulary)
                {
                    if let Ok(Some(body)) = model.manifest.vocabulary() {
                        if here.insert(axis_text(body.axis)) {
                            out.push((body.axis, model.clone()));
                        }
                    }
                }
                out
            } else {
                self.service.vocabulary_models_of(&row).await?
            };
            for (axis, model) in mine {
                if taken.insert(axis_text(axis)) {
                    models.push(model);
                }
            }
        }
        let refs: Vec<&ModelPart> = models.iter().collect();
        VocabularySet::from_parts(&refs)
            .map_err(|_| PartSourceError::Unavailable { retryable: false })
    }

    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError> {
        let mut users: Vec<SourcedPart> = PartSource::used_by(&self.service, part)
            .await?
            .into_iter()
            .filter(|u| u.part.agent_repository_id != self.repository)
            .collect();
        let scope = self.service.source_scope(repo_ref_of(&self.row)).await?;
        let wanted = Self::key(part);
        for (agent, _) in self
            .parts
            .iter()
            .filter(|(s, _)| s.part.kind == PartKind::WorkspaceProfile)
        {
            let mut seen: BTreeSet<(PartKind, String, String)> = BTreeSet::new();
            let mut frontier = vec![agent.clone()];
            let mut found = false;
            let mut depth = 0;
            while let Some(current) = frontier.pop() {
                depth += 1;
                if depth > 2000 {
                    break;
                }
                for reference in current.manifest.references() {
                    let kind = ordin_agent_model::relation_kind(&reference.relation);
                    let resolved = match kind {
                        Some(kind) => self.resolve(&scope, kind, &reference.id, None).await?,
                        None => self.resolve_any_kind(&scope, &reference.id).await?,
                    };
                    if let Resolution::Found(next) = resolved {
                        if Self::key(&next) == wanted {
                            found = true;
                        }
                        if seen.insert(Self::key(&next)) {
                            frontier.push(next);
                        }
                    }
                }
                if found {
                    break;
                }
            }
            if found {
                users.push(agent.clone());
            }
        }
        Ok(users)
    }

    async fn citing_parts(
        &self,
        repository: &RepositoryRef,
        axis: VocabularyAxis,
        code: &str,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        let mut out: Vec<SourcedPart> = self
            .service
            .citing_parts(repository, axis, code)
            .await?
            .into_iter()
            .filter(|p| p.part.agent_repository_id != self.repository)
            .collect();
        for (s, _) in &self.parts {
            let metadata = &s.manifest.metadata;
            let cites = match axis {
                VocabularyAxis::Category => metadata.category == code,
                VocabularyAxis::AudienceRole => metadata.audience.roles.iter().any(|r| r == code),
                _ => false,
            };
            if cites {
                out.push(s.clone());
            }
        }
        out.sort_by(|a, b| {
            (&a.repository_label, a.part.kind, &a.part.identifier).cmp(&(
                &b.repository_label,
                b.part.kind,
                &b.part.identifier,
            ))
        });
        Ok(out)
    }
}
