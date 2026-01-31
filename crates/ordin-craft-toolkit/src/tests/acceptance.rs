//! One test per Gherkin scenario of CraftToolkit.md (X2), named after it.

use std::path::Path;
use std::time::Duration;

use ordin_organisation_profile::ProfilePlatform;
use ordin_types::domain::places::{CopyFormat, CopyMaker, FileVersionOrigin};
use ordin_types::time::Timestamp;
use ordin_types::wire::startup::{StartUpReportItem, UnavailableReason};
use ordin_types::RunId;
use serde_json::json;
use wiremock::MockServer;

use super::support::*;
use crate::definitions::PathRole;
use crate::{
    CraftCliMode, CraftInstallationStatus, MakeCopy, OpenOutcome, ProgramCheckState,
    VerificationResult,
};

const MCP: &[CraftCliMode] = &[CraftCliMode::Mcp];

/// The app's zip: its program at the top, executable.
fn zip_with(cli: &str, fake: &FakeApp) -> Vec<u8> {
    zip_of(&[(cli, fake.script().as_bytes(), 0o755)])
}

fn converting(name: &str, format: CopyFormat) -> crate::definitions::CraftToolDefinition {
    let mut made = tool(
        name,
        &[("input", PathRole::Input), ("output", PathRole::Output)],
    );
    made.converts_markdown_to = Some(format);
    made
}

/// `asset_url` keeps the scenario's path on the loopback stand-in: real network is never used.
#[tokio::test]
async fn happy_path_install_an_approved_app_from_the_mirror() {
    let mirror = MockServer::start().await;
    let archive = zip_with("photocraft-cli", &FakeApp::with(vec![]));
    let hash = sha256_of(&archive);
    let file_name = "photocraft-cli-0.7.0-macos-universal.zip";
    let at = format!("/craft/photocraft/0.7.0/{file_name}");
    let url = serve(&mirror, &at, 200, archive, 1).await;
    let rig = Rig::new(Setup {
        platform: crate::CraftPlatform::MacosArm64,
        apps: vec![definition("photocraft", vec![resize()])],
        craft: section(
            vec![app(
                "photocraft",
                "0.7.0",
                vec![(ProfilePlatform::MacosArm64, asset(&url, &hash, file_name))],
            )],
            &[],
        ),
        ..Setup::default()
    })
    .await;

    let report = rig.toolkit.sync_installations().await.unwrap();

    assert!(report.blocked.is_empty());
    let installed = &report.installations[0];
    assert_eq!(installed.status, CraftInstallationStatus::Installed);
    assert_eq!(
        installed.verification_result,
        Some(VerificationResult::Verified)
    );
    assert_eq!(installed.installed_version.as_deref(), Some("0.7.0"));
    assert!(installed.cli_modes.contains(&CraftCliMode::Mcp));
    let entries = rig.entries("craft_installed").await;
    assert_eq!(entries[0]["app"], "photocraft");
    assert!(rig
        .data_root
        .join("craft/photocraft/0.7.0/photocraft-cli")
        .is_file());
    assert!(!rig.data_root.join("craft/photocraft/0.7.0.tmp").exists());
    assert!(!rig
        .data_root
        .join("craft-scratch/downloads")
        .join(format!("{file_name}.part"))
        .exists());
}

#[tokio::test]
async fn error_path_hash_mismatch_is_never_installed() {
    let mirror = MockServer::start().await;
    let url = serve(&mirror, "/p.zip", 200, b"tampered".to_vec(), 1).await;
    let rig = Rig::new(Setup {
        craft: section(
            vec![app(
                "photocraft",
                "0.7.0",
                vec![(
                    ProfilePlatform::LinuxX86_64,
                    asset(&url, &sha256_of(b"pinned"), "p.zip"),
                )],
            )],
            &[],
        ),
        ..Setup::default()
    })
    .await;

    assert!(rig.toolkit.sync_installations().await.is_err());

    let rows = rig.toolkit.installations().await.unwrap();
    assert_eq!(rows[0].status, CraftInstallationStatus::VerificationFailed);
    assert_eq!(
        rows[0].verification_result,
        Some(VerificationResult::Failed)
    );
    assert!(!rig
        .data_root
        .join("craft-scratch/downloads/p.zip.part")
        .exists());
    assert!(
        !rig.data_root.join("craft/photocraft").exists(),
        "nothing is unpacked"
    );
    assert_eq!(rig.entries("craft_verification_failed").await.len(), 1);
    // Permanent until the profile pins another file: a second sync downloads nothing (`expect(1)`).
    assert!(rig.toolkit.sync_installations().await.is_ok());
}

