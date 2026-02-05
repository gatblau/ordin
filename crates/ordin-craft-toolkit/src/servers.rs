//! The per-session app servers (CraftToolkit.md *Internal Logic* 5): each is a child process serving
//! its app's MCP mode, reached with an rmcp client over its stdio (SA-MCP-01).

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rmcp::model::Tool;
use rmcp::service::{Peer, RunningService};
use rmcp::{RoleClient, ServiceExt};
use tokio::process::{Child, Command};

use crate::error::CraftError;
use crate::toolkit::{lock, CraftToolkit};

/// How long a closing app server has to end after its stdin closes (Internal Logic 5.4).
const STOP_WAIT: Duration = Duration::from_secs(2);

/// The variables an app keeps from ordin's environment: no tokens or credentials reach it.
pub(crate) const KEPT_ENV: [&str; 6] = ["PATH", "HOME", "USERPROFILE", "TMPDIR", "TEMP", "LANG"];

/// Who a server belongs to: a session, or the one shared conversion session (`run_id` absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Owner {
    Run(ordin_types::RunId),
    Conversion,
}

impl Owner {
    fn run_text(self) -> String {
        match self {
            Owner::Run(run_id) => run_id.to_string(),
            Owner::Conversion => String::new(),
        }
    }
}

type Key = (Owner, String);

struct Live {
    service: RunningService<RoleClient, ()>,
    child: Child,
    tools: Arc<Vec<Tool>>,
}

struct Slot {
    live: tokio::sync::Mutex<Option<Live>>,
    last_used: Mutex<Instant>,
    in_flight: Arc<AtomicUsize>,
}

