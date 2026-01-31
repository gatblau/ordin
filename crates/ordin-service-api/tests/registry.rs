//! X4, and the catalogue's own invariants (Internal Logic 1; ADR-114 groups; PERSON_ONLY_COMMANDS).

use std::collections::BTreeSet;

use ordin_service_api::*;
use ordin_types::error_registry::{lookup, owns_prefix};

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
    assert_eq!(CATALOGUE.len(), 415);
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
    assert_eq!(others, 404);
}

#[test]
fn groups_by_build_switch_cover_the_catalogue() {
    let modules: BTreeSet<&str> = CATALOGUE.iter().map(|m| m.module).collect();
    let mut table: Vec<&str> = ALWAYS_ANSWERED_GROUPS.to_vec();
    table.extend(GROUPS_BY_SWITCH.iter().map(|(_, group)| *group));
    assert_eq!(ALWAYS_ANSWERED_GROUPS.len(), 39);
    assert_eq!(GROUPS_BY_SWITCH.len(), 9);
    let table: BTreeSet<&str> = table.into_iter().collect();
    assert_eq!(table.len(), 48);
    assert_eq!(table, modules);
}

#[test]
fn person_only_commands_are_catalogue_commands() {
    assert_eq!(PERSON_ONLY_COMMANDS.len(), 18);
    for name in PERSON_ONLY_COMMANDS {
        let meta = command(name).unwrap_or_else(|| panic!("{name} in the catalogue"));
        assert_eq!(meta.effect, Effect::Change, "{name}");
    }
}

#[test]
fn schema_lists_every_command_and_event() {
    let schema = json_schemas();
    assert_eq!(schema["x-ordin-commands"].as_array().unwrap().len(), 415);
    // 22, and `ordin_turn_changed` (SVC-WK-1).
    assert_eq!(schema["x-ordin-events"].as_array().unwrap().len(), 23);
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
    let mut window: Vec<&str> = list.split('\'').skip(1).step_by(2).collect();
    window.sort_unstable();
    let mut rows: Vec<&str> = CATALOGUE
        .iter()
        .filter(|m| m.budget == Budget::WaitsForPerson)
        .map(|m| m.name)
        .collect();
    rows.sort_unstable();
    assert_eq!(window, rows);
}
