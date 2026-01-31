//! Single-file kind proposal (FormatPlugins.md *Internal Logic* 9).

use ordin_agent_model::{parse_manifest, PartKind, VocabularySet};
use serde_json::Value;

use crate::error::FormatError;
use crate::front::file_name;
use crate::guard::path_is_invalid;
use crate::types::{FormatId, KindProposal};

fn proposal(kind: PartKind, from_format: FormatId, rule: &'static str) -> KindProposal {
    KindProposal {
        kind,
        from_format,
        rule,
    }
}

/// The format a suffix-named file most likely comes from: `.apm/` and `.claude/` paths say so; the
/// `.instructions.md`, `.prompt.md`, `.chatmode.md` and `.agent.md` suffixes are Copilot's.
// The format is read from the path (`.apm/`, `.claude/`), then from the file name (`CLAUDE.md`,
// `SKILL.md`, `.mcp.json` are Claude Code's; the suffixes above are Copilot's); an ordin manifest is Ordin.
fn format_of(path: &str, default: FormatId) -> FormatId {
    if path.starts_with(".apm/") {
        FormatId::Apm
    } else if path.starts_with(".claude/") {
        FormatId::ClaudeCode
    } else if path.starts_with(".github/") {
        FormatId::Copilot
    } else {
        default
    }
}

/// A fixed table, first match wins; `rule` names the matching row.
pub(crate) fn propose(path: &str, bytes: &[u8]) -> Result<KindProposal, FormatError> {
    if path_is_invalid(path) {
        return Err(FormatError::PathInvalid {
            path: path.to_owned(),
        });
    }
    let name = file_name(path);
    if name == "SKILL.md" {
        return Ok(proposal(
            PartKind::Skill,
            format_of(path, FormatId::ClaudeCode),
            "skill-file",
        ));
    }
    if name.ends_with(".instructions.md")
        || name == "CLAUDE.md"
        || name == "copilot-instructions.md"
    {
        let default = if name == "CLAUDE.md" {
            FormatId::ClaudeCode
        } else {
            FormatId::Copilot
        };
        return Ok(proposal(
            PartKind::Rule,
            format_of(path, default),
            "instructions-file",
        ));
    }
    if name.ends_with(".prompt.md") || path.starts_with(".claude/commands/") {
        return Ok(proposal(
            PartKind::Command,
            format_of(path, FormatId::Copilot),
            "prompt-file",
        ));
    }
    if name.ends_with(".chatmode.md") {
        return Ok(proposal(
            PartKind::OperatingMode,
            format_of(path, FormatId::Copilot),
            "chat-mode-file",
        ));
    }
    if name.ends_with(".agent.md") || path.starts_with(".claude/agents/") {
        return Ok(proposal(
            PartKind::SystemRole,
            format_of(path, FormatId::Copilot),
            "agent-file",
        ));
    }
    if name.ends_with(".json") {
        let has_servers = matches!(
            serde_json::from_slice::<Value>(bytes),
            Ok(Value::Object(top)) if top.get("mcpServers").is_some_and(Value::is_object)
        );
        if has_servers {
            return Ok(proposal(
                PartKind::ToolBinding,
                FormatId::ClaudeCode,
                "mcp-servers-file",
            ));
        }
    }
    if let Ok(Some(manifest)) = parse_manifest(path, bytes) {
        if manifest.require_api_version().is_ok() {
            if let Some(kind) = manifest.kind(&VocabularySet::default()) {
                return Ok(proposal(kind, FormatId::Ordin, "ordin-manifest"));
            }
        }
    }
    Err(FormatError::KindNotProposable {
        path: path.to_owned(),
    })
}
