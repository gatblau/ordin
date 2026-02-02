//! One `sandboxd` controller per agent instance and workspace, and the ordin implementations of
//! the seams the daemon's own parts would fill (Sandbox.md *Internal Logic* D and E).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ordin_identity::IdentityService;
use ordin_mcp_front::{ToolDispatch, ToolKey, ToolSpec};
use ordin_types::{AgentInstanceId, CodedError, CredentialId, RunId, WorkspaceId};
use sandboxd::audit::{AuditLogger, AuditSink};
use sandboxd::broker::{
    freeze_api_keys, BrokerCall, BrokerError, CredentialSource, IdentityBroker,
};
use sandboxd::config::{ConfigError, SecretResolver};
use sandboxd::controller::{CapabilityMinter, IdTokenSink, Interactor, SandboxController};
use sandboxd::downstream::{DownstreamClient, DEFAULT_CALL_TIMEOUT};
use sandboxd::interaction::{InteractionError, InteractionOutcome};
use sandboxd::mcp_upstream::McpUpstreamClient;
use sandboxd::obo::OboClient;
use sandboxd::policy::PolicyEngine;
use sandboxd::runtime::{Limits, SandboxRuntime};
use sandboxd::session::SessionManager;
use sandboxd::types::{
    AuditEntry, CapabilityHandle, ClientIdentity, InteractionRequired, Params, Principal,
    ResolvedCapability, UntrustedMcpResult, UntrustedPart, UntrustedResponse,
};
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::args::RESERVED_CAPABILITY;
use crate::error::Refusal;
use crate::grants::SandboxCapability;
use crate::manifest::{self, KEY_REF_PREFIX};

/// Response and module-result size: 1 MiB.
pub(crate) const MAX_RESPONSE_BYTES: usize = 1_048_576;
/// Calls per run, applied to module calls too (`config.rs:148`).
const MAX_CALLS_PER_RUN: u32 = 50;
/// Sandbox runs per agent session (`config.rs:149`).
const MAX_RUNS_PER_SESSION: u32 = 500;
/// The capability-handle validity of `sandboxd` (`broker.rs:26`).
const CAP_TTL: Duration = Duration::from_secs(300);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A `SecretResolver` over values fetched before the call, because the trait cannot await
/// (Sandbox.md A.2).
pub(crate) struct PrefilledResolver(pub HashMap<String, Vec<u8>>);

impl SecretResolver for PrefilledResolver {
    fn resolve(&self, reference: &str) -> Result<Vec<u8>, ConfigError> {
        self.0.get(reference).cloned().ok_or_else(|| ConfigError {
            code: "CFG_SECRET_UNRESOLVED",
            field: reference.to_owned(),
        })
    }
}

