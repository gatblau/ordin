//! A part as the resolver holds it, and the reader that finds parts for one request (ResolverAndCompiler.md
//! *Internal Logic* 1, 3, 7 and 8).

use std::collections::HashMap;

use ordin_agent_model::{
    compare_versions, content_digest, kind_info, prefixed_digest, LoadingMode, Manifest, Part,
    PartFiles, PartId, PartKind, PartRef, PartSource, PartStatus, RepositoryRef, Resolution,
    Sha256Hex, SourceScope, SourcedPart, StagedPart, WithdrawnListEntry, BUNDLED_REPOSITORY_ID,
};

use crate::error::{manifest_invalid, map_source_error, ResolveError};
use crate::log;
use crate::request::Replacement;

/// A staged part has no commit until it is merged: forty `0`s in plan and lock.
const STAGED_COMMIT: &str = "0000000000000000000000000000000000000000";

/// The most replacement hops followed for one withdrawn part.
const MAX_REPLACEMENT_HOPS: usize = 16;

/// One part of the closure: a source part, or a draft's staged part presented in the same shape.
#[derive(Debug, Clone)]
pub(crate) struct Held {
    pub sourced: SourcedPart,
    /// The content digest the plan records (a staged part's is computed from its files).
    pub digest: Sha256Hex,
    /// The files of a staged part; a source part's files are read through `PartSource::files`.
    pub files: Option<PartFiles>,
    pub staged: bool,
}

/// The key a node of the reference graph is met by.
pub(crate) type NodeKey = (PartKind, String, String);

impl Held {
    pub(crate) fn from_sourced(sourced: SourcedPart) -> Held {
        let digest = sourced.part.content_digest.clone();
        Held {
            sourced,
            digest,
            files: None,
            staged: false,
        }
    }

    pub(crate) fn from_staged(staged: &StagedPart) -> Result<Held, ResolveError> {
        let manifest = &staged.part.manifest;
        let id = manifest.metadata.id.clone();
        let status = manifest
            .status_or_default()
            .map_err(|error| manifest_invalid(&id, &error))?;
        let digest = content_digest(&staged.part.files);
        let agent_repository_id = match &staged.repository {
            RepositoryRef::Repository(repository) => *repository,
            RepositoryRef::Bundled => BUNDLED_REPOSITORY_ID,
        };
        let part = Part {
            id: PartId::new(),
            agent_repository_id,
            kind: staged.part.kind,
            identifier: id,
            name: manifest.metadata.name.clone(),
            version: manifest.metadata.version.clone(),
            status,
            supersedes: manifest.metadata.supersedes.clone(),
            owner: manifest.metadata.owner.clone(),
            file_path: staged.part.manifest_path.clone(),
            commit: STAGED_COMMIT.to_owned(),
            content_digest: digest.clone(),
            vocabulary_terms_cited: Vec::new(),
            uses: Vec::new(),
            withdrawn: false,
        };
        Ok(Held {
            sourced: SourcedPart {
                part,
                repository: staged.repository.clone(),
                repository_label: staged.repository_label.clone(),
                repository_display_name: staged.repository_label.clone(),
                manifest: manifest.clone(),
            },
            digest,
            files: Some(staged.part.files.clone()),
            staged: true,
        })
    }

    pub(crate) fn kind(&self) -> PartKind {
        self.sourced.part.kind
    }

    pub(crate) fn id(&self) -> &str {
        &self.sourced.part.identifier
    }

    pub(crate) fn version(&self) -> &str {
        &self.sourced.part.version
    }

    pub(crate) fn status(&self) -> PartStatus {
        self.sourced.part.status
    }

    pub(crate) fn manifest(&self) -> &Manifest {
        &self.sourced.manifest
    }

    /// `slug_from_id(metadata.id)`.
    pub(crate) fn name(&self) -> String {
        self.sourced.manifest.slug()
    }

    pub(crate) fn key(&self) -> NodeKey {
        (self.kind(), self.id().to_owned(), self.version().to_owned())
    }

    /// The loading mode a part declares, else its kind's default.
    pub(crate) fn loading(&self) -> LoadingMode {
        parse_loading(self.sourced.manifest.metadata.loading.as_deref())
            .unwrap_or(kind_info(self.kind()).default_loading)
    }

