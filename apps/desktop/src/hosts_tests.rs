//! §7.2 program-implemented traits of rows 3.24–3.49, each over the real modules of the build with no
//! switch, wired on a temporary data root.

use std::sync::Arc;

use ordin_claude_adapter::SessionSetupSource;
use ordin_crew_orchestrator::{AgentAvailability, CrewHost};
use ordin_home_search::{GoToKind, HomeSourceError, JournalReader, RunAgainSubject, SearchSource};
use ordin_knowledge::{
    KnowledgeFiles, KnowledgeFolders, KnowledgeIdentity, KnowledgeMemory, KnowledgeRules,
    KnowledgeWorkspaces, SourceReadCheck,
};
use ordin_package_catalogue::{HarnessSettings, InstanceActivity};
use ordin_projects::{ServerInstaller, ServerPin, StreamId, StreamPayload, StreamSink};
use ordin_push::WorkingCopyStore;
use ordin_service_api::blocks::{EntityRef, RefResolver};
use ordin_types::common::AgentRef;
use ordin_types::domain::HarnessKind;
use ordin_types::session::ToolCaller;
use ordin_types::wire::ContentRef;
use ordin_types::{
    AgentInstanceId, CrewRunId, EmailLinkId, KnowledgePageId, PlaceId, RemoteAgentId, RunId,
    WorkflowId, WorkingCopyItemId, WorkspaceId,
};

use crate::hosts::Late;
use crate::hosts_home::{
    AgentSearch, ConversationSearch, DesktopJournalReader, DesktopRefResolver, WorkflowSearch,
};
use crate::hosts_knowledge::{
    DesktopKnowledgeFiles, DesktopKnowledgeIdentity, DesktopKnowledgeMemory, DesktopKnowledgeRules,
    DesktopKnowledgeWorkspaces, NoKnowledgeFolders,
};
use crate::hosts_run::{
    assemble, confluence_settings, project_settings, BridgeStreams, DesktopCrewHost,
    DesktopSessionSetup, DesktopWorkingCopy, ProfileHarness, SetupLanguageServers, SetupParts,
    SupervisorActivity,
};
use crate::plan::Row;
use crate::test_support::TempDir;
use crate::wiring::ModuleRunner;
use crate::wiring_tests::{build_until, wire_all};

fn crew_host(runner: &ModuleRunner) -> DesktopCrewHost {
    DesktopCrewHost {
        late: runner.late.clone(),
        localisation: runner.modules.localisation.clone().expect("localisation"),
        profile: runner.modules.profile.clone().expect("profile"),
        mcp_front: runner.modules.mcp_front.clone().expect("front"),
    }
}

// Gherkin (amendment January 2026): Edge case — a paused desktop run is given no paused run in its
// session setup (DSK-WIRE-3).
#[test]
fn edge_case_a_paused_desktop_run_is_given_no_paused_run() {
    // A run paused on approval "A1": its row carries the harness session it continues.
    let setup = assemble(SetupParts {
        working_dir: "/agents/analyst".into(),
        append_system_prompt_file: "/agents/analyst/.ordin/system.md".into(),
        plugin_dir: "/agents/analyst/.ordin/plugin".into(),
        plugin_name: "analyst".to_owned(),
        extra_dirs: vec!["/work/briefs".into()],
        resume_session_id: Some("session-of-A1".to_owned()),
        crew_charter_id: None,
    });
    assert!(setup.paused.is_none());
    assert!(setup.run_starter.is_none(), "agent profile only");
    assert_eq!(setup.resume_session_id.as_deref(), Some("session-of-A1"));
    assert_eq!(
        setup.extra_dirs,
        vec![std::path::PathBuf::from("/work/briefs")]
    );
}

// §7.2: `SessionSetupSource` (Claude Local Adapter). An agent with no install record has no setup.
#[tokio::test]
async fn session_setup_of_an_unknown_agent_is_the_installers_code() {
    let dir = TempDir::new("session-setup");
    let runner = build_until(&dir, Row::ClaudeAdapter).await;
    let m = &runner.modules;
    let source = DesktopSessionSetup {
        late: runner.late.clone(),
        installers: m.installers.clone().expect("installers"),
        usage: m.usage.clone().expect("usage"),
        agent_folders: m.agent_folders.clone().expect("folders"),
        journal: m.journal.clone().expect("journal"),
        files: m.files.clone().expect("files"),
        projects: m.projects.clone().expect("projects"),
    };
    let err = source
        .session_setup(RunId::new(), AgentInstanceId::new(), &[])
        .await
        .expect_err("no agent folder");
    // DSK-REC-8: the `Session` lease is taken before anything else; its error is the code.
    assert!(err.code.starts_with("AFM_"), "{}", err.code);
    assert!(!runner.late.runs.running(AgentInstanceId::new()));
}

