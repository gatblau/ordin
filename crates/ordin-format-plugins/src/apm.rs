//! The APM plug-in: read and write (FormatPlugins.md *Internal Logic* 2, 5, 6).
//!
//! The layout is pinned as the spec states it (C-62 fallback): the tested-version list is empty, so
//! every package reads as `unversioned` and `version_tested = false`, and a write to a repository that
//! already holds an `apm.yml` is refused with `FMT_VERSION_UNTESTED` (R-70).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use ordin_agent_model::{
    kind_info, markdown_body, slug_from_id, LoadingMode, ModelPart, PartKind, TaskInputKind, Urn,
    YamlValue,
};
use ordin_types::yaml::{read_restricted_node, YamlError, YamlNode};

use crate::error::FormatError;
use crate::front::{default_name, file_name, scalar, yaml_scalar};
use crate::guard::{guard, MAX_FILE_BYTES};
use crate::import::{IdSpec, Importer, MdSpec, Refs};
use crate::log;
use crate::types::{
    Detection, FormatId, FormatPlugin, FormatVersions, LossItem, LossReason, LossReport,
    MappingReport, PackageIdentity, ReadOutcome, ReadRequest, SourceTree, UnmappedReason,
    WriteOutcome, WriteRequest,
};

/// The `apm.yml` versions this plug-in was tested with. Empty until the C-62 lab reports.
pub(crate) const APM_TESTED_VERSIONS: &[&str] = &[];
const UNVERSIONED: &str = "unversioned";
const MANIFEST: &str = "apm.yml";

pub(crate) struct ApmPlugin;

impl FormatPlugin for ApmPlugin {
    fn id(&self) -> FormatId {
        FormatId::Apm
    }

    fn versions(&self) -> FormatVersions {
        let tested: Vec<String> = APM_TESTED_VERSIONS
            .iter()
            .map(|v| (*v).to_owned())
            .collect();
        FormatVersions {
            reads: tested.clone(),
            writes: tested,
        }
    }

    fn detect(&self, tree: &SourceTree) -> Option<Detection> {
        let bytes = tree.0.get(MANIFEST)?;
        let text = std::str::from_utf8(bytes).ok()?;
        match read_restricted_node(text, MAX_FILE_BYTES) {
            Ok(YamlNode::Map(_)) => Some(Detection {
                format: FormatId::Apm,
                declared_version: UNVERSIONED.to_owned(),
                version_tested: APM_TESTED_VERSIONS.contains(&UNVERSIONED),
                agent_count: 1,
            }),
            _ => None,
        }
    }

