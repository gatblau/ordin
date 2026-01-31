//! Language servers (ADR-055, ADR-056; Internal Logic 9.3, 9.4): child processes speaking LSP over
//! stdio, framed here with `Content-Length` headers and `serde_json` messages.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use ordin_types::domain::LanguagePluginStatus;
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::{CodingTaskId, PlaceId, ProjectId};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::error::ProjectError;
use crate::git;
use crate::languages::{pin_of, server_of};
use crate::log::LanguageServerEvent;
use crate::service::{lock, ProjectService};
use crate::store;
use crate::types::{StreamId, StreamPayload, StreamSink};

/// A frame over this size is malformed.
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Which instance: one per (project, language) for the editor, one per (coding task, language), and
/// the transient base instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum InstanceKey {
    Editor(ProjectId, String),
    Task(CodingTaskId, String),
    Base(CodingTaskId, String),
}

impl InstanceKey {
    fn language(&self) -> &str {
        match self {
            InstanceKey::Editor(_, language)
            | InstanceKey::Task(_, language)
            | InstanceKey::Base(_, language) => language,
        }
    }
}

/// Who waits for the answer to a request id.
enum Waiter {
    Ordin(oneshot::Sender<Value>),
    Editor { stream: StreamId, client_id: Value },
}

/// The state the reader task shares with the service.
pub(crate) struct Instance {
    pub key: InstanceKey,
    pub number: u64,
    pub server: String,
    pub root: PathBuf,
    outgoing: mpsc::UnboundedSender<Value>,
    next_id: AtomicI64,
    waiters: Mutex<HashMap<i64, Waiter>>,
    diagnostics: Mutex<HashMap<String, Vec<Value>>>,
    last_diagnostics: Mutex<Instant>,
    settings: Mutex<Value>,
    sessions: Mutex<HashSet<StreamId>>,
    opened: Mutex<HashSet<String>>,
    last_used: Mutex<Instant>,
    pub(crate) stopping: AtomicBool,
    dead: CancellationToken,
    streams: Arc<dyn StreamSink>,
}

impl Instance {
    fn write(&self, message: Value) -> Result<(), ProjectError> {
        self.outgoing
            .send(message)
            .map_err(|_| ProjectError::LspUnavailable)
    }