    /// Step 12: the reference a plan, bundle manifest or lock file holds.
    pub(crate) fn part_ref(&self) -> PartRef {
        PartRef {
            kind: self.kind(),
            id: self.id().to_owned(),
            name: self.name(),
            version: self.version().to_owned(),
            content_hash: prefixed_digest(&self.digest),
            repository: self.sourced.repository_label.clone(),
            commit: self.sourced.part.commit.clone(),
            loading: parse_loading(self.sourced.manifest.metadata.loading.as_deref()),
        }
    }
}

/// `metadata.loading` as the format spells it (`always-on`), or None when absent or unknown.
fn parse_loading(text: Option<&str>) -> Option<LoadingMode> {
    let text = text?;
    serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
}

/// The files of one part: a staged part's own, otherwise the source's. The caller owns the digest check
/// of `PartSource::files`.
// spec: ordin-desktop/components/ResolverAndCompiler#SA-RECON-04
pub(crate) async fn files_of(
    source: &dyn PartSource,
    held: &Held,
) -> Result<PartFiles, ResolveError> {
    let files = match &held.files {
        Some(files) => files.clone(),
        None => source
            .files(&held.sourced)
            .await
            .map_err(map_source_error)?,
    };
    if content_digest(&files) != held.digest {
        log::digest_mismatch(held.id(), &held.sourced.repository_label);
        return Err(ResolveError::DigestMismatch {
            part: held.id().to_owned(),
        });
    }
    Ok(files)
}

