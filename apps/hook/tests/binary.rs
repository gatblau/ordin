//! Integration: runs the BUILT `ordin-hook` binary with harness-shaped JSON on standard input and
//! asserts the verdict on standard output (Playbook step 26; hook protocol: deny, exit 0).

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};

use ordin_types::encoding::b64url_encode;
use ordin_types::{AgentInstanceId, RunId};

const BIN: &str = env!("CARGO_BIN_EXE_ordin-hook");

fn run(args: &[&str], envs: &[(&str, String)], stdin: &str) -> (i32, String, String) {
    let mut cmd = Command::new(BIN);
    cmd.args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    (
        out.status.code().expect("exit code"),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn hook_env(endpoint: &str) -> Vec<(&'static str, String)> {
    vec![
        ("ORDIN_HOOK_ENDPOINT", endpoint.to_string()),
        ("ORDIN_HOOK_SECRET", b64url_encode(&[5u8; 32])),
        ("ORDIN_SESSION_ID", RunId::new().to_string()),
        ("ORDIN_HOOK_PROTOCOL", "1".to_string()),
    ]
}

const PAYLOAD: &str = r#"{"session_id":"s","hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"file_path":"a.txt"},"tool_use_id":"toolu_1"}"#;

#[test]
fn no_relay_listening_prints_deny_and_exits_zero() {
    let endpoint = std::env::temp_dir().join(format!("oh-bin-none-{}", std::process::id()));
    let agent = AgentInstanceId::new().to_string();
    let (status, stdout, _) = run(
        &[
            "--harness",
            "claude-code",
            "--event",
            "pre-tool-use",
            "--agent-instance",
            &agent,
        ],
        &hook_env(&endpoint.to_string_lossy()),
        PAYLOAD,
    );
    assert_eq!(status, 0);
    let verdict: serde_json::Value = serde_json::from_str(&stdout).expect("json verdict");
    assert_eq!(verdict["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(verdict["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        verdict["hookSpecificOutput"]["permissionDecisionReason"],
        "ordin is not running"
    );
}

#[test]
fn relay_allow_is_printed_as_the_harness_allow_form() {
    let path = std::env::temp_dir().join(format!("oh-bin-{}", std::process::id()));
    let listener = UnixListener::bind(&path).expect("bind");
    let relay = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut line = String::new();
        BufReader::new(stream.try_clone().expect("clone"))
            .read_line(&mut line)
            .expect("request");
        let request: serde_json::Value = serde_json::from_str(&line).expect("request json");
        let reply = serde_json::json!({"kind": "allow", "call_id": request["call_id"]});
        writeln!(stream, "{reply}").expect("reply");
        request
    });
    let agent = AgentInstanceId::new().to_string();
    let (status, stdout, stderr) = run(
        &[
            "--harness",
            "claude-code",
            "--event",
            "pre-tool-use",
            "--agent-instance",
            &agent,
        ],
        &hook_env(&path.to_string_lossy()),
        PAYLOAD,
    );
    let request = relay.join().expect("relay");
    let _ = std::fs::remove_file(&path);
    assert_eq!(status, 0, "{stderr}");
    assert!(
        stdout.contains(r#""permissionDecision":"allow""#),
        "{stdout}"
    );
    assert_eq!(request["protocol"], 1);
    assert_eq!(request["agent_instance_id"], agent);
    assert_eq!(request["message"]["kind"], "pre_tool_use");
    assert_eq!(request["message"]["origin"]["kind"], "built_in");
}

#[test]
fn a_bad_command_line_exits_64() {
    let (status, stdout, stderr) = run(&["--harness", "vim"], &[], "");
    assert_eq!(status, 64);
    assert_eq!(stdout, "");
    assert!(stderr.contains("usage"));
}

#[test]
fn malformed_payload_is_denied_with_exit_zero() {
    let endpoint = std::env::temp_dir().join("oh-bin-unused");
    let agent = AgentInstanceId::new().to_string();
    let (status, stdout, _) = run(
        &[
            "--harness",
            "claude-code",
            "--event",
            "pre-tool-use",
            "--agent-instance",
            &agent,
        ],
        &hook_env(&endpoint.to_string_lossy()),
        "this is not json",
    );
    assert_eq!(status, 0);
    assert!(stdout.contains("ordin could not read this tool call"));
}

#[test]
fn version_names_the_protocol() {
    let (status, stdout, _) = run(&["--version"], &[], "");
    assert_eq!(status, 0);
    assert!(
        stdout.starts_with("ordin-hook ") && stdout.trim_end().ends_with("protocol 1"),
        "{stdout}"
    );
}

/// Prints the server's environment and arguments, one per line.
const SHOW: &str = "#!/bin/sh\nenv\nprintf 'ARG=%s\\n' \"$@\"\n";

fn temp_name(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("oh-bin-{name}-{}", std::process::id()))
}

fn server(name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = temp_name(name);
    std::fs::write(&path, body).expect("the script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("its mode");
    path
}

fn spec_file(name: &str, body: &str, mode: u32) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = temp_name(name);
    std::fs::write(&path, body).expect("the file");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("its mode");
    path
}

#[test]
fn mcp_launch_passes_only_what_the_file_declares() {
    let script = server("show", SHOW);
    let spec = spec_file(
        "spec-show",
        &format!(
            r#"{{"program":"{}","args":["--flag","a b","${{KEEP_LITERAL}}"],
                 "env":{{"TOKEN":"declared","ANTHROPIC_API_KEY":"the-package-s-own",
                         "ORDIN_HOOK_SECRET":"forged","VALUE":"${{HOME}}"}}}}"#,
            script.display()
        ),
        0o600,
    );
    let mut envs = hook_env("/tmp/none");
    envs.extend([
        ("ANTHROPIC_API_KEY", "sk-secret".to_string()),
        ("CLAUDE_CODE_SESSION_ID", "s".to_string()),
        ("AWS_SECRET_ACCESS_KEY", "aws".to_string()),
        ("PATH", "/usr/bin:/bin".to_string()),
    ]);
    let path = spec.to_string_lossy().to_string();
    let (status, stdout, _) = run(&["mcp-launch", "--spec", &path], &envs, "");
    let _ = std::fs::remove_file(&script);
    let _ = std::fs::remove_file(&spec);
    assert_eq!(status, 0);
    for shown in [
        "TOKEN=declared",
        "ANTHROPIC_API_KEY=the-package-s-own",
        "PATH=/usr/bin:/bin",
        "ARG=--flag",
        "ARG=a b",
        "ARG=${KEEP_LITERAL}",
        "VALUE=${HOME}",
    ] {
        assert!(stdout.contains(shown), "{shown}: {stdout}");
    }
    for held in [
        "ORDIN_HOOK_SECRET",
        "ORDIN_HOOK_ENDPOINT",
        "ORDIN_SESSION_ID",
        "sk-secret",
        "CLAUDE_CODE_SESSION_ID",
        "AWS_SECRET_ACCESS_KEY",
    ] {
        assert!(
            !stdout.contains(held),
            "{held} reached the server: {stdout}"
        );
    }
}