/// What one call in progress needs to answer the controller and the module (C.8).
pub(crate) struct RunContext {
    pub run_id: RunId,
    pub person_credential: Option<CredentialId>,
    pub cancel: CancellationToken,
    pub tools: Vec<(ToolKey, ToolSpec)>,
    pub module_calls: u32,
    /// The sign-in fault the interactor recorded: (code, retryable).
    pub fault: Option<(&'static str, bool)>,
}

/// Run contexts by `client_label` (the ordin run id as a string).
pub(crate) type RunContexts = Arc<Mutex<HashMap<String, RunContext>>>;
/// `ordin_tools` capability references → (client label, expiry).
pub(crate) type ModuleRefs = Arc<Mutex<HashMap<[u8; 16], (String, Instant)>>>;

/// Holds one run's tokens in memory for the broker (`CredentialSource`) and receives them from
/// the controller (`IdTokenSink`).
#[derive(Default)]
pub(crate) struct RunCredentials {
    id_token: Mutex<Option<String>>,
    access_token: Mutex<Option<String>>,
}

impl RunCredentials {
    /// Drops the run's tokens (Sandbox.md *Security*: dropped when the run context is removed).
    pub fn clear(&self) {
        *lock(&self.id_token) = None;
        *lock(&self.access_token) = None;
    }
}

impl CredentialSource for RunCredentials {
    fn id_token(&self) -> Option<String> {
        lock(&self.id_token).clone()
    }
    fn access_token(&self) -> Option<String> {
        lock(&self.access_token).clone()
    }
}

impl IdTokenSink for RunCredentials {
    fn set_id_token(&self, id_token: String) {
        *lock(&self.id_token) = Some(id_token);
    }
    fn set_access_token(&self, access_token: String) {
        *lock(&self.access_token) = Some(access_token);
    }
}

/// The interaction surface (I-9): sign-in from Identity, consent already given by the Rules
/// Engine, step-up never requested. Nothing is shown to the person.
pub(crate) struct OrdinInteractor {
    pub runs: RunContexts,
    pub identity: Arc<IdentityService>,
}

impl OrdinInteractor {
    async fn require(
        &self,
        who: &ClientIdentity,
        what: InteractionRequired,
    ) -> Result<InteractionOutcome, InteractionError> {
        match what {
            InteractionRequired::Consent { .. } => Ok(InteractionOutcome::Allowed),
            InteractionRequired::StepUp { .. } => Err(InteractionError::Denied),
            InteractionRequired::SignIn { .. } => {
                let credential = lock(&self.runs)
                    .get(&who.client_label)
                    .and_then(|c| c.person_credential);
                let Some(credential) = credential else {
                    return Err(InteractionError::Denied);
                };
                match self.identity.access_token_by_id(credential).await {
                    Ok(token) => Ok(InteractionOutcome::SignedIn {
                        principal: Principal {
                            subject: "ordin:person".to_owned(),
                            issuer: String::new(),
                            acr: None,
                            amr: Vec::new(),
                            auth_time: None,
                        },
                        id_token: String::new(),
                        access_token: token.token.expose().to_owned(),
                    }),
                    Err(e) => {
                        if let Some(ctx) = lock(&self.runs).get_mut(&who.client_label) {
                            ctx.fault = Some((e.code(), e.retryable()));
                        }
                        Err(InteractionError::SignInFailed)
                    }
                }
            }
        }
    }
}

impl Interactor for OrdinInteractor {
    fn require_boxed<'a>(
        &'a self,
        who: &'a ClientIdentity,
        what: InteractionRequired,
    ) -> Pin<Box<dyn Future<Output = Result<InteractionOutcome, InteractionError>> + Send + 'a>>
    {
        Box::pin(self.require(who, what))
    }
}

/// Mints `ordin_tools` references itself and delegates every other capability (D.7).
pub(crate) struct OrdinMinter {
    pub inner: Arc<IdentityBroker>,
    pub refs: ModuleRefs,
}

impl CapabilityMinter for OrdinMinter {
    fn mint_caps(
        &self,
        principal: &Principal,
        run_id: &str,
        client_label: &str,
        caps: &[ResolvedCapability],
    ) -> Vec<CapabilityHandle> {
        let mut handles = Vec::with_capacity(caps.len());
        for cap in caps {
            if cap.id == RESERVED_CAPABILITY {
                let mut cap_id = [0u8; 16];
                if getrandom::fill(&mut cap_id).is_err() {
                    continue; // never mint a reference from a failed random draw
                }
                lock(&self.refs)
                    .insert(cap_id, (client_label.to_owned(), Instant::now() + CAP_TTL));
                let expires_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or_default()
                    + CAP_TTL.as_secs() as i64;
                handles.push(CapabilityHandle {
                    cap_id,
                    capability_id: cap.id.clone(),
                    expires_at,
                });
            } else {
                handles.extend(self.inner.mint_caps(
                    principal,
                    run_id,
                    client_label,
                    std::slice::from_ref(cap),
                ));
            }
        }
        handles
    }
}

/// Routes an `ordin_tools` reference to the module call (section E) and everything else to the
/// `IdentityBroker` (D.7).
pub(crate) struct OrdinBroker {
    pub inner: Arc<IdentityBroker>,
    pub module: Arc<ModuleRoute>,
}

impl BrokerCall for OrdinBroker {
    fn call_boxed<'a>(
        &'a self,
        cap_id: &'a [u8; 16],
        verb: &'a str,
        path: &'a str,
        params: &'a Params,
        body: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<UntrustedResponse, BrokerError>> + Send + 'a>> {
        self.inner.call_boxed(cap_id, verb, path, params, body)
    }

    fn call_tool_boxed<'a>(
        &'a self,
        cap_id: &'a [u8; 16],
        tool: &'a str,
        arguments: &'a Value,
    ) -> Pin<Box<dyn Future<Output = Result<UntrustedMcpResult, BrokerError>> + Send + 'a>> {
        if self.module.owns(cap_id) {
            Box::pin(self.module.call(cap_id, tool, arguments))
        } else {
            self.inner.call_tool_boxed(cap_id, tool, arguments)
        }
    }
}

