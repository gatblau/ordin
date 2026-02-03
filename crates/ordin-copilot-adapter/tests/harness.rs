//! The Copilot compatibility proof (plan 08 Phase 8): tests that drive the REAL Copilot CLI through
//! `CliCopilotPort`, ordin's own speaker of the CLI's JSON-RPC. They settle C-01 and C-60 (the CLI
//! loads what the Copilot Installer writes), C-02 and C-27 (every tool request reaches ordin's gate
//! and a deny blocks it), C-03 (two sessions in two folders at once) and C-05 (a session resumes
//! after the CLI restarts) (`docs/design/ordin-desktop/10-risks.md`).
//!
//! They are `#[ignore]`, so the ordinary suite stays independent of the CLI; `make harness-test`
//! runs them. When run they fail, never skip, if the CLI is missing, its version differs from
//! [`PINNED`], or it is not signed in.
//!
//! The model is the real one behind the person's own Copilot sign-in: whoever runs the job needs a
//! Copilot licence on that machine (user decision 2026-02-03). The server environment is built as the
//! adapter builds it (an allow-list, no GitHub token), so the CLI finds its sign-in through `HOME`
//! exactly as it does in the desktop. The prompts ask for one specific action each; the assertions
//! check what ordin's gate saw and what reached the disk, never the model's wording, except for a
//! marker token the instructions or the first turn supply.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use ordin_copilot_adapter::{
    CliCopilotPort, CliServerSpec, CopilotEvent, CopilotPort, CopilotServer, GateAnswer,
    SessionOptions, ToolGate, ToolObserver,
};
use serde_json::{json, Value};

/// How long one turn of the real model may take.
const TURN: Duration = Duration::from_secs(240);
/// The adapter's own stop deadline (SA-STOP-01).
const STOP: Duration = Duration::from_secs(10);
/// The variables the adapter copies into the CLI's environment (`src/env.rs`, CPA-CPX-3).
const ALLOWED: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
];

/// The Copilot CLI version this job holds the adapter to: the version the proof first passed on
/// (2026-02-03, plan 08 Phase 8).
const PINNED: &str = "1.0.114";

/// The real CLI on `PATH`, at the pinned version. A missing CLI or another version is a failure of
/// the job.
fn pinned_copilot() -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH");
    let copilot = std::env::split_paths(&path)
        .map(|dir| dir.join("copilot"))
        .find(|candidate| candidate.is_file())
        .expect("the harness job needs the Copilot CLI on PATH");
    let output = std::process::Command::new(&copilot)
        .arg("--version")
        .env_clear()
        .envs(environment(&copilot))
        .output()
        .expect("run copilot --version");
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(
        printed.contains(PINNED),
        "the harness job is pinned to Copilot CLI {PINNED}; found {}",
        printed.trim()
    );
    copilot
}

/// The adapter's environment: the allow-list of this process, the CLI's folder on `PATH`.
fn environment(cli: &Path) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = ALLOWED
        .iter()
        .filter_map(|name| std::env::var(name).ok().map(|v| ((*name).to_owned(), v)))
        .collect();
    if let Some(folder) = cli.parent() {
        let path = env.entry("PATH".to_owned()).or_default();
        if !path.is_empty() {
            path.push(':');
        }
        path.push_str(&folder.to_string_lossy());
    }
    env
}

