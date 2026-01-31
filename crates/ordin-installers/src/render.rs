//! The pure render shared by every installer (Installers.md *Internal Logic* steps 1, 2, 6 and 7):
//! read the bundle's parts, check the control loop, hand the parts to the installer's layout, add
//! the lock file and bound the tree.

use std::collections::BTreeMap;

use ordin_agent_model::{
    kind_info, parse_manifest, ControlLoopSpec, KindFamily, LoadingMode, Manifest, PartKind,
    PartRef, Urn, LOCK_FILE_NAME,
};
use ordin_resolver::Bundle;
use ordin_types::AgentInstanceId;

use crate::error::{bundle_unavailable, InstallError};
use crate::types::{InstallerKind, RenderedTree, MAX_INSTALLED_FILES};

/// One part of the bundle, read and checked (step 1).
pub(crate) struct Part {
    pub part: PartRef,
    pub loading: LoadingMode,
    pub files: BTreeMap<String, Vec<u8>>,
    /// The file that holds the part's manifest, when one does.
    pub primary: Option<String>,
    pub manifest: Option<Manifest>,
    pub urn: Option<Urn>,
}

impl Part {
    /// The Markdown body of the primary file; the whole primary file when it has no front matter.
    pub(crate) fn body(&self) -> String {
        let Some(primary) = &self.primary else {
            return String::new();
        };
        let bytes = &self.files[primary];
        ordin_agent_model::markdown_body(primary, bytes)
            .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned())
    }

    /// The part's files other than the primary one.
    pub(crate) fn others(&self) -> impl Iterator<Item = (&String, &Vec<u8>)> {
        self.files
            .iter()
            .filter(move |(name, _)| Some(*name) != self.primary.as_ref())
    }

    /// `<Kind>-<name>`, the fallback directory name.
    pub(crate) fn fallback_name(&self) -> String {
        format!("{:?}-{}", self.part.kind, self.part.name)
    }

    /// `<Kind>/<name>`, as the bootstrap lists a reference-only part.
    pub(crate) fn listed_name(&self) -> String {
        format!("{:?}/{}", self.part.kind, self.part.name)
    }
}

/// Everything a layout needs: the root, its description, the parts, the loop.
pub(crate) struct Inputs {
    pub root: PartRef,
    pub root_description: String,
    pub parts: Vec<Part>,
    pub control_loop: Option<(PartRef, ControlLoopSpec)>,
    pub id: Option<AgentInstanceId>,
}

/// The behaviour common to every layout: what is behavioural (always applied) and what is an overlay.
pub(crate) fn is_overlay(kind: PartKind) -> bool {
    kind_info(kind).family == KindFamily::Overlay
}

pub(crate) fn is_behavioural(kind: PartKind) -> bool {
    matches!(
        kind,
        PartKind::SystemRole | PartKind::Rule | PartKind::OperatingMode | PartKind::ControlLoop
    ) || is_overlay(kind)
}

/// The loops each installer can run (port of the per-target strategy and capability lists).
fn loop_support(kind: InstallerKind) -> (&'static [&'static str], &'static [&'static str]) {
    match kind {
        InstallerKind::ClaudeCode => (
            &[
                "goal-evaluated",
                "plan-then-execute",
                "reactive",
                "tool-iterative",
            ],
            &["bounded-iteration", "completion-evaluator"],
        ),
        InstallerKind::Vscode | InstallerKind::AgentsMd => (&["reactive", "tool-iterative"], &[]),
    }
}

/// Step 2: the strategy must be in the installer's list and each `requires` entry in its
/// capability list.
pub(crate) fn check_loop(kind: InstallerKind, spec: &ControlLoopSpec) -> Result<(), InstallError> {
    let (strategies, capabilities) = loop_support(kind);
    if !strategies.contains(&spec.strategy.as_str()) {
        return Err(InstallError::LoopUnsupported {
            strategy: spec.strategy.clone(),
            missing_capability: None,
        });
    }
    if let Some(missing) = spec
        .requires
        .iter()
        .find(|r| !capabilities.contains(&r.as_str()))
    {
        return Err(InstallError::LoopUnsupported {
            strategy: spec.strategy.clone(),
            missing_capability: Some(missing.clone()),
        });
    }
    Ok(())
}