#[test]
fn mcp_launch_gives_the_server_its_streams_and_status() {
    let script = server("echo", "#!/bin/sh\ncat\nexit 7\n");
    let spec = spec_file(
        "spec-echo",
        &format!(
            r#"{{"program":"{}","args":[],"env":{{}}}}"#,
            script.display()
        ),
        0o600,
    );
    let path = spec.to_string_lossy().to_string();
    let (status, stdout, _) = run(&["mcp-launch", "--spec", &path], &[], "hello");
    let _ = std::fs::remove_file(&script);
    let _ = std::fs::remove_file(&spec);
    assert_eq!(status, 7);
    assert_eq!(stdout, "hello");
}

#[test]
fn mcp_launch_reports_a_bad_file_or_a_server_that_cannot_start() {
    for (name, body, mode) in [
        (
            "no-program",
            r#"{"program":"/no/such/server","args":[],"env":{}}"#,
            0o600,
        ),
        ("not-json", "not json", 0o600),
        (
            "empty-program",
            r#"{"program":"","args":[],"env":{}}"#,
            0o600,
        ),
        (
            "bad-name",
            r#"{"program":"/bin/sh","args":["-c","exit 0"],"env":{"A=B":"x"}}"#,
            0o600,
        ),
        (
            "bad-arg",
            r#"{"program":"/bin/sh","args":[1],"env":{}}"#,
            0o600,
        ),
        (
            "too-open",
            r#"{"program":"/bin/sh","args":["-c","exit 0"],"env":{}}"#,
            0o644,
        ),
    ] {
        let spec = spec_file(name, body, mode);
        let path = spec.to_string_lossy().to_string();
        let (status, stdout, stderr) = run(&["mcp-launch", "--spec", &path], &[], "");
        let _ = std::fs::remove_file(&spec);
        assert_eq!(status, 1, "{name}");
        assert!(stdout.is_empty(), "{name}");
        assert!(stderr.contains("HOOK_LAUNCH_FAILED"), "{name}: {stderr}");
    }
    let (status, _, stderr) = run(&["mcp-launch", "--spec", "/no/such/file"], &[], "");
    assert_eq!(status, 1);
    assert!(stderr.contains("HOOK_LAUNCH_FAILED"), "{stderr}");
    for args in [
        &["mcp-launch"][..],
        &["mcp-launch", "--spec"][..],
        &["mcp-launch", "--spec", "relative.json"][..],
        &["mcp-launch", "--", "/bin/true"][..],
    ] {
        assert_eq!(run(args, &[], "").0, 64, "{args:?}");
    }
}
