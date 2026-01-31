//! Section B: the context of a check — the vocabulary first, then the repository's parts with the
//! change applied (Checker.md *Internal Logic* 5–6).

use std::collections::BTreeSet;
use std::sync::Arc;

use ordin_agent_model::{
    AmError, Manifest, ModelPart, PartFiles, PartKind, PartSource, RepositoryRef, SourceOverride,
    SourceScope, SourcedPart, StagedPart, VocabularySet,
};
use ordin_resolver::{Replacement, ResolveRequest};
use ordin_types::domain::HarnessKind;

use crate::error::{from_source, CheckError};
use crate::finding::part_failure;
use crate::model::{Finding, RepositoryCheck};

/// One part as the checks see it, from the source or from the change.
#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub kind: PartKind,
    pub manifest: Manifest,
    /// Repository-relative path of the manifest file.
    pub path: String,
    /// The files, for a staged part; None for a part read from the source.
    pub files: Option<PartFiles>,
    /// True for a part of the change.
    pub changed: bool,
    /// The source's row, for a part read from the source.
    pub sourced: Option<SourcedPart>,
}

impl Item {
    pub(crate) fn from_sourced(sourced: SourcedPart) -> Item {
        Item {
            kind: sourced.part.kind,
            manifest: sourced.manifest.clone(),
            path: sourced.part.file_path.clone(),
            files: None,
            changed: false,
            sourced: Some(sourced),
        }
    }

