//! The hub: registry, sessions and start-up (McpFront.md Internal Logic A to C2).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ordin_journal::Journal;
use ordin_rules::RulesEngine;
use ordin_service_api::blocks::{BlockCatalogueCheck, RefResolver};
use ordin_types::domain::identity::HarnessKind;
use ordin_types::RunId;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::error::McpError;
use crate::names::{is_server_prefix, is_tool_name, ServerName};
use crate::provider::{
    ServerSpec, ShownBlockStore, ToolDispatch, ToolFailure, ToolKey, ToolProvider, ToolRegistry,
    ToolSpec,
};
use crate::types::{
    McpFrontConfig, McpMetrics, McpServerLaunch, OfferedTool, ProgramProfile, SessionGrant,
    SessionMcpConfig, ToolCallResult,
};
use crate::util::lock;
use crate::work::{WorkProvider, WorkShared, WORK_PROVIDER_ID, WORK_SERVER};
use crate::{bridge, call};

/// How long a provider's `offer` may take (Internal Logic 5.3).
pub(crate) const OFFER_TIMEOUT: Duration = Duration::from_secs(2);
/// How long `close_session` waits for the providers' `session_closed` (Internal Logic 6).
const CLOSE_WAIT: Duration = Duration::from_secs(5);

/// A registered provider and what it claimed.
#[derive(Clone)]
pub(crate) struct ProviderSlot {
    pub(crate) provider: Arc<dyn ToolProvider>,
    pub(crate) id: &'static str,
    pub(crate) claimed: Vec<ServerName>,
    pub(crate) prefix: Option<&'static str>,
    /// Set for the built-in `work` provider, whose calls carry structured failures.
    pub(crate) work: Option<Arc<WorkShared>>,
}

#[derive(Default)]
pub(crate) struct Registry {
    pub(crate) slots: Vec<ProviderSlot>,
    pub(crate) started: bool,
}

/// One bridge connection of a session.
pub(crate) struct Conn {
    pub(crate) id: u64,
    pub(crate) server: ServerName,
    pub(crate) tx: UnboundedSender<String>,
}

/// The state of one open session (Internal Logic 5.4).
pub(crate) struct Session {
    pub(crate) grant: SessionGrant,
    pub(crate) token_digest: [u8; 32],
    /// Servers with at least one offered tool when the session opened.
    pub(crate) launched: BTreeSet<ServerName>,
    /// `ToolKey` to (provider index, spec).
    pub(crate) offered: Mutex<BTreeMap<ToolKey, (usize, ToolSpec)>>,
    pub(crate) cancel: CancellationToken,
    pub(crate) semaphore: Arc<Semaphore>,
    pub(crate) conns: Mutex<Vec<Conn>>,
    pub(crate) next_conn: AtomicU64,
    pub(crate) calls: AtomicU64,
    pub(crate) denied: AtomicU64,
    pub(crate) failed: AtomicU64,
}

impl Session {
    /// Sends one line on every bridge connection of the session that serves `server`.
    pub(crate) fn notify_server(&self, server: &ServerName, line: &str) {
        for conn in lock(&self.conns).iter().filter(|c| &c.server == server) {
            let _ = conn.tx.send(line.to_owned());
        }
    }
}

pub(crate) struct Inner {
    pub(crate) config: McpFrontConfig,
    pub(crate) rules: Arc<RulesEngine>,
    pub(crate) journal: Journal,
    pub(crate) metrics: Option<Arc<dyn McpMetrics>>,
    pub(crate) work: Arc<WorkShared>,
    pub(crate) registry: Mutex<Registry>,
    pub(crate) start_gate: tokio::sync::Mutex<()>,
    pub(crate) shutdown: OnceLock<CancellationToken>,
    pub(crate) endpoint: OnceLock<String>,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) own_uid: OnceLock<u32>,
    pub(crate) sessions: Mutex<HashMap<RunId, Arc<Session>>>,
    /// Servers with a `tools_changed` task running; the flag asks for one more pass.
    pub(crate) changes: Mutex<HashMap<ServerName, bool>>,
}

/// The hub. One per program.
#[derive(Clone)]
pub struct McpFront {
    pub(crate) inner: Arc<Inner>,
}

