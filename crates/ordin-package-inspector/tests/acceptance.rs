//! X2: one test per Gherkin scenario of PackageInspector.md, named after the scenario.

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use futures::StreamExt;
use ordin_agent_model::PartKind;
use ordin_package_inspector::*;
use ordin_rules::InstanceRuleSet;
use ordin_types::domain::agents::Authority;
use ordin_types::{AgentInstanceId, WorkspaceId};
use support::*;

fn rules_of(f: &Fixture, id: AgentInstanceId) -> InstanceRuleSet {
    serde_json::from_slice(&std::fs::read(f.rules_file(id)).expect("rules file")).expect("rule set")
}

// Scenario: Happy path - an installed ordin agent
#[tokio::test]
async fn happy_path_an_installed_ordin_agent() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    let id = fixed_id();
    let outcome = f
        .inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    assert_eq!(outcome.model.tasks.len(), 10);
    assert_eq!(outcome.model.authority, Authority::DocumentOnly);
    assert_eq!(outcome.model.authority_source, AuthoritySource::Derived);
    assert!(outcome.model.tools.is_empty() && outcome.model.connections.is_empty());
    assert!(outcome
        .parsed_declaration_ref
        .starts_with("pki:0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff:"));
    // The reference's digest is the digest of the stored file.
    let bytes = std::fs::read(f.model_file(id)).expect("model file");
    assert_eq!(
        outcome.parsed_declaration_ref,
        format!("pki:{id}:{}", sha256(&bytes))
    );
    assert_eq!(f.inspector.model(id).await.expect("model"), outcome.model);
    assert_eq!(outcome.model.display_name, "Software Engineer (CSPA-431)");
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

// Scenario: Happy path - what it's made of for a business user
#[tokio::test]
async fn happy_path_what_its_made_of_for_a_business_user() {
    let f = Fixture::new().await;
    let mut parts = vec![PartDef::markdown_part(
        PartKind::SystemRole,
        "system-roles",
        "analyst",
        "Analyst",
        "The role.",
    )];
    for n in 0..3 {
        parts.push(PartDef::markdown_part(
            PartKind::Rule,
            "rules",
            &format!("rule-{n}"),
            &format!("Rule {n}"),
            "A rule.",
        ));
    }
    for n in 0..4 {
        parts.push(PartDef::markdown_part(
            PartKind::Skill,
            "skills",
            &format!("skill-{n}"),
            &format!("Skill {n}"),
            "An ability.",
        ));
    }
    for n in 0..3 {
        parts.push(PartDef::command(
            &format!("task-{n}"),
            &format!("Task {n}"),
            None,
        ));
    }
    let installed = install(&f.dir.0, "a", &parts, no_policy());
    let id = AgentInstanceId::new();
    f.inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    let made = f
        .inspector
        .made_of(id, Detail::BusinessUser)
        .await
        .expect("made_of");
    let ids: Vec<&str> = made
        .groups
        .iter()
        .map(|g| g.plain_kind_message_id)
        .collect();
    assert_eq!(
        ids,
        [
            "part-kind-role",
            "part-kind-rule",
            "part-kind-ability",
            "part-kind-task"
        ]
    );
    assert_eq!(
        made.groups.iter().map(|g| g.count).collect::<Vec<_>>(),
        [1, 3, 4, 3]
    );
    for line in made.groups.iter().flat_map(|g| &g.parts) {
        assert!(line.part.id.is_empty());
        assert!(line.part.version.is_empty());
        assert!(line.part.content_hash.is_empty());
        assert!(line.part.repository.is_empty());
        assert!(line.part.commit.is_empty());
        assert!(!line.part.name.is_empty());
    }
    assert!(made.pinned_by.is_none());
    assert_eq!(made.part_count, 11);
    assert_eq!(made.tasks.len(), 3);
}

