//! One test per row of CraftToolkit.md's Error Table (X1), each provoking the condition and
//! checking the code and the retryability, plus the registry check (X4).

use std::time::Duration;

use ordin_organisation_profile::ProfilePlatform;
use ordin_types::domain::engineering::FileTypeMapEntry;
use ordin_types::domain::places::CopyFormat;
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::time::Timestamp;
use ordin_types::{CodedError, FileTypeMapEntryId, RepairAction, RunId};
use serde_json::json;
use wiremock::MockServer;

use super::support::*;
use crate::definitions::PathRole;
use crate::filetypes::check_entry;
use crate::store::{self, Row};
use crate::{CraftCliMode, CraftError, CraftInstallationStatus, MakeCopy, ALL_CODES};

const MCP: &[CraftCliMode] = &[CraftCliMode::Mcp];

fn photo_setup(fake_tools: Vec<crate::definitions::CraftToolDefinition>) -> Setup {
    Setup {
        apps: vec![definition("photocraft", fake_tools)],
        craft: section(vec![pinned("photocraft", "0.7.0")], &[]),
        ..Setup::default()
    }
}

fn assert_error(error: &CraftError, code: &str, retryable: bool) {
    assert_eq!(error.code(), code);
    assert_eq!(error.retryable(), retryable, "{code}");
    assert_eq!(error.to_string(), code);
}

fn copy(format: CopyFormat) -> MakeCopy {
    MakeCopy {
        original_name: "rates.md".to_owned(),
        original_from: "supplier-docs".to_owned(),
        branch: Some("main".to_owned()),
        markdown: "# Rates\n".to_owned(),
        format,
        updated_at: Timestamp::now(),
    }
}

fn pdf_tool() -> crate::definitions::CraftToolDefinition {
    let mut made = tool(
        "to_pdf",
        &[("input", PathRole::Input), ("output", PathRole::Output)],
    );
    made.converts_markdown_to = Some(CopyFormat::Pdf);
    made
}

#[tokio::test]
async fn row_app_not_approved() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize()])],
        ..Setup::default()
    })
    .await;
    rig.seed("photocraft", "0.7.0", MCP, &FakeApp::with(vec![]))
        .await;
    let failure = rig
        .call(RunId::new(), "photocraft", "resize", json!({}))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_NOT_APPROVED");
    assert!(!failure.retryable);
    assert_eq!(failure.message, "Your company hasn't approved this app.");
    assert_error(&CraftError::NotApproved, "CRAFT_NOT_APPROVED", false);
}

#[tokio::test]
async fn row_app_approved_but_not_installed() {
    let rig = Rig::new(photo_setup(vec![resize()])).await;
    let failure = rig
        .call(RunId::new(), "photocraft", "resize", json!({}))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_NOT_INSTALLED");
    assert!(failure.retryable);
    let error = CraftError::NotInstalled;
    assert_eq!(CodedError::repair(&error), Some(RepairAction::TryAgain));
}

#[tokio::test]
async fn row_download_failed_after_retries() {
    let mirror = MockServer::start().await;
    let url = serve(&mirror, "/p.zip", 503, Vec::new(), 3).await;
    let hash = sha256_of(b"x");
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
    assert_error(&error, "CRAFT_DOWNLOAD_FAILED", true);
    assert_eq!(CodedError::repair(&error), Some(RepairAction::TryAgain));
    let rows = rig.toolkit.installations().await.unwrap();
    assert_eq!(rows[0].status, CraftInstallationStatus::Approved);
}

#[tokio::test]
async fn row_downloaded_file_does_not_match_the_pinned_hash() {
    let mirror = MockServer::start().await;
    let url = serve(&mirror, "/p.zip", 200, b"other".to_vec(), 1).await;
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
    let error = rig.toolkit.sync_installations().await.unwrap_err();
    assert_error(&error, "CRAFT_HASH_MISMATCH", false);
    assert_eq!(CodedError::repair(&error), None);
}

#[tokio::test]
async fn row_no_asset_for_this_platform_in_the_pin() {
    let hash = sha256_of(b"x");
    let url = "https://mirror.acme.co.uk/p.zip";
    let rig = Rig::new(Setup {
        craft: section(
            vec![app(
                "photocraft",
                "0.7.0",
                vec![(ProfilePlatform::WindowsX86_64, asset(url, &hash, "p.zip"))],
            )],
            &[],
        ),
        ..Setup::default()
    })
    .await;
    let error = rig.toolkit.sync_installations().await.unwrap_err();
    assert_error(&error, "CRAFT_PLATFORM_UNSUPPORTED", false);
    assert_eq!(CodedError::repair(&error), None);
}

