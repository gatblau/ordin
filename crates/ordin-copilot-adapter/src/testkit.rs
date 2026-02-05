//! The scripted `CopilotPort` double (CopilotLocalAdapter.md *Approach*: "one scripted test double,
//! so every rule below is tested without the CLI"). Built for this crate's tests and, through
//! feature `testkit`, for other crates' tests; never by a normal dependency.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::stream::{BoxStream, StreamExt};
use tokio::sync::mpsc;

use crate::port::{
    CliServerSpec, CopilotAuthStatus, CopilotEvent, CopilotPort, CopilotServer, HostTool,
    HostToolAnswer, HostToolHandler, PortError, SessionOptions, ToolGate, ToolObserver,
};

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The gates and observer a session was given.
#[derive(Clone)]
pub struct SessionGates {
    pub pre_tool_use: Arc<dyn ToolGate>,
    pub permission: Arc<dyn ToolGate>,
    pub post_tool_use: Arc<dyn ToolObserver>,
    /// The tools the session declared (CPA-HT-1).
    pub host_tools: Vec<HostTool>,
    pub host_tool_handler: Arc<dyn HostToolHandler>,
}

/// What the double was asked, and what it answers. Every field is shared by all servers it starts.
#[derive(Default)]
pub struct Script {
    /// `start_server` fails.
    pub start_fails: AtomicBool,
    /// `auth_status` fails.
    pub auth_fails: AtomicBool,
    /// `auth_status` reports signed in.
    pub signed_in: AtomicBool,
    /// After `start_login`, `auth_status` reports signed in once this much time has passed.
    pub login_signs_in_after: Mutex<Option<Duration>>,
    /// The sign-in method `auth_status` reports.
    pub method: Mutex<Option<String>>,
    /// `create_session` fails.
    pub create_fails: AtomicBool,
    /// `resume_session` fails.
    pub resume_fails: AtomicBool,
    /// `send` fails.
    pub send_fails: AtomicBool,
    /// The next `send` fails as for a session the CLI does not hold.
    pub send_session_lost: AtomicBool,
    /// `abort` fails.
    pub abort_fails: AtomicBool,
    /// `stop` takes this long before it returns.
    pub stop_takes: Mutex<Duration>,

    pub started: Mutex<Vec<CliServerSpec>>,
    pub auth_calls: AtomicUsize,
    pub logins: AtomicUsize,
    login_at: Mutex<Option<Instant>>,
    pub created: Mutex<Vec<(Option<String>, serde_json::Value)>>,
    pub resumed: Mutex<Vec<String>>,
    pub sent: Mutex<Vec<(String, String)>>,
    pub aborted: AtomicUsize,
    pub stopped: AtomicUsize,
    gates: Mutex<HashMap<String, SessionGates>>,
    streams: Mutex<HashMap<String, mpsc::UnboundedSender<CopilotEvent>>>,
    pending: Mutex<HashMap<String, mpsc::UnboundedReceiver<CopilotEvent>>>,
    next_session: AtomicUsize,
}

impl Script {
    /// The gates of a session, once created or resumed.
    pub fn gates(&self, session_id: &str) -> Option<SessionGates> {
        lock(&self.gates).get(session_id).cloned()
    }

    /// Sends one event to a session's stream; `false` when the session has no stream.
    pub fn emit(&self, session_id: &str, event: CopilotEvent) -> bool {
        lock(&self.streams)
            .get(session_id)
            .is_some_and(|tx| tx.send(event).is_ok())
    }

