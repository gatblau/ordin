# SPEC: Error Handling

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-108; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** `ordin-types` (`CodedError`, `RepairAction`, `TypesError`, module `error_registry` *(chosen here)*); `ordin-service-api` (`ServiceError`, `CommandReply`, `ErrorSentences`, `handle`); `ordin-caller-api` (wrightrun envelope, panic layer); `ordin-hook-relay` and `apps/hook` (hook protocol); `ordin-localisation` (sentences)

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Applicability by program](#applicability-by-program)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Error-code prefixes and owners](#error-code-prefixes-and-owners)
- [Error-code registry](#error-code-registry)
- [Codes of embedded and external components, mapped at the boundary](#codes-of-embedded-and-external-components-mapped-at-the-boundary)
- [Registry defects](#registry-defects)
- [Gaps](#gaps)

## Purpose

Fixes, for every program, the one way an error leaves a module and reaches its caller: a typed module error carrying a stable registry code, a retry flag and at most one repair action; four envelopes, one per surface (Service API and Remote API, Caller API, hook protocol, MCP tool result); the mapping from code to HTTP status; panic recovery at every surface; and the error-code registry, which lists every code the 74 component specs (*amended January 2026, ADR-102; ADR-103*) and the Phase 2C files define, with its owning component and surface. The person never sees a code: the screens and the chat channels show one plain-language sentence with at most one action ("Try again", "Sign in again", "Send to IT"); codes, tool output and stack details appear only in Diagnostics and in help requests (HLD 07 — *Error handling*).

## Approach

One trait, `CodedError` in `ordin-types`, is implemented by every module's error enum; each surface has one function that turns a `&dyn CodedError` into that surface's envelope, so no module builds an envelope itself. The registry below was **enumerated from the component specs, not from a table** (audit-checklist error-code item): every `\b[A-Z][A-Z0-9]+_[A-Z0-9_]+\b` token in each spec's Error Table *Code* column (1,627 distinct codes in 1,819 rows; *amended January 2026, ADR-102; ADR-103: 28 codes in 28 rows*), every such token in a code span of the specs' Internal Logic, Public Interface and Shared Context that names an error code (202 more: 199 Phase 2C validation codes copied into a component, 3 chosen in a component's validation table), and every code in the validation tables of the two Phase 2C files that no component copies (29 more, registered under the crate that owns the prefix or checks the 2C entity), with environment variables, constants, message-text identifiers and examples filtered out by name and by reading each line. The registry is generated data: `ordin-types` (`error_registry`) holds it as a compiled-in table, each crate's own test fails when one of its codes is not in it, and a workspace source scan in CI catches the rest (Internal Logic 11). Rejected: one error enum shared by all crates, because it would make every crate depend on every other crate's failure modes and break the 2B-1 build test; free-text error messages with codes parsed out, because a code is a contract and a parser is not.

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | Service API envelope over Tauri IPC (`CommandReply`), MCP tool results, hook protocol; Diagnostics holds codes and references | SA-XC-02, SA-ENV-01, SA-ERR-01 |
| Personal server (P) | Yes | The same, plus the Remote API (HTTP status from the 2C table), chat channels showing the sentence only, `ordin-server` exit statuses | SA-XC-02, SA-ENV-01 |
| Agent profile (A) | Yes | Caller API with wrightrun's envelope unchanged (SA-ENV-02); start-up report lines `error[CODE]: <sentence>`; exit status 1 on a start refusal | SA-XC-02, SA-ENV-02 |

## Shared Context

Copied from Phase 2C and the component specs named; a reader implements from this section alone.

```rust
// ordin-types (SharedTypes.md)
pub trait CodedError: std::error::Error + Send + Sync + 'static {
    /// The registry code, `<PREFIX>_<NAME>`.
    fn code(&self) -> &'static str;
    /// True when the same call may succeed if repeated unchanged later.
    fn retryable(&self) -> bool;
    /// The one repair action shown to the person, if any (07 — Error handling).
    fn repair(&self) -> Option<RepairAction> { None }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction { TryAgain, SignInAgain, SendToIt }

// ordin-service-api (2C wire §1, ServiceApi.md)
pub struct CommandReply<O> { pub request_id: CommandRequestId, #[serde(flatten)] pub outcome: CommandOutcome<O> }
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandOutcome<O> { Ok { result: O }, Error { error: ServiceError } }
pub struct ServiceError {
    pub code: String,                       // stable code, never shown to the person
    pub message: String,                    // one sentence in the person's locale, 1–300 characters
    pub action: Option<RepairAction>,       // omitted when None
    pub reference: Option<String>,          // Diagnostics line id, 1–64 characters ("r-81")
}
pub trait ErrorSentences: Send + Sync {
    fn sentence(&self, code: &str) -> String;
    fn action(&self, code: &str) -> Option<RepairAction>;
}

// 2C wire §4 (hook protocol)
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookReply {
    Holding { call_id: HookCallId, approval_id: ApprovalId, deadline: Timestamp },
    Allow { call_id: HookCallId },
    Deny { call_id: HookCallId, reason: String },   // reason 1–500 characters
    Ack { call_id: HookCallId },
}
```

| Assumption | Value | Source |
|---|---|---|
| Code form | `^[A-Z]{2,6}_[A-Z0-9_]{2,60}$`; prefix then one underscore | 2C wire `CommandReply` validation; SA-ERR-01 |
| Carried wrightrun prefixes | `SRV_`, `WF_`, `CFG_`, `PRE_`, `RUN_`, `CHAT_` keep their names (plus `TOOL_`, `LOAD_`, `REL_`, `DEC_`, `KNOW_`, `AUTH_`, `DB_`, named as carried by Connection Binder, Agent Loader, Release Builder, Knowledge Service, Caller Authentication and Run Queue) | SA-ERR-01; HLD 07 decision 80; CallerAuthentication.md; RunQueue.md |
| D8 renames | ordin's workflow-file codes `WF_` → `WFF_`; Tool request `TOOL_` → `TREQ_`; 2C `REL_REFERENCE_CONFLICT` → `REL_EXISTS_DIFFERENT` | SA-ERR-01 amended; HLD 07 revision 0.49.0 |
| Correction-round renames (January 2026) | Authoring `AUTH_*` → `AUTHR_*`; `PUB_AUDIENCE_GREW`, `PUB_DELETE_UNCHECKED` → `PUSH_*`; Memory `SCOPE_CROSS_WRITE` → `MEM_SCOPE_CROSS_WRITE`; `PART_DEPRECATED_AS_AGENT` → `PKG_DEPRECATED_AS_AGENT`; `CHARTER_*` → `CHRT_*`; `RUN_ADAPTER_MISMATCH`, `RUN_PAUSE_REASON_MISMATCH` and the seven further 2C `Run` codes (`RUN_ABANDONED_NOT_REMOTE`, `RUN_CACHE_EXCEEDS_TOKENS`, `RUN_ENDED_AT_MISMATCH`, `RUN_END_REASON_TOO_LONG`, `RUN_INPUTS_TOO_MANY`, `RUN_LIMIT_OUT_OF_RANGE`, `RUN_NO_REPLY_NOT_SLACK`) → `RUNST_*`; Server Connector `SRV_ADDRESS_INVALID`, `SRV_PORT_OUT_OF_RANGE`, `SRV_DUPLICATE` → `CONN_*`; `CMD_ROLE_COMBINATION`, `CMD_NOTHING_TO_SERVE` → `SERVER_*`; `SERVER_SECRETS_KEY_*` → Identity and Credentials' `CRED_SECRET_KEY_*`; `APPR_DATASTORE_UNAVAILABLE` → `APPR_STORE_UNAVAILABLE`; removed `TOOL_REGISTER_FAILED`, `RA_SESSION_NOT_EXPORTABLE`, `USAGE_ESTIMATE_OUT_OF_RANGE` | *Registry defects* below; the component specs named there |
| Caller API envelope | body exactly `{"error": "<sentence>", "code": "<CODE>"}`, `application/json`, no `action` | SA-ENV-02 |
| Unregistered code | Service API: `SVC_INTERNAL`, 500, action `send_to_it`; Caller API: `SRV_INTERNAL`, 500, sentence "the server could not answer the request" | ServiceApi.md Internal Logic 4; CallerApi.md |
| Service API deadlines | `Interactive` 10 s, `Search` 10 s, `Transfer` 60 s; timeout → `SVC_DEADLINE` | ServiceApi.md Internal Logic 3 |
| Hook deadlines | allow/deny within 2 s; hook's own deadline 50 s; harness hook timeout 60 s | 2C wire `HookReply` |

## Public Interface

**Surfaces and their envelopes.**

| Surface | Envelope | Status / exit | Built by |
|---|---|---|---|
| Service API over Tauri IPC (D) | `CommandReply` with `kind: "error"` and `ServiceError` | none (IPC) | `ordin_service_api::handle` |
| Remote API (P) | the same `CommandReply` body | HTTP status from the 2C table: 200 for `ok` and every domain refusal; 422 `SVC_INPUT_INVALID`, `SVC_BODY_TOO_LARGE`, `SVC_REQUEST_ID_MALFORMED`, `SVC_VERSION_MALFORMED`, `SRV_BAD_CURSOR`; 409 `SVC_VERSION_UNSUPPORTED`; 404 `SVC_COMMAND_UNKNOWN`, `SVC_COMMAND_LOCAL_ONLY`, `SVC_BACKEND_UNKNOWN`, `SRV_NO_ROUTE`; 405 `SRV_METHOD_NOT_ALLOWED`; 403 `SRV_REFUSED`; 401 `SVC_UNAUTHENTICATED`; 429 `SVC_RATE_LIMITED`; 503 `SVC_DEADLINE`; 500 `SVC_INTERNAL` | `ordin_service_api::handle` (`HandledReply.http_status`) and `ordin-remote-api` for 401, 403, 404 (route), 405, 429 |
| Caller API (A) | `{"error": "<sentence>", "code": "<CODE>"}` | the registry's HTTP status (column *HTTP status* below) | `ordin-caller-api` |
| Operator commands and start (A, P) | one line `error[<CODE>]: <sentence>` on standard error and in the start-up report | exit status 1 (start refusal), 64 (bad command line of `ordin-hook`) | `apps/server`, `ordin-agent-loader` |
| Hook protocol (D, P, A) | `HookReply::Deny { call_id, reason }`; the code is journalled and logged, not sent | the hook exits 0 with the harness's deny verdict | `ordin-hook-relay`, `apps/hook` |
| MCP tool result (D, P, A) | the tool result with `isError: true` and a text the model reads; block checks return `[{"code","at","reason"}]` | — | `ordin-mcp-front`, `ordin-service-api::check_block` |
| Chat channels (P, A) | the sentence only, with a button for the one action | — | `ordin-channel-gateway` |

**Functions** *(chosen here, in `ordin-types::error_registry`)*:

```rust
/// One row of the registry, compiled in from the table below.
pub struct RegistryEntry {
    pub code: &'static str,
    pub owner_crate: &'static str,     // e.g. "ordin-approval"
    pub http_status: Option<u16>,      // Caller API or Remote API status; None elsewhere
    pub retryable: bool,
    pub sentence_id: &'static str,     // Fluent message id, the code in kebab-case ("appr-card-closed")
}
/// Look up a code; None when it is not registered.
pub fn lookup(code: &str) -> Option<&'static RegistryEntry>;
/// True when `prefix` (without the underscore) is owned by `crate_name`.
pub fn owns_prefix(crate_name: &str, prefix: &str) -> bool;
```

### Example

A Remote API command whose module refuses it:

```json
{"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","kind":"error",
 "error":{"code":"APPR_CARD_ALREADY_DECIDED","message":"This was already decided on Asha's laptop."}}
```
HTTP status 200. The same refusal on the Caller API: status 409, body `{"error": "<sentence>", "code": "SRV_APPROVAL_ALREADY_DECIDED"}`.

## Internal Logic

1. **Raise.** A module returns its own error enum implementing `CodedError`. `code()` returns a code whose prefix the crate owns (*Error-code prefixes and owners*); `retryable()` follows the registry's *Retryable* column; `repair()` is set only where the owning spec names an action. A module never builds a `ServiceError`, a Caller API body or a hook reply.
2. **Service API.** `ordin_service_api::handle` runs the command inside `timeout_at(deadline)` and `catch_unwind`. On `Err(e)` the program's `ServiceApi` implementation builds `ServiceError { code: e.code(), message: sentences.sentence(e.code()), action: e.repair().or(sentences.action(e.code())), reference }`. A code `lookup` does not find becomes `SVC_INTERNAL` and is logged at `error` with the original code. Timeout → `SVC_DEADLINE` (retryable, `try_again`); panic → `SVC_INTERNAL` (`send_to_it`).
3. **Sentence.** `ErrorSentences` is implemented by the program with Localisation: the message id is the code in kebab-case; a missing message falls back to `en-GB`; a missing `en-GB` message fails the build (SA-I18N-00). The sentence holds no internal state, path, token, tool output or exception text.
4. **Reference.** When a reply offers a repair, the program writes one Diagnostics line holding the code, the owning crate and the detail, and puts its identifier in `reference`.
5. **Remote API status.** `HandledReply.http_status` follows the 2C table (Public Interface); the Remote API adds 401 `SVC_UNAUTHENTICATED`, 429 `SVC_RATE_LIMITED`, 404 `SRV_NO_ROUTE`, 405 `SRV_METHOD_NOT_ALLOWED` and 403 `SRV_REFUSED` for an `Origin` header.
6. **Caller API.** Each route maps a module error to the registry's HTTP status and writes `{"error", "code"}`. A panic-catching layer answers 500 `SRV_INTERNAL` and logs `server_error` with the panic's type and the method; any failure without a registered code is answered the same way.
7. **Hook protocol.** Every failure on the hook path denies (fail closed, ADR-045): the relay writes `Deny` with a fixed reason and journals the code; the hook program, on its own failure, exits 0 with the harness's deny verdict and logs the code.
8. **MCP tools.** A provider's refusal becomes a tool result with `isError: true`; the text is for the model and names the correct form; codes are journalled with the tool request.
9. **Panic recovery.** Desktop and ordin server install a panic hook that logs `panic` with the location and the first 200 characters of the message (OrdinDesktop.md step 2). A panic inside a command is caught by `handle` (step 2); inside a Caller API route by the panic layer (step 6); inside the hook relay's connection task the task ends and the connection is closed, which the hook reads as `HOOK_REPLY_MALFORMED` and denies. A panic in a long-lived module task ends that task and is reported to the program, which stops the core with exit status 1 on the server and shows the start-failure screen on the desktop; library code never panics on input (patch-mode rule C5).
10. **Repair first.** Where the Setup Assistant can repair the cause (HLD 07), the error is shown only after the repair failed.
11. **Registry check.** `ordin-types` depends on no workspace crate (2B-1 rule 2), so it cannot read another crate's codes; the check is split in two. (a) **Per crate, at each crate's exit:** every crate exports `ALL_CODES: &[&str]` (*chosen here*, one constant beside its error enum), and its own unit test asserts that `ordin_types::error_registry::lookup` finds every entry and that `owns_prefix(<crate>, <prefix>)` is true for each entry's prefix — a code missing from the registry or under a prefix the crate does not own fails that crate's tests. (b) **Workspace source scan, in CI:** every string literal in `crates/` and `apps/` shaped as a code (`^[A-Z]{2,6}_[A-Z0-9_]{2,60}$`) is either a registered code or an environment-variable name of Configuration.md's 2D-1 tables, and every registered code appears in some crate's `ALL_CODES`; any other literal, or a registered code no crate exports, fails the check. There is no single cross-crate test in `ordin-types`. *Amended January 2026 (ADR-114):* the reverse half of (b) — a registered code no crate exports — counts only the crates built with the switches being checked, that is the workspace members a program depends on with those build switches (Cargo features of `apps/desktop` and `apps/server`, OrdinDesktop.md and OrdinServer.md amendments January 2026) turned on; a registered code whose owner crate is not built with those switches is skipped, not failed. *Decoupled January 2026: build switches replace release-numbered features; releases are planned in the maintainers' release roadmap, kept outside the repository.* The forward half (every code-shaped literal is registered) is unchanged.

## Data Model

N/A — the registry is compiled-in data in `ordin-types`; Diagnostics lines belong to the Journal and Artefact Store.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| A module returns a code not in the registry (Service API) | 500 / `Err(permanent)` | `SVC_INTERNAL` | `{"request_id":…,"kind":"error","error":{"code":"SVC_INTERNAL","message":"Something went wrong in ordin.","action":"send_to_it"}}` |
| A command panics | 500 / `Err(permanent)` | `SVC_INTERNAL` | as above |
| A command does not answer by its deadline | 503 / `Err(retryable)` | `SVC_DEADLINE` | as above with `SVC_DEADLINE`, action `try_again` |
| A Caller API route panics or fails without a registered code | 500 | `SRV_INTERNAL` | `{"error": "the server could not answer the request", "code": "SRV_INTERNAL"}` |
| The hook relay ends a connection without a final reply | hook exits 0, deny | `HOOK_REPLY_MALFORMED` | — (harness deny verdict) |
| Any internal error on the relay's path | `deny` | `HOOK_INTERNAL` | `{"kind":"deny","call_id":…,"reason":"…"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Error Handling

  Scenario: Happy path — a module refusal reaches the screens as one sentence
    Given the Approval Service returns APPR_CARD_ALREADY_DECIDED for an approval press
    When the desktop sends approval.decide over the Remote API
    Then the HTTP status is 200
    And the body has kind "error" and error.code "APPR_CARD_ALREADY_DECIDED"
    And error.message is one sentence from the person's locale with no code in it

  Scenario: Edge case — an unregistered code
    Given a module returns the code "XYZ_UNKNOWN" which the registry does not hold
    When the command completes
    Then the reply has code SVC_INTERNAL and action send_to_it
    And an error log line records "XYZ_UNKNOWN"

  Scenario: Edge case — the Caller API keeps wrightrun's envelope
    Given a caller resumes a run that is not paused
    When the Caller API answers
    Then the status is 409
    And the body is exactly {"error": "<sentence>", "code": "RUN_NOT_PAUSED"} with no action key

  Scenario: Error path — a panic in a Caller API route
    Given a route panics while answering
    When the panic layer catches it
    Then the status is 500 with code SRV_INTERNAL
    And a server_error line is logged with the panic's type

  Scenario: Error path — the hook relay fails
    Given the ordin hook asks about a tool call
    When the relay ends the connection before a final reply
    Then the hook exits 0 with a deny verdict
    And HOOK_REPLY_MALFORMED is logged
```

## Performance, Security, Observability

- **Performance targets:** `lookup` is a binary search over a sorted static table, under 1 µs; building a `ServiceError` adds under 1 ms to a command (sentence lookup in a loaded Fluent bundle).
- **Security:** no sentence, envelope or tool text holds a token, path, internal state, tool output or exception text (07); the Caller API body never names a module; the hook path fails closed; codes appear only in Diagnostics, help requests, logs and the start-up report.
- **Observability:** `svc.command` (`request_id`, `command`, `origin`, `outcome` = `ok` or the code, `elapsed_ms`); `server_error` (Caller API); `panic` (location, first 200 characters); the registry's code in every log line that reports an error, as the field `code`. Metrics in the agent profile follow Metrics.md.

## Error-code prefixes and owners

Each prefix with the component specs that raise codes under it and the number of codes each owns. A prefix with two owners is a defect unless SA-ERR-01 or the owning spec sanctions the sharing (column *Note*); the defects are listed in *Registry defects*.

| Prefix | Owner (crate) | Codes | Note |
|---|---|---|---|
| `ADRAFT_` | Authoring Service (`ordin-authoring`) | 7 |  |
| `AFM_` | Agent Folder Manager (`ordin-agent-folders`) | 8 |  |
| `AGI_` | Package Catalogue (`ordin-package-catalogue`) | 7 |  |
| `AM_` | Agent Model (`ordin-agent-model`) | 21 |  |
| `APPR_` | Approval Service (`ordin-approval`); Mail Service (`ordin-mail`) | 28 | 2C card-limit code `APPR_TOO_MANY_RECIPIENTS` enforced by the Mail Service |
| `ARCH_` | Architecture Test (`crates/ordin-arch-test`) | 12 | three added January 2026 (ADR-110) |
| `AREPO_` | Catalogue Service (`ordin-catalogue`) | 8 |  |
| `ART_` | Journal And Artefact Store (`ordin-journal`) | 3 |  |
| `AUTH_` | Caller Authentication (`ordin-caller-auth`) | 7 | wrightrun carried; this crate alone (CallerAuthentication.md; EH-D1 resolved) |
| `AUTHR_` | Authoring Service (`ordin-authoring`) | 29 | renamed from `AUTH_` (EH-D1) |
| `BLOCK_` | Service API (`ordin-service-api`); MCP Front (`ordin-mcp-front`); Ordin Host (`ordin-host`) | 35 | 2C block catalogue; MCP Front and Ordin Host raise their tool-level block refusals under it (McpFront.md, OrdinHost.md Approach) |
| `BRAND_` | Brand (`ordin-brand`) | 16 |  |
| `BRG_` | Command Bridge (`ordin-command-bridge`) | 6 |  |
| `CARD_` | Channel Gateway (`ordin-channel-gateway`) | 2 |  |
| `CAT_` | Catalogue Service (`ordin-catalogue`) | 22 |  |
| `CFG_` | Agent Configuration (`ordin-agent-configuration`) | 47 | wrightrun carried; `CFG_KNOWLEDGE_AUDIENCE_UNATTESTED` added January 2026 (ADR-113); `CFG_KNOWLEDGE_HOST_KIND_MISSING`, `CFG_KNOWLEDGE_HOST_MIXED`, `CFG_KNOWLEDGE_GLOSSARY_RESTRICTED`, `CFG_KNOWLEDGE_CONFLUENCE_RESTRICTED_SPACE`, `CFG_KNOWLEDGE_SHAREPOINT_INCOMPLETE` added January 2026 (ADR-115) |
| `CFL_` | Workspace Service (`ordin-workspaces`) | 4 |  |
| `CHAN_` | Channel Gateway (`ordin-channel-gateway`) | 20 |  |
| `CHAT_` | Channel Gateway (`ordin-channel-gateway`) | 12 | wrightrun carried |
| `CHK_` | Checker (`ordin-checker`) | 10 |  |
| `CHRT_` | Ordin Host (`ordin-host`) | 8 | renamed from `CHARTER_` (EH-D9) |
| `CLA_` | Claude Local Adapter (`ordin-claude-adapter`) | 33 |  |
| `CMD_` | Agent Loader (`ordin-agent-loader`) | 3 | command expansion (AgentLoader.md; EH-D2 resolved) |
| `CODE_` | Project Service (`ordin-projects`) | 5 |  |
| `CONF_` | Confluence Adapter (`ordin-confluence`) | 21 |  |
| `CONN_` | Server Connector (`ordin-server-connector`) | 23 | includes the server-connection field codes (EH-D7) |
| `CPA_` | Copilot Local Adapter (`ordin-copilot-adapter`) | 30 |  |
| `CRAFT_` | Craft Toolkit (`ordin-craft-toolkit`) | 22 |  |
| `CRED_` | Identity And Credentials (`ordin-identity`) | 39 |  |
| `CREW_` | Crew Orchestrator (`ordin-crew-orchestrator`) | 18 |  |
| `CSET_` | Project Service (`ordin-projects`) | 3 |  |
| `DB_` | Run Queue (`ordin-run-queue`) | 1 | wrightrun carried (RunQueue.md; EH-D12 resolved) |
| `DEC_` | Release Builder (`ordin-release-builder`) | 21 | wrightrun carried |
| `DESK_` | Remote API (`ordin-remote-api`) | 8 |  |
| `DICT_` | Dictation Service (`ordin-dictation`) | 11 |  |
| `DSK_` | Ordin Desktop (`apps/desktop`) | 19 |  |
| `ERASE_` | Retention And Erasure (`ordin-retention`) | 6 |  |
| `EXP_` | Checker (`ordin-checker`) | 2 |  |
| `EXT_` | Journal And Artefact Store (`ordin-journal`) | 3 | 2C `ExternalAction` codes, also checked by the adapters that write external actions |
| `FILE_` | File Service (`ordin-files`) | 23 |  |
| `FMT_` | Format Plugins (`ordin-format-plugins`) | 10 |  |
| `FTM_` | Craft Toolkit (`ordin-craft-toolkit`) | 4 |  |
| `FVER_` | File Service (`ordin-files`) | 3 |  |
| `GH_` | Project Service (`ordin-projects`) | 7 |  |
| `GHOP_` | Project Service (`ordin-projects`) | 2 |  |
| `GRANT_` | Workspace Service (`ordin-workspaces`) | 2 |  |
| `HAA_` | Hosted Agent Adapter (`ordin-hosted-agent-adapter`) | 24 |  |
| `HAND_` | Handover Service (`ordin-handover`) | 30 |  |
| `HARN_` | Setup Assistant (`ordin-setup`) | 5 |  |
| `HASH_` | Shared Types (`ordin-types`) | 1 |  |
| `HOME_` | Home And Search (`ordin-home-search`) | 7 |  |
| `HOOK_` | Hook Relay (`ordin-hook-relay`); Ordin Hook (`apps/hook`) | 28 | 2C hook-protocol prefix, raised at both ends (HookRelay.md, OrdinHook.md; EH-D10 resolved) |
| `HPLACE_` | Handover Service (`ordin-handover`) | 2 |  |
| `ID_` | Shared Types (`ordin-types`) | 2 |  |
| `INPUT_` | Shared Types (`ordin-types`) | 3 |  |
| `INST_` | Installers (`ordin-installers`) | 11 |  |
| `INTAKE_` | Caller API (`ordin-caller-api`) | 10 |  |
| `JIRA_` | Jira Adapter (`ordin-jira`) | 15 | added January 2026, ADR-106 |
| `JOB_` | Job Runner (`ordin-job-runner`) | 11 |  |
| `JRNL_` | Journal And Artefact Store (`ordin-journal`) | 19 |  |
| `KNOW_` | Knowledge Service (`ordin-knowledge`) | 50 | wrightrun codes carried unchanged beside ordin's; 7 added January 2026 (ADR-112), four of them the `okf` module's `KNOW_OKF_*` |
| `KQ_` | Knowledge Query Client (`ordin-knowledge-query-client`); Knowledge Query Server (`ordin-knowledge-query-server`) | 25 | added January 2026 (ADR-113); shared and sanctioned: the client passes the server's refusals through to the Service API, and both raise `KQ_NOT_IN_AUDIENCE`, `KQ_STORE_UNAVAILABLE`, `KQ_BAD_QUERY` and `KQ_CONFIG_INVALID` (KnowledgeQueryClient.md, KnowledgeQueryServer.md); `KQ_ACCOUNT_LINK_REFUSED`, `KQ_ASKER_STATEMENT_INVALID`, `KQ_BAD_ACCOUNTS`, `KQ_DELEGATION_NOT_ALLOWED`, `KQ_NOTICE_REJECTED` added January 2026 (ADR-115); the client passes `KQ_BAD_ACCOUNTS` and `KQ_NOTICE_REJECTED` through |
| `KRD_` | Knowledge Reader (`ordin-knowledge-reader`) | 17 | added January 2026 (ADR-113); operator-facing only; `KRD_SOURCE_UNREADABLE`, `KRD_SOURCE_FETCH_FAILED`, `KRD_SOURCE_ITEM_REFUSED`, `KRD_RESTRICTIONS_UNAVAILABLE` added January 2026 (ADR-115) |
| `LANG_` | Project Service (`ordin-projects`) | 3 |  |
| `LIFE_` | Catalogue Service (`ordin-catalogue`); Handover Service (`ordin-handover`); Checker (`ordin-checker`) | 11 | 2C lifecycle codes; Handover Service enforces one |
| `LOAD_` | Agent Loader (`ordin-agent-loader`) | 7 | wrightrun carried |
| `LOC_` | Localisation (`ordin-localisation`) | 21 | *amended January 2026, ADR-109: 14 person-language codes* |
| `MAIL_` | Workspace Service (`ordin-workspaces`); Mail Service (`ordin-mail`) | 45 | 2C mail entities: Workspace Service enforces the mail-link codes (MailService.md; EH-D11 resolved) |
| `MCP_` | MCP Front (`ordin-mcp-front`) | 37 |  |
| `MEM_` | Memory Service (`ordin-memory-service`) | 53 | includes `MEM_SCOPE_CROSS_WRITE` (EH-D5 resolved; KnowledgeService.md cites it) |
| `MPOL_` | Model Policy (`ordin-model-policy`) | 5 |  |
| `NATS_` | NATS Adapter (`ordin-nats`) | 15 |  |
| `NUM_` | Shared Types (`ordin-types`) | 1 |  |
| `OH_` | Ordin Host (`ordin-host`) | 45 | *amended January 2026, ADR-108: `OH_CONVERSATION_NOT_CONTINUABLE`; ADR-109: `OH_DRAFT_RUNNING`, `OH_NOTHING_TO_DRAFT`, `OH_NOT_PERSONS_WORDS`* |
| `OPS_` | Caller API (`ordin-caller-api`) | 5 |  |
| `PART_` | Catalogue Service (`ordin-catalogue`) | 6 | Catalogue Service only (EH-D6 resolved) |
| `PDRAFT_` | Authoring Service (`ordin-authoring`) | 7 |  |
| `PKG_` | Package Catalogue (`ordin-package-catalogue`) | 23 |  |
| `PKI_` | Package Inspector (`ordin-package-inspector`) | 10 |  |
| `PLACE_` | File Service (`ordin-files`) | 2 |  |
| `PRE_` | Release Builder (`ordin-release-builder`) | 14 | wrightrun carried |
| `PROF_` | Organisation Profile (`ordin-organisation-profile`) | 21 |  |
| `PROJ_` | Project Service (`ordin-projects`) | 32 |  |
| `PUB_` | File Service (`ordin-files`) | 4 | File Service only (EH-D4 resolved) |
| `PUSH_` | Push Service (`ordin-push`) | 36 | includes `PUSH_AUDIENCE_GREW`, `PUSH_DELETE_UNCHECKED` (EH-D4) |
| `RA_` | Runtime Adapter Interface (`ordin-runtime-adapter`) | 23 |  |
| `RAG_` | Organisation Profile (`ordin-organisation-profile`) | 3 |  |
| `RAPI_` | Remote API (`ordin-remote-api`) | 7 |  |
| `REH_` | Authoring Service (`ordin-authoring`) | 4 |  |
| `REL_` | Agent Loader (`ordin-agent-loader`) | 19 | wrightrun carried; `REL_EXISTS_DIFFERENT` replaces 2C `REL_REFERENCE_CONFLICT` (D8) |
| `REPORT_` | Journal And Artefact Store (`ordin-journal`) | 12 |  |
| `RES_` | Resolver And Compiler (`ordin-resolver`) | 32 |  |
| `REUSE_` | Usage Planner (`ordin-usage-planner`) | 3 |  |
| `ROUND_` | Crew Orchestrator (`ordin-crew-orchestrator`) | 4 |  |
| `RREC_` | Run Queue (`ordin-run-queue`) | 4 |  |
| `RSTEP_` | Journal And Artefact Store (`ordin-journal`) | 4 |  |
| `RULE_` | Rules Engine (`ordin-rules`) | 26 |  |
| `RUN_` | Run Queue (`ordin-run-queue`) | 9 | wrightrun carried (Run Queue) only; the seven 2C `Run` codes moved to `RUNST_` (EH-D8 resolved). The Session Supervisor writes one carried value, `RUN_LIMIT`, as the `error_code` of its `run_summary` log event, and owns no `RUN_` code |
| `RUNST_` | Session Supervisor (`ordin-session-supervisor`) | 9 | ordin's 2C `Run` entity codes, renamed from `RUN_` (EH-D8 resolved; SessionSupervisor.md Shared Context) |
| `SACC_` | Source Access (`ordin-source-access`) | 10 | added January 2026 (ADR-115); internal only — the Knowledge Query Server maps them to `KQ_` codes on its routes |
| `SBX_` | Sandbox (`ordin-sandbox`) | 30 |  |
| `SCA_` | Slack Channel Adapter (`ordin-slack-channel-adapter`) | 19 |  |
| `SCOPE_` | Retention And Erasure (`ordin-retention`) | 4 | Retention and Erasure only (EH-D5); `SCOPE_KNOWLEDGE_DUPLICATE` added January 2026 (ADR-113) |
| `SERVER_` | Ordin Server (`apps/server`) | 35 | includes `SERVER_ROLE_COMBINATION`, `SERVER_NOTHING_TO_SERVE` (EH-D2); `SERVER_SECRET_NOT_STORABLE`, `SERVER_KNOWLEDGE_NOT_CONFIGURED`, `SERVER_SIGN_IN_TIMED_OUT`, `SERVER_SIGN_IN_REFUSED` added January 2026 (ADR-115) |
| `SET_` | Settings Store (`ordin-settings-store`) | 5 |  |
| `SETUP_` | Setup Assistant (`ordin-setup`) | 40 | 5 added January 2026 (ADR-114) |
| `SLK_` | Slack Service (`ordin-slack-service`) | 52 |  |
| `SP_` | SharePoint Adapter (`ordin-sharepoint`) | 16 |  |
| `SPROF_` | Ordin Server (`apps/server`) | 2 |  |
| `SRC_` | Workspace Service (`ordin-workspaces`) | 5 |  |
| `SRV_` | Caller API (`ordin-caller-api`); Shared Types (`ordin-types`); Remote API (`ordin-remote-api`) | 28 | Shared and sanctioned: wrightrun carried (Caller API), the routing refusals the Remote API raises too, and `SRV_BAD_CURSOR` (Shared types; SA-ERR-01) |
| `STA_` | Slack Thread Adapter (`ordin-slack-thread-adapter`) | 16 |  |
| `START_` | Journal And Artefact Store (`ordin-journal`); Session Supervisor (`ordin-session-supervisor`) | 5 | 2C `StartUpReportItem` codes; the Session Supervisor checks the item list |
| `STEP_` | Crew Orchestrator (`ordin-crew-orchestrator`) | 13 |  |
| `SUP_` | Session Supervisor (`ordin-session-supervisor`) | 13 |  |
| `SVC_` | Service API (`ordin-service-api`); Remote API (`ordin-remote-api`) | 18 | Service API contract prefix (2C); the Remote API raises its transport codes under it; `SVC_NOT_IN_THIS_VERSION` added January 2026 (ADR-114) |
| `TASK_` | Session Supervisor (`ordin-session-supervisor`) | 5 |  |
| `TEAM_` | Team Knowledge Service (`ordin-team-knowledge`) | 41 | added January 2026 (ADR-103); 18 added January 2026 (ADR-110); 3 added January 2026 (ADR-112) |
| `TEL_` | Telemetry (`ordin-telemetry`) | 6 |  |
| `TEV_` | Trigger Service (`ordin-triggers`) | 3 |  |
| `TEXT_` | Shared Types (`ordin-types`) | 1 |  |
| `TG_` | Telegram Service (`ordin-telegram-service`) | 38 |  |
| `TGA_` | Telegram Adapter (`ordin-telegram-adapter`) | 20 |  |
| `TIMER_` | Duty Timer (`ordin-duty-timer`) | 1 |  |
| `TOOL_` | Connection Binder (`ordin-connection-binder`) | 16 | wrightrun carried |
| `TREQ_` | Journal And Artefact Store (`ordin-journal`) | 4 | renamed from `TOOL_` (D8) |
| `TRIG_` | Trigger Service (`ordin-triggers`) | 27 |  |
| `TS_` | Shared Types (`ordin-types`) | 1 |  |
| `UI_` | Screens Module (`apps/desktop/ui/`) | 12 | *amended January 2026, ADR-109: `UI_CATALOGUE_RELOAD_FAILED`* |
| `USAGE_` | Usage Planner (`ordin-usage-planner`) | 8 |  |
| `VOCAB_` | Catalogue Service (`ordin-catalogue`); Authoring Service (`ordin-authoring`) | 5 | 2C vocabulary codes; Authoring Service raises two |
| `VOICE_` | Ordin Host (`ordin-host`) | 3 |  |
| `WCI_` | Workspace Service (`ordin-workspaces`) | 3 |  |
| `WDL_` | Catalogue Service (`ordin-catalogue`) | 4 |  |
| `WF_` | File Service (`ordin-files`) | 6 | wrightrun carried (work folder) |
| `WFF_` | Ordin Host (`ordin-host`) | 43 | renamed from `WF_` (D8) |
| `WS_` | Workspace Service (`ordin-workspaces`) | 32 |  |
| `YAML_` | Shared Types (`ordin-types`) | 3 | restricted YAML reader `ordin_types::yaml` |

## Error-code registry

**2042 codes, 79 owning components.** *Amended January 2026 (ADR-115): 28 codes added from the new Source Access (10, prefix `SACC_`) and the Agent Configuration (5), Knowledge Query Server (5, `KQ_`), Knowledge Reader (4) and ordin server (4) amendments; one owner added.* *Amended January 2026 (ADR-114): 6 codes added — `SVC_NOT_IN_THIS_VERSION` (Service API) and `SETUP_SELF_UPDATE_UNAVAILABLE`, `SETUP_UPDATE_NO_ARCHIVE`, `SETUP_UPDATE_REPLACE_FAILED`, `SETUP_UPDATE_IN_PROGRESS`, `SETUP_UPDATE_VERSION_CHANGED` (Setup Assistant); no owner added.* *Amended January 2026 (ADR-112, ADR-113): 50 codes added from the new Knowledge Query Client and Knowledge Query Server (20, prefix `KQ_`, shared by the two) and Knowledge Reader (13, `KRD_`), and the Knowledge Service (7), Memory Service (3), Team Knowledge Service (3), Agent Configuration (1), Ordin Server (1), Screens Module (1) and Phase 2C scope rule (1) amendments; the count before was 1958 rows, counted on January 2026, not the 1915 this line stated.* *Amended January 2026 (ADR-109): 18 codes added from the Localisation (14), Ordin Host (3) and Screens Module (1) amendments.* *Amended January 2026 (ADR-108): 1 code added from the Ordin Host amendment (`OH_CONVERSATION_NOT_CONTINUABLE`). Amended January 2026 (ADR-107): 9 codes added from the Agent Configuration (4), Model Policy (1), Claude Local Adapter (1), Rules Engine (1), Connection Binder (1) and Slack Channel Adapter (1) amendments; 1 removed (`MPOL_PROVIDER_NOT_READY`). Amended January 2026 (ADR-106): 25 codes added from the Jira Adapter (15) and the Handover (5), Workspace (4) and Catalogue (1) Services' amendments. Amended January 2026 (ADR-104): 5 codes added from the Knowledge (4) and File (1) Services' amendments. Amended January 2026 (ADR-103): 28 codes added from the Team Knowledge Service (20) and the Memory (4), Knowledge (1) and Push (3) Services' amendments. Amended January 2026 (ADR-102): 66 codes added from the Slack Service and the amended specs.* Generated from the component specs and the Phase 2C files on January 2026 (see *Approach*); regenerate rather than edit by hand. Columns:

- **Owner** — the component spec whose crate raises the code; *also* names specs that pass it through, map it to their surface, or check the same 2C rule.
- **Surface** — where a caller outside the crate meets the code: *Service API* (a `CommandReply` error over Tauri IPC or the Remote API, through `CodedError`); *Remote API* (an HTTP-level refusal of the transport); *Caller API* (wrightrun envelope, agent profile); *hook* (a deny to the ordin hook); *MCP tool* (a tool result the model reads); *internal* (a start refusal, exit status, run or job outcome, journal or log record that no API returns as such); *log field only* (a carried value written only into a log event's field, never returned or recorded as an outcome). A module code of a desktop or personal-server module is listed as *Service API* when its owner spec returns it as `Err`, because a command that calls the module propagates it.
- **HTTP status** — Caller API status, Remote API transport status, or `200 (kind: error)` for a Service API refusal carried by the Remote API (2C table); `—` where no HTTP surface carries it.
- **Retryable** — from the owner row: `Err(retryable)`, 503 or 429 → yes; `yes / no (per row)` where the owner raises it for both kinds of cause.
- **Src** — `ET` the owner's Error Table; `2C` a Phase 2C validation code the owner copies into its Shared Context; `2C-only` a Phase 2C validation code no component spec copies, registered under the crate that owns its prefix (for `START_`, `STEP_` the crate that checks the 2C entity); `IL` a code *chosen here* in a validation table of the owner's Internal Logic or Public Interface, with no Error Table row of its own.

| Code | Owner | Surface | HTTP status | Retryable | Meaning | Src |
|---|---|---|---|---|---|---|
| `ADRAFT_BRANCH_INVALID` | Authoring Service | Service API | 200 (`kind: error`) | no | branch: valid git ref, ≤ 255 bytes | 2C |
| `ADRAFT_LIST_TOO_LONG` | Authoring Service | Service API | 200 (`kind: error`) | no | Draft field out of its 2C range | ET |
| `ADRAFT_NAME_INVALID` | Authoring Service | Service API | 200 (`kind: error`) | no | Draft field out of its 2C range | ET |
| `ADRAFT_PR_ALREADY_OPEN` | Authoring Service | Service API | 200 (`kind: error`) | no | A pull request is already open | ET |
| `ADRAFT_PURPOSE_INVALID` | Authoring Service | Service API | 200 (`kind: error`) | no | Draft field out of its 2C range | ET |
| `ADRAFT_REHEARSAL_STALE` | Authoring Service | Service API | 200 (`kind: error`) | no | Draft changed since rehearsal | ET |
| `ADRAFT_SIGNING_NOT_REQUIRED` | Authoring Service | Service API | 200 (`kind: error`) | no | state = approved_not_signed_yet: only where the repository requires signed merges (ADR-084) | 2C |
| `AFM_DATASTORE_FAILED` | Agent Folder Manager | Service API | 200 (`kind: error`) | yes / no (per row) | Datastore busy or locked | ET |
| `AFM_FOLDER_BUSY` | Agent Folder Manager | Service API | 200 (`kind: error`) | yes | A lease conflicts and the wait ran out, or another process holds the OS lock | ET |
| `AFM_FOLDER_EXISTS` | Agent Folder Manager | Service API | 200 (`kind: error`) | no | Folder or row already exists for the instance | ET |
| `AFM_FOLDER_NOT_FOUND` | Agent Folder Manager | Service API | 200 (`kind: error`) | no | No row in state `created` for the instance | ET |
| `AFM_GRANT_INVALID` | Agent Folder Manager | Service API | 200 (`kind: error`) | no | `set_workspace_grants` with a grant of another workspace | ET |
| `AFM_IO_FAILED` | Agent Folder Manager | Service API | 200 (`kind: error`) | yes | File-system operation fails | ET |
| `AFM_ROOT_UNWRITABLE` | Agent Folder Manager | Service API | 200 (`kind: error`) | no | Data root cannot be created or written | ET |
| `AFM_SESSION_UNKNOWN` | Agent Folder Manager | Service API | 200 (`kind: error`) | no | `session_access` for a run with no applied grants and no instance | ET |
| `AGI_COMMIT_INVALID` | Package Catalogue | Service API | 200 (`kind: error`) | no | commit: 40 lowercase hex chars when present | 2C |
| `AGI_FOLDER_INVALID` | Package Catalogue | Service API | 200 (`kind: error`) | no | agent_folder_path: absolute, under the application data root, unique | 2C |
| `AGI_NAME_INVALID` | Package Catalogue | Service API | 200 (`kind: error`) | no | display_name: 1–120 chars (assumed) | 2C |
| `AGI_PACKAGE_NAME_INVALID` | Package Catalogue | Service API | 200 (`kind: error`) | no | package_name: 1–214 chars `[a-z0-9._-]` (assumed) | 2C |
| `AGI_PACKAGE_VERSION_INVALID` | Package Catalogue | Service API | 200 (`kind: error`) | no | package_version: 1–64 chars (assumed) | 2C |
| `AGI_REPOSITORY_MISMATCH` | Package Catalogue | Service API | 200 (`kind: error`) | no | commit, agent_repository_id: both present or both absent | 2C |
| `AGI_STATE_INCONSISTENT` | Package Catalogue | Service API | 200 (`kind: error`) | no | status: `refused` only when `audit_result = failed`, the Checker refused it, or `stopped_reason` is present (amended January 2026, decision 80: C-22) | 2C |
| `AM_API_VERSION_UNKNOWN` | Agent Model | Service API | 200 (`kind: error`) | no | `apiVersion` is not `ai.enterprise/v1` when `Manifest::require_api_version` is called | ET |
| `AM_DIGEST_INVALID` | Agent Model | Service API | 200 (`kind: error`) | no | `sha256:` digest text malformed | ET |
| `AM_DRAFT_FIELDS_INVALID` | Agent Model | Service API | 200 (`kind: error`) | no | Draft fields not an object, `metadata.id` missing, a typed field of the wrong type, a non-integer number without `$decimal`, an empty draft name | ET |
| `AM_DRAFT_KIND_NOT_COMPOSABLE` | Agent Model | Service API | 200 (`kind: error`) | no | A generated or reused part of a kind a WorkspaceProfile cannot reference, or a second Memory or ControlLoop | ET |
| `AM_FIELD_SHAPE_INVALID` | Agent Model | Service API | 200 (`kind: error`) | no | A typed key is present with the wrong shape | ET |
| `AM_FILE_TOO_LARGE` | Agent Model | Service API | 200 (`kind: error`) | no | A file is larger than 1 MiB | ET |
| `AM_ID_INVALID` | Agent Model | Service API | 200 (`kind: error`) | no | Identifier does not parse as a crucible URN where one is required | ET |
| `AM_KIND_UNKNOWN` | Agent Model | Service API | 200 (`kind: error`) | no | Authored kind maps to no kind | ET |
| `AM_LOCK_FILE_INVALID` | Agent Model | Service API | 200 (`kind: error`) | no | Lock file YAML does not parse, schema is not `ordin.lock/v1`, or a required key is missing | ET |
| `AM_MANIFEST_FIELD_MISSING` | Agent Model | Service API | 200 (`kind: error`) | no | `apiVersion`, `kind` or `metadata.id` missing | ET |
| `AM_MANIFEST_MALFORMED` | Agent Model | Service API | 200 (`kind: error`) | no | Front matter opened with `---` but never closed, YAML syntax error, top level not a mapping, non-string or duplicate key | ET |
| `AM_NO_FILE_CONVENTION` | Agent Model | Service API | 200 (`kind: error`) | no | Render or placement asked for a kind with no file convention | ET |
| `AM_PATH_INVALID` | Agent Model | Service API | 200 (`kind: error`) | no | Path absolute, with `..`, `\`, NUL, non-UTF-8, or not representable in ustar | ET |
| `AM_SOURCE_CORRUPT` | Agent Model *(also: Catalogue Service)* | Service API | 200 (`kind: error`) | no | `PartSource` files do not hash to the part's digest | ET |
| `AM_SOURCE_UNAVAILABLE` | Agent Model *(also: Catalogue Service)* | Service API | 200 (`kind: error`) | yes | `PartSource` cannot read its clone or index | ET |
| `AM_SOURCE_WITHDRAWN_LIST_UNREADABLE` | Agent Model *(also: Catalogue Service)* | Service API | 200 (`kind: error`) | no | `PartSource` has no readable withdrawn list and no earlier copy | ET |
| `AM_STATUS_UNKNOWN` | Agent Model | Service API | 200 (`kind: error`) | no | `metadata.status` is not a known status term | ET |
| `AM_VERSION_INVALID` | Agent Model | Service API | 200 (`kind: error`) | no | Version does not match the version pattern | ET |
| `AM_VOCABULARY_AXIS_DUPLICATE` | Agent Model | Service API | 200 (`kind: error`) | no | Two vocabulary parts for one axis | ET |
| `AM_WITHDRAWN_LIST_INVALID` | Agent Model | Service API | 200 (`kind: error`) | no | `withdrawn.yaml` not a mapping, wrong `listSchema`, `entries` not a list, an entry of the wrong shape or with an unknown key | ET |
| `AM_YAML_FEATURE_REFUSED` | Agent Model | Service API | 200 (`kind: error`) | no | Anchor, alias or custom tag in a manifest | ET |
| `APPR_ACTION_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | A proposal field breaks its limit (B.1) | ET |
| `APPR_AFFECTS_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | A card field breaks its limit (B.1) | ET |
| `APPR_ALREADY_DECIDED` | Approval Service | Service API | 200 (`kind: error`) | no | decision: only from `pending` | 2C-only |
| `APPR_ASKER_MAY_NOT_APPROVE` | Approval Service *(also: Caller API)* | Service API, Caller API | 403 (Caller API); 200 `kind: error` (Remote API) | no | Asker approves own action that leaves the company (C.3) | ET |
| `APPR_ATTACHMENT_NOT_ALLOWED` | Approval Service | Service API | 200 (`kind: error`) | no | attachments: none from places the profile excludes (ADR-070) | 2C-only |
| `APPR_CARD_ALREADY_DECIDED` | Approval Service *(also: Remote API, Screens Module)* | Service API, Remote API | 200 (Caller API); 200 `kind: error` (Remote API) | no | Already decided elsewhere (C.2, C.6) | ET |
| `APPR_CARD_CLOSED` | Approval Service *(also: Caller API)* | Service API, Caller API | 409 (Caller API); 200 `kind: error` (Remote API) | no | The run ended first (C.2) | ET |
| `APPR_CARD_REF_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | `CardRef`: 22 characters base64url (16 random bytes), fits Telegram's 64-byte button data (`04-domain.md:277`) | 2C-only |
| `APPR_CARD_REF_MISMATCH` | Approval Service | Service API | 200 (`kind: error`) | no | Reference of the other button (C.1) | ET |
| `APPR_CARD_REF_UNKNOWN` | Approval Service | Service API | 200 (`kind: error`) | no | Press with an unknown reference (C.1) | ET |
| `APPR_CHANGES_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | A card field breaks its limit (B.1) | ET |
| `APPR_CONSEQUENCE_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | A proposal field breaks its limit (B.1) | ET |
| `APPR_CONTENT_LABEL_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | A card field breaks its limit (B.1) | ET |
| `APPR_CONTENT_TOO_LARGE` | Approval Service | Service API | 200 (`kind: error`) | no | A card field breaks its limit (B.1) | ET |
| `APPR_DECISION_MISMATCH` | Approval Service | Service API | 200 (`kind: error`) | no | decided_by, decided_at: present if and only if state ∈ {approved, rejected} | 2C-only |
| `APPR_DESTINATION_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | A proposal field breaks its limit (B.1) | ET |
| `APPR_EMIT_FAILED` | Approval Service | internal | — | no | Journal emit fails twice (B.6, C.8, K.4, L) | ET |
| `APPR_NOT_AN_APPROVER` | Approval Service *(also: Caller API)* | Service API, Caller API | 403 (Caller API); 200 `kind: error` (Remote API) | no | Press from someone who is not an approver (C.3, C.4) | ET |
| `APPR_NOT_FOUND` | Approval Service | Service API | 200 (`kind: error`) | no | Unknown approval id (D.2, E.1, `get`) | ET |
| `APPR_OPERATOR_NOT_ALLOWED` | Approval Service *(also: Caller API)* | Service API, Caller API | 409 (Caller API); 200 `kind: error` (Remote API) | no | Operator decides while approvers are named (D.1) | ET |
| `APPR_QUESTION_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | A card field breaks its limit (B.1) | ET |
| `APPR_RECIPIENT_NOT_ALLOWED` | Approval Service | Service API | 200 (`kind: error`) | no | recipients: inside the company only, when the profile says so (ADR-070) | 2C-only |
| `APPR_RECORDS_TOO_MANY` | Approval Service | Service API | 200 (`kind: error`) | no | affected_records: ≤ 1000 entries (assumed) | 2C-only |
| `APPR_SHOW_DETAILS_WITHOUT_CONTENT` | Approval Service | Service API | 200 (`kind: error`) | no | `show_details` without content (B.1) | ET |
| `APPR_STORE_UNAVAILABLE` | Approval Service | Service API | 200 (`kind: error`) | yes | Store read or write fails | ET |
| `APPR_TOO_MANY_RECIPIENTS` | Mail Service | Service API | 200 (`kind: error`) | no | `to + cc + bcc`: 1 to the profile's maximum recipients (default 20, `09-decisions.md:1596`) | 2C |
| `APPR_VERB_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | A card field breaks its limit (B.1) | ET |
| `APPR_WHERE_INVALID` | Approval Service | Service API | 200 (`kind: error`) | no | A card field breaks its limit (B.1) | ET |
| `ARCH_CYCLE` | Architecture Test | internal | — | no | A cycle among normal and build edges | ET |
| `ARCH_DEV_EDGE_NOT_ALLOWED` | Architecture Test | internal | — | no | A dev edge to a member of an equal or higher phase | ET |
| `ARCH_EDGE_NOT_ALLOWED` | Architecture Test | internal | — | no | A normal or build edge between members not listed | ET |
| `ARCH_EMBEDDED_EDGE` | Architecture Test | internal | — | no | `memory` or `sandboxd` used by a crate other than its one wrapper | ET |
| `ARCH_METADATA_FAILED` | Architecture Test | internal | — | no | `cargo metadata` exits non-zero (no lock file, a manifest error, `--locked` with an out-of-date `Cargo.lock`) | ET |
| `ARCH_METADATA_INVALID` | Architecture Test | internal | — | no | Standard output is not the expected JSON, or `resolve` is `null` | ET |
| `ARCH_PERSON_ONLY_CALLED` | Architecture Test | internal | — | no | An adopt method of the Team Knowledge Service is called outside the Service API dispatch modules (ADR-110) | ET |
| `ARCH_PERSON_ONLY_NAMED` | Architecture Test | internal | — | no | A command only a person may send is named outside the catalogue and the dispatch modules (ADR-110) | ET |
| `ARCH_SOURCE_UNREADABLE` | Architecture Test | internal | — | no | A workspace source file does not parse during the person-only scan (ADR-110) | ET |
| `ARCH_TABLE_INVALID` | Architecture Test | internal | — | no | The compiled-in table lists an edge to an equal or higher phase, or a duplicate row | ET |
| `ARCH_UNKNOWN_CRATE` | Architecture Test | internal | — | no | A workspace member is not a row of the table | ET |
| `ARCH_UNKNOWN_PATH_PACKAGE` | Architecture Test | internal | — | no | A path package that is neither a member nor embedded | ET |
| `AREPO_ADD_DISABLED` | Catalogue Service | Service API | 200 (`kind: error`) | no | Profile turns adding off | ET |
| `AREPO_BRANCH_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | protected_branch: 1–255 bytes, a valid git ref name | 2C |
| `AREPO_COMMIT_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | last_fetched_commit: 40 lowercase hex chars | 2C |
| `AREPO_DUPLICATE` | Catalogue Service | Service API | 200 (`kind: error`) | no | Repository already read | ET |
| `AREPO_HOST_UNSUPPORTED` | Catalogue Service | Service API | 200 (`kind: error`) | no | Host is not GitHub or a profile host | ET |
| `AREPO_NAME_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | Web address not a repository URL | ET |
| `AREPO_REMOVE_NOT_OWN` | Catalogue Service | Service API | 200 (`kind: error`) | no | Remove or rename a company repository | ET |
| `AREPO_SIGNED_MERGES_MISMATCH` | Catalogue Service | Service API | 200 (`kind: error`) | no | signed_merges_required: present if and only if `source = profile` | 2C |
| `ART_DELETE_IN_PLACE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | deletion in a person's place: moved to the system's bin, never deleted (04:204) | 2C |
| `ART_MEDIA_TYPE_INVALID` | Journal And Artefact Store *(also: Sandbox)* | Service API | 200 (`kind: error`) | no | media_type: RFC 6838 `type/subtype`, ≤ 255 chars | 2C |
| `ART_PATH_INVALID` | Journal And Artefact Store *(also: Sandbox)* | Service API | 200 (`kind: error`) | no | path: relative to its folder, ≤ 4096 bytes | 2C |
| `AUTHR_APM_WITHDRAW_ONLY` | Authoring Service | Service API | 200 (`kind: error`) | no | APM repository: only withdrawal | ET |
| `AUTHR_ARCHIVE_INVALID` | Authoring Service | Service API | 200 (`kind: error`) | no | Archive import with an unsafe name, over 2,000 entries or over 50 MB unpacked, or not a readable ZIP file | ET |
| `AUTHR_CHECKS_FAILING` | Authoring Service | Service API | 200 (`kind: error`) | no | Rehearse with failing checks | ET |
| `AUTHR_CHECK_FAILED` | Authoring Service | Service API | 200 (`kind: error`) | no | Checker refuses the request (`CHK_INPUT_INVALID`, `CHK_TOO_MANY_FINDINGS`, `CHK_RUN_NOT_FOUND`, `CHK_CONTRACT_INVALID`, `CHK_SOURCE_ALREADY_BOUND`) | ET |
| `AUTHR_CHECK_UNAVAILABLE` | Authoring Service | Service API | 200 (`kind: error`) | yes | Checker unavailable | ET |
| `AUTHR_CONTRACT_EMPTY` | Authoring Service | Service API | 200 (`kind: error`) | no | Role-dossier contract has no requirement sections | ET |
| `AUTHR_DRAFT_NOT_EDITABLE` | Authoring Service | Service API | 200 (`kind: error`) | no | Draft not editable in its state | ET |
| `AUTHR_DRAFT_UNKNOWN` | Authoring Service | Service API | 200 (`kind: error`) | no | Unknown draft | ET |
| `AUTHR_FIELDS_INVALID` | Authoring Service | Service API | 200 (`kind: error`) | no | Part fields refused by the Agent Model (`AM_DRAFT_FIELDS_INVALID`, not size) | ET |
| `AUTHR_FINDING_UNKNOWN` | Authoring Service | Service API | 200 (`kind: error`) | no | Fix for a finding not in the report | ET |
| `AUTHR_FORMAT_NOT_RECOGNISED` | Authoring Service | Service API | 200 (`kind: error`) | no | Import format not recognised | ET |
| `AUTHR_FORMAT_NOT_WRITABLE` | Authoring Service | Service API | 200 (`kind: error`) | no | Target format cannot be written (`FMT_WRITE_UNSUPPORTED`) | ET |
| `AUTHR_FOUNDRY_FAILED` | Authoring Service | Service API | 200 (`kind: error`) | yes / no (per row) | Foundry or generation failed | ET |
| `AUTHR_IDENTIFIER_TAKEN` | Authoring Service | Service API | 200 (`kind: error`) | no | New part identifier taken | ET |
| `AUTHR_IMPORT_FAILED` | Authoring Service | Service API | 200 (`kind: error`) | no | Import read failed (any `FormatError` other than `FMT_NOT_RECOGNISED`) | ET |
| `AUTHR_JOB_TASKS_INVALID` | Authoring Service | Service API | 200 (`kind: error`) | no | `job_tasks` names a job index or task the draft does not hold | ET |
| `AUTHR_KIND_NOT_WRITABLE` | Authoring Service | Service API | 200 (`kind: error`) | no | Kind not written as a part draft | ET |
| `AUTHR_NOT_READY_TO_PUBLISH` | Authoring Service | Service API | 200 (`kind: error`) | no | Publish before checks pass | ET |
| `AUTHR_NOT_READY_TO_TRY` | Authoring Service | Service API | 200 (`kind: error`) | no | Try before every check and rehearsal pass | ET |
| `AUTHR_NO_CHANGE_REQUESTED` | Authoring Service | Service API | 200 (`kind: error`) | no | Send change without a change request | ET |
| `AUTHR_PUBLISH_CONTEXT_UNEXPECTED` | Authoring Service | Service API | 200 (`kind: error`) | no | `PublishContext` given for a draft that is not a lifecycle | ET |
| `AUTHR_REHEARSAL_FAILED` | Authoring Service | Service API | 200 (`kind: error`) | yes | Rehearsal run failed | ET |
| `AUTHR_SEND_FAILED` | Authoring Service | Service API | 200 (`kind: error`) | no | Push Service refuses the pull-request proposal | ET |
| `AUTHR_STARTING` | Authoring Service | Service API | 200 (`kind: error`) | yes | `lifecycle_of` called before `run` loaded the drafts | ET |
| `AUTHR_STORE_FAILED` | Authoring Service | Service API | 200 (`kind: error`) | yes / no (per row) | Prune of authoring history fails | ET |
| `AUTHR_TOOL_FILE_CONTENT` | Authoring Service | MCP tool | — | no | Tool call carrying file contents | ET |
| `AUTHR_TOOL_NOT_FOR_SESSION` | Authoring Service | MCP tool | — | no | Tool called by another session | ET |
| `AUTHR_TRIAL_FAILED` | Authoring Service | Service API | 200 (`kind: error`) | yes | Trial refused by the Ordin Host or the Package Catalogue for a reason other than trust | ET |
| `AUTHR_TRIAL_NEEDS_TRUST` | Authoring Service | Service API | 200 (`kind: error`) | no | Trial needs IT approval | ET |
| `AUTH_ALGORITHM` | Caller Authentication | internal | — | no | `auth.algorithms` holds a value outside the accepted eight, at `Verifier::new` | ET |
| `AUTH_FORBIDDEN` | Caller Authentication *(also: Caller API)* | Caller API | 403 | no | A caller token on an operator route | ET |
| `AUTH_INVALID` | Caller Authentication *(also: Caller API)* | Caller API | 401 | no | Not a token; unknown `kid`; signature, expiry, not-before, issuer, audience, required claim, algorithm or `sub` fails | ET |
| `AUTH_KEYS_UNAVAILABLE` | Caller Authentication *(also: Caller API)* | Caller API | 503 | yes | Keys unreadable within 5 s or unusable, and no kept set younger than 300 s holds the `kid` | ET |
| `AUTH_MISSING` | Caller Authentication *(also: Caller API)* | Caller API | 401 | no | No `Authorization` header, or not `Bearer <token>`, or a token over 16,384 bytes | ET |
| `AUTH_NO_ROLE` | Caller Authentication *(also: Caller API)* | Caller API | 403 | no | Valid token whose role claim holds neither value | ET |
| `AUTH_ROUTE_UNMAPPED` | Caller Authentication *(also: Caller API)* | internal, Caller API | 403 | no | A route not in `ROUTE_ROLES`, at a request | ET |
| `BLOCK_AS_OF_INVALID` | Service API | Service API | 200 (`kind: error`) | no | report: `title`, `heading` 1–120; `as_of` ≤ now + 5 minutes | 2C |
| `BLOCK_CELL_TOO_LONG` | Service API | Service API | 200 (`kind: error`) | no | `Text` cell: 0–2,000 characters (assumed) | 2C |
| `BLOCK_CELL_TYPE_MISMATCH` | Service API | Service API | 200 (`kind: error`) | no | cell type: matches its column's type; numbers finite (no NaN, no infinity) | 2C |
| `BLOCK_CHART_AXIS_INVALID` | Service API | Service API | 200 (`kind: error`) | no | chart: `x` names a column; scatter `x` numeric | 2C |
| `BLOCK_CHART_INVALID` | Service API | Service API | 200 (`kind: error`) | no | titles: 1–120 characters (assumed) | 2C |
| `BLOCK_CHART_PIE_INVALID` | Service API | Service API | 200 (`kind: error`) | no | `Pie`: exactly 1 series and at most 6 rows (ADR-067) | 2C |
| `BLOCK_CHART_SERIES_INVALID` | Service API | Service API | 200 (`kind: error`) | no | `series`: 1–8 (A-34); each names a numeric column | 2C |
| `BLOCK_CHART_TOO_MANY_POINTS` | Service API | Service API | 200 (`kind: error`) | no | points: `rows × series` ≤ 5,000 (A-34) | 2C |
| `BLOCK_CHOICES_INVALID` | Service API | Service API | 200 (`kind: error`) | no | question: `question` 1–300; `choices` 2–4, each 1–60, unique | 2C |
| `BLOCK_CURRENCY_INVALID` | Service API | Service API | 200 (`kind: error`) | no | `Money.currency`: `^[A-Z]{3}$` (ISO 4217) | 2C |
| `BLOCK_EMAIL_NOT_LINKED` | Service API | MCP tool | — | no | Email message not in the workspace's links | ET |
| `BLOCK_FIELD_UNKNOWN` | Service API | MCP tool | — | no | Unknown block kind / field | ET |
| `BLOCK_FIGURES_COUNT` | Service API | Service API | 200 (`kind: error`) | no | key figures: `figures` 1–8 | 2C |
| `BLOCK_FIGURES_INVALID` | Service API | Service API | 200 (`kind: error`) | no | key figures: `label` 1–60, `unit` 1–24, `against` 1–60; values finite | 2C |
| `BLOCK_FORM_INVALID` | Service API | Service API | 200 (`kind: error`) | no | form: `fields` 1–20; `name` `^[a-z][a-z0-9_]{0,31}$` unique; `label` 1–80; `Text.value` 0–16,384; `Choice.options` 2–20; `verb` not an approval verb | 2C |
| `BLOCK_KIND_UNKNOWN` | Service API | MCP tool | — | no | Unknown block kind / field | ET |
| `BLOCK_LINK_NOT_ALLOWED` | Service API | MCP tool | — | no | URL where a reference is expected | ET |
| `BLOCK_OFFER_UNKNOWN` | Service API | Service API | 200 (`kind: error`) | no | result: `summary` 1–2,000; offer from the closed list | 2C |
| `BLOCK_PATH_INVALID` | Service API | Service API | 200 (`kind: error`) | no | `relative_path`: 1–1,024 bytes, no `..`, no leading `/` | 2C |
| `BLOCK_PLAN_INVALID` | MCP Front | MCP tool | — | no | `plan` state breaks 3–7 steps or 1–80 characters | ET |
| `BLOCK_PLAN_POSITION_INVALID` | MCP Front | MCP tool | — | no | `plan` mark position outside the plan | ET |
| `BLOCK_PLAN_TOO_MANY_VERSIONS` | MCP Front | MCP tool | — | no | A 21st plan version | ET |
| `BLOCK_REFUSED` | Ordin Host | MCP tool | — | no | `show` block refused by the catalogue check | ET |
| `BLOCK_REF_UNRESOLVED` | Service API | MCP tool | — | no | A reference not found, or not readable by the person | ET |
| `BLOCK_REPORT_INVALID` | Service API | Service API | 200 (`kind: error`) | no | `title`, `heading` 1–120; `as_of` ≤ now + 5 minutes | IL |
| `BLOCK_REPORT_PARTS_COUNT` | Service API | Service API | 200 (`kind: error`) | no | report: `parts` 1–12 | 2C |
| `BLOCK_REPORT_REFUSED` | MCP Front | MCP tool | — | no | `report` arguments fail the block check entirely | ET |
| `BLOCK_SOURCE_INVALID` | Service API | Service API | 200 (`kind: error`) | no | part source: `documents` 1–20; `version` 1–64; `system` 1–80; `by` 1–80 and the producing run's agent | 2C |
| `BLOCK_SOURCE_MISSING` | Service API | Service API | 200 (`kind: error`) | no | `source`: required (ADR-067) | 2C |
| `BLOCK_TABLE_INVALID` | Service API | Service API | 200 (`kind: error`) | no | `title` / `name`: 1–120 / 1–60 characters (assumed); column names unique | 2C |
| `BLOCK_TABLE_TOO_LONG` | Service API | Service API | 200 (`kind: error`) | no | table: `columns` 1–30 / `rows` 0–10,000 | 2C |
| `BLOCK_TABLE_TOO_WIDE` | Service API | Service API | 200 (`kind: error`) | no | table: `columns` 1–30 / `rows` 0–10,000 | 2C |
| `BLOCK_TEXT_INVALID` | Service API | Service API | 200 (`kind: error`) | no | message: `text` 1–8,000; `attachments` ≤ 20 | 2C |
| `BLOCK_TOO_LARGE` | Service API | MCP tool | — | no | Block over 2 MiB | ET |
| `BLOCK_TOO_MANY_ITEMS` | Service API | Service API | 200 (`kind: error`) | no | message: `text` 1–8,000; `attachments` ≤ 20 | 2C |
| `BRAND_ASSET_FETCH_FAILED` | Brand | Service API | 200 (`kind: error`) | yes | Asset download fails after 3 attempts (network, 5xx, 429) | ET |
| `BRAND_ASSET_HASH_MISMATCH` | Brand | internal | — | no | Asset body's SHA-256 differs from the section | ET |
| `BRAND_ASSET_INVALID` | Brand | internal | — | no | Asset too large, wrong media type, SVG with active content or external reference, HTTP 4xx | ET |
| `BRAND_ASSET_UNKNOWN` | Brand | Service API | 200 (`kind: error`) | no | `brand.get_asset` with a font index out of range | ET |
| `BRAND_CONTRAST_FAILED` | Brand | internal | — | no | A pair below its contrast threshold | ET |
| `BRAND_FIXED_TOKEN` | Brand | internal | — | no | Token sets a fixed safety colour | ET |
| `BRAND_ID_INVALID` | Brand | internal | — | no | Brand identifier invalid | ET |
| `BRAND_LOCALE_DUPLICATE` | Brand | internal | — | no | Two locales equal after canonicalisation | ET |
| `BRAND_REFUSAL_REASON_MISMATCH` | Brand | Service API | 200 (`kind: error`) | no | A stored row with `refused` and no reason, or the reverse | ET |
| `BRAND_STORE_UNAVAILABLE` | Brand | Service API | 200 (`kind: error`) | yes | `brand_packs` unreadable or unwritable | ET |
| `BRAND_STRINGS_MISSING` | Brand | internal | — | no | No strings, or a locale lacking legal text or strings | ET |
| `BRAND_STRING_INVALID` | Brand | internal | — | no | A brand string outside its length | ET |
| `BRAND_SUPERSEDED` | Brand | Service API | 200 (`kind: error`) | no | A `received` row replaced by a newer pack before checking | ET |
| `BRAND_TOKENS_INVALID` | Brand | internal | — | no | Token name outside the four namespaces, bad value form, a required colour missing, or over 500 tokens | ET |
| `BRAND_URL_INVALID` | Brand | internal | — | no | A link not `https` or over 2048 bytes | ET |
| `BRAND_VERSION_INVALID` | Brand | internal | — | no | Version not `MAJOR.MINOR.PATCH` | ET |
| `BRG_BACKEND_UNKNOWN` | Command Bridge | Service API | 200 (`kind: error`) | no | `service_events` names an unknown backend | ET |
| `BRG_CLOSING` | Command Bridge *(also: Ordin Desktop)* | Service API | 200 (`kind: error`) | no | Command after `close` began | ET |
| `BRG_JOURNAL_LAGGED` | Command Bridge | internal | — | no | The Bridge's Journal queue overflowed | ET |
| `BRG_STREAM_LAGGING` | Command Bridge | Service API | 200 (`kind: error`) | no | `ScreenStreams::send` with the window's stream queue full | ET |
| `BRG_STREAM_NO_WINDOW` | Command Bridge | Service API | 200 (`kind: error`) | no | `ScreenStreams::send` with no open `service_streams` channel | ET |
| `BRG_WINDOW_LAGGING` | Command Bridge | Service API | 200 (`kind: error`) | no | Window not reading events | ET |
| `CARD_ALREADY_USED` | Channel Gateway | Service API | 200 (`kind: error`) | no | Card reference unknown, closed or already used | ET |
| `CARD_REFERENCE_INVALID` | Channel Gateway | Service API | 200 (`kind: error`) | no | Card reference unknown, closed or already used | ET |
| `CAT_BRANCH_EXISTS` | Catalogue Service | Service API | 200 (`kind: error`) | no | Draft branch already exists | ET |
| `CAT_CHECK_FAILED` | Catalogue Service | internal | — | no | Ingest: a check failure that names no part | ET |
| `CAT_CHECK_UNAVAILABLE` | Catalogue Service | internal | — | no | Commit refused (format changed, too large, duplicate, checker down) | ET |
| `CAT_FETCH_FAILED` | Catalogue Service | Service API | 200 (`kind: error`) | yes | Git fetch or clone failed otherwise | ET |
| `CAT_FORMAT_CHANGED` | Catalogue Service | internal | — | no | Commit refused (format changed, too large, duplicate, checker down) | ET |
| `CAT_FORMAT_UNKNOWN` | Catalogue Service | Service API | 200 (`kind: error`) | no | No format plug-in recognises the tree | ET |
| `CAT_LIFECYCLE_UNKNOWN` | Catalogue Service | Service API | 200 (`kind: error`) | no | Unknown part, repository or lifecycle | ET |
| `CAT_MERGE_UNSIGNED` | Catalogue Service | Service API | 200 (`kind: error`) | no | Merge not signed where required | ET |
| `CAT_NAME_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | Display name empty, too long or with control characters | ET |
| `CAT_NO_READ_ACCESS` | Catalogue Service | Service API | 200 (`kind: error`) | no | No read access | ET |
| `CAT_NO_REVIEW_RIGHT` | Catalogue Service | Service API | 200 (`kind: error`) | no | Worktree requested without review right | ET |
| `CAT_PART_UNKNOWN` | Catalogue Service | Service API | 200 (`kind: error`) | no | Unknown part, repository or lifecycle | ET |
| `CAT_PROBE_EXPIRED` | Catalogue Service | Service API | 200 (`kind: error`) | no | Probe unknown or expired | ET |
| `CAT_PROFILE_KEY_RESERVED` | Catalogue Service | Service API | 200 (`kind: error`) | no | A profile entry's `key` is `ordin`, reserved for the bundled catalogue (§B step 1) | ET |
| `CAT_QUERY_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | Search text over 500 chars | ET |
| `CAT_REPOSITORY_TOO_LARGE` | Catalogue Service | internal | — | no | Commit refused (format changed, too large, duplicate, checker down) | ET |
| `CAT_REPOSITORY_UNKNOWN` | Catalogue Service | Service API | 200 (`kind: error`) | no | Unknown part, repository or lifecycle | ET |
| `CAT_REQUIREMENTS_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | Requirements list empty, over 50, or an item over 2000 chars | ET |
| `CAT_SIGN_IN_NEEDED` | Catalogue Service | Service API | 200 (`kind: error`) | no | GitHub sign-in missing or expired | ET |
| `CAT_STORE_FAILED` | Catalogue Service | Service API | 200 (`kind: error`) | yes | Datastore failure | ET |
| `CAT_UNREACHABLE` | Catalogue Service | Service API | 200 (`kind: error`) | yes | Repository unreachable (network) | ET |
| `CAT_WITHDRAWN_LIST_UNREADABLE` | Catalogue Service | internal | — | no | Withdrawn list unreadable | ET |
| `CFG_ACTION_LIST_INVALID` | Agent Configuration | internal | — | no | A list-backed link's field keys missing, repeated, or not fields of its list setting (ADR-107) | ET |
| `CFG_APPROVAL_NOT_EXPOSED` | Agent Configuration | internal | — | no | `approve-before` names a tool that is not offered | ET |
| `CFG_BUNDLE_PATH_WRITABLE` | Agent Configuration | internal | — | no | A writable path is or holds a bundle path | ET |
| `CFG_CAPABILITY_REQUIRED_OFF` | Agent Configuration | internal | — | no | A required capability is off | ET |
| `CFG_CHECKER_UNKNOWN` | Agent Configuration | internal | — | no | **ordin** a content check names no HTTP operation that takes a body (ADR-107) | ET |
| `CFG_CONNECTOR_PLUGIN_NOT_CARRIED` | Agent Configuration | internal | — | no | **ordin** `tools.connectors` holds an item | ET |
| `CFG_CONTEXT_STEP_INVALID` | Agent Configuration | internal | — | no | **ordin** context steps repeat a single step, exceed 10, or hold a precedence note without both of its steps (ADR-107) | ET |
| `CFG_DOCUMENTS_SIZE_MISSING` | Agent Configuration | internal | — | no | Channel documents read with no total size | ET |
| `CFG_DUPLICATE` | Agent Configuration | internal | — | no | A duplicate name, offered name, entry, source, setting, control, chat command text or mapping | ET |
| `CFG_FILE_NOT_FOUND` | Agent Configuration | internal | — | no | File missing, unreadable, above 1,048,576 bytes, or not UTF-8 | ET |
| `CFG_GATE_OVERRIDE` | Agent Configuration | internal | — | no | A configuration control reuses a package control's name | ET |
| `CFG_HARNESS_NOT_OFFERED` | Agent Configuration | internal | — | no | **ordin** `harness: copilot` (C-75 open) | ET |
| `CFG_INVALID_HOST` | Agent Configuration | internal | — | no | A host that is not a valid host name | ET |
| `CFG_INVALID_PATTERN` | Agent Configuration | internal | — | no | A pattern the `regex` crate refuses | ET |
| `CFG_INVALID_VALUE` | Agent Configuration | internal | — | no | A value breaks its rule; a credential value where a variable name is expected; an address with a user name or password | ET |
| `CFG_KEYWORD_SHARE` | Agent Configuration | internal | — | no | A keyword share outside 0.5 to 1.0 | ET |
| `CFG_KNOWLEDGE_AUDIENCE_UNATTESTED` | Agent Configuration | internal | — | no | A `knowledge-agent` repository or glossary without `audience-may-read: true`; the check command and start refuse the configuration (ADR-113) | ET |
| `CFG_KNOWLEDGE_CONFLUENCE_RESTRICTED_SPACE` | Agent Configuration | internal | — | no | A Confluence space with `audience-may-read: false` while `person-checks` is off (ADR-115) | ET |
| `CFG_KNOWLEDGE_GLOSSARY_RESTRICTED` | Agent Configuration | internal | — | no | A glossary with `audience-may-read: false`; glossaries name things for every answer (ADR-115) | ET |
| `CFG_KNOWLEDGE_HOST_KIND_MISSING` | Agent Configuration | internal | — | no | A repository with `audience-may-read: false` and no `host-kind` (ADR-115) | ET |
| `CFG_KNOWLEDGE_HOST_MIXED` | Agent Configuration | internal | — | no | Per-person repositories of one host kind on two hosts (ADR-115) | ET |
| `CFG_KNOWLEDGE_SHAREPOINT_INCOMPLETE` | Agent Configuration | internal | — | no | A `sharepoint` section missing a key or with no library (ADR-115) | ET |
| `CFG_MAPPING_ARGUMENT` | Agent Configuration | internal | — | no | A mapping names an argument the operation lacks | ET |
| `CFG_MAPPING_NOT_EXPOSED` | Agent Configuration | internal | — | no | A mapping names an operation its connection does not expose | ET |
| `CFG_MISSING_KEY` | Agent Configuration | internal | — | no | Required key absent | ET |
| `CFG_MODEL_ENVIRONMENT_INVALID` | Agent Configuration | internal | — | no | **ordin** a `model.environment` entry with a bad or reserved name, neither or both of `literal` and `secret`, or a bad value (ADR-107) | ET |
| `CFG_MODEL_NOT_OFFERED` | Agent Configuration | internal | — | no | **ordin** a provider of `REFUSED_PROVIDERS` (Gemini); every other provider is accepted (amended January 2026, ADR-107) | ET |
| `CFG_NAMED_FILE_MISSING` | Agent Configuration | internal | — | no | A named file does not exist | ET |
| `CFG_NAMED_FILE_OUTSIDE` | Agent Configuration | internal | — | no | A named file lies above the configuration's folder | ET |
| `CFG_NOT_YAML` | Agent Configuration | internal | — | no | Not YAML, or not one mapping; or uses a YAML anchor, alias, merge key or custom tag (the restricted reader's `YAML_MALFORMED` or `YAML_FEATURE_REFUSED`) | ET |
| `CFG_REGISTRY_SERVER` | Agent Configuration | internal | — | no | A binding without transport names a registry-form entry | ET |
| `CFG_RESERVED_TOOL_NAME` | Agent Configuration | internal | — | no | A connection exposes a reserved name | ET |
| `CFG_RULE_BOUNDS` | Agent Configuration | internal | — | no | A pattern, list or rule count beyond its bound | ET |
| `CFG_SCHEDULE_OVER_CYCLES` | Agent Configuration | internal | — | no | A schedule fires more often than its duty's declared cycles | ET |
| `CFG_SCHEDULE_TOO_FREQUENT` | Agent Configuration | internal | — | no | A schedule fires more often than the run limit | ET |
| `CFG_UNKNOWN_ACTION` | Agent Configuration | internal | — | no | A rule attaches an undeclared reply action | ET |
| `CFG_UNKNOWN_COMMAND` | Agent Configuration | internal | — | no | An unknown command | ET |
| `CFG_UNKNOWN_CONNECTION` | Agent Configuration | internal | — | no | A mapping or source names an undeclared connection | ET |
| `CFG_UNKNOWN_DUTY` | Agent Configuration | internal | — | no | A schedule names a skill that is not a recurring duty | ET |
| `CFG_UNKNOWN_KEY` | Agent Configuration | internal | — | no | Unknown key at any depth, a key with an underscore included | ET |
| `CFG_UNKNOWN_KIND` | Agent Configuration | internal | — | no | A value outside a closed vocabulary; a condition at a point that does not offer it | ET |
| `CFG_UNKNOWN_OPERATION` | Agent Configuration | internal | — | no | A mapping names an undeclared operation | ET |
| `CFG_UNKNOWN_SECTION` | Agent Configuration | internal | — | no | `serves` names a section the package lacks | ET |
| `CFG_UNKNOWN_SERVER` | Agent Configuration | internal | — | no | A binding without transport names a server the package lacks | ET |
| `CFG_UNKNOWN_SETTING` | Agent Configuration | internal | — | no | A setting reference neither source declares, or of an unfit kind | ET |
| `CFG_UNKNOWN_SOURCE` | Agent Configuration | internal | — | no | A job command or lookup names an unfit source | ET |
| `CFG_UNKNOWN_TOOL` | Agent Configuration | internal | — | no | A control names a tool not offered, or an unknown sub-agent | ET |
| `CFL_AUDIENCE_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Confluence link fields invalid | ET |
| `CFL_NOT_APPROVED` | Workspace Service | Service API | 200 (`kind: error`) | no | Confluence link fields invalid | ET |
| `CFL_PARENT_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Confluence link fields invalid | ET |
| `CFL_SPACE_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Confluence link fields invalid | ET |
| `CHAN_ACTION_CLOSED` | Channel Gateway | Service API | 200 (`kind: error`) | no | Chat action unknown, used or expired | ET |
| `CHAN_ADAPTER_ALREADY_SET` | Channel Gateway | Service API | 200 (`kind: error`) | no | Second adapter of one kind, or second trigger handler | ET |
| `CHAN_ALREADY_PAIRED` | Channel Gateway | Service API | 200 (`kind: error`) | no | Second paired row of one kind (unique index) | ET |
| `CHAN_CHANNEL_NOT_ALLOWED` | Channel Gateway | Service API | 200 (`kind: error`) | no | Channel not allowed by the profile or the owner's choice | ET |
| `CHAN_CODE_EXPIRED` | Channel Gateway | Service API | 200 (`kind: error`) | no | Pairing completed after the code expired | ET |
| `CHAN_CORE_UNAVAILABLE` | Channel Gateway | Service API | 200 (`kind: error`) | no | Core call fails | ET |
| `CHAN_DATASTORE_FAILED` | Channel Gateway | Service API | 200 (`kind: error`) | yes | Datastore read or write fails | ET |
| `CHAN_FILE_FAILED` | Channel Gateway | Service API | 200 (`kind: error`) | no | File cannot be downloaded or saved | ET |
| `CHAN_FILE_TOO_LARGE` | Channel Gateway | Service API | 200 (`kind: error`) | no | Received file over the channel's limit | ET |
| `CHAN_HANDLER_ALREADY_SET` | Channel Gateway | Service API | 200 (`kind: error`) | no | Second adapter of one kind, or second trigger handler | ET |
| `CHAN_IDS_MISSING` | Channel Gateway | Service API | 200 (`kind: error`) | no | Chat user or chat id missing or over 64 chars | ET |
| `CHAN_INPUT_INVALID` | Channel Gateway | Service API | 200 (`kind: error`) | no | `send_update_available` with an empty version or one over 40 characters | ET |
| `CHAN_NOT_PAIRED` | Channel Gateway | Service API | 200 (`kind: error`) | no | Message from anyone but the paired account | ET |
| `CHAN_NOT_SETUP_COMMAND` | Channel Gateway | Service API | 200 (`kind: error`) | no | Pairing changed outside the setup command | ET |
| `CHAN_OUTBOX_FULL` | Channel Gateway | internal | — | no | Outbox at 1,000 rows | ET |
| `CHAN_PAIRING_NOT_PENDING` | Channel Gateway | Service API | 200 (`kind: error`) | no | Pairing not pending | ET |
| `CHAN_SEND_DROPPED` | Channel Gateway | internal | — | no | Send fails, permanent | ET |
| `CHAN_SEND_RETRY` | Channel Gateway | Service API | 200 (`kind: error`) | no | Send fails, retryable | ET |
| `CHAN_SETUP_ONLY` | Channel Gateway | Service API | 200 (`kind: error`) | no | Call other than pairing on a setup-mode gateway | ET |
| `CHAN_STOPPING` | Channel Gateway | Service API | 200 (`kind: error`) | yes | Item received after the shutdown token was cancelled | ET |
| `CHAT_BUSY` | Channel Gateway | MCP tool | — | no | Agent profile: 200 open pending questions | ET |
| `CHAT_DATASTORE_FAILED` | Channel Gateway | Service API | 200 (`kind: error`) | yes | Agent profile: a retention provider's (`ChatReplies`, `PendingQuestions`) datastore read or delete fails (step 14) | ET |
| `CHAT_DELIVERY_FAILED` | Channel Gateway | Service API | 200 (`kind: error`) | no | Agent profile: reply not posted | ET |
| `CHAT_LINK_UNKNOWN` | Channel Gateway | Service API | 200 (`kind: error`) | no | `notify_job_ended` for a connection with no registered link | ET |
| `CHAT_MESSAGE_TOO_LONG` | Channel Gateway | MCP tool | — | no | Agent profile: question longer than 12,000 chars | ET |
| `CHAT_NOT_PERMITTED` | Channel Gateway | internal | — | no | Agent profile: thread member check false or failed | ET |
| `CHAT_PENDING_FAILED` | Channel Gateway | MCP tool | — | no | Agent profile: pending question not written | ET |
| `CHAT_POST_FAILED` | Channel Gateway | Service API | 200 (`kind: error`) | yes | `notify_job_ended` post refused by the chat system | ET |
| `CHAT_RECONCILE_FAILED` | Channel Gateway | Service API | 200 (`kind: error`) | no | Agent profile: comparison with the chat system's list failed | ET |
| `CHAT_RUN_FAILED` | Channel Gateway | MCP tool | — | no | Agent profile: run not queued, or run ended without a reply | ET |
| `CHAT_SETTINGS_UNAVAILABLE` | Channel Gateway | MCP tool | — | no | Agent profile: settings store unavailable | ET |
| `CHAT_UNKNOWN_COMMAND` | Channel Gateway | Service API | 200 (`kind: error`) | no | Agent profile: unknown chat command | ET |
| `CHK_CONTRACT_INVALID` | Checker | Service API | 200 (`kind: error`) | no | `read_dossier` given a part that is not an OutputContract, has no sections, or has a section without a name | ET |
| `CHK_DATASTORE_FAILED` | Checker | Service API | 200 (`kind: error`) | yes / no (per row) | Datastore busy or locked | ET |
| `CHK_INPUT_INVALID` | Checker | Service API | 200 (`kind: error`) | no | Request malformed (job index outside `draft.jobs`, empty `entry.reason` with no list, unknown axis) | ET |
| `CHK_INSTANCE_UNKNOWN` | Checker | Service API | 200 (`kind: error`) | no | `executable_parts` for an instance with no install check stored | ET |
| `CHK_RESOLVER_FAILED` | Checker | Service API | 200 (`kind: error`) | yes | Resolver and Compiler fails for a reason that is not a finding (source, store) | ET |
| `CHK_RUN_NOT_FOUND` | Checker | Service API | 200 (`kind: error`) | no | `report` for an unknown or pruned run | ET |
| `CHK_SOURCE_ALREADY_BOUND` | Checker | Service API | 200 (`kind: error`) | no | `install_part_source` called a second time | ET |
| `CHK_SOURCE_NOT_BOUND` | Checker | Service API | 200 (`kind: error`) | no | A part-reading call before `install_part_source` and without a per-request source | ET |
| `CHK_SOURCE_UNAVAILABLE` | Checker | Service API | 200 (`kind: error`) | yes / no (per row) | `PartSource` cannot read the clone or index | ET |
| `CHK_TOO_MANY_FINDINGS` | Checker | Service API | 200 (`kind: error`) | no | Subject unreadable even for stage one (no part at all in a draft) | ET |
| `CHRT_EXTERNAL_NOT_MEMBER` | Ordin Host *(also: Crew Orchestrator)* | Service API | 200 (`kind: error`) | no | members_allowed_external_actions: subset of members | 2C |
| `CHRT_GUARDRAIL_ABOVE_MAX` | Ordin Host *(also: Crew Orchestrator)* | Service API | 200 (`kind: error`) | no | guardrails: each value ≥ 1 and at or below the profile's maximum | 2C |
| `CHRT_HANDOFF_INVALID` | Ordin Host *(also: Crew Orchestrator)* | Service API | 200 (`kind: error`) | no | allowed_handoffs: both ends are members | 2C |
| `CHRT_MEMBER_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | members: 1–20 (assumed); every member installed or in the remote agent catalogue | 2C |
| `CHRT_NAME_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | name: present when `kept`; 1–80 chars (assumed, matches workflow names) | 2C-only |
| `CHRT_NOT_STARTED_BY_PERSON` | Ordin Host | Service API | 200 (`kind: error`) | no | Start pressed on a charter not `valid` | ET |
| `CHRT_PURPOSE_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | purpose: 1–2000 chars (assumed) | 2C |
| `CHRT_ROLE_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | members.role: 1–200 chars (assumed) | 2C |
| `CLA_AGENT_FOLDER_MISSING` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | Agent folder missing | ET |
| `CLA_ALLOWANCE_EXHAUSTED` | Claude Local Adapter | internal | — | no | `result` with `is_error` after an allowance notice | ET |
| `CLA_AUTH_STATUS_UNREADABLE` | Claude Local Adapter | Service API | 200 (`kind: error`) | yes | `claude auth status --json` unreadable | ET |
| `CLA_CONFIG_INVALID` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | Configuration path not absolute, profile and sign-in mode inconsistent, empty key | ET |
| `CLA_CONFIG_WRITE_FAILED` | Claude Local Adapter | Service API | 200 (`kind: error`) | yes | Session file cannot be written | ET |
| `CLA_DEADLINE` | Claude Local Adapter | Service API | 200 (`kind: error`) | yes | `start` exceeds `ctx.deadline` | ET |
| `CLA_ENV_RESERVED` | Claude Local Adapter | internal | — | no | A `models.environment` name is reserved (ADR-107) | ET |
| `CLA_HARNESS_EXITED` | Claude Local Adapter | internal | — | no | Child exits before the run ended | ET |
| `CLA_HARNESS_NOT_FOUND` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | `claude` executable not found | ET |
| `CLA_HARNESS_UNTESTED` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | Version not in the tested list | ET |
| `CLA_HOOK_OPEN_FAILED` | Claude Local Adapter | Service API | 200 (`kind: error`) | yes | Hook Relay `open_session` fails | ET |
| `CLA_INPUT_INVALID` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | Command name, arguments or text outside 2C limits | ET |
| `CLA_MCP_OPEN_FAILED` | Claude Local Adapter | Service API | 200 (`kind: error`) | yes | MCP Front `open_session` fails | ET |
| `CLA_NOT_PAUSED` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | `resume` of a run that is not paused | ET |
| `CLA_RESUME_MISMATCH` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | `resume` with another `approval_id` or a wrong decision kind | ET |
| `CLA_RULES_UNAVAILABLE` | Claude Local Adapter | internal | — | no | Rules Engine unavailable at run start or end | ET |
| `CLA_RUN_START_ENDED` | Claude Local Adapter | internal | — | no | Rules Engine ends the run at start (`StartOutcome::Ended` or `Replied`) | ET |
| `CLA_SERVER_NAME_CLASH` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | Package server name equals an MCP Front server name | ET |
| `CLA_SESSION_EXISTS` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | `start` for a run already held | ET |
| `CLA_SESSION_RECORD_MISSING` | Claude Local Adapter | internal | — | no | Agent profile: no session record for a resume, or `SessionRecordStore::load` returns a `RecordError` | ET |
| `CLA_SESSION_RECORD_SAVE_FAILED` | Claude Local Adapter | internal | — | no | Agent profile: session record cannot be saved at a pause | ET |
| `CLA_SETUP_FAILED` | Claude Local Adapter | Service API | 200 (`kind: error`) | yes | Session setup source fails | ET |
| `CLA_SIGNED_OUT` | Claude Local Adapter | internal | — | no | Harness signed out at start | ET |
| `CLA_SIGN_IN_FAILED` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | `claude auth login` exits non-zero | ET |
| `CLA_SIGN_IN_STATUS_FAILED` | Claude Local Adapter | Service API | 200 (`kind: error`) | yes | `sign_in_status`: `claude auth status --json` times out after 10 s or prints no JSON | ET |
| `CLA_SPAWN_FAILED` | Claude Local Adapter | Service API | 200 (`kind: error`) | yes | Child cannot be spawned | ET |
| `CLA_STOP_DEADLINE` | Claude Local Adapter | internal | — | no | Child did not exit within the stop deadline | ET |
| `CLA_STREAM_LINE_TOO_LONG` | Claude Local Adapter | internal | — | no | Stdout line above 4 MiB | ET |
| `CLA_STREAM_MALFORMED` | Claude Local Adapter | internal | — | no | Stdout line not JSON (20 in a row ends the run) | ET |
| `CLA_TARGET_MISMATCH` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | `start` target is a remote agent | ET |
| `CLA_TURN_FAILED` | Claude Local Adapter | internal | — | no | `result` with `is_error` otherwise | ET |
| `CLA_UNKNOWN_RUN` | Claude Local Adapter | Service API | 200 (`kind: error`) | no | `resume`/`read`/`cancel`/`events` for an unknown run | ET |
| `CLA_VERSION_CHECK_FAILED` | Claude Local Adapter | Service API | 200 (`kind: error`) | yes | `claude --version` times out or is unparsable | ET |
| `CMD_PLACEHOLDER_MISSING` | Agent Loader *(also: Session Supervisor)* | internal | — | no | The command's prompt body holds no `${input:arguments}` (L-23.4) | ET |
| `CMD_PROMPT_UNREADABLE` | Agent Loader *(also: Session Supervisor)* | internal | — | no | The command's prompt file is absent, unreadable, not UTF-8 or has no front matter (L-23.2, L-23.3) | ET |
| `CMD_UNKNOWN_COMMAND` | Agent Loader *(also: Session Supervisor)* | internal | — | no | A run's command is not a command of the manifest (L-23.1) | ET |
| `CODE_BRANCH_INVALID` | Project Service | Service API | 200 (`kind: error`) | no | branch: `ordin/` + 1–100 chars `[a-z0-9-]` (assumed), valid git ref | 2C |
| `CODE_COMMIT_INVALID` | Project Service | Service API | 200 (`kind: error`) | no | base_commit: 40 lowercase hex chars | 2C |
| `CODE_GROUP_TOO_SMALL` | Project Service | Service API | 200 (`kind: error`) | no | attempt_group_id: a group holds 2 or more tasks on the same request (04:147) | 2C |
| `CODE_REQUEST_INVALID` | Project Service | Service API | 200 (`kind: error`) | no | request: 1–4000 chars (assumed) | 2C |
| `CODE_WORKTREE_OUTSIDE_ROOT` | Project Service | Service API | 200 (`kind: error`) | no | worktree_path: under the application data root (02:760) | 2C |
| `CONF_ACCESS_DENIED` | Confluence Adapter | Service API | 200 (`kind: error`) | no | 403 | ET |
| `CONF_ACTION_KEY_USED` | Confluence Adapter | Service API | 200 (`kind: error`) | no | External-action key already used | ET |
| `CONF_APPROVAL_FAILED` | Confluence Adapter | Service API | 200 (`kind: error`) | yes | Approval Service refused the request | ET |
| `CONF_CONVERSION_FAILED` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Storage XHTML malformed (bring back, storage push) | ET |
| `CONF_DATASTORE_UNAVAILABLE` | Confluence Adapter | Service API | 200 (`kind: error`) | yes | Own tables unreachable, or the Journal refused to record the external action for a reason other than a used key | ET |
| `CONF_FILES_FAILED` | Confluence Adapter | Service API | 200 (`kind: error`) | yes | File Service call failed during sync | ET |
| `CONF_LINK_NOT_ACTIVE` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Link not in the state the call needs | ET |
| `CONF_LOCATION_INVALID` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Stored origin form malformed | ET |
| `CONF_NOT_CONFIGURED` | Confluence Adapter | Service API | 200 (`kind: error`) | no | No `confluence.site` in the profile, or Identity answers `CRED_PROVIDER_NOT_CONFIGURED` | ET |
| `CONF_NOT_FOUND` | Confluence Adapter | Service API | 200 (`kind: error`) | no | 404 | ET |
| `CONF_NOT_TRACKED` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Link has no tracking state (`check_changes`, `item_for_path`) | ET |
| `CONF_ORIGIN_CHANGED` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Page or attachment version differs from the one read, or 409 | ET |
| `CONF_OUTCOME_UNKNOWN` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Timeout after sending and the destination moved | ET |
| `CONF_QUERY_INVALID` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Search text empty or too long; more than 50 checked items | ET |
| `CONF_RESPONSE_INVALID` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Response body does not parse | ET |
| `CONF_SIGN_IN_NEEDED` | Confluence Adapter | Service API | 200 (`kind: error`) | no | No sign-in, refused, or a second 401 | ET |
| `CONF_SITE_NOT_GRANTED` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Signed-in account cannot reach the company site | ET |
| `CONF_THROTTLED` | Confluence Adapter | Service API | 200 (`kind: error`) | yes | 429 after retries | ET |
| `CONF_TITLE_EXISTS` | Confluence Adapter | Service API | 200 (`kind: error`) | no | A page with this title already exists in the space | ET |
| `CONF_TOO_LARGE` | Confluence Adapter | Service API | 200 (`kind: error`) | no | Body above 5 MiB, attachment above 100 MiB, or 413 | ET |
| `CONF_UNAVAILABLE` | Confluence Adapter | Service API | 200 (`kind: error`) | yes | 5xx or network failure after retries | ET |
| `CONN_ADDRESS_INVALID` | Server Connector | Service API | 200 (`kind: error`) | no | Address not a host name or IP literal | ET |
| `CONN_ADDRESS_LOCKED` | Server Connector | Service API | 200 (`kind: error`) | yes | Server locked this address | ET |
| `CONN_ALREADY_STARTED` | Server Connector | Service API | 200 (`kind: error`) | no | `start` called a second time | ET |
| `CONN_BAD_ANSWER` | Server Connector | Service API | 200 (`kind: error`) | yes | Answer not a `CommandReply` / `PairReply`, or no version header | ET |
| `CONN_CERT_UNTRUSTED` | Server Connector | Service API | 200 (`kind: error`) | no | Certificate not trusted by the computer and no pin | ET |
| `CONN_CLIENT_CERT_UNAVAILABLE` | Server Connector | Service API | 200 (`kind: error`) | no | Required client certificate not available | ET |
| `CONN_CODE_REFUSED` | Server Connector | Service API | 200 (`kind: error`) | no | Code malformed, wrong or expired | ET |
| `CONN_COMPUTER_NAME_INVALID` | Server Connector | Service API | 200 (`kind: error`) | no | Server refused this computer's name (`DESK_NAME_INVALID`) | ET |
| `CONN_DATASTORE` | Server Connector | Service API | 200 (`kind: error`) | yes | Datastore read or write failed | ET |
| `CONN_DEADLINE` | Server Connector *(also: Command Bridge)* | Service API | 200 (`kind: error`) | yes | Server did not answer by the deadline | ET |
| `CONN_DUPLICATE` | Server Connector | Service API | 200 (`kind: error`) | no | Address and port already paired | ET |
| `CONN_EVENT_UNREADABLE` | Server Connector | internal | — | no | Event frame unreadable or for another backend | ET |
| `CONN_FINGERPRINT_MISMATCH` | Server Connector | Service API | 200 (`kind: error`) | no | Pinned fingerprint does not match | ET |
| `CONN_KEYCHAIN` | Server Connector | Service API | 200 (`kind: error`) | yes | Keychain read, write or delete failed | ET |
| `CONN_KEY_REFUSED` | Server Connector *(also: Command Bridge)* | Service API | 200 (`kind: error`) | no | Server refused the desktop key (401) | ET |
| `CONN_NOT_CONNECTED` | Server Connector *(also: Command Bridge)* | Service API | 200 (`kind: error`) | yes | Command while not connected (no cached read) | ET |
| `CONN_PAIRING_LINE_INVALID` | Server Connector | Service API | 200 (`kind: error`) | no | Pairing line malformed | ET |
| `CONN_PAIRING_STOPPED` | Server Connector | Service API | 200 (`kind: error`) | no | Server stopped pairing after five wrong codes | ET |
| `CONN_PORT_OUT_OF_RANGE` | Server Connector | Service API | 200 (`kind: error`) | no | Port outside 1025–65535 | ET |
| `CONN_REFUSED_FOR_VERSION` | Server Connector *(also: Command Bridge)* | Service API | 200 (`kind: error`) | no | Service API version unusable | ET |
| `CONN_TOO_MANY_DESKTOPS` | Server Connector | Service API | 200 (`kind: error`) | no | Server already has its most paired desktops (`DESK_TOO_MANY`) | ET |
| `CONN_UNKNOWN_SERVER` | Server Connector | Service API | 200 (`kind: error`) | no | Unknown server id or backend | ET |
| `CONN_UNREACHABLE` | Server Connector *(also: Command Bridge)* | Service API | 200 (`kind: error`) | yes | Server unreachable (connect, TLS transport, timeout) | ET |
| `CPA_AGENT_FOLDER_MISSING` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Agent folder missing | ET |
| `CPA_ALLOWANCE_EXHAUSTED` | Copilot Local Adapter | internal | — | no | Allowance used up | ET |
| `CPA_AUTH_STATUS_UNREADABLE` | Copilot Local Adapter | Service API | 200 (`kind: error`) | yes | Sign-in status unreadable | ET |
| `CPA_CONFIG_INVALID` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Configuration path not absolute | ET |
| `CPA_DEADLINE` | Copilot Local Adapter | Service API | 200 (`kind: error`) | yes | `start` past `ctx.deadline` | ET |
| `CPA_GATE_FAILED` | Copilot Local Adapter | MCP tool | — | no | Decision call errors or exceeds 55 s | ET |
| `CPA_HARNESS_UNTESTED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | CLI version not tested | ET |
| `CPA_HOOK_OPEN_FAILED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | yes | Hook Relay `open_session` fails | ET |
| `CPA_INPUT_INVALID` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Input outside 2C limits | ET |
| `CPA_MCP_OPEN_FAILED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | yes | MCP Front `open_session` fails | ET |
| `CPA_NOT_PAUSED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Resume of a run not paused | ET |
| `CPA_PROBE_FAILED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | yes | A probe call cannot reach or start the CLI server | ET |
| `CPA_PROFILE_NOT_OFFERED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Constructed for the agent profile | ET |
| `CPA_RESUME_MISMATCH` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Resume with another approval or a wrong decision kind | ET |
| `CPA_RULES_UNAVAILABLE` | Copilot Local Adapter | internal | — | no | Rules Engine unavailable at run start or end | ET |
| `CPA_RUN_START_ENDED` | Copilot Local Adapter | internal | — | no | Rules Engine ends the run at start (`StartOutcome::Ended` or `Replied`) | ET |
| `CPA_SEND_FAILED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | yes | First turn cannot be sent | ET |
| `CPA_SERVER_EXITED` | Copilot Local Adapter | internal | — | no | CLI server exits before the run ended | ET |
| `CPA_SERVER_NAME_CLASH` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Package server name clashes with an MCP Front server | ET |
| `CPA_SERVER_SIGN_IN_UNCONFIRMED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Constructed for the personal server while C-48 is unconfirmed | ET |
| `CPA_SERVER_START_FAILED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | yes | CLI server does not start | ET |
| `CPA_SESSION_CREATE_FAILED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | yes | SDK session cannot be created | ET |
| `CPA_SESSION_ERROR` | Copilot Local Adapter | internal | — | no | SDK session error | ET |
| `CPA_SESSION_EXISTS` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Run already held | ET |
| `CPA_SESSION_NOT_RESUMABLE` | Copilot Local Adapter | internal | — | no | Persisted session cannot be resumed (C-05) | ET |
| `CPA_SETUP_FAILED` | Copilot Local Adapter | Service API | 200 (`kind: error`) | yes | Session setup source fails | ET |
| `CPA_SIGNED_OUT` | Copilot Local Adapter | internal | — | no | Signed out after the silent GitHub refresh | ET |
| `CPA_STOP_DEADLINE` | Copilot Local Adapter | internal | — | no | CLI server not stopped within the deadline | ET |
| `CPA_TARGET_MISMATCH` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Target is a remote agent | ET |
| `CPA_UNKNOWN_RUN` | Copilot Local Adapter | Service API | 200 (`kind: error`) | no | Verb on an unknown run | ET |
| `CRAFT_ARGUMENTS_INVALID` | Craft Toolkit | Service API | 200 (`kind: error`) | no | A path argument missing or not an absolute path | ET |
| `CRAFT_CONVERSION_UNAVAILABLE` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Conversion app not confirmed or not installed (C-28; for `to_markdown`, C-13) | ET |
| `CRAFT_DATASTORE_UNAVAILABLE` | Craft Toolkit | Service API | 200 (`kind: error`) | yes | Own tables unreachable | ET |
| `CRAFT_DOWNLOAD_FAILED` | Craft Toolkit | Service API | 200 (`kind: error`) | yes | Download failed after retries | ET |
| `CRAFT_DUPLICATE` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Duplicate app row; installed without verification; bad file-type map entry | ET |
| `CRAFT_FILES_FAILED` | Craft Toolkit | Service API | 200 (`kind: error`) | yes | File Service write failed | ET |
| `CRAFT_HASH_MISMATCH` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Downloaded file does not match the pinned hash | ET |
| `CRAFT_INSTALL_FAILED` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Unpacking failed or an unsafe archive path | ET |
| `CRAFT_NOT_APPROVED` | Craft Toolkit | Service API | 200 (`kind: error`) | no | App not approved in the profile | ET |
| `CRAFT_NOT_INSTALLED` | Craft Toolkit | Service API | 200 (`kind: error`) | yes | App approved but not installed | ET |
| `CRAFT_NO_MCP_MODE` | Craft Toolkit | Service API | 200 (`kind: error`) | no | App has no MCP mode | ET |
| `CRAFT_OPEN_FAILED` | Craft Toolkit | Service API | 200 (`kind: error`) | yes | Opening the window or the system default failed | ET |
| `CRAFT_OUTPUT_MISSING` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Output file missing or empty | ET |
| `CRAFT_OUTPUT_TOO_LARGE` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Output above 256 MiB | ET |
| `CRAFT_PATH_NOT_ALLOWED` | Craft Toolkit | Service API | 200 (`kind: error`) | no | A path argument outside every place, or a conversion path refused | ET |
| `CRAFT_PLATFORM_UNSUPPORTED` | Craft Toolkit | Service API | 200 (`kind: error`) | no | No asset for this platform in the pin | ET |
| `CRAFT_SERVER_START_FAILED` | Craft Toolkit | Service API | 200 (`kind: error`) | yes | App's MCP server did not start in 15 s | ET |
| `CRAFT_START_BLOCKED` | Craft Toolkit | Service API | 200 (`kind: error`) | no | The operating system refused to start the program | ET |
| `CRAFT_TIMEOUT` | Craft Toolkit | Service API | 200 (`kind: error`) | yes | Tool call or conversion exceeded 120 s | ET |
| `CRAFT_TOOL_FAILED` | Craft Toolkit | Service API | 200 (`kind: error`) | no | The app returned an error | ET |
| `CRAFT_TOOL_NOT_OFFERED` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Tool not offered to this session | ET |
| `CRAFT_UNVERIFIED_INSTALL` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Duplicate app row; installed without verification; bad file-type map entry | ET |
| `CRED_AMBIGUOUS` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Several active credentials, none named | ET |
| `CRED_BROWSER_FAILED` | Identity And Credentials | Service API | 200 (`kind: error`) | yes | System browser could not be opened | ET |
| `CRED_CERTIFICATE_EXPIRED` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Imported client certificate already expired | ET |
| `CRED_CERTIFICATE_INVALID` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Imported client certificate or key unreadable, missing, unloadable or not a pair | ET |
| `CRED_CHALLENGE_UNKNOWN` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Unknown challenge id | ET |
| `CRED_CONFIG_INVALID` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Config: back end and program disagree, pool missing | ET |
| `CRED_DUPLICATE` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Active credential with same kind and label exists | ET |
| `CRED_FLOW_NOT_OFFERED` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Flow not offered here (Confluence/Slack/Google on the server, agent profile, no browser) | ET |
| `CRED_KEYCHAIN_MISSING` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Secret item missing for an active credential | ET |
| `CRED_KEYCHAIN_UNAVAILABLE` | Identity And Credentials | Service API | 200 (`kind: error`) | yes | Keychain or Secret Service locked, refused or unreachable | ET |
| `CRED_KIND_HAS_NO_TOKEN` | Identity And Credentials | Service API | 200 (`kind: error`) | no | `access_token` on a password kind | ET |
| `CRED_KIND_MISMATCH` | Identity And Credentials | Service API | 200 (`kind: error`) | no | `mail_password` on a token kind | ET |
| `CRED_LABEL_INVALID` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Account label out of range | ET |
| `CRED_LOOPBACK_BUSY` | Identity And Credentials | Service API | 200 (`kind: error`) | yes | All loopback ports busy | ET |
| `CRED_NOT_FOUND` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Unknown credential id | ET |
| `CRED_PASSWORD_INVALID` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Password empty or > 1024 bytes | ET |
| `CRED_PROVIDER_NOT_CONFIGURED` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Client id or GitHub App id absent | ET |
| `CRED_PROVIDER_REFUSED` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Provider refused the client or the grant (incl. C-12/C-14 failing) | ET |
| `CRED_PROVIDER_UNREACHABLE` | Identity And Credentials | Service API | 200 (`kind: error`) | yes | Provider unreachable (DNS, TLS, timeout, 5xx) | ET |
| `CRED_REFRESH_REFUSED` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Provider refused the refresh (`invalid_grant`, HTTP 400 or 401; step 4) | ET |
| `CRED_RELAY_STATE_MISMATCH` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Relayed sign-in's 'state' differs from the challenge's | ET |
| `CRED_SCOPE_MISSING` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Active credential lacks a scope the caller needs; a pasted GitLab token lacks `read_repository`, `write_repository` or `api` | ET |
| `CRED_SECRET_IN_RECORD` | Identity And Credentials | Service API | 200 (`kind: error`) | no | A secret found in a record field (test-time guard) | ET |
| `CRED_SECRET_KEY_CONFLICT` | Identity And Credentials *(also: Ordin Server)* | Service API | 200 (`kind: error`) | no | Both server key variables set | ET |
| `CRED_SECRET_KEY_EXISTS` | Identity And Credentials | Service API | 200 (`kind: error`) | no | `create_secrets_key_credential` target file already exists | ET |
| `CRED_SECRET_KEY_INVALID` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Key bytes not 32 bytes / base64 of 32 | ET |
| `CRED_SECRET_KEY_MISSING` | Identity And Credentials *(also: Ordin Server)* | Service API | 200 (`kind: error`) | no | No session bus and no key variable | ET |
| `CRED_SECRET_NAME_INVALID` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Secret name fails pattern | ET |
| `CRED_SECRET_STORE_ABSENT` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Scaled shape, secret store called | ET |
| `CRED_SECRET_STORE_CORRUPT` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Secrets file cannot be decrypted or unknown format | ET |
| `CRED_SECRET_STORE_WRITE_FAILED` | Identity And Credentials | Service API | 200 (`kind: error`) | yes | Secrets file write failed | ET |
| `CRED_SECRET_TOO_LARGE` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Secret value > 64 KiB | ET |
| `CRED_SIGN_IN_CANCELLED` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Person cancelled or denied at the provider | ET |
| `CRED_SIGN_IN_REQUIRED` | Identity And Credentials *(also: Setup Assistant)* | Service API | 200 (`kind: error`) | no | No usable credential / marked needs sign-in / refresh refused | ET |
| `CRED_SIGN_IN_TIMEOUT` | Identity And Credentials | Service API | 200 (`kind: error`) | yes | Sign-in not finished in time (10 min / device expiry) | ET |
| `CRED_STORE_FAILED` | Identity And Credentials | Service API | 200 (`kind: error`) | yes | Database write failed | ET |
| `CRED_SYSTEMD_CREDS_FAILED` | Identity And Credentials | Service API | 200 (`kind: error`) | no | `systemd-creds decrypt --user` failed or timed out | ET |
| `CRED_TOO_MANY_SIGN_INS` | Identity And Credentials | Service API | 200 (`kind: error`) | yes | More than 4 sign-ins pending | ET |
| `CRED_URL_INVALID` | Identity And Credentials | Service API | 200 (`kind: error`) | no | Issuer or site not `https` / too long | ET |
| `CREW_BUDGET_EXCEEDED` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | budget.requests_used: ≤ charter `request_budget` | 2C |
| `CREW_CHARTER_INVALID` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Charter breaks a 2C rule or the plan does not fit (B.2) | ET |
| `CREW_COORDINATOR_UNAVAILABLE` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Ordin unavailable twice (C.2) | ET |
| `CREW_COORDINATOR_UNUSABLE` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Ordin's answer unusable (D.2) | ET |
| `CREW_ENDED` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Kill of an ended crew run (I) | ET |
| `CREW_END_REASON_TOO_LONG` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | end_reason: ≤ 500 chars (assumed) | 2C-only |
| `CREW_GUARDRAIL_REFUSED` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Delegate refused by a guardrail (F.2) | ET |
| `CREW_LIMIT_REACHED` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Round, time or request limit (F.1) | ET |
| `CREW_MEMBER_FAILED_TWICE` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Same member failed twice (F.3) | ET |
| `CREW_NOT_FOUND` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Kill, continue or get of an unknown crew run (I, J) | ET |
| `CREW_NOT_PAUSED` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Continue of a crew not paused by a restart (J.2) | ET |
| `CREW_NOT_STARTED_BY_PERSON` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Start without the person's press (B.1) | ET |
| `CREW_PAUSE_REASON_MISMATCH` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | pause_reason: present if and only if `state = paused` | 2C |
| `CREW_ROUNDS_EXCEEDED` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | round_counter: ≤ charter `max_rounds` | 2C |
| `CREW_STORE_UNAVAILABLE` | Crew Orchestrator | Service API | 200 (`kind: error`) | yes | Store unavailable | ET |
| `CREW_TAKE_OVER_REFUSED` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | Ordin asks for a take-over the orchestrator does not accept (D.8) | ET |
| `CREW_TOO_MANY_MEMBERS` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | More than 8 members (A.1) | ET |
| `CREW_WORKFLOW_MISMATCH` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | workflow_id, workflow_version: both present or both absent | 2C |
| `CSET_DUPLICATE` | Project Service | Service API | 200 (`kind: error`) | no | coding_task_id: unique (one change set per coding task, 04:147) | 2C |
| `CSET_HUNK_TOO_LARGE` | Project Service | Service API | 200 (`kind: error`) | no | entries.hunk: ≤ 1 MiB each (assumed) | 2C |
| `CSET_UNDECIDED_ENTRIES` | Project Service | Service API | 200 (`kind: error`) | no | state = settled: no entry `pending` | 2C |
| `DB_UNAVAILABLE` | Run Queue *(also: Caller API, Duty Timer, Job Runner, Retention And Erasure)* | Caller API, internal | 503 | yes | The datastore does not answer | ET |
| `DEC_CONTRACT_RANGE` | Release Builder | internal | — | no | Contract version outside 1 to 1 | ET |
| `DEC_DUPLICATE` | Release Builder | internal | — | no | A duplicate | ET |
| `DEC_FORMAT_VERSION` | Release Builder | internal | — | no | Format version outside 1 to 1 | ET |
| `DEC_HEARTBEAT_ENDINGS` | Release Builder | internal | — | no | Heartbeat endings lack one ordin writes | ET |
| `DEC_INVALID_HOST` | Release Builder | internal | — | no | A host that is not a host name | ET |
| `DEC_INVALID_PATTERN` | Release Builder | internal | — | no | A pattern the `regex` crate refuses | ET |
| `DEC_INVALID_VALUE` | Release Builder | internal | — | no | A value breaks its rule | ET |
| `DEC_KEYWORD_SHARE` | Release Builder | internal | — | no | A keyword share outside 0.5 to 1.0 | ET |
| `DEC_MISSING_KEY` | Release Builder | internal | — | no | Required key absent | ET |
| `DEC_NOT_YAML` | Release Builder | internal | — | no | Declaration not a UTF-8 YAML mapping of ≤ 1 MiB, or uses a YAML anchor, alias, merge key or custom tag (the restricted reader's `YAML_*` codes) | ET |
| `DEC_RESERVED_TOOL_NAME` | Release Builder | internal | — | no | An operation named with a reserved tool name | ET |
| `DEC_RULE_BOUNDS` | Release Builder | internal | — | no | A pattern, list or rule count beyond its bound | ET |
| `DEC_UNKNOWN_COMMAND` | Release Builder | internal | — | no | A command the package lacks | ET |
| `DEC_UNKNOWN_DUTY` | Release Builder | internal | — | no | A duty the package lacks | ET |
| `DEC_UNKNOWN_KEY` | Release Builder | internal | — | no | Unknown key | ET |
| `DEC_UNKNOWN_KIND` | Release Builder | internal | — | no | Value outside the closed vocabulary; condition at a point that lacks it | ET |
| `DEC_UNKNOWN_PATH` | Release Builder | internal | — | no | A bundle path the bundle lacks | ET |
| `DEC_UNKNOWN_SECTION` | Release Builder | internal | — | no | A tool section the package lacks | ET |
| `DEC_UNKNOWN_SETTING` | Release Builder | internal | — | no | A setting the declaration lacks, or of an unfit kind | ET |
| `DEC_UNKNOWN_SUBAGENT` | Release Builder | internal | — | no | A sub-agent the package lacks | ET |
| `DEC_UNKNOWN_TOOL` | Release Builder | internal | — | no | A tool the package cannot name | ET |
| `DESK_ADDRESS_LOCKED` | Remote API | Remote API | 429 | yes | Address locked after 10 failed key checks | ET |
| `DESK_CODE_EXPIRED` | Remote API | Remote API | 403 | no | Pairing code expired | ET |
| `DESK_CODE_INVALID` | Remote API | Remote API | 403 | no | Pairing code malformed or not a pending code | ET |
| `DESK_KEY_STORED` | Remote API | Service API | 200 (`kind: error`) | no | desktop_key_hash: the key is never stored, only its hash (02:578) | 2C |
| `DESK_NAME_INVALID` | Remote API | Remote API | 422 | no | Computer name empty or over 63 characters | ET |
| `DESK_PAIRING_STOPPED` | Remote API | Remote API | 403 | no | Five wrong codes within 10 minutes | ET |
| `DESK_TOO_MANY` | Remote API | Remote API | 403 | no | Ten desktops already paired | ET |
| `DESK_UNKNOWN` | Remote API | internal | — | no | `end_pairing` of an unknown or ended id | ET |
| `DICT_ASSETS_UNAVAILABLE` | Dictation Service | Service API | 200 (`kind: error`) | yes | System could not install recognition assets (status 6) | ET |
| `DICT_AUDIO_FAILED` | Dictation Service | Service API | 200 (`kind: error`) | yes | No audio input device, or the audio engine failed to start (status 7) | ET |
| `DICT_BRIDGE_FAILED` | Dictation Service | Service API | 200 (`kind: error`) | yes | Bridge returned status 9, or an analyzer error mid-session | ET |
| `DICT_FIELD_INVALID` | Dictation Service | Service API | 200 (`kind: error`) | no | `field` outside 1–128 chars or the allowed characters | ET |
| `DICT_LANGUAGE_NOT_SUPPORTED` | Dictation Service | Service API | 200 (`kind: error`) | no | Locale supported by neither transcriber | ET |
| `DICT_MICROPHONE_DENIED` | Dictation Service | Service API | 200 (`kind: error`) | no | Microphone permission denied or restricted (status 4) | ET |
| `DICT_NOT_OFFERED` | Dictation Service | Service API | 200 (`kind: error`) | no | Started in a build with no dictation (server) | ET |
| `DICT_SESSION_UNKNOWN` | Dictation Service | Service API | 200 (`kind: error`) | no | `stop`/`cancel` with an unknown or ended session | ET |
| `DICT_SPEECH_DENIED` | Dictation Service | Service API | 200 (`kind: error`) | no | Speech recognition permission denied or restricted (status 5) | ET |
| `DICT_UNAVAILABLE_MACOS_VERSION` | Dictation Service | Service API | 200 (`kind: error`) | no | Started on macOS before 26 | ET |
| `DICT_UNAVAILABLE_WINDOWS` | Dictation Service | Service API | 200 (`kind: error`) | no | Started on Windows | ET |
| `DSK_AGENT_PROFILE_ONLY` | Ordin Desktop | Service API | 200 (`kind: error`) | no | An agent-profile-only `SupervisorHost` method is reached on the desktop | ET |
| `DSK_DATA_ROOT_UNWRITABLE` | Ordin Desktop | Service API | 200 (`kind: error`) | no | `<data root>` cannot be created or written | ET |
| `DSK_DIALOG_CANCELLED` | Ordin Desktop | Service API | 200 (`kind: error`) | no | The person cancels the save dialog of `files.save_copy` or `files.save_export` (amended January 2026, ADR-109) | ET |
| `DSK_DRAFT_LIST_FAILED` | Ordin Desktop | internal | — | no | `authoring.list` fails while the Notifier compares draft states | ET |
| `DSK_LINK_REFUSED` | Ordin Desktop | Service API, internal | 200 (`kind: error`) | no | An `ordin:` URL that is not a handover link, or a URL `app.open_slack_link` refuses | ET |
| `DSK_LINK_REGISTER_FAILED` | Ordin Desktop | internal | — | no | The 'ordin' link scheme could not be registered for this user | ET |
| `DSK_NOTIFY_FAILED` | Ordin Desktop | internal | — | no | The operating system refuses a notification (permission off) | ET |
| `DSK_PICKER_UNAVAILABLE` | Ordin Desktop | Service API | 200 (`kind: error`) | yes | The operating system's folder picker fails to open | ET |
| `DSK_PREFERENCES_UNREADABLE` | Ordin Desktop | internal | — | no | `preferences.json` unreadable at `settings.get` | ET |
| `DSK_PREFERENCES_WRITE_FAILED` | Ordin Desktop | Service API | 200 (`kind: error`) | yes | `settings.change` of a `notifications.*` or `display.appearance` key cannot write the file | ET |
| `DSK_PREFERENCE_VALUE_INVALID` | Ordin Desktop | Service API | 200 (`kind: error`) | no | `settings.change` of one of those keys with a value of the wrong type | ET |
| `DSK_RELAY_TIMEOUT` | Ordin Desktop | Service API | 200 (`kind: error`) | yes | No browser callback within 10 minutes of a relayed sign-in | ET |
| `DSK_RELAY_URL_REFUSED` | Ordin Desktop | Service API | 200 (`kind: error`) | no | A relayed sign-in URL is not Slack's authorise URL with a loopback redirect of this computer | ET |
| `DSK_SAVE_COPY_FAILED` | Ordin Desktop | Service API | 200 (`kind: error`) | yes | The copy, or an exported file of `files.save_export` (amended January 2026, ADR-109), cannot be written where the person chose | ET |
| `DSK_SHUTDOWN_STEP_TIMEOUT` | Ordin Desktop | internal | — | no | A shutdown step overruns its bound | ET |
| `DSK_SIDECAR_MISSING` | Ordin Desktop | internal | — | no | A bundled program (`ordin-hook`, `copilot`) is missing beside the executable | ET |
| `DSK_START_FAILED` | Ordin Desktop | Service API | 200 (`kind: error`) | no | A module's constructor, a part-source binding or `hook_relay.start()` fails (database newer than the program, migration failure, hook channel in use) | ET |
| `DSK_UPDATE_CHECK_FAILED` | Ordin Desktop | Service API | 200 (`kind: error`) | yes | The update feed cannot be read | ET |
| `DSK_UPDATE_INSTALL_FAILED` | Ordin Desktop | Service API | 200 (`kind: error`) | yes | Download or signature check of an update fails | ET |
| `ERASE_BAD_CHECKPOINT` | Retention And Erasure | internal | — | no | The retention checkpoint has a version other than 1 | ET |
| `ERASE_BAD_TARGET` | Retention And Erasure *(also: Caller API)* | Caller API, internal | 400 | no | A connection that is no slug, or a person's or scope's identifier empty, too long or holding white space; a scope text of another form | ET |
| `ERASE_DUPLICATE_PROVIDER` | Retention And Erasure | internal | — | no | A second provider for one content | ET |
| `ERASE_INCOMPLETE` | Retention And Erasure *(also: Caller API)* | Caller API, internal | 500 | yes | The Journal's count of the runs' rows failed during an erasure's Count step (13a); nothing deleted | ET |
| `ERASE_STORE_FAILED` | Retention And Erasure *(also: Job Runner)* | internal | — | no | A store's count or delete fails with a non-retryable error during the retention step | ET |
| `ERASE_UNAVAILABLE` | Retention And Erasure *(also: Caller API)* | Caller API, internal | 503 | yes | This crate's own tables cannot be read or written | ET |
| `EXP_PART_DUPLICATE` | Checker | Service API | 200 (`kind: error`) | no | part_name: 1–200 chars (assumed); unique per (agent_instance_id, kind) | 2C |
| `EXP_SOURCE_NOT_POLICY` | Checker | Service API | 200 (`kind: error`) | no | source: always `organisation_policy`; never the business user (04:50) | 2C |
| `EXT_AUTO_RESEND` | Journal And Artefact Store *(also: Push Service, Telegram Service)* | Service API | 200 (`kind: error`) | no | resend: never resent automatically (04:200) | 2C |
| `EXT_DESTINATION_INVALID` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | destination: 1–2048 chars (assumed) | 2C |
| `EXT_KEY_DUPLICATE` | Journal And Artefact Store *(also: Confluence Adapter, Push Service, SharePoint Adapter, Telegram Service)* | Service API | 200 (`kind: error`) | no | action_key: unique; written before the action leaves the application | 2C |
| `FILE_AGENT_REFUSED` | File Service | MCP tool | — | no | Agent file tool refused by the work folder | ET |
| `FILE_BIN_UNAVAILABLE` | File Service | Service API | 200 (`kind: error`) | no | System bin unavailable | ET |
| `FILE_CHANGED_MEANWHILE` | File Service | Service API | 200 (`kind: error`) | yes | `expected_hash` differs from the file on disk | ET |
| `FILE_COPY_NOT_FOUND` | File Service | Service API | 200 (`kind: error`) | no | Published copy id unknown | ET |
| `FILE_COPY_TRANSITION_INVALID` | File Service | Service API | 200 (`kind: error`) | no | Published copy state change not in the state table | ET |
| `FILE_DATASTORE_UNAVAILABLE` | File Service | Service API | 200 (`kind: error`) | yes | Datastore unreachable | ET |
| `FILE_FOLDER_UNREADABLE` | File Service | Service API | 200 (`kind: error`) | no | Folder cannot be read by the person's account | ET |
| `FILE_GRANT_HANDOFF_FAILED` | File Service | Service API | 200 (`kind: error`) | yes | Agent Folder Manager refused the grants (`AfmError`) | ET |
| `FILE_INCOMING_REFUSED` | File Service | Service API | 200 (`kind: error`) | no | Incoming file fails the safety check | ET |
| `FILE_MOVE_TARGET_EXISTS` | File Service | internal | — | no | `move_file` onto a path where another file or its versions exist (ADR-104) | ET |
| `FILE_NOT_FOUND` | File Service | Service API | 200 (`kind: error`) | no | File missing at the path | ET |
| `FILE_NOT_GRANTED` | File Service | Service API | 200 (`kind: error`) | no | Agent instance has no grant on the place (`read_for_agent`) | ET |
| `FILE_PATH_ESCAPES_PLACE` | File Service | Service API | 200 (`kind: error`) | no | Relative path escapes its place (`..`, link outside) | ET |
| `FILE_PLACE_EXISTS` | File Service | Service API | 200 (`kind: error`) | no | Folder already a place | ET |
| `FILE_PLACE_NOT_FOUND` | File Service | Service API | 200 (`kind: error`) | no | Place id unknown | ET |
| `FILE_PROTECTED_LOCATION` | File Service | Service API | 200 (`kind: error`) | no | Folder is a protected system location | ET |
| `FILE_QUERY_INVALID` | File Service | Service API | 200 (`kind: error`) | no | Search query empty or over 200 chars; folder given to `import_files` | ET |
| `FILE_RESTORE_TARGET_EXISTS` | File Service | Service API | 200 (`kind: error`) | no | A file already exists where a binned file would be restored | ET |
| `FILE_TOO_LARGE` | File Service | Service API | 200 (`kind: error`) | no | Read or write above 64 MiB | ET |
| `FILE_VERSION_COPY_MISSING` | File Service | Service API | 200 (`kind: error`) | no | Version copy file missing under the data root | ET |
| `FILE_VERSION_NOT_FOUND` | File Service | Service API | 200 (`kind: error`) | no | Version id unknown | ET |
| `FILE_WATCH_FAILED` | File Service | Service API | 200 (`kind: error`) | yes | Watcher cannot be started on a place | ET |
| `FILE_WRITE_FAILED` | File Service | Service API | 200 (`kind: error`) | yes | Disk write or rename fails | ET |
| `FMT_COMMIT_INVALID` | Format Plugins | Service API | 200 (`kind: error`) | no | `TreeSource::new` given a commit that is not 40 lowercase hex characters | ET |
| `FMT_KIND_NOT_PROPOSABLE` | Format Plugins | Service API | 200 (`kind: error`) | no | `propose_kind` finds no rule for the file | ET |
| `FMT_NOT_RECOGNISED` | Format Plugins | Service API | 200 (`kind: error`) | no | No plug-in recognises the tree | ET |
| `FMT_PACKAGE_MANIFEST_INVALID` | Format Plugins | Service API | 200 (`kind: error`) | no | `apm.yml` does not parse, or lacks `name` or `version` | ET |
| `FMT_PART_INVALID` | Format Plugins | Service API | 200 (`kind: error`) | no | An Agent Model render or placement fails for a part | ET |
| `FMT_PATH_INVALID` | Format Plugins | Service API | 200 (`kind: error`) | no | A path is absolute, has `..`, `\` or NUL | ET |
| `FMT_ROOT_NOT_SINGLE` | Format Plugins | Service API | 200 (`kind: error`) | no | `TreeSource::root` finds no WorkspaceProfile or more than one | ET |
| `FMT_TREE_TOO_LARGE` | Format Plugins | Service API | 200 (`kind: error`) | no | Tree over 20,000 files or 256 MiB | ET |
| `FMT_VERSION_UNTESTED` | Format Plugins | Service API | 200 (`kind: error`) | no | APM write to a repository whose declared version is untested (R-70, C-62 fallback) | ET |
| `FMT_WRITE_UNSUPPORTED` | Format Plugins | Service API | 200 (`kind: error`) | no | `write` called on Claude Code or Copilot | ET |
| `FTM_EXTENSION_DUPLICATE` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Duplicate app row; installed without verification; bad file-type map entry | ET |
| `FTM_EXTENSION_INVALID` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Duplicate app row; installed without verification; bad file-type map entry | ET |
| `FTM_IMMUTABLE` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Duplicate app row; installed without verification; bad file-type map entry | ET |
| `FTM_MEDIA_TYPE_INVALID` | Craft Toolkit | Service API | 200 (`kind: error`) | no | Duplicate app row; installed without verification; bad file-type map entry | ET |
| `FVER_LATEST_PRUNE` | File Service | Service API | 200 (`kind: error`) | no | Prune would remove a path's latest version | ET |
| `FVER_PATH_INVALID` | File Service | Service API | 200 (`kind: error`) | no | path: relative to the place, ≤ 4096 bytes | 2C |
| `FVER_VERSION_GAP` | File Service | Service API | 200 (`kind: error`) | yes | Version number gap or concurrent insert after one retry | ET |
| `GHOP_ABOVE_CEILING` | Project Service | Service API | 200 (`kind: error`) | no | approval_required = false: refused when the profile's ceiling requires approval for the kind (04:139) | 2C |
| `GHOP_DUPLICATE` | Project Service | Service API | 200 (`kind: error`) | no | (project_id, operation_kind): unique | 2C |
| `GH_CONFLICT` | Project Service | Service API | 200 (`kind: error`) | no | `GithubAccess`: file changed since its sha was read, or branch exists | ET |
| `GH_FAILED` | Project Service | Service API | 200 (`kind: error`) | no | `GithubAccess`: any other tool failure | ET |
| `GH_HOST_NOT_SUPPORTED` | Project Service | Service API | 200 (`kind: error`) | no | `GithubAccess`: host is neither GitHub nor the company's GitLab, or the method is GitHub-only | ET |
| `GH_NOT_FOUND` | Project Service | Service API | 200 (`kind: error`) | no | `GithubAccess`: repository, file or pull request not found or not visible | ET |
| `GH_RATE_LIMITED` | Project Service | Service API | 200 (`kind: error`) | yes | `GithubAccess`: host rate limit | ET |
| `GH_SIGN_IN_NEEDED` | Project Service | Service API | 200 (`kind: error`) | no | `GithubAccess`: no sign-in for the host | ET |
| `GH_UNREACHABLE` | Project Service | Service API | 200 (`kind: error`) | yes | `GithubAccess`: host unreachable | ET |
| `GRANT_DUPLICATE` | Workspace Service | Service API | 200 (`kind: error`) | no | (workspace_id, grantee, place_id): unique | 2C |
| `GRANT_PLACE_NOT_IN_WORKSPACE` | Workspace Service | Service API | 200 (`kind: error`) | no | Grant on a place not in the workspace | ET |
| `HAA_ABANDONED` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | No answer for 15 minutes while following | ET |
| `HAA_ADDRESS_INVALID` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | Remote agent not in the directory, or not `https` | ET |
| `HAA_AGENT_UNKNOWN` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | Remote agent not in the directory, or not `https` | ET |
| `HAA_ANSWER_NOT_OFFERED` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | `resume` with `Answer` | ET |
| `HAA_APPROVALS_UNSUPPORTED` | Hosted Agent Adapter | internal | — | no | Hosted agent without the approver routes | ET |
| `HAA_ATTACHMENTS_DROPPED` | Hosted Agent Adapter | internal | — | no | `StartRun.attachments` non-empty | ET |
| `HAA_CANCEL_UNSUPPORTED` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | `cancel` on a hosted agent without the route | ET |
| `HAA_CARD_DECIDED` | Hosted Agent Adapter *(also: Ordin Desktop, Service API)* | Service API | 200 (`kind: error`) | no | Approver card already decided | ET |
| `HAA_CARD_UNKNOWN` | Hosted Agent Adapter *(also: Ordin Desktop, Service API)* | Service API | 200 (`kind: error`) | no | Approver card unknown | ET |
| `HAA_COMMAND_FORM` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | Text over 2C limits or command not a slug | ET |
| `HAA_DEADLINE` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | yes | `ctx.deadline` passed | ET |
| `HAA_EVENT_MALFORMED` | Hosted Agent Adapter | internal | — | no | SSE frame not parseable | ET |
| `HAA_INPUT_INVALID` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | Text over 2C limits or command not a slug | ET |
| `HAA_NO_ACCESS` | Hosted Agent Adapter *(also: Ordin Desktop, Service API)* | Service API | 200 (`kind: error`) | no | Token lacks caller role | ET |
| `HAA_QUEUE_FULL` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | yes | Thread busy, own FIFO full | ET |
| `HAA_REQUEST_REFUSED` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | Request refused (400/413) | ET |
| `HAA_RESUME_REFUSED` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | `resume` when not paused, or refused by the hosted agent | ET |
| `HAA_RUN_UNKNOWN` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | `read`/`events` of a run the adapter cannot find | ET |
| `HAA_SIGNED_OUT` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | No company sign-in, refresh refused, or two 401s | ET |
| `HAA_STREAM_UNSUPPORTED` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | Events route absent (404/405) | ET |
| `HAA_TARGET_MISMATCH` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | `StartRun.target` is an agent instance | ET |
| `HAA_THREAD_BUSY_REMOTE` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | Remote 409 `RUN_THREAD_BUSY` | ET |
| `HAA_THREAD_UNKNOWN` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | no | Thread unknown at the hosted agent | ET |
| `HAA_UNREACHABLE` | Hosted Agent Adapter | Service API | 200 (`kind: error`) | yes | Network, TLS, 429, 5xx, request time limit | ET |
| `HAND_ALREADY_ACCEPTED` | Handover Service | Service API | 200 (`kind: error`) | no | Take back after acceptance | ET |
| `HAND_CHECKS_FAILING` | Handover Service | Service API | 200 (`kind: error`) | no | A check fails | ET |
| `HAND_EVENT_NOT_APPEND` | Handover Service | Service API | 200 (`kind: error`) | no | events: append-only; each installation appends only its own (04:398) | 2C |
| `HAND_ISSUE_TYPE_NONE` | Handover Service | internal | — | no | ordin cannot create an issue in the Jira project (outcome `SendFailed`) (ADR-106) | ET |
| `HAND_JIRA_ASSIGN_FAILED` | Handover Service | internal | — | no | Label or assignment still refused after 3 checks (outcome, screen line) (ADR-106) | ET |
| `HAND_JIRA_TOO_LONG` | Handover Service | Service API | 200 (`kind: error`) | no | The opening comment would exceed 32,000 characters (ADR-106) | ET |
| `HAND_LIFECYCLE_UNKNOWN` | Handover Service | Service API | 200 (`kind: error`) | no | Lifecycle or stage unknown | ET |
| `HAND_LINK_INVALID` | Handover Service | Service API | 200 (`kind: error`) | no | 'open_link' text is not a handover link of the fixed form | ET |
| `HAND_LINK_PLACE_REFUSED` | Handover Service | Service API | 200 (`kind: error`) | no | The link's place is not on GitHub or SharePoint | ET |
| `HAND_NOTE_TOO_LONG` | Handover Service | Service API | 200 (`kind: error`) | no | Note section over 4000 chars | ET |
| `HAND_NOT_FOUND_YET` | Handover Service | Service API | 200 (`kind: error`) | no | Place followed, handover not readable there yet | ET |
| `HAND_PLACE_BUSY` | Handover Service | Service API | 200 (`kind: error`) | yes | Conditional write keeps conflicting | ET |
| `HAND_PLACE_REFUSED` | Handover Service | internal | — | no | The place refuses the write | ET |
| `HAND_PLACE_SITE_OTHER` | Handover Service | Service API | 200 (`kind: error`) | no | The Jira place is on a different Jira site from the one connected (ADR-106) | ET |
| `HAND_PLACE_TYPE_UNAVAILABLE` | Handover Service | Service API | 200 (`kind: error`) | no | A Jira place in a programme without the Jira Adapter (the server) (ADR-106) | ET |
| `HAND_PLACE_UNREACHABLE` | Handover Service | Service API | 200 (`kind: error`) | yes | Place unreachable | ET |
| `HAND_QUERY_INVALID` | Handover Service | Service API | 200 (`kind: error`) | no | `find_people` query empty or over 100 characters | ET |
| `HAND_REASON_MISSING` | Handover Service | Service API | 200 (`kind: error`) | no | Return without a reason | ET |
| `HAND_RECEIVER_INVALID` | Handover Service | Service API | 200 (`kind: error`) | no | Receiver address invalid | ET |
| `HAND_RECORD_UNREADABLE` | Handover Service | internal | — | no | Record unreadable (`record_schema` other; the restricted reader's `YAML_FEATURE_REFUSED`, `YAML_MALFORMED` or `YAML_TOO_LARGE`) | ET |
| `HAND_RESULTS_TOO_MANY` | Handover Service | Service API | 200 (`kind: error`) | no | More than 50 results | ET |
| `HAND_RESULT_UNREADABLE` | Handover Service | Service API | 200 (`kind: error`) | no | results: 1–50 links (assumed), each readable by the receiver (Checker, 02:734) | 2C |
| `HAND_SIGN_IN_NEEDED` | Handover Service | Service API | 200 (`kind: error`) | no | Sign-in for the place expired | ET |
| `HAND_STAGE_IS_LAST` | Handover Service | Service API | 200 (`kind: error`) | no | Handing over from a last stage | ET |
| `HAND_STAGE_MISMATCH` | Handover Service | Service API | 200 (`kind: error`) | no | lifecycle_id, stage_name: both present or both absent | 2C |
| `HAND_STAGE_UNKNOWN` | Handover Service | Service API | 200 (`kind: error`) | no | Lifecycle or stage unknown | ET |
| `HAND_STATE_INVALID` | Handover Service | Service API | 200 (`kind: error`) | no | Action not allowed in the state | ET |
| `HAND_STORE_FAILED` | Handover Service | Service API | 200 (`kind: error`) | yes | Datastore failure | ET |
| `HAND_TITLE_INVALID` | Handover Service | Service API | 200 (`kind: error`) | no | Title empty or over 120 chars | ET |
| `HAND_UNKNOWN` | Handover Service | Service API | 200 (`kind: error`) | no | Unknown handover | ET |
| `HARN_DUPLICATE` | Setup Assistant | Service API | 200 (`kind: error`) | no | `harness_attachments` rule broken (2C) | ET |
| `HARN_PATH_INVALID` | Setup Assistant | Service API | 200 (`kind: error`) | no | `harness_attachments` rule broken (2C) | ET |
| `HARN_SIGN_IN_METHOD_INVALID` | Setup Assistant | Service API | 200 (`kind: error`) | no | `harness_attachments` rule broken (2C) | ET |
| `HARN_STATE_INCONSISTENT` | Setup Assistant | Service API | 200 (`kind: error`) | no | `harness_attachments` rule broken (2C) | ET |
| `HARN_VERSION_INVALID` | Setup Assistant | Service API | 200 (`kind: error`) | no | `harness_attachments` rule broken (2C) | ET |
| `HASH_NOT_SHA256_HEX` | Shared Types | Service API | 200 (`kind: error`) | no | Hash not exactly 64 lowercase hex characters | ET |
| `HOME_DATASTORE_UNAVAILABLE` | Home And Search | Service API | 200 (`kind: error`) | yes | Own tables unreachable | ET |
| `HOME_MARK_DUPLICATE` | Home And Search | Service API | 200 (`kind: error`) | no | Duplicate mark | ET |
| `HOME_PATH_INVALID` | Home And Search | Service API | 200 (`kind: error`) | no | Mark path malformed | ET |
| `HOME_QUERY_INVALID` | Home And Search | Service API | 200 (`kind: error`) | no | Search text empty or over 200 characters | ET |
| `HOME_RUN_NOT_FOUND` | Home And Search | Service API | 200 (`kind: error`) | no | Run again on an unknown run | ET |
| `HOME_SOURCE_UNAVAILABLE` | Home And Search | internal, Service API | 200 (`kind: error`) | yes / no (per row) | A dependency failed while building a view | ET |
| `HOME_WORKSPACE_NOT_FOUND` | Home And Search | Service API | 200 (`kind: error`) | no | Workspace filter unknown | ET |
| `HOOK_APPROVAL_PENDING` | Hook Relay + Ordin Hook | hook | — | no | The person did not decide within 50 s (D.5) | ET |
| `HOOK_APPROVAL_UNAVAILABLE` | Hook Relay | hook | — | no | The Approval Service fails while holding (D.5) | ET |
| `HOOK_BRIDGE_ENV_INVALID` | Ordin Hook | internal | — | no | Bridge environment missing or malformed (G.1) | ET |
| `HOOK_BRIDGE_REFUSED` | Ordin Hook | internal | — | no | Bridge refused, broken or no `ready` within 5 s (G.2–G.4) | ET |
| `HOOK_CORE_TIMEOUT` | Ordin Hook | hook | — | no | No first reply within 2 s (F.3) | ET |
| `HOOK_CORE_UNREACHABLE` | Ordin Hook | hook | — | no | Core not reachable within 1 s (F.1) | ET |
| `HOOK_DECISION_TIMEOUT` | Hook Relay | hook | — | no | The engine does not decide within 1.5 s (D.2) | ET |
| `HOOK_ENDPOINT_TOO_LONG` | Hook Relay | internal | — | no | Socket path too long (A.1) | ET |
| `HOOK_ENV_INVALID` | Ordin Hook | hook | — | no | Environment missing or malformed (B) | ET |
| `HOOK_INTERNAL` | Hook Relay | hook | — | no | Any other internal error (C.9) | ET |
| `HOOK_KEY_UNREADABLE` | Ordin Hook | internal | — | no | Key file missing, unreadable, too open or empty (H) | ET |
| `HOOK_LISTEN_FAILED` | Hook Relay | internal | — | no | Another core holds the socket, or bind fails (A.1, A.2) | ET |
| `HOOK_MALFORMED` | Hook Relay | hook | — | no | Line too long, not JSON, unknown fields, read timeout (C.2, C.3) | ET |
| `HOOK_OUTPUT_FAILED` | Ordin Hook | internal | — | no | Standard output cannot be written (C) | ET |
| `HOOK_PAYLOAD_INVALID` | Ordin Hook | hook | — | no | Payload unreadable or tool name too long (D) | ET |
| `HOOK_PEER_REFUSED` | Hook Relay | hook | — | no | Socket peer is another user (C.1) | ET |
| `HOOK_PROTOCOL_UNSUPPORTED` | Hook Relay | hook | — | no | Protocol other than 1 (C.4) | ET |
| `HOOK_RANDOM_UNAVAILABLE` | Hook Relay | internal | — | yes | No randomness from the operating system (B) | ET |
| `HOOK_REPLAYED` | Hook Relay | hook | — | no | Repeated or malformed call id (C.8) | ET |
| `HOOK_REPLY_MALFORMED` | Ordin Hook | hook | — | no | Reply unreadable, wrong call id, wrong kind, second holding, closed (F.4–F.6) | ET |
| `HOOK_REQUEST_TOO_LARGE` | Ordin Hook | hook | — | no | Request line over 1 MiB (E) | ET |
| `HOOK_SESSION_EXISTS` | Hook Relay | internal | — | no | Session opened twice (B) | ET |
| `HOOK_SESSION_MISMATCH` | Hook Relay | hook | — | no | Agent instance does not match the session (C.7) | ET |
| `HOOK_SESSION_UNKNOWN` | Hook Relay | hook | — | no | No open, live session (C.5) | ET |
| `HOOK_STORE_UNAVAILABLE` | Hook Relay | hook | — | yes | Store unavailable (A.3, D.5) | ET |
| `HOOK_UNAUTHENTICATED` | Hook Relay | hook | — | no | MAC mismatch (C.6) | ET |
| `HOOK_USAGE` | Ordin Hook | internal | — | no | Bad command line (A) | ET |
| `HPLACE_ADDRESS_INVALID` | Handover Service | Service API | 200 (`kind: error`) | no | address: `https` URL ≤ 2048 bytes; unique | 2C |
| `HPLACE_NAME_INVALID` | Handover Service | Service API | 200 (`kind: error`) | no | display_name: 1–120 chars (assumed) | 2C |
| `ID_MEMORY_REF_INVALID` | Shared Types | Service API | 200 (`kind: error`) | no | `MemoryThingId` or `MemoryLinkId` not `entity:` / `relationship:` followed by a lowercase hyphenated UUID (ADR-111) | ET |
| `ID_NOT_UUIDV7` | Shared Types | Service API | 200 (`kind: error`) | no | Identifier not 36 characters, uppercase, unparseable or not version 7 | ET |
| `INPUT_LOCATION_INVALID` | Shared Types *(also: Journal And Artefact Store)* | Service API | 200 (`kind: error`) | no | `InputUsed.location`: 1–2048 chars (assumed), in one of the forms below (closing round, K21) | 2C |
| `INPUT_NAME_INVALID` | Shared Types | Service API | 200 (`kind: error`) | no | Length, display-name, path, URL or base64 rule fails | ET |
| `INPUT_VERSION_INVALID` | Shared Types *(also: Journal And Artefact Store)* | Service API | 200 (`kind: error`) | no | `InputUsed.version_read`: 1–256 chars (assumed) | 2C |
| `INST_BUNDLE_UNAVAILABLE` | Installers | Service API | 200 (`kind: error`) | no | Bundle missing or corrupt | ET |
| `INST_FOLDER_NOT_EMPTY` | Installers | Service API | 200 (`kind: error`) | no | `write_tree` target already holds a path of the tree | ET |
| `INST_FOLDER_UNAVAILABLE` | Installers | Service API | 200 (`kind: error`) | yes / no (per row) | Agent folder missing | ET |
| `INST_HARNESS_UNSUPPORTED` | Installers | Service API | 200 (`kind: error`) | no | C-60/C-01 rule out Copilot (`COPILOT_SUPPORTED = false`) and the harness is `CopilotCli` | ET |
| `INST_IMPORT_CYCLE` | Installers | Service API | 200 (`kind: error`) | no | `@` import deeper than 5 or circular | ET |
| `INST_LOOP_UNSUPPORTED` | Installers | Service API | 200 (`kind: error`) | no | Loop strategy or capability not supported by the installer | ET |
| `INST_NOT_INSTALLED` | Installers | Service API | 200 (`kind: error`) | no | Update, verify, repair or uninstall with no install record | ET |
| `INST_PATH_INVALID` | Installers | Service API | 200 (`kind: error`) | no | A part file name is absolute, has `..`, `\` or NUL | ET |
| `INST_RECORD_INVALID` | Installers | Service API | 200 (`kind: error`) | no | `install-record.json` does not parse or has another schema | ET |
| `INST_REPAIR_FAILED` | Installers | Service API | 200 (`kind: error`) | no | Repair cannot rebuild from the lock file | ET |
| `INST_WRITE_FAILED` | Installers | Service API | 200 (`kind: error`) | yes / no (per row) | File write, rename or delete fails | ET |
| `INTAKE_BUSY` | Caller API | Caller API | 429 | yes | Intake at its runs-in-progress limit | ET |
| `INTAKE_FORBIDDEN` | Caller API | Caller API | 403 | no | Valid token without a role | ET |
| `INTAKE_NOT_TEXT` | Caller API | Caller API | 400 | no | Intake body empty, not UTF-8 or with NUL | ET |
| `INTAKE_SECRET_INVALID` | Caller API | internal | — | no | An intake secret absent or under 32 characters, at build | ET |
| `INTAKE_SECRET_MISSING` | Caller API | internal | — | no | An intake secret absent or under 32 characters, at build | ET |
| `INTAKE_START_FAILED` | Caller API | Caller API | 503 | yes | Place or run could not be recorded | ET |
| `INTAKE_TOO_LARGE` | Caller API | Caller API | 413 | no | Intake body over its limit | ET |
| `INTAKE_UNAUTHENTICATED` | Caller API | Caller API | 401 | no | No valid token or signature; stale timestamp | ET |
| `INTAKE_UNAVAILABLE` | Caller API | Caller API | 503 | yes | Keys unreadable for an intake token | ET |
| `INTAKE_UNKNOWN` | Caller API | Caller API | 404 | no | Unknown intake | ET |
| `JIRA_ACCESS_DENIED` | Jira Adapter | Service API | 200 (`kind: error`) | no | 403, or an assignment refused (400 on assign) (ADR-106) | ET |
| `JIRA_ACTION_KEY_USED` | Jira Adapter | internal | — | no | External-action key already used (caller defect) (ADR-106) | ET |
| `JIRA_ADDRESS_INVALID` | Jira Adapter | Service API | 200 (`kind: error`) | no | Address not https, not on `*.atlassian.net`, bad key or number, or over 2048 characters (ADR-106) | ET |
| `JIRA_ISSUE_TYPE_NONE` | Jira Adapter | Service API | 200 (`kind: error`) | no | Project offers no issue type that is not a sub-task (ADR-106) | ET |
| `JIRA_NOT_CONNECTED` | Jira Adapter | Service API | 200 (`kind: error`) | no | No Jira credential (ADR-106) | ET |
| `JIRA_NOT_FOUND` | Jira Adapter | Service API | 200 (`kind: error`) | no | 404, or attachment id not on the issue (ADR-106) | ET |
| `JIRA_OUTCOME_UNKNOWN` | Jira Adapter | Service API | 200 (`kind: error`) | no | Comment or issue creation timed out after sending and could not be found; never retried automatically (ADR-106) | ET |
| `JIRA_QUERY_INVALID` | Jira Adapter | Service API | 200 (`kind: error`) | no | Search text, label, key list, account id, summary or limit out of range; 400 on a built JQL (ADR-106) | ET |
| `JIRA_RESPONSE_INVALID` | Jira Adapter | Service API | 200 (`kind: error`) | no | Body unparseable, over 10 MiB, missing a field, unknown status category, or a redirect to another host (ADR-106) | ET |
| `JIRA_SIGN_IN_NEEDED` | Jira Adapter | Service API | 200 (`kind: error`) | no | Credential needs sign-in, scope missing, second 401, or OAuth site not granted (ADR-106) | ET |
| `JIRA_SITE_MISMATCH` | Jira Adapter | Service API | 200 (`kind: error`) | no | Argument names a site other than the connected one (ADR-106) | ET |
| `JIRA_THROTTLED` | Jira Adapter | Service API | 200 (`kind: error`) | yes | 429 after retries (ADR-106) | ET |
| `JIRA_TOKEN_REFUSED` | Jira Adapter | Service API | 200 (`kind: error`) | no | Pasted token refused, blocked by an Atlassian admin, or email or token out of range (ADR-106) | ET |
| `JIRA_TOO_LARGE` | Jira Adapter | Service API | 200 (`kind: error`) | no | Attachment over 100 MiB, over 2,000 comments, body over 32,767 characters, or 413 (ADR-106) | ET |
| `JIRA_UNAVAILABLE` | Jira Adapter | Service API | 200 (`kind: error`) | yes | 5xx, network failure or timeout before sending, after retries; Journal unavailable (ADR-106) | ET |
| `JOB_BAD_CRON` | Job Runner | internal | — | no | A timer cron that is not five valid fields | ET |
| `JOB_BAD_KEY` | Job Runner *(also: Caller API)* | Caller API, internal | 400 | no | A key not of its kind's form, or over 400 chars | ET |
| `JOB_BUSY` | Job Runner | internal | — | no | `run_one`: the job runs under another instance's live lease | ET |
| `JOB_CHECKPOINT_TOO_LARGE` | Job Runner | internal | — | no | A checkpoint over 1,048,576 bytes | ET |
| `JOB_DB_UNAVAILABLE` | Job Runner *(also: Caller API)* | Caller API, internal | 503 | yes | The datastore cannot be reached | ET |
| `JOB_DUPLICATE` | Job Runner | internal | — | no | Two timers of one name | ET |
| `JOB_FAILED` | Job Runner | internal | — | no | A step panicked | ET |
| `JOB_LEASE_LOST` | Job Runner | internal | — | no | `save`, `finish`, `fail` by an instance that lost the lease | ET |
| `JOB_REQUEST_CONFLICT` | Job Runner *(also: Caller API)* | Caller API, internal | 409 | yes | `request` found no open job after three refused inserts | ET |
| `JOB_REVISITED` | Job Runner | internal | — | no | A step returned a checkpoint already saved | ET |
| `JOB_STALLED` | Job Runner | internal | — | no | A running job saved no checkpoint for 600 s | ET |
| `JRNL_BACKUP_FAILED` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Backup copy before migrating cannot be written | ET |
| `JRNL_DATASTORE_OPEN_FAILED` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | SQLite file fails `PRAGMA quick_check`, or cannot be created or opened | ET |
| `JRNL_DATASTORE_UNAVAILABLE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | yes | A datastore call fails (connection lost, disk I/O, lock timeout after 5 s) | ET |
| `JRNL_ENTRY_INVALID` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Journal entry fails its rules | ET |
| `JRNL_EVENT_KIND_INVALID` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Event kind malformed, `reset`, or `data` not an object | ET |
| `JRNL_EVENT_TOO_LARGE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Event data larger than 256 KiB | ET |
| `JRNL_EXPORT_INVALID` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Support export asked for no run or more than 20 runs | ET |
| `JRNL_EXT_STATE_INVALID` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Confirm or mark-arrived on an external action in the wrong state | ET |
| `JRNL_MIGRATION_CHANGED` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | An applied step's SQL differs from the registered step | ET |
| `JRNL_MIGRATION_DUPLICATE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | A module registered twice, or versions not 1, 2, 3 … | ET |
| `JRNL_MIGRATION_FAILED` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | A migration step fails; transaction rolled back | ET |
| `JRNL_NOT_FOUND` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Record, report, dashboard or plan not found | ET |
| `JRNL_PLAN_NOT_FOUND` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Record, report, dashboard or plan not found | ET |
| `JRNL_RESUME_TOO_OLD` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | `events_since` asked for a sequence older than kept | ET |
| `JRNL_RETENTION_ABOVE_COMPANY_MAX` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | History days above the company maximum | ET |
| `JRNL_RETENTION_INVALID` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | History days < 1, empty or not a whole number | ET |
| `JRNL_SCHEMA_BEHIND` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Scaled-shape instance started on a schema with pending steps | ET |
| `JRNL_SCHEMA_NEWER` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Database written by a newer ordin (unknown module or higher version) | ET |
| `JRNL_SEQ_AHEAD` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | `events_since` asked for a sequence beyond `last_seq` | ET |
| `KNOW_ADOPT_PERSONAL_DATA` | Knowledge Service | internal | — | no | A page to adopt may hold personal data; the Team Knowledge Service maps it to `TEAM_ADOPT_PERSONAL_DATA` (ADR-110) | ET |
| `KNOW_ADOPT_REFUSED` | Knowledge Service | internal | — | yes / no (per cause) | A page to adopt reads as an instruction, or the content check could not run; mapped to `TEAM_ADOPT_REFUSED` (ADR-110) | ET |
| `KNOW_AGENT_PAGE_NOT_FOUND` | Knowledge Service | Service API | 200 (`kind: error`) | no | Agent reference page not in the agent's `knowledge/` folder | ET |
| `KNOW_ALIAS_INVALID` | Knowledge Service | Service API | 200 (`kind: error`) | no | An alias empty, over 200 characters, with control characters, or more than 16 (ADR-104) | ET |
| `KNOW_BAD_PATTERN` | Knowledge Service | internal | — | no | A question-type pattern does not compile | ET |
| `KNOW_BODY_TOO_LARGE` | Knowledge Service | Service API | 200 (`kind: error`) | no | Body over 1 MiB | ET |
| `KNOW_CHECKPOINT_INVALID` | Knowledge Service | Service API | 200 (`kind: error`) | no | `ingest_step`: checkpoint of another shape | ET |
| `KNOW_CONCEPT_INVALID` | Knowledge Service *(also: Team Knowledge Service, recorded as `TEAM_FILE_UNREADABLE`)* | internal | — | no | A concept file of a plain Open Knowledge Format bundle is not UTF-8, too large, or its front matter cannot be read; the get continues (ADR-112) | ET |
| `KNOW_DB_UNAVAILABLE` | Knowledge Service | Service API, MCP tool | 200 (`kind: error`) | yes / no (per row) | `ingest_step`: the chat history cannot be read, or indexing the page fails (SA-CLOSE-02) | ET |
| `KNOW_DESCRIPTION_INVALID` | Knowledge Service *(also: Ordin Host, passed through as a tool error)* | Service API | 200 (`kind: error`) | no | A page description empty, over 200 characters, over one line, or with control characters (ADR-112) | ET |
| `KNOW_ENTRY_INVALID` | Knowledge Service *(also: Caller API)* | Caller API, Service API | 400 (Caller API); 200 `kind: error` (Remote API) | no | Entry text empty or over 20,000 chars | ET |
| `KNOW_ENTRY_NOT_FOUND` | Knowledge Service *(also: Caller API)* | Caller API, Service API | 404 (Caller API); 200 `kind: error` (Remote API) | no | Entry id unknown | ET |
| `KNOW_FILE_UNAVAILABLE` | Knowledge Service | Service API | 200 (`kind: error`) | yes | File Service cannot write or read the page file | ET |
| `KNOW_HEADER_INVALID` | Knowledge Service | internal | — | no | Page file header cannot be read (edited outside ordin) | ET |
| `KNOW_HISTORY_UNAVAILABLE` | Knowledge Service | Service API | 200 (`kind: error`) | yes / no (per row) | `ingest_step`: the chat history cannot be read | ET |
| `KNOW_LANGUAGE_UNKNOWN` | Knowledge Service | Service API | 200 (`kind: error`) | no | `ingest_step`: the chat history cannot be read, or indexing the page fails (SA-CLOSE-02) | ET |
| `KNOW_NOTE_EMPTY` | Knowledge Service | MCP tool | — | no | Note with no text | ET |
| `KNOW_NOTE_LIMIT` | Knowledge Service | MCP tool | — | no | Note beyond `max-notes-per-scope` | ET |
| `KNOW_NOTE_NOT_FOUND` | Knowledge Service *(also: Caller API)* | Caller API, MCP tool | 404 | no | Replace / retire an unknown note or a non-UUID id | ET |
| `KNOW_NOTE_TOO_LONG` | Knowledge Service | MCP tool | — | no | Note over `max-note-chars` | ET |
| `KNOW_NOT_OUT_OF_DATE` | Knowledge Service | Service API | 200 (`kind: error`) | no | Leave it / propose update on a current page | ET |
| `KNOW_NOT_REMOVED` | Knowledge Service | Service API | 200 (`kind: error`) | no | Undo on a page that is not removed | ET |
| `KNOW_NO_NOTES_SOURCE` | Knowledge Service | internal | — | no | Note tools wanted with no `notes` source | ET |
| `KNOW_NO_PENDING_UPDATE` | Knowledge Service | Service API | 200 (`kind: error`) | no | Use / keep an update when none is pending | ET |
| `KNOW_OKF_HEADER_INVALID` | Knowledge Service *(also: Knowledge Reader, which reads the file as a document)* | internal | — | no | `okf::parse_concept`: the front matter does not pass the restricted YAML reader (ADR-112) | ET |
| `KNOW_OKF_NOT_UTF8` | Knowledge Service *(also: Knowledge Reader, outcome `not_utf8`)* | internal | — | no | `okf::parse_concept`: the file is not UTF-8 (ADR-112) | ET |
| `KNOW_OKF_RESERVED` | Knowledge Service | internal | — | no | `okf::parse_concept` given `index.md` or `log.md`, which are never concepts (ADR-112) | ET |
| `KNOW_OKF_TOO_LARGE` | Knowledge Service *(also: Knowledge Reader, outcome `too_large`)* | internal | — | no | `okf::parse_concept`: the file is over the page bound (ADR-112) | ET |
| `KNOW_PAGE_ID_INVALID` | Knowledge Service | MCP tool | — | no | `knowledge_read_page` with a value that is not a UUID | ET |
| `KNOW_PAGE_LIMIT` | Knowledge Service | Service API | 200 (`kind: error`) | no | 500 pages already in the workspace | ET |
| `KNOW_PAGE_NOT_FOUND` | Knowledge Service | Service API, MCP tool | 200 (`kind: error`) | no | Page id unknown (or bin emptied before Undo) | ET |
| `KNOW_PAGE_REMOVED` | Knowledge Service | Service API | 200 (`kind: error`) | no | Write or remove on a removed page | ET |
| `KNOW_PROFILE_MISMATCH` | Knowledge Service | internal, Service API | 200 (`kind: error`) | no | Agent dependencies present on a desktop program, or absent in the agent profile | ET |
| `KNOW_PUBLISH_FAILED` | Knowledge Service | Service API | 200 (`kind: error`) | yes | Push Service refuses the publish request | ET |
| `KNOW_QUERY_INVALID` | Knowledge Service | MCP tool, Service API | 200 (`kind: error`) | no | `knowledge_search_pages` arguments outside the schema | ET |
| `KNOW_READ_ONLY` | Knowledge Service | Service API | 200 (`kind: error`) | no | An edit, removal, copy, publication or update of another team's page (ADR-110) | ET |
| `KNOW_SCOPE_NOT_ENABLED` | Knowledge Service *(also: Job Runner)* | Service API | 200 (`kind: error`) | no | `ingest_step`: the scope has no enabled chat of the source's connection, or the chat system refuses | ET |
| `KNOW_SOURCES_TOO_MANY` | Knowledge Service | Service API | 200 (`kind: error`) | no | More than 100 sources | ET |
| `KNOW_SOURCE_INVALID` | Knowledge Service | Service API | 200 (`kind: error`) | no | Source origin not `place`/`sharepoint`/`confluence`, or location/version out of bounds | ET |
| `KNOW_SUGGESTION_NOT_FOUND` | Knowledge Service | Service API | 200 (`kind: error`) | no | Suggestion id unknown for the page, or accept on a duplicate suggestion (ADR-104) | ET |
| `KNOW_SUGGESTION_STALE` | Knowledge Service | Service API | 200 (`kind: error`) | no | The page changed since the suggestion was made (ADR-104) | ET |
| `KNOW_TAG_INVALID` | Knowledge Service | Service API | 200 (`kind: error`) | no | A tag empty, over 64 characters, with white space, or more than 32 (ADR-104) | ET |
| `KNOW_TEAM_PAGE_ELSEWHERE` | Knowledge Service | internal | — | no | A read page's id is already a page here or a read page from another folder; mapped to `TEAM_ITEM_HELD_ELSEWHERE` (ADR-110) | ET |
| `KNOW_TEAM_PAGE_INVALID` | Knowledge Service | internal | — | no | A team page file breaks the header or body rules on import (ADR-103) | ET |
| `KNOW_TITLE_DUPLICATE` | Knowledge Service | Service API | 200 (`kind: error`) | no | Title already used by another page of the workspace | ET |
| `KNOW_TITLE_INVALID` | Knowledge Service | Service API | 200 (`kind: error`) | no | Title empty, over 200 chars or with control characters | ET |
| `KNOW_TYPE_INVALID` | Knowledge Service *(also: Ordin Host, passed through as a tool error)* | Service API | 200 (`kind: error`) | no | A page type not one of `PAGE_TYPES` through the Service API or `write_page`, or outside 1–64 characters (ADR-112) | ET |
| `KNOW_UNKNOWN_KIND` | Knowledge Service | Service API | 200 (`kind: error`) | no | `ingest_step`: source unknown, or not `chat-history` | ET |
| `KNOW_UNKNOWN_SOURCE` | Knowledge Service *(also: Caller API)* | Caller API, Service API | 404 (Caller API); 200 `kind: error` (Remote API) | no | `ingest_step`: source unknown, or not `chat-history` | ET |
| `KNOW_WORKSPACE_NOT_FOUND` | Knowledge Service | Service API | 200 (`kind: error`) | no | Workspace absent or in ordin's Bin | ET |
| `KQ_ACCOUNT_LINK_REFUSED` | Knowledge Query Server *(also: Knowledge Query Client, connection details)* | Caller API | 200 (entry of `refused`) | no | A source system refused the token presented for linking (ADR-115) | ET |
| `KQ_ALREADY_CONNECTED` | Knowledge Query Client | Service API | 200 (`kind: error`) | no | The front door is already connected to the workspace (ADR-113) | ET |
| `KQ_ASKER_STATEMENT_INVALID` | Knowledge Query Server | Caller API, knowledge MCP endpoint | 400 | no | An asker statement from a member, malformed, out of bounds or older than 30 s (ADR-115) | ET |
| `KQ_BAD_ACCOUNTS` | Knowledge Query Server *(also: Knowledge Query Client, passed through)* | Caller API | 400 | no | A route 41 or 42 body or path breaks a bound or names a system other than GitHub, GitLab or Confluence (ADR-115) | ET |
| `KQ_BAD_QUERY` | Knowledge Query Server *(also: Knowledge Query Client, internal)* | Caller API, knowledge MCP endpoint | 400 | no | A query body breaks a bound, gives both or neither of `query` and `searches`, or a member sets `access_group` (ADR-113) | ET |
| `KQ_CONFIG_INVALID` | Knowledge Query Client + Knowledge Query Server | internal | — | no | The client or the server is constructed against its configuration rules; the start-up report names the key (ADR-113) | ET |
| `KQ_CONNECTION_NOT_FOUND` | Knowledge Query Client | Service API | 200 (`kind: error`) | no | No such connection for the workspace (ADR-113) | ET |
| `KQ_DELEGATION_NOT_ALLOWED` | Knowledge Query Server | Caller API, knowledge MCP endpoint | 400 | no | A front door or service identity sent a SharePoint delegation (ADR-115) | ET |
| `KQ_INTERNAL` | Knowledge Query Server | Caller API | 500 | no | No `KnowledgePrincipal` in the request: a wiring fault (ADR-113) | ET |
| `KQ_LOOP` | Knowledge Query Server *(also: Knowledge Query Client, which drops the agent silently)* | Caller API | 409 | no | This agent is already in `visited`, or `visited` is full (ADR-113) | ET |
| `KQ_NOTICE_REJECTED` | Knowledge Query Server *(also: Knowledge Query Client, passed through)* | Caller API | 401 | no | A sharing-change notice whose signature or token does not match (ADR-115) | ET |
| `KQ_NOT_A_FRONT_DOOR` | Knowledge Query Server | Caller API | 403 | no | A member, not a listed front door, opens the change stream or sets `access_group` (ADR-113) | ET |
| `KQ_NOT_A_KNOWLEDGE_AGENT` | Knowledge Query Client | Service API | 200 (`kind: error`) | no | The address answers `about`, but not as a knowledge agent (ADR-113) | ET |
| `KQ_NOT_IN_AUDIENCE` | Knowledge Query Server *(also: Knowledge Query Client, passed through to the Service API)* | Caller API, knowledge MCP endpoint, Service API | 403 / 200 (`kind: error`) | no | The asker is in none of the agent's audience groups, or a front door's `access_group` is not one of them; no facts are returned (ADR-113) | ET |
| `KQ_NOT_READY` | Knowledge Query Server | Caller API, knowledge MCP endpoint | 503 | yes | No table of contents published yet: the first reading has not finished (ADR-113) | ET |
| `KQ_NOT_SUPPORTED` | Knowledge Query Client | Service API | 200 (`kind: error`) | no | A connection command in the agent profile, or proxies asked in `Person` mode (ADR-113) | ET |
| `KQ_NO_CONNECTIONS` | Knowledge Query Client | internal | — | no | `ask` on a workspace with no live connection; the Memory Service skips the wider search (ADR-113) | ET |
| `KQ_OTHER_COMPANY` | Knowledge Query Client | Service API | 200 (`kind: error`) | no | The knowledge agent trusts another identity provider (ADR-113) | ET |
| `KQ_SIGN_IN_NEEDED` | Knowledge Query Client | Service API | 200 (`kind: error`) | no | The person must sign in again, or the agent refuses the token twice (ADR-113) | ET |
| `KQ_STORE_UNAVAILABLE` | Knowledge Query Client + Knowledge Query Server *(also: Caller API route 40)* | Service API, Caller API | 200 (`kind: error`) / 503 | yes | The datastore cannot be read or written for connections, the cache, the contents or the status (ADR-113) | ET |
| `KQ_STREAM_OPEN` | Knowledge Query Server | Caller API | 409 | no | A front door opens a second change stream (ADR-113) | ET |
| `KQ_TOO_MANY_CONNECTIONS` | Knowledge Query Client | Service API | 200 (`kind: error`) | no | The workspace already has 8 front doors (ADR-113) | ET |
| `KQ_UNKNOWN_PROXY` | Knowledge Query Client | internal | — | no | `ask` names a proxy the configuration does not list; logged ERROR (ADR-113) | ET |
| `KQ_UNREACHABLE` | Knowledge Query Client | Service API | 200 (`kind: error`) | yes | The knowledge agent is unreachable or timed out, or the sign-in service failed temporarily (ADR-113) | ET |
| `KQ_URL_INVALID` | Knowledge Query Client | Service API | 200 (`kind: error`) | no | The address is not https, is malformed, carries user information or a fragment, or is over 2,048 bytes (ADR-113) | ET |
| `KRD_CYCLE_LEASED` | Knowledge Reader | internal | — | no | Another instance holds the cycle lease; the duty maps it to success (ADR-113) | ET |
| `KRD_DATA_DIR_UNWRITABLE` | Knowledge Reader | internal | — | no | The reader's data directory cannot be created or written; the instance never reports ready (ADR-113) | ET |
| `KRD_DB_UNAVAILABLE` | Knowledge Reader | internal | — | yes | The datastore does not answer; the next firing retries (ADR-113) | ET |
| `KRD_FETCH_FAILED` | Knowledge Reader | internal | — | no | A fetch fails on the network or passes 300 s; per repository, retried at the next cycle (ADR-113) | ET |
| `KRD_HOT_COPY_OFF` | Knowledge Reader | internal | — | no | A hot copy is asked for with no `hot-copy.credential-env` (ADR-113) | ET |
| `KRD_HOT_URL_INVALID` | Knowledge Reader | internal | — | no | A hot copy is asked for an address that is not https (ADR-113) | ET |
| `KRD_LEASE_LOST` | Knowledge Reader | internal | — | yes | The cycle lease was taken over during a cycle; the cycle ends at the next document boundary (ADR-113) | ET |
| `KRD_MEMORY_REFUSED` | Knowledge Reader | internal | — | no | The Memory Service refuses a document's facts permanently; per document (ADR-113) | ET |
| `KRD_MEMORY_UNAVAILABLE` | Knowledge Reader *(also: Duty Timer, logged as the duty's code)* | internal | — | yes | The Memory Service is unavailable for three documents in a row; the cycle stops and resumes at the next firing (ADR-113) | ET |
| `KRD_MODEL_OUTPUT_INVALID` | Knowledge Reader | internal | — | no | The model's output for a document is not the JSON form; given up after 3 attempts for one commit (ADR-113) | ET |
| `KRD_MODEL_READ_FAILED` | Knowledge Reader | internal | — | no | The model is unreachable, out of time or refuses; given up after 3 attempts for one commit (ADR-113) | ET |
| `KRD_NOT_HOT` | Knowledge Reader | internal | — | no | `remove_hot_repository` names a repository that is not a hot copy (ADR-113) | ET |
| `KRD_REPOSITORY_UNREADABLE` | Knowledge Reader | internal | — | no | A repository has no token, refuses it, or has no such repository or branch; shown in the status, never answered from (ADR-113) | ET |
| `KRD_RESTRICTIONS_UNAVAILABLE` | Knowledge Reader | internal | — | yes | A space's page restrictions could not be read while person checks are off; nothing of the space is read this cycle (ADR-115) | ET |
| `KRD_SOURCE_FETCH_FAILED` | Knowledge Reader | internal | — | yes | Listing a Confluence space or SharePoint library failed, was rate-limited or passed 300 s (ADR-115) | ET |
| `KRD_SOURCE_ITEM_REFUSED` | Knowledge Reader | internal | — | no | One page or file was refused or gone when fetched (ADR-115) | ET |
| `KRD_SOURCE_UNREADABLE` | Knowledge Reader | internal | — | no | A Confluence space or SharePoint library the agent's account may not read, or that does not exist (ADR-115) | ET |
| `LANG_DUPLICATE` | Project Service | Service API | 200 (`kind: error`) | no | language: 1–40 chars (assumed); unique | 2C |
| `LANG_SERVER_NOT_APPROVED` | Project Service | Service API | 200 (`kind: error`) | no | default_server, chosen_server: members of `approved_servers` | 2C |
| `LANG_SETTINGS_INVALID` | Project Service | Service API | 200 (`kind: error`) | no | server_settings: JSON object ≤ 64 KiB (assumed) | 2C |
| `LIFE_CHOICES_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | next.one_of: 2–4 choices (assumed, mirrors workflow paths) | 2C |
| `LIFE_DAYS_OUT_OF_RANGE` | Catalogue Service | Service API | 200 (`kind: error`) | no | days_to_answer: 0 (not set) or 1–365 (assumed) | 2C |
| `LIFE_PLACE_INVALID` | Catalogue Service | internal | — | no | A stage's handover place type is unknown, or its Jira target is not an `https` address on `*.atlassian.net` of at most 2,048 bytes (ADR-106) | ET |
| `LIFE_ROLE_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | role: 1–200 chars (assumed) | 2C |
| `LIFE_SEND_BACK_NOT_EARLIER` | Catalogue Service | Service API | 200 (`kind: error`) | no | send_backs.to_stage: names an earlier stage | 2C |
| `LIFE_SEND_BACK_ROUNDS_OUT_OF_RANGE` | Catalogue Service | Service API | 200 (`kind: error`) | no | send_backs.max_rounds: 1–10 (04:329) | 2C |
| `LIFE_SLACK_CHANNEL_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | A stage's 'slackChannel' has a malformed id or name | ET |
| `LIFE_STAGES_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | stages: at least 1, at most 30 (assumed); names unique, 1–80 chars (assumed) | 2C |
| `LIFE_STAGE_BACKWARD` | Catalogue Service | Service API | 200 (`kind: error`) | no | next: no stage leads to an earlier one (04:329) | 2C |
| `LIFE_STAGE_UNREACHABLE` | Catalogue Service | Service API | 200 (`kind: error`) | no | stages: every stage reachable from the first (04:329) | 2C |
| `LIFE_WORKSPACE_FOLLOWS_TWO` | Handover Service | Service API | 200 (`kind: error`) | no | WorkspaceStage.workspace_id: unique | 2C |
| `LOAD_BUILD_FAILED` | Agent Loader | internal | — | no | The file tools or the rules cannot be built | ET |
| `LOAD_BUNDLE_MISSING` | Agent Loader | internal | — | no | The bundle, its manifest, `AGENTS.md`, the Claude Code instructions or plugin folder, or the reviewer file is missing | ET |
| `LOAD_KNOWLEDGE_MISMATCH` | Agent Loader | internal | — | no | The Knowledge Service does not serve the knowledge tools the configuration exposes | ET |
| `LOAD_MANIFEST_INVALID` | Agent Loader | internal | — | no | The manifest does not parse or has another schema version | ET |
| `LOAD_SCHEMA_NEWER` | Agent Loader | internal | — | no | The datastore schema is newer and removed or narrowed something | ET |
| `LOAD_SCHEMA_OLD` | Agent Loader | internal | — | no | The datastore schema is older than ordin needs | ET |
| `LOAD_STORE_UNAVAILABLE` | Agent Loader | internal | — | yes | The datastore cannot be read or written | ET |
| `LOC_BUILD_CATALOGUE_INVALID` | Localisation | Service API | 200 (`kind: error`) | no | A message identifier absent from `en-GB`, a bad `.ftl` file, or an orphan message in another locale | ET |
| `LOC_CATALOGUE_MISSING` | Localisation | Service API | 200 (`kind: error`) | no | `catalogue` for a tag with no bundled catalogue | ET |
| `LOC_FILE_TOO_LARGE` | Localisation | Service API | 200 (`kind: error`) | no | Import: language file over 2 MiB (ADR-109) | ET |
| `LOC_FILE_UNREADABLE` | Localisation | Service API | 200 (`kind: error`) | no | Import: not UTF-8, no `### ordin language file` header, or no parseable message (ADR-109) | ET |
| `LOC_FORMAT_ERROR` | Localisation | internal | — | no | Fluent reports a formatting error for one message | ET |
| `LOC_LANGUAGES_UNAVAILABLE` | Localisation | Service API | 200 (`kind: error`) | no | Any person-language command in the agent profile (`PreferenceStore::None`) (ADR-109) | ET |
| `LOC_LANGUAGE_ACTIVE` | Localisation | Service API | 200 (`kind: error`) | no | Remove the active language (ADR-109) | ET |
| `LOC_LANGUAGE_DRAFTING` | Localisation | Service API | 200 (`kind: error`) | no | Edit, confirm, import or remove a language Ordin is drafting (ADR-109) | ET |
| `LOC_LANGUAGE_EXISTS` | Localisation | Service API | 200 (`kind: error`) | no | Import or draft of a made or imported language that exists, `replace` false (ADR-109) | ET |
| `LOC_LANGUAGE_NOT_CONFIRMED` | Localisation | Service API | 200 (`kind: error`) | no | `set_locale` on a person language not in use (ADR-109) | ET |
| `LOC_LANGUAGE_NOT_EDITABLE` | Localisation | Service API | 200 (`kind: error`) | no | en-GB or a pseudo-locale named for editing, export or import (ADR-109) | ET |
| `LOC_LANGUAGE_UNKNOWN` | Localisation | Service API | 200 (`kind: error`) | no | No bundled or person language with that tag (ADR-109) | ET |
| `LOC_MESSAGE_REFUSED` | Localisation | Service API | 200 (`kind: error`) | no | Edit fails the fixed check (`check_entry`) (ADR-109) | ET |
| `LOC_MESSAGE_UNKNOWN` | Localisation | Service API | 200 (`kind: error`) | no | Message identifier not in en-GB (ADR-109) | ET |
| `LOC_NOT_OFFERED` | Localisation | Service API | 200 (`kind: error`) | no | `set_locale` given a tag the profile does not offer | ET |
| `LOC_NOT_SHIPPED` | Localisation | Service API | 200 (`kind: error`) | no | Restore on a made or imported language (ADR-109) | ET |
| `LOC_REVIEW_INCOMPLETE` | Localisation | Service API | 200 (`kind: error`) | no | Confirm with safety wording not yet checked (ADR-109) | ET |
| `LOC_STORE_CORRUPT` | Localisation | internal | — | no | Preference row holds a value outside the schema | ET |
| `LOC_STORE_UNAVAILABLE` | Localisation | Service API | 200 (`kind: error`) | yes | Preference store cannot be read or written (`sqlx::Error`) | ET |
| `LOC_TAG_INVALID` | Localisation | Service API | 200 (`kind: error`) | no | `set_locale` or `catalogue` given a tag that does not parse as BCP 47 | ET |
| `LOC_TOO_MANY_LANGUAGES` | Localisation | Service API | 200 (`kind: error`) | no | A 21st person language (ADR-109) | ET |
| `MAIL_ADDRESS_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Email link or grant invalid | ET |
| `MAIL_ANSWERS_MISMATCH` | Mail Service | Service API | 200 (`kind: error`) | no | `answers` given with `kind = new`, or missing for reply/forward | ET |
| `MAIL_APPROVAL_UNAVAILABLE` | Mail Service | Service API | 200 (`kind: error`) | yes | Approval Service cannot open the card | ET |
| `MAIL_ARGUMENTS_INVALID` | Mail Service | MCP tool | — | no | Arguments do not match the schema | ET |
| `MAIL_ATTACHMENTS_TOO_LARGE` | Mail Service | Service API | 200 (`kind: error`) | no | > 20 attachments or > 25 MiB in all | ET |
| `MAIL_ATTACHMENT_NOT_FOUND` | Mail Service | Service API | 200 (`kind: error`) | no | Attachment index out of range | ET |
| `MAIL_ATTACHMENT_NOT_READABLE` | Mail Service | Service API | 200 (`kind: error`) | no | Attachment from a place the session may not read | ET |
| `MAIL_ATTACHMENT_TOO_LARGE` | Mail Service | Service API | 200 (`kind: error`) | no | Attachment over 25 MiB | ET |
| `MAIL_BODY_TOO_LARGE` | Mail Service | Service API | 200 (`kind: error`) | no | Body > 1 MiB | ET |
| `MAIL_DOMAIN_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Email link or grant invalid | ET |
| `MAIL_DRAFT_NOT_FOUND` | Mail Service | Service API | 200 (`kind: error`) | no | Draft not found in the workspace | ET |
| `MAIL_DRAFT_STATE_INVALID` | Mail Service | Service API | 200 (`kind: error`) | no | Draft in a state that does not allow the action | ET |
| `MAIL_FAILURE_REASON_MISMATCH` | Mail Service | Service API | 200 (`kind: error`) | no | `failure_reason` present without `not_sent` | ET |
| `MAIL_FROM_NOT_PERSON` | Mail Service | Service API | 200 (`kind: error`) | no | From is not the person | ET |
| `MAIL_GRANT_DUPLICATE` | Workspace Service | Service API | 200 (`kind: error`) | no | Email link or grant invalid | ET |
| `MAIL_GRANT_READ_ONLY` | Mail Service | MCP tool | — | no | Read-only grant calls a draft tool | ET |
| `MAIL_IMAP_STATE_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Email link or grant invalid | ET |
| `MAIL_INCOMING_FILE_REFUSED` | Mail Service | Service API | 200 (`kind: error`) | no | Incoming-file safety check refused the file | ET |
| `MAIL_INTERNAL` | Mail Service | Service API | 200 (`kind: error`) | no | Unexpected internal fault | ET |
| `MAIL_LINK_FIELDS_MISMATCH` | Workspace Service | Service API | 200 (`kind: error`) | no | Email link or grant invalid | ET |
| `MAIL_LINK_NOT_FOUND` | Mail Service | Service API | 200 (`kind: error`) | no | Email link not in the workspace | ET |
| `MAIL_MESSAGE_ID_INVALID` | Mail Service | Service API | 200 (`kind: error`) | no | `message_id` empty or > 998 bytes | ET |
| `MAIL_MESSAGE_NOT_FOUND` | Mail Service | Service API | 200 (`kind: error`) | no | Message no longer in the mailbox | ET |
| `MAIL_MESSAGE_REF_INVALID` | Mail Service | Service API | 200 (`kind: error`) | no | `message_ref` malformed | ET |
| `MAIL_NOT_LINKED` | Mail Service | Service API | 200 (`kind: error`) | no | Message not covered by the workspace's links | ET |
| `MAIL_NOT_SIGNED_IN` | Mail Service | Service API | 200 (`kind: error`) | no | Not signed in to email | ET |
| `MAIL_NO_GRANT` | Mail Service | MCP tool | — | no | Caller has no email grant | ET |
| `MAIL_OFF` | Mail Service | MCP tool | — | no | Email off in the profile, or no profile | ET |
| `MAIL_PROFILE_DOMAINS_MISSING` | Mail Service | Service API | 200 (`kind: error`) | no | `company_server` with no company domains | ET |
| `MAIL_RATE_LIMITED` | Mail Service | Service API | 200 (`kind: error`) | yes | Provider rate limit (HTTP 429, IMAP `[LIMIT]`) | ET |
| `MAIL_RECIPIENTS_INVALID` | Mail Service | Service API | 200 (`kind: error`) | no | Recipients missing, malformed or > 100 | ET |
| `MAIL_RECIPIENT_REFUSED` | Mail Service | Service API | 200 (`kind: error`) | no | SMTP refused one recipient | ET |
| `MAIL_RULE_REFUSED` | Mail Service | Service API | 200 (`kind: error`) | no | Profile rule broken (internal only, maximum recipients, closed place, foot line) | ET |
| `MAIL_SEND_FAILED` | Mail Service | Service API | 200 (`kind: error`) | no | Mail system rejected the send otherwise | ET |
| `MAIL_SEND_WITHOUT_APPROVAL` | Mail Service | Service API | 200 (`kind: error`) | no | Attempt to send a draft not approved, or approval already used | ET |
| `MAIL_SENT_COPY_FAILED` | Mail Service | Service API | 200 (`kind: error`) | no | Accepted, copy to Sent failed | ET |
| `MAIL_SENT_DUPLICATE` | Mail Service | Service API | 200 (`kind: error`) | no | Second `sent_emails` row for a draft | ET |
| `MAIL_SENT_IMMUTABLE` | Mail Service | Service API | 200 (`kind: error`) | no | Update of a sent record other than `saved_in_sent` | ET |
| `MAIL_SERVER_UNREACHABLE` | Mail Service | Service API | 200 (`kind: error`) | yes | Network failure after 3 attempts | ET |
| `MAIL_SIGN_IN_REFUSED` | Mail Service | Service API | 200 (`kind: error`) | no | Server refused the password or token | ET |
| `MAIL_STORE_FAILED` | Mail Service | Service API | 200 (`kind: error`) | yes | Datastore failure | ET |
| `MAIL_SUBJECT_TOO_LONG` | Mail Service | Service API | 200 (`kind: error`) | no | Subject > 998 chars | ET |
| `MAIL_SYSTEM_UNAVAILABLE` | Mail Service | Service API | 200 (`kind: error`) | no | Graph or Gmail registration unavailable (C-43/C-44 fallback) | ET |
| `MAIL_TLS_FAILED` | Mail Service | Service API | 200 (`kind: error`) | no | TLS cannot be set up | ET |
| `MAIL_TOO_LARGE_FOR_SERVER` | Mail Service | Service API | 200 (`kind: error`) | no | Message larger than the server's size | ET |
| `MCP_ALREADY_STARTED` | MCP Front | Service API | 200 (`kind: error`) | no | `start` called twice | ET |
| `MCP_ARGUMENTS_INVALID` | MCP Front | MCP tool | — | no | `check_arguments` refuses | ET |
| `MCP_ARGUMENTS_TOO_LARGE` | MCP Front | MCP tool | — | no | Arguments above 1 MiB | ET |
| `MCP_AWAITING_APPROVAL` | MCP Front | MCP tool | — | no | No decision within `approval_wait` | ET |
| `MCP_BRIDGE_MALFORMED` | MCP Front | Service API | 200 (`kind: error`) | no | Handshake missing within 5 s, over 1 KiB, not JSON | ET |
| `MCP_BRIDGE_PEER_REFUSED` | MCP Front | Service API | 200 (`kind: error`) | no | Bridge peer is another OS user | ET |
| `MCP_BRIDGE_PROTOCOL_UNSUPPORTED` | MCP Front | Service API | 200 (`kind: error`) | no | Handshake `protocol` ≠ 1 | ET |
| `MCP_BRIDGE_UNAUTHENTICATED` | MCP Front | Service API | 200 (`kind: error`) | no | Handshake token does not match | ET |
| `MCP_CALL_CANCELLED` | MCP Front | Service API | 200 (`kind: error`) | no | Call cancelled by the harness or session close | ET |
| `MCP_CALL_TIMEOUT` | MCP Front | MCP tool | — | no | Provider call exceeds `call_timeout` | ET |
| `MCP_DECISION_FAILED` | MCP Front | MCP tool | — | no | Waiting for the person's approval fails (`wait_for_approval` returns `Err`) | ET |
| `MCP_DENIED` | MCP Front | MCP tool | — | no | Rules Engine denies | ET |
| `MCP_INVALID_NAME` | MCP Front | Service API | 200 (`kind: error`) | no | `ServerName::new` given a value breaking `^[a-z][a-z0-9_]{0,31}$` | ET |
| `MCP_JOURNAL_FAILED` | MCP Front | MCP tool, Service API | 200 (`kind: error`) | yes / no (per row) | `report`: `journal.store_report` fails | ET |
| `MCP_NOT_STARTED` | MCP Front | Service API | 200 (`kind: error`) | yes | `open_session` before `start` | ET |
| `MCP_OUTPUT_TRUNCATED` | MCP Front | Service API | 200 (`kind: error`) | no | Output above 4 MiB | ET |
| `MCP_PLAN_NOT_STATED` | MCP Front | MCP tool | — | no | `plan` mark with no plan | ET |
| `MCP_PLAN_ONE_NOW` | MCP Front | MCP tool | — | no | `plan` marks a second step `now` | ET |
| `MCP_PROVIDER_DUPLICATE` | MCP Front | Service API | 200 (`kind: error`) | no | Same `provider_id` registered twice | ET |
| `MCP_PROVIDER_FAILED` | MCP Front | MCP tool, Service API | 200 (`kind: error`) | yes / no (per row) | `tools_changed`: the provider's `offer` fails or exceeds 2 s for an open session | ET |
| `MCP_PROVIDER_PANICKED` | MCP Front | MCP tool | — | no | Provider panics | ET |
| `MCP_REF_RESOLVER_ALREADY_BOUND` | MCP Front *(also: Ordin Desktop)* | Service API | 200 (`kind: error`) | no | `bind_ref_resolver` called a second time | ET |
| `MCP_REF_RESOLVER_NOT_BOUND` | MCP Front | MCP tool | — | yes | A `report` or `plan` call before the program bound the resolver | ET |
| `MCP_REGISTRY_FROZEN` | MCP Front | Service API | 200 (`kind: error`) | no | `register` after `start` | ET |
| `MCP_REJECTED` | MCP Front | MCP tool | — | no | The approval expired without a decision | ET |
| `MCP_REST_OFFSET_INVALID` | MCP Front | MCP tool | — | no | `read_rest` offset beyond the end | ET |
| `MCP_REST_UNKNOWN` | MCP Front | MCP tool | — | no | `read_rest` reference unknown or evicted | ET |
| `MCP_SERVER_CLAIMED` | MCP Front | Service API | 200 (`kind: error`) | no | A server name already claimed by another provider, or a name or prefix that overlaps another provider's prefix | ET |
| `MCP_SERVER_NOT_OFFERED` | MCP Front | Service API | 200 (`kind: error`) | no | Handshake server not launched for the session | ET |
| `MCP_SESSION_CLOSED` | MCP Front | Service API | 200 (`kind: error`) | no | Call cancelled by the harness or session close | ET |
| `MCP_SESSION_EXISTS` | MCP Front | Service API | 200 (`kind: error`) | no | `open_session` for a run with an open session | ET |
| `MCP_SESSION_UNKNOWN` | MCP Front | Service API | 200 (`kind: error`) | no | `offered_tools` / `session_tools` / `dispatch` for an unknown run | ET |
| `MCP_SHUTTING_DOWN` | MCP Front | MCP tool | — | no | Program shutting down | ET |
| `MCP_SOCKET_UNAVAILABLE` | MCP Front | Service API | 200 (`kind: error`) | yes | Socket or pipe cannot be bound | ET |
| `MCP_TOOL_NOT_OFFERED` | MCP Front | MCP tool | — | no | Tool not offered to the session (or not sandbox-callable from the sandbox) | ET |
| `MCP_TOOL_SPEC_INVALID` | MCP Front | MCP tool, internal | — | no | A `ToolSpec` breaks a rule of step 2.3 (name pattern, description length for the profile, `argument_form`, `plain`, schema), or the server prefix… | ET |
| `MCP_TOO_MANY_CALLS` | MCP Front | MCP tool | — | no | More than 64 calls in flight for 30 s | ET |
| `MEM_ABOUT_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | `about` outside 1–200 characters | ET |
| `MEM_ADOPT_PERSONAL_DATA` | Memory Service | internal | — | no | The fact to adopt has a possible personal-data finding; mapped to `TEAM_ADOPT_PERSONAL_DATA` (ADR-110) | ET |
| `MEM_ADOPT_REFUSED` | Memory Service | internal | — | no | The fact to adopt reads as an instruction or the write gate does not admit it; mapped to `TEAM_ADOPT_REFUSED` (ADR-110) | ET |
| `MEM_ADOPT_SOURCE_NOT_READ` | Memory Service | internal | — | no | `adopt_fact` on a fact that is not from a read folder; mapped to `TEAM_ADOPT_NOT_READ_ITEM` (ADR-110) | ET |
| `MEM_ALREADY_EVERYWHERE` | Memory Service | Service API | 200 (`kind: error`) | no | `remember_everywhere` on a fact already in all workspaces | ET |
| `MEM_ALREADY_KEPT` | Memory Service | Service API | 200 (`kind: error`) | no | `memory.keep` on a fact that is already known or a summary (ADR-111) | ET |
| `MEM_CLASS_EXCLUDED` | Memory Service | Service API | 200 (`kind: error`) | no | Class in the agent's `excludes` | ET |
| `MEM_CLASS_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | `fact_class` outside 1–128 characters | ET |
| `MEM_CLASS_NOT_RETAINED` | Memory Service | Service API | 200 (`kind: error`) | no | Class not in the agent's `retains` (or the component's authority check refuses) | ET |
| `MEM_CLASS_REQUIRED` | Memory Service | Service API | 200 (`kind: error`) | no | Agent's rules name classes and no class given | ET |
| `MEM_DELETE_UNVERIFIED` | Memory Service | Service API | 200 (`kind: error`) | no | Hard delete verification finds rows left | ET |
| `MEM_DIM_MISMATCH` | Memory Service | Service API | 200 (`kind: error`) | no | A stored embedding has a length other than 384 | ET |
| `MEM_ENTITIES_INVALID` | Memory Service | MCP tool | — (tool error) | no | `entities` of `remember` break a rule: count, length or control characters (ADR-111) | ET |
| `MEM_FACT_NOT_FOUND` | Memory Service | Service API | 200 (`kind: error`) | no | Fact id unknown, or not in this workspace and not "All workspaces" | ET |
| `MEM_FOLDER_UNKNOWN` | Memory Service | MCP tool | — (tool error) | no | `recall` names a folder the workspace does not read, or `folders` breaks its rules (ADR-110) | ET |
| `MEM_IDS_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | `memory.get` with 0 or more than 100 ids | ET |
| `MEM_INTERNAL` | Memory Service | Service API | 200 (`kind: error`) | no | The memory component refuses a body this service built (`VAL_*`), or any unmapped failure | ET |
| `MEM_INVALID_ARGUMENTS` | Memory Service | Service API | 200 (`kind: error`) | no | Arguments fail the schema or carry unknown fields | ET |
| `MEM_LIMIT_INVALID` | Memory Service | internal | — | no | `top_things` with a limit outside 1–1,000 (ADR-113) | ET |
| `MEM_LINKS_INVALID` | Memory Service | MCP tool | — (tool error) | no | A link of `remember` or `memory.correct_link` breaks a rule, or names a thing the fact does not name (ADR-111) | ET |
| `MEM_LINK_NOT_FOUND` | Memory Service | Service API | 200 (`kind: error`) | no | A link id unknown or not readable in this workspace or scope (ADR-111) | ET |
| `MEM_MADE_FROM_INVALID` | Memory Service | MCP tool | — (tool error) | no | `made_from` with fewer than 2 or more than 64 ids, or an id that is not a kept fact of this workspace (ADR-111) | ET |
| `MEM_MADE_FROM_NOT_ALLOWED` | Memory Service | MCP tool | — (tool error) | no | `made_from` from a caller other than Ordin (ADR-111) | ET |
| `MEM_MODEL_UNAVAILABLE` | Memory Service | Service API | 200 (`kind: error`) | yes / no (per row) | A model file is missing or its digest differs | ET |
| `MEM_NOT_HELD` | Memory Service | Service API | 200 (`kind: error`) | no | The personal-data check is asked for a fact that is not held for it (ADR-103) | ET |
| `MEM_NOT_SUPPORTED` | Memory Service | Service API | 200 (`kind: error`) | no | `remember_everywhere`, `erase_scope` or `count_scope` in a program that does not support it | ET |
| `MEM_PAGE_SECTIONS_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | Page has 0 or more than 200 sections | ET |
| `MEM_PAGE_SECTION_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | A section's heading over 200 or text outside 1–4,000 characters | ET |
| `MEM_QUERY_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | Query empty or over 4,096 characters | ET |
| `MEM_QUEUE_UNAVAILABLE` | Memory Service | Service API | 200 (`kind: error`) | yes | Write queue unreachable | ET |
| `MEM_RATE_LIMITED` | Memory Service | Service API | 200 (`kind: error`) | yes | Rate bucket exhausted (600 a minute per scope) | ET |
| `MEM_RECALL_IDS_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | `memory.used` with 0 or more than 20 ids, or an id not of this workspace or scope (ADR-111) | ET |
| `MEM_RESULT_CAP_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | `result_cap` outside 1–20 | ET |
| `MEM_RULES_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | `max_age_days` outside 1–36,500 in `set_agent_rules` | ET |
| `MEM_SAME_THING` | Memory Service | Service API | 200 (`kind: error`) | no | `memory.same_as` with the same thing twice (ADR-111) | ET |
| `MEM_SCOPE_CROSS_WRITE` | Memory Service | Service API | 200 (`kind: error`) | no | Write aimed at a scope other than the session's | ET |
| `MEM_SCOPE_MISSING` | Memory Service | Service API | 200 (`kind: error`) | no | Agent-profile write with no scope on the session | ET |
| `MEM_SEARCHES_INVALID` | Memory Service | MCP tool | — (tool error) | no | `recall` with both or neither of `query` and `searches`, or a sub-query breaking its rules (ADR-110) | ET |
| `MEM_SEARCH_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | `memory.things` search outside 1–200 characters (ADR-111) | ET |
| `MEM_SOURCE_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | Source origin or version outside its limits | ET |
| `MEM_STARTUP_FAILED` | Memory Service | Service API | 200 (`kind: error`) | no | Datastore kind does not match the program | ET |
| `MEM_STATEMENT_INVALID` | Memory Service | Service API | 200 (`kind: error`) | no | Statement empty, over 500 characters, or with control characters | ET |
| `MEM_STORE_TIMEOUT` | Memory Service | Service API | 200 (`kind: error`) | yes | Store statement timeout | ET |
| `MEM_STORE_UNAVAILABLE` | Memory Service | Service API | 200 (`kind: error`) | yes | Store unreachable | ET |
| `MEM_TEAM_BATCH_INVALID` | Memory Service | internal | — | no | A team import batch breaks a size or reference rule (ADR-103) | ET |
| `MEM_TEAM_FACT_ELSEWHERE` | Memory Service | internal | — | no | A team fact's id is already held here from another folder or locally; outcome `NotTaken` (ADR-110) | ET |
| `MEM_TEAM_FACT_INVALID` | Memory Service | internal | — | no | A team fact breaks a field rule on import (ADR-103) | ET |
| `MEM_TEAM_GATE_REFUSED` | Memory Service | internal | — | no | The write gate refused an imported team fact (instruction-like or below the trust bands; ADR-103) | ET |
| `MEM_THING_COUNT_CHANGED` | Memory Service | Service API | 200 (`kind: error`) | no | `memory.forget_thing` when the facts or links differ from what the person confirmed (ADR-111) | ET |
| `MEM_THING_NOT_FOUND` | Memory Service | Service API | 200 (`kind: error`) | no | A thing id unknown or not readable in this workspace or scope (ADR-111) | ET |
| `MEM_TOOL_UNKNOWN` | Memory Service | Service API | 200 (`kind: error`) | no | Tool name not served | ET |
| `MEM_TRUST_NOT_ALLOWED` | Memory Service | internal | — | no | A knowledge agent calls `remember`; it writes only through `index_knowledge_document` (ADR-113) | ET |
| `MEM_WIDER_NOT_SUPPORTED` | Memory Service | MCP tool | — (tool error) | no | `wider` other than `never` from a hosted or knowledge agent, or `always` in the agent profile (ADR-113) | ET |
| `MPOL_KEY_FILE_UNREADABLE` | Model Policy | internal | — | no | The key file is absent, not a regular file or not readable | ET |
| `MPOL_KEY_INVALID` | Model Policy | internal | — | no | The key is empty, above 4,096 bytes, not UTF-8, or holds white space or a control character | ET |
| `MPOL_KEY_MISSING` | Model Policy | internal | — | no | Neither `ORDIN_MODEL_KEY_FILE` nor the secret store gives a key | ET |
| `MPOL_SECRET_MISSING` | Model Policy | internal | — | no | A `model.environment` entry names a secret that has no value (ADR-107) | ET |
| `MPOL_SECRET_STORE_UNAVAILABLE` | Model Policy | internal | — | yes | The secret store cannot be read | ET |
| `NATS_ACK_FAILED` | NATS Adapter | Service API | 200 (`kind: error`) | yes | `+ACK` or `+WPI` not accepted | ET |
| `NATS_CA_INVALID` | NATS Adapter | Service API | 200 (`kind: error`) | no | Profile CA PEM does not parse | ET |
| `NATS_CONSUMER_FAILED` | NATS Adapter | Service API | 200 (`kind: error`) | yes | Creating or binding the durable consumer fails | ET |
| `NATS_CREDENTIALS_MALFORMED` | NATS Adapter | Service API | 200 (`kind: error`) | no | Credentials secret not in one of the three shapes | ET |
| `NATS_CREDENTIALS_REFUSED` | NATS Adapter | Service API | 200 (`kind: error`) | no | Server refuses credentials | ET |
| `NATS_NOT_CONFIGURED` | NATS Adapter | Service API | 200 (`kind: error`) | no | `open_feed` called while `servers` is empty | ET |
| `NATS_NOT_CONNECTED` | NATS Adapter | Service API | 200 (`kind: error`) | yes | `delete_feed` while not connected | ET |
| `NATS_NO_CREDENTIALS` | NATS Adapter | Service API | 200 (`kind: error`) | no | No `nats.credentials` secret | ET |
| `NATS_PAYLOAD_TOO_LARGE` | NATS Adapter | MCP tool | — | no | Payload above the ceiling | ET |
| `NATS_SECRET_STORE_UNAVAILABLE` | NATS Adapter | Service API | 200 (`kind: error`) | yes | Secret store unreadable | ET |
| `NATS_SERVER_URL_INVALID` | NATS Adapter | Service API | 200 (`kind: error`) | no | A profile server URL is not `tls://` or `wss://`, or unparsable | ET |
| `NATS_STREAM_LOOKUP_FAILED` | NATS Adapter | Service API | 200 (`kind: error`) | yes | Stream lookup fails for a reason other than "no stream" | ET |
| `NATS_SUBJECT_INVALID` | NATS Adapter | Service API | 200 (`kind: error`) | no | A profile subject is malformed | ET |
| `NATS_SUBJECT_NOT_ALLOWED` | NATS Adapter | Service API | 200 (`kind: error`) | no | Trigger subject outside `subjects_allowed` | ET |
| `NATS_SUBSCRIBE_FAILED` | NATS Adapter | Service API | 200 (`kind: error`) | yes | Core subscription fails | ET |
| `NUM_NOT_DECIMAL` | Shared Types | Service API | 200 (`kind: error`) | no | Decimal string not matching the pattern | ET |
| `OH_AGENT_MISSING` | Ordin Host | Service API | 200 (`kind: error`) | no | Agent missing at Save | ET |
| `OH_AGENT_UNKNOWN` | Ordin Host | MCP tool | — | no | Agent key not in the directory | ET |
| `OH_ATTACHMENTS_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | More than 20 attachments on a message | ET |
| `OH_BLOCK_ANSWERED` | Ordin Host | Service API | 200 (`kind: error`) | no | Answer to a block already answered | ET |
| `OH_BLOCK_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | Block unknown in the workspace | ET |
| `OH_CHARTER_UNKNOWN` | Ordin Host | MCP tool | — | no | Charter unknown or not this workspace's | ET |
| `OH_CHECK_FAILED` | Ordin Host | Service API | 200 (`kind: error`) | no | Check finds problems on save | ET |
| `OH_CONVERSATION_NOT_CONTINUABLE` | Ordin Host | Service API | 200 (`kind: error`) | no | Continue: the current conversation, or one already continued | ET |
| `OH_CONVERSATION_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | Conversation unknown | ET |
| `OH_COORDINATOR_NO_MOVE` | Ordin Host | Service API | 200 (`kind: error`) | no | Ordin makes no move after one reminder | ET |
| `OH_COORDINATOR_SESSION_LOST` | Ordin Host | Service API | 200 (`kind: error`) | yes | Ordin's run not available during a coordinator call | ET |
| `OH_COORDINATOR_TIMEOUT` | Ordin Host | Service API | 200 (`kind: error`) | no | Ordin's turn exceeds `coordinator_turn_timeout` (10 minutes) | ET |
| `OH_CREW_RUNNING` | Ordin Host | Service API | 200 (`kind: error`) | no | Start fresh while a crew Ordin coordinates runs | ET |
| `OH_CREW_RUN_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | Crew run unknown for Run again | ET |
| `OH_CREW_TOO_MANY_MEMBERS` | Ordin Host | MCP tool | — | no | Crew with more than 8 members | ET |
| `OH_DASHBOARD_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | Dashboard unknown | ET |
| `OH_DATASTORE_UNAVAILABLE` | Ordin Host | Service API | 200 (`kind: error`) | yes | Datastore unavailable | ET |
| `OH_DECISION_KIND_MISMATCH` | Ordin Host | MCP tool | — | no | `decide` kind differs from the request | ET |
| `OH_DEPENDENCY_FAILED` | Ordin Host | MCP tool | — | yes / no (per row) | Agent instance not ready before its session starts (`ensure_ready`, steps 3.1, 8.3) | ET |
| `OH_DRAFT_RUNNING` | Ordin Host | Service API | 200 (`kind: error`) | no | `ordin.draft_language` while a draft runs on this installation (ADR-109) | ET |
| `OH_FOLDER_NOT_LINKED` | Ordin Host | Service API | 200 (`kind: error`) | no | Share to a folder not linked | ET |
| `OH_INPUT_INVALID` | Ordin Host | Service API, MCP tool | 200 (`kind: error`) | no | Import file name empty, over 255 characters or holding a path separator; answer kind not fitting the block | ET |
| `OH_INPUT_MISSING` | Ordin Host | Service API | 200 (`kind: error`) | no | Immediate start without a required input | ET |
| `OH_INPUT_UNREADABLE` | Ordin Host | Service API | 200 (`kind: error`) | no | Immediate start: a place-file input cannot be read | ET |
| `OH_ITEM_WAITING` | Ordin Host | Service API | 200 (`kind: error`) | no | Close on a block still waiting | ET |
| `OH_MESSAGE_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | Message empty or over 8,000 characters | ET |
| `OH_NAME_SUFFIXED` | Ordin Host | Service API | 200 (`kind: error`) | no | Name already used on import | ET |
| `OH_NEEDS_AGENTS` | Ordin Host | Service API | 200 (`kind: error`) | no | Immediate start of a workflow that needs agents | ET |
| `OH_NOTHING_TO_DRAFT` | Ordin Host | Service API | 200 (`kind: error`) | no | `ordin.draft_language` for a language with no message missing (ADR-109) | ET |
| `OH_NOT_COORDINATING` | Ordin Host | MCP tool | — | no | `finish`/`escalate`/`decide` with no crew waiting | ET |
| `OH_NOT_ORDIN_SESSION` | Ordin Host | MCP tool | — | no | Tool call from a run that is not Ordin's current run | ET |
| `OH_NOT_PERSONS_WORDS` | Ordin Host | MCP tool | — | no | `change_setting` inside a Slack turn (ADR-109) | ET |
| `OH_PATH_UNKNOWN` | Ordin Host | MCP tool | — | no | `decide` condition path not offered | ET |
| `OH_REMOTE_AGENT_IN_WORKFLOW` | Ordin Host | MCP tool | — | no | Remote agent in a workflow draft | ET |
| `OH_REMOVE_NOT_CONFIRMED` | Ordin Host | Service API | 200 (`kind: error`) | no | Remove not confirmed | ET |
| `OH_REPOSITORY_UNREACHABLE` | Ordin Host | Service API | 200 (`kind: error`) | no | Repository folder unreadable | ET |
| `OH_RESTORE_SAME` | Ordin Host | Service API | 200 (`kind: error`) | no | Restore of the latest version | ET |
| `OH_SESSION_START_FAILED` | Ordin Host | Service API | 200 (`kind: error`) | yes | Ordin's session cannot start | ET |
| `OH_STALE_VERSION` | Ordin Host | Service API | 200 (`kind: error`) | no | Save on an older base version | ET |
| `OH_SUMMARY_FAILED` | Ordin Host | Service API | 200 (`kind: error`) | yes | Summary turn fails | ET |
| `OH_TEMPLATE_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | Generation template not known | ET |
| `OH_TOO_MANY_WAITING` | Ordin Host | Service API | 200 (`kind: error`) | yes | More than 5 messages queued | ET |
| `OH_VERSION_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | Version unknown | ET |
| `OH_WORKFLOW_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | Workflow unknown | ET |
| `OH_WORKSPACE_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | Workspace unknown | ET |
| `OPS_BAD_REQUEST` | Caller API | Caller API | 400 | no | Operator body breaks its schema; bad `state`, key or name | ET |
| `OPS_BAD_SCOPE` | Caller API | Caller API | 400 | no | `{scope}` of another form | ET |
| `OPS_BODY_TOO_LARGE` | Caller API | Caller API | 413 | no | Operator body over 65,536 bytes | ET |
| `OPS_NOT_FOUND` | Caller API | Caller API | 404 | no | Scope, note, entry or duty not found | ET |
| `OPS_SCOPE_NOT_ALLOWED` | Caller API | Caller API | 400 | no | Settings or entries on the agent-wide scope | ET |
| `PART_COMMIT_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | commit: 40 lowercase hex chars | 2C |
| `PART_DUPLICATE` | Catalogue Service | internal | — | no | Commit refused (format changed, too large, duplicate, checker down) | ET |
| `PART_KIND_UNKNOWN` | Catalogue Service | Service API | 200 (`kind: error`) | no | kind: one of the 25 values (04:312) | 2C |
| `PART_SUPERSEDES_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | supersedes: names a part of the same repository and kind; a part is replaced by at most one (04:314) | 2C |
| `PART_VERSION_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | version: semantic version (assumed) | 2C |
| `PART_WITHDRAWAL_FINAL` | Catalogue Service | Service API | 200 (`kind: error`) | no | withdrawn = true: never set back to false (04:315) | 2C |
| `PDRAFT_ALREADY_OPEN_FOR_PART` | Authoring Service | Service API | 200 (`kind: error`) | no | Open part draft for the part exists | ET |
| `PDRAFT_ANSWER_MISSING` | Authoring Service | Service API | 200 (`kind: error`) | no | Correction question unanswered | ET |
| `PDRAFT_FIELDS_TOO_LARGE` | Authoring Service | Service API | 200 (`kind: error`) | no | Draft field out of its 2C range | ET |
| `PDRAFT_NO_REVIEW_RIGHT` | Authoring Service | Service API | 200 (`kind: error`) | no | No right to open a review | ET |
| `PDRAFT_PART_MISMATCH` | Authoring Service | Service API | 200 (`kind: error`) | no | part_id: absent if and only if `change_kind = new_part` and not yet merged | 2C |
| `PDRAFT_PR_ALREADY_OPEN` | Authoring Service | Service API | 200 (`kind: error`) | no | A pull request is already open | ET |
| `PDRAFT_VERSION_INVALID` | Authoring Service | Service API | 200 (`kind: error`) | no | computed_version: semantic version | 2C |
| `PKG_AGENT_NAME_AMBIGUOUS` | Package Catalogue | Service API | 200 (`kind: error`) | no | Two instances in the workspace share the name a format 1–2 workflow cites | ET |
| `PKG_AGENT_NOT_OFFERED` | Package Catalogue | Service API | 200 (`kind: error`) | no | Agent not offered (failed checks, withdrawn) | ET |
| `PKG_AGENT_UNKNOWN` | Package Catalogue | Service API | 200 (`kind: error`) | no | Agent part unknown | ET |
| `PKG_ALREADY_ADDED` | Package Catalogue | Service API | 200 (`kind: error`) | no | Already added to this workspace | ET |
| `PKG_CHECK_UNAVAILABLE` | Package Catalogue *(also: Ordin Host)* | Service API | 200 (`kind: error`) | yes | Checker unavailable | ET |
| `PKG_CHOICE_NOT_ALLOWED` | Package Catalogue *(also: Ordin Host)* | Service API | 200 (`kind: error`) | no | Choice outside what the agent allows | ET |
| `PKG_DEPRECATED_AS_AGENT` | Package Catalogue | Service API | 200 (`kind: error`) | no | Deprecated part added as an agent | ET |
| `PKG_FOLDER_MISMATCH` | Package Catalogue | Service API | 200 (`kind: error`) | no | Folder differs from its lock file (`verify_agent_folders` without repair) | ET |
| `PKG_FOLDER_UNUSABLE` | Package Catalogue *(also: Ordin Host)* | Service API | 200 (`kind: error`) | no | Folder cannot be verified or rebuilt | ET |
| `PKG_INSTALL_FAILED` | Package Catalogue *(also: Ordin Host)* | Service API | 200 (`kind: error`) | yes | Build, install or inspection failed twice | ET |
| `PKG_INSTANCE_NOT_READY` | Package Catalogue *(also: Ordin Host)* | Service API | 200 (`kind: error`) | no | Instance not ready for a session, or `pending_choices` asked for an instance not `ready`/`updating` | ET |
| `PKG_INSTANCE_UNKNOWN` | Package Catalogue *(also: Ordin Host)* | Service API | 200 (`kind: error`) | no | Instance unknown or removed | ET |
| `PKG_LIBRARY_UNAVAILABLE` | Package Catalogue | Service API | 200 (`kind: error`) | yes | Catalogue Service search failed | ET |
| `PKG_NEEDS_IT_APPROVAL` | Package Catalogue | internal | — | no | Outside the trust policy | ET |
| `PKG_NEED_INVALID` | Package Catalogue | Service API | 200 (`kind: error`) | no | `find_agents` need empty or over 500 characters | ET |
| `PKG_NOT_AN_AGENT` | Package Catalogue | Service API | 200 (`kind: error`) | no | Part is not an agent | ET |
| `PKG_NO_UPDATE` | Package Catalogue | Service API | 200 (`kind: error`) | no | No update waiting | ET |
| `PKG_PACK_MISSING` | Package Catalogue | internal | — | no | Workspace cannot settle a required pack | ET |
| `PKG_STORE_FAILED` | Package Catalogue *(also: Ordin Host)* | Service API | 200 (`kind: error`) | yes / no (per row) | Stored `pending_choices` value unreadable (§N.2) | ET |
| `PKG_TRIAL_NEEDS_TRUST` | Package Catalogue | Service API | 200 (`kind: error`) | no | Trial needs IT approval | ET |
| `PKG_TRIAL_NOT_NAMEABLE` | Package Catalogue | Service API | 200 (`kind: error`) | no | Identity asked for a trial of an own draft | ET |
| `PKG_TRIAL_NO_SCOPE` | Package Catalogue | Service API | 200 (`kind: error`) | no | `source_scope` asked for a trial of an own draft | ET |
| `PKG_UNSAFE` | Package Catalogue | internal | — | no | Checks fail | ET |
| `PKI_BUNDLE_UNREADABLE` | Package Inspector | Service API | 200 (`kind: error`) | no | Bundle directory missing, or its plan hash differs from the lock file's, or a part's primary file cannot be read at `part_page` | ET |
| `PKI_DECLARATION_INVALID` | Package Inspector | Service API | 200 (`kind: error`) | no | `wrightrun-declaration.yaml` does not parse into the Rules Engine's `Declaration` | ET |
| `PKI_DECLARATION_REFUSED` | Package Inspector | Service API | 200 (`kind: error`) | yes | The Rules Engine refuses the rule set (`load_rule_set` → `RulesError`) | ET |
| `PKI_DECLARATION_TOO_LARGE` | Package Inspector | Service API | 200 (`kind: error`) | no | Declaration over 1 MiB (1,048,576 bytes) | ET |
| `PKI_FOLDER_UNREADABLE` | Package Inspector | Service API | 200 (`kind: error`) | yes / no (per row) | Agent folder cannot be listed or read (transient) | ET |
| `PKI_FORMAT_UNRECOGNISED` | Package Inspector | Service API | 200 (`kind: error`) | no | Neither `ordin.lock` nor `apm.yml`, or the APM plug-in refuses the folder | ET |
| `PKI_LOCK_FILE_INVALID` | Package Inspector | Service API | 200 (`kind: error`) | no | `ordin.lock` does not parse or has another schema | ET |
| `PKI_NOT_INSPECTED` | Package Inspector | Service API | 200 (`kind: error`) | no | `model`, `made_of` or `part_page` before any `inspect` | ET |
| `PKI_PART_NOT_FOUND` | Package Inspector | Service API | 200 (`kind: error`) | no | `part_page` for a part the model does not hold | ET |
| `PKI_STORE_FAILED` | Package Inspector | internal, Service API | 200 (`kind: error`) | yes / no (per row) | `reload_rule_sets`: one stored set unparseable or refused | ET |
| `PLACE_NAME_INVALID` | File Service *(also: Shared Types)* | Service API | 200 (`kind: error`) | no | Friendly name empty, too long or with control characters | ET |
| `PLACE_PATH_INVALID` | File Service | Service API | 200 (`kind: error`) | no | Path not absolute or over 4096 bytes | ET |
| `PRE_BROKEN_LINK` | Release Builder | internal | — | no | Link targets missing from the bundle | ET |
| `PRE_CHECK_FAILED` | Release Builder | internal | — | no | The Checker finds one or more stops | ET |
| `PRE_COMPILE_FAILED` | Release Builder | internal | — | no | The Resolver and Compiler cannot compile the instructions | ET |
| `PRE_LAYOUT_FAILED` | Release Builder | internal | — | no | The Claude Code session form cannot be written | ET |
| `PRE_NO_AGENTS_MD` | Release Builder | internal | — | no | The compiled `AGENTS.md` is empty | ET |
| `PRE_NO_APM_DIR` | Release Builder | internal | — | no | No `.apm/` folder | ET |
| `PRE_NO_APM_YML` | Release Builder | internal | — | no | No `apm.yml` in the package folder | ET |
| `PRE_OUTPUT_EXISTS` | Release Builder | internal | — | no | Output folder exists and is not empty | ET |
| `PRE_OUTPUT_INSIDE_PACKAGE` | Release Builder | internal | — | no | Output folder is the package or inside it | ET |
| `PRE_PACKAGE_UNREADABLE` | Release Builder | internal | — | no | The package cannot be read into parts | ET |
| `PRE_SKILL_DESCRIPTION` | Release Builder | internal | — | no | Skill description above 1,024 characters | ET |
| `PRE_SKILL_FRONT_MATTER` | Release Builder | internal | — | no | Skill front matter absent or not readable | ET |
| `PRE_SKILL_NAME` | Release Builder | internal | — | no | Skill name differs from its folder | ET |
| `PRE_UNPARSED` | Release Builder | internal | — | no | A package fact missing or of another form; a package shipping `AGENTS.md`, a manifest or `.ordin` | ET |
| `PROF_ADDRESS_INVALID` | Organisation Profile | Service API | 200 (`kind: error`) | no | Setup command gives a non-`https` or over-long address | ET |
| `PROF_CACHE_INVALID` | Organisation Profile | internal | — | no | Cached profile fails verification at start | ET |
| `PROF_CONTENT_INVALID` | Organisation Profile | Service API | 200 (`kind: error`) | no | Unknown key, wrong type, or a bound broken | ET |
| `PROF_DATASTORE_UNAVAILABLE` | Organisation Profile | Service API | 200 (`kind: error`) | yes | Datastore read or write fails | ET |
| `PROF_FORMAT_UNSUPPORTED` | Organisation Profile | Service API | 200 (`kind: error`) | no | `format_version` not read by this build | ET |
| `PROF_HTTP_STATUS` | Organisation Profile | Service API | 200 (`kind: error`) | yes | The address answers a status other than 200 or 304 | ET |
| `PROF_INSTALLER_INVALID` | Organisation Profile | internal | — | no | `installer.json` unreadable or invalid | ET |
| `PROF_INSTALLER_WRITE_FAILED` | Organisation Profile | Service API | 200 (`kind: error`) | yes | `installer.json` cannot be written | ET |
| `PROF_KEY_UNKNOWN` | Organisation Profile | Service API | 200 (`kind: error`) | no | `kid` names no installer key | ET |
| `PROF_MULTIPLE_ACTIVE` | Organisation Profile | Service API | 200 (`kind: error`) | no | A second `active` row (unique index) | ET |
| `PROF_NOT_USED` | Organisation Profile | Service API | 200 (`kind: error`) | no | Command used in the agent profile | ET |
| `PROF_NO_ADDRESS` | Organisation Profile | Service API | 200 (`kind: error`) | no | No profile address known (desktop: setup cannot finish, R-11) | ET |
| `PROF_REMOTE_AGENT_NOT_ENABLED` | Organisation Profile | Service API | 200 (`kind: error`) | no | `verified` asked for a `listed` remote agent | ET |
| `PROF_REMOTE_AGENT_NOT_FOUND` | Organisation Profile | Service API | 200 (`kind: error`) | no | Remote agent id unknown | ET |
| `PROF_REMOTE_AGENT_WITHDRAWN` | Organisation Profile | Service API | 200 (`kind: error`) | no | Status change on a withdrawn remote agent | ET |
| `PROF_SIGNATURE_INVALID` | Organisation Profile | Service API | 200 (`kind: error`) | no | JWS malformed, wrong algorithm, or signature fails | ET |
| `PROF_SIGNATURE_MISSING` | Organisation Profile | Service API | 200 (`kind: error`) | no | Signature file absent | ET |
| `PROF_TOO_LARGE` | Organisation Profile | Service API | 200 (`kind: error`) | no | Document over 1 MiB or signature over 8 KiB | ET |
| `PROF_UNREACHABLE` | Organisation Profile | Service API | 200 (`kind: error`) | yes | The address cannot be reached (DNS, TLS, proxy, timeout) | ET |
| `PROF_UNVERIFIED_ACTIVATION` | Organisation Profile | Service API | 200 (`kind: error`) | no | Activation of a row that is not `verified` | ET |
| `PROF_VERSION_NOT_NEWER` | Organisation Profile | Service API | 200 (`kind: error`) | no | Version not newer than the active one | ET |
| `PROJ_BRANCH_EXISTS` | Project Service | Service API | 200 (`kind: error`) | no | Branch name taken | ET |
| `PROJ_BRANCH_MISMATCH` | Project Service | Service API | 200 (`kind: error`) | no | 2C rule broken | ET |
| `PROJ_CHECKS_INVALID` | Project Service | Service API | 200 (`kind: error`) | no | 2C rule broken | ET |
| `PROJ_CHECK_DENIED` | Project Service | Service API | 200 (`kind: error`) | no | Check command denied by the rules or the person | ET |
| `PROJ_CHECK_TIMEOUT` | Project Service | Service API | 200 (`kind: error`) | no | Check command over 30 minutes | ET |
| `PROJ_DATASTORE_UNAVAILABLE` | Project Service | Service API | 200 (`kind: error`) | yes | Own tables unreachable | ET |
| `PROJ_DECISION_INVALID` | Project Service | Service API | 200 (`kind: error`) | no | Entry id unknown / hunk no longer applies | ET |
| `PROJ_ENTRY_NOT_FOUND` | Project Service | Service API | 200 (`kind: error`) | no | Entry id unknown / hunk no longer applies | ET |
| `PROJ_FILES_FAILED` | Project Service | Service API | 200 (`kind: error`) | yes | File Service write refused | ET |
| `PROJ_GIT_FAILED` | Project Service | Service API | 200 (`kind: error`) | no | Other git2 error | ET |
| `PROJ_IDE_NOT_FOUND` | Project Service | Service API | 200 (`kind: error`) | yes | IDE not found / failed to start | ET |
| `PROJ_IDE_START_FAILED` | Project Service | Service API | 200 (`kind: error`) | yes | IDE not found / failed to start | ET |
| `PROJ_LSP_METHOD_NOT_ALLOWED` | Project Service | Service API | 200 (`kind: error`) | no | Editor sent an LSP method outside the list | ET |
| `PROJ_LSP_UNAVAILABLE` | Project Service | Service API | 200 (`kind: error`) | yes | Language server off, failed, or not confirmed for worktrees (C-33) | ET |
| `PROJ_NOTHING_TO_COMMIT` | Project Service | Service API | 200 (`kind: error`) | no | Nothing to commit | ET |
| `PROJ_NOT_A_REPOSITORY` | Project Service | Service API | 200 (`kind: error`) | no | Git action on a folder that is not a repository | ET |
| `PROJ_NOT_FOUND` | Project Service | Service API | 200 (`kind: error`) | no | Project id unknown | ET |
| `PROJ_ORIGIN_CHANGED` | Project Service | Service API | 200 (`kind: error`) | no | Remote moved since the files were read | ET |
| `PROJ_PLACE_ALREADY_OPEN` | Project Service | Service API | 200 (`kind: error`) | no | Place already open as a project | ET |
| `PROJ_PUSH_REFUSED` | Project Service | Service API | 200 (`kind: error`) | yes | Push rejected (permission, protected branch, non-fast-forward) | ET |
| `PROJ_RATE_LIMITED` | Project Service | Service API | 200 (`kind: error`) | yes | Host rate limit | ET |
| `PROJ_REMOTE_UNREACHABLE` | Project Service | Service API | 200 (`kind: error`) | yes | Remote unreachable, unsupported host, SSH remote | ET |
| `PROJ_SERVER_INSTALL_FAILED` | Project Service | Service API | 200 (`kind: error`) | yes | Language server could not be installed | ET |
| `PROJ_SIGN_IN_NEEDED` | Project Service | Service API | 200 (`kind: error`) | no | No GitHub or GitLab sign-in | ET |
| `PROJ_TASK_NOT_FOUND` | Project Service | Service API | 200 (`kind: error`) | no | Task id unknown / wrong state for the call | ET |
| `PROJ_TASK_STATE_INVALID` | Project Service | Service API | 200 (`kind: error`) | no | Task id unknown / wrong state for the call | ET |
| `PROJ_TERMINAL_FAILED` | Project Service | Service API | 200 (`kind: error`) | yes | Terminal could not start | ET |
| `PROJ_TOO_MANY_ATTEMPTS` | Project Service | Service API | 200 (`kind: error`) | no | Attempts above the company's cap | ET |
| `PROJ_TOO_MANY_FOLLOWED` | Project Service | Service API | 200 (`kind: error`) | no | More than 20 followed repositories | ET |
| `PROJ_UNDECIDED_CHANGES` | Project Service | Service API | 200 (`kind: error`) | no | Commit while changes wait for a decision | ET |
| `PROJ_WORKING_COPY_DIRTY` | Project Service | Service API | 200 (`kind: error`) | no | Switch would overwrite uncommitted changes | ET |
| `PROJ_WORKTREE_FAILED` | Project Service | Service API | 200 (`kind: error`) | yes | Worktree or branch could not be created or removed | ET |
| `PUB_BRANCH_UNEXPECTED` | File Service | Service API | 200 (`kind: error`) | no | Published copy fields break a 2C rule | ET |
| `PUB_FAILURE_REASON_MISMATCH` | File Service | Service API | 200 (`kind: error`) | no | Published copy fields break a 2C rule | ET |
| `PUB_FORMAT_DESTINATION_MISMATCH` | File Service | Service API | 200 (`kind: error`) | no | Published copy fields break a 2C rule | ET |
| `PUB_FORMAT_MAKER_MISMATCH` | File Service | Service API | 200 (`kind: error`) | no | Published copy fields break a 2C rule | ET |
| `PUSH_ACTION_KEY_PREMATURE` | Push Service | Service API | 200 (`kind: error`) | no | items.external_action_key: set only once the item is pushed (04:103) | 2C |
| `PUSH_ALL_ORIGINS_CHANGED` | Push Service | Service API | 200 (`kind: error`) | no | Every origin changed since read | ET |
| `PUSH_APPROVAL_FAILED` | Push Service | Service API | 200 (`kind: error`) | yes | Approval card could not be raised | ET |
| `PUSH_APPROVAL_MISSING` | Push Service | Service API | 200 (`kind: error`) | no | Approved send without an approved card | ET |
| `PUSH_AUDIENCE_GREW` | Push Service | internal | — | no | Audience grew before an automatic update | ET |
| `PUSH_BRANCH_MOVED` | Push Service | internal | — | no | Pull request branch moved | ET |
| `PUSH_CONFLUENCE_UNAVAILABLE` | Push Service | internal | — | no | Confluence item on the ordin server, which has no Confluence Adapter | ET |
| `PUSH_COPY_CANNOT_BRING_BACK` | Push Service | Service API | 200 (`kind: error`) | no | Bring back from PDF or slides | ET |
| `PUSH_COPY_MAKER_FAILED` | Push Service | internal | — | no | Copy maker failed | ET |
| `PUSH_COPY_STATE` | Push Service | Service API | 200 (`kind: error`) | no | Copy action not allowed in its state | ET |
| `PUSH_DELETE_UNCHECKED` | Push Service | Service API | 200 (`kind: error`) | no | delete at destination: only after the person's check (04:126) | 2C |
| `PUSH_DESTINATION_REFUSED` | Push Service | internal | — | no | The destination refused the change for a reason not listed above | ET |
| `PUSH_FORBIDDEN` | Push Service | internal | — | no | No permission at the origin | ET |
| `PUSH_FORMAT_NOT_READY` | Push Service | Service API | 200 (`kind: error`) | no | Copy format not ready (C-28) | ET |
| `PUSH_GIT_BRANCH_UNKNOWN` | Push Service | internal | — | no | A git folder item records no branch | ET |
| `PUSH_GIT_IDENTITY_MISSING` | Push Service | internal | — | no | Git author identity missing | ET |
| `PUSH_ITEMS_OUT_OF_RANGE` | Push Service | Service API | 200 (`kind: error`) | no | Request text, items or summary out of range | ET |
| `PUSH_ITEM_ALREADY_PENDING` | Push Service | Service API | 200 (`kind: error`) | no | Item already in an open push | ET |
| `PUSH_ITEM_NOT_CHANGED` | Push Service | Service API | 200 (`kind: error`) | no | Item has no change to send | ET |
| `PUSH_NAME_EXISTS` | Push Service | internal | — | no | A file or page with that name is already at the destination | ET |
| `PUSH_NOT_HELD` | Push Service | Service API | 200 (`kind: error`) | no | Choose a version on a push not held | ET |
| `PUSH_NO_REVIEW_RIGHT` | Push Service | internal | — | no | Cannot open a review in the repository | ET |
| `PUSH_ORIGINAL_UNREADABLE` | Push Service | internal | — | no | The original cannot be read at the version to publish | ET |
| `PUSH_ORIGIN_CHANGED` | Push Service | internal | — | no | Origin changed at send time | ET |
| `PUSH_ORIGIN_NOT_FOUND` | Push Service | internal | — | no | The document no longer exists at its origin | ET |
| `PUSH_ORIGIN_UNREACHABLE` | Push Service | internal | — | yes | Origin unreachable | ET |
| `PUSH_OUTCOME_UNKNOWN` | Push Service | internal | — | no | A send may have arrived but the answer was lost (timeout after sending, or the key was already used) | ET |
| `PUSH_REQUEST_INVALID` | Push Service | Service API | 200 (`kind: error`) | no | Request text, items or summary out of range | ET |
| `PUSH_SIGN_IN_NEEDED` | Push Service | internal | — | yes | Sign-in for the origin expired | ET |
| `PUSH_STORE_FAILED` | Push Service | Service API | 200 (`kind: error`) | yes | Datastore failure, or the program's working-copy store answered a failure | ET |
| `PUSH_SUMMARY_INVALID` | Push Service | Service API | 200 (`kind: error`) | no | Request text, items or summary out of range | ET |
| `PUSH_TEAM_COMMIT_MISSING` | Push Service | internal | — | no | The prepared commit is not in the team knowledge clone (ADR-103) | ET |
| `PUSH_TEAM_SHARE_STATE` | Push Service | internal | — | no | A team-share call not allowed in the share's state (ADR-103) | ET |
| `PUSH_TEAM_SHARE_UNKNOWN` | Push Service | internal | — | no | A team share id this service does not hold (ADR-103) | ET |
| `PUSH_TOO_LARGE` | Push Service | internal | — | no | File too large for the origin | ET |
| `PUSH_UNKNOWN` | Push Service | Service API | 200 (`kind: error`) | no | Unknown push, item, copy or submission | ET |
| `RAG_HTTP_FIELDS_INVALID` | Organisation Profile | Service API | 200 (`kind: error`) | no | A remote agent row breaks a 2C rule | ET |
| `RAG_NAME_INVALID` | Organisation Profile | Service API | 200 (`kind: error`) | no | A remote agent row breaks a 2C rule | ET |
| `RAG_SLACK_FIELDS_INVALID` | Organisation Profile | Service API | 200 (`kind: error`) | no | A remote agent row breaks a 2C rule | ET |
| `RAPI_ADDRESS_NOT_PRIVATE` | Remote API | internal | — | no | Listen address not loopback or private, or port ≤ 1024 | ET |
| `RAPI_BIND_FAILED` | Remote API | internal | — | yes | Port already in use | ET |
| `RAPI_DATASTORE` | Remote API | Remote API | 503 | yes | Datastore failure during pairing or key check | ET |
| `RAPI_LISTENING_FORBIDDEN` | Remote API | internal | — | no | `configure_listening` while the profile forbids listening | ET |
| `RAPI_PORT_INVALID` | Remote API | internal | — | no | `configure_listening` with a port outside 1025–65535 | ET |
| `RAPI_SECRET_STORE` | Remote API | internal | — | yes | `configure_listening` cannot store or delete the key | ET |
| `RAPI_TLS_MATERIAL_INVALID` | Remote API | internal | — | no | Certificate chain or key missing or unreadable | ET |
| `RA_ADAPTER_DUPLICATE` | Runtime Adapter Interface *(also: Session Supervisor)* | Service API | 200 (`kind: error`) | no | Second adapter of one kind registered | ET |
| `RA_ADAPTER_UNKNOWN` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | no | No adapter of the kind in this program; unknown kind on read | ET |
| `RA_ALLOWANCE_EXHAUSTED` | Runtime Adapter Interface *(also: Session Supervisor)* | Service API | 200 (`kind: error`) | no | Plan allowance exhausted (harness limit message) | ET |
| `RA_ATTACHMENT_NOT_GRANTED` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | no | An `AttachmentNotGranted` found by the Session Supervisor | ET |
| `RA_COMMAND_UNKNOWN` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | no | An adapter's own input check fails | ET |
| `RA_DEADLINE` | Runtime Adapter Interface *(also: Session Supervisor)* | Service API | 200 (`kind: error`) | yes | Call deadline passed | ET |
| `RA_EVENT_INVALID` | Runtime Adapter Interface | internal | — | no | A pause reason from the wrong route (`remote_approvers` not from `hosted_agent`, `decision_elsewhere` not from `slack_thread`) | ET |
| `RA_EVENT_ORDER` | Runtime Adapter Interface *(also: Session Supervisor)* | internal | — | no | Event before `started`, after `ended`, a second `started`, or a stream with no `ended` | ET |
| `RA_HANDLE_INVALID` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | no | An adapter's own input check fails | ET |
| `RA_HARNESS_UNTESTED` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | no | Harness version not in the tested list | ET |
| `RA_INPUT_TOO_LARGE` | Runtime Adapter Interface *(also: Session Supervisor)* | Service API | 200 (`kind: error`) | no | An adapter's own input check fails | ET |
| `RA_LIMIT_OUT_OF_RANGE` | Runtime Adapter Interface *(also: Session Supervisor)* | Service API | 200 (`kind: error`) | no | Wall-clock, idle timeout or request budget out of range | ET |
| `RA_MODEL_INVALID` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | no | Model name malformed | ET |
| `RA_MODEL_UNKNOWN` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | no | Model given to a remote adapter | ET |
| `RA_PLAN_INVALID` | Runtime Adapter Interface | internal | — | no | Harness plan outside 3–7 steps or 1–80 characters | ET |
| `RA_RESUME_MISMATCH` | Runtime Adapter Interface *(also: Hosted Agent Adapter)* | Service API | 200 (`kind: error`) | no | An adapter's own input check fails | ET |
| `RA_SIGNED_OUT` | Runtime Adapter Interface *(also: Session Supervisor)* | Service API | 200 (`kind: error`) | no | Harness signed out | ET |
| `RA_SNAPSHOT_INCONSISTENT` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | no | Snapshot end and state disagree | ET |
| `RA_TARGET_MISMATCH` | Runtime Adapter Interface *(also: Hosted Agent Adapter, Session Supervisor, Slack Thread Adapter)* | Service API | 200 (`kind: error`) | no | An adapter's own input check fails | ET |
| `RA_THREAD_BUSY` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | yes | Remote thread busy | ET |
| `RA_UNKNOWN_RUN` | Runtime Adapter Interface | Service API | 200 (`kind: error`) | no | Adapter does not know the run | ET |
| `RA_UNREACHABLE` | Runtime Adapter Interface *(also: Ordin Desktop, Session Supervisor)* | Service API | 200 (`kind: error`) | yes | Route unreachable (network, process failed to start) | ET |
| `RA_VERB_NOT_OFFERED` | Runtime Adapter Interface *(also: Slack Thread Adapter)* | Service API | 200 (`kind: error`) | no | Verb in the adapter's unsupported list | ET |
| `REH_DESCRIPTION_INVALID` | Authoring Service | Service API | 200 (`kind: error`) | no | situation.description: 1–1000 chars (assumed) | 2C |
| `REH_EXPECTATIONS_INVALID` | Authoring Service | Service API | 200 (`kind: error`) | no | expectations: 1–20 per situation (assumed); text 1–500 chars (assumed) | 2C |
| `REH_IMMUTABLE` | Authoring Service | Service API | 200 (`kind: error`) | no | row: written once (04:310) | 2C |
| `REH_SITUATIONS_OUT_OF_RANGE` | Authoring Service | Service API | 200 (`kind: error`) | no | situations: 1–20 (assumed) | 2C |
| `REL_AUTH_FILE` | Agent Loader | internal | — | no | The credentials file cannot be read, has another form, or has no entry for the registry | ET |
| `REL_CONTRACT_RANGE` | Agent Loader | internal | — | no | The release was prepared with a contract version outside the supported range | ET |
| `REL_DEST_NOT_EMPTY` | Agent Loader | internal | — | no | The destination folder holds something other than this release | ET |
| `REL_DIGEST_INVALID` | Agent Loader | internal | — | no | digest: `sha256:` followed by 64 lowercase hex chars | 2C-only |
| `REL_DIGEST_MISMATCH` | Agent Loader | internal | — | no | The manifest or a part does not match its digest or size | ET |
| `REL_EXISTS_DIFFERENT` | Agent Loader | internal | — | no | The release identifier exists in the registry with other content | ET |
| `REL_IMMUTABLE` | Agent Loader | internal | — | no | row: never changed after publish except `state` (04:360) | 2C-only |
| `REL_INCOMPLETE` | Agent Loader | internal | — | no | The release folder lacks a part, or its record or `obtained.json` is malformed | ET |
| `REL_INVALID_INPUT` | Agent Loader | internal | — | no | A record field breaks its rule, or a named file lies outside the configuration's folder or takes its place | ET |
| `REL_NOT_A_RELEASE` | Agent Loader | internal | — | no | The artefact is not an agent release | ET |
| `REL_NOT_FOUND` | Agent Loader | internal | — | no | The registry holds no manifest under the digest, or lacks a part | ET |
| `REL_OUTPUT_EXISTS` | Agent Loader | internal | — | no | The folder a release is to be written into exists and is not empty | ET |
| `REL_REFERENCE_INVALID` | Agent Loader | internal | — | no | `ORDIN_RELEASE` has another form, no digest, or a release identifier that differs from the record | ET |
| `REL_REGISTRY_ERROR` | Agent Loader | internal | — | yes | The registry answers an unexpected status | ET |
| `REL_REGISTRY_REFUSED` | Agent Loader | internal | — | no | The registry refuses the credentials (401, 403) | ET |
| `REL_REGISTRY_UNREACHABLE` | Agent Loader | internal | — | yes | The registry cannot be reached or a request times out | ET |
| `REL_RUNTIME_RANGE` | Agent Loader | internal | — | no | The release was prepared with an ordin version outside the supported range | ET |
| `REL_TOO_LARGE` | Agent Loader | internal | — | no | A part, the manifest, the entry count or the unpacked size is above its limit | ET |
| `REL_UNSAFE_ARCHIVE` | Agent Loader | internal | — | no | An archive holds a link, a device, an absolute path or a `..` segment | ET |
| `REPORT_ACTIVE_CONTENT` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | any text: data only: no markup, script, style, formulas or expressions (04:241) | 2C |
| `REPORT_CHART_TOO_LARGE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | chart: ≤ 8 series, ≤ 5 000 points (A-34) | 2C |
| `REPORT_CURRENCY_MISMATCH` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | currency: present if and only if `column_type = money`; ISO 4217 | 2C |
| `REPORT_DASHBOARD_NAME_DUPLICATE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | Dashboard.name: 1–80 chars (assumed); unique in its workspace (04:242) | 2C |
| `REPORT_FIGURES_OUT_OF_RANGE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | key_figures: 1–8 (A-34; 09:1473) | 2C |
| `REPORT_LINK_OUTSIDE_WORKSPACE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | document_link: points only to a document or place in the workspace (09:1476) | 2C |
| `REPORT_PARTS_OUT_OF_RANGE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | parts: 1–12 (04:240; A-34) | 2C |
| `REPORT_PART_NO_SOURCE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | sources: ≥ 1 per part (04:241) | 2C |
| `REPORT_PIE_TOO_MANY_SLICES` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | chart pie: ≤ 6 slices (09:1472) | 2C |
| `REPORT_TABLE_TOO_LARGE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | table: ≤ 10 000 rows, ≤ 30 columns (A-34) | 2C |
| `REPORT_TITLE_INVALID` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | title, heading: 1–200 chars (assumed) | 2C |
| `REPORT_TOO_LARGE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | whole report: ≤ 2 MB serialised (A-34) | 2C |
| `RES_BUNDLE_CORRUPT` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Bundle manifest, lock file or part files fail their digests | ET |
| `RES_BUNDLE_NOT_FOUND` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | No bundle directory for a plan hash | ET |
| `RES_CLOSURE_TOO_LARGE` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Closure over 1,000 parts | ET |
| `RES_CONFLICT` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Two distinct parts fill one slot | ET |
| `RES_DEPENDENCY_CYCLE` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Reference cycle, or depth over 64 | ET |
| `RES_DIGEST_MISMATCH` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A part's files do not hash to its recorded digest | ET |
| `RES_DRAFT_CONSTITUENT` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A non-staged constituent is draft | ET |
| `RES_LOCK_MISMATCH` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Recomputed plan hash differs from the lock file's | ET |
| `RES_LOCK_PART_UNAVAILABLE` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A lock-file entry is gone, withdrawn or changed in the source | ET |
| `RES_LOOP_MISSING` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Required control loop absent | ET |
| `RES_LOOP_MODE_ESCAPE` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Loop names a mode the root does not allow | ET |
| `RES_LOOP_MULTIPLE` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | More than one control loop | ET |
| `RES_LOOP_NOT_PERMITTED` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Loop outside the root's allow-list | ET |
| `RES_MANIFEST_INVALID` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A manifest field the resolver reads has the wrong shape | ET |
| `RES_MEMORY_CARDINALITY` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | More than one memory contract | ET |
| `RES_NOT_INSTALLABLE` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Root is not a WorkspaceProfile | ET |
| `RES_OVERLAY_CARDINALITY` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Two overlays on an exactly-one axis | ET |
| `RES_OVERLAY_NOT_ALLOWED` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A changed overlay is not in the axis's `available` list | ET |
| `RES_OVERLAY_NOT_APPLICABLE` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | An overlay's `appliesTo` excludes this agent | ET |
| `RES_PACK_NOT_ALLOWED` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | An answered or changed pack is not allowed by the agent | ET |
| `RES_PART_WITHDRAWN` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A resolved part is withdrawn and no replacement was given | ET |
| `RES_REFERENCE_UNRESOLVED` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A reference resolves to nothing | ET |
| `RES_REQUIRED_OVERLAY_MISSING` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A required overlay axis is unsatisfied | ET |
| `RES_REQUIRED_PACK_UNSETTLED` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A required pack rule cannot be satisfied and is not left to ask | ET |
| `RES_ROOT_NOT_APPROVED` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Root is draft (without `allow_draft_root`) or deprecated | ET |
| `RES_ROOT_NOT_FOUND` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Root identifier not in scope | ET |
| `RES_SOURCE_ALREADY_BOUND` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | `install_part_source` called a second time | ET |
| `RES_SOURCE_AMBIGUOUS` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | Same identifier in two repositories at one resolution step | ET |
| `RES_SOURCE_NOT_BOUND` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | A part-reading call before `install_part_source` and without `request.source` | ET |
| `RES_SOURCE_UNAVAILABLE` | Resolver And Compiler | Service API | 200 (`kind: error`) | yes | `PartSource` unavailable, or the withdrawn list unreadable | ET |
| `RES_STORE_WRITE_FAILED` | Resolver And Compiler | Service API | 200 (`kind: error`) | yes | Disk write or rename fails | ET |
| `RES_TOOL_POLICY_INVALID` | Resolver And Compiler | Service API | 200 (`kind: error`) | no | An MCP binding lacks name, server type or a valid transport | ET |
| `REUSE_EXPIRY_INVALID` | Usage Planner | Service API | 200 (`kind: error`) | no | expires_at: = `answered_at` + 7 days (04:188) | 2C |
| `REUSE_INPUT_NEWER` | Usage Planner | Service API | 200 (`kind: error`) | no | reuse: refused when any input has a newer version (04:188) | 2C |
| `REUSE_REQUEST_INVALID` | Usage Planner | Service API | 200 (`kind: error`) | no | Request text empty or over 8,000 characters (G.1) | ET |
| `ROUND_DECISION_INVALID` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | coordinator_decision: 1–4000 chars (assumed) | 2C |
| `ROUND_IMMUTABLE` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | row: immutable once `closed_at` set (04:251) | 2C |
| `ROUND_NUMBER_GAP` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | (crew_run_id, number): primary key; number starts at 1, no gaps | 2C |
| `ROUND_TOO_MANY_RUNNING` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | member_runs: ≤ charter `max_members_running` running at once | 2C-only |
| `RREC_END_KIND_MISMATCH` | Run Queue | internal | — | no | end_kind: present if and only if `state = ended` | 2C |
| `RREC_INTERRUPTED_RESTART` | Run Queue | internal | — | no | state = interrupted: never started again (04:365) | 2C |
| `RREC_LEASE_MISMATCH` | Run Queue | internal | — | no | lease_holder, lease_expires_at: present if and only if `state = executing` | 2C |
| `RREC_MESSAGE_INVALID` | Run Queue | internal | — | no | message text: 1–262,144 chars (the Caller API's limit, ADR-098 item 2; amended January 2026, decision 80: C-28) | 2C |
| `RSTEP_IMMUTABLE` | Journal And Artefact Store *(also: MCP Front)* | Service API | 200 (`kind: error`) | no | row: immutable once its run ends (04:247) | 2C |
| `RSTEP_LINE_TOO_LONG` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | line: 1–80 chars (04:245) | 2C |
| `RSTEP_PLAN_SIZE_OUT_OF_RANGE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | steps per (run_id, plan_version): 3–7 (04:246) | 2C |
| `RSTEP_PLAN_VERSION_GAP` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | plan_version: a changed plan is a new version, starting at 1 (04:246) | 2C |
| `RULE_APPROVAL_UNAVAILABLE` | Rules Engine | internal, Service API | 200 (`kind: error`) | yes / no (per row) | The Approval Service refuses or fails for an ask (D.8) | ET |
| `RULE_ARGUMENTS_TOO_LARGE` | Rules Engine | internal | — | no | Tool name too long, arguments over 1 MiB, malformed origin (D.2) | ET |
| `RULE_BAD_PATTERN` | Rules Engine | Service API | 200 (`kind: error`) | no | A pattern is empty, over 200 characters, or refused by `regex` (A.2) | ET |
| `RULE_CHECKER_UNREACHABLE` | Rules Engine | internal | — | no | The content checker times out, refuses, answers outside 200–299 or with another shape, or no checker is attached; logged, and the run ends with the safe reply unless `on-unreachable: allow` (L.3, L.5, L.6; ADR-107) | ET |
| `RULE_COMMAND_INVALID` | Rules Engine | internal | — | no | A command request with empty or over-long text, or a relative working folder (K.1.1) | ET |
| `RULE_COMMAND_NOT_DECLARED` | Rules Engine | internal | — | no | A command from a person that is not a declared check (K.1.3) | ET |
| `RULE_FACT_UNAVAILABLE` | Rules Engine | Service API, internal | 200 (`kind: error`) | yes / no (per row) | A fact cannot be read at the start (B.10) | ET |
| `RULE_FAIL_CLOSED_NOT_DENY` | Rules Engine | Service API | 200 (`kind: error`) | no | `rule` with `fail_closed`: verdict is `deny` | 2C-only |
| `RULE_GATE_OVERRIDE` | Rules Engine | Service API | 200 (`kind: error`) | no | A configured control takes a package control's name (A.3) | ET |
| `RULE_ID_INVALID` | Rules Engine | Service API | 200 (`kind: error`) | no | `rule.rule_id`: 1–128 characters (assumed) | 2C-only |
| `RULE_JOURNAL_UNAVAILABLE` | Rules Engine | internal | — | no | The ToolRequest cannot be journalled (D.1) | ET |
| `RULE_NOT_AN_ADDRESS` | Rules Engine | Service API | 200 (`kind: error`) | no | A text is not an address (I) | ET |
| `RULE_ORIGIN_INVALID` | Rules Engine | internal | — | no | Tool name too long, arguments over 1 MiB, malformed origin (D.2) | ET |
| `RULE_PATH_NOT_ALLOWED` | Rules Engine | internal | — | no | A path checked outside a call is protected or outside the grants (K.5) | ET |
| `RULE_POLICY_INVALID` | Rules Engine | Service API | 200 (`kind: error`) | no | An agent policy entry is malformed (A.4) | ET |
| `RULE_POLICY_WIDENS` | Rules Engine *(also: Resolver And Compiler)* | Service API | 200 (`kind: error`) | no | combination: an agent's own tool policy can only narrow (ADR-090): it never turns a deny into allow or ask | 2C |
| `RULE_REASON_INVALID` | Rules Engine | Service API | 200 (`kind: error`) | no | `Deny.reason`: 1–500 characters (assumed); plain words, shown to the model and in Diagnostics | 2C-only |
| `RULE_RULESET_NOT_LOADED` | Rules Engine | MCP tool | — | no | A decision for an instance with no loaded set (B.1, D.4) | ET |
| `RULE_RUN_UNKNOWN` | Rules Engine | internal, Service API | 200 (`kind: error`) | no | The call's run is unknown or belongs to another instance (D.4) | ET |
| `RULE_SESSION_MISMATCH` | Rules Engine | internal | — | no | The call's run is unknown or belongs to another instance (D.4) | ET |
| `RULE_SETTING_RULE_NOT_ALLOWED` | Rules Engine | Service API | 200 (`kind: error`) | no | A rule kept as a setting is not allowed (J) | ET |
| `RULE_STATE_UNAVAILABLE` | Rules Engine *(also: Caller API)* | Caller API, Service API | 503 (Caller API); 200 `kind: error` (Remote API) | yes | A control-state statement fails (G.7, D.11) | ET |
| `RULE_TOOL_NAME_INVALID` | Rules Engine | internal | — | no | Tool name too long, arguments over 1 MiB, malformed origin (D.2) | ET |
| `RULE_TOO_MANY_RULES` | Rules Engine | Service API | 200 (`kind: error`) | no | More than 200 argument rules (A.2) | ET |
| `RULE_UNBOUND_TOOL` | Rules Engine | Service API | 200 (`kind: error`) | no | A control names a tool or sub-agent that is not bound (A.2) | ET |
| `RULE_UNKNOWN_ARGUMENT` | Rules Engine | Service API | 200 (`kind: error`) | no | An argument rule names a place the tool does not describe (A.2) | ET |
| `RUNST_ABANDONED_NOT_REMOTE` | Session Supervisor | Service API | 200 (`kind: error`) | no | state = abandoned: only with a remote adapter (04:178) | 2C |
| `RUNST_ADAPTER_MISMATCH` | Session Supervisor | Service API | 200 (`kind: error`) | no | adapter: `claude_local`/`copilot_local` only with `agent.kind = agent_instance`; `hosted_agent`/`slack_thread` only with `remote_agent` | 2C |
| `RUNST_CACHE_EXCEEDS_TOKENS` | Session Supervisor | Service API | 200 (`kind: error`) | no | tokens_from_cache: ≤ tokens | 2C |
| `RUNST_ENDED_AT_MISMATCH` | Session Supervisor | Service API | 200 (`kind: error`) | no | ended_at: present if and only if the state is an ended state | 2C |
| `RUNST_END_REASON_TOO_LONG` | Session Supervisor | Service API | 200 (`kind: error`) | no | end_reason: ≤ 500 chars (assumed) | 2C |
| `RUNST_INPUTS_TOO_MANY` | Session Supervisor | Service API | 200 (`kind: error`) | no | inputs_used: ≤ 100 (assumed) | 2C |
| `RUNST_LIMIT_OUT_OF_RANGE` | Session Supervisor | Service API | 200 (`kind: error`) | no | limits: each ≥ 1 (`request_budget` when present) and within the profile's maxima | 2C |
| `RUNST_NO_REPLY_NOT_SLACK` | Session Supervisor | Service API | 200 (`kind: error`) | no | state = no_reply: only with `adapter = slack_thread` (04:178) | 2C |
| `RUNST_PAUSE_REASON_MISMATCH` | Session Supervisor | Service API | 200 (`kind: error`) | no | pause_reason: present if and only if `state = paused` | 2C |
| `RUN_BAD_INPUT` | Run Queue *(also: Duty Timer)* | internal | — | no | Input breaks a rule of step 1 or step 6, `pause` with no pending item, or `end` with `Control` and `ended_by_control` false (step 12) | ET |
| `RUN_DUPLICATE_ID` | Run Queue | internal | — | no | `enqueue` with a `run_id` a run already has | ET |
| `RUN_LEASE_LOST` | Run Queue | internal | — | no | `end`, `pause`, `record_cancelled`, `append_progress` by an instance that no longer holds the lease | ET |
| `RUN_LIMIT` | Run Queue (wrightrun carried) *(also: Session Supervisor, emits it in `run_summary` only)* | log field only | — | no | The run ended at a limit; written by the Session Supervisor as the `error_code` value of the `run_summary` log event (SessionSupervisor.md Internal Logic), never returned to a caller | IL |
| `RUN_NOT_CANCELLABLE` | Run Queue *(also: Caller API)* | Caller API, internal | 409 | no | `cancel` of a run that has ended or does not exist | ET |
| `RUN_NOT_PAUSED` | Run Queue *(also: Caller API)* | Caller API, internal | 409 | no | `resume` of a run that is not paused or does not exist | ET |
| `RUN_SESSION_RECORD_TOO_LARGE` | Run Queue | internal | — | no | `pause` with a session record over 64 MiB | ET |
| `RUN_THREAD_BUSY` | Run Queue *(also: Caller API)* | Caller API, internal | 409 | no | `enqueue` on a thread with a queued, executing or paused run | ET |
| `RUN_THREAD_NOT_FOUND` | Run Queue | internal | — | no | `enqueue` on a `thread_id` no thread has | ET |
| `SACC_CHECK_FORBIDDEN` | Source Access | internal | — | no | The agent's own credential lacks the permission to check, or can no longer read the repository (ADR-115) | ET |
| `SACC_CHECK_TIMEOUT` | Source Access | internal | — | yes | A check did not finish by its deadline; a notice took over 10 s (ADR-115) | ET |
| `SACC_CHECK_UNAVAILABLE` | Source Access | internal | — | yes | The source system or the datastore could not be reached after retries (ADR-115) | ET |
| `SACC_DELEGATION_REFUSED` | Source Access | internal | — | no | The on-behalf-of exchange was refused or the delegation expired (ADR-115) | ET |
| `SACC_LINK_REFUSED` | Source Access | internal | — | no | The system did not accept the token presented for linking, or named no usable account (ADR-115) | ET |
| `SACC_NOTICE_SIGNATURE_INVALID` | Source Access | internal | — | no | A webhook's signature or token is missing or wrong, or its body is over 1 MiB or not JSON (ADR-115) | ET |
| `SACC_NOT_LINKED` | Source Access | internal | — | no | A check found no linked account for that person in that system; never on a route (unlinking is idempotent) (ADR-115) | ET |
| `SACC_RATE_LIMITED` | Source Access | internal | — | yes | The source system asked ordin to slow down (ADR-115) | ET |
| `SACC_READ_FAILED` | Source Access | internal | — | no | A Confluence page or SharePoint file could not be read, or the agent's SharePoint refresh token was refused (ADR-115) | ET |
| `SACC_SYSTEM_NOT_CONFIGURED` | Source Access | internal | — | no | The agent has no per-person check for that system, or its settings are inconsistent at start-up (ADR-115) | ET |
| `SBX_ARGS_INVALID` | Sandbox | MCP tool | — | no | Arguments malformed, empty or oversize `code`, too many entries, reserved `ordin_tools` requested | ET |
| `SBX_AUDIT_KEY_UNAVAILABLE` | Sandbox | MCP tool | — | yes | Start-up: audit key unavailable | ET |
| `SBX_BUSY` | Sandbox | MCP tool | — | yes | No guest slot within 30 s | ET |
| `SBX_CALLER_NOT_SUPPORTED` | Sandbox | MCP tool | — | no | Hosted-agent caller | ET |
| `SBX_CAPABILITY_UNKNOWN` | Sandbox | MCP tool | — | no | Requested capability not granted | ET |
| `SBX_CONFIG_INVALID` | Sandbox | Service API | 200 (`kind: error`) | yes | Start-up: internal configuration refused | ET |
| `SBX_CREDENTIAL_UNAVAILABLE` | Sandbox | MCP tool | — | yes | Keychain or secret store unavailable | ET |
| `SBX_ENVIRONMENT_INVALID` | Sandbox | MCP tool | — | no | Environment variable name reserved or invalid, or value looks like a secret | ET |
| `SBX_ENVIRONMENT_UNSUPPORTED` | Sandbox | MCP tool | — | no | `environment` given without the extra guests | ET |
| `SBX_FILE_INVALID` | Sandbox | MCP tool | — | no | Bad, duplicate or undecodable `files` path (`VAL_ERR` from `WorkDir`) | ET |
| `SBX_GUEST_DIGEST_MISMATCH` | Sandbox | Service API | 200 (`kind: error`) | yes | Start-up: guest digest mismatch or guest invalid | ET |
| `SBX_GUEST_START_FAILED` | Sandbox | MCP tool | — | no | Guest could not start (`exit_code -3`) | ET |
| `SBX_INPUTS_TOO_LARGE` | Sandbox | MCP tool | — | no | Inputs over 1,000 files, 64 MiB, or one file over 16 MiB | ET |
| `SBX_INPUT_NOT_FOUND` | Sandbox | MCP tool | — | no | Input file not found | ET |
| `SBX_INPUT_NOT_GRANTED` | Sandbox | MCP tool | — | no | Input place not granted | ET |
| `SBX_INPUT_UNREADABLE` | Sandbox | MCP tool | — | yes | Input file unreadable | ET |
| `SBX_INTERACTION_REFUSED` | Sandbox | MCP tool | — | no | Controller refused an interaction for another reason | ET |
| `SBX_INTERNAL` | Sandbox | MCP tool | — | yes | Host fault preparing the folder (`WorkDirError::Io`) | ET |
| `SBX_KEY_MISSING` | Sandbox | MCP tool | — | no | Agent key not entered | ET |
| `SBX_MIXED_CREDENTIALS` | Sandbox | MCP tool | — | no | Two person credentials in one run | ET |
| `SBX_MODULE_HANDLE_INVALID` | Sandbox | Service API | 200 (`kind: error`) | no | Module call with an unknown or expired capability reference (inside the room) | ET |
| `SBX_OUTPUT_SAVE_FAILED` | Sandbox | MCP tool | — | yes | Output artefact not saved | ET |
| `SBX_POLICY_INVALID` | Sandbox | MCP tool | — | no | Manifest rendered from grants refused by `PolicyEngine::load` | ET |
| `SBX_RULES_UNAVAILABLE` | Sandbox | MCP tool | — | yes | Rules Engine grants unreadable | ET |
| `SBX_RUN_BUDGET_EXHAUSTED` | Sandbox | MCP tool | — | no | Per-run broker or module calls over 50, or sandbox runs over 500 in the session | ET |
| `SBX_RUN_TIMEOUT` | Sandbox | MCP tool | — | no | Run did not return within 35 s | ET |
| `SBX_SESSIONS_FULL` | Sandbox | MCP tool | — | yes | 16 work sessions open | ET |
| `SBX_SESSION_BUSY` | Sandbox | MCP tool | — | yes | Work session already running a call | ET |
| `SBX_SIGN_IN_NEEDED` | Sandbox | MCP tool | — | no | Person's sign-in missing or expired | ET |
| `SBX_TOOL_UNKNOWN` | Sandbox | MCP tool | — | no | Tool name not offered | ET |
| `SCA_BAD_RESPONSE` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Answer missing a field read | ET |
| `SCA_CALL_FAILED` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Other Slack error word | ET |
| `SCA_CODE_EXPIRED` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Pairing code wrong / expired / fifth wrong | ET |
| `SCA_CODE_WRONG` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Pairing code wrong / expired / fifth wrong | ET |
| `SCA_DATASTORE_FAILED` | Slack Channel Adapter | Service API | 200 (`kind: error`) | yes | Code table read or write fails | ET |
| `SCA_FILE_TOO_LARGE` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | File over 100 MB or over the caller's limit | ET |
| `SCA_FORM_TOO_LARGE` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Form over Slack's limits | ET |
| `SCA_LINK_LOST` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | WebSocket closed, silent 60 s or proxy `CONNECT` refused | ET |
| `SCA_MESSAGE_INVALID` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Message or blocks refused | ET |
| `SCA_MISSING_SCOPE` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Scope missing from the app | ET |
| `SCA_NOT_ALLOWED` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Pairing outside an allowed channel | ET |
| `SCA_NOT_FOUND` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Channel, message or file not found / not in channel | ET |
| `SCA_PAIRING_STOPPED` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Pairing code wrong / expired / fifth wrong | ET |
| `SCA_RATE_LIMITED` | Slack Channel Adapter | Service API | 200 (`kind: error`) | yes | Rate limited after 2 waits | ET |
| `SCA_RESPONSE_URL_INVALID` | Slack Channel Adapter | internal | — | no | `replace_ephemeral` with an address that is not `https://hooks.slack.com/…`; nothing sent (ADR-107) | ET |
| `SCA_TOKEN_INVALID` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | Token not of the right form at setup | ET |
| `SCA_TOKEN_MISSING` | Slack Channel Adapter | Service API | 200 (`kind: error`) | no | A token missing at start | ET |
| `SCA_TOKEN_REFUSED` | Slack Channel Adapter | Service API | 200 (`kind: error`) | yes | Token refused (`invalid_auth`, `token_revoked`) or Socket Mode disabled (`link_disabled`) | ET |
| `SCA_UNREACHABLE` | Slack Channel Adapter | Service API | 200 (`kind: error`) | yes | Transport error, timeout, 5xx | ET |
| `SCOPE_AGENT_WIDE_DUPLICATE` | Retention And Erasure | internal | — | no | kind = agent_wide: exactly one per agent | 2C |
| `SCOPE_ERASED_SILENT` | Retention And Erasure | internal | — | no | state = erased: silent until removed and added again (04:370) | 2C |
| `SCOPE_IDENTIFIER_MISMATCH` | Retention And Erasure | internal | — | no | `enable` with kind agent-wide, or a chat scope without identifier | ET |
| `SCOPE_KNOWLEDGE_DUPLICATE` | Retention And Erasure | internal | — | no | kind = knowledge: exactly one per agent, only when the configuration has a `knowledge-agent` section (ADR-113) | 2C |
| `SERVER_ALREADY_RUNNING` | Ordin Server | internal | — | no | Lock held at `start` | ET |
| `SERVER_CHANNEL_NOT_ALLOWED` | Ordin Server | internal | — | no | `pair-chat` for a channel the profile does not allow | ET |
| `SERVER_CHANNEL_TOKEN_MISSING` | Ordin Server | internal | — | no | `pair-chat` before the channel's tokens | ET |
| `SERVER_CONTROL_REFUSED` | Ordin Server | internal | — | no | Control socket peer of another user | ET |
| `SERVER_DATA_ROOT_UNWRITABLE` | Ordin Server | internal | — | no | Data dir not owned by the user or not writable | ET |
| `SERVER_ENV_INVALID` | Ordin Server | internal | — | no | A variable of ordin's own is malformed, or `ORDIN_RELEASE_DIR` missing for `agent start` | ET |
| `SERVER_HEALTH_TIMEOUT` | Ordin Server | internal | — | yes | `health` gets no answer in 5 s | ET |
| `SERVER_HISTORY_DAYS_INVALID` | Ordin Server | internal | — | no | `history` days out of range | ET |
| `SERVER_KNOWLEDGE_NOT_CONFIGURED` | Ordin Server | command line | — | no | A knowledge command ran against a configuration without the section or key it needs (ADR-115) | ET |
| `SERVER_KNOWLEDGE_SCOPE_INVALID` | Ordin Server | internal | — | no | The `knowledge_scope_id` setting read back is not a UUID; start stops with `SERVER_START_FAILED` (ADR-113) | ET |
| `SERVER_LISTENING_FORBIDDEN` | Ordin Server | internal | — | no | The profile forbids listening | ET |
| `SERVER_LISTENING_NOT_SET` | Ordin Server | internal | — | no | `pair-desktop` before `listen` | ET |
| `SERVER_LISTEN_ADDRESS_REFUSED` | Ordin Server | internal | — | no | `listen` with a public address | ET |
| `SERVER_NOTHING_TO_SERVE` | Ordin Server | internal | — | no | `chat` role and no chat connection in the configuration | ET |
| `SERVER_NOT_RUNNING` | Ordin Server | internal | — | yes | `health` finds no server | ET |
| `SERVER_NOT_SET_UP` | Ordin Server | internal | — | no | No server profile | ET |
| `SERVER_PAIRING_NAME_AMBIGUOUS` | Ordin Server | internal | — | no | `pair-desktop --end NAME` matches more than one paired desktop | ET |
| `SERVER_PAIRING_REFUSED_AGENT` | Ordin Server | internal | — | no | `pair-desktop` on an agent profile | ET |
| `SERVER_PORT_INVALID` | Ordin Server | internal | — | no | `listen` port outside 1025–65535 | ET |
| `SERVER_PROFILE_IMMUTABLE` | Ordin Server | internal | — | no | A second profile choice | ET |
| `SERVER_PROFILE_MISMATCH` | Ordin Server | internal | — | no | `start` on an agent profile, or `agent start` on a personal one | ET |
| `SERVER_PROFILE_NOT_USED` | Ordin Server | internal | — | no | `profile` on an agent profile | ET |
| `SERVER_PROFILE_URL_INVALID` | Ordin Server | internal | — | no | `profile` URL not `https` or too long | ET |
| `SERVER_RELEASE_REFERENCE_INVALID` | Ordin Server | internal | — | no | `release` reference malformed | ET |
| `SERVER_ROLE_COMBINATION` | Ordin Server | internal | — | no | Roles repeated, overlapping, or absent in the scaled shape | ET |
| `SERVER_SECRET_EMPTY` | Ordin Server | internal | — | no | `secret` with an empty value | ET |
| `SERVER_SECRET_NAME_UNKNOWN` | Ordin Server | internal | — | no | `secret` with an unknown name | ET |
| `SERVER_SECRET_NOT_STORABLE` | Ordin Server | command line | — | no | `knowledge sign-in sharepoint` in the scaled shape without `--print` (ADR-115) | ET |
| `SERVER_SETTINGS_INVALID` | Ordin Server | internal | — | no | `server_settings.agent_instance_id` holds a value that is not a UUID | ET |
| `SERVER_SETUP_NEEDS_TERMINAL` | Ordin Server | internal | — | no | Guided `setup` without a terminal | ET |
| `SERVER_SHUTDOWN_STEP_TIMEOUT` | Ordin Server | internal | — | no | A stop step overruns its bound | ET |
| `SERVER_SIGN_IN_REFUSED` | Ordin Server | command line | — | no | Microsoft refused the agent's device sign-in or returned no refresh token (ADR-115) | ET |
| `SERVER_SIGN_IN_TIMED_OUT` | Ordin Server | command line | — | no | The agent's device sign-in was not completed within 15 minutes (ADR-115) | ET |
| `SERVER_START_FAILED` | Ordin Server | internal | — | no | A module fails to construct or start | ET |
| `SERVER_WATCHDOG_SEND_FAILED` | Ordin Server | internal | — | no | A watchdog datagram cannot be sent | ET |
| `SETUP_AGENT_FOLDER_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | no | Agent folder could not be re-installed | ET |
| `SETUP_CATALOGUE_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Agent repository clone or fetch failed | ET |
| `SETUP_CRAFT_APP_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | ArtCraft app missing after repair | ET |
| `SETUP_HARNESS_CHECK_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Harness output unreadable / command timed out | ET |
| `SETUP_HARNESS_MISSING` | Setup Assistant | Service API | 200 (`kind: error`) | no | No listed harness found | ET |
| `SETUP_HARNESS_UNKNOWN` | Setup Assistant | Service API | 200 (`kind: error`) | no | No probe registered for the harness | ET |
| `SETUP_HARNESS_UNTESTED` | Setup Assistant | Service API | 200 (`kind: error`) | no | Harness version not in `tested_versions` | ET |
| `SETUP_HELP_ADDRESS_INVALID` | Setup Assistant | Service API | 200 (`kind: error`) | no | Entered address invalid | ET |
| `SETUP_HELP_DESTINATION_INVALID` | Setup Assistant | Service API | 200 (`kind: error`) | no | Destination not a valid `https` URL | ET |
| `SETUP_HELP_DESTINATION_MISSING` | Setup Assistant | Service API | 200 (`kind: error`) | no | No destination and no address | ET |
| `SETUP_HELP_INPUT_INVALID` | Setup Assistant | Service API | 200 (`kind: error`) | no | Help input too long or empty | ET |
| `SETUP_HELP_MAIL_APP_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Mail app could not be opened | ET |
| `SETUP_HELP_PREVIEW_UNKNOWN` | Setup Assistant | Service API | 200 (`kind: error`) | no | Preview unknown or expired | ET |
| `SETUP_HELP_REFUSED` | Setup Assistant | Service API | 200 (`kind: error`) | no | Ticket URL answered 4xx | ET |
| `SETUP_HELP_SEND_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Delivery timed out, 5xx, Slack `ok:false` | ET |
| `SETUP_LANGUAGE_SERVER_DIGEST_MISMATCH` | Setup Assistant | Service API | 200 (`kind: error`) | no | Downloaded language server's SHA-256 differs from the pinned one | ET |
| `SETUP_LANGUAGE_SERVER_INSTALL_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Language server install failed (download, unpack, missing binary) | ET |
| `SETUP_LANGUAGE_SERVER_NOT_APPROVED` | Setup Assistant | Service API | 200 (`kind: error`) | no | Language server not approved by the profile | ET |
| `SETUP_LANGUAGE_SERVER_PLATFORM_UNSUPPORTED` | Setup Assistant | Service API | 200 (`kind: error`) | no | No download for this computer's platform | ET |
| `SETUP_NO_HARNESS_ACCESS` | Setup Assistant | Service API | 200 (`kind: error`) | no | Harness reports no access / no seat | ET |
| `SETUP_NO_PROFILE_ADDRESS` | Setup Assistant | Service API | 200 (`kind: error`) | no | Installer carried no profile address | ET |
| `SETUP_NO_READY_HARNESS` | Setup Assistant | Service API | 200 (`kind: error`) | no | No ready harness for a session | ET |
| `SETUP_PROFILE_UNREACHABLE` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Profile cannot be fetched or verified | ET |
| `SETUP_PROGRAM_DAMAGED` | Setup Assistant | Service API | 200 (`kind: error`) | no | Bundled program missing or damaged, restore failed | ET |
| `SETUP_PROGRAM_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Program exited non-zero or timed out | ET |
| `SETUP_PROGRAM_REFUSED` | Setup Assistant | Service API | 200 (`kind: error`) | no | Program refused by the operating system | ET |
| `SETUP_SELF_UPDATE_UNAVAILABLE` | Setup Assistant *(also: Ordin Server, agent profile, through `SELF_UPDATE_UNAVAILABLE`)* | Service API | 200 (`kind: error`) | no | This backend cannot replace itself: the desktop's own core for the two server commands, the agent profile, a container, or a server not run by its user unit (ADR-114) | ET |
| `SETUP_SIGN_IN_ON_SERVER` | Setup Assistant | Service API | 200 (`kind: error`) | no | Harness sign-in asked on the server | ET |
| `SETUP_SIGN_IN_TIMEOUT` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Harness sign-in not finished in 10 min | ET |
| `SETUP_STORE_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Database write failed | ET |
| `SETUP_STORE_UNSUPPORTED` | Setup Assistant | Service API | 200 (`kind: error`) | no | `new` given a PostgreSQL datastore | ET |
| `SETUP_UPDATE_FEED_INVALID` | Setup Assistant | Service API | 200 (`kind: error`) | no | Server feed has no readable `version`; *amended January 2026 (ADR-114):* also a feed or download address that is not `https`, a signed archive lacking `bin/ordin-server` or `bin/ordin-hook` or whose program reports another version, and an archive entry that is not a regular file | ET |
| `SETUP_UPDATE_INSTALL_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Installing the update failed | ET |
| `SETUP_UPDATE_IN_PROGRESS` | Setup Assistant | Service API | 200 (`kind: error`) | no | A server update is already under way (ADR-114) | ET |
| `SETUP_UPDATE_NONE` | Setup Assistant | Service API | 200 (`kind: error`) | no | `install_update` with no offer; *amended January 2026 (ADR-114):* also `update_server` when the feed offers no newer version | ET |
| `SETUP_UPDATE_NO_ARCHIVE` | Setup Assistant | Service API | 200 (`kind: error`) | no | Feed has no archive for this server's architecture (ADR-114) | ET |
| `SETUP_UPDATE_REPLACE_FAILED` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Program files could not be replaced, or the unit's restart was refused and the programs were restored from `.previous`; the running version is kept (ADR-114) | ET |
| `SETUP_UPDATE_SIGNATURE_INVALID` | Setup Assistant | Service API | 200 (`kind: error`) | no | Update signature refused by the build's update key | ET |
| `SETUP_UPDATE_UNREACHABLE` | Setup Assistant | Service API | 200 (`kind: error`) | yes | Update feed unreachable | ET |
| `SETUP_UPDATE_VERSION_CHANGED` | Setup Assistant | Service API | 200 (`kind: error`) | no | `setup.update_server`'s version is not the one the feed now offers (ADR-114) | ET |
| `SET_INVALID` | Settings Store *(also: Caller API)* | Caller API, internal | 400 | no | The value breaks the declaration (type, limit, unknown option, duplicate entry, refused rule, line that does not fit the form, faulty line form,… | ET |
| `SET_NOT_ALLOWED` | Settings Store *(also: Caller API)* | Caller API, internal | 403 | no | An `operators-only` setting written by a member | ET |
| `SET_UNAVAILABLE` | Settings Store *(also: Caller API, Retention And Erasure)* | Caller API, internal | 503 | yes | A statement fails, no connection can be had, or the selection lookup fails | ET |
| `SET_UNKNOWN` | Settings Store *(also: Caller API)* | Caller API, internal | 404 | no | The name is not a declared setting | ET |
| `SET_VERSION_CONFLICT` | Settings Store *(also: Caller API)* | Caller API, internal | 409 | no | A list edit against an older version | ET |
| `SLK_APPROVAL_UNAVAILABLE` | Slack Service | Service API | 200 (`kind: error`) | yes | Approval Service fails | ET |
| `SLK_APP_NOT_APPROVED` | Slack Service | Service API | 200 (`kind: error`) | no | Slack refused the app's install (app approval on) | ET |
| `SLK_ARGS_INVALID` | Slack Service | MCP tool | — | no | Arguments malformed or unknown field | ET |
| `SLK_CHANNEL_ID_INVALID` | Slack Service | internal | — | no | Channel id malformed | ET |
| `SLK_CHANNEL_NOT_ALLOWED` | Slack Service | Service API | 200 (`kind: error`) | no | Channel not in 'slack.allowed_channels' | ET |
| `SLK_CHANNEL_NOT_LINKED` | Slack Service | internal | — | no | Read guard: a channel that is not linked, or a direct message | ET |
| `SLK_CHANNEL_NOT_OFFERED` | Slack Service | internal | — | no | 'link' for a channel the person is not in, or archived | ET |
| `SLK_CHANNEL_UNREACHABLE` | Slack Service | Service API | 200 (`kind: error`) | no | Channel unreachable ('not_in_channel', 'channel_not_found', 'is_archived') | ET |
| `SLK_CLIENT_ID_INVALID` | Slack Service | Service API | 200 (`kind: error`) | no | Pasted client id malformed | ET |
| `SLK_DRAFT_ALREADY_ASKED` | Slack Service | Service API | 200 (`kind: error`) | no | Draft already has an open card or post | ET |
| `SLK_DRAFT_ALREADY_POSTED` | Slack Service | Service API | 200 (`kind: error`) | no | Draft already posted | ET |
| `SLK_DRAFT_NOT_FOUND` | Slack Service | Service API | 200 (`kind: error`) | no | Draft not found in the workspace | ET |
| `SLK_DRAFT_TEXT_INVALID` | Slack Service | MCP tool | — | no | Draft text with the foot line not 1–4,000 chars | ET |
| `SLK_KEYCHAIN_UNAVAILABLE` | Slack Service | Service API | 200 (`kind: error`) | yes | The Slack sign-in cannot be read from the keychain now | ET |
| `SLK_LINK_DUPLICATE` | Slack Service | internal | — | no | 'link' for an already linked channel | ET |
| `SLK_LINK_INVALID` | Slack Service | internal | — | no | Link fields malformed (team id, channel id, name, ts) | ET |
| `SLK_LINK_NOT_FOUND` | Slack Service | Service API | 200 (`kind: error`) | no | 'link_id' not a link of the session's workspace | ET |
| `SLK_MEMBER_NOT_FOUND` | Slack Service | Service API | 200 (`kind: error`) | no | Receiver's address matches no Slack member | ET |
| `SLK_MESSAGE_DUPLICATE` | Slack Service | internal | — | no | Duplicate kept message | ET |
| `SLK_MESSAGE_NOT_FOUND` | Slack Service | Service API | 200 (`kind: error`) | no | Message or thread not kept for the link | ET |
| `SLK_NAME_INVALID` | Slack Service | internal | — | no | Channel or rule name not 1–80 chars | ET |
| `SLK_NOT_GRANTED` | Slack Service | MCP tool | — | no | Caller lacks the grant the tool needs | ET |
| `SLK_NO_AUTHORITY` | Slack Service | Service API | 200 (`kind: error`) | no | 'send' without an approved card or a matching rule; a post in 'waiting', 'posting' or 'posted' without an authority | ET |
| `SLK_NO_SLACK_APP` | Slack Service | Service API | 200 (`kind: error`) | no | No Slack app: profile names none and the person connected none | ET |
| `SLK_OWN_APP_NOT_NEEDED` | Slack Service | internal | — | no | Own app connected while the profile names one | ET |
| `SLK_POST_FIELDS_MISMATCH` | Slack Service | internal | — | no | A post record whose 'ts', 'permalink', 'action_key' do not match its state, or a draft whose 'slack_post_id' does not | ET |
| `SLK_POST_NOT_FOUND` | Slack Service | internal | — | no | Post id unknown | ET |
| `SLK_POST_REFUSED` | Slack Service | Service API | 200 (`kind: error`) | no | Slack refused the post for another reason | ET |
| `SLK_POST_STATE_INVALID` | Slack Service | internal | — | no | Post not in a state that allows the call | ET |
| `SLK_POST_UNKNOWN_OUTCOME` | Slack Service | Service API | 200 (`kind: error`) | no | No answer after the request left, or a crash during a post | ET |
| `SLK_PROGRAM_UNSUPPORTED` | Slack Service | internal | — | no | Started in an agent-profile program | ET |
| `SLK_QUERY_INVALID` | Slack Service | MCP tool | — | no | Search query empty or over 200 chars | ET |
| `SLK_RATE_LIMITED` | Slack Service | Service API | 200 (`kind: error`) | yes | Rate limited twice, or 'Retry-After' over 60 s | ET |
| `SLK_REACTOR_FAILED` | Slack Service | internal | — | no | The reactor refused or failed | ET |
| `SLK_RULES_OFF` | Slack Service | Service API | 200 (`kind: error`) | no | 'slack.rules_allowed = false' | ET |
| `SLK_RULE_CAP_INVALID` | Slack Service | internal | — | no | Rule daily cap outside 1–20 | ET |
| `SLK_RULE_CAP_REACHED` | Slack Service | Service API | 200 (`kind: error`) | no | Rule's daily cap reached | ET |
| `SLK_RULE_DUPLICATE` | Slack Service | internal | — | no | Same rule already on | ET |
| `SLK_RULE_NOT_FOUND` | Slack Service | internal | — | no | Rule id unknown | ET |
| `SLK_RULE_SCOPE_MISMATCH` | Slack Service | internal | — | no | Rule kind and scope do not match, or the link's channel differs | ET |
| `SLK_SCOPE_NEEDED` | Slack Service | Service API | 200 (`kind: error`) | no | Scope not granted ('CRED_SCOPE_MISSING', 'missing_scope') | ET |
| `SLK_SERVER_SLACK_OFF` | Slack Service | Service API | 200 (`kind: error`) | no | 'slack_post' in 'server.external_actions_off' | ET |
| `SLK_SIGN_IN_NEEDED` | Slack Service | Service API | 200 (`kind: error`) | no | No Slack sign-in, sign-in ended, token refused | ET |
| `SLK_SIGN_IN_NOT_OFFERED` | Slack Service | Service API | 200 (`kind: error`) | no | Personal server without the relayed sign-in | ET |
| `SLK_STORE_UNAVAILABLE` | Slack Service | Service API | 200 (`kind: error`) | yes | Datastore read or write fails | ET |
| `SLK_TEAM_MISMATCH` | Slack Service | internal | — | no | A link from another Slack workspace than the one signed in | ET |
| `SLK_TEXT_INVALID` | Slack Service | internal | — | no | Post text not 1–4,000 chars, contains '<!', or Slack answers 'msg_too_long' | ET |
| `SLK_TEXT_TOO_LONG` | Slack Service | internal | — | no | Kept text over 40,000 chars | ET |
| `SLK_TOKEN_REFUSED` | Slack Service | Service API | 200 (`kind: error`) | no | Token refused by Slack (recorded on the credential) | ET |
| `SLK_TOOLS_OFF` | Slack Service | MCP tool | — | no | 'slack.tools_enabled = false' (tools, links, drafts, reactions) | ET |
| `SLK_UNREACHABLE` | Slack Service | Service API | 200 (`kind: error`) | yes | Slack unreachable before the request left | ET |
| `SLK_WORKSPACE_NOT_FOUND` | Slack Service | internal | — | no | 'link' for a workspace that does not exist | ET |
| `SPROF_IMMUTABLE` | Ordin Server | Service API | 200 (`kind: error`) | no | row: written once; any update refused (04:350) | 2C |
| `SPROF_SHAPE_MISMATCH` | Ordin Server | Service API | 200 (`kind: error`) | no | shape: present if and only if `profile = agent` (04:348) | 2C |
| `SP_ACCESS_DENIED` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | Graph 403 | ET |
| `SP_ACTION_KEY_USED` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | The caller's external-action key was already used | ET |
| `SP_DATASTORE_UNAVAILABLE` | SharePoint Adapter | Service API | 200 (`kind: error`) | yes | Own tables unreachable | ET |
| `SP_LOCATION_INVALID` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | Stored origin form or a file name is malformed | ET |
| `SP_NAME_EXISTS` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | A file of that name exists (409 on create) | ET |
| `SP_NOT_CONFIGURED` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | The company has not registered ordin with Microsoft (no tenant or client in the profile) | ET |
| `SP_NOT_FOUND` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | Graph 404 | ET |
| `SP_NOT_TRACKED` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | `check_changes` on a link with no tracking state | ET |
| `SP_ORIGIN_CHANGED` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | Origin version differs from the one read (before sending, or 412) | ET |
| `SP_OUTCOME_UNKNOWN` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | Upload or create timed out after sending and the origin shows a change | ET |
| `SP_QUERY_INVALID` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | Search text empty or over 200 characters | ET |
| `SP_RESPONSE_INVALID` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | Graph body does not match the expected shape | ET |
| `SP_SIGN_IN_NEEDED` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | No Microsoft sign-in, refused, or a second 401 | ET |
| `SP_THROTTLED` | SharePoint Adapter | Service API | 200 (`kind: error`) | yes | 429 after three retries | ET |
| `SP_TOO_LARGE` | SharePoint Adapter | Service API | 200 (`kind: error`) | no | File above 250 MiB, or Graph 413 | ET |
| `SP_UNAVAILABLE` | SharePoint Adapter | Service API | 200 (`kind: error`) | yes | 5xx, 503, 504 or network failure after retries; Identity unreachable | ET |
| `SRC_DUPLICATE` | Workspace Service | Service API | 200 (`kind: error`) | no | Location already linked | ET |
| `SRC_KIND_MISMATCH` | Workspace Service | Service API | 200 (`kind: error`) | no | `origin_version` asked for a local or git link kind | ET |
| `SRC_LOCATION_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Location or name malformed | ET |
| `SRC_NAME_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Location or name malformed | ET |
| `SRC_PLACE_MISMATCH` | Workspace Service | Service API | 200 (`kind: error`) | no | Location or name malformed | ET |
| `SRV_APPROVAL_ALREADY_DECIDED` | Caller API | Caller API | 409 | no | Approval already decided, or its reference spent | ET |
| `SRV_APPROVAL_NOT_FOUND` | Caller API | Caller API | 404 | no | Approval absent | ET |
| `SRV_APPROVAL_UNAVAILABLE` | Caller API | Caller API | 503 | yes | Approval Service store unreachable | ET |
| `SRV_BAD_CURSOR` | Shared Types *(also: Approval Service, Caller API, Catalogue Service, File Service, Job Runner, Journal And Artefact Store, Knowledge Service, Remote API, Run Queue, Service API, SharePoint Adapter, Telegram Service)* | Caller API, Remote API, Service API | 422 (Remote API) / 400 (Caller API) | no | Cursor over 1,024 characters, bad base64url, or wrong key shape | ET |
| `SRV_BAD_REQUEST` | Caller API | Caller API | 400 | no | Body not a JSON object or breaking *StartRun*/*Resume*; bad `Last-Event-ID` | ET |
| `SRV_BODY_TOO_LARGE` | Caller API | Caller API | 413 | no | Body above the route's ceiling (caller, work-folder routes) | ET |
| `SRV_DECISION_MISMATCH` | Caller API | Caller API | 400 | no | Decisions count ≠ pending tool calls | ET |
| `SRV_FORBIDDEN` | Caller API | internal | — | no | Operator client: the server answers 403 | ET |
| `SRV_INTERNAL` | Caller API | Caller API | 500 | no | Any unnamed failure or panic | ET |
| `SRV_METHOD_NOT_ALLOWED` | Caller API + Remote API | Caller API, Remote API | 405 | no | Route does not accept the method | ET |
| `SRV_NOT_READY` | Caller API | Caller API | 503 | yes | Instance not ready | ET |
| `SRV_NO_ROUTE` | Caller API + Remote API | Caller API, Remote API | 404 | no | No route has this path | ET |
| `SRV_PORT_UNAVAILABLE` | Caller API | internal | — | no | Port cannot be opened | ET |
| `SRV_REFUSED` | Caller API + Remote API | Caller API, Remote API | 403/503 / 403 | yes / no (per row) | A request with an `Origin` header; more than 100 open requests; another framework refusal | ET |
| `SRV_RESPONSE` | Caller API | internal | — | no | Operator client: any other status of 400 or above not in `accept` | ET |
| `SRV_ROUTE_CONFLICT` | Caller API | internal | — | no | Two routes for one method and path, at build | ET |
| `SRV_RUN_NOT_FOUND` | Caller API | Caller API | 404 | no | Run absent or another caller's | ET |
| `SRV_STOPPED` | Caller API | internal | — | no | Server ended without being told to stop | ET |
| `SRV_THREAD_NOT_FOUND` | Caller API | Caller API | 404 | no | Thread absent or another caller's | ET |
| `SRV_TOKEN_MISSING` | Caller API | internal | — | no | Operator client: `ORDIN_TOKEN_FILE` unset, unreadable or empty | ET |
| `SRV_UNAUTHORISED` | Caller API | internal | — | no | Operator client: the server answers 401 | ET |
| `SRV_UNKNOWN_COMMAND` | Caller API | Caller API | 400 | no | Unknown command | ET |
| `SRV_UNKNOWN_GATE` | Caller API | Caller API | 400 | no | `set_aside` names a gate the run was not paused for | ET |
| `SRV_UNREACHABLE` | Caller API | internal | — | no | Operator client: transport error or timeout | ET |
| `SRV_URL_MISSING` | Caller API | internal | — | no | Operator client: `ORDIN_SERVER_URL` unset, empty or a refused address | ET |
| `START_DUPLICATE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | subject: one report per subject per start | 2C |
| `START_LIST_TOO_LONG` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | each list: ≤ 500 items, each ≤ 500 chars (assumed) | 2C |
| `START_PLAIN_INVALID` | Session Supervisor | Service API | 200 (`kind: error`) | no | `plain`: 1–200 characters (assumed), no identifier from the HLD's "never shown" column | 2C-only |
| `START_REASON_MISSING` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | rules_not_enforced.reason: 1–500 chars (assumed) | 2C |
| `START_TOO_MANY_ITEMS` | Session Supervisor | Service API | 200 (`kind: error`) | no | list: at most 200 items (assumed) | 2C |
| `STA_ABANDONED` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | No successful read for 6 hours, or `abandon` | ET |
| `STA_AGENT_UNKNOWN` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | Remote agent not resolved, or identifiers malformed | ET |
| `STA_ATTACHMENTS_DROPPED` | Slack Thread Adapter | internal | — | no | `StartRun.attachments` non-empty (step 2) | ET |
| `STA_CHANNEL_UNREACHABLE` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | Channel or thread not reachable | ET |
| `STA_DEADLINE` | Slack Thread Adapter | Service API | 200 (`kind: error`) | yes | `ctx.deadline` passed | ET |
| `STA_DECISION_NOT_SEEN` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | Decision in Slack not followed by a result within 24 hours | ET |
| `STA_MISSING_SCOPE` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | Scope missing | ET |
| `STA_NO_REPLY` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | No agent reply within 30 minutes | ET |
| `STA_NO_THREAD_REPLY` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | No agent reply within 30 minutes | ET |
| `STA_RATE_LIMITED` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | Rate limited | ET |
| `STA_RUN_UNKNOWN` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | `read`/`events` of an unknown run | ET |
| `STA_SIGNED_OUT` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | No Slack sign-in in Identity | ET |
| `STA_TARGET_MISMATCH` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | Target is an agent instance | ET |
| `STA_TEXT_TOO_LONG` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | Text over 4,000 characters, or Slack `msg_too_long` | ET |
| `STA_TOKEN_REFUSED` | Slack Thread Adapter | Service API | 200 (`kind: error`) | no | Token refused at start | ET |
| `STA_UNREACHABLE` | Slack Thread Adapter | Service API | 200 (`kind: error`) | yes | Network, TLS, 5xx, 15 s limit at start | ET |
| `STEP_CALL_CYCLE` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | call: another workflow of the same workspace; at most 3 levels; no cycle (04:237) | 2C-only |
| `STEP_CALL_INVALID` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | call: another workflow of the same workspace; at most 3 levels; no cycle (04:237) | 2C |
| `STEP_CONDITION_INVALID` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | condition words: 1–500 chars (assumed) | 2C-only |
| `STEP_EACH_MAX_OUT_OF_RANGE` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | each.maximum: 1–50 (04:237) | 2C-only |
| `STEP_EACH_NOT_LIST` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | each.list: a list input or an earlier step's list result | 2C-only |
| `STEP_GROUP_TOO_LARGE` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | group: holds at most the company's maximum of agents running at once (04:237) | 2C-only |
| `STEP_IF_PATHS_OUT_OF_RANGE` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | if.paths: 2–4 named paths (04:237); names 1–40 chars (assumed), unique | 2C-only |
| `STEP_LINE_INVALID` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | line: one sentence, 1–200 chars (assumed) | 2C-only |
| `STEP_NESTING_TOO_DEEP` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | nesting: at most 4 deep (04:237) | 2C-only |
| `STEP_POSITION_INVALID` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | position: format 3 display number (digits, dots, one trailing letter); unique within its path; ascending (amended January 2026, decision 80: C-24) | 2C-only |
| `STEP_TOO_MANY` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | count: at most 50 steps and blocks in all (04:237) | 2C-only |
| `STEP_UNTIL_MAX_OUT_OF_RANGE` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | until.maximum: 1–10 (04:237) | 2C-only |
| `STEP_USES_LATER_OR_ABSENT` | Crew Orchestrator | Service API | 200 (`kind: error`) | no | uses: only inputs and results of earlier steps that exist on the step's path (04:237) | 2C-only |
| `SUP_ADAPTER_REFUSED` | Session Supervisor | Service API | 200 (`kind: error`) | yes | Adapter refuses a resume (F) | ET |
| `SUP_LEASE_LOST` | Session Supervisor | Service API | 200 (`kind: error`) | no | Lease lost (agent profile, H.1) | ET |
| `SUP_LIMIT_REACHED` | Session Supervisor | Service API | 200 (`kind: error`) | no | Wall clock, budget or idle limit (D.3) | ET |
| `SUP_NO_ADAPTER` | Session Supervisor *(also: Crew Orchestrator)* | Service API | 200 (`kind: error`) | no | No adapter registered for the kind (B.1) | ET |
| `SUP_RESUME_REFUSED` | Session Supervisor | Service API | 200 (`kind: error`) | no | A duty lock refuses a resume (J.6) | ET |
| `SUP_RUN_ENDED` | Session Supervisor | Service API | 200 (`kind: error`) | no | Cancel or resume of an ended run | ET |
| `SUP_RUN_NOT_FOUND` | Session Supervisor | Service API | 200 (`kind: error`) | no | Session record saved for an unknown or ended run (J.5) | ET |
| `SUP_RUN_NOT_PAUSED` | Session Supervisor | Service API | 200 (`kind: error`) | no | Answer to a run not paused for the person (F) | ET |
| `SUP_SEED_EMPTY` | Session Supervisor | Service API | 200 (`kind: error`) | no | Conversation seed empty (J.1) | ET |
| `SUP_STOPPING` | Session Supervisor | Service API | 200 (`kind: error`) | no | New run while shutting down | ET |
| `SUP_STORE_UNAVAILABLE` | Session Supervisor | Service API | 200 (`kind: error`) | yes | Store unavailable | ET |
| `SUP_TARGET_IS_CREW` | Session Supervisor | Service API | 200 (`kind: error`) | no | A crew task submitted here (A.1) | ET |
| `SUP_VERB_NOT_OFFERED` | Session Supervisor | Service API | 200 (`kind: error`) | no | Adapter lacks the verb (F) | ET |
| `SVC_BACKEND_ID_MALFORMED` | Service API | Service API | 200 (`kind: error`) | no | value: UUID version 7, lowercase, 36 characters | 2C |
| `SVC_BACKEND_UNKNOWN` | Service API *(also: Command Bridge)* | Service API | 404 | no | Backend id is not this core | ET |
| `SVC_BODY_TOO_LARGE` | Service API *(also: Command Bridge, Remote API)* | Remote API, Service API | 422 | no | Command body over 1 MiB (9 MiB for the three text-save commands, `localisation.import_language` and `files.save_export`) | ET |
| `SVC_COMMAND_LOCAL_ONLY` | Service API *(also: Command Bridge)* | Service API | 404 | no | Local-only command from a paired desktop | ET |
| `SVC_COMMAND_UNKNOWN` | Service API | Service API | 404 | no | Command name malformed, unknown, or not the routed one | ET |
| `SVC_DEADLINE` | Service API *(also: Remote API)* | Remote API, Service API | 503 | yes | Implementation did not answer by the deadline | ET |
| `SVC_EVENT_GAP` | Service API *(also: Screens Module, Server Connector)* | internal | — | no | `seq` jump seen by the screens | ET |
| `SVC_EVENT_INVALID` | Service API *(also: Command Bridge, Remote API)* | internal | — | no | Journal record not decodable, or event rule fails | ET |
| `SVC_EVENT_TOO_LARGE` | Service API *(also: Command Bridge, Remote API)* | internal | — | no | Non-block event over 256 KiB | ET |
| `SVC_INPUT_INVALID` | Service API *(also: Remote API)* | Remote API, Service API | 422 | no | Body not an envelope, unknown envelope field, input shape or rule fails | ET |
| `SVC_INTERNAL` | Service API *(also: Remote API)* | Remote API, Service API | 500 | no | Implementation panicked, or returned an unregistered code | ET |
| `SVC_NOT_IN_THIS_VERSION` | Service API *(also: Command Bridge, Remote API)* | Remote API, Service API | 404 | no | Command's group is not among the groups this build answers (ADR-114) | ET |
| `SVC_RATE_LIMITED` | Remote API | Remote API | 429 | yes | Command rate above the pairing's bucket | ET |
| `SVC_REQUEST_ID_MALFORMED` | Service API | Service API | 422 | no | `request_id` not UUIDv7 | ET |
| `SVC_RESUME_TOO_OLD` | Remote API | Remote API | 200 | no | `Last-Event-ID` older than the journal keeps | ET |
| `SVC_UNAUTHENTICATED` | Remote API | Remote API | 401 | no | Missing, malformed or unknown desktop key | ET |
| `SVC_VERSION_MALFORMED` | Service API | Service API | 422 | no | Version numbers out of range | ET |
| `SVC_VERSION_UNSUPPORTED` | Service API *(also: Remote API)* | Remote API, Service API | 409 | no | Version pair not accepted | ET |
| `TASK_ARGUMENTS_INVALID` | Session Supervisor | Service API | 200 (`kind: error`) | no | Task breaks a 2C rule (A.1) | ET |
| `TASK_COMMAND_INVALID` | Session Supervisor | Service API | 200 (`kind: error`) | no | Task breaks a 2C rule (A.1) | ET |
| `TASK_COMMAND_XOR_TEXT` | Session Supervisor | Service API | 200 (`kind: error`) | no | Task breaks a 2C rule (A.1) | ET |
| `TASK_IMMUTABLE` | Session Supervisor | Service API | 200 (`kind: error`) | no | row: immutable once submitted (04:171) | 2C |
| `TASK_TEXT_INVALID` | Session Supervisor | Service API | 200 (`kind: error`) | no | Task breaks a 2C rule (A.1) | ET |
| `TEAM_ADOPT_INVALID` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | The adopt edit breaks a length or field rule (ADR-110) | ET |
| `TEAM_ADOPT_NOT_READ_ITEM` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Adopt on an item not from a folder the workspace reads, or whose source is gone (ADR-110) | ET |
| `TEAM_ADOPT_PERSONAL_DATA` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | The copy fails the personal-data scan; nothing written (ADR-110) | ET |
| `TEAM_ADOPT_REFUSED` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | The copy fails the instruction check or the write gate; nothing written (ADR-110) | ET |
| `TEAM_ALREADY_ON` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Workspace already in Team Knowledge mode | ET |
| `TEAM_BUNDLE_LIMIT` | Team Knowledge Service | internal | — | no | A plain Open Knowledge Format bundle holds more than 2,000 concept files; recorded in the status, the get reads the first 2,000 by path (ADR-112) | ET |
| `TEAM_CLONE_FAILED` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | The local copy of the repository cannot be made or read (disk) | ET |
| `TEAM_COMPANY_FOLDER_NO_ACCESS` | Team Knowledge Service | status (`TeamFolderStatus.blocked`) | — | no | A company-named folder the person's sign-in cannot read; action `send_to_it` (ADR-110) | ET |
| `TEAM_DATASTORE_UNAVAILABLE` | Team Knowledge Service | Service API | 200 (`kind: error`) | yes | This module's tables cannot be read or written | ET |
| `TEAM_DESCRIPTION_NOT_TAKEN` | Team Knowledge Service | Service API; status problem | 200 (`kind: error`) | no | A description or check reads as an instruction, may hold personal data or breaks a bound (ADR-110) | ET |
| `TEAM_FACT_NOT_TAKEN` | Team Knowledge Service | internal | — | no | A shared fact was refused by the write gate; recorded in the status, get continues | ET |
| `TEAM_FILE_UNREADABLE` | Team Knowledge Service | internal | — | no | One shared file does not parse or breaks a field rule; recorded in the status, get continues | ET |
| `TEAM_FOLDER_ALREADY_READ` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | The workspace already reads that folder (ADR-110) | ET |
| `TEAM_FOLDER_IS_OWN` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Reading the workspace's own team folder (ADR-110) | ET |
| `TEAM_FOLDER_IS_READ` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Sharing to a folder the workspace reads (ADR-110) | ET |
| `TEAM_FOLDER_NOT_FOUND` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | The folder is not one this workspace reads (ADR-110) | ET |
| `TEAM_FOLDER_SET_BY_COMPANY` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Stop reading a folder the company requires (ADR-110) | ET |
| `TEAM_FORMAT_NEWER` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | The folder's format is newer than this installation reads | ET |
| `TEAM_INDEX_NOT_OURS` | Team Knowledge Service | internal | — | no | The own folder's `index.md` was written by hand; recorded once and never overwritten (ADR-112) | ET |
| `TEAM_ITEM_HELD_ELSEWHERE` | Team Knowledge Service | status problem | — | no | A shared item already held here from another folder or as an own item; get continues (ADR-110) | ET |
| `TEAM_LINK_NOT_SUITABLE` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Link not an active remote repository folder of the workspace, host not GitHub or the company's GitLab, or the branch does not exist | ET |
| `TEAM_LOG_UNREADABLE` | Team Knowledge Service | internal | — | no | The own folder's `log.md` is not UTF-8 or is over 1 MiB; recorded, the share continues without a log entry (ADR-112) | ET |
| `TEAM_MEMORY_UNAVAILABLE` | Team Knowledge Service | Service API | 200 (`kind: error`) | yes / no (per row) | Memory export or import failed | ET |
| `TEAM_NOTHING_TO_SHARE` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Nothing new, changed or removed to share | ET |
| `TEAM_NOT_ADOPTED` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Adopt again or Keep as it is on an item that was not adopted (ADR-110) | ET |
| `TEAM_NOT_ON` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Workspace not in Team Knowledge mode | ET |
| `TEAM_NO_ACCESS` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | The person cannot read the repository | ET |
| `TEAM_NO_OWN_FOLDER` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Share, Adopt or descriptions in a workspace with no own folder (ADR-110) | ET |
| `TEAM_NO_SOURCE_CHANGE` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Adopt again or Keep as it is with no change at the source (ADR-110) | ET |
| `TEAM_PAGES_UNAVAILABLE` | Team Knowledge Service | Service API | 200 (`kind: error`) | yes / no (per row) | Knowledge pages could not be exported | ET |
| `TEAM_PUSH_FAILED` | Team Knowledge Service | Service API | 200 (`kind: error`) | yes / no (per row) | The Push Service refused the proposal or the send for another reason | ET |
| `TEAM_READ_LIMIT` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | A person-added read folder beyond 20 per workspace (company folders are never refused) (ADR-110) | ET |
| `TEAM_READ_ONLY` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Share with read-only access | ET |
| `TEAM_REMOTE_UNREACHABLE` | Team Knowledge Service | Service API | 200 (`kind: error`) | yes | Host unreachable or rate-limited after 3 attempts | ET |
| `TEAM_REVIEW_CANNOT_TELL` | Team Knowledge Service | status (`TeamFolderStatus.blocked`) | — | no | A company folder set to need review whose rules the person's access does not show (ADR-110) | ET |
| `TEAM_REVIEW_NOT_REQUIRED` | Team Knowledge Service | status (`TeamFolderStatus.blocked`) | — | no | A company folder set to need review whose branch does not require it (ADR-110) | ET |
| `TEAM_SHARE_PENDING` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | A share is already waiting for its card or its review | ET |
| `TEAM_SHARE_RACE` | Team Knowledge Service | Service API | 200 (`kind: error`) | yes | The branch moved more than 3 times while sending | ET |
| `TEAM_SHARE_UNKNOWN` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | The send's outcome is not known (no answer after it left) | ET |
| `TEAM_SIGN_IN_NEEDED` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | No sign-in for the host, or the token was refused | ET |
| `TEAM_WORKSPACE_NOT_FOUND` | Team Knowledge Service | Service API | 200 (`kind: error`) | no | Workspace absent or in the bin | ET |
| `TEL_ALREADY_INITIALISED` | Telemetry | internal | — | no | `init` called a second time in one process | ET |
| `TEL_ENDPOINT_INVALID` | Telemetry | internal | — | no | `ORDIN_OTLP_ENDPOINT` is not a URL, has another scheme, uses `http` to a host other than loopback, or holds a user name or password | ET |
| `TEL_EXPORTER_BUILD` | Telemetry | internal | — | no | The OTLP exporter or the tracer provider cannot be built | ET |
| `TEL_LABEL_INVALID` | Telemetry | internal | — | no | `event_from_named` given missing, extra or out-of-set labels | ET |
| `TEL_NAME_UNKNOWN` | Telemetry | internal | — | no | `event_from_named` given a name outside the closed list | ET |
| `TEL_RELEASE_INVALID` | Telemetry | internal | — | no | `set_release` given an empty release, a release above 256 characters, or a digest above 256 characters | ET |
| `TEV_IDENTIFIER_INVALID` | Trigger Service | internal | — | no | Identifier absent or over 256 chars | ET |
| `TEV_PAYLOAD_STORED` | Trigger Service | Service API | 200 (`kind: error`) | no | payload: never stored (04:286) | 2C |
| `TEV_RUN_MISMATCH` | Trigger Service | Service API | 200 (`kind: error`) | no | crew_run_id: absent unless `outcome = started`; set once the workflow run has started (amended January 2026, decision 80: C-8) | 2C |
| `TEXT_CONTROL_CHAR` | Shared Types | Service API | 200 (`kind: error`) | no | Text contains a control character other than `\n`, `\t` | ET |
| `TGA_BAD_RESPONSE` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Answer missing a field this crate reads | ET |
| `TGA_BLOCKED` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Bot blocked by the person, or not in the chat (403) | ET |
| `TGA_BUTTON_DATA_TOO_LONG` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Button data over 64 bytes | ET |
| `TGA_CODE_EXPIRED` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Pairing code expired | ET |
| `TGA_CODE_WRONG` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Pairing code wrong | ET |
| `TGA_DATASTORE_FAILED` | Telegram Adapter | Service API | 200 (`kind: error`) | yes | Offset or code table read/write fails | ET |
| `TGA_FILE_TOO_LARGE` | Telegram Adapter | Service API | 200 (`kind: error`) | no | File over the send or receive limit | ET |
| `TGA_MESSAGE_GONE` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Message to edit no longer exists | ET |
| `TGA_NOT_ALLOWED` | Telegram Adapter | Service API | 200 (`kind: error`) | no | `begin_pairing` while Telegram is not an allowed channel | ET |
| `TGA_NOT_SERVER_MODE` | Telegram Adapter | Service API | 200 (`kind: error`) | no | `begin_pairing` outside server mode | ET |
| `TGA_PAIRING_STOPPED` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Fifth wrong code in 10 minutes | ET |
| `TGA_POLL_CONFLICT` | Telegram Adapter | Service API | 200 (`kind: error`) | no | `getUpdates` conflict (409) | ET |
| `TGA_RATE_LIMITED` | Telegram Adapter | Service API | 200 (`kind: error`) | yes | Telegram says too many requests (429) | ET |
| `TGA_REFUSED` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Other 4xx | ET |
| `TGA_SECRET_STORE_FAILED` | Telegram Adapter | Service API | 200 (`kind: error`) | yes | Secret store unreadable | ET |
| `TGA_TEXT_TOO_LONG` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Text over 4,096 chars reaches Telegram | ET |
| `TGA_TOKEN_INVALID` | Telegram Adapter | Service API | 200 (`kind: error`) | no | Token not in Telegram's form (desktop key, setup command) | ET |
| `TGA_TOKEN_MISSING` | Telegram Adapter | Service API | 200 (`kind: error`) | no | No token in the secret store | ET |
| `TGA_TOKEN_REFUSED` | Telegram Adapter | Service API | 200 (`kind: error`) | yes | Token refused by Telegram (401) | ET |
| `TGA_UNREACHABLE` | Telegram Adapter | Service API | 200 (`kind: error`) | yes | Transport error, timeout or 5xx | ET |
| `TG_APPROVAL_UNAVAILABLE` | Telegram Service | Service API | 200 (`kind: error`) | yes | Approval Service fails to open the card | ET |
| `TG_APP_NOT_FOUND` | Telegram Service | Service API | 200 (`kind: error`) | no | Desktop: Telegram app not found | ET |
| `TG_ARGS_INVALID` | Telegram Service | MCP tool | — | no | Arguments malformed or unknown field | ET |
| `TG_BOT_KEY_INVALID` | Telegram Service | Service API | 200 (`kind: error`) | no | Desktop: pasted key malformed or refused by Telegram | ET |
| `TG_BOT_REMOVED` | Telegram Service | Service API | 200 (`kind: error`) | no | Bot removed from the chat | ET |
| `TG_CHAT_NOT_ALLOWED` | Telegram Service | Service API | 200 (`kind: error`) | no | Chat not in the profile's allowed chats | ET |
| `TG_CHAT_NOT_OFFERED` | Telegram Service | Service API | 200 (`kind: error`) | no | `link` for a chat the bot is not in | ET |
| `TG_DRAFT_ALREADY_ASKED` | Telegram Service | Service API | 200 (`kind: error`) | no | Draft already has an open card | ET |
| `TG_DRAFT_ALREADY_POSTED` | Telegram Service | Service API | 200 (`kind: error`) | no | Draft already posted | ET |
| `TG_DRAFT_FILES_TOO_MANY` | Telegram Service | Service API | 200 (`kind: error`) | no | More than 10 files | ET |
| `TG_DRAFT_FILE_NOT_FOUND` | Telegram Service | Service API | 200 (`kind: error`) | no | Attached file missing | ET |
| `TG_DRAFT_FILE_NOT_READABLE` | Telegram Service | Service API | 200 (`kind: error`) | no | Attached file in a place the session may not read | ET |
| `TG_DRAFT_FILE_TOO_LARGE` | Telegram Service | Service API | 200 (`kind: error`) | no | Attached file over 50 MB | ET |
| `TG_DRAFT_NOT_FOUND` | Telegram Service | Service API | 200 (`kind: error`) | no | Draft not found in the workspace | ET |
| `TG_DRAFT_TEXT_INVALID` | Telegram Service | Service API | 200 (`kind: error`) | no | Draft text with foot line not 1–4,096 chars | ET |
| `TG_FILE_NOT_FOUND` | Telegram Service | Service API | 200 (`kind: error`) | no | File id not on the message | ET |
| `TG_FILE_SAVE_FAILED` | Telegram Service | Service API | 200 (`kind: error`) | yes | Writing the saved file fails | ET |
| `TG_FILE_TOO_LARGE` | Telegram Service | Service API | 200 (`kind: error`) | no | File over 20 MB | ET |
| `TG_FILE_UNAVAILABLE` | Telegram Service | Service API | 200 (`kind: error`) | yes | Telegram no longer serves the file | ET |
| `TG_FILE_UNSAFE` | Telegram Service | Service API | 200 (`kind: error`) | no | Incoming-file safety check fails | ET |
| `TG_LINK_DUPLICATE` | Telegram Service | Service API | 200 (`kind: error`) | no | `link` for an already linked chat or topic | ET |
| `TG_LINK_NOT_FOUND` | Telegram Service | MCP tool | — | no | `link_id` not a linked chat of the session's workspace | ET |
| `TG_MESSAGE_DUPLICATE` | Telegram Service | Service API | 200 (`kind: error`) | no | Duplicate kept message | ET |
| `TG_MESSAGE_NOT_FOUND` | Telegram Service | Service API | 200 (`kind: error`) | no | Message id not kept for the link | ET |
| `TG_NOT_GRANTED` | Telegram Service | MCP tool | — | no | Caller lacks the grant the tool needs | ET |
| `TG_POST_FIELDS_MISMATCH` | Telegram Service | Service API | 200 (`kind: error`) | no | action_key, telegram_message_id: present if and only if `state = posted` | 2C |
| `TG_POST_REFUSED` | Telegram Service | Service API | 200 (`kind: error`) | no | Telegram refuses the post | ET |
| `TG_POST_UNKNOWN_OUTCOME` | Telegram Service | internal | — | no | Crash during a post | ET |
| `TG_PROGRAM_UNSUPPORTED` | Telegram Service | Service API | 200 (`kind: error`) | no | Started in an agent-profile program | ET |
| `TG_QUERY_INVALID` | Telegram Service | MCP tool | — | no | Search query empty or > 200 chars | ET |
| `TG_SERVER_BOT_FROM_SETUP` | Telegram Service | Service API | 200 (`kind: error`) | no | Server: `connect_bot` called | ET |
| `TG_STORE_UNAVAILABLE` | Telegram Service | Service API | 200 (`kind: error`) | yes | Datastore read or write fails | ET |
| `TG_TELEGRAM_UNREACHABLE` | Telegram Service | Service API | 200 (`kind: error`) | yes | Telegram unreachable (download, connect, post before sending) | ET |
| `TG_TEXT_TOO_LONG` | Telegram Service | Service API | 200 (`kind: error`) | no | Kept text over 4,096 chars | ET |
| `TG_TITLE_INVALID` | Telegram Service | Service API | 200 (`kind: error`) | no | title: 1–255 chars (Telegram's chat title limit) | 2C |
| `TG_TOOLS_OFF` | Telegram Service | MCP tool | — | no | Telegram not in `channels.allowed`, or `telegram.tools_enabled = false` | ET |
| `TG_TOPIC_MISMATCH` | Telegram Service | Service API | 200 (`kind: error`) | no | `link` with a topic for a non-forum chat | ET |
| `TG_WORKSPACE_NOT_FOUND` | Telegram Service | Service API | 200 (`kind: error`) | no | `link` for a workspace that does not exist | ET |
| `TIMER_BAD_ENTRY` | Duty Timer | internal | — | no | An entry names an unknown command or duty, has a cron that is not five valid fields, or shares its key with another entry | ET |
| `TOOL_APPROVAL_UNBOUND` | Connection Binder | internal | — | no | An approval name is no bound tool | ET |
| `TOOL_CALL_FAILED` | Connection Binder | MCP tool | — | no | In a run: a wrong argument, an unknown argument, a timeout, a failed send or another status | ET |
| `TOOL_CHAT_CLIENT_MISSING` | Connection Binder | internal | — | no | A runtime tool is listed and its host (settings, documents, run) is not attached | ET |
| `TOOL_CHECKER_UNFIT` | Connection Binder | internal | — | no | A content check names an operation that takes no JSON body or has a path parameter (ADR-107) | ET |
| `TOOL_CONTRACT_MISMATCH` | Connection Binder | internal | — | no | A serving instance has no run host for `event_record`, `run_facts`, `reply_action` or `channel_documents` | ET |
| `TOOL_MAPPING_MISFIT` | Connection Binder | internal | — | no | A mapping's arguments do not fit the connection's operation | ET |
| `TOOL_MAPPING_NOT_EXPOSED` | Connection Binder | internal | — | no | A mapping names an unbound connection or an operation it did not keep | ET |
| `TOOL_NAME_INVALID` | Connection Binder | internal | — | no | A kept tool's name does not match the tool-name pattern | ET |
| `TOOL_NAME_TAKEN` | Connection Binder | internal | — | no | A kept tool takes a reserved name; two tools share a name; a declared operation has another tool's name | ET |
| `TOOL_NOT_OFFERED` | Connection Binder | internal | — | no | A listed name is not among the connection's operations | ET |
| `TOOL_OPENAPI_INVALID` | Connection Binder | internal | — | no | The OpenAPI file is outside the configuration folder, too large, refused by the restricted YAML reader (anchor, alias, custom tag, malformed), not… | ET |
| `TOOL_OPERATION_NOT_MAPPED` | Connection Binder | internal | — | no | A required declared operation has no mapping | ET |
| `TOOL_SECRETS_DIR_INVALID` | Connection Binder | internal | — | no | `ORDIN_SECRETS_DIR` is not a readable folder | ET |
| `TOOL_SECRET_MISSING` | Connection Binder | internal | — | no | A credential variable the configuration lists has no value | ET |
| `TOOL_SERVER_UNREACHABLE` | Connection Binder | internal | — | yes | An MCP server does not answer within 30 s, refuses, or its command is empty or cannot start | ET |
| `TOOL_URL_INVALID` | Connection Binder | internal | — | no | An outbound destination held in a variable is not an https address | ET |
| `TREQ_APPROVAL_MISMATCH` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | approval_id: present if and only if `decision = ask` once decided by the person | 2C |
| `TREQ_IMMUTABLE` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | row: immutable after `executed` or `refused` (04:192) | 2C |
| `TREQ_NAME_INVALID` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | tool: 1–200 chars (assumed) | 2C |
| `TREQ_UNDECIDED` | Journal And Artefact Store | Service API | 200 (`kind: error`) | no | decision, deciding_rule: present from `decided` on | 2C |
| `TRIG_CONSUMER_INVALID` | Trigger Service *(also: NATS Adapter)* | Service API | 200 (`kind: error`) | no | consumer_name: 1–255 chars `[A-Za-z0-9_-]` (NATS consumer name rule, assumed) | 2C |
| `TRIG_DAILY_LIMIT_REACHED` | Trigger Service | internal | — | no | Event after the daily limit | ET |
| `TRIG_DATASTORE_FAILED` | Trigger Service | Service API | 200 (`kind: error`) | yes | Datastore read or write fails | ET |
| `TRIG_EVENTS_NOT_CONFIGURED` | Trigger Service | Service API | 200 (`kind: error`) | no | Turn on a NATS trigger with no NATS servers in the profile | ET |
| `TRIG_FILE_KIND_INVALID` | Trigger Service | Service API | 200 (`kind: error`) | no | File kind or name pattern invalid | ET |
| `TRIG_FILTER_TOO_LONG` | Trigger Service | Service API | 200 (`kind: error`) | no | More than 20 filter conditions | ET |
| `TRIG_FILTER_VALUE_INVALID` | Trigger Service | Service API | 200 (`kind: error`) | no | A filter value of the wrong shape for its operator | ET |
| `TRIG_INPUT_UNMAPPED` | Trigger Service | Service API | 200 (`kind: error`) | no | A required input not mapped | ET |
| `TRIG_MEMBER_INVALID` | Trigger Service | Service API | 200 (`kind: error`) | no | 'SlackMessage.from_member' matches '^[UW][A-Z0-9]{8,20}$' when present | ET |
| `TRIG_NAME_INVALID` | Trigger Service | Service API | 200 (`kind: error`) | no | Proposal name empty, over 80 chars, or taken in the workspace | ET |
| `TRIG_NATS_FAILED` | Trigger Service | Service API | 200 (`kind: error`) | yes | Opening or deleting a NATS feed fails | ET |
| `TRIG_NATS_FIELDS_MISMATCH` | Trigger Service | Service API | 200 (`kind: error`) | no | `delivery`/`identifier_source` present for a chat trigger or absent for a NATS trigger | ET |
| `TRIG_NOT_FOUND` | Trigger Service | Service API | 200 (`kind: error`) | no | Trigger id unknown | ET |
| `TRIG_NO_WORKSPACE` | Trigger Service | Service API | 200 (`kind: error`) | no | `/run` with no workspace in the conversation | ET |
| `TRIG_PATH_INVALID` | Trigger Service | Service API | 200 (`kind: error`) | no | A path outside the grammar | ET |
| `TRIG_PATTERN_INVALID` | Trigger Service | Service API | 200 (`kind: error`) | no | File kind or name pattern invalid | ET |
| `TRIG_PAYLOAD_TOO_LONG` | Trigger Service | Service API | 200 (`kind: error`) | no | A fixed value over 4,000 chars (proposal); a payload value over 4,000 chars (event → `unreadable`) | ET |
| `TRIG_PHRASE_INVALID` | Trigger Service | Service API | 200 (`kind: error`) | no | 'SlackMessage.phrase' 1–200 chars after trimming | ET |
| `TRIG_RUN_IN_PROGRESS` | Trigger Service | internal | — | no | Event while the run slot is busy and the queue is full | ET |
| `TRIG_SLACK_LINK_UNKNOWN` | Trigger Service | Service API | 200 (`kind: error`) | no | 'SlackMessage.slack_link_id' names a link in state 'linked' of the trigger's own workspace | ET |
| `TRIG_SLACK_OFF` | Trigger Service | Service API | 200 (`kind: error`) | no | Slack turned off by the profile ('slack.tools_enabled = false') at proposal or **Turn on** | ET |
| `TRIG_START_FAILED` | Trigger Service | Service API | 200 (`kind: error`) | yes | Ordin Host refuses or fails to start the workflow run | ET |
| `TRIG_STATE_INVALID` | Trigger Service | Service API | 200 (`kind: error`) | no | Turn on/off/remove from a state that does not allow it | ET |
| `TRIG_STOPPING` | Trigger Service | Service API | 200 (`kind: error`) | yes | A start or proposal after the shutdown token was cancelled | ET |
| `TRIG_SUBJECT_NOT_ALLOWED` | Trigger Service | Service API | 200 (`kind: error`) | no | Subject outside the profile's list | ET |
| `TRIG_WAITING_OUT_OF_RANGE` | Trigger Service | Service API | 200 (`kind: error`) | no | `max_waiting` above 10 | ET |
| `TRIG_WORKFLOW_NOT_FOUND` | Trigger Service | Service API | 200 (`kind: error`) | no | Workflow or pinned version not found | ET |
| `TS_NOT_RFC3339_MS_UTC` | Shared Types | Service API | 200 (`kind: error`) | no | Timestamp not `YYYY-MM-DDTHH:MM:SS.mmmZ`, or an impossible date | ET |
| `UI_BACKEND_UNKNOWN` | Screens Module | internal | — | no | An event for an unknown backend | ET |
| `UI_BLOCK_KIND_UNKNOWN` | Screens Module | Service API | 200 (`kind: error`) | no | A block of an unknown `kind` | ET |
| `UI_CATALOGUE_RELOAD_FAILED` | Screens Module | internal | — | no | `localisation.get_catalogue` fails on `language_changed`; recovered, the text in use stays, recorded in the diagnostics ring only (ADR-109) | ET |
| `UI_CHART_RENDER_FAILED` | Screens Module | Service API | 200 (`kind: error`) | no | A chart fails to render | ET |
| `UI_COMMAND_TIMEOUT` | Screens Module | Service API | 200 (`kind: error`) | yes | A command that waits for no model has no reply after 10 s | ET |
| `UI_EVENT_KIND_UNKNOWN` | Screens Module | Service API | 200 (`kind: error`) | no | An event of an unknown `kind` (newer minor version) | ET |
| `UI_EVENT_VERSION` | Screens Module | internal | — | no | An event of another major version | ET |
| `UI_IPC_FAILED` | Screens Module | Service API | 200 (`kind: error`) | yes | `invoke` rejects: the Command Bridge is not reachable | ET |
| `UI_KNOWLEDGE_ADDRESS_NOT_HTTPS` | Screens Module | internal | — | no | The address in the connect dialog does not start with `https://`; nothing is sent, recorded in the diagnostics ring only (ADR-113) | ET |
| `UI_LINK_REFUSED` | Screens Module | Service API | 200 (`kind: error`) | no | A Slack permalink from the core is not 'https://*.slack.com/archives/…' | ET |
| `UI_MESSAGE_MISSING` | Screens Module | Service API | 200 (`kind: error`) | no | A catalogue message missing in the active locale | ET |
| `UI_STORAGE_UNAVAILABLE` | Screens Module | Service API | 200 (`kind: error`) | no | `localStorage` throws or holds an unparsable value | ET |
| `USAGE_ABOVE_PROFILE_LIMIT` | Usage Planner | Service API | 200 (`kind: error`) | no | A value above the company's frame (B.2–B.4) | ET |
| `USAGE_AGENTS_OUT_OF_RANGE` | Usage Planner | Service API | 200 (`kind: error`) | no | Agents at once outside 1–3 (B.3) | ET |
| `USAGE_DAILY_PAUSE_REACHED` | Usage Planner | Service API | 200 (`kind: error`) | no | The daily pause is reached (F) | ET |
| `USAGE_FIXED_BY_COMPANY` | Usage Planner | Service API | 200 (`kind: error`) | no | The person changes a setting the company fixed (B.1) | ET |
| `USAGE_JOURNAL_UNAVAILABLE` | Usage Planner | Service API | 200 (`kind: error`) | yes | Journal unavailable while recording | ET |
| `USAGE_LEVEL_UNKNOWN` | Usage Planner | Service API | 200 (`kind: error`) | no | Unknown kind or level (B.2) | ET |
| `USAGE_NOT_ANSWER_ONLY` | Usage Planner | Service API | 200 (`kind: error`) | no | Indexing a run that acted, changed or was approved (G.3) | ET |
| `USAGE_STORE_UNAVAILABLE` | Usage Planner | Service API | 200 (`kind: error`) | yes | Store unavailable | ET |
| `VOCAB_CODE_INVALID` | Catalogue Service *(also: Checker)* | Service API | 200 (`kind: error`) | no | code: 1–64 chars `[a-z0-9-]` (assumed); unique per (agent_repository_id, axis) | 2C |
| `VOCAB_DESCRIPTION_TOO_LONG` | Catalogue Service | Service API | 200 (`kind: error`) | no | description: ≤ 1000 chars (assumed) | 2C |
| `VOCAB_LABEL_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | label: 1–120 chars (assumed) | 2C |
| `VOCAB_STATUS_TERM_FIXED` | Authoring Service | Service API | 200 (`kind: error`) | no | Status term retired | ET |
| `VOCAB_TERM_CITED` | Authoring Service *(also: Checker)* | Service API | 200 (`kind: error`) | no | retire: refused while any part of the repository cites it (04:345) | 2C |
| `VOICE_FIXED_BY_COMPANY` | Ordin Host | Service API | 200 (`kind: error`) | no | Voice field set by the company | ET |
| `VOICE_NAME_TOO_LONG` | Ordin Host | Service API | 200 (`kind: error`) | no | Voice notes over 300 / name over 60 | ET |
| `VOICE_NOTES_TOO_LONG` | Ordin Host | Service API | 200 (`kind: error`) | no | Voice notes over 300 / name over 60 | ET |
| `WCI_DUPLICATE` | Workspace Service | Service API | 200 (`kind: error`) | no | (source_link_id, origin_path): unique | 2C |
| `WCI_PATH_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | origin_path: 1–2048 bytes | 2C |
| `WCI_VERSION_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | origin_version_read: 1–256 chars (assumed) | 2C |
| `WDL_APPEND_ONLY` | Catalogue Service *(also: Checker)* | Service API | 200 (`kind: error`) | no | row: never updated except `removed_by_hand_seen_at`; never deleted while the repository is read | 2C |
| `WDL_REASON_INVALID` | Catalogue Service | Service API | 200 (`kind: error`) | no | reason: 1–1000 chars (assumed) | 2C |
| `WDL_REPLACEMENT_INCOMPLETE` | Catalogue Service | Service API | 200 (`kind: error`) | no | replacement_identifier, replacement_version: both present or both absent | 2C |
| `WDL_VERSION_DIGEST_MISMATCH` | Catalogue Service *(also: Checker)* | Service API | 200 (`kind: error`) | no | part_version, part_content_digest: both present or both absent (04:323) | 2C |
| `WFF_AGENTS_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `agents`: 1–8 (A-32 original figure and `07-cross-cutting.md:42`, crews of up to 8 members) | 2C |
| `WFF_AGENT_ID_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `agents[].id`: 1–256 characters, starts `urn:ordin:` | 2C |
| `WFF_AGENT_KEY_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `agents[].key`: `^[a-z][a-z0-9_-]{0,31}$`, unique | 2C |
| `WFF_AGENT_KEY_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | `limits.external_action_agents`: each a declared agent key | 2C |
| `WFF_AGENT_NAME_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | 1–120 characters; matched to an installed agent by name | IL |
| `WFF_AGENT_ROLE_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | agents.role: one line, 1–200 chars (assumed) | 2C |
| `WFF_AGENT_UNMATCHED` | Ordin Host | Service API | 200 (`kind: error`) | no | Format 1 or 2 file naming an agent not in the workspace | ET |
| `WFF_AGENT_VERSION_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `agents[].version`: a SemVer requirement in Cargo syntax (assumed; see I-3) | 2C |
| `WFF_CALL_CYCLE` | Ordin Host | Service API | 200 (`kind: error`) | no | `run_workflow.workflow`: a workflow of the workspace, at most 3 levels deep, no cycle ("This would run … again inside itself") | 2C |
| `WFF_CALL_TOO_DEEP` | Ordin Host | Service API | 200 (`kind: error`) | no | `run_workflow.workflow`: a workflow of the workspace, at most 3 levels deep, no cycle ("This would run … again inside itself") | 2C |
| `WFF_CALL_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | `run_workflow.workflow`: a workflow of the workspace, at most 3 levels deep, no cycle ("This would run … again inside itself") | 2C |
| `WFF_CHANGE_NOTE_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | change_note: one line, 1–200 chars (assumed) | 2C |
| `WFF_CHECK_FAILED` | Ordin Host | Service API | 200 (`kind: error`) | no | any version: that fails the core's check is never saved (04:233) | 2C |
| `WFF_COMMIT_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | followed_commit: 40 lowercase hex chars | 2C |
| `WFF_CONDITION_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `decided_by.words`: 1–300 characters (assumed) | 2C |
| `WFF_FIELD_UNKNOWN` | Ordin Host | Service API | 200 (`kind: error`) | no | content: no field for documents, places, links, folder grants, memory, knowledge pages, run history, inputs used or a person's name… | 2C |
| `WFF_FILE_MALFORMED` | Ordin Host | Service API | 200 (`kind: error`) | no | Import file malformed | ET |
| `WFF_FILE_TOO_LARGE` | Ordin Host | Service API | 200 (`kind: error`) | no | Import text or export output over 256 KB | ET |
| `WFF_FORMAT_NEWER` | Ordin Host | Service API | 200 (`kind: error`) | no | Import file of a newer format | ET |
| `WFF_FORMAT_UNSUPPORTED` | Ordin Host | Service API | 200 (`kind: error`) | no | Import file whose `format` is missing, not an integer or below 1 | ET |
| `WFF_GROUP_TOO_LARGE` | Ordin Host | Service API | 200 (`kind: error`) | no | steps at the same time: a group (same number, letters) holds at most `agents_at_once` | 2C |
| `WFF_IF_PATHS_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `if.paths`: 2–4, names 1–40 characters (assumed) and unique; at least one path holds a step | 2C |
| `WFF_INPUT_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `inputs`: 0–20 (assumed); `label` 1–60 characters (assumed), unique | 2C |
| `WFF_INPUT_LABEL_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | inputs.label: 1–80 chars (assumed); unique per version | 2C-only |
| `WFF_LIMIT_ABOVE_MAX` | Ordin Host | Service API | 200 (`kind: error`) | no | limits: each at or below the company's maximum (04:228) | 2C-only |
| `WFF_LIMIT_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `limits.failure_repeat_limit`: 1–5 (assumed; the default escalation is after 2, `07-cross-cutting.md:47`) | 2C |
| `WFF_LIMIT_LOWERED` | Ordin Host | Service API | 200 (`kind: error`) | no | Limit above the company's maximum | ET |
| `WFF_LINE_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `line`: 1–200 characters (A-32 original) | 2C |
| `WFF_LOOP_LIST_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `for_each.list`: a `list` input or an earlier step | 2C |
| `WFF_LOOP_MAX_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `for_each.max_items`: 1–50 (ADR-065) | 2C |
| `WFF_NAME_DUPLICATE` | Ordin Host | Service API | 200 (`kind: error`) | no | Name taken in the workspace | ET |
| `WFF_NAME_EMPTY` | Ordin Host | Service API | 200 (`kind: error`) | no | name: required, ≤ 80 chars (04:233) | 2C |
| `WFF_NAME_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `name`: 1–80 characters (`04-domain.md:229`); a name used in the workspace gets " (2)" | 2C |
| `WFF_NAME_TOO_LONG` | Ordin Host | Service API | 200 (`kind: error`) | no | name: required, ≤ 80 chars (04:233) | 2C |
| `WFF_ORIGIN_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `origin`: 0–200 characters (assumed) | 2C |
| `WFF_POSITION_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `position`: `^[0-9]{1,2}[a-z]?(\.[0-9]{1,2}[a-z]?){0,3}$`, unique, in outline order (ADR-065 numbering) | 2C |
| `WFF_PURPOSE_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | purpose: 1–2000 chars (assumed) | 2C |
| `WFF_REPOSITORY_INVALID` | Ordin Host | Service API | 200 (`kind: error`) | no | `agents[].repository`: a profile key known to the organisation profile, or an `https://` address; one the person has not added is listed, not an… | 2C |
| `WFF_SEND_WITHOUT_ASK` | Ordin Host | Service API | 200 (`kind: error`) | no | sending steps: a step whose agent sends email is always `asks_first: true` (`09-decisions.md:1592`) | 2C |
| `WFF_TOO_DEEP` | Ordin Host | Service API | 200 (`kind: error`) | no | steps and blocks: at most 50 in all, nested at most 4 deep (A-32 revised; ADR-065) | 2C |
| `WFF_TOO_MANY_STEPS` | Ordin Host | Service API | 200 (`kind: error`) | no | steps and blocks: at most 50 in all, nested at most 4 deep (A-32 revised; ADR-065) | 2C |
| `WFF_USE_NOT_ON_PATH` | Ordin Host | Service API | 200 (`kind: error`) | no | `uses`: each input exists; each step comes before this one and its result exists on this path (ADR-065) | 2C |
| `WFF_VERSION_GAP` | Ordin Host | Service API | 200 (`kind: error`) | no | version: starts at 1, +1 per **Save** or **Restore** | 2C |
| `WF_BUNDLE_PATH` | File Service *(also: Caller API)* | Caller API | 400 | no | Work folder: bundle path to operator method | ET |
| `WF_INVALID_NAMESPACE` | File Service | Caller API | — | no | Work folder: namespace does not match its pattern | ET |
| `WF_INVALID_PATH` | File Service *(also: Caller API)* | Caller API | 400 | no | Work folder: invalid path (operator) | ET |
| `WF_NOT_FOUND` | File Service *(also: Caller API)* | Caller API | 404 | no | Work folder: no file (operator) | ET |
| `WF_STORE_UNAVAILABLE` | File Service *(also: Caller API)* | Caller API | 503 | yes | Work folder: store unreachable (operator) | ET |
| `WF_TOO_LARGE` | File Service *(also: Caller API)* | Caller API | 413 | no | Work folder: above 10,485,760 bytes (operator) | ET |
| `WS_BIN_MISMATCH` | Workspace Service | Service API | 200 (`kind: error`) | no | moved_to_bin_at: present if and only if `state = in_bin` | 2C |
| `WS_CONFLUENCE_UNAVAILABLE` | Workspace Service | Service API | 200 (`kind: error`) | yes | Confluence asked for on the personal server, or its approval could not be requested | ET |
| `WS_DATASTORE_UNAVAILABLE` | Workspace Service | Service API | 200 (`kind: error`) | yes | Own tables unreachable | ET |
| `WS_FILES_FAILED` | Workspace Service | Service API | 200 (`kind: error`) | yes | File Service call failed | ET |
| `WS_FILE_NAME_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Saved file name empty, too long, with a path character, or no free numbered name | ET |
| `WS_FILE_TOO_LARGE` | Workspace Service | Service API | 200 (`kind: error`) | no | Saved file over 100 MiB | ET |
| `WS_GIT_REMOTE_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Git repository address, branch or path malformed | ET |
| `WS_GRANT_PUBLISH_FAILED` | Workspace Service | Service API | 200 (`kind: error`) | yes | Grants could not be handed to enforcement | ET |
| `WS_ITEM_TOO_LARGE` | Workspace Service | Service API | 200 (`kind: error`) | no | `read_item` of a file above the adapter's limit | ET |
| `WS_JIRA_ADDRESS_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | The pasted text is not a Jira Cloud issue address (ADR-106) | ET |
| `WS_JIRA_READ_ONLY` | Workspace Service | Service API | 200 (`kind: error`) | no | Change access asked for on a Jira issue's items (ADR-106) | ET |
| `WS_JIRA_SITE_MISMATCH` | Workspace Service | Service API | 200 (`kind: error`) | no | The issue is on another Jira site than the one connected (ADR-106) | ET |
| `WS_JIRA_UNAVAILABLE` | Workspace Service | Service API | 200 (`kind: error`) | no | Jira issue link asked for on the personal server (ADR-106) | ET |
| `WS_LINK_NOT_FOUND` | Workspace Service | Service API | 200 (`kind: error`) | no | Link id unknown | ET |
| `WS_LINK_STATE_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Link not in the state the call needs | ET |
| `WS_LOCAL_FOLDER_ON_SERVER` | Workspace Service | Service API | 200 (`kind: error`) | no | Local or git folder linked on the ordin server | ET |
| `WS_NAME_DUPLICATE` | Workspace Service | Service API | 200 (`kind: error`) | no | Name empty / longer than 80 / already used | ET |
| `WS_NAME_EMPTY` | Workspace Service | Service API | 200 (`kind: error`) | no | Name empty / longer than 80 / already used | ET |
| `WS_NAME_TOO_LONG` | Workspace Service | Service API | 200 (`kind: error`) | no | Name empty / longer than 80 / already used | ET |
| `WS_NOT_ACTIVE` | Workspace Service | Service API | 200 (`kind: error`) | no | Work started in an archived or binned workspace | ET |
| `WS_NOT_A_REPOSITORY` | Workspace Service | Service API | 200 (`kind: error`) | no | Chosen folder is not a git repository | ET |
| `WS_NOT_FOUND` | Workspace Service | Service API | 200 (`kind: error`) | no | Workspace id unknown | ET |
| `WS_OWN_FOLDER_FAILED` | Workspace Service | Service API | 200 (`kind: error`) | yes | Own folder cannot be created | ET |
| `WS_PLACE_REFUSED` | Workspace Service | Service API | 200 (`kind: error`) | no | Folder protected, unreadable, or a hidden place | ET |
| `WS_SOURCE_ACCESS_DENIED` | Workspace Service | Service API | 200 (`kind: error`) | no | No access at the origin | ET |
| `WS_SOURCE_NOT_CONFIGURED` | Workspace Service | Service API | 200 (`kind: error`) | no | Origin not set up by the company | ET |
| `WS_SOURCE_NOT_FOUND` | Workspace Service | Service API | 200 (`kind: error`) | no | Origin location gone | ET |
| `WS_SOURCE_SIGN_IN_NEEDED` | Workspace Service | Service API | 200 (`kind: error`) | no | Origin needs a sign-in | ET |
| `WS_SOURCE_UNAVAILABLE` | Workspace Service | Service API | 200 (`kind: error`) | yes | Origin unreachable or throttled | ET |
| `WS_STATE_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Lifecycle change from a state that does not allow it | ET |
| `WS_WORKING_COPY_ITEM_NOT_FOUND` | Workspace Service | Service API | 200 (`kind: error`) | no | Working-copy item id unknown | ET |
| `WS_WORKING_COPY_TRANSITION_INVALID` | Workspace Service | Service API | 200 (`kind: error`) | no | Working-copy state change not allowed | ET |
| `YAML_FEATURE_REFUSED` | Shared Types | Service API | 200 (`kind: error`) | no | YAML uses an anchor, an alias, a merge key or a tag outside the core schema | ET |
| `YAML_MALFORMED` | Shared Types | Service API | 200 (`kind: error`) | no | YAML not well-formed, several documents, duplicate or non-scalar key, nesting over 64, a non-finite float, or not the shape of `T` | ET |
| `YAML_TOO_LARGE` | Shared Types | Service API | 200 (`kind: error`) | no | YAML text longer than `max_bytes` | ET |

## Codes of embedded and external components, mapped at the boundary

These codes are produced by code ordin embeds or calls, not by an ordin crate. They never leave the crate that receives them: each is mapped to one of that crate's registered codes, as the cited spec states. They are not registry rows.

| Foreign code | Produced by | Received and mapped by | Source |
|---|---|---|---|
| `RATE_LIMITED`, `AUTHZ_CLASS_FORBIDDEN`, `QUEUE_UNAVAILABLE`, `STORE_UNAVAILABLE`, `STORE_TIMEOUT`, `PROVIDER_TIMEOUT`, `PROVIDER_ERROR`, `NOT_FOUND`, `VAL_INVALID_BODY` | embedded `memory` crate (`CallError::code()`) | Memory Service → `MEM_*` (for example `RATE_LIMITED` → `MEM_RATE_LIMITED`, retryable) | MemoryService.md Internal Logic 4–7 |
| `CAP_UNKNOWN`, `INTERACTION_DENIED`, `RATE_LIMITED`, `VAL_ERR`, `CFG_SECRET_UNRESOLVED` | embedded `sandboxd` (`ControllerError`, `SecretResolver`) | Sandbox → `SBX_*` (for example `CapUnknown` → `SBX_CAPABILITY_UNKNOWN`) | Sandbox.md Internal Logic A.2, C.10 |
| `RUNTIME_LIMIT` | embedded `sandboxd` guest stderr when a run hits its time or resource limit (`exit_code == -2`, `RUNTIME_LIMIT_EXIT`) | Sandbox: not mapped to an error code; returned to the agent as a result with `exitCode: -2` and `stderr` "RUNTIME_LIMIT". The person never sees it | Sandbox.md Internal Logic C.11, Error Table closing note |
| `SIGN_IN_NEEDED`, `NOT_CONFIGURED`, `ACCESS_DENIED`, `NOT_FOUND`, `TOO_LARGE`, `UNAVAILABLE`, `THROTTLED` (as suffixes of `SP_` and `CONF_`) | SharePoint and Confluence adapters' own codes, read by suffix | Workspace Service's link checks | WorkspaceService.md *Types of the adapters* |
| wrightrun `TEXT_*` identifiers (`TEXT_REPLY_GONE`, `TEXT_ACTION_GONE`, `TEXT_UNKNOWN_COMMAND`, `TEXT_NOT_ENABLED`, `TEXT_NOT_FOR_CHAT`, `TEXT_NO_HAND_OFF`, `TEXT_HANDED_OFF`, `TEXT_ALREADY_HANDED_OFF`, `TEXT_ALREADY_SHARED`, `TEXT_SETTINGS_SAVED`) | chat reply texts, not error codes | Channel Gateway (agent profile) | ChannelGateway.md §L |
| `ERROR_ACCESS_DISABLED_BY_POLICY` (1260), `ERROR_SYSTEM_INTEGRITY_POLICY_VIOLATION` (4551) | Windows raw OS errors on spawn | Setup Assistant (company allow-listing refusal) | SetupAssistant.md Internal Logic 4 |

Code-shaped tokens excluded after reading each line: environment variables (`ORDIN_*`, `ANTHROPIC_*`, `CLAUDE_CODE_OAUTH_TOKEN`, `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, `GH_PROMPT_DISABLED`, `GITHUB_APP_CLIENT_ID`, `HTTP(S)_PROXY`, `NOTIFY_SOCKET`, `DBUS_SESSION_BUS_ADDRESS`, `XDG_DATA_HOME`, `PYS_*`), constants (`MAX_*`, `*_SECONDS`, `*_TIMEOUT`, `*_ROOT`, `*_LIMIT`, `*_BYTES`, `LEASE_*`, `SPAN_*`, `WF_MAX_*`, `WF_BUNDLE_*`, `WF_NAMESPACE_PATTERN`, `FRONT_MATTER`, `BANNER_TEMPLATE`, `BUNDLED_REPOSITORY_ID`, `TOOL_NAME`, `FILE_TOOLS`, `FILE_TOOL_ARGUMENTS`, `RETENTION_*`, `CHECKPOINT_VERSION`), message identifiers (`RUN_PROGRESS_*`), socket options (`SO_PEERCRED`), list identifiers (`ERASE_*_COVERS`, `ERASE_*_NOT_COVERED`), the 2C naming-convention example `SCREAMING_SNAKE_CASE`, the `text.rs` parameter examples in SharedTypes.md (`WS_PATH_INVALID`, `CONF_URL_INVALID`), codes the specs record as removed or renamed (`TIMER_UNAVAILABLE`, DutyTimer.md; `AFM_PATH_INVALID`, AgentFolderManager.md SA-CLOSE-01; `RREC_THREAD_BUSY`, 2C `RunRecord`, now `RUN_THREAD_BUSY`), and the two retired tokens of resolved defects EH-D5 and EH-D13 (`SCOPE_CROSS_WRITE`, `CTL_UNKNOWN_ARGUMENT`), which no spec cites any longer.

## Registry defects

Re-checked on January 2026 against the corrected component specs and Phase 2C files. Each defect needs a change to the spec named (Plan mode, `/spec` correction round), not a choice made here. "Two owners" means two crates raise the code or the prefix themselves; a crate that only passes another's code through (Caller API over Run Queue, Command Bridge over Server Connector, Session Supervisor over adapters and the Agent Loader, Ordin Server over Identity and Credentials) is not a second owner.

### Resolved

| ID | Defect | Resolved where |
|---|---|---|
| EH-D1 | `AUTH_` had two owners (Authoring Service, Caller Authentication). | The Authoring Service's 29 codes are `AUTHR_` (AuthoringService.md Error Table); CallerAuthentication.md records `AUTH_` as carried from wrightrun and its own. |
| EH-D2 | `CMD_` had two owners (Ordin Server; command expansion with no owner). | Ordin Server's codes are `SERVER_ROLE_COMBINATION`, `SERVER_NOTHING_TO_SERVE` (OrdinServer.md); `CMD_UNKNOWN_COMMAND`, `CMD_PROMPT_UNREADABLE`, `CMD_PLACEHOLDER_MISSING` are in AgentLoader.md's Error Table; the Session Supervisor passes them through. |
| EH-D3 | `APPR_DATASTORE_UNAVAILABLE` cited for the Approval Service's `APPR_STORE_UNAVAILABLE`. | No spec cites `APPR_DATASTORE_UNAVAILABLE`; ChannelGateway.md and JournalAndArtefactStore.md use `APPR_STORE_UNAVAILABLE`. |
| EH-D4 | `PUB_` had two owners (File Service, Push Service). | `PUSH_AUDIENCE_GREW`, `PUSH_DELETE_UNCHECKED` (PushService.md, 2C domain). |
| EH-D6 | `PART_` had two owners. | `PKG_DEPRECATED_AS_AGENT` (PackageCatalogue.md Error Table). |
| EH-D9 | `CHARTER_` broke the code pattern `^[A-Z]{2,6}_`. | `CHRT_*` (OrdinHost.md, CrewOrchestrator.md, 2C domain `CrewCharter`); every registered prefix now has 2–6 letters and every code matches `^[A-Z]{2,6}_[A-Z0-9_]{2,60}$`. |
| EH-D10 | `HOOK_` raised at both ends without a record. | HookRelay.md and OrdinHook.md state that `HOOK_` is the 2C protocol prefix raised at both ends and `HOOK_APPROVAL_PENDING` the one shared code. |
| EH-D11 | Workspace Service raised `MAIL_` codes under the Mail Service's prefix. | MailService.md records that the five 2C email-link and email-grant codes are enforced by the Workspace Service. |
| EH-D12 | `DB_UNAVAILABLE` used an undeclared prefix. | RunQueue.md Error Table declares it a carried wrightrun code. |
| EH-D14 | 2C pointed at a "2C error registry". | phase-2c-shared-types-wire.md `CommandReply` validation points at *Error Handling — Error-code registry*. |
| EH-D15 | Two codes from two owners for one secrets-key condition. | OrdinServer.md passes `CRED_SECRET_KEY_CONFLICT` and `CRED_SECRET_KEY_MISSING` through; the `SERVER_SECRETS_KEY_*` codes are gone; the key variables are `ORDIN_SECRETS_KEY_FILE` and `ORDIN_SECRETS_KEY_CREDENTIAL`. |

### Resolved in the correction round of January 2026

| ID | Defect | Resolved where |
|---|---|---|
| EH-D5 | The Knowledge Service cited the old 2C code `SCOPE_CROSS_WRITE`, renamed `MEM_SCOPE_CROSS_WRITE` in the Memory Service and the 2C `Scope` rules. | KnowledgeService.md Shared Context (the `Scope` rule comment), Internal Logic 2 and Performance, Security, Observability now cite `MEM_SCOPE_CROSS_WRITE`; no spec cites `SCOPE_CROSS_WRITE`. |
| EH-D7 | Phase 1 SA-ENV-01 gave `SRV_INTERNAL` for an unregistered Service API error, and SA-ERR-01 said the Service API shares `SRV_`. | phase-1-analysis.md SA-ENV-01 gives `SVC_INTERNAL` (ServiceApi.md Internal Logic 4); SA-ERR-01 says `SRV_` is shared by wrightrun's carried codes (Caller API), the Remote API's routing refusals and `SRV_BAD_CURSOR` (Shared Types), matching *Error-code prefixes and owners*. |
| EH-D8 | Seven 2C `Run` entity codes sat under wrightrun's carried prefix `RUN_`. | phase-2c-shared-types-domain.md `Run` validation table and SessionSupervisor.md Shared Context use `RUNST_ABANDONED_NOT_REMOTE`, `RUNST_CACHE_EXCEEDS_TOKENS`, `RUNST_ENDED_AT_MISMATCH`, `RUNST_END_REASON_TOO_LONG`, `RUNST_INPUTS_TOO_MANY`, `RUNST_LIMIT_OUT_OF_RANGE`, `RUNST_NO_REPLY_NOT_SLACK`; the registry rows are renamed and sourced `2C`; `RUN_` is the Run Queue's alone. |
| EH-D13 | `CTL_UNKNOWN_ARGUMENT` appeared in the message of `LOAD_BUILD_FAILED` with no owner. | AgentLoader.md Error Table row "The file tools or the rules cannot be built" now shows the Rules Engine's registered `RULE_UNKNOWN_ARGUMENT` (raised by `load_rule_set`, RulesEngine.md A.2) as the passed-through code; no spec cites `CTL_UNKNOWN_ARGUMENT`. |

No registry defect is open. Apart from the ones resolved above, no code was found with two different meanings in two owners: every code listed by more than one spec either belongs to one owner and is passed through (105 codes), or is a sanctioned shared code (`SRV_NO_ROUTE`, `SRV_METHOD_NOT_ALLOWED`, `SRV_REFUSED` on the Caller API and the Remote API; `HOOK_APPROVAL_PENDING` at both ends of the hook protocol). The two-owner prefixes left (`APPR_`, `BLOCK_`, `HOOK_`, `LIFE_`, `MAIL_`, `SRV_`, `START_`, `SVC_`, `VOCAB_`) are each sanctioned by the note in *Error-code prefixes and owners*; `RUN_` now has one owner, the Run Queue.

## Gaps

None.