// Scenario: Edge - a borrowed part says where it comes from
#[tokio::test]
async fn edge_a_borrowed_part_says_where_it_comes_from() {
    let f = Fixture::new().await;
    let parts = vec![
        PartDef::markdown_part(PartKind::Rule, "rules", "own", "Own rule", "Own."),
        PartDef::markdown_part(
            PartKind::Rule,
            "rules",
            "borrowed",
            "Borrowed rule",
            "Borrowed.",
        )
        .in_repository("github.com/acme/company-agents"),
    ];
    let installed = install(&f.dir.0, "a", &parts, no_policy());
    let mut request = installed.request(AgentInstanceId::new());
    request.repository_names = BTreeMap::from([
        ("bundled".to_owned(), "ordin".to_owned()),
        (
            "github.com/acme/company-agents".to_owned(),
            "Company agents".to_owned(),
        ),
    ]);
    let outcome = f.inspector.inspect(&request).await.expect("inspect");
    let line = |name: &str| {
        outcome
            .model
            .parts
            .iter()
            .find(|l| l.name == name)
            .expect("line")
    };
    assert_eq!(
        line("Borrowed rule").from.as_deref(),
        Some("Company agents")
    );
    assert_eq!(line("Own rule").from, None);
}

// Scenario: Edge - a declaration's approval list reaches the Rules Engine
#[tokio::test]
async fn edge_a_declarations_approval_list_reaches_the_rules_engine() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    installed
        .declaration("format-version: 1\napproval-required:\n  - email_send\n  - email_send\n");
    let id = AgentInstanceId::new();
    let outcome = f
        .inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    assert_eq!(rules_of(&f, id).approval_list, ["email_send"]);
    assert_eq!(outcome.model.authority, Authority::SystemChanging);
    assert_eq!(outcome.model.authority_source, AuthoritySource::Derived);
    assert!(outcome.model.declaration_sha256.is_some());
    assert!(loaded(&f.engine, id).await);
}

// Scenario: Edge - argument places from a tool binding
#[tokio::test]
async fn edge_argument_places_from_a_tool_binding() {
    let f = Fixture::new().await;
    let body = "bindings:\n  - name: email_send\n    type: mcp\n    permission: requires-approval\n    argumentPaths:\n      - \"/to/[]/address\"\n      - \"/attachments/*\"\n";
    let parts = vec![PartDef::tool_binding("email", body)];
    let installed = install(&f.dir.0, "a", &parts, no_policy());
    let id = AgentInstanceId::new();
    let outcome = f
        .inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    let set = rules_of(&f, id);
    let tool = set
        .tools
        .iter()
        .find(|t| t.name == "email_send")
        .expect("tool");
    assert_eq!(tool.arguments, ["to[].address", "attachments.*"]);
    assert_eq!(outcome.model.tools[0].kind, ToolKind::McpServer);
    assert_eq!(outcome.model.authority, Authority::SystemChanging);
}

// Scenario: Edge - a command's argument hint reaches the agent directory
#[tokio::test]
async fn edge_a_commands_argument_hint_reaches_the_agent_directory() {
    let f = Fixture::new().await;
    let parts = vec![PartDef::command(
        "compare-quotes",
        "Compare quotes",
        Some("<suppliers> <quarter>"),
    )];
    let installed = install(&f.dir.0, "a", &parts, no_policy());
    let id = AgentInstanceId::new();
    let workspace = WorkspaceId::new();
    f.inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    f.inspector
        .set_workspaces(id, vec![workspace])
        .await
        .expect("set");
    let directory = f.inspector.directory(workspace).await.expect("directory");
    let task = directory[0]
        .tasks
        .iter()
        .find(|t| t.name == "Compare quotes")
        .expect("task");
    assert_eq!(task.argument_hint.as_deref(), Some("<suppliers> <quarter>"));
}