#[tokio::test]
async fn row_unpacking_failed_or_an_unsafe_archive_path() {
    let mirror = MockServer::start().await;
    let garbage = b"this is not a zip".to_vec();
    let hash = sha256_of(&garbage);
    let url = serve(&mirror, "/p.zip", 200, garbage, 1).await;
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
    assert_error(&error, "CRAFT_INSTALL_FAILED", false);
    assert_eq!(CodedError::repair(&error), None);
}

#[tokio::test]
async fn row_the_operating_system_refused_to_start_the_program() {
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![])],
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
    *rig.launcher.refuse.lock().unwrap() = Some(std::io::ErrorKind::PermissionDenied);
    let error = rig
        .toolkit
        .open_in_app(rig.place.id, "logo.png")
        .await
        .unwrap_err();
    assert_error(&error, "CRAFT_START_BLOCKED", false);
    assert!(matches!(&error, CraftError::StartBlocked { program, .. } if program == "photocraft"));
    assert_eq!(CodedError::repair(&error), None);
    let named = CraftError::StartBlocked {
        program: "PhotoCraft".to_owned(),
        reason: String::new(),
    };
    assert_eq!(
        named.sentence(),
        "Your company's settings stopped PhotoCraft from opening on this computer."
    );
}

#[tokio::test]
async fn row_app_has_no_mcp_mode() {
    let rig = Rig::new(photo_setup(vec![resize()])).await;
    rig.seed(
        "photocraft",
        "0.7.0",
        &[CraftCliMode::Run],
        &FakeApp::with(vec![]),
    )
    .await;
    let failure = rig
        .call(RunId::new(), "photocraft", "resize", json!({}))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_NO_MCP_MODE");
    assert!(!failure.retryable);
}

#[tokio::test]
async fn row_app_s_mcp_server_did_not_start_in_15_s() {
    let mut setup = photo_setup(vec![resize()]);
    setup.limits.start = Duration::from_millis(300);
    let rig = Rig::new(setup).await;
    let mut fake = FakeApp::with(vec![("resize", Does::Write)]);
    fake.hang_at_start = true;
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let failure = rig
        .call(RunId::new(), "photocraft", "resize", json!({}))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_SERVER_START_FAILED");
    assert!(failure.retryable);
    assert_eq!(failure.message, "The app didn't start.");
}

