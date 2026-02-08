//! Tests of the real construction (every row and wiring step of the build with no switch), the
//! program-side adapters over the real modules, and `DesktopServiceApi` over them.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_hook_relay::{AgentWrites, HookRelay, HookRelayConfig, NoPlaces, WriteTarget};
use ordin_rules::{InstanceRuleSet, RulesError, RulesHost, RunKey, RunStartFacts};
use ordin_runtime_adapter::RunTarget;
use ordin_service_api::{command, dispatch, CallContext, CallOrigin, CATALOGUE};
use ordin_session_supervisor::{ContentCheck, ContentPoint, SupervisorHost};
use ordin_setup::{AppUpdater, InstallKind, UpdateOffer, UpdaterError};
use ordin_types::domain::agents::Authority;
use ordin_types::{AgentInstanceId, CommandRequestId, RunId, WorkspaceId};
use serde_json::json;

use crate::core::StepRunner;
use crate::error::{StartError, WRITES_NOT_ATTRIBUTED};
use crate::hosts::{DesktopRulesHost, DesktopSupervisorHost, FileAgentWrites};
use crate::paths::DesktopPaths;
use crate::plan::{start_steps, Adapter, Provider, Row, Step};
use crate::service_api::{ApiModules, DesktopErrorSentences, DesktopServiceApi};
use crate::test_support::TempDir;
use crate::wiring::{check_attribution, protected_roots, CoreInputs, ModuleRunner};

pub(crate) fn inputs(dir: &TempDir) -> CoreInputs {
    let installation_id = uuid::Uuid::now_v7();
    let paths = DesktopPaths::new(
        dir.path().join("data"),
        dir.path(),
        &installation_id.to_string(),
    );
    paths.prepare().expect("data root");
    CoreInputs {
        paths,
        http: reqwest::Client::new(),
        bundle_identifier: "io.gatblau.ordin.test".to_owned(),
        computer_name: "Test computer".to_owned(),
        installation_id,
        os_locales: vec!["en-GB".to_owned()],
        home: dir.path().join("home"),
        installer_defaults: Default::default(),
        browser: None,
        models_dir: dir.path().join("models"),
        model_manifest: Default::default(),
        models_source: None,
        claude_path: dir.path().join("claude"),
        documents_dir: dir.path().join("Documents"),
        bundled_catalogue: dir.path().join("bundled-catalogue"),
        url_opener: None,
        updater: Some(Arc::new(NoUpdate)),
        shell: None,
    }
}

/// The shell's updater, with no newer version (the window part is slice B's).
struct NoUpdate;

#[async_trait::async_trait]
impl AppUpdater for NoUpdate {
    async fn check(&self, _feed_url: &reqwest::Url) -> Result<Option<UpdateOffer>, UpdaterError> {
        Ok(None)
    }
    async fn download(&self, _offer: &UpdateOffer) -> Result<(), UpdaterError> {
        Ok(())
    }
    async fn install_and_restart(&self) -> Result<(), UpdaterError> {
        Ok(())
    }
    async fn install_on_exit(&self) -> Result<(), UpdaterError> {
        Ok(())
    }
    fn install_kind(&self) -> InstallKind {
        InstallKind::PerUser
    }
    fn other_install(&self) -> Option<std::path::PathBuf> {
        None
    }
}

/// Runs every step of this build, each of which must succeed.
pub(crate) async fn wire_all(dir: &TempDir) -> ModuleRunner {
    let mut runner = ModuleRunner::new(inputs(dir));
    for step in start_steps() {
        runner
            .run(step)
            .await
            .unwrap_or_else(|e| panic!("{step:?}: {e:?}"));
    }
    runner
}

/// Runs every step of this build up to, not including, `Construct(until)`.
pub(crate) async fn build_until(dir: &TempDir, until: Row) -> ModuleRunner {
    let mut runner = ModuleRunner::new(inputs(dir));
    for step in start_steps() {
        if step == Step::Construct(until) {
            break;
        }
        runner
            .run(step)
            .await
            .unwrap_or_else(|e| panic!("{step:?}: {e:?} {:?}", e.module()));
    }
    runner
}

