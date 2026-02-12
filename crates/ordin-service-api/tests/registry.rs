//! X4, and the catalogue's own invariants (Internal Logic 1; ADR-114 groups; PERSON_ONLY_COMMANDS).

use std::collections::BTreeSet;

use ordin_service_api::*;
use ordin_types::error_registry::{lookup, owns_prefix};

/// 452 rows (451, then setup.choose_harness, SVC-CHO-1); 472 with the twenty `minutes` rows (SVC-MIN-1,
/// SVC-MIN-3).
const ROWS: usize = if cfg!(feature = "minutes") { 473 } else { 453 };

#[test]
fn every_code_is_registered_under_this_crate() {
    for code in ALL_CODES {
        assert!(lookup(code).is_some(), "{code} registered");
        let prefix = code.split('_').next().unwrap();
        assert!(
            owns_prefix("ordin-service-api", prefix),
            "{prefix} owned by ordin-service-api"
        );
    }
}

#[test]
fn catalogue_rows_are_derived_mechanically() {
    assert_eq!(CATALOGUE.len(), ROWS);
    let mut names = BTreeSet::new();
    for meta in CATALOGUE {
        assert!(names.insert(meta.name), "{} once", meta.name);
        let (module, verb) = meta.name.split_once('.').unwrap();
        assert_eq!((module, verb), (meta.module, meta.verb));
        for part in [module, verb] {
            assert!(
                (1..=31).contains(&part.len())
                    && part.starts_with(|c: char| c.is_ascii_lowercase())
            );
            assert!(part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        }
        assert_eq!(meta.ipc_name, meta.name.replace('.', "_"));
        assert_eq!(meta.rest_path, format!("/v1/{module}/{verb}"));
        assert_eq!(meta.since, ServiceApiVersion::CURRENT);
        assert!(std::ptr::eq(command(meta.name).unwrap(), meta));
    }
}

#[test]
fn budgets_follow_the_notes() {
    for name in [
        "search.query",
        "files.search",
        "catalogue.search",
        "catalogue.facets",
    ] {
        assert_eq!(command(name).unwrap().budget, Budget::Search, "{name}");
    }
    assert_eq!(command("files.download").unwrap().budget, Budget::Transfer);
    for name in [
        "setup.run_guided",
        "setup.sign_in_harness",
        "identity.sign_in_wait",
        // SVC-R1-8: the commands that open the operating system's own dialog.
        "app.pick_folder",
        "files.save_export",
        "identity.import_client_certificate",
        // SVC-R1-10: the two other commands that open the save dialog.
        "files.save_copy",
        "diagnostics.support_export",
        // SVC-DBX-1: a dashboard saved, or added to a presentation the person picks.
        "dashboards.export",
        "dashboards.add_to_presentation",
        // SVC-KEX-1: a knowledge page saved as a PDF or Word document.
        "knowledge.export",
    ] {
        assert_eq!(
            command(name).unwrap().budget,
            Budget::WaitsForPerson,
            "{name}"
        );
    }
    let others = CATALOGUE
        .iter()
        .filter(|m| m.budget == Budget::Interactive)
        .count();
    // 422 after SVC-R1-10 moved two rows and SVC-LOG-1 added diagnostics.clear_logs; 423 with
    // app.open_web_page (SVC-WEB-1); 426 with identity.save_github_token (SVC-GH-PAT-1); 427 with agents.installed_file (SM-NS-11); 429 with
    // app.agent_settings and app.change_agent_setting (SVC-AGM-1); 433 with the four SSH key commands
    // (SVC-SSH-1); 435 with confluence.connect_token and confluence.connect_with_jira (SVC-CONF-TOKEN-1).
    // 453 with the eighteen interactive minutes rows (SVC-MIN-1). One more with setup.choose_harness
    // (SVC-CHO-1), and two with app.install_place and app.move_to_applications (SVC-MOVE-1). One fewer
    // without ordin.prepare_start_fresh (SVC-NKP-1).
    assert_eq!(others, if cfg!(feature = "minutes") { 455 } else { 437 });
}

#[test]
fn groups_by_build_switch_cover_the_catalogue() {
    let modules: BTreeSet<&str> = CATALOGUE.iter().map(|m| m.module).collect();
    let mut table: Vec<&str> = ALWAYS_ANSWERED_GROUPS.to_vec();
    table.extend(GROUPS_BY_SWITCH.iter().map(|(_, group)| *group));
    assert_eq!(ALWAYS_ANSWERED_GROUPS.len(), 39);
    assert_eq!(GROUPS_BY_SWITCH.len(), 10);
    let mut table: BTreeSet<&str> = table.into_iter().collect();
    // 49 names (SVC-MIN-3); a build without `minutes` has no row of that group.
    assert_eq!(table.len(), 49);
    if !cfg!(feature = "minutes") {
        table.remove("minutes");
    }
    assert_eq!(table, modules);
}

#[test]
fn person_only_commands_are_catalogue_commands() {
    // 26 with diagnostics.clear_logs (SVC-LOG-1); 27 with app.open_web_page (SVC-WEB-1); 30 with
    // app.create_ssh_key and app.choose_ssh_key (SVC-SSH-1); 32 with confluence.connect_token and
    // confluence.connect_with_jira (SVC-CONF-TOKEN-1).
    // 37 with the five minutes commands (SVC-MIN-2); 39 with minutes.delete_recording and
    // minutes.remove_model (SVC-MIN-2 amended 2026-11-21). One more with setup.choose_harness
    // (SVC-CHO-1), and one more with app.move_to_applications (SVC-MOVE-1).
    assert_eq!(
        PERSON_ONLY_COMMANDS.len(),
        if cfg!(feature = "minutes") { 41 } else { 34 }
    );
    for name in PERSON_ONLY_COMMANDS {
        let meta = command(name).unwrap_or_else(|| panic!("{name} in the catalogue"));
        assert_eq!(meta.effect, Effect::Change, "{name}");
    }
}

#[test]
fn schema_lists_every_command_and_event() {
    let schema = json_schemas();
    assert_eq!(schema["x-ordin-commands"].as_array().unwrap().len(), ROWS);
    // 22, and `ordin_turn_changed` (SVC-WK-1).
    assert_eq!(schema["x-ordin-events"].as_array().unwrap().len(), 24);
    for name in [
        "Block",
        "ServiceEvent",
        "PairRequest",
        "PairReply",
        "ServiceError",
    ] {
        assert!(schema["$defs"].get(name).is_some(), "{name}");
    }
}

/// The window keeps its own list of the commands it never times out (`WAITS_FOR_PERSON` in
/// `apps/desktop/ui/src/lib/backend/backend.ts`); it must name exactly the `WaitsForPerson` rows, or the
/// window gives up on a dialog the backend still waits for (plan 04: the folder picker, SVC-R1-8).
// spec: ordin-desktop/components/ServiceApi#SVC-R1-8
#[test]
fn the_window_never_times_out_exactly_the_commands_that_wait_for_the_person() {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop/ui/src/lib/backend/backend.ts"
    );
    let text = std::fs::read_to_string(file).expect("backend.ts");
    let start = text.find("WAITS_FOR_PERSON").expect("WAITS_FOR_PERSON");
    let list = &text[start..start + text[start..].find("]);").expect("end of the list")];
    // The window names the two `minutes` rows in every build; only a build with the switch has them.
    let mut window: Vec<&str> = list
        .split('\'')
        .skip(1)
        .step_by(2)
        .filter(|name| cfg!(feature = "minutes") || !name.starts_with("minutes."))
        .collect();
    window.sort_unstable();
    let mut rows: Vec<&str> = CATALOGUE
        .iter()
        .filter(|m| m.budget == Budget::WaitsForPerson)
        .map(|m| m.name)
        .collect();
    rows.sort_unstable();
    assert_eq!(window, rows);
}
