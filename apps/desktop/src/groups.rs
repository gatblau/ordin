//! The command groups this build answers (OrdinDesktop.md amendment January 2026, DSK-UP-7;
//! ServiceApi.md *Command groups by build switch*). A command of any other group is refused by the
//! Service API with `SVC_NOT_IN_THIS_VERSION` before it reaches this program.

use std::sync::OnceLock;

use ordin_service_api::{ALWAYS_ANSWERED_GROUPS, GROUPS_BY_SWITCH};

/// Each build switch that names a group, and whether this build has it.
const SWITCHES: &[(&str, bool)] = &[
    ("dictation", cfg!(feature = "dictation")),
    ("mail", cfg!(feature = "mail")),
    ("slack", cfg!(feature = "slack")),
    ("handover", cfg!(feature = "handover")),
    ("authoring", cfg!(feature = "authoring")),
    ("team-knowledge", cfg!(feature = "team-knowledge")),
    ("telegram", cfg!(feature = "telegram")),
    ("hosted-agents", cfg!(feature = "hosted-agents")),
    ("knowledge-agents", cfg!(feature = "knowledge-agents")),
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-1
    ("minutes", cfg!(feature = "minutes")),
];

/// Whether this build has the build switch `switch` (a key of `GROUPS_BY_SWITCH`).
pub fn switch_on(switch: &str) -> bool {
    SWITCHES.iter().any(|(name, on)| *name == switch && *on)
}

/// `ServiceApi::groups()` of `DesktopServiceApi` and `ServiceHello.groups`: sorted, no repeats,
/// fixed for the life of the process.
// spec: ordin-desktop/components/OrdinDesktop#DSK-UP-7
pub fn groups() -> &'static [&'static str] {
    static GROUPS: OnceLock<Vec<&'static str>> = OnceLock::new();
    GROUPS.get_or_init(|| {
        let mut groups: Vec<&'static str> = ALWAYS_ANSWERED_GROUPS.to_vec();
        groups.extend(
            GROUPS_BY_SWITCH
                .iter()
                .filter(|(switch, _)| switch_on(switch))
                .map(|(_, group)| *group),
        );
        groups.sort_unstable();
        groups.dedup();
        groups
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_service_api::{accept, CallOrigin, Incoming, CATALOGUE};
    use ordin_types::{BackendId, CommandRequestId};
    use serde_json::json;

    #[test]
    fn every_switch_of_the_table_is_a_switch_of_this_program() {
        for (switch, _) in GROUPS_BY_SWITCH {
            assert!(SWITCHES.iter().any(|(name, _)| name == switch), "{switch}");
        }
        for meta in CATALOGUE {
            let always = ALWAYS_ANSWERED_GROUPS.contains(&meta.module);
            let switched = GROUPS_BY_SWITCH.iter().any(|(_, g)| *g == meta.module);
            assert!(always ^ switched, "{} is classified once", meta.module);
        }
    }

    #[test]
    fn the_groups_are_sorted_with_no_repeats() {
        let groups = groups();
        let mut sorted = groups.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(groups, sorted.as_slice());
        for always in ALWAYS_ANSWERED_GROUPS {
            assert!(groups.contains(always), "{always}");
        }
        for (switch, group) in GROUPS_BY_SWITCH {
            assert_eq!(groups.contains(group), switch_on(switch), "{group}");
        }
    }

    #[cfg(not(any(
        feature = "mail",
        feature = "telegram",
        feature = "dictation",
        feature = "knowledge-agents"
    )))]
    #[test]
    fn the_greeting_names_the_groups_a_build_without_those_switches_answers() {
        let groups = groups();
        assert!(groups.contains(&"ordin") && groups.contains(&"setup"));
        for absent in ["mail", "telegram", "dictation", "knowledge_agents"] {
            assert!(!groups.contains(&absent), "{absent}");
        }
    }

    // Error Table (amendment January 2026): a command of a group this build does not answer.
    #[cfg(not(feature = "mail"))]
    #[test]
    fn error_path_a_command_of_a_group_the_build_lacks() {
        let backend = BackendId::new();
        let body = json!({
            "api_version": {"major": 1, "minor": 0},
            "request_id": CommandRequestId::new(),
            "command": "mail.list_drafts",
            "backend": backend,
            "input": {},
        })
        .to_string();
        let refusal = accept(&Incoming {
            body: body.as_bytes(),
            routed_name: "mail.list_drafts",
            origin: CallOrigin::LocalScreens,
            this_backend: backend,
            groups: groups(),
        })
        .expect_err("refused before any module is called");
        assert_eq!(refusal.code, "SVC_NOT_IN_THIS_VERSION");
    }

    #[test]
    fn a_command_of_a_group_the_build_answers_passes_the_group_check() {
        let backend = BackendId::new();
        let body = json!({
            "api_version": {"major": 1, "minor": 0},
            "request_id": CommandRequestId::new(),
            "command": "service.hello",
            "backend": backend,
            "input": {},
        })
        .to_string();
        let accepted = accept(&Incoming {
            body: body.as_bytes(),
            routed_name: "service.hello",
            origin: CallOrigin::LocalScreens,
            this_backend: backend,
            groups: groups(),
        })
        .expect("accepted");
        assert_eq!(accepted.meta.module, "service");
    }
}