    pub(crate) fn notify(&self, method: &str, params: Value) -> Result<(), ProjectError> {
        self.write(json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// One request of ordin's own, answered within `limit`.
    pub(crate) async fn request(
        &self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, ProjectError> {
        *lock(&self.last_used) = Instant::now();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        lock(&self.waiters).insert(id, Waiter::Ordin(sender));
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let answer = tokio::select! {
            _ = self.dead.cancelled() => None,
            answer = tokio::time::timeout(limit, receiver) => answer.ok().and_then(Result::ok),
        };
        lock(&self.waiters).remove(&id);
        let answer = answer.ok_or(ProjectError::LspUnavailable)?;
        if answer.get("error").is_some() {
            return Err(ProjectError::LspUnavailable);
        }
        Ok(answer.get("result").cloned().unwrap_or(Value::Null))
    }

    /// An editor's request: its id is remapped so several editors share the instance.
    pub(crate) fn relay(&self, stream: StreamId, mut message: Value) -> Result<(), ProjectError> {
        *lock(&self.last_used) = Instant::now();
        if let Some(client_id) = message.get("id").cloned() {
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            lock(&self.waiters).insert(id, Waiter::Editor { stream, client_id });
            message["id"] = json!(id);
        }
        self.write(message)
    }

    /// Sends `didOpen` once per document.
    pub(crate) fn open_document(
        &self,
        uri: &str,
        language: &str,
        text: &str,
    ) -> Result<(), ProjectError> {
        if !lock(&self.opened).insert(uri.to_owned()) {
            return Ok(());
        }
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": language, "version": 1, "text": text,
            }}),
        )
    }

    pub(crate) fn attach(&self, stream: StreamId) {
        lock(&self.sessions).insert(stream);
    }

    pub(crate) fn detach(&self, stream: StreamId) {
        lock(&self.sessions).remove(&stream);
    }

    pub(crate) fn alive(&self) -> bool {
        !self.dead.is_cancelled()
    }

    pub(crate) fn set_settings(&self, settings: &Value) {
        *lock(&self.settings) = settings.clone();
    }

    /// The last `publishDiagnostics` for a document.
    pub(crate) fn diagnostics_of(&self, uri: &str) -> Vec<Value> {
        lock(&self.diagnostics)
            .get(uri)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn all_diagnostics(&self) -> HashMap<String, Vec<Value>> {
        lock(&self.diagnostics).clone()
    }

    /// Waits until `settle` passes with no new `publishDiagnostics`, at most `limit` in all.
    pub(crate) async fn settled(&self, settle: Duration, limit: Duration) {
        let started = Instant::now();
        *lock(&self.last_diagnostics) = started;
        loop {
            let quiet = lock(&self.last_diagnostics).elapsed();
            if quiet >= settle || started.elapsed() >= limit || !self.alive() {
                return;
            }
            tokio::time::sleep((settle - quiet).min(Duration::from_millis(50))).await;
        }
    }

    fn on_message(&self, message: Value) {
        match (
            message.get("method").and_then(Value::as_str),
            message.get("id"),
        ) {
            (Some(method), Some(id)) => self.on_server_request(method, id.clone(), &message),
            (Some(method), None) => self.on_notification(method, message.clone()),
            (None, Some(id)) => self.on_response(id.as_i64(), message.clone()),
            (None, None) => {}
        }
    }

    /// `workspace/configuration` is answered with the settings; every other request is answered
    /// with an empty result.
    fn on_server_request(&self, method: &str, id: Value, message: &Value) {
        let result = if method == "workspace/configuration" {
            let items = message["params"]["items"].as_array().map_or(1, Vec::len);
            Value::Array(vec![lock(&self.settings).clone(); items])
        } else {
            Value::Null
        };
        let _ = self.write(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn on_notification(&self, method: &str, message: Value) {
        if method != "textDocument/publishDiagnostics" {
            return;
        }
        if let Some(uri) = message["params"]["uri"].as_str() {
            let list = message["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            lock(&self.diagnostics).insert(uri.to_owned(), list);
            *lock(&self.last_diagnostics) = Instant::now();
        }
        let sessions: Vec<StreamId> = lock(&self.sessions).iter().copied().collect();
        for stream in sessions {
            self.streams
                .send(stream, StreamPayload::Lsp(message.clone()));
        }
    }

    fn on_response(&self, id: Option<i64>, mut message: Value) {
        let waiter = id.and_then(|id| lock(&self.waiters).remove(&id));
        match waiter {
            Some(Waiter::Ordin(sender)) => {
                let _ = sender.send(message);
            }
            Some(Waiter::Editor { stream, client_id }) => {
                message["id"] = client_id;
                self.streams.send(stream, StreamPayload::Lsp(message));
            }
            None => {}
        }
    }

    /// Ends the process and the tasks; a stop that was asked for is not a crash.
    fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.write(json!({"jsonrpc": "2.0", "method": "exit"}));
        self.dead.cancel();
    }
}

/// Every instance and editor session of the service.
#[derive(Default)]
pub(crate) struct LspState {
    pub instances: Mutex<HashMap<InstanceKey, Arc<Instance>>>,
    pub sessions: Mutex<HashMap<StreamId, EditorState>>,
    pub starts: tokio::sync::Mutex<()>,
    /// Answers for a repository with no origin or no commit, kept for this run of ordin only.
    pub unbound_answers: Mutex<HashMap<PlaceId, (SeenRepository, bool)>>,
    next_instance: AtomicU64,
}

/// The origin address and root commit a folder's repository has.
pub(crate) type SeenRepository = (Option<String>, Option<String>);

/// What a language-server answer is bound to.
enum Identity {
    /// The SHA-256 of the origin address and the root commit.
    Known(String),
    /// A repository lacking either: what it has, compared but not stored.
    Partial(SeenRepository),
}

impl Identity {
    fn of(seen: SeenRepository) -> Identity {
        match seen {
            (Some(origin), Some(root)) => {
                let digest: [u8; 32] = Sha256::digest(format!("{origin}\n{root}")).into();
                Identity::Known(Sha256Hex::from_digest(&digest).0)
            }
            partial => Identity::Partial(partial),
        }
    }
}

/// One editor tab.
#[derive(Debug, Clone)]
pub(crate) struct EditorState {
    pub project: ProjectId,
    pub folder: PathBuf,
    pub language: Option<String>,
}

/// Reads one frame; `Ok(None)` at the end of the stream.
pub(crate) async fn read_frame<R>(reader: &mut R) -> Result<Option<Value>, ()>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut length = None;
    let mut seen_any = false;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await.map_err(|_| ())?;
        if read == 0 {
            return if seen_any { Err(()) } else { Ok(None) };
        }
        seen_any = true;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(())?;
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse::<usize>().map_err(|_| ())?);
        }
    }
    let length = length.ok_or(())?;
    if length > MAX_FRAME_BYTES {
        return Err(());
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await.map_err(|_| ())?;
    serde_json::from_slice(&body).map(Some).map_err(|_| ())
}

