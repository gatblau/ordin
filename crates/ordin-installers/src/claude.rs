//! The Claude Code layout (Installers.md *Internal Logic* step 3) and the release-bundle session form
//! (step 14a).
//!
//! Paths are relative to the agent folder. Besides the native layout and its `CLAUDE.md` bootstrap,
//! the installer writes `instructions.md` (the bootstrap with its `@` imports inlined) and `plugin/`
//! (the skills and commands as a plugin), because Claude Code under `--restricted` reads nothing in
//! the folder (C-61 (b)). No settings file, no MCP file and no policy file is written (ADR-090 item 1).

use std::collections::BTreeMap;
use std::path::Path;

use ordin_agent_model::{
    canonical_json, ControlLoopSpec, LoadingMode, MemoryContract, PartKind, PartRef, Urn,
};

use crate::error::InstallError;
use crate::imports::{self, ImportSource};
use crate::log::{self, PathEvent};
use crate::render::{
    categories_path, check_file, check_path, header, is_overlay, single_file, whole_dir, Inputs,
    Part, TreeBuilder,
};
use crate::types::{ClaudeSessionFormInput, CommandName, RenderedTree, MAX_INSTALLED_FILES};

pub(crate) const BOOTSTRAP: &str = "CLAUDE.md";
pub(crate) const INSTRUCTIONS: &str = "instructions.md";
pub(crate) const PLUGIN_DIR: &str = "plugin";
const PLUGIN_MANIFEST: &str = "plugin/.claude-plugin/plugin.json";
/// The fallback directory for parts with no Claude Code home of their own.
const FALLBACK_DIR: &str = ".claude/ordin";
const MAX_PLUGIN_NAME: usize = 64;

/// What the bootstrap is built from, gathered while the parts are placed.
#[derive(Default)]
struct Bootstrap {
    persona: Option<String>,
    /// Rule and operating-mode imports.
    rules: Vec<String>,
    /// Mode slug → its *When to exit* section.
    mode_exits: BTreeMap<String, String>,
    loop_import: Option<String>,
    overlays: Vec<String>,
    context_eager: Vec<String>,
    context_on_demand: Vec<String>,
    memory: Vec<String>,
    fallback: Vec<String>,
}

/// Step 3, whole: the native layout, `CLAUDE.md`, `instructions.md` and `plugin/`.
// spec: ordin-desktop/components/Installers#C-61
pub(crate) fn layout(inputs: &Inputs) -> Result<RenderedTree, InstallError> {
    let mut tree = TreeBuilder::default();
    let mut boot = Bootstrap::default();
    let mut reference_only = Vec::new();
    let mut skills: Vec<String> = Vec::new();
    let mut commands: Vec<(String, String)> = Vec::new();
    for part in &inputs.parts {
        if !place(part, &mut tree, &mut boot, &mut skills, &mut commands) {
            let dir = format!("{FALLBACK_DIR}/{}", part.fallback_name());
            tree.place(whole_dir(part, &dir));
            boot.fallback.push(part.listed_name());
            reference_only.push(part.part.id.clone());
        }
    }
    let bootstrap = render_bootstrap(&mut boot, inputs);
    tree.files
        .insert(BOOTSTRAP.to_owned(), bootstrap.clone().into_bytes());

    // `instructions.md`: the bootstrap with its imports inlined against the rendered map.
    let id = inputs.id;
    let instructions = imports::inline(&bootstrap, BOOTSTRAP, &MapSource(&tree.files), &mut |t| {
        log::path_warning(PathEvent::ImportMissing, id, t)
    })?;

    // `plugin/`: a copy of each skill folder and command file.
    let name = plugin_name(&root_slug(&inputs.root));
    let mut plugin: Vec<(String, Vec<u8>)> = vec![(
        PLUGIN_MANIFEST.to_owned(),
        plugin_manifest(&inputs.root_description, &name, &inputs.root.version),
    )];
    for slug in &skills {
        let prefix = format!(".claude/skills/{slug}/");
        for (path, bytes) in tree.files.range(prefix.clone()..) {
            let Some(rest) = path.strip_prefix(&prefix) else {
                break;
            };
            plugin.push((format!("{PLUGIN_DIR}/skills/{slug}/{rest}"), bytes.clone()));
        }
    }
    for (_, slug) in &commands {
        if let Some(bytes) = tree.files.get(&format!(".claude/commands/{slug}.md")) {
            plugin.push((format!("{PLUGIN_DIR}/commands/{slug}.md"), bytes.clone()));
        }
    }
    let mut files = tree.files;
    files.insert(INSTRUCTIONS.to_owned(), instructions.into_bytes());
    files.extend(plugin);

    reference_only.sort();
    let mut command_names: Vec<CommandName> = commands
        .into_iter()
        .map(|(command_id, slug)| CommandName {
            command_id,
            session_name: format!("{name}:{slug}"),
        })
        .collect();
    command_names.sort_by(|a, b| a.session_name.cmp(&b.session_name));
    Ok(RenderedTree {
        files,
        launch_paths: Some((INSTRUCTIONS.to_owned(), PLUGIN_DIR.to_owned(), name)),
        commands: command_names,
        reference_only_parts: reference_only,
    })
}