    pub(crate) fn from_staged(staged: &StagedPart) -> Item {
        Item {
            kind: staged.part.kind,
            manifest: staged.part.manifest.clone(),
            path: staged.part.manifest_path.clone(),
            files: Some(staged.part.files.clone()),
            changed: true,
            sourced: None,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.manifest.metadata.id
    }

    pub(crate) fn name(&self) -> &str {
        crate::finding::display_name(&self.manifest.metadata.name, &self.manifest.metadata.id)
    }

    /// A model part over the manifest, for the Agent Model's readers that take one.
    pub(crate) fn model_part(&self) -> ModelPart {
        ModelPart {
            kind: self.kind,
            manifest: self.manifest.clone(),
            manifest_path: self.path.clone(),
            files: self.files.clone().unwrap_or_default(),
            content_digest: self
                .sourced
                .as_ref()
                .map(|s| s.part.content_digest.clone())
                .unwrap_or_else(|| ordin_agent_model::Sha256Hex(String::new())),
        }
    }
}

/// The context of a repository check.
pub(crate) struct Gathered {
    pub vocabulary: VocabularySet,
    /// The repository before the change.
    pub before: Vec<Item>,
    /// The repository with the change applied.
    pub union: Vec<Item>,
    /// Findings of the context itself (`sync_duplicate_vocabulary_axis`).
    pub findings: Vec<Finding>,
}

/// A vocabulary set over `parts`, dropping every axis two parts define (step 5): each such axis is
/// one `sync_duplicate_vocabulary_axis` finding and an empty set for that axis.
pub(crate) fn vocabulary_of(parts: &[ModelPart]) -> (VocabularySet, Vec<Finding>) {
    let mut kept: Vec<&ModelPart> = parts
        .iter()
        .filter(|p| p.kind == PartKind::Vocabulary)
        .collect();
    let mut findings = Vec::new();
    loop {
        match VocabularySet::from_parts(&kept) {
            Ok(set) => return (set, findings),
            Err(AmError::VocabularyAxisDuplicate { axis }) => {
                let same_axis = |p: &ModelPart| {
                    p.manifest
                        .vocabulary()
                        .ok()
                        .flatten()
                        .is_some_and(|b| b.axis == axis)
                };
                if let Some(second) = kept.iter().filter(|p| same_axis(p)).nth(1) {
                    findings.push(part_failure(
                        "sync_duplicate_vocabulary_axis",
                        &second.manifest.metadata.id,
                        crate::finding::display_name(
                            &second.manifest.metadata.name,
                            &second.manifest.metadata.id,
                        ),
                        Vec::new(),
                        format!("two vocabulary parts define the axis {axis:?}"),
                    ));
                }
                kept.retain(|p| !same_axis(p));
            }
            Err(_) => {
                // A malformed vocabulary body: that part is dropped from the set.
                match kept.iter().position(|p| p.manifest.vocabulary().is_err()) {
                    Some(at) => {
                        kept.remove(at);
                    }
                    None => return (VocabularySet::default(), findings),
                }
            }
        }
    }
}

/// Step 6: the snapshot with the change applied — each staged part shadows the part of the same kind
/// and identifier, and deleted identifiers are removed.
pub(crate) fn apply_change(
    before: &[Item],
    changes: &[StagedPart],
    deletions: &[String],
) -> Vec<Item> {
    let staged: Vec<Item> = changes.iter().map(Item::from_staged).collect();
    let shadowed: BTreeSet<(PartKind, String)> =
        staged.iter().map(|i| (i.kind, i.id().to_owned())).collect();
    let deleted: BTreeSet<&str> = deletions.iter().map(String::as_str).collect();
    before
        .iter()
        .filter(|i| !shadowed.contains(&(i.kind, i.id().to_owned())) && !deleted.contains(i.id()))
        .cloned()
        .chain(staged)
        .collect()
}

/// Steps 5–6 for a repository check.
// A `VocabularySet` cannot be edited axis by axis, so when the change holds Vocabulary
// parts the set is rebuilt from the repository's own vocabulary parts with the change applied (the
// borrowed repositories' vocabularies are then not part of it); without one, the source's set for the
// scope is used unchanged.
// spec: ordin-desktop/components/Checker#ADR-091
pub(crate) async fn gather(
    source: &dyn PartSource,
    request: &RepositoryCheck,
) -> Result<Gathered, CheckError> {
    let before: Vec<Item> = source
        .repository_parts(&request.repository)
        .await
        .map_err(from_source)?
        .into_iter()
        .map(Item::from_sourced)
        .collect();
    let union = apply_change(&before, &request.changes, &request.deletions);
    let changes_vocabulary = request
        .changes
        .iter()
        .any(|s| s.part.kind == PartKind::Vocabulary);
    let (vocabulary, findings) = if changes_vocabulary {
        let parts: Vec<ModelPart> = union
            .iter()
            .filter(|i| i.kind == PartKind::Vocabulary)
            .map(Item::model_part)
            .collect();
        vocabulary_of(&parts)
    } else {
        let set = source
            .vocabulary(&request.scope)
            .await
            .map_err(from_source)?;
        (set, Vec::new())
    };
    Ok(Gathered {
        vocabulary,
        before,
        union,
        findings,
    })
}

/// The source of one operation: the request's own, else the bound one.
pub(crate) fn pick_source(
    request: Option<&SourceOverride>,
    bound: Option<&Arc<dyn PartSource>>,
) -> Option<Arc<dyn PartSource>> {
    request
        .map(|s| Arc::clone(&s.0))
        .or_else(|| bound.map(Arc::clone))
}

/// A resolve request for the Checker's own resolutions, always naming the source so the Resolver
/// reads the same parts the Checker does.
// The harness and interface language are `claude_code` and `en-GB`. Neither changes which
// constituents an agent's closure holds.
pub(crate) fn resolve_request(
    scope: &SourceScope,
    root_id: &str,
    staged: Vec<StagedPart>,
    replacements: Vec<Replacement>,
    allow_draft_root: bool,
    source: &Arc<dyn PartSource>,
) -> ResolveRequest {
    ResolveRequest {
        scope: scope.clone(),
        root_id: root_id.to_owned(),
        root_version: None,
        target_harness: HarnessKind::ClaudeCode,
        workspace_languages: Vec::new(),
        interface_language: "en-GB".to_owned(),
        pack_answers: Vec::new(),
        pack_changes: Vec::new(),
        overlay_changes: Vec::new(),
        replacements,
        staged,
        allow_draft_root,
        source: Some(SourceOverride(Arc::clone(source))),
    }
}

/// The scope an agent of `repository` resolves through: its own repository first, then the profile
/// repositories and the bundled catalogue of `scope`.
pub(crate) fn scope_of(repository: &RepositoryRef, scope: &SourceScope) -> SourceScope {
    SourceScope {
        home: repository.clone(),
        profile: scope.profile.clone(),
        include_bundled: scope.include_bundled,
    }
}
