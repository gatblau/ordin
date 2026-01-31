//! The ordin plug-in: lossless read and write (FormatPlugins.md *Internal Logic* 2, 3, 4).
//!
//! The plug-in delegates to the Agent Model's tree reading and file placement, so the ordin format
//! has one implementation. A part keeps its original bytes: writing an unchanged part back gives the
//! same file (ADR-087).

use std::collections::BTreeMap;
use std::time::Instant;

use ordin_agent_model::{
    markdown_body, parse_manifest, read_tree, render_part, ModelPart, PartKind, VocabularyAxis,
    VocabularySet, API_VERSION, IMPORTED_TERM,
};

use crate::error::FormatError;
use crate::front::{dir_name, file_name, join};
use crate::guard::guard;
use crate::log;
use crate::types::{
    Detection, FormatId, FormatPlugin, FormatVersions, LossReport, MappingItem, MappingReport,
    ReadOutcome, ReadRequest, SourceTree, UnmappedReason, WriteOutcome, WriteRequest,
};

pub(crate) struct OrdinPlugin;

/// True for `models/@<publisher>/<something>` with a publisher matching `[a-z][a-z0-9-]*`.
fn under_models(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("models/@") else {
        return false;
    };
    let Some((publisher, tail)) = rest.split_once('/') else {
        return false;
    };
    let mut chars = publisher.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !tail.is_empty()
}

impl FormatPlugin for OrdinPlugin {
    fn id(&self) -> FormatId {
        FormatId::Ordin
    }

    fn versions(&self) -> FormatVersions {
        FormatVersions {
            reads: vec![API_VERSION.to_owned()],
            writes: vec![API_VERSION.to_owned()],
        }
    }

    fn detect(&self, tree: &SourceTree) -> Option<Detection> {
        let vocabulary = VocabularySet::default();
        let mut found = false;
        let mut agents: u32 = 0;
        for (path, bytes) in tree.0.iter().filter(|(path, _)| under_models(path)) {
            if let Ok(Some(manifest)) = parse_manifest(path, bytes) {
                if manifest.api_version == API_VERSION {
                    found = true;
                    if manifest.kind(&vocabulary) == Some(PartKind::WorkspaceProfile) {
                        agents = agents.saturating_add(1);
                    }
                }
            }
        }
        found.then(|| Detection {
            format: FormatId::Ordin,
            declared_version: API_VERSION.to_owned(),
            version_tested: true,
            agent_count: agents,
        })
    }

    fn read(&self, tree: &SourceTree, request: &ReadRequest) -> Result<ReadOutcome, FormatError> {
        guard(tree)?;
        let reading = read_tree(&tree.0, &request.vocabulary);
        let mut items: Vec<MappingItem> = reading
            .refused
            .iter()
            .map(|(path, error)| MappingItem {
                source_path: path.clone(),
                construct: error.code().to_owned(),
                reason: UnmappedReason::Unreadable,
                part_identifier: None,
            })
            .collect();
        items.extend(reading.unowned_files.iter().map(|path| MappingItem {
            source_path: path.clone(),
            construct: "file".to_owned(),
            reason: UnmappedReason::UnrecognisedPath,
            part_identifier: None,
        }));
        Ok(ReadOutcome {
            parts: reading.parts,
            mapping_report: MappingReport {
                format: Some(FormatId::Ordin),
                items,
            },
            package: None,
        })
    }

    fn write(&self, request: &WriteRequest) -> Result<WriteOutcome, FormatError> {
        let started = Instant::now();
        let result = write_parts(request);
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match &result {
            Ok(outcome) => log::write_ok(
                FormatId::Ordin,
                request.parts.len(),
                outcome.starting_files.len(),
                0,
                elapsed,
            ),
            Err(error) => log::write_failed(FormatId::Ordin, error, elapsed),
        }
        result
    }
}

fn part_invalid(identifier: &str, code: &str) -> FormatError {
    FormatError::PartInvalid {
        identifier: identifier.to_owned(),
        code: code.to_owned(),
    }
}