/// Places one part at its native path. False when the part takes the fallback placement.
fn place(
    part: &Part,
    tree: &mut TreeBuilder,
    boot: &mut Bootstrap,
    skills: &mut Vec<String>,
    commands: &mut Vec<(String, String)>,
) -> bool {
    match part.part.kind {
        PartKind::SystemRole => {
            let placed = tree.place(single_file(part, ".claude/rules/system-role", ".md"));
            if placed {
                boot.persona = Some(".claude/rules/system-role.md".to_owned());
            }
            return placed;
        }
        PartKind::Memory => {
            // No file: a policy block in `CLAUDE.md`.
            let contract = part
                .manifest
                .as_ref()
                .and_then(|m| m.memory_contract().ok().flatten());
            if let Some(contract) = contract {
                boot.memory.push(memory_policy(&part.part.name, &contract));
                return true;
            }
            return false;
        }
        _ => {}
    }
    // Every other native placement is derived from the part's identifier.
    let Some(urn) = &part.urn else {
        return false;
    };
    let slug = &urn.slug;
    let categories = categories_path(urn);
    match part.part.kind {
        PartKind::Skill => {
            let placed = tree.place(whole_dir(part, &format!(".claude/skills/{slug}")));
            if placed {
                skills.push(slug.clone());
            }
            placed
        }
        PartKind::Command => {
            let mut entries = single_file(part, &format!(".claude/commands/{slug}"), ".md");
            // INST-TF-1: a command with inputs is written with one place for its values.
            if let (Some(rendered), Some((_, first))) =
                (crate::render::command_task_body(part), entries.first_mut())
            {
                if let Some(primary) = &part.primary {
                    let bytes = &part.files[primary];
                    let body_len =
                        ordin_agent_model::markdown_body(primary, bytes).map_or(0, |b| b.len());
                    let mut out = bytes[..bytes.len() - body_len].to_vec();
                    out.extend_from_slice(rendered.as_bytes());
                    *first = out;
                }
            }
            let placed = tree.place(entries);
            if placed {
                commands.push((part.part.id.clone(), slug.clone()));
            }
            placed
        }
        PartKind::Rule | PartKind::OperatingMode | PartKind::ControlLoop => {
            let base = format!(".claude/rules/{categories}{slug}");
            let placed = tree.place(single_file(part, &base, ".md"));
            if placed {
                let import = format!("{base}.md");
                match part.part.kind {
                    PartKind::ControlLoop => boot.loop_import = Some(import),
                    PartKind::OperatingMode => {
                        boot.mode_exits
                            .insert(slug.clone(), exit_section(&part.body()));
                        boot.rules.push(import);
                    }
                    _ => boot.rules.push(import),
                }
            }
            placed
        }
        PartKind::ContextResource => {
            let base = format!(".claude/context/{categories}{slug}");
            let placed = tree.place(single_file(part, &base, ".md"));
            if placed {
                let import = format!("{base}.md");
                if part.loading == LoadingMode::AlwaysOn {
                    boot.context_eager.push(import);
                } else {
                    boot.context_on_demand.push(import);
                }
            }
            placed
        }
        kind if is_overlay(kind) => {
            let dir = format!(".claude/rules/overlays/{categories}{slug}");
            let placed = tree.place(whole_dir(part, &dir));
            if placed {
                if let Some(primary) = &part.primary {
                    boot.overlays.push(format!("{dir}/{primary}"));
                }
            }
            placed
        }
        _ => false,
    }
}