// §7.2: `CrewHost` (Crew Orchestrator), before and after the late-bound modules exist.
#[tokio::test]
async fn crew_host_answers_over_the_package_catalogue_and_the_ordin_host() {
    let dir = TempDir::new("crew-host");
    let early = build_until(&dir, Row::CrewOrchestrator).await;
    let host = crew_host(&early);
    let workspace = WorkspaceId::new();
    let member = AgentRef::AgentInstance {
        agent_instance_id: AgentInstanceId::new(),
    };
    // Before rows 3.40 and 3.41: the permanent answers.
    assert!(matches!(
        host.agent_status(workspace, &member).await,
        AgentAvailability::Missing
    ));
    assert!(host
        .saved_agent_versions(WorkflowId::new(), 1)
        .await
        .is_empty());
    assert!(host
        .workflow_steps(workspace, WorkflowId::new())
        .await
        .is_none());
    assert_eq!(host.remote_route(RemoteAgentId::new()), None);
    drop(early);

    let dir = TempDir::new("crew-host-wired");
    let runner = wire_all(&dir).await;
    let host = crew_host(&runner);
    assert!(
        matches!(
            host.agent_status(workspace, &member).await,
            AgentAvailability::Missing
        ),
        "an agent this computer does not have"
    );
    let remote = AgentRef::RemoteAgent {
        remote_agent_id: RemoteAgentId::new(),
    };
    assert!(
        matches!(
            host.agent_status(workspace, &remote).await,
            AgentAvailability::Missing
        ),
        "SPEC-GAP-8 (OQ-REC-1)"
    );
    assert!(host
        .saved_agent_versions(WorkflowId::new(), 1)
        .await
        .is_empty());
    assert!(host
        .workflow_steps(workspace, WorkflowId::new())
        .await
        .is_none());
    runner.shutdown.cancel();
}

// §7.2: `WorkingCopyStore` (Push Service), `HarnessSettings` and `InstanceActivity` (Package
// Catalogue).
#[tokio::test]
async fn working_copy_harness_and_activity_answer_over_their_modules() {
    let dir = TempDir::new("working-copy");
    let runner = build_until(&dir, Row::Catalogue).await;
    let m = &runner.modules;
    let store = DesktopWorkingCopy {
        workspaces: m.workspaces.clone().expect("workspaces"),
        files: m.files.clone().expect("files"),
    };
    let missing = WorkingCopyItemId::new();
    let failure = store.entry(missing).await.expect_err("no such item");
    assert!(!failure.code.is_empty());
    assert!(store.content(missing, None).await.is_err());
    assert!(store.content(missing, Some(1)).await.is_err());
    assert!(store.restore_version(missing, 1).await.is_err());
    let _changes = store.local_changes();

    let harness = ProfileHarness {
        profile: m.profile.clone().expect("profile"),
    };
    assert_eq!(harness.default_harness(), HarnessKind::ClaudeCode);

    let activity = SupervisorActivity {
        late: runner.late.clone(),
    };
    assert!(!activity.running(AgentInstanceId::new()));
    let _idle = activity.idle();
}

// §7.2: `StreamSink` and `ServerInstaller` (Project Service), late-bound to the Command Bridge
// (row 3.49) and the Setup Assistant (row 3.43); the settings channels from the profile.
#[tokio::test]
async fn project_ports_wait_for_the_bridge_and_the_setup_assistant() {
    let dir = TempDir::new("project-ports");
    let early = build_until(&dir, Row::Projects).await;
    let streams = BridgeStreams::new(early.late.clone());
    let stream = StreamId(uuid::Uuid::now_v7());
    // No bridge yet: dropped, never a panic.
    streams.send(stream, StreamPayload::TerminalBytes(b"ls\n".to_vec()));
    let installer = SetupLanguageServers {
        late: early.late.clone(),
    };
    let pin = ServerPin {
        name: "rust-analyzer".to_owned(),
        version: "2026-01-05".to_owned(),
    };
    assert!(installer.ensure("rust", &pin).await.is_err());
    let profile = early.modules.profile.clone().expect("profile");
    let settings = project_settings(&profile).borrow().clone();
    assert_eq!(settings.gitlab_host, "gitlab.com");
    assert!(settings.max_attempts >= 1);
    assert!(confluence_settings(&profile).borrow().site.is_none());
    drop(early);

    let dir = TempDir::new("project-ports-wired");
    let runner = wire_all(&dir).await;
    let streams = BridgeStreams::new(runner.late.clone());
    // The bridge exists but no window has opened its streams: dropped.
    streams.send(stream, StreamPayload::TerminalBytes(b"ls\n".to_vec()));
    streams.send(stream, StreamPayload::Lsp(serde_json::json!({ "id": 1 })));
    streams.send(
        stream,
        StreamPayload::Closed {
            reason: "exited".to_owned(),
        },
    );
    let installer = SetupLanguageServers {
        late: runner.late.clone(),
    };
    // A language no table names is refused by the Setup Assistant itself.
    assert!(installer.ensure("cobol", &pin).await.is_err());
    runner.shutdown.cancel();
}

