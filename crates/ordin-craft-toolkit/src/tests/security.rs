//! The security rules of CraftToolkit.md: folder grants on a tool's paths (Internal Logic 6.2), the
//! version as a folder name (2.1), the installed hash (2.3), the profile check before a conversion
//! (7.1) and what may be opened (4.1).

use std::path::{Path, PathBuf};
use std::time::Duration;

use ordin_mcp_front::{ToolFailure, ToolOutput};
use ordin_organisation_profile::{CraftApp, ProfilePlatform};
use ordin_types::domain::places::{CopyFormat, FolderAccess};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, RunId};
use serde_json::{json, Value};
use wiremock::MockServer;

use super::support::*;
use crate::definitions::{CraftToolDefinition, PathRole};
use crate::store::{self, Row};
use crate::toolkit::version_ok;
use crate::{CraftCliMode, CraftInstallationStatus, CraftPlatform, MakeCopy, ProgramCheckState};

const MCP: &[CraftCliMode] = &[CraftCliMode::Mcp];

fn photo_setup() -> Setup {
    Setup {
        apps: vec![definition("photocraft", vec![resize()])],
        craft: section(vec![pinned("photocraft", "0.7.0")], &[]),
        ..Setup::default()
    }
}

/// A rig with photocraft installed, recording what reaches it.
async fn photo_rig() -> (Rig, PathBuf) {
    let rig = Rig::new(photo_setup()).await;
    let mut fake = FakeApp::with(vec![("resize", Does::Write)]);
    let record = rig.dir.0.join("calls.log");
    fake.record = Some(record.clone());
    rig.seed("photocraft", "0.7.0", MCP, &fake).await;
    (rig, record)
}

/// A second place, which no grant names, with one file in it.
async fn other_place(rig: &Rig) -> (ordin_files::Place, PathBuf) {
    let root = rig.dir.0.join("Finance");
    std::fs::create_dir_all(&root).unwrap();
    let place = rig
        .files
        .add_place(ordin_files::AddPlace {
            path: root.clone(),
            friendly_name: Some("Finance".to_owned()),
            kind: ordin_files::PlaceKind::PersonsFolder,
        })
        .await
        .unwrap();
    let file = root.join("salaries.png");
    std::fs::write(&file, b"secret").unwrap();
    (place, file)
}

async fn resize_call(rig: &Rig, input: &str, output: &str) -> Result<ToolOutput, ToolFailure> {
    rig.call(
        RunId::new(),
        "photocraft",
        "resize",
        json!({ "input": input, "output": output }),
    )
    .await
}

fn assert_refused(record: &Path, outcome: Result<ToolOutput, ToolFailure>) {
    assert_eq!(outcome.err().unwrap().code, "CRAFT_PATH_NOT_ALLOWED");
    assert!(!record.exists(), "nothing reaches the app");
}