    fn read(&self, tree: &SourceTree, request: &ReadRequest) -> Result<ReadOutcome, FormatError> {
        guard(tree)?;
        let package = read_package(tree)?;
        let mut importer = Importer::new(&request.publisher, &package.identity.version);
        importer.check_publisher()?;
        let workspace_id = importer
            .mint(PartKind::WorkspaceProfile, None, &package.identity.name)
            .ok_or_else(|| invalid_manifest(None))?;
        for key in &package.other_keys {
            importer.item(
                MANIFEST,
                &format!("apm.yml key {key}"),
                UnmappedReason::UnknownField,
                None,
            );
        }
        let mut refs = Refs::default();
        let mut skills: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
        let mut agent_seen = false;
        for (path, bytes) in &tree.0 {
            if path == MANIFEST {
                continue;
            }
            match classify(path) {
                Entry::Instruction(slug) => {
                    let created = importer.md_part(md(
                        PartKind::Rule,
                        path,
                        bytes,
                        Some("imported"),
                        slug,
                    ))?;
                    if let Some(created) = created {
                        refs.rules.push(created.id);
                    }
                }
                Entry::Prompt(slug) => {
                    let mut spec = md(PartKind::Command, path, bytes, Some("imported"), slug);
                    spec.prompt_inputs = true;
                    let created = importer.md_part(spec)?;
                    if let Some(created) = created {
                        refs.commands.push(created.id);
                    }
                }
                Entry::Context(slug) => {
                    let created = importer.md_part(md(
                        PartKind::ContextResource,
                        path,
                        bytes,
                        Some("imported"),
                        slug,
                    ))?;
                    if let Some(created) = created {
                        refs.contexts.push(created.id);
                    }
                }
                Entry::Chatmode(slug) => {
                    let created =
                        importer.md_part(md(PartKind::OperatingMode, path, bytes, None, slug))?;
                    if let Some(created) = created {
                        refs.modes.push(created.id);
                    }
                }
                Entry::Agent(slug) => {
                    if agent_seen {
                        importer.item(path, "agent", UnmappedReason::NoEquivalentKind, None);
                        continue;
                    }
                    agent_seen = true;
                    let spec = MdSpec {
                        kind: PartKind::SystemRole,
                        path,
                        bytes,
                        id: IdSpec::Fixed(format!("{workspace_id}:system-role")),
                        default_slug: slug,
                        known_keys: &[],
                        extras: BTreeMap::new(),
                        whole_text: false,
                        prompt_inputs: false,
                    };
                    if let Some(created) = importer.md_part(spec)? {
                        refs.system_role = Some(created.id);
                    }
                }
                Entry::SkillFile(slug, rest) => {
                    skills
                        .entry(slug.to_owned())
                        .or_default()
                        .insert(rest.to_owned(), bytes.clone());
                }
                Entry::Hook => importer.item(path, "hook", UnmappedReason::Executable, None),
                Entry::Other => importer.item(path, "file", UnmappedReason::UnrecognisedPath, None),
            }
        }
        for (slug, mut files) in skills {
            let Some(skill_md) = files.remove("SKILL.md") else {
                for rest in files.keys() {
                    let path = format!(".apm/skills/{slug}/{rest}");
                    importer.item(&path, "file", UnmappedReason::UnrecognisedPath, None);
                }
                continue;
            };
            let path = format!(".apm/skills/{slug}/SKILL.md");
            let spec = MdSpec {
                kind: PartKind::Skill,
                path: &path,
                bytes: &skill_md,
                id: IdSpec::Mint {
                    category: Some("imported"),
                    slug: &slug,
                },
                default_slug: &slug,
                known_keys: &[],
                extras: files,
                whole_text: false,
                prompt_inputs: false,
            };
            if let Some(created) = importer.md_part(spec)? {
                refs.skills.push(created.id);
            }
        }
        importer.workspace(
            &workspace_id,
            &package.identity.name,
            package.description.as_deref().unwrap_or(""),
            &refs,
        )?;
        let (parts, items) = importer.finish();
        Ok(ReadOutcome {
            parts,
            mapping_report: MappingReport {
                format: Some(FormatId::Apm),
                items,
            },
            package: Some(package.identity),
        })
    }

    fn write(&self, request: &WriteRequest) -> Result<WriteOutcome, FormatError> {
        let started = Instant::now();
        let result = write_package(request);
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match &result {
            Ok(outcome) => log::write_ok(
                FormatId::Apm,
                request.parts.len(),
                outcome.starting_files.len(),
                outcome.loss_report.items.len(),
                elapsed,
            ),
            Err(error) => log::write_failed(FormatId::Apm, error, elapsed),
        }
        result
    }
}

fn md<'a>(
    kind: PartKind,
    path: &'a str,
    bytes: &'a [u8],
    category: Option<&'a str>,
    slug: &'a str,
) -> MdSpec<'a> {
    let id = match kind {
        PartKind::OperatingMode => IdSpec::Mint {
            category: None,
            slug,
        },
        _ => IdSpec::Mint { category, slug },
    };
    MdSpec {
        kind,
        path,
        bytes,
        id,
        default_slug: slug,
        known_keys: &[],
        extras: BTreeMap::new(),
        whole_text: false,
        prompt_inputs: false,
    }
}

// ---- reading ----