pub(crate) fn context() -> CallContext {
    let request_id = CommandRequestId::new();
    CallContext {
        correlation_id: request_id.to_string(),
        request_id,
        origin: CallOrigin::LocalScreens,
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: tokio_util::sync::CancellationToken::new(),
    }
}

pub(crate) fn rule_set(agent: AgentInstanceId, folder: &std::path::Path) -> InstanceRuleSet {
    InstanceRuleSet {
        agent_instance_id: agent,
        package_name: "analyst".to_owned(),
        authority: Authority::DocumentOnly,
        agent_folder: folder.to_path_buf(),
        agent_folder_read_only: false,
        declaration: None,
        configured: None,
        tool_policy: None,
        approval_list: Vec::new(),
        package_read_only_tools: Vec::new(),
        tools: Vec::new(),
        unmapped_operations: Vec::new(),
        external: Vec::new(),
        commands: Vec::new(),
        reviewer: None,
    }
}

pub(crate) fn start_facts(run: RunId, agent: AgentInstanceId) -> RunStartFacts {
    RunStartFacts {
        run_id: run,
        agent_instance_id: agent,
        workspace_id: WorkspaceId::new(),
        crew_charter_id: None,
        command: None,
        text: String::new(),
        scope: None,
        asker: None,
        started_by: "person".to_owned(),
        started_by_id: "person:asha".to_owned(),
    }
}

// Gherkin (amendment January 2026): Happy path — the Hook Relay over the File Service lets start
// continue (DSK-WIRE-1, DSK-WIRE-2).
#[tokio::test]
async fn rows_up_to_the_hook_relay_are_built_with_their_owners_constructors() {
    let dir = TempDir::new("wiring");
    let mut runner = build_until(&dir, Row::CrewOrchestrator).await;
    let m = &runner.modules;
    assert!(m.backend.is_some() && m.journal.is_some() && m.profile.is_some());
    assert!(m.identity.is_some() && m.approval.is_some() && m.localisation.is_some());
    assert!(m.agent_folders.is_some() && m.resolver.is_some() && m.sharepoint.is_some());
    assert!(m.usage.is_some() && m.connector.is_some() && m.rules.is_some());
    assert!(m.brand.is_some() && m.checker.is_some() && m.installers.is_some());
    assert!(m.supervisor.is_some() && m.mcp_front.is_some() && m.inspector.is_some());
    assert!(m.files.is_some());
    let relay = m.hook_relay.clone().expect("row 3.23");
    assert!(relay.attributes_writes());
    // The late-bound handles are filled right after their rows.
    assert!(runner.late.rules.get().is_some());
    assert!(runner.late.supervisor.get().is_some());
    assert!(runner.late.inspector.get().is_some());
    assert!(runner.late.files.get().is_some());
    assert!(
        runner.late.memory.get().is_none(),
        "row 3.28 is not built yet"
    );
    // And the Crew Orchestrator of row 3.24 is constructed.
    runner
        .run(Step::Construct(Row::CrewOrchestrator))
        .await
        .expect("row 3.24");
    assert!(runner.modules.orchestrator.is_some());
    assert!(runner.late.orchestrator.get().is_some());
}

