//! Integration with the real MCP Front and Rules Engine (plan 01 Phase 7, Playbook 7.2): server
//! `memory` registers, a crew member's session is offered `recall` and `remember`, and both run
//! through the Rules Engine. A denied call never reaches the provider. SQLite on a temporary file;
//! the models are the deterministic ones, and nothing touches the network.
#![cfg(unix)]

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry,
    ModuleMigrations, SystemClock, MIGRATIONS,
};
use ordin_mcp_front::{McpFront, McpFrontConfig, ProgramProfile, SessionGrant, ToolCaller};
use ordin_memory_service::{MemoryCaller, MemoryService, ProgramKind, RecallInput, WiderMode};
use ordin_organisation_profile::{
    Clock, InstallerSettings, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
    MODULE_NAME, SQLITE_MIGRATIONS,
};
use ordin_rules::{
    AgentToolPolicy, CardWording, ControlLine, FactError, InstanceRuleSet, MessageRule,
    ProgramKind as RulesProgram, Proposal, RulesConfig, RulesEngine, RulesHost, RunKey,
    RunStartFacts,
};
use ordin_service_api::blocks::{EntityRef, PlanState, RefResolver, StandardBlockCheck};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::identity::HarnessKind;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{AgentInstanceId, BackendId, CrewRunId, EmailLinkId, RunId, WorkspaceId};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio_util::sync::CancellationToken;

/// A folder removed when dropped. A short name: a Unix socket path is limited to about 100 bytes.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let dir = std::env::temp_dir().join(format!("omi-{}", &id[..8]));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(std::fs::canonicalize(&dir).expect("canonical temporary folder"))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

struct Host;