fn knowledge_parts(
    runner: &ModuleRunner,
) -> (
    DesktopKnowledgeFiles,
    DesktopKnowledgeWorkspaces,
    DesktopKnowledgeRules,
    DesktopKnowledgeMemory,
) {
    let m = &runner.modules;
    let workspaces = m.workspaces.clone().expect("workspaces");
    (
        DesktopKnowledgeFiles {
            files: m.files.clone().expect("files"),
            agent_folders: m.agent_folders.clone().expect("folders"),
        },
        DesktopKnowledgeWorkspaces {
            workspaces: workspaces.clone(),
            late: runner.late.clone(),
        },
        DesktopKnowledgeRules {
            rules: m.rules.clone().expect("rules"),
            files: m.files.clone().expect("files"),
            workspaces: workspaces.clone(),
            late: runner.late.clone(),
        },
        DesktopKnowledgeMemory::new(m.memory.clone().expect("memory")),
    )
}

// §7.2: the Knowledge Service's ports (row 3.38).
#[tokio::test]
async fn knowledge_ports_delegate_to_their_modules() {
    let dir = TempDir::new("knowledge-ports");
    let early = build_until(&dir, Row::Knowledge).await;
    let (files, workspaces, rules, memory) = knowledge_parts(&early);
    let agent = AgentInstanceId::new();
    assert!(files.list_agent_knowledge(agent).await.is_err());
    assert!(files.read_agent_knowledge(agent, "../x.md").await.is_err());
    assert!(files
        .read_agent_knowledge(agent, "notes.txt")
        .await
        .is_err());
    assert!(files.read_file(PlaceId::new(), "a.md").await.is_err());
    let _changes = files.subscribe_changes();
    let created = early
        .modules
        .workspaces
        .clone()
        .expect("workspaces")
        .create("Northwind".to_owned())
        .await
        .expect("created");
    let listed = workspaces.list_not_in_bin().await.expect("list");
    assert!(listed.iter().any(|w| w.id == created.id));
    assert_eq!(
        workspaces.get(created.id).await.expect("get").id,
        created.id
    );
    // The Package Catalogue (row 3.40) is not built yet.
    let err = workspaces
        .agents_in_workspace(created.id)
        .await
        .expect_err("late-bound");
    assert_eq!(err.code, "PKG_STORE_FAILED");
    let check = SourceReadCheck {
        workspace_id: created.id,
        caller: ToolCaller::Ordin,
        agent_instance_id: agent,
        origin: "sharepoint".to_owned(),
        location: "https://example.invalid/sites/team/doc.docx".to_owned(),
    };
    // A source with no working copy is not readable (fail-safe).
    assert_ne!(rules.may_read_source(&check).await, Ok(true));
    // DSK-REC-12: forwarded with the caller's workspace, never refused as unsupported.
    if let Err(e) = memory.check_content(created.id, "text").await {
        assert_ne!(e.code(), "MEM_NOT_SUPPORTED");
    }
    memory
        .remove_page(created.id, None, KnowledgePageId::new())
        .await
        .expect("nothing to remove");
    assert!(NoKnowledgeFolders
        .adoption_for(created.id, KnowledgePageId::new())
        .await
        .is_none());
    let identity = DesktopKnowledgeIdentity {
        identity: early.modules.identity.clone().expect("identity"),
    };
    assert_eq!(identity.git_login().await, None, "no Git host sign-in");
    early.shutdown.cancel();
}

fn reader(runner: &ModuleRunner) -> DesktopJournalReader {
    DesktopJournalReader {
        journal: runner.modules.journal.clone().expect("journal"),
        approval: runner.modules.approval.clone().expect("approval"),
        late: runner.late.clone(),
    }
}