#[tokio::test]
async fn error_path_the_operating_system_blocks_the_program() {
    let mirror = MockServer::start().await;
    // A program the system will not execute: its file has no execute permission.
    let blocked = zip_of(&[("photocraft-cli", b"#!/bin/sh\n", 0o644)]);
    let fine = zip_with("wordcraft-cli", &FakeApp::with(vec![]));
    let (blocked_hash, fine_hash) = (sha256_of(&blocked), sha256_of(&fine));
    let blocked_url = serve(&mirror, "/p.zip", 200, blocked, 1).await;
    let fine_url = serve(&mirror, "/w.zip", 200, fine, 1).await;
    let rig = Rig::new(Setup {
        craft: section(
            vec![
                app(
                    "photocraft",
                    "0.7.0",
                    vec![(
                        ProfilePlatform::LinuxX86_64,
                        asset(&blocked_url, &blocked_hash, "p.zip"),
                    )],
                ),
                app(
                    "wordcraft",
                    "1.4.0",
                    vec![(
                        ProfilePlatform::LinuxX86_64,
                        asset(&fine_url, &fine_hash, "w.zip"),
                    )],
                ),
            ],
            &[],
        ),
        ..Setup::default()
    })
    .await;

    let report = rig.toolkit.sync_installations().await.unwrap();

    assert_eq!(report.blocked.len(), 1);
    assert_eq!(report.blocked[0].program, "photocraft-cli");
    assert_eq!(report.blocked[0].sha256, blocked_hash);
    let status = |name: &str| {
        report
            .installations
            .iter()
            .find(|r| r.app_name == name)
            .unwrap()
            .status
    };
    assert_eq!(status("photocraft"), CraftInstallationStatus::Downloaded);
    assert_eq!(status("wordcraft"), CraftInstallationStatus::Installed);
}

#[tokio::test]
async fn edge_case_an_archive_entry_escaping_the_folder_is_refused() {
    let mirror = MockServer::start().await;
    let archive = zip_of(&[
        ("photocraft-cli", b"#!/bin/sh\n", 0o755),
        ("../../evil", b"evil", 0o644),
    ]);
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
    assert!(!rig.data_root.join("evil").exists());
    assert!(!rig.data_root.join("craft/evil").exists());
    assert!(!rig.data_root.join("craft/photocraft/0.7.0").exists());
    assert!(!rig.data_root.join("craft/photocraft/0.7.0.tmp").exists());
}

#[tokio::test]
async fn happy_path_open_an_image_in_photocraft() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize()])],
        craft: section(
            vec![pinned("photocraft", "0.7.0")],
            &[("png", "photocraft")],
        ),
        ..Setup::default()
    })
    .await;
    rig.seed("photocraft", "0.7.0", MCP, &FakeApp::with(vec![]))
        .await;
    std::fs::create_dir_all(rig.place_root.join("Q3")).unwrap();
    let file = rig.file_in_place("Q3/logo.png", b"png");

    let outcome = rig
        .toolkit
        .open_in_app(rig.place.id, "Q3/logo.png")
        .await
        .unwrap();

    assert!(matches!(outcome, OpenOutcome::OpenedInApp { app } if app == "photocraft"));
    let exe = rig.data_root.join("craft/photocraft/0.7.0/photocraft");
    assert_eq!(rig.opened(), [[exe.into_os_string(), file.into()]]);
    assert_eq!(
        rig.entries("file_opened_in_app").await[0]["app"],
        "photocraft"
    );
}

