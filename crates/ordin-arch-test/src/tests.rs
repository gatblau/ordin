//! Unit tests: one per Error Table row and one per Gherkin scenario of `ArchitectureTest.md`, over
//! hand-written `Metadata` values (no `cargo` call except the one that provokes `ARCH_METADATA_FAILED`).

use super::*;
use crate::scan::{check_sources, tokenise, SourceFile, Unreadable};

/// A hand-written graph: `members` and `embedded` are path packages, `registry` crates have a source.
/// Each edge is `(from, to, kind)` with kind `None` for normal, `Some("dev")` or `Some("build")`.
fn graph(members: &[&str], embedded: &[&str], edges: &[(&str, &str, Option<&str>)]) -> Metadata {
    let mut packages: Vec<Package> = Vec::new();
    for name in members.iter().chain(embedded) {
        packages.push(Package {
            id: (*name).to_string(),
            name: (*name).to_string(),
            source: None,
            manifest_path: format!("/w/{name}/Cargo.toml"),
        });
    }
    let mut nodes: Vec<Node> = packages
        .iter()
        .map(|p| Node {
            id: p.id.clone(),
            deps: Vec::new(),
        })
        .collect();
    for (from, to, kind) in edges {
        let node = nodes
            .iter_mut()
            .find(|n| n.id == *from)
            .expect("edge source exists");
        node.deps.push(NodeDep {
            pkg: (*to).to_string(),
            dep_kinds: vec![DepKindInfo {
                kind: kind.map(str::to_string),
            }],
        });
    }
    Metadata {
        workspace_members: members.iter().map(|m| (*m).to_string()).collect(),
        packages,
        resolve: Some(Resolve { nodes }),
    }
}

fn lines(report: &Report) -> Vec<String> {
    report.violations.iter().map(|v| v.to_string()).collect()
}

/// Every row of the table as a member, with no edges.
fn all_members() -> Vec<&'static str> {
    ALLOWED.iter().map(|r| r.name).collect()
}

// ---- the table ----

#[test]
fn check_table_accepts_the_allowed_table() {
    check_table(ALLOWED).expect("the compiled-in table is consistent");
}

#[test]
fn check_table_every_allowed_edge_points_to_a_lower_phase() {
    for row in ALLOWED {
        if let AllowedDeps::List(deps) = row.deps {
            for dep in deps {
                let target = ALLOWED
                    .iter()
                    .find(|r| r.name == *dep)
                    .expect("dependency is a row");
                assert!(target.phase < row.phase, "{} -> {dep}", row.name);
            }
        }
    }
}

#[test]
fn table_rows_per_phase_match_phase_2b3() {
    // Phase 2B-3 as amended up to ADR-114: `ordin-jira` and `ordin-knowledge-query-client` in phase 2,
    // `ordin-knowledge-query-server` in 6, `ordin-slack-service` in 7, `ordin-team-knowledge` and
    // `ordin-knowledge-reader` in 8. `ordin-hook` is in phase 2; `ordin-server`, `ordin-desktop` and
    // `ordin-arch-test` are in phase 12.
    let expected: [usize; 13] = [1, 8, 18, 9, 5, 8, 5, 5, 6, 5, 3, 1, 3];
    for (phase, count) in expected.iter().enumerate() {
        let actual = ALLOWED
            .iter()
            .filter(|r| usize::from(r.phase) == phase)
            .count();
        assert_eq!(actual, *count, "rows in phase {phase}");
    }
    assert_eq!(ALLOWED.len(), 77);
}

#[test]
fn error_path_the_table_itself_breaks_the_phase_order() {
    let table = [
        AllowedCrate {
            name: "ordin-types",
            phase: 0,
            programs: Programs {
                desktop: true,
                personal: true,
                agent: true,
                build_only: false,
            },
            deps: AllowedDeps::List(&[]),
        },
        AllowedCrate {
            name: "ordin-rules",
            phase: 3,
            programs: Programs {
                desktop: true,
                personal: true,
                agent: true,
                build_only: false,
            },
            deps: AllowedDeps::List(&["ordin-files"]),
        },
        AllowedCrate {
            name: "ordin-files",
            phase: 4,
            programs: Programs {
                desktop: true,
                personal: true,
                agent: true,
                build_only: false,
            },
            deps: AllowedDeps::List(&[]),
        },
    ];
    let err = check_table(&table).expect_err("phase order is broken");
    assert_eq!(err.code(), "ARCH_TABLE_INVALID");
    assert_eq!(
        err.to_string(),
        "ARCH_TABLE_INVALID: ordin-rules -> ordin-files: phase 4 is not below 3"
    );
}

