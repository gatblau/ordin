# SPEC: Sandbox

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-sandbox` · **Build phase:** 5 · **Programs:** D P
> **Dependencies:** MCP Front, Rules Engine, Identity, Journal

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Gaps](#gaps)

---

## Purpose

The Sandbox runs code an agent writes — Python through `python_sandbox`, and shell scripts and JavaScript through the sandbox's two further tools once the embedded crate offers those guests — in the WebAssembly room of `sandboxd`, linked into the core in-process (02:380–385, ADR-031, ADR-040). The code has no network and no files of its own: input files are passed in by the core (from places the agent may read, or as content the session supplies), and every file the code adds or changes comes back as an artefact in the agent's folder. Calls the code makes to company systems go through `sandboxd`'s broker, under the capabilities the Rules Engine grants that agent, with the person's tokens from Identity and Credentials; the code never holds a token. It also keeps one working folder for the length of an agent session when the agent asks (ADR-040 F5), and offers the session's other ordin tools inside the room as the generated `ordin_tools` module (ADR-062, SA-NAME-06). It owns nothing durable (02:383).

## Approach

The Sandbox is a thin in-process wrapper around the `sandboxd` library: it builds `sandboxd`'s own components (`PolicyEngine`, `IdentityBroker`, `SandboxRuntime`, `SessionManager`, `SandboxController`) directly in Rust, one controller per agent instance, and replaces the daemon's interaction surface, secret resolver and audit sink with ordin implementations backed by Identity and Credentials and the Journal (2D-1f; I-9). It never runs `sandboxd`'s `main`, its control socket, its `mcp-stdio` front door or its credential CLI. The capability manifest each controller loads is rendered in memory from the Rules Engine's grants for that agent instance. Work sessions and the `ordin_tools` module are implemented in the wrapper over the crate's public seams (`files`, `CapabilityMinter`, `BrokerCall`), so they need no change in `sandboxd`; the shell and JavaScript guests and run environment variables do need a `sandboxd` change and are behind the Cargo feature `extra-guests` *(chosen here)* until it lands. Prefix for error codes: `SBX_`.
Reconciliation assumptions (Phase 3 reconciliation):

- **SA-RECON-01** The Sandbox owns the `ordin_tools` module generator (ruling R15): it builds the prelude from the MCP Front's list of the session's sandbox-callable tools and routes each module call back through the MCP Front, which only routes (`ToolDispatch`, owned and implemented by the MCP Front). The MCP Front generates no module text.
- **SA-RECON-02** The Sandbox does not register itself: it exposes `tool_provider()` and the program registers it with the MCP Front before `start`, in the MCP Front's registration order (reconciliation row 325).
- **SA-RECON-03** All three tools are `sandbox_callable = false`: agent-written code cannot start a further sandbox run (reconciliation row 324).
- **SA-RECON-05** `SandboxGrants` is owned here and implemented by the program from the Package Inspector's declared connections, the organisation profile and Identity and Credentials; the Rules Engine does not implement it (RulesEngine.md SA-RECON-02; ruling R6).
- **SA-RECON-06** Identity is reached through its generic interface (reconciliation R9): `IdentityService::access_token_by_id`, `SecretStore::get_or_create_random` and `SecretStore::agent_key`; there is no `SandboxSecrets`.
- **SA-RECON-07** The Journal indexes artefacts and never writes bytes (ruling R3): output bytes go into the agent's folder through the program's `PlaceFiles::write_agent_output` (over the Agent Folder Manager), then `Journal::record_artefact` indexes them. Journal entries use `append_entry`; run ends arrive through a Journal `EventSink`.
- **SA-RECON-04** The programs read `Sandbox::status()` for the start-up report through the Session Supervisor's `SupervisorHost::startup_items`, and for Diagnostics (reconciliation row 326, O83).

**C-17 (bound confirmation item, Phase 1).** Every assertion of this spec is conditional on `sandboxd`'s library target embedding in the core without its daemon. The HLD names no fallback for C-17 beyond its spike; if the spike fails, the Sandbox cannot be built in-process and the decision returns to the HLD (ADR-031). The `sandboxd` changes this spec works around — per-agent guest compile, one person credential per run, and the shell and JavaScript guests and run environment behind `extra-guests` — are recorded in *Changes required in `sandbox/sandbox-host`* below, not as gaps.

### Changes required in `sandbox/sandbox-host`

Changes to the embedded `sandboxd` crate that this spec works around until they land; none blocks this spec, and none is a gap.

| # | Change in `sandboxd` | What this spec does meanwhile | Where |
|---|---|---|---|
| SH-1 | Share one compiled Python guest across agent controllers, instead of one compile per `SandboxRuntime` | Compiles the guest once per agent controller | Internal Logic D.8 |
| SH-2 | Allow more than one person credential per run (the crate installs one access token per run, `controller.rs:242`–`243`) | Refuses a call whose `PersonToken` capabilities name two `credential_id`s with `SBX_MIXED_CREDENTIALS` | Internal Logic C.5 |
| SH-3 | Ship the shell and JavaScript guests (`sandboxd` ADR-043, not implemented) | Offers `shell_sandbox` and `javascript_sandbox` only under the Cargo feature `extra-guests` | *MCP tools*; Internal Logic B.1 |
| SH-4 | Accept run environment variables (`sandboxd` ADR-046) | Accepts `environment` only under `extra-guests`; otherwise refuses with `SBX_ENVIRONMENT_UNSUPPORTED` | *MCP tools*; Error Table |

Rejected: *run `sandboxd` as a per-user daemon and relay to its socket* — a second process, a token file and its own consent screens, which ADR-031 and ADR-037 replace with in-process linking and the Rules Engine; *one `PolicyEngine` shared by every agent* — capability names are the guest's `api.<name>`, so names would collide between agents and one agent's grants would be visible to another's code.

## Shared Context

### Shared types used (from Phase 2C, copied verbatim)

```rust
// ordin-types (domain part) — identifiers are UUIDv7 newtypes, lowercase hyphenated.
pub struct RunId(pub Uuid);
pub struct WorkspaceId(pub Uuid);
pub struct AgentInstanceId(pub Uuid);
pub struct CrewRunId(pub Uuid);
pub struct ScopeId(pub Uuid);
pub struct ArtefactId(pub Uuid);
pub struct PlaceId(pub Uuid);
pub struct CredentialId(pub Uuid);

/// RFC 3339, UTC, millisecond precision, `Z` suffix.
pub struct Timestamp(pub DateTime<Utc>);

/// SHA-256 as 64 lowercase hexadecimal characters.
#[serde(transparent)]
pub struct Sha256Hex(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Artefact {
    pub id: ArtefactId,
    pub location: ArtefactLocation,
    pub path: String,                 // relative to its folder, ≤ 4096 bytes (ART_PATH_INVALID)
    pub version: u32,
    pub hash: Sha256Hex,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producing_run_id: Option<RunId>,
    pub media_type: String,           // RFC 6838 type/subtype, ≤ 255 chars (ART_MEDIA_TYPE_INVALID)
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtefactLocation {
    AgentFolder { agent_instance_id: AgentInstanceId },
    Place { place_id: PlaceId },
}

/// Lifecycle of a Run (04 — Run). Terminal states: completed, failed, stopped_at_limit,
/// cancelled, interrupted, abandoned, no_reply.
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued, Running, Paused,
    Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply,
}

#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Github, Oidc, Slack, Confluence, Microsoft, Google,
    MailServerSingleSignOn, MailServerPassword, MailServerAppPassword,
}
```

```rust
// ordin-types (wire part)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProgressCount {
    pub done: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

// The Sandbox reads one event, from the Journal's `EventSink` (`JournalEvent`, below):
//   kind "run_state_changed", data { "run_id": RunId, "state": RunState }

/// The Rules Engine's tool-call input (2C §3). The MCP Front builds it; the Sandbox never does.
pub struct ToolCall {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub point: DecisionPoint,
    pub tool: String,
    pub origin: ToolOrigin,          // OrdinMcpFront { server: "sandbox" } for this component's tools
    pub arguments: serde_json::Value,
}
```

### `ToolProvider` and `ToolRegistry` — owned by the MCP Front (McpFront.md, ADR-100; copied, ruling R2)

```rust
// crate ordin-mcp-front (owner). This crate implements ToolProvider; the program registers it.
pub use ordin_types::session::ToolCaller;   // Ordin | Agent | CrewMember { crew_run_id } | HostedAgent
pub struct ServerName(String);              // ^[a-z][a-z0-9_]{0,31}$
pub enum ProgramProfile { Desktop, PersonalServer, AgentProfile }

