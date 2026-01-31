//! The pure helpers, and X4.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordin_hook_relay::HookSessionEnv;
use ordin_mcp_front::{McpServerLaunch, ServerName, SessionMcpConfig};
use ordin_runtime_adapter::{
    HarnessKind, HarnessModels, ModelLevel, RunEndReason, RunEvent, RunState, StartInput,
    ToolCaller,
};
use ordin_types::domain::agent_profile::RunStarter;
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::{AgentInstanceId, RunId, WorkspaceId};
use serde_json::json;
use tokio::sync::mpsc;

use super::support::*;
use crate::command::{session_args, user_line, CommandInput, SessionFlag};
use crate::config::{ClaudeEffort, KeyRoute, PackageMcpServer, SignInMode};
use crate::env::{child_environment, ParentEnv};
use crate::files;
use crate::models::ClaudeModels;
use crate::process::{read_lines, Line};
use crate::progress::plain_line;
use crate::run::{Run, RunIdentity, MAX_BUFFERED_EVENTS};
use crate::session::{first_turn, lists_work_server, with_progress_line, PROGRESS_LINE};
use crate::stream::{parse_line, Malformed, StreamItem, MAX_LINE_BYTES};
use crate::{ClaudeAdapterError, ALL_CODES, DESKTOP_BUILT_IN_TOOLS};

#[test]
fn all_codes_are_registered_and_the_prefix_is_owned() {
    assert!(ALL_CODES.windows(2).all(|pair| pair[0] < pair[1]), "sorted");
    assert_eq!(ALL_CODES.len(), 33);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, "ordin-claude-adapter");
        let prefix = code.split('_').next().expect("a prefix");
        assert!(owns_prefix("ordin-claude-adapter", prefix));
    }
}

#[test]
fn the_public_error_matches_the_registry() {
    for error in [
        ClaudeAdapterError::ConfigInvalid("x"),
        ClaudeAdapterError::SignInStatusFailed,
        ClaudeAdapterError::EnvReserved("X".to_owned()),
    ] {
        use ordin_types::CodedError;
        let entry = lookup(CodedError::code(&error)).expect("registered");
        assert_eq!(entry.retryable, CodedError::retryable(&error));
    }
}

#[test]
fn harness_models_are_those_of_the_public_interface() {
    let models = ClaudeModels;
    assert_eq!(models.harness(), HarnessKind::ClaudeCode);
    assert!(models.per_session_model());
    let map = models.default_level_models();
    assert_eq!(map.get(&ModelLevel::Smaller).unwrap(), "haiku");
    assert_eq!(map.get(&ModelLevel::Standard).unwrap(), "sonnet");
    assert_eq!(map.get(&ModelLevel::MostCapable).unwrap(), "opus");
    assert!(!map.contains_key(&ModelLevel::Automatic));
    assert!(models.accepts_model("claude-sonnet-4.6_x"));
    assert!(models.accepts_model(&"m".repeat(100)));
    assert!(!models.accepts_model(""));
    assert!(!models.accepts_model(&"m".repeat(101)));
    assert!(!models.accepts_model("has space"));
}

