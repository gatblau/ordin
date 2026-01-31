//! The Claude Code import plug-in (FormatPlugins.md *Internal Logic* 2 and 7). Read only.
//!
//! Layouts are pinned to the harness versions ordin pins (C-70); a set-up file outside the paths
//! below is reported, never guessed. Nothing in a set-up file is executed or followed (R-84).

use std::collections::{BTreeMap, BTreeSet};

use ordin_agent_model::{PartKind, YamlValue};
use serde_json::Value;

use crate::error::FormatError;
use crate::front::{child_slug, file_name};
use crate::guard::{guard, MAX_FILE_BYTES};
use crate::import::{IdSpec, Importer, MdSpec, Refs};
use crate::log;
use crate::types::{
    Detection, FormatId, FormatPlugin, FormatVersions, MappingReport, ReadOutcome, ReadRequest,
    SourceTree, UnmappedReason, WriteOutcome, WriteRequest,
};

const UNVERSIONED: &str = "unversioned";
/// The version every part an import creates carries: a set-up has no package version.
// `1.0.0` is the draft default the Agent Model uses for a new part.
pub(crate) const IMPORT_VERSION: &str = "1.0.0";

pub(crate) struct ClaudeCodePlugin;

fn agent_slug(path: &str) -> Option<&str> {
    child_slug(path, ".claude/agents", ".md")
}

fn command_slug(path: &str) -> Option<&str> {
    child_slug(path, ".claude/commands", ".md")
}

/// `.claude/skills/<slug>/<rest>`.
fn skill_file(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix(".claude/skills/")?;
    let (slug, inner) = rest.split_once('/')?;
    (!slug.is_empty() && !inner.is_empty()).then_some((slug, inner))
}

impl FormatPlugin for ClaudeCodePlugin {
    fn id(&self) -> FormatId {
        FormatId::ClaudeCode
    }

    fn versions(&self) -> FormatVersions {
        FormatVersions {
            reads: vec![UNVERSIONED.to_owned()],
            writes: Vec::new(),
        }
    }

    fn detect(&self, tree: &SourceTree) -> Option<Detection> {
        let has_root = tree.0.contains_key("CLAUDE.md");
        let agents = tree.0.keys().filter(|p| agent_slug(p).is_some()).count();
        let others = tree.0.keys().any(|p| {
            command_slug(p).is_some() || skill_file(p).is_some_and(|(_, inner)| inner == "SKILL.md")
        });
        if !has_root && agents == 0 && !others {
            return None;
        }
        Some(Detection {
            format: FormatId::ClaudeCode,
            declared_version: UNVERSIONED.to_owned(),
            version_tested: true,
            agent_count: u32::try_from(agents + usize::from(has_root)).unwrap_or(u32::MAX),
        })
    }

