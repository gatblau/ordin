# SPEC: Model Policy

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (Phase 1 build corrections: `OrganisationKey::new` and `HarnessEnvVar::new`; approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-model-policy` · **Build phase:** 2 · **Programs:** A
> **Dependencies:** Agent Configuration, Identity

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)

## Purpose

Turns the agent configuration's model choices into the settings a hosted agent's harness session is given, and decides the text a run or a sub-agent ends with when the model does not answer (`02:813`, ADR-098 item 3). It resolves the primary model, the fallback model tried once, the gateway address and the organisation's model key (from `ORDIN_MODEL_KEY_FILE` or the server's secret store, ADR-097 item 2), and it owns the safe reply — used when both models fail or a content filter blocks the agent's own call — and the fixed texts that replace a sub-agent's failure (wrightrun ADR-068). It reports what wrightrun's model policy offered and ordin does not carry: a sampling temperature, which Claude Code does not expose, and Gemini models, which the Agent Configuration already refuses (C-77, OQ-65). It owns the model settings of the loaded release (`02:813` *Owns*). The key it resolves reaches the Claude Code process only (C-74).

*Amended January 2026 (ADR-107; HLD 0.55.0):* Model Policy names no provider. The provider is the text before `:` in the configured model, and what the harness's own provider mode needs is the configuration's `model.environment`: Model Policy resolves each declared secret, checks the form of each entry and hands the names and values to the Claude Local Adapter, which sets them in the Claude Code child alone. `MPOL_PROVIDER_NOT_READY` is removed; C-77's question for providers is closed by the local test T1 of January 2026 (a base address and a key given in the child's environment are honoured, with no sign-in).

## Approach

A pure builder: `build` reads the configuration's `model` key through the Agent Configuration crate, resolves the key once at start, and returns an immutable `ModelPolicy` whose `harness_settings` value the Agent Loader places in every run's session set-up; the Claude Local Adapter turns it into Claude Code options. The decisions wrightrun made inside its model middleware (fallback once, one more attempt without gathered context after a block, safe reply, sub-agent text) become small decision functions the Session Supervisor calls, because in ordin the harness makes the model calls and ordin sees only their outcome in the event stream. Rejected: setting Claude Code flags here, because the option names are C-77's to read and belong to the adapter that runs the harness; re-reading the key file per run, because a key change reaches an instance by a restart (2D-1a).

**Error prefix:** `MPOL_` *(chosen here)*.

### Reconciliation assumptions

- **SA-RECON-01** The secret name stays `agent.model_key` (ruling R6, provider wins); the setup command's `model-key` is the person-facing name of the step, not the stored name (reconciliation row 275).
- **SA-RECON-02** `reply_for` and `after_block` are reached from the Session Supervisor through the program's `SupervisorHost`, which holds the built `ModelPolicy`; the Session Supervisor's host methods `reply_for` and `retry_after_block` carry them (reconciliation row 277).
- **SA-RECON-03** The key is read through Identity's `SecretStore::get` with a `SecretName` (Identity offers no `ServerSecretStore`, `SecretText` or `SecretStoreError`); the configuration enum is `ModelSetting` (reconciliation rows 278, N78, N144; rulings R6, R7).

## Shared Context

**From 2D (copied).**

| Variable | Type | Required | Owner | Description |
|---|---|---|---|---|
| `ORDIN_MODEL_KEY_FILE` | path | Yes, in the scaled shape for instances serving runs, the check and run-once; in the single shape only when the key is not in the secret store | Model Policy | Path of a file holding the organisation's model key. ordin reads it and gives the key to the Claude Code process alone (2D-1c); no hook, MCP server, sandbox or tool receives it. |
| `ANTHROPIC_API_KEY` (set in a child's environment) | string (secret) | Agent profile: set for Claude Code only | Claude Local Adapter | Set from `ORDIN_MODEL_KEY_FILE` or the secret store in the Claude Code child only, unless C-74 finds that its hooks, stdio MCP servers or tools would inherit it, in which case the key goes through Claude Code's own key setting instead. Removed from every other child. |

2D-3: "Organisation model key … Server setup command (single shape); Kubernetes Secret behind `ORDIN_MODEL_KEY_FILE` (scaled)". 2D-1c names `ANTHROPIC_API_KEY` and, for a gateway, `ANTHROPIC_BASE_URL` (ClaudeLocalAdapter.md SA-CLOSE-01). *Amended January 2026 (ADR-107):* every other variable a provider needs is declared in the configuration's `model.environment`; a `{secret: NAME}` value is resolved like a connection's credential — from `ORDIN_SECRETS_DIR/<NAME>` (scaled shape) or the server's secret store (single shape), never from ordin's own environment (2D-1d) — through the trait `EnvironmentSecrets` below, which the server program implements over the same source it gives the Connection Binder.

**From the Agent Configuration spec (copied; crate `ordin-types`, module `domain::agent_profile`, and functions of `ordin-agent-configuration`).**

```rust
pub enum ModelSetting { Short(String), Policy(ModelPolicyConfig) }
pub struct ModelPolicyConfig {
    pub primary: String,               // `provider:model`, e.g. "anthropic:claude-sonnet-4-6"
    pub fallback: Option<String>,      // same provider as primary (checked by Agent Configuration)
    pub temperature: f64,              // 0.0..=2.0; not carried (C-77)
    pub gateway_url: Option<String>,   // https, or http to loopback
    pub safe_reply: String,            // 1..=2000 characters
    pub environment: BTreeMap<String, EnvValue>,   // ADR-107; checked by Agent Configuration (names, reserved names, one of the two keys)
}
pub struct EnvValue { pub literal: Option<String>, pub secret: Option<String> }   // exactly one is Some
pub const DEFAULT_SAFE_REPLY: &str = "I could not answer that.";
pub fn model_policy(config: &AgentConfig) -> ModelPolicyConfig;   // a string `model` widened: no fallback, 0.0, no gateway, DEFAULT_SAFE_REPLY, no environment
```

**From Identity and Credentials (copied from IdentityAndCredentials.md).** The server's secret store (Secret Service, or the encrypted `0600` file, `02:570`, C-50). Identity offers no `ServerSecretStore`; modules use `SecretStore` directly:

```rust
/// Validated name, pattern `^[a-z0-9][a-z0-9_./-]{0,199}$` (CRED_SECRET_NAME_INVALID otherwise).
pub struct SecretName(/* private */ String);
impl SecretName { pub fn new(s: &str) -> Result<Self, CredError>; }
pub struct SecretStore { /* private */ }
impl SecretStore {
    /// Ok(None) when absent.
    pub async fn get(&self, name: &SecretName) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, CredError>;
}
```

The server program's setup command `secret model-key` writes the key with `IdentityService::put_server_secret("agent.model_key", value)` (OrdinServer step 9.6).

## Public Interface

### Types added to 2C (crate `ordin-types`, module `domain::agent_profile`)

*Placed in the shared types crate* because the Agent Loader produces and the Claude Local Adapter and Session Supervisor consume them, and neither consumer depends on this crate (2B-3).

```rust
/// Amended January 2026 (ADR-107): the provider is the text before ":" of the configured model, as written
/// (MODEL_ID's `^[a-z0-9_-]+`), not a closed enum; ordin holds no provider-specific code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProvider(pub String);
impl ModelProvider {
    /// The provider whose key is the organisation's model key (`ANTHROPIC_API_KEY`, MP-3).
    pub const DEFAULT: &'static str = "anthropic";
    pub fn is_default(&self) -> bool;     // self.0 == DEFAULT
}

/// The organisation's model key. Never serialised, never displayed; `Debug` prints `OrganisationKey(<redacted>)`.
#[derive(Clone)]
pub struct OrganisationKey(String);
impl OrganisationKey {
    /// The only constructor; Model Policy calls it with the key text read in MP-3 (amended January 2026).
    pub fn new(key: String) -> OrganisationKey;
    pub fn expose(&self) -> &str;
}
impl std::fmt::Debug for OrganisationKey { /* writes "OrganisationKey(<redacted>)" */ }

/// One resolved `model.environment` entry (ADR-107). The value is never serialised, displayed or logged;
/// `Debug` prints `HarnessEnvVar { name: "<NAME>", value: <redacted> }`, for a literal and a secret alike.
#[derive(Clone)]
pub struct HarnessEnvVar { pub name: String, value: String }
impl HarnessEnvVar {
    /// The only constructor; Model Policy calls it in MP-2a (amended January 2026).
    pub fn new(name: String, value: String) -> HarnessEnvVar;
    pub fn value(&self) -> &str;
}

/// What the harness session is given about models. Built by Model Policy; applied by the Claude Local Adapter.
#[derive(Debug, Clone)]
pub struct HarnessModelSettings {
    pub provider: ModelProvider,
    pub primary_model: String,            // the text after "provider:", e.g. "claude-sonnet-4-6"
    pub fallback_model: Option<String>,   // tried once, by the harness's own fallback option (C-77)
    pub gateway_url: Option<String>,      // the gateway the harness sends model calls to (C-77)
    /// Amended January 2026 (ADR-107): None only for a provider other than the default with no key set (MP-3.3).
    pub key: Option<OrganisationKey>,
    /// Amended January 2026 (ADR-107): in the configuration's order (name order of the YAML mapping).
    pub environment: Vec<HarnessEnvVar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFailureCause {
    /// Every model the policy allows failed (the harness reports an API error after its fallback).
    ModelFailed,
    /// A content check blocked the question or the answer. Amended January 2026 (ADR-107): raised by
    /// ordin's own `content-check` control (Rules Engine `check_content`), never read from harness output.
    ContentFilter,
    /// The harness could not shorten a conversation that grew too long.
    SummaryFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPlace { Agent, Subagent }
```

### Crate `ordin-model-policy`

```rust
/// Where the organisation's key comes from (MP-3). Built by the server program.
pub struct KeySource {
    pub key_file: Option<std::path::PathBuf>,                         // ORDIN_MODEL_KEY_FILE
    pub secret_store: Option<std::sync::Arc<ordin_identity::SecretStore>>,  // single shape only
    /// Amended January 2026 (ADR-107): the values of `model.environment` secrets (MP-2a).
    pub environment_secrets: std::sync::Arc<dyn EnvironmentSecrets>,
}

/// Amended January 2026 (ADR-107; chosen here, SA-HOST-03). Owned here (ADR-100 form); implemented by the
/// server program over the same source it gives the Connection Binder's `SecretSource`: `ORDIN_SECRETS_DIR/<NAME>`
/// in the scaled shape, the server's secret store in the single shape.
#[async_trait::async_trait]
pub trait EnvironmentSecrets: Send + Sync {
    /// Ok(None) when the variable has no value.
    async fn get(&self, name: &str) -> Result<Option<zeroize::Zeroizing<String>>, EnvironmentSecretsUnavailable>;
}
#[derive(Debug)] pub struct EnvironmentSecretsUnavailable;
/// The name the key is stored under in the secret store (chosen here). The server program's
/// `ordin-server setup secret model-key` writes it under this name (reconciliation row 275).
pub const MODEL_KEY_SECRET_NAME: &str = "agent.model_key";
pub const MAX_KEY_BYTES: u64 = 4_096;

pub const SUBAGENT_STOPPED_MODEL_FAILED: &str = "The sub-agent stopped before it finished this task: the model did not answer. What it did before it stopped is not reported here.";
pub const SUBAGENT_STOPPED_CONTENT_FILTER: &str = "The sub-agent stopped before it finished this task: the model provider blocked a request. What it did before it stopped is not reported here.";
pub const SUBAGENT_STOPPED_SUMMARY_FAILED: &str = "The sub-agent stopped before it finished this task: its conversation grew too long and the model did not shorten it. It can be called again with a smaller task.";
// the three texts are wrightrun's SUBAGENT_STOPPED, verbatim (ModelPolicy.md)

pub struct ModelPolicy { /* settings, safe reply, not-carried list, key origin */ }

/// Resolves the configuration's model choices and the key. Called once at instance start, by the
/// check command and by run-once.
pub async fn build(config: &AgentConfig, keys: &KeySource) -> Result<ModelPolicy, ModelPolicyError>;

impl ModelPolicy {
    pub fn harness_settings(&self) -> HarnessModelSettings;
    /// The text a run (Agent) or a sub-agent call (Subagent) ends with for this cause.
    /// Agent + SummaryFailed is `None`: the run ends as failed with no safe reply (wrightrun ADR-066).
    pub fn reply_for(&self, cause: ModelFailureCause, place: CallPlace) -> Option<&str>;
    /// After a content-filter block of the agent's own call: try once more without the gathered
    /// context when the run carried some and has not been tried without it yet; otherwise the safe reply.
    pub fn after_block(&self, carried_gathered_context: bool, already_retried: bool) -> BlockAction;
    /// Lines for the start-up report (Agent Loader).
    pub fn report(&self) -> ModelPolicyReport;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockAction { RetryWithoutGatheredContext, SafeReply }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPolicyReport {
    pub model: String,                 // "anthropic:claude-sonnet-4-6"
    pub fallback: Option<String>,
    pub gateway_used: bool,
    pub safe_reply: String,
    pub key_from: KeyOrigin,
    pub not_carried: Vec<(String, String)>, // (key, reason)
    /// Amended January 2026 (ADR-107): the names set in the harness child, in order; never a value.
    pub environment_names: Vec<String>,
}
/// `None` (ADR-107): a provider other than the default with no key set (MP-3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum KeyOrigin { KeyFile, SecretStore, None }

#[derive(Debug, thiserror::Error)]
pub enum ModelPolicyError {
    #[error("error[MPOL_KEY_MISSING]: no model key is set; set ORDIN_MODEL_KEY_FILE or store the key with the setup command")]
    KeyMissing,
    #[error("error[MPOL_KEY_FILE_UNREADABLE]: the model key file cannot be read ({reason}) (ORDIN_MODEL_KEY_FILE)")]
    KeyFileUnreadable { reason: &'static str },
    #[error("error[MPOL_KEY_INVALID]: the model key must be one line of 1 to 4096 printable characters with no space (ORDIN_MODEL_KEY_FILE)")]
    KeyInvalid,
    #[error("error[MPOL_SECRET_STORE_UNAVAILABLE]: the server's secret store cannot be read")]
    SecretStoreUnavailable,
    /// Amended January 2026 (ADR-107): replaces MPOL_PROVIDER_NOT_READY, which is removed.
    #[error("error[MPOL_SECRET_MISSING]: the variable {name} has no value; add it to the mounted secret or the secret store (model.environment.{entry})")]
    SecretMissing { name: String, entry: String },
}
```

**Called by.**

| Public item | Called by |
|---|---|
| `KeySource`, `MODEL_KEY_SECRET_NAME` | the server program (OrdinServer): builds `KeySource` from `ORDIN_MODEL_KEY_FILE` and, in the single shape, the server secret store; its `setup secret model-key` step stores the key under `MODEL_KEY_SECRET_NAME` |
| `build`, `harness_settings`, `report` | Agent Loader (step L-14; the settings go into every `HostedSessionSetup`, the report into the start-up report) |
| `HarnessModelSettings`, `OrganisationKey`, `ModelProvider`, `HarnessEnvVar` (in `ordin-types`) | Claude Local Adapter (applies models, `gateway_url`, key and `environment` to Claude Code) and Session Supervisor (passes them through to the adapter), both from `HostedSessionSetup` |
| `EnvironmentSecrets` | implemented by the server program (ADR-107), over `ORDIN_SECRETS_DIR` or the server secret store |
| `reply_for`, `after_block`, `ModelFailureCause`, `CallPlace`, `BlockAction`, `SUBAGENT_STOPPED_*` | the Session Supervisor's agent-profile run worker, through the program's `SupervisorHost::reply_for(cause, place) -> Option<String>` (= `policy.reply_for(cause, place).map(str::to_owned)`) and `SupervisorHost::retry_after_block(carried, already_retried) -> bool` (= `policy.after_block(..) == BlockAction::RetryWithoutGatheredContext`) (the Session Supervisor has no Model Policy edge, 2B-3). The text `reply_for(_, Agent)` returns becomes `RunOutcome.reply` with `RunOutcome.safe_reply = true` in the Run Queue (reconciliation row 277, O69) |

### Example

Configuration `model: {primary: anthropic:claude-sonnet-4-6, fallback: anthropic:claude-haiku-4-5, temperature: 0.2, safe-reply: "I could not answer that. Please ask the service desk."}`, scaled shape, `ORDIN_MODEL_KEY_FILE=/var/run/secrets/model/key` holding `sk-ant-api03-…` and a newline:

- `build(&config, &KeySource { key_file: Some("/var/run/secrets/model/key".into()), secret_store: None, environment_secrets }).await` returns a `ModelPolicy`.
- `harness_settings()` returns `HarnessModelSettings { provider: ModelProvider("anthropic"), primary_model: "claude-sonnet-4-6", fallback_model: Some("claude-haiku-4-5"), gateway_url: None, key: Some(OrganisationKey(<redacted>)), environment: [] }`.
- `reply_for(ContentFilter, Agent)` returns `Some("I could not answer that. Please ask the service desk.")`; `reply_for(ModelFailed, Subagent)` returns `Some(SUBAGENT_STOPPED_MODEL_FAILED)`; `reply_for(SummaryFailed, Agent)` returns `None`.
- `after_block(true, false)` returns `RetryWithoutGatheredContext`; `after_block(true, true)` returns `SafeReply`.
- `report()` returns `{ model: "anthropic:claude-sonnet-4-6", fallback: Some("anthropic:claude-haiku-4-5"), gateway_used: false, safe_reply: "I could not answer that. Please ask the service desk.", key_from: KeyFile, not_carried: [("model.temperature", "Claude Code does not expose a sampling temperature; the value 0.2 is not applied (C-77)")], environment_names: [] }`.
- *ADR-107:* the same configuration with `primary: other-provider:claude-sonnet-4-6`, no fallback and `environment: {SERVICE_REGION: {literal: eu-1}, TOKEN: {secret: MODEL_TOKEN}}`, no key file and `EnvironmentSecrets` holding `MODEL_TOKEN`: `harness_settings()` returns `provider: ModelProvider("other-provider")`, `key: None`, `environment: [SERVICE_REGION, TOKEN]` (values redacted in `Debug`); `report().environment_names` is `["SERVICE_REGION", "TOKEN"]` and `key_from` is `None`.

## Internal Logic

`build(config, keys)`:

1. **MP-1 Policy.** `p = ordin_agent_configuration::model_policy(config)`. Split `p.primary` at the first `:` into `provider` and `model`; the same for `p.fallback`. The Agent Configuration has already refused a provider of `REFUSED_PROVIDERS`, a fallback on another provider and every malformed `model.environment` entry; this function does not repeat those checks.
2. **MP-2 Provider (amended January 2026, ADR-107).** `provider` → `ModelProvider(provider)`, whatever its text: Model Policy validates the form of the settings only and holds no provider-specific code. What a provider other than the default needs (a region, a project, credentials, a base address) is the deployment's `model.environment`. C-77's provider question is closed by test T1 (January 2026): settings given in the Claude Code child's environment are honoured with no sign-in.
2a. **MP-2a Environment (ADR-107).** For each `(name, value)` of `p.environment`, in key order: `literal: Some(v)` → `HarnessEnvVar { name, value: v }`; `secret: Some(s)` → `keys.environment_secrets.get(s).await`: `Ok(Some(v))` → `HarnessEnvVar { name, value: v }` (one trailing `\n` removed, as MP-3.1); `Ok(None)` → `MPOL_SECRET_MISSING { name: s, entry: name }`; `Err(_)` → `MPOL_SECRET_STORE_UNAVAILABLE`. No value is logged, placed in an error or written to a file.
3. **MP-3 Key.** In this order:
   1. `keys.key_file` is `Some(path)`: `tokio::fs::metadata(path)`; not found, not a regular file, or unreadable → `MPOL_KEY_FILE_UNREADABLE` with `reason` `"not found"`, `"not a regular file"` or `"not readable"`; larger than `MAX_KEY_BYTES` → `MPOL_KEY_INVALID`. Read the bytes; not UTF-8 → `MPOL_KEY_INVALID`. Remove one trailing `\n` (and a `\r` before it). Empty, or holding any character that is white space or a control character → `MPOL_KEY_INVALID`. `key_from = KeyFile`.
   2. Else `keys.secret_store` is `Some(store)`: `store.get(&SecretName::new(MODEL_KEY_SECRET_NAME)?).await` (the constant passes the pattern); `Err(_)` (any `CredError`) → `MPOL_SECRET_STORE_UNAVAILABLE` (retryable by restarting; the start stops); `Ok(None)` → step 3.3; `Ok(Some(bytes))` → the same UTF-8 and text checks as 3.1 → `key_from = SecretStore`.
   3. Neither gave a key → `MPOL_KEY_MISSING` when the provider is the default (`ModelProvider::is_default`); *amended January 2026 (ADR-107; chosen here, SA-HOST-02)* for any other provider, `key = None` and `key_from = None`: that provider's credentials come from `model.environment`, and `ANTHROPIC_API_KEY` is not set.
   The key text is held only in `OrganisationKey`; it is never logged, never placed in an error, never written to a file.
4. **MP-4 Not carried.** For the mapping form, push `("model.temperature", format!("Claude Code does not expose a sampling temperature; the value {t} is not applied (C-77)"))`. *Conditional on C-77:* if C-77 finds that Claude Code does expose a temperature with an organisation key, this entry is removed and the value is added to `HarnessModelSettings` by a revision of this spec. Gemini models never reach here: the Agent Configuration refuses them with `CFG_MODEL_NOT_OFFERED` (OQ-65).
5. **MP-5** Log one `info` event `model_policy_built` with fields `provider`, `model`, `fallback` (the model text or `none`), `gateway` (`used` or `not_used`), `key_from` (`key_file`, `secret_store` or `none`), `environment` (the names joined by `,`, or `none`; ADR-107). Return the `ModelPolicy`.

`harness_settings()` (MP-6): clone of the built settings. The fallback model is applied by the harness's own fallback option, tried once by the harness (C-77). *Conditional on C-77:* if Claude Code offers no fallback option with an organisation key, the Claude Local Adapter reports "fallback not carried" in the start-up report and the run ends with the safe reply after the primary model fails; this function does not change.

`reply_for(cause, place)` (MP-7):

| `cause` | `Agent` | `Subagent` |
|---|---|---|
| `ModelFailed` | `Some(safe_reply)` | `Some(SUBAGENT_STOPPED_MODEL_FAILED)` |
| `ContentFilter` | `Some(safe_reply)` | `Some(SUBAGENT_STOPPED_CONTENT_FILTER)` |
| `SummaryFailed` | `None` (the run ends as failed; wrightrun ADR-066) | `Some(SUBAGENT_STOPPED_SUMMARY_FAILED)` |

The provider's own text never becomes a reply: the Session Supervisor logs it (cut at 4,000 characters, wrightrun Logging step 9) and gives the person this text.

`after_block(carried, already_retried)` (MP-8): `carried && !already_retried` → `RetryWithoutGatheredContext` (wrightrun ADR-052: the run is tried once more without the context gathered before it, and the channel-document tool is offered from then on); otherwise `SafeReply`. *Amended January 2026 (ADR-107):* the block is the verdict of a `content-check` control whose `on-block` is `retry-without-context` (Session Supervisor J.7); a control whose `on-block` is `end` never asks this function and ends with the safe reply.

`report()` (MP-9): the fields as built; `safe_reply` with each line break and tab written as one space (wrightrun StartupReport rule); `environment_names` the names of `environment`, never a value (ADR-107).

## Data Model

N/A — the model settings of the loaded release live in memory for the life of the instance (`02:813` *Owns*); the key is read from a file or the secret store at start and never stored by this crate.

## Error Table

In-process `ModelPolicyError`. Each stops the start of an instance (or the check or run-once command, exit status 1) before it reports ready.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Neither `ORDIN_MODEL_KEY_FILE` nor the secret store gives a key | `Err(permanent)` | `MPOL_KEY_MISSING` | `error[MPOL_KEY_MISSING]: no model key is set; set ORDIN_MODEL_KEY_FILE or store the key with the setup command` |
| The key file is absent, not a regular file or not readable | `Err(permanent)` | `MPOL_KEY_FILE_UNREADABLE` | `error[MPOL_KEY_FILE_UNREADABLE]: the model key file cannot be read (not found) (ORDIN_MODEL_KEY_FILE)` |
| The key is empty, above 4,096 bytes, not UTF-8, or holds white space or a control character | `Err(permanent)` | `MPOL_KEY_INVALID` | `error[MPOL_KEY_INVALID]: the model key must be one line of 1 to 4096 printable characters with no space (ORDIN_MODEL_KEY_FILE)` |
| The secret store, or the source behind `EnvironmentSecrets`, cannot be read | `Err(retryable)` — by restarting the instance | `MPOL_SECRET_STORE_UNAVAILABLE` | `error[MPOL_SECRET_STORE_UNAVAILABLE]: the server's secret store cannot be read` |
| A `model.environment` entry names a secret that has no value (ADR-107) | `Err(permanent)` | `MPOL_SECRET_MISSING` | `error[MPOL_SECRET_MISSING]: the variable MODEL_TOKEN has no value; add it to the mounted secret or the secret store (model.environment.TOKEN)` |

*Amended January 2026 (ADR-107):* `MPOL_PROVIDER_NOT_READY` is removed; no provider is refused here.

The person never sees a code. A person in chat sees the configuration's safe reply or a sub-agent's fixed text only as the agent's answer (`03:1622`–`1624`).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Model Policy

  Scenario: Happy path — settings from the mapping form and a key file
    Given the configuration of the Example and a key file holding "sk-test-123" followed by a newline
    When build is called with that key file
    Then harness_settings returns provider Anthropic, primary model "claude-sonnet-4-6" and fallback model "claude-haiku-4-5"
    And the key exposes "sk-test-123"
    And the Debug form of the settings does not contain "sk-test-123"

  Scenario: Happy path — the string form is widened
    Given the configuration "model: anthropic:claude-sonnet-4-6"
    When build is called
    Then report returns no fallback, gateway not used, safe reply "I could not answer that." and no not-carried entry

  Scenario: Happy path — the key from the secret store in the single shape
    Given no key file and a secret store holding "agent.model_key" = "sk-test-456"
    When build is called
    Then report returns key_from SecretStore

  Scenario: Edge case — the temperature is reported as not carried
    Given the mapping form with temperature 0.2
    When report is called
    Then not_carried holds "model.temperature" with a reason naming C-77

  Scenario: Edge case — replies by cause and place
    When reply_for is called for each cause and place
    Then ContentFilter for Agent returns the safe reply
    And SummaryFailed for Agent returns None
    And ModelFailed for Subagent returns the text beginning "The sub-agent stopped before it finished this task: the model did not answer."

  Scenario: Edge case — one more attempt without gathered context, once
    When after_block is called with carried true and already_retried false
    Then it returns RetryWithoutGatheredContext
    When after_block is called with carried true and already_retried true
    Then it returns SafeReply

  Scenario: Error path — no key anywhere
    Given no key file and a secret store with no "agent.model_key"
    When build is called
    Then it returns MPOL_KEY_MISSING

  Scenario: Error path — a key with a space
    Given a key file holding "sk test"
    When build is called
    Then it returns MPOL_KEY_INVALID
    And the error text does not contain "sk test"

  Scenario: Error path — the secret store cannot be read
    Given no key file and a secret store that returns an error
    When build is called
    Then it returns MPOL_SECRET_STORE_UNAVAILABLE

  Scenario: Happy path — a provider other than the default with declared settings (ADR-107)
    Given the configuration "model: {primary: other-provider:claude-sonnet-4-6, temperature: 0, safe-reply: x, environment: {SERVICE_REGION: {literal: eu-1}}}"
    And no key file and no "agent.model_key" in the secret store
    When build is called
    Then harness_settings returns provider "other-provider", no key and one environment entry SERVICE_REGION
    And report returns key_from None and environment_names ["SERVICE_REGION"]

  Scenario: Happy path — a secret setting resolves and only its name is reported (ADR-107)
    Given model.environment names TOKEN as {secret: MODEL_TOKEN} and EnvironmentSecrets holds MODEL_TOKEN = "tok-123"
    When build is called
    Then the TOKEN entry's value is "tok-123"
    And report and the model_policy_built event hold TOKEN and do not contain "tok-123"
    And the Debug form of the settings does not contain "tok-123"

  Scenario: Error path — a declared secret with no value (ADR-107)
    Given model.environment names TOKEN as {secret: MODEL_TOKEN} and EnvironmentSecrets has no MODEL_TOKEN
    When build is called
    Then it returns MPOL_SECRET_MISSING naming MODEL_TOKEN and model.environment.TOKEN

  Scenario: Error path — the default provider still needs the key (ADR-107)
    Given the configuration "model: anthropic:claude-sonnet-4-6" and no key anywhere
    When build is called
    Then it returns MPOL_KEY_MISSING
```

## Performance / Security / Observability

- **Performance targets:** `build` completes in under 50 ms p95 with a key file and under 500 ms p95 with the secret store (one read, *assumed here*); every other function is synchronous and under 1 µs. Memory under 64 KiB.
- **Security:** No network entry point. The key is read from a file path or the secret store only, never from ordin's own environment (2D-1d rule) and never from the configuration file; it is held in `OrganisationKey`, whose `Debug` is redacted and which implements neither `Serialize` nor `Display`; it appears in no error, log line, metric or span. The Agent Loader passes it to the Claude Local Adapter only, which gives it to the Claude Code process alone (C-74). The provider's own failure text never reaches a caller or a chat user (MP-7). *Amended January 2026 (ADR-107):* every `model.environment` value, a literal included, is held in `HarnessEnvVar`, whose `Debug` is redacted, and reaches the Claude Code child alone, by the same route and under the same C-74 finding as the key; only names are reported or logged.
- **Observability:** One `tracing` event, `model_policy_built` (`info`; fields `provider`, `model`, `fallback`, `gateway`, `key_from`, `environment`). No metric of its own: `wrightrun_safe_replies_total` and `wrightrun_fallback_runs_total` are recorded by the Session Supervisor through the Telemetry bridge when it applies `reply_for`. No span.

## Gaps

None.