pub struct SessionGrant {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,          // agent profile: the one workspace the Agent Loader assigns
    pub agent_instance_id: AgentInstanceId,
    pub harness: HarnessKind,
    pub profile: ProgramProfile,
    pub caller: ToolCaller,
    pub scope_id: Option<ScopeId>,          // agent profile only; None = agent-wide or no scope
    pub coding_task_id: Option<CodingTaskId>,
}
pub struct ToolKey { pub server: ServerName, pub tool: String }   // tool ^[A-Za-z][A-Za-z0-9_-]{0,63}$
pub struct ServerSpec { pub name: ServerName, pub tools: Vec<ToolSpec> }
pub struct ToolSpec {
    pub name: String,
    pub description: String,                // one line; 1–120 chars (1–1,024 in the agent profile)
    pub input_schema: serde_json::Value,    // JSON Schema, "type": "object"
    pub argument_form: String,              // 1–300 chars, quoted to the model on a wrong call
    pub sandbox_callable: bool,             // offered in the Sandbox's ordin_tools module
    pub plain: String,                      // start-up report sentence, 1–200 chars
}
pub enum CallVia { Harness, Sandbox }
pub struct ProgressSink { /* private */ }
impl ProgressSink { pub fn report(&self, done: u64, total: Option<u64>); }
pub struct ToolCallContext {
    pub run_id: RunId, pub workspace_id: WorkspaceId, pub agent_instance_id: AgentInstanceId,
    pub caller: ToolCaller, pub scope_id: Option<ScopeId>, pub coding_task_id: Option<CodingTaskId>,
    pub call_id: ToolRequestId, pub correlation_id: String,
    pub deadline: std::time::Instant, pub cancel: tokio_util::sync::CancellationToken,
    pub progress: ProgressSink, pub only_needed_parts: bool, pub via: CallVia,
}
pub struct ToolOutput { pub text: String, pub structured: Option<serde_json::Value> }
pub struct ToolFailure { pub code: String, pub message: String /* 1–300 chars */, pub retryable: bool }
pub struct ArgumentProblem { pub at: String /* JSON Pointer */, pub reason: String /* 1–300 chars */ }