// Gherkin: Happy path — start wires every module and shows the window (the part with no window: the
// build with no switch reaches the end of step 3 on a temporary data root, every row in order).
#[tokio::test]
async fn start_wires_every_module_of_the_build_with_no_switch() {
    let dir = TempDir::new("wire-all");
    let mut runner = ModuleRunner::new(inputs(&dir));
    let mut constructed = Vec::new();
    for step in start_steps() {
        // DSK-REC-31: every step succeeds, the Ordin Host's registration included.
        runner
            .run(step)
            .await
            .unwrap_or_else(|e| panic!("{step:?}: {e:?}"));
        if let Step::Construct(row) = step {
            constructed.push(row);
        }
    }
    assert_eq!(constructed, crate::plan::rows(), "every row, in order");
    let m = &runner.modules;
    assert_eq!(
        m.providers,
        [
            Provider::Files,
            #[cfg(feature = "sandbox")]
            Provider::Sandbox,
            Provider::MemoryService,
            Provider::CraftToolkit,
            Provider::Projects,
            Provider::Knowledge,
            Provider::OrdinHost,
        ]
    );
    assert_eq!(
        m.adapters,
        [
            Adapter::Claude,
            #[cfg(feature = "copilot")]
            Adapter::Copilot,
        ]
    );
    assert!(
        m.mcp_started,
        "the MCP Front started after bind_ref_resolver"
    );
    assert!(m.orchestrator.is_some() && m.memory.is_some() && m.craft.is_some());
    assert!(m.confluence.is_some() && m.jira.is_some() && m.projects.is_some());
    assert!(m.claude.is_some() && m.workspaces.is_some() && m.catalogue.is_some());
    assert!(m.push.is_some() && m.knowledge.is_some() && m.packages.is_some());
    assert!(m.ordin_host.is_some() && m.refs.is_some() && m.home.is_some());
    assert!(m.setup.is_some() && m.service_api.is_some() && m.bridge.is_some());
    assert!(
        m.server_events.is_some(),
        "the sender of 3.49 is kept for step 4"
    );
    let late = &runner.late;
    assert!(late.memory.get().is_some() && late.workspaces.get().is_some());
    assert!(late.push.get().is_some() && late.knowledge.get().is_some());
    assert!(late.packages.get().is_some() && late.ordin_host.get().is_some());
    assert!(late.setup.get().is_some() && late.streams.get().is_some());
    // The registry is frozen: a provider after start is refused.
    let memory = m.memory.clone().expect("memory");
    assert!(m
        .mcp_front
        .clone()
        .expect("front")
        .register(memory.tool_provider())
        .is_err());
    runner.shutdown.cancel();
}

// Integration stub (§6.4): construction records the order of 3.1–3.49 with the real modules; a
// wiring step whose module is not built stops start naming the row.
#[tokio::test]
async fn a_wiring_step_before_its_row_stops_start() {
    let dir = TempDir::new("early-step");
    let mut runner = build_until(&dir, Row::CrewOrchestrator).await;
    let err = runner
        .run(Step::BindPartSource)
        .await
        .expect_err("the catalogue is not built");
    assert_eq!(err.code(), "DSK_START_FAILED");
    assert_eq!(err.module(), Some("ordin-catalogue"));
    let err = runner
        .run(Step::RegisterAdapter(Adapter::Claude))
        .await
        .expect_err("the adapter is not built");
    assert_eq!(err.module(), Some("ordin-claude-adapter"));
}

// Gherkin (amendment January 2026): Error path — a build whose Hook Relay attributes no writes does
// not start (DSK-WIRE-2).
#[tokio::test]
async fn error_path_a_hook_relay_with_no_places_stops_start() {
    let dir = TempDir::new("no-places");
    let runner = build_until(&dir, Row::HookRelay).await;
    let m = &runner.modules;
    let journal = m.journal.clone().expect("journal");
    let relay = HookRelay::new(
        journal.datastore().clone(),
        journal,
        m.rules.clone().expect("rules"),
        m.approval.clone().expect("approval"),
        m.supervisor.clone().expect("supervisor"),
        Arc::new(NoPlaces),
        HookRelayConfig {
            app_data_root: runner.inputs.paths.data_root.clone(),
            installation_id: runner.inputs.installation_id.to_string(),
        },
    );
    let err = check_attribution(&relay).expect_err("no attribution");
    crate::core::log_start_failed(&err);
    assert_eq!(err.code(), "DSK_START_FAILED");
    assert_eq!(err.module(), Some("ordin-hook-relay"));
    assert!(matches!(&err, StartError::ModuleFailed { code, .. } if code == WRITES_NOT_ATTRIBUTED));
    assert!(m.hook_relay.is_none(), "nothing after the relay was wired");
}