/// `CLAUDE.md`, in the bootstrap's order: header, the installed-by paragraph, then the sections.
// This repository holds ordin's own wording of the auto-discovery sentence and the control-loop text,
// in the reference's section order and with the same `@` import lines.
fn render_bootstrap(boot: &mut Bootstrap, inputs: &Inputs) -> String {
    boot.rules.sort();
    boot.overlays.sort();
    boot.context_eager.sort();
    boot.context_on_demand.sort();
    boot.memory.sort();
    boot.fallback.sort();
    let mut out = header(&inputs.root);
    out.push_str(
        "\nInstalled by ordin. Claude Code finds the agent's skills under `.claude/skills/` and its \
         commands under `.claude/commands/` by itself; the imports below load everything else.\n",
    );
    if let Some(persona) = &boot.persona {
        out.push_str(&format!("\n## System role\n\n@{persona}\n"));
    }
    if !boot.rules.is_empty() {
        out.push_str("\n## Operating modes & rules\n\n");
        for import in &boot.rules {
            out.push_str(&format!("@{import}\n"));
        }
    }
    if let Some((loop_ref, spec)) = &inputs.control_loop {
        out.push_str(&control_loop_section(
            loop_ref,
            spec,
            &boot.mode_exits,
            boot.loop_import.as_deref(),
        ));
    }
    if !boot.overlays.is_empty() {
        out.push_str("\n## Overlays\n\n");
        for import in &boot.overlays {
            out.push_str(&format!("@{import}\n"));
        }
    }
    if !boot.context_eager.is_empty() || !boot.context_on_demand.is_empty() {
        out.push_str("\n## Context resources\n\n");
        for import in &boot.context_eager {
            out.push_str(&format!("@{import}\n"));
        }
        if !boot.context_on_demand.is_empty() {
            out.push_str("Reference material read when it is relevant, not held in every turn:\n");
            for path in &boot.context_on_demand {
                out.push_str(&format!("- `{path}`\n"));
            }
        }
    }
    if !boot.memory.is_empty() {
        out.push_str("\n## Memory policy\n\n");
        out.push_str(&boot.memory.join("\n"));
    }
    if !boot.fallback.is_empty() {
        out.push_str("\n## Bundled artefacts (reference; not auto-loaded)\n\n");
        for listed in &boot.fallback {
            let dir = listed.replacen('/', "-", 1);
            out.push_str(&format!("- {listed} — `{FALLBACK_DIR}/{dir}/`\n"));
        }
    }
    out
}

/// The *Control loop* section: the loop, the graph derived from the modes' exit sections (port of
/// the reference's graph derivation), the capabilities it needs and the loop file's import.
fn control_loop_section(
    loop_ref: &PartRef,
    spec: &ControlLoopSpec,
    exits: &BTreeMap<String, String>,
    import: Option<&str>,
) -> String {
    let mut out = format!(
        "\n## Control loop\n\n`{}` — strategy `{}`",
        loop_ref.name, spec.strategy
    );
    if !spec.trigger.is_empty() {
        out.push_str(&format!(", trigger `{}`", spec.trigger));
    }
    out.push('\n');
    let graph = derive_graph(spec, exits);
    if !graph.is_empty() {
        out.push_str(
            "\nMoves within one iteration, from each mode's *When to exit* section and the modes \
             this loop composes:\n\n",
        );
        for (from, to) in &graph {
            if to.is_empty() {
                let note = if from == ESCALATE_MODE && is_autonomous(spec) {
                    " (unattended: the run stops here and reports)"
                } else {
                    ""
                };
                out.push_str(&format!("- **{}** → *terminal*{note}\n", display(from)));
            } else {
                let names: Vec<String> = to.iter().map(|s| display(s)).collect();
                out.push_str(&format!(
                    "- **{}** → {}\n",
                    display(from),
                    names.join(" · ")
                ));
            }
        }
    }
    if !spec.requires.is_empty() {
        out.push_str(&format!(
            "\nRuntime capabilities this loop needs: `{}`.\n",
            spec.requires.join("`, `")
        ));
    }
    if let Some(import) = import {
        out.push_str(&format!("\n@{import}\n"));
    }
    out
}

const ESCALATE_MODE: &str = "escalate-mode";

fn is_autonomous(spec: &ControlLoopSpec) -> bool {
    spec.trigger.trim().eq_ignore_ascii_case("autonomous")
}

/// The loop's modes in its order, each with the other modes its exit section names by display
/// name (as a whole word), sorted. Escalate under an autonomous trigger has no exit.
fn derive_graph(
    spec: &ControlLoopSpec,
    exits: &BTreeMap<String, String>,
) -> Vec<(String, Vec<String>)> {
    let modes: Vec<String> = spec
        .modes
        .iter()
        .filter_map(|id| Urn::parse(id).map(|u| u.slug))
        .collect();
    modes
        .iter()
        .map(|from| {
            if from == ESCALATE_MODE && is_autonomous(spec) {
                return (from.clone(), Vec::new());
            }
            let text = exits.get(from).map(String::as_str).unwrap_or("");
            let mut to: Vec<String> = modes
                .iter()
                .filter(|m| *m != from && names_word(text, &display(m)))
                .cloned()
                .collect();
            to.sort();
            (from.clone(), to)
        })
        .collect()
}