/// `Content-Length: N\r\n\r\n` + N bytes of UTF-8 JSON.
pub(crate) async fn write_frame<W>(writer: &mut W, message: &Value) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(message)?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    writer.write_all(&body).await?;
    writer.flush().await
}

/// A `file://` URI of an absolute path.
pub(crate) fn uri_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let encoded: Vec<String> = text.split('/').map(crate::hosts::encode).collect();
    let joined = encoded.join("/");
    if joined.starts_with('/') {
        format!("file://{joined}")
    } else {
        format!("file:///{joined}")
    }
}

/// The path of a `file://` URI.
// spec: ordin-desktop/components/ProjectService#PROJ-PATH-1
pub(crate) fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = rest.get(at + 1..at + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    // On Windows `file:///C:/x` names `C:/x`: the slash before the drive is not part of the path.
    #[cfg(windows)]
    let text = match text.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => text[1..].to_owned(),
        _ => text,
    };
    Some(PathBuf::from(text))
}

async fn writer_task(
    mut stdin: ChildStdin,
    mut outgoing: mpsc::UnboundedReceiver<Value>,
    dead: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = dead.cancelled() => return,
            message = outgoing.recv() => match message {
                Some(message) => {
                    if write_frame(&mut stdin, &message).await.is_err() {
                        return;
                    }
                }
                None => return,
            },
        }
    }
}

/// A malformed frame from a server, or its end, kills that instance; the next use restarts it.
async fn reader_task(
    service: Weak<ProjectService>,
    instance: Arc<Instance>,
    mut child: Child,
    stdout: tokio::process::ChildStdout,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        tokio::select! {
            _ = instance.dead.cancelled() => break,
            frame = read_frame(&mut reader) => match frame {
                Ok(Some(message)) => instance.on_message(message),
                Ok(None) | Err(()) => break,
            },
        }
    }
    instance.dead.cancel();
    let _ = child.kill().await;
    let language = instance.key.language();
    let event = if instance.stopping.load(Ordering::SeqCst) {
        LanguageServerEvent::Stopped
    } else {
        LanguageServerEvent::Crashed
    };
    crate::log::language_server(event, language, &instance.server, instance.number);
    if let Some(service) = service.upgrade() {
        let mut instances = lock(&service.lsp.instances);
        if instances
            .get(&instance.key)
            .is_some_and(|current| current.number == instance.number)
        {
            instances.remove(&instance.key);
        }
    }
}

impl ProjectService {
    /// Internal Logic 9.3: reuses the live instance of `key`, or starts one.
    pub(crate) async fn instance(
        &self,
        key: InstanceKey,
        root: &Path,
        language: &str,
    ) -> Result<Arc<Instance>, ProjectError> {
        // The answer is read under the start lock, so that a no taking the same lock either stops
        // what this starts or is seen here.
        let _start = self.lsp.starts.lock().await;
        if let Some(found) = self.live(&key) {
            return Ok(found);
        }
        self.require_language_server_answer(&key).await?;
        self.start_locked(key, root, language).await
    }