#[test]
fn error_table_arch_table_invalid_duplicate_row() {
    let row = ALLOWED[0];
    let err = check_table(&[row, row]).expect_err("duplicate row");
    assert_eq!(
        err.to_string(),
        "ARCH_TABLE_INVALID: duplicate row ordin-types"
    );
}

#[test]
fn error_table_arch_table_invalid_every_module_rows_on_the_wrong_crate() {
    let mut table = ALLOWED.to_vec();
    let index = table
        .iter()
        .position(|r| r.name == "ordin-rules")
        .expect("row exists");
    table[index].deps = AllowedDeps::EveryDesktopModule;
    assert_eq!(
        check_table(&table).expect_err("wrong crate").code(),
        "ARCH_TABLE_INVALID"
    );
    table[index].deps = AllowedDeps::EveryServerModule;
    assert_eq!(
        check_table(&table).expect_err("wrong crate").code(),
        "ARCH_TABLE_INVALID"
    );
}

// ---- Gherkin: the crate graph ----

#[test]
fn happy_path_the_graph_matches_2b3() {
    let members = all_members();
    let report = check(
        &graph(&members, &[], &[("ordin-approval", "ordin-journal", None)]),
        ALLOWED,
    );
    assert!(report.is_ok(), "{:?}", lines(&report));
    assert!(report.not_yet_built.is_empty());
}

#[test]
fn edge_case_crates_of_later_phases_not_yet_built() {
    let members: Vec<&str> = ALLOWED
        .iter()
        .filter(|r| r.phase <= 3)
        .map(|r| r.name)
        .collect();
    let report = check(&graph(&members, &[], &[]), ALLOWED);
    assert!(report.is_ok());
    for row in ALLOWED.iter().filter(|r| r.phase >= 4) {
        assert!(report.not_yet_built.contains(&row.name), "{}", row.name);
    }
    assert!(!report.not_yet_built.contains(&"ordin-rules"));
}

#[test]
fn edge_case_the_shared_types_crate_from_a_high_phase() {
    let members = all_members();
    let mut edges = vec![("ordin-telegram-service", "ordin-types", None)];
    for dep in [
        "ordin-telegram-adapter",
        "ordin-mcp-front",
        "ordin-rules",
        "ordin-approval",
        "ordin-files",
        "ordin-workspaces",
        "ordin-organisation-profile",
        "ordin-journal",
    ] {
        edges.push(("ordin-telegram-service", dep, None));
    }
    assert!(check(&graph(&members, &[], &edges), ALLOWED).is_ok());
}

#[test]
fn edge_case_a_dev_dependency_on_a_lower_phase() {
    let members = all_members();
    let edges = [
        ("ordin-rules", "ordin-journal", Some("dev")),
        ("ordin-rules", "ordin-approval", Some("dev")),
    ];
    assert!(check(&graph(&members, &[], &edges), ALLOWED).is_ok());
}

#[test]
fn error_path_an_edge_not_listed() {
    let members = all_members();
    let report = check(
        &graph(
            &members,
            &[],
            &[("ordin-approval", "ordin-command-bridge", None)],
        ),
        ALLOWED,
    );
    assert!(lines(&report).contains(
        &"ARCH_EDGE_NOT_ALLOWED ordin-approval -> ordin-command-bridge (normal)".to_string()
    ));
    assert!(!report.is_ok());
}

#[test]
fn error_table_arch_edge_not_allowed_build_edge() {
    let members = all_members();
    let report = check(
        &graph(
            &members,
            &[],
            &[("ordin-approval", "ordin-command-bridge", Some("build"))],
        ),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_EDGE_NOT_ALLOWED ordin-approval -> ordin-command-bridge (build)"]
    );
}

#[test]
fn error_path_a_cycle() {
    let members = all_members();
    let edges = [
        ("ordin-files", "ordin-workspaces", None),
        ("ordin-workspaces", "ordin-files", None),
    ];
    let report = check(&graph(&members, &[], &edges), ALLOWED);
    let got = lines(&report);
    assert!(
        got.contains(&"ARCH_CYCLE ordin-files -> ordin-workspaces -> ordin-files".to_string()),
        "{got:?}"
    );
    assert!(
        got.contains(&"ARCH_EDGE_NOT_ALLOWED ordin-files -> ordin-workspaces (normal)".to_string()),
        "{got:?}"
    );
}