// §7.2 program-implemented trait: `SupervisorHost` (Session Supervisor). FU-087: `run_ended` calls
// the Rules Engine's `end_run`, so an ended run has no Rules record.
#[tokio::test]
async fn supervisor_host_run_ended_drops_the_rules_record() {
    let dir = TempDir::new("run-ended");
    let runner = build_until(&dir, Row::McpFront).await;
    let rules = runner.modules.rules.clone().expect("rules");
    let agent = AgentInstanceId::new();
    rules
        .load_rule_set(rule_set(agent, &dir.path().join("agent")))
        .expect("rule set");
    let run = RunId::new();
    rules
        .begin_run(start_facts(run, agent))
        .await
        .expect("begun");
    let host = DesktopSupervisorHost {
        late: runner.late.clone(),
    };
    assert_eq!(host.run_resuming(run).await, None, "a recorded run resumes");
    host.run_ended(run).await;
    assert!(matches!(
        rules.resume_run(run).await,
        Err(RulesError::RunUnknown)
    ));
    // A run with no Rules record resumes (SPEC-GAP-6).
    assert_eq!(host.run_resuming(RunId::new()).await, None);
    // The agent-profile-only methods.
    let target = RunTarget::AgentInstance {
        agent_instance_id: agent,
    };
    let expanded = host.expand_command(&target, "brief", "").await;
    assert_eq!(
        expanded.expect_err("agent profile only").code,
        "DSK_AGENT_PROFILE_ONLY"
    );
    assert!(host.run_controls(run).await.tools.is_empty());
    assert!(!host.retry_after_block(true, false));
    assert_eq!(
        host.check_content(run, ContentPoint::Input, "text").await,
        ContentCheck::Passed
    );
    assert_eq!(host.remote_route(ordin_types::RemoteAgentId::new()), None);
}

// §7.2 program-implemented trait: `RulesHost` (Rules Engine).
#[tokio::test]
async fn rules_host_answers_run_facts_over_the_supervisor_and_no_settings() {
    let dir = TempDir::new("rules-host");
    let runner = build_until(&dir, Row::McpFront).await;
    let host = DesktopRulesHost {
        agent_folders: runner.modules.agent_folders.clone().expect("folders"),
        localisation: runner.modules.localisation.clone().expect("localisation"),
        late: runner.late.clone(),
    };
    assert_eq!(host.runs_in_progress("/brief").await.expect("count"), 0);
    assert!(!host
        .paused_run_with_command(&["/brief".to_owned()], RunId::new())
        .await
        .expect("paused"));
    assert_eq!(host.run_ended("not an id").await.expect("fact"), None);
    assert_eq!(
        host.run_ended(&RunId::new().to_string())
            .await
            .expect("fact"),
        None
    );
    assert!(host.setting_value(None, "x").await.expect("none").is_null());
    assert!(host.setting_rules("scope").await.expect("none").is_empty());
    // The path facts need the run's agent folder; an unknown agent is no fact.
    let key = RunKey {
        run_id: RunId::new(),
        agent_instance_id: AgentInstanceId::new(),
        workspace_id: WorkspaceId::new(),
    };
    assert!(host.path_exists(&key, "STOP").await.is_err());
    let words = host.word(
        &key,
        &ordin_rules::Proposal::ShellCommand {
            command: "make".to_owned(),
            runs_in: "/work".to_owned(),
        },
    );
    // DSK-REC-1: catalogue words, never the proposal's own fields.
    assert_eq!(
        words.proposal.action.replace(['\u{2068}', '\u{2069}'], ""),
        "Run this command in /work"
    );
    assert_eq!(words.verb, "Run");
    assert_eq!(words.content_label.as_deref(), Some("See the command"));
}

// §7.2 program-implemented trait: `AgentWrites` (Hook Relay) over the File Service (DSK-WIRE-1).
#[tokio::test]
async fn agent_writes_open_and_end_claims_through_the_file_service() {
    let dir = TempDir::new("agent-writes");
    let runner = build_until(&dir, Row::HookRelay).await;
    let writes = FileAgentWrites {
        files: runner.modules.files.clone().expect("files"),
    };
    assert!(writes.attributes_writes());
    let run = RunId::new();
    // A wide claim lasts until the run ends: nothing to settle after the call.
    let claim = writes
        .open(run, "Bash", WriteTarget::Anywhere)
        .await
        .expect("recorded");
    assert_eq!(claim, None);
    // A path in no place: nothing to settle either.
    let claim = writes
        .open(
            run,
            "Write",
            WriteTarget::File(dir.path().join("elsewhere.txt")),
        )
        .await
        .expect("recorded");
    assert_eq!(claim, None);
    writes.run_ended(run).await.expect("ended");
}