#[tokio::test]
async fn a_tool_may_not_read_a_place_the_agent_has_no_grant_for() {
    let (rig, record) = photo_rig().await;
    let (_, secret) = other_place(&rig).await;
    let output = rig.path_in_place("out.png");

    let outcome = resize_call(&rig, &secret.to_string_lossy(), &output).await;

    assert_refused(&record, outcome);
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn a_tool_may_not_write_to_a_place_the_agent_has_no_grant_for() {
    let (rig, record) = photo_rig().await;
    let (_, secret) = other_place(&rig).await;
    let input = rig.file_in_place("in.png", b"png");

    let outcome = resize_call(&rig, &input, &secret.to_string_lossy()).await;

    assert_refused(&record, outcome);
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn a_place_granted_to_another_agent_is_refused() {
    let (rig, record) = photo_rig().await;
    let other = AgentInstanceId::new();
    rig.rules
        .load_rule_set(rule_set(other, &rig.dir.0.join("agents/eng-2")))
        .unwrap();
    let input = rig.file_in_place("in.png", b"png");
    let mut ctx = rig.context(RunId::new(), Duration::from_secs(60));
    ctx.agent_instance_id = other;

    let outcome = rig
        .provider
        .call(
            ctx,
            &key("photocraft", "resize"),
            json!({ "input": input, "output": rig.path_in_place("out.png") }),
        )
        .await;

    assert_refused(&record, outcome);
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn a_grant_in_another_workspace_is_refused() {
    let (rig, record) = photo_rig().await;
    let input = rig.file_in_place("in.png", b"png");
    let mut ctx = rig.context(RunId::new(), Duration::from_secs(60));
    ctx.workspace_id = ordin_types::WorkspaceId::new();

    let outcome = rig
        .provider
        .call(
            ctx,
            &key("photocraft", "resize"),
            json!({ "input": input, "output": rig.path_in_place("out.png") }),
        )
        .await;

    assert_refused(&record, outcome);
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn read_access_serves_an_input_and_refuses_an_output() {
    let (rig, record) = photo_rig().await;
    let (finance, secret) = other_place(&rig).await;
    let finance_root = secret.parent().unwrap();
    rig.rules.replace_grants(
        rig.workspace,
        vec![
            granted(
                rig.agent,
                rig.place.id,
                &rig.place_root,
                FolderAccess::ReadAndChange,
            ),
            granted(rig.agent, finance.id, finance_root, FolderAccess::Read),
        ],
    );
    let secret = secret.to_string_lossy().into_owned();

    let refused = resize_call(&rig, &secret, &secret).await;
    assert_refused(&record, refused);

    let allowed = resize_call(&rig, &secret, &rig.path_in_place("out.png")).await;
    assert!(allowed.is_ok(), "reading a read-only grant is allowed");
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn a_link_inside_a_grant_answers_for_its_target() {
    let (rig, record) = photo_rig().await;
    let (_, secret) = other_place(&rig).await;
    let link = rig.place_root.join("harmless.png");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    let output = rig.path_in_place("out.png");

    let outcome = resize_call(&rig, &link.to_string_lossy(), &output).await;

    assert_refused(&record, outcome);
    rig.toolkit.shutdown().await;
}

#[test]
fn only_a_semver_version_becomes_a_folder() {
    for good in ["0.7.0", "1.2.3-rc.1", "1.2.3+build.5"] {
        assert!(version_ok(good), "{good}");
    }
    for bad in [
        "",
        "..",
        "../../evil",
        "0.7.0/../../evil",
        "0.7.0/evil",
        "0.7.0\\evil",
        "..\\evil",
        "/etc/evil",
        "C:\\evil",
        "1.0",
        "latest",
        "1.0.0-a..b",
    ] {
        assert!(!version_ok(bad), "{bad}");
    }
}

fn linux_pin(version: &str, url: &str, hash: &ordin_types::num::Sha256Hex) -> CraftApp {
    app(
        "photocraft",
        version,
        vec![(ProfilePlatform::LinuxX86_64, asset(url, hash, "p.zip"))],
    )
}

fn photo_zip(fake: &FakeApp) -> Vec<u8> {
    zip_of(&[("photocraft-cli", fake.script().as_bytes(), 0o755)])
}

/// The profile validator refuses this document; a snapshot reaches the toolkit without it.
#[tokio::test]
async fn a_pin_whose_version_is_a_path_writes_nothing() {
    let mirror = MockServer::start().await;
    let archive = photo_zip(&FakeApp::with(vec![]));
    let hash = sha256_of(&archive);
    let url = serve(&mirror, "/p.zip", 200, archive, 0).await;
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize()])],
        craft: section(vec![linux_pin("../../evil", &url, &hash)], &[]),
        ..Setup::default()
    })
    .await;

    rig.toolkit.sync_installations().await.unwrap();

    assert!(!rig.data_root.join("evil").exists());
    assert!(store::load_rows(rig.pool()).await.unwrap().is_empty());
    rig.toolkit.check_apps(false).await.unwrap();
}