/// True when the part's manifest still equals the one its original file parses to, and no body
/// override differs from the original body: the original bytes are then the file to write.
fn unchanged(part: &ModelPart, bodies: &BTreeMap<String, String>) -> bool {
    let Some(bytes) = part.files.0.get(file_name(&part.manifest_path)) else {
        return false;
    };
    match parse_manifest(&part.manifest_path, bytes) {
        Ok(Some(manifest)) if manifest == part.manifest => {}
        _ => return false,
    }
    match bodies.get(&part.manifest.metadata.id) {
        None => true,
        Some(body) => markdown_body(&part.manifest_path, bytes).unwrap_or_default() == *body,
    }
}

/// The files of one part: its original bytes when unchanged, otherwise the Agent Model's rendering
/// at the Agent Model's placement, with the extra files beside it.
pub(crate) fn emit_part(
    part: &ModelPart,
    bodies: &BTreeMap<String, String>,
) -> Result<Vec<(String, Vec<u8>)>, FormatError> {
    let id = part.manifest.metadata.id.as_str();
    if unchanged(part, bodies) {
        let dir = dir_name(&part.manifest_path);
        return Ok(part
            .files
            .0
            .iter()
            .map(|(name, bytes)| (join(dir, name), bytes.clone()))
            .collect());
    }
    let own = file_name(&part.manifest_path);
    let body = bodies.get(id).cloned().or_else(|| {
        part.files
            .0
            .get(own)
            .and_then(|bytes| markdown_body(&part.manifest_path, bytes))
    });
    let rendered = render_part(part.kind, &part.manifest, body.as_deref())
        .map_err(|e| part_invalid(id, e.code()))?;
    let dir = dir_name(&rendered.path).to_owned();
    let mut out = vec![(rendered.path.clone(), rendered.content)];
    for (name, bytes) in &part.files.0 {
        if name != own {
            out.push((join(&dir, name), bytes.clone()));
        }
    }
    Ok(out)
}

/// The axes a part cites through `metadata.category`, `metadata.class` and `metadata.audience.roles`.
fn cited_axes(part: &ModelPart) -> Vec<VocabularyAxis> {
    let meta = &part.manifest.metadata;
    let mut axes = Vec::new();
    // The built-in `imported` term needs no vocabulary part in the target (FU-097).
    if !meta.category.is_empty() && meta.category != IMPORTED_TERM {
        axes.push(VocabularyAxis::Category);
    }
    if !meta.class.is_empty() {
        axes.push(VocabularyAxis::PackClass);
    }
    if meta.audience.roles.iter().any(|role| role != IMPORTED_TERM) {
        axes.push(VocabularyAxis::AudienceRole);
    }
    axes
}

fn vocabulary_axis(part: &ModelPart) -> Option<VocabularyAxis> {
    if part.kind != PartKind::Vocabulary {
        return None;
    }
    part.manifest
        .vocabulary()
        .ok()
        .flatten()
        .map(|body| body.axis)
}

fn write_parts(request: &WriteRequest) -> Result<WriteOutcome, FormatError> {
    guard(&request.existing)?;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for part in &request.parts {
        files.extend(emit_part(part, &request.bodies)?);
    }
    // Starting files (ADR-088): the vocabulary of every axis a written part cites and the target
    // lacks.
    let existing = read_tree(&request.existing.0, &VocabularySet::default());
    let present: Vec<VocabularyAxis> = existing
        .parts
        .iter()
        .chain(request.parts.iter())
        .filter_map(vocabulary_axis)
        .collect();
    let mut starting_files = BTreeMap::new();
    let mut handled: Vec<VocabularyAxis> = Vec::new();
    for part in &request.parts {
        for axis in cited_axes(part) {
            if present.contains(&axis) || handled.contains(&axis) {
                continue;
            }
            let Some(vocabulary) = request
                .starting_vocabulary
                .iter()
                .find(|v| vocabulary_axis(v) == Some(axis))
            else {
                return Err(part_invalid(
                    &part.manifest.metadata.id,
                    "starting_vocabulary_missing",
                ));
            };
            handled.push(axis);
            starting_files.extend(emit_part(vocabulary, &BTreeMap::new())?);
        }
    }
    Ok(WriteOutcome {
        files,
        starting_files,
        loss_report: LossReport {
            format: Some(FormatId::Ordin),
            items: Vec::new(),
        },
    })
}