impl Inner {
    pub(crate) fn providers(&self) -> Vec<ProviderSlot> {
        lock(&self.registry).slots.clone()
    }

    pub(crate) fn session(&self, run_id: RunId) -> Option<Arc<Session>> {
        lock(&self.sessions).get(&run_id).cloned()
    }

    pub(crate) fn is_started(&self) -> bool {
        lock(&self.registry).started
    }
}

/// Why one provider's offer could not be used.
pub(crate) struct OfferFailure {
    pub(crate) code: String,
    pub(crate) retryable: bool,
}

/// Checks one `ToolSpec` against the rules of Internal Logic 2.3; the name of the first broken
/// rule is returned.
// spec: ordin-desktop/components/McpFront#SA-RECON-04
pub(crate) fn check_tool_spec(
    profile: ProgramProfile,
    spec: &ToolSpec,
    seen: &BTreeSet<String>,
) -> Result<(), &'static str> {
    if !is_tool_name(&spec.name) {
        return Err("name");
    }
    if seen.contains(&spec.name) {
        return Err("name_unique");
    }
    let max_description = if profile == ProgramProfile::AgentProfile {
        1024
    } else {
        120
    };
    let length = spec.description.chars().count();
    if length == 0
        || length > max_description
        || spec.description.contains('\n')
        || spec.description.contains('\r')
    {
        return Err("description");
    }
    if !(1..=300).contains(&spec.argument_form.chars().count()) {
        return Err("argument_form");
    }
    if !(1..=200).contains(&spec.plain.chars().count()) {
        return Err("plain");
    }
    if spec.input_schema.get("type").and_then(|t| t.as_str()) != Some("object") {
        return Err("input_schema");
    }
    Ok(())
}