#[tokio::test]
async fn a_pin_with_a_bad_version_does_not_withdraw_the_installed_app() {
    let (rig, _) = photo_rig().await;
    let hash = sha256_of(b"x");
    let craft = section(
        vec![linux_pin("../x", "https://mirror.acme.co.uk/p.zip", &hash)],
        &[],
    );
    rig.feed.send_replace(snapshot("2.0.0", craft));

    rig.toolkit.sync_installations().await.unwrap();

    let rows = store::load_rows(rig.pool()).await.unwrap();
    assert_eq!(rows[0].status, CraftInstallationStatus::Installed);
    assert!(rig.data_root.join("craft/photocraft/0.7.0").is_dir());
    rig.toolkit.shutdown().await;
}

/// An update deletes the previous version's folder, whose name comes from the row.
#[tokio::test]
async fn a_stored_version_that_is_a_path_deletes_nothing() {
    let mirror = MockServer::start().await;
    let archive = photo_zip(&FakeApp::with(vec![]));
    let hash = sha256_of(&archive);
    let url = serve(&mirror, "/p.zip", 200, archive, 1).await;
    let (rig, _) = photo_rig().await;
    let mut row = rig.toolkit.row("photocraft").unwrap();
    row.installed_version = Some("../../keep".to_owned());
    store::update_row(rig.pool(), &row).await.unwrap();
    let keep = rig.data_root.join("keep");
    std::fs::create_dir_all(&keep).unwrap();
    let craft = section(vec![linux_pin("0.7.1", &url, &hash)], &[]);
    rig.feed.send_replace(snapshot("2.0.0", craft));

    rig.toolkit.sync_installations().await.unwrap();

    assert!(keep.is_dir());
    let rows = store::load_rows(rig.pool()).await.unwrap();
    assert_eq!(rows[0].installed_version.as_deref(), Some("0.7.1"));
    rig.toolkit.shutdown().await;
}

/// The second archive's tool fails with its own text, so a call shows which build answers.
#[tokio::test]
async fn a_new_hash_for_the_same_version_replaces_the_install_and_its_servers() {
    let mirror = MockServer::start().await;
    let first = photo_zip(&FakeApp::with(vec![("resize", Does::Write)]));
    let second = photo_zip(&FakeApp::with(vec![(
        "resize",
        Does::Fail("the second build".to_owned()),
    )]));
    let (first_hash, second_hash) = (sha256_of(&first), sha256_of(&second));
    let first_url = serve(&mirror, "/first.zip", 200, first, 1).await;
    let second_url = serve(&mirror, "/second.zip", 200, second.clone(), 1).await;
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize()])],
        craft: section(vec![linux_pin("0.7.0", &first_url, &first_hash)], &[]),
        ..Setup::default()
    })
    .await;
    rig.toolkit.sync_installations().await.unwrap();
    let input = rig.file_in_place("in.png", b"png");
    let output = rig.path_in_place("out.png");
    let run = RunId::new();
    let arguments = json!({ "input": input, "output": output });
    rig.call(run, "photocraft", "resize", arguments.clone())
        .await
        .expect("the first build answers");

    let repinned = section(vec![linux_pin("0.7.0", &second_url, &second_hash)], &[]);
    rig.feed.send_replace(snapshot("2.0.0", repinned));
    let checks = rig.toolkit.check_apps(false).await.unwrap();
    assert!(
        matches!(checks[0].state, ProgramCheckState::Missing),
        "the old file is not what the profile pins"
    );

    rig.toolkit.sync_installations().await.unwrap();

    let rows = store::load_rows(rig.pool()).await.unwrap();
    assert_eq!(rows[0].installed_sha256.as_deref(), Some(&*second_hash.0));
    let checks = rig.toolkit.check_apps(false).await.unwrap();
    assert!(matches!(checks[0].state, ProgramCheckState::Ready));
    let failure = rig
        .call(run, "photocraft", "resize", arguments)
        .await
        .unwrap_err();
    assert_eq!(failure.message, "the second build");
    rig.toolkit.shutdown().await;
}