#[test]
fn the_protected_roots_end_with_the_data_root() {
    let home = std::path::Path::new("/home/asha");
    let data = std::path::Path::new("/data/ordin");
    let roots = protected_roots(home, data);
    assert_eq!(roots.last().map(|p| p.as_path()), Some(data));
    assert!(roots.len() > 1);
}

// Integration stub (§6.4): a fake `ServiceApi` call reaches `DesktopServiceApi` through the catalogue's
// dispatcher, and a row whose module this program does not answer is `SVC_NOT_IN_THIS_VERSION`.
#[tokio::test]
async fn service_api_answers_its_own_commands_and_delegates_to_the_modules() {
    let dir = TempDir::new("service-api");
    let runner = build_until(&dir, Row::Brand).await;
    let m = &runner.modules;
    let sentences = Arc::new(DesktopErrorSentences {
        localisation: m.localisation.clone().expect("localisation"),
    });
    let api = DesktopServiceApi::new(
        ApiModules {
            backend: m.backend,
            journal: m.journal.clone(),
            approval: m.approval.clone(),
            localisation: m.localisation.clone(),
            ..ApiModules::default()
        },
        sentences,
    );
    let ctx = context();
    let hello = dispatch(
        &api,
        &ctx,
        command("service.hello").expect("row"),
        json!({}),
    )
    .await
    .expect("hello");
    assert_eq!(hello["backend"], json!(m.backend.expect("backend")));
    assert_eq!(hello["profile"], json!("desktop"));
    // DSK-REC-5: the catalogue's `desktop-backend-name` in the active locale.
    assert_eq!(hello["backend_name"], json!("This computer"));
    assert_eq!(hello["groups"], json!(crate::groups::groups()));
    // Delegated to the Approval Service: an unknown approval is the module's own code.
    let missing = dispatch(
        &api,
        &ctx,
        command("approval.get").expect("row"),
        json!({ "approval_id": ordin_types::ApprovalId::new() }),
    )
    .await
    .expect_err("unknown approval");
    assert!(missing.code.starts_with("APPR_"), "{}", missing.code);
    assert!(!missing.message.is_empty());
    let pending = dispatch(
        &api,
        &ctx,
        command("approval.list_pending").expect("row"),
        json!({ "workspace_id": null, "cursor": null }),
    )
    .await
    .expect("empty list");
    assert_eq!(pending["items"], json!([]));
    // DSK-REC-32 (LOC-REC-1): a first start follows the operating system.
    let settings = dispatch(
        &api,
        &ctx,
        command("localisation.get_settings").expect("row"),
        json!({}),
    )
    .await
    .expect("settings");
    assert_eq!(settings["following_operating_system"], json!(true));
    assert_eq!(settings["active"]["source"], json!("operating_system"));
    assert!(settings["offered"].is_array());
    // The person's choice is not the operating system's.
    let localisation = m.localisation.clone().expect("localisation");
    let tag = settings["active"]["locale"]
        .as_str()
        .expect("a tag")
        .to_owned();
    localisation.set_locale(Some(&tag)).await.expect("chosen");
    let settings = dispatch(
        &api,
        &ctx,
        command("localisation.get_settings").expect("row"),
        json!({}),
    )
    .await
    .expect("settings");
    assert_eq!(settings["following_operating_system"], json!(false));
    assert_eq!(settings["active"]["source"], json!("person"));
    localisation
        .set_locale(None)
        .await
        .expect("following again");
    // A row of an answered group whose module is not answered here yet.
    let later = dispatch(
        &api,
        &ctx,
        command("workspaces.list").expect("row"),
        json!({ "filter": "active" }),
    )
    .await
    .expect_err("not answered");
    assert_eq!(later.code, "SVC_NOT_IN_THIS_VERSION");
    // Every row of the catalogue has a method (the trait is generated from the catalogue).
    assert!(CATALOGUE.len() > 400);
}