#[test]
fn plain_lines_follow_the_table() {
    let built_in = ToolOrigin::BuiltIn;
    let front = |server: &str| ToolOrigin::OrdinMcpFront {
        server: server.to_owned(),
    };
    let package = ToolOrigin::PackageMcpServer {
        server: "tools".to_owned(),
    };
    for (tool, origin, line) in [
        ("Read", &built_in, Some("Read files")),
        ("Glob", &built_in, Some("Read files")),
        ("Grep", &built_in, Some("Read files")),
        ("Edit", &built_in, Some("Changed a file")),
        ("Write", &built_in, Some("Changed a file")),
        ("Bash", &built_in, Some("Ran an approved command")),
        ("TodoWrite", &built_in, None),
        ("Task", &built_in, Some("Asked a helper")),
        ("Agent", &built_in, Some("Asked a helper")),
        ("Other", &built_in, Some("Used a tool")),
    ] {
        assert_eq!(plain_line(tool, origin, true).as_deref(), line, "{tool}");
    }
    for (server, line) in [
        ("sandbox", Some("Ran code in the sandbox")),
        ("memory", Some("Checked what it remembers")),
        ("mail", Some("Worked with email")),
        ("work", None),
        ("ordin", None),
        ("code", Some("Looked at the code")),
        ("telegram", Some("Worked with Telegram")),
        ("craft_blender", Some("Worked on a file in an ArtCraft app")),
        ("other", Some("Used a tool")),
    ] {
        assert_eq!(
            plain_line("t", &front(server), true).as_deref(),
            line,
            "{server}"
        );
    }
    assert_eq!(
        plain_line("t", &package, true).as_deref(),
        Some("Used one of the agent's own tools")
    );
    assert_eq!(
        plain_line("Read", &built_in, false).as_deref(),
        Some("Read files (it didn't work)")
    );
}

#[test]
fn a_user_line_keeps_the_spec_order_and_ends_with_a_newline() {
    assert_eq!(
        user_line("a \"b\""),
        b"{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"a \\\"b\\\"\"}}\n"
    );
}

fn parent_of(pairs: &[(&'static str, &'static str)]) -> ParentEnv {
    let map: BTreeMap<&str, &str> = pairs.iter().copied().collect();
    Arc::new(move |name: &str| map.get(name).map(Into::into))
}

#[test]
fn the_child_environment_is_an_allow_list() {
    let parent = parent_of(&[
        ("PATH", "/bin"),
        ("https_proxy", "http://p"),
        ("HTTP_PROXY", "http://q"),
        ("SECRET_THING", "x"),
        ("ANTHROPIC_BASE_URL", "http://inherited"),
    ]);
    let env = child_environment(
        &parent,
        &SignInMode::PersonSignIn,
        None,
        Path::new("/bin/claude"),
    );
    let names: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["PATH", "https_proxy", "HTTP_PROXY"]);
}

// CLA-PATH-1: a `claude` found outside `PATH` (a Finder or Dock start's short `PATH` leaves out the
// person's own `~/.local/bin`) has its folder added at the end, once, and only when missing.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-PATH-1
#[cfg(unix)]
#[test]
fn the_child_path_holds_the_folder_of_claude() {
    let path_of = |env: Vec<(String, std::ffi::OsString)>| {
        env.into_iter()
            .find(|(name, _)| name == "PATH")
            .map(|(_, value)| value)
    };
    let short = parent_of(&[("PATH", "/usr/bin:/bin")]);
    let cases: [(&ParentEnv, &str, Option<&str>); 4] = [
        (
            &short,
            "/home/a/.local/bin/claude",
            Some("/usr/bin:/bin:/home/a/.local/bin"),
        ),
        (&short, "/bin/claude", Some("/usr/bin:/bin")),
        (&short, "claude", Some("/usr/bin:/bin")),
        (
            &parent_of(&[]),
            "/home/a/.local/bin/claude",
            Some("/home/a/.local/bin"),
        ),
    ];
    for (parent, program, expected) in cases {
        let env = child_environment(parent, &SignInMode::PersonSignIn, None, Path::new(program));
        assert_eq!(
            path_of(env),
            expected.map(std::ffi::OsString::from),
            "{program}"
        );
    }
}

#[test]
fn the_helper_route_keeps_the_key_out_of_the_environment() {
    let parent = parent_of(&[]);
    let mode = SignInMode::OrganisationKey {
        models: models(Some("sk-org")),
        route: KeyRoute::SettingsApiKeyHelper,
    };
    assert!(child_environment(&parent, &mode, None, Path::new("claude")).is_empty());
    let hook = HookSessionEnv {
        endpoint: "/e".to_owned(),
        secret: "s".to_owned(),
        session_id: RunId::new(),
        protocol: 1,
    };
    let env = child_environment(&parent, &mode, Some(&hook), Path::new("claude"));
    assert_eq!(env.len(), 4);
}