// Scenario: Happy path - the memory contract of an installed agent
#[tokio::test]
async fn happy_path_the_memory_contract_of_an_installed_agent() {
    let f = Fixture::new().await;
    let body = "retains:\n  - decision\nretention:\n  maxAgeDays: 90\nstore:\n  location: local\n";
    let parts = vec![PartDef::yaml_part(
        PartKind::Memory,
        "memories",
        "notes",
        "Notes",
        body,
    )];
    let installed = install(&f.dir.0, "a", &parts, no_policy());
    let id = AgentInstanceId::new();
    f.inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    let contract = f
        .inspector
        .memory_contract(id)
        .await
        .expect("contract")
        .expect("some");
    assert_eq!(contract.retains, ["decision"]);
    assert_eq!(contract.max_age_days, Some(90));
}

// Scenario: Error - memory contract before inspection
#[tokio::test]
async fn error_memory_contract_before_inspection() {
    let f = Fixture::new().await;
    let error = f
        .inspector
        .memory_contract(AgentInstanceId::new())
        .await
        .unwrap_err();
    assert!(matches!(error, InspectError::NotInspected));
}

// Scenario: Edge - an APM-era folder
#[tokio::test]
async fn edge_an_apm_era_folder() {
    let f = Fixture::new().await;
    let folder = f.dir.path("apm");
    write_apm_folder(&folder);
    let id = AgentInstanceId::new();
    let outcome = f
        .inspector
        .inspect(&apm_request(id, &folder))
        .await
        .expect("inspect");
    assert_eq!(
        outcome.model.source,
        ModelSource::ApmFolder {
            package_name: "supplier-analyst".to_owned(),
            package_version: "1.3.0".to_owned(),
        }
    );
    assert!(outcome.model.not_carried.contains(&NotCarried {
        what: ".apm/hooks/pre-commit.sh".to_owned(),
        reason: NotCarriedReason::Executable,
    }));
    assert_eq!(outcome.model.tasks.len(), 1);
    assert_eq!(outcome.model.tasks[0].session_name, None);
    assert_eq!(outcome.model.tool_policy_sha256, None);
    assert_eq!(outcome.model.plugin_name, None);
    assert!(!outcome.model.needs_person_present);
    assert!(f
        .inspector
        .memory_contract(id)
        .await
        .expect("contract")
        .is_none());
}

fn write_apm_folder(folder: &std::path::Path) {
    for (path, text) in [
        ("apm.yml", "name: supplier-analyst\nversion: 1.3.0\n"),
        (
            ".apm/instructions/tone.instructions.md",
            "Write in plain British English.\n",
        ),
        (
            ".apm/hooks/pre-commit.sh",
            "#!/bin/sh\necho before commit\n",
        ),
        (
            ".apm/prompts/compare-quotes.prompt.md",
            "Compare the supplier quotes the person names.\n",
        ),
    ] {
        let target = folder.join(path);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("dir");
        std::fs::write(target, text).expect("file");
    }
}

// Scenario: Edge - installed files list the lock file first
#[tokio::test]
async fn edge_installed_files_list_the_lock_file_first() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    std::fs::write(installed.folder.join("a-notes.txt"), "a").expect("file");
    std::fs::create_dir_all(installed.folder.join("sub")).expect("dir");
    std::fs::write(installed.folder.join("sub").join("b.txt"), "bb").expect("file");
    let id = AgentInstanceId::new();
    f.inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    let files = f
        .inspector
        .installed_files(id, &installed.folder)
        .await
        .expect("files");
    let paths: Vec<&str> = files.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["ordin.lock", "a-notes.txt", "sub/b.txt"]);
    assert_eq!(files[1].size, 1);
    assert_eq!(files[1].sha256, sha256(b"a"));
}

// Scenario: Error - lock file damaged
#[tokio::test]
async fn error_lock_file_damaged() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    std::fs::write(installed.folder.join("ordin.lock"), "not: [valid").expect("write");
    let error = f
        .inspector
        .inspect(&installed.request(AgentInstanceId::new()))
        .await
        .unwrap_err();
    assert!(matches!(error, InspectError::LockFileInvalid));
}

