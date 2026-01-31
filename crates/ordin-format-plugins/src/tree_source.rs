//! A read-only `PartSource` over the parts of one read (FormatPlugins.md *Internal Logic* 12).
//!
//! One repository, at one commit, holding exactly the parts of an outcome. A package is
//! self-contained (ADR-098): only a scope whose `home` is this repository sees anything.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use async_trait::async_trait;
use ordin_agent_model::{
    compare_versions, kind_info, overlay_order, prefixed_digest, relation_kind, slug_from_id,
    AgentRepositoryId, ModelPart, Part, PartFiles, PartId, PartKind, PartRef, PartSource,
    PartSourceError, RepositoryRef, Resolution, SourceScope, SourcedPart, VocabularyAxis,
    VocabularySet, WithdrawnListEntry,
};

use crate::error::FormatError;
use crate::types::ReadOutcome;

struct Entry {
    sourced: SourcedPart,
    model: ModelPart,
}

/// A read-only `PartSource` over the parts of one read.
pub struct TreeSource {
    repository_id: AgentRepositoryId,
    label: String,
    commit: String,
    entries: Vec<Entry>,
}

fn version_order(a: &str, b: &str) -> Ordering {
    compare_versions(a, b).unwrap_or_else(|_| a.cmp(b))
}

fn newest<'a>(entries: impl Iterator<Item = &'a Entry>) -> Option<&'a Entry> {
    entries.max_by(|a, b| version_order(&a.sourced.part.version, &b.sourced.part.version))
}

fn sort_key(entry: &Entry) -> (PartKind, &str) {
    (
        entry.sourced.part.kind,
        entry.sourced.part.identifier.as_str(),
    )
}

impl TreeSource {
    /// `commit`: 40 lowercase hex characters, or 40 zeros for a package not under Git.
    pub fn new(
        outcome: &ReadOutcome,
        repository_id: AgentRepositoryId,
        repository_label: &str,
        commit: &str,
    ) -> Result<TreeSource, FormatError> {
        let valid = commit.len() == 40
            && commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !valid {
            return Err(FormatError::CommitInvalid {
                commit: commit.to_owned(),
            });
        }
        let mut entries = Vec::with_capacity(outcome.parts.len());
        for model in &outcome.parts {
            let manifest = &model.manifest;
            let identifier = manifest.metadata.id.clone();
            let status = manifest
                .status_or_default()
                .map_err(|e| FormatError::PartInvalid {
                    identifier: identifier.clone(),
                    code: e.code().to_owned(),
                })?;
            let part = Part {
                id: PartId::new(),
                agent_repository_id: repository_id,
                kind: model.kind,
                identifier,
                name: manifest.metadata.name.clone(),
                version: manifest.metadata.version.clone(),
                status,
                supersedes: manifest.metadata.supersedes.clone(),
                owner: manifest.metadata.owner.clone(),
                file_path: model.manifest_path.clone(),
                commit: commit.to_owned(),
                content_digest: model.content_digest.clone(),
                vocabulary_terms_cited: Vec::new(),
                uses: Vec::new(),
                withdrawn: false,
            };
            entries.push(Entry {
                sourced: SourcedPart {
                    part,
                    repository: RepositoryRef::Repository(repository_id),
                    repository_label: repository_label.to_owned(),
                    repository_display_name: repository_label.to_owned(),
                    manifest: manifest.clone(),
                },
                model: model.clone(),
            });
        }
        Ok(TreeSource {
            repository_id,
            label: repository_label.to_owned(),
            commit: commit.to_owned(),
            entries,
        })
    }

    /// The package's one WorkspaceProfile as a `PartRef`; `FMT_ROOT_NOT_SINGLE` when the read
    /// produced none or more than one.
    pub fn root(&self) -> Result<PartRef, FormatError> {
        let roots: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.sourced.part.kind == PartKind::WorkspaceProfile)
            .collect();
        match roots.as_slice() {
            [only] => {
                let part = &only.sourced.part;
                Ok(PartRef {
                    kind: part.kind,
                    id: part.identifier.clone(),
                    name: slug_from_id(&part.identifier),
                    version: part.version.clone(),
                    content_hash: prefixed_digest(&part.content_digest),
                    repository: self.label.clone(),
                    commit: self.commit.clone(),
                    loading: Some(kind_info(part.kind).default_loading),
                })
            }
            other => Err(FormatError::RootNotSingle {
                found: u32::try_from(other.len()).unwrap_or(u32::MAX),
            }),
        }
    }

    fn here(&self) -> RepositoryRef {
        RepositoryRef::Repository(self.repository_id)
    }

    fn in_scope(&self, scope: &SourceScope) -> bool {
        scope.home == self.here()
    }

    /// Indexes of the entries a reference reaches.
    fn targets(&self, id: &str, relation: &str) -> Vec<usize> {
        let kind = relation_kind(relation);
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.sourced.part.identifier == id && kind.is_none_or(|k| k == e.sourced.part.kind)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Every entry the forward references of entry `start` reach, `start` excluded.
    fn closure(&self, start: usize) -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(index) = stack.pop() {
            let Some(entry) = self.entries.get(index) else {
                continue;
            };
            for reference in entry.sourced.manifest.references() {
                for target in self.targets(&reference.id, &reference.relation) {
                    if target != start && seen.insert(target) {
                        stack.push(target);
                    }
                }
            }
        }
        seen
    }
}

