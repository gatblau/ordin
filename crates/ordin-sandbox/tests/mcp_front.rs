//! Integration with the real MCP Front and the real Rules Engine (Playbook §7.2, `ToolProvider`
//! row): the program registers the Sandbox's provider before the MCP Front starts; a session is
//! offered `python_sandbox`; a call the engine allows runs Python and indexes its artefact; a call
//! the engine denies never reaches the guest; agent-written code reaches an ordin tool through the
//! MCP Front's `ToolDispatch`.

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_mcp_front::{
    McpFront, McpFrontConfig, ProgramProfile, SessionGrant, ToolCaller, ToolDispatch,
};
use ordin_organisation_profile::{
    Clock, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
};
use ordin_rules::{
    CardWording, ControlLine, FactError, InstanceRuleSet, MessageRule, ProgramKind, Proposal,
    RulesConfig, RulesEngine, RulesHost, RunKey, RunStartFacts,
};
use ordin_sandbox::{Sandbox, SandboxDeps};
use ordin_service_api::blocks::{EntityRef, PlanState, RefResolver, StandardBlockCheck};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::identity::HarnessKind;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{AgentInstanceId, BackendId, RunId, WorkspaceId};
use serde_json::{json, Value};
use support::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// The program's facts for the Rules Engine; nothing in these tests reads them.
struct Host;