#[tokio::test]
async fn edge_case_no_editor_for_the_kind() {
    let rig = Rig::new(Setup::default()).await;
    let file = rig.file_in_place("notes.xyz", b"text");

    let outcome = rig
        .toolkit
        .open_in_app(rig.place.id, "notes.xyz")
        .await
        .unwrap();
    assert!(matches!(outcome, OpenOutcome::NoEditor));
    assert!(rig.opened().is_empty());

    let outcome = rig
        .toolkit
        .open_with_system_default(rig.place.id, "notes.xyz")
        .await
        .unwrap();
    assert!(matches!(outcome, OpenOutcome::OpenedWithSystemDefault));
    assert_eq!(
        rig.opened(),
        [["xdg-open".into(), std::ffi::OsString::from(file)]]
    );
}

#[tokio::test]
async fn error_path_a_runnable_file_is_not_opened_in_the_usual_app() {
    let rig = Rig::new(Setup::default()).await;
    for name in [
        "run.command",
        "RUN.COMMAND",
        "a.b.Command",
        "open.lnk",
        "open.url",
        "open.hta",
        "open.desktop",
        "open.exe",
        "open.ps1",
        "trail.bat.",
        "trail.bat ",
    ] {
        rig.file_in_place(name, b"#!/bin/sh\ntouch ran\n");
        let error = rig
            .toolkit
            .open_with_system_default(rig.place.id, name)
            .await
            .err()
            .unwrap_or_else(|| panic!("{name} was opened"));
        assert_eq!(error.code(), "CRAFT_OPEN_REFUSED", "{name}");
        assert!(!error.retryable());
    }
    assert!(rig.opened().is_empty());
}

