//! Cross-component test (plan 01, Phase 7): a session started by the real adapter, through the real
//! Session Supervisor, makes a tool call that reaches the real Hook Relay through the REAL
//! `ordin-hook` binary, and the verdict follows the real Rules Engine. The harness is a program
//! written by the test: it takes the hook commands from the `--settings` file it was given, exactly
//! as Claude Code does, and runs them. The real CLI runs in `tests/harness.rs`.

#![cfg(unix)]

mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::stream::StreamExt;
use ordin_agent_folders::AgentFolderManager;
use ordin_claude_adapter::SignInMode;
use ordin_files::{
    AddPlace, AgentWriteTarget, FileService, FileServiceDeps, FileVersionOrigin, PlaceKind,
};
use ordin_hook_relay::{AgentWriteError, AgentWrites, WriteTarget};
use ordin_mcp_front::ProgramProfile;
use ordin_runtime_adapter::{
    AdapterContext, AdapterKind, RunEvent, RunHandle, RunState, RuntimeAdapter,
};
use ordin_types::domain::agents::Authority;
use ordin_types::RunId;
use support::{hook_binary, write_script, Options, Stack, TempDir};
use tokio_util::sync::CancellationToken;

/// A harness that answers the CLI checks, then runs the hooks of its `--settings` for one call of
/// `$TOOL` on `$FILE`, and prints what each answered as its final message.
const HARNESS: &str = r#"#!/bin/sh
case "$1" in
  --version) echo '2.1.316 (Claude Code)'; exit 0 ;;
  auth) echo '{"loggedIn":true,"authMethod":"api_key"}'; exit 0 ;;
esac
settings=""; prev=""
for arg in "$@"; do [ "$prev" = "--settings" ] && settings="$arg"; prev="$arg"; done
command_of() { grep -o "\"command\":\"[^\"]*--event $1 [^\"]*\"" "$settings" | sed 's/^"command":"//; s/"$//'; }
IFS= read -r first
pre='{"session_id":"s","hook_event_name":"PreToolUse","tool_name":"'$TOOL'","tool_input":{"file_path":"'$FILE'"},"tool_use_id":"toolu_1"}'
post='{"session_id":"s","hook_event_name":"PostToolUse","tool_name":"'$TOOL'","tool_input":{},"tool_use_id":"toolu_1","tool_response":{"is_error":false}}'
verdict=$(printf '%s' "$pre" | eval "$(command_of pre-tool-use)")
case "$verdict" in
  *'"permissionDecision":"allow"'*)
    [ -n "$BODY" ] && printf '%s' "$BODY" > "$FILE"
    printf '%s' "$post" | eval "$(command_of post-tool-use)"
    answer=allowed ;;
  *'"permissionDecision":"deny"'*) answer=denied ;;
  *) answer=broken ;;
esac
printf '%s' '{"session_id":"s","hook_event_name":"Stop"}' | eval "$(command_of stop)"
echo '{"type":"result","is_error":false,"result":"'$answer'","usage":{"input_tokens":1,"output_tokens":1}}'
IFS= read -r rest
"#;

fn ctx() -> AdapterContext {
    AdapterContext {
        correlation_id: "corr".to_owned(),
        deadline: std::time::Instant::now() + Duration::from_secs(5),
        cancel: CancellationToken::new(),
    }
}

fn options(dir: &Path, tool: &str, file: &Path, body: &str, agent_folder: PathBuf) -> Options {
    std::fs::create_dir_all(&agent_folder).expect("the agent folder");
    let script = HARNESS.replacen(
        "#!/bin/sh\n",
        &format!(
            "#!/bin/sh\nTOOL='{tool}'\nFILE='{}'\nBODY='{body}'\n",
            file.display()
        ),
        1,
    );
    let claude = dir.join("claude");
    write_script(&claude, &script);
    Options {
        claude,
        hook: hook_binary().clone(),
        profile: ProgramProfile::Desktop,
        sign_in: SignInMode::PersonSignIn,
        tested_versions: vec!["2.1.316".to_owned()],
        agent_folder,
    }
}

async fn stack_with(dir: &Path, tool: &str, file: &Path) -> Stack {
    Stack::start(dir, options(dir, tool, file, "", dir.join("agent"))).await
}

/// What the program does: the relay's port over the File Service, which starts after it.
#[derive(Default)]
struct Files(OnceLock<Arc<FileService>>);

impl Files {
    fn service(&self) -> Result<&Arc<FileService>, AgentWriteError> {
        self.0.get().ok_or(AgentWriteError)
    }
}

#[async_trait::async_trait]
impl AgentWrites for Files {
    async fn open(
        &self,
        run_id: RunId,
        tool: &str,
        target: WriteTarget,
    ) -> Result<Option<String>, AgentWriteError> {
        let target = match target {
            WriteTarget::File(path) => AgentWriteTarget::File(path),
            WriteTarget::Anywhere => AgentWriteTarget::Anywhere,
        };
        self.service()?
            .open_agent_write(run_id, tool, target)
            .await
            .map_err(|_| AgentWriteError)
    }
    async fn settle(&self, claim: &str) -> Result<(), AgentWriteError> {
        self.service()?
            .settle_agent_write(claim)
            .await
            .map_err(|_| AgentWriteError)
    }
    async fn run_ended(&self, run_id: RunId) -> Result<(), AgentWriteError> {
        self.service()?
            .agent_run_ended(run_id)
            .await
            .map_err(|_| AgentWriteError)
    }
}