impl Slot {
    fn new() -> Slot {
        Slot {
            live: tokio::sync::Mutex::new(None),
            last_used: Mutex::new(Instant::now()),
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn idle_for(&self) -> Option<Duration> {
        (self.in_flight.load(Ordering::SeqCst) == 0).then(|| lock(&self.last_used).elapsed())
    }
}

#[derive(Default)]
pub(crate) struct ServerPool {
    slots: Mutex<HashMap<Key, Arc<Slot>>>,
}

/// A use of a slot. Taken under the pool's lock, so that a slot being started or called is never
/// stopped as idle.
struct Claim(Arc<AtomicUsize>);

impl Claim {
    fn of(slot: &Slot) -> Claim {
        slot.in_flight.fetch_add(1, Ordering::SeqCst);
        Claim(Arc::clone(&slot.in_flight))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A started server in use. While it is held the server counts as busy.
pub(crate) struct ServerHandle {
    pub(crate) peer: Peer<RoleClient>,
    pub(crate) tools: Arc<Vec<Tool>>,
    _claim: Claim,
}

impl ServerHandle {
    pub(crate) fn lists(&self, tool: &str) -> bool {
        self.tools.iter().any(|t| t.name == tool)
    }
}

impl CraftToolkit {
    #[cfg(test)]
    pub(crate) fn servers_running(&self) -> usize {
        lock(&self.servers.slots).len()
    }

    /// The running server of `owner` for `app`, started when there is none (Internal Logic 5.2).
    pub(crate) async fn server(&self, owner: Owner, app: &str) -> Result<ServerHandle, CraftError> {
        let key = (owner, app.to_owned());
        let (slot, claim) = self.slot_for(&key).await?;
        let mut live = slot.live.lock().await;
        if live.is_none() {
            match self.start_server(owner, app).await {
                Ok(started) => *live = Some(started),
                Err(error) => {
                    lock(&self.servers.slots).remove(&key);
                    return Err(error);
                }
            }
        }
        let Some(running) = live.as_ref() else {
            return Err(CraftError::ServerStartFailed);
        };
        *lock(&slot.last_used) = Instant::now();
        Ok(ServerHandle {
            peer: running.service.peer().clone(),
            tools: Arc::clone(&running.tools),
            _claim: claim,
        })
    }

    /// The slot of `key`, claimed. At `MAX_SERVERS`, the least recently used idle server is stopped
    /// first.
    async fn slot_for(&self, key: &Key) -> Result<(Arc<Slot>, Claim), CraftError> {
        let (slot, claim, evicted) = {
            let mut slots = lock(&self.servers.slots);
            if let Some(slot) = slots.get(key) {
                let claim = Claim::of(slot);
                return Ok((Arc::clone(slot), claim));
            }
            let evicted = if slots.len() >= self.limits.max_servers {
                let oldest = slots
                    .iter()
                    .filter_map(|(k, s)| s.idle_for().map(|idle| (idle, k.clone())))
                    .max_by_key(|(idle, _)| *idle)
                    .map(|(_, k)| k);
                let Some(oldest) = oldest else {
                    return Err(CraftError::ServerStartFailed);
                };
                slots.remove(&oldest).map(|slot| (oldest, slot))
            } else {
                None
            };
            let slot = Arc::new(Slot::new());
            let claim = Claim::of(&slot);
            slots.insert(key.clone(), Arc::clone(&slot));
            (slot, claim, evicted)
        };
        if let Some((old_key, old)) = evicted {
            self.stop_slot(&old_key, &old, "capacity", true).await;
        }
        Ok((slot, claim))
    }

    /// Spawns `<cli> mcp`, connects and asks for its tools, all within `START_TIMEOUT`.
    async fn start_server(&self, owner: Owner, app: &str) -> Result<Live, CraftError> {
        let row = self
            .row(app)
            .filter(|row| row.is_installed())
            .ok_or(CraftError::NotInstalled)?;
        if !row.has_mcp() {
            return Err(CraftError::NoMcpMode);
        }
        let version = row
            .installed_version
            .as_deref()
            .ok_or(CraftError::NotInstalled)?;
        let folder = self.owner_folder(owner);
        tokio::fs::create_dir_all(&folder)
            .await
            .map_err(|_| CraftError::ServerStartFailed)?;
        let cli = self
            .cli_path(app, version)
            .ok_or(CraftError::ServerStartFailed)?;
        let mut child = spawn_app(&cli, &folder)?;
        let started = tokio::time::timeout(self.limits.start, connect(&mut child)).await;
        let (service, tools) = match started {
            Ok(Ok(connected)) => connected,
            _ => {
                let _ = child.kill().await;
                return Err(CraftError::ServerStartFailed);
            }
        };
        tracing::info!(
            component = "craft",
            app,
            run_id = owner.run_text().to_string().as_str(),
            "craft_server_started"
        );
        self.learn_schemas(app, &tools);
        Ok(Live {
            service,
            child,
            tools: Arc::new(tools),
        })
    }

    /// `input_schema` is the app's (5.3). Sessions opened before an app's list is known got a bare
    /// object schema, so the first list is announced.
    fn learn_schemas(&self, app: &str, tools: &[Tool]) {
        let mut state = lock(&self.state);
        let mut first = false;
        for tool in tools {
            let key = (app.to_owned(), tool.name.to_string());
            let schema = serde_json::Value::Object((*tool.input_schema).clone());
            first |= state.schemas.insert(key, schema).is_none();
        }
        drop(state);
        if first {
            if let Ok(name) = ordin_mcp_front::ServerName::new(format!("craft_{app}")) {
                self.registry.tools_changed(&name);
            }
        }
    }

    /// Closes stdin, waits `STOP_WAIT`, then kills; `graceful` false kills at once.
    async fn stop_slot(&self, key: &Key, slot: &Slot, reason: &str, graceful: bool) {
        let taken = slot.live.lock().await.take();
        let Some(mut live) = taken else {
            return;
        };
        if !graceful {
            let _ = live.child.start_kill();
        }
        let _ = live.service.cancel().await;
        if tokio::time::timeout(STOP_WAIT, live.child.wait())
            .await
            .is_err()
        {
            let _ = live.child.kill().await;
        }
        let (owner, app) = key;
        tracing::info!(
            component = "craft",
            app = app.to_string().as_str(),
            run_id = owner.run_text().to_string().as_str(),
            reason,
            "craft_server_stopped"
        );
    }

    /// Stops the session's server at once; it is restarted on the next call (6.3).
    pub(crate) async fn kill_server(&self, owner: Owner, app: &str, reason: &str) {
        let key = (owner, app.to_owned());
        let slot = lock(&self.servers.slots).remove(&key);
        if let Some(slot) = slot {
            self.stop_slot(&key, &slot, reason, false).await;
        }
    }

    /// Every server of `app`, for a removed app or a replaced version.
    pub(crate) async fn stop_app_servers(&self, app: &str) {
        self.stop_matching("app_changed", |(_, a)| a == app).await;
    }

    /// `session_closed`: stops the session's servers and deletes `SCRATCH_ROOT/<run_id>/`.
    pub(crate) async fn close_session(&self, run_id: ordin_types::RunId) {
        self.stop_matching("session_closed", |(owner, _)| *owner == Owner::Run(run_id))
            .await;
        let _ = tokio::fs::remove_dir_all(self.owner_folder(Owner::Run(run_id))).await;
    }

    pub(crate) async fn stop_all_servers(&self) {
        self.stop_matching("shutdown", |_| true).await;
    }

    async fn stop_matching(&self, reason: &str, matches: impl Fn(&Key) -> bool) {
        let taken: Vec<(Key, Arc<Slot>)> = {
            let mut slots = lock(&self.servers.slots);
            let keys: Vec<Key> = slots.keys().filter(|k| matches(k)).cloned().collect();
            keys.into_iter()
                .filter_map(|k| slots.remove(&k).map(|s| (k, s)))
                .collect()
        };
        for (key, slot) in taken {
            self.stop_slot(&key, &slot, reason, true).await;
        }
    }

    /// `SESSION_IDLE_STOP`: stops idle servers. A session's scratch folder goes with its last
    /// server, because another app of the session may still be working there.
    pub(crate) async fn stop_idle(&self) {
        let idle = self.limits.idle;
        let taken: Vec<(Key, Arc<Slot>)> = {
            let mut slots = lock(&self.servers.slots);
            let keys: Vec<Key> = slots
                .iter()
                .filter(|(_, s)| s.idle_for().is_some_and(|d| d >= idle))
                .map(|(k, _)| k.clone())
                .collect();
            keys.into_iter()
                .filter_map(|k| slots.remove(&k).map(|s| (k, s)))
                .collect()
        };
        for (key, slot) in taken {
            self.stop_slot(&key, &slot, "idle", true).await;
            let owner = key.0;
            let others = lock(&self.servers.slots).keys().any(|(o, _)| *o == owner);
            if !others && owner != Owner::Conversion {
                let _ = tokio::fs::remove_dir_all(self.owner_folder(owner)).await;
            }
        }
    }
}

// spec: ordin-desktop/components/CraftToolkit#SA-RECON-01
fn spawn_app(cli: &Path, folder: &Path) -> Result<Child, CraftError> {
    Command::new(cli)
        .arg("mcp")
        .env_clear()
        .envs(
            KEPT_ENV
                .iter()
                .filter_map(|name| std::env::var_os(name).map(|value| (name.to_string(), value))),
        )
        .current_dir(folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| CraftError::ServerStartFailed)
}

async fn connect(child: &mut Child) -> Result<(RunningService<RoleClient, ()>, Vec<Tool>), ()> {
    let (Some(stdout), Some(stdin)) = (child.stdout.take(), child.stdin.take()) else {
        return Err(());
    };
    let service = ().serve((stdout, stdin)).await.map_err(|_| ())?;
    let tools = service.peer().list_all_tools().await.map_err(|_| ())?;
    Ok((service, tools))
}