/// A short folder removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("temporary root")
            .join(format!("cpx-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Records every request and answers with one fixed verdict.
struct Gate {
    allow: bool,
    seen: Mutex<Vec<(String, Value)>>,
}

impl Gate {
    fn new(allow: bool) -> Arc<Gate> {
        Arc::new(Gate {
            allow,
            seen: Mutex::new(Vec::new()),
        })
    }
    fn seen(&self) -> Vec<(String, Value)> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ToolGate for Gate {
    async fn decide(&self, tool: &str, arguments: Value, _id: Option<String>) -> GateAnswer {
        self.seen.lock().unwrap().push((tool.to_owned(), arguments));
        if self.allow {
            GateAnswer::Allow
        } else {
            GateAnswer::Deny {
                reason: "ordin's compatibility proof denies every tool".into(),
            }
        }
    }
}

struct Ran(Mutex<Vec<(String, bool)>>);

#[async_trait::async_trait]
impl ToolObserver for Ran {
    async fn tool_used(&self, tool: &str, _id: Option<String>, succeeded: bool) {
        self.0.lock().unwrap().push((tool.to_owned(), succeeded));
    }
}

/// One CLI server and one session on it, with the gates the test reads afterwards.
struct Live {
    server: Box<dyn CopilotServer>,
    session_id: String,
    events: BoxStream<'static, CopilotEvent>,
    pre_tool_use: Arc<Gate>,
    permission: Arc<Gate>,
}

fn options(pre: &Arc<Gate>, permission: &Arc<Gate>) -> SessionOptions {
    SessionOptions {
        model: None,
        mcp_servers: json!({}),
        pre_tool_use: pre.clone(),
        permission: permission.clone(),
        post_tool_use: Arc::new(Ran(Mutex::new(Vec::new()))),
    }
}

async fn server(cli: &Path, folder: &Path) -> Box<dyn CopilotServer> {
    let server = CliCopilotPort::new()
        .start_server(CliServerSpec {
            cli_path: cli.to_path_buf(),
            working_folder: folder.to_path_buf(),
            env: environment(cli),
        })
        .await
        .expect("the CLI server starts and answers connect");
    let auth = server.auth_status().await.expect("auth.getStatus answers");
    assert!(
        auth.signed_in,
        "the harness job needs a Copilot CLI signed in with a Copilot licence ({auth:?})"
    );
    server
}

impl Live {
    async fn start(cli: &Path, folder: &Path, allow: bool) -> Live {
        let server = server(cli, folder).await;
        let (pre_tool_use, permission) = (Gate::new(allow), Gate::new(allow));
        let session_id = server
            .create_session(options(&pre_tool_use, &permission))
            .await
            .expect("session.create");
        let events = server.events(&session_id);
        Live {
            server,
            session_id,
            events,
            pre_tool_use,
            permission,
        }
    }

    /// Sends one prompt and returns the turn's final text, failing on any error event.
    async fn turn(&mut self, prompt: &str) -> String {
        self.server
            .send(&self.session_id, prompt)
            .await
            .expect("session.send");
        let mut text = String::new();
        let ended = tokio::time::timeout(TURN, async {
            while let Some(event) = self.events.next().await {
                match event {
                    CopilotEvent::AssistantText { text: t } => text.push_str(&t),
                    CopilotEvent::TurnEnded { final_text } => {
                        return Some(final_text.unwrap_or_else(|| text.clone()));
                    }
                    CopilotEvent::Error { message } => panic!("the session failed: {message}"),
                    CopilotEvent::SignedOut => panic!("the CLI reports it is signed out"),
                    CopilotEvent::AllowanceExhausted { message } => {
                        panic!("the Copilot allowance is used up: {message}")
                    }
                    CopilotEvent::ServerExited { status } => {
                        panic!("the CLI server exited ({status:?})")
                    }
                    _ => {}
                }
            }
            None
        })
        .await
        .expect("the turn ended within the deadline");
        ended.expect("the event stream ended before the turn did")
    }

    fn gated(&self) -> Vec<(String, Value)> {
        let mut all = self.pre_tool_use.seen();
        all.extend(self.permission.seen());
        all
    }

    async fn stop(self) {
        self.server.stop(STOP).await;
    }
}

/// What the Copilot Installer writes as the agent's general instructions
/// (`crates/ordin-installers/src/copilot.rs` `INDEX`).
fn install_instructions(folder: &Path, mark: &str) {
    let github = folder.join(".github");
    std::fs::create_dir_all(&github).expect(".github");
    std::fs::write(
        github.join("copilot-instructions.md"),
        format!(
            "# Agent instructions\n\nEnd every reply with the exact token {mark} on its own line.\n"
        ),
    )
    .expect("copilot-instructions.md");
}

/// C-01, C-60: the CLI in server mode applies the instructions the Copilot Installer wrote into the
/// session's working folder.
#[tokio::test]
#[ignore = "drives the real Copilot CLI; run by make harness-test"]
async fn the_cli_applies_the_instructions_the_installer_wrote() {
    let cli = pinned_copilot();
    let dir = TempDir::new("c01");
    install_instructions(&dir.0, "ORDIN-C60-MARK");
    let mut live = Live::start(&cli, &dir.0, true).await;
    let reply = live.turn("Say hello in five words or fewer.").await;
    live.stop().await;
    assert!(reply.contains("ORDIN-C60-MARK"), "reply: {reply}");
}

/// C-02, C-27: a shell command, a file write and a file read each reach ordin's gate before they run,
/// and a deny blocks them.
#[tokio::test]
#[ignore = "drives the real Copilot CLI; run by make harness-test"]
async fn every_tool_kind_reaches_the_gate_and_a_deny_blocks_it() {
    let cli = pinned_copilot();
    let dir = TempDir::new("c02");
    std::fs::write(dir.0.join("notes.txt"), "The code word is HERON.\n").expect("notes.txt");
    let mut live = Live::start(&cli, &dir.0, false).await;
    live.turn(
        "Do these three steps one after another, and carry on with the next step if one is \
         refused. 1. Use your shell tool to run exactly: touch made-by-shell.txt \
         2. Use your file creation tool to create made-by-edit.txt containing the word hi. \
         3. Use your file reading tool to read notes.txt. Then say which steps were refused.",
    )
    .await;
    let gated = live.gated();
    live.stop().await;
    assert!(
        !dir.0.join("made-by-shell.txt").exists(),
        "a denied shell command ran; gate saw {gated:?}"
    );
    assert!(
        !dir.0.join("made-by-edit.txt").exists(),
        "a denied file write ran; gate saw {gated:?}"
    );
    let all = format!("{gated:?}");
    assert!(
        all.contains("made-by-shell.txt"),
        "the shell request never reached the gate: {all}"
    );
    assert!(
        all.contains("made-by-edit.txt"),
        "the file write never reached the gate: {all}"
    );
    assert!(
        all.contains("notes.txt"),
        "the file read never reached the gate: {all}"
    );
}

/// C-03: two CLI servers, each with its own working folder, run a turn at the same time, and each
/// applies only its own folder's instructions.
#[tokio::test]
#[ignore = "drives the real Copilot CLI; run by make harness-test"]
async fn two_sessions_in_two_folders_run_at_once() {
    let cli = pinned_copilot();
    let (one, two) = (TempDir::new("c03a"), TempDir::new("c03b"));
    install_instructions(&one.0, "ORDIN-FOLDER-ONE");
    install_instructions(&two.0, "ORDIN-FOLDER-TWO");
    let (mut a, mut b) = tokio::join!(
        Live::start(&cli, &one.0, true),
        Live::start(&cli, &two.0, true)
    );
    let (ra, rb) = tokio::join!(a.turn("Say hello."), b.turn("Say hello."));
    tokio::join!(a.stop(), b.stop());
    assert!(
        ra.contains("ORDIN-FOLDER-ONE") && !ra.contains("ORDIN-FOLDER-TWO"),
        "{ra}"
    );
    assert!(
        rb.contains("ORDIN-FOLDER-TWO") && !rb.contains("ORDIN-FOLDER-ONE"),
        "{rb}"
    );
}

/// C-05: a session resumes on a new CLI server after the first one stopped, and remembers its first
/// turn.
#[tokio::test]
#[ignore = "drives the real Copilot CLI; run by make harness-test"]
async fn a_session_resumes_after_the_cli_restarts() {
    let cli = pinned_copilot();
    let dir = TempDir::new("c05");
    let mut first = Live::start(&cli, &dir.0, true).await;
    first
        .turn("Remember the word PELICAN-42. Reply with OK only.")
        .await;
    let session_id = first.session_id.clone();
    first.stop().await;

    let server = server(&cli, &dir.0).await;
    let (pre_tool_use, permission) = (Gate::new(true), Gate::new(true));
    server
        .resume_session(&session_id, options(&pre_tool_use, &permission))
        .await
        .expect("session.resume on a new CLI server");
    let events = server.events(&session_id);
    let mut second = Live {
        server,
        session_id,
        events,
        pre_tool_use,
        permission,
    };
    let reply = second
        .turn("Which word did I ask you to remember? Reply with the word only.")
        .await;
    second.stop().await;
    assert!(reply.contains("PELICAN-42"), "reply: {reply}");
}