async fn events_of(stack: &Stack, run: RunId) -> Vec<RunEvent> {
    let handle = RunHandle {
        run_id: run,
        adapter: AdapterKind::ClaudeLocal,
        runtime_session_id: None,
        remote_thread_id: None,
    };
    for _ in 0..500 {
        if let Ok(stream) = stack.adapter.events(&ctx(), &handle) {
            return tokio::time::timeout(Duration::from_secs(30), stream.collect::<Vec<_>>())
                .await
                .expect("the stream ends");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the run never reached the adapter");
}

fn final_message(events: &[RunEvent]) -> Option<String> {
    match events.last() {
        Some(RunEvent::Ended { end }) => end.final_message.clone(),
        other => panic!("last event {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_allowed_tool_call_reaches_the_hook_relay_and_comes_back_as_progress() {
    let dir = TempDir::new();
    let notes = dir.0.join("agent").join("notes.md");
    let stack = Arc::new(stack_with(&dir.0, "Read", &notes).await);
    let (run, _agent) = stack.start_run(Authority::DocumentOnly).await;
    let events = events_of(&stack, run).await;
    assert_eq!(final_message(&events).as_deref(), Some("allowed"));
    let rows = stack.requests(run).await;
    assert_eq!(rows.len(), 1, "the Rules Engine journalled the call");
    assert_eq!((rows[0].0.as_str(), rows[0].1.as_str()), ("Read", "allow"));
    assert!(
        events.iter().any(|event| matches!(
            event,
            RunEvent::Progress { progress } if progress.line == "Read files"
                && progress.count.as_ref().is_some_and(|c| c.done == 1)
        )),
        "the relay's after-tool event became a progress line: {events:?}"
    );
    let usage = events.iter().find_map(|event| match event {
        RunEvent::Usage { usage } => Some(*usage),
        _ => None,
    });
    assert_eq!(usage.map(|u| u.tool_calls), Some(1));
    assert_eq!(stack.wait_end(run).await.0, RunState::Completed);
    stack.stop().await;
}

/// FU-073's chain: the hook's call opens the claim before the harness writes, and the after-tool
/// event makes the write a version of the run, with no watcher involved.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_harness_write_to_a_place_becomes_a_version_of_its_run() {
    let dir = TempDir::new();
    let linked = TempDir::new();
    let rates = linked.0.join("rates.md");
    std::fs::write(&rates, "# Rates").expect("the file");
    let files = Arc::new(Files::default());
    let stack = Arc::new(
        Stack::start_writing(
            &dir.0,
            options(&dir.0, "Write", &rates, "# Agent rates", linked.0.clone()),
            files.clone(),
        )
        .await,
    );
    let service = FileService::new(FileServiceDeps {
        journal: stack.journal.clone(),
        rules: stack.engine.clone(),
        agent_folders: Arc::new(
            AgentFolderManager::new((*stack.journal).clone(), dir.0.join("folders"))
                .await
                .expect("agent folder manager"),
        ),
        data_root: dir.0.join("files"),
    })
    .await
    .expect("file service");
    let place = service
        .add_place(AddPlace {
            path: linked.0.clone(),
            friendly_name: Some("Linked".to_owned()),
            kind: PlaceKind::WorkingCopy,
        })
        .await
        .expect("the place");
    service
        .keep_published_version(place.id, "rates.md", b"# Rates".to_vec())
        .await
        .expect("the file's history");
    files.0.set(service.clone()).map_err(|_| ()).expect("once");

    let (run, _agent) = stack.start_run(Authority::SystemChanging).await;
    let events = events_of(&stack, run).await;
    assert_eq!(final_message(&events).as_deref(), Some("allowed"));

    assert_eq!(
        std::fs::read_to_string(&rates).expect("the file"),
        "# Agent rates"
    );
    let versions = service.versions(place.id, "rates.md").await.expect("rows");
    assert_eq!(versions.len(), 2, "{versions:?}");
    assert!(
        matches!(&versions[1].origin, FileVersionOrigin::Run { run_id, tool } if *run_id == run && tool == "Write"),
        "{:?}",
        versions[1].origin
    );
    assert!(!service
        .agent_write_open(place.id, "rates.md")
        .await
        .expect("claims"));
    stack.stop().await;
}

/// With nowhere to record the claim, the harness never gets to write.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_that_cannot_be_claimed_never_reaches_the_disk() {
    let dir = TempDir::new();
    let linked = TempDir::new();
    let rates = linked.0.join("rates.md");
    std::fs::write(&rates, "# Rates").expect("the file");
    let stack = Arc::new(
        Stack::start_writing(
            &dir.0,
            options(&dir.0, "Write", &rates, "# Agent rates", linked.0.clone()),
            Arc::new(Files::default()),
        )
        .await,
    );
    let (run, _agent) = stack.start_run(Authority::SystemChanging).await;
    let events = events_of(&stack, run).await;
    assert_eq!(final_message(&events).as_deref(), Some("denied"));
    // The engine allowed it: the refusal is the claim's.
    let rows = stack.requests(run).await;
    assert_eq!((rows[0].0.as_str(), rows[0].1.as_str()), ("Write", "allow"));
    assert_eq!(
        std::fs::read_to_string(&rates).expect("the file"),
        "# Rates"
    );
    stack.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_tool_call_is_denied_by_the_real_rules_engine() {
    let dir = TempDir::new();
    let outside = dir.0.join("elsewhere.md");
    let stack = Arc::new(stack_with(&dir.0, "Bash", &outside).await);
    let (run, _agent) = stack.start_run(Authority::DocumentOnly).await;
    let events = events_of(&stack, run).await;
    assert_eq!(final_message(&events).as_deref(), Some("denied"));
    let rows = stack.requests(run).await;
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].0.as_str(), rows[0].1.as_str()), ("Bash", "deny"));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunEvent::Progress { .. })));
    stack.stop().await;
}