#[tokio::test]
async fn row_tool_not_offered_to_this_session() {
    let mut setup = photo_setup(vec![resize(), tool("crop", &[])]);
    setup.craft.apps[0].allowed_tools = Some(vec!["resize".to_owned()]);
    let rig = Rig::new(setup).await;
    // `crop` is defined, not allowed; `export_all` is listed, not defined; `resize` is allowed and
    // defined, and the app does not list it in the second app below.
    let fake = FakeApp::with(vec![("crop", Does::Nothing), ("export_all", Does::Nothing)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    for name in ["crop", "export_all", "resize"] {
        let failure = rig
            .call(run, "photocraft", name, json!({}))
            .await
            .unwrap_err();
        assert_eq!(failure.code, "CRAFT_TOOL_NOT_OFFERED", "{name}");
        assert!(!failure.retryable);
        assert_eq!(failure.message, "This tool isn't available to you.");
    }
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn row_a_path_argument_outside_every_place_or_a_conversion_path_refused() {
    let rig = Rig::new(photo_setup(vec![resize()])).await;
    let fake = FakeApp::with(vec![("resize", Does::Write)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let outside = rig.dir.0.join("elsewhere.png");
    let failure = rig
        .call(
            RunId::new(),
            "photocraft",
            "resize",
            json!({ "input": outside, "output": rig.path_in_place("o.png") }),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_PATH_NOT_ALLOWED");
    assert!(!failure.retryable);
    assert_eq!(
        failure.message,
        "That file is outside the folders you can use."
    );
    rig.toolkit.shutdown().await;

    let rig = Rig::new(Setup {
        apps: vec![definition("pdfcraft", vec![pdf_tool()])],
        craft: section(vec![pinned("pdfcraft", "1.0.0")], &[]),
        protect_data_root: true,
        ..Setup::default()
    })
    .await;
    let fake = FakeApp::with(vec![("to_pdf", Does::Copy)]);
    rig.seed("pdfcraft", "1.0.0", MCP, &fake).await;
    let error = rig
        .toolkit
        .make_copy(copy(CopyFormat::Pdf))
        .await
        .err()
        .expect("refused");
    assert_error(&error, "CRAFT_PATH_NOT_ALLOWED", false);
}

#[tokio::test]
async fn row_a_path_argument_missing_or_not_an_absolute_path() {
    let rig = Rig::new(photo_setup(vec![resize()])).await;
    let fake = FakeApp::with(vec![("resize", Does::Write)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let input = rig.file_in_place("logo.png", b"x");
    for arguments in [
        json!({ "output": rig.path_in_place("o.png") }),
        json!({ "input": input, "output": "o.png" }),
        json!({ "input": 7, "output": rig.path_in_place("o.png") }),
    ] {
        let failure = rig
            .call(RunId::new(), "photocraft", "resize", arguments.clone())
            .await
            .unwrap_err();
        assert_eq!(failure.code, "CRAFT_ARGUMENTS_INVALID");
        assert!(!failure.retryable);
        assert_eq!(failure.message, "Give the file as a full path.");
        // The same finding reaches the MCP Front before the Rules Engine as `ArgumentProblem`.
        let problem = rig
            .provider
            .check_arguments(&key("photocraft", "resize"), &arguments)
            .unwrap_err();
        assert!(problem.at == "/input" || problem.at == "/output");
    }
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn row_tool_call_or_conversion_exceeded_120_s() {
    let rig = Rig::new(photo_setup(vec![resize()])).await;
    let fake = FakeApp::with(vec![("resize", Does::Hang)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    let input = rig.file_in_place("logo.png", b"x");
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
    assert!(failure.retryable);
    assert_eq!(failure.message, "The app took too long and was stopped.");

    let mut setup = Setup {
        apps: vec![definition("pdfcraft", vec![pdf_tool()])],
        craft: section(vec![pinned("pdfcraft", "1.0.0")], &[]),
        ..Setup::default()
    };
    setup.limits.call = Duration::from_millis(400);
    let rig = Rig::new(setup).await;
    let fake = FakeApp::with(vec![("to_pdf", Does::Hang)]);
    rig.seed("pdfcraft", "1.0.0", MCP, &fake).await;
    let error = rig
        .toolkit
        .make_copy(copy(CopyFormat::Pdf))
        .await
        .err()
        .expect("refused");
    assert_error(&error, "CRAFT_TIMEOUT", true);
}

#[tokio::test]
async fn row_the_app_returned_an_error() {
    let rig = Rig::new(photo_setup(vec![resize(), tool("rpc", &[])])).await;
    let long = "x".repeat(3_000);
    let fake = FakeApp::with(vec![("resize", Does::Fail(long)), ("rpc", Does::RpcError)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    let input = rig.file_in_place("logo.png", b"x");
    let failure = rig
        .call(
            run,
            "photocraft",
            "resize",
            json!({ "input": input, "output": rig.path_in_place("o.png") }),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_TOOL_FAILED");
    assert!(!failure.retryable);
    assert_eq!(failure.message.chars().count(), 2_000);
    assert!(failure.message.starts_with("xxx"));

    let failure = rig
        .call(run, "photocraft", "rpc", json!({}))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_TOOL_FAILED");
    assert!(failure.message.contains("boom"));
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn row_output_file_missing_or_empty() {
    let rig = Rig::new(photo_setup(vec![
        resize(),
        tool(
            "empty",
            &[("input", PathRole::Input), ("output", PathRole::Output)],
        ),
    ]))
    .await;
    let fake = FakeApp::with(vec![("resize", Does::Nothing), ("empty", Does::WriteEmpty)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let run = RunId::new();
    let input = rig.file_in_place("logo.png", b"x");
    for name in ["resize", "empty"] {
        let failure = rig
            .call(
                run,
                "photocraft",
                name,
                json!({ "input": input, "output": rig.path_in_place("o.png") }),
            )
            .await
            .unwrap_err();
        assert_eq!(failure.code, "CRAFT_OUTPUT_MISSING", "{name}");
        assert!(!failure.retryable);
        assert_eq!(failure.message, "The app didn't write the file.");
    }
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn row_output_above_256_mib() {
    let mut setup = photo_setup(vec![resize()]);
    setup.limits.max_output = 4;
    let rig = Rig::new(setup).await;
    let fake = FakeApp::with(vec![("resize", Does::WriteEight)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let input = rig.file_in_place("logo.png", b"x");
    let failure = rig
        .call(
            RunId::new(),
            "photocraft",
            "resize",
            json!({ "input": input, "output": rig.path_in_place("o.png") }),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_OUTPUT_TOO_LARGE");
    assert!(!failure.retryable);
    assert_eq!(failure.message, "The file the app wrote is too large.");
    rig.toolkit.shutdown().await;
    assert_eq!(crate::toolkit::MAX_OUTPUT_BYTES, 256 * 1024 * 1024);
}

#[tokio::test]
async fn row_conversion_app_not_confirmed_or_not_installed() {
    let rig = Rig::new(Setup::default()).await;
    let error = rig
        .toolkit
        .make_copy(copy(CopyFormat::Word))
        .await
        .err()
        .expect("refused");
    assert_error(&error, "CRAFT_CONVERSION_UNAVAILABLE", false);
    assert_eq!(
        error.sentence(),
        "Word, PDF and slides copies aren't ready yet."
    );
    let error = rig
        .toolkit
        .make_copy(copy(CopyFormat::Page))
        .await
        .err()
        .expect("refused");
    assert_error(&error, "CRAFT_CONVERSION_UNAVAILABLE", false);
    let error = rig
        .toolkit
        .to_markdown(CopyFormat::Pdf, b"x".to_vec())
        .await
        .unwrap_err();
    assert_error(&error, "CRAFT_CONVERSION_UNAVAILABLE", false);
}

#[tokio::test]
async fn row_opening_the_window_or_the_system_default_failed() {
    let rig = Rig::new(Setup::default()).await;
    let error = rig
        .toolkit
        .open_in_app(rig.place.id, "missing.png")
        .await
        .unwrap_err();
    assert_error(&error, "CRAFT_OPEN_FAILED", true);
    assert_eq!(CodedError::repair(&error), Some(RepairAction::TryAgain));
    let error = rig
        .toolkit
        .open_in_app(rig.place.id, "../escape.png")
        .await
        .unwrap_err();
    assert_error(&error, "CRAFT_OPEN_FAILED", true);
    rig.file_in_place("a.xyz", b"x");
    *rig.launcher.refuse.lock().unwrap() = Some(std::io::ErrorKind::NotFound);
    let error = rig
        .toolkit
        .open_with_system_default(rig.place.id, "a.xyz")
        .await
        .unwrap_err();
    assert_error(&error, "CRAFT_OPEN_FAILED", true);
}

#[tokio::test]
async fn row_file_service_write_failed() {
    let rig = Rig::new(photo_setup(vec![resize()])).await;
    let fake = FakeApp::with(vec![("resize", Does::Write)]);
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    let input = rig.file_in_place("logo.png", b"x");
    // A folder stands where the file would go, so the File Service cannot write it.
    let output = rig.path_in_place("taken.png");
    std::fs::create_dir_all(&output).unwrap();
    let failure = rig
        .call(
            RunId::new(),
            "photocraft",
            "resize",
            json!({ "input": input, "output": output }),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CRAFT_FILES_FAILED");
    assert!(failure.retryable);
    assert_eq!(failure.message, "ordin couldn't save the file.");
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn row_own_tables_unreachable() {
    let rig = Rig::new(Setup::default()).await;
    rig.pool().close().await;
    let error = rig.toolkit.installations().await.unwrap_err();
    assert_error(&error, "CRAFT_DATASTORE_UNAVAILABLE", true);
    assert_eq!(CodedError::repair(&error), Some(RepairAction::TryAgain));
    let error = rig.toolkit.check_apps(false).await.unwrap_err();
    assert_error(&error, "CRAFT_DATASTORE_UNAVAILABLE", true);
}

fn entry(extensions: &[&str], media_types: &[&str]) -> FileTypeMapEntry {
    FileTypeMapEntry {
        id: FileTypeMapEntryId::new(),
        extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
        media_types: media_types.iter().map(|m| (*m).to_owned()).collect(),
        open_with_app: None,
        craft_tools: Vec::new(),
        profile_version: "1.0.0".to_owned(),
    }
}

#[tokio::test]
async fn row_duplicate_app_row_installed_without_verification_bad_file_type_map_entry() {
    let rig = Rig::new(Setup::default()).await;
    // CRAFT_DUPLICATE
    store::insert_row(rig.pool(), &Row::approved("photocraft", "0.7.0"))
        .await
        .unwrap();
    let error = store::insert_row(rig.pool(), &Row::approved("photocraft", "0.7.1"))
        .await
        .unwrap_err();
    assert_error(&error, "CRAFT_DUPLICATE", false);
    // CRAFT_UNVERIFIED_INSTALL
    let mut row = store::load_rows(rig.pool()).await.unwrap().remove(0);
    row.status = CraftInstallationStatus::Installed;
    let error = store::update_row(rig.pool(), &row).await.unwrap_err();
    assert_error(&error, "CRAFT_UNVERIFIED_INSTALL", false);
    // FTM_EXTENSION_INVALID
    for bad in [entry(&[], &["image/png"]), entry(&["P.NG"], &["image/png"])] {
        let error = check_entry(&bad).unwrap_err();
        assert_error(&error, "FTM_EXTENSION_INVALID", false);
    }
    // FTM_MEDIA_TYPE_INVALID
    let error = check_entry(&entry(&["png"], &["not a type"])).unwrap_err();
    assert_error(&error, "FTM_MEDIA_TYPE_INVALID", false);
    assert!(check_entry(&entry(&["png"], &["image/png"])).is_ok());
    // FTM_EXTENSION_DUPLICATE
    let twice = [
        entry(&["png"], &["image/png"]),
        entry(&["png"], &["image/png"]),
    ];
    let error = store::replace_entries(rig.pool(), &twice)
        .await
        .unwrap_err();
    assert_error(&error, "FTM_EXTENSION_DUPLICATE", false);
    // FTM_IMMUTABLE: another map under the version the rows already carry.
    let profile = crate::CraftProfile {
        profile_version: "2.0.0".to_owned(),
        apps: Vec::new(),
        file_types: [("png".to_owned(), "system".to_owned())].into(),
    };
    rig.toolkit.apply_file_types(&profile).await.unwrap();
    let changed = crate::CraftProfile {
        file_types: [("jpg".to_owned(), "system".to_owned())].into(),
        ..profile
    };
    let error = rig.toolkit.apply_file_types(&changed).await.unwrap_err();
    assert_error(&error, "FTM_IMMUTABLE", false);
    assert!(rig.toolkit.file_kind("png").is_some(), "the rows stay");
}

/// X4: every code is the registry's, this crate's, with the registry's retry flag.
#[test]
fn all_codes_are_registered_and_owned_here() {
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is registered"));
        assert_eq!(entry.owner_crate, "ordin-craft-toolkit", "{code}");
        let (prefix, _) = code.split_once('_').expect("a prefix");
        assert!(owns_prefix("ordin-craft-toolkit", prefix), "{code}");
    }
    let every = [
        CraftError::NotApproved,
        CraftError::NotInstalled,
        CraftError::DownloadFailed,
        CraftError::HashMismatch,
        CraftError::PlatformUnsupported,
        CraftError::InstallFailed,
        CraftError::StartBlocked {
            program: String::new(),
            reason: String::new(),
        },
        CraftError::NoMcpMode,
        CraftError::ServerStartFailed,
        CraftError::ToolNotOffered,
        CraftError::PathNotAllowed,
        CraftError::ArgumentsInvalid,
        CraftError::Timeout,
        CraftError::ToolFailed,
        CraftError::OutputMissing,
        CraftError::OutputTooLarge,
        CraftError::ConversionUnavailable,
        CraftError::OpenFailed,
        CraftError::OpenRefused,
        CraftError::FilesFailed,
        CraftError::DatastoreUnavailable,
        CraftError::Invalid("CRAFT_DUPLICATE"),
    ];
    for error in &every {
        assert!(ALL_CODES.contains(&error.code()), "{}", error.code());
        let entry = lookup(error.code()).unwrap();
        assert_eq!(error.retryable(), entry.retryable, "{}", error.code());
        assert!(!error.sentence().is_empty());
    }
}