#[tokio::test]
async fn error_path_a_path_with_a_comma_is_not_given_to_explorer() {
    let rig = Rig::new(Setup {
        platform: crate::CraftPlatform::WindowsX86_64,
        ..Setup::default()
    })
    .await;
    rig.file_in_place("a,b.txt", b"text");
    let error = rig
        .toolkit
        .open_with_system_default(rig.place.id, "a,b.txt")
        .await
        .err()
        .unwrap();
    assert_eq!(error.code(), "CRAFT_OPEN_REFUSED");
    assert!(rig.opened().is_empty());
    rig.file_in_place("plain.txt", b"text");
    rig.toolkit
        .open_with_system_default(rig.place.id, "plain.txt")
        .await
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn error_path_a_file_with_an_execute_bit_is_not_opened_in_the_usual_app() {
    use std::os::unix::fs::PermissionsExt;
    let rig = Rig::new(Setup::default()).await;
    let file = rig.file_in_place("notes.xyz", b"#!/bin/sh\n");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = rig
        .toolkit
        .open_with_system_default(rig.place.id, "notes.xyz")
        .await
        .err()
        .unwrap();
    assert_eq!(error.code(), "CRAFT_OPEN_REFUSED");
    assert!(rig.opened().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn error_path_a_link_to_a_runnable_file_is_read_as_that_file() {
    let rig = Rig::new(Setup::default()).await;
    rig.file_in_place("run.command", b"x");
    std::os::unix::fs::symlink("run.command", rig.place_root.join("notes.txt")).unwrap();
    let error = rig
        .toolkit
        .open_with_system_default(rig.place.id, "notes.txt")
        .await
        .err()
        .unwrap();
    assert_eq!(error.code(), "CRAFT_OPEN_REFUSED");
    assert!(rig.opened().is_empty());
}

#[tokio::test]
async fn happy_path_an_agent_s_output_becomes_a_new_version() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize()])],
        craft: section(vec![pinned("photocraft", "0.7.0")], &[]),
        ..Setup::default()
    })
    .await;
    let mut fake = FakeApp::with(vec![("resize", Does::Write)]);
    let record = rig.dir.0.join("calls.log");
    fake.record = Some(record.clone());
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    let input = rig.file_in_place("logo.png", b"png");
    let output = rig.path_in_place("logo-512.png");

    let result = rig
        .call(
            run,
            "photocraft",
            "resize",
            json!({ "input": input, "output": output, "width": 512 }),
        )
        .await
        .unwrap();

    // The app was given a scratch path in place of the output.
    let seen = std::fs::read_to_string(&record).unwrap();
    assert!(
        !seen.contains(&output),
        "the original path never reaches the app"
    );
    let scratch = rig.data_root.join("craft-scratch").join(run.to_string());
    assert!(seen.contains(&scratch.to_string_lossy().into_owned()));
    assert!(
        seen.contains(r#""width":512"#),
        "other arguments pass unchanged"
    );

    // The File Service recorded it.
    let versions = rig
        .files
        .versions(rig.place.id, "logo-512.png")
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(
        versions[0].origin,
        FileVersionOrigin::Run {
            run_id: run,
            tool: "craft_photocraft.resize".to_owned()
        }
    );
    assert_eq!(
        rig.files
            .read_file(rig.place.id, "logo-512.png")
            .await
            .unwrap()
            .bytes,
        b"PNGDATA"
    );

    // The result names the original path, never the scratch path.
    let structured = result.structured.unwrap();
    assert_eq!(
        structured["written"],
        json!([{ "path": "logo-512.png", "version": 1 }])
    );
    assert_eq!(result.text, structured.to_string());
    let text = structured["app_result"].to_string();
    assert!(text.contains(&output));
    assert!(!text.contains("craft-scratch"));
    assert!(
        rig.scratch_files(run).is_empty(),
        "the scratch file is deleted"
    );
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn error_path_the_app_times_out() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize(), tool("info", &[])])],
        craft: section(vec![pinned("photocraft", "0.7.0")], &[]),
        ..Setup::default()
    })
    .await;
    let fake = FakeApp::with(vec![("resize", Does::Hang), ("info", Does::Nothing)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    let input = rig.file_in_place("logo.png", b"png");

    let failure = rig
        .provider
        .call(
            rig.context(run, Duration::from_millis(400)),
            &key("photocraft", "resize"),
            json!({ "input": input, "output": rig.path_in_place("o.png") }),
        )
        .await
        .unwrap_err();

    assert_eq!(failure.code, "CRAFT_TIMEOUT");
    assert!(rig
        .files
        .versions(rig.place.id, "o.png")
        .await
        .unwrap()
        .is_empty());
    assert!(!rig.place_root.join("o.png").exists());
    // It was stopped: the next call starts it again.
    rig.call(run, "photocraft", "info", json!({}))
        .await
        .unwrap();
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn error_path_the_app_writes_nothing() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize()])],
        craft: section(vec![pinned("photocraft", "0.7.0")], &[]),
        ..Setup::default()
    })
    .await;
    let fake = FakeApp::with(vec![("resize", Does::WriteEmpty)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let input = rig.file_in_place("logo.png", b"png");
    let output = rig.file_in_place("existing.png", b"keep me");

    let failure = rig
        .call(
            RunId::new(),
            "photocraft",
            "resize",
            json!({ "input": input, "output": output }),
        )
        .await
        .unwrap_err();

    assert_eq!(failure.code, "CRAFT_OUTPUT_MISSING");
    assert_eq!(std::fs::read(&output).unwrap(), b"keep me");
    assert!(rig
        .files
        .versions(rig.place.id, "existing.png")
        .await
        .unwrap()
        .is_empty());
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn edge_case_a_tool_missing_from_the_definitions_is_never_offered() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize()])],
        craft: section(vec![pinned("photocraft", "0.7.0")], &[]),
        ..Setup::default()
    })
    .await;
    let fake = FakeApp::with(vec![("resize", Does::Write), ("export_all", Does::Nothing)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;

    assert_eq!(rig.offered(RunId::new()).await, ["resize"]);
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn edge_case_an_app_without_mcp_mode() {
    let rig = Rig::new(Setup {
        apps: vec![definition("gridcraft", vec![])],
        craft: section(vec![pinned("gridcraft", "1.0.0")], &[]),
        ..Setup::default()
    })
    .await;
    let modes = [CraftCliMode::Run, CraftCliMode::Batch];
    rig.seed("gridcraft", "1.0.0", &modes, &FakeApp::with(vec![]))
        .await;

    let items = rig.toolkit.start_up_items(&grant(RunId::new())).await;

    assert_eq!(items.len(), 1);
    match &items[0] {
        StartUpReportItem::UnavailablePart {
            part,
            plain,
            reason,
        } => {
            assert_eq!(part, "craft_gridcraft");
            assert_eq!(
                plain,
                "GridCraft can't be used by agents on this computer yet."
            );
            assert_eq!(*reason, UnavailableReason::MissingDependency);
        }
        other => panic!("unexpected item {other:?}"),
    }
    assert!(rig.offered(RunId::new()).await.is_empty());
}

#[tokio::test]
async fn error_path_word_copies_before_c_28() {
    let rig = Rig::new(Setup {
        apps: vec![definition("wordcraft", vec![])],
        craft: section(vec![pinned("wordcraft", "1.4.0")], &[]),
        ..Setup::default()
    })
    .await;
    rig.seed("wordcraft", "1.4.0", MCP, &FakeApp::with(vec![]))
        .await;

    let error = rig
        .toolkit
        .make_copy(MakeCopy {
            original_name: "rates.md".to_owned(),
            original_from: "supplier-docs".to_owned(),
            branch: None,
            markdown: "# Rates".to_owned(),
            format: CopyFormat::Word,
            updated_at: Timestamp::now(),
        })
        .await
        .err()
        .expect("refused");

    assert_eq!(error.code(), "CRAFT_CONVERSION_UNAVAILABLE");
    assert!(!rig.toolkit.conversion_available(CopyFormat::Word));
}

#[tokio::test]
async fn happy_path_the_setup_assistant_repairs_a_missing_app() {
    let mirror = MockServer::start().await;
    let archive = zip_with("wordcraft-cli", &FakeApp::with(vec![]));
    let hash = sha256_of(&archive);
    let url = serve(&mirror, "/w.zip", 200, archive, 1).await;
    let rig = Rig::new(Setup {
        craft: section(
            vec![app(
                "wordcraft",
                "1.4.0",
                vec![(ProfilePlatform::LinuxX86_64, asset(&url, &hash, "w.zip"))],
            )],
            &[],
        ),
        ..Setup::default()
    })
    .await;

    let checks = rig.toolkit.check_apps(true).await.unwrap();
    assert_eq!(checks[0].app_name, "wordcraft");
    assert!(matches!(checks[0].state, ProgramCheckState::Repaired));

    // Next time it is simply ready, and nothing is downloaded again (`expect(1)`).
    let checks = rig.toolkit.check_apps(true).await.unwrap();
    assert!(matches!(checks[0].state, ProgramCheckState::Ready));
}

#[tokio::test]
async fn error_path_check_apps_reports_an_app_the_system_refuses_to_start() {
    let mirror = MockServer::start().await;
    let archive = zip_of(&[("pdfcraft-cli", b"#!/bin/sh\n", 0o644)]);
    let hash = sha256_of(&archive);
    let url = serve(&mirror, "/p.zip", 200, archive, 1).await;
    let rig = Rig::new(Setup {
        craft: section(
            vec![app(
                "pdfcraft",
                "0.6.0",
                vec![(ProfilePlatform::LinuxX86_64, asset(&url, &hash, "p.zip"))],
            )],
            &[],
        ),
        ..Setup::default()
    })
    .await;
    rig.toolkit.sync_installations().await.unwrap();

    let checks = rig.toolkit.check_apps(false).await.unwrap();

    match &checks[0].state {
        ProgramCheckState::Refused { publisher, sha256 } => {
            assert_eq!(publisher, "unknown");
            assert_eq!(sha256, &hash);
        }
        other => panic!("unexpected state {other:?}"),
    }

    // With no memory of the refusal (a restart), the row and its folder say the same.
    rig.toolkit.state.lock().unwrap().blocked.clear();
    let checks = rig.toolkit.check_apps(false).await.unwrap();
    assert!(matches!(checks[0].state, ProgramCheckState::Refused { .. }));
}

#[tokio::test]
async fn edge_case_an_app_installed_while_ordin_runs_reaches_new_sessions() {
    let mirror = MockServer::start().await;
    let archive = zip_with("deckcraft-cli", &FakeApp::with(vec![]));
    let hash = sha256_of(&archive);
    let url = serve(&mirror, "/d.zip", 200, archive, 1).await;
    let rig = Rig::new(Setup {
        apps: vec![definition("deckcraft", vec![tool("convert", &[])])],
        craft: section(
            vec![app(
                "deckcraft",
                "2.0.0",
                vec![(ProfilePlatform::LinuxX86_64, asset(&url, &hash, "d.zip"))],
            )],
            &[],
        ),
        ..Setup::default()
    })
    .await;
    assert!(rig.provider.servers().is_empty());

    rig.toolkit.sync_installations().await.unwrap();

    assert_eq!(rig.changed(), ["craft_deckcraft"]);
    let servers = rig.provider.servers();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name.as_str(), "craft_deckcraft");
    assert_eq!(servers[0].tools[0].name, "convert");
}

#[tokio::test]
async fn error_path_word_to_markdown_before_c_13() {
    let rig = Rig::new(Setup {
        apps: vec![definition(
            "wordcraft",
            vec![converting("to_word", CopyFormat::Word)],
        )],
        craft: section(vec![pinned("wordcraft", "1.4.0")], &[]),
        ..Setup::default()
    })
    .await;
    rig.seed("wordcraft", "1.4.0", MCP, &FakeApp::with(vec![]))
        .await;

    let error = rig
        .toolkit
        .to_markdown(CopyFormat::Word, b"docx".to_vec())
        .await
        .unwrap_err();

    assert_eq!(error.code(), "CRAFT_CONVERSION_UNAVAILABLE");
}

#[tokio::test]
async fn happy_path_a_pdf_copy_carries_the_line_naming_its_original() {
    let rig = Rig::new(Setup {
        apps: vec![definition(
            "pdfcraft",
            vec![converting("to_pdf", CopyFormat::Pdf)],
        )],
        craft: section(vec![pinned("pdfcraft", "0.6.0")], &[]),
        ..Setup::default()
    })
    .await;
    // The fake copies its Markdown input to its output, so the result shows what it was given.
    let fake = FakeApp::with(vec![("to_pdf", Does::Copy)]);
    rig.seed("pdfcraft", "0.6.0", MCP, &fake).await;
    assert!(rig.toolkit.conversion_available(CopyFormat::Pdf));

    let made = rig
        .toolkit
        .make_copy(MakeCopy {
            original_name: "rates.md".to_owned(),
            original_from: "supplier-docs".to_owned(),
            branch: Some("main".to_owned()),
            markdown: "# Rates\n".to_owned(),
            format: CopyFormat::Pdf,
            updated_at: Timestamp::now(),
        })
        .await
        .map_err(|e| e.code())
        .expect("a copy");

    let given = String::from_utf8(made.bytes).unwrap();
    assert!(given.starts_with("Published copy of rates.md, from supplier-docs, branch main."));
    assert!(given.ends_with("Make changes in the original.\n\n# Rates\n"));
    assert_eq!(made.file_name, "rates.pdf");
    assert_eq!(made.maker, CopyMaker::PdfCraft);
    assert_eq!(rig.entries("craft_copy_made").await[0]["format"], "pdf");
    let left = std::fs::read_dir(rig.data_root.join("craft-scratch/conversions")).unwrap();
    assert_eq!(left.count(), 0, "both scratch files are deleted");
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn an_app_without_an_allowed_tools_list_offers_every_defined_tool() {
    let rig = Rig::new(Setup {
        apps: vec![definition(
            "photocraft",
            vec![resize(), tool("crop", &[]), tool("not_listed", &[])],
        )],
        craft: section(vec![pinned("photocraft", "0.7.0")], &[]),
        ..Setup::default()
    })
    .await;
    assert!(rig.toolkit.craft_profile().apps[0].allowed_tools.is_none());
    let fake = FakeApp::with(vec![
        ("resize", Does::Write),
        ("crop", Does::Nothing),
        ("undefined", Does::Nothing),
    ]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;

    assert_eq!(rig.offered(RunId::new()).await, ["crop", "resize"]);
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn a_pin_with_a_non_https_address_is_dropped() {
    let hash = sha256_of(b"x");
    let rig = Rig::new(Setup {
        craft: section(
            vec![app(
                "photocraft",
                "0.7.0",
                vec![(
                    ProfilePlatform::LinuxX86_64,
                    asset("http://mirror.acme.co.uk/p.zip", &hash, "p.zip"),
                )],
            )],
            &[],
        ),
        ..Setup::default()
    })
    .await;

    let error = rig.toolkit.sync_installations().await.unwrap_err();

    assert_eq!(error.code(), "CRAFT_PLATFORM_UNSUPPORTED");
    assert!(!rig.data_root.join("craft-scratch/downloads").exists());
    let path = Path::new("craft/photocraft");
    assert!(!rig.data_root.join(path).exists());
}