// Scenario: Error - declaration refused
#[tokio::test]
async fn error_declaration_refused() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    installed.declaration(UNBOUND_DECLARATION);
    let id = AgentInstanceId::new();
    let error = f
        .inspector
        .inspect(&installed.request(id))
        .await
        .unwrap_err();
    match error {
        InspectError::DeclarationRefused { code, retryable } => {
            assert_eq!(code, "RULE_UNBOUND_TOOL");
            assert!(!retryable);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(!f.model_file(id).exists(), "no model is stored");
    assert!(matches!(
        f.inspector.model(id).await,
        Err(InspectError::NotInspected)
    ));
}

// Scenario: Error - declaration does not parse
#[tokio::test]
async fn error_declaration_does_not_parse() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    installed.declaration("format-version: 1\nshared: &a [1]\nother: *a\n");
    let id = AgentInstanceId::new();
    let error = f
        .inspector
        .inspect(&installed.request(id))
        .await
        .unwrap_err();
    assert!(matches!(error, InspectError::DeclarationInvalid));
    // The Rules Engine was not called: it holds no rule set and nothing was written for it.
    assert!(!loaded(&f.engine, id).await);
    assert!(!f.rules_file(id).exists());
}

// Scenario: Happy path - rule sets reload at start
#[tokio::test]
async fn happy_path_rule_sets_reload_at_start() {
    let f = Fixture::new().await;
    let first = install(&f.dir.0, "a", &ten_commands(), no_policy());
    let second = install(&f.dir.0, "b", &ten_commands(), no_policy());
    let (id_a, id_b) = (AgentInstanceId::new(), AgentInstanceId::new());
    f.inspector.inspect(&first.request(id_a)).await.expect("a");
    f.inspector.inspect(&second.request(id_b)).await.expect("b");
    // The engine now refuses the second instance's stored set.
    let path = f.rules_file(id_b);
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    value["declaration"] = serde_json::json!({
        "format-version": 1,
        "gates": {"steps": [{"name": "ghost-gate", "point": "before-tool", "tool": "ghost",
            "conditions": [], "outcome": {"kind": "refuse"}}]}
    });
    std::fs::write(&path, serde_json::to_vec(&value).expect("json")).expect("write");
    f.engine.unload_rule_set(id_a);
    f.engine.unload_rule_set(id_b);
    let failures = f.restarted().reload_rule_sets().await.expect("reload");
    assert!(loaded(&f.engine, id_a).await);
    assert!(!loaded(&f.engine, id_b).await);
    assert_eq!(
        failures,
        [RuleSetReloadFailure {
            agent_instance_id: id_b,
            code: "RULE_UNBOUND_TOOL".to_owned(),
        }]
    );
}

// Scenario: Error - unknown folder format
#[tokio::test]
async fn error_unknown_folder_format() {
    let f = Fixture::new().await;
    let folder = f.dir.path("empty");
    std::fs::create_dir_all(&folder).expect("dir");
    std::fs::write(folder.join("README.md"), "hello").expect("file");
    let error = f
        .inspector
        .inspect(&apm_request(AgentInstanceId::new(), &folder))
        .await
        .unwrap_err();
    assert!(matches!(error, InspectError::FormatUnrecognised));
}

fn search_binding() -> PartDef {
    PartDef::tool_binding(
        "search",
        "bindings:\n  - name: search\n    type: mcp\n    permission: read-only\n",
    )
}

// Scenario: Happy path - an update that changes nothing the agent may do
#[tokio::test]
async fn happy_path_an_update_that_changes_nothing_the_agent_may_do() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "installed", &[search_binding()], no_policy());
    let id = AgentInstanceId::new();
    let outcome = f
        .inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    assert_eq!(outcome.may_do.reads, ["search"]);
    let before = std::fs::read(f.model_file(id)).expect("model");
    let candidate = install(&f.dir.0, "candidate", &[search_binding()], no_policy());
    let model = f
        .inspector
        .inspect_candidate(&candidate.request(id))
        .await
        .expect("candidate");
    assert!(compare_may_do(&outcome.may_do, &may_do(&model)).is_empty());
    assert_eq!(std::fs::read(f.model_file(id)).expect("model"), before);
}