enum Entry<'a> {
    Instruction(&'a str),
    Prompt(&'a str),
    Agent(&'a str),
    Context(&'a str),
    Chatmode(&'a str),
    SkillFile(&'a str, &'a str),
    Hook,
    Other,
}

/// Step 5, the layout table.
fn classify(path: &str) -> Entry<'_> {
    let Some(rest) = path.strip_prefix(".apm/") else {
        return Entry::Other;
    };
    let Some((dir, tail)) = rest.split_once('/') else {
        return Entry::Other;
    };
    let single = |suffix: &str| -> Option<&str> {
        tail.strip_suffix(suffix)
            .filter(|slug| !slug.is_empty() && !slug.contains('/'))
    };
    match dir {
        "instructions" => single(".instructions.md").map_or(Entry::Other, Entry::Instruction),
        "prompts" => single(".prompt.md").map_or(Entry::Other, Entry::Prompt),
        "agents" => single(".agent.md").map_or(Entry::Other, Entry::Agent),
        "context" => single(".context.md").map_or(Entry::Other, Entry::Context),
        "chatmodes" => single(".chatmode.md").map_or(Entry::Other, Entry::Chatmode),
        "skills" => match tail.split_once('/') {
            Some((slug, inner)) if !slug.is_empty() && !inner.is_empty() => {
                Entry::SkillFile(slug, inner)
            }
            _ => Entry::Other,
        },
        "hooks" => Entry::Hook,
        _ => Entry::Other,
    }
}

struct Package {
    identity: PackageIdentity,
    description: Option<String>,
    other_keys: Vec<String>,
}

fn invalid_manifest(line: Option<u32>) -> FormatError {
    FormatError::PackageManifestInvalid {
        path: MANIFEST.to_owned(),
        line,
    }
}

/// Step 5: `apm.yml` through the restricted reader; its refusals map to `FMT_PACKAGE_MANIFEST_INVALID`.
fn read_package(tree: &SourceTree) -> Result<Package, FormatError> {
    let bytes = tree.0.get(MANIFEST).ok_or_else(|| invalid_manifest(None))?;
    let text = std::str::from_utf8(bytes).map_err(|_| invalid_manifest(None))?;
    let root = read_restricted_node(text, MAX_FILE_BYTES).map_err(|e| match e {
        YamlError::FeatureRefused { line, .. } | YamlError::Malformed { line } => {
            invalid_manifest((line > 0).then_some(line))
        }
        YamlError::TooLarge { .. } => invalid_manifest(None),
    })?;
    let YamlNode::Map(entries) = root else {
        return Err(invalid_manifest(None));
    };
    let get = |key: &str| {
        entries
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| scalar(v))
            .map(str::to_owned)
    };
    let name = get("name").filter(|n| !n.trim().is_empty());
    let version = get("version").filter(|v| !v.trim().is_empty());
    let (Some(name), Some(version)) = (name, version) else {
        return Err(invalid_manifest(None));
    };
    let other_keys = entries
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| !matches!(k.as_str(), "name" | "version" | "description"))
        .collect();
    Ok(Package {
        identity: PackageIdentity { name, version },
        description: get("description").filter(|d| !d.is_empty()),
        other_keys,
    })
}

// ---- writing ----

/// The effective loading mode: `metadata.loading` when it names one, else the kind default.
fn effective_loading(part: &ModelPart) -> LoadingMode {
    part.manifest
        .metadata
        .loading
        .as_deref()
        .and_then(|text| serde_json::from_value(serde_json::Value::String(text.to_owned())).ok())
        .unwrap_or(kind_info(part.kind).default_loading)
}

fn part_body(part: &ModelPart, bodies: &BTreeMap<String, String>) -> String {
    if let Some(body) = bodies.get(&part.manifest.metadata.id) {
        return body.clone();
    }
    part.files
        .0
        .get(file_name(&part.manifest_path))
        .and_then(|bytes| markdown_body(&part.manifest_path, bytes))
        .unwrap_or_default()
}

/// FP-TF-3: the `input` and `argument-hint` lines of a prompt written from a task, and what the APM
/// format cannot hold of the task's title and inputs.
// spec: ordin-desktop/components/FormatPlugins#FP-TF-3
fn prompt_front(part: &ModelPart, slug: &str) -> (String, Vec<&'static str>) {
    let manifest = &part.manifest;
    let hint = match manifest.body.get("argument-hint") {
        Some(YamlValue::Str(hint)) => Some(hint.clone()),
        _ => None,
    };
    let mut front = String::new();
    let mut lost = Vec::new();
    let one_short_text = match manifest.inputs() {
        Ok(None) => None,
        Ok(Some(inputs)) if inputs.len() == 1 && inputs[0].kind == TaskInputKind::ShortText => {
            inputs.into_iter().next()
        }
        // Several inputs, one of another kind, or inputs that do not read: one loss for the task.
        _ => {
            lost.push("inputs");
            None
        }
    };
    match one_short_text {
        Some(input) => {
            front.push_str(&format!("input: [{}]\n", yaml_scalar(&input.name)));
            let hint = hint.unwrap_or_else(|| format!("<{}>", input.label));
            front.push_str(&format!("argument-hint: {}\n", yaml_scalar(&hint)));
            if input.hint.is_some() {
                lost.push("input hint");
            }
            if input.default.is_some() {
                lost.push("input default");
            }
            if !input.required {
                lost.push("optional input");
            }
        }
        None => {
            if let Some(hint) = hint {
                front.push_str(&format!("argument-hint: {}\n", yaml_scalar(&hint)));
            }
        }
    }
    let title_lost = match manifest.title() {
        Ok(Some(title)) => title != default_name(slug),
        Ok(None) => false,
        Err(_) => true,
    };
    if title_lost {
        lost.push("title");
    }
    (front, lost)
}

/// An APM Markdown file: front matter only for what the default would not give back, then the body.
fn apm_markdown(part: &ModelPart, slug: &str, body: &str) -> String {
    apm_markdown_with(part, slug, body, "")
}

/// `apm_markdown` with `extra` front-matter lines written after `description`.
fn apm_markdown_with(part: &ModelPart, slug: &str, body: &str, extra: &str) -> String {
    let meta = &part.manifest.metadata;
    let mut front = String::new();
    let without_name = if meta.description.is_empty() {
        default_name(slug)
    } else {
        meta.description.clone()
    };
    if !meta.name.is_empty() && meta.name != without_name {
        front.push_str(&format!("name: {}\n", yaml_scalar(&meta.name)));
    }
    if !meta.description.is_empty() {
        front.push_str(&format!(
            "description: {}\n",
            yaml_scalar(&meta.description)
        ));
    }
    front.push_str(extra);
    if let Some(YamlValue::Str(glob)) = part.manifest.body.get("appliesToFiles") {
        front.push_str(&format!("applyTo: {}\n", yaml_scalar(glob)));
    }
    if front.is_empty() {
        body.to_owned()
    } else {
        format!("---\n{front}---\n{body}")
    }
}

/// A file slug no earlier part of this write has taken: the last identifier segment, then the whole
/// category-and-slug form, then a numbered one.
fn unique_slug(
    id: &str,
    taken: &mut BTreeSet<String>,
    template: &dyn Fn(&str) -> String,
) -> String {
    let last = Urn::parse(id)
        .map(|u| u.slug)
        .unwrap_or_else(|| slug_from_id(id));
    let whole = slug_from_id(id);
    let mut candidates = vec![last, whole.clone()];
    candidates.extend((2..).take(1000).map(|n| format!("{whole}-{n}")));
    for slug in candidates {
        if taken.insert(template(&slug)) {
            return slug;
        }
    }
    slug_from_id(id)
}

fn is_overlay_with_instructions(kind: PartKind) -> bool {
    matches!(
        kind,
        PartKind::LanguageOverlay
            | PartKind::SourceWorkflowOverlay
            | PartKind::StyleOverlay
            | PartKind::HandoffOverlay
    )
}

/// WorkspaceProfile fields with no APM key (ADR-089).
const WORKSPACE_LOSSES: [&str; 7] = [
    "requiredPracticePacks",
    "selectablePracticePacks",
    "fixedPracticePacks",
    "governanceMetadata",
    "toolPermissions",
    "requiredControlLoop",
    "controlLoop",
];

// spec: ordin-desktop/components/FormatPlugins#SA-APM-01
fn write_package(request: &WriteRequest) -> Result<WriteOutcome, FormatError> {
    guard(&request.existing)?;
    if !request.existing.0.is_empty() {
        if let Some(detection) = ApmPlugin.detect(&request.existing) {
            if !detection.version_tested {
                return Err(FormatError::VersionUntested {
                    declared: detection.declared_version,
                });
            }
        }
    }
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut losses: Vec<LossItem> = Vec::new();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut manifest_written = false;
    let mut loss = |part: &ModelPart, what: &str, reason: LossReason| {
        losses.push(LossItem {
            part_identifier: part.manifest.metadata.id.clone(),
            kind: part.kind,
            what: what.to_owned(),
            reason,
        });
    };
    for part in &request.parts {
        let id = part.manifest.metadata.id.as_str();
        let mode = effective_loading(part);
        let body = part_body(part, &request.bodies);
        match (part.kind, mode) {
            (PartKind::Rule | PartKind::ControlLoop, LoadingMode::AlwaysOn) => {
                if part.kind == PartKind::ControlLoop {
                    loss(part, "kind semantics", LossReason::FieldNotHeld);
                }
                let slug = unique_slug(id, &mut taken, &|s| {
                    format!(".apm/instructions/{s}.instructions.md")
                });
                files.insert(
                    format!(".apm/instructions/{slug}.instructions.md"),
                    apm_markdown(part, &slug, &body).into_bytes(),
                );
            }
            (kind, LoadingMode::AlwaysOn) if is_overlay_with_instructions(kind) => {
                loss(part, "kind semantics", LossReason::FieldNotHeld);
                let slug = unique_slug(id, &mut taken, &|s| {
                    format!(".apm/instructions/{s}.instructions.md")
                });
                files.insert(
                    format!(".apm/instructions/{slug}.instructions.md"),
                    apm_markdown(part, &slug, &body).into_bytes(),
                );
            }
            (PartKind::SystemRole, _) => {
                // A system role placed beside its workspace is the agent file; any other is an
                // always-on instruction.
                let workspace_slug = id
                    .strip_suffix(":system-role")
                    .and_then(|head| head.rsplit(':').next());
                match workspace_slug {
                    Some(slug) => {
                        let slug = unique_slug(
                            &format!("urn:ordin:x:workspaces:{slug}"),
                            &mut taken,
                            &|s| format!(".apm/agents/{s}.agent.md"),
                        );
                        files.insert(
                            format!(".apm/agents/{slug}.agent.md"),
                            apm_markdown(part, &slug, &body).into_bytes(),
                        );
                    }
                    None => {
                        let slug = unique_slug(id, &mut taken, &|s| {
                            format!(".apm/instructions/{s}.instructions.md")
                        });
                        files.insert(
                            format!(".apm/instructions/{slug}.instructions.md"),
                            apm_markdown(part, &slug, &body).into_bytes(),
                        );
                    }
                }
            }
            (PartKind::Skill, LoadingMode::OnDemand) => {
                let slug = unique_slug(id, &mut taken, &|s| format!(".apm/skills/{s}/SKILL.md"));
                files.insert(
                    format!(".apm/skills/{slug}/SKILL.md"),
                    apm_markdown(part, &slug, &body).into_bytes(),
                );
                let own = file_name(&part.manifest_path);
                for (name, bytes) in &part.files.0 {
                    if name != own {
                        files.insert(format!(".apm/skills/{slug}/{name}"), bytes.clone());
                    }
                }
            }
            (PartKind::ContextResource, LoadingMode::OnDemand) => {
                let slug = unique_slug(id, &mut taken, &|s| format!(".apm/context/{s}.context.md"));
                files.insert(
                    format!(".apm/context/{slug}.context.md"),
                    apm_markdown(part, &slug, &body).into_bytes(),
                );
            }
            (PartKind::Command, LoadingMode::RegisteredInvocation) => {
                let slug = unique_slug(id, &mut taken, &|s| format!(".apm/prompts/{s}.prompt.md"));
                let (front, lost) = prompt_front(part, &slug);
                for what in lost {
                    loss(part, what, LossReason::FieldNotHeld);
                }
                files.insert(
                    format!(".apm/prompts/{slug}.prompt.md"),
                    apm_markdown_with(part, &slug, &body, &front).into_bytes(),
                );
            }
            (PartKind::OperatingMode, LoadingMode::ModeSwitch) => {
                let slug = unique_slug(id, &mut taken, &|s| {
                    format!(".apm/chatmodes/{s}.chatmode.md")
                });
                files.insert(
                    format!(".apm/chatmodes/{slug}.chatmode.md"),
                    apm_markdown(part, &slug, &body).into_bytes(),
                );
            }
            (PartKind::WorkspaceProfile, _) if !manifest_written => {
                manifest_written = true;
                let meta = &part.manifest.metadata;
                let (name, version) = match &request.package {
                    Some(package) => (package.name.clone(), package.version.clone()),
                    None => (
                        if meta.name.is_empty() {
                            slug_from_id(id)
                        } else {
                            meta.name.clone()
                        },
                        meta.version.clone(),
                    ),
                };
                files.insert(
                    MANIFEST.to_owned(),
                    apm_yml(&name, &version, &meta.description).into_bytes(),
                );
                for key in WORKSPACE_LOSSES {
                    if part.manifest.body.contains_key(key) {
                        loss(part, key, LossReason::FieldNotHeld);
                    }
                }
            }
            // Every other kind, and a supported kind whose loading mode APM has no primitive for.
            _ => loss(part, "whole part", LossReason::NoPrimitive),
        }
    }
    if !manifest_written {
        if let Some(package) = &request.package {
            files.insert(
                MANIFEST.to_owned(),
                apm_yml(&package.name, &package.version, "").into_bytes(),
            );
        }
    }
    for borrowed in &request.borrowed {
        let kind = request
            .parts
            .iter()
            .find(|p| &p.manifest.metadata.id == borrowed)
            .map(|p| p.kind)
            .unwrap_or_else(|| kind_of_identifier(borrowed));
        losses.push(LossItem {
            part_identifier: borrowed.clone(),
            kind,
            what: "borrowed part".to_owned(),
            reason: LossReason::BorrowedPart,
        });
    }
    let mut starting_files = BTreeMap::new();
    if !request.existing.0.contains_key(MANIFEST) {
        if let Some(bytes) = files.get(MANIFEST) {
            starting_files.insert(MANIFEST.to_owned(), bytes.clone());
        }
    }
    Ok(WriteOutcome {
        files,
        starting_files,
        loss_report: LossReport {
            format: Some(FormatId::Apm),
            items: losses,
        },
    })
}

/// The kind of a borrowed identifier that names no part of the request, from its kind directory.
// `LossItem.kind` is required but `WriteRequest.borrowed` holds bare identifiers. The kind
// is read from the identifier's kind directory (`urn:ordin:<publisher>:<dir>:...`, or `@<publisher>/<dir>/...`);
// an identifier whose directory names no kind is reported as a Rule.
fn kind_of_identifier(id: &str) -> PartKind {
    // Directory candidates, longest first: `overlays/style` before `overlays`.
    let segments: Vec<String> = match Urn::parse(id) {
        Some(urn) => urn
            .kind_dir
            .split(':')
            .map(str::to_owned)
            .chain(urn.categories)
            .collect(),
        None => id
            .strip_prefix('@')
            .map(|rest| rest.split('/').skip(1).map(str::to_owned).collect())
            .unwrap_or_default(),
    };
    let candidates = [
        segments.get(..2).map(|s| s.join("/")),
        segments.first().cloned(),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|dir| {
            ordin_agent_model::all_kinds()
                .iter()
                .copied()
                .find(|kind| kind_info(*kind).kind_dir == Some(dir.as_str()))
        })
        .unwrap_or(PartKind::Rule)
}

fn apm_yml(name: &str, version: &str, description: &str) -> String {
    let mut text = format!(
        "name: {}\nversion: {}\n",
        yaml_scalar(name),
        yaml_scalar(version)
    );
    if !description.is_empty() {
        text.push_str(&format!("description: {}\n", yaml_scalar(description)));
    }
    text
}
