//! Behaviour the scenarios leave out: profile changes, the session servers' lifetime, updates,
//! removals, conversions in their other forms, and what is never logged.

use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use ordin_organisation_profile::ProfilePlatform;
use ordin_types::domain::places::{CopyFormat, CopyMaker};
use ordin_types::time::Timestamp;
use ordin_types::RunId;
use serde_json::json;
use wiremock::MockServer;

use super::support::*;
use crate::definitions::AppWindow;
use crate::definitions::{CraftToolDefinition, PathRole};
use crate::opening::{system_command, window_command};
use crate::{CraftCliMode, CraftInstallationStatus, CraftPlatform, MakeCopy, ProgramCheckState};

const MCP: &[CraftCliMode] = &[CraftCliMode::Mcp];

fn setup(tools: Vec<CraftToolDefinition>) -> Setup {
    Setup {
        apps: vec![definition("photocraft", tools)],
        craft: section(vec![pinned("photocraft", "0.7.0")], &[]),
        ..Setup::default()
    }
}

fn copy_of(markdown: &str, format: CopyFormat) -> MakeCopy {
    MakeCopy {
        original_name: "deck.md".to_owned(),
        original_from: "Brand".to_owned(),
        branch: None,
        markdown: markdown.to_owned(),
        format,
        updated_at: Timestamp::now(),
    }
}

async fn until(what: &str, check: impl Fn() -> bool) {
    for _ in 0..200 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{what} did not happen");
}

#[tokio::test]
async fn a_new_profile_replaces_the_map_and_starts_no_download() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize(), tool("crop", &[])])],
        craft: section(
            vec![pinned("photocraft", "0.7.0")],
            &[
                ("png", "photocraft"),
                ("jpg", "photocraft"),
                ("xyz", "gone"),
            ],
        ),
        ..Setup::default()
    })
    .await;
    let entry = rig.toolkit.file_kind(".PNG").expect("mapped");
    assert_eq!(entry.open_with_app.as_deref(), Some("photocraft"));
    assert_eq!(entry.craft_tools, ["resize", "crop"]);
    assert_eq!(entry.extensions, ["jpg", "png"]);
    assert_eq!(entry.media_types, ["image/jpeg", "image/png"]);
    assert_eq!(entry.profile_version, "1.0.0");
    let other = rig.toolkit.file_kind("xyz").expect("mapped");
    assert_eq!(
        other.open_with_app, None,
        "not an approved app: the system default"
    );
    assert!(other.craft_tools.is_empty());

    let mut narrower = pinned("photocraft", "0.7.0");
    narrower.allowed_tools = Some(vec!["crop".to_owned(), "absent".to_owned()]);
    rig.feed
        .send(snapshot(
            "1.1.0",
            section(vec![narrower], &[("PNG", "photocraft"), ("BAD!", "x")]),
        ))
        .unwrap();
    until("the new map", || {
        rig.toolkit
            .file_kind("png")
            .is_some_and(|e| e.profile_version == "1.1.0")
    })
    .await;
    assert!(rig.toolkit.file_kind("jpg").is_none());
    assert_eq!(rig.toolkit.file_kind("png").unwrap().craft_tools, ["crop"]);
    assert!(
        rig.toolkit.installations().await.unwrap().is_empty(),
        "no row, no download"
    );

    // The rows survive a restart.
    rig.toolkit.shutdown().await;
    rig.toolkit.reload_entries().await.unwrap();
    assert!(rig.toolkit.file_kind("png").is_some());
}

