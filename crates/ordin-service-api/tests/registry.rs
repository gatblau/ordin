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
    assert_eq!(CATALOGUE.len(), 400);
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
    let others = CATALOGUE
        .iter()
        .filter(|m| m.budget == Budget::Interactive)
        .count();
    assert_eq!(others, 395);
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
    assert_eq!(PERSON_ONLY_COMMANDS.len(), 15);
    for name in PERSON_ONLY_COMMANDS {
        let meta = command(name).unwrap_or_else(|| panic!("{name} in the catalogue"));
        assert_eq!(meta.effect, Effect::Change, "{name}");
    }
}

#[test]
fn schema_lists_every_command_and_event() {
    let schema = json_schemas();
    assert_eq!(schema["x-ordin-commands"].as_array().unwrap().len(), 400);
    assert_eq!(schema["x-ordin-events"].as_array().unwrap().len(), 21);
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