    fn live(&self, key: &InstanceKey) -> Option<Arc<Instance>> {
        lock(&self.lsp.instances)
            .get(key)
            .filter(|found| found.alive())
            .cloned()
    }

    /// `instance` without the person's answer, for tests of what happens after it.
    #[cfg(test)]
    pub(crate) async fn start_instance(
        &self,
        key: InstanceKey,
        root: &Path,
        language: &str,
    ) -> Result<Arc<Instance>, ProjectError> {
        let _start = self.lsp.starts.lock().await;
        if let Some(found) = self.live(&key) {
            return Ok(found);
        }
        self.start_locked(key, root, language).await
    }

    async fn start_locked(
        &self,
        key: InstanceKey,
        root: &Path,
        language: &str,
    ) -> Result<Arc<Instance>, ProjectError> {
        let mut plugin = store::plugin(&self.pool, language)
            .await?
            .filter(|p| p.enabled && p.status != LanguagePluginStatus::Removed)
            .ok_or(ProjectError::LspUnavailable)?;
        let pin = pin_of(&self.current_settings(), &plugin).ok_or(ProjectError::LspUnavailable)?;
        let launch = self
            .installer
            .ensure(language, &pin)
            .await
            .map_err(|_| ProjectError::ServerInstallFailed)?;
        if plugin.status == LanguagePluginStatus::Added {
            plugin.status = LanguagePluginStatus::Installed;
            store::update_plugin(&self.pool, &plugin).await?;
        }
        self.make_room().await?;
        let mut command = Command::new(&launch.program);
        command
            .args(&launch.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        crate::checks::set_allowed_environment(&mut command);
        command.envs(launch.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        let mut child = command.spawn().map_err(|_| ProjectError::LspUnavailable)?;
        let stdin = child.stdin.take().ok_or(ProjectError::LspUnavailable)?;
        let stdout = child.stdout.take().ok_or(ProjectError::LspUnavailable)?;
        let (sender, receiver) = mpsc::unbounded_channel();
        let instance = Arc::new(Instance {
            key: key.clone(),
            number: self.lsp.next_instance.fetch_add(1, Ordering::SeqCst),
            server: server_of(&plugin).to_owned(),
            root: root.to_path_buf(),
            outgoing: sender,
            next_id: AtomicI64::new(1),
            waiters: Mutex::new(HashMap::new()),
            diagnostics: Mutex::new(HashMap::new()),
            last_diagnostics: Mutex::new(Instant::now()),
            settings: Mutex::new(plugin.server_settings.clone()),
            sessions: Mutex::new(HashSet::new()),
            opened: Mutex::new(HashSet::new()),
            last_used: Mutex::new(Instant::now()),
            stopping: AtomicBool::new(false),
            dead: CancellationToken::new(),
            streams: self.streams.clone(),
        });
        tokio::spawn(writer_task(stdin, receiver, instance.dead.clone()));
        tokio::spawn(reader_task(
            self.me.clone(),
            instance.clone(),
            child,
            stdout,
        ));
        let limit = lock(&self.tunables).lsp_start_timeout;
        let initialised = instance
            .request(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "rootUri": uri_of(root),
                    "capabilities": {},
                    "initializationOptions": plugin.server_settings,
                }),
                limit,
            )
            .await
            .and_then(|_| instance.notify("initialized", json!({})));
        if let Err(error) = initialised {
            instance.stop();
            return Err(error);
        }
        crate::log::language_server(
            LanguageServerEvent::Started,
            language,
            &instance.server,
            instance.number,
        );
        lock(&self.lsp.instances).insert(key.clone(), instance.clone());
        if let InstanceKey::Editor(project, _) = key {
            self.note_servers_in_use(project).await?;
        }
        Ok(instance)
    }

