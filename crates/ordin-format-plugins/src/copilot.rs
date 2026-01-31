//! The Copilot import plug-in (FormatPlugins.md *Internal Logic* 2 and 8). Read only.
//!
//! The agent and chat-mode paths are *assumed here* pending C-70.

use std::collections::BTreeMap;

use ordin_agent_model::PartKind;

use crate::claude::IMPORT_VERSION;
use crate::error::FormatError;
use crate::front::child_slug;
use crate::guard::guard;
use crate::import::{IdSpec, Importer, MdSpec, Refs};
use crate::log;
use crate::types::{
    Detection, FormatId, FormatPlugin, FormatVersions, MappingReport, ReadOutcome, ReadRequest,
    SourceTree, UnmappedReason, WriteOutcome, WriteRequest,
};

const UNVERSIONED: &str = "unversioned";
const GENERAL: &str = ".github/copilot-instructions.md";

pub(crate) struct CopilotPlugin;

fn instruction_slug(path: &str) -> Option<&str> {
    child_slug(path, ".github/instructions", ".instructions.md")
}
fn prompt_slug(path: &str) -> Option<&str> {
    child_slug(path, ".github/prompts", ".prompt.md")
}
fn agent_slug(path: &str) -> Option<&str> {
    child_slug(path, ".github/agents", ".agent.md")
}
fn chatmode_slug(path: &str) -> Option<&str> {
    child_slug(path, ".github/chatmodes", ".chatmode.md")
}

impl FormatPlugin for CopilotPlugin {
    fn id(&self) -> FormatId {
        FormatId::Copilot
    }

    fn versions(&self) -> FormatVersions {
        FormatVersions {
            reads: vec![UNVERSIONED.to_owned()],
            writes: Vec::new(),
        }
    }

    fn detect(&self, tree: &SourceTree) -> Option<Detection> {
        let general = tree.0.contains_key(GENERAL);
        let agents = tree.0.keys().filter(|p| agent_slug(p).is_some()).count();
        let others = tree.0.keys().any(|p| {
            instruction_slug(p).is_some() || prompt_slug(p).is_some() || chatmode_slug(p).is_some()
        });
        if !general && agents == 0 && !others {
            return None;
        }
        Some(Detection {
            format: FormatId::Copilot,
            declared_version: UNVERSIONED.to_owned(),
            version_tested: true,
            agent_count: u32::try_from(agents + usize::from(general)).unwrap_or(u32::MAX),
        })
    }

    fn read(&self, tree: &SourceTree, request: &ReadRequest) -> Result<ReadOutcome, FormatError> {
        guard(tree)?;
        let mut importer = Importer::new(&request.publisher, IMPORT_VERSION);
        importer.check_publisher()?;
        let mut refs = Refs::default();
        let mut agents: Vec<(&str, &[u8], &str)> = Vec::new();
        for (path, bytes) in &tree.0 {
            if !path.starts_with(".github/") {
                continue;
            }
            let (kind, slug) = if path == GENERAL {
                (PartKind::Rule, "copilot-instructions")
            } else if let Some(slug) = instruction_slug(path) {
                (PartKind::Rule, slug)
            } else if let Some(slug) = prompt_slug(path) {
                (PartKind::Command, slug)
            } else if let Some(slug) = chatmode_slug(path) {
                (PartKind::OperatingMode, slug)
            } else if let Some(slug) = agent_slug(path) {
                agents.push((path, bytes, slug));
                continue;
            } else {
                importer.item(path, "file", UnmappedReason::UnrecognisedPath, None);
                continue;
            };
            let spec = MdSpec {
                kind,
                path,
                bytes,
                id: IdSpec::Mint {
                    category: (kind != PartKind::OperatingMode).then_some("imported"),
                    slug,
                },
                default_slug: slug,
                known_keys: &[],
                extras: BTreeMap::new(),
                whole_text: false,
            };
            if let Some(created) = importer.md_part(spec)? {
                match kind {
                    PartKind::Rule => refs.rules.push(created.id),
                    PartKind::Command => refs.commands.push(created.id),
                    _ => refs.modes.push(created.id),
                }
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
                format: Some(FormatId::Copilot),
                items,
            },
            package: None,
        })
    }

    fn write(&self, _request: &WriteRequest) -> Result<WriteOutcome, FormatError> {
        let error = FormatError::WriteUnsupported {
            format: FormatId::Copilot,
        };
        log::write_failed(FormatId::Copilot, &error, 0);
        Err(error)
    }
}