impl McpFront {
    pub fn new(
        config: McpFrontConfig,
        rules: Arc<RulesEngine>,
        journal: Journal,
        blocks: Arc<dyn BlockCatalogueCheck>,
        metrics: Option<Arc<dyn McpMetrics>>,
    ) -> Self {
        let work = Arc::new(WorkShared::new(
            journal.clone(),
            blocks,
            config.trim_keep_chars,
            config.max_rest_bytes_per_session,
        ));
        let mut registry = Registry::default();
        if let Ok(name) = ServerName::new(WORK_SERVER) {
            registry.slots.push(ProviderSlot {
                provider: Arc::new(WorkProvider {
                    shared: work.clone(),
                }),
                id: WORK_PROVIDER_ID,
                claimed: vec![name],
                prefix: None,
                work: Some(work.clone()),
            });
        }
        McpFront {
            inner: Arc::new(Inner {
                config,
                rules,
                journal,
                metrics,
                work,
                registry: Mutex::new(registry),
                start_gate: tokio::sync::Mutex::new(()),
                shutdown: OnceLock::new(),
                endpoint: OnceLock::new(),
                own_uid: OnceLock::new(),
                sessions: Mutex::new(HashMap::new()),
                changes: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Binds the program's `RefResolver` once, after the program has built the Journal and the
    /// File Service (SA-CLOSE-01). A second call fails with `MCP_REF_RESOLVER_ALREADY_BOUND`.
    // spec: ordin-desktop/components/McpFront#SA-CLOSE-01
    pub fn bind_ref_resolver(&self, resolver: Arc<dyn RefResolver>) -> Result<(), McpError> {
        if self.inner.work.bind_resolver(resolver) {
            Ok(())
        } else {
            Err(McpError::RefResolverAlreadyBound)
        }
    }

    /// Binds the program's `ShownBlockStore` once, before `start`. A second call fails with
    /// `MCP_BLOCK_STORE_ALREADY_BOUND`.
    // spec: ordin-desktop/components/McpFront#SA-MCP-BLOCK-02
    pub fn bind_block_store(&self, store: Arc<dyn ShownBlockStore>) -> Result<(), McpError> {
        if self.inner.work.bind_block_store(store) {
            Ok(())
        } else {
            Err(McpError::BlockStoreAlreadyBound)
        }
    }

    /// Registers a provider. Only before `start`.
    // spec: ordin-desktop/components/McpFront#ADR-100
    pub fn register(&self, provider: Arc<dyn ToolProvider>) -> Result<(), McpError> {
        let inner = &self.inner;
        let id = provider.provider_id();
        let profile = inner.config.profile;
        let mut registry = lock(&inner.registry);
        if registry.started {
            return Err(McpError::RegistryFrozen);
        }
        if registry.slots.iter().any(|s| s.id == id) {
            return Err(McpError::ProviderDuplicate { provider_id: id });
        }
        let prefix = provider.server_prefix();
        if let Some(p) = prefix {
            if !is_server_prefix(p) {
                return Err(McpError::ToolSpecInvalid {
                    provider_id: id,
                    tool: String::new(),
                    rule: "server_prefix",
                });
            }
            for other in &registry.slots {
                if let Some(q) = other.prefix {
                    if p.starts_with(q) || q.starts_with(p) {
                        return Err(McpError::ServerClaimed {
                            server: ServerName(p.to_owned()),
                            by: other.id,
                        });
                    }
                }
                if let Some(name) = other.claimed.iter().find(|n| n.as_str().starts_with(p)) {
                    return Err(McpError::ServerClaimed {
                        server: name.clone(),
                        by: other.id,
                    });
                }
            }
        }
        let specs = provider.servers();
        let mut claimed: Vec<ServerName> = Vec::new();
        for spec in &specs {
            if claimed.contains(&spec.name) {
                return Err(McpError::ToolSpecInvalid {
                    provider_id: id,
                    tool: String::new(),
                    rule: "server_name",
                });
            }
            for other in &registry.slots {
                let by_name = other.claimed.contains(&spec.name);
                let by_prefix = other
                    .prefix
                    .is_some_and(|q| spec.name.as_str().starts_with(q));
                if by_name || by_prefix {
                    return Err(McpError::ServerClaimed {
                        server: spec.name.clone(),
                        by: other.id,
                    });
                }
            }
            let mut seen = BTreeSet::new();
            for tool in &spec.tools {
                check_tool_spec(profile, tool, &seen).map_err(|rule| {
                    McpError::ToolSpecInvalid {
                        provider_id: id,
                        tool: tool.name.clone(),
                        rule,
                    }
                })?;
                seen.insert(tool.name.clone());
            }
            claimed.push(spec.name.clone());
        }
        let tool_count: usize = specs.iter().map(|s| s.tools.len()).sum();
        tracing::info!(
            component = "mcp_front",
            provider_id = id,
            servers = claimed
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(",")
                .as_str(),
            tool_count,
            "mcp_front.provider_registered"
        );
        registry.slots.push(ProviderSlot {
            provider,
            id,
            claimed,
            prefix,
            work: None,
        });
        Ok(())
    }

    /// Freezes the registry, binds the socket (or pipe) and starts accepting bridge connections.
    pub async fn start(&self, shutdown: CancellationToken) -> Result<(), McpError> {
        let _gate = self.inner.start_gate.lock().await;
        if self.inner.is_started() {
            return Err(McpError::AlreadyStarted);
        }
        let listener = bridge::bind(&self.inner).await?;
        {
            let mut registry = lock(&self.inner.registry);
            registry.started = true;
        }
        let _ = self.inner.shutdown.set(shutdown.clone());
        let endpoint = self.inner.endpoint.get().cloned().unwrap_or_default();
        tracing::info!(component = "mcp_front", endpoint = %endpoint, "mcp_front.started");
        bridge::spawn_accept_loop(self.inner.clone(), listener, shutdown);
        Ok(())
    }

    /// Opens a session: mints its token, asks every provider what to offer, returns the launch list.
    pub async fn open_session(&self, grant: SessionGrant) -> Result<SessionMcpConfig, McpError> {
        let inner = &self.inner;
        let shutdown = match (inner.is_started(), inner.shutdown.get()) {
            (true, Some(token)) => token.clone(),
            _ => return Err(McpError::NotStarted),
        };
        if inner.session(grant.run_id).is_some() {
            return Err(McpError::SessionExists {
                run_id: grant.run_id,
            });
        }
        // Mint the token; only its digest is kept.
        let mut raw = [0u8; 32];
        fill_random(&mut raw)?;
        let token = URL_SAFE_NO_PAD.encode(raw);
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();

        let slots = inner.providers();
        let mut offered: BTreeMap<ToolKey, (usize, ToolSpec)> = BTreeMap::new();
        for (index, slot) in slots.iter().enumerate() {
            match offer_from(inner, index, &slots, &grant).await {
                Ok(items) => {
                    for (key, spec) in items {
                        offered.insert(key, (index, spec));
                    }
                }
                Err(failure) => {
                    tracing::warn!(
                        component = "mcp_front",
                        run_id = %grant.run_id,
                        provider_id = slot.id,
                        code = failure.code.as_str(),
                        "mcp_front.open_failed"
                    );
                    return Err(McpError::ProviderFailed {
                        provider_id: slot.id,
                        code: failure.code,
                        retryable: failure.retryable,
                    });
                }
            }
        }
        let launched: BTreeSet<ServerName> = offered.keys().map(|k| k.server.clone()).collect();
        let endpoint = inner.endpoint.get().cloned().unwrap_or_default();
        // spec: ordin-desktop/components/McpFront#SA-RECON-03
        let servers: Vec<McpServerLaunch> = launched
            .iter()
            .map(|name| McpServerLaunch {
                name: name.clone(),
                command: inner.config.hook_program.clone(),
                args: vec![
                    "mcp-bridge".to_owned(),
                    name.as_str().to_owned(),
                    "--agent-instance".to_owned(),
                    grant.agent_instance_id.to_string(),
                ],
                env: BTreeMap::from([
                    ("ORDIN_MCP_ENDPOINT".to_owned(), endpoint.clone()),
                    ("ORDIN_SESSION_ID".to_owned(), grant.run_id.to_string()),
                    ("ORDIN_MCP_SERVER".to_owned(), name.as_str().to_owned()),
                    ("ORDIN_MCP_TOKEN".to_owned(), token.clone()),
                ]),
            })
            .collect();
        let tool_count = offered.len();
        let session = Arc::new(Session {
            grant: grant.clone(),
            token_digest: digest,
            launched,
            offered: Mutex::new(offered),
            cancel: shutdown.child_token(),
            semaphore: Arc::new(Semaphore::new(
                inner.config.max_concurrent_calls_per_session.max(1),
            )),
            conns: Mutex::new(Vec::new()),
            next_conn: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            denied: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        });
        inner.work.open(grant.run_id).await;
        {
            let mut sessions = lock(&inner.sessions);
            if sessions.contains_key(&grant.run_id) {
                drop(sessions);
                inner.work.close(grant.run_id);
                return Err(McpError::SessionExists {
                    run_id: grant.run_id,
                });
            }
            sessions.insert(grant.run_id, session);
        }
        tracing::info!(
            component = "mcp_front",
            run_id = %grant.run_id,
            workspace_id = grant.workspace_id.to_string().as_str(),
            agent_instance_id = grant.agent_instance_id.to_string().as_str(),
            harness = match grant.harness {
                HarnessKind::ClaudeCode => "claude_code",
                HarnessKind::CopilotCli => "copilot_cli",
            },
            servers = servers.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(",").as_str(),
            tool_count,
            "mcp_front.session_opened"
        );
        Ok(SessionMcpConfig { servers })
    }

    /// Idempotent. Closes the session's bridge connections, cancels its calls, drops its state,
    /// and calls `session_closed` on every provider.
    pub async fn close_session(&self, run_id: RunId) -> Result<(), McpError> {
        let inner = &self.inner;
        let Some(session) = lock(&inner.sessions).remove(&run_id) else {
            return Ok(());
        };
        session.cancel.cancel();
        lock(&session.conns).clear();
        let providers = inner.providers();
        let closing = futures::future::join_all(
            providers
                .iter()
                .map(|slot| slot.provider.session_closed(run_id)),
        );
        if tokio::time::timeout(CLOSE_WAIT, closing).await.is_err() {
            tracing::warn!(
                component = "mcp_front",
                run_id = %run_id,
                "mcp_front.session_close_slow"
            );
        }
        tracing::info!(
            component = "mcp_front",
            run_id = %run_id,
            calls = session.calls.load(Ordering::SeqCst),
            denied = session.denied.load(Ordering::SeqCst),
            failed = session.failed.load(Ordering::SeqCst),
            "mcp_front.session_closed"
        );
        Ok(())
    }

    /// The tools offered to an open session. Err: `MCP_SESSION_UNKNOWN`.
    pub fn offered_tools(&self, run_id: RunId) -> Result<Vec<OfferedTool>, McpError> {
        let session = self
            .inner
            .session(run_id)
            .ok_or(McpError::SessionUnknown { run_id })?;
        let offered = lock(&session.offered);
        Ok(offered
            .iter()
            .map(|(key, (_, spec))| OfferedTool {
                server: key.server.clone(),
                tool: key.tool.clone(),
                plain: spec.plain.clone(),
            })
            .collect())
    }

    /// Every server name a registered provider serves now (claimed names, plus the servers
    /// currently listed under a claimed prefix), sorted, and the claimed prefixes. Infallible.
    pub fn server_names(&self) -> (Vec<ServerName>, Vec<&'static str>) {
        let mut names: BTreeSet<ServerName> = BTreeSet::new();
        let mut prefixes: BTreeSet<&'static str> = BTreeSet::new();
        for slot in self.inner.providers() {
            names.extend(slot.claimed.iter().cloned());
            if let Some(prefix) = slot.prefix {
                prefixes.insert(prefix);
                for spec in slot.provider.servers() {
                    if spec.name.as_str().starts_with(prefix) {
                        names.insert(spec.name);
                    }
                }
            }
        }
        (names.into_iter().collect(), prefixes.into_iter().collect())
    }

    /// Turns trimming of long output on or off for calls that start after this point.
    pub fn set_only_needed_parts(&self, on: bool) {
        self.inner.work.set_only_needed(on);
    }

    fn serving_slot(&self, server: &ServerName) -> Option<(usize, ProviderSlot)> {
        for (index, slot) in self.inner.providers().into_iter().enumerate() {
            let claimed = slot.claimed.contains(server);
            let listed = slot.prefix.is_some_and(|p| server.as_str().starts_with(p))
                && slot.provider.servers().iter().any(|s| &s.name == server);
            if claimed || listed {
                return Some((index, slot));
            }
        }
        None
    }
}

/// Fills `buf` from the operating system's random source.
fn fill_random(buf: &mut [u8]) -> Result<(), McpError> {
    use rand::TryRng;
    rand::rngs::SysRng
        .try_fill_bytes(buf)
        .map_err(|_| McpError::SocketUnavailable {
            detail: "random source unavailable".to_owned(),
        })
}

/// Reads one provider's current servers and offer for `grant`; keeps only keys the provider may
/// serve (Internal Logic 5.3). A timeout or provider error is a failure.
pub(crate) async fn offer_from(
    inner: &Inner,
    index: usize,
    slots: &[ProviderSlot],
    grant: &SessionGrant,
) -> Result<Vec<(ToolKey, ToolSpec)>, OfferFailure> {
    let slot = &slots[index];
    let mut accepted: BTreeMap<ServerName, BTreeMap<String, ToolSpec>> = BTreeMap::new();
    for ServerSpec { name, tools } in slot.provider.servers() {
        let by_claim = slot.claimed.contains(&name);
        let by_prefix = slot.prefix.is_some_and(|p| name.as_str().starts_with(p))
            && !slots
                .iter()
                .enumerate()
                .any(|(i, other)| i != index && other.claimed.contains(&name));
        if !by_claim && !by_prefix {
            tracing::warn!(
                component = "mcp_front",
                provider_id = slot.id,
                server = name.as_str(),
                tool = "",
                "mcp_front.offer_ignored"
            );
            continue;
        }
        let entry = accepted.entry(name.clone()).or_default();
        let mut seen = BTreeSet::new();
        for tool in tools {
            match check_tool_spec(inner.config.profile, &tool, &seen) {
                Ok(()) => {
                    seen.insert(tool.name.clone());
                    entry.insert(tool.name.clone(), tool);
                }
                // spec: ordin-desktop/components/McpFront#SA-RECON-06
                Err(_) => tracing::warn!(
                    component = "mcp_front",
                    provider_id = slot.id,
                    server = name.as_str(),
                    tool = tool.name.as_str(),
                    "mcp_front.offer_ignored"
                ),
            }
        }
    }
    let keys = match tokio::time::timeout(OFFER_TIMEOUT, slot.provider.offer(grant)).await {
        Err(_) => {
            return Err(OfferFailure {
                code: "MCP_PROVIDER_FAILED".to_owned(),
                retryable: true,
            })
        }
        Ok(Err(ToolFailure {
            code, retryable, ..
        })) => return Err(OfferFailure { code, retryable }),
        Ok(Ok(keys)) => keys,
    };
    let mut out: BTreeMap<ToolKey, ToolSpec> = BTreeMap::new();
    for key in keys {
        // A session limited to named servers is offered nothing else, after the provider's own
        // offer, so no provider can widen it; such keys were never meant for it, so no log line.
        // spec: ordin-desktop/components/McpFront#SA-MCP-SRV-01
        if let Some(only) = &grant.only_servers {
            if !only.contains(&key.server) {
                continue;
            }
        }
        match accepted.get(&key.server).and_then(|t| t.get(&key.tool)) {
            Some(spec) => {
                out.insert(key, spec.clone());
            }
            None => tracing::warn!(
                component = "mcp_front",
                provider_id = slot.id,
                server = key.server.as_str(),
                tool = key.tool.as_str(),
                "mcp_front.offer_ignored"
            ),
        }
    }
    Ok(out.into_iter().collect())
}

impl ToolRegistry for McpFront {
    fn register(&self, provider: Arc<dyn ToolProvider>) -> Result<(), McpError> {
        McpFront::register(self, provider)
    }

    // spec: ordin-desktop/components/McpFront#SA-RECON-02
    fn tools_changed(&self, server: &ServerName) {
        let inner = &self.inner;
        if !inner.is_started() || self.serving_slot(server).is_none() {
            tracing::debug!(
                component = "mcp_front",
                server = server.as_str(),
                "mcp_front.tools_changed_ignored"
            );
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::debug!(
                component = "mcp_front",
                server = server.as_str(),
                "mcp_front.tools_changed_ignored"
            );
            return;
        };
        {
            // Two changes for one server while a task runs are merged: it runs once more.
            let mut changes = lock(&inner.changes);
            if let Some(again) = changes.get_mut(server) {
                *again = true;
                return;
            }
            changes.insert(server.clone(), false);
        }
        let front = self.clone();
        let server = server.clone();
        runtime.spawn(async move {
            loop {
                call::tools_changed_pass(&front, &server).await;
                let mut changes = lock(&front.inner.changes);
                match changes.get_mut(&server) {
                    Some(again) if *again => *again = false,
                    _ => {
                        changes.remove(&server);
                        break;
                    }
                }
            }
        });
    }

    fn served_tools(&self, server: &ServerName) -> Option<Vec<String>> {
        let (_, slot) = self.serving_slot(server)?;
        let mut names: Vec<String> = slot
            .provider
            .servers()
            .into_iter()
            .filter(|s| &s.name == server)
            .flat_map(|s| s.tools.into_iter().map(|t| t.name))
            .collect();
        names.sort();
        Some(names)
    }
}

#[async_trait]
impl ToolDispatch for McpFront {
    fn session_tools(&self, run_id: RunId) -> Result<Vec<(ToolKey, ToolSpec)>, McpError> {
        let session = self
            .inner
            .session(run_id)
            .ok_or(McpError::SessionUnknown { run_id })?;
        let offered = lock(&session.offered);
        Ok(offered
            .iter()
            .filter(|(_, (_, spec))| spec.sandbox_callable)
            .map(|(key, (_, spec))| (key.clone(), spec.clone()))
            .collect())
    }

    // spec: ordin-desktop/components/McpFront#SA-RECON-05
    async fn dispatch(
        &self,
        run_id: RunId,
        key: ToolKey,
        arguments: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<ToolCallResult, McpError> {
        let session = self
            .inner
            .session(run_id)
            .ok_or(McpError::SessionUnknown { run_id })?;
        Ok(call::run_call(
            &self.inner,
            &session,
            key,
            arguments,
            crate::provider::CallVia::Sandbox,
            Some(cancel),
            None,
        )
        .await)
    }
}
