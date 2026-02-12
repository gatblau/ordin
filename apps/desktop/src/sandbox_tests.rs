//! Row 3.27 with `sandbox` (plan 08 Phase 5): the Playbook §7.2 `ToolProvider` test for the Sandbox
//! over the desktop's real construction, and the program-trait tests of `SandboxGrants` and
//! `PlaceFiles` (OrdinDesktop.md *Program-side implementations and calls*).

use std::time::Duration;

use ordin_journal::{ArtefactFilter, EntryFilter};
use ordin_mcp_front::{ProgramProfile, SessionGrant, SessionMcpConfig, ToolCaller};
use ordin_sandbox::{PlaceFileError, PlaceFiles, SandboxGrants};
use ordin_types::domain::HarnessKind;
use ordin_types::{AgentInstanceId, PlaceId, RunId, WorkspaceId};
use serde_json::{json, Value};

use crate::core::StepRunner;
use crate::hosts_sandbox::{DesktopPlaceFiles, DesktopSandboxGrants};
use crate::plan::Provider;
use crate::plan::{start_steps, Row, Step};
use crate::test_support::TempDir;
use crate::wiring::ModuleRunner;
use crate::wiring_tests::{build_until, rule_set, start_facts};

/// Every step of the build, with Identity's secrets in an encrypted file in place of the operating
/// system's keychain from row 3.27 on: the Sandbox's audit key is a secret, and the Linux test
/// container has no keychain (the shipped desktop runs on macOS and Windows only).
async fn wire_all(dir: &TempDir) -> ModuleRunner {
    use base64::Engine as _;
    let mut runner = build_until(dir, Row::Sandbox).await;
    let root = dir.path().join("identity-file");
    std::fs::create_dir_all(&root).expect("identity folder");
    let key_file = root.join("secrets.key");
    std::fs::write(
        &key_file,
        base64::engine::general_purpose::STANDARD.encode([7u8; 32]),
    )
    .expect("key file");
    let pool = runner
        .modules
        .journal
        .clone()
        .expect("journal")
        .datastore()
        .sqlite()
        .cloned();
    let identity = ordin_identity::IdentityService::new(
        ordin_identity::IdentityConfig {
            program: ordin_identity::Program::Desktop,
            data_root: root,
            keychain_service: "io.gatblau.ordin.test".to_owned(),
            secret_backend: ordin_identity::SecretBackendConfig::EncryptedFile {
                key_source: ordin_identity::KeySource::KeyFile(key_file),
            },
            github_app_client_id: None,
            microsoft_client_id: None,
            http: reqwest::Client::new(),
            browser: None,
        },
        pool,
    )
    .await
    .expect("identity over a file");
    runner.modules.identity = Some(std::sync::Arc::new(identity));
    let mut started = false;
    for step in start_steps() {
        started |= step == Step::Construct(Row::Sandbox);
        if started {
            runner
                .run(step)
                .await
                .unwrap_or_else(|e| panic!("{step:?}: {e:?}"));
        }
    }
    runner
}

/// An agent with its folder and rule set; `deny` names the verbs its tool policy prohibits.
async fn agent(runner: &ModuleRunner, deny: &[&str]) -> (AgentInstanceId, std::path::PathBuf) {
    let id = AgentInstanceId::new();
    let folder = runner
        .modules
        .agent_folders
        .clone()
        .expect("row 3.6")
        .create(id)
        .await
        .expect("agent folder")
        .folder_path;
    let mut set = rule_set(id, &folder);
    if !deny.is_empty() {
        set.tool_policy = Some(ordin_rules::AgentToolPolicy {
            prohibited_verbs: deny.iter().map(|s| (*s).to_owned()).collect(),
            ..Default::default()
        });
    }
    runner
        .modules
        .rules
        .clone()
        .expect("row 3.13")
        .load_rule_set(set)
        .expect("the set loads");
    (id, folder)
}