    fn read(&self, tree: &SourceTree, request: &ReadRequest) -> Result<ReadOutcome, FormatError> {
        guard(tree)?;
        let mut importer = Importer::new(&request.publisher, IMPORT_VERSION);
        importer.check_publisher()?;
        let mut refs = Refs::default();
        let mut consumed: BTreeSet<String> = BTreeSet::new();
        if let Some(bytes) = tree.0.get("CLAUDE.md") {
            read_claude_md(&mut importer, tree, bytes, &mut refs, &mut consumed)?;
        }
        let mut agents: Vec<(&str, &[u8], &str)> = Vec::new();
        let mut skills: BTreeMap<&str, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
        for (path, bytes) in &tree.0 {
            if path == "CLAUDE.md" || consumed.contains(path) {
                continue;
            }
            if path == ".mcp.json" {
                read_mcp(&mut importer, path, bytes, &mut refs)?;
            } else if path == ".claude/settings.json" || path == ".claude/settings.local.json" {
                let construct = if has_hooks_key(bytes) {
                    "hook"
                } else {
                    "settings"
                };
                importer.item(path, construct, UnmappedReason::Executable, None);
            } else if let Some(slug) = agent_slug(path) {
                agents.push((path, bytes, slug));
            } else if let Some(slug) = command_slug(path) {
                let spec = MdSpec {
                    kind: PartKind::Command,
                    path,
                    bytes,
                    id: IdSpec::Mint {
                        category: Some("imported"),
                        slug,
                    },
                    default_slug: slug,
                    known_keys: &[],
                    extras: BTreeMap::new(),
                    whole_text: false,
                    prompt_inputs: false,
                };
                if let Some(created) = importer.md_part(spec)? {
                    refs.commands.push(created.id);
                }
            } else if let Some((slug, inner)) = skill_file(path) {
                skills
                    .entry(slug)
                    .or_default()
                    .insert(inner.to_owned(), bytes.clone());
            } else if path.starts_with(".claude/") {
                importer.item(path, "file", UnmappedReason::UnrecognisedPath, None);
            }
        }
        for (slug, mut files) in skills {
            let Some(skill_md) = files.remove("SKILL.md") else {
                for inner in files.keys() {
                    let path = format!(".claude/skills/{slug}/{inner}");
                    importer.item(&path, "file", UnmappedReason::UnrecognisedPath, None);
                }
                continue;
            };
            let path = format!(".claude/skills/{slug}/SKILL.md");
            let spec = MdSpec {
                kind: PartKind::Skill,
                path: &path,
                bytes: &skill_md,
                id: IdSpec::Mint {
                    category: Some("imported"),
                    slug,
                },
                default_slug: slug,
                known_keys: &[],
                extras: files,
                whole_text: false,
                prompt_inputs: false,
            };
            if let Some(created) = importer.md_part(spec)? {
                refs.skills.push(created.id);
            }
        }
        if agents.is_empty() {
            importer.source_workspace(&request.source_label, &refs)?;
        } else {
            for (path, bytes, slug) in agents {
                importer.agent_profile(path, bytes, slug, &refs)?;
            }
        }
        let (parts, items) = importer.finish();
        Ok(ReadOutcome {
            parts,
            mapping_report: MappingReport {
                format: Some(FormatId::ClaudeCode),
                items,
            },
            package: None,
        })
    }

    fn write(&self, _request: &WriteRequest) -> Result<WriteOutcome, FormatError> {
        let error = FormatError::WriteUnsupported {
            format: FormatId::ClaudeCode,
        };
        log::write_failed(FormatId::ClaudeCode, &error, 0);
        Err(error)
    }
}

/// `CLAUDE.md` is one Rule; each `@<relative path>` line that names a file of the tree is a Rule of
/// its own, and one that does not is reported.
fn read_claude_md(
    importer: &mut Importer,
    tree: &SourceTree,
    bytes: &[u8],
    refs: &mut Refs,
    consumed: &mut BTreeSet<String>,
) -> Result<(), FormatError> {
    let spec = MdSpec {
        kind: PartKind::Rule,
        path: "CLAUDE.md",
        bytes,
        id: IdSpec::Mint {
            category: Some("imported"),
            slug: "claude-md",
        },
        default_slug: "claude-md",
        known_keys: &[],
        extras: BTreeMap::new(),
        whole_text: true,
        prompt_inputs: false,
    };
    let Some(created) = importer.md_part(spec)? else {
        return Ok(());
    };
    refs.rules.push(created.id);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok(());
    };
    for line in text.lines() {
        let Some(target) = line.trim().strip_prefix('@') else {
            continue;
        };
        if target.is_empty() || target.contains(char::is_whitespace) {
            continue;
        }
        let target = target.strip_prefix("./").unwrap_or(target);
        let found = tree
            .0
            .get_key_value(target)
            .filter(|_| !target.split('/').any(|s| s == ".." || s.is_empty()));
        let Some((path, imported)) = found else {
            importer.item(
                "CLAUDE.md",
                &format!("@{target}"),
                UnmappedReason::UnrecognisedPath,
                None,
            );
            continue;
        };
        if !consumed.insert(path.clone()) {
            continue;
        }
        let name = file_name(path);
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        let spec = MdSpec {
            kind: PartKind::Rule,
            path,
            bytes: imported,
            id: IdSpec::Mint {
                category: Some("imported"),
                slug: stem,
            },
            default_slug: stem,
            known_keys: &[],
            extras: BTreeMap::new(),
            whole_text: true,
            prompt_inputs: false,
        };
        if let Some(created) = importer.md_part(spec)? {
            refs.rules.push(created.id);
        }
    }
    Ok(())
}