    /// The tools a session declared, by name, once created or resumed.
    pub fn declared(&self, session_id: &str) -> Vec<String> {
        self.gates(session_id)
            .map(|g| g.host_tools.iter().map(|t| t.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Sends one declared-tool call as the CLI would and returns the answer; a session the double
    /// does not hold is answered as `CliCopilotPort` answers it.
    pub async fn call_tool(
        &self,
        session_id: &str,
        name: &str,
        arguments: serde_json::Value,
    ) -> HostToolAnswer {
        match self.gates(session_id) {
            Some(gates) => {
                gates
                    .host_tool_handler
                    .call(name, arguments, tokio_util::sync::CancellationToken::new())
                    .await
            }
            None => HostToolAnswer::Failure {
                text: "This tool is not offered to this session.".to_owned(),
            },
        }
    }

    pub fn sent_texts(&self) -> Vec<String> {
        lock(&self.sent).iter().map(|(_, t)| t.clone()).collect()
    }

    fn channel(&self, session_id: &str) {
        let (tx, rx) = mpsc::unbounded_channel();
        lock(&self.streams).insert(session_id.to_owned(), tx);
        lock(&self.pending).insert(session_id.to_owned(), rx);
    }

    fn keep(&self, session_id: &str, opts: &SessionOptions) {
        lock(&self.gates).insert(
            session_id.to_owned(),
            SessionGates {
                pre_tool_use: opts.pre_tool_use.clone(),
                permission: opts.permission.clone(),
                post_tool_use: opts.post_tool_use.clone(),
                host_tools: opts.host_tools.clone(),
                host_tool_handler: opts.host_tool_handler.clone(),
            },
        );
    }
}

/// The double. Clones share one [`Script`].
#[derive(Clone, Default)]
pub struct ScriptedCopilotPort {
    pub script: Arc<Script>,
}

impl ScriptedCopilotPort {
    /// A double whose CLI is signed in.
    pub fn signed_in() -> ScriptedCopilotPort {
        let port = ScriptedCopilotPort::default();
        port.script.signed_in.store(true, Ordering::SeqCst);
        port
    }
}

#[async_trait::async_trait]
impl CopilotPort for ScriptedCopilotPort {
    async fn start_server(&self, spec: CliServerSpec) -> Result<Box<dyn CopilotServer>, PortError> {
        lock(&self.script.started).push(spec);
        if self.script.start_fails.load(Ordering::SeqCst) {
            return Err(PortError::new("scripted: the server does not start"));
        }
        Ok(Box::new(ScriptedServer {
            script: self.script.clone(),
        }))
    }
}

struct ScriptedServer {
    script: Arc<Script>,
}

#[async_trait::async_trait]
impl CopilotServer for ScriptedServer {
    async fn auth_status(&self) -> Result<CopilotAuthStatus, PortError> {
        let script = &self.script;
        script.auth_calls.fetch_add(1, Ordering::SeqCst);
        if script.auth_fails.load(Ordering::SeqCst) {
            return Err(PortError::new("scripted: status unreadable"));
        }
        let after_login = match (*lock(&script.login_at), *lock(&script.login_signs_in_after)) {
            (Some(at), Some(after)) => at.elapsed() >= after,
            _ => false,
        };
        Ok(CopilotAuthStatus {
            signed_in: script.signed_in.load(Ordering::SeqCst) || after_login,
            method: lock(&script.method).clone(),
        })
    }

    async fn start_login(&self) -> Result<(), PortError> {
        self.script.logins.fetch_add(1, Ordering::SeqCst);
        lock(&self.script.login_at).get_or_insert_with(Instant::now);
        Ok(())
    }

    async fn create_session(&self, opts: SessionOptions) -> Result<String, PortError> {
        let script = &self.script;
        lock(&script.created).push((opts.model.clone(), opts.mcp_servers.clone()));
        if script.create_fails.load(Ordering::SeqCst) {
            return Err(PortError::new("scripted: no session"));
        }
        let id = format!(
            "sess_{}",
            script.next_session.fetch_add(1, Ordering::SeqCst) + 1
        );
        script.keep(&id, &opts);
        script.channel(&id);
        Ok(id)
    }

    async fn resume_session(
        &self,
        session_id: &str,
        opts: SessionOptions,
    ) -> Result<(), PortError> {
        let script = &self.script;
        lock(&script.resumed).push(session_id.to_owned());
        if script.resume_fails.load(Ordering::SeqCst) {
            return Err(PortError::new("scripted: not resumable"));
        }
        script.keep(session_id, &opts);
        script.channel(session_id);
        Ok(())
    }

    async fn send(&self, session_id: &str, text: &str) -> Result<(), PortError> {
        lock(&self.script.sent).push((session_id.to_owned(), text.to_owned()));
        if self.script.send_fails.load(Ordering::SeqCst) {
            return Err(PortError::new("scripted: not sent"));
        }
        if self.script.send_session_lost.swap(false, Ordering::SeqCst) {
            return Err(PortError::session_not_found("scripted: session not found"));
        }
        Ok(())
    }

    fn events(&self, session_id: &str) -> BoxStream<'static, CopilotEvent> {
        match lock(&self.script.pending).remove(session_id) {
            Some(rx) => {
                futures::stream::unfold(
                    rx,
                    |mut rx| async move { rx.recv().await.map(|e| (e, rx)) },
                )
                .boxed()
            }
            None => futures::stream::pending().boxed(),
        }
    }

    async fn abort(&self, _session_id: &str) -> Result<(), PortError> {
        self.script.aborted.fetch_add(1, Ordering::SeqCst);
        if self.script.abort_fails.load(Ordering::SeqCst) {
            return Err(PortError::new("abort failed"));
        }
        Ok(())
    }

    async fn stop(&self, deadline: Duration) {
        let takes = *lock(&self.script.stop_takes);
        tokio::time::sleep(takes.min(deadline)).await;
        self.script.stopped.fetch_add(1, Ordering::SeqCst);
    }
}
