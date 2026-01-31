//! The Copilot layouts (Installers.md *Internal Logic* steps 4 and 5): vscode, which C-60 selects
//! for `HarnessKind::CopilotCli`, and agents-md, kept for C-60's other outcome. Neither writes an
//! MCP file: the agent's MCP servers start only through the trust policy (ADR-090 item 1).

use ordin_agent_model::{canonical_json, PartKind};

use crate::render::{header, is_behavioural, single_file, whole_dir, Inputs, Part, TreeBuilder};
use crate::types::{CommandName, RenderedTree};

const INDEX: &str = ".github/copilot-instructions.md";
const AGENTS_MD: &str = "AGENTS.md";
const VSCODE_SETTINGS: &str = ".vscode/settings.json";
const FALLBACK_DIR: &str = "agents";
const INSTALLED_BY: &str = "Installed by ordin.";

/// The slug a Copilot layout names a part by: the identifier's last segment.
fn slug(part: &Part) -> Option<&str> {
    part.urn.as_ref().map(|u| u.slug.as_str())
}

/// Step 4: the vscode layout.
// The `.vscode/settings.json` toggle is VS Code's `chat.useAgentsMdFile`.
pub(crate) fn vscode(inputs: &Inputs) -> RenderedTree {
    let mut tree = TreeBuilder::default();
    let mut instructions = Vec::new();
    let mut prompts: Vec<(String, String)> = Vec::new();
    let mut context = Vec::new();
    let mut fallback = Vec::new();
    let mut reference_only = Vec::new();
    for part in &inputs.parts {
        let placed = match (part.part.kind, slug(part)) {
            (kind, Some(slug)) if is_behavioural(kind) => {
                let base = format!(".github/instructions/{slug}");
                let mut entries = single_file(part, &base, ".instructions.md");
                if let Some((_, first)) = entries.first_mut() {
                    *first = format!("---\napplyTo: \"**\"\n---\n\n{}", part.body()).into_bytes();
                }
                let placed = part.primary.is_some() && tree.place(entries);
                if placed {
                    instructions.push(format!("{base}.instructions.md"));
                }
                placed
            }
            (PartKind::Command, Some(slug)) => {
                let base = format!(".github/prompts/{slug}");
                let mut entries = single_file(part, &base, ".prompt.md");
                if let Some((_, first)) = entries.first_mut() {
                    // INST-TF-1: a command with inputs is written with one place for its values.
                    *first = crate::render::command_task_body(part)
                        .unwrap_or_else(|| part.body())
                        .into_bytes();
                }
                let placed = part.primary.is_some() && tree.place(entries);
                if placed {
                    prompts.push((part.part.id.clone(), slug.to_owned()));
                }
                placed
            }
            (PartKind::ContextResource, Some(slug)) => {
                let base = format!(".github/context/{slug}");
                let placed = part.primary.is_some() && tree.place(single_file(part, &base, ".md"));
                if placed {
                    context.push(format!("{base}.md"));
                }
                placed
            }
            _ => false,
        };
        if !placed {
            tree.place(whole_dir(
                part,
                &format!("{FALLBACK_DIR}/{}", part.fallback_name()),
            ));
            fallback.push(part.listed_name());
            reference_only.push(part.part.id.clone());
        }
    }
    instructions.sort();
    prompts.sort_by(|a, b| a.1.cmp(&b.1));
    context.sort();
    fallback.sort();
    reference_only.sort();

    let mut index = header(&inputs.root);
    index.push_str(&format!(
        "\n{INSTALLED_BY} Copilot applies the instructions under `.github/instructions/` to every \
         request; the agent's commands are the prompt files under `.github/prompts/`.\n"
    ));
    section(
        &mut index,
        "Instructions",
        instructions.iter().map(|p| format!("`{p}`")),
    );
    section(
        &mut index,
        "Commands",
        prompts
            .iter()
            .map(|(_, s)| format!("`/{s}` — `.github/prompts/{s}.prompt.md`")),
    );
    section(
        &mut index,
        "Context resources",
        context.iter().map(|p| format!("`{p}`")),
    );
    fallback_section(&mut index, &fallback);
    tree.files.insert(INDEX.to_owned(), index.into_bytes());

    let mut agents = header(&inputs.root);
    agents.push_str(&format!(
        "\n{INSTALLED_BY} The agent's instructions are under `.github/instructions/`, its commands \
         under `.github/prompts/`, and the index of everything installed is \
         `.github/copilot-instructions.md`.\n"
    ));
    tree.files.insert(AGENTS_MD.to_owned(), agents.into_bytes());
    tree.files.insert(
        VSCODE_SETTINGS.to_owned(),
        canonical_json(&serde_json::json!({ "chat.useAgentsMdFile": true })),
    );
    RenderedTree {
        files: tree.files,
        launch_paths: None,
        commands: prompts
            .into_iter()
            .map(|(command_id, session_name)| CommandName {
                command_id,
                session_name,
            })
            .collect(),
        reference_only_parts: reference_only,
    }
}

/// Step 5: the agents-md layout. Commands have no invocable form in it, so `commands` is empty.
// Commands are placed as reference only and named nowhere, and each behavioural part is one section
// headed by its kind and name.
pub(crate) fn agents_md(inputs: &Inputs) -> RenderedTree {
    let mut tree = TreeBuilder::default();
    let mut sections = String::new();
    let mut context = Vec::new();
    let mut fallback = Vec::new();
    let mut reference_only = Vec::new();
    for part in &inputs.parts {
        let placed = match (part.part.kind, slug(part)) {
            (kind, _) if is_behavioural(kind) && part.primary.is_some() => {
                sections.push_str(&format!(
                    "\n## {:?}: {}\n\n{}\n",
                    kind,
                    part.part.name,
                    part.body().trim_end_matches('\n')
                ));
                true
            }
            (PartKind::ContextResource, Some(slug)) => {
                let base = format!("{FALLBACK_DIR}/context/{slug}");
                let placed = part.primary.is_some() && tree.place(single_file(part, &base, ".md"));
                if placed {
                    context.push(format!("{base}.md"));
                }
                placed
            }
            _ => false,
        };
        if !placed {
            tree.place(whole_dir(
                part,
                &format!("{FALLBACK_DIR}/{}", part.fallback_name()),
            ));
            fallback.push(part.listed_name());
            reference_only.push(part.part.id.clone());
        }
    }
    context.sort();
    fallback.sort();
    reference_only.sort();
    let mut agents = header(&inputs.root);
    agents.push_str(&format!(
        "\n{INSTALLED_BY} The agent's instructions follow; reference material is under `agents/`.\n"
    ));
    agents.push_str(&sections);
    section(
        &mut agents,
        "Context resources",
        context.iter().map(|p| format!("`{p}`")),
    );
    fallback_section(&mut agents, &fallback);
    tree.files.insert(AGENTS_MD.to_owned(), agents.into_bytes());
    RenderedTree {
        files: tree.files,
        launch_paths: None,
        commands: Vec::new(),
        reference_only_parts: reference_only,
    }
}

/// `## <title>` and one bullet per item; nothing when there is no item.
fn section(out: &mut String, title: &str, items: impl Iterator<Item = String>) {
    let items: Vec<String> = items.collect();
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {title}\n\n"));
    for item in items {
        out.push_str(&format!("- {item}\n"));
    }
}

fn fallback_section(out: &mut String, listed: &[String]) {
    section(
        out,
        "Bundled artefacts (reference; not auto-loaded)",
        listed.iter().map(|l| {
            let dir = l.replacen('/', "-", 1);
            format!("{l} — `{FALLBACK_DIR}/{dir}/`")
        }),
    );
}