// §7.2: `RefResolver` (Service API), `JournalReader` and `SearchSource` (Home and Search).
#[tokio::test]
async fn ref_resolver_journal_reader_and_search_sources_answer_on_an_empty_computer() {
    let dir = TempDir::new("home-ports");
    let runner = wire_all(&dir).await;
    let journal = runner.modules.journal.clone().expect("journal");
    let refs = DesktopRefResolver::new(
        journal,
        runner.modules.catalogue.clone().expect("catalogue"),
        &runner.modules.localisation.clone().expect("localisation"),
        runner.late.clone(),
    );
    let workspace = WorkspaceId::new();
    let file = ContentRef::PlaceFile {
        place_id: PlaceId::new(),
        relative_path: "brief.md".to_owned(),
    };
    assert!(!refs.content(workspace, &file).await);
    assert!(
        !refs
            .email_message(workspace, EmailLinkId::new(), "<m@example.invalid>")
            .await
    );
    let page = EntityRef::KnowledgePage(KnowledgePageId::new());
    assert!(!refs.entity(workspace, page).await);
    assert_eq!(refs.run_agent_name(RunId::new()).await, None);
    let plan = refs.plan_state(RunId::new()).await;
    assert_eq!((plan.versions, plan.current_len), (0, 0));
    // DSK-REC-14: the four approve-button words of every bundled locale.
    for verb in ["Run", "Allow", "Finish anyway", "Carry on"] {
        assert!(refs.approval_verbs().iter().any(|v| v == verb), "{verb}");
    }
    // DSK-REC-17: an unknown run, memory fact or repository, and any charter, answer false.
    assert!(!refs.entity(workspace, EntityRef::Run(RunId::new())).await);
    assert!(
        !refs
            .entity(
                workspace,
                EntityRef::CrewCharter(ordin_types::CrewCharterId::new())
            )
            .await
    );
    // 2C wire §13: a Confluence link the workspace does not hold is unresolved.
    assert!(
        !refs
            .entity(
                workspace,
                EntityRef::ConfluenceLink(ordin_types::ConfluenceLinkId::new())
            )
            .await
    );
    // DSK-REC-16: an artefact no row has is unresolved.
    let artefact = ContentRef::Artefact {
        artefact_id: ordin_types::ArtefactId(uuid::Uuid::now_v7()),
    };
    assert!(!refs.content(workspace, &artefact).await);

    let reader = reader(&runner);
    assert!(reader.waiting_items().await.expect("waiting").is_empty());
    assert!(reader.running().await.expect("running").is_empty());
    assert!(reader
        .finished_since(ordin_types::time::Timestamp::now(), 10)
        .await
        .expect("finished")
        .is_empty());
    assert_eq!(
        reader
            .run_inputs(RunAgainSubject::Run {
                run_id: RunId::new()
            })
            .await,
        Err(HomeSourceError::NotFound)
    );
    assert_eq!(
        reader
            .run_inputs(RunAgainSubject::CrewRun {
                crew_run_id: CrewRunId::new()
            })
            .await,
        Err(HomeSourceError::NotFound)
    );

    let late: Arc<Late> = runner.late.clone();
    let sources: [(Arc<dyn SearchSource>, GoToKind); 3] = [
        (
            Arc::new(AgentSearch { late: late.clone() }),
            GoToKind::Agent,
        ),
        (
            Arc::new(WorkflowSearch { late: late.clone() }),
            GoToKind::Workflow,
        ),
        (
            Arc::new(ConversationSearch { late: late.clone() }),
            GoToKind::Conversation,
        ),
    ];
    for (source, group) in sources {
        assert_eq!(source.group(), group);
        assert!(source.find("brief", 5).await.is_empty());
    }
    runner.shutdown.cancel();
}

// Gherkin (amendment January 2026): Happy path — a session holds its lease until the run ends
// (DSK-REC-8, DSK-REC-10).
#[tokio::test]
async fn happy_path_a_session_holds_its_lease_until_the_run_ends() {
    use ordin_session_supervisor::SupervisorHost;
    let dir = TempDir::new("run-register");
    let runner = build_until(&dir, Row::Catalogue).await;
    let folders = runner.modules.agent_folders.clone().expect("folders");
    let agent = AgentInstanceId::new();
    folders.create(agent).await.expect("an agent folder");
    let activity = SupervisorActivity {
        late: runner.late.clone(),
    };
    let mut idle = activity.idle();
    assert!(!activity.running(agent), "agent A has no run");
    let run = RunId::new();
    let lease = folders
        .lock(agent, ordin_agent_folders::LockMode::Session)
        .await
        .expect("a session lease");
    runner.late.runs.hold(run, agent, lease);
    assert!(activity.running(agent));
    // An update of A waits for the folder.
    let blocked = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        folders.lock(agent, ordin_agent_folders::LockMode::Write),
    )
    .await;
    assert!(blocked.is_err(), "the write lease waits");
    let host = crate::hosts::DesktopSupervisorHost {
        late: runner.late.clone(),
    };
    host.run_ended(run).await;
    assert!(!activity.running(agent));
    assert_eq!(idle.try_recv().expect("idle"), agent);
    folders
        .lock(agent, ordin_agent_folders::LockMode::Write)
        .await
        .expect("the folder is free");
    // An absent entry is no error.
    host.run_ended(RunId::new()).await;
}

fn plain(text: &str) -> String {
    text.replace(['\u{2068}', '\u{2069}'], "")
}

fn words_runner_localisation(runner: &ModuleRunner) -> Arc<ordin_localisation::Localisation> {
    runner.modules.localisation.clone().expect("localisation")
}

