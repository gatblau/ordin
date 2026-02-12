//! The `minutes` group (ServiceApi.md SVC-MIN-1 to SVC-MIN-4), one test per Gherkin scenario.
//! The build-without-the-switch scenario runs in every build without `minutes`.

use ordin_service_api::*;

// spec: ordin-desktop/components/ServiceApi#SVC-MIN-1
#[cfg(feature = "minutes")]
#[test]
fn contract_twenty_local_commands_in_a_build_with_minutes() {
    assert_eq!(CATALOGUE.len(), 475);
    let rows: Vec<_> = CATALOGUE.iter().filter(|m| m.module == "minutes").collect();
    assert_eq!(rows.len(), 20);
    for meta in &rows {
        assert_eq!(meta.scope, Scope::Local, "{}", meta.name);
    }
    for name in ["minutes.import", "minutes.export"] {
        assert_eq!(
            command(name).unwrap().budget,
            Budget::WaitsForPerson,
            "{name}"
        );
    }
    let interactive = CATALOGUE
        .iter()
        .filter(|m| m.budget == Budget::Interactive)
        .count();
    assert_eq!(interactive, 457);
}

// spec: ordin-desktop/components/ServiceApi#SVC-MIN-3
#[cfg(not(feature = "minutes"))]
#[test]
fn edge_a_build_without_the_switch() {
    assert!(command("minutes.status").is_none());
    let schema = serde_json::to_string(&json_schemas()).unwrap();
    assert!(!schema.contains("\"minutes."));
    assert!(!schema.contains("MinutesStatus"));
    assert!(!PERSON_ONLY_COMMANDS
        .iter()
        .any(|n| n.starts_with("minutes.")));
}

// spec: ordin-desktop/components/ServiceApi#SVC-MIN-3
#[cfg(feature = "minutes")]
#[test]
fn the_schema_of_a_build_with_minutes_lists_the_group_and_its_types() {
    let schema = serde_json::to_string(&json_schemas()).unwrap();
    assert!(schema.contains("\"minutes.status\""));
    assert!(schema.contains("MinutesStatus"));
}

// spec: ordin-desktop/components/ServiceApi#SVC-MIN-2
#[cfg(feature = "minutes")]
#[test]
fn contract_only_the_person_records() {
    for name in [
        "minutes.record",
        "minutes.give_model_consent",
        "minutes.accept_notice",
        "minutes.save_obs_password",
        "minutes.remove",
        // SVC-MIN-2 amended 2026-11-21.
        "minutes.delete_recording",
        "minutes.remove_model",
    ] {
        assert!(PERSON_ONLY_COMMANDS.contains(&name), "{name}");
    }
}

// spec: ordin-desktop/components/ServiceApi#SVC-MIN-4
#[test]
fn the_obs_download_page_is_named() {
    let page: commands::WebPage = serde_json::from_str("\"obs_download\"").unwrap();
    assert_eq!(page, commands::WebPage::ObsDownload);
}