/// The broker the start-up probe compiles the guest with; it is never called.
pub(crate) struct ProbeBroker;

impl BrokerCall for ProbeBroker {
    fn call_boxed<'a>(
        &'a self,
        _cap_id: &'a [u8; 16],
        _verb: &'a str,
        _path: &'a str,
        _params: &'a Params,
        _body: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<UntrustedResponse, BrokerError>> + Send + 'a>> {
        Box::pin(async { Err(BrokerError::CapInvalid) })
    }

    fn call_tool_boxed<'a>(
        &'a self,
        _cap_id: &'a [u8; 16],
        _tool: &'a str,
        _arguments: &'a Value,
    ) -> Pin<Box<dyn Future<Output = Result<UntrustedMcpResult, BrokerError>> + Send + 'a>> {
        Box::pin(async { Err(BrokerError::CapInvalid) })
    }
}

fn envelope(is_error: bool, body: Vec<u8>, truncated: bool) -> UntrustedMcpResult {
    UntrustedMcpResult {
        untrusted: true,
        is_error,
        parts: vec![UntrustedPart::Json { body }],
        truncated,
    }
}

fn error_envelope(body: Value) -> UntrustedMcpResult {
    envelope(true, body.to_string().into_bytes(), false)
}

/// A call from agent-written code to one of the session's ordin tools (section E).
pub(crate) struct ModuleRoute {
    pub refs: ModuleRefs,
    pub runs: RunContexts,
    pub dispatch: Arc<dyn ToolDispatch>,
}

impl ModuleRoute {
    fn owns(&self, cap_id: &[u8; 16]) -> bool {
        lock(&self.refs).contains_key(cap_id)
    }

    // spec: ordin-desktop/components/Sandbox#SBX_MODULE_HANDLE_INVALID
    async fn call(
        &self,
        cap_id: &[u8; 16],
        tool: &str,
        arguments: &Value,
    ) -> Result<UntrustedMcpResult, BrokerError> {
        let label = lock(&self.refs)
            .get(cap_id)
            .filter(|(_, expires)| *expires > Instant::now())
            .map(|(label, _)| label.clone());
        let invalid = || error_envelope(json!({"code": "SBX_MODULE_HANDLE_INVALID"}));
        let Some(label) = label else {
            return Ok(invalid());
        };
        let found = {
            let mut runs = lock(&self.runs);
            match runs.get_mut(&label) {
                None => None,
                Some(ctx) => {
                    ctx.module_calls += 1;
                    let over = ctx.module_calls > MAX_CALLS_PER_RUN;
                    let key = ctx
                        .tools
                        .iter()
                        .find(|(k, _)| k.tool == tool)
                        .map(|(k, _)| k.clone());
                    Some((ctx.run_id, ctx.cancel.child_token(), over, key))
                }
            }
        };
        let Some((run_id, cancel, over, key)) = found else {
            return Ok(invalid());
        };
        if over {
            return Ok(error_envelope(json!({"code": "SBX_RUN_BUDGET_EXHAUSTED"})));
        }
        // A tool outside the run's module list is refused as `sandboxd` refuses one outside a
        // capability's `toolAllow` (E.3).
        let Some(key) = key else {
            return Err(BrokerError::ToolDenied);
        };
        if !arguments.is_object() {
            return Ok(error_envelope(json!({"code": "SBX_ARGS_INVALID"})));
        }
        match self
            .dispatch
            .dispatch(run_id, key, arguments.clone(), cancel)
            .await
        {
            Ok(result) if !result.is_error => {
                let value = result
                    .structured
                    .unwrap_or_else(|| json!({"text": result.text}));
                let mut body = serde_json::to_vec(&value).unwrap_or_default();
                let truncated = body.len() > MAX_RESPONSE_BYTES;
                body.truncate(MAX_RESPONSE_BYTES);
                Ok(envelope(false, body, truncated))
            }
            Ok(result) => Ok(error_envelope(
                json!({"code": result.code, "reason": result.text}),
            )),
            Err(e) => Ok(error_envelope(json!({"code": e.code()}))),
        }
    }
}