#[tokio::test]
async fn the_same_hash_downloads_nothing_again() {
    let mirror = MockServer::start().await;
    let archive = photo_zip(&FakeApp::with(vec![]));
    let hash = sha256_of(&archive);
    let url = serve(&mirror, "/p.zip", 200, archive, 1).await;
    let upper = ordin_types::num::Sha256Hex(hash.0.to_uppercase());
    let rig = Rig::new(Setup {
        apps: vec![definition("photocraft", vec![resize()])],
        craft: section(vec![linux_pin("0.7.0", &url, &hash)], &[]),
        ..Setup::default()
    })
    .await;
    // FU-052: the first sync must have installed the app, or a second download is the first one
    // finishing; this assertion names that cause instead of the mock's count.
    let first = rig.toolkit.sync_installations().await.unwrap();
    assert_eq!(
        first.installations[0].installed_version.as_deref(),
        Some("0.7.0"),
        "the first sync installed the app: {first:?}"
    );
    let craft = section(vec![linux_pin("0.7.0", &url, &upper)], &[]);
    rig.feed.send_replace(snapshot("2.0.0", craft));

    rig.toolkit.sync_installations().await.unwrap();
}

fn converting() -> CraftToolDefinition {
    let mut made = tool(
        "to_pdf",
        &[("input", PathRole::Input), ("output", PathRole::Output)],
    );
    made.converts_markdown_to = Some(CopyFormat::Pdf);
    made
}

fn copy() -> MakeCopy {
    MakeCopy {
        original_name: "rates.md".to_owned(),
        original_from: "Brand".to_owned(),
        branch: None,
        markdown: "# Rates\n".to_owned(),
        format: CopyFormat::Pdf,
        updated_at: Timestamp::now(),
    }
}

fn reverse() -> CraftToolDefinition {
    let mut made = tool(
        "to_md",
        &[("input", PathRole::Input), ("output", PathRole::Output)],
    );
    made.converts_to_markdown_from = Some(CopyFormat::Word);
    made
}