/// Finds parts for one request: staged parts shadow source parts, errors map to `ResolveError`, and
/// every lookup is remembered for the request.
pub(crate) struct Reader<'a> {
    source: &'a dyn PartSource,
    scope: &'a SourceScope,
    staged: &'a [StagedPart],
    replacements: &'a [Replacement],
    cache: HashMap<(Option<PartKind>, String, Option<String>), Option<Held>>,
    withdrawn: Vec<WithdrawnListEntry>,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(
        source: &'a dyn PartSource,
        scope: &'a SourceScope,
        staged: &'a [StagedPart],
        replacements: &'a [Replacement],
    ) -> Reader<'a> {
        Reader {
            source,
            scope,
            staged,
            replacements,
            cache: HashMap::new(),
            withdrawn: Vec::new(),
        }
    }

    pub(crate) fn withdrawn_entries(&self) -> &[WithdrawnListEntry] {
        &self.withdrawn
    }

    /// Step 2: the withdrawn entries in force. A list that cannot be read fails closed.
    pub(crate) async fn load_withdrawn(&mut self) -> Result<(), ResolveError> {
        self.withdrawn = self
            .source
            .withdrawn(self.scope)
            .await
            .map_err(map_source_error)?;
        Ok(())
    }

    /// The staged part that shadows `(kind, id)` at `version`, the highest version when several do.
    fn staged_match(
        &self,
        kind: Option<PartKind>,
        id: &str,
        version: Option<&str>,
    ) -> Option<&'a StagedPart> {
        let staged: &'a [StagedPart] = self.staged;
        staged
            .iter()
            .filter(|candidate| {
                let metadata = &candidate.part.manifest.metadata;
                metadata.id == id
                    && kind.is_none_or(|k| candidate.part.kind == k)
                    && version.is_none_or(|v| metadata.version == v)
            })
            .max_by(|a, b| {
                compare_versions(
                    &a.part.manifest.metadata.version,
                    &b.part.manifest.metadata.version,
                )
                .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// One lookup. `Ok(None)` is "resolves to nothing"; an ambiguous identifier is an error.
    pub(crate) async fn lookup(
        &mut self,
        kind: Option<PartKind>,
        id: &str,
        version: Option<&str>,
    ) -> Result<Option<Held>, ResolveError> {
        let key = (kind, id.to_owned(), version.map(str::to_owned));
        if let Some(hit) = self.cache.get(&key) {
            return Ok(hit.clone());
        }
        let found = self.lookup_uncached(kind, id, version).await?;
        self.cache.insert(key, found.clone());
        Ok(found)
    }

    async fn lookup_uncached(
        &self,
        kind: Option<PartKind>,
        id: &str,
        version: Option<&str>,
    ) -> Result<Option<Held>, ResolveError> {
        if let Some(staged) = self.staged_match(kind, id, version) {
            return Held::from_staged(staged).map(Some);
        }
        let resolution = match kind {
            Some(kind) => self.source.resolve(self.scope, kind, id, version).await,
            None => self.source.resolve_any_kind(self.scope, id).await,
        }
        .map_err(map_source_error)?;
        match resolution {
            Resolution::Found(part) => Ok(Some(Held::from_sourced(part))),
            Resolution::NotFound => Ok(None),
            Resolution::Ambiguous { repositories } => Err(ResolveError::SourceAmbiguous {
                id: id.to_owned(),
                repositories,
            }),
        }
    }

    /// Step 3: every visible PracticePack and overlay, with staged ones shadowing by (kind, id).
    pub(crate) async fn ingredients(&mut self) -> Result<Vec<Held>, ResolveError> {
        let listed = self
            .source
            .ingredients(self.scope)
            .await
            .map_err(map_source_error)?;
        let mut out: Vec<Held> = listed.into_iter().map(Held::from_sourced).collect();
        for staged in self.staged {
            let family = kind_info(staged.part.kind).family;
            let is_ingredient = matches!(
                family,
                ordin_agent_model::KindFamily::Ingredient | ordin_agent_model::KindFamily::Overlay
            );
            if !is_ingredient {
                continue;
            }
            let held = Held::from_staged(staged)?;
            match out
                .iter()
                .position(|h| h.kind() == held.kind() && h.id() == held.id())
            {
                Some(at) => out[at] = held,
                None => out.push(held),
            }
        }
        Ok(out)
    }

    /// The withdrawn entry that names `held`: its kind and identifier, and either no version (every
    /// version is withdrawn) or this version and digest. An entry with a replacement is preferred.
    fn withdrawal_of(&self, held: &Held) -> Option<&WithdrawnListEntry> {
        let mut matching = self.withdrawn.iter().filter(|entry| {
            entry.part_kind == held.kind()
                && entry.part_identifier == held.id()
                && entry.part_version.as_ref().is_none_or(|version| {
                    version == held.version()
                        && entry
                            .part_content_digest
                            .as_ref()
                            .is_none_or(|digest| *digest == held.digest)
                })
        });
        let first = matching.next()?;
        if first.replacement_identifier.is_some() {
            return Some(first);
        }
        matching
            .find(|entry| entry.replacement_identifier.is_some())
            .or(Some(first))
    }

    /// Step 8 for one part: a part that is not withdrawn passes; a withdrawn one is replaced by the
    /// request's `Replacement` for it, else the agent stops.
    pub(crate) async fn admit(&mut self, held: Held) -> Result<Held, ResolveError> {
        let mut current = held;
        for _ in 0..MAX_REPLACEMENT_HOPS {
            let entry = self.withdrawal_of(&current);
            if !current.sourced.part.withdrawn && entry.is_none() {
                return Ok(current);
            }
            let replacement_named = entry.and_then(|e| e.replacement_identifier.clone());
            let kind = current.kind();
            let wanted = self
                .replacements
                .iter()
                .find(|r| r.withdrawn_kind == kind && r.withdrawn_id == current.id())
                .cloned();
            let Some(wanted) = wanted else {
                return Err(ResolveError::PartWithdrawn {
                    part: current.id().to_owned(),
                    replacement: replacement_named,
                });
            };
            let from = current.id().to_owned();
            // A replacement has no relation key, so the word "replacement" is written into
            // `ReferenceUnresolved.relation` when it resolves to nothing.
            current = self
                .lookup(
                    Some(kind),
                    &wanted.replacement_id,
                    wanted.replacement_version.as_deref(),
                )
                .await?
                .ok_or(ResolveError::ReferenceUnresolved {
                    from,
                    id: wanted.replacement_id.clone(),
                    relation: "replacement".to_owned(),
                })?;
        }
        Err(ResolveError::PartWithdrawn {
            part: current.id().to_owned(),
            replacement: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_commit_is_forty_zeros() {
        assert_eq!(STAGED_COMMIT.len(), 40);
        assert!(STAGED_COMMIT.bytes().all(|b| b == b'0'));
    }

    #[test]
    fn loading_spellings() {
        assert_eq!(
            parse_loading(Some("always-on")),
            Some(LoadingMode::AlwaysOn)
        );
        assert_eq!(
            parse_loading(Some("registered-invocation")),
            Some(LoadingMode::RegisteredInvocation)
        );
        assert_eq!(parse_loading(Some("sometimes")), None);
        assert_eq!(parse_loading(None), None);
    }
}