// §7.2: the Service API of row 3.48 delegates the rows of the build with no switch to their owners,
// through the real modules.
#[tokio::test]
async fn service_api_delegates_rows_to_the_owning_modules() {
    let dir = TempDir::new("service-api-delegates");
    let runner = wire_all(&dir).await;
    let api = runner.modules.service_api.clone().expect("row 3.48");
    let ctx = context();
    let call = |name: &'static str, input: serde_json::Value| {
        let api = api.clone();
        let ctx = ctx.clone();
        async move { dispatch(&*api, &ctx, command(name).expect("row"), input).await }
    };
    let created = call("workspaces.create", json!({ "name": "Northwind" }))
        .await
        .expect("created");
    let id = created["id"].clone();
    let listed = call("workspaces.list", json!({ "filter": "active" }))
        .await
        .expect("listed");
    assert!(listed
        .as_array()
        .expect("a list")
        .iter()
        .any(|w| w["id"] == id));
    let renamed = call(
        "workspaces.rename",
        json!({ "workspace_id": id, "name": "Northwind bids" }),
    )
    .await
    .expect("renamed");
    assert_eq!(renamed["name"], json!("Northwind bids"));
    let links = call("workspaces.links", json!({ "workspace_id": id }))
        .await
        .expect("links");
    assert_eq!(links, json!([]));
    let emptied = call("workspaces.empty_bin", json!({}))
        .await
        .expect("emptied");
    assert_eq!(emptied["deleted"], json!(0));
    call("home.view", json!({ "workspace_id": null }))
        .await
        .expect("home");
    call(
        "search.query",
        json!({ "query": "Northwind", "current_workspace_id": null }),
    )
    .await
    .expect("search");
    // An unknown run or crew run is the owner's own code.
    let stopped = call("runs.stop", json!({ "run_id": RunId::new() }))
        .await
        .expect_err("unknown run");
    assert!(stopped.code.starts_with("SUP_"), "{}", stopped.code);
    let killed = call(
        "crews.stop",
        json!({ "crew_run_id": ordin_types::CrewRunId::new() }),
    )
    .await
    .expect_err("unknown crew run");
    assert!(killed.code.starts_with("CREW_"), "{}", killed.code);
    // A row whose owner needs the shell is not answered by the core.
    let quit = call("app.quit", json!({})).await.expect_err("the shell");
    assert_eq!(quit.code, "SVC_NOT_IN_THIS_VERSION");
    // The rows delegated by hand on 2026-01-26, each reaching its owner.
    let days = call("history.set_days", json!({ "days": 30 }))
        .await
        .expect("set");
    assert_eq!(days["days"], json!(30));
    let settings = call("usage.get_settings", json!({}))
        .await
        .expect("usage settings");
    assert!(settings.is_object());
    let plan = call("plans.get", json!({ "run_id": RunId::new() }))
        .await
        .expect("an empty plan");
    assert_eq!(plan["items"], json!([]));
    let status = call("setup.status", json!({})).await.expect("setup status");
    assert!(status["needs_guided"].is_boolean());
    assert!(status["attachments"].is_array());
    let arrived = call(
        "journal.mark_arrived",
        json!({ "action_key": ordin_types::ExternalActionKey::new() }),
    )
    .await
    .expect_err("an unknown action");
    assert!(arrived.code.starts_with("JRNL_"), "{}", arrived.code);
    let conversions = call("craft.conversions", json!({}))
        .await
        .expect("conversions");
    assert!(conversions["word"].is_boolean() && conversions["slides"].is_boolean());
    let lines = call("diagnostics.lines", json!({})).await.expect("lines");
    assert!(lines["items"].is_array());
    let messages = call("localisation.list_messages", json!({ "locale": "en-GB" })).await;
    if let Err(error) = &messages {
        assert!(error.code.starts_with("LOC_"), "{}", error.code);
    }
    // The commands the screens wait on (ServiceApi.md SVC-SCR-1 to SVC-SCR-5, DSK-REC-40).
    let version = call(
        "files.read_version",
        json!({ "file_version_id": ordin_types::FileVersionId::new() }),
    )
    .await
    .expect_err("an unknown version");
    assert_eq!(version.code, "FILE_VERSION_NOT_FOUND");
    let artefact = call(
        "artefacts.get",
        json!({ "artefact_id": ordin_types::ArtefactId::new() }),
    )
    .await
    .expect("no row");
    assert_eq!(artefact, json!({}));
    let project = call(
        "projects.get",
        json!({ "project_id": ordin_types::ProjectId::new() }),
    )
    .await
    .expect_err("an unknown project");
    assert_eq!(project.code, "PROJ_NOT_FOUND");
    let items = call(
        "workspaces.working_copy_items",
        json!({ "source_link_id": ordin_types::SourceLinkId::new() }),
    )
    .await
    .expect_err("an unknown link");
    assert!(items.code.starts_with("WS_"), "{}", items.code);
    let held = call("memory.held_writes", json!({ "workspace_id": id })).await;
    match held {
        Ok(list) => assert_eq!(list, json!([])),
        Err(error) => assert!(error.code.starts_with("MEM_"), "{}", error.code),
    }
    // DSK-REC-42: the tree of a new workspace.
    let tree = call("workspaces.tree", json!({ "workspace_id": id }))
        .await
        .expect("the tree");
    assert_eq!(tree["workspace"]["id"], id);
    assert_eq!(tree["running"], json!([]));
    assert_eq!(tree["dashboards"], json!([]));
    assert_eq!(tree["workflows"], json!([]));
    assert_eq!(tree["waiting_for_person"], json!(0));
    let unknown = call(
        "workspaces.tree",
        json!({ "workspace_id": ordin_types::WorkspaceId::new() }),
    )
    .await
    .expect_err("an unknown workspace");
    assert!(unknown.code.starts_with("WS_"), "{}", unknown.code);
    // SVC-SCR-9: the owner's read of a charter that does not exist.
    let charter = call(
        "crews.charter",
        json!({ "crew_charter_id": ordin_types::CrewCharterId::new() }),
    )
    .await
    .expect_err("an unknown charter");
    assert_eq!(charter.code, "OH_CHARTER_UNKNOWN");
    // DSK-REC-43: an exact version is refused before any dialog.
    let copy = call(
        "files.save_copy",
        json!({
            "backend": runner.modules.backend.expect("backend"),
            "workspace_id": id,
            "item": { "kind": "file_version", "file_version_id": ordin_types::FileVersionId::new() }
        }),
    )
    .await
    .expect_err("refused");
    assert_eq!(copy.code, "SVC_INPUT_INVALID");
    // DSK-REC-41: the Journal refuses an export of no run.
    let export = call("diagnostics.support_export", json!({ "run_ids": [] }))
        .await
        .expect_err("no run");
    assert_eq!(export.code, "JRNL_EXPORT_INVALID");
    runner.shutdown.cancel();
}