#[async_trait]
impl RulesHost for Host {
    async fn path_exists(&self, _run: &RunKey, _path: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn path_contains(
        &self,
        _run: &RunKey,
        _path: &str,
        _text: &str,
    ) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn read_text(&self, _run: &RunKey, _path: &str) -> Result<Option<String>, FactError> {
        Ok(None)
    }
    async fn append_line(&self, _run: &RunKey, _path: &str, _line: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn remove(&self, _run: &RunKey, _path: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn runs_in_progress(&self, _command: &str) -> Result<u32, FactError> {
        Ok(1)
    }
    async fn paused_run_with_command(
        &self,
        _commands: &[String],
        _except: RunId,
    ) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn run_ended(&self, _run_id: &str) -> Result<Option<bool>, FactError> {
        Ok(None)
    }
    async fn setting_value(&self, _scope: Option<&str>, _name: &str) -> Result<Value, FactError> {
        Ok(Value::Null)
    }
    async fn setting_rules(&self, _scope: &str) -> Result<Vec<(String, MessageRule)>, FactError> {
        Ok(Vec::new())
    }
    fn word(&self, _run: &RunKey, _proposal: &Proposal) -> CardWording {
        let affects = vec![AffectedItem::Record {
            label: "The run's work".to_owned(),
        }];
        CardWording {
            question: "Allow this action?".to_owned(),
            verb: "Allow".to_owned(),
            where_line: "This computer".to_owned(),
            affects: affects.clone(),
            changes: "The action runs.".to_owned(),
            content_label: None,
            proposal: ActionProposal {
                action: "Run a tool".to_owned(),
                destination: "This computer".to_owned(),
                affects,
                consequence: "The action runs.".to_owned(),
            },
        }
    }
    fn plain_line(&self, line: &ControlLine) -> String {
        format!("{} applies.", line.name)
    }
    fn content_checker(&self) -> Option<Arc<dyn ordin_rules::ContentChecker>> {
        None
    }
}

struct Resolver;

#[async_trait]
impl RefResolver for Resolver {
    async fn content(&self, _w: WorkspaceId, _item: &ordin_types::wire::ContentRef) -> bool {
        true
    }
    async fn email_message(&self, _w: WorkspaceId, _l: ordin_types::EmailLinkId, _m: &str) -> bool {
        true
    }
    async fn entity(&self, _w: WorkspaceId, _e: EntityRef) -> bool {
        true
    }
    async fn run_agent_name(&self, _run: RunId) -> Option<String> {
        Some("Analyst".to_owned())
    }
    async fn plan_state(&self, _run_id: RunId) -> PlanState {
        PlanState {
            versions: 0,
            current_len: 0,
        }
    }
    fn approval_verbs(&self) -> &[String] {
        &[]
    }
}

struct Env {
    _dir: TempDir,
    journal: Journal,
    engine: Arc<RulesEngine>,
    front: McpFront,
    sandbox: Arc<Sandbox>,
}

fn app_root() -> PathBuf {
    PathBuf::from("/Users/asha/Library/Application Support/ordin")
}

/// The program's start order: Journal, Rules Engine, MCP Front, Sandbox; the Sandbox's provider
/// and a sandbox-callable `mail` provider registered before the MCP Front starts.
async fn env() -> Env {
    capture_logs();
    let dir = TempDir::new();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("data").join("journal.db"),
        backup_dir: dir.0.join("data").join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry
        .register(ApprovalService::migrations())
        .expect("approval");
    registry.register(RulesEngine::migrations()).expect("rules");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the migrations");
    let journal = Journal::start(
        store.clone(),
        JournalOptions {
            automatic_pruning: false,
            prune_interval: Duration::from_secs(24 * 3600),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .expect("start the journal");
    let shared = Arc::new(journal.clone());
    let approvals = ApprovalService::new(
        store.clone(),
        shared.clone(),
        ApprovalConfig {
            backend: BackendId(journal.backend_uuid()),
            local_computer_name: "Asha's laptop".to_owned(),
            approvers: None,
        },
    )
    .await
    .expect("approval service");
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::NotUsed,
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(WallClock),
            },
        )
        .await
        .expect("profile service"),
    );
    let engine = RulesEngine::new(
        store.clone(),
        shared,
        profile,
        approvals,
        Arc::new(Host),
        RulesConfig {
            ordin_servers: ["ordin", "work", "sandbox", "mail"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            ordin_server_prefixes: Vec::new(),
            protected_roots: vec![PathBuf::from("/System"), PathBuf::from("/etc")],
            app_data_root: app_root(),
            program: ProgramKind::Desktop,
            instance_name: "local".to_owned(),
        },
    );
    let front = McpFront::new(
        McpFrontConfig {
            app_data_root: dir.0.clone(),
            installation_id: uuid::Uuid::now_v7(),
            hook_program: PathBuf::from("/Applications/ordin.app/Contents/MacOS/ordin-hook"),
            ..McpFrontConfig::default()
        },
        engine.clone(),
        journal.clone(),
        Arc::new(StandardBlockCheck),
        None,
    );

    let ident = identity(&dir).await;
    let grants = Arc::new(Grants::default());
    grants.set(Vec::new());
    let places = Arc::new(Places::new(dir.0.join("agent")));
    let sandbox = Sandbox::start(SandboxDeps {
        dispatch: Arc::new(front.clone()) as Arc<dyn ToolDispatch>,
        grants,
        identity: ident,
        journal: journal.clone(),
        place_files: places,
        session_idle: Duration::from_secs(1800),
        data_root: dir.0.join("app"),
        log_level: "info".to_owned(),
    })
    .await;
    front
        .register(sandbox.tool_provider())
        .expect("register the sandbox");
    // A sandbox-callable tool for agent-written code, served by the test's own `mail` provider.
    front
        .register(Arc::new(MailProvider))
        .expect("register mail");
    front
        .bind_ref_resolver(Arc::new(Resolver))
        .expect("resolver");
    front.start(CancellationToken::new()).await.expect("start");
    Env {
        _dir: dir,
        journal,
        engine,
        front,
        sandbox,
    }
}

/// A `mail` server with one sandbox-callable tool answering a fixed hit count.
struct MailProvider;

#[async_trait]
impl ordin_mcp_front::ToolProvider for MailProvider {
    fn provider_id(&self) -> &'static str {
        "mail-test"
    }
    fn servers(&self) -> Vec<ordin_mcp_front::ServerSpec> {
        let (key, spec) = tool("mail", "email_search");
        vec![ordin_mcp_front::ServerSpec {
            name: key.server,
            tools: vec![spec],
        }]
    }
    async fn offer(
        &self,
        _grant: &SessionGrant,
    ) -> Result<Vec<ordin_mcp_front::ToolKey>, ordin_mcp_front::ToolFailure> {
        Ok(vec![tool("mail", "email_search").0])
    }
    fn check_arguments(
        &self,
        _key: &ordin_mcp_front::ToolKey,
        _arguments: &Value,
    ) -> Result<(), ordin_mcp_front::ArgumentProblem> {
        Ok(())
    }
    async fn call(
        &self,
        _ctx: ordin_mcp_front::ToolCallContext,
        _key: &ordin_mcp_front::ToolKey,
        _arguments: Value,
    ) -> Result<ordin_mcp_front::ToolOutput, ordin_mcp_front::ToolFailure> {
        Ok(ordin_mcp_front::ToolOutput {
            text: "{\"hits\":3}".to_owned(),
            structured: Some(json!({"hits": 3})),
        })
    }
    async fn session_closed(&self, _run_id: RunId) {}
}

impl Env {
    fn agent(&self, deny: &[&str]) -> AgentInstanceId {
        let id = AgentInstanceId::new();
        self.engine
            .load_rule_set(InstanceRuleSet {
                agent_instance_id: id,
                package_name: "analyst".to_owned(),
                authority: Authority::SystemChanging,
                agent_folder: app_root().join("agents").join("eng-1"),
                agent_folder_read_only: false,
                declaration: None,
                configured: None,
                tool_policy: Some(ordin_rules::AgentToolPolicy {
                    prohibited_verbs: deny.iter().map(|s| (*s).to_owned()).collect(),
                    ..Default::default()
                }),
                approval_list: Vec::new(),
                package_read_only_tools: Vec::new(),
                tools: Vec::new(),
                unmapped_operations: Vec::new(),
                external: Vec::new(),
                commands: Vec::new(),
                reviewer: None,
            })
            .expect("the set loads");
        id
    }

    async fn open(&self, deny: &[&str]) -> (RunId, ordin_mcp_front::SessionMcpConfig) {
        let agent = self.agent(deny);
        let run = RunId::new();
        let workspace = WorkspaceId::new();
        self.engine
            .begin_run(RunStartFacts {
                run_id: run,
                agent_instance_id: agent,
                workspace_id: workspace,
                crew_charter_id: None,
                command: None,
                text: String::new(),
                scope: None,
                asker: None,
                started_by: "person".to_owned(),
                started_by_id: "person:asha".to_owned(),
            })
            .await
            .expect("the run begins");
        let config = self
            .front
            .open_session(SessionGrant {
                run_id: run,
                workspace_id: workspace,
                agent_instance_id: agent,
                harness: HarnessKind::ClaudeCode,
                profile: ProgramProfile::Desktop,
                caller: ToolCaller::Agent,
                scope_id: None,
                coding_task_id: None,
                only_servers: None,
            })
            .await
            .expect("open_session");
        (run, config)
    }
}

/// A bridge connection to one server of a session, speaking JSON lines.
struct Bridge {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl Bridge {
    async fn connect(
        run: RunId,
        config: &ordin_mcp_front::SessionMcpConfig,
        server: &str,
    ) -> Bridge {
        let env = &config.servers[0].env;
        let stream = tokio::net::UnixStream::connect(&env["ORDIN_MCP_ENDPOINT"])
            .await
            .expect("connect");
        let (r, w) = stream.into_split();
        let mut b = Bridge {
            reader: BufReader::new(r),
            writer: w,
        };
        b.send(json!({"protocol": 1, "session": run.to_string(), "server": server, "token": env["ORDIN_MCP_TOKEN"]}))
            .await;
        assert_eq!(b.recv().await, json!({"kind": "ready"}));
        b
    }

    async fn send(&mut self, value: Value) {
        self.writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .expect("write");
    }

    async fn recv(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(120), self.reader.read_line(&mut line))
            .await
            .expect("a line in time")
            .expect("read");
        serde_json::from_str(line.trim()).expect("JSON")
    }

    async fn call(&mut self, id: i64, tool: &str, arguments: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": tool, "arguments": arguments}}))
            .await;
        loop {
            let line = self.recv().await;
            if line.get("id") == Some(&json!(id)) {
                return line;
            }
        }
    }
}

fn text_of(answer: &Value) -> String {
    answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

fn is_error(answer: &Value) -> bool {
    answer["result"]["isError"].as_bool().unwrap_or(false)
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sandbox_is_offered_answers_an_allowed_call_and_is_refused_a_denied_one() {
    let env = env().await;

    // Allowed: Python runs, its output is returned and its file indexed as an artefact.
    let (run, config) = env.open(&[]).await;
    let offered = env.front.offered_tools(run).expect("offered");
    assert!(offered
        .iter()
        .any(|t| t.server.as_str() == "sandbox" && t.tool == "python_sandbox"));
    let mut bridge = Bridge::connect(run, &config, "sandbox").await;
    let answer = bridge
        .call(
            1,
            "python_sandbox",
            // A file given as content, so the run has a `/work` folder (C.6).
            json!({"code": "r = ordin_tools.email_search(query='Kestrel')\nopen('/work/out.txt','w').write(str(r['parts'][0]['json']['hits']))\nprint('done')",
                   "files": [{"path": "seed.txt", "contentBase64": "eA=="}]}),
        )
        .await;
    assert!(!is_error(&answer), "{answer}");
    let result: Value = serde_json::from_str(&text_of(&answer)).expect("result JSON");
    assert_eq!(result["stdout"], "done\n", "{result}");
    assert_eq!(result["changes"][0]["path"], "out.txt");
    let artefacts = env
        .journal
        .list_artefacts(ordin_journal::ArtefactFilter::default(), None)
        .await
        .unwrap()
        .items;
    assert_eq!(artefacts.len(), 1);
    assert_eq!(artefacts[0].path, format!("sandbox-output/{run}/out.txt"));

    // Denied: the engine refuses `python_sandbox`; the guest never runs.
    let before = env
        .journal
        .list_entries(
            ordin_journal::EntryFilter {
                run_id: None,
                kind: Some("sandbox_run".to_owned()),
            },
            None,
        )
        .await
        .unwrap()
        .items
        .len();
    let (denied_run, denied_config) = env.open(&["python_sandbox"]).await;
    let mut denied = Bridge::connect(denied_run, &denied_config, "sandbox").await;
    let refused = denied
        .call(2, "python_sandbox", json!({"code": "print(1)"}))
        .await;
    assert!(is_error(&refused), "{refused}");
    let after = env
        .journal
        .list_entries(
            ordin_journal::EntryFilter {
                run_id: None,
                kind: Some("sandbox_run".to_owned()),
            },
            None,
        )
        .await
        .unwrap()
        .items
        .len();
    assert_eq!(before, after, "no sandbox run for the denied call");

    // `shell_sandbox` is not offered (no `extra-guests`).
    assert!(!offered.iter().any(|t| t.tool == "shell_sandbox"));
    env.sandbox.shutdown().await;
}
