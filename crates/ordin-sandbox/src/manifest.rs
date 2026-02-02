//! The capability manifest each agent controller loads, rendered in memory from the agent's
//! grants (Sandbox.md *Internal Logic* D.3).

use serde_json::{json, Map, Value};

use crate::args::RESERVED_CAPABILITY;
use crate::grants::{
    KeyPlacementSpec, SandboxCapability, SandboxCapabilityKind, SandboxCredential,
};

/// The origin of the reserved `ordin_tools` capability; never dialled (the wrapper answers it).
pub(crate) const ORDIN_TOOLS_URL: &str = "https://ordin-tools.invalid";
/// The prefix of an agent key's resolver reference.
pub(crate) const KEY_REF_PREFIX: &str = "ordin-key:";

/// `^[a-z][a-z0-9_]{0,39}$`
pub(crate) fn is_capability_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && name.len() <= 40
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn credential_fields(credential: &SandboxCredential, entry: &mut Map<String, Value>) {
    match credential {
        SandboxCredential::None => {
            entry.insert("authMode".into(), json!("none"));
        }
        SandboxCredential::PersonToken { kind, .. } => {
            entry.insert("authMode".into(), json!("passthrough"));
            entry.insert(
                "provider".into(),
                serde_json::to_value(kind).unwrap_or(Value::Null),
            );
        }
        SandboxCredential::AgentKey {
            key_name,
            placement,
        } => {
            entry.insert("authMode".into(), json!("api_key"));
            entry.insert(
                "secretRef".into(),
                json!(format!("{KEY_REF_PREFIX}{key_name}")),
            );
            let placement = match placement {
                KeyPlacementSpec::Header { name, scheme } => {
                    json!({"header": {"name": name, "scheme": scheme}})
                }
                KeyPlacementSpec::Query { param } => json!({"query": {"param": param}}),
            };
            entry.insert("keyPlacement".into(), placement);
        }
    }
}

/// The capabilities kept from the grants: well-named, never `ordin_tools` (each dropped grant is
/// logged), in the order given.
pub(crate) fn kept(grants: Vec<SandboxCapability>) -> Vec<SandboxCapability> {
    grants
        .into_iter()
        .filter(|g| {
            let keep = g.name != RESERVED_CAPABILITY && is_capability_name(&g.name);
            if !keep {
                tracing::warn!(target: "ordin_sandbox", reason = "reserved_or_invalid_name", "sandbox.grant_dropped");
            }
            keep
        })
        .collect()
}

/// The manifest JSON for `PolicyEngine::load` (field names of `policy.rs:11`–`57`).
// spec: ordin-desktop/components/Sandbox#SA-RECON-05
pub(crate) fn render(capabilities: &[SandboxCapability]) -> String {
    let mut caps = Map::new();
    for cap in capabilities {
        let mut entry = Map::new();
        match &cap.kind {
            SandboxCapabilityKind::Rest {
                host,
                path_allow,
                methods,
            } => {
                entry.insert("kind".into(), json!("rest"));
                entry.insert("host".into(), json!(host));
                entry.insert("pathAllow".into(), json!(path_allow));
                entry.insert("methods".into(), json!(methods));
            }
            SandboxCapabilityKind::Mcp {
                server_url,
                tool_allow,
            } => {
                entry.insert("kind".into(), json!("mcp"));
                entry.insert("serverUrl".into(), json!(server_url));
                entry.insert("toolAllow".into(), json!(tool_allow));
            }
        }
        entry.insert("allowWrite".into(), json!(cap.allow_write));
        entry.insert(
            "allowPrivateNetwork".into(),
            json!(cap.allow_private_network),
        );
        entry.insert("requireStepUpAuth".into(), json!(false));
        credential_fields(&cap.credential, &mut entry);
        caps.insert(cap.name.clone(), Value::Object(entry));
    }
    // SPEC-GAP-1: D.3 sets `toolAllow` to "every module tool name", but the manifest is built
    // once per agent controller and the module's tools are the run's session tools (C.7), which
    // differ per run; `sandboxd`'s manifest check also refuses an empty `toolAllow`. The
    // wrapper's own check against the run's `session_tools` (E.3) is the enforced allow-list, so
    // the manifest carries a fixed wildcard entry that `sandboxd` never reads for this capability.
    caps.insert(
        RESERVED_CAPABILITY.to_owned(),
        json!({
            "kind": "mcp",
            "serverUrl": ORDIN_TOOLS_URL,
            "toolAllow": ["*"],
            "authMode": "none",
        }),
    );
    json!({ "capabilities": caps }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::domain::identity::CredentialKind;
    use ordin_types::CredentialId;
    use sandboxd::policy::PolicyEngine;

    fn rest(name: &str, credential: SandboxCredential) -> SandboxCapability {
        SandboxCapability {
            name: name.to_owned(),
            kind: SandboxCapabilityKind::Rest {
                host: "tickets.example.com".to_owned(),
                path_allow: vec!["^/v1/.*$".to_owned()],
                methods: vec!["GET".to_owned()],
            },
            allow_write: false,
            allow_private_network: false,
            credential,
        }
    }

    #[test]
    fn every_credential_form_loads_in_sandboxd() {
        let caps = vec![
            rest("open", SandboxCredential::None),
            rest(
                "tickets",
                SandboxCredential::PersonToken {
                    credential_id: CredentialId::new(),
                    kind: CredentialKind::Microsoft,
                },
            ),
            rest(
                "crm",
                SandboxCredential::AgentKey {
                    key_name: "crm_key".to_owned(),
                    placement: KeyPlacementSpec::Header {
                        name: "Authorization".to_owned(),
                        scheme: Some("Token".to_owned()),
                    },
                },
            ),
            rest(
                "search",
                SandboxCredential::AgentKey {
                    key_name: "search_key".to_owned(),
                    placement: KeyPlacementSpec::Query {
                        param: "key".to_owned(),
                    },
                },
            ),
            SandboxCapability {
                name: "wiki".to_owned(),
                kind: SandboxCapabilityKind::Mcp {
                    server_url: "https://wiki.example.com/mcp".to_owned(),
                    tool_allow: vec!["search".to_owned()],
                },
                allow_write: false,
                allow_private_network: true,
                credential: SandboxCredential::None,
            },
        ];
        let json = render(&caps);
        let policy = PolicyEngine::load(&json, None, &|_, _| false).expect("the manifest loads");
        assert_eq!(
            policy.resolve("tickets").expect("tickets").provider,
            "microsoft"
        );
        assert!(policy.resolve(RESERVED_CAPABILITY).is_some());
        assert_eq!(
            policy.api_key_secret_refs(),
            ["ordin-key:crm_key", "ordin-key:search_key"]
        );
    }

    #[test]
    fn a_reserved_or_badly_named_grant_is_dropped() {
        let kept = kept(vec![
            rest("ordin_tools", SandboxCredential::None),
            rest("Bad-Name", SandboxCredential::None),
            rest("ok", SandboxCredential::None),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "ok");
    }

    #[test]
    fn capability_names_follow_the_pattern() {
        assert!(is_capability_name("tickets_2"));
        assert!(!is_capability_name(""));
        assert!(!is_capability_name("2x"));
        assert!(!is_capability_name(&"a".repeat(41)));
    }
}