/// Appends one Journal entry per brokered call, without the path (D, `JournalAuditSink`).
/// `emit` is synchronous, so entries go through a channel drained by a task.
pub(crate) struct JournalAuditSink {
    pub tx: UnboundedSender<(AuditEntry, WorkspaceId)>,
    pub workspace: WorkspaceId,
}

impl AuditSink for JournalAuditSink {
    fn emit(&self, entry: &AuditEntry) {
        let _ = self.tx.send((entry.clone(), self.workspace));
    }
}

/// The Journal entry of one brokered call.
pub(crate) fn broker_call_entry(
    entry: &AuditEntry,
    workspace: WorkspaceId,
) -> ordin_journal::NewJournalEntry {
    ordin_journal::NewJournalEntry {
        kind: "sandbox_broker_call".to_owned(),
        component: "sandbox".to_owned(),
        run_id: RunId::parse(&entry.client_label).ok(),
        crew_run_id: None,
        workspace_id: Some(workspace),
        summary: "Sandboxed code called a company system.".to_owned(),
        data: json!({
            "capability_id": entry.capability_id,
            "provider": entry.provider,
            "method": entry.method,
            "host": entry.host,
            "status_code": entry.status_code,
            "request_bytes": entry.request_bytes,
            "response_bytes": entry.response_bytes,
            "duration_ms": entry.duration_ms,
            "user_hmac": entry.user_hmac,
        }),
    }
}

/// One agent's controller and what was built from its grants.
pub(crate) struct AgentController {
    pub controller: SandboxController,
    pub creds: Arc<RunCredentials>,
    pub capabilities: Vec<SandboxCapability>,
    /// SHA-256 of the grants it was built from (D.1).
    pub digest: String,
    pub last_used: Mutex<Instant>,
}

impl AgentController {
    pub fn granted_names(&self) -> Vec<String> {
        self.capabilities.iter().map(|c| c.name.clone()).collect()
    }
}

/// What building a controller needs from the Sandbox.
pub(crate) struct BuildParts<'a> {
    pub agent: AgentInstanceId,
    pub workspace: WorkspaceId,
    pub capabilities: Vec<SandboxCapability>,
    pub digest: String,
    pub identity: &'a Arc<IdentityService>,
    pub audit_key: Vec<u8>,
    pub audit_tx: UnboundedSender<(AuditEntry, WorkspaceId)>,
    pub runs: RunContexts,
    pub module: Arc<ModuleRoute>,
    pub guest: &'static [u8],
    pub guest_digest: &'static str,
}