#[test]
fn error_table_arch_cycle_three_members_and_self_edge() {
    let members = all_members();
    let edges = [
        ("ordin-checker", "ordin-resolver", None),
        ("ordin-resolver", "ordin-catalogue", None),
        ("ordin-catalogue", "ordin-checker", None),
        ("ordin-journal", "ordin-journal", None),
    ];
    let got = lines(&check(&graph(&members, &[], &edges), ALLOWED));
    assert!(
        got.contains(
            &"ARCH_CYCLE ordin-catalogue -> ordin-checker -> ordin-resolver -> ordin-catalogue"
                .to_string()
        ),
        "{got:?}"
    );
    assert!(
        got.contains(&"ARCH_CYCLE ordin-journal -> ordin-journal".to_string()),
        "{got:?}"
    );
}

#[test]
fn error_path_a_dev_dependency_on_a_higher_phase() {
    let members = all_members();
    let report = check(
        &graph(
            &members,
            &[],
            &[("ordin-journal", "ordin-rules", Some("dev"))],
        ),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_DEV_EDGE_NOT_ALLOWED ordin-journal -> ordin-rules"]
    );
}

#[test]
fn error_path_an_unknown_crate() {
    let mut members = all_members();
    members.push("ordin-reports");
    let report = check(&graph(&members, &[], &[]), ALLOWED);
    assert_eq!(lines(&report), ["ARCH_UNKNOWN_CRATE ordin-reports"]);
}

#[test]
fn error_path_the_memory_component_used_outside_its_wrapper() {
    let members = all_members();
    let report = check(
        &graph(
            &members,
            &["memory"],
            &[("ordin-knowledge", "memory", None)],
        ),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_EMBEDDED_EDGE ordin-knowledge -> memory"]
    );
}

#[test]
fn edge_case_the_embedded_package_used_by_its_wrapper() {
    let members = all_members();
    let edges = [
        ("ordin-memory-service", "memory", None),
        ("ordin-sandbox", "sandboxd", None),
    ];
    let meta = graph(&members, &["memory", "sandboxd", "sandboxd-ipc"], &edges);
    assert!(check(&meta, ALLOWED).is_ok());
}

#[test]
fn error_table_arch_unknown_path_package() {
    let members = all_members();
    let report = check(&graph(&members, &["scratch-tool"], &[]), ALLOWED);
    assert_eq!(lines(&report), ["ARCH_UNKNOWN_PATH_PACKAGE scratch-tool"]);
}

#[test]
fn error_path_a_program_depends_on_a_module_of_another_program() {
    let members = all_members();
    let report = check(
        &graph(&members, &[], &[("ordin-desktop", "ordin-run-queue", None)]),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_EDGE_NOT_ALLOWED ordin-desktop -> ordin-run-queue (normal)"]
    );
}

#[test]
fn edge_case_a_program_depends_on_a_module_of_its_program() {
    let members = all_members();
    let edges = [
        ("ordin-desktop", "ordin-mail", None),
        ("ordin-server", "ordin-caller-api", None),
    ];
    assert!(check(&graph(&members, &[], &edges), ALLOWED).is_ok());
}

#[test]
fn error_path_cargo_metadata_fails() {
    let missing = std::env::temp_dir()
        .join("ordin-arch-test-no-such-workspace")
        .join("Cargo.toml");
    let err = read_metadata(&missing).expect_err("no manifest there");
    assert_eq!(err.code(), "ARCH_METADATA_FAILED");
    assert!(
        err.to_string()
            .starts_with("ARCH_METADATA_FAILED: cargo metadata exited with "),
        "{err}"
    );
}

