//! The integration test of `ArchitectureTest.md`: runs `cargo metadata` over the workspace and holds
//! its crate graph to Phase 2B-3, then scans the sources for the person-only commands.

/// `PERSON_ONLY_COMMANDS` as exported by `ordin-service-api` (ServiceApi.md, amendments January 2026),
/// copied in full by ArchitectureTest.md (*Shared Context*, amendment January 2026: 15 names), and the
/// three ServiceApi.md SVC-TW-2 adds (ADR-121), which the check of that list covers. This crate has no
/// workspace dependency, so it holds its own copy.
const PERSON_ONLY_COMMANDS: &[&str] = &[
    "team_knowledge.adopt",
    "team_knowledge.adopt_again",
    "team_knowledge.keep_as_is",
    "memory.correct",
    "memory.forget",
    "memory.remember_everywhere",
    "memory.keep",
    "memory.correct_link",
    "memory.forget_link",
    "memory.forget_thing",
    "memory.same_as",
    "knowledge_agents.add",
    "knowledge_agents.remove",
    "setup.update_server",
    "setup.install_update",
    // spec: ordin-desktop/components/ServiceApi#SVC-TW-2
    "team_knowledge.open_workspace",
    "team_knowledge.allow_grant",
    "team_knowledge.decline_grant",
    // spec: ordin-desktop/components/ServiceApi#SVC-LOG-1
    "diagnostics.clear_logs",
    // spec: ordin-desktop/components/ServiceApi#SVC-WEB-1
    "app.open_web_page",
    // spec: ordin-desktop/components/ServiceApi#SVC-MIN-2
    "minutes.record",
    "minutes.give_model_consent",
    "minutes.accept_notice",
    "minutes.save_obs_password",
    "minutes.remove",
    "minutes.delete_recording",
    "minutes.remove_model",
];

fn workspace_manifest() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml")
}

#[test]
fn crate_graph_matches_phase_2b3() {
    ordin_arch_test::check_table(ordin_arch_test::ALLOWED).expect("allowed table is consistent");
    let metadata = ordin_arch_test::read_metadata(&workspace_manifest()).expect("cargo metadata");
    let report = ordin_arch_test::check(&metadata, ordin_arch_test::ALLOWED);
    for name in &report.not_yet_built {
        eprintln!("not yet built: {name}");
    }
    assert!(
        report.is_ok(),
        "{}",
        report
            .violations
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn person_only_commands_not_reachable_by_agents() {
    let metadata = ordin_arch_test::read_metadata(&workspace_manifest()).expect("cargo metadata");
    let report = ordin_arch_test::scan::check_person_only(&metadata, PERSON_ONLY_COMMANDS);
    assert!(
        report.is_ok(),
        "{}",
        report
            .violations
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