// Gherkin (amendment January 2026): DSK-REC-1 card words pass the Approval Service's checks.
#[tokio::test]
async fn card_words_for_rules_proposals_pass_the_approval_checks() {
    use ordin_rules::Proposal;
    let dir = TempDir::new("card-words");
    let runner = build_until(&dir, Row::Brand).await;
    let localisation = words_runner_localisation(&runner);
    let words = crate::words::Words {
        localisation: &localisation,
    };
    // Happy path — a shell command card.
    let card = words.proposal(&Proposal::ShellCommand {
        command: "make test".to_owned(),
        runs_in: "supplier-portal".to_owned(),
    });
    assert_eq!(
        plain(&card.question),
        "Run this command in supplier-portal?"
    );
    assert_eq!(card.verb, "Run");
    assert!(matches!(
        card.affects.as_slice(),
        [ordin_types::wire::approval::AffectedItem::Record { label }] if label == "supplier-portal"
    ));
    assert_eq!(
        plain(&card.changes),
        "The command can read and change files in supplier-portal."
    );
    assert_eq!(card.content_label.as_deref(), Some("See the command"));
    assert_eq!(
        plain(&card.proposal.action),
        "Run this command in supplier-portal"
    );
    assert_eq!(card.proposal.consequence, card.changes);
    // Edge case — an empty field reads "Not stated".
    let card = words.proposal(&Proposal::ShellCommand {
        command: "ls".to_owned(),
        runs_in: String::new(),
    });
    assert_eq!(card.where_line, "Not stated");
    // Edge case — a long tool name is cut so the question fits.
    let card = words.proposal(&Proposal::ApprovalListTool {
        tool: "t".repeat(300),
    });
    assert!(card.question.chars().count() <= 120, "{}", card.question);
    assert!(card.question.ends_with('?'));
    // Every variant fills every field the Approval Service checks.
    let all = [
        Proposal::ExternalTool {
            tool: "send_mail".to_owned(),
            kind: "mail".to_owned(),
            destination: "mail.example.invalid".to_owned(),
        },
        Proposal::PackageTool {
            server: "files".to_owned(),
            tool: "delete".to_owned(),
        },
        Proposal::ArgumentRulePause {
            rule: "no-prod".to_owned(),
            tool: "deploy".to_owned(),
        },
        Proposal::AgentPolicyApproval {
            verb: "publish".to_owned(),
            tool: "confluence".to_owned(),
        },
        Proposal::EndGatesSetAside {
            gates: vec!["tests".to_owned(), "lint".to_owned()],
        },
        Proposal::EndGatesSetAside { gates: vec![] },
    ];
    for proposal in all {
        let card = words.proposal(&proposal);
        assert!(card.question.ends_with('?'), "{proposal:?}");
        assert!(card.question.chars().count() <= 120, "{proposal:?}");
        assert!(!card.verb.is_empty() && !card.where_line.is_empty());
        assert!(!card.affects.is_empty() && card.affects.len() <= 50);
        assert!(!card.changes.is_empty() && !card.proposal.consequence.is_empty());
        assert!(!card.proposal.action.is_empty() && !card.proposal.destination.is_empty());
    }
}

// Gherkin (amendment January 2026): Happy path — a rules line names where the rule comes from
// (DSK-REC-3); and the crew step's card (DSK-REC-2).
#[tokio::test]
async fn rules_lines_and_crew_step_cards_use_the_catalogue() {
    use ordin_rules::{ControlLine, ControlSource};
    let dir = TempDir::new("rules-lines");
    let runner = build_until(&dir, Row::Brand).await;
    let localisation = words_runner_localisation(&runner);
    let words = crate::words::Words {
        localisation: &localisation,
    };
    let line = ControlLine {
        name: "no-commands".to_owned(),
        source: ControlSource::Application,
        point: ordin_rules::Point::BeforeTool,
        command: None,
        enforced: true,
        reason: String::new(),
        detail: "this agent may not run commands".to_owned(),
    };
    assert_eq!(
        plain(&words.rules_line(&line)),
        "Always on: this agent may not run commands"
    );
    let mut long = line.clone();
    long.detail = "x".repeat(400);
    assert_eq!(words.rules_line(&long).chars().count(), 200);
    let view = ordin_crew_orchestrator::CrewView {
        crew_run_id: CrewRunId::new(),
        workspace_id: WorkspaceId::new(),
        purpose: "Q3 supplier review".to_owned(),
        members: Vec::new(),
        round: 1,
        rounds_left: 3,
        seconds_left: 600,
        requests_left: None,
        inputs: Vec::new(),
        latest_results: Vec::new(),
        position: None,
        cancel: tokio_util::sync::CancellationToken::new(),
    };
    let card = words.crew_step(&view, "Check the contract", "");
    assert_eq!(plain(&card.question), "Carry on with “Check the contract”?");
    assert_eq!(card.verb, "Carry on");
    assert_eq!(plain(&card.where_line), "Crew: Q3 supplier review");
    assert_eq!(card.changes, "The crew waits here until you choose.");
}