#[tokio::test]
async fn a_removed_app_goes_with_its_folder_and_can_come_back() {
    let rig = Rig::new(setup(vec![resize()])).await;
    let fake = FakeApp::with(vec![("resize", Does::Write)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    assert_eq!(rig.offered(RunId::new()).await, ["resize"]);
    let folder = rig.data_root.join("craft/photocraft");
    assert!(folder.exists());

    rig.feed
        .send(snapshot("1.1.0", section(vec![], &[])))
        .unwrap();
    rig.toolkit.sync_installations().await.unwrap();

    let rows = rig.toolkit.installations().await.unwrap();
    assert_eq!(rows[0].status, CraftInstallationStatus::Removed);
    assert!(!folder.exists());
    assert!(rig.provider.servers().is_empty());
    assert!(rig.changed().contains(&"craft_photocraft".to_owned()));

    // Back in the profile, it needs an asset for this computer like any new app.
    let mut back = pinned("photocraft", "0.7.1");
    back.platforms.clear();
    rig.feed
        .send(snapshot("1.2.0", section(vec![back], &[])))
        .unwrap();
    let error = rig.toolkit.sync_installations().await.unwrap_err();
    assert_eq!(error.code(), "CRAFT_PLATFORM_UNSUPPORTED");
    let rows = rig.toolkit.installations().await.unwrap();
    assert_eq!(rows[0].status, CraftInstallationStatus::Approved);
    assert_eq!(rows[0].pinned_version, "0.7.1");
}

#[tokio::test]
async fn a_changed_pin_updates_the_app_and_deletes_the_old_version() {
    let mirror = MockServer::start().await;
    let archive = zip_of(&[(
        "photocraft-cli",
        FakeApp::with(vec![("resize", Does::Write)])
            .script()
            .as_bytes(),
        0o755,
    )]);
    let hash = sha256_of(&archive);
    let url = serve(&mirror, "/p.zip", 200, archive, 1).await;
    let rig = Rig::new(setup(vec![resize()])).await;
    let fake = FakeApp::with(vec![("resize", Does::Write)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    assert_eq!(rig.offered(run).await, ["resize"]);

    let new_pin = app(
        "photocraft",
        "0.8.0",
        vec![(ProfilePlatform::LinuxX86_64, asset(&url, &hash, "p.zip"))],
    );
    rig.feed
        .send(snapshot("1.1.0", section(vec![new_pin], &[])))
        .unwrap();
    let report = rig.toolkit.sync_installations().await.unwrap();

    assert_eq!(
        report.installations[0].installed_version.as_deref(),
        Some("0.8.0")
    );
    assert!(!rig.data_root.join("craft/photocraft/0.7.0").exists());
    assert!(rig
        .data_root
        .join("craft/photocraft/0.8.0/photocraft-cli")
        .exists());
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn a_blocked_app_is_tried_again_and_a_changed_hash_is_downloaded_again() {
    let mirror = MockServer::start().await;
    let refused = zip_of(&[("photocraft-cli", b"#!/bin/sh\n", 0o644)]);
    let refused_hash = sha256_of(&refused);
    let url = serve(&mirror, "/a.zip", 200, refused, 3).await;
    let pin = |url: &str, hash| {
        app(
            "photocraft",
            "0.7.0",
            vec![(ProfilePlatform::LinuxX86_64, asset(url, hash, "a.zip"))],
        )
    };
    let rig = Rig::new(Setup {
        craft: section(vec![pin(&url, &refused_hash)], &[]),
        ..Setup::default()
    })
    .await;
    assert_eq!(
        rig.toolkit
            .sync_installations()
            .await
            .unwrap()
            .blocked
            .len(),
        1
    );
    assert_eq!(
        rig.toolkit
            .sync_installations()
            .await
            .unwrap()
            .blocked
            .len(),
        1
    );

    // A hash failure holds until the pin's hash changes.
    let wrong = sha256_of(b"wrong");
    rig.feed
        .send(snapshot("1.1.0", section(vec![pin(&url, &wrong)], &[])))
        .unwrap();
    let error = rig.toolkit.sync_installations().await;
    assert_eq!(error.unwrap_err().code(), "CRAFT_HASH_MISMATCH");
    assert!(
        rig.toolkit.sync_installations().await.is_ok(),
        "not downloaded again"
    );
}

#[tokio::test]
async fn a_program_missing_from_the_archive_is_an_install_failure() {
    let mirror = MockServer::start().await;
    let archive = zip_of(&[("readme.txt", b"hello", 0o644), ("docs/", b"", 0o755)]);
    let hash = sha256_of(&archive);
    let url = serve(&mirror, "/p.zip", 200, archive, 1).await;
    let rig = Rig::new(Setup {
        craft: section(
            vec![app(
                "photocraft",
                "0.7.0",
                vec![(ProfilePlatform::LinuxX86_64, asset(&url, &hash, "p.zip"))],
            )],
            &[],
        ),
        ..Setup::default()
    })
    .await;
    let error = rig.toolkit.sync_installations().await.unwrap_err();
    assert_eq!(error.code(), "CRAFT_INSTALL_FAILED");
    let checks = rig.toolkit.check_apps(false).await.unwrap();
    assert!(matches!(checks[0].state, ProgramCheckState::Refused { .. }));
}

#[tokio::test]
async fn a_program_with_no_output_before_the_limit_is_blocked() {
    let mirror = MockServer::start().await;
    let archive = zip_of(&[
        ("slowcraft-cli", b"#!/bin/sh\nexec sleep 30\n", 0o755),
        ("mutecraft-cli", b"#!/bin/sh\nexit 0\n", 0o755),
    ]);
    let hash = sha256_of(&archive);
    let slow = serve(&mirror, "/s.zip", 200, archive.clone(), 1).await;
    let mute = serve(&mirror, "/m.zip", 200, archive, 1).await;
    let mut setup = Setup {
        craft: section(
            vec![
                app(
                    "slowcraft",
                    "1.0.0",
                    vec![(ProfilePlatform::LinuxX86_64, asset(&slow, &hash, "s.zip"))],
                ),
                app(
                    "mutecraft",
                    "1.0.0",
                    vec![(ProfilePlatform::LinuxX86_64, asset(&mute, &hash, "m.zip"))],
                ),
            ],
            &[],
        ),
        ..Setup::default()
    };
    setup.limits.check = Duration::from_millis(300);
    let rig = Rig::new(setup).await;
    let report = rig.toolkit.sync_installations().await.unwrap();
    assert_eq!(report.blocked.len(), 2);
    assert!(report
        .blocked
        .iter()
        .all(|b| b.reason.contains("no output")));
}

#[tokio::test]
async fn an_unusable_app_name_gets_no_row_or_no_server() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photo-craft", vec![resize()])],
        craft: section(
            vec![pinned("Not/Valid", "1.0.0"), pinned("photo-craft", "1.0.0")],
            &[],
        ),
        ..Setup::default()
    })
    .await;
    rig.seed("photo-craft", "1.0.0", MCP, &FakeApp::with(vec![]))
        .await;
    assert!(rig.toolkit.sync_installations().await.is_ok());
    let rows = rig.toolkit.installations().await.unwrap();
    assert_eq!(rows.len(), 1, "the invalid name gets no row");
    assert!(
        rig.provider.servers().is_empty(),
        "no valid server name for a hyphen"
    );
    let items = rig.toolkit.start_up_items(&grant(RunId::new())).await;
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn an_app_that_will_not_start_offers_nothing_and_a_slow_one_offers_its_defined_tools() {
    let mut slow = setup(vec![resize()]);
    slow.limits.offer = Duration::from_millis(200);
    let rig = Rig::new(slow).await;
    let mut fake = FakeApp::with(vec![("resize", Does::Write)]);
    fake.hang_at_start = true;
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    assert_eq!(rig.offered(RunId::new()).await, ["resize"]);
    rig.toolkit.shutdown().await;

    let mut failing = setup(vec![resize()]);
    failing.limits.start = Duration::from_millis(100);
    let rig = Rig::new(failing).await;
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    assert!(rig.offered(RunId::new()).await.is_empty());
}

#[tokio::test]
async fn the_app_s_schema_is_learnt_and_announced_once() {
    let rig = Rig::new(setup(vec![resize()])).await;
    let fake = FakeApp::with(vec![("resize", Does::Write)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let bare = rig.provider.servers().remove(0).tools.remove(0);
    assert_eq!(bare.input_schema, json!({ "type": "object" }));
    assert_eq!(bare.description, "Does resize.");
    assert_eq!(
        bare.argument_form,
        "resize(input: full path, output: full path)"
    );
    assert!(!bare.sandbox_callable);
    assert_eq!(bare.plain, "PhotoCraft can change files for this agent.");

    rig.offered(RunId::new()).await;
    rig.offered(RunId::new()).await;

    let learnt = rig.provider.servers().remove(0).tools.remove(0);
    assert_eq!(learnt.input_schema["properties"]["width"]["type"], "number");
    assert_eq!(
        learnt.description, "Does resize.",
        "the definition's line, never the app's"
    );
    assert_eq!(rig.changed(), ["craft_photocraft"]);
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn a_closed_session_stops_its_server_and_deletes_its_scratch_folder() {
    let rig = Rig::new(setup(vec![resize()])).await;
    let fake = FakeApp::with(vec![("resize", Does::Write)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    rig.offered(run).await;
    let folder = rig.data_root.join("craft-scratch").join(run.to_string());
    assert!(folder.is_dir());

    rig.provider.session_closed(run).await;

    assert!(!folder.exists());
    assert!(rig.toolkit.servers_running() == 0);
}

#[tokio::test]
async fn an_idle_server_is_stopped_and_starts_again_on_the_next_call() {
    let mut short = setup(vec![resize(), tool("info", &[])]);
    short.limits.idle = Duration::from_millis(50);
    short.limits.sweep = Duration::from_millis(50);
    let rig = Rig::new(short).await;
    let fake = FakeApp::with(vec![("info", Does::Nothing)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    rig.call(run, "photocraft", "info", json!({}))
        .await
        .unwrap();
    until("the idle stop", || rig.toolkit.servers_running() == 0).await;
    until("the scratch folder's removal", || {
        !rig.data_root
            .join("craft-scratch")
            .join(run.to_string())
            .exists()
    })
    .await;
    rig.call(run, "photocraft", "info", json!({}))
        .await
        .unwrap();
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn at_max_servers_the_least_recently_used_idle_server_is_stopped() {
    let mut small = setup(vec![tool("info", &[])]);
    small.limits.max_servers = 2;
    let rig = Rig::new(small).await;
    let fake = FakeApp::with(vec![("info", Does::Nothing)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    for _ in 0..3 {
        rig.call(RunId::new(), "photocraft", "info", json!({}))
            .await
            .unwrap();
    }
    assert_eq!(rig.toolkit.servers_running(), 2);

    // All busy: nothing can be stopped, so the new session fails to start one.
    let mut full = setup(vec![tool("info", &[]), tool("wait", &[])]);
    full.limits.max_servers = 1;
    let rig = Arc::new(Rig::new(full).await);
    let fake = FakeApp::with(vec![("wait", Does::Hang), ("info", Does::Nothing)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let busy = Arc::clone(&rig);
    let held = tokio::spawn(async move {
        busy.call(RunId::new(), "photocraft", "wait", json!({}))
            .await
    });
    until("the first server", || rig.toolkit.servers_running() == 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let failure = rig
        .call(RunId::new(), "photocraft", "info", json!({}))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_SERVER_START_FAILED");
    held.abort();
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn a_cancelled_call_ends_like_a_timeout() {
    let rig = Rig::new(setup(vec![tool("wait", &[])])).await;
    let fake = FakeApp::with(vec![("wait", Does::Hang)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let context = rig.context(RunId::new(), Duration::from_secs(60));
    let cancel = context.cancel.clone();
    let cancelling = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel();
    });
    let failure = rig
        .provider
        .call(context, &key("photocraft", "wait"), json!({}))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_TIMEOUT");
    cancelling.await.unwrap();
    assert_eq!(rig.toolkit.servers_running(), 0);
}

#[tokio::test]
async fn the_app_runs_with_a_cleared_environment_in_the_scratch_folder() {
    let env_tool = tool("env", &[("output", PathRole::Output)]);
    let cwd_tool = tool("cwd", &[("output", PathRole::Output)]);
    let rig = Rig::new(setup(vec![env_tool, cwd_tool])).await;
    let fake = FakeApp::with(vec![("env", Does::WriteEnv), ("cwd", Does::WriteCwd)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    let output = rig.path_in_place("env.txt");
    rig.call(run, "photocraft", "env", json!({ "output": output }))
        .await
        .unwrap();
    let seen = std::fs::read_to_string(&output).unwrap();
    for line in seen.lines() {
        let name = line.split('=').next().unwrap_or_default();
        assert!(
            [
                "PATH",
                "HOME",
                "TMPDIR",
                "LANG",
                "PWD",
                "SHLVL",
                "_",
                "OLDPWD",
                "USERPROFILE",
                "TEMP",
                "SYSTEMROOT"
            ]
            .contains(&name),
            "{name} reached the app"
        );
    }
    let output = rig.path_in_place("cwd.txt");
    rig.call(run, "photocraft", "cwd", json!({ "output": output }))
        .await
        .unwrap();
    let folder = rig.data_root.join("craft-scratch").join(run.to_string());
    let seen = std::fs::read_to_string(&output).unwrap();
    assert!(
        seen.trim_end().ends_with(&run.to_string()),
        "{seen} in {}",
        folder.display()
    );
    let calls = rig.entries("craft_tool_called").await;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["result"], "ok");
    assert_eq!(calls[0]["outputs"], 1);
    let recorded = calls[0].to_string();
    assert!(
        !recorded.contains("env.txt"),
        "no argument value in the journal"
    );
    assert_eq!(calls[0]["arguments_digest"].as_str().unwrap().len(), 64);
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn slides_take_the_banner_as_a_footer_or_in_the_first_slide() {
    let with_footer = || {
        let mut made = tool(
            "to_deck",
            &[("input", PathRole::Input), ("output", PathRole::Output)],
        );
        made.converts_markdown_to = Some(CopyFormat::Slides);
        made.footer_arg = Some("footer".to_owned());
        made
    };
    let rig = Rig::new(Setup {
        apps: vec![definition("deckcraft", vec![with_footer()])],
        craft: section(vec![pinned("deckcraft", "2.0.0")], &[]),
        ..Setup::default()
    })
    .await;
    let mut fake = FakeApp::with(vec![("to_deck", Does::Copy)]);
    let record = rig.dir.0.join("calls.log");
    fake.record = Some(record.clone());
    rig.seed("deckcraft", "2.0.0", MCP, &fake).await;
    let made = rig
        .toolkit
        .make_copy(copy_of("# One\n", CopyFormat::Slides))
        .await
        .map_err(|e| e.code())
        .expect("a copy");
    assert_eq!(made.bytes, b"# One\n", "the Markdown is untouched");
    assert_eq!(made.file_name, "deck.pptx");
    assert_eq!(made.maker, CopyMaker::DeckCraft);
    let seen = std::fs::read_to_string(&record).unwrap();
    assert!(seen.contains(r#""footer":"Published copy of deck.md, from Brand. Last updated"#));
    rig.toolkit.shutdown().await;

    let mut plain = with_footer();
    plain.footer_arg = None;
    let rig = Rig::new(Setup {
        apps: vec![definition("deckcraft", vec![plain])],
        craft: section(vec![pinned("deckcraft", "2.0.0")], &[]),
        ..Setup::default()
    })
    .await;
    let fake = FakeApp::with(vec![("to_deck", Does::Copy)]);
    rig.seed("deckcraft", "2.0.0", MCP, &fake).await;
    let made = rig
        .toolkit
        .make_copy(copy_of("# One\n\n# Two\n", CopyFormat::Slides))
        .await
        .map_err(|e| e.code())
        .expect("a copy");
    let text = String::from_utf8(made.bytes).unwrap();
    assert!(text.starts_with("# One\n\nPublished copy of deck.md"));
    assert!(text.ends_with("# Two\n"));
    rig.toolkit.shutdown().await;
}

fn word_to_markdown() -> CraftToolDefinition {
    let mut made = tool(
        "from_word",
        &[("input", PathRole::Input), ("output", PathRole::Output)],
    );
    made.converts_to_markdown_from = Some(CopyFormat::Word);
    made
}

#[tokio::test]
async fn to_markdown_runs_once_a_definition_carries_the_tool() {
    let mut limited = Setup {
        apps: vec![definition(
            "wordcraft",
            vec![word_to_markdown(), tool("bad", &[])],
        )],
        craft: section(vec![pinned("wordcraft", "1.4.0")], &[]),
        ..Setup::default()
    };
    limited.limits.max_output = 64;
    let rig = Rig::new(limited).await;
    let fake = FakeApp::with(vec![("from_word", Does::Copy)]);
    rig.seed("wordcraft", "1.4.0", MCP, &fake).await;
    let markdown = rig
        .toolkit
        .to_markdown(CopyFormat::Word, b"# Rates\n".to_vec())
        .await
        .unwrap();
    assert_eq!(markdown, "# Rates\n");
    assert_eq!(rig.entries("craft_markdown_made").await.len(), 1);
    let error = rig
        .toolkit
        .to_markdown(CopyFormat::Word, vec![b'x'; 65])
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CRAFT_OUTPUT_TOO_LARGE");
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn to_markdown_refuses_output_that_is_not_utf8_and_reports_the_app_s_failures() {
    let rig = Rig::new(Setup {
        apps: vec![definition("wordcraft", vec![word_to_markdown()])],
        craft: section(vec![pinned("wordcraft", "1.4.0")], &[]),
        ..Setup::default()
    })
    .await;
    let fake = FakeApp::with(vec![("from_word", Does::WriteBadUtf8)]);
    rig.seed("wordcraft", "1.4.0", MCP, &fake).await;
    let error = rig
        .toolkit
        .to_markdown(CopyFormat::Word, b"x".to_vec())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CRAFT_TOOL_FAILED");
    rig.toolkit.shutdown().await;

    let rig = Rig::new(Setup {
        apps: vec![definition("wordcraft", vec![word_to_markdown()])],
        craft: section(vec![pinned("wordcraft", "1.4.0")], &[]),
        ..Setup::default()
    })
    .await;
    let fake = FakeApp::with(vec![("from_word", Does::Fail("no".to_owned()))]);
    rig.seed("wordcraft", "1.4.0", MCP, &fake).await;
    let error = rig
        .toolkit
        .to_markdown(CopyFormat::Word, b"x".to_vec())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CRAFT_TOOL_FAILED");
    rig.toolkit.shutdown().await;

    // A tool the app does not list, or a definition with no output argument, makes no copy.
    let mut one_way = word_to_markdown();
    one_way.path_args.pop();
    let rig = Rig::new(Setup {
        apps: vec![definition("wordcraft", vec![one_way])],
        craft: section(vec![pinned("wordcraft", "1.4.0")], &[]),
        ..Setup::default()
    })
    .await;
    rig.seed("wordcraft", "1.4.0", MCP, &FakeApp::with(vec![]))
        .await;
    let error = rig
        .toolkit
        .to_markdown(CopyFormat::Word, b"x".to_vec())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CRAFT_CONVERSION_UNAVAILABLE");
    let rig = Rig::new(Setup {
        apps: vec![definition("wordcraft", vec![word_to_markdown()])],
        craft: section(vec![pinned("wordcraft", "1.4.0")], &[]),
        ..Setup::default()
    })
    .await;
    rig.seed("wordcraft", "1.4.0", MCP, &FakeApp::with(vec![]))
        .await;
    let error = rig
        .toolkit
        .to_markdown(CopyFormat::Word, b"x".to_vec())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CRAFT_CONVERSION_UNAVAILABLE");
    rig.toolkit.shutdown().await;
}

#[test]
fn each_platform_opens_with_its_own_command() {
    let window = AppWindow {
        macos_bundle: Some("PhotoCraft.app".to_owned()),
        windows_exe: Some("PhotoCraft.exe".to_owned()),
        linux_exe: Some("photocraft".to_owned()),
    };
    let folder = std::path::Path::new("/d/craft/photocraft/0.7.0");
    let file = std::path::Path::new("/b/logo.png");
    let (program, args) = window_command(CraftPlatform::MacosArm64, folder, &window, file).unwrap();
    assert_eq!(program, "open");
    assert_eq!(args[0], "-a");
    assert_eq!(args[1], "/d/craft/photocraft/0.7.0/PhotoCraft.app");
    assert_eq!(args[2], "/b/logo.png");
    let (program, args) =
        window_command(CraftPlatform::WindowsX86_64, folder, &window, file).unwrap();
    assert_eq!(program, "/d/craft/photocraft/0.7.0/PhotoCraft.exe");
    assert_eq!(args, ["/b/logo.png"]);
    let none = AppWindow {
        macos_bundle: None,
        windows_exe: None,
        linux_exe: None,
    };
    assert!(window_command(CraftPlatform::LinuxX86_64, folder, &none, file).is_none());
    assert_eq!(system_command(CraftPlatform::MacosX86_64, file).0, "open");
    assert_eq!(
        system_command(CraftPlatform::WindowsX86_64, file).0,
        "explorer.exe"
    );
    assert_eq!(
        system_command(CraftPlatform::LinuxX86_64, file).0,
        "xdg-open"
    );
}

#[tokio::test]
async fn an_installed_app_with_no_definition_has_no_known_window() {
    let rig = Rig::new(Setup {
        craft: section(
            vec![pinned("photocraft", "0.7.0")],
            &[("png", "photocraft")],
        ),
        ..Setup::default()
    })
    .await;
    rig.seed("photocraft", "0.7.0", MCP, &FakeApp::with(vec![]))
        .await;
    rig.file_in_place("logo.png", b"x");
    let outcome = rig
        .toolkit
        .open_in_app(rig.place.id, "logo.png")
        .await
        .unwrap();
    assert!(matches!(outcome, crate::OpenOutcome::NoEditor));
}

#[tokio::test]
async fn a_refused_window_shows_in_the_setup_check() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![])],
        craft: section(
            vec![pinned("photocraft", "0.7.0")],
            &[("png", "photocraft")],
        ),
        ..Setup::default()
    })
    .await;
    rig.toolkit
        .state
        .lock()
        .unwrap()
        .rows
        .insert("photocraft".to_owned(), {
            let mut row = crate::store::Row::approved("photocraft", "0.7.0");
            row.status = CraftInstallationStatus::Installed;
            row.installed_version = Some("0.7.0".to_owned());
            row
        });
    rig.file_in_place("logo.png", b"x");
    *rig.launcher.refuse.lock().unwrap() = Some(std::io::ErrorKind::PermissionDenied);
    assert!(rig
        .toolkit
        .open_in_app(rig.place.id, "logo.png")
        .await
        .is_err());
    let checks = rig.toolkit.check_apps(false).await.unwrap();
    assert!(matches!(checks[0].state, ProgramCheckState::Refused { .. }));
}

#[tokio::test]
async fn migrations_register_module_craft_on_sqlite_only() {
    let migrations = crate::CraftToolkit::migrations();
    assert_eq!(migrations.module, "craft");
    assert_eq!(migrations.sqlite.len(), 1);
    assert!(migrations.postgres.is_empty());
    let rig = Rig::new(Setup::default()).await;
    // The row rule is the table's too. The table's CHECK lets a NULL `verification_result` through
    // (a NULL comparison passes a CHECK), which is why `update_row` guards it as well.
    let refused = sqlx::query(
        "INSERT INTO craft_installations (id, app_name, pinned_version, verification_result, \
         status, updated_at) VALUES ('1', 'a', '1', 'failed', 'installed', 'now')",
    )
    .execute(rig.pool())
    .await;
    assert!(refused.is_err());
}

// ---- logs ----

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static LOG_INIT: Once = Once::new();

struct Capture(Vec<u8>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let text = String::from_utf8_lossy(&self.0).into_owned();
        LOGS.lock().unwrap().push(text);
    }
}

struct CaptureMaker;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureMaker {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        Capture(Vec::new())
    }
}

fn capture_logs() {
    LOG_INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_env_filter(tracing_subscriber::EnvFilter::new(
                "warn,ordin_craft_toolkit=debug",
            ))
            .with_writer(CaptureMaker)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

/// Performance / Security / Observability: the events are written with `component = "craft"`, and
/// argument values, file paths, content and the app's result text never are.
#[tokio::test]
async fn events_name_the_component_and_never_hold_a_path_a_value_or_a_result() {
    capture_logs();
    let definitions = vec![
        definition("photocraft", vec![resize(), tool("bad", &[])]),
        {
            let mut wrong = definition("pdfcraft", vec![]);
            wrong.versions = "not a requirement".to_owned();
            wrong
        },
    ];
    let rig = Rig::new(Setup {
        apps: definitions,
        craft: section(
            vec![pinned("photocraft", "0.7.0")],
            &[("BAD!", "photocraft")],
        ),
        ..Setup::default()
    })
    .await;
    let fake = FakeApp::with(vec![
        ("resize", Does::Write),
        ("bad", Does::Fail("secret text from the app".to_owned())),
    ]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    let input = rig.file_in_place("private-logo.png", b"private bytes");
    let output = rig.path_in_place("private-out.png");
    rig.call(
        run,
        "photocraft",
        "resize",
        json!({ "input": input, "output": output }),
    )
    .await
    .unwrap();
    assert!(rig.call(run, "photocraft", "bad", json!({})).await.is_err());
    rig.provider.session_closed(run).await;

    let lines = LOGS.lock().unwrap().join("");
    for event in [
        "definition_invalid",
        "craft_server_started",
        "craft_server_stopped",
        "craft_tool_called",
        "craft_extension_skipped",
    ] {
        assert!(lines.contains(event), "{event} was written");
    }
    for line in lines.lines().filter(|l| l.contains("craft_tool_called")) {
        assert!(line.contains(r#""component":"craft""#));
    }
    assert!(!lines.contains("private-logo"));
    assert!(!lines.contains("private-out"));
    assert!(!lines.contains("secret text"));
}
