//! The resolved tool policy (ResolverAndCompiler.md *Internal Logic* 13).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use ordin_agent_model::{DecimalString, McpServerDecl, PartKind, ResolvedToolPolicy};

use crate::error::{manifest_invalid, ResolveError};
use crate::held::Held;

/// The transports an MCP binding may declare.
const TRANSPORTS: [&str; 3] = ["stdio", "http", "sse"];

/// Compares two decimals (`-12.50`) by value. Both were validated as `^-?[0-9]{1,28}(\.[0-9]{1,10})?$`
/// on the way into the model.
pub(crate) fn compare_decimals(a: &str, b: &str) -> Ordering {
    fn parts(text: &str) -> (bool, String, String) {
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        let whole = whole.trim_start_matches('0').to_owned();
        let fraction = fraction.trim_end_matches('0').to_owned();
        let zero = whole.is_empty() && fraction.is_empty();
        (negative && !zero, whole, fraction)
    }
    let ((neg_a, whole_a, frac_a), (neg_b, whole_b, frac_b)) = (parts(a), parts(b));
    let magnitude = |whole_a: &str, frac_a: &str, whole_b: &str, frac_b: &str| {
        whole_a
            .len()
            .cmp(&whole_b.len())
            .then_with(|| whole_a.cmp(whole_b))
            .then_with(|| frac_a.cmp(frac_b))
    };
    match (neg_a, neg_b) {
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (false, false) => magnitude(&whole_a, &frac_a, &whole_b, &frac_b),
        (true, true) => magnitude(&whole_b, &frac_b, &whole_a, &frac_a),
    }
}

fn raise_floor(floor: &mut Option<DecimalString>, candidate: &DecimalString) {
    let higher = match floor {
        Some(current) => compare_decimals(&candidate.0, &current.0) == Ordering::Greater,
        None => true,
    };
    if higher {
        *floor = Some(candidate.clone());
    }
}

/// Composes the policy of the closure `held` for the agent `root`.
pub(crate) fn compose_tool_policy(
    root: &Held,
    held: &[Held],
) -> Result<ResolvedToolPolicy, ResolveError> {
    let mut forbidden: BTreeSet<String> = BTreeSet::new();
    let mut floor: Option<DecimalString> = None;
    let mut approvals: BTreeSet<String> = BTreeSet::new();
    let mut servers: BTreeMap<String, McpServerDecl> = BTreeMap::new();

    let mut bindings: Vec<&Held> = held
        .iter()
        .filter(|h| h.kind() == PartKind::ToolBinding)
        .collect();
    bindings.sort_by(|a, b| a.id().cmp(b.id()));
    for binding in bindings {
        let specs = binding
            .manifest()
            .tool_bindings()
            .map_err(|error| manifest_invalid(binding.id(), &error))?;
        for spec in specs {
            forbidden.extend(spec.forbidden_flags.iter().cloned());
            if let Some(value) = &spec.coverage_floor {
                raise_floor(&mut floor, value);
            }
            if spec.binding_type != "mcp" {
                continue;
            }
            let invalid = |reason: &'static str| ResolveError::ToolPolicyInvalid {
                binding: binding.id().to_owned(),
                reason,
            };
            if spec.name.is_empty() {
                return Err(invalid("name_missing"));
            }
            let server_type = match &spec.server_type {
                Some(value) if !value.is_empty() => value.clone(),
                _ => return Err(invalid("server_type_missing")),
            };
            let transport = match &spec.transport {
                Some(value) if TRANSPORTS.contains(&value.as_str()) => value.clone(),
                _ => return Err(invalid("transport_invalid")),
            };
            servers
                .entry(spec.name.clone())
                .or_insert_with(|| McpServerDecl {
                    name: spec.name.clone(),
                    server_type,
                    scope: spec.scope.clone(),
                    env_ref: spec.env_ref.clone(),
                    transport: Some(transport),
                    declared_by: binding.id().to_owned(),
                });
        }
    }

    for part in held.iter().filter(|h| h.kind() == PartKind::PracticePack) {
        let toolchain = part
            .manifest()
            .pack_toolchain()
            .map_err(|error| manifest_invalid(part.id(), &error))?;
        if let Some(toolchain) = toolchain {
            forbidden.extend(toolchain.forbidden_flags);
            if let Some(value) = &toolchain.coverage_floor {
                raise_floor(&mut floor, value);
            }
        }
    }

    for part in held.iter().filter(|h| h.kind() == PartKind::Capability) {
        let needed = part
            .manifest()
            .requires_human_approval()
            .map_err(|error| manifest_invalid(part.id(), &error))?;
        approvals.extend(needed);
    }

    let verbs: BTreeSet<String> = root
        .manifest()
        .prohibited_verbs()
        .map_err(|error| manifest_invalid(root.id(), &error))?
        .into_iter()
        .collect();

    Ok(ResolvedToolPolicy {
        forbidden_flags: forbidden.into_iter().collect(),
        coverage_floor: floor,
        prohibited_verbs: verbs.into_iter().collect(),
        requires_human_approval: approvals.into_iter().collect(),
        mcp_servers: servers.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_compare_by_value() {
        let cases = [
            ("70", "70.0", Ordering::Equal),
            ("0.75", "70", Ordering::Less),
            ("80.5", "80.25", Ordering::Greater),
            ("-1", "1", Ordering::Less),
            ("-1", "-2", Ordering::Greater),
            ("-0", "0", Ordering::Equal),
            ("100", "99.9999", Ordering::Greater),
            ("0.5", "0.50", Ordering::Equal),
        ];
        for (a, b, expected) in cases {
            assert_eq!(compare_decimals(a, b), expected, "{a} vs {b}");
            assert_eq!(compare_decimals(b, a), expected.reverse(), "{b} vs {a}");
        }
    }
}