// Gherkin (amendment January 2026): Edge case — an application document condition takes its first
// path (DSK-REC-6).
#[tokio::test]
async fn edge_case_an_application_document_condition_takes_its_first_path() {
    let dir = TempDir::new("document-changed");
    let runner = build_until(&dir, Row::CrewOrchestrator).await;
    let host = crew_host(&runner);
    let input = ordin_crew_orchestrator::CrewInput {
        label: "contract".to_owned(),
        value: ordin_crew_orchestrator::CrewInputValue::Text {
            text: "the contract".to_owned(),
        },
        used: Vec::new(),
    };
    assert!(host.document_changed(WorkspaceId::new(), &input).await);
}

/// Polls `check` every 50 ms for up to 5 s.
async fn eventually<F, Fut>(mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if check().await {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

// FU-106: the Knowledge Service's own writes and a `move_file` rename, against the real File
// Service's change events (row 3.22 through the program's `KnowledgeFiles`).
#[tokio::test]
async fn knowledge_follows_the_real_file_services_events() {
    let dir = TempDir::new("knowledge-events");
    let runner = wire_all(&dir).await;
    let m = &runner.modules;
    let knowledge = m.knowledge.clone().expect("row 3.38");
    let files = m.files.clone().expect("row 3.22");
    let workspace = m
        .workspaces
        .clone()
        .expect("row 3.35")
        .create("Northwind".to_owned())
        .await
        .expect("a workspace");
    let listening = tokio::spawn(Arc::clone(&knowledge).run(runner.shutdown.clone()));
    // An own write: the page is written through the File Service, and its change event is not
    // taken for another app's edit or a new file to adopt.
    let page = knowledge
        .write_page(
            ordin_types::domain::places::KnowledgeAuthor::Person,
            ordin_knowledge::WritePage {
                workspace_id: workspace.id,
                page_id: None,
                title: "Supplier terms".to_owned(),
                body_markdown: "Kestrel pays in 30 days.".to_owned(),
                sources: Vec::new(),
                run_id: None,
                tags: None,
                aliases: None,
                page_type: None,
                description: None,
            },
        )
        .await
        .expect("a page");
    let path = knowledge
        .get_page(page)
        .await
        .expect("the page")
        .relative_path;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let listed = knowledge
        .list_pages(workspace.id, None)
        .await
        .expect("listed");
    assert_eq!(
        listed.this_workspace.len(),
        1,
        "the own write adopts nothing"
    );
    assert!(listed.not_adopted.is_empty());
    // `move_file`'s Renamed event moves the page with its file.
    files
        .move_file(
            workspace.own_folder_place_id,
            &path,
            "knowledge/Supplier payment terms.md",
        )
        .await
        .expect("moved");
    let k = knowledge.clone();
    assert!(
        eventually(|| {
            let k = k.clone();
            async move {
                k.get_page(page)
                    .await
                    .is_ok_and(|v| v.relative_path == "knowledge/Supplier payment terms.md")
            }
        })
        .await,
        "the page follows its renamed file"
    );
    runner.shutdown.cancel();
    let _ = listening.await;
}

// ---------------------------------------------------------------- attachments (DSK-REC-26, 29)

mod attachments {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use ordin_claude_adapter::SessionSetupError;
    use ordin_types::domain::places::{Artefact, ArtefactLocation};
    use ordin_types::num::Sha256Hex;
    use ordin_types::time::Timestamp;
    use ordin_types::{ArtefactId, PlaceId, RunId};

    use crate::hosts_run::{attachment_paths, AttachmentReads, AttachmentRun};
    use crate::test_support::TempDir;

    /// Artefacts in one place whose folder is `place`; an id in `failing` fails its Journal read.
    struct Reads {
        place: PathBuf,
        artefacts: HashMap<ArtefactId, Artefact>,
        failing: Option<ArtefactId>,
    }

    #[async_trait::async_trait]
    impl AttachmentReads for Reads {
        async fn artefact(&self, id: ArtefactId) -> Result<Option<Artefact>, SessionSetupError> {
            if self.failing == Some(id) {
                return Err(SessionSetupError {
                    code: "JRNL_DATASTORE_UNAVAILABLE".to_owned(),
                });
            }
            Ok(self.artefacts.get(&id).cloned())
        }
        async fn folder(&self, _location: ArtefactLocation) -> Result<PathBuf, SessionSetupError> {
            Ok(self.place.clone())
        }
    }

    fn artefact(path: &str) -> Artefact {
        Artefact {
            id: ArtefactId::new(),
            location: ArtefactLocation::Place {
                place_id: PlaceId::new(),
            },
            path: path.to_owned(),
            version: 1,
            hash: Sha256Hex("0".repeat(64)),
            producing_run_id: None,
            media_type: "text/plain".to_owned(),
            created_at: Timestamp::now(),
        }
    }

    struct World {
        _dir: TempDir,
        agent_folder: PathBuf,
        place: PathBuf,
        reads: Reads,
    }

    /// An agent folder and a place, each with one file; `inside.md` is in the place.
    fn world(label: &str) -> World {
        let dir = TempDir::new(label);
        let agent_folder = dir.path().join("agent");
        let place = dir.path().join("place");
        std::fs::create_dir_all(&agent_folder).unwrap();
        std::fs::create_dir_all(place.join("sub")).unwrap();
        std::fs::write(place.join("inside.md"), "x").unwrap();
        std::fs::write(place.join("sub/other.md"), "y").unwrap();
        World {
            _dir: dir,
            agent_folder,
            reads: Reads {
                place: place.clone(),
                artefacts: HashMap::new(),
                failing: None,
            },
            place,
        }
    }

    impl World {
        fn add(&mut self, path: &str) -> ArtefactId {
            let a = artefact(path);
            let id = a.id;
            self.reads.artefacts.insert(id, a);
            id
        }

        async fn resolve(
            &self,
            crew_round: bool,
            extra: &[PathBuf],
            ids: &[ArtefactId],
        ) -> Result<Vec<PathBuf>, SessionSetupError> {
            attachment_paths(
                &self.reads,
                AttachmentRun {
                    run_id: RunId::new(),
                    crew_round,
                    agent_folder: &self.agent_folder,
                    extra_dirs: extra,
                },
                ids,
            )
            .await
        }
    }

    fn canonical(p: &Path) -> PathBuf {
        std::fs::canonicalize(p).unwrap()
    }

    // Scenario: Happy path — a crew member is given its input document (DSK-REC-29)
    #[tokio::test]
    async fn a_crew_member_is_given_its_input_document_in_a_granted_place() {
        let mut w = world("attach-granted");
        let a = w.add("inside.md");
        let paths = w
            .resolve(true, std::slice::from_ref(&w.place), &[a])
            .await
            .unwrap();
        assert_eq!(paths, vec![canonical(&w.place.join("inside.md"))]);
    }

    // DSK-REC-29: duplicates removed, first kept, the order of the request otherwise.
    #[tokio::test]
    async fn duplicate_attachments_are_given_once_in_order() {
        let mut w = world("attach-dupes");
        let a = w.add("inside.md");
        let b = w.add("sub/other.md");
        let paths = w
            .resolve(false, std::slice::from_ref(&w.place), &[b, a, b])
            .await
            .unwrap();
        assert_eq!(
            paths,
            vec![
                canonical(&w.place.join("sub/other.md")),
                canonical(&w.place.join("inside.md"))
            ]
        );
    }

    // Scenario: Edge case — a crew member's input outside its folders is left out (OQ-REC-9)
    #[tokio::test]
    async fn a_crew_members_input_outside_its_folders_is_left_out() {
        let mut w = world("attach-crew-outside");
        let a = w.add("inside.md");
        let paths = w.resolve(true, &[], &[a]).await.unwrap();
        assert!(paths.is_empty());
    }

    // Scenario: Error path — a task's attachment outside its folders still fails (DSK-REC-29)
    #[tokio::test]
    async fn a_tasks_attachment_outside_its_folders_fails() {
        let mut w = world("attach-task-outside");
        let a = w.add("inside.md");
        let err = w.resolve(false, &[], &[a]).await.unwrap_err();
        assert_eq!(err.code, "RA_ATTACHMENT_NOT_GRANTED");
    }

    // Scenario: Error path — a crew member's unknown input still fails (DSK-REC-29)
    #[tokio::test]
    async fn a_crew_members_unknown_input_still_fails() {
        let w = world("attach-unknown");
        let err = w
            .resolve(true, std::slice::from_ref(&w.place), &[ArtefactId::new()])
            .await
            .unwrap_err();
        assert_eq!(err.code, "RA_ATTACHMENT_NOT_GRANTED");
    }

    // DSK-REC-26 step 4: a path that names no existing file fails, for a crew round too.
    #[tokio::test]
    async fn a_missing_file_fails_for_every_origin() {
        let mut w = world("attach-missing");
        let a = w.add("gone.md");
        for crew_round in [false, true] {
            let err = w
                .resolve(crew_round, std::slice::from_ref(&w.place), &[a])
                .await
                .unwrap_err();
            assert_eq!(err.code, "RA_ATTACHMENT_NOT_GRANTED");
        }
        // A folder is not a regular file.
        let b = w.add("sub");
        let err = w
            .resolve(false, std::slice::from_ref(&w.place), &[b])
            .await
            .unwrap_err();
        assert_eq!(err.code, "RA_ATTACHMENT_NOT_GRANTED");
    }

    // DSK-REC-26 step 4: a path that climbs out of a granted folder is resolved before the test.
    #[tokio::test]
    async fn a_path_that_climbs_out_of_the_grant_is_refused() {
        let mut w = world("attach-climb");
        let a = w.add("../place/inside.md");
        let granted = w.place.join("sub");
        let err = w.resolve(false, &[granted], &[a]).await.unwrap_err();
        assert_eq!(err.code, "RA_ATTACHMENT_NOT_GRANTED");
    }

    // DSK-REC-26 step 2: a failed Journal read is the owner's code.
    #[tokio::test]
    async fn a_failed_journal_read_is_the_owners_code() {
        let mut w = world("attach-journal");
        let a = w.add("inside.md");
        w.reads.failing = Some(a);
        let err = w
            .resolve(false, std::slice::from_ref(&w.place), &[a])
            .await
            .unwrap_err();
        assert_eq!(err.code, "JRNL_DATASTORE_UNAVAILABLE");
    }

    // DSK-REC-29: more than 50 distinct ids are too many; 50 repeated ids are one.
    #[tokio::test]
    async fn more_than_fifty_attachments_are_too_many() {
        let w = world("attach-many");
        let ids: Vec<ArtefactId> = (0..51).map(|_| ArtefactId::new()).collect();
        let err = w.resolve(false, &[], &ids).await.unwrap_err();
        assert_eq!(err.code, "RA_INPUT_TOO_LARGE");
        let mut w = world("attach-many-same");
        let a = w.add("inside.md");
        let paths = w
            .resolve(false, std::slice::from_ref(&w.place), &[a; 60])
            .await
            .unwrap();
        assert_eq!(paths.len(), 1);
    }

    // DSK-REC-26 step 4: an artefact in the run's own agent folder passes with no grant.
    #[tokio::test]
    async fn an_artefact_in_the_agent_folder_passes() {
        let mut w = world("attach-own");
        std::fs::write(w.agent_folder.join("notes.md"), "z").unwrap();
        w.reads.place = w.agent_folder.clone();
        let a = w.add("notes.md");
        let paths = w.resolve(false, &[], &[a]).await.unwrap();
        assert_eq!(paths, vec![canonical(&w.agent_folder.join("notes.md"))]);
        assert!(w.resolve(false, &[], &[]).await.unwrap().is_empty());
    }
}

// Gherkin (amendment January 2026, DSK-REC-30): two long arguments still fit the question, and
// short arguments are not cut.
#[tokio::test]
async fn questions_with_two_long_arguments_fit_the_approval_service() {
    use ordin_rules::Proposal;
    let dir = TempDir::new("card-question-length");
    let runner = build_until(&dir, Row::Brand).await;
    let localisation = words_runner_localisation(&runner);
    let words = crate::words::Words {
        localisation: &localisation,
    };
    // Edge case — two long arguments still fit the question.
    let long = |c: char| c.to_string().repeat(100);
    for proposal in [
        Proposal::ExternalTool {
            tool: long('t'),
            kind: "mail".to_owned(),
            destination: long('d'),
        },
        Proposal::PackageTool {
            server: long('s'),
            tool: long('t'),
        },
        Proposal::AgentPolicyApproval {
            verb: long('v'),
            tool: long('t'),
        },
    ] {
        let card = words.proposal(&proposal);
        let n = card.question.chars().count();
        assert!((1..=120).contains(&n), "{n}: {}", card.question);
        assert!(card.question.ends_with('?'), "{}", card.question);
        // With the en-GB messages, each argument keeps at least 43 characters (DSK-REC-30).
        let kept = plain(&card.question)
            .split(|c: char| !c.is_alphanumeric() && c != '…')
            .map(|w| w.chars().count())
            .max()
            .unwrap();
        assert!(kept >= 43, "{kept}: {}", card.question);
        assert!(card.question.contains('…'));
    }
    // Edge case — short arguments are not cut.
    let card = words.proposal(&Proposal::PackageTool {
        server: "jira".to_owned(),
        tool: "read_issue".to_owned(),
    });
    assert_eq!(
        plain(&card.question),
        "Let the agent use read_issue from jira?"
    );
    // One argument of 60 characters fits as DSK-REC-1 cuts it.
    let card = words.proposal(&Proposal::ShellCommand {
        command: "ls".to_owned(),
        runs_in: "r".repeat(60),
    });
    assert!(plain(&card.question).contains(&"r".repeat(60)));
    assert!(card.question.chars().count() <= 120);
}