// Scenario: Happy path — the Ordin Host's provider registers on the desktop (OrdinHost.md OH-REC-4,
// OrdinDesktop.md DSK-REC-31): the MCP Front of the desktop profile accepts it at start.
#[tokio::test]
async fn start_passes_the_ordin_hosts_registration() {
    let dir = TempDir::new("ordin-host-registers");
    let mut runner = ModuleRunner::new(inputs(&dir));
    for step in start_steps() {
        let result = runner.run(step).await;
        if step == Step::RegisterProvider(Provider::OrdinHost) {
            result.expect("the MCP Front accepts the Ordin Host's provider");
            break;
        }
        result.unwrap_or_else(|e| panic!("{step:?}: {e:?}"));
    }
    assert!(runner.modules.providers.contains(&Provider::OrdinHost));
}

#[test]
fn scenario_the_feed_address_is_a_build_input_and_only_https_counts() {
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-46
    let set = crate::wiring::installer_defaults_from(Some("https://example.test/latest.json"));
    assert_eq!(
        set.update_feed_url.as_deref(),
        Some("https://example.test/latest.json")
    );
    assert!(set.profile_url.is_none() && set.profile_signing_keys.is_empty());
    assert!(crate::wiring::installer_defaults_from(None)
        .update_feed_url
        .is_none());
    assert!(
        crate::wiring::installer_defaults_from(Some("http://example.test/x"))
            .update_feed_url
            .is_none()
    );
    assert!(crate::wiring::installer_defaults_from(Some(""))
        .update_feed_url
        .is_none());
}
