//! The scripted end-to-end run of the R1 ship gate (plan 01, Phase 13, *Behavioural check*): the
//! program's real core, started the way the program starts it, driven only through the Command
//! Bridge the window uses (`CommandBridge::invoke` with a Service API envelope) and the events the
//! screens receive (`CommandBridge::open_events`). Each step asserts what a person would see.
//!
//! It is an end-to-end job, not part of the ordinary suite: it needs the real Claude Code at the
//! pinned version on `PATH` and memory's int8 models in `ORDIN_TEST_MODEL_DIR` (`embedding/` and
//! `reranker/`), and it fails, never skips, when either is missing. `make e2e-test` runs it.
//!
//! Nothing leaves the computer and nothing is spent. The model is a local `wiremock` stand-in reached
//! through `ANTHROPIC_BASE_URL`; a wrapper around Claude Code points `HOME` and `CLAUDE_CONFIG_DIR`
//! at a temporary folder, so the person's own sign-in is never read or changed (the pattern of
//! `crates/ordin-claude-adapter/tests/harness.rs`). The stand-in scripts Ordin's replies: the
//! delegation that starts one session, the crew plan, the crew's round and the recall; the agent's
//! one shell call (`touch`, on its approval list) is the tool call that waits for the approval.
//!
//! The Confluence link and the push reach `wiremock` stand-ins through their owners' test seams,
//! which exist only with each crate's `test-support` feature (ConfluenceAdapter.md CONF-SEAM-2,
//! ProjectService.md PROJ-SEAM-2): the Confluence Adapter is given the stand-in's address as its
//! gateway, the company site and a token, and the Project Service a stand-in host and its token. The
//! push stand-in answers Git's smart HTTP protocol through `git http-backend` over the bare
//! repository, once the request carries the token.
//!
//! The update check uses the core's `AppUpdater` seam: the window's updater is Tauri's plug-in, which
//! needs a running window and is built for macOS and Windows only. The stand-in updater here reads
//! the feed the Setup Assistant gives it, in Tauri's `latest.json` form; the signature is not checked.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use ordin_command_bridge::{CommandBridge, EventBatch};
use ordin_confluence::ConfluenceTestSeam;
use ordin_desktop::memory_models::ModelsSource;
use ordin_desktop::{Core, CoreInputs, DesktopPaths, QuitReason};
use ordin_memory_service::{ModelFileDigest, ModelManifest};
use ordin_organisation_profile::InstallerSettings;
use ordin_package_catalogue::BundledAgent;
use ordin_service_api::ServiceApiVersion;
use ordin_setup::{AppUpdater, InstallKind, UpdateOffer, UpdateState, UpdaterError};
use ordin_types::{BackendId, CommandRequestId};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// The Claude Code version this job holds the program to (the harness job's).
const PINNED: &str = "2.1.316";
const FAKE_KEY: &str = "sk-ant-not-a-real-key";
/// The ten model files (OrdinDesktop.md DSK-MD-1).
const MODEL_FILES: [&str; 10] = ordin_desktop::memory_models::MODEL_FILES;
/// The window label the client speaks as.
const WINDOW: &str = "main";
/// The knowledge page saved and recalled.
const PAGE_TITLE: &str = "Quarzite release checklist";
const PAGE_BODY: &str = "The quarzite release checklist has three steps: build, sign and publish.";
/// The company Confluence site, the stand-in's cloud id, the parent page linked to and the token.
const CONFLUENCE_SITE: &str = "https://acme.atlassian.net";
const CONFLUENCE_CLOUD: &str = "cloud-e2e";
const CONFLUENCE_PARENT: &str = "4000";
const CONFLUENCE_TOKEN: &str = "confluence-stand-in-token";
/// The push host's token.
const PUSH_TOKEN: &str = "push-stand-in-token";

// ---------------------------------------------------------------------------------------------
// The machine around the core.

/// A folder for the run, removed on drop. Under the system's temporary folder, whose short path
/// leaves room for the Unix socket paths under `<data root>/run`.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let id = uuid::Uuid::now_v7().simple().to_string();
        let dir = std::env::temp_dir().join(format!("ordin-e2e-{}", &id[20..]));
        std::fs::create_dir_all(&dir).expect("the run's folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The real CLI on `PATH`; a missing CLI or another version fails the job.
fn pinned_claude() -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH");
    let claude = std::env::split_paths(&path)
        .map(|dir| dir.join("claude"))
        .find(|candidate| candidate.is_file())
        .expect("the end-to-end job needs Claude Code on PATH");
    let output = std::process::Command::new(&claude)
        .arg("--version")
        .env_clear()
        .output()
        .expect("run claude --version");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("{PINNED} (Claude Code)"),
        "the end-to-end job is pinned to Claude Code {PINNED}"
    );
    claude
}

/// `CoreInputs.claude_path`: the real CLI in a temporary home, with the stand-in's address and a
/// key that is not a real one. The program signs in as the person (`SignInMode::PersonSignIn`), so
/// it sets neither itself.
fn claude_wrapper(dir: &Path, stand_in: &str) -> PathBuf {
    let real = pinned_claude();
    let home = dir.join("claude-home");
    std::fs::create_dir_all(&home).expect("the temporary home");
    let wrapper = dir.join("claude-wrapper");
    let script = format!(
        "#!/bin/sh\n\
         export HOME='{home}' CLAUDE_CONFIG_DIR='{home}/config'\n\
         export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 DISABLE_TELEMETRY=1\n\
         export ENABLE_TOOL_SEARCH=false\n\
         export ANTHROPIC_API_KEY='{FAKE_KEY}' ANTHROPIC_BASE_URL='{stand_in}'\n\
         exec '{real}' \"$@\"\n",
        home = home.display(),
        real = real.display(),
    );
    std::fs::write(&wrapper, script).expect("the wrapper");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).expect("mode");
    wrapper
}