/// Step 1's path rule: absolute, `..`, `\` or NUL is refused.
pub(crate) fn path_unsafe(path: &str) -> bool {
    path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.split('/').any(|segment| segment == "..")
        || declares_hooks(path)
}

/// `hooks/hooks.json` anywhere in a part. Claude Code runs a plugin's hooks as shell commands outside
/// the Rules Engine, and no part kind installs one (*Internal Logic* step 3).
// spec: ordin-desktop/components/Installers#SA-RECON-06
fn declares_hooks(path: &str) -> bool {
    path.eq_ignore_ascii_case("hooks/hooks.json")
        || path.to_ascii_lowercase().ends_with("/hooks/hooks.json")
}

/// The YAML of a Markdown file's front matter, as generously as Claude Code might find it.
fn front_matter(text: &str) -> Option<&str> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let rest = text.strip_prefix("---")?;
    let (first, rest) = rest.split_once('\n')?;
    if !first.trim().is_empty() {
        return None;
    }
    let mut offset = 0;
    for line in rest.split('\n') {
        if line.trim() == "---" {
            return Some(&rest[..offset]);
        }
        offset += line.len() + 1;
    }
    Some(rest)
}

/// Whether YAML text may have a key `hooks`. Read by text, so it refuses more than YAML would: a top
/// level that is not plain `key: value` lines, a quoted key holding a backslash, and the word as a
/// key in block or flow style, quoted or not, at any depth.
fn names_hooks_key(yaml: &str) -> bool {
    // An allow-list first: an anchor, a tag, a complex key, a merge key, flow style or a quoted key
    // at the top level can each spell `hooks` so that no text search sees it.
    let plain_top_level = yaml.lines().all(|line| {
        let key = line.split(':').next().unwrap_or_default();
        line.is_empty()
            || line.starts_with([' ', '\t', '#'])
            || line.starts_with("- ")
            || (line.contains(':')
                && !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    });
    let lower = yaml.to_lowercase();
    let quoted_escape = yaml.lines().any(|line| {
        line.contains('\\')
            && (line.contains("\":") || line.contains("':") || line.contains("\" :"))
    });
    !plain_top_level
        || quoted_escape
        || lower.match_indices("hooks").any(|(at, word)| {
            let before = lower[..at].trim_end_matches([' ', '\t', '"', '\'']);
            let starts_key = before.is_empty() || before.ends_with(['\n', '{', ',', '-', '?']);
            let after = lower[at + word.len()..].trim_start_matches([' ', '\t', '"', '\'']);
            starts_key && (after.starts_with(':') || before.ends_with('?'))
        })
}

/// A Markdown part file whose front matter declares hooks. Claude Code runs those of a skill as shell
/// commands as the person, under `--restricted` and outside the Rules Engine (run on 2.1.316).
// spec: ordin-desktop/components/Installers#SA-RECON-06
fn front_matter_hooks(name: &str, bytes: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.ends_with(".md") || lower.ends_with(".markdown"))
        && front_matter(&String::from_utf8_lossy(bytes).replace("\r\n", "\n"))
            .is_some_and(names_hooks_key)
}

pub(crate) fn check_file(name: &str, bytes: &[u8]) -> Result<(), InstallError> {
    if front_matter_hooks(name, bytes) {
        return Err(InstallError::PathInvalid {
            path: name.to_owned(),
        });
    }
    check_path(name)
}

pub(crate) fn check_path(path: &str) -> Result<(), InstallError> {
    if path_unsafe(path) {
        Err(InstallError::PathInvalid {
            path: path.to_owned(),
        })
    } else {
        Ok(())
    }
}

/// The file holding the part's manifest: the shallowest, then shortest, then first by name that
/// parses as one.
fn primary_of(files: &BTreeMap<String, Vec<u8>>) -> Option<(String, Manifest)> {
    let mut names: Vec<&String> = files.keys().collect();
    names.sort_by_key(|name| (name.matches('/').count(), name.len(), (*name).clone()));
    names.into_iter().find_map(|name| {
        parse_manifest(name, &files[name])
            .ok()
            .flatten()
            .map(|manifest| (name.clone(), manifest))
    })
}