/// Builds one agent's controller (D.3–D.8).
pub(crate) async fn build(parts: BuildParts<'_>) -> Result<AgentController, Refusal> {
    let json = manifest::render(&parts.capabilities);
    let policy = Arc::new(PolicyEngine::load(&json, None, &|_, _| false).map_err(|_| {
        tracing::warn!(target: "ordin_sandbox", agent_instance_id = parts.agent.to_string().as_str(), "sandbox.policy_invalid");
        Refusal::policy_invalid()
    })?);

    // spec: ordin-desktop/components/Sandbox#SBX_KEY_MISSING
    let mut keys = HashMap::new();
    let secrets = parts.identity.secrets();
    for reference in policy.api_key_secret_refs() {
        let key_name = reference
            .strip_prefix(KEY_REF_PREFIX)
            .unwrap_or(&reference)
            .to_owned();
        match secrets.agent_key(parts.agent, &key_name).await {
            Ok(Some(key)) => {
                keys.insert(reference.clone(), key.to_vec());
            }
            Ok(None) => return Err(Refusal::key_missing(&key_name)),
            Err(_) => return Err(Refusal::credential_unavailable()),
        }
    }
    let api_keys = freeze_api_keys(policy.api_key_secret_refs(), &PrefilledResolver(keys))
        .map_err(|_| Refusal::internal())?;

    let creds = Arc::new(RunCredentials::default());
    let audit = Arc::new(AuditLogger::new(
        parts.audit_key,
        Box::new(JournalAuditSink {
            tx: parts.audit_tx,
            workspace: parts.workspace,
        }),
    ));
    let max = MAX_RESPONSE_BYTES as u64;
    let downstream = Arc::new(
        DownstreamClient::new(max, DEFAULT_CALL_TIMEOUT, false).map_err(|_| Refusal::internal())?,
    );
    let upstream = Arc::new(
        McpUpstreamClient::new(max, sandboxd::mcp_upstream::DEFAULT_CALL_TIMEOUT, false)
            .map_err(|_| Refusal::internal())?,
    );
    let obo = Arc::new(OboClient::new(String::new()).map_err(|_| Refusal::internal())?);
    let broker = Arc::new(
        IdentityBroker::new(
            policy.clone(),
            audit,
            obo,
            downstream,
            creds.clone(),
            MAX_RESPONSE_BYTES,
            Arc::new(api_keys),
        )
        .with_mcp_upstream(upstream),
    );
    let ordin_broker: Arc<dyn BrokerCall> = Arc::new(OrdinBroker {
        inner: broker.clone(),
        module: parts.module.clone(),
    });
    // SH-1 of *Changes required in `sandbox/sandbox-host`* (not a gap): one guest compile per
    // agent controller until `sandboxd` shares one compiled guest.
    let guest = parts.guest;
    let guest_digest = parts.guest_digest;
    let runtime =
        tokio::task::spawn_blocking(move || SandboxRuntime::new(guest_digest, guest, ordin_broker))
            .await
            .map_err(|_| Refusal::internal())?
            .map_err(|_| Refusal::guest_start_failed())?;

    let controller = SandboxController::new(
        policy,
        Arc::new(OrdinInteractor {
            runs: parts.runs,
            identity: parts.identity.clone(),
        }),
        Arc::new(OrdinMinter {
            inner: broker,
            refs: parts.module.refs.clone(),
        }),
        Arc::new(runtime),
        Arc::new(SessionManager::new(MAX_RUNS_PER_SESSION)),
        creds.clone(),
        String::new(),
        Limits {
            fuel: Some(u64::MAX),
            epoch_deadline: Duration::from_secs(30),
            max_memory_bytes: 536_870_912,
            max_output_bytes: MAX_RESPONSE_BYTES,
        },
    );
    Ok(AgentController {
        controller,
        creds,
        capabilities: parts.capabilities,
        digest: parts.digest,
        last_used: Mutex::new(Instant::now()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prefilled_resolver_answers_only_what_it_holds() {
        let r = PrefilledResolver(HashMap::from([("a".to_owned(), b"k".to_vec())]));
        assert_eq!(r.resolve("a").unwrap(), b"k");
        assert_eq!(r.resolve("b").unwrap_err().code, "CFG_SECRET_UNRESOLVED");
    }

    #[test]
    fn run_credentials_are_held_then_dropped() {
        let c = RunCredentials::default();
        c.set_access_token("t".to_owned());
        c.set_id_token("i".to_owned());
        assert_eq!(c.access_token().as_deref(), Some("t"));
        assert_eq!(c.id_token().as_deref(), Some("i"));
        c.clear();
        assert!(c.access_token().is_none() && c.id_token().is_none());
    }

    #[test]
    fn a_broker_call_entry_carries_no_path() {
        let run = RunId::new();
        let entry = AuditEntry {
            timestamp: 0,
            run_id: "x".to_owned(),
            user_hmac: "h".to_owned(),
            client_label: run.to_string(),
            provider: "microsoft".to_owned(),
            capability_id: "tickets".to_owned(),
            method: "GET".to_owned(),
            host: "tickets.example.com".to_owned(),
            path: "/v1/open/4711".to_owned(),
            status_code: 200,
            request_bytes: 1,
            response_bytes: 2,
            duration_ms: 3,
        };
        let e = broker_call_entry(&entry, WorkspaceId::new());
        assert_eq!(e.run_id, Some(run));
        assert!(!e.data.to_string().contains("/v1/open"));
        assert_eq!(e.data["capability_id"], "tickets");
    }

    #[tokio::test]
    async fn the_probe_broker_refuses_every_call() {
        let p = ProbeBroker;
        let params = Vec::new();
        assert!(p
            .call_boxed(&[0; 16], "GET", "/", &params, &[])
            .await
            .is_err());
        assert!(p.call_tool_boxed(&[0; 16], "t", &json!({})).await.is_err());
    }
}