/// `observe-mode` → `Observe`.
fn display(slug: &str) -> String {
    let base = slug.strip_suffix("-mode").unwrap_or(slug);
    let mut chars = base.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// True when `word` appears in `text` with no word character either side.
fn names_word(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

/// The text under a heading `When to exit` (any level), up to the next heading.
fn exit_section(body: &str) -> String {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in body.lines() {
        if line.starts_with('#') {
            if inside {
                break;
            }
            inside = line
                .trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("when to exit");
            continue;
        }
        if inside {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// One memory policy block (the Memory part's contract as prose; no file is written).
// The block states each field of the contract, one sentence each.
pub(crate) fn memory_policy(name: &str, contract: &MemoryContract) -> String {
    let list = |items: &[String]| {
        if items.is_empty() {
            "nothing listed".to_owned()
        } else {
            items.join(", ")
        }
    };
    let retention = if contract.unbounded {
        "unbounded".to_owned()
    } else {
        match contract.max_age_days {
            Some(days) => format!("{days} days"),
            None => "not stated".to_owned(),
        }
    };
    let mut out = format!(
        "**{name}** keeps: {}. Never keeps: {}. Retention: {retention}. Store: {}.\n",
        list(&contract.retains),
        list(&contract.excludes),
        if contract.store_location.is_empty() {
            "not stated"
        } else {
            contract.store_location.as_str()
        }
    );
    if let Some(service) = &contract.memory_service {
        out.push_str(&format!("Memory service: `{service}`.\n"));
    }
    out
}

/// The root's slug: the last segment of its identifier, or its name when the identifier is not a URN.
fn root_slug(root: &PartRef) -> String {
    Urn::parse(&root.id)
        .map(|u| u.slug)
        .unwrap_or_else(|| root.name.clone())
}

/// Step 3's plugin name: lowercased, each run outside `[a-z0-9-]` replaced by `-`, trimmed, at most
/// 64 characters.
// A name with no character left after normalising becomes `agent`.
pub(crate) fn plugin_name(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut in_run = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
            out.push(c);
            in_run = false;
        } else if !in_run {
            out.push('-');
            in_run = true;
        }
    }
    let cut: String = out
        .trim_matches('-')
        .chars()
        .take(MAX_PLUGIN_NAME)
        .collect();
    let name = cut.trim_end_matches('-');
    if name.is_empty() {
        "agent".to_owned()
    } else {
        name.to_owned()
    }
}

/// Canonical JSON `{"description", "name", "version"}`.
fn plugin_manifest(description: &str, name: &str, version: &str) -> Vec<u8> {
    canonical_json(&serde_json::json!({
        "description": description,
        "name": name,
        "version": version,
    }))
}

/// Imports read from the rendered map.
struct MapSource<'a>(&'a BTreeMap<String, Vec<u8>>);

impl ImportSource for MapSource<'_> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
}

/// Imports read, read-only, from the files under a release bundle. A link is never followed.
struct DirSource<'a>(&'a Path);

impl ImportSource for DirSource<'_> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let mut full = self.0.to_path_buf();
        for segment in path.split('/') {
            full.push(segment);
            let meta = std::fs::symlink_metadata(&full).ok()?;
            if meta.file_type().is_symlink() {
                return None;
            }
        }
        std::fs::read(full).ok()
    }
}