#[async_trait]
impl PartSource for TreeSource {
    async fn resolve(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, PartSourceError> {
        if !self.in_scope(scope) {
            return Ok(Resolution::NotFound);
        }
        let found = newest(self.entries.iter().filter(|e| {
            let part = &e.sourced.part;
            part.kind == kind && part.identifier == id && version.is_none_or(|v| part.version == v)
        }));
        Ok(found.map_or(Resolution::NotFound, |e| {
            Resolution::Found(e.sourced.clone())
        }))
    }

    async fn resolve_any_kind(
        &self,
        scope: &SourceScope,
        id: &str,
    ) -> Result<Resolution, PartSourceError> {
        if !self.in_scope(scope) {
            return Ok(Resolution::NotFound);
        }
        let matching: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.sourced.part.identifier == id)
            .collect();
        let kinds: BTreeSet<PartKind> = matching.iter().map(|e| e.sourced.part.kind).collect();
        if kinds.len() > 1 {
            return Ok(Resolution::Ambiguous {
                repositories: vec![self.label.clone()],
            });
        }
        Ok(
            newest(matching.into_iter()).map_or(Resolution::NotFound, |e| {
                Resolution::Found(e.sourced.clone())
            }),
        )
    }

    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError> {
        self.entries
            .iter()
            .find(|e| e.sourced.part.id == part.part.id)
            .map(|e| e.model.files.clone())
            .ok_or_else(|| PartSourceError::Corrupt {
                part_identifier: part.part.identifier.clone(),
            })
    }

    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError> {
        if !self.in_scope(scope) {
            return Ok(Vec::new());
        }
        let wanted =
            |kind: PartKind| kind == PartKind::PracticePack || overlay_order().contains(&kind);
        let mut seen: BTreeSet<(PartKind, &str)> = BTreeSet::new();
        let mut out = Vec::new();
        for entry in self.entries.iter().filter(|e| wanted(e.sourced.part.kind)) {
            let key = sort_key(entry);
            if !seen.insert(key) {
                continue;
            }
            let latest = newest(self.entries.iter().filter(|e| sort_key(e) == key));
            if let Some(latest) = latest {
                out.push(latest.sourced.clone());
            }
        }
        Ok(out)
    }

    async fn withdrawn(
        &self,
        _scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        Ok(Vec::new())
    }

    async fn repository_parts(
        &self,
        repository: &RepositoryRef,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        if *repository != self.here() {
            return Ok(Vec::new());
        }
        Ok(self.entries.iter().map(|e| e.sourced.clone()).collect())
    }

    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError> {
        if !self.in_scope(scope) {
            return Ok(VocabularySet::default());
        }
        let vocabularies: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.sourced.part.kind == PartKind::Vocabulary)
            .collect();
        let models: Vec<&ModelPart> = vocabularies.iter().map(|e| &e.model).collect();
        VocabularySet::from_parts(&models).map_err(|_| PartSourceError::Corrupt {
            part_identifier: vocabularies
                .get(1)
                .or_else(|| vocabularies.first())
                .map(|e| e.sourced.part.identifier.clone())
                .unwrap_or_default(),
        })
    }

    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError> {
        let Some(target) = self
            .entries
            .iter()
            .position(|e| e.sourced.part.id == part.part.id)
        else {
            return Ok(Vec::new());
        };
        let mut users: Vec<&Entry> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(index, e)| {
                *index != target
                    && e.sourced.part.kind == PartKind::WorkspaceProfile
                    && self.closure(*index).contains(&target)
            })
            .map(|(_, e)| e)
            .collect();
        users.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        Ok(users.into_iter().map(|e| e.sourced.clone()).collect())
    }

    async fn citing_parts(
        &self,
        repository: &RepositoryRef,
        axis: VocabularyAxis,
        code: &str,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        if *repository != self.here() {
            return Ok(Vec::new());
        }
        let mut cited: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| {
                let meta = &e.sourced.manifest.metadata;
                match axis {
                    VocabularyAxis::Category => meta.category == code,
                    VocabularyAxis::PackClass => meta.class == code,
                    VocabularyAxis::AudienceRole => meta.audience.roles.iter().any(|r| r == code),
                    _ => false,
                }
            })
            .collect();
        cited.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        Ok(cited.into_iter().map(|e| e.sourced.clone()).collect())
    }
}
