//! Plan 04, safety net C: a first start with the agents the release ships.
//!
//! The program's real core is started the way `window.rs` starts it, on an empty data root, with the
//! bundled catalogue the release build copies: `RELEASE_BUNDLED_AGENTS_SOURCE` at
//! `RELEASE_BUNDLED_AGENTS_COMMIT` (`release/release.conf`), extracted with `git archive` as
//! `scripts/release/build.sh` does. The bundled agents of this build must end up ready (Ordin's agent,
//! and the agent maker with the `authoring` switch, DSK-REC-24a), the document agent must serve a
//! workspace created after the start (PB-CHK-1), and the start must log no warning or error outside
//! `tests/fixtures/first-start-expected.txt`.
//!
//! It needs neither Claude Code nor memory's models. It needs the bundled agents' repository, and
//! fails, never skips, when that is missing. `make first-start-check` runs it.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ordin_command_bridge::{CommandBridge, EventBatch};
use ordin_desktop::{Core, CoreInputs, DesktopPaths};
use ordin_memory_service::ModelManifest;
use ordin_organisation_profile::InstallerSettings;
use ordin_package_catalogue::BundledAgent;
use ordin_service_api::ServiceApiVersion;
use ordin_types::{BackendId, CommandRequestId};
use serde_json::{json, Value};

const WINDOW: &str = "main";
/// How long the bundled agents may take: five attempts 30 s apart fail in about two minutes
/// (DSK-REC-24), so a success comes well before this.
const AGENTS_READY_WITHIN: Duration = Duration::from_secs(180);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root")
}

/// `KEY=value` lines of `release/release.conf`.
fn release_conf() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(repo_root().join("release/release.conf"))
        .expect("release/release.conf");
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().trim_matches('"').to_owned()))
        .collect()
}

/// The release's bundled catalogue, extracted into `into`.
fn bundled_catalogue(into: &Path) -> PathBuf {
    let conf = release_conf();
    let source = std::env::var("RELEASE_BUNDLED_AGENTS_SOURCE")
        .ok()
        .or_else(|| conf.get("RELEASE_BUNDLED_AGENTS_SOURCE").cloned())
        .expect("RELEASE_BUNDLED_AGENTS_SOURCE in release/release.conf");
    let commit = std::env::var("RELEASE_BUNDLED_AGENTS_COMMIT")
        .ok()
        .or_else(|| conf.get("RELEASE_BUNDLED_AGENTS_COMMIT").cloned())
        .expect("RELEASE_BUNDLED_AGENTS_COMMIT in release/release.conf");
    let source = repo_root().join(source);
    assert!(
        source.join(".git").exists(),
        "the bundled agents' repository is needed at {} (a checkout holding commit {commit})",
        source.display()
    );
    let catalogue = into.join("catalogue");
    std::fs::create_dir_all(&catalogue).expect("the catalogue folder");
    let archive = std::process::Command::new("git")
        .arg("-C")
        .arg(&source)
        .args(["archive", &commit])
        .output()
        .expect("run git archive");
    assert!(
        archive.status.success(),
        "git archive {commit} in {}: {}",
        source.display(),
        String::from_utf8_lossy(&archive.stderr)
    );
    let mut tar = std::process::Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(&catalogue)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("run tar");
    tar.stdin
        .take()
        .expect("tar's input")
        .write_all(&archive.stdout)
        .expect("feed tar");
    assert!(tar.wait().expect("tar").success(), "tar -x failed");
    catalogue
}

/// True when the catalogue holds the workspace profile `models/@default/workspaces/<name>`.
fn catalogue_holds(catalogue: &Path, name: &str) -> bool {
    catalogue
        .join("models/@default/workspaces")
        .join(name)
        .join("workspace-profile.yaml")
        .is_file()
}

/// The log lines a clean first start may write at WARN or above: `<message> <reason>` per line.
fn expected_messages() -> Vec<String> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-start-expected.txt");
    let text = std::fs::read_to_string(&path).expect("tests/fixtures/first-start-expected.txt");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (message, reason) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
            assert!(
                !reason.trim().is_empty(),
                "{l}: every expected line needs a reason"
            );
            message.to_owned()
        })
        .collect()
}