#[test]
fn settings_follow_the_example() {
    let agent = AgentInstanceId::parse("0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff").unwrap();
    let settings = files::settings_json(Path::new("/opt/ordin/ordin-hook"), agent, None);
    let flags = "--harness claude-code --event";
    let id = "--agent-instance 0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff";
    assert_eq!(
        settings,
        json!({"disableSkillShellExecution": true, "hooks": {
            "PreToolUse": [{"matcher": "*", "hooks": [{"type": "command",
                "command": format!("/opt/ordin/ordin-hook {flags} pre-tool-use {id}"),
                "timeout": 60, "onFailure": "block"}]}],
            "PostToolUse": [{"matcher": "*", "hooks": [{"type": "command",
                "command": format!("/opt/ordin/ordin-hook {flags} post-tool-use {id}"),
                "timeout": 60}]}],
            "Stop": [{"hooks": [{"type": "command",
                "command": format!("/opt/ordin/ordin-hook {flags} stop {id}"),
                "timeout": 60}]}]
        }})
    );
}

#[test]
fn a_path_with_a_space_is_quoted_for_the_shell() {
    assert_eq!(files::shell_word_for("/a/b-c_d.e", false), "/a/b-c_d.e");
    assert_eq!(
        files::shell_word_for(
            "/Users/asha/Library/Application Support/ordin/bin/ordin-hook",
            false
        ),
        "'/Users/asha/Library/Application Support/ordin/bin/ordin-hook'"
    );
    assert_eq!(files::shell_word_for("it's", false), r"'it'\''s'");
    assert_eq!(files::shell_word_for("", false), "''");
}