// Scenario: Edge - an update that adds a connection
#[test]
fn edge_an_update_that_adds_a_connection() {
    let installed = MayDo {
        tools: vec![],
        reads: vec!["search".to_owned()],
        connections: vec![],
        tool_policy_digest: None,
        declaration_digest: None,
        authority: Authority::DocumentOnly,
    };
    let candidate = MayDo {
        connections: vec!["mail".to_owned()],
        ..installed.clone()
    };
    assert_eq!(
        compare_may_do(&installed, &candidate),
        [MayDoChange {
            kind: MayDoChangeKind::ConnectionAdded,
            name: "mail".to_owned(),
            message_id: "may-do-connection-added",
        }]
    );
}

// Scenario: Edge - a candidate with a changed declaration
#[tokio::test]
async fn edge_a_candidate_with_a_changed_declaration() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "installed", &ten_commands(), no_policy());
    installed.declaration("format-version: 1\n");
    let id = AgentInstanceId::new();
    let outcome = f
        .inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    let rules_before = std::fs::read(f.rules_file(id)).expect("rules");
    // A declaration the engine would refuse: the candidate still inspects, so the engine is not called.
    let candidate = install(&f.dir.0, "candidate", &ten_commands(), no_policy());
    candidate.declaration(UNBOUND_DECLARATION);
    let model = f
        .inspector
        .inspect_candidate(&candidate.request(id))
        .await
        .expect("candidate");
    assert!(model.declared_rules.is_empty());
    assert_eq!(
        std::fs::read(f.rules_file(id)).expect("rules"),
        rules_before
    );
    assert!(
        loaded(&f.engine, id).await,
        "the installed rules stay in force"
    );
    let changes = compare_may_do(&outcome.may_do, &may_do(&model));
    assert!(changes
        .iter()
        .any(|c| c.kind == MayDoChangeKind::DeclarationChanged));
}

// Scenario: Happy path - the agent directory of a workspace
#[tokio::test]
async fn happy_path_the_agent_directory_of_a_workspace() {
    let f = Fixture::new().await;
    let workspace = WorkspaceId::new();
    let mut ids = Vec::new();
    for (label, name, session) in [
        ("b", "Builder", "builder:build"),
        ("a", "Analyst", "analyst:summarise"),
    ] {
        let parts = vec![PartDef::command("summarise", "Summarise", None)];
        let installed = install_named(&f.dir.0, label, name, &parts, no_policy());
        let mut request = installed.request(AgentInstanceId::new());
        request.install.as_mut().expect("facts").command_names =
            BTreeMap::from([(urn("commands", "summarise"), session.to_owned())]);
        f.inspector.inspect(&request).await.expect("inspect");
        ids.push(request.agent_instance_id);
    }
    let mut changes = f.inspector.changes();
    for id in &ids {
        f.inspector
            .set_workspaces(*id, vec![workspace])
            .await
            .expect("set");
    }
    let directory = f.inspector.directory(workspace).await.expect("directory");
    assert_eq!(directory.len(), 2);
    assert_eq!(directory[0].display_name, "Analyst");
    assert_eq!(directory[1].display_name, "Builder");
    assert_eq!(
        directory[0].tasks[0].session_name.as_deref(),
        Some("analyst:summarise")
    );
    assert_eq!(
        directory[1].tasks[0].session_name.as_deref(),
        Some("builder:build")
    );
    let yielded = tokio::time::timeout(Duration::from_secs(5), changes.next())
        .await
        .expect("a change in time");
    assert_eq!(yielded, Some(workspace));
}

// Scenario: Error - page for an unknown part
#[tokio::test]
async fn error_page_for_an_unknown_part() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    let id = AgentInstanceId::new();
    f.inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    let error = f
        .inspector
        .part_page(
            id,
            "urn:ordin:default:skills:none",
            &installed.bundle_dir,
            Detail::LinksCode,
        )
        .await
        .unwrap_err();
    match error {
        InspectError::PartNotFound { part_id } => {
            assert_eq!(part_id, "urn:ordin:default:skills:none")
        }
        other => panic!("unexpected {other:?}"),
    }
}