/// True when a JSON settings file has a top-level `hooks` key.
fn has_hooks_key(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|v| v.as_object().map(|o| o.contains_key("hooks")))
        .unwrap_or(false)
}

/// `.mcp.json`: a ToolBinding per `mcpServers` entry. An `env` or `headers` value is never copied
/// (SA-CRED-01); each such entry is reported by name only.
// spec: ordin-desktop/components/FormatPlugins#SA-CRED-01
pub(crate) fn read_mcp(
    importer: &mut Importer,
    path: &str,
    bytes: &[u8],
    refs: &mut Refs,
) -> Result<(), FormatError> {
    if bytes.len() > MAX_FILE_BYTES {
        importer.item(
            path,
            "file larger than 1 MiB",
            UnmappedReason::Unreadable,
            None,
        );
        return Ok(());
    }
    let Ok(Value::Object(top)) = serde_json::from_slice::<Value>(bytes) else {
        importer.item(path, "JSON object", UnmappedReason::Unreadable, None);
        return Ok(());
    };
    for key in top.keys().filter(|k| k.as_str() != "mcpServers") {
        let (construct, reason) = if key == "hooks" {
            ("hook".to_owned(), UnmappedReason::Executable)
        } else {
            (format!("key {key}"), UnmappedReason::UnknownField)
        };
        importer.item(path, &construct, reason, None);
    }
    let servers = match top.get("mcpServers") {
        None => return Ok(()),
        Some(Value::Object(servers)) => servers,
        Some(_) => {
            importer.item(path, "mcpServers", UnmappedReason::Unreadable, None);
            return Ok(());
        }
    };
    for (name, entry) in servers {
        let Value::Object(entry) = entry else {
            importer.item(
                path,
                &format!("mcp server {name}"),
                UnmappedReason::Unreadable,
                None,
            );
            continue;
        };
        let server_type = if entry.contains_key("command") {
            Some("stdio")
        } else if entry.contains_key("url") {
            Some("http")
        } else {
            None
        };
        let transport = entry
            .get("type")
            .and_then(Value::as_str)
            .or(server_type)
            .map(str::to_owned);
        let mut binding = BTreeMap::new();
        binding.insert("name".to_owned(), YamlValue::Str(name.clone()));
        binding.insert("type".to_owned(), YamlValue::Str("mcp".to_owned()));
        if let Some(server_type) = server_type {
            binding.insert(
                "serverType".to_owned(),
                YamlValue::Str(server_type.to_owned()),
            );
        }
        if let Some(transport) = transport {
            binding.insert("transport".to_owned(), YamlValue::Str(transport));
        }
        let Some(id) = importer.tool_binding(name, YamlValue::Map(binding))? else {
            importer.item(
                path,
                &format!("mcp server {name}"),
                UnmappedReason::UnrecognisedPath,
                None,
            );
            continue;
        };
        if !refs.tools.contains(&id) {
            refs.tools.push(id.clone());
        }
        for (key, value) in entry {
            match key.as_str() {
                "command" | "url" | "type" => {}
                "env" | "headers" => {
                    if let Value::Object(inner) = value {
                        for secret_name in inner.keys() {
                            importer.item(
                                path,
                                &format!("{key} {secret_name}"),
                                UnmappedReason::UnknownField,
                                Some(&id),
                            );
                        }
                    } else {
                        importer.item(path, key, UnmappedReason::UnknownField, Some(&id));
                    }
                }
                other => importer.item(
                    path,
                    &format!("mcp server key {other}"),
                    UnmappedReason::UnknownField,
                    Some(&id),
                ),
            }
        }
    }
    Ok(())
}