#[test]
fn a_windows_path_reaches_git_bash_whole() {
    // Found on Windows: `C:\o\target\release\ordin-hook.exe` unquoted reached Git Bash as
    // `C:otargetreleaseordin-hook.exe`, and every tool call was blocked.
    assert_eq!(
        files::shell_word_for(r"C:\o\target\release\ordin-hook.exe", true),
        r#""C:/o/target/release/ordin-hook.exe""#
    );
    assert_eq!(
        files::shell_word_for(r"C:\Users\Asha Rao\AppData\Local\org.ordin.desktop", true),
        r#""C:/Users/Asha Rao/AppData/Local/org.ordin.desktop""#
    );
    assert_eq!(files::shell_word_for(r"C:\a$b`c", true), r#""C:/a\$b\`c""#);
    // Unix keeps its form; a backslash is no longer a plain character there either.
    assert_eq!(files::shell_word_for("/a/b-c_d.e", false), "/a/b-c_d.e");
    assert_eq!(files::shell_word_for(r"/a\b", false), r"'/a\b'");
}

#[test]
fn the_helper_line_names_the_run_and_the_root() {
    let run = RunId::new();
    let root = Path::new("/data/root x");
    assert_eq!(
        files::api_key_helper(Path::new("/h"), run, root),
        format!(
            "{} api-key --run {run} --data-root {}",
            files::shell_word("/h"),
            files::shell_word("/data/root x")
        )
    );
}

#[test]
fn package_servers_join_the_front_s_list() {
    let front = SessionMcpConfig {
        servers: vec![McpServerLaunch {
            name: ServerName::new("memory").unwrap(),
            command: PathBuf::from("/h"),
            args: vec!["mcp-bridge".to_owned()],
            env: BTreeMap::new(),
        }],
    };
    let package = PackageMcpServer {
        name: "tools".to_owned(),
        command: PathBuf::from("/p"),
        args: vec!["--x".to_owned()],
        env: BTreeMap::from([("K".to_owned(), "v".to_owned())]),
    };
    let hook = Path::new("/ordin-hook");
    let dir = Path::new("/session");
    let made = files::mcp_files(&front, std::slice::from_ref(&package), hook, dir).unwrap();
    assert_eq!(
        made.mcp_json["mcpServers"]["tools"],
        json!({"type": "stdio", "command": "/ordin-hook",
               "args": ["mcp-launch", "--spec", "/session/package-server-0.json"]})
    );
    assert_eq!(
        made.mcp_json["mcpServers"]["memory"]["args"],
        json!(["mcp-bridge"])
    );
    let (path, bytes) = &made.launches[0];
    assert_eq!(path, Path::new("/session/package-server-0.json"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(bytes).unwrap(),
        json!({"program": "/p", "args": ["--x"], "env": {"K": "v"}})
    );
    assert!(format!("{package:?}").contains("env_names"));
    let clash = PackageMcpServer {
        name: "memory".to_owned(),
        ..package
    };
    assert_eq!(
        files::mcp_files(&front, &[clash], hook, dir).err().unwrap(),
        "memory"
    );
}

#[test]
fn nothing_a_package_declares_enters_mcp_json_where_the_cli_would_expand_it() {
    let front = SessionMcpConfig { servers: vec![] };
    let package = PackageMcpServer {
        name: "tools".to_owned(),
        command: PathBuf::from("/${ORDIN_HOOK_SECRET}"),
        args: vec!["${ANTHROPIC_API_KEY}".to_owned()],
        env: BTreeMap::from([("X".to_owned(), "${ORDIN_HOOK_SECRET}".to_owned())]),
    };
    let made = files::mcp_files(&front, &[package], Path::new("/h"), Path::new("/s")).unwrap();
    let text = made.mcp_json.to_string();
    assert!(!text.contains('$'), "{text}");
    assert!(!text.contains("\"env\""), "{text}");
}

#[test]
fn free_text_starting_a_command_is_plain_whoever_started_the_run() {
    let fake = Fake::new();
    let free = StartInput::FreeText {
        text: "  /analyst:deploy now".to_owned(),
    };
    let mut asker = fake.setup();
    asker.run_starter = Some(RunStarter::ChatConnection {
        connection: "c".to_owned(),
        asker: "someone".to_owned(),
    });
    for setup in [fake.setup(), asker] {
        let run = plain_run(&fake, setup);
        let turn = first_turn(&run, &free, None);
        assert!(!turn.trim_start().starts_with('/'), "{turn}");
        assert!(turn.ends_with("/analyst:deploy now"), "{turn}");
    }
    let run = plain_run(&fake, fake.setup());
    let hello = StartInput::FreeText {
        text: "Hello /there".to_owned(),
    };
    assert_eq!(first_turn(&run, &hello, None), "Hello /there");
    let command = StartInput::Command {
        name: "compare".to_owned(),
        arguments: String::new(),
    };
    assert!(first_turn(&run, &command, None).starts_with('/'));
}

// Scenario: The owner's typed command runs (SA-RECON-06, amended 2026-01-24).
#[test]
fn the_owners_typed_command_is_written_as_typed() {
    let fake = Fake::new();
    let typed = StartInput::FreeText {
        text: "/analyst:deploy now".to_owned(),
    };
    let run = run_started_by(&fake, fake.setup(), true);
    assert_eq!(first_turn(&run, &typed, None), "/analyst:deploy now");
    // The same text from anyone else stays text.
    let run = run_started_by(&fake, fake.setup(), false);
    let turn = first_turn(&run, &typed, None);
    assert!(!turn.trim_start().starts_with('/'), "{turn}");
}

#[test]
fn session_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    let session = files::session_dir(&dir.0, RunId::new());
    files::create_session_dir(&session).unwrap();
    let file = session.join("settings.json");
    files::write_private(&file, b"{}").unwrap();
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&session), 0o700);
    assert_eq!(mode(&file), 0o600);
    files::sweep_sessions(&dir.0);
    assert!(!session.exists());
    files::sweep_sessions(&dir.0.join("nowhere"));
}

fn input_with<'a>(
    setup: &'a crate::config::SessionSetup,
    subset: Option<&'a [String]>,
    agent_profile: bool,
) -> CommandInput<'a> {
    CommandInput {
        runtime_session_id: "sid",
        flag: SessionFlag::New,
        session_dir: Path::new("/d"),
        setup,
        agent_profile,
        tool_subset: subset,
    }
}