/// Step 1: read every part of the bundle and check every file name. The root (the bundle's
/// composition) is read for its description and is not itself placed.
// The root is named by the bootstrap's header and is otherwise not written.
pub(crate) fn read_inputs(
    kind: InstallerKind,
    bundle: &Bundle,
    id: Option<AgentInstanceId>,
) -> Result<Inputs, InstallError> {
    let root = bundle.manifest.composition.clone();
    let root_files = bundle
        .read_part_files(&root)
        .map_err(|e| bundle_unavailable(&e))?;
    let root_description = primary_of(&root_files.0)
        .map(|(_, manifest)| manifest.metadata.description)
        .unwrap_or_default();
    let mut parts = Vec::new();
    let mut control_loop = None;
    for artefact in &bundle.manifest.artefacts {
        let part = &artefact.part;
        if part.kind == root.kind && part.id == root.id && part.version == root.version {
            continue;
        }
        let files = bundle
            .read_part_files(part)
            .map_err(|e| bundle_unavailable(&e))?
            .0;
        for (name, bytes) in &files {
            check_file(name, bytes)?;
        }
        let (primary, manifest) = match primary_of(&files) {
            Some((name, manifest)) => (Some(name), Some(manifest)),
            None => (None, None),
        };
        if part.kind == PartKind::ControlLoop {
            if let Some(manifest) = &manifest {
                // A loop body the Agent Model refuses makes the bundle unusable.
                let spec =
                    manifest
                        .control_loop()
                        .map_err(|e| InstallError::BundleUnavailable {
                            code: e.code().to_owned(),
                            retryable: false,
                        })?;
                if let Some(spec) = spec {
                    check_loop(kind, &spec)?;
                    control_loop = Some((part.clone(), spec));
                }
            }
        }
        parts.push(Part {
            part: part.clone(),
            loading: artefact.loading,
            urn: Urn::parse(&part.id),
            files,
            primary,
            manifest,
        });
    }
    Ok(Inputs {
        root,
        root_description,
        parts,
        control_loop,
        id,
    })
}

/// A rendered tree and the one fact of the parts the install record keeps beside it.
pub(crate) struct Rendered {
    pub tree: RenderedTree,
    /// Step 2: the control loop's trigger is `human-pulled`.
    pub needs_person_present: bool,
}

/// Steps 1 to 7 for one installer: the tree, without touching any folder.
pub(crate) fn render(
    kind: InstallerKind,
    bundle: &Bundle,
    id: Option<AgentInstanceId>,
) -> Result<Rendered, InstallError> {
    let inputs = read_inputs(kind, bundle, id)?;
    let needs_person_present = inputs
        .control_loop
        .as_ref()
        .is_some_and(|(_, spec)| spec.trigger == "human-pulled");
    let mut tree = match kind {
        InstallerKind::ClaudeCode => crate::claude::layout(&inputs)?,
        InstallerKind::Vscode => crate::copilot::vscode(&inputs),
        InstallerKind::AgentsMd => crate::copilot::agents_md(&inputs),
    };
    // Step 6: every layout holds the lock file at the folder root.
    tree.files
        .insert(LOCK_FILE_NAME.to_owned(), bundle.lock_file_bytes.clone());
    bound(&tree)?;
    Ok(Rendered {
        tree,
        needs_person_present,
    })
}

/// Step 7: a tree over 20,000 files is refused.
pub(crate) fn bound(tree: &RenderedTree) -> Result<(), InstallError> {
    if tree.files.len() > MAX_INSTALLED_FILES {
        return Err(InstallError::WriteFailed { retryable: false });
    }
    Ok(())
}