/// `app_name` installed with `tools`, recording what reaches it.
async fn conversion_rig(
    app_name: &str,
    tools: Vec<CraftToolDefinition>,
    does: (&'static str, Does),
) -> (Rig, PathBuf) {
    let rig = Rig::new(Setup {
        apps: vec![definition(app_name, tools)],
        craft: section(vec![pinned(app_name, "1.0.0")], &[]),
        ..Setup::default()
    })
    .await;
    let mut fake = FakeApp::with(vec![does]);
    let record = rig.dir.0.join("calls.log");
    fake.record = Some(record.clone());
    rig.seed(app_name, "1.0.0", MCP, &fake).await;
    (rig, record)
}

async fn assert_no_copy(rig: &Rig, record: &Path) {
    assert!(!rig.toolkit.conversion_available(CopyFormat::Pdf));
    let error = rig.toolkit.make_copy(copy()).await.err().expect("refused");
    assert_eq!(error.code(), "CRAFT_CONVERSION_UNAVAILABLE");
    assert!(!record.exists(), "no document reaches the app");
}

#[tokio::test]
async fn a_withdrawn_app_is_not_sent_documents_and_stays_on_disk() {
    let (rig, record) =
        conversion_rig("pdfcraft", vec![converting()], ("to_pdf", Does::Copy)).await;
    assert!(rig.toolkit.conversion_available(CopyFormat::Pdf));

    rig.feed
        .send_replace(snapshot("2.0.0", section(vec![], &[])));

    assert_no_copy(&rig, &record).await;
    assert!(rig.data_root.join("craft/pdfcraft/1.0.0").is_dir());
    let rows = store::load_rows(rig.pool()).await.unwrap();
    assert_eq!(rows[0].status, CraftInstallationStatus::Installed);
}

#[tokio::test]
async fn a_tool_removed_by_allowed_tools_is_not_sent_documents() {
    let (rig, record) =
        conversion_rig("pdfcraft", vec![converting()], ("to_pdf", Does::Copy)).await;
    let mut pin = pinned("pdfcraft", "1.0.0");
    pin.allowed_tools = Some(vec!["something_else".to_owned()]);

    rig.feed
        .send_replace(snapshot("2.0.0", section(vec![pin], &[])));

    assert_no_copy(&rig, &record).await;
}

#[tokio::test]
async fn a_withdrawn_app_is_not_sent_a_word_file() {
    let (rig, record) = conversion_rig("wordcraft", vec![reverse()], ("to_md", Does::Copy)).await;

    rig.feed
        .send_replace(snapshot("2.0.0", section(vec![], &[])));

    let error = rig
        .toolkit
        .to_markdown(CopyFormat::Word, b"docx".to_vec())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CRAFT_CONVERSION_UNAVAILABLE");
    assert!(!record.exists(), "no document reaches the app");
}

fn opened_paths(rig: &Rig) -> Vec<String> {
    rig.opened()
        .iter()
        .filter_map(|line| line.last())
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[tokio::test]
async fn a_link_out_of_the_place_is_not_opened() {
    let rig = Rig::new(Setup {
        platform: CraftPlatform::LinuxX86_64,
        ..photo_setup()
    })
    .await;
    let outside = rig.dir.0.join("outside.png");
    std::fs::write(&outside, b"secret").unwrap();
    std::os::unix::fs::symlink(&outside, rig.place_root.join("link.png")).unwrap();
    std::fs::create_dir_all(rig.dir.0.join("elsewhere")).unwrap();
    std::fs::write(rig.dir.0.join("elsewhere/f.png"), b"secret").unwrap();
    std::os::unix::fs::symlink(rig.dir.0.join("elsewhere"), rig.place_root.join("dir")).unwrap();

    for path in ["link.png", "dir/f.png"] {
        let id = rig.place.id;
        let in_app = rig.toolkit.open_in_app(id, path).await.err().unwrap();
        assert_eq!(in_app.code(), "CRAFT_OPEN_FAILED");
        let system = rig
            .toolkit
            .open_with_system_default(id, path)
            .await
            .err()
            .unwrap();
        assert_eq!(system.code(), "CRAFT_OPEN_FAILED");
    }
    assert!(rig.opened().is_empty());
}

#[tokio::test]
async fn a_link_inside_the_place_opens_its_target() {
    let rig = Rig::new(photo_setup()).await;
    let target = rig.file_in_place("real.png", b"png");
    std::os::unix::fs::symlink(&target, rig.place_root.join("link.png")).unwrap();

    rig.toolkit
        .open_with_system_default(rig.place.id, "link.png")
        .await
        .unwrap();

    assert_eq!(opened_paths(&rig), [target]);
}

#[tokio::test]
async fn a_folder_is_not_opened() {
    let rig = Rig::new(photo_setup()).await;
    std::fs::create_dir_all(rig.place_root.join("Q3")).unwrap();

    for path in ["Q3", "."] {
        let error = rig
            .toolkit
            .open_with_system_default(rig.place.id, path)
            .await
            .err()
            .unwrap();
        assert_eq!(error.code(), "CRAFT_OPEN_FAILED");
    }
    assert!(rig.opened().is_empty());
}

#[tokio::test]
async fn a_row_keeps_the_hash_it_was_installed_from() {
    let rig = Rig::new(Setup::default()).await;
    let mut row = Row::approved("photocraft", "0.7.0");
    row.installed_sha256 = Some("b".repeat(64));
    store::insert_row(rig.pool(), &row).await.unwrap();

    let rows: Vec<Value> = store::load_rows(rig.pool())
        .await
        .unwrap()
        .iter()
        .map(|r| json!(r.installed_sha256))
        .collect();

    assert_eq!(rows, [json!("b".repeat(64))]);
}
