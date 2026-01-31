//! The log lines of *Observability* and what must never be in them, alone in its test binary (plan
//! FU-017): one global subscriber is installed before any test logs, and each test reads only the
//! lines it can tell are its own.

mod support;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use support::fixture;
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn buffer() -> &'static Buffer {
    static BUFFER: OnceLock<Buffer> = OnceLock::new();
    BUFFER.get_or_init(|| {
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(ordin_types::log::JsonLineFormat)
                .with_writer(move || writer.clone()),
        );
        tracing::subscriber::set_global_default(subscriber).expect("one global subscriber");
        buffer.clone()
    })
}

/// Lists the folder: the terminal runs `cmd.exe` on Windows.
const LIST: &str = if cfg!(windows) { "dir" } else { "ls" };

/// Two lines as typed at the keyboard: Enter is a carriage return to the Windows console, which
/// first asks where the cursor is (`ESC [6n`) and waits for the answer a terminal gives.
fn typed(first: &str, second: &str) -> Vec<u8> {
    let (cursor, enter) = if cfg!(windows) {
        ("\x1b[1;1R", "\r")
    } else {
        ("", "\n")
    };
    format!("{cursor}{first}{enter}{second}{enter}").into_bytes()
}

fn log_text() -> String {
    String::from_utf8_lossy(&buffer().0.lock().unwrap()).into_owned()
}

#[tokio::test]
async fn edge_case_the_terminal_never_reaches_an_agent_or_the_journal() {
    buffer();
    let f = fixture().await;
    let (project, _) = f.project().await;
    let stream = f.service.terminal_open(project.id, 80, 24).await.unwrap();
    f.service.terminal_resize(stream, 100, 30).await.unwrap();
    let secret = "marker-typed-by-the-engineer";
    f.service
        .terminal_input(stream, typed(&format!("echo {secret}"), LIST))
        .await
        .unwrap();
    let mut seen = false;
    for _ in 0..500 {
        if f.streams.terminal_text(stream).contains("README.md") {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(seen, "the output goes to the terminal stream");
    f.service.terminal_close(stream).await;

    let entries = f
        .journal
        .list_entries(ordin_journal::EntryFilter::default(), None)
        .await
        .unwrap();
    for entry in entries.items {
        assert!(!serde_json::to_string(&entry).unwrap().contains(secret));
    }
    assert!(!log_text().contains(secret));
    let tools = f.service.tool_provider().servers();
    assert!(tools
        .iter()
        .flat_map(|server| &server.tools)
        .all(|tool| !tool.name.contains("terminal")));
    let after = f.service.terminal_input(stream, vec![b'x']).await;
    assert_eq!(after.unwrap_err().code(), "PROJ_TERMINAL_FAILED");
}

#[tokio::test]
async fn the_events_carry_their_fields_and_no_content() {
    buffer();
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let marker = "line-content-not-for-the-log";
    let started = f.start_task(&project, "logged").await;
    let tree = support::worktree(&started.task);
    support::write(&tree, "src/rates.rs", &format!("{marker}\n"));
    let set = f
        .service
        .end_coding_run(started.task.id, ordin_projects::RunId::new())
        .await
        .unwrap();
    f.service
        .decide_entry(set.entries[0].id, ordin_projects::EntryDecision::Reject)
        .await
        .unwrap();
    f.service
        .set_check_commands(project.id, vec!["echo secret-output".to_owned()])
        .await
        .unwrap();
    f.service
        .run_check(
            project.id,
            None,
            "echo secret-output",
            ordin_projects::CommandOrigin::Person,
        )
        .await
        .unwrap();
    support::write(&folder, "README.md", "edited\n");
    f.service
        .commit(project.id, "A commit message not for the log", None)
        .await
        .unwrap();

    let text = log_text();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .filter(|line| {
            line.contains(&project.id.to_string()) || line.contains(&started.task.id.to_string())
        })
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let event = |name: &str| lines.iter().find(|line| line["event"] == name);
    assert_eq!(event("project_opened").unwrap()["is_git"], true);
    assert!(event("coding_task_started").is_some());
    assert_eq!(
        event("coding_task_removed").unwrap()["component"],
        "projects"
    );
    assert_eq!(event("change_set_ready").unwrap()["entries"], 1);
    assert!(event("check_run").unwrap()["elapsed_ms"].is_number());
    assert!(text.contains("entry_decided"));
    assert!(!text.contains(marker));
    assert!(!text.contains("secret-output"));
    assert!(!text.contains("A commit message not for the log"));
}