/// A tree under construction: placing a part is all or nothing, and a path already taken is never
/// overwritten (the part then takes the fallback placement).
// The layouts place parts by slug alone (`.github/instructions/<slug>.instructions.md`), so two parts
// can name one path: the first part in bundle order keeps it and the second is placed as reference
// only.
#[derive(Default)]
pub(crate) struct TreeBuilder {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl TreeBuilder {
    /// Inserts every entry, or none when any path is already taken.
    pub(crate) fn place(&mut self, entries: Vec<(String, Vec<u8>)>) -> bool {
        if entries
            .iter()
            .any(|(path, _)| self.files.contains_key(path))
        {
            return false;
        }
        self.files.extend(entries);
        true
    }
}

/// `<base>.md` for the primary file and `<base>/<name>` for the others: the placement of a
/// single-file kind.
// Any other file of the part is written beside the primary under `<base>/`, so nothing the part holds
// is dropped.
pub(crate) fn single_file(part: &Part, base: &str, suffix: &str) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    if let Some(primary) = &part.primary {
        entries.push((format!("{base}{suffix}"), part.files[primary].clone()));
    }
    for (name, bytes) in part.others() {
        entries.push((format!("{base}/{name}"), bytes.clone()));
    }
    entries
}

/// Every file of the part under `dir/`.
pub(crate) fn whole_dir(part: &Part, dir: &str) -> Vec<(String, Vec<u8>)> {
    part.files
        .iter()
        .map(|(name, bytes)| (format!("{dir}/{name}"), bytes.clone()))
        .collect()
}

/// `a/b/` for categories `[a, b]`, empty for none.
pub(crate) fn categories_path(urn: &Urn) -> String {
    urn.categories
        .iter()
        .map(|c| format!("{c}/"))
        .collect::<String>()
}

/// The bootstrap's header line.
pub(crate) fn header(root: &PartRef) -> String {
    format!("# ordin agent — {}@{}\n", root.name, root.version)
}

/// INST-TF-1 step 2: the section a command with inputs ends with; `$ARGUMENTS` receives the values.
pub(crate) const INPUTS_GIVEN: &str = "## Inputs given\n\nEach input written above as [name] is given below on a line \"name: value\". A value of several lines continues on the following lines, each indented by two spaces. An input that is not listed was left empty.\n\n$ARGUMENTS\n";

/// INST-TF-1 steps 1–2: each `${input:<name>}` becomes `[<name>]`, and the body gains, after one
/// blank line, the *Inputs given* section.
// spec: ordin-desktop/components/Installers#INST-TF-1
pub(crate) fn task_body(body: &str) -> String {
    let mut out = body.to_owned();
    for name in ordin_agent_model::task_placeholders(body) {
        out = out.replace(&format!("${{input:{name}}}"), &format!("[{name}]"));
    }
    let trimmed = out.trim_end_matches(['\n', '\r']);
    format!("{trimmed}\n\n{INPUTS_GIVEN}")
}

/// What a Command part's file becomes under INST-TF-1: `Some(rendered body)` when its inputs read and
/// are present, None when the part is written unchanged (no inputs, or not a manifest). Inputs that
/// do not read leave the file unchanged and log `installers.task_inputs_unreadable`.
// spec: ordin-desktop/components/Installers#INST-TF-1
pub(crate) fn command_task_body(part: &Part) -> Option<String> {
    let manifest = part.manifest.as_ref()?;
    match manifest.inputs() {
        Ok(Some(_)) => Some(task_body(&part.body())),
        Ok(None) => None,
        Err(_) => {
            crate::log::task_inputs_unreadable(&part.part.id);
            None
        }
    }
}

/// The same for a command file given as bytes (the Claude Code session form): the file with its
/// body rendered and its front matter kept as read.
// spec: ordin-desktop/components/Installers#INST-TF-1
pub(crate) fn command_file_with_inputs(path: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    let manifest = ordin_agent_model::parse_manifest(path, bytes)
        .ok()
        .flatten()?;
    match manifest.inputs() {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(_) => {
            crate::log::task_inputs_unreadable(&manifest.metadata.id);
            return None;
        }
    }
    let body = ordin_agent_model::markdown_body(path, bytes)?;
    let head = &bytes[..bytes.len() - body.len()];
    let mut out = head.to_vec();
    out.extend_from_slice(task_body(&body).as_bytes());
    Some(out)
}