#[async_trait]
impl RulesHost for Host {
    async fn path_exists(&self, _: &RunKey, _: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn path_contains(&self, _: &RunKey, _: &str, _: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn read_text(&self, _: &RunKey, _: &str) -> Result<Option<String>, FactError> {
        Ok(None)
    }
    async fn append_line(&self, _: &RunKey, _: &str, _: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn remove(&self, _: &RunKey, _: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn runs_in_progress(&self, _: &str) -> Result<u32, FactError> {
        Ok(1)
    }
    async fn paused_run_with_command(&self, _: &[String], _: RunId) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn run_ended(&self, _: &str) -> Result<Option<bool>, FactError> {
        Ok(None)
    }
    async fn setting_value(&self, _: Option<&str>, _: &str) -> Result<Value, FactError> {
        Ok(Value::Null)
    }
    async fn setting_rules(&self, _: &str) -> Result<Vec<(String, MessageRule)>, FactError> {
        Ok(Vec::new())
    }
    fn word(&self, _: &RunKey, _: &Proposal) -> CardWording {
        let affects = vec![AffectedItem::Record {
            label: "A record".to_owned(),
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
}

struct Resolver;

#[async_trait]
impl RefResolver for Resolver {
    async fn content(&self, _: WorkspaceId, _: &ordin_types::wire::ContentRef) -> bool {
        true
    }
    async fn email_message(&self, _: WorkspaceId, _: EmailLinkId, _: &str) -> bool {
        true
    }
    async fn entity(&self, _: WorkspaceId, _: EntityRef) -> bool {
        true
    }
    async fn run_agent_name(&self, _: RunId) -> Option<String> {
        Some("Analyst".to_owned())
    }
    async fn plan_state(&self, _: RunId) -> PlanState {
        PlanState {
            versions: 0,
            current_len: 0,
        }
    }
    fn approval_verbs(&self) -> &[String] {
        &[]
    }
}

struct Stack {
    dir: TempDir,
    engine: Arc<RulesEngine>,
    front: McpFront,
    memory: MemoryService,
    store: Datastore,
    shutdown: CancellationToken,
}

impl Stack {
    async fn start() -> Stack {
        let dir = TempDir::new();
        let data_root = dir.0.join("data");
        std::fs::create_dir_all(&data_root).expect("data root");
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: data_root.join("journal.db"),
            backup_dir: data_root.join("backups"),
        })
        .await
        .expect("open the SQLite datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).expect("journal");
        registry
            .register(ModuleMigrations {
                module: MODULE_NAME,
                sqlite: SQLITE_MIGRATIONS,
                postgres: &[],
            })
            .expect("profile");
        registry
            .register(ApprovalService::migrations())
            .expect("approval");
        registry.register(RulesEngine::migrations()).expect("rules");
        registry
            .register(MemoryService::migrations())
            .expect("memory");
        registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .expect("apply the migrations");
        let pool = store.sqlite().expect("SQLite").clone();
        let journal = Arc::new(
            Journal::start(
                store.clone(),
                JournalOptions {
                    automatic_pruning: false,
                    prune_interval: Duration::from_secs(24 * 3600),
                    clock: Arc::new(SystemClock),
                },
            )
            .await
            .expect("start the journal"),
        );
        let approvals = ApprovalService::new(
            store.clone(),
            journal.clone(),
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
                ProfileMode::Read {
                    pool,
                    data_root: data_root.clone(),
                    http: reqwest::Client::new(),
                    build_defaults: InstallerSettings::default(),
                },
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
            journal.clone(),
            profile,
            approvals,
            Arc::new(Host),
            RulesConfig {
                ordin_servers: vec!["ordin".to_owned(), "work".to_owned(), "memory".to_owned()],
                ordin_server_prefixes: Vec::new(),
                protected_roots: vec![dir.0.join("protected")],
                app_data_root: data_root.clone(),
                program: RulesProgram::Desktop,
                instance_name: "local".to_owned(),
            },
        );
        let shutdown = CancellationToken::new();
        let memory = MemoryService::start_with(
            support::config(ProgramKind::Desktop),
            journal.clone(),
            shutdown.clone(),
            support::seams(),
        )
        .await
        .expect("the memory service");
        let front = McpFront::new(
            McpFrontConfig {
                app_data_root: dir.0.clone(),
                installation_id: uuid::Uuid::now_v7(),
                hook_program: PathBuf::from("/Applications/ordin.app/Contents/MacOS/ordin-hook"),
                ..McpFrontConfig::default()
            },
            engine.clone(),
            journal.as_ref().clone(),
            Arc::new(StandardBlockCheck),
            None,
        );
        front.register(memory.tool_provider()).expect("registers");
        front.bind_ref_resolver(Arc::new(Resolver)).expect("binds");
        front.start(CancellationToken::new()).await.expect("starts");
        Stack {
            dir,
            engine,
            front,
            memory,
            store,
            shutdown,
        }
    }

    fn agent(&self, deny: &[&str]) -> AgentInstanceId {
        let id = AgentInstanceId::new();
        self.engine
            .load_rule_set(InstanceRuleSet {
                agent_instance_id: id,
                package_name: "analyst".to_owned(),
                authority: Authority::SystemChanging,
                agent_folder: self.dir.0.join("agents/eng-1"),
                agent_folder_read_only: false,
                declaration: None,
                configured: None,
                tool_policy: Some(AgentToolPolicy {
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

    async fn open(&self, workspace: WorkspaceId, deny: &[&str]) -> Session {
        let agent = self.agent(deny);
        let run = RunId::new();
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
        let grant = SessionGrant {
            run_id: run,
            workspace_id: workspace,
            agent_instance_id: agent,
            harness: HarnessKind::ClaudeCode,
            profile: ProgramProfile::Desktop,
            caller: ToolCaller::CrewMember {
                crew_run_id: CrewRunId::new(),
            },
            scope_id: None,
            coding_task_id: None,
            only_servers: None,
        };
        let config = self.front.open_session(grant).await.expect("open_session");
        let launch = config
            .servers
            .iter()
            .find(|s| s.name.as_str() == "memory")
            .expect("the server is launched");
        Session {
            run,
            token: launch.env["ORDIN_MCP_TOKEN"].clone(),
            endpoint: launch.env["ORDIN_MCP_ENDPOINT"].clone(),
        }
    }
}

struct Session {
    run: RunId,
    token: String,
    endpoint: String,
}

struct Bridge {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl Session {
    async fn connect(&self) -> Bridge {
        let stream = UnixStream::connect(&self.endpoint)
            .await
            .expect("the socket");
        let (read, write) = stream.into_split();
        let mut bridge = Bridge {
            reader: BufReader::new(read),
            writer: write,
        };
        bridge
            .send(json!({
                "protocol": 1,
                "session": self.run.to_string(),
                "server": "memory",
                "token": self.token,
            }))
            .await;
        assert_eq!(bridge.recv().await, json!({"kind": "ready"}));
        bridge
    }
}

impl Bridge {
    async fn send(&mut self, value: Value) {
        self.writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .expect("write");
    }

    async fn recv(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(20), self.reader.read_line(&mut line))
            .await
            .expect("a line within the wait")
            .expect("read");
        serde_json::from_str(line.trim()).expect("a JSON line")
    }

    async fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let line = self.recv().await;
            if line.get("id") == Some(&json!(id)) {
                return line;
            }
        }
    }

    async fn tools(&mut self) -> Vec<Value> {
        let answer = self.request(1000, "tools/list", json!({})).await;
        answer["result"]["tools"]
            .as_array()
            .expect("tools array")
            .clone()
    }

    async fn call(&mut self, id: i64, tool: &str, arguments: Value) -> Value {
        self.request(
            id,
            "tools/call",
            json!({"name": tool, "arguments": arguments}),
        )
        .await
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

async fn kept_rows(store: &Datastore) -> i64 {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_fact_refs WHERE state = 'kept'")
        .fetch_one(store.sqlite().expect("SQLite"))
        .await
        .expect("count");
    n
}

#[tokio::test]
async fn remember_and_recall_run_through_the_rules_engine_and_a_denied_call_writes_nothing() {
    let stack = Stack::start().await;
    let (names, _) = stack.front.server_names();
    assert!(names.iter().any(|n| n.as_str() == "memory"));

    let workspace = WorkspaceId::new();
    let session = stack.open(workspace, &["recall"]).await;
    let offered: Vec<String> = stack
        .front
        .offered_tools(session.run)
        .expect("offered")
        .into_iter()
        .map(|t| t.tool)
        .collect();
    assert!(offered.contains(&"remember".to_owned()));

    let mut bridge = session.connect().await;
    let listed = bridge.tools().await;
    let names: Vec<&str> = listed.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"remember"), "{names:?}");
    for tool in &listed {
        for word in ["oneOf", "anyOf", "allOf"] {
            assert!(tool["inputSchema"].get(word).is_none(), "{tool}");
        }
    }

    // Allowed: the Rules Engine records it and the fact is kept.
    let answer = bridge
        .call(
            1,
            "remember",
            json!({
                "source": {"origin_ref": "kestrel-quote.pdf", "version": "1"},
                "facts": [{
                    "subject": {"name": "Kestrel"},
                    "predicate": "prefers",
                    "object": {"text": "Kestrel's Q3 quote is valid until 30 November."},
                }],
            }),
        )
        .await;
    assert!(!is_error(&answer), "{answer}");
    assert!(text_of(&answer).contains("pending"), "{answer}");
    let started = Instant::now();
    while kept_rows(&stack.store).await != 1 {
        assert!(started.elapsed() < Duration::from_secs(10), "never kept");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // A wrong call is answered by `check_arguments`, before any provider acts.
    let answer = bridge
        .call(2, "remember", json!({ "statement": "x", "made_from": [] }))
        .await;
    assert!(is_error(&answer), "{answer}");

    // Refused by the agent's tool policy before the provider acts: nothing recalled, nothing logged.
    let answer = bridge
        .call(3, "recall", json!({ "query": "Kestrel" }))
        .await;
    assert!(is_error(&answer), "{answer}");
    let (recalls,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_recalls")
        .fetch_one(stack.store.sqlite().expect("SQLite"))
        .await
        .expect("count");
    assert_eq!(recalls, 0);

    // A session without the refusal recalls the fact, through the same path.
    let other = stack.open(workspace, &[]).await;
    let mut bridge = other.connect().await;
    let answer = bridge
        .call(
            4,
            "recall",
            json!({ "query": "When does the Kestrel quote expire?" }),
        )
        .await;
    assert!(!is_error(&answer), "{answer}");
    assert!(
        text_of(&answer).contains("valid until 30 November"),
        "{answer}"
    );

    // Ordin reads the same fact in process.
    let result = stack
        .memory
        .recall(
            MemoryCaller::Ordin {
                workspace_id: workspace,
                run_id: RunId::new(),
            },
            RecallInput {
                query: Some("Kestrel quote".to_owned()),
                searches: None,
                folders: None,
                result_cap: 5,
                wider: WiderMode::Never,
            },
        )
        .await
        .expect("recall");
    assert_eq!(result.facts.len(), 1);
    stack.front.close_session(session.run).await.expect("close");
    stack.front.close_session(other.run).await.expect("close");
    stack.shutdown.cancel();
}

/// The provider's published tools pass the MCP Front's checks at `register`, and no input schema has
/// a root `oneOf`, `anyOf` or `allOf` (McpFront.md SA-RECON-10).
#[tokio::test]
async fn the_tool_schemas_have_no_root_combinators_and_the_lines_fit_the_front() {
    let stack = Stack::start().await;
    for server in stack.memory.tool_provider().servers() {
        for tool in server.tools {
            for word in ["oneOf", "anyOf", "allOf"] {
                assert!(tool.input_schema.get(word).is_none(), "{}", tool.name);
            }
            assert_eq!(tool.input_schema["type"], "object");
            assert_eq!(tool.input_schema["additionalProperties"], false);
            assert!(tool.description.chars().count() <= 120, "{}", tool.name);
            assert!(tool.argument_form.chars().count() <= 300, "{}", tool.name);
            assert!(tool.sandbox_callable);
        }
    }
    stack.shutdown.cancel();
}