/// `ordin-hook`, built for this target and copied beside the program, where `DesktopPaths` looks
/// for its bundled programs.
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

/// The models folder and its manifest: every file is needed and hashed (DSK-MD-1).
fn models() -> (PathBuf, ModelManifest) {
    let dir = PathBuf::from(
        std::env::var("ORDIN_TEST_MODEL_DIR")
            .expect("ORDIN_TEST_MODEL_DIR must name the folder holding memory's models"),
    );
    let files = MODEL_FILES
        .iter()
        .map(|file| {
            let bytes = std::fs::read(dir.join(file))
                .unwrap_or_else(|error| panic!("the model file {file} is needed: {error}"));
            ModelFileDigest {
                path: (*file).to_owned(),
                sha256: hex(&Sha256::digest(&bytes)),
            }
        })
        .collect();
    (dir, ModelManifest { files })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A bare repository with one commit, and a clone of it whose origin is the bare repository.
fn bare_and_clone(dir: &Path) -> (PathBuf, PathBuf) {
    let bare = dir.join("remote.git");
    std::fs::create_dir_all(&bare).expect("bare");
    git(&bare, &["init", "--bare", "-q", "-b", "main"]);
    let clone = dir.join("project");
    git(dir, &["clone", "-q", &bare.to_string_lossy(), "project"]);
    git(&clone, &["config", "user.name", "Asha"]);
    git(&clone, &["config", "user.email", "asha@example.test"]);
    git(&clone, &["checkout", "-q", "-b", "main"]);
    std::fs::write(clone.join("README.md"), "Project\n").expect("readme");
    git(&clone, &["add", "README.md"]);
    git(&clone, &["commit", "-q", "-m", "First"]);
    git(&clone, &["push", "-q", "origin", "main"]);
    (bare, clone)
}

// ---------------------------------------------------------------------------------------------
// The model stand-in.

/// One scripted tool call of Ordin's: made once, then Ordin's turn ends with text.
#[derive(Clone)]
struct Call {
    id: String,
    name: &'static str,
    input: Value,
}

/// The stand-in's script. Ordin's requests carry Ordin's own tools; the agent's carry `Bash` (a
/// system-changing agent in a workspace with a project) and none of Ordin's.
#[derive(Clone, Default)]
struct Script {
    /// The next call Ordin makes; set by the test before the step that needs it.
    ordin: Arc<Mutex<Option<Call>>>,
    /// The directory key of the crew's member, for Ordin's `delegate` as coordinator.
    crew_member: Arc<Mutex<String>>,
    /// Every request body, in order.
    bodies: Arc<Mutex<Vec<String>>>,
}

/// The agent's one shell call: `touch` is on its approval list (the fixture's capability).
const AGENT_TOUCH_ID: &str = "toolu_e2e_agent_touch";
/// Ordin's calls as the crew's coordinator.
const CREW_DELEGATE_ID: &str = "toolu_e2e_crew_delegate";
const CREW_FINISH_ID: &str = "toolu_e2e_crew_finish";

impl Script {
    fn next(&self, call: Option<Call>) {
        *self.ordin.lock().unwrap() = call;
    }

    fn bodies(&self) -> Vec<String> {
        self.bodies.lock().unwrap().clone()
    }

    /// The text Claude Code sent back for the tool call `id`, once it has.
    fn tool_result(&self, id: &str) -> Option<String> {
        let marker = format!("\"tool_use_id\":\"{id}\"");
        self.bodies()
            .into_iter()
            .find(|body| body.contains(&marker))
            .map(|body| {
                let at = body.find(&marker).unwrap_or(0);
                body[at..].chars().take(4_000).collect()
            })
    }
}

fn sse(content: &[Value], stop_reason: &str) -> Vec<u8> {
    let mut events = vec![json!({"type": "message_start", "message": {
        "id": "msg_e2e", "type": "message", "role": "assistant", "model": "stand-in",
        "content": [], "stop_reason": null, "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 1}}})];
    for (index, block) in content.iter().enumerate() {
        if let Some(text) = block.get("text") {
            events.push(json!({"type": "content_block_start", "index": index,
                "content_block": {"type": "text", "text": ""}}));
            events.push(json!({"type": "content_block_delta", "index": index,
                "delta": {"type": "text_delta", "text": text}}));
        } else {
            events.push(json!({"type": "content_block_start", "index": index,
                "content_block": {"type": "tool_use", "id": block["id"], "name": block["name"],
                                  "input": {}}}));
            events.push(json!({"type": "content_block_delta", "index": index,
                "delta": {"type": "input_json_delta",
                          "partial_json": block["input"].to_string()}}));
        }
        events.push(json!({"type": "content_block_stop", "index": index}));
    }
    events.push(json!({"type": "message_delta",
        "delta": {"stop_reason": stop_reason, "stop_sequence": null},
        "usage": {"output_tokens": 5}}));
    events.push(json!({"type": "message_stop"}));
    let mut body = String::new();
    for event in events {
        body.push_str(&format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().expect("a type")
        ));
    }
    body.into_bytes()
}

fn text_reply(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(
        sse(&[json!({ "text": text })], "end_turn"),
        "text/event-stream",
    )
}

fn tool_reply(id: &str, name: &str, input: &Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(
        sse(
            &[json!({"id": id, "name": name, "input": input})],
            "tool_use",
        ),
        "text/event-stream",
    )
}

impl Respond for Script {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        if !request.url.path().ends_with("/v1/messages") {
            return ResponseTemplate::new(200).set_body_json(json!({"input_tokens": 1}));
        }
        let body = String::from_utf8_lossy(&request.body).into_owned();
        self.bodies.lock().unwrap().push(body.clone());
        let answered = |id: &str| body.contains(&format!("\"tool_use_id\":\"{id}\""));
        if body.contains("mcp__ordin__delegate") {
            // Ordin coordinating the crew: its first round turn gives the writer its step, the
            // next one finishes. The conversation is one session, so the body holds both turns.
            let rounds = body.matches("Crew round ").count();
            if rounds >= 2 && !answered(CREW_FINISH_ID) {
                let input = json!({"summary": "The writer checked the note."});
                return tool_reply(CREW_FINISH_ID, "mcp__ordin__finish", &input);
            }
            if rounds == 1 && !answered(CREW_DELEGATE_ID) {
                let input = json!({"agent": self.crew_member.lock().unwrap().clone(),
                                   "brief": "Check the note in note.md."});
                return tool_reply(CREW_DELEGATE_ID, "mcp__ordin__delegate", &input);
            }
            let next = self.ordin.lock().unwrap().clone();
            return match next {
                Some(call) if !answered(&call.id) => tool_reply(&call.id, call.name, &call.input),
                _ => text_reply("Done."),
            };
        }
        // The crew member's step (its brief names "Your part:") only reports.
        if body.contains("\"name\":\"Bash\"")
            && !body.contains("Your part:")
            && !answered(AGENT_TOUCH_ID)
        {
            let input = json!({"command": "touch note.md", "description": "Make the note"});
            return tool_reply(AGENT_TOUCH_ID, "Bash", &input);
        }
        text_reply("Written.")
    }
}

// ---------------------------------------------------------------------------------------------
// The Confluence stand-in: Atlassian's gateway and the company site's v1 and v2 surfaces, in the
// shapes ConfluenceAdapter.md names, for one space and its parent page.

fn wiki(tail: &str) -> String {
    format!("/ex/confluence/{CONFLUENCE_CLOUD}/wiki{tail}")
}

fn confluence_page(id: &str, title: &str, version: u64) -> Value {
    json!({
        "id": id,
        "status": "current",
        "title": title,
        "spaceId": "65010",
        "parentId": CONFLUENCE_PARENT,
        "version": {"number": version, "authorId": "acc-1", "createdAt": "2026-01-29T09:00:00.000Z"},
        "body": {"storage": {"value": "<p>Body</p>", "representation": "storage"}},
        "_links": {"webui": format!("/spaces/PROC/pages/{id}")},
    })
}

async fn mount_confluence(server: &MockServer) {
    let replies = [
        (
            "/oauth/token/accessible-resources".to_owned(),
            json!([{"id": CONFLUENCE_CLOUD, "url": CONFLUENCE_SITE, "name": "acme", "scopes": []}]),
        ),
        (
            wiki(&format!(
                "/rest/api/content/{CONFLUENCE_PARENT}/restriction/byOperation/read"
            )),
            json!({"restrictions": {
                "user": {"results": []},
                "group": {"results": [{"name": "Procurement team", "id": "g-1"}]},
            }}),
        ),
        (
            wiki("/api/v2/spaces"),
            json!({"results": [{"id": "65010", "key": "PROC", "name": "Procurement",
                                "_links": {"webui": "/spaces/PROC"}}]}),
        ),
        (
            wiki(&format!("/api/v2/pages/{CONFLUENCE_PARENT}")),
            confluence_page(CONFLUENCE_PARENT, "Supplier pages", 3),
        ),
        (
            wiki(&format!("/api/v2/pages/{CONFLUENCE_PARENT}/children")),
            json!({"results": [], "_links": {}}),
        ),
        (
            wiki(&format!("/api/v2/pages/{CONFLUENCE_PARENT}/attachments")),
            json!({"results": [], "_links": {}}),
        ),
        (
            wiki("/rest/api/user"),
            json!({"accountId": "acc-1", "displayName": "Sam Lee"}),
        ),
    ];
    for (at, body) in replies {
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }
}

// ---------------------------------------------------------------------------------------------
// The push stand-in: Git's smart HTTP protocol through `git http-backend` over the folder that
// holds the bare repository, for a request that carries the token as GitHub's user name and
// password; any other request is challenged.

struct GitHost {
    root: PathBuf,
    authorization: String,
}

impl GitHost {
    fn new(root: &Path) -> GitHost {
        let pair = format!("x-access-token:{PUSH_TOKEN}");
        GitHost {
            root: root.to_path_buf(),
            authorization: format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(pair)
            ),
        }
    }
}

impl Respond for GitHost {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let header = |name: &str| {
            request
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };
        if header("authorization") != self.authorization {
            return ResponseTemplate::new(401)
                .insert_header("WWW-Authenticate", "Basic realm=\"stand-in\"");
        }
        let mut cgi = std::process::Command::new("git")
            .arg("http-backend")
            .env("GIT_PROJECT_ROOT", &self.root)
            .env("GIT_HTTP_EXPORT_ALL", "1")
            .env("REMOTE_USER", "x-access-token")
            .env("REMOTE_ADDR", "127.0.0.1")
            .env("REQUEST_METHOD", request.method.as_str())
            .env("PATH_INFO", request.url.path())
            .env("QUERY_STRING", request.url.query().unwrap_or_default())
            .env("CONTENT_TYPE", header("content-type"))
            .env("CONTENT_LENGTH", request.body.len().to_string())
            .env("HTTP_CONTENT_ENCODING", header("content-encoding"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("git http-backend");
        {
            use std::io::Write as _;
            let mut stdin = cgi.stdin.take().expect("stdin");
            stdin.write_all(&request.body).expect("the request body");
        }
        let output = cgi.wait_with_output().expect("git http-backend's answer");
        let out = output.stdout;
        let (head, body) = match out.windows(4).position(|w| w == b"\r\n\r\n") {
            Some(at) => (&out[..at], &out[at + 4..]),
            None => match out.windows(2).position(|w| w == b"\n\n") {
                Some(at) => (&out[..at], &out[at + 2..]),
                None => (&out[..0], &out[..]),
            },
        };
        let mut status = 200;
        let mut content_type = "application/octet-stream".to_owned();
        let mut extra = Vec::new();
        for line in String::from_utf8_lossy(head).lines() {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().to_owned();
            match name.trim().to_ascii_lowercase().as_str() {
                "status" => {
                    status = value
                        .split_whitespace()
                        .next()
                        .and_then(|code| code.parse().ok())
                        .unwrap_or(500);
                }
                "content-type" => content_type = value,
                other => extra.push((other.to_owned(), value)),
            }
        }
        let mut reply = ResponseTemplate::new(status).set_body_raw(body.to_vec(), &content_type);
        for (name, value) in extra {
            reply = reply.insert_header(name.as_str(), value.as_str());
        }
        reply
    }
}

// ---------------------------------------------------------------------------------------------
// The update feed and the stand-in for the window's updater.

/// Reads the feed the Setup Assistant names, in Tauri's `latest.json` form, as the window's updater
/// does; a download is kept at once.
struct FeedUpdater {
    http: reqwest::Client,
}

#[async_trait::async_trait]
impl AppUpdater for FeedUpdater {
    async fn check(&self, feed_url: &reqwest::Url) -> Result<Option<UpdateOffer>, UpdaterError> {
        let feed: Value = self
            .http
            .get(feed_url.clone())
            .send()
            .await
            .map_err(|_| UpdaterError::Unreachable)?
            .json()
            .await
            .map_err(|_| UpdaterError::Unreachable)?;
        let version = feed["version"].as_str().unwrap_or_default().to_owned();
        let notes = feed["notes"].as_str().unwrap_or_default().to_owned();
        Ok(Some(UpdateOffer {
            version,
            notes,
            state: UpdateState::Downloading,
        }))
    }
    async fn download(&self, _offer: &UpdateOffer) -> Result<(), UpdaterError> {
        Ok(())
    }
    async fn install_and_restart(&self) -> Result<(), UpdaterError> {
        Ok(())
    }
    async fn install_on_exit(&self) -> Result<(), UpdaterError> {
        Ok(())
    }
    fn install_kind(&self) -> InstallKind {
        InstallKind::PerUser
    }
    fn other_install(&self) -> Option<PathBuf> {
        None
    }
}

/// A version newer than the one running: the next patch, marked as a test version.
fn test_version() -> String {
    let mut version = semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("the version");
    version.patch += 1;
    version.pre = semver::Prerelease::new("test.1").expect("pre-release");
    version.to_string()
}

// ---------------------------------------------------------------------------------------------
// The test client: Service API envelopes through the Command Bridge, and the screens' events.

struct Client {
    bridge: CommandBridge,
    backend: BackendId,
    events: Arc<Mutex<Vec<Value>>>,
}

impl Client {
    async fn open(core: &Core) -> Client {
        let bridge = (*core.modules.bridge.clone().expect("row 3.49")).clone();
        let backend = core.modules.backend.expect("the backend");
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        bridge
            .open_events(
                WINDOW,
                BTreeMap::new(),
                Box::new(move |batch: EventBatch| {
                    let mut sink = sink.lock().unwrap();
                    for event in batch.events {
                        if let Ok(value) = serde_json::from_str::<Value>(event.get()) {
                            sink.push(value);
                        }
                    }
                    Ok(())
                }),
            )
            .await
            .expect("service_events");
        Client {
            bridge,
            backend,
            events,
        }
    }

    /// One command as the window sends it; `Err` is the reply's `error`.
    async fn call(&self, name: &str, input: Value) -> Result<Value, Value> {
        let envelope = json!({
            "api_version": ServiceApiVersion::CURRENT,
            "request_id": CommandRequestId::new(),
            "command": name,
            "backend": self.backend,
            "input": input,
        });
        let reply = self
            .bridge
            .invoke(WINDOW, name, &envelope.to_string())
            .await
            .expect("the window is open");
        let reply: Value = serde_json::from_str(&reply).expect("a reply");
        match reply["kind"].as_str() {
            Some("ok") => Ok(reply["result"].clone()),
            _ => Err(reply["error"].clone()),
        }
    }

    async fn ok(&self, name: &str, input: Value) -> Value {
        match self.call(name, input).await {
            Ok(result) => result,
            Err(error) => panic!("{name} was refused: {error}"),
        }
    }

    fn events_of(&self, kind: &str) -> Vec<Value> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == kind)
            .cloned()
            .collect()
    }
}

/// Waits up to `limit` for `check` to give a value.
async fn wait_for<T, F, Fut>(what: &str, limit: Duration, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let started = Instant::now();
    loop {
        if let Some(value) = check().await {
            return value;
        }
        assert!(started.elapsed() < limit, "{what}: not within {limit:?}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// The first value of `key` anywhere in `value`.
fn find(value: &Value, key: &str) -> Option<Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .cloned()
            .or_else(|| map.values().find_map(|v| find(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find(v, key)),
        _ => None,
    }
}

fn text_of(value: &Value, key: &str) -> String {
    find(value, key)
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("no {key} in {value}"))
}

fn step(n: u32, name: &str, started: Instant) {
    eprintln!(
        "e2e step {n:>2} done at {:>6.1}s: {name}",
        started.elapsed().as_secs_f64()
    );
}

// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "end-to-end job: needs Claude Code 2.1.316 and the memory models"]
async fn a_person_goes_through_the_r1_journeys_end_to_end() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
    let started = Instant::now();
    let dir = TempDir::new();
    let (models_dir, model_manifest) = models();

    // The stand-ins: the model, the update feed.
    let script = Script::default();
    let model = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(script.clone())
        .mount(&model)
        .await;
    let feed = MockServer::start().await;
    let version = test_version();
    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "version": version,
            "notes": "The test release.",
            "pub_date": "2026-01-29T00:00:00Z",
            "platforms": {
                "darwin-aarch64": {"signature": "test", "url": format!("{}/ordin.app.tar.gz", feed.uri())},
                "windows-x86_64": {"signature": "test", "url": format!("{}/ordin-setup.exe", feed.uri())},
                "linux-x86_64": {"signature": "test", "url": format!("{}/ordin.AppImage", feed.uri())}
            }
        })))
        .mount(&feed)
        .await;
    let confluence = MockServer::start().await;
    mount_confluence(&confluence).await;
    let git_host = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(GitHost::new(&dir.0))
        .mount(&git_host)
        .await;

    // The program's inputs, as `window.rs` fills them, on an empty data root.
    let exe_dir = dir.0.join("program");
    hook_beside_the_program(&exe_dir);
    let installation_id = uuid::Uuid::now_v7();
    let paths = DesktopPaths::new(dir.0.join("data"), &exe_dir, &installation_id.to_string());
    paths.prepare().expect("the data root");
    let http = reqwest::Client::new();
    let inputs = CoreInputs {
        paths,
        http: http.clone(),
        bundle_identifier: "io.gatblau.ordin.e2e".to_owned(),
        computer_name: "Test computer".to_owned(),
        installation_id,
        os_locales: vec!["en-GB".to_owned()],
        home: dir.0.join("home"),
        installer_defaults: InstallerSettings {
            update_feed_url: Some(format!("{}/latest.json", feed.uri())),
            ..InstallerSettings::default()
        },
        browser: None,
        models_dir,
        model_manifest,
        // Never fetched: the models are already in place.
        models_source: Some(ModelsSource {
            url: "https://models.invalid/ordin-memory-models.zip"
                .parse()
                .expect("url"),
            sha256: "0".repeat(64),
            bytes: 1,
        }),
        claude_path: claude_wrapper(&dir.0, &model.uri()),
        documents_dir: dir.0.join("Documents"),
        bundled_catalogue: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/e2e-catalogue"),
        url_opener: None,
        updater: Some(Arc::new(FeedUpdater { http })),
        shell: None,
    };

    // 1. First start.
    let core = Core::start(inputs).await.expect("start");
    let client = Client::open(&core).await;
    let status = client.ok("setup.status", json!({})).await;
    assert_eq!(status["needs_guided"], json!(true), "{status}");
    let listed = client
        .ok("workspaces.list", json!({"filter": "active"}))
        .await;
    assert_eq!(listed, json!([]), "no workspace yet");
    let memory = client.ok("app.memory_models", json!({})).await;
    assert_eq!(memory["state"], json!("ready"), "{memory}");
    let ordin = BundledAgent::Ordin.instance_id();
    wait_for(
        "Ordin's own agent is ready",
        Duration::from_secs(120),
        || {
            let client = &client;
            async move {
                let agent = client
                    .call("agents.get", json!({ "agent_instance_id": ordin }))
                    .await
                    .ok()?;
                (agent["instance"]["status"] == "ready").then_some(())
            }
        },
    )
    .await;
    step(1, "first start", started);

    // 2. A workspace is created, the note writer is added to it from the bundled catalogue, and a
    // repository folder is linked.
    let workspace = client
        .ok("workspaces.create", json!({ "name": "Release" }))
        .await;
    let ws = text_of(&workspace, "id");
    let own_place = text_of(&workspace, "own_folder_place_id");
    wait_for("the screens hear of it", Duration::from_secs(10), || {
        let client = &client;
        async move { (!client.events_of("workspace_changed").is_empty()).then_some(()) }
    })
    .await;
    let library = client
        .ok(
            "catalogue.search",
            json!({"text": null, "kinds": [], "repositories": [], "area": null,
                   "audience": null, "publisher": null, "include_not_for_new": false,
                   "cursor": null}),
        )
        .await;
    let writer_part = library["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|e| e["part"]["part"]["identifier"] == "urn:ordin:default:workspaces:echo")
        .map(|e| e["part"]["part"]["id"].clone())
        .unwrap_or_else(|| panic!("the note writer is in the library: {library}"));
    let added = client
        .ok(
            "agents.add",
            json!({"workspace_id": ws, "agent": writer_part, "interface_locale": "en-GB",
                   "choices": []}),
        )
        .await;
    assert_eq!(added["kind"], json!("ready"), "{added}");
    let writer = added["agent"]["instance"]["id"]
        .as_str()
        .expect("the agent's id")
        .to_owned();
    let workspaces = client
        .ok("workspaces.list", json!({"filter": "active"}))
        .await;
    assert_eq!(workspaces.as_array().map(Vec::len), Some(1));
    // A repository folder linked and opened as a project: a system-changing agent is given a shell
    // only in a workspace with a project (OrdinDesktop.md DSK-REC-27).
    let (bare, clone) = bare_and_clone(&dir.0);
    let before = git(&bare, &["rev-parse", "main"]);
    let link = client
        .ok(
            "workspaces.add_link",
            json!({"workspace_id": ws,
                   "location": {"kind": "git_repository_folder", "path": clone.to_string_lossy()},
                   "display_name": "Project"}),
        )
        .await;
    let place = text_of(&link, "place_id");
    let project = client
        .ok(
            "projects.open",
            json!({"place_id": place, "workspace_id": ws}),
        )
        .await;
    let project_id = find(&project, "project_id")
        .or_else(|| find(&project, "id"))
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("a project id: {project}"));
    step(2, "workspace created", started);

    // 3 and 4. A message to Ordin; Ordin delegates to the note writer: one session.
    script.next(Some(Call {
        id: "toolu_e2e_delegate".to_owned(),
        name: "mcp__ordin__delegate",
        input: json!({"agent": writer, "brief": "Write a short note in note.md."}),
    }));
    let sent = client
        .ok(
            "ordin.send_message",
            json!({"workspace_id": ws, "text": "Please write a short note."}),
        )
        .await;
    assert_eq!(sent["accepted"], json!(true), "{sent}");
    let delegated = wait_for("Ordin's delegation", Duration::from_secs(90), || {
        let script = &script;
        async move { script.tool_result("toolu_e2e_delegate") }
    })
    .await;
    // The tool result is the structured `{"run_id": …}` of the started session.
    assert!(delegated.contains("run_id"), "{delegated}");
    step(3, "message to Ordin", started);

    // 5. One approval: the session's `touch` waits for the person.
    let pending = wait_for("the approval", Duration::from_secs(90), || {
        let client = &client;
        let ws = &ws;
        async move {
            let page = client
                .ok(
                    "approval.list_pending",
                    json!({"workspace_id": ws, "cursor": null}),
                )
                .await;
            page["items"].as_array()?.first().cloned()
        }
    })
    .await;
    assert!(!client.events_of("approval_requested").is_empty());
    let approve = text_of(&find(&pending, "buttons").expect("buttons"), "approve");
    let outcome = client
        .ok(
            "approval.decide",
            json!({"card_ref": approve, "decision": "approve"}),
        )
        .await;
    assert_eq!(outcome["state"], json!("approved"), "{outcome}");
    let touched = wait_for("the approved call", Duration::from_secs(60), || {
        let script = &script;
        async move { script.tool_result(AGENT_TOUCH_ID) }
    })
    .await;
    assert!(!touched.contains("is_error\":true"), "{touched}");
    let runs = wait_for("the session ends", Duration::from_secs(60), || {
        let client = &client;
        let ws = &ws;
        async move {
            let page = client
                .ok("runs.list", json!({"workspace_id": ws, "cursor": null}))
                .await;
            let task = page["items"]
                .as_array()?
                .iter()
                .find(|r| find(r, "origin").is_some_and(|o| o["kind"] == "task"))?
                .clone();
            (task["state"] == "completed").then_some(page)
        }
    })
    .await;
    let sessions = runs["items"]
        .as_array()
        .expect("runs")
        .iter()
        .filter(|r| find(r, "origin").is_some_and(|o| o["kind"] == "task"))
        .count();
    assert_eq!(sessions, 1, "one session: {runs}");
    assert!(!client.events_of("approval_decided").is_empty());
    step(4, "one session and one approval", started);

    // 4 (crew). Ordin proposes a crew of one; the person presses Start; Ordin coordinates it from
    // its conversation, which stays open between turns (ClaudeLocalAdapter.md CLA-CONV-2): round 1
    // gives the writer its step, round 2 finishes, and the crew ends `finished`.
    *script.crew_member.lock().unwrap() = writer.clone();
    script.next(Some(Call {
        id: "toolu_e2e_crew".to_owned(),
        name: "mcp__ordin__create_crew",
        input: json!({"purpose": "Check the note.",
                      "members": [{"agent": writer, "role": "writer"}]}),
    }));
    client
        .ok(
            "ordin.send_message",
            json!({"workspace_id": ws, "text": "Make a crew to check the note."}),
        )
        .await;
    let charter = wait_for("the crew plan", Duration::from_secs(90), || {
        let client = &client;
        async move {
            client
                .events_of("block_shown")
                .iter()
                .find_map(|e| find(e, "crew_charter_id"))
        }
    })
    .await;
    script.next(None);
    let crew = client
        .ok("crews.start", json!({"crew_charter_id": charter}))
        .await;
    let crew_run = text_of(&crew, "crew_run_id");
    let ended = wait_for("the crew ends", Duration::from_secs(120), || {
        let client = &client;
        let crew_run = crew_run.clone();
        async move {
            let got = client
                .ok("crews.get", json!({"crew_run_id": crew_run}))
                .await;
            let state = got["crew_run"]["state"].as_str()?.to_owned();
            (!matches!(state.as_str(), "starting" | "running" | "paused")).then_some(got)
        }
    })
    .await;
    assert!(!client.events_of("crew_round_changed").is_empty());
    assert_eq!(ended["crew_run"]["state"], json!("finished"), "{ended}");
    assert!(
        ended["rounds"].as_array().is_some_and(|r| !r.is_empty()),
        "{ended}"
    );
    assert!(
        script.tool_result(CREW_FINISH_ID).is_some(),
        "Ordin finished the crew"
    );
    // One conversation run carried every turn of Ordin's so far.
    let runs = client
        .ok("runs.list", json!({"workspace_id": ws, "cursor": null}))
        .await;
    let conversations = runs["items"]
        .as_array()
        .expect("runs")
        .iter()
        .filter(|r| find(r, "origin").is_some_and(|o| o["kind"] == "conversation"))
        .count();
    assert_eq!(conversations, 1, "one conversation run: {runs}");
    step(4, "one crew coordinated by Ordin, finished", started);

    // 6. A knowledge page is saved and found again; memory recalls with the real models.
    let saved = client
        .ok(
            "knowledge.write",
            json!({"workspace_id": ws, "title": PAGE_TITLE, "body_markdown": PAGE_BODY,
                   "type": "How-to", "description": "How the quarzite release is checked."}),
        )
        .await;
    let page_id = text_of(&saved, "page_id");
    let page = client
        .ok("knowledge.get", json!({"page_id": page_id}))
        .await;
    assert_eq!(find(&page, "title"), Some(json!(PAGE_TITLE)), "{page}");
    let found = client
        .ok(
            "search.query",
            json!({"query": "quarzite", "current_workspace_id": ws}),
        )
        .await;
    assert!(found.to_string().contains(PAGE_TITLE), "{found}");
    // Memory with the real models: the page reaches memory through the Knowledge Service's index
    // task (OrdinDesktop.md DSK-KS-1), and Ordin's recall finds it.
    let mut attempt = 0;
    let recalled = wait_for("Ordin recalls the page", Duration::from_secs(120), || {
        attempt += 1;
        let id = format!("toolu_e2e_recall_{attempt}");
        let client = &client;
        let script = &script;
        let ws = &ws;
        async move {
            let result = ordin_call(
                client,
                script,
                ws,
                &id,
                "mcp__ordin__recall",
                json!({"query": "what are the steps of the quarzite release checklist"}),
            )
            .await;
            if result.contains("build, sign and publish") {
                return Some(result);
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
            None
        }
    })
    .await;
    assert!(!recalled.contains("\"is_error\":true"), "{recalled}");
    step(
        6,
        "knowledge page saved, found, and recalled from memory",
        started,
    );

    // 7. A Confluence link of the workspace's own folder: its audience is read from the stand-in
    // with the seam's token, the person approves the card, and the link is active.
    let confluence_adapter = core.modules.confluence.clone().expect("row 3.30");
    confluence_adapter
        .use_test_seam(ConfluenceTestSeam {
            gateway: confluence.uri(),
            site: CONFLUENCE_SITE.parse().expect("the site"),
            token: CONFLUENCE_TOKEN.to_owned(),
        })
        .expect("the Confluence seam");
    // The linked folder exists before it is linked; a new file in it waits for the person's card.
    client
        .ok(
            "files.save_edit",
            json!({"place_id": own_place, "path": "Confluence/Rates.md", "text": "# Rates\n"}),
        )
        .await;
    let proposed = client
        .ok(
            "workspaces.propose_confluence_link",
            json!({"workspace_id": ws, "place_id": own_place, "folder": "Confluence",
                   "location": {"space_key": "PROC", "space_name": "Procurement",
                                "page_id": CONFLUENCE_PARENT, "title": "Supplier pages",
                                "web_url": format!("{CONFLUENCE_SITE}/wiki/spaces/PROC/pages/{CONFLUENCE_PARENT}")}}),
        )
        .await;
    assert_eq!(proposed["state"], json!("proposed"), "{proposed}");
    assert_eq!(
        proposed["audience"],
        json!("Procurement team"),
        "{proposed}"
    );
    let link_id = text_of(&proposed, "id");
    let card = wait_for("the link's approval", Duration::from_secs(30), || {
        let client = &client;
        let ws = &ws;
        async move {
            let page = client
                .ok(
                    "approval.list_pending",
                    json!({"workspace_id": ws, "cursor": null}),
                )
                .await;
            page["items"].as_array()?.first().cloned()
        }
    })
    .await;
    let approve = text_of(&find(&card, "buttons").expect("buttons"), "approve");
    let decided = client
        .ok(
            "approval.decide",
            json!({"card_ref": approve, "decision": "approve"}),
        )
        .await;
    assert_eq!(decided["state"], json!("approved"), "{decided}");
    // Only an active link can be paused, so a pause that is answered proves the approval made it
    // active; it is resumed at once.
    let paused = wait_for("the link is active", Duration::from_secs(30), || {
        let client = &client;
        let link_id = &link_id;
        async move {
            client
                .call(
                    "workspaces.pause_confluence_link",
                    json!({"confluence_link_id": link_id}),
                )
                .await
                .ok()
        }
    })
    .await;
    assert_eq!(paused["state"], json!("paused"), "{paused}");
    let resumed = client
        .ok(
            "workspaces.resume_confluence_link",
            json!({"confluence_link_id": link_id}),
        )
        .await;
    assert_eq!(resumed["state"], json!("active"), "{resumed}");
    let seen = confluence.received_requests().await.unwrap_or_default();
    assert!(
        seen.iter()
            .any(|r| r.url.path().ends_with("/restriction/byOperation/read")),
        "the audience was read from the stand-in"
    );
    assert!(
        seen.iter().all(
            |r| r.headers.get("authorization").and_then(|v| v.to_str().ok())
                == Some(format!("Bearer {CONFLUENCE_TOKEN}").as_str())
        ),
        "every request carried the seam's token"
    );
    step(7, "Confluence link proposed, approved and active", started);

    // 8 and 9. The person edits a file of the project; it is committed and pushed to the stand-in
    // host, which keeps the push in the bare repository. Home shows the change.
    client
        .ok(
            "files.save_edit",
            json!({"place_id": place, "path": "notes.md", "text": "The release is ready.\n"}),
        )
        .await;
    client
        .ok(
            "projects.commit",
            json!({"project_id": project_id, "message": "Add the release notes", "terminal": null}),
        )
        .await;
    assert_eq!(
        git(&clone, &["log", "-1", "--format=%s"]),
        "Add the release notes"
    );
    let head = git(&clone, &["rev-parse", "HEAD"]);
    assert_eq!(git(&bare, &["rev-parse", "main"]), before);
    git(
        &clone,
        &[
            "remote",
            "set-url",
            "origin",
            &format!("{}/remote.git", git_host.uri()),
        ],
    );
    core.modules
        .projects
        .clone()
        .expect("row 3.31")
        .use_test_host(&git_host.uri(), PUSH_TOKEN)
        .expect("the push seam");
    let pushed = client
        .ok(
            "projects.push",
            json!({"project_id": project_id, "terminal": null}),
        )
        .await;
    assert_eq!(pushed["commit"], json!(head), "{pushed}");
    assert_eq!(pushed["branch"], json!("main"), "{pushed}");
    assert_eq!(
        git(&bare, &["rev-parse", "main"]),
        head,
        "the stand-in host has the commit"
    );
    step(8, "commit made and pushed to the stand-in host", started);

    let marks = wait_for("Home marks the change", Duration::from_secs(30), || {
        let client = &client;
        let ws = ws.clone();
        async move {
            let marks = client
                .ok("home.changed_marks", json!({"workspace_id": ws}))
                .await;
            marks
                .as_array()?
                .iter()
                .any(|m| m["path"].as_str().is_some_and(|p| p.ends_with("notes.md")))
                .then_some(marks)
        }
    })
    .await;
    let home = client.ok("home.view", json!({"workspace_id": ws})).await;
    let since = home["since_last_looked"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!since.is_empty(), "Home shows what changed: {home}");
    assert!(!marks.as_array().expect("marks").is_empty());
    step(9, "Home shows the changes", started);

    // 10. Check for updates finds the test version.
    let checked = client.ok("setup.check_update", json!({})).await;
    assert_eq!(checked["offer"]["version"], json!(version), "{checked}");
    let ready = wait_for("the update is ready", Duration::from_secs(30), || {
        let client = &client;
        async move {
            let offer = client.ok("setup.update_offer", json!({})).await;
            (find(&offer, "state") == Some(json!("ready"))).then_some(offer)
        }
    })
    .await;
    assert_eq!(find(&ready, "version"), Some(json!(version)));
    let fetched = feed.received_requests().await.unwrap_or_default();
    assert!(
        fetched.iter().any(|r| r.url.path() == "/latest.json"),
        "the feed was read"
    );
    step(10, "Check for updates finds the test version", started);

    // Quit as the person does.
    let _ = core.quit(QuitReason::PersonQuit).await;
    assert!(
        script.bodies().iter().all(|b| !b.contains("sk-ant-api")),
        "no real key reached the stand-in"
    );
}

/// One turn in which Ordin makes `name` with `input`; the text Claude Code sent back for it.
async fn ordin_call(
    client: &Client,
    script: &Script,
    ws: &str,
    id: &str,
    name: &'static str,
    input: Value,
) -> String {
    script.next(Some(Call {
        id: id.to_owned(),
        name,
        input,
    }));
    client
        .ok(
            "ordin.send_message",
            json!({"workspace_id": ws, "text": "Please go on."}),
        )
        .await;
    wait_for_quietly(Duration::from_secs(60), || script.tool_result(id))
        .await
        .unwrap_or_else(|| panic!("Ordin's {name} call was not answered"))
}

/// Like [`wait_for`], but gives `None` instead of failing when the limit passes.
async fn wait_for_quietly<T>(limit: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let started = Instant::now();
    loop {
        if let Some(value) = check() {
            return Some(value);
        }
        if started.elapsed() > limit {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