fn joined(args: Vec<std::ffi::OsString>) -> String {
    args.iter()
        .map(|a| a.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_command_line_is_in_the_spec_s_order() {
    let fake = Fake::new();
    let mut setup = fake.setup();
    setup.shell_allowed = true;
    setup.extra_dirs = vec![PathBuf::from("/x"), PathBuf::from("/y")];
    setup.model = Some("opus".to_owned());
    setup.effort = Some(ClaudeEffort::Xhigh);
    setup.fallback_model = Some("sonnet".to_owned());
    assert_eq!(
        joined(session_args(&input_with(&setup, None, false))),
        "-p --input-format stream-json --output-format stream-json --verbose --session-id sid \
         --restricted --strict-mcp-config --mcp-config /d/mcp.json --settings /d/settings.json \
         --permission-prompts none --tools Read,Edit,Write,Glob,Grep,Task,Bash \
         --add-dir /x --add-dir /y --model opus --effort xhigh"
    );
}

#[test]
fn every_effort_has_its_flag_value() {
    for (effort, value) in [
        (ClaudeEffort::Low, "low"),
        (ClaudeEffort::Medium, "medium"),
        (ClaudeEffort::High, "high"),
        (ClaudeEffort::Xhigh, "xhigh"),
        (ClaudeEffort::Max, "max"),
    ] {
        assert_eq!(effort.flag_value(), value);
        assert_eq!(
            ClaudeEffort::parse(value),
            Some(effort),
            "{value} reads back"
        );
    }
    assert_eq!(ClaudeEffort::parse("extreme"), None);
    assert_eq!(ClaudeEffort::parse(""), None);
}

#[test]
fn the_agent_profile_reads_only_and_never_adds_bash() {
    let fake = Fake::new();
    let mut setup = fake.setup();
    setup.shell_allowed = true;
    setup.fallback_model = Some("sonnet".to_owned());
    let line = joined(session_args(&input_with(&setup, None, true)));
    assert!(line.contains("--tools Read --disallowedTools Write,Edit"));
    assert!(line.ends_with("--fallback-model sonnet"));
    assert!(!line.contains("Bash"));
}

#[test]
fn a_tool_subset_narrows_the_list_and_never_widens_it() {
    let fake = Fake::new();
    let setup = fake.setup();
    let subset = vec!["Read".to_owned(), "Bash".to_owned()];
    let line = joined(session_args(&input_with(&setup, Some(&subset), false)));
    assert!(line.ends_with("--tools Read"), "{line}");
}

/// 2.1.316 reads an empty `--tools` value as no built-in tool; absent, it gives the default set. The
/// value must stay on the command line, as its own argument.
#[test]
fn a_subset_with_no_overlap_gives_an_empty_tools_value_never_no_flag() {
    let fake = Fake::new();
    let setup = fake.setup();
    let subset = vec!["WebFetch".to_owned()];
    for agent_profile in [false, true] {
        let args = session_args(&input_with(&setup, Some(&subset), agent_profile));
        let at = args
            .iter()
            .position(|arg| arg == "--tools")
            .expect("--tools");
        assert_eq!(args[at + 1], "");
    }
}

#[test]
fn the_built_in_list_is_the_spec_s() {
    assert_eq!(
        DESKTOP_BUILT_IN_TOOLS.join(","),
        "Read,Edit,Write,Glob,Grep,Task"
    );
}

#[test]
fn stream_lines_become_typed_items() {
    assert_eq!(parse_line(b"nope"), Err(Malformed));
    assert_eq!(
        parse_line(br#"{"type":"system","subtype":"init"}"#),
        Ok(StreamItem::Init)
    );
    assert_eq!(
        parse_line(br#"{"type":"system","subtype":"api_retry"}"#),
        Ok(StreamItem::Other)
    );
    assert_eq!(parse_line(br#"{"no":"type"}"#), Ok(StreamItem::Other));
    assert_eq!(
        parse_line(br#"{"type":"user"}"#),
        Ok(StreamItem::Activity { plan: None })
    );
    assert_eq!(
        parse_line(
            br#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning"}}"#
        ),
        Ok(StreamItem::RateLimit {
            allowance_used_up: false
        })
    );
    let StreamItem::Result(result) =
        parse_line(br#"{"type":"result","is_error":false,"result":"ok"}"#).unwrap()
    else {
        panic!("a result");
    };
    assert_eq!(result.input_tokens, None, "C-37: usage absent stays None");
    assert_eq!(result.result.as_deref(), Some("ok"));
}

#[test]
fn a_todowrite_input_is_the_harness_plan() {
    let line = br#"{"type":"assistant","message":{"content":[
        {"type":"text","text":"x"},
        {"type":"tool_use","name":"TodoWrite","input":{"todos":[{"content":"One"},{"content":"Two"}]}}]}}"#;
    assert_eq!(
        parse_line(line),
        Ok(StreamItem::Activity {
            plan: Some(vec!["One".to_owned(), "Two".to_owned()])
        })
    );
    let other = br#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{}}]}}"#;
    assert_eq!(parse_line(other), Ok(StreamItem::Activity { plan: None }));
}

async fn lines_of(bytes: Vec<u8>) -> Vec<Line> {
    let (sender, mut receiver) = mpsc::channel(8);
    tokio::spawn(read_lines(std::io::Cursor::new(bytes), sender));
    let mut lines = Vec::new();
    while let Some(line) = receiver.recv().await {
        lines.push(line);
    }
    lines
}

#[tokio::test]
async fn lines_are_split_and_an_oversized_one_is_dropped() {
    assert_eq!(
        lines_of(b"a\nb\nrest".to_vec()).await,
        [
            Line::Text(b"a".to_vec()),
            Line::Text(b"b".to_vec()),
            Line::Text(b"rest".to_vec())
        ]
    );
    let mut bytes = vec![b'x'; MAX_LINE_BYTES + 1];
    bytes.extend_from_slice(b"\nnext\n");
    bytes.extend(vec![b'y'; MAX_LINE_BYTES + 1]);
    assert_eq!(
        lines_of(bytes).await,
        [Line::TooLong, Line::Text(b"next".to_vec()), Line::TooLong]
    );
    let mut exact = vec![b'z'; MAX_LINE_BYTES];
    exact.push(b'\n');
    assert_eq!(lines_of(exact.clone()).await.len(), 1);
    assert!(matches!(&lines_of(exact).await[0], Line::Text(t) if t.len() == MAX_LINE_BYTES));
}

fn plain_run(fake: &Fake, setup: crate::config::SessionSetup) -> Arc<Run> {
    run_started_by(fake, setup, false)
}

/// As [`plain_run`], with `StartRun.started_by_person` as given.
fn run_started_by(
    fake: &Fake,
    setup: crate::config::SessionSetup,
    started_by_person: bool,
) -> Arc<Run> {
    let _ = fake;
    Run::new(
        RunIdentity {
            run_id: RunId::new(),
            agent_instance_id: AgentInstanceId::new(),
            workspace_id: WorkspaceId::new(),
            runtime_session_id: "sid".to_owned(),
            correlation_id: "c".to_owned(),
            caller: ToolCaller::Agent,
            scope_id: None,
            coding_task_id: None,
            started_by_person,
            harness_version: "2.1.304".to_owned(),
            setup,
            only_servers: None,
        },
        false,
    )
    .0
}

#[test]
fn the_first_turn_follows_step_11() {
    let fake = Fake::new();
    let run = plain_run(&fake, fake.setup());
    let command = StartInput::Command {
        name: "compare".to_owned(),
        arguments: String::new(),
    };
    assert_eq!(
        first_turn(&run, &command, None),
        "/supplier-analyst:compare"
    );
    assert_eq!(
        first_turn(&run, &command, Some("other")),
        "/supplier-analyst:other"
    );
    let free = StartInput::FreeText {
        text: "Hello".to_owned(),
    };
    assert_eq!(first_turn(&run, &free, Some("ignored")), "Hello");
    let mut setup = fake.setup();
    setup.attachment_paths = vec![PathBuf::from("/a/one.md"), PathBuf::from("/a/two.md")];
    let run = plain_run(&fake, setup);
    assert_eq!(
        first_turn(&run, &free, None),
        "Hello\nAttached: /a/one.md\nAttached: /a/two.md"
    );
}

#[tokio::test]
async fn progress_past_the_buffer_limit_is_merged() {
    use futures::stream::StreamExt;
    let fake = Fake::new();
    let run = plain_run(&fake, fake.setup());
    run.emit(RunEvent::Started {
        runtime_session_id: None,
        remote_thread_id: None,
    });
    for done in 1..MAX_BUFFERED_EVENTS as u64 + 50 {
        run.emit_progress(format!("line {done}"), Some(done));
    }
    run.emit(RunEvent::Resumed);
    run.emit_progress("after".to_owned(), None);
    run.emit_end(RunEndReason::Completed, None);
    run.emit_progress("past the end".to_owned(), None);
    let events: Vec<RunEvent> = run.subscribe().collect().await;
    assert_eq!(events.len(), MAX_BUFFERED_EVENTS + 3);
    let RunEvent::Progress { progress } = &events[MAX_BUFFERED_EVENTS - 1] else {
        panic!("the merged progress");
    };
    assert_eq!(progress.line, format!("line {}", MAX_BUFFERED_EVENTS + 49));
    assert_eq!(
        progress.count.as_ref().map(|c| c.done),
        Some(MAX_BUFFERED_EVENTS as u64 + 49)
    );
    assert!(matches!(events.last(), Some(RunEvent::Ended { .. })));
}

#[test]
fn a_snapshot_follows_the_phase() {
    let fake = Fake::new();
    let run = plain_run(&fake, fake.setup());
    assert_eq!(run.snapshot().state, RunState::Running);
    run.emit(RunEvent::Paused {
        reason: ordin_runtime_adapter::PauseReason::SignIn,
    });
    assert_eq!(run.snapshot().state, RunState::Paused);
    run.emit(RunEvent::Resumed);
    assert_eq!(run.snapshot().state, RunState::Running);
    run.emit_end(RunEndReason::StoppedAtLimit, None);
    let snapshot = run.snapshot();
    assert_eq!(snapshot.state, RunState::StoppedAtLimit);
    assert!(snapshot.end.is_some());
}

// Scenario: An agent run's first turn asks for progress as it happens (CLA-PROGRESS-1).
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-PROGRESS-1
#[test]
fn an_agent_runs_first_turn_asks_for_progress_as_it_happens() {
    let fake = Fake::new();
    let run = plain_run(&fake, fake.setup());
    let free = StartInput::FreeText {
        text: "Read the contract".to_owned(),
    };
    let turn = with_progress_line(first_turn(&run, &free, None), &free);
    assert_eq!(turn, format!("Read the contract\n{PROGRESS_LINE}"));
    assert!(PROGRESS_LINE.contains("never mark them all at the end"));
    // A typed command is written as it was.
    let command = StartInput::Command {
        name: "compare".to_owned(),
        arguments: String::new(),
    };
    let typed = first_turn(&run, &command, None);
    assert_eq!(with_progress_line(typed.clone(), &command), typed);
}

// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-PROGRESS-1
#[test]
fn only_a_run_with_the_work_server_is_asked_for_progress() {
    assert!(lists_work_server(
        &serde_json::json!({"mcpServers": {"work": {}, "memory": {}}})
    ));
    assert!(!lists_work_server(
        &serde_json::json!({"mcpServers": {"ordin": {}}})
    ));
    assert!(!lists_work_server(&serde_json::json!({})));
}