/// The JSON log, WARN and above, kept in memory.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn lines(&self) -> Vec<Value> {
        let bytes = self.0.lock().unwrap().clone();
        String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let id = uuid::Uuid::now_v7().simple().to_string();
        let dir = std::env::temp_dir().join(format!("ordin-fs-{}", &id[20..]));
        std::fs::create_dir_all(&dir).expect("the run's folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `ordin-hook`, built for this target and copied beside the program, where `DesktopPaths` looks
/// for its bundled programs (as `end_to_end.rs` does).
fn hook_beside_the_program(exe_dir: &Path) {
    let exe = std::env::current_exe().expect("this test's path");
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("target/<profile>")
        .to_path_buf();
    let target_dir = profile_dir.parent().expect("the target folder");
    let status = std::process::Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .args(["build", "-p", "ordin-hook", "--target-dir"])
        .arg(target_dir)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("run cargo");
    assert!(status.success(), "cargo build -p ordin-hook failed");
    std::fs::create_dir_all(exe_dir).expect("the program's folder");
    std::fs::copy(profile_dir.join("ordin-hook"), exe_dir.join("ordin-hook")).expect("the hook");
}

async fn call(
    bridge: &CommandBridge,
    backend: BackendId,
    name: &str,
    input: Value,
) -> Result<Value, Value> {
    let envelope = json!({
        "api_version": ServiceApiVersion::CURRENT,
        "request_id": CommandRequestId::new(),
        "command": name,
        "backend": backend,
        "input": input,
    });
    let reply = bridge
        .invoke(WINDOW, name, &envelope.to_string())
        .await
        .expect("the window is open");
    let reply: Value = serde_json::from_str(&reply).expect("a reply");
    match reply["kind"].as_str() {
        Some("ok") => Ok(reply["result"].clone()),
        _ => Err(reply["error"].clone()),
    }
}

fn describe(lines: &[Value]) -> String {
    lines
        .iter()
        .map(|l| format!("  {} {} {}", l["level"], l["target"], l["fields"]))
        .collect::<Vec<_>>()
        .join("\n")
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-24
// spec: ordin-desktop/phase-5-playbook#PB-CHK-1
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "first-start check: needs the bundled agents' repository; make first-start-check"]
async fn a_first_start_installs_the_agents_the_release_ships_and_logs_no_warning() {
    let log = Captured::default();
    let writer = log.clone();
    tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_max_level(tracing_subscriber::filter::LevelFilter::WARN)
        .with_writer(move || writer.clone())
        .try_init()
        .expect("the test's log");

    let dir = TempDir::new();
    let catalogue_dir = bundled_catalogue(&dir.0);
    let exe_dir = dir.0.join("program");
    hook_beside_the_program(&exe_dir);
    let installation_id = uuid::Uuid::now_v7();
    let paths = DesktopPaths::new(dir.0.join("data"), &exe_dir, &installation_id.to_string());
    paths.prepare().expect("the data root");
    let models_dir = dir.0.join("data/models");
    let inputs = CoreInputs {
        paths,
        http: reqwest::Client::new(),
        bundle_identifier: "io.gatblau.ordin.first-start".to_owned(),
        computer_name: "Test computer".to_owned(),
        installation_id,
        os_locales: vec!["en-GB".to_owned()],
        home: dir.0.join("home"),
        installer_defaults: InstallerSettings::default(),
        browser: None,
        models_dir,
        // A build with no models input (DSK-MD-2).
        model_manifest: ModelManifest { files: Vec::new() },
        models_source: None,
        // Claude Code is not needed to install the bundled agents.
        claude_path: dir.0.join("no-claude"),
        documents_dir: dir.0.join("Documents"),
        bundled_catalogue: catalogue_dir.clone(),
        url_opener: None,
        updater: None,
        shell: None,
    };

    let core = Core::start(inputs).await.expect("the core starts");
    let bridge = (*core.modules.bridge.clone().expect("the Command Bridge")).clone();
    let backend = core.modules.backend.expect("the backend");
    bridge
        .open_events(WINDOW, BTreeMap::new(), Box::new(|_: EventBatch| Ok(())))
        .await
        .expect("service_events");

    // The bundled agents this build adds (DSK-REC-24a): Ordin's agent, and the agent maker only with the
    // `authoring` switch, whose catalogue then holds it.
    let agents = ordin_desktop::bundled::bundled_agents();
    if agents.contains(&BundledAgent::AgentMaker) {
        assert!(
            catalogue_holds(&catalogue_dir, "advisor"),
            "a build with the authoring switch needs the agent maker in its bundled catalogue"
        );
    }
    let started = Instant::now();
    for agent in agents {
        let id = agent.instance_id();
        loop {
            let got = call(
                &bridge,
                backend,
                "agents.get",
                json!({ "agent_instance_id": id }),
            )
            .await;
            if got
                .as_ref()
                .is_ok_and(|a| a["instance"]["status"] == "ready")
            {
                break;
            }
            assert!(
                started.elapsed() < AGENTS_READY_WITHIN,
                "the bundled agent {agent:?} is not ready after {AGENTS_READY_WITHIN:?} (last answer: {got:?}).\nWarnings and errors:\n{}",
                describe(&log.lines())
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    let listed = call(&bridge, backend, "agents.list", json!({})).await;
    eprintln!("agents.list: {listed:?}");

    // PB-CHK-1: the document agent serves the workspace the check creates (PKG-DOC-2, DSK-DOC-1).
    assert!(
        catalogue_holds(&catalogue_dir, "document-agent"),
        "the release's bundled catalogue needs the document agent"
    );
    let workspace = call(
        &bridge,
        backend,
        "workspaces.create",
        json!({ "name": "Board papers" }),
    )
    .await
    .expect("the workspace is created");
    let workspace_id = workspace["id"].clone();
    let started = Instant::now();
    loop {
        let got = call(
            &bridge,
            backend,
            "agents.list",
            json!({ "workspace_id": workspace_id }),
        )
        .await;
        let ready = got.as_ref().is_ok_and(|list| {
            list.as_array().is_some_and(|agents| {
                agents.len() == 1
                    && agents[0]["instance"]["status"] == "ready"
                    && agents[0]["instance"]["package_name"]
                        .as_str()
                        .is_some_and(|name| name.contains("document-agent"))
            })
        });
        if ready {
            break;
        }
        assert!(
            started.elapsed() < AGENTS_READY_WITHIN,
            "the document agent does not serve the new workspace after {AGENTS_READY_WITHIN:?} (last answer: {got:?}).\nWarnings and errors:\n{}",
            describe(&log.lines())
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let expected = expected_messages();
    let unexpected: Vec<Value> = log
        .lines()
        .into_iter()
        .filter(|l| {
            let message = l["fields"]["message"].as_str().unwrap_or_default();
            !expected.iter().any(|e| e == message)
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "a clean first start logged warnings or errors not in tests/fixtures/first-start-expected.txt:\n{}",
        describe(&unexpected)
    );
}