#[async_trait::async_trait]
pub trait ToolProvider: Send + Sync + 'static {
    fn provider_id(&self) -> &'static str;                 // ^[a-z][a-z0-9-]{0,39}$
    fn servers(&self) -> Vec<ServerSpec>;
    fn server_prefix(&self) -> Option<&'static str> { None }
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure>;   // within 2 s
    fn check_arguments(&self, key: &ToolKey, arguments: &serde_json::Value) -> Result<(), ArgumentProblem>;
    async fn call(&self, ctx: ToolCallContext, key: &ToolKey, arguments: serde_json::Value)
        -> Result<ToolOutput, ToolFailure>;
    async fn session_closed(&self, run_id: RunId);
}
pub trait ToolRegistry: Send + Sync {
    fn register(&self, provider: std::sync::Arc<dyn ToolProvider>) -> Result<(), McpError>;
    fn tools_changed(&self, server: &ServerName);
    fn served_tools(&self, server: &ServerName) -> Option<Vec<String>>;
}
```

How this spec reads the earlier shape (SA-RECON, reconciliation rows 279, 283–286, 294–297): what it called `tools(session)` is `servers()` (the full tool list with `ToolSpec`s) plus `offer(&grant)` (the keys this session gets); `session.caller`, `session.workspace_id`, `session.scope_id` are the grant's or the context's fields of the same names, and the agent instance is `agent_instance_id`; `session.program` is `grant.profile` (`AgentSingle` / `AgentScaled` both read as `AgentProfile`; a provider that must tell the single and scaled shapes apart reads its own configuration, row 285); the session's progress channel is `ctx.progress`. A `ToolError::InvalidArguments { code, reason }` raised by a schema or type check is returned from `check_arguments` as `ArgumentProblem { at, reason }` (the MCP Front answers `MCP_ARGUMENTS_INVALID`); an invalid-argument finding that needs state (a missing record) stays in `call` as `ToolFailure { code, message: reason, retryable: false }`. `ToolError::Refused { code, reason }` → `ToolFailure { code, message: reason, retryable: false }`; `ToolError::Unavailable { code, reason, retryable }` → `ToolFailure { code, message: reason, retryable }`; `ToolError::Internal { code }` → `ToolFailure { code, message: <the error table's sentence>, retryable: false }`. `ToolResult { content }` → `ToolOutput { text: <content as compact JSON, or the result text where this spec gives one>, structured: Some(content) for JSON content, None for a text result }`. `session_ended` / session end → `session_closed(run_id)`.

### `ToolDispatch` — owned and implemented by the MCP Front; the Sandbox consumes it (SA-RECON-01)

```rust
#[async_trait::async_trait]
pub trait ToolDispatch: Send + Sync {
    /// The session's offered tools whose spec is `sandbox_callable`, sorted by server then tool.
    fn session_tools(&self, run_id: RunId) -> Result<Vec<(ToolKey, ToolSpec)>, McpError>;
    /// One call from agent-written code, through the pipeline of a harness call (`via = Sandbox`).
    async fn dispatch(&self, run_id: RunId, key: ToolKey, arguments: serde_json::Value,
                      cancel: tokio_util::sync::CancellationToken) -> Result<ToolCallResult, McpError>;
}
pub struct ToolCallResult { pub is_error: bool, pub text: String, pub structured: Option<serde_json::Value>, pub code: Option<String> }
```

### Journal (`ordin-journal`, JournalAndArtefactStore.md; ruling R3)

```rust
// The concrete `ordin_journal::Journal` (no trait object; reconciliation row 41).
impl Journal {
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
    /// Indexes content the caller has already written; never writes or reads bytes.
    pub async fn record_artefact(&self, new: NewArtefact) -> Result<Artefact, JournalError>;
    pub fn subscribe(&self, sink: std::sync::Arc<dyn EventSink>) -> SubscriptionId;
    pub fn unsubscribe(&self, id: SubscriptionId);
}
pub struct NewJournalEntry { pub kind: String /* ^[a-z][a-z0-9_]{0,63}$ */, pub component: String,
    pub run_id: Option<RunId>, pub crew_run_id: Option<CrewRunId>, pub workspace_id: Option<WorkspaceId>,
    pub summary: String /* 1–300 chars */, pub data: serde_json::Value /* ≤ 64 KiB */ }
pub struct NewArtefact { pub location: ArtefactLocation, pub path: String, pub hash: Sha256Hex,
    pub media_type: String, pub producing_run_id: Option<RunId> }
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    fn name(&self) -> &'static str;
    async fn deliver(&self, event: &JournalEvent);   // kind = EventKind::as_str(); data = the variant's fields
    async fn lagged(&self, next_seq: u64);
}
pub struct JournalEvent { pub backend: uuid::Uuid, pub seq: u64, pub at: Timestamp, pub kind: String, pub data: serde_json::Value }
```

### Sandbox grants — trait owned here, implemented by the program (RulesEngine.md SA-RECON-02)

The Rules Engine does not implement this trait: the capabilities need the package's declared connections and their credentials, which the Rules Engine's `InstanceRuleSet` does not carry. The program implements it from the Package Inspector's declared connections, the organisation profile and Identity and Credentials (ADR-100 item 1 as amended, D3; SA-RECON-05).

```rust
#[async_trait] pub trait SandboxGrants: Send + Sync {
    /// The company systems this agent instance's sandboxed code may call in this workspace:
    /// its package's declared connections, narrowed by the organisation profile and the
    /// agent's own tool policy (ADR-090). Empty = no system. Errors fail closed.
    async fn sandbox_capabilities(&self, agent_instance_id: AgentInstanceId,
                                  workspace_id: WorkspaceId)
        -> Result<Vec<SandboxCapability>, GrantsUnavailable>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxCapability {
    /// The guest's name for it: `api.<name>` / `mcp.<name>`. `^[a-z][a-z0-9_]{0,39}$`; never "ordin_tools".
    pub name: String,
    pub kind: SandboxCapabilityKind,
    /// Write methods (POST, PUT, PATCH, DELETE) allowed; false = GET only.
    pub allow_write: bool,
    /// Private-network destinations (RFC 1918, CGNAT, IPv6 ULA) allowed.
    pub allow_private_network: bool,
    pub credential: SandboxCredential,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxCapabilityKind {
    Rest { host: String, path_allow: Vec<String> /* regexes */, methods: Vec<String> },
    Mcp { server_url: String /* https origin */, tool_allow: Vec<String> },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxCredential {
    /// No credential is sent.
    None,
    /// The person's own token of this kind, forwarded as a bearer token (sandboxd `passthrough`).
    PersonToken { credential_id: CredentialId, kind: CredentialKind },
    /// A key the person entered once for this agent's connection (SA-CRED-01), applied by the broker.
    AgentKey { key_name: String, placement: KeyPlacementSpec },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyPlacementSpec { Header { name: String, scheme: Option<String> }, Query { param: String } }
/// The grants could not be read; the Sandbox fails closed.
pub struct GrantsUnavailable;
```

### Identity and Credentials (`ordin-identity`, IdentityAndCredentials.md; reconciliation R9)

```rust
impl IdentityService {
    pub fn secrets(&self) -> std::sync::Arc<SecretStore>;
    /// A current token for one credential, refreshed when due. Errors: CRED_SIGN_IN_REQUIRED
    /// (permanent, the person must sign in), CRED_KEYCHAIN_* and other retryable codes.
    pub async fn access_token_by_id(&self, credential_id: CredentialId) -> Result<AccessToken, CredError>;
}
impl SecretStore {
    pub async fn get_or_create_random(&self, name: &SecretName, len: usize) -> Result<zeroize::Zeroizing<Vec<u8>>, CredError>;
    /// `Ok(None)` when the person has not entered the key.
    pub async fn agent_key(&self, agent_instance_id: AgentInstanceId, key_name: &str) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, CredError>;
}
pub struct AccessToken { pub token: SecretString, pub expires_at: Option<Timestamp>, pub credential_id: CredentialId }
pub enum CredError { Retryable { code: &'static str }, Permanent { code: &'static str } }   // CodedError: code(), retryable()
```

### Place files — trait owned here, implemented by the program over the File Service and the Agent Folder Manager

The Sandbox's 2B-3 dependency list has no File Service and no Agent Folder Manager. The Sandbox therefore owns this trait (ADR-100 item 1 as amended, D3: `PlaceFiles`) and the program supplies an adapter: reads go to the File Service's grant-checked read, output writes go into the agent's folder that the Agent Folder Manager's `get(agent_instance_id)` names (2B-1 item 3: programs wire modules).

```rust
#[async_trait::async_trait]
pub trait PlaceFiles: Send + Sync {
    /// Reads one file of a place for this agent instance in this workspace. The implementer
    /// refuses a place the agent has no folder grant for (`NotGranted`), a path outside the
    /// place (`NotFound`), and a file over `max_bytes` (`TooLarge`).
    async fn read_for_agent(&self, agent_instance_id: AgentInstanceId, workspace_id: WorkspaceId,
                            place_id: PlaceId, relative_path: &str, max_bytes: u64)
        -> Result<Vec<u8>, PlaceFileError>;
    /// Writes one output file into the agent's own folder at `relative_path` ('/'-separated, no
    /// `..`), replacing an earlier file at that path; returns its SHA-256. `Unreadable` when the
    /// folder cannot be written.
    async fn write_agent_output(&self, agent_instance_id: AgentInstanceId, relative_path: &str, bytes: &[u8])
        -> Result<Sha256Hex, PlaceFileError>;
}
pub enum PlaceFileError { NotGranted, NotFound, TooLarge, Unreadable }
```

### Types consumed from `sandboxd` (referenced, not redefined; 2C §9)

| Type or function | Location |
|---|---|
| `SandboxController::new(policy, interactor, minter, runtime, sessions, creds, issuer, limits)`; `run(req, session) -> Result<RunOutcome, ControllerError>` | `sandbox/sandbox-host/src/controller.rs:128`–`156` |
| `Interactor` (`require_boxed(who, what)`), `CapabilityMinter` (`mint_caps`), `IdTokenSink` (`set_id_token`, `set_access_token`) | `controller.rs:32`, `:53`, `:79` |
| `ControllerError::{CapUnknown, InteractionDenied, RateLimited, WorkDir(WorkDirError)}`, codes `CAP_UNKNOWN`, `INTERACTION_DENIED`, `RATE_LIMITED` | `controller.rs:86`–`105` |
| `RunOutcome::{Run(RunResult), DryRun(DryRunResult)}` | `controller.rs:110` |
| `RunRequest { code, requested_capabilities, timeout_ms, dry_run, workspace_id, run_id, files }` (camelCase on the wire) | `types.rs:167`–`184` |
| `RunResult { stdout, stderr, exit_code, api_calls, truncated, changes }`, `DryRunResult { planned_calls }`, `CallSummary { provider, host, path, method, status }` | `types.rs:187`–`224` |
| `ClientIdentity { principal, client_label }`, `SessionHandle { client, workspace_id }` | `types.rs:10`, `:195` |
| `InteractionRequired::{SignIn{issuer}, Consent{..}, StepUp{..}}`; `InteractionOutcome::{SignedIn{principal,id_token,access_token}, Allowed}`; `InteractionError::{Denied, Unavailable, SignInFailed}` | `types.rs:134`–`149`; `interaction.rs:41`–`59` |
| `Principal { subject, issuer, acr, amr, auth_time }`; `CapabilityHandle { cap_id: [u8;16], capability_id, expires_at }`; `ResolvedCapability` | `types.rs:124`, `:153`, `:79` |
| `UntrustedMcpResult { untrusted, is_error, parts, truncated }`, `UntrustedPart::Json { body }` | `types.rs:278`, `:288` |
| `WorkFile { path, content_base64 }`, `FileChange { path, kind, content_base64 }`, `ChangeKind::{Added, Changed, Removed}`, `WorkLimits` default 1,000 files / 64 MiB total / 16 MiB per file, `WorkDirError` codes | `workdir.rs:28`–`105` |
| `PolicyEngine::load(default_json, signed_override, verify)`, `resolve`, `has_oidc_capability`, `api_key_secret_refs` | `policy.rs:79`, `:235`–`262` |
| `IdentityBroker::new(policy, audit, obo, downstream, creds, max_response_bytes, api_keys)`, `with_mcp_upstream`; traits `CredentialSource`, `ApiKeyStore`, `BrokerCall`; `freeze_api_keys` | `broker.rs:31`–`60`, `:121`–`137`, `:190`, `:239` |
| `SandboxRuntime::new(expected_digest_hex, guest, broker)`, `Limits { fuel, epoch_deadline, max_memory_bytes, max_output_bytes }`, `RUNTIME_LIMIT_EXIT = -2`, `RUNTIME_INSTANTIATE_EXIT = -3`, `RuntimeError::ArtifactMismatch` | `runtime.rs:33`–`57`, `:107`–`123` |
| `SessionManager::new(max_per_session)` | `session.rs:17` |
| `AuditLogger::new(hmac_key, sink)`, trait `AuditSink { fn emit(&self, &AuditEntry) }`, `AuditEntry` (sizes and keyed-HMAC user id, never tokens or bodies) | `audit.rs:10`–`40`; `types.rs:313` |
| `OboClient::new(base)`, `DownstreamClient::new(max_bytes, timeout, allow_plaintext_loopback)`, `McpUpstreamClient::new(..)`, `DEFAULT_CALL_TIMEOUT` = 30 s | `obo.rs:71`; `downstream.rs:19`, `:79`; `mcp_upstream.rs:91` |
| `Config::load(get, resolver)`; trait `SecretResolver { fn resolve(&self, reference) -> Result<Vec<u8>, ConfigError> }` | `config.rs:9`, `:88` |
| Guest SDK globals `api.<name>.get/post/patch/delete(path)` and `mcp.<name>.call_tool(tool, arguments)`; Python code delivered on stdin | `guest/src/main.rs:6`–`9`, `:95`–`120`; `runtime.rs:10` |
| Python guest artefact `guest/pysandbox-guest.wasm` | `sandbox/sandbox-host/src/main.rs:33` |

### Configuration and constants

| Name | Value | Source |
|---|---|---|
| Organisation profile `sandbox.session_idle_minutes` | integer 1–240, default 30 | 2D-2 |
| Calls per run; sandbox runs per agent session | 50; 500 | 2D-4; `config.rs:148`–`149` |
| Response size; guest memory; guest deadline | 1 MiB; 512 MiB; 30 s | 2D-4; `config.rs:157`, `:172`–`173` |
| Concurrent guest executions per program | 4 *(assumed here: four 512 MiB guests stay within 2 GiB)* | — |
| Concurrent work sessions per program | 16 *(assumed here: ADR-045 of `sandboxd` leaves the default to `/spec`)* | `sandbox/docs/design/sandbox-host/09-decisions.md:309` |
| Place input files per call; total bytes of place inputs | 100; 64 MiB (the `WorkLimits` total) *(assumed here)* | — |
| `code` length | 1 byte – 256 KiB *(assumed here)* | — |
| Environment variables per run (feature `extra-guests`) | ≤ 64 names of ≤ 64 chars, values ≤ 4 KiB *(assumed here; `sandboxd` ADR-046 leaves the limits to `/spec`)* | — |
| Output folder in the agent folder | `sandbox-output/<run_id>/<path>` *(chosen here)* | — |
| Work-session folders | `<application data root>/sandbox/sessions/<run_id>/` *(chosen here)* | ADR-072 |
| Audit key secret name (Identity `SecretStore::get_or_create_random`) | `sandbox/audit-hmac-key`, 32 bytes *(chosen here; Identity's `SecretName` pattern)* | 2D-1f `PYS_AUDIT_HMAC_KEY_REF` |
| `ORDIN_LOG_LEVEL` | given in-process as `PYS_LOG_LEVEL` | 2D-1a, 2D-1f |

In-process map given to `sandboxd::config::Config::load` (2D-1f): `PYS_REQUIRE_FIRST_CONNECT_CONSENT=false`, `PYS_POLICY_PATH=<application data root>/sandbox/policy.json` (required by `Config::load`, `config.rs:136`; the file is never written or read, because the manifest is passed to `PolicyEngine::load` as JSON in memory — this settles 2D-1f's *Needs verification*), `PYS_MAX_CALLS_PER_RUN=50`, `PYS_MAX_CALLS_PER_SESSION=500`, `PYS_RESPONSE_MAX_BYTES=1048576`, `PYS_ALLOW_PLAINTEXT_LOOPBACK_EGRESS=false`, `PYS_WASM_MAX_MEMORY_BYTES=536870912`, `PYS_WASM_DEADLINE_SECONDS=30`, `PYS_GUEST_ARTIFACT_DIGEST=<digest compiled in>`, `PYS_AUDIT_HMAC_KEY_REF=ordin:sandbox.audit_hmac_key`, `PYS_LOG_LEVEL=<ORDIN_LOG_LEVEL>`. No other `PYS_*` key is supplied; no process environment variable is read.

## Public Interface

### Crate surface

```rust
// crate ordin-sandbox

pub struct SandboxDeps {
    pub dispatch: std::sync::Arc<dyn ordin_mcp_front::ToolDispatch>,
    /// Implemented by the program (SA-RECON-05).
    pub grants: std::sync::Arc<dyn SandboxGrants>,
    pub identity: std::sync::Arc<ordin_identity::IdentityService>,
    pub journal: ordin_journal::Journal,
    pub place_files: std::sync::Arc<dyn PlaceFiles>,
    /// Profile `sandbox.session_idle_minutes`, already clamped to 1–240 by the Organisation Profile.
    pub session_idle: std::time::Duration,
    pub data_root: std::path::PathBuf,
    pub log_level: String,
}

pub struct Sandbox { /* private */ }

impl Sandbox {
    /// Builds the shared parts, verifies the guest digest, deletes leftover work-session
    /// folders and starts the idle sweeper and the run-end listener. Never fails the program's
    /// start: on a start-up fault its provider offers no tools and `status()` reports
    /// `SandboxStatus::Unavailable`.
    pub async fn start(deps: SandboxDeps) -> std::sync::Arc<Sandbox>;

    /// The `sandbox` tool provider, for the program to register with the MCP Front before the
    /// MCP Front's `start` (SA-RECON-02). Every call returns the same provider.
    pub fn tool_provider(&self) -> std::sync::Arc<dyn ordin_mcp_front::ToolProvider>;

    /// For the start-up report (G9) and Diagnostics.
    pub fn status(&self) -> SandboxStatus;

    /// Ends one agent session's work session now (called on the run's end; also by the sweeper).
    pub async fn end_work_session(&self, run_id: RunId, reason: SessionEndReason);

    /// Stops accepting calls, waits up to 10 s (SA-STOP-01) for runs in progress, deletes every
    /// work-session folder.
    pub async fn shutdown(&self);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxStatus {
    Ready { guests: Vec<GuestKind> },
    /// `code` is one of SBX_GUEST_DIGEST_MISMATCH, SBX_CONFIG_INVALID, SBX_AUDIT_KEY_UNAVAILABLE.
    Unavailable { code: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestKind { Python, Shell, JavaScript }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEndReason { RunEnded, Idle, Shutdown }

/// Registered with the MCP Front by the program; implements `ToolProvider` for server `sandbox`.
pub struct SandboxToolProvider { /* private */ }
```

**Called by.**

| Public item | Called by |
|---|---|
| `Sandbox::start`, `tool_provider`, `shutdown` | the desktop program and the personal-server program (OrdinDesktop, OrdinServer); the program passes `tool_provider()` to `McpFront::register` |
| `Sandbox::status` | the programs' `SupervisorHost::startup_items` (the Session Supervisor's start-up report, G9) and Diagnostics (SA-RECON-04) |
| `Sandbox::end_work_session` | the Sandbox's own run-end listener and idle sweeper (section F); the programs may call it at run end |
| tools `python_sandbox`, `shell_sandbox`, `javascript_sandbox` | the MCP Front, for harness sessions only (never from the `ordin_tools` module, SA-RECON-03) |
| `PlaceFiles` (owned here, implemented) | implemented by the desktop and personal-server programs over the File Service (ADR-100 item 1 as amended, D3) |

### MCP tools (server `sandbox`, SA-NAME-04)

| Tool | Offered when | Guest | `sandbox_callable` |
|---|---|---|---|
| `python_sandbox` | `SandboxStatus::Ready` and the caller is `Ordin`, `Agent` or `CrewMember` | Python (RustPython, `pysandbox-guest.wasm`) | false (SA-RECON-03) |
| `shell_sandbox` *(chosen here; the name `sandboxd`'s own HLD recommends, `sandbox/docs/design/sandbox-host/08-interfaces.md:17`)* | as above, and the crate is built with feature `extra-guests` | shell guest (`sandboxd` ADR-043, not implemented) | false |
| `javascript_sandbox` *(chosen here)* | as above, and feature `extra-guests` | JavaScript guest (`sandboxd` ADR-043, not implemented) | false |

`argument_form` (one line, ≤ 300 characters), the same for the three tools with the tool's name: `python_sandbox(code: string, requestedCapabilities?: [name], dryRun?: bool, files?: [{path, contentBase64}], inputs?: [{placeId, path, as?}], workSession?: bool)`. `plain` (start-up report, ≤ 200 characters): `python_sandbox` — "Can run Python code in a sandbox with no network and no files of its own."; `shell_sandbox` — "Can run shell scripts in a sandbox with no network."; `javascript_sandbox` — "Can run JavaScript in a sandbox with no network."

The three tools take the same arguments. Argument names keep `sandboxd`'s own camelCase contract (2C §9: "Its own serde convention, kept as is"; existing package text is written against `python_sandbox`'s schema). Unknown fields are refused.

```json
{
  "type": "object",
  "additionalProperties": false,
  "required": ["code"],
  "properties": {
    "code":                  { "type": "string", "minLength": 1, "maxLength": 262144 },
    "requestedCapabilities": { "type": "array", "items": { "type": "string" }, "maxItems": 50 },
    "dryRun":                { "type": "boolean" },
    "files": {
      "type": "array", "maxItems": 1000,
      "items": { "type": "object", "additionalProperties": false, "required": ["path", "contentBase64"],
                 "properties": { "path": { "type": "string" }, "contentBase64": { "type": "string" } } }
    },
    "inputs": {
      "type": "array", "maxItems": 100,
      "items": { "type": "object", "additionalProperties": false, "required": ["placeId", "path"],
                 "properties": { "placeId": { "type": "string", "format": "uuid" },
                                 "path":    { "type": "string" },
                                 "as":      { "type": "string" } } }
    },
    "workSession":           { "type": "boolean" },
    "environment":           { "type": "object", "additionalProperties": { "type": "string" } }
  }
}
```

- `inputs` *(chosen here)*: files the core reads from a place the agent may read (ADR-031 "Inputs the agent names from granted places are passed in by the core") and writes at `as` (default: `path`'s last component) under `/work`.
- `workSession` *(chosen here)*: `true` keeps `/work` for the rest of this agent session (ADR-040 F5).
- `environment` *(chosen here)*: non-secret variables for the run (ADR-040 F7); accepted only with feature `extra-guests`.

Descriptions (one line each, ADR-062, C-38): `python_sandbox` — "Run Python in the sandbox; call granted systems with api.<name>, ordin tools with ordin_tools.<tool>; files in /work."; `shell_sandbox` — "Run a POSIX shell script in the sandbox (a bash subset, no native programs); files in /work."; `javascript_sandbox` — "Run JavaScript in the sandbox without Node modules; call ordin tools with ordin_tools.<tool>; files in /work."

Result content (`ToolOutput.structured`, with the same object as compact JSON in `ToolOutput.text`; camelCase like the crate's `RunResult`):

```json
{
  "stdout": "string", "stderr": "string", "exitCode": 0, "truncated": false,
  "apiCalls": [{ "provider": "", "host": "", "path": "/v1/quotes", "method": "GET", "status": 200 }],
  "changes": [{ "path": "summary.csv", "kind": "added", "savedAs": "sandbox-output/<run_id>/summary.csv" }],
  "sessionChanges": [{ "path": "summary.csv", "kind": "added" }],
  "plannedCalls": [ ]
}
```
`changes` lists this run's record without file content; `savedAs` is the path relative to the agent's folder, present for `added` and `changed`. `sessionChanges` is present only for a work session (cumulative since the session began). A dry run returns only `plannedCalls`.

### The `ordin_tools` module inside the room (SA-NAME-06)

For a Python run (and a JavaScript run under `extra-guests`) the wrapper prepends a prelude that binds the global `ordin_tools`, whose attribute for each tool of `ToolDispatch::session_tools` (excluding the `sandbox` server's own tools) calls `mcp.ordin_tools.call_tool("<tool>", kwargs)`. Each call returns `sandboxd`'s untrusted envelope `{"untrusted": true, "is_error": bool, "parts": [{"kind": "json", "body": <tool result>}], "truncated": bool}` (`types.rs:278`–`309`). A shell run gets no module.

Python prelude (exactly, `<tool>` repeated per tool). A tool name is used as the attribute when it matches `^[A-Za-z][A-Za-z0-9_]{0,63}$` — the MCP Front's tool-name pattern without the hyphen, which no Python or JavaScript identifier may hold; a name with a hyphen is skipped and logged `sandbox.module_tool_skipped`:

```python
class _OrdinTools: pass
ordin_tools = _OrdinTools()
ordin_tools.<tool> = lambda **kw: mcp.ordin_tools.call_tool("<tool>", kw)
```

### Example

Call from a Claude session (`mcp__sandbox__python_sandbox`), after the Rules Engine allowed it:

```json
{
  "code": "import json\nrows=[l.split(',') for l in open('/work/rates.csv').read().splitlines()[1:]]\nopen('/work/summary.csv','w').write('supplier,avg\\n'+'\\n'.join(r[0]+','+r[1] for r in rows))\nprint(len(rows))",
  "inputs": [{ "placeId": "0192a0f2-2222-7333-8444-555566667777", "path": "Suppliers/Q3/rates.csv" }],
  "workSession": true
}
```

Result:

```json
{
  "stdout": "42\n", "stderr": "", "exitCode": 0, "truncated": false, "apiCalls": [],
  "changes": [{ "path": "summary.csv", "kind": "added", "savedAs": "sandbox-output/0192a1c2-3344-7556-8677-8899aabbccdd/summary.csv" }],
  "sessionChanges": [{ "path": "summary.csv", "kind": "added" }]
}
```

Refusal (place not granted to this agent):

```json
{ "isError": true, "content": [{ "type": "text", "text": "The agent may not read that place in this workspace; ask the person to grant it, or pass the file's content in files." }] }
```
(journalled code `SBX_INPUT_NOT_GRANTED`).

## Internal Logic

### A. Start (`Sandbox::start`)

1. Fetch the audit key: `identity.secrets().get_or_create_random(&SecretName::new("sandbox/audit-hmac-key")?, 32)`. On error set `Unavailable { code: "SBX_AUDIT_KEY_UNAVAILABLE" }`, log `sandbox.unavailable` (`code`), go to step 7.
2. Build `PrefilledResolver` *(chosen here)*: a `sandboxd::config::SecretResolver` (synchronous, `config.rs:9`) over an in-memory map `{"ordin:sandbox.audit_hmac_key" → key}`; any other reference returns `ConfigError { code: "CFG_SECRET_UNRESOLVED" }`. The map is filled before the call because `SecretResolver` cannot await.
3. Call `Config::load(&|k| map.get(k).cloned(), &resolver)` with the in-process map of *Shared Context*. On `ConfigError` set `Unavailable { code: "SBX_CONFIG_INVALID" }`, log `sandbox.unavailable` with `field`, go to step 7. The resulting `Config.credential_mode` is `Mock` (no OTLP endpoint, `config.rs:181`–`185`); no code of the crate reads it today (2D *Former gaps, now settled* item 6).
4. Verify and compile the Python guest once: `SandboxRuntime::new(digest, GUEST_PY, probe_broker)` where `GUEST_PY = include_bytes!("<path to>/sandbox/sandbox-host/guest/pysandbox-guest.wasm")` and the digest is computed by `build.rs` with `sha2` and compiled in as `env!("ORDIN_SANDBOX_PY_DIGEST")`. `RuntimeError::ArtifactMismatch` or `GuestInvalid` → `Unavailable { code: "SBX_GUEST_DIGEST_MISMATCH" }`, log, go to step 7. With `extra-guests`, repeat for the shell and JavaScript guests; a failure there removes only that `GuestKind` and logs `sandbox.guest_unavailable` (`guest`).
5. Delete every directory under `<data_root>/sandbox/sessions/` (leftovers of a process that did not stop cleanly); create the directory with mode `0700` on macOS and Linux. Log `sandbox.sessions_cleared` (`count`).
6. Spawn: (a) the run-end listener (section F); (b) the idle sweeper, every 60 s (section F).
7. Build `SandboxToolProvider` (returned by `tool_provider()`; the program registers it, SA-RECON-02). Log `sandbox.started` (`status`, `guests`).

### B. Tools offered (`ToolProvider::servers` and `offer`)

1. `provider_id()` = `"sandbox"`. `servers()` = one `ServerSpec { name: "sandbox", tools }` with `python_sandbox` plus, under `extra-guests`, each ready further guest's tool, each with the schema, description, `argument_form`, `plain` and `sandbox_callable = false` of *Public Interface*; empty `tools` when `status` is `Unavailable`.
2. `offer(grant)`: `status` `Unavailable`, or `grant.caller` is `HostedAgent` → `Ok([])` (the server is not offered). Otherwise the keys of step 1.
3. `check_arguments(key, arguments)`: step C.2 (typed deserialisation and the field rules); a failure → `ArgumentProblem { at: <the field's JSON Pointer>, reason }` (rule names `SBX_ARGS_INVALID`, `SBX_ENVIRONMENT_INVALID` are logged; the model sees the MCP Front's `MCP_ARGUMENTS_INVALID`). `environment` without `extra-guests` is a refusal of `call`, not an argument problem.
4. `session_closed(run_id)`: `end_work_session(run_id, RunEnded)`.

### C. A call (`ToolProvider::call`)

Every failure below is returned as `ToolFailure { code, message: <the Error Table's sentence>, retryable }` (the earlier `Refused`/`InvalidArguments` are `retryable: false`, `Unavailable` keeps its flag).

1. **Tool.** `key.tool` not one of the offered names → `SBX_TOOL_UNKNOWN`. `ctx.caller` is `HostedAgent` → `SBX_CALLER_NOT_SUPPORTED`. Let `agent = ctx.agent_instance_id`, `workspace = ctx.workspace_id`, `run_id = ctx.run_id`.
2. **Arguments** (already checked by `check_arguments`; repeated here only to build the typed value). Deserialise into `SandboxArgs` *(chosen here; `#[serde(rename_all = "camelCase", deny_unknown_fields)]` mirroring the schema)*. Failure, empty `code`, `code` over 256 KiB, more than 50 `requestedCapabilities` or 100 `inputs` → `InvalidArguments { SBX_ARGS_INVALID }` naming the field. `requestedCapabilities` containing `ordin_tools` → `InvalidArguments { SBX_ARGS_INVALID }` (reserved). `environment` non-empty without `extra-guests` → `Refused { SBX_ENVIRONMENT_UNSUPPORTED }`. With `extra-guests`, each name must match `^[A-Za-z_][A-Za-z0-9_]{0,63}$`, not be `PATH`, `HOME`, `PWD` or start with `WASI_`, value ≤ 4 KiB and not match the redaction pattern of `controller.rs:350` or `gh[pousr]_[A-Za-z0-9]{20,}` or `sk-[A-Za-z0-9]{20,}`; any failure → `InvalidArguments { SBX_ENVIRONMENT_INVALID }` naming the variable (never its value).
3. **Concurrency.** If `workSession` and the run's work session is executing → `Refused { SBX_SESSION_BUSY }`. If `workSession`, no session exists for `run_id` and 16 exist → `Unavailable { SBX_SESSIONS_FULL, retryable: true }`. Acquire one permit of the 4-permit guest semaphore (wait at most 30 s, else `Unavailable { SBX_BUSY, retryable: true }`).
4. **Controller for the agent.** Get or build `AgentController` for `(agent, workspace)` (section D). Errors from D are returned as they are.
5. **Capability check before any input is read.** Every `requestedCapabilities` name must be in the agent's manifest, else `Refused { SBX_CAPABILITY_UNKNOWN }` naming it (the controller would return `CAP_UNKNOWN`, `controller.rs:161`–`164`; checking first avoids reading files for a refused call). If the requested set holds `PersonToken` capabilities with two different `credential_id`s → `Refused { SBX_MIXED_CREDENTIALS }` (the crate installs one access token per run, `controller.rs:242`–`243`).
6. **Inputs.** Build the `WorkFile` list:
   a. work session existing: every file of `<data_root>/sandbox/sessions/<run_id>/` as `WorkFile`s (symbolic links skipped);
   b. each `inputs[i]`: `place_files.read_for_agent(agent, workspace, placeId, path, 16 MiB)`; `NotGranted` → `Refused { SBX_INPUT_NOT_GRANTED }`; `NotFound` → `InvalidArguments { SBX_INPUT_NOT_FOUND }`; `TooLarge` → `InvalidArguments { SBX_INPUTS_TOO_LARGE }`; `Unreadable` → `Unavailable { SBX_INPUT_UNREADABLE, retryable: true }`. Target path is `as` or the last component of `path`;
   c. each `files[i]` as given.
   Later sources override earlier ones for the same target path in the order a, b, c; two entries of the same source naming one path → `InvalidArguments { SBX_FILE_INVALID }`. Total bytes over 64 MiB or over 1,000 files → `InvalidArguments { SBX_INPUTS_TOO_LARGE }`. If no source gave a file and `workSession` is false, `files` stays `None` (no `/work`).
7. **Module prelude.** If the guest is Python or JavaScript: `dispatch.session_tools(run_id)` (only sandbox-callable tools, so none of the `sandbox` server's own); an `Err` → no module (logged `sandbox.module_tool_skipped` with reason `session_unknown`). Drop any name failing the regex (log `sandbox.module_tool_skipped`), and on a duplicate name keep the first server's tool. If at least one tool remains, prepend the prelude and add `ordin_tools` to the capabilities passed to the controller. Record the prelude's line count `L`.
8. **Run context.** Insert into the run-context map, keyed by `client_label = run_id` string: `{ person_credential: Option<CredentialId>, run_id, cancel: ctx.cancel, tools: the step-7 list, module_calls: 0 }`.
9. **Run.** Build `RunRequest { code: prelude + code, requested_capabilities, timeout_ms: None, dry_run, workspace_id: workspace uuid string, run_id: Some(run_id string), files }` and `SessionHandle { client: ClientIdentity { principal: agent uuid string, client_label: run_id string }, workspace_id: workspace uuid string }`; call `AgentController.controller.run(req, handle)` inside `tokio::time::timeout(35 s)` (30 s guest deadline plus 5 s margin). Timeout → `Unavailable { SBX_RUN_TIMEOUT, retryable: false }`.
10. **Controller errors.** `CapUnknown` → `Refused { SBX_CAPABILITY_UNKNOWN }`; `InteractionDenied` → read the run context's recorded sign-in fault: a permanent `CredError` (`CRED_SIGN_IN_REQUIRED` and other permanent codes) → `Refused { SBX_SIGN_IN_NEEDED }`, a retryable `CredError` → `Unavailable { SBX_CREDENTIAL_UNAVAILABLE, retryable: true }`, none recorded → `Refused { SBX_INTERACTION_REFUSED }`; `RateLimited` → `Refused { SBX_RUN_BUDGET_EXHAUSTED }`; `WorkDir(e)` with `e.code() == "VAL_ERR"` → `InvalidArguments { SBX_FILE_INVALID }` naming the path carried by `InvalidPath`/`DuplicatePath`/`InvalidContent` (`workdir.rs:73`–`86`), `InputsTooLarge` → `InvalidArguments { SBX_INPUTS_TOO_LARGE }`, `Io` → `Internal { SBX_INTERNAL }`.
11. **Result.** For `RunOutcome::DryRun` return `{ "plannedCalls": … }`. For `RunOutcome::Run(r)`: in `r.stderr`, replace each `line N` with `line N−L` where `N > L` (prelude offset); `exit_code == -2` (`RUNTIME_LIMIT_EXIT`) is returned as a result with `exitCode: -2` and `stderr` "RUNTIME_LIMIT", not as an error; `exit_code == -3` → `Internal { SBX_GUEST_START_FAILED }`.
12. **Outputs as artefacts.** For each `FileChange` of kind `added` or `changed` (content present): `place_files.write_agent_output(agent, "sandbox-output/<run_id>/<path>", bytes)` → `hash`, then `journal.record_artefact(NewArtefact { location: AgentFolder { agent_instance_id: agent }, path: "sandbox-output/<run_id>/<path>", hash, media_type: from the extension (`csv` → `text/csv`, `json` → `application/json`, `txt`/`md` → `text/plain`/`text/markdown`, `pdf` → `application/pdf`, `png` → `image/png`, otherwise `application/octet-stream`), producing_run_id: Some(run_id) })`. Any failure → `Unavailable { SBX_OUTPUT_SAVE_FAILED, retryable: true }` after logging which paths were saved; the work session (step 13) is still updated so no output is lost.
13. **Work session.** If `workSession`: create the folder on first use (mode `0700`) and record the baseline (path → SHA-256 of the files written in step 6 on that first run); apply this run's changes (write added/changed, delete removed; reject a path with `..` or an absolute path, which the crate already prevents, `workdir.rs:10`–`13`); set `last_used = now`; compute `sessionChanges` against the baseline.
14. **Journal.** `journal.append_entry(NewJournalEntry { kind: "sandbox_run", component: "sandbox", run_id: Some(run_id), crew_run_id: None, workspace_id: Some(workspace), summary: "Sandbox run finished with exit code <n>.", data: { tool, agent_instance_id, code_sha256, exit_code, duration_ms, api_calls: count, module_calls, files_in: count, files_out: count, truncated, work_session, environment_names } })` — never the code, the files, stdout or stderr. A journal failure is logged (`sandbox.journal_failed`) and does not fail the call.
15. Remove the run context, release the permit, return `ToolOutput { text: <the result object as compact JSON>, structured: Some(<the result object>) }`. Log `sandbox.run_finished`.

### D. Building an agent's controller

1. Cache key `(agent, workspace)`; entries unused for 30 minutes are dropped by the sweeper *(assumed here)*. A `grants_changed` signal is not available, so each call re-reads the grants (step 2) and rebuilds when their digest differs from the cached one.
2. `grants.sandbox_capabilities(agent, workspace)`; `Err(GrantsUnavailable)` → `Unavailable { SBX_RULES_UNAVAILABLE, retryable: true }` (fail closed: no run without the grants).
3. Render the manifest JSON (`{"capabilities": { "<name>": {...} } }`, field names of `policy.rs:11`–`57`): `Rest` → `kind:"rest", host, pathAllow, methods`; `Mcp` → `kind:"mcp", serverUrl, toolAllow`; `allowWrite`, `allowPrivateNetwork`; credential `None` → `authMode:"none"`; `PersonToken { kind }` → `authMode:"passthrough", provider:"<kind in snake_case>"`; `AgentKey { key_name, placement }` → `authMode:"api_key", secretRef:"ordin-key:<key_name>", keyPlacement`. `requireStepUpAuth` is always `false`; `exchange` is never produced (ADR-031: the off-laptop broker is not used). Add the reserved capability `ordin_tools` → `kind:"mcp", serverUrl:"https://ordin-tools.invalid", toolAllow:[every module tool name], authMode:"none"`. A grant named `ordin_tools` is dropped and logged.
4. `PolicyEngine::load(json, None, &|_, _| false)`; `WireError` → `Internal { SBX_POLICY_INVALID }`, log `sandbox.policy_invalid` with the agent id (not the manifest).
5. API keys: for each `api_key_secret_refs()` entry, `identity.secrets().agent_key(agent, key_name)`; then `freeze_api_keys` with a `PrefilledResolver` over those bytes. `Ok(None)` → `Refused { SBX_KEY_MISSING }` naming `key_name`; a retryable `CredError` → `Unavailable { SBX_CREDENTIAL_UNAVAILABLE, retryable: true }`.
6. Build `IdentityBroker::new(policy, audit, OboClient::new(String::new()), downstream, creds, 1_048_576, api_keys).with_mcp_upstream(upstream)` where `audit = AuditLogger::new(audit_key, JournalAuditSink)`, `downstream = DownstreamClient::new(1_048_576, DEFAULT_CALL_TIMEOUT, false)`, `upstream = McpUpstreamClient::new(1_048_576, DEFAULT_CALL_TIMEOUT, false)` and `creds` is the controller's `RunCredentials` (holds the run's access token in memory; implements `CredentialSource` and `IdTokenSink`).
7. Wrap: `OrdinMinter` implements `CapabilityMinter` — for `ordin_tools` it mints a capability reference of 16 random bytes (`getrandom`), stores `reference → run_id` with expiry 300 s (the crate's TTL, `broker.rs:26`); every other capability is delegated to the `IdentityBroker`. `OrdinBroker` implements `BrokerCall` — `call_tool_boxed` for an `ordin_tools` reference goes to section E; all other calls go to the `IdentityBroker`.
8. Build the runtime: `SandboxRuntime::new(digest, GUEST_PY, ordin_broker)` (one compile per agent controller — change SH-1 of *Changes required in `sandbox/sandbox-host`* (not a gap) removes this), the `SessionManager::new(500)`, and `SandboxController::new(policy, OrdinInteractor, ordin_minter, runtime, sessions, run_credentials, String::new(), Limits { fuel: Some(u64::MAX), epoch_deadline: 30 s, max_memory_bytes: 536_870_912, max_output_bytes: 1_048_576 })` — the values `main.rs:424`–`429` derives from the same configuration.

`OrdinInteractor` (implements `Interactor`, I-9): `SignIn` → look up the run context by `who.client_label`; with a `person_credential`, `identity.access_token_by_id(id)`: success → `SignedIn { principal: Principal { subject: "ordin:person", issuer: "", acr: None, amr: vec![], auth_time: None }, id_token: String::new(), access_token: token.token.expose().to_owned() }`; failure → record the `CredError` in the run context, return `Err(SignInFailed)`. `Consent` → `Ok(Allowed)`: the Rules Engine already granted the capability (it is in the manifest) and allowed the call. `StepUp` → `Err(Denied)` (never requested: `requireStepUpAuth` is false). Nothing is shown to the person by the Sandbox.

`JournalAuditSink` (implements `AuditSink`): appends `NewJournalEntry { kind: "sandbox_broker_call", component: "sandbox", run_id: the ordin run of `entry.run_id`'s mapping when known, workspace_id, crew_run_id: None, summary: "Sandboxed code called a company system.", data: { capability_id, provider, method, host, status_code, request_bytes, response_bytes, duration_ms, user_hmac } }` without `path` *(chosen here: a path can carry record identifiers)*. `emit` is synchronous; it pushes onto an unbounded channel drained by a task that calls `journal.append_entry`.

### E. A module call from inside the room

1. Look up `reference → run_id → run context`; unknown or expired reference → return `UntrustedMcpResult { untrusted: true, is_error: true, parts: [Json { body: {"code":"SBX_MODULE_HANDLE_INVALID"} }], truncated: false }`.
2. Increment `module_calls`; above 50 → `is_error: true` with `SBX_RUN_BUDGET_EXHAUSTED` (the crate's per-run call budget, applied to module calls too).
3. Find the tool's server from the run's `session_tools` list; parse `arguments` as JSON (failure → `is_error` with `SBX_ARGS_INVALID`).
4. `dispatch.dispatch(run_id, ToolKey { server, tool }, arguments, context.cancel.child_token())` — the Rules Engine decides as for a harness call (ADR-062 "go through the same Rules Engine decision").
5. `Ok(ToolCallResult { is_error: false, text, structured, .. })` → `UntrustedMcpResult { untrusted: true, is_error: false, parts: [UntrustedPart::Json { body: serde_json::to_vec(&(structured or {"text": text})) }], truncated: body > 1 MiB }` (body cut to 1 MiB when truncated). `Ok(ToolCallResult { is_error: true, text, code, .. })` → `is_error: true`, `Json { body: {"code": code, "reason": text} }`. `Err(McpError)` (session unknown) → `is_error: true`, `Json { body: {"code": e.code()} }`.

### F. Work-session end and idle sweep

1. Run-end listener: an `EventSink` (name `sandbox_run_end`) registered with `journal.subscribe`; `deliver` acts on `kind == "run_state_changed"` whose `data.state` is a terminal `RunState`: `end_work_session(data.run_id, RunEnded)`. `lagged(next_seq)` runs the sweep immediately. The MCP Front's `session_closed(run_id)` also ends the work session.
2. Sweeper, every 60 s: each work session with `now − last_used ≥ session_idle` → `end_work_session(run_id, Idle)`; controllers unused for 30 minutes are dropped.
3. `end_work_session`: wait for an executing run of that session to finish (at most 35 s), delete the folder recursively, remove the entry, log `sandbox.session_deleted` (`run_id`, `reason`, `files`, `bytes`). A delete failure is logged at `warn` (`sandbox.session_delete_failed`) and retried by the next sweep.

## Data Model

N/A — the Sandbox owns no table (02:383 "Owns: nothing durable"). Work-session folders under `<application data root>/sandbox/sessions/` are temporary (04:413) and deleted at session end, after the idle limit, at shutdown and at the next start. Run records and broker-call records are Journal entries written through `Journal::append_entry`; output bytes are written into the agent's folder through `PlaceFiles::write_agent_output` and indexed with `Journal::record_artefact`.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Tool name not offered | `ToolFailure` (`retryable: false`) | `SBX_TOOL_UNKNOWN` | "Use python_sandbox; that tool is not offered here." |
| Hosted-agent caller | `ToolFailure` (`retryable: false`) | `SBX_CALLER_NOT_SUPPORTED` | "The sandbox is not offered to hosted agents." |
| Arguments malformed, empty or oversize `code`, too many entries, reserved `ordin_tools` requested | `ArgumentProblem` from `check_arguments` (the model sees `MCP_ARGUMENTS_INVALID`) | `SBX_ARGS_INVALID` | "`<field>` is not valid: <rule>." |
| `environment` given without the extra guests | `ToolFailure` (`retryable: false`) | `SBX_ENVIRONMENT_UNSUPPORTED` | "Environment variables are not available in this version; put the values in the code." |
| Environment variable name reserved or invalid, or value looks like a secret | `ArgumentProblem` from `check_arguments` (the model sees `MCP_ARGUMENTS_INVALID`) | `SBX_ENVIRONMENT_INVALID` | "Variable `<name>` is refused: names are plain identifiers and values may not hold secrets." |
| Work session already running a call | `ToolFailure` (`retryable: true`) | `SBX_SESSION_BUSY` | "The work folder is in use by another call; wait for it to finish." |
| 16 work sessions open | `ToolFailure` (`retryable: true`) | `SBX_SESSIONS_FULL` | "Too many work folders are open; try again shortly or run without workSession." |
| No guest slot within 30 s | `ToolFailure` (`retryable: true`) | `SBX_BUSY` | "The sandbox is busy; try again shortly." |
| Rules Engine grants unreadable | `ToolFailure` (`retryable: true`) | `SBX_RULES_UNAVAILABLE` | "The sandbox cannot check which systems this agent may use; try again." |
| Manifest rendered from grants refused by `PolicyEngine::load` | `ToolFailure` (`retryable: false`) | `SBX_POLICY_INVALID` | (text) "The sandbox could not prepare this agent's systems." |
| Requested capability not granted | `ToolFailure` (`retryable: false`) | `SBX_CAPABILITY_UNKNOWN` | "`<name>` is not a system this agent may call; granted: <names>." |
| Two person credentials in one run | `ToolFailure` (`retryable: false`) | `SBX_MIXED_CREDENTIALS` | "Call systems that use different sign-ins in separate runs." |
| Agent key not entered | `ToolFailure` (`retryable: false`) | `SBX_KEY_MISSING` | "The key `<key_name>` for this agent has not been entered." |
| Input place not granted | `ToolFailure` (`retryable: false`) | `SBX_INPUT_NOT_GRANTED` | "The agent may not read that place in this workspace; ask the person to grant it, or pass the file's content in files." |
| Input file not found | `ToolFailure` (`retryable: false`) | `SBX_INPUT_NOT_FOUND` | "`<path>` was not found in that place." |
| Inputs over 1,000 files, 64 MiB, or one file over 16 MiB | `ToolFailure` (`retryable: false`) | `SBX_INPUTS_TOO_LARGE` | "Inputs exceed the sandbox limits (1,000 files, 64 MiB, 16 MiB per file)." |
| Input file unreadable | `ToolFailure` (`retryable: true`) | `SBX_INPUT_UNREADABLE` | "`<path>` could not be read; try again." |
| Bad, duplicate or undecodable `files` path (`VAL_ERR` from `WorkDir`) | `ToolFailure` (`retryable: false`) | `SBX_FILE_INVALID` | "`<path>` is not a valid file entry: relative path, no `..`, base64 content." |
| Person's sign-in missing or expired | `ToolFailure` (`retryable: false` until the person signs in) | `SBX_SIGN_IN_NEEDED` | "The person needs to sign in to <system> again before this can run." |
| Keychain or secret store unavailable | `ToolFailure` (`retryable: true`) | `SBX_CREDENTIAL_UNAVAILABLE` | "The sign-in store is not available; try again." |
| Controller refused an interaction for another reason | `ToolFailure` (`retryable: false`) | `SBX_INTERACTION_REFUSED` | "The sandbox could not get permission for that system." |
| Per-run broker or module calls over 50, or sandbox runs over 500 in the session | `ToolFailure` (`retryable: false`) | `SBX_RUN_BUDGET_EXHAUSTED` | "This session has used its sandbox call allowance." |
| Run did not return within 35 s | `ToolFailure` (`retryable: false`) | `SBX_RUN_TIMEOUT` | "The code did not finish within 30 seconds." |
| Guest could not start (`exit_code -3`) | `ToolFailure` (`retryable: false`) | `SBX_GUEST_START_FAILED` | (text) "The sandbox could not start the code." |
| Host fault preparing the folder (`WorkDirError::Io`) | `ToolFailure` (`retryable: true`) | `SBX_INTERNAL` | (text) "The sandbox failed; try again." |
| Output artefact not saved | `ToolFailure` (`retryable: true`) | `SBX_OUTPUT_SAVE_FAILED` | "The results could not be saved to the agent's folder; run it again." |
| Module call with an unknown or expired capability reference (inside the room) | envelope `is_error: true` | `SBX_MODULE_HANDLE_INVALID` | `{"code":"SBX_MODULE_HANDLE_INVALID"}` |
| Start-up: audit key unavailable | `SandboxStatus::Unavailable` (no tools offered) | `SBX_AUDIT_KEY_UNAVAILABLE` | — (start-up report: the sandbox is not available) |
| Start-up: in-process configuration refused | `SandboxStatus::Unavailable` | `SBX_CONFIG_INVALID` | — |
| Start-up: guest digest mismatch or guest invalid | `SandboxStatus::Unavailable` | `SBX_GUEST_DIGEST_MISMATCH` | — |

A run whose code fails or hits a resource limit is not an error: it returns its `exitCode` (`-2` for `RUNTIME_LIMIT`) and `stderr` to the agent. The person never sees a code (07); the Sandbox owns no words shown to the person.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Sandbox

  Background:
    Given the Sandbox started with status Ready and guests [python]
    And agent instance "Supplier analyst" in workspace "Q3 supplier review" has no sandbox capabilities

  Scenario: Happy path — Python with a place input returns an artefact
    Given the agent has a read grant on place "Suppliers" holding "Q3/rates.csv" with 43 lines
    When the session calls python_sandbox with inputs [{placeId: Suppliers, path: "Q3/rates.csv"}] and code that writes /work/summary.csv
    Then the result has exitCode 0 and changes [{path: "summary.csv", kind: "added", savedAs: "sandbox-output/<run_id>/summary.csv"}]
    And an artefact exists at AgentFolder "Supplier analyst" path "sandbox-output/<run_id>/summary.csv" version 1
    And the journal holds one "sandbox_run" entry with code_sha256 and without the code text

  Scenario: Edge — work session keeps files between calls and ends with the run
    When the session calls python_sandbox with workSession true and code writing /work/a.txt
    And the session calls python_sandbox with workSession true and code reading /work/a.txt
    Then the second result's stdout holds the content of a.txt
    And the second result's sessionChanges is [{path: "a.txt", kind: "added"}]
    When the run reaches state completed
    Then the folder <data root>/sandbox/sessions/<run_id>/ no longer exists

  Scenario: Edge — idle work session is deleted after the profile limit
    Given the profile sets sandbox.session_idle_minutes to 1
    And a work session was last used 61 seconds ago
    When the sweeper runs
    Then the session folder is deleted and "sandbox.session_deleted" is logged with reason "idle"

  Scenario: Edge — second call on a busy work session
    Given a python_sandbox call with workSession true is executing for run R
    When another python_sandbox call with workSession true arrives for run R
    Then it fails with SBX_SESSION_BUSY

  Scenario: Edge — runtime limit is a result, not an error
    When the session calls python_sandbox with code "while True: pass"
    Then the result has exitCode -2 and stderr "RUNTIME_LIMIT"

  Scenario: Edge — ordin_tools call goes through the Rules Engine
    Given the session may call email_search on server "mail"
    When the code calls ordin_tools.email_search(query="Kestrel")
    Then ToolDispatch.dispatch is called with server "mail", tool "email_search" and {"query":"Kestrel"}
    And the code receives an envelope with untrusted true and is_error false

  Scenario: Edge — a brokered call uses the person's token without exposing it
    Given the agent has capability "tickets" Rest host "tickets.example.com" with PersonToken Microsoft
    And Identity returns access token "eyJhbGciOi.x.y" for that credential
    When the code calls api.tickets.get("/v1/open") and prints the response
    Then the outbound request carries "Authorization: Bearer eyJhbGciOi.x.y"
    And stdout does not contain "eyJhbGciOi.x.y"
    And the journal holds a "sandbox_broker_call" entry with no path and no token

  Scenario: Error — requested capability not granted
    When the session calls python_sandbox with requestedCapabilities ["crm"]
    Then it fails with SBX_CAPABILITY_UNKNOWN and no input file is read

  Scenario: Error — place not granted
    When the session calls python_sandbox with inputs naming a place the agent has no grant for
    Then it fails with SBX_INPUT_NOT_GRANTED

  Scenario: Error — path leaving the work folder
    When the session calls python_sandbox with files [{path: "../x", contentBase64: "eA=="}]
    Then it fails with SBX_FILE_INVALID naming "../x"

  Scenario: Error — expired sign-in
    Given capability "tickets" uses PersonToken Microsoft and Identity returns Expired
    When the session calls python_sandbox with requestedCapabilities ["tickets"]
    Then it fails with SBX_SIGN_IN_NEEDED

  Scenario: Error — mixed person credentials
    Given capability "tickets" uses PersonToken Microsoft and "repo" uses PersonToken Github
    When the session calls python_sandbox with requestedCapabilities ["tickets", "repo"]
    Then it fails with SBX_MIXED_CREDENTIALS

  Scenario: Error — Rules Engine unavailable fails closed
    Given sandbox_capabilities returns an error
    When the session calls python_sandbox
    Then it fails with SBX_RULES_UNAVAILABLE and no code runs

  Scenario: Error — environment without the extra guests
    When the session calls python_sandbox with environment {"MODE": "fast"}
    Then it fails with SBX_ENVIRONMENT_UNSUPPORTED

  Scenario: Error — guest digest mismatch at start
    Given the compiled-in digest differs from the guest bytes
    When the Sandbox starts
    Then status is Unavailable with code SBX_GUEST_DIGEST_MISMATCH
    And offer() returns no tool key for any session

  Scenario: Happy path — the program registers the provider and reads the status
    When the Sandbox starts with status Ready and guests [python]
    Then the Sandbox has registered nothing with the MCP Front itself
    And tool_provider() returns a provider that the program registers before the MCP Front starts
    And the provider's servers() lists python_sandbox with sandbox_callable false
    And status() returns Ready with guests [python] for the start-up report

  Scenario: Edge — a tool name with a hyphen is left out of ordin_tools
    Given the session is offered sandbox-callable tools "email_search" and "get-ticket"
    When the session calls python_sandbox
    Then the prelude binds ordin_tools.email_search and not "get-ticket"
    And "sandbox.module_tool_skipped" is logged for "get-ticket"
```

## Performance, Security, Observability

- **Performance targets.** Wrapper time for a call without inputs, excluding guest execution: p95 ≤ 50 ms. Warm guest start (instance from the compiled module) to first instruction: p95 ≤ 250 ms *(assumed here; C-23 measures cold and warm starts)*. First call of an agent controller (one guest compile until the `sandboxd` change listed at the end of this spec (not a gap)): p95 ≤ 3 s *(assumed here)*. Guest deadline 30 s, guest memory 512 MiB, at most 4 guests at once, so the Sandbox's ceiling is about 2.1 GiB of guest memory plus at most 64 MiB of inputs per call in flight. Work sessions: at most 16, each at most 64 MiB on disk.
- **Security.**
  - Entry points: `ToolProvider::call` only, reached through the MCP Front after the Rules Engine allowed the call; the `ToolCallContext` is never built from arguments. Arguments are typed and unknown fields refused (SA-XC-12).
  - The code holds no token: tokens live in `RunCredentials` for one run, are applied by the broker and are redacted from stdout and stderr (`controller.rs:277`–`278`, `:348`–`353`). The run's token is dropped when the run context is removed.
  - Agent-written code reaches only: `/work`, the granted capabilities (allow-listed host, path, methods; write methods only when `allow_write`; public addresses unless `allow_private_network`, `egress.rs:22`–`71`), and `ordin_tools`, which goes through the Rules Engine. No direct network path exists (`runtime.rs:1`–`11`).
  - Place inputs are read only through `PlaceFiles::read_for_agent`, which enforces the folder grant; the person's folders are never mounted (`workdir.rs:8`–`9`).
  - Outputs and module results are data: every module result is wrapped as untrusted; output files enter the agent's folder only, never a place, so nothing reaches a person's document without the Push Service and an approval.
  - Never logged or journalled: the code, file contents, stdout, stderr, environment values, tokens, API keys, request and response bodies, broker paths.
- **Observability** (`tracing` JSON lines, target `ordin_sandbox`; no metrics and no spans on the desktop or the personal server, SA-XC-04/05):
  - `sandbox.started` {status, guests}; `sandbox.unavailable` {code, field?}; `sandbox.guest_unavailable` {guest}; `sandbox.sessions_cleared` {count}
  - `sandbox.run_finished` {run_id, agent_instance_id, workspace_id, tool, exit_code, duration_ms, api_calls, module_calls, files_in, files_out, truncated, work_session, code_sha256}
  - `sandbox.run_refused` {run_id, agent_instance_id, tool, code}
  - `sandbox.policy_invalid` {agent_instance_id}; `sandbox.module_tool_skipped` {run_id, tool, reason}
  - `sandbox.session_deleted` {run_id, reason, files, bytes}; `sandbox.session_delete_failed` {run_id}
  - `sandbox.journal_failed` {run_id, kind}
  - Journal entry kinds *(chosen here; the Journal's `^[a-z][a-z0-9_]{0,63}$` pattern)*: `sandbox_run`, `sandbox_broker_call`.

## Gaps

None.
