//! The crate's tests that run on Windows only. The `tests` module is built on Unix alone (its fake
//! `claude` is a shell script), so the Windows scenarios of the spec live here.

use std::path::PathBuf;

/// A temporary folder named in its plain form, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir()
            .canonicalize()
            .map(ordin_types::paths::plain_form)
            .expect("temporary root")
            .join(format!(
                "cla-{}",
                &uuid::Uuid::now_v7().simple().to_string()[16..]
            ));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Gherkin "Happy path — the session record lands where Claude Code reads it on Windows": the agent
/// folder handed in its verbatim form `\\?\C:\...` is encoded from its plain form, so the encoded
/// folder starts with the drive letter and `--`, as for `C--Users-me-ordin-agents-a1`.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-PATH-1
#[test]
fn the_session_record_lands_where_claude_code_reads_it_on_windows() {
    let dir = TempDir::new();
    let plain = dir.0.join("ordin").join("agents").join("a1");
    std::fs::create_dir_all(&plain).unwrap();
    let verbatim = plain.canonicalize().unwrap();
    assert!(
        verbatim.to_string_lossy().starts_with(r"\\?\"),
        "{verbatim:?}"
    );
    let expected: String = plain
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let encoded = crate::session::encoded_folder(&verbatim);
    assert_eq!(encoded, expected);
    assert!(encoded.ends_with("-ordin-agents-a1"), "{encoded}");
    let mut chars = encoded.chars();
    assert!(
        chars.next().is_some_and(|c| c.is_ascii_alphabetic()),
        "{encoded}"
    );
    assert_eq!(chars.as_str().get(..2), Some("--"), "{encoded}");
}

/// A command shell that starts PowerShell, as `claude.cmd` starts Claude Code. The grandchild writes its
/// process id to `pid_file` and waits a minute.
fn shell_with_a_grandchild(pid_file: &std::path::Path) -> tokio::process::Child {
    let script = format!(
        "$PID | Out-File -Encoding ascii '{}'; Start-Sleep 60",
        pid_file.display()
    );
    tokio::process::Command::new("cmd.exe")
        .args(["/c", "powershell", "-NoProfile", "-Command", &script])
        .kill_on_drop(true)
        .spawn()
        .expect("cmd.exe starts")
}

/// The grandchild's process id, once it has written it.
async fn grandchild(pid_file: &std::path::Path) -> u32 {
    for _ in 0..200 {
        if let Ok(text) = std::fs::read_to_string(pid_file) {
            if let Ok(pid) = text.trim().parse() {
                return pid;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the grandchild did not start");
}

/// Whether a process with this id is running.
fn running(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist runs");
    String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}

/// Waits up to 10 s for a process to end.
async fn ends(pid: u32) -> bool {
    for _ in 0..100 {
        if !running(pid) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}

/// Gherkin "Happy path — stopping a session started through a command shell ends Claude Code too":
/// ending the session's job ends the grandchild, which ending the started process alone leaves running.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-JOB-1
#[tokio::test]
async fn stopping_a_session_started_through_a_command_shell_ends_its_whole_tree() {
    let dir = TempDir::new();
    let pid_file = dir.0.join("grandchild.pid");
    let mut child = shell_with_a_grandchild(&pid_file);
    let job = crate::job::Job::holding(&child).expect("a job for the session");
    let pid = grandchild(&pid_file).await;
    assert!(running(pid));

    job.terminate();

    assert!(ends(pid).await, "the grandchild ended with the job");
    let _ = child.wait().await;
}

/// Gherkin "Edge case — ordin ends without stopping the session": when the job's last handle closes, as
/// it does when ordin's process ends however it ends, the session's processes end with it.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-JOB-1
#[tokio::test]
async fn a_session_ends_when_its_job_is_let_go() {
    let dir = TempDir::new();
    let pid_file = dir.0.join("grandchild.pid");
    let mut child = shell_with_a_grandchild(&pid_file);
    let job = crate::job::Job::holding(&child).expect("a job for the session");
    let pid = grandchild(&pid_file).await;

    drop(job);

    assert!(
        ends(pid).await,
        "the grandchild ended when the job was let go"
    );
    let _ = child.wait().await;
}