/// A run of `agent` with its MCP session open.
async fn open(runner: &ModuleRunner, agent: AgentInstanceId) -> (RunId, SessionMcpConfig) {
    let run = RunId::new();
    let facts = start_facts(run, agent);
    let workspace = facts.workspace_id;
    runner
        .modules
        .rules
        .clone()
        .expect("rules")
        .begin_run(facts)
        .await
        .expect("the run begins");
    let config = runner
        .modules
        .mcp_front
        .clone()
        .expect("front")
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

/// A bridge connection to one server of a session, speaking JSON lines.
#[cfg(unix)]
struct Bridge {
    reader: tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

#[cfg(unix)]
impl Bridge {
    async fn connect(run: RunId, config: &SessionMcpConfig, server: &str) -> Bridge {
        let env = &config.servers[0].env;
        let stream = tokio::net::UnixStream::connect(&env["ORDIN_MCP_ENDPOINT"])
            .await
            .expect("connect");
        let (r, w) = stream.into_split();
        let mut b = Bridge {
            reader: tokio::io::BufReader::new(r),
            writer: w,
        };
        b.send(json!({"protocol": 1, "session": run.to_string(), "server": server, "token": env["ORDIN_MCP_TOKEN"]}))
            .await;
        assert_eq!(b.recv().await, json!({"kind": "ready"}));
        b
    }

    async fn send(&mut self, value: Value) {
        use tokio::io::AsyncWriteExt;
        self.writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .expect("write");
    }

    async fn recv(&mut self) -> Value {
        use tokio::io::AsyncBufReadExt;
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

fn is_error(answer: &Value) -> bool {
    answer["result"]["isError"].as_bool().unwrap_or(false)
}

async fn sandbox_runs(runner: &ModuleRunner) -> usize {
    runner
        .modules
        .journal
        .clone()
        .expect("journal")
        .list_entries(
            EntryFilter {
                run_id: None,
                kind: Some("sandbox_run".to_owned()),
            },
            None,
        )
        .await
        .expect("entries")
        .items
        .len()
}

// Playbook §7.2, `ToolProvider` row, for the Sandbox: registered at start, offered, answers one
// allowed call (its output lands in the agent's folder and is indexed), refuses one denied call.
// spec: ordin-desktop/components/Sandbox#SA-RECON-02
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sandbox_registers_at_start_answers_an_allowed_call_and_refuses_a_denied_one() {
    let dir = TempDir::new("sandbox");
    let runner = wire_all(&dir).await;
    assert_eq!(
        runner.modules.providers.get(..2),
        Some(&[Provider::Files, Provider::Sandbox][..]),
        "row 3.27 registers after the File Service (row 3.22) and before the Memory Service"
    );
    let sandbox = runner.modules.sandbox.clone().expect("row 3.27");
    let status = sandbox.status();
    assert!(
        matches!(status, ordin_sandbox::SandboxStatus::Ready { .. }),
        "{status:?}"
    );

    // Allowed: the run is offered `python_sandbox`; Python runs and its file lands in the folder.
    let (agent_id, folder) = agent(&runner, &[]).await;
    let (run, config) = open(&runner, agent_id).await;
    let offered = runner
        .modules
        .mcp_front
        .clone()
        .expect("front")
        .offered_tools(run)
        .expect("offered");
    assert!(offered
        .iter()
        .any(|t| t.server.as_str() == "sandbox" && t.tool == "python_sandbox"));
    assert!(!offered.iter().any(|t| t.tool == "shell_sandbox"));
    let mut bridge = Bridge::connect(run, &config, "sandbox").await;
    let answer = bridge
        .call(
            1,
            "python_sandbox",
            json!({"code": "open('/work/out.txt','w').write('2')\nprint(1 + 1)",
                   "files": [{"path": "seed.txt", "contentBase64": "eA=="}]}),
        )
        .await;
    assert!(!is_error(&answer), "{answer}");
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    let result: Value = serde_json::from_str(text).expect("result JSON");
    assert_eq!(result["stdout"], "2\n", "{result}");
    let written = folder.join(format!("sandbox-output/{run}/out.txt"));
    assert_eq!(std::fs::read(&written).expect("the output file"), b"2");
    let artefacts = runner
        .modules
        .journal
        .clone()
        .expect("journal")
        .list_artefacts(ArtefactFilter::default(), None)
        .await
        .expect("artefacts")
        .items;
    assert!(artefacts
        .iter()
        .any(|a| a.path == format!("sandbox-output/{run}/out.txt")));

    // Denied: the agent's policy prohibits `python_sandbox`; the guest never runs.
    let before = sandbox_runs(&runner).await;
    let (denied_agent, _) = agent(&runner, &["python_sandbox"]).await;
    let (denied_run, denied_config) = open(&runner, denied_agent).await;
    let mut denied = Bridge::connect(denied_run, &denied_config, "sandbox").await;
    let refused = denied
        .call(2, "python_sandbox", json!({"code": "print(1)"}))
        .await;
    assert!(is_error(&refused), "{refused}");
    assert_eq!(before, sandbox_runs(&runner).await, "no sandbox run");

    sandbox.shutdown().await;
    runner.shutdown.cancel();
}

// SPEC-GAP-37: the desktop's grants name no company system.
#[tokio::test]
async fn sandbox_grants_name_no_system() {
    let caps = DesktopSandboxGrants
        .sandbox_capabilities(AgentInstanceId::new(), WorkspaceId::new())
        .await
        .expect("grants");
    assert!(caps.is_empty());
}

// `PlaceFiles` over the real File Service and Agent Folder Manager.
#[tokio::test]
async fn place_files_read_through_the_grant_and_write_into_the_agents_folder() {
    let dir = TempDir::new("place-files");
    let runner = wire_all(&dir).await;
    let places = DesktopPlaceFiles {
        files: runner.modules.files.clone().expect("row 3.22"),
        agent_folders: runner.modules.agent_folders.clone().expect("row 3.6"),
    };
    let (agent_id, folder) = agent(&runner, &[]).await;

    // A place the agent holds no grant on is refused.
    let read = places
        .read_for_agent(
            agent_id,
            WorkspaceId::new(),
            PlaceId::new(),
            "notes.txt",
            1024,
        )
        .await;
    assert_eq!(read, Err(PlaceFileError::NotGranted));

    // A write lands at the path in the agent's folder and replaces an earlier file there.
    places
        .write_agent_output(agent_id, "sandbox-output/r/out.txt", b"first")
        .await
        .expect("first write");
    let digest = places
        .write_agent_output(agent_id, "sandbox-output/r/out.txt", b"second")
        .await
        .expect("second write");
    assert_eq!(
        std::fs::read(folder.join("sandbox-output/r/out.txt")).expect("file"),
        b"second"
    );
    assert_eq!(
        digest,
        ordin_types::num::Sha256Hex::parse(
            "16367aacb67a4a017c8da8ab95682ccb390863780f7114dda0a0e0c55644c7c4"
        )
        .expect("hex")
    );

    // A path leaving the folder, or an agent with no folder, is refused.
    assert_eq!(
        places.write_agent_output(agent_id, "../x", b"x").await,
        Err(PlaceFileError::Unreadable)
    );
    assert_eq!(
        places
            .write_agent_output(AgentInstanceId::new(), "a.txt", b"x")
            .await,
        Err(PlaceFileError::Unreadable)
    );
    runner.shutdown.cancel();
}