/// Step 14a: `instructions.md` and `plugin/` for a release bundle.
// spec: ordin-desktop/components/Installers#SA-RECON-04
pub(crate) fn session_form(
    input: &ClaudeSessionFormInput,
    bundle_dir: &Path,
) -> Result<RenderedTree, InstallError> {
    let mut count = 2 + input.commands.len();
    for (slug, files) in &input.skills {
        check_slug(&format!("plugin/skills/{slug}"), slug)?;
        for (path, bytes) in files {
            check_path(&format!("plugin/skills/{slug}/{path}"))?;
            check_file(path, bytes)?;
        }
        count += files.len();
    }
    for (slug, bytes) in &input.commands {
        check_slug(&format!("plugin/commands/{slug}.md"), slug)?;
        check_file(&format!("{slug}.md"), bytes)?;
    }
    if count > MAX_INSTALLED_FILES {
        return Err(InstallError::WriteFailed { retryable: false });
    }
    let name = plugin_name(&input.agent_name);
    let instructions = imports::inline(
        &input.instructions,
        INSTRUCTIONS,
        &DirSource(bundle_dir),
        &mut |t| log::path_warning(PathEvent::ImportMissing, None, t),
    )?;
    let mut files = BTreeMap::new();
    files.insert(INSTRUCTIONS.to_owned(), instructions.into_bytes());
    files.insert(
        PLUGIN_MANIFEST.to_owned(),
        plugin_manifest(&input.description, &name, &input.version),
    );
    for (slug, skill) in &input.skills {
        for (path, bytes) in skill {
            files.insert(format!("{PLUGIN_DIR}/skills/{slug}/{path}"), bytes.clone());
        }
    }
    for (slug, bytes) in &input.commands {
        let path = format!("{PLUGIN_DIR}/commands/{slug}.md");
        let rendered = crate::render::command_file_with_inputs(&path, bytes);
        files.insert(path, rendered.unwrap_or_else(|| bytes.clone()));
    }
    let commands = input
        .commands
        .keys()
        .map(|slug| CommandName {
            command_id: slug.clone(),
            session_name: format!("{name}:{slug}"),
        })
        .collect();
    Ok(RenderedTree {
        files,
        launch_paths: Some((INSTRUCTIONS.to_owned(), PLUGIN_DIR.to_owned(), name)),
        commands,
        reference_only_parts: Vec::new(),
    })
}

/// A skill or command slug is one path segment that passes step 1's rule.
fn check_slug(path: &str, slug: &str) -> Result<(), InstallError> {
    check_path(path)?;
    if slug.is_empty() || slug.contains('/') || slug == "." {
        return Err(InstallError::PathInvalid {
            path: path.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_names_are_normalised() {
        assert_eq!(plugin_name("cspa-431"), "cspa-431");
        assert_eq!(plugin_name("  Support Agent!! v2 "), "support-agent-v2");
        assert_eq!(plugin_name("***"), "agent");
        assert_eq!(plugin_name(&"a".repeat(80)).len(), 64);
        assert_eq!(
            plugin_name(&format!("{}-b", "a".repeat(63))),
            "a".repeat(63)
        );
    }

    #[test]
    fn the_graph_follows_the_exit_sections() {
        let spec = ControlLoopSpec {
            strategy: "reactive".to_owned(),
            trigger: "autonomous".to_owned(),
            requires: Vec::new(),
            modes: vec![
                "urn:ordin:x:rules:modes:observe-mode".to_owned(),
                "urn:ordin:x:rules:modes:escalate-mode".to_owned(),
                "not-a-urn".to_owned(),
            ],
        };
        let mut exits = BTreeMap::new();
        exits.insert(
            "observe-mode".to_owned(),
            "Hand to Escalate when stuck; Observed is not a mode.".to_owned(),
        );
        exits.insert("escalate-mode".to_owned(), "Back to Observe.".to_owned());
        let graph = derive_graph(&spec, &exits);
        assert_eq!(
            graph,
            vec![
                ("observe-mode".to_owned(), vec!["escalate-mode".to_owned()]),
                ("escalate-mode".to_owned(), Vec::new()),
            ]
        );
        assert!(names_word("to Observe.", "Observe"));
        assert!(!names_word("Observed", "Observe"));
        assert!(!names_word("x", ""));
        assert_eq!(display("escalate-mode"), "Escalate");
        assert_eq!(display(""), "");
    }

    #[test]
    fn exit_sections_end_at_the_next_heading() {
        let body = "# Mode\nIntro\n## When to exit\nHand to Build.\n## Other\nNo.";
        assert_eq!(exit_section(body), "Hand to Build.");
        assert_eq!(exit_section("No heading"), "");
    }

    #[test]
    fn memory_policy_states_each_field() {
        let contract = MemoryContract {
            retains: Vec::new(),
            excludes: vec!["secrets".to_owned()],
            max_age_days: None,
            unbounded: true,
            store_location: String::new(),
            memory_service: Some("urn:ordin:x:tool-bindings:memory".to_owned()),
        };
        assert_eq!(
            memory_policy("m", &contract),
            "**m** keeps: nothing listed. Never keeps: secrets. Retention: unbounded. Store: not stated.\nMemory service: `urn:ordin:x:tool-bindings:memory`.\n"
        );
        let none = MemoryContract {
            unbounded: false,
            ..contract
        };
        assert!(memory_policy("m", &none).contains("Retention: not stated."));
    }
}
