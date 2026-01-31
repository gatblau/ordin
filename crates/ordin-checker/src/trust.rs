//! The trust policy, the executable parts of an agent and their trust (Checker.md *Internal Logic*
//! 17 and 31; ADR-082, ADR-086, ADR-094 item 5).

use std::collections::BTreeSet;

use ordin_agent_model::{LoadingMode, Manifest, PartKind, ResolvedToolPolicy};
use ordin_types::domain::agents::{ExecutablePartKind, ExecutablePartTrust};

use crate::finding::{finding, msg};
use crate::log;
use crate::model::{ExecutablePart, Finding, Fix, Severity, TrustPolicy};

impl TrustPolicy {
    /// Parses the profile's JSON value. Absent → empty policy. A value that does not parse →
    /// empty policy (fail closed: every executable part needs IT approval), logged
    /// `checker.trust_policy_invalid` at `error`.
    pub fn from_profile_value(value: Option<&serde_json::Value>) -> TrustPolicy {
        parse_policy(value, None)
    }

    /// The trust of one executable part: the declaring ToolBinding is listed with the program or
    /// server in its `programs`, or the hook is listed.
    // spec: ordin-desktop/components/Checker#CHK-TR-1
    pub(crate) fn decide(&self, part: &ExecutablePart) -> ExecutablePartTrust {
        if part.bundled {
            return ExecutablePartTrust::Trusted;
        }
        let trusted = match part.kind {
            ExecutablePartKind::Hook => self.hooks.contains(&part.part_name),
            ExecutablePartKind::McpServer | ExecutablePartKind::BundledProgram => {
                self.tool_bindings.iter().any(|binding| {
                    binding.id == part.declared_by && binding.programs.contains(&part.part_name)
                })
            }
        };
        if trusted {
            ExecutablePartTrust::Trusted
        } else {
            ExecutablePartTrust::NeedsItApproval
        }
    }
}

/// `from_profile_value` with the profile version for the log line.
pub(crate) fn parse_policy(
    value: Option<&serde_json::Value>,
    profile_version: Option<&str>,
) -> TrustPolicy {
    match value {
        None => TrustPolicy::default(),
        Some(value) => serde_json::from_value(value.clone()).unwrap_or_else(|_| {
            log::trust_policy_invalid(profile_version);
            TrustPolicy::default()
        }),
    }
}

/// One part as the derivation sees it: kind, manifest.
pub(crate) struct Declaring<'a> {
    pub kind: PartKind,
    pub manifest: &'a Manifest,
    /// Read from the bundled catalogue (Checker.md CHK-TR-1).
    pub bundled: bool,
}

/// True for a part loaded with mode `event` (a hook).
fn is_hook(manifest: &Manifest) -> bool {
    manifest.metadata.loading.as_deref().is_some_and(|l| {
        serde_json::from_value::<LoadingMode>(serde_json::Value::String(l.to_owned()))
            .is_ok_and(|m| m == LoadingMode::Event)
    })
}