#[test]
fn error_table_arch_metadata_invalid() {
    let err = parse_metadata(b"{}").expect_err("fields are missing");
    assert_eq!(err.code(), "ARCH_METADATA_INVALID");
    let err = parse_metadata(br#"{"packages":[],"workspace_members":[],"resolve":null}"#)
        .expect_err("resolve is null");
    assert_eq!(err.code(), "ARCH_METADATA_INVALID");
    let ok = parse_metadata(
        br#"{"packages":[],"workspace_members":[],"resolve":{"nodes":[]},"ignored":1}"#,
    );
    assert!(ok.is_ok(), "fields outside the subset are ignored");
}

// ---- Gherkin: amendments (ADR-102, ADR-106, ADR-113, ADR-114) ----

#[test]
fn happy_path_the_slack_services_edges_are_allowed() {
    let members = all_members();
    let mut edges: Vec<(&str, &str, Option<&str>)> = [
        "ordin-identity",
        "ordin-approval",
        "ordin-mcp-front",
        "ordin-workspaces",
        "ordin-organisation-profile",
        "ordin-journal",
    ]
    .iter()
    .map(|d| ("ordin-slack-service", *d, None))
    .collect();
    edges.push(("ordin-handover", "ordin-slack-service", None));
    edges.push(("ordin-triggers", "ordin-slack-service", None));
    assert!(check(&graph(&members, &[], &edges), ALLOWED).is_ok());
}

#[test]
fn error_path_the_ordin_host_may_not_depend_on_the_slack_service() {
    let members = all_members();
    let report = check(
        &graph(
            &members,
            &[],
            &[("ordin-host", "ordin-slack-service", None)],
        ),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_EDGE_NOT_ALLOWED ordin-host -> ordin-slack-service (normal)"]
    );
}

#[test]
fn happy_path_the_jira_adapters_edges_are_allowed() {
    let members = all_members();
    let edges = [
        ("ordin-jira", "ordin-identity", None),
        ("ordin-jira", "ordin-journal", None),
        ("ordin-workspaces", "ordin-jira", None),
        ("ordin-handover", "ordin-jira", None),
    ];
    assert!(check(&graph(&members, &[], &edges), ALLOWED).is_ok());
}

#[test]
fn error_path_the_server_reaching_the_jira_adapter() {
    let members = all_members();
    let report = check(
        &graph(&members, &[], &[("ordin-server", "ordin-jira", None)]),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_EDGE_NOT_ALLOWED ordin-server -> ordin-jira (normal)"]
    );
}

#[test]
fn happy_path_the_knowledge_crates_edges_are_allowed() {
    let members = all_members();
    let edges = [
        ("ordin-memory-service", "ordin-knowledge-query-client", None),
        ("ordin-caller-api", "ordin-knowledge-query-server", None),
        ("ordin-knowledge-reader", "ordin-knowledge", None),
        ("ordin-knowledge-reader", "ordin-memory-service", None),
    ];
    assert!(check(&graph(&members, &[], &edges), ALLOWED).is_ok());
}

#[test]
fn error_path_the_query_server_reaching_the_reader() {
    let members = all_members();
    let report = check(
        &graph(
            &members,
            &[],
            &[(
                "ordin-knowledge-query-server",
                "ordin-knowledge-reader",
                None,
            )],
        ),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_EDGE_NOT_ALLOWED ordin-knowledge-query-server -> ordin-knowledge-reader (normal)"]
    );
}

#[test]
fn error_path_the_desktop_building_the_query_server() {
    let members = all_members();
    let report = check(
        &graph(
            &members,
            &[],
            &[("ordin-desktop", "ordin-knowledge-query-server", None)],
        ),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_EDGE_NOT_ALLOWED ordin-desktop -> ordin-knowledge-query-server (normal)"]
    );
}

#[test]
fn error_path_a_knowledge_crate_reaching_the_memory_component_directly() {
    let members = all_members();
    let report = check(
        &graph(
            &members,
            &["memory"],
            &[("ordin-knowledge-reader", "memory", None)],
        ),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_EMBEDDED_EDGE ordin-knowledge-reader -> memory"]
    );
}

#[test]
fn happy_path_an_optional_program_dependency_is_still_checked() {
    // `cargo metadata --all-features` reports an optional dependency as an ordinary edge.
    let members = all_members();
    let report = check(
        &graph(&members, &[], &[("ordin-desktop", "ordin-mail", None)]),
        ALLOWED,
    );
    assert!(report.is_ok());
}

#[test]
fn error_path_an_optional_edge_outside_the_programs_set() {
    let members = all_members();
    let report = check(
        &graph(&members, &[], &[("ordin-desktop", "ordin-run-queue", None)]),
        ALLOWED,
    );
    assert_eq!(
        lines(&report),
        ["ARCH_EDGE_NOT_ALLOWED ordin-desktop -> ordin-run-queue (normal)"]
    );
}

#[test]
fn the_report_is_sorted_and_free_of_duplicates() {
    let members = all_members();
    let edges = [
        ("ordin-approval", "ordin-command-bridge", None),
        ("ordin-approval", "ordin-command-bridge", None),
    ];
    let report = check(&graph(&members, &["zzz", "aaa"], &edges), ALLOWED);
    assert_eq!(
        lines(&report),
        [
            "ARCH_EDGE_NOT_ALLOWED ordin-approval -> ordin-command-bridge (normal)",
            "ARCH_UNKNOWN_PATH_PACKAGE aaa",
            "ARCH_UNKNOWN_PATH_PACKAGE zzz",
        ]
    );
}

#[test]
fn every_code_is_listed_in_all_codes() {
    let violations = [
        Violation::UnknownCrate {
            name: String::new(),
        },
        Violation::EdgeNotAllowed {
            from: String::new(),
            to: String::new(),
            kind: EdgeKind::Normal,
        },
        Violation::DevEdgeNotAllowed {
            from: String::new(),
            to: String::new(),
        },
        Violation::Cycle { members: vec![] },
        Violation::EmbeddedEdge {
            from: String::new(),
            to: String::new(),
        },
        Violation::UnknownPathPackage {
            name: String::new(),
        },
        Violation::PersonOnlyNamed {
            krate: String::new(),
            file: String::new(),
            line: 0,
            command: String::new(),
        },
        Violation::PersonOnlyCalled {
            krate: String::new(),
            file: String::new(),
            line: 0,
            method: String::new(),
        },
        Violation::SourceUnreadable {
            file: String::new(),
        },
    ];
    for v in &violations {
        assert!(ALL_CODES.contains(&v.code()), "{}", v.code());
    }
    for code in [
        "ARCH_METADATA_FAILED",
        "ARCH_METADATA_INVALID",
        "ARCH_TABLE_INVALID",
    ] {
        assert!(ALL_CODES.contains(&code));
    }
    assert_eq!(ALL_CODES.len(), 12);
    assert!(ALL_CODES.iter().all(|c| c.starts_with("ARCH_")));
}

// ---- Gherkin: the person-only scan (ADR-110, ADR-111) ----

const COMMANDS: &[&str] = &[
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
];

fn file(krate: &str, relative: &str, text: &str) -> SourceFile {
    SourceFile {
        krate: krate.to_string(),
        relative: relative.to_string(),
        text: Ok(text.to_string()),
    }
}

fn scan(files: &[SourceFile], dependents: &[&str]) -> Vec<String> {
    let set = dependents.iter().map(|d| (*d).to_string()).collect();
    lines(&check_sources(files, COMMANDS, &set))
}

#[test]
fn happy_path_only_the_catalogue_and_dispatch_name_the_adopt_commands() {
    let files = [
        file(
            "ordin-service-api",
            "src/catalogue.rs",
            r#"const A: &str = "team_knowledge.adopt";"#,
        ),
        file(
            "ordin-desktop",
            "src/service_api.rs",
            r#"let _ = "team_knowledge.adopt";"#,
        ),
        file(
            "ordin-desktop",
            "src/service_api/dispatch.rs",
            r#"let _ = "memory.same_as";"#,
        ),
    ];
    assert!(scan(&files, &[]).is_empty());
}

#[test]
fn error_path_a_tool_names_an_adopt_command() {
    let files = [file(
        "ordin-host",
        "src/tools.rs",
        "fn handler() {\n    send(\"team_knowledge.adopt\");\n}\n",
    )];
    assert_eq!(
        scan(&files, &[]),
        ["ARCH_PERSON_ONLY_NAMED ordin-host ordin-host/src/tools.rs:2 names team_knowledge.adopt, which only a person may send"]
    );
}

#[test]
fn error_path_a_workflow_step_calls_adopt_directly() {
    let files = [file(
        "ordin-run-queue",
        "src/step.rs",
        "fn run(t: &T) {\n    t.adopt_again(1);\n}\n",
    )];
    assert_eq!(
        scan(&files, &["ordin-run-queue"]),
        ["ARCH_PERSON_ONLY_CALLED ordin-run-queue ordin-run-queue/src/step.rs:2 calls adopt_again, which only a person may start"]
    );
}

#[test]
fn edge_a_comment_is_not_a_violation() {
    let files = [file(
        "ordin-mcp-front",
        "src/lib.rs",
        "/// Never send team_knowledge.adopt \"team_knowledge.adopt\" from here.\n// t.adopt(1)\n/* nested /* \"memory.forget\" */ still a comment */\nfn f() {}\n",
    )];
    assert!(scan(&files, &["ordin-mcp-front"]).is_empty());
}

#[test]
fn error_path_a_tool_names_a_person_only_memory_command() {
    let files = [file(
        "ordin-host",
        "src/tools.rs",
        "fn h() { send(\"memory.forget_thing\"); }\n",
    )];
    assert_eq!(
        scan(&files, &[]),
        ["ARCH_PERSON_ONLY_NAMED ordin-host ordin-host/src/tools.rs:1 names memory.forget_thing, which only a person may send"]
    );
}

#[test]
fn error_path_a_workflow_step_names_a_person_only_update_command() {
    let files = [file(
        "ordin-run-queue",
        "src/step.rs",
        "fn h() { send(\"setup.install_update\"); }\n",
    )];
    assert_eq!(
        scan(&files, &[]),
        ["ARCH_PERSON_ONLY_NAMED ordin-run-queue ordin-run-queue/src/step.rs:1 names setup.install_update, which only a person may send"]
    );
}

#[test]
fn happy_path_the_dispatch_module_may_name_the_memory_commands() {
    let files = [
        file(
            "ordin-service-api",
            "src/lib.rs",
            r#"const X: &str = "memory.same_as";"#,
        ),
        file(
            "ordin-desktop",
            "src/service_api.rs",
            r#"let _ = "memory.same_as";"#,
        ),
    ];
    assert!(scan(&files, &[]).is_empty());
}

#[test]
fn edge_a_read_command_is_not_person_only() {
    let files = [file(
        "ordin-host",
        "src/tools.rs",
        "fn h() { send(\"memory.used\"); }\n",
    )];
    assert!(scan(&files, &[]).is_empty());
}

#[test]
fn edge_the_team_knowledge_crate_and_non_dependents_may_use_the_method_names() {
    let files = [
        file(
            "ordin-team-knowledge",
            "src/lib.rs",
            "fn f(t: &T) { t.adopt(1); }\n",
        ),
        file(
            "ordin-journal",
            "src/lib.rs",
            "fn f(t: &T) { t.adopt(1); }\n",
        ),
        file(
            "ordin-desktop",
            "src/service_api.rs",
            "fn f(t: &T) { t.keep_as_is(1); }\n",
        ),
    ];
    assert!(scan(&files, &["ordin-team-knowledge", "ordin-desktop"]).is_empty());
}

#[test]
fn error_table_arch_source_unreadable() {
    let not_utf8 = SourceFile {
        krate: "ordin-host".into(),
        relative: "src/bad.rs".into(),
        text: Err(()),
    };
    assert_eq!(
        scan(&[not_utf8], &[]),
        ["ARCH_SOURCE_UNREADABLE ordin-host/src/bad.rs could not be read"]
    );
    let open_string = file(
        "ordin-host",
        "src/open.rs",
        "fn f() { let _ = \"never closed;\n",
    );
    assert_eq!(
        scan(&[open_string], &[]),
        ["ARCH_SOURCE_UNREADABLE ordin-host/src/open.rs could not be read"]
    );
    let open_comment = file("ordin-host", "src/open.rs", "/* never closed\n");
    assert_eq!(
        scan(&[open_comment], &[]),
        ["ARCH_SOURCE_UNREADABLE ordin-host/src/open.rs could not be read"]
    );
}

#[test]
fn error_table_arch_person_only_named_and_called_codes() {
    let named = Violation::PersonOnlyNamed {
        krate: "c".into(),
        file: "f".into(),
        line: 3,
        command: "x".into(),
    };
    let called = Violation::PersonOnlyCalled {
        krate: "c".into(),
        file: "f".into(),
        line: 3,
        method: "adopt".into(),
    };
    assert_eq!(named.code(), "ARCH_PERSON_ONLY_NAMED");
    assert_eq!(called.code(), "ARCH_PERSON_ONLY_CALLED");
}

// ---- the tokeniser ----

#[test]
fn tokeniser_reads_raw_strings_escapes_and_lifetimes() {
    let src = "fn f<'a>(x: &'a str) {\n    let _c = '\"';\n    let _a = r#\"team_knowledge.adopt \"quoted\"\"#;\n    let _b = \"esc \\\" memory.keep\";\n    let _d = b\"bytes\";\n    x.run (1);\n}\n";
    let tokens = tokenise(src).expect("readable");
    let strings: Vec<&str> = tokens.strings.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(
        strings,
        [
            "team_knowledge.adopt \"quoted\"",
            "esc \" memory.keep",
            "bytes"
        ]
    );
    assert_eq!(tokens.strings[0].1, 3);
    assert_eq!(tokens.method_calls, [("run".to_string(), 6)]);
}

#[test]
fn tokeniser_rejects_an_unterminated_raw_string() {
    assert_eq!(tokenise("let _ = r#\"open\";"), Err(Unreadable));
}