    /// A language server runs the repository's own code, so none starts before the person's yes
    /// (SA-RECON-11). An agent cannot ask: it finds code intelligence unavailable.
    // spec: ordin-desktop/components/ProjectService#SA-RECON-11
    async fn require_language_server_answer(&self, key: &InstanceKey) -> Result<(), ProjectError> {
        let project = match key {
            InstanceKey::Editor(project, _) => *project,
            InstanceKey::Task(task, _) | InstanceKey::Base(task, _) => {
                self.task_row(*task).await?.task.project_id
            }
        };
        match self.language_servers_answer(project).await? {
            Some(true) => Ok(()),
            _ => Err(ProjectError::LspUnavailable),
        }
    }

    /// The place and the identity of the repository now in its folder.
    async fn repository_at(&self, project: ProjectId) -> Result<(PlaceId, Identity), ProjectError> {
        let place = self.project(project).await?.place_id;
        let folder = self.place_folder(place).await?;
        let identity = git::blocking(move || Ok(git::repository_identity(&folder))).await?;
        Ok((place, Identity::of(identity)))
    }

    /// The person's answer for the repository in this project's folder; None until they have given
    /// one for that repository. An answer given for another repository at the path does not count.
    pub async fn language_servers_answer(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<bool>, ProjectError> {
        let (place, identity) = self.repository_at(project_id).await?;
        match identity {
            Identity::Known(repository) => {
                store::language_server_answer(&self.pool, place, &repository).await
            }
            Identity::Partial(seen) => Ok(lock(&self.lsp.unbound_answers)
                .get(&place)
                .filter(|(given_for, _)| *given_for == seen)
                .map(|(_, allowed)| *allowed)),
        }
    }

    /// Records the answer. A no stops what is running on the repository, its tasks' included, and
    /// waits for a start in progress so that it stops that too. A repository with no origin or no
    /// commit has nothing to bind an answer to, so its answer lasts until ordin next starts.
    // spec: ordin-desktop/components/ProjectService#SA-RECON-11
    pub async fn answer_language_servers(
        &self,
        project_id: ProjectId,
        allowed: bool,
    ) -> Result<(), ProjectError> {
        self.open_row(project_id).await?;
        let (place, identity) = self.repository_at(project_id).await?;
        let _start = self.lsp.starts.lock().await;
        match identity {
            Identity::Known(repository) => {
                store::set_language_server_answer(
                    &self.pool,
                    place,
                    allowed,
                    &repository,
                    Timestamp::now(),
                )
                .await?;
            }
            Identity::Partial(seen) => {
                lock(&self.lsp.unbound_answers).insert(place, (seen, allowed));
            }
        }
        if !allowed {
            self.stop_project_instances(project_id).await;
            let running = self.keys_where(|key| !matches!(key, InstanceKey::Editor(..)));
            for (key, _) in running {
                if let InstanceKey::Task(task, _) | InstanceKey::Base(task, _) = key {
                    // A server whose task row is gone belongs to no project that can answer for it.
                    let other = self
                        .task_row(task)
                        .await
                        .is_ok_and(|row| row.task.project_id != project_id);
                    if !other {
                        self.stop_instance(&key).await;
                    }
                }
            }
        }
        Ok(())
    }

    /// At most `MAX_LANGUAGE_INSTANCES`: the least recently used editor instance is stopped first.
    async fn make_room(&self) -> Result<(), ProjectError> {
        let oldest = {
            let instances = lock(&self.lsp.instances);
            if instances.len() < crate::MAX_LANGUAGE_INSTANCES {
                return Ok(());
            }
            instances
                .values()
                .filter(|i| matches!(i.key, InstanceKey::Editor(..)))
                .min_by_key(|i| *lock(&i.last_used))
                .map(|i| i.key.clone())
        };
        match oldest {
            Some(key) => {
                self.stop_instance(&key).await;
                Ok(())
            }
            None => Err(ProjectError::LspUnavailable),
        }
    }

    pub(crate) async fn stop_instance(&self, key: &InstanceKey) {
        let removed = lock(&self.lsp.instances).remove(key);
        if let Some(instance) = removed {
            instance.stop();
            if let InstanceKey::Editor(project, _) = key {
                if let Err(error) = self.note_servers_in_use(*project).await {
                    crate::log::background_failed(
                        "servers_in_use",
                        ordin_types::CodedError::code(&error),
                    );
                }
            }
        }
    }

    /// Keeps `projects.language_servers_in_use` equal to the project's running editor instances.
    async fn note_servers_in_use(&self, project: ProjectId) -> Result<(), ProjectError> {
        let mut servers: Vec<String> = lock(&self.lsp.instances)
            .values()
            .filter(|i| matches!(&i.key, InstanceKey::Editor(p, _) if *p == project))
            .map(|i| i.server.clone())
            .collect();
        servers.sort();
        store::set_servers_in_use(&self.pool, project, &servers).await
    }

    fn keys_where(&self, wanted: impl Fn(&InstanceKey) -> bool) -> Vec<(InstanceKey, PathBuf)> {
        lock(&self.lsp.instances)
            .iter()
            .filter(|(key, _)| wanted(key))
            .map(|(key, instance)| (key.clone(), instance.root.clone()))
            .collect()
    }

    /// Stops every instance of a language; returns what was running.
    pub(crate) async fn stop_language_instances(
        &self,
        language: &str,
    ) -> Vec<(InstanceKey, PathBuf)> {
        let keys = self.keys_where(|key| key.language() == language);
        for (key, _) in &keys {
            self.stop_instance(key).await;
        }
        keys
    }

    pub(crate) async fn stop_project_instances(&self, project: ProjectId) {
        let keys = self.keys_where(|key| matches!(key, InstanceKey::Editor(p, _) if *p == project));
        for (key, _) in keys {
            self.stop_instance(&key).await;
        }
    }

    pub(crate) async fn stop_task_instances(&self, task: CodingTaskId) {
        let keys = self.keys_where(
            |key| matches!(key, InstanceKey::Task(t, _) | InstanceKey::Base(t, _) if *t == task),
        );
        for (key, _) in keys {
            self.stop_instance(&key).await;
        }
    }

    pub(crate) async fn stop_all_instances(&self) {
        for (key, _) in self.keys_where(|_| true) {
            self.stop_instance(&key).await;
        }
    }

    /// Running instances of the language get the new settings (Internal Logic 9.2).
    pub(crate) fn configure_running(&self, language: &str, settings: &Value) {
        let running: Vec<Arc<Instance>> = lock(&self.lsp.instances)
            .values()
            .filter(|i| i.key.language() == language)
            .cloned()
            .collect();
        for instance in running {
            instance.set_settings(settings);
            let _ = instance.notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": settings }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_frame_survives_the_trip() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &json!({"a": 1})).await.unwrap();
        assert!(bytes.starts_with(b"Content-Length: 7\r\n\r\n"));
        let mut reader = BufReader::new(bytes.as_slice());
        assert_eq!(
            read_frame(&mut reader).await.unwrap(),
            Some(json!({"a": 1}))
        );
        assert_eq!(read_frame(&mut reader).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_malformed_frame_is_refused() {
        for bytes in [
            &b"no colon here\r\n\r\n"[..],
            b"Content-Length: x\r\n\r\n",
            b"Other: 1\r\n\r\n",
            b"Content-Length: 99999999999\r\n\r\n",
            b"Content-Length: 3\r\n\r\nnot",
            b"Content-Length: 30\r\n\r\nshort",
            b"Content-Length: 3\r\n",
        ] {
            let mut reader = BufReader::new(bytes);
            assert!(read_frame(&mut reader).await.is_err());
        }
    }

    #[test]
    fn uris_and_paths_are_inverses() {
        let uri = uri_of(Path::new("/tmp/a b/c.rs"));
        assert_eq!(uri, "file:///tmp/a%20b/c.rs");
        assert_eq!(path_of(&uri).unwrap(), Path::new("/tmp/a b/c.rs"));
        assert!(path_of("http://x").is_none());
        assert!(path_of("file:///a%zz").is_none());
    }
}