/// Step 31: each MCP server of the tool policy, each program a ToolBinding starts (a binding whose
/// type is neither `mcp` nor `builtin`), each part loaded on an event. A duplicate (name, kind)
/// keeps the first (`EXP_PART_DUPLICATE` is never stored).
// spec: ordin-desktop/components/Checker#SA-CLOSE-03
pub(crate) fn executable_parts(
    policy: Option<&ResolvedToolPolicy>,
    parts: &[Declaring<'_>],
) -> Vec<ExecutablePart> {
    let mut out: Vec<ExecutablePart> = Vec::new();
    if let Some(policy) = policy {
        out.extend(policy.mcp_servers.iter().map(|server| {
            ExecutablePart {
                part_name: server.name.clone(),
                kind: ExecutablePartKind::McpServer,
                declared_by: server.declared_by.clone(),
                bundled: parts
                    .iter()
                    .any(|p| p.bundled && p.manifest.metadata.id == server.declared_by),
            }
        }));
    }
    for part in parts {
        let id = &part.manifest.metadata.id;
        if part.kind == PartKind::ToolBinding {
            for binding in part.manifest.tool_bindings().unwrap_or_default() {
                let kind = match binding.binding_type.as_str() {
                    "builtin" => continue,
                    // A draft has no resolved tool policy: its MCP servers come from its bindings.
                    "mcp" if policy.is_some() => continue,
                    "mcp" => ExecutablePartKind::McpServer,
                    _ => ExecutablePartKind::BundledProgram,
                };
                out.push(ExecutablePart {
                    part_name: binding.name.clone(),
                    kind,
                    declared_by: id.clone(),
                    bundled: part.bundled,
                });
            }
        }
        if is_hook(part.manifest) {
            out.push(ExecutablePart {
                part_name: id.clone(),
                kind: ExecutablePartKind::Hook,
                declared_by: id.clone(),
                bundled: part.bundled,
            });
        }
    }
    let mut seen = BTreeSet::new();
    out.retain(|p| seen.insert((p.part_name.clone(), p.kind as u8)));
    out
}

/// Step 17: a finding for each executable part the policy does not trust, and for each built-in
/// tool the policy does not allow.
pub(crate) fn trust_findings(
    policy: &TrustPolicy,
    executable: &[ExecutablePart],
    parts: &[Declaring<'_>],
) -> Vec<Finding> {
    let mut out = Vec::new();
    for part in executable {
        if policy.decide(part) == ExecutablePartTrust::NeedsItApproval {
            let what = match part.kind {
                ExecutablePartKind::McpServer => "mcp server",
                ExecutablePartKind::BundledProgram => "program",
                ExecutablePartKind::Hook => "hook",
            };
            out.push(finding(
                "trust_needs_it_approval",
                Severity::Failure,
                Some(&part.declared_by),
                msg::TRUST_NEEDS_IT_APPROVAL,
                &[("tool", &part.part_name)],
                vec![
                    Fix::LeaveItOut {
                        what: part.part_name.clone(),
                    },
                    Fix::AskIt,
                ],
                format!(
                    "{what} {} declared by {} is not in agent_catalogue.trust_policy",
                    part.part_name, part.declared_by
                ),
            ));
        }
    }
    if !policy.allowed_builtin_tools.is_empty() {
        // A part read from the bundled catalogue is outside the company's policy (CHK-TR-1).
        for part in parts
            .iter()
            .filter(|p| p.kind == PartKind::ToolBinding && !p.bundled)
        {
            for binding in part.manifest.tool_bindings().unwrap_or_default() {
                if binding.binding_type == "builtin"
                    && !policy.allowed_builtin_tools.contains(&binding.name)
                {
                    out.push(finding(
                        "tool_not_allowed",
                        Severity::Failure,
                        Some(&part.manifest.metadata.id),
                        msg::TOOL_NOT_ALLOWED,
                        &[("tool", &binding.name)],
                        vec![
                            Fix::LeaveItOut {
                                what: binding.name.clone(),
                            },
                            Fix::AskIt,
                        ],
                        format!(
                            "built-in tool {} is not in agent_catalogue.trust_policy.allowed_builtin_tools",
                            binding.name
                        ),
                    ));
                }
            }
        }
    }
    out
}

pub(crate) fn kind_text(kind: ExecutablePartKind) -> &'static str {
    match kind {
        ExecutablePartKind::Hook => "hook",
        ExecutablePartKind::McpServer => "mcp_server",
        ExecutablePartKind::BundledProgram => "bundled_program",
    }
}

pub(crate) fn kind_from_text(text: &str) -> Option<ExecutablePartKind> {
    match text {
        "hook" => Some(ExecutablePartKind::Hook),
        "mcp_server" => Some(ExecutablePartKind::McpServer),
        "bundled_program" => Some(ExecutablePartKind::BundledProgram),
        _ => None,
    }
}

pub(crate) fn trust_text(trust: ExecutablePartTrust) -> &'static str {
    match trust {
        ExecutablePartTrust::Trusted => "trusted",
        ExecutablePartTrust::NeedsItApproval => "needs_it_approval",
    }
}

pub(crate) fn trust_from_text(text: &str) -> Option<ExecutablePartTrust> {
    match text {
        "trusted" => Some(ExecutablePartTrust::Trusted),
        "needs_it_approval" => Some(ExecutablePartTrust::NeedsItApproval),
        _ => None,
    }
}
