# SPEC: Service API

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-103) · amended January 2026 (ADR-104; promoted to Approved by the user January 2026) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-108; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (`knowledge.publish_to_confluence` takes the Knowledge Service's shape; renamed result types `DictationAvailability`, `ProjectCheckResult`, `ProjectFetchOutcome`, `SharePointItem`, `SavedAttachment`; the catalogue unit test counts 400) · amended January 2026 (ADR-115) · amended 2026-01-10 (as-built readings) · amended January 2026 (`workspaces.confluence_publishing`, SVC-AUTO-1; Approved by the user 2026-01-10) · amended January 2026 (commands the screens wait on, SVC-SCR-1 to SVC-SCR-6; Approved by the user 2026-01-10) · amended January 2026 (OQ-SCR-1 and OQ-SCR-2 resolved: `crews.charter`, SVC-SCR-7, waiting on G-SCR-1; the sources of `workspaces.tree`, SVC-SCR-8; Approved by the user 2026-01-10) · amended January 2026 (`crews.charter` enters the catalogue, SVC-SCR-9, closing G-SCR-1; Approved by the user 2026-01-10, "proceed with recommendations") · amended 2026-01-28 (the memory models download on first use, SVC-MD-1 to SVC-MD-3; Approved by the user 2026-01-28) · amended January 2026 (starting an agent's task from its form, SVC-TF-1 and SVC-TF-2, ADR-119; Approved by the user January 2026) · amended January 2026 (ADR-118, ADR-120: facts as triples, words, conflicts, passages and the memory diagram; Approved by the user January 2026) · amended January 2026 (a team workspace: ten `team_knowledge.*` commands, SVC-TW-1 to SVC-TW-4, ADR-121; Approved by the user January 2026) · amended January 2026 (`app.open_web_page`, SVC-WEB-1; `ServerView.address`, SVC-SRV-1; user decision) · amended January 2026 (the page `claude_code_install`, SVC-WEB-1; no sentence sends the person to IT; user decision) · amended January 2026 (user decision; Approved by the user January 2026) (`BlockAnswer` takes a dismissal, SVC-DIS-1)
> **Mode:** greenfield
> **derivedFromHld:** 0.69.0
> **Crate:** `ordin-service-api` · **Build phase:** 1 · **Programs:** D P A
> **Dependencies:** Shared types

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
  - [The `ServiceApi` trait and call context](#the-serviceapi-trait-and-call-context)
  - [Command catalogue, version 1.0](#command-catalogue-version-10)
  - [Streams outside the catalogue](#streams-outside-the-catalogue-service_streams)
  - [Command input and result types](#command-input-and-result-types)
  - [Event catalogue](#event-catalogue)
  - [Envelope functions](#envelope-functions)
  - [Pairing route types](#pairing-route-types)
  - [Block catalogue and its check](#block-catalogue-and-its-check)
  - [Schema and TypeScript generation](#schema-and-typescript-generation)
  - [Example](#example)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Gaps](#gaps)
- [Amendment January 2026 — Slack for working together (ADR-102)](#amendment-january-2026--slack-for-working-together-adr-102)
- [Amendment January 2026 — Team knowledge (ADR-103)](#amendment-january-2026--team-knowledge-adr-103)
- [Amendment January 2026 — Page links (ADR-104)](#amendment-january-2026--page-links-adr-104)
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)
- [Amendment January 2026 — Reading other teams' knowledge (ADR-110)](#amendment-january-2026--reading-other-teams-knowledge-adr-110)
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)
- [Amendment January 2026 — Knowledge agents and the Open Knowledge Format (ADR-112, ADR-113)](#amendment-january-2026--knowledge-agents-and-the-open-knowledge-format-adr-112-adr-113)
- [Amendment January 2026 — Releases and updates (ADR-114)](#amendment-january-2026--releases-and-updates-adr-114)

## Purpose

`ordin-service-api` is the one definition of what ordin's screens can ask a backend and what a backend tells them (ADR-096 item 2). It holds the versioned command catalogue and the `ServiceApi` trait with one method per command (ADR-100 item 1; the two programs implement it), the command and event envelopes, the event catalogue and its decoding from the Journal's records, the block catalogue with the check every block passes before it is drawn (ADR-028, ADR-067, ADR-068, ADR-070), and the generator of the JSON Schemas and the TypeScript types the screens module is written against (SA-API-02, 2B-1 rule 6). Both transports — the Command Bridge on Tauri IPC and the Remote API over HTTPS — call the same envelope functions in this crate, so a command is checked the same way whichever transport carries it, and the screens never know which backend they talk to.

## Approach

The catalogue is written once as data in a declarative macro (`service_catalogue!`); the macro generates the trait, the static command list, the dispatcher, a callback macro (`for_each_command!`) the transports use to generate their routes and Tauri commands, and the schema registry the generator walks. A command's name `<module>.<verb>`, its IPC name `<module>_<verb>` and its REST path `POST /v1/<module>/<verb>` are derived mechanically, so the transports cannot drift (SA-API-01). The crate does no I/O except the generator binary; reference resolution inside the block check is delegated to a trait the caller implements. Error prefix `SVC_` for the envelope, `BLOCK_` for the block check (2C wire §0). Rejected: *a hand-written trait plus a separate hand-written route table* — two lists of 350 commands drift; *generating TypeScript with `ts-rs` or `specta`* — not in 1A or 05, and SA-API-02 already fixes generation from the JSON Schema, which this crate walks with its own generator.

Reconciliation assumptions (decision 80, rulings R1, R6, R7):

- **SA-RECON-01** — R1: the catalogue adopts each owning module's command list; owners keep their names, inputs and results, this spec holds the merged table with the owner per group, and the conversation group is `ordin` because the Ordin Host (its owner) names it so (X5).
- **SA-RECON-02** — Owners' command types are restated in `ordin_service_api::commands::<module>` because this phase-1 crate cannot depend on module crates (2B-3); unnamed inline inputs and results take the mechanical names `<Module><Verb>Input` / `<Module><Verb>Result`; a contract test in each program crate keeps the two forms equal.
- **SA-RECON-03** — Scope follows the owner where it states the transport (Identity and Credentials, Setup Assistant and Localisation name the Remote API, so their commands are `Any`; Dictation and Brand are desktop-only, so `Local`); commands that take a path on this computer (`files.add_place`, `files.import`), the desktop's update offer (`setup.update_offer`, `setup.install_update`), edit-in-app, Confluence (no Confluence Adapter on the server, 2B-3) and code commands (OQ-60) are `Local`.
- **SA-RECON-04** — Event payloads added in 1.0 take their emitting owner's shape (`locale_changed` Localisation's, `brand_changed` Brand's, `setup_progress` Setup Assistant's, `sign_in_changed` Identity and Credentials').
- **SA-CLOSE-01** — The **Check it** commands are the Checker's own (`checker.check_installed`, `checker.check_library_item`, `checker.report`, Checker.md *Service API commands*); this catalogue restates them in a group of their own with owner Checker (closes O165).
- **SA-CLOSE-02** — `ServiceEvent::SignInChanged` names its credential kind `credential_kind`, because `kind` is the tag of every `ServiceEvent` and a payload field of the same name would collide in the serialised envelope (*chosen here*; IdentityAndCredentials.md uses the same name).
- **SA-CLOSE-03** — The File Service is not a `workspace_changed` emitter; the emitters are the Workspace Service, the Package Catalogue and the Ordin Host.
- **SA-CLOSE-04** — Identity and Credentials adds `identity.save_gitlab_token` (a pasted GitLab personal access token when the profile names no GitLab OAuth application) and `identity.import_client_certificate` (a client certificate imported once from PEM files, desktop only); both are its commands, restated here.
- **SA-CLOSE-06** — The installed agent's **What it's made of**, **Show the installed files** and **Read it** views (ADR-093) are the Package Inspector's own commands (`agents.made_of`, `agents.installed_files`, `agents.part_page`; PackageInspector.md *Service API commands*), restated in a group of their own with owner Package Inspector; `catalogue.made_of` stays the Catalogue Service's command for a library `PartId`. The screen never sends a path: the program's `ServiceApi` implementation fills `folder_path` with `PackageCatalogue::get(id)` → `instance.agent_folder_path` and `bundle_dir` with `ResolverAndCompiler::open_bundle(installers.record(id).plan_hash)` → `Bundle.dir` before calling `made_of`, `installed_files` or `part_page`.
- **SA-CLOSE-05** — A hosted agent's approver card (03 *A desktop uses a hosted agent by role*) is pressed through `hosted.decide_card`, owner the Hosted Agent Adapter (`HostedAgentAdapter.md`) through the desktop program, which calls `HostedAgentAdapter::decide_approver_card` (*chosen here*). The agent is named by the adapter's own identifier type `RemoteAgentId`, the approval by `ApprovalId` and the decision by `ApproverDecision`, both as the adapter defines them; the result `state` is `approved` or `rejected`, from the decision the agent accepted. Scope `Local`: the hosted agent checks the person's own company sign-in, which only the desktop holds.
- **SA-CLOSE-07** — *(chosen here)* A project's check commands are set by the engineer on the project's settings screen through `projects.set_check_commands { project_id, commands: Vec<String> }` → `Project`, owner the Project Service (`ProjectService.md` SA-CLOSE-03), scope `Any`, effect `Change`. The command calls `ProjectService::set_check_commands`, which writes the list to the Project Service's own `projects.check_commands` column; a project with none set runs no checks. The organisation profile carries no check-commands key, and no program sets them.
- **SA-RECON-05** — R6: the block check is offered as free functions and as one injectable trait, `BlockCatalogueCheck`, because the MCP Front and the Ordin Host both want it injected (reconciliation rows 146, 147); the chat text form of a block is the Channel Gateway's (row 149), so this crate has no `block_text`.

## Shared Context

### From 2C wire §1 (types defined in this crate, copied verbatim)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServiceApiVersion {
    pub major: u16,
    pub minor: u16,
}

impl ServiceApiVersion {
    pub const CURRENT: ServiceApiVersion = ServiceApiVersion { major: 1, minor: 0 };

    /// ADR-096 item 2: a desktop works with a server of the same major version
    /// and the same or the previous minor version.
    pub fn desktop_accepts(desktop: Self, server: Self) -> bool {
        desktop.major == server.major
            && (server.minor == desktop.minor || server.minor + 1 == desktop.minor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CommandEnvelope<I> {
    pub api_version: ServiceApiVersion,
    /// Chosen by the caller; repeated on the reply.
    pub request_id: CommandRequestId,
    /// `<module>.<verb>` in snake_case, for example `approval.decide`.
    pub command: CommandName,
    /// Which backend the command is for.
    pub backend: BackendId,
    pub input: I,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandName(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CommandReply<O> {
    pub request_id: CommandRequestId,
    #[serde(flatten)]
    pub outcome: CommandOutcome<O>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandOutcome<O> {
    Ok { result: O },
    Error { error: ServiceError },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServiceError {
    /// Stable code, never shown to the person (07 — Error handling).
    pub code: String,
    /// One plain-language sentence in the person's locale, built by Localisation.
    pub message: String,
    /// At most one repair action (07 — Error handling).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<RepairAction>,
    /// The Diagnostics line identifier, shown as "Reference r-81" under an error that offers a
    /// repair (11 §6, UX-A70); never a path, token or internal state (2C wire §1, decision 80: C-30).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EventEnvelope {
    pub api_version: ServiceApiVersion,
    pub backend: BackendId,
    /// The backend's journal sequence number: strictly increasing per backend, starting at 1,
    /// never reused. On SSE it is also the `id:` line.
    pub seq: u64,
    pub at: Timestamp,
    #[serde(flatten)]
    pub event: ServiceEvent,
}
```

`ServiceEvent` is given in full under *Event catalogue*, with the 2C variants first and unchanged.

### From `ordin-types` (SharedTypes spec)

- Identifiers (UUIDv7 newtypes, 36-character lowercase hyphenated, `ID_NOT_UUIDV7`): `BackendId`, `CommandRequestId`, `BlockId`, `RunId`, `TaskId`, `ApprovalId`, `WorkspaceId`, `PlaceId`, `ArtefactId`, `CrewRunId`, `CrewCharterId`, `WorkflowId`, `ReportId`, `DashboardId`, `KnowledgePageId`, `MemoryFactId`, `EmailDraftId`, `EmailLinkId`, `AgentInstanceId`, `AgentRepositoryId`, `AgentDraftId`, `HandoverId`, `PendingPushId`, `TelegramLinkId`, `TelegramDraftId`, `ServerConnectionId`, `DesktopPairingId`, `ProjectId`, `ChangeSetId`, `CodingTaskId`, `FileVersionId`, `RemoteAgentId`.
- `Timestamp` (RFC 3339, UTC, three fractional digits, `Z`), `DecimalString`, `Sha256Hex` (64 lowercase hex), `SetBy` (`person`, `company`), `ordin_types::paging::{Page<T>, PageRequest, Cursor}` (`{"items": [...], "next_cursor": "<opaque>" | null}`, page size 100), `CodedError`, `RepairAction` (`try_again`, `sign_in_again`), `EventKind`, `AgentRef`.
- `wire::run`: `RunProgress { line, count: Option<ProgressCount> }`, `ProgressCount { done, total: Option<u64> }`, `StepMark { Next, Now, Done, Skipped }`, `RunTarget { AgentInstance { agent_instance_id } | RemoteAgent { remote_agent_id } }`, `AdapterKind`, `AdapterVerb`.
- `wire::approval`: `ApprovalCard`, `ApprovalPress { card_ref: CardRef, decision: PressDecision }`, `PressDecision { Approve, Reject }`, `CardRef` (22 characters base64url), `ContentRef { Artefact { artefact_id } | PlaceFile { place_id, relative_path } }`.
- `wire::startup`: `StartUpSubject { Run { run_id } | CrewRun { crew_run_id } }`, `StartUpReportItem`.
- Domain entities passed through unchanged as command results (2C part 1; fields not restated because the Service API imposes no rule on them beyond their own 2C validation): `ExternalAction`, `ExternalActionKey`, `Artefact`, `RemoteAgent`, `BrandPackId`, `Workspace`, `Place`, `HomeMarks`, `Run`, `RunState` (`queued`, `running`, `paused`, `completed`, `failed`, `stopped_at_limit`, `cancelled`, `interrupted`, `abandoned`, `no_reply`), `RunStep`, `CrewRun`, `CrewRunState` (`starting`, `running`, `paused`, `finished`, `escalated`, `stopped_at_limit`, `killed`, `failed`), `CrewCharter`, `Workflow`, `Report`, `Dashboard`, `KnowledgePage`, `MemoryFact`, `EmailDraft`, `AgentInstance`, `AgentRepository`, `AgentDraft`, `Handover`, `PendingPush`, `FileVersion`, `KeptTelegramMessage`, `TelegramDraft`, `ChangeSet`, `ServerConnectionState` (`connected`, `not_connected`, `disconnected`, `refused_for_version`), `ApprovalState` (`pending`, `approved`, `rejected`, `expired`).

### Validation rules copied from 2C wire §1

| Field | Rule | Error code |
|---|---|---|
| `ServiceApiVersion.major` / `minor` | 1–999 / 0–999 | `SVC_VERSION_MALFORMED` |
| version pair | `desktop_accepts(desktop, server)` | `SVC_VERSION_UNSUPPORTED` |
| `BackendId` | UUID v7; names the local core or a server this desktop holds | `SVC_BACKEND_ID_MALFORMED`, `SVC_BACKEND_UNKNOWN` |
| whole command body | at most 1 MiB of JSON | `SVC_BODY_TOO_LARGE` |
| `request_id` | UUIDv7 | `SVC_REQUEST_ID_MALFORMED` |
| `command` | `^[a-z][a-z0-9_]{0,30}\.[a-z][a-z0-9_]{0,30}$` and in this version's catalogue | `SVC_COMMAND_UNKNOWN` |
| `input` | matches the command's input schema exactly, unknown fields refused | `SVC_INPUT_INVALID` |
| `ServiceError.code` | `^[A-Z]{2,6}_[A-Z0-9_]{2,60}$`, registered | emitter defect, caught by test |
| `ServiceError.message` | 1–300 characters, one sentence, no code, path, stack or tool output | emitter defect |
| `ServiceError.reference` | 1–64 characters, the identifier of the Diagnostics line that recorded the error | emitter defect |
| event `seq` | > last `seq` from that backend | `SVC_EVENT_GAP` (desktop diagnostic) |
| `Last-Event-ID` older than the journal keeps | one `reset` event | `SVC_RESUME_TOO_OLD` (diagnostic) |
| serialised event `data` | at most 256 KiB; larger is sent as a reference fetched by command | `SVC_EVENT_TOO_LARGE` |
| `RunProgress.line` | 1–160 characters | `SVC_EVENT_INVALID` |
| `ProgressCount` | `total` absent, or `done ≤ total` | `SVC_EVENT_INVALID` |
| `StepMarked.position` | 1–7 | `SVC_EVENT_INVALID` |
| `ApprovalDecided.decided_on` | 1–80 characters | `SVC_EVENT_INVALID` |
| Remote API status | `ok` → 200; `SVC_INPUT_INVALID` → 422; `SVC_UNAUTHENTICATED` → 401; `SVC_VERSION_UNSUPPORTED` → 409; `SVC_RATE_LIMITED` → 429; any other error → 200 with `kind: "error"` | — |

Versioning (2C wire §1): a minor increment adds commands, events, optional fields or enum variants an older reader may ignore; anything else is a major increment and a new `/v2/` prefix. The block catalogue is versioned with the Service API minor version.

### Libraries

`serde` 1, `serde_json` 1, `schemars` 1.2.3 (SA-API-02), `async-trait` 0.1, `tokio-util` 0.7 (`CancellationToken`), `thiserror`, `serde_path_to_error` 0.1.17 *(the JSON Pointer of a failing block field; HLD 05, row Utility and cryptography crates)*. The generator binary adds nothing beyond these.

## Public Interface

### The `ServiceApi` trait and call context

```rust
/// Where a command came from. The transports set it; a module never trusts a value from input.
#[derive(Debug, Clone)]
pub enum CallOrigin {
    /// The desktop's own screens, over Tauri IPC.
    LocalScreens,
    /// A paired desktop, over the Remote API (personal profile only).
    PairedDesktop { desktop_pairing_id: DesktopPairingId, computer_name: String },
}

/// Per-command context.
#[derive(Debug, Clone)]
pub struct CallContext {
    pub request_id: CommandRequestId,
    /// The request id as a string; carried into every log line of the call.
    pub correlation_id: String,
    pub origin: CallOrigin,
    /// The time by which the reply must be sent (see Internal Logic step 3).
    pub deadline: std::time::Instant,
    /// Cancelled when the transport drops the call (window closed, stream closed).
    pub cancel: tokio_util::sync::CancellationToken,
}

/// Turns a code into the one sentence and the action the person sees (implemented by the
/// program with Localisation; ADR-100 hub pattern).
pub trait ErrorSentences: Send + Sync {
    fn sentence(&self, code: &str) -> String;
    fn action(&self, code: &str) -> Option<RepairAction>;
}

/// Implemented once by each program (desktop, ordin server); one method per command of the
/// catalogue below, generated by `service_catalogue!`. Each method returns the command's result
/// or a `ServiceError` whose message the implementation built with `ErrorSentences`.
#[async_trait::async_trait]
pub trait ServiceApi: Send + Sync + 'static {
    // One method per catalogue row, in catalogue order, of the form:
    // async fn <module>_<verb>(&self, ctx: &CallContext, input: <Input>) -> Result<<Result>, ServiceError>;
    // for example:
    async fn approval_decide(&self, ctx: &CallContext, input: ApprovalPress)
        -> Result<ApprovalOutcome, ServiceError>;
    async fn runs_stop(&self, ctx: &CallContext, input: RunsStopInput) -> Result<Done, ServiceError>;
    // … every row of the catalogue …
}

/// Static description of one command.
#[derive(Debug, Clone, Copy)]
pub struct CommandMeta {
    pub name: &'static str,        // "approval.decide"
    pub module: &'static str,      // "approval"
    pub verb: &'static str,        // "decide"
    pub ipc_name: &'static str,    // "approval_decide"
    pub rest_path: &'static str,   // "/v1/approval/decide"
    pub scope: Scope,
    pub effect: Effect,
    pub budget: Budget,
    pub since: ServiceApiVersion,  // 1.0 for every row below
    pub input_schema: fn(&mut schemars::SchemaGenerator) -> schemars::Schema,
    pub result_schema: fn(&mut schemars::SchemaGenerator) -> schemars::Schema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Served by any backend: the desktop's own core and a personal server's Remote API.
    Any,
    /// Served only to `CallOrigin::LocalScreens` (desktop-only features, OQ-60).
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect { Read, Change }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    /// A-03 / A-46: 150 ms p95.
    Interactive,
    /// A-21: first results within 1 s (`search.query`, `files.search`, `catalogue.search`, `catalogue.facets`).
    Search,
    /// A transfer of up to 25 MiB (`files.download`); deadline 60 s (Internal Logic step 3).
    Transfer,
}

/// The catalogue, in table order.
pub static CATALOGUE: &[CommandMeta];

/// Looks a command up by its dotted name.
pub fn command(name: &str) -> Option<&'static CommandMeta>;

/// Calls the trait method a command names, with input already checked.
pub async fn dispatch(
    api: &dyn ServiceApi,
    ctx: &CallContext,
    meta: &'static CommandMeta,
    input: serde_json::Value,
) -> Result<serde_json::Value, ServiceError>;

/// Invokes `$callback! { (module, verb, method, Input, Result, scope, effect); … }` once with
/// every catalogue row. The Command Bridge generates its Tauri commands and capability list
/// with it; the Remote API needs only `CATALOGUE`.
#[macro_export]
macro_rules! for_each_command { ($callback:ident) => { /* generated */ } }
```

The empty result:

```rust
/// Serialises as `{}`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Done {}
```

### Command catalogue, version 1.0

The catalogue is the merged list of every module's commands (ruling R1, SA-RECON-01): each module owns its command names, inputs and results, and this table restates them unchanged with the owner named per group. Every command is new in 1.0. The program's `ServiceApi` method for a row calls the method of the same verb on the owner's service interface (or the method named in *Note*). Budget is `Interactive` unless *Note* says otherwise. Commands of scope `Local` are refused over the Remote API with `SVC_COMMAND_LOCAL_ONLY`. Effect `Read` changes nothing; `Change` changes stored state or starts work.

**Called by.** Every command is sent by the screens module (`ScreensModule.md`) — on the desktop through the Command Bridge (`CommandBridge.md`, Tauri IPC `<module>_<verb>`), for a server's backend through the Server Connector and the Remote API (`RemoteApi.md`, `POST /v1/<module>/<verb>`). The desktop program (`OrdinDesktop.md`) and the server program (`OrdinServer.md`) implement `ServiceApi` and call the owner. On the personal server the Channel Gateway also reaches the `handover.*` commands from a chat (HandoverService.md). No module calls another module through this catalogue.

**Inputs written as `{ field, … }`.** Where the owner's spec gives an inline object or a method signature rather than a named type, the input is a JSON object with one field per listed name, spelt exactly as the owner spells it (a method parameter `ws` is the field `ws`); `field?` and `Option<T>` are optional; text fields without a type are `String`. Results written `{}` are `Done`. The Rust name of an unnamed input or result is derived mechanically: `<Module><Verb>Input` / `<Module><Verb>Result` in PascalCase (`files.preview` → `FilesPreviewInput`) (SA-RECON-02).

`workspaces.tree` is composed by the program from the Workspace Service, the Ordin Host, the Knowledge Service and the Memory Service (WorkspaceService.md); its input and result are this spec's types. `diagnostics.lines` and `diagnostics.support_export` are the Journal's commands over this spec's `DiagnosticLine`, `SupportExport` and `SavedCopy` types.

#### Backend — owner program (`OrdinDesktop.md`, `OrdinServer.md`)

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `service.hello` | `Done` | `ServiceHello` | Any | Read |  |

#### This program: quit, folder picker, settings, save a copy — owner program (`OrdinDesktop.md`)

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `app.quit` | `Done` | `AppQuitResult { stopping_sessions: u32 }` | Local | Change |  |
| `app.pick_folder` | `Done` | `AppPickFolderResult { folder: Option<String> }` | Local | Read |  |
| `settings.get` | `SettingsSectionRef` | `SettingsView` | Local | Read | routes by key owner, 2D-3 |
| `settings.change` | `ChangeSetting` | `SettingView` | Local | Change | routes by key owner, 2D-3 |
| `files.save_copy` | `SaveCopy` | `SavedCopy` | Local | Change | dialog, then `files.download` on the source backend, then a local write |
| `files.save_export` | `{ file_name, text }` (file name 1–255 characters, no path separator; `text` ≤ 8 MiB of UTF-8) | `SavedCopy` | Local | Change | the save dialog, then a local write of `text`; how the screens write the result of `workflows.export` and `localisation.export_language` (amended January 2026, ADR-109); 9 MiB body |
| `app.open_slack_link` | `{ url }` | `{}` | Local | Read | opens a Slack permalink the core produced; refused unless `https://<name>.slack.com/archives/…` (OrdinDesktop.md) |
| `app.open_web_page` | `OpenWebPage { page: WebPage }` | `{}` | Local | Change | person-only; opens one page ordin names (`telegram_botfather`, `atlassian_api_tokens`, `claude_code_install`) in the person's browser; never a free address (SVC-WEB-1; amended January 2026 (user decision; Approved by the user January 2026); `claude_code_install` amended January 2026 (user decision; Approved by the user January 2026)) |
| `app.agent_settings` | `{}` | `AgentSettingsView` | Local | Read | the person's own harness, model per level and effort per kind of work, with each default and each harness's readiness (SVC-AGM-1; amended January 2026; user decision; Approved by the user January 2026) |
| `app.change_agent_setting` | `ChangeAgentSetting` | `AgentSettingsView` | Local | Change | one of those choices, or back to its default (SVC-AGM-1; amended January 2026; user decision; Approved by the user January 2026) |
| `identity.relay_sign_in` | `{ server: ServerRef, reason: String? }` | `CredentialView` | Local | Change | the desktop half of a server's relayed Slack sign-in |

#### Home and search — owner `HomeAndSearch.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `home.view` | `{ workspace_id: WorkspaceId? }` | `HomeView` | Any | Read |  |
| `home.opened` | `{}` | `{}` | Any | Change |  |
| `home.workspace_opened` | `{ workspace_id }` | `{}` | Any | Change |  |
| `home.changed_marks` | `{ workspace_id }` | `Vec<ChangedMark>` | Any | Read |  |
| `home.document_opened` | `{ workspace_id, item: ContentRef }` | `{}` | Any | Change |  |
| `home.run_again_inputs` | `{ subject: RunAgainSubject }` | `Vec<RunAgainInput>` | Any | Read |  |
| `home.needs_you` | `{ workspace_id: WorkspaceId? }` | `Vec<HomeRow>` | Any | Read |  |
| `search.query` | `{ query, current_workspace_id: WorkspaceId? }` | `SearchAnswer` | Any | Read | budget `Search` |

#### Workspaces — owner `WorkspaceService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `workspaces.create` | `{ name }` | `Workspace` | Any | Change |  |
| `workspaces.rename` | `{ workspace_id, name }` | `Workspace` | Any | Change |  |
| `workspaces.archive` | `{ workspace_id }` | `Workspace` | Any | Change |  |
| `workspaces.restore` | `{ workspace_id }` | `Workspace` | Any | Change |  |
| `workspaces.bin_impact` | `{ workspace_id }` | `BinImpact` | Any | Read |  |
| `workspaces.move_to_bin` | `{ workspace_id }` | `Workspace` | Any | Change |  |
| `workspaces.restore_from_bin` | `{ workspace_id }` | `Workspace` | Any | Change |  |
| `workspaces.delete_from_bin` | `{ workspace_id }` | `{}` | Any | Change |  |
| `workspaces.empty_bin` | `{}` | `{ deleted: u32 }` | Any | Change |  |
| `workspaces.list` | `{ filter: WorkspaceListFilter }` | `Vec<Workspace>` | Any | Read |  |
| `workspaces.open` | `{ workspace_id }` | `OpenReport` | Any | Change |  |
| `workspaces.links` | `{ workspace_id }` | `Vec<SourceLink>` | Any | Read |  |
| `workspaces.add_link` | `{ workspace_id, location: LinkLocation, display_name: String? }` | `SourceLink` | Any | Change |  |
| `workspaces.confirm_link` | `{ source_link_id }` | `SourceLink` | Any | Change |  |
| `workspaces.decline_link` | `{ source_link_id }` | `{}` | Any | Change |  |
| `workspaces.remove_link` | `{ source_link_id }` | `{}` | Any | Change |  |
| `workspaces.link_progress` | `{ source_link_id }` | `LinkProgress` | Any | Read |  |
| `workspaces.set_grant` | `{ workspace_id, grantee: Grantee, place_id, access: FolderAccess? }` | `Option<FolderGrant>` | Any | Change |  |
| `workspaces.grants` | `{ workspace_id }` | `Vec<FolderGrant>` | Any | Read |  |
| `workspaces.propose_confluence_link` | `{ workspace_id, place_id, folder, location: ConfluenceLocation }` | `ConfluenceLink` | Any | Change |  |
| `workspaces.pause_confluence_link` | `{ confluence_link_id }` | `ConfluenceLink` | Any | Change |  |
| `workspaces.resume_confluence_link` | `{ confluence_link_id }` | `ConfluenceLink` | Any | Change |  |
| `workspaces.remove_confluence_link` | `{ confluence_link_id }` | `{}` | Any | Change |  |
| `workspaces.email_links` | `{ workspace_id }` | `Vec<EmailLink>` | Any | Read |  |
| `workspaces.add_email_link` | `{ workspace_id, kind, folder_or_label?, addresses, domains, display_name }` | `EmailLink` | Any | Change |  |
| `workspaces.remove_email_link` | `{ email_link_id }` | `{}` | Any | Change |  |
| `workspaces.set_email_grant` | `{ workspace_id, agent_instance_id, access: EmailAccess? }` | `Option<EmailGrant>` | Any | Change |  |
| `workspaces.set_telegram_grant` | `{ workspace_id, agent_instance_id, access: TelegramAccess? }` | `Option<TelegramGrant>` | Any | Change |  |
| `workspaces.telegram_grants` | `{ workspace_id }` | `Vec<TelegramGrant>` | Any | Read |  |
| `workspaces.set_slack_grant` | `{ workspace_id, agent_instance_id, access: SlackAccess? }` | `Option<SlackGrant>` | Any | Change |  |
| `workspaces.slack_grants` | `{ workspace_id }` | `Vec<SlackGrant>` | Any | Read |  |
| `workspaces.tree` | `WorkspaceRef` | `WorkspaceTree` (this spec's type) | Any | Read | program composes it from the Workspace Service, Ordin Host, Knowledge Service and Memory Service (WorkspaceService.md) |

#### Documents — owner `FileService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `files.add_place` | `{ path, friendly_name: String? }` | `Place` | Local | Change | path from the system folder picker |
| `files.rename_place` | `{ place_id, friendly_name }` | `Place` | Any | Change |  |
| `files.list_folder` | `{ place_id, folder, sort: FolderSort, cursor: Cursor? }` | `Page<FileEntry>` | Any | Read |  |
| `files.preview` | `{ place_id, path }` | `Preview` | Any | Read |  |
| `files.versions` | `{ place_id, path }` | `Vec<FileVersion>` | Any | Read |  |
| `files.restore_version` | `{ file_version_id }` | `FileVersion` | Any | Change |  |
| `files.import` | `{ place_id, folder, sources: Vec<String> }` | `Vec<FileEntry>` | Local | Change | paths from a drop event |
| `files.move_to_bin` | `{ place_id, path }` | `BinTicket` | Any | Change |  |
| `files.restore_from_bin` | `{ ticket: BinTicket }` | `FileVersion` | Any | Change |  |
| `files.save_edit` | `{ place_id, path, text }` | `FileVersion` | Any | Change | 9 MiB body |
| `files.published_copies` | `{ original: CopyOriginal }` | `Vec<PublishedCopy>` | Any | Read |  |
| `files.search` | `{ place_ids: Vec<PlaceId>, query, limit }` | `SearchResults` | Any | Read | budget `Search` |
| `files.download` | `{ place_id, path }` | `{ name, media_type, bytes_b64 }` | Any | Read | budget `Transfer` |

#### Edit in an app — owner `CraftToolkit.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `craft.open` | `{ place_id, path }` | `OpenOutcome` | Local | Change |  |
| `craft.open_with_system_default` | `{ place_id, path }` | `OpenOutcome` | Local | Change |  |
| `craft.installations` | `{}` | `Vec<CraftInstallation>` | Local | Read |  |
| `craft.conversions` | `{}` | `{ word: bool, pdf: bool, slides: bool }` | Local | Read |  |

#### Sending changes back — owner `PushService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `push.list_pending` | `{ workspace_id }` | `Vec<PendingPush>` | Any | Read |  |
| `push.choose_version` | `ChooseVersionInput` | `PendingPush` | Any | Change |  |
| `push.try_again` | `TryAgainInput` | `PendingPush` | Any | Change |  |
| `push.publish_copy` | `PublishCopyRequest` | `PublishedCopyId` | Any | Change |  |
| `push.update_copies` | `{ original: CopyOriginal, workspace_id }` | `Option<ApprovalId>` | Any | Change |  |
| `push.copy_action` | `CopyActionInput` | `PublishedCopy` | Any | Change |  |

#### Confluence — owner `ConfluenceAdapter.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `confluence.find` | `{ query (1–200), limit: u32 (1–25) }` | `Vec<ConfluenceLocation>` | Local | Read |  |
| `confluence.list` | `{ location: ConfluenceLocation, cursor: Cursor? }` | `Page<RemoteItem>` | Local | Read |  |
| `confluence.ready_to_publish` | `{ confluence_link_id }` | `Vec<ReadyItem>` | Local | Read |  |
| `confluence.publish_checked` | `{ confluence_link_id, items: Vec<ReadyItem> }` | `{ approval_id }` | Local | Change |  |
| `confluence.keep_mine` | `{ published_copy_id }` | `{}` | Local | Change |  |

#### Jira — owner `JiraAdapter.md` (amended January 2026, ADR-106)

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `jira.connect_token` | `{ site (host), email (3–254), token (1–1024) }` | `JiraUser` (no token in the result) | Local | Change | sensitive input: carries a secret in; never logged, journalled or echoed |
| `jira.find_issues` | `{ text (1–200), limit: u32 (1–25) }` | `Vec<JiraIssueSummary>` | Local | Read | the connected site |

#### SharePoint — owner `SharePointAdapter.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `sharepoint.find` | `{ query (1–200), limit: u32 (1–50) }` | `Vec<SharePointLocation>` | Any | Read |  |
| `sharepoint.list` | `{ location: SharePointLocation, cursor: Cursor? }` | `Page<SharePointItem>` | Any | Read |  |

#### Conversation with Ordin (group `ordin`, X5) — owner `OrdinHost.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `ordin.send_message` | `{ workspace_id, text, attachments?: Vec<ContentRef> }` (text 1–8,000 characters; 0–20 attachments) | `{ accepted: true, position_in_queue: u8 }` | Any | Change |  |
| `ordin.answer_block` | `{ workspace_id, block_id, answer: BlockAnswer }` | `{ accepted: true }` | Any | Change |  |
| `ordin.summarise` | `{ workspace_id }` | `{ conversation_id }` | Any | Change |  |
| `ordin.prepare_start_fresh` | `{ workspace_id }` | `{ accepted: true }` | Any | Change |  |
| `ordin.start_fresh` | `{ workspace_id, kept_points: Vec<String> }` | `{ conversation_id }` | Any | Change |  |
| `ordin.continue_conversation` | `{ workspace_id, conversation_id, kept_points: Vec<String> }` | `{ conversation_id }` | Any | Change |  |
| `ordin.list_conversations` | `{ workspace_id, cursor: Cursor? }` | `Page<ConversationSummary>` | Any | Read |  |
| `ordin.read_conversation` | `{ conversation_id, cursor: Cursor? }` | `Page<ConversationMessage>` | Any | Read |  |
| `ordin.ask_about_conversation` | `{ workspace_id, conversation_id, question }` | `{ accepted: true }` | Any | Change |  |
| `ordin.block` | `{ workspace_id, block_id }` | `ConversationBlock` | Any | Read |  |
| `ordin.bring_page_up_to_date` | `{ workspace_id, page_id: KnowledgePageId }` | `{ accepted: true }` | Any | Change |  |
| `ordin.estimate_language_draft` | `{ locale }` | `{ messages: u32, estimate: UsageEstimate? }` | Any | Read | ADR-109 |
| `ordin.draft_language` | `{ locale, name }` (name 1–60 characters) | `{ accepted: true, messages: u32 }` | Any | Change | ADR-109; one draft at a time |
| `ordin.stop_language_draft` | `{ locale }` | `{}` | Any | Change | ADR-109; no effect when no draft runs |
| `voice.get` | `{}` | `VoiceSettings` | Any | Read |  |
| `voice.update` | `{ tone?, register?, length?, name_to_use?, notes? }` | `VoiceSettings` | Any | Change |  |

#### Runs — owner `SessionSupervisor.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `runs.get` | `{ run_id }` | `RunView` | Any | Read |  |
| `runs.list` | `{ workspace_id: WorkspaceId?, cursor: Cursor? }` | `Page<Run>` | Any | Read |  |
| `runs.stop` | `{ run_id }` | `{}` | Any | Change |  |
| `runs.answer` | `{ run_id, text }` | `{}` | Any | Change |  |
| `runs.try_again` | `{ workspace_id, run_id }` | `RunsTryAgainResult` | Any | Change | Owner `OrdinHost.md` `try_again_run` (OH-TA-1); added by SVC-TA-1 |
| `runs.start_task` | `RunsStartTaskInput` | `RunsStartTaskResult` | Any | Change | Owner `OrdinHost.md` `start_task_from_form` (OH-TF-1); added by SVC-TF-1; person only |

#### Crews — owner `CrewOrchestrator.md` (get, stop, continue); `OrdinHost.md` (start, discard_plan)

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `crews.get` | `{ crew_run_id }` | `{ crew_run, rounds }` | Any | Read | Crew Orchestrator |
| `crews.stop` | `{ crew_run_id }` | `{}` | Any | Change | Crew Orchestrator |
| `crews.continue` | `{ crew_run_id }` | `{}` | Any | Change | Crew Orchestrator |
| `crews.start` | `{ crew_charter_id }` | `{ crew_run_id }` | Any | Change | Ordin Host |
| `crews.discard_plan` | `{ crew_charter_id }` | `{}` | Any | Change | Ordin Host |

#### Workflows and dashboard refresh — owner `OrdinHost.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `dashboards.refresh` | `{ workspace_id, dashboard_id }` | `DashboardRefreshStarted` | Any | Change |  |
| `workflows.list` | `{ workspace_id, cursor: Cursor? }` | `Page<WorkflowSummary>` | Any | Read |  |
| `workflows.get` | `{ workflow_id, version: u32? }` | `WorkflowView` | Any | Read |  |
| `workflows.check` | `{ workspace_id, file: WorkflowFile }` | `WorkflowCheckReport` | Any | Read |  |
| `workflows.save` | `{ workflow_id, base_version: Option<u32>, file: WorkflowFile, change_note }` | `{ version }`, or a `WorkflowCheckReport` refusal | Any | Change |  |
| `workflows.discard_draft` | `{ workflow_id }` | `{}` | Any | Change |  |
| `workflows.restore` | `{ workflow_id, version }` | `{ version }` | Any | Change |  |
| `workflows.rename` | `{ workflow_id, name }` | `{}` | Any | Change |  |
| `workflows.duplicate` | `{ workflow_id, version, name, to_workspace_id }` | `{ workflow_id }` | Any | Change |  |
| `workflows.remove` | `{ workflow_id, confirmed: bool }` | `{}` | Any | Change | a confirmation, not an approval card (D11) |
| `workflows.export` | `{ workflow_id, version }` | `{ file_name, yaml: String }` (≤ 256 KB) | Any | Read | the screens write it where the person chooses, with `files.save_export` (amended January 2026, ADR-109) |
| `workflows.import` | `{ workspace_id, file_name, yaml: String }` (file name 1–255 characters; YAML ≤ 256 KB) | `{ workflow_id, check: WorkflowCheckReport }` | Any | Change |  |
| `workflows.list_in_repositories` | `{ workspace_id }` | `Vec<RepositoryWorkflowFile>` | Any | Read |  |
| `workflows.add_from_repository` | `{ workspace_id, host, repository, folder, file_name }` | `{ workflow_id, check: WorkflowCheckReport }` | Any | Change |  |
| `workflows.share` | `{ workflow_id, version, folder: RepositoryFolder }` | `{ submission_id }` | Any | Change |  |
| `workflows.send_back` | `{ workflow_id }` | `{ submission_id }` | Any | Change |  |
| `workflows.use_newer` | `{ workflow_id }` | `{ version }` | Any | Change |  |
| `workflows.keep_as_own` | `{ workflow_id }` | `{}` | Any | Change |  |
| `workflows.run` | `{ workflow_id, version: Option<u32>, request: Option<String> }` | `{ crew_charter_id }` | Any | Change | shows the crew plan; R11's immediate start is in-process only |

#### Approvals — owner `ApprovalService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `approval.decide` | `ApprovalPress` | `ApprovalOutcome` | Any | Change |  |
| `approval.list_pending` | `{ workspace_id: WorkspaceId \| null, cursor: Cursor \| null }` | `Page<ApprovalRecord>` | Any | Read |  |
| `approval.get` | `{ approval_id }` | `ApprovalRecord` | Any | Read |  |

#### Journal, artefacts, reports, dashboards, plans, history, diagnostics — owner `JournalAndArtefactStore.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `journal.list_entries` | `{ filter: EntryFilter, cursor: Cursor? }` | `Page<JournalEntry>` | Local | Read |  |
| `journal.list_external_actions` | `{ state: "unknown_outcome"?, cursor: Cursor? }` | `Page<ExternalAction>` | Any | Read |  |
| `journal.mark_arrived` | `{ action_key: ExternalActionKey }` | `ExternalAction` | Any | Change |  |
| `artefacts.list` | `{ filter: ArtefactFilter, cursor: Cursor? }` | `Page<Artefact>` | Any | Read |  |
| `artefacts.versions` | `{ location: ArtefactLocation, path }` | `{ items: Vec<Artefact> }` | Any | Read |  |
| `reports.get` | `{ report_id }` | `Report` | Any | Read |  |
| `reports.list` | `{ workspace_id, cursor: Cursor? }` | `Page<ReportSummary>` | Any | Read |  |
| `dashboards.list` | `{ workspace_id, cursor: Cursor? }` | `Page<Dashboard>` | Any | Read |  |
| `dashboards.keep` | `NewDashboard` | `Dashboard` | Any | Change |  |
| `dashboards.remove` | `{ dashboard_id }` | `{}` | Any | Change |  |
| `plans.get` | `{ run_id }` | `{ items: Vec<RunStep> }` | Any | Read |  |
| `history.get` | `{}` | `RetentionSetting` | Local | Read |  |
| `history.set_days` | `{ days: u32 }` | `RetentionSetting` | Local | Change |  |
| `journal.run_external_actions` | `{ run_id: RunId }` | `{ items: Vec<ExternalAction> }` | Any | Read |  |
| `diagnostics.lines` | `PageRequest` | `Page<DiagnosticLine>` (this spec's type) | Local | Read |  |
| `diagnostics.support_export` | `SupportExport` | `SavedCopy` (this spec's type) | Local | Change | the desktop program writes the bytes to the path chosen in the save dialog |
| `diagnostics.clear_logs` | `{}` | `LogsCleared` (this spec's type) | Local | Change | person-only; deletes the past log files and empties the current one, then empties the Diagnostics event list from now on (SVC-LOG-1; JournalAndArtefactStore.md JA-DIAG-CLEAR-1; amended January 2026 (user decision; Approved by the user January 2026)) |

#### Hosted agents' approver cards — owner `HostedAgentAdapter.md`, through the desktop program (`OrdinDesktop.md`)

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `hosted.decide_card` | `{ server_or_agent: RemoteAgentId, approval_id: ApprovalId, decision: ApproverDecision, note: String? }` | `{ state: "approved" \| "rejected" }` | Local | Change | `decision` is `approve` or `reject`; `note` 0–2,000 characters, never logged; the program calls `HostedAgentAdapter::decide_approver_card` (SA-CLOSE-05) |

#### Organisation profile and remote agents — owner `OrganisationProfile.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `profile.status` | `{}` | `ProfileStatus` | Local | Read |  |
| `profile.refresh` | `{}` | `RefreshOutcome` | Local | Change |  |
| `profile.get_document` | `{}` | `{ source: "verified" \| "defaults", profile_version: String?, document: OrganisationProfileDocument }` | Local | Read |  |
| `remote_agents.list` | `{}` | `{ items: Vec<RemoteAgent> }` | Any | Read |  |

#### Knowledge — owner `KnowledgeService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `knowledge.list` | `{ workspace_id, filter? }` | `PageListing` | Any | Read |  |
| `knowledge.get` | `{ page_id }` | `PageView` | Any | Read |  |
| `knowledge.read_agent_page` | `{ workspace_id, agent_instance_id, relative_path }` | `{ markdown }` | Any | Read |  |
| `knowledge.save_edit` | `{ page_id, title, body_markdown, tags?, aliases? }` | `{}` | Any | Change | 9 MiB body; `tags`, `aliases` added January 2026 (ADR-104) |
| `knowledge.write` | `{ workspace_id, title, body_markdown }` | `{ page_id }` | Any | Change |  |
| `knowledge.use_update` | `{ page_id }` | `{}` | Any | Change |  |
| `knowledge.keep_page` | `{ page_id }` | `{}` | Any | Change |  |
| `knowledge.leave_it` | `{ page_id }` | `{}` | Any | Change |  |
| `knowledge.remove` | `{ page_id }` | `{}` | Any | Change |  |
| `knowledge.undo_remove` | `{ page_id }` | `{}` | Any | Change |  |
| `knowledge.copy_to_workspace` | `{ page_id, to_workspace_id }` | `{ page_id }` | Any | Change |  |
| `knowledge.publish_to_confluence` | `{ page_id, destination: CopyDestination, keep_up_to_date: bool }` | `{ published_copy_id: PublishedCopyId }` | Any | Change | *amended January 2026*: the Knowledge Service's own shape (KnowledgeService.md *Service API commands served*); a Confluence copy needs its destination |
| `knowledge.accept_suggestion` | `{ page_id, suggestion_id }` | `{}` | Any | Change | added January 2026 (ADR-104) |
| `knowledge.dismiss_suggestion` | `{ page_id, suggestion_id }` | `{}` | Any | Change | added January 2026 (ADR-104) |

#### What Ordin remembers — owner `MemoryService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `memory.list` | `{ workspace_id, cursor? }` | `FactPage` | Any | Read |  |
| `memory.get` | `{ workspace_id, ids: Vec<MemoryFactId> }` (1–100) | `{ facts: Vec<FactView> }` | Any | Read |  |
| `memory.correct` | `{ id, statement }` | `FactView` | Any | Change |  |
| `memory.forget` | `{ id }` | `ForgetReceipt` | Any | Change |  |
| `memory.remember_everywhere` | `{ id }` | `FactView` | Any | Change |  |
| `memory.keep_checked` | `{ id }` | `FactView` | Any | Change | **Keep it** on a fact waiting for a personal-data check; it may then be shared (amended January 2026, ADR-103) |

#### Email — owner `MailService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `mail.list_messages` | `ListMessages` | `MessagePage` | Any | Read |  |
| `mail.open_message` | `OpenMessage` | `MessageView` | Any | Read |  |
| `mail.save_attachment` | `SaveAttachment` | `SavedAttachment` | Any | Change |  |
| `mail.get_draft` | `{ id: EmailDraftId }` | `EmailDraft` | Any | Read |  |
| `mail.list_drafts` | `{ workspace: WorkspaceId }` | `Vec<EmailDraft>` | Any | Read |  |
| `mail.edit_draft` | `EditDraft` | `EmailDraft` | Any | Change |  |
| `mail.send` | `{ id: EmailDraftId }` | `ApprovalId` | Any | Change |  |
| `mail.mark_as_sent` | `{ id: EmailDraftId }` | `EmailDraft` | Any | Change |  |
| `mail.save_to_mail_drafts` | `{ id: EmailDraftId }` | `{}` | Any | Change |  |
| `mail.save_in_sent` | `{ id: SentEmailId }` | `SentEmail` | Any | Change |  |
| `mail.discard_draft` | `{ id: EmailDraftId }` | `{}` | Any | Change |  |
| `mail.grant` | `GrantEmail` | `EmailGrant` | Any | Change |  |
| `mail.check` | `{ workspace: WorkspaceId }` | `Vec<LinkCheckState>` | Any | Change |  |
| `mail.link_states` | `{ workspace: WorkspaceId }` | `Vec<LinkCheckState>` | Any | Read |  |
| `mail.mark_looked` | `{ link: EmailLinkId }` | `{}` | Any | Change |  |
| `mail.settings` | `{}` | `MailSettings` | Any | Read |  |
| `mail.set_settings` | `MailSettingsUpdate` | `MailSettings` | Any | Change |  |
| `mail.verify_sign_in` | `{}` | `SignInCheck` | Any | Read |  |

#### Telegram — owner `TelegramService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `telegram.status` | `{}` | `TelegramStatus` | Any | Read |  |
| `telegram.connect_bot` | `{ key }` | `BotInfo` | Any | Change | refused on a server by the module |
| `telegram.remove` | `{}` | `{}` | Any | Change |  |
| `telegram.offered_chats` | `{ ws: WorkspaceId }` | `Vec<OfferedChat>` | Any | Read |  |
| `telegram.link` | `{ ws, chat_id: i64, topic_id: Option<i64> }` | `TelegramLink` | Any | Change |  |
| `telegram.unlink` | `{ ws, link_id: TelegramLinkId }` | `{}` | Any | Change |  |
| `telegram.links` | `{ ws }` | `Vec<LinkSummary>` | Any | Read |  |
| `telegram.messages` | `{ ws, link_id, cursor: Option<String> }` | `Page<TelegramMessageOut>` | Any | Change | marks the page seen |
| `telegram.save_file` | `{ ws, link_id, message_id: i64, file_id }` | `SavedFile` | Any | Change |  |
| `telegram.try_again` | `{ ws, draft_id: TelegramDraftId }` | `ApprovalId` | Any | Change |  |

#### Slack — owner `SlackService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `slack.status` | `{}` | `SlackStatus` | Any | Read | sign-in state, whose Slack app, rules allowed, permissions still needed |
| `slack.sign_in` | `{ scopes: SlackScopeSet }` | `SignInChallenge` | Any | Change | `base`, `names` or `direct_messages`; the screen then waits with Identity's own command |
| `slack.connect_own_app` | `{ client_id }` | `SlackStatus` | Any | Change | only when the profile names no Slack app |
| `slack.offered_channels` | `{ ws: WorkspaceId }` | `Vec<OfferedSlackChannel>` | Any | Read | channels the person belongs to, not linked, within `slack.allowed_channels` |
| `slack.link` | `{ ws, channel_id }` | `SlackLink` | Any | Change |  |
| `slack.unlink` | `{ ws, link_id: SlackLinkId }` | `{}` | Any | Change | deletes the link's kept messages and drafts |
| `slack.links` | `{ ws }` | `Vec<SlackLinkSummary>` | Any | Read |  |
| `slack.messages` | `{ ws, link_id, cursor: Option<String> }` | `Page<SlackMessageOut>` | Any | Change | marks the page seen (SA-PAGE-01) |
| `slack.set_ordin_reads` | `{ ws, link_id, on: bool }` | `SlackLink` | Any | Change | "Let Ordin look at new messages" |
| `slack.rules` | `{}` | `Vec<SlackRuleView>` | Any | Read | each rule with today's count |
| `slack.add_rule` | `{ rule: NewSlackRule }` | `SlackRule` | Any | Change | sent only after the person's confirmation on the rule screen |
| `slack.turn_off_rule` | `{ rule_id: SlackRuleId }` | `SlackRule` | Any | Change |  |
| `slack.try_again` | `{ ws, post_id: SlackPostId }` | `ApprovalId` | Any | Change | a new card for a refused post |
| `slack.post_record` | `{ post_id: SlackPostId }` | `SlackPost` | Any | Read | one post's record (outcome line, handover screen) |

#### Team knowledge — owner `TeamKnowledgeService.md` (amended January 2026, ADR-103)

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `team_knowledge.status` | `{ ws }` | `TeamKnowledgeStatus` | Any | Read | header line, what is new and from whom, what would be shared, facts waiting for a check, problems |
| `team_knowledge.candidate_links` | `{ ws }` | `Vec<SourceLink>` | Any | Read | the workspace's active `GitRemote` links on GitHub or the company's GitLab |
| `team_knowledge.turn_on` | `{ ws, choice: TeamFolderChoice }` | `TeamKnowledgeStatus` | Any | Change | **Share knowledge with a team…**; `NewFolder` adds the link in the same step (*amended January 2026 (user decision; Approved by the user January 2026)*: no screen sends it now that knowledge-only sharing is not offered on its own, ScreensModule.md SM-TW-13; the command stays) |
| `team_knowledge.turn_off` | `{ ws }` | `{}` | Any | Change | **Stop sharing with the team**; local items and shared files stay |
| `team_knowledge.check_now` | `{ ws }` | `TeamKnowledgeStatus` (`busy: checking`) | Any | Read | answers at once; the check runs in the background and ends with `workspace_changed` (owner §H) |
| `team_knowledge.get` | `{ ws }` | `TeamKnowledgeStatus` (`busy: getting`) | Any | Change | **Get your team's memory and knowledge pages** (label *amended January 2026 (user decision; Approved by the user January 2026)*); answers at once; the outcome arrives in `last_get` or `last_error`, then `workspace_changed` (owner §H) |
| `team_knowledge.share` | `{ ws }` | `TeamKnowledgeStatus` (`busy: sharing`) | Any | Change | **Share Ordin's memory and knowledge pages with your team** (label *amended January 2026 (user decision; Approved by the user January 2026)*); answers at once; gets first, then raises the card; the outcome arrives in `last_share` or `last_error`, then `workspace_changed` (owner §H) |

#### Agent repositories and the library — owner `CatalogueService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `catalogue.list_repositories` | `{}` | `Vec<RepositorySummary>` | Any | Read |  |
| `catalogue.probe_repository` | `ProbeRepositoryInput` | `RepositoryProbe` | Any | Change |  |
| `catalogue.add_repository` | `AddRepositoryInput` | `AgentRepository` | Any | Change |  |
| `catalogue.rename_repository` | `RenameRepositoryInput` | `AgentRepository` | Any | Change |  |
| `catalogue.remove_repository` | `{ id: AgentRepositoryId }` | `{}` | Any | Change |  |
| `catalogue.fetch_now` | `{ id: Option<AgentRepositoryId> }` | `Vec<FetchOutcome>` | Any | Change |  |
| `catalogue.search` | `LibraryQuery` | `Page<LibraryEntry>` | Any | Read | budget `Search` |
| `catalogue.facets` | `LibraryQuery` | `LibraryFacets` | Any | Read | budget `Search` |
| `catalogue.part` | `{ id: PartId }` | `PartView` | Any | Read |  |
| `catalogue.read_part` | `{ id: PartId }` | `PartDocument` | Any | Read |  |
| `catalogue.used_by` | `{ id: PartId }` | `UsedBy` | Any | Read |  |
| `catalogue.made_of` | `{ agent: PartId }` | `MadeOf` | Any | Read |  |
| `catalogue.part_history` | `{ id: PartId }` | `PartHistory` | Any | Read |  |
| `catalogue.health` | `{ id: AgentRepositoryId }` | `HealthReport` | Any | Read |  |
| `catalogue.withdrawn_list` | `{ id: AgentRepositoryId }` | `Vec<WithdrawnListEntry>` | Any | Read |  |
| `catalogue.vocabulary` | `{ id: AgentRepositoryId }` | `Vec<VocabularyTerm>` | Any | Read |  |
| `catalogue.layout` | `{ id: AgentRepositoryId }` | `RepositoryLayout` | Any | Read |  |
| `catalogue.lifecycles` | `{ id: Option<AgentRepositoryId> }` | `Vec<Lifecycle>` | Any | Read |  |
| `catalogue.lifecycle` | `{ id: LifecycleId }` | `Lifecycle` | Any | Read |  |

#### Check it — owner `Checker.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `checker.check_installed` | `CheckInstalledInput { agent_instance_id: AgentInstanceId }` | `CheckReport` | Any | Change | records a check run; the program adds `lock` and `scope` (SA-CLOSE-01) |
| `checker.check_library_item` | `CheckLibraryItemInput { agent_repository_id: Option<AgentRepositoryId>, kind: PartKind, identifier }` | `CheckReport` | Any | Change | records a check run; the program adds `scope` (SA-CLOSE-01) |
| `checker.report` | `CheckRunRef { check_run_id: CheckRunId }` | `CheckReport` | Any | Read |  |

#### Agents in a workspace — owner `PackageCatalogue.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `agents.preview_add` | `AddAgentInput` | `AddPreview` | Any | Read |  |
| `agents.add` | `AddAgentInput` | `AddOutcome` | Any | Change |  |
| `agents.list` | `{ workspace_id: Option<WorkspaceId> }` | `Vec<InstalledAgent>` | Any | Read |  |
| `agents.get` | `{ agent_instance_id: AgentInstanceId }` | `InstalledAgent` | Any | Read |  |
| `agents.change_choice` | `ChangeChoiceInput` | `InstalledAgent` | Any | Change |  |
| `agents.accept_update` | `{ agent_instance_id: AgentInstanceId }` | `InstalledAgent` | Any | Change |  |
| `agents.decline_update` | `{ agent_instance_id: AgentInstanceId }` | `InstalledAgent` | Any | Change |  |
| `agents.remove` | `RemoveAgentInput` | `{}` | Any | Change |  |

#### What an installed agent is made of — owner `PackageInspector.md` (ADR-093)

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `agents.made_of` | `MadeOfInput` | `InstalledMadeOf` | Any | Read | **What it's made of** (SA-CLOSE-06) |
| `agents.installed_files` | `InstalledFilesInput` | `Vec<InstalledFileEntry>` | Any | Read | **Show the installed files**, `LinksCode` view only; the program adds `folder_path` (SA-CLOSE-06) |
| `agents.installed_file` | `InstalledFileInput { agent_instance_id, path }` | `InstalledFileText { path, text, cut }` | Any | Read | The reader of **What it's made of**; `path` 1–1,024 characters; the program adds `folder_path` (SVC-IF-1 (amended January 2026; user decision; Approved by the user January 2026)) |
| `agents.part_page` | `PartPageInput` | `PartPage` | Any | Read | **Read it**; the program adds `bundle_dir` (SA-CLOSE-06) |

#### Making agents and parts — owner `AuthoringService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `authoring.make_agent` | `MakeAgentInput` | `AgentDraftView` | Any | Change |  |
| `authoring.make_this_agent` | `AgentDraftId` | `AgentDraftView` | Any | Change |  |
| `authoring.update_agent_draft` | `UpdateAgentDraftInput` | `AgentDraftView` | Any | Change |  |
| `authoring.rehearse` | `RehearseInput` | `Rehearsal` | Any | Change |  |
| `authoring.try_it` | `TryItInput` | `AgentInstanceId` | Any | Change |  |
| `authoring.set_publish_to` | `SetPublishToInput { id: AgentDraftId, repository: AgentRepositoryId }` | `AgentDraftView` | Any | Change |  |
| `authoring.new_part` | `NewPartInput` | `PartDraftView` | Any | Change |  |
| `authoring.change_part` | `ChangePartInput` | `PartDraftView` | Any | Change |  |
| `authoring.stop_using_in_new_agents` | `PartId` | `PartDraftView` | Any | Change |  |
| `authoring.withdraw` | `WithdrawInput` | `PartDraftView` | Any | Change |  |
| `authoring.vocabulary_change` | `VocabularyChangeInput` | `PartDraftView` | Any | Change |  |
| `authoring.update_part_draft` | `UpdatePartDraftInput` | `PartDraftView` | Any | Change |  |
| `authoring.suggest_wording` | `SuggestWordingInput` | `PartDraftView` | Any | Change |  |
| `authoring.import` | `ImportInput` | `DraftRef` | Any | Change |  |
| `authoring.apply_fix` | `ApplyFixInput` | `DraftRef` | Any | Change |  |
| `authoring.publish` | `DraftRef` | `Done` | Any | Change | the program adds `PublishContext` |
| `authoring.send_change` | `DraftRef` | `Done` | Any | Change |  |
| `authoring.discard` | `DraftRef` | `Done` | Any | Change |  |
| `authoring.list` | `DraftFilter` | `Vec<DraftSummary>` | Any | Read |  |
| `authoring.agent_draft` | `AgentDraftId` | `AgentDraftView` | Any | Read |  |
| `authoring.part_draft` | `PartDraftId` | `PartDraftView` | Any | Read |  |

#### Handovers and lifecycles — owner `HandoverService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `handover.open_draft` | `OpenDraftInput` | `HandoverDraftView` | Any | Change |  |
| `handover.update_draft` | `UpdateDraftInput` | `HandoverDraftView` | Any | Change |  |
| `handover.find_people` | `FindPeopleInput` | `Vec<PersonChoice>` | Any | Read |  |
| `handover.hand_over` | `{ draft: DraftGroupId }` | `ApprovalId` | Any | Change |  |
| `handover.take_back` | `{ id: HandoverId }` | `ApprovalId` | Any | Change |  |
| `handover.remind` | `{ id: HandoverId }` | `{}` | Any | Change |  |
| `handover.change_and_hand_over_again` | `{ id: HandoverId }` | `HandoverDraftView` | Any | Change |  |
| `handover.check_now` | `{}` | `u32` (new handovers) | Any | Change |  |
| `handover.open` | `{ id: HandoverId }` | `HandoverScreen` | Any | Change | marks it seen |
| `handover.accept_and_start` | `AcceptInput` | `WorkspaceId` | Any | Change |  |
| `handover.return_options` | `{ id: HandoverId }` | `Vec<ReturnOption>` | Any | Read |  |
| `handover.return_to` | `ReturnInput` | `ApprovalId` | Any | Change |  |
| `handover.follow_stage` | `FollowStageInput` | `WorkspaceStage` | Any | Change |  |
| `handover.stop_following` | `{ workspace: WorkspaceId }` | `{}` | Any | Change |  |
| `handover.start_workspace_for_stage` | `StartForStageInput` | `WorkspaceId` | Any | Change |  |
| `handover.stage_of` | `{ workspace: WorkspaceId }` | `Option<StageView>` | Any | Read |  |
| `handover.lifecycle_view` | `{ id: LifecycleId }` | `LifecycleView` | Any | Read |  |
| `handover.list` | `{ filter: HandoverFilter }` | `Vec<HandoverLine>` | Any | Read |  |
| `handover.open_link` | `{ link }` | `LinkOpened` (`Opened { handover_id }` \| `NotFoundYet { place }`) | Local | Change | follows the handover's place; the screens' paste box and the desktop program's `ordin://handover/open` handler call it |

#### Code (engineering mode) — owner `ProjectService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `projects.open` | `{ place_id, workspace_id }` | `Project` | Local | Change | `open_project` |
| `projects.close` | `{ project_id }` | `Project` | Local | Change | `close_project` |
| `projects.status` | `{ project_id }` | `Vec<StatusEntry>` | Local | Read |  |
| `projects.diff` | `{ project_id, path: Option<String> }` | `String` | Local | Read |  |
| `projects.history` | `{ project_id, path: Option<String>, limit: u32 }` | `Vec<CommitSummary>` | Local | Read |  |
| `projects.branches` | `{ project_id }` | `Vec<String>` | Local | Read |  |
| `projects.create_branch` | `{ project_id, name }` | `{}` | Local | Change |  |
| `projects.switch_branch` | `{ project_id, name }` | `{}` | Local | Change |  |
| `projects.commit` | `{ project_id, message, terminal: Option<StreamId> }` | `CommitSummary` | Local | Change |  |
| `projects.push` | `{ project_id, terminal: Option<StreamId> }` | `PushOutcome` | Local | Change |  |
| `projects.save_file` | `{ project_id, path, text, expected_hash: Option<Sha256Hex> }` | `FileVersionId` | Local | Change | 9 MiB body |
| `projects.review` | `{ project_id }` | `Vec<ReviewTask>` | Local | Read |  |
| `projects.decide_entry` | `{ entry: ChangeSetEntryId, decision: EntryDecision }` | `DecisionOutcome` | Local | Change |  |
| `projects.keep_attempt` | `{ task: CodingTaskId }` | `{}` | Local | Change |  |
| `projects.run_check` | `{ project_id, task: Option<CodingTaskId>, command, origin: CommandOrigin }` | `ProjectCheckResult` | Local | Change |  |
| `projects.git_host_settings` | `{ project_id }` | `Vec<GithubOperationSetting>` | Local | Read |  |
| `projects.set_git_host_approval` | `{ project_id, kind, approval_required: bool }` | `GithubOperationSetting` | Local | Change |  |
| `projects.language_plugins` | `{}` | `Vec<LanguagePlugin>` | Local | Read |  |
| `projects.configure_language` | `{ language, chosen_server: Option<String>, enabled: bool, server_settings }` | `LanguagePlugin` | Local | Change |  |
| `projects.restart_language_server` | `{ language }` | `{}` | Local | Change |  |
| `projects.language_servers_answer` | `{ project_id }` | `bool?` | Local | Read |  |
| `projects.answer_language_servers` | `{ project_id, allowed: bool }` | `{}` | Local | Change |  |
| `projects.editor_open` | `{ project_id, path }` | `EditorSession` | Local | Change | stream |
| `projects.lsp_send` | `{ session: StreamId, message }` | `{}` | Local | Change | stream |
| `projects.editor_close` | `{ session: StreamId }` | `{}` | Local | Change |  |
| `projects.ides` | `{}` | `Vec<Ide>` | Local | Read |  |
| `projects.set_ide_enabled` | `{ ide_id, enabled: bool }` | `{}` | Local | Change |  |
| `projects.open_in_ide` | `{ project_id, ide_id, path: Option<String> }` | `{}` | Local | Change |  |
| `projects.terminal_open` | `{ project_id, cols: u16, rows: u16 }` | `StreamId` | Local | Change | stream |
| `projects.terminal_input` | `{ terminal: StreamId, bytes }` | `{}` | Local | Change | stream |
| `projects.terminal_resize` | `{ terminal: StreamId, cols: u16, rows: u16 }` | `{}` | Local | Change |  |
| `projects.terminal_close` | `{ terminal: StreamId }` | `{}` | Local | Change |  |
| `projects.check_now` | `{ repo_place: PlaceId, branch }` | `ProjectFetchOutcome` | Local | Change | `fetch_followed` for one original |
| `projects.set_check_commands` | `{ project_id, commands: Vec<String> }` | `Project` | Any | Change | `set_check_commands`; the project settings screen (SA-CLOSE-07) |

#### Using your plan — owner `UsagePlanner.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `usage.get_settings` | `{}` | `UsageSettings` | Any | Read |  |
| `usage.update_settings` | `UsageSettingsChange` | `UsageSettings` | Any | Change |  |
| `usage.today` | `{}` | `Today` | Any | Read |  |

#### Sign-ins — owner `IdentityAndCredentials.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `identity.list` | `{}` | `{ items: Vec<CredentialView> }` | Any | Read |  |
| `identity.sign_in` | `{ kind: CredentialKind, reason: String?, relay_port: u16?, site: String? }` | `SignInChallenge` (without credential secrets) | Any | Change | `relay_port` 47821–47823, personal server only; `SignInChallenge::RelayedBrowser` then (amended January 2026, ADR-102); `site` for `jira` only (amended January 2026, ADR-106) |
| `identity.sign_in_wait` | `{ challenge_id: SignInChallengeId }` | `CredentialView` | Any | Change |  |
| `identity.sign_in_cancel` | `{ challenge_id: SignInChallengeId }` | `{}` | Any | Change |  |
| `identity.complete_relayed_sign_in` | `{ challenge_id: SignInChallengeId, code, state }` | `CredentialView` | Any | Change | carries a secret in; personal server only |
| `identity.sign_out` | `{ credential_id: CredentialId }` | `{}` | Any | Change |  |
| `identity.save_mail_password` | `{ account_label, kind: "mail_password" \| "app_password", password }` | `CredentialView` | Any | Change | carries a secret in |
| `identity.save_gitlab_token` | `{ token }` | `CredentialView` | Any | Change | carries a secret in; GitLab without an OAuth application (SA-CLOSE-04) |
| `identity.import_client_certificate` | `{}` | `{ certificate: Option<ClientCertificateView> }` | Local | Change | the desktop program opens the file picker; no path crosses the screens (SA-CLOSE-04) |

#### Getting ready and help — owner `SetupAssistant.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `setup.status` | `{}` | `{ needs_guided: bool, progress: SetupProgress, attachments: Vec<HarnessAttachment> }` | Any | Read |  |
| `setup.run_guided` | `{}` | `SetupReport` | Any | Change |  |
| `setup.check_now` | `{}` | `SetupReport` | Any | Change |  |
| `setup.sign_in_harness` | `{ harness: "claude_code" \| "copilot_cli" }` | `HarnessAttachment` | Any | Change |  |
| `setup.update_offer` | `{}` | `{ offer: UpdateOffer? }` | Local | Read | desktop |
| `setup.install_update` | `{}` | `{}` | Local | Change | desktop |

#### Language — owner `Localisation.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `localisation.get_settings` | `{}` | `{ active: ActiveLocale, offered: Vec<OfferedLocale>, following_operating_system: bool }` | Any | Read |  |
| `localisation.set_locale` | `{ locale: String \| null }` | `ActiveLocale` | Any | Change |  |
| `localisation.get_catalogue` | `{ locale }` | `CatalogueText` | Any | Read |  |
| `localisation.list_languages` | `{}` | `{ languages: Vec<LanguageSummary>, areas: Vec<String> }` | Any | Read | ADR-109 |
| `localisation.list_messages` | `{ locale, text: String?, area: String?, only: MessageOnly?, cursor: Cursor? }` (text 1–200 characters) | `Page<MessageRow>` | Any | Read | ADR-109; page size 100 |
| `localisation.edit_message` | `{ locale, message_id, ftl }` (ftl 1–4,096 characters) | `MessageRow` | Any | Change | ADR-109 |
| `localisation.restore_message` | `{ locale, message_id }` | `MessageRow` | Any | Change | ADR-109; shipped languages only |
| `localisation.restore_language` | `{ locale }` | `LanguageSummary` | Any | Change | ADR-109; shipped languages only |
| `localisation.mark_reviewed` | `{ locale, message_ids: Vec<String> }` (1–200 ids) | `LanguageSummary` | Any | Change | ADR-109 |
| `localisation.confirm_language` | `{ locale }` | `LanguageSummary` | Any | Change | ADR-109 |
| `localisation.remove_language` | `{ locale }` | `{}` | Any | Change | ADR-109; made or imported languages only |
| `localisation.export_language` | `{ locale }` | `LanguageFile` (`ftl` ≤ 2 MiB) | Any | Read | ADR-109; the screens write it where the person chooses, as `workflows.export` |
| `localisation.import_language` | `{ file_name, ftl, replace: bool }` (file name 1–255 characters; `ftl` ≤ 2 MiB) | `ImportReport` | Any | Change | ADR-109; 9 MiB body |

#### Brand — owner `Brand.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `brand.get` | `{}` | `{ brand_identifier, version, source, strings: BrandStrings, design_tokens, links, font_count: u8, refusal }` | Local | Read |  |
| `brand.get_asset` | `{ asset: AssetRef }` | `AssetBytes` | Local | Read |  |

#### Dictation — owner `DictationService.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `dictation.get_availability` | `{}` | `DictationAvailability` | Local | Read |  |
| `dictation.start` | `{ field: FieldRef }` | `{ session_id }` | Local | Change | stream: `DictationUpdate` values |
| `dictation.stop` | `{ session_id }` | `{}` | Local | Change |  |
| `dictation.cancel` | `{ session_id }` | `{}` | Local | Change |  |

#### Servers this desktop is paired with — owner `ServerConnector.md`

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `servers.list` | `Done` | `Vec<ServerView>` | Local | Read |  |
| `servers.connect` | `ConnectServer` | `ServerView` | Local | Change |  |
| `servers.disconnect` | `ServerRef` | `Done` | Local | Change |  |
| `servers.remove` | `ServerRef` | `Done` | Local | Change |  |

369 commands in 38 groups (*Amended January 2026 (ADR-109):* 14 added — ten `localisation.*` for languages the person edits, makes, exports and imports, three `ordin.*` for Ordin's drafting of a language, and the desktop's `files.save_export`, which writes an exported text file where the person chooses.) (*Amended January 2026 (ADR-108):* 1 added — `ordin.continue_conversation`, continuing an earlier conversation.) (*Amended January 2026 (ADR-106):* 2 added — `jira.connect_token` and `jira.find_issues` in a group of their own (receivers for a Jira place come from `handover.find_people`); `identity.sign_in` gains optional `site`.) (*Amended January 2026 (ADR-104):* 2 added — `knowledge.accept_suggestion` and `knowledge.dismiss_suggestion`; `knowledge.save_edit` gains optional `tags` and `aliases`.) (*Amended January 2026 (ADR-103):* 8 added — seven `team_knowledge.*` in a group of their own and `memory.keep_checked`.) (*Amended January 2026 (ADR-102).* 20 added: fourteen `slack.*`, two `workspaces.*` Slack grant commands, `handover.open_link`, `identity.complete_relayed_sign_in`, and the desktop's `app.open_slack_link` and `identity.relay_sign_in`.) Rows of the earlier draft of this catalogue that no owner defines and no consumer calls were removed, and rows that duplicated an owner's command under another name were replaced by the owner's command (SA-RECON-01): `home.overview` → `home.view`; `home.mark_opened` → `home.document_opened`; `home.run_again` → `home.run_again_inputs`; `workspaces.remove` → `workspaces.move_to_bin`; `workspaces.add_place` → `files.add_place` (a folder) or `workspaces.add_link` (a remote location); `workspaces.remove_place` → `workspaces.remove_link`; `files.list` → `files.list_folder`; `files.read_text` → `files.preview`; `files.save_text` → `files.save_edit`; `files.open_in_app` → `craft.open_with_system_default`; `push.pending` → `push.list_pending`; `confluence.publish` → `confluence.publish_checked`; `conversation.*` → `ordin.*` (X5: `send` → `send_message`; `answer`, `submit_form`, `answer_offer` → `answer_block` (`BlockAnswer::Choice`/`Other`, `Form`, `Offer`); `block` → the same verb under `ordin` (`close_item`, `show_item` and `close_finished` were removed January 2026, plan 05 Phase 9); `history` → `read_conversation`; `earlier_list` → `list_conversations`; `clear` → `prepare_start_fresh` then `start_fresh`); `approval.pending` → `approval.list_pending`; `agents.library` → `catalogue.search`; `agents.repositories` → `catalogue.list_repositories`; `agents.repository_add` → `catalogue.probe_repository` then `catalogue.add_repository`; `agents.repository_remove` → `catalogue.remove_repository`; `authoring.drafts` → `authoring.list`; `authoring.draft_get` → `authoring.agent_draft`; `authoring.send_for_review` → `authoring.publish`; `handover.get` → `handover.open`; `handover.accept` → `handover.accept_and_start`; `handover.return` → `handover.return_to`; `mail.message_get` → `mail.open_message`; `mail.draft_get` → `mail.get_draft`; `mail.draft_save` → `mail.edit_draft`; `mail.ask_to_send` → `mail.send`; `signins.list` / `start` / `sign_out` → `identity.list` / `sign_in` / `sign_out`; `setup.retry` → `setup.check_now`; `help.send_to_it` → `setup.help_preview` then `setup.help_send`; `projects.change_set` → `projects.review`; `projects.commit_and_push` → `projects.commit` then `projects.push`; `projects.discard` → `projects.decide_entry`. Removed with no successor: `runs.start` (no owner; tasks start through `ordin.answer_block` with a form), `dashboards.get`, `telegram.draft_save`, `telegram.ask_to_send`, `push.send_back` (no consumer).

### Streams outside the catalogue (`service_streams`)

Three commands start a flow of values that are not replies and are never journalled: `dictation.start` (`DictationUpdate` values, ADR-043), `projects.terminal_open` / `projects.terminal_input` (terminal bytes) and `projects.editor_open` / `projects.lsp_send` (language-server messages). Their command replies are ordinary catalogue replies (`{ session_id }`, `StreamId`, `EditorSession`); the values that follow travel on the Command Bridge's transport command `service_streams`, which is outside the catalogue like `service_events` and the pairing route, and carries `StreamMessage { stream: uuid::Uuid, kind: StreamKind, payload: serde_json::Value }` (CommandBridge.md, its SA-RECON-01). `stream` is the `session_id` or `StreamId` the command returned. This crate defines no stream type and no stream command; `service_streams` is desktop-only, so the four commands are scope `Local`, and the Remote API serves none of them.

### Command input and result types

The owners' types named in the catalogue are restated, field for field and with the owner's `serde` attributes and limits, in `ordin_service_api::commands::<module>` (SA-RECON-02): this crate is in build phase 1 and depends only on `ordin-types` (2B-3), so it cannot import a module crate. Each program's `ServiceApi` implementation converts between the module type and the restated type; a contract test in each program crate (which depends on both) serialises a sample of every module type and deserialises it into the restated type, and the reverse, and fails on any difference. Types that are 2C types (`Workspace`, `Place`, `FileVersion`, `Run`, `Report`, `Dashboard`, `ExternalAction`, `Artefact`, `RemoteAgent`, `ApprovalPress`, `EmailDraft`, `PendingPush`, `AgentRepository`, `Lifecycle` and the other 2C entities) are imported from `ordin-types`, never restated.

The types this spec owns — the program's commands, `workspaces.tree`, the diagnostics types and the shared references — are defined in `ordin_service_api::commands` *(chosen here)*, derive `Debug, Clone, Serialize, Deserialize, schemars::JsonSchema`, use `#[serde(rename_all = "snake_case")]`, and every input type adds `deny_unknown_fields`. Text lengths count Unicode scalar values after NFC normalisation (`ordin_types::text::normalise_text`).

```rust
// ---- references ----
pub struct WorkspaceRef { pub workspace_id: WorkspaceId }
pub struct ServerRef { pub server_connection_id: ServerConnectionId }

// ---- backend ----
pub struct ServiceHello {
    pub api_version: ServiceApiVersion,
    pub backend: BackendId,
    /// "This computer" for the desktop's own core; the server's display name for a server.
    pub backend_name: String,
    pub profile: BackendProfile,
    /// The last sequence number this backend's journal has assigned; 0 when none.
    pub last_seq: u64,
}
pub enum BackendProfile { Desktop, PersonalServer }

// ---- this program (OrdinDesktop.md) ----
pub struct AppQuitResult { pub stopping_sessions: u32 }
pub struct AppPickFolderResult { #[serde(skip_serializing_if = "Option::is_none")] pub folder: Option<String> }
pub struct SettingsSectionRef { pub section: SettingsSection }
pub enum SettingsSection {
    SignIns, AgentRepositories, Email, Telegram, Servers, Dictation, HowOrdinTalks,
    UsingYourPlan, Notifications, History, Display, Code, WhatOrdinRemembers, Diagnostics,
}
pub struct SettingsView { pub section: SettingsSection, pub settings: Vec<SettingView> }
pub struct SettingView {
    /// The 2D-2 / 2D-3 key, for example `usage.plan_mode`.
    pub key: String,
    pub value: serde_json::Value,
    pub set_by: SetBy,
    /// False when the organisation profile locks it ("Set by your company").
    pub person_may_change: bool,
}
pub struct ChangeSetting { pub key: String, pub value: serde_json::Value }
pub struct SaveCopy { pub backend: BackendId, pub workspace_id: WorkspaceId, pub item: ContentRef }
pub struct SavedCopy { /// The path the person chose; shown only on this computer.
    pub saved_path: String }

// ---- composed by the program, or served by the Journal over this spec's types ----
/// `workspaces.tree`.
pub struct WorkspaceTree {
    pub workspace: Workspace,
    pub places: Vec<Place>,
    pub agents: Vec<AgentInstance>,
    pub workflows: Vec<WorkflowId>,
    pub dashboards: Vec<DashboardId>,
    pub running: Vec<RunningItem>,
    pub knowledge_pages: u32,
    pub memory_facts: u32,
    pub waiting_for_person: u32,
}
#[serde(tag = "kind")]
pub enum RunningItem {
    Run { workspace_id: WorkspaceId, run_id: RunId, title: String, state: RunState },
    CrewRun { workspace_id: WorkspaceId, crew_run_id: CrewRunId, title: String, state: CrewRunState },
}
/// `diagnostics.lines`, `diagnostics.support_export`.
pub struct DiagnosticLine { pub reference: String, pub at: Timestamp, pub level: String, pub code: String, pub summary: String }
pub struct SupportExport { pub run_ids: Vec<RunId> }

// ---- servers (ServerConnector.md uses these types unchanged) ----
pub struct ServerView {
    pub server_connection_id: ServerConnectionId,
    pub display_name: String,
    pub state: ServerConnectionState,
    /// "Not connected · as of 10:42" (11 §5.31).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<BackendId>,
    /// Present only when `state` is `refused_for_version`: which side must be updated, so the
    /// screens pick one of the two 11 §5.31 sentences (reconciliation row 144; filled by the
    /// Server Connector from `RefusedForVersion { server_older }` and the stored server version).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub older_side: Option<OlderSide>,
    /// The server's address as `host:port` (`[host]:port` for an IPv6 host), as **Connect again…**
    /// fills it in (SVC-SRV-1; amended January 2026 (user decision; Approved by the user January 2026)).
    pub address: String,
}
pub enum OlderSide { Desktop, Server }
#[serde(tag = "kind")]
pub enum ConnectServer {
    AddressAndCode { address: String, code: String },
    PairingLine { line: String },
}
```

Input limits of this spec's types *(each a constant in `commands::limits`)*. The limits of a restated owner type are the owner's validation rows, applied by its `check` (below); the Error Table codes stay the owner's when the owner's module re-checks, and are `SVC_INPUT_INVALID` when this crate refuses before dispatch.

| Field | Rule | Error code |
|---|---|---|
| `ChangeSetting.key` | `^[a-z][a-z0-9_]*(\.[a-z0-9_<>]+)*$`, 1–128 characters; value at most 64 KiB serialised | `SVC_INPUT_INVALID` |
| `ContentRef::PlaceFile.relative_path` | 1–1,024 bytes, no `..`, no leading `/` | `SVC_INPUT_INVALID` |
| `ConnectServer::AddressAndCode.address` | host name (≤ 253 characters) or IP literal, optional `:port` 1025–65535 | `SVC_INPUT_INVALID` |
| `ConnectServer::AddressAndCode.code` | the pairing-code form (8 characters of `[A-HJ-NP-Z2-9]`, an optional hyphen after the fourth) | `SVC_INPUT_INVALID` |
| `ConnectServer::PairingLine.line` | 1–400 characters, the pairing-line form | `SVC_INPUT_INVALID` |
| `SupportExport.run_ids` | at most 20 | `SVC_INPUT_INVALID` |
| any text field of any input | NFC-normalised; control characters other than `\n`, `\t` refused | `SVC_INPUT_INVALID` |
| any `Cursor` | ≤ 1,024 characters | `SRV_BAD_CURSOR` |
| body of `files.save_edit`, `knowledge.save_edit`, `projects.save_file`, `localisation.import_language`, `files.save_export` (amended January 2026, ADR-109) | at most 9 MiB (text at most 8 MiB of UTF-8; Localisation refuses a language file over 2 MiB with `LOC_FILE_TOO_LARGE`) | `SVC_BODY_TOO_LARGE` |

Every input type implements:

```rust
pub trait CommandInput: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static {
    /// NFC-normalises every text field in place and applies the rules above (or, for a
    /// restated owner type, the owner's validation rows). Returns the JSON Pointer of the
    /// first failing field.
    fn check(&mut self) -> Result<(), InputRejection>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InputRejection { pub code: &'static str, pub at: String }
```

### Event catalogue

```rust
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceEvent {
    // ---- 2C wire §1, unchanged ----
    /// A `Run`'s state changed (04 — Run lifecycle).
    RunStateChanged { run_id: RunId, state: RunState },
    /// One observable progress line, or a tool count (ADR-068 "What happened").
    RunProgress { run_id: RunId, progress: RunProgress },
    /// A stated or changed plan (ADR-068); `steps` are `RunStep`s of the new plan version.
    PlanChanged { run_id: RunId, plan_version: u32, steps: Vec<RunStep> },
    /// One step's mark changed (ADR-068).
    StepMarked { run_id: RunId, plan_version: u32, position: u8, mark: StepMark },
    /// The start-up report of a run or crew run is ready (G9, P5).
    StartUpReportReady { subject: StartUpSubject, items: Vec<StartUpReportItem> },
    ApprovalRequested { card: ApprovalCard },
    ApprovalDecided { approval_id: ApprovalId, state: ApprovalState, decided_on: String },
    CrewRoundChanged { crew_run_id: CrewRunId, round: u32, state: CrewRunState },
    /// A block Ordin or an agent showed, after it passed the block check.
    BlockShown { workspace_id: WorkspaceId, block_id: BlockId, block: Block },
    /// Remote API only: `Last-Event-ID` is older than the journal keeps; reload by commands.
    Reset,
    // ---- added in 1.0 by this spec (chosen here) ----
    /// Desktop only, on the desktop's own backend: a paired server's link state changed
    /// (02:589). Emitted by the Server Connector.
    ServerStateChanged {
        server_connection_id: ServerConnectionId,
        state: ServerConnectionState,
        #[serde(skip_serializing_if = "Option::is_none")]
        as_of: Option<Timestamp>,
    },
    /// Something in a workspace's tree changed (a place, an agent, a workflow, a dashboard);
    /// the screens reload it with `workspaces.tree`.
    WorkspaceChanged { workspace_id: WorkspaceId },
    /// An item was added to or settled in *Needs you*; reload with `home.needs_you`.
    NeedsYouChanged { workspace_id: WorkspaceId },
    /// A shown block whose event would exceed 256 KiB; fetch it with `ordin.block`.
    BlockAvailable { workspace_id: WorkspaceId, block_id: BlockId },
    /// The active locale changed (the person's choice or a newer profile). Emitted by the program
    /// from Localisation; `ActiveLocale` restated from Localisation.md (SA-RECON-04).
    LocaleChanged { active: ActiveLocale },
    /// A language's wording changed (edited, restored, checked, confirmed, imported, removed, or
    /// drafted by Ordin). Emitted by the program from Localisation's `language_changes()`;
    /// payload restated from Localisation.md (SA-RECON-04; amended January 2026, ADR-109).
    LanguageChanged { locale: String, change: LanguageChangeKind },
    /// Desktop only: the active brand pack changed (Brand.md's payload).
    BrandChanged { brand_identifier: String, version: String, source: BrandPackSource },
    /// Getting ready progressed (Setup Assistant's payload); reload with `setup.status`.
    SetupProgress { progress: SetupProgress },
    /// A sign-in's state changed (Identity and Credentials' payload); reload with `identity.list`.
    /// `credential_kind`, not `kind`: `kind` is the event's own tag (SA-CLOSE-02).
    SignInChanged { credential_id: CredentialId, credential_kind: CredentialKind, state: SignInState },
}
```

`BrandPackSource` and `CredentialKind` are 2C domain types (`ordin-types`); `ActiveLocale`, `SetupProgress` and `SignInState` are restated from their owners' specs under SA-RECON-02.

| Kind | Emitted by (writes to the Journal) | Data limits | Carried to a paired desktop |
|---|---|---|---|
| `run_state_changed` | Session Supervisor | — | yes |
| `run_progress` | Session Supervisor, MCP Front (tool progress counts) | `line` 1–160; `done ≤ total` | yes |
| `plan_changed` | MCP Front (`plan` tool) | 3–7 steps | yes |
| `step_marked` | MCP Front (`plan` tool) | `position` 1–7 | yes |
| `start_up_report_ready` | Session Supervisor, Crew Orchestrator | ≤ 200 items (LW-28) | yes |
| `approval_requested` | Approval Service | the card's 2C rules | yes |
| `approval_decided` | Approval Service | `decided_on` 1–80 | yes |
| `crew_round_changed` | Crew Orchestrator | — | yes |
| `block_shown` | MCP Front (`report`, `plan`), Ordin Host (`show` and the person's own messages) | ≤ 256 KiB serialised, else `block_available` | yes |
| `reset` | Remote API (never journalled) | — | yes |
| `server_state_changed` | Server Connector (desktop journal) | — | no (desktop-local) |
| `workspace_changed` | Workspace Service, Package Catalogue, Ordin Host (the File Service emits none: a file change reaches the screens through the Workspace Service's record, SA-CLOSE-03) | — | yes |
| `needs_you_changed` | Approval Service, Ordin Host, Crew Orchestrator, Handover Service, Push Service, Package Catalogue, Authoring Service | — | yes |
| `locale_changed` | program (from Localisation) | `active.locale` a BCP 47 tag, 2–35 characters | yes |
| `language_changed` | program (from Localisation; amended January 2026, ADR-109) | `locale` a BCP 47 tag, 2–35 characters; `change` one of `LanguageChangeKind` | yes |
| `brand_changed` | desktop program (from Brand) | `version` 1–64 | no (desktop-local) |
| `setup_progress` | program (from Setup Assistant) | — | yes |
| `sign_in_changed` | program (from Identity and Credentials) | — | yes |
| `block_available` | MCP Front, when a `block_shown` record's `data` would exceed 256 KiB (the Journal refuses it with `JRNL_EVENT_TOO_LARGE`); also substituted by `encode_for_transport` when the envelope overhead pushes a frame past the limit | — | yes |

The Journal stores each event as `kind` (a string) plus JSON `data` and delivers `JournalEvent { backend: uuid::Uuid, seq, at, kind: String, data }` through its `EventSink` (Journal spec). This crate cannot depend on the Journal, so it takes the record's parts as arguments.

```rust
impl ServiceEvent {
    pub fn kind(&self) -> EventKind;
    /// Decodes a Journal record's `data` into the variant `kind` names, then runs `check`.
    /// An unknown kind string → `SVC_EVENT_INVALID`.
    pub fn from_journal(kind: &str, data: serde_json::Value) -> Result<ServiceEvent, EventError>;
    /// The JSON object of the variant without its `kind` key (what an emitter writes as `data`).
    pub fn to_journal_data(&self) -> serde_json::Value;
    /// The validation rules of the *Validation rules* table for events.
    pub fn check(&self) -> Result<(), EventError>;
}

impl EventEnvelope {
    /// Builds the envelope of one Journal record: `api_version` = CURRENT, then `from_journal`.
    pub fn new(backend: BackendId, seq: u64, at: Timestamp, kind: &str, data: serde_json::Value)
        -> Result<EventEnvelope, EventError>;
}

/// The SSE frame or IPC payload of one event, with the 256 KiB rule applied.
pub struct TransportEvent { pub seq: u64, pub kind: EventKind, pub json: String }

/// Serialises the envelope; a `block_shown` over 256 KiB becomes `block_available` with the
/// same backend, seq and time; any other kind over 256 KiB → `SVC_EVENT_TOO_LARGE`.
pub fn encode_for_transport(envelope: &EventEnvelope) -> Result<TransportEvent, EventError>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct EventError { pub code: &'static str }
```

### Envelope functions

```rust
pub const MAX_BODY_BYTES: usize = 1024 * 1024;                    // LW-2
pub const MAX_TEXT_SAVE_BODY_BYTES: usize = 9 * 1024 * 1024;      // files.save_edit, knowledge.save_edit, projects.save_file
pub const MAX_EVENT_DATA_BYTES: usize = 256 * 1024;               // LW-6

/// A transport's view of one incoming command, after the transport authenticated the caller.
pub struct Incoming<'a> {
    pub body: &'a [u8],
    /// For the Remote API: the command name from the path (`/v1/<module>/<verb>`); it must
    /// equal the envelope's `command`. For IPC: the Tauri command name mapped back to the
    /// dotted name.
    pub routed_name: &'a str,
    pub origin: CallOrigin,
    /// The backend this core is.
    pub this_backend: BackendId,
}

/// Parses and checks an envelope: size, JSON, version, request id, command, scope, backend,
/// input. Returns the checked command ready for `dispatch`.
pub fn accept(incoming: &Incoming<'_>) -> Result<Accepted, Refusal>;

pub struct Accepted {
    pub meta: &'static CommandMeta,
    pub request_id: CommandRequestId,
    pub api_version: ServiceApiVersion,
    pub input: serde_json::Value, // re-serialised after `CommandInput::check` normalised it
}

/// A refusal before dispatch; `request_id` is absent when the body could not be read.
pub struct Refusal { pub request_id: Option<CommandRequestId>, pub code: &'static str, pub at: Option<String> }

/// Runs a whole command: accept → dispatch → reply. Never panics; a panic inside the
/// implementation is caught and answered as `SVC_INTERNAL`.
pub async fn handle(
    api: &dyn ServiceApi,
    sentences: &dyn ErrorSentences,
    incoming: Incoming<'_>,
    deadline: std::time::Instant,
    cancel: tokio_util::sync::CancellationToken,
) -> HandledReply;

pub struct HandledReply {
    /// The serialised `CommandReply<serde_json::Value>`.
    pub json: String,
    pub code: Option<&'static str>,
    /// HTTP status per the 2C status table (the Remote API uses it; IPC ignores it).
    pub http_status: u16,
}
```

### Pairing route types

The one route outside the catalogue (2C wire *Names* item 6) is pairing. Its body types live here so that the Remote API (server side) and the Server Connector (desktop side) share them; the route itself is the Remote API spec's. Module `ordin_service_api::pairing` *(chosen here; closes 2C G-2)*:

```rust
/// Body of `POST /v1/pairing`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PairRequest {
    pub api_version: ServiceApiVersion,
    /// Normalised pairing code: 8 characters of `[A-HJ-NP-Z2-9]`, no hyphen.
    pub code: String,
    /// The desktop computer's name, 1–63 characters ("Asha's laptop").
    pub computer_name: String,
}

/// `kind: "ok"` result of `POST /v1/pairing`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PairReply {
    pub desktop_pairing_id: DesktopPairingId,
    /// 32 random bytes, base64url without padding (43 characters). Sent once, never again.
    pub desktop_key: String,
    pub backend: BackendId,
    /// The server's display name, 1–120 characters ("Northwind server").
    pub server_name: String,
    pub api_version: ServiceApiVersion,
}

/// The one line the setup command prints when the server's certificate is not one the
/// desktop already trusts (ADR-096 item 5, OQ-61):
/// `ordin-pair:1:<host>:<port>:<sha256 of the leaf certificate, 64 lowercase hex>:<code>`,
/// with an IPv6 host in brackets, e.g.
/// `ordin-pair:1:10.20.4.17:8443:9f2c…e41a:K7QM2XPA`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingLine { pub host: String, pub port: u16, pub fingerprint: Sha256Hex, pub code: String }

impl PairingLine {
    pub fn parse(raw: &str) -> Result<PairingLine, InputRejection>;   // SVC_INPUT_INVALID
    pub fn format(&self) -> String;
}

/// Uppercases, removes spaces and one hyphen, and refuses any character outside the
/// alphabet below or a length other than 8. Returns the 8-character code.
pub fn normalise_code(raw: &str) -> Result<String, InputRejection>;  // SVC_INPUT_INVALID

/// Displays a code as `ABCD-EFGH`.
pub fn display_code(code: &str) -> String;
```

Code alphabet `ABCDEFGHJKLMNPQRSTUVWXYZ23456789` (32 symbols, no `I`, `O`, `0`, `1`) *(chosen here: the person may type it, and the four look-alike symbols are left out)*. Eight symbols carry 40 bits; with five wrong codes allowed per ten minutes (ADR-096 item 5) a guess succeeds with probability below 5 × 10⁻¹². `PairingLine.port` is 1025–65535; `host` is a host name of at most 253 characters or an IP literal.

### Block catalogue and its check

The block types are 2C wire §7, copied verbatim into `ordin_service_api::blocks`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Message(MessageBlock), Question(QuestionBlock), Form(FormBlock), Documents(DocumentsBlock),
    Preview(PreviewBlock), Document(DocumentBlock), Changes(ChangesBlock),
    PlanProgress(PlanProgressBlock), Table(TableBlock), Chart(ChartBlock),
    KeyFigures(KeyFiguresBlock), Report(ReportBlock), EmailList(EmailListBlock), Email(EmailBlock),
    TelegramList(TelegramListBlock), SlackList(SlackListBlock), Result(ResultBlock), AgentProposal(AgentProposalBlock),
    AgentLibrary(AgentLibraryBlock), CrewPlan(CrewPlanBlock), Memory(MemoryBlock), Knowledge(KnowledgeBlock),
}

pub struct MessageBlock { pub text: String, pub attachments: Vec<ContentRef> }
pub struct QuestionBlock { pub question: String, pub choices: Vec<String>, pub allow_other: bool }
pub struct FormBlock { pub title: String, pub verb: String, pub target: RunTarget,
    #[serde(skip_serializing_if = "Option::is_none")] pub command: Option<String>, pub fields: Vec<FormField> }
/// The Usage Planner's `UsageEstimate`, restated (2C wire §7); `CrewPlanBlock.estimate`, filled by the Ordin Host.
pub struct UsageEstimate { pub band: EstimateBand, pub min_turns: u32, pub max_turns: u32,
    pub typical_day_permille: u32, pub based_on: Vec<EstimateBasis>, pub ask_first: bool }
pub enum EstimateBand { Light, Medium, Heavy }
#[serde(tag = "kind")]
pub enum EstimateBasis { EarlierRuns { count: u32 }, Limits { max_rounds: u32 }, Members { count: u32 },
    Steps { count: u32 }, Documents { count: u32, total_bytes: u64 } }
pub struct FormField { pub name: String, pub label: String, pub required: bool,
    #[serde(flatten)] pub input: FormInput, pub filled_by_ordin: bool }
#[serde(tag = "input", rename_all = "snake_case")]
pub enum FormInput {
    Text { value: Option<String>, placeholder: Option<String> },
    Document { value: Option<ContentRef> },
    Place { value: Option<PlaceId> },
    Choice { options: Vec<String>, value: Option<String> },
}
pub struct DocumentsBlock { pub workspace_id: WorkspaceId, pub items: Vec<ContentRef> }
pub struct PreviewBlock { pub item: ContentRef }
pub struct DocumentBlock { pub item: ContentRef }
pub struct ChangesBlock { pub before: ContentRef, pub after: ContentRef, pub view: ChangeView }
pub enum ChangeView { TextDifferences, BeforeAndAfterPages, BeforeAndAfterImages }
pub struct PlanProgressBlock { pub subject: StartUpSubject }
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanToolArgs { State { steps: Vec<String> }, Mark { position: u8, mark: AgentStepMark } }
pub enum AgentStepMark { Now, Done, Skipped }
pub struct TableBlock { pub title: String, pub columns: Vec<TableColumn>, pub rows: Vec<Vec<Cell>>, pub source: ReportPartSource }
pub struct TableColumn { pub name: String, #[serde(flatten)] pub column_type: ColumnType }
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ColumnType { Text, Number { decimals: Option<u8> }, Money { currency: String }, Percentage, Date, DocumentLink }
#[serde(untagged)]
pub enum Cell { Empty, Number(f64), Text(String), Link(ContentRef) }
pub struct ChartBlock { pub title: String, #[serde(flatten)] pub chart: ChartKind,
    pub columns: Vec<TableColumn>, pub rows: Vec<Vec<Cell>>, pub x: String, pub series: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub x_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub y_title: Option<String>, pub source: ReportPartSource }
#[serde(tag = "chart", rename_all = "snake_case")]
pub enum ChartKind { Bar { orientation: BarOrientation, arrangement: BarArrangement }, Line, Area, Scatter, Pie }
pub enum BarOrientation { Vertical, Horizontal }
/// Defined once in `ordin_types::domain::runs` and re-exported here (2C, decision 80: C-23).
pub use ordin_types::domain::runs::BarArrangement;
pub struct KeyFiguresBlock { pub figures: Vec<KeyFigure>, pub source: ReportPartSource }
pub struct KeyFigure { pub label: String, pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")] pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub change: Option<FigureChange> }
pub struct FigureChange { pub against: String, pub earlier_value: f64 }
pub struct ReportBlock { pub title: String, pub as_of: Timestamp, pub parts: Vec<ReportPart> }
pub struct ReportPart { pub heading: String, pub content: ReportPartContent }
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportPartContent { Table(TableBlock), Chart(ChartBlock), KeyFigures(KeyFiguresBlock) }
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportToolArgs { Report(ReportBlock), Table(TableBlock), Chart(ChartBlock), KeyFigures(KeyFiguresBlock) }
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// Renamed from `PartSource` (2C wire §7, decision 80: C-5); `PartSource` is the Agent Model's trait.
pub enum ReportPartSource {
    Documents { documents: Vec<SourceDocument> },
    CompanySystem { system: String, read_at: Timestamp },
    EarlierResult { run_id: RunId },
    WorkedOut { by: String, from: Vec<ContentRef> },
}
pub struct SourceDocument { pub item: ContentRef, pub version: String }
pub struct EmailListBlock { pub workspace_id: WorkspaceId, pub email_link_id: EmailLinkId, pub messages: Vec<String> }
#[serde(tag = "of", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmailBlock { Message { workspace_id: WorkspaceId, message_id: String }, Draft { draft_id: EmailDraftId } }
pub struct TelegramListBlock { pub telegram_link_id: TelegramLinkId }
#[serde(tag = "of", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultBlock {
    Produced { run_id: RunId, summary: String, items: Vec<ContentRef> },
    Offer { offer: ResultOffer, run_id: Option<RunId> },
    Checklist { title: String, items: Vec<ContentRef> },
}
pub enum ResultOffer { SaveAsWorkflow, UpdateTheWorkflow }
pub struct AgentProposalBlock { pub agent_repository_id: AgentRepositoryId, pub agent: String, pub reason: String }
pub struct AgentLibraryBlock { #[serde(skip_serializing_if = "Option::is_none")] pub search: Option<String> }
pub struct CrewPlanBlock { pub crew_charter_id: CrewCharterId,
    #[serde(skip_serializing_if = "Option::is_none")] pub estimate: Option<UsageEstimate> }
pub struct MemoryBlock { pub workspace_id: WorkspaceId, pub facts: Vec<MemoryFactId> }
pub struct KnowledgeBlock { pub workspace_id: WorkspaceId, #[serde(skip_serializing_if = "Option::is_none")] pub page: Option<KnowledgePageId> }

pub struct BlockRejection {
    pub code: String,
    /// JSON Pointer to the failing field, e.g. `/parts/2/content/source`.
    pub at: String,
    /// One sentence naming the correct form, for the model.
    pub reason: String,
}
```

(Every struct above carries `#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]` and `#[serde(rename_all = "snake_case", deny_unknown_fields)]`, and every unit enum `#[serde(rename_all = "snake_case")]`, exactly as 2C wire §7 writes them.)

Block rules, copied from 2C wire §7 (LW-29 to LW-42):

| Kind / field | Rule | Error code |
|---|---|---|
| any block | serialised size ≤ 2 MiB | `BLOCK_TOO_LARGE` |
| any block | unknown `kind` / unknown field | `BLOCK_KIND_UNKNOWN` / `BLOCK_FIELD_UNKNOWN` |
| links | only `ContentRef`, never a URL | `BLOCK_LINK_NOT_ALLOWED` |
| message | `text` 1–8,000; `attachments` ≤ 20 | `BLOCK_TEXT_INVALID`, `BLOCK_TOO_MANY_ITEMS` |
| question | `question` 1–300; `choices` 2–4, each 1–60, unique | `BLOCK_CHOICES_INVALID` |
| form | `fields` 1–20; `name` `^[a-z][a-z0-9_]{0,31}$` unique; `label` 1–80; `Text.value` 0–16,384; `Choice.options` 2–20; `verb` not an approval verb | `BLOCK_FORM_INVALID` |
| documents | `items` 1–500 | `BLOCK_TOO_MANY_ITEMS` |
| any `ContentRef` | resolvable and readable by the person | `BLOCK_REF_UNRESOLVED` |
| `relative_path` | 1–1,024 bytes, no `..`, no leading `/` | `BLOCK_PATH_INVALID` |
| `plan` tool | `steps` 3–7, each 1–80 | `BLOCK_PLAN_INVALID` |
| `plan` tool | `position` within the current plan | `BLOCK_PLAN_POSITION_INVALID` |
| `plan` tool | at most 20 plan versions per run | `BLOCK_PLAN_TOO_MANY_VERSIONS` |
| table | `title` 1–120, column `name` 1–60, unique | `BLOCK_TABLE_INVALID` |
| table | `columns` 1–30 / `rows` 0–10,000 | `BLOCK_TABLE_TOO_WIDE` / `BLOCK_TABLE_TOO_LONG` |
| table cell | matches column type; numbers finite; `Date` is `YYYY-MM-DD` | `BLOCK_CELL_TYPE_MISMATCH` |
| table cell | `Text` 0–2,000 | `BLOCK_CELL_TOO_LONG` |
| `Money.currency` | `^[A-Z]{3}$` | `BLOCK_CURRENCY_INVALID` |
| table, chart, key figures | `source` present | `BLOCK_SOURCE_MISSING` |
| chart | `series` 1–8, each a numeric column | `BLOCK_CHART_SERIES_INVALID` |
| chart | `rows × series` ≤ 5,000 | `BLOCK_CHART_TOO_MANY_POINTS` |
| chart | pie: exactly 1 series, ≤ 6 rows | `BLOCK_CHART_PIE_INVALID` |
| chart | `x` names a column; scatter `x` numeric | `BLOCK_CHART_AXIS_INVALID` |
| chart | titles 1–120 | `BLOCK_CHART_INVALID` |
| key figures | `figures` 1–8 | `BLOCK_FIGURES_COUNT` |
| key figures | `label` 1–60, `unit` 1–24, `against` 1–60; values finite | `BLOCK_FIGURES_INVALID` |
| report | `parts` 1–12 | `BLOCK_REPORT_PARTS_COUNT` |
| report | `title`, `heading` 1–120; `as_of` ≤ now + 5 minutes | `BLOCK_REPORT_INVALID`, `BLOCK_AS_OF_INVALID` |
| part source | `documents` 1–20; `version` 1–64; `system` 1–80; `by` 1–80 and the producing run's agent | `BLOCK_SOURCE_INVALID` |
| email list | `messages` 1–200, each 1–998 and in the workspace's email links | `BLOCK_EMAIL_NOT_LINKED` |
| result | `summary` 1–2,000; offer from the closed list | `BLOCK_TEXT_INVALID`, `BLOCK_OFFER_UNKNOWN` |
| agent proposal | `reason` 1–300; `agent` 1–256 | `BLOCK_TEXT_INVALID` |
| memory | `facts` 1–100 | `BLOCK_TOO_MANY_ITEMS` |
| rejections | at most 20 returned per call; `reason` 1–300 | — |

`BLOCK_REPORT_INVALID` and `BLOCK_TEXT_INVALID` for result and agent-proposal texts are *chosen here*: 2C gives those limits without a code.

```rust
/// What the check needs from the caller to resolve references. Implemented by the desktop and
/// server programs, each over the File Service, Mail Service, Journal, Telegram Service
/// (`resolve_list_block` behind `entity`) and Knowledge Service (`resolve_block` behind `entity`).
#[async_trait::async_trait]
pub trait RefResolver: Send + Sync {
    async fn content(&self, workspace_id: WorkspaceId, item: &ContentRef) -> bool;
    async fn email_message(&self, workspace_id: WorkspaceId, email_link_id: EmailLinkId, message_id: &str) -> bool;
    async fn entity(&self, workspace_id: WorkspaceId, entity: EntityRef) -> bool;
    /// The display name of the agent of `run_id`, for `ReportPartSource::WorkedOut.by`.
    async fn run_agent_name(&self, run_id: RunId) -> Option<String>;
    /// Plan versions already recorded for `run_id`, and the current plan's length.
    async fn plan_state(&self, run_id: RunId) -> PlanState;
    /// The Approval Service's approval verbs, which a form's `verb` may not use.
    fn approval_verbs(&self) -> &[String];
}

pub enum EntityRef {
    Run(RunId), Draft(EmailDraftId), TelegramLink(TelegramLinkId), SlackLink(SlackLinkId), CrewCharter(CrewCharterId),
    MemoryFact(MemoryFactId), KnowledgePage(KnowledgePageId), AgentRepository(AgentRepositoryId),
}
pub struct PlanState { pub versions: u32, pub current_len: u8 }

/// Who sends the block, for the source and context rules.
pub struct BlockCheckContext {
    pub workspace_id: WorkspaceId,
    /// The run producing it; `None` for Ordin's own session.
    pub run_id: Option<RunId>,
    pub now: Timestamp,
}

/// The `show` tool: one block of any kind.
pub async fn check_block(raw: &serde_json::Value, ctx: &BlockCheckContext, refs: &dyn RefResolver)
    -> Result<Block, Vec<BlockRejection>>;

/// The `report` tool: a report keeps the parts that pass and lists the rejected ones.
pub async fn check_report_tool(raw: &serde_json::Value, ctx: &BlockCheckContext, refs: &dyn RefResolver)
    -> Result<CheckedReport, Vec<BlockRejection>>;
pub struct CheckedReport { pub block: ReportToolArgs, pub dropped_parts: Vec<BlockRejection> }

/// The `plan` tool.
pub async fn check_plan_tool(raw: &serde_json::Value, run_id: RunId, refs: &dyn RefResolver)
    -> Result<PlanToolArgs, Vec<BlockRejection>>;

/// The stored form of a checked report (Journal `Report`, 2C part 1).
pub fn report_to_domain(block: &ReportBlock, report_id: ReportId, workspace_id: WorkspaceId, run_id: RunId)
    -> ordin_types::domain::runs::Report;

/// The block check as one injectable trait (SA-RECON-05). Called by the MCP Front (`report`,
/// `plan` tools) and the Ordin Host (`show` and the blocks of its own session); `StandardBlockCheck`
/// is the one implementation and calls the three free functions above.
#[async_trait::async_trait]
pub trait BlockCatalogueCheck: Send + Sync {
    async fn check_block(&self, raw: &serde_json::Value, ctx: &BlockCheckContext, refs: &dyn RefResolver)
        -> Result<Block, Vec<BlockRejection>>;
    async fn check_report_tool(&self, raw: &serde_json::Value, ctx: &BlockCheckContext, refs: &dyn RefResolver)
        -> Result<CheckedReport, Vec<BlockRejection>>;
    async fn check_plan_tool(&self, raw: &serde_json::Value, run_id: RunId, refs: &dyn RefResolver)
        -> Result<PlanToolArgs, Vec<BlockRejection>>;
}
#[derive(Debug, Clone, Copy, Default)]
pub struct StandardBlockCheck;
impl BlockCatalogueCheck for StandardBlockCheck { /* delegates to check_block, check_report_tool, check_plan_tool */ }
```

`report_to_domain` is called by the MCP Front before it hands a checked report to the Journal. The free functions and `ServiceEvent::to_journal_data` are called by the MCP Front, the Ordin Host and every event emitter listed in the event table (which write through `Journal::append_event(EventKind::X.as_str(), data)`, R3).

### Schema and TypeScript generation

- `pub fn json_schemas() -> serde_json::Value` returns one JSON Schema document (draft 2020-12, as `schemars` 1 writes it) with `$defs` for every input, result, event and block type, and a top-level `x-ordin-commands` array of `{name, ipc_name, rest_path, scope, effect, input: {"$ref"}, result: {"$ref"}}` and `x-ordin-events` array of `{kind, data: {"$ref"}}`.
- Binary target `ordin-service-api-gen` (`src/bin/gen.rs`) *(chosen here)*:
  - `ordin-service-api-gen write <ui_dir>` writes `<ui_dir>/src/lib/service-api/schema.gen.json`, `types.gen.ts` and `commands.gen.ts`.
  - `ordin-service-api-gen check <ui_dir>` regenerates in memory and exits 1 with the line `service-api types are stale: <file>` when any file differs; `make check` runs it with `apps/desktop/ui`.
- `types.gen.ts`: one `export type` per `$defs` entry. `commands.gen.ts`: `export const API_VERSION = { major: 1, minor: 0 } as const;`, `export type Commands = { "approval.decide": { input: ApprovalPress; result: ApprovalOutcome; scope: "any"; effect: "change" }; … }`, `export const IPC_NAMES: Record<keyof Commands, string>`, and `export type ServiceEvent = …` as a union discriminated by `kind`.

##### Example

IPC call from the screens (Tauri command `approval_decide`, payload `{ "envelope": … }`):

```json
{
  "api_version": {"major": 1, "minor": 0},
  "request_id": "0192a1c0-0d1e-7f20-8a31-425364758697",
  "command": "approval.decide",
  "backend": "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b",
  "input": {"card_ref": "q8Zt3VxN0pLk2mRb7sYw1A", "decision": "approve"}
}
```

Reply:

```json
{"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","kind":"ok","result":{"approval_id":"0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f","state":"approved","decided_on":"Asha's laptop"}}
```

Refusal of an unknown field:

```json
{"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","kind":"error","error":{"code":"SVC_INPUT_INVALID","message":"ordin couldn't read that request. Try again.","action":"try_again"}}
```

Event, decoded from the Journal record `{seq: 48213, kind: "run_progress", data: {"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","progress":{"line":"Read supplier quotes","count":{"done":12,"total":40}}}}`:

```json
{"api_version":{"major":1,"minor":0},"backend":"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b","seq":48213,"at":"2026-01-10T09:41:07.312Z","kind":"run_progress","run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","progress":{"line":"Read supplier quotes","count":{"done":12,"total":40}}}
```

Generated TypeScript (excerpt of `commands.gen.ts`):

```ts
export type Commands = {
  "approval.decide": { input: ApprovalPress; result: ApprovalOutcome; scope: "any"; effect: "change" };
  "servers.connect": { input: ConnectServer; result: ServerView; scope: "local"; effect: "change" };
};
```

## Internal Logic

1. **Catalogue macro.** `service_catalogue!` takes rows `module verb => Input -> Result, Scope, Effect, Budget;` and generates: the `ServiceApi` trait (method `<module>_<verb>`); `CATALOGUE` in row order; `command()` as a `match` on the dotted name; `dispatch()` as a `match` on `meta.ipc_name` that deserialises `Input` with `serde_json::from_value`, awaits the method and serialises the result with `serde_json::to_value`; `for_each_command!`; and the schema registry. A unit test asserts that `CATALOGUE.len() == 400` (*amended January 2026*: the total after the ADR-114 amendment; it read 319 before the amendments of January 2026 and January 2026), that every name matches `^[a-z][a-z0-9_]{0,30}\.[a-z][a-z0-9_]{0,30}$`, that `ipc_name == name.replace('.', "_")`, that `rest_path == format!("/v1/{}/{}", module, verb)`, and that no two rows share a name.
2. **`accept(incoming)`** — in this order, each failure a `Refusal`:
   1. Body longer than `MAX_BODY_BYTES` (or `MAX_TEXT_SAVE_BODY_BYTES` when `routed_name` is `files.save_edit`, `knowledge.save_edit` or `projects.save_file`) → `SVC_BODY_TOO_LARGE`.
   2. Parse as `CommandEnvelope<serde_json::Value>` with `deny_unknown_fields`; failure → `SVC_INPUT_INVALID` with `request_id: None`, unless the `request_id` key alone parses, in which case it is carried on the refusal.
   3. `request_id` not a UUIDv7 → `SVC_REQUEST_ID_MALFORMED`.
   4. `api_version.major` outside 1–999 or `minor` outside 0–999 → `SVC_VERSION_MALFORMED`. `desktop_accepts(api_version, ServiceApiVersion::CURRENT)` false → `SVC_VERSION_UNSUPPORTED`. On the desktop's own core the WebView and core are built together, so this fails only after a partial update.
   5. `command` not matching the name pattern, not in `CATALOGUE`, or different from `routed_name` → `SVC_COMMAND_UNKNOWN`.
   6. `meta.scope == Local` and `origin` is `PairedDesktop` → `SVC_COMMAND_LOCAL_ONLY`.
   7. `backend != this_backend` → `SVC_BACKEND_UNKNOWN`. (The Command Bridge routes a server's commands to the Server Connector before calling `accept`; a core only accepts commands for itself.)
   8. Deserialise `input` into the command's `Input` with `serde_path_to_error`; failure → `SVC_INPUT_INVALID`, `at` = the error's path as a JSON Pointer.
   9. `Input::check()`; failure → its code (`SVC_INPUT_INVALID`, `SVC_BODY_TOO_LARGE` or `SRV_BAD_CURSOR`), `at` its pointer.
   10. Return `Accepted` with the normalised input re-serialised.
3. **`handle(...)`.** Calls `accept`; on `Refusal` builds `ServiceError { code, message: sentences.sentence(code), action: sentences.action(code), reference: None }`. Otherwise builds `CallContext { request_id, correlation_id: request_id.to_string(), origin, deadline, cancel }` and runs `dispatch` inside `tokio::time::timeout_at(deadline)` and `std::panic::AssertUnwindSafe(..).catch_unwind()`. The transport chooses `deadline`: `Interactive` 10 s, `Search` 10 s, `Transfer` 60 s after receipt *(assumed here: the p95 targets are 150 ms and 1 s; the deadline only stops a stuck module from holding a connection)*. Timeout → `SVC_DEADLINE` (retryable, action `try_again`); panic → `SVC_INTERNAL` (no action); `cancel` fired → no reply is sent and the transport drops the call. The reply's `http_status` follows the 2C table: 200 for `ok` and domain errors; 422 `SVC_INPUT_INVALID`, `SVC_BODY_TOO_LARGE` *(assumed here: a body refusal is an input refusal)*, `SVC_REQUEST_ID_MALFORMED`, `SVC_VERSION_MALFORMED`, `SRV_BAD_CURSOR`; 409 `SVC_VERSION_UNSUPPORTED`; 404 `SVC_COMMAND_UNKNOWN`, `SVC_COMMAND_LOCAL_ONLY`, `SVC_BACKEND_UNKNOWN`; 503 `SVC_DEADLINE`; 500 `SVC_INTERNAL`. The Remote API adds 401 and 429 itself.
4. **Error mapping inside a `ServiceApi` implementation.** A program maps a module error `e: &dyn CodedError` to `ServiceError { code: e.code().into(), message: sentences.sentence(e.code()), action: e.repair().or(sentences.action(e.code())) }`. A module error whose code is not in the registry becomes `SVC_INTERNAL` (SA-ENV-01's unregistered rule, with this crate's prefix) and is logged at `error` with the original code.
5. **Logging per command.** `handle` logs one `info` line at completion: `svc.command` with `request_id`, `command`, `origin` (`local` or `paired_desktop`), `outcome` (`ok` or the code), `elapsed_ms`. Input and result bodies are never logged.
6. **Events: `from_journal(kind, data)`.** `EventKind::parse(kind)` (unknown → `SVC_EVENT_INVALID`); rebuild a tagged object by inserting `"kind": kind` into `data` (an object; anything else → `SVC_EVENT_INVALID`) and deserialise as `ServiceEvent`; failure → `SVC_EVENT_INVALID`. Then `check()`:
   - `RunProgress.line` 1–160, `done ≤ total`; `StepMarked.position` 1–7; `PlanChanged.steps` 3–7; `ApprovalDecided.decided_on` 1–80; `StartUpReportReady.items` ≤ 200 → otherwise `SVC_EVENT_INVALID`.
   - `Reset` from the Journal → `SVC_EVENT_INVALID` (only the Remote API makes it).
   An invalid record is not sent; the transport logs `svc.event_dropped` with `seq`, `kind`, `code` and moves on, and the screens' gap rule reloads (step 9).
7. **Journal contract.** The Journal (`Journal::append_event(kind: &str, data)`, `EventSink::deliver(&JournalEvent)`, `Journal::events_since(after_seq, limit)`) assigns contiguous `seq` per backend; an emitter writes `data = event.to_journal_data()` (or the same JSON built from its own types) under `kind = event.kind().as_str()`. This crate does not depend on the Journal; the transports pass the record's parts in.
8. **`encode_for_transport`.** Serialise the envelope; if the serialised `data` part (the envelope minus `api_version`, `backend`, `seq`, `at`) exceeds 256 KiB: for `BlockShown` substitute `BlockAvailable { workspace_id, block_id }` and serialise again; for any other kind return `SVC_EVENT_TOO_LARGE` (the transport logs it and sends nothing for that `seq`).
9. **Gap and order rule (screens).** The generated TypeScript exposes `expectNext(backend, seq)`: a `seq` ≤ the last seen is ignored as late; a `seq` > last + 1 marks a gap, logged as `SVC_EVENT_GAP` to the desktop's Diagnostics through `diagnostics.lines`' source, and the screens reload the affected views by command. Written here because the generator emits it; it is a pure function in `commands.gen.ts`.
10. **`check_block(raw, ctx, refs)`.**
    1. `serde_json::to_vec(raw).len() > 2 MiB` → one rejection `BLOCK_TOO_LARGE`, `at` `""`.
    2. Deserialise with `serde_path_to_error` into `Block`. An unknown `kind` → `BLOCK_KIND_UNKNOWN` at `/kind`; an unknown field → `BLOCK_FIELD_UNKNOWN` at its path; any string where a `ContentRef` object is expected and that parses as a URL → `BLOCK_LINK_NOT_ALLOWED`; any other shape error → `BLOCK_FIELD_UNKNOWN` at its path with serde's message as `reason`.
    3. NFC-normalise every text field and run the per-kind rules of the table above, collecting every failure with its JSON Pointer, stopping after 20.
    4. Resolve every `ContentRef` with `refs.content`, every email message with `refs.email_message`, every id in `EntityRef` with `refs.entity`; each failure → `BLOCK_REF_UNRESOLVED` (or `BLOCK_EMAIL_NOT_LINKED`) at its pointer. Calls run concurrently, at most 16 at once, each bounded by 2 s; a timeout counts as unresolved.
    5. `ReportPartSource::WorkedOut.by` must equal `refs.run_agent_name(ctx.run_id)` when `run_id` is present → else `BLOCK_SOURCE_INVALID`. `ReportBlock.as_of > ctx.now + 5 minutes` → `BLOCK_AS_OF_INVALID`.
    6. A `FormBlock.verb` equal (case-insensitive, trimmed) to one of `refs.approval_verbs()`, or to "OK", "Yes" or "Continue" → `BLOCK_FORM_INVALID`.
    7. No rejection → `Ok(block)`; otherwise `Err(rejections)`. Each `reason` is a fixed English sentence per code naming the correct form (for the model, not the person; models read English *(assumed here: Ordin's instructions are English; the person sees Ordin's own explanation in their locale)*).
11. **`check_report_tool`.** For `ReportToolArgs::Report`, run step 10 on the report's envelope (title, `as_of`, part count) and then on each part separately; a failing part is removed and its rejections go to `dropped_parts`; if no part survives → `Err` with every rejection. The other three variants follow step 10 whole.
12. **`check_plan_tool`.** `State`: steps 3–7, each 1–80 → else `BLOCK_PLAN_INVALID`; `refs.plan_state(run_id).versions >= 20` → `BLOCK_PLAN_TOO_MANY_VERSIONS`. `Mark`: `position` 1..=`current_len` → else `BLOCK_PLAN_POSITION_INVALID`.
13. **`report_to_domain`.** Table → `ReportData::Table` with each column's `ColumnType` mapped by name (`Money { currency }` → `ColumnType::Money` and `currency: Some(currency)`; `Number { decimals }` → `ColumnType::Number` and `decimals: decimals`, C-23), each `Cell::Number(v)` → `CellValue::Number(DecimalString(format!("{v}")))`, `Cell::Text` → `CellValue::Text`, `Cell::Empty` → `CellValue::Null`, `Cell::Link(r)` → `CellValue::Text` of the reference's JSON. Chart → `ReportData::Chart` with `Bar{Vertical, arrangement}` → `BarVertical`, `Bar{Horizontal, arrangement}` → `BarHorizontal`, with `bar_arrangement: Some(arrangement)` (C-23; `None` for other kinds), `x` → `x_column`, `series` → `series_columns`, absent titles → `""`. Key figures → `KeyFigure { value: DecimalString, unit: unit.unwrap_or_default(), change_against: change.map(|c| FigureChange { earlier_label: c.against, earlier_value: DecimalString(..) }) }`. `ReportPartSource::Documents` → one `ReportSource::Document` per document (`location` = the reference's JSON, `version_read` = `version`); `CompanySystem` and `EarlierResult` map by name; `WorkedOut { by, from }` → `WorkedOutBy { agent: <the run's AgentRef, filled by the caller after the call>, from: <joined references> }`.
14. **`StandardBlockCheck`.** Each method calls the free function of the same name with the same arguments and returns its result unchanged; it holds no state.
15. **TypeScript generator.** Walks `json_schemas()`: object → `export interface` (required keys without `?`, optional keys with `?`); `oneOf` whose members each carry a `const` value under the same property → discriminated union; string `enum` → literal union; `$ref` → the type name; `integer`/`number` → `number`; `additionalProperties` with a schema → `Record<string, T>`; `Vec` → `T[]`. Names are the Rust type names. `u64` sequence numbers stay `number`: JavaScript represents integers exactly up to 2^53 − 1, which a journal at 1,000 events a second reaches after 285,000 years. Output is sorted by name and formatted with two-space indentation so it is byte-stable.

## Data Model

N/A — the crate owns no table. The Journal owns the event records this crate decodes; the Remote API owns desktop pairings.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Command body over 1 MiB (9 MiB for the three text-save commands, `localisation.import_language` and `files.save_export`) | 422 / `Err(permanent)` | `SVC_BODY_TOO_LARGE` | `{"request_id":…,"kind":"error","error":{"code":"SVC_BODY_TOO_LARGE","message":"…","action":null}}` |
| Body not an envelope, unknown envelope field, input shape or rule fails | 422 / `Err(permanent)` | `SVC_INPUT_INVALID` | as above with `SVC_INPUT_INVALID`, action `try_again` |
| `request_id` not UUIDv7 | 422 / `Err(permanent)` | `SVC_REQUEST_ID_MALFORMED` | as above |
| Version numbers out of range | 422 / `Err(permanent)` | `SVC_VERSION_MALFORMED` | as above |
| Version pair not accepted | 409 / `Err(permanent)` | `SVC_VERSION_UNSUPPORTED` | as above; message names the side to update |
| Command name malformed, unknown, or not the routed one | 404 / `Err(permanent)` | `SVC_COMMAND_UNKNOWN` | as above |
| Local-only command from a paired desktop | 404 / `Err(permanent)` | `SVC_COMMAND_LOCAL_ONLY` | as above |
| Backend id is not this core | 404 / `Err(permanent)` | `SVC_BACKEND_UNKNOWN` | as above |
| Malformed cursor | 422 / `Err(permanent)` | `SRV_BAD_CURSOR` | as above |
| Implementation did not answer by the deadline | 503 / `Err(retryable)` | `SVC_DEADLINE` | as above, action `try_again` |
| Implementation panicked, or returned an unregistered code | 500 / `Err(permanent)` | `SVC_INTERNAL` | as above, no action |
| Journal record not decodable, or event rule fails | dropped, logged | `SVC_EVENT_INVALID` | — (no frame) |
| Non-block event over 256 KiB | dropped, logged | `SVC_EVENT_TOO_LARGE` | — (no frame) |
| `seq` jump seen by the screens | desktop diagnostic | `SVC_EVENT_GAP` | — |
| Block over 2 MiB | tool error | `BLOCK_TOO_LARGE` | `[{"code":"BLOCK_TOO_LARGE","at":"","reason":"…"}]` |
| Unknown block kind / field | tool error | `BLOCK_KIND_UNKNOWN` / `BLOCK_FIELD_UNKNOWN` | `[{"code":…,"at":"/kind","reason":"…"}]` |
| URL where a reference is expected | tool error | `BLOCK_LINK_NOT_ALLOWED` | `[{…}]` |
| A per-kind rule fails | tool error | the rule's `BLOCK_…` code from the block rules table | `[{…}]` |
| A reference not found, or not readable by the person | tool error | `BLOCK_REF_UNRESOLVED` | `[{…}]` |
| Email message not in the workspace's links | tool error | `BLOCK_EMAIL_NOT_LINKED` | `[{…}]` |
| `hosted.decide_card` refused by the hosted agent: card already decided, person not an approver there or the asker, card unknown (SA-CLOSE-05) | the owner's error, passed through as for every module | `HAA_CARD_DECIDED` / `HAA_NO_ACCESS` / `HAA_CARD_UNKNOWN` | as above with that code |
| Generated TypeScript differs from the committed files | exit status 1 | — | `service-api types are stale: <file>` |

The words shown to the person for `SVC_` codes are Localisation's (07, 11 §6); this crate supplies none. Proposed message identifiers: `svc-input-invalid` "ordin couldn't read that request. Try again."; `svc-deadline` "That took too long. Try again."; `svc-internal` "Something went wrong inside ordin." (amended January 2026 (user decision; Approved by the user January 2026): no sentence sends the person to IT); `svc-version-unsupported` uses the two sentences of 11 §5.31.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Service API

  Scenario: Happy path — an approval press is accepted and dispatched
    Given the desktop core with backend 0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b
    And a pending approval whose approve button carries card_ref "q8Zt3VxN0pLk2mRb7sYw1A"
    When the screens send approval.decide with that card_ref and decision approve
    Then handle returns a reply with kind "ok" and state "approved"
    And the reply's request_id equals the envelope's request_id

  Scenario: Error path — an unknown input field
    Given an approval.decide envelope whose input has an extra field "force": true
    When handle is called
    Then the reply has kind "error" and code SVC_INPUT_INVALID
    And http_status is 422
    And the ServiceApi method is not called

  Scenario: Error path — a local-only command over the Remote API
    Given a call whose origin is PairedDesktop
    When it sends servers.connect
    Then the reply has code SVC_COMMAND_LOCAL_ONLY and http_status 404

  Scenario: Error path — a desktop two minor versions ahead
    Given a server at Service API version 1.0
    When an envelope with api_version 1.2 arrives
    Then the reply has code SVC_VERSION_UNSUPPORTED and http_status 409

  Scenario: Edge case — a desktop one minor version ahead is accepted
    Given a server at Service API version 1.0
    When an envelope with api_version 1.1 arrives for a command that exists in 1.0
    Then the command is dispatched

  Scenario: Error path — the path and the envelope disagree
    Given a Remote API call to /v1/runs/stop whose envelope command is "approval.decide"
    When accept is called
    Then it is refused with SVC_COMMAND_UNKNOWN

  Scenario: Error path — a stuck module
    Given a ServiceApi method that never returns
    When handle is called with a deadline 10 seconds ahead
    Then after 10 seconds the reply has code SVC_DEADLINE and action try_again

  Scenario: Error path — a panicking module
    Given a ServiceApi method that panics
    When handle is called
    Then the reply has code SVC_INTERNAL and no action
    And the process keeps running

  Scenario: Happy path — a journal record becomes an event
    Given the journal record seq 48213, kind run_progress, data {"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","progress":{"line":"Read supplier quotes","count":{"done":12,"total":40}}}
    When EventEnvelope::new is called with that record's parts for backend 0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b
    Then the envelope serialises with "kind":"run_progress" and "seq":48213

  Scenario: Error path — a progress count above its total
    Given a run_progress record with done 41 and total 40
    When ServiceEvent::from_journal is called
    Then it fails with code SVC_EVENT_INVALID

  Scenario: Edge case — a large block is sent as a reference
    Given a block_shown event whose block serialises to 300 KiB
    When encode_for_transport is called
    Then the frame's kind is block_available with the same workspace_id, block_id and seq

  Scenario: Happy path — a table block passes the check
    Given the 2C table example with columns Supplier, Rate (money GBP), Change (percentage) and a documents source
    And a resolver that finds the referenced file
    When check_block is called
    Then it returns the TableBlock unchanged

  Scenario: Error path — a pie chart with two series
    Given a chart block of kind pie with series ["Q2","Q3"]
    When check_block is called
    Then it fails with one rejection of code BLOCK_CHART_PIE_INVALID at "/series"

  Scenario: Error path — a link given as a URL
    Given a message block whose attachments contain the string "https://example.com/a.pdf"
    When check_block is called
    Then it fails with code BLOCK_LINK_NOT_ALLOWED at "/attachments/0"

  Scenario: Edge case — a report keeps its good parts
    Given a report with three parts, the second a table without a source
    When check_report_tool is called
    Then it returns a report with parts one and three
    And dropped_parts holds one rejection with code BLOCK_SOURCE_MISSING at "/parts/1/content/source"

  Scenario: Error path — a form that approves
    Given a form block whose verb is "Approve"
    And the Approval Service's verbs include "Approve"
    When check_block is called
    Then it fails with code BLOCK_FORM_INVALID at "/verb"

  Scenario: Happy path — the merged catalogue
    Given the CATALOGUE static
    Then it holds 369 rows with unique names (amended January 2026, ADR-104 and ADR-106; January 2026, ADR-108 and ADR-109)
    And it contains "ordin.send_message", "catalogue.search", "identity.sign_in", "app.quit" and "app.pick_folder"
    And it contains "checker.check_installed", "checker.check_library_item" and "checker.report"
    And it contains "projects.set_check_commands" with scope Any and effect Change
    And it contains no "conversation.send", "signins.list" or "agents.library"

  Scenario: Edge case — sign_in_changed carries credential_kind
    Given the journal record kind sign_in_changed, data {"credential_id":"0192a1d0-2222-7333-8444-555566667777","credential_kind":"github","state":"signed_in"}
    When ServiceEvent::from_journal is called
    Then it returns SignInChanged with credential_kind Github
    And the serialised envelope has exactly one "kind" key, whose value is "sign_in_changed"

  Scenario: Error path — a code command over the Remote API
    Given a call whose origin is PairedDesktop
    When it sends projects.terminal_open
    Then the reply has code SVC_COMMAND_LOCAL_ONLY and http_status 404

  Scenario: Happy path — app.quit from the screens
    Given the desktop core and two local sessions running
    When the screens send app.quit with input {}
    Then the reply has kind "ok" and result {"stopping_sessions":2}

  Scenario: Edge case — a server refused for its version names the older side
    Given a ServerView in state refused_for_version whose server is two minor versions behind
    When it is serialised
    Then it carries "older_side":"server"
    And a ServerView in state connected carries no older_side key

  Scenario: Happy path — an error reply carries its Diagnostics reference
    Given a module error APPR_CARD_ALREADY_DECIDED recorded on Diagnostics line "r-81"
    When the program builds the ServiceError
    Then the reply's error has "reference":"r-81"

  Scenario: Happy path — a large text save is accepted
    Given a files.save_edit body of 6 MiB
    When accept is called
    Then it is not refused with SVC_BODY_TOO_LARGE

  Scenario: Happy path — the injected block check equals the free function
    Given StandardBlockCheck and the 2C table example
    When check_block is called through BlockCatalogueCheck
    Then the result equals that of the free function check_block

  Scenario: Happy path — locale_changed carries the owner's payload
    Given the journal record kind locale_changed, data {"active":{"locale":"fr-FR","source":"person","direction":"ltr","fallback_chain":["fr-FR","en-GB"]}}
    When ServiceEvent::from_journal is called
    Then it returns LocaleChanged with active.locale "fr-FR"

  Scenario: Happy path — pressing a hosted agent's approver card (SA-CLOSE-05)
    Given the screens send hosted.decide_card with server_or_agent "support-agent"'s id, approval_id A and decision "approve"
    When accept is called with CallOrigin LocalScreens
    Then the program calls decide_approver_card("support-agent", A, Approve, None) and the result is {"state":"approved"}

  Scenario: Error path — hosted.decide_card from a paired desktop
    Given a hosted.decide_card envelope arriving over the Remote API
    When accept is called
    Then it is refused with SVC_COMMAND_LOCAL_ONLY

  Scenario: Error path — the card was already decided by another approver
    Given decide_approver_card returns Refused with code HAA_CARD_DECIDED
    When hosted.decide_card is dispatched
    Then the reply's error code is HAA_CARD_DECIDED

  Scenario: Error path — the generated TypeScript is stale
    Given a new optional field added to RunView without regenerating
    When ordin-service-api-gen check apps/desktop/ui runs
    Then it exits with status 1 and prints "service-api types are stale: types.gen.ts"
```

## Performance, Security, Observability

- **Performance targets:** `accept` ≤ 200 µs p95 for a 4 KiB envelope and ≤ 5 ms p95 for a 1 MiB envelope; `encode_for_transport` ≤ 100 µs p95 for a 4 KiB event; `check_block` without reference resolution ≤ 20 ms p95 for a 10,000-row table (A-34's largest), and with resolution bounded by step 10.4 (2 s per reference, 16 at once). These leave the transports their share of A-03's and A-46's 150 ms p95 and A-46's 500 ms event delivery. `cargo bench -p ordin-service-api`.
- **Security:** every command's input is typed and refused on unknown fields before any module sees it (SA-API-02, SA-XC-12); text is NFC-normalised and control characters refused; `CallOrigin` comes only from the transport, never from input, so a paired desktop cannot claim to be the local screens; `Local` commands (app, settings, servers, hosted agents' approver cards, history, profile, diagnostics, dictation, brand, update offer, edit-in-app, Confluence, code and the path-taking file commands) are unreachable from the Remote API; blocks carry no markup, script, style or URL (ADR-028) and links are references only. Never logged: input and result bodies, event data, block content, `card_ref` values, message text. `#![forbid(unsafe_code)]`.
- **Observability:** log events `svc.command` (`request_id`, `command`, `origin`, `outcome`, `elapsed_ms`; `info`), `svc.event_dropped` (`seq`, `kind`, `code`; `warn`), `svc.unregistered_code` (`request_id`, `command`, `module_code`; `error`), `block.rejected` (`run_id` or `ordin`, `kind`, `codes` as a list, `count`; `info`). Agent profile only (SA-XC-04/05): none — the agent profile serves no Service API (it refuses desktop pairing and has no WebView).

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

### Command catalogue

The catalogue above lists 20 commands added on January 2026:

- **This program** (`OrdinDesktop.md`): `app.open_slack_link`, `identity.relay_sign_in` (Local).
- **Workspaces** (`WorkspaceService.md`): `workspaces.set_slack_grant`, `workspaces.slack_grants`.
- **Slack** (`SlackService.md`), a group of its own after **Telegram**: `slack.status`, `slack.sign_in`, `slack.connect_own_app`, `slack.offered_channels`, `slack.link`, `slack.unlink`, `slack.links`, `slack.messages`, `slack.set_ordin_reads`, `slack.rules`, `slack.add_rule`, `slack.turn_off_rule`, `slack.try_again`, `slack.post_record`.
- **Handovers and lifecycles** (`HandoverService.md`): `handover.open_link` (Local).
- **Sign-ins** (`IdentityAndCredentials.md`): `identity.complete_relayed_sign_in`; the input of `identity.sign_in` carries `relay_port: u16?` (47821–47823, personal server only, decision 82) and its result `SignInChallenge` has the variant `RelayedBrowser { challenge_id, authorisation_url, expires_at }`.

Result types named in those rows are the owners' (`SlackStatus`, `OfferedSlackChannel`, `SlackLinkSummary`, `SlackMessageOut`, `SlackRule`, `SlackRuleView`, `NewSlackRule`, `SlackScopeSet`, `SlackPost`, `SlackGrant`, `LinkOpened`); this spec restates them unchanged in the generated TypeScript file. Input `daily_cap` 1–20, default 20 (the owner's validation applies through `check`). The count line reads 342 commands in 36 groups.

### Blocks (2C wire §7)

`Block` has the variant `SlackList(SlackListBlock)`, after `TelegramList(TelegramListBlock)`:

```rust
pub struct SlackListBlock { pub slack_link_id: SlackLinkId }
```

Block kind identifier `slack_list` (2C wire *Names fixed*). Validation row (block check, as `telegram_list`): the link exists, is `linked` and belongs to the block's workspace → else `BLOCK_REF_UNRESOLVED` (through `RefResolver::entity(EntityRef::SlackLink(id))`).

### `RefResolver`

```rust
pub enum EntityRef {
    Run(RunId), Draft(EmailDraftId), TelegramLink(TelegramLinkId), SlackLink(SlackLinkId), CrewCharter(CrewCharterId),
    MemoryFact(MemoryFactId), KnowledgePage(KnowledgePageId), AgentRepository(AgentRepositoryId),
}
```

Both programs answer `EntityRef::SlackLink` over the Slack Service's `resolve_list_block` (`OrdinDesktop.md`, `OrdinServer.md`).

### Events

No new event kind. A Slack link added or removed, a rule changed and a grant changed are announced with the existing `workspace_changed` (the Slack Service and the Workspace Service append it); a sign-in with new Slack permissions with the existing `sign_in_changed`; new kept messages raise `needs_you_changed` only when Ordin shows something waiting.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the Slack group is in the catalogue
    When the static command list is read
    Then it contains "slack.status", "slack.link", "slack.add_rule", "handover.open_link" and "workspaces.set_slack_grant"
    And it holds 369 commands (342 after ADR-102; 350 after ADR-103; 352 after ADR-104; 354 after ADR-106; 355 after ADR-108; amended January 2026, ADR-109)

  Scenario: Edge case — a Slack list block for another workspace's link
    Given a slack_list block naming a link of workspace "HR"
    When Ordin shows it in workspace "Q3 supplier review"
    Then the block check refuses it with BLOCK_REF_UNRESOLVED

  Scenario: Error path — an unknown field in slack.add_rule
    When slack.add_rule is sent with an extra field "post_now"
    Then the reply is an error with code SVC_INPUT_INVALID
```

### Gaps

None.

## Amendment January 2026 — Team knowledge (ADR-103)

> Source: decision 83 (the user's decisions D1–D10 of January 2026); HLD revision 0.51.0, ADR-103. Owners: `components/TeamKnowledgeService.md` (Public Interface, *Service API commands*), `components/MemoryService.md` (amendment January 2026 — Team knowledge, `keep_checked`).

### Command catalogue

The catalogue above lists 8 commands added on January 2026:

- **Team knowledge** (`TeamKnowledgeService.md`), a group of its own after **Slack**: `team_knowledge.status`, `team_knowledge.candidate_links`, `team_knowledge.turn_on`, `team_knowledge.turn_off`, `team_knowledge.check_now`, `team_knowledge.get`, `team_knowledge.share`.
- **What Ordin remembers** (`MemoryService.md`): `memory.keep_checked`.

Result and input types named in those rows are the owners' (`TeamKnowledgeStatus` with `TeamBusy`, `GetReport` and `ShareProposed` inside it, `TeamFolderChoice`, `SourceLink`, `FactView`); every team knowledge command keeps the `Interactive` budget, because `check_now`, `get` and `share` reply at once and run their repository work in the background (TeamKnowledgeService.md §H); this spec restates them unchanged in the generated TypeScript file. The count line read 350 commands in 37 groups (352 after ADR-104).

### Blocks (2C wire §7)

N/A — no block kind is added; Ordin shows nothing of team knowledge as a block.

### `RefResolver`

N/A — no `EntityRef` variant is added.

### Events

No new event kind. A change of a workspace's team knowledge status is announced with the existing `workspace_changed` (the Team Knowledge Service appends it, WorkspaceService.md amendment January 2026 — Team knowledge); a share whose send failed with a repair action raises the existing `needs_you_changed` (the Push Service appends it).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the team knowledge group is in the catalogue
    When the static command list is read
    Then it contains "team_knowledge.status", "team_knowledge.turn_on", "team_knowledge.get", "team_knowledge.share" and "memory.keep_checked"
    And it holds 350 commands in 37 groups (352 after ADR-104; 354 in 38 groups after ADR-106)

  Scenario: Error path — an unknown choice kind in team_knowledge.turn_on
    When team_knowledge.turn_on is sent with choice {"kind":"local_folder","path":"/tmp/x"}
    Then the reply is an error with code SVC_INPUT_INVALID

  Scenario: Edge case — team knowledge over the Remote API
    Given a desktop connected to the person's own ordin server
    When the screens send team_knowledge.status for a server workspace
    Then the Remote API answers POST /v1/team_knowledge/status with the server's TeamKnowledgeStatus
```

### Gaps

None.

---

## Amendment January 2026 — Page links (ADR-104)

> Source: decision 84 (the user's decisions K1–K9 of January 2026); HLD revision 0.52.0, ADR-104. Owner: `components/KnowledgeService.md` (amendment January 2026 — Page links and editing in other apps, *Service API commands*).

### Command catalogue

The catalogue above lists 2 commands added and 1 changed on January 2026, all in **Knowledge**: `knowledge.accept_suggestion` and `knowledge.dismiss_suggestion` (new); `knowledge.save_edit` (input gains optional `tags` and `aliases`, absent = unchanged, so a client from before the amendment keeps working). Result and input types are the owner's (`PageView` with its added fields, `SuggestionView`, `KnowledgeSuggestionId`). Both new commands keep the `Interactive` budget. The count line reads 352 commands in 37 groups.

### Blocks, `RefResolver`, Events

N/A — no block kind, no `EntityRef` variant, no event kind is added. A page changed by a rename rewrite or an adoption is announced as any page change is.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the suggestion commands are in the catalogue
    When the static command list is read
    Then it contains "knowledge.accept_suggestion" and "knowledge.dismiss_suggestion"
    And it holds 352 commands in 37 groups (354 in 38 groups after ADR-106)

  Scenario: Edge case — save_edit without the new fields
    When knowledge.save_edit is sent with {page_id, title, body_markdown} only
    Then the call is accepted and the page's tags and aliases are unchanged

  Scenario: Error path — a suggestion id that is not a UUID
    When knowledge.accept_suggestion is sent with suggestion_id "abc"
    Then the reply is an error with code SVC_INPUT_INVALID
```

### Gaps

None.

## Amendment January 2026 — Jira issues and handovers (ADR-106)

> Source: decision 86 (the user's decisions of January 2026: Jira as a handover place and Jira issues as workspace links); HLD revision 0.54.0, ADR-106. Owners: `components/JiraAdapter.md` (*Service API commands*), `components/HandoverService.md`, `components/WorkspaceService.md` and `components/IdentityAndCredentials.md` (each amendment January 2026 — Jira issues and handovers).

### Command catalogue

The catalogue above lists 2 commands added and 4 changed on January 2026:

- **Jira** (`JiraAdapter.md`), a group of its own after **Confluence**: `jira.connect_token` and `jira.find_issues` (both Local: Jira is on the desktop only). `jira.connect_token` is the only command of the group that carries a secret in; the Command Bridge and the Remote API never log its input, and its result holds no token.
- **Sign-ins** (`IdentityAndCredentials.md`): `identity.sign_in` gains optional `site: String?`, read for `kind: jira` only.
- **Workspaces** (`WorkspaceService.md`): `workspaces.add_link` and `workspaces.propose_link` accept `LinkLocation::JiraIssue { address }`; `workspaces.read_item` also brings a Jira item up to date; `WorkingCopyItem` gains optional `origin_newer_at`; `SourceLinkKind` gains `jira_issue`.
- **Handovers and lifecycles** (`HandoverService.md`): no new command. `handover.update_draft` takes optional `jira_issue: JiraIssueSelection` (`Linked { key }` | `CreateNew`); `handover.open_draft` and `handover.update_draft` return `jira: Option<DraftJiraView { project, options: Vec<JiraIssueOption { key, summary }>, selection, line }>`; `handover.find_people` returns `PersonChoice.jira_account_id`, and `address` may be empty for a Jira choice. `HandoverPlaceType` gains `jira`.

Types named in those rows are the owners' (`JiraUser`, `JiraIssueSummary`, `JiraIssueSelection`, `DraftJiraView`, `JiraIssueOption`, `PersonChoice`, `LinkLocation`, `WorkingCopyItem`); this spec restates them unchanged in the generated TypeScript file. Every added field is optional or a new enum variant, so a client from before the amendment keeps working. The count line reads 354 commands in 38 groups.

### Blocks, `RefResolver`, Events

N/A — no block kind, no `EntityRef` variant and no event kind is added. A Jira link added, read, changed at its origin or removed is announced with the existing `workspace_changed`; a Jira sign-in with the existing `sign_in_changed`; a Jira handover with the existing `needs_you_changed`.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the Jira group is in the catalogue
    When the static command list is read
    Then it contains "jira.connect_token" and "jira.find_issues"
    And it does not contain "jira.find_assignable"
    And it holds 354 commands in 38 groups

  Scenario: Error path — a Jira command over the Remote API
    Given a desktop connected to the person's own ordin server
    When jira.find_issues is sent to the server's backend
    Then the reply is an error with code SVC_COMMAND_LOCAL_ONLY

  Scenario: Edge case — the token is not echoed
    When jira.connect_token is sent with an invalid email
    Then the error reply and the Command Bridge's log line contain no part of the token

  Scenario: Error path — an unknown issue selection kind
    When handover.update_draft is sent with jira_issue {"kind":"pick_any"}
    Then the reply is an error with code SVC_INPUT_INVALID
```

### Gaps

None.

## Amendment January 2026 — Ordin's language and language files (ADR-109)

> Source: decision 89 (the user's decisions of January 2026); HLD revision 0.58.0, ADR-109 (amends ADR-042 and ADR-063), 11 §4, §5.19a and UX-A77. Owners: `components/Localisation.md` and `components/OrdinHost.md` (each amendment January 2026 — Ordin's language and language files).

### Command catalogue

The catalogue above lists 14 commands added on January 2026. Thirteen have scope Any, so a paired desktop lists, edits, imports and exports a personal server's languages through the Remote API with the same commands; the fourteenth, `files.save_export` (scope Local), writes an exported file on this computer, whichever backend produced it:

- **Language** (`Localisation.md`): `localisation.list_languages`, `localisation.list_messages`, `localisation.edit_message`, `localisation.restore_message`, `localisation.restore_language`, `localisation.mark_reviewed`, `localisation.confirm_language`, `localisation.remove_language`, `localisation.export_language` and `localisation.import_language`. `export_language` and `import_language` carry the file's text, never a path (`import_language` takes the 9 MiB body of the text-save commands, because a 2 MiB file exceeds the 1 MiB default once quoted as JSON): the screens write and read the file where the person chooses, as `workflows.export` and `workflows.import` do. In the agent profile each is refused with `LOC_LANGUAGES_UNAVAILABLE` (Localisation decides).
- **Ordin's conversation** (`OrdinHost.md`): `ordin.estimate_language_draft`, `ordin.draft_language` and `ordin.stop_language_draft`.
- **Desktop** (`OrdinDesktop.md`): `files.save_export { file_name, text }` → `SavedCopy`, scope Local: the save dialog, then a write of `text`; the screens use it for `localisation.export_language` and `workflows.export`.

Types named in those rows are the owners' (`LanguageSummary`, `MessageOnly`, `MessageRow`, `LanguageFile`, `ImportReport`, `LanguageChangeKind` from `Localisation.md`; `UsageEstimate` as restated above); this spec restates them unchanged in the generated TypeScript file. The existing `localisation.*` replies and `ActiveLocale` keep their shape. Every addition is a new command, a new event kind or a new enum variant, which the versioning rule above counts as a minor change; the catalogue stays within 1.0, as the earlier amendments' commands do. The count line reads 369 commands in 38 groups.

### Blocks, `RefResolver`, Events

- Blocks, `RefResolver`: N/A — no block kind and no `EntityRef` variant is added. The draft offer Ordin shows is a `question` block; the end of a draft is a `message` block.
- Events: one kind added, `language_changed` (`ServiceEvent::LanguageChanged { locale, change }`, 2C `EventKind::LanguageChanged`; 19 event kinds). The program forwards it from Localisation's `language_changes()` with `Journal::append_event`, exactly as `locale_changed`; it is carried to a paired desktop. A change of the active locale is still `locale_changed`, unchanged.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the language commands are in the catalogue
    When the static command list is read
    Then it contains "localisation.list_languages", "localisation.edit_message", "localisation.import_language" and "ordin.draft_language"
    And it holds 369 commands in 38 groups

  Scenario: Happy path — language_changed carries the owner's payload
    Given the journal record kind language_changed, data {"locale":"ja-JP","change":"draft_progress"}
    When from_journal is called
    Then it returns LanguageChanged with locale "ja-JP" and change DraftProgress

  Scenario: Error path — an import larger than the limit
    When localisation.import_language is sent with an ftl of 2,097,153 bytes
    Then the reply is an error with code LOC_FILE_TOO_LARGE
    And localisation.list_languages lists the same languages as before

  Scenario: Error path — an unknown filter value
    When localisation.list_messages is sent with only "everything"
    Then the reply is an error with code SVC_INPUT_INVALID

  Scenario: Edge case — the existing replies keep their shape
    When localisation.get_settings is sent after the amendment
    Then the reply holds exactly the keys active, offered and following_operating_system
```

### Gaps

None.

---

## Amendment January 2026 — Reading other teams' knowledge (ADR-110)

> Source: decision 90 (the user's decisions of January 2026); HLD revision 0.59.0, ADR-110 (amends ADR-103: one own folder, any number of read folders; adds Adopt and folders the company names). Owners: `components/TeamKnowledgeService.md` (amendment January 2026, *Service API commands*), `components/KnowledgeService.md` (amendment January 2026, `knowledge.list_read_pages`), `components/MemoryService.md` (amendment January 2026, the `FactView` fields).

### Command catalogue

The catalogue gains 8 commands, all scope Any (a paired desktop reads and adopts in a personal server's workspaces through the Remote API with the same commands), all with the `Interactive` budget:

| Command | Input | Result | Scope | Effect |
|---|---|---|---|---|
| `team_knowledge.add_read_folder` | `{ ws, choice: TeamFolderChoice }` | `TeamKnowledgeStatus` | Any | Change |
| `team_knowledge.stop_reading` | `{ ws, folder_id: TeamFolderId }` | `TeamKnowledgeStatus` | Any | Change |
| `team_knowledge.adopt` | `{ ws, item: AdoptItem, edit: Option<AdoptEdit> }` | `AdoptResult` | Any | Change |
| `team_knowledge.adopt_again` | `{ ws, item: AdoptItem, edit: Option<AdoptEdit> }` | `AdoptResult` | Any | Change |
| `team_knowledge.keep_as_is` | `{ ws, item: AdoptItem }` | `{}` | Any | Change |
| `team_knowledge.set_description` | `{ ws, path: String, text: Option<String> }` | `TeamKnowledgeStatus` | Any | Change |
| `team_knowledge.check_recall` | `{ ws }` | `TeamKnowledgeStatus` (`busy: checking_recall`; outcome in `last_recall_check`) | Any | Read |
| `knowledge.list_read_pages` | `{ workspace_id, folder_id: Option<TeamFolderId> }` | `{ "items": [PageSummary] }` | Any | Read |

Two existing rows change their input additively: `team_knowledge.check_now` and `team_knowledge.get` take `{ ws, folder_id: Option<TeamFolderId> }`; an absent `folder_id` means the own folder, so an input written before this amendment is still valid. `adopt`, `adopt_again`, `set_description` and `check_recall` reply at once where their work is long: `check_recall` returns the status with `busy` set and runs in the background (TeamKnowledgeService.md §H); `adopt` and `adopt_again` run the memory component's checks and one write before replying; they may exceed the `Interactive` 150 ms p95 target and stay inside the transport's 10 s `Interactive` deadline (Internal Logic step 3), with their own targets in the owners' specs (p95 ≤ 2 s).

Result and input types are the owners' (`TeamFolderId`, `TeamFolderStatus`, `TeamHeld`, `ReviewState`, `RecallCheckReport`, `FolderDescription`, `AdoptItem`, `AdoptEdit`, `AdoptResult`, `TeamBusy::CheckingRecall` from `TeamKnowledgeService.md`; `PageSummary` with its added fields from `KnowledgeService.md`; `FactView` with its added fields from `MemoryService.md`); this spec restates them unchanged in the generated TypeScript file. The count line read 369 commands in 38 groups after ADR-109; it now reads 377 commands in 38 groups.

### Adopting is the person's alone

`team_knowledge.adopt`, `team_knowledge.adopt_again` and `team_knowledge.keep_as_is` rely on the boundary this spec already draws and add one check:

1. **Only screens call the Service API.** A command reaches `dispatch` only through `accept`, with a `CallOrigin` the transport sets (`LocalScreens` over Tauri IPC, `PairedDesktop` over the Remote API) and never from input (*Security* above). There is no `CallOrigin` for a run, a crew member, a workflow step or Ordin: those reach modules only through MCP tools served by the MCP Front, and a module's tool list is its own (`ToolProvider`), not the catalogue.
2. **No tool, step or block names them** (*chosen here*). The catalogue crate exports `pub const PERSON_ONLY_COMMANDS: &[&str] = &["team_knowledge.adopt", "team_knowledge.adopt_again", "team_knowledge.keep_as_is"];`. The Architecture Test asserts that no `ToolProvider`'s tool, no workflow step kind and no block action of 2C wire §7 carries one of these names or calls the Team Knowledge Service's `adopt`, `adopt_again` or `keep_as_is` methods, so a later change cannot open a path without failing `make check`.

A run whose instructions or recalled content ask for another team's knowledge to be copied into the own folder therefore has nothing to call; at most Ordin says so in words, and the person decides in the Adopt dialog (ScreensModule.md amendment January 2026).

### Blocks (2C wire §7)

N/A — no block kind or block action is added; adoption is a dialog of the screens, never a block.

### `RefResolver`

N/A — no `EntityRef` variant is added.

### Events

No new event kind. Every change of a workspace's team knowledge status, including read folders added, stopped, got or blocked, an adoption and a mark raised or cleared, is announced with the existing `workspace_changed`, which the Team Knowledge Service appends; the screens re-read `team_knowledge.status`.

### Error Table (rows added)

N/A — no `SVC_*` code is added. The new `TEAM_*`, `KNOW_*` and `MEM_*` codes are the owners'; each is registered with its sentence and action in the program's `ErrorSentences` as every module code is (`svc.unregistered_code` otherwise).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the new commands are in the catalogue
    When the static command list is read
    Then it contains "team_knowledge.add_read_folder", "team_knowledge.stop_reading", "team_knowledge.adopt", "team_knowledge.adopt_again", "team_knowledge.keep_as_is", "team_knowledge.set_description", "team_knowledge.check_recall" and "knowledge.list_read_pages"
    And it holds 377 commands in 38 groups

  Scenario: Edge case — an old get input is still valid
    When team_knowledge.get is sent with input {"ws": "0192a0f1-1111-7222-8333-444455556666"}
    Then accept returns Accepted and the own folder is got

  Scenario: Error path — an unknown adopt item kind
    When team_knowledge.adopt is sent with item {"kind":"relationship","id":"0192a2d0-1c3e-7a10-8b2f-5d6e7f809a1b"}
    Then the reply is an error with code SVC_INPUT_INVALID at "/input/item"

  Scenario: Edge case — adopting is the person's alone
    When the Architecture Test reads every ToolProvider's tools, every workflow step kind and every block action
    Then none names "team_knowledge.adopt", "team_knowledge.adopt_again" or "team_knowledge.keep_as_is"

  Scenario: Edge case — adopting over the Remote API
    Given a desktop connected to the person's own ordin server
    When the screens send team_knowledge.adopt for a server workspace
    Then the Remote API answers POST /v1/team_knowledge/adopt with the server's AdoptResult
```

### Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111 (amends ADR-030: one memory every agent uses, a graph that is written and used, and all of it visible to the person). Owner of every command and type below: `components/MemoryService.md` (amendment January 2026, *Service API commands*); the answer block's field: `components/OrdinHost.md` (amendment January 2026).

### Command catalogue

The group *What Ordin remembers* gains 13 commands, all scope Any (a paired desktop reads a personal server's memory through the Remote API with the same commands; in the agent profile the hosted agent's owners reach them through its Remote API, RemoteApi.md amendment January 2026), all with the `Interactive` budget:

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `memory.things` | `{ workspace_id \| scope_id, search?, cursor? }` | `ThingPage` | Any | Read | search 1–200 |
| `memory.thing` | `{ workspace_id \| scope_id, id: MemoryThingId }` | `ThingDetail` | Any | Read |  |
| `memory.fact_history` | `{ id }` | `FactHistoryView` | Any | Read |  |
| `memory.fact_origins` | `{ id }` | `OriginsView` | Any | Read |  |
| `memory.keep` | `{ id }` | `FactView` | Any | Change | **Keep** on a fading fact |
| `memory.used` | `{ workspace_id \| scope_id, recall_ids: Vec<MemoryRecallId> }` (1–20) | `MemoryUsedView` | Any | Read | **What Ordin used** |
| `memory.remembered_in_run` | `{ run_id }` | `RememberedInRun` | Any | Read |  |
| `memory.correct_link` | `{ id: MemoryLinkId, kind, from: MemoryThingId, to: MemoryThingId }` | `LinkView` | Any | Change |  |
| `memory.forget_link` | `{ id: MemoryLinkId }` | `LinkForgetReceipt` | Any | Change |  |
| `memory.thing_count` | `{ workspace_id \| scope_id, id }` | `ThingCount` | Any | Read | the numbers the confirmation shows |
| `memory.forget_thing` | `{ workspace_id \| scope_id, id, expected: ThingCount }` | `ThingForgetReceipt` | Any | Change | refused `MEM_THING_COUNT_CHANGED` when the numbers moved |
| `memory.same_as` | `{ workspace_id \| scope_id, keep, merge }` | `ThingView` | Any | Change | **Same as…** |
| `memory.forget_count` | `{ id }` | `ForgetCount` | Any | Read | summaries a **Forget** takes with it |

`workspace_id | scope_id` is exactly one of the two keys, checked by the typed input (`SVC_INPUT_INVALID` at `/input` for both or neither). Two existing results change additively: `FactView` gains `kind`, `true_from`, `true_until`, `replaces`, `fading`, `not_used_since`, `things` and `made_from_count`, and `ForgetReceipt` gains `summaries_removed`. `memory.forget_thing` deletes up to the thing's facts with verification and may exceed the `Interactive` 150 ms p95 target; it stays inside the transport's 10 s `Interactive` deadline, with its own target in the owner's spec (p95 ≤ 5 s for 50 facts).

Result and input types are the owner's (`MemoryThingId`, `MemoryLinkId` and `MemoryRecallId` from `ordin-types`; `ThingPage`, `ThingView`, `ThingDetail`, `PageLabel`, `FactHistoryView`, `OriginsView`, `OriginItem`, `OriginState`, `MemoryUsedView`, `RememberedInRun`, `RememberedItem`, `RememberedBy`, `LinkView`, `LinkBy`, `ThingRef`, `MemoryKind`, `ThingCount`, `ThingForgetReceipt`, `LinkForgetReceipt`, `ForgetCount` from `MemoryService.md`); this spec restates them unchanged in the generated TypeScript file. The count line read 377 commands in 38 groups after ADR-110; it now reads 390 commands in 38 groups.

### Changing memory is the person's alone

Keeping, correcting a link, forgetting a link or a thing and merging two things rely on the boundary of the ADR-110 amendment (*Adopting is the person's alone*): only screens reach `dispatch`. `PERSON_ONLY_COMMANDS` gains `"memory.keep"`, `"memory.correct_link"`, `"memory.forget_link"`, `"memory.forget_thing"` and `"memory.same_as"`, so the Architecture Test's existing assertion covers them (*chosen here*; the existing `memory.correct`, `memory.forget` and `memory.remember_everywhere` were already reachable only from screens and are added to the constant in the same change).

### Blocks (2C wire §7)

`MessageBlock` gains `memory_used: Vec<MemoryRecallId>` (0–20, omitted when empty; 2C wire §7 as amended January 2026). The Ordin Host fills it with the recalls of the turn that produced the answer; the screens send `memory.used` with those ids when the person opens **What Ordin used**. No block kind or block action is added.

### `RefResolver`

N/A — no `EntityRef` variant is added; a thing and a link are opened inside *What Ordin remembers*, never by reference.

### Events

No new event kind. Every change the commands make is announced with the existing `workspace_changed`, which the Memory Service's callers already use for the memory block; the screens re-read what they show.

### Error Table (rows added)

N/A — no `SVC_*` code is added. The new `MEM_*` codes (`MEM_ENTITIES_INVALID`, `MEM_LINKS_INVALID`, `MEM_MADE_FROM_NOT_ALLOWED`, `MEM_MADE_FROM_INVALID`, `MEM_THING_NOT_FOUND`, `MEM_LINK_NOT_FOUND`, `MEM_THING_COUNT_CHANGED`, `MEM_SAME_THING`, `MEM_RECALL_IDS_INVALID`, `MEM_ALREADY_KEPT`, `MEM_SEARCH_INVALID`) and `ID_MEMORY_REF_INVALID` are the owners'; each is registered with its sentence and action in the program's `ErrorSentences` (`svc.unregistered_code` otherwise).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the new memory commands are in the catalogue
    When the static command list is read
    Then it contains "memory.things", "memory.thing", "memory.fact_history", "memory.fact_origins", "memory.keep", "memory.used", "memory.remembered_in_run", "memory.correct_link", "memory.forget_link", "memory.thing_count", "memory.forget_thing", "memory.same_as" and "memory.forget_count"
    And it holds 390 commands in 38 groups

  Scenario: Error path — both a workspace and a scope
    When memory.things is sent with input {"workspace_id":"0192a0f1-1111-7222-8333-444455556666","scope_id":"0192a0f1-2222-7333-8444-555566667777"}
    Then the reply is an error with code SVC_INPUT_INVALID at "/input"

  Scenario: Error path — a malformed thing id
    When memory.thing is sent with id "Borealis"
    Then the reply is an error with code SVC_INPUT_INVALID at "/input/id"

  Scenario: Edge case — changing memory is the person's alone
    When the Architecture Test reads every ToolProvider's tools, every workflow step kind and every block action
    Then none names "memory.keep", "memory.correct_link", "memory.forget_link", "memory.forget_thing" or "memory.same_as"

  Scenario: Edge case — an answer names the recalls it used
    Given an Ordin answer whose turn made recalls R1 and R2
    When the conversation's message block is read
    Then its memory_used is ["R1", "R2"]
```

### Gaps

None.

---

## Amendment January 2026 — Knowledge agents and the Open Knowledge Format (ADR-112, ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-112 (ordin's wiki is an Open Knowledge Format bundle; amends ADR-104 items 1–3 and ADR-057) and ADR-113 (knowledge agents: a team talks to one front door, which asks proxies in parallel within deadlines; amends ADR-097 and ADR-111). Owners: `components/KnowledgeQueryClient.md` (the `knowledge_agents.*` commands, `KnowledgeConnectionView`, `KnowledgeAbout`), `components/KnowledgeService.md` (amendment January 2026 — the page `type` and `description`), `components/MemoryService.md` (amendment January 2026 — `WiderFact`, `WiderStatus` and the `MemoryUsedView` addition).

### Command catalogue

A new group **Knowledge agents** (group `knowledge_agents`, owner `KnowledgeQueryClient.md`) holds 4 commands, all scope Any (a paired desktop connects a personal server's workspace to a front door through the Remote API with the same commands; the server then asks the front door with the person's company sign-in held on that server), all with the `Interactive` budget:

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `knowledge_agents.list` | `{ workspace_id }` | `{ connections: Vec<KnowledgeConnectionView> }` | Any | Read | the workspace's front doors, the person's and the company's, in the order added |
| `knowledge_agents.add` | `{ workspace_id, url }` | `{ connection_id: KnowledgeConnectionId, about: KnowledgeAbout }` | Any | Change | person only (below); reads the front door's `about` with the person's company sign-in before it keeps anything |
| `knowledge_agents.remove` | `{ connection_id: KnowledgeConnectionId }` | `{}` | Any | Change | person only; a company entry is removed for this workspace and not added again |
| `knowledge_agents.details` | `{ connection_id: KnowledgeConnectionId }` | `{ connection: KnowledgeConnectionView, about: KnowledgeAbout }` | Any | Read | `about` read fresh from the front door; unreachable → the client's code, the stored view kept |

`KnowledgeConnectionView` is `{ connection_id, name, url, added_by: SetBy, sources_record: { enabled: bool, retention_days: u32 } | null, last_answered_at: Timestamp | null }`; `sources_record` is `null` until the front door's `about` has been read once. `url` is an `https` address of at most 2,048 bytes (`SVC_INPUT_INVALID` at `/input/url` otherwise, before the owner is called). `knowledge_agents.add` and `knowledge_agents.details` call the front door over the network and may exceed the `Interactive` 150 ms p95 target; they stay inside the transport's 10 s `Interactive` deadline, with their own target in the owner's spec (p95 ≤ 2 s).

One command is added to the existing **Memory** group (owner `MemoryService.md`):

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `memory.more_found` | `{ workspace_id, recall_id: MemoryRecallId }` | `MoreFoundView { recall_id, wider: Vec<WiderFact>, unanswered: Vec<UnansweredSource>, done, expired }` | Any | Read | the late outside facts of one recall, held 30 minutes after it (MemoryService.md SA-MKA-07); `expired: true` after that |

Three existing rows change additively:

- `knowledge.save_edit` input gains optional `type` (one of `Explanation`, `How-to`, `Decision`, `Reference`, `Glossary`, or the value already in the file) and `description` (1–200 characters, one sentence); absent = unchanged, so an input written before this amendment is still valid.
- `knowledge.write` input gains optional `type` (default `Explanation`) and `description` (absent → the Knowledge Service takes the body's first sentence, KnowledgeService.md amendment January 2026).
- `memory.used` result `MemoryUsedView` gains `wider: Vec<WiderUsed>` (0–50, omitted when empty): the sources from knowledge agents that the recall used, each `{ agent, repository, path, commit, section?, page_title?, as_of, answer_id?, more_found }`, with no statement — the recall record keeps sources only, never an outside statement (MemoryService.md SA-MKA-07). `FactView` gains `page_tier` and `page_status` (MemoryService.md amendment January 2026); `PageView` and `PageSummary` gain `type`, `description`, `trust` (a `PageTier`), `status` and `past_stale_after`, and `PageView` also `verified` (KnowledgeService.md amendment January 2026).

Result and input types are the owners' (`KnowledgeConnectionId`, `KnowledgeAnswerId`, `PageTier`, `PageStatus` and `UnansweredSource` from `ordin-types`; `KnowledgeConnectionView`, `KnowledgeAbout` from `KnowledgeQueryClient.md`; `WiderFact`, `WiderUsed`, `WiderStatus`, `MoreFoundView` from `MemoryService.md`; the page fields from `KnowledgeService.md`); this spec restates them unchanged in the generated TypeScript file. The count line read 390 commands in 38 groups after ADR-111; it now reads **395 commands in 39 groups** (four in the new group, one in Memory).

### Connecting a front door is the person's alone

A connection sends the workspace's questions to another installation, so only the person may make or remove one. `PERSON_ONLY_COMMANDS` gains `"knowledge_agents.add"` and `"knowledge_agents.remove"` (*chosen here*, SA-SVC-KA-01); the Architecture Test's existing assertion then covers them: no `ToolProvider`'s tool, workflow step kind or block action carries either name or calls the Knowledge Query Client's `add_connection` or `remove_connection`. When the person asks Ordin in words to connect a front door, Ordin has nothing to call; it answers in words and the person connects in Settings › Knowledge agents (ScreensModule.md amendment January 2026). `knowledge_agents.list` and `knowledge_agents.details` read only and are not in the constant.

### Blocks (2C wire §7)

N/A — no block kind or block action is added. **What Ordin used** keeps reading the answer block's existing `memory_used` ids; the agent facts arrive in `MemoryUsedView.wider`.

### `RefResolver`

N/A — no `EntityRef` variant is added.

### Events

Two kinds are added (*chosen here*, SA-SVC-KA-02; event kinds are single snake_case names, as every existing kind is):

```rust
    /// The workspace's front doors changed (added, removed, a company entry applied or withdrawn,
    /// or a front door's `about` read anew). Reload with `knowledge_agents.list`.
    /// Emitted by the Knowledge Query Client (amended January 2026, ADR-113).
    KnowledgeAgentsChanged { workspace_id: WorkspaceId },
    /// Late results arrived from knowledge agents for one recall, after the first deadline and
    /// before the second. Emitted by the Memory Service (amended January 2026, ADR-113).
    MoreFound { workspace_id: WorkspaceId, recall_id: MemoryRecallId, added: u16, unanswered: Vec<UnansweredSource> },
```

| Kind | Emitted by (writes to the Journal) | Data limits | Carried to a paired desktop |
|---|---|---|---|
| `knowledge_agents_changed` | Knowledge Query Client | — | yes |
| `more_found` | Memory Service | `added` 1–50; `unanswered` 0–8 entries `{ agent (1–200), reason: not_in_time, refused or unreachable }` | yes |

2C `EventKind` gains `KnowledgeAgentsChanged` and `MoreFound`; the catalogue holds 21 event kinds. Neither event carries query text, a statement or a file path.

### Error Table (rows added)

N/A — no `SVC_*` code is added. The new `KQ_*` codes are the Knowledge Query Client's, and the new `KNOW_*` and `MEM_*` codes their owners'; each is registered with its sentence and action in the program's `ErrorSentences` as every module code is (`svc.unregistered_code` otherwise).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the knowledge agents commands are in the catalogue
    When the static command list is read
    Then it contains "knowledge_agents.list", "knowledge_agents.add", "knowledge_agents.remove", "knowledge_agents.details" and "memory.more_found"
    And it holds 395 commands in 39 groups

  Scenario: Error path — a front door address that is not https
    When knowledge_agents.add is sent with {"workspace_id":"0192a0f1-1111-7222-8333-444455556666","url":"http://knowledge.example.org"}
    Then the reply is an error with code SVC_INPUT_INVALID at "/input/url"
    And the Knowledge Query Client is not called

  Scenario: Edge case — connecting a front door is the person's alone
    When the Architecture Test reads every ToolProvider's tools, every workflow step kind and every block action
    Then none names "knowledge_agents.add" or "knowledge_agents.remove"

  Scenario: Edge case — an old page edit is still valid
    When knowledge.save_edit is sent with {page_id, title, body_markdown} only
    Then accept returns Accepted and the page's type and description are unchanged

  Scenario: Happy path — late results are announced without their content
    Given a recall R in workspace W whose second front door answered after the first deadline with 3 facts
    When the Memory Service appends the event
    Then the screens receive more_found with workspace_id W, recall_id R, added 3
    And the event's data holds no query text and no statement
```

### Assumptions

- **SA-SVC-KA-01** `knowledge_agents.add` and `knowledge_agents.remove` are person-only: a run, a crew member, a workflow step or Ordin cannot connect a front door, because a connection sends the workspace's questions to another installation.
- **SA-SVC-KA-02** The two event kinds are named `knowledge_agents_changed` and `more_found`, single snake_case names as every existing kind is; the group's commands keep the dotted `knowledge_agents.*` form of commands.
- **SA-SVC-KA-03** The group `knowledge_agents` is new (the 39th); its commands are scope Any so that a personal server's workspace can be connected from a paired desktop.

### Gaps

None.

## Amendment January 2026 — Releases and updates (ADR-114)

> Source: decision 93 (the user's decisions of January 2026); HLD revision 0.62.0, ADR-114 (each build answers the module groups of its build switches; one signed release feed; a personal server updates itself when the person asks; amends ADR-076, ADR-041 and ADR-072). Owners: this spec (`ServiceHello.groups`, the refusal `SVC_NOT_IN_THIS_VERSION`, the table of groups by build switch), `components/SetupAssistant.md` (amendment January 2026 — the five `setup.*` rows and their types), `components/OrdinDesktop.md` (`SettingsSection::About`). This amendment states which command groups a build answers for the build switches it was compiled with.
>
> *Decoupled January 2026: build switches replace release-numbered features; releases are planned in the maintainers' release roadmap, kept outside the repository.*

### Command catalogue

Five commands are added to the existing **Getting ready and help** group (group `setup`, owner `SetupAssistant.md`), all with the `Interactive` budget:

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `setup.check_update` | `{}` | `{ offer: UpdateOffer? }` | Local | Change | the person's **Check for updates**; one check at a time, a second call while one runs returns that check's result |
| `setup.whats_new` | `{}` | `{ whats_new: WhatsNew? }` | Local | Read | the notes of the version now running, with `seen`; the screens draw the **What's new** card only while `seen` is false |
| `setup.whats_new_seen` | `{ version }` | `{}` | Local | Change | `version` 1–64 characters, semantic-version text |
| `setup.server_update` | `{}` | `{ update: ServerUpdate? }` | Any | Read | answered by a personal server; the desktop's own core and the agent profile → `SETUP_SELF_UPDATE_UNAVAILABLE` |
| `setup.update_server` | `{ version }` | `{ accepted: true }` | Any | Change | person only (SVC-RL-4); the update runs in the background after the reply |

Two existing rows change: `setup.update_offer`'s `UpdateOffer` gains `state` (below), and `setup.install_update` no longer installs by itself: with an offer in state `Ready` it answers `{}` (otherwise `SETUP_UPDATE_NONE`), and the desktop program then runs its quit flow, which asks when sessions are running, and calls `SetupAssistant::finish_update(UpdateFinish::Restart)`, which installs and restarts ordin (`SetupAssistant.md` SET-UP-5, SET-UP-6; `OrdinDesktop.md` DSK-UP-10, DSK-UP-13; amendments January 2026). Both stay `Local`.

The count line read 369 commands in 38 groups, last amended for ADR-109; the amendments for ADR-110, ADR-111 and ADR-112/ADR-113 each stated their own total, the last one **395 commands in 39 groups**. With the five rows above the catalogue holds **400 commands in 39 groups** (the `setup` group is not new). "Groups" in this count line are the catalogue's sections; the `CommandMeta.module` names that `ServiceHello.groups` reports are finer (48 names, table below).

### Types

Restated unchanged from `SetupAssistant.md` (amendment January 2026) in the generated TypeScript file:

```rust
pub struct UpdateOffer { pub version: String, pub notes: String, pub state: UpdateState }
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState { Downloading, Ready, AdministratorNeeded }
pub struct WhatsNew { pub version: String, pub notes: String, pub seen: bool }
pub struct ServerUpdate { pub version: String, pub notes: String, pub self_update: bool }
```

`ServiceHello` gains one field (owner: this spec):

```rust
pub struct ServiceHello {
    pub api_version: ServiceApiVersion,
    pub backend: BackendId,
    pub backend_name: String,
    pub profile: BackendProfile,
    pub last_seq: u64,
    /// The `CommandMeta.module` names whose commands this backend answers, sorted, no repeats
    /// (amended January 2026, ADR-114). The screens draw no entry whose commands belong to a group
    /// absent from this list.
    pub groups: Vec<String>,
}
```

`SettingsSection` gains `About` (wire `about`), last in the enum: the version running, **Check for updates** and **What's new** (`OrdinDesktop.md` and `ScreensModule.md`, amendments January 2026).

### The `ServiceApi` trait and `accept`

- **SVC-RL-1** The `ServiceApi` trait gains one required method, implemented once by each program from the features it was built with *(chosen here: the program is the only place that knows which modules it composed)*:

  ```rust
  /// The `CommandMeta.module` names this implementation answers, sorted. Fixed for the life of
  /// the process; `service.hello` reports the same list in `ServiceHello.groups`.
  fn groups(&self) -> &'static [&'static str];
  ```

- **SVC-RL-2** `Incoming` gains `pub groups: &'a [&'static str]`, which each transport fills with `api.groups()` of the `ServiceApi` it serves (the Command Bridge on the desktop, the Remote API on a personal server) *(chosen here: `accept` keeps its one argument, and the check runs at the place the contract fixes in its order)*.
- **SVC-RL-3** `accept` gains one step between step 5 (`SVC_COMMAND_UNKNOWN`) and step 6 (`SVC_COMMAND_LOCAL_ONLY`): **5a.** `meta.module` not in `incoming.groups` → `SVC_NOT_IN_THIS_VERSION`. The refusal carries the `request_id`; no implementation method is called. A command of an answered group whose behaviour needs a module the build lacks is refused by its owner with the owner's own codes, never with this one (for example `setup.sign_in_harness { harness: "copilot_cli" }` in a build without the `copilot` switch, refused by the Setup Assistant).
- **SVC-RL-4** `PERSON_ONLY_COMMANDS` gains `"setup.update_server"` and `"setup.install_update"`: an update stops running work, so no run, crew member, workflow step or Ordin may start one; the Architecture Test's existing assertion covers them.
- **SVC-RL-5** A test asserts that every program's `groups()` is a subset of the module names in `CATALOGUE`, sorted, with no repeats, and that `service.hello` returns it unchanged.

### Command groups by build switch

Each `CommandMeta.module` name is answered when the build constructs its owner. The build switches are the Cargo features of the programs (`OrdinDesktop.md` and `OrdinServer.md`, amendments January 2026); a group with no switch is answered by every build of the program. A personal server (switch `personal-profile`) answers the groups of the modules its profile composes; the agent profile (switch `agent-profile`) answers none of these through the Service API.

| Build switch | Groups (`CommandMeta.module`) | Owner steps |
|---|---|---|
| none (always answered) | `service`, `app`, `settings`, `files`, `home`, `search`, `workspaces`, `craft`, `push`, `confluence`, `jira`, `sharepoint`, `ordin`, `voice`, `workflows`, `dashboards`, `runs`, `crews`, `approval`, `journal`, `artefacts`, `reports`, `plans`, `history`, `diagnostics`, `profile`, `remote_agents`, `knowledge`, `memory`, `catalogue`, `checker`, `agents`, `projects`, `usage`, `identity`, `setup`, `localisation`, `brand`, `servers` | programs (70, 72); 38, 58, 49, 43, 51, 44, 75, 16, 57, 32, 40, 11, 5, 6, 53, 42, 50, 30, 55, 37, 45, 20, 7, 59, 12, 28, 25 |
| `dictation` | `dictation` | 29 |
| `mail` | `mail` | 54 |
| `slack` | `slack` | 73 |
| `handover` | `handover` | 63 |
| `authoring` | `authoring` | 61 |
| `team-knowledge` | `team_knowledge` | 74 |
| `telegram` | `telegram` | 69 |
| `hosted-agents` | `hosted` | 17 (through the desktop program) |
| `knowledge-agents` | `knowledge_agents` | 76 |

48 names: 39 always answered and 9 behind a switch. The switches `sandbox` and `copilot` add no group: their modules answer through groups that are always answered (the Copilot harness through `setup` and `runs`, the sandbox through Ordin's tools). `servers` is always answered; with no server paired the screens show Settings › Servers empty. `app.open_slack_link` stays in the always-answered `app` group: it opens a link and needs no Slack module. `memory.more_found` stays in the always-answered `memory` group and returns no late facts in a build without the `knowledge-agents` switch.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Command's group (`meta.module`) is not among the groups this build answers | 404 / `Err(permanent)` | `SVC_NOT_IN_THIS_VERSION` | `{"request_id":…,"kind":"error","error":{"code":"SVC_NOT_IN_THIS_VERSION","message":"This needs a newer version of ordin.","action":null}}` |

The Setup Assistant's new codes (`SETUP_SELF_UPDATE_UNAVAILABLE`, `SETUP_UPDATE_NO_ARCHIVE`, `SETUP_UPDATE_REPLACE_FAILED`, `SETUP_UPDATE_IN_PROGRESS`, `SETUP_UPDATE_VERSION_CHANGED`) are its own; each is registered with its sentence and action in the program's `ErrorSentences` as every module code is. Proposed message identifier: `svc-not-in-this-version` "This needs a newer version of ordin."

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the update commands are in the catalogue
    When the static command list is read
    Then it contains "setup.check_update", "setup.whats_new", "setup.whats_new_seen", "setup.server_update" and "setup.update_server"
    And it holds 400 commands in 39 groups

  Scenario: Happy path — the greeting names the groups this build answers
    Given a desktop built with no build switch
    When service.hello is sent
    Then ServiceHello.groups contains "ordin" and "setup"
    And it does not contain "mail", "dictation" or "knowledge_agents"
    And it is sorted with no repeats

  Scenario: Error path — a command whose group the build lacks
    Given a desktop built with no build switch
    When mail.list_messages is sent with a well-formed envelope
    Then the reply is an error with code SVC_NOT_IN_THIS_VERSION and http_status 404
    And no ServiceApi method is called

  Scenario: Edge case — an absent group is refused before the local-only rule
    Given a personal server whose build lacks the group "dictation"
    When a paired desktop sends dictation.start
    Then the reply is an error with code SVC_NOT_IN_THIS_VERSION

  Scenario: Edge case — an unknown command is still unknown
    Given a desktop built with no build switch
    When mail.no_such_verb is sent
    Then the reply is an error with code SVC_COMMAND_UNKNOWN

  Scenario: Edge case — starting an update is the person's alone
    When the Architecture Test reads every ToolProvider's tools, every workflow step kind and every block action
    Then none names "setup.update_server" or "setup.install_update"

  Scenario: Edge case — an older desktop reads a newer greeting
    Given a ServiceHello JSON with a "groups" field
    When it is read by the generated TypeScript of this amendment
    Then groups is a string array
```

### Assumptions

- **SA-SVC-RL-01** A command group is the `CommandMeta.module` name, the prefix before the dot; the screens hide entries by it.
- **SA-SVC-RL-02** The check of SVC-RL-3 runs after the unknown-command check so that a misspelt command is still reported as unknown, and before the local-only check so that a paired desktop learns the server's version is the cause.
- **SA-SVC-RL-03** The Command Bridge (`CommandBridge.md`) and the Remote API (`RemoteApi.md`, including its direct `accept` call for `approval.decide`) fill `Incoming.groups` with `api.groups()`; neither transport decides groups itself.

### Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (knowledge inherits its readers; the person's own ordin links their accounts at each front door). Owners: `components/KnowledgeQueryClient.md` (`ConnectionAccount`, `AccountLinkState`, the `knowledge_agents.details` result), `components/MemoryService.md` (`MemoryUsedView.wider_unchecked`), both amendments January 2026.

### Command catalogue — no command added

Linking is done by the Knowledge Query Client on its own when sign-ins change; the person has nothing to press, so no command is added and `PERSON_ONLY_COMMANDS` is unchanged. Two results gain fields, which the versioning rule counts as a minor change:

| Command | Change |
|---|---|
| `knowledge_agents.details` | result `{ connection, about, accounts: Vec<ConnectionAccount> }`; one entry per `about.accounts` entry, in that order; empty when the front door checks nobody per person |
| `memory.used` | `MemoryUsedView` gains `wider_unchecked: bool` — true when a front door asked for this recall said some sources could not be checked |

`ConnectionAccount` is `{ system: SourceSystem, state: AccountLinkState }` with `AccountLinkState` = `linked` | `not_signed_in` | `refused`; SharePoint never appears (it is delegated per question, never linked). `about` (`KnowledgeAbout`) gains `accounts: Vec<AccountNeed { system, host }>` and `sharepoint_check_scope: Option<String>` (2C wire §12 amendment January 2026). No account id, account label or token crosses the Service API.

The count line stays **400 commands in 39 groups**.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Service API shapes for per-document read access (ADR-115)

  Scenario: Happy path — details lists the checked accounts
    Given a connection whose about has accounts [{"system":"github","host":"github.com"},{"system":"confluence","host":"acme.atlassian.net"}]
    And the person's GitHub account is linked and Confluence is not signed in
    When knowledge_agents.details is called for that connection
    Then accounts is [{"system":"github","state":"linked"},{"system":"confluence","state":"not_signed_in"}]

  Scenario: Edge — a front door that checks nobody per person
    Given a connection whose about has accounts []
    When knowledge_agents.details is called
    Then accounts is []

  Scenario: Happy path — memory.used says some sources could not be checked
    Given a recall whose front door answer carried unchecked_sources true
    When memory.used is called for it
    Then the view's wider_unchecked is true

  Scenario: Error — details for an unknown connection
    When knowledge_agents.details is called with a connection_id that does not exist
    Then the command fails with KQ_CONNECTION_NOT_FOUND and no accounts are returned

  Scenario: Contract — the catalogue count is unchanged
    When the catalogue unit test counts the commands
    Then it counts 400
```

## Amendment 2026-01-10 — Language servers ask first

`projects.language_servers_answer` and `projects.answer_language_servers` are added, because a language server runs a repository's own code and starts only after the person's yes (`ProjectService.md` SA-RECON-11). `EditorSession` gains `asks_to_start_servers`, and `CodingTask` gains `problems_unchecked`. The count line reads **402 commands in 39 groups**.

## Amendment January 2026 — A linked folder's automatic publishing (FU-127, command side)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Closes the command half of `WorkspaceService.md` G-AUTO-1 and `ScreensModule.md` G-SF-3. Owner: `components/WorkspaceService.md`, amendment 2026-01-10 — Automatic Confluence publishing and agents that can run commands (`ConfluencePublishing`, `confluence_publishing`, SA-WS-AUTO-01 to 03, Internal Logic 8.7). Screens: `components/ScreensModule.md` SM-SF-7 and the `confluencePublishing` function of `lib/backend/wiring.ts`. Every section above stays in force.

- **SVC-AUTO-1** One command is added to the **Workspaces** group (owner `WorkspaceService.md`):

  | Command | Input | Result | Scope | Effect | Note |
  |---|---|---|---|---|---|
  | `workspaces.confluence_publishing` | `{ confluence_link_id }` | `ConfluencePublishing` | Any | Read | the program delegates to `WorkspaceService::confluence_publishing(confluence_link_id)`; nothing is stored |

  - **Input.** `confluence_link_id: ConfluenceLinkId` (2C, UUIDv7 in its 36-character lowercase form); unknown fields refused, as for every command.
  - **Result.** `ConfluencePublishing { confluence_link_id: ConfluenceLinkId, automatic: AutomaticPublishing, turned_off_by: Vec<Grantee> }`, restated field for field from the owner: `automatic` is `"on"` or `"off"` (`AutomaticPublishing`, ConfluenceAdapter.md, `snake_case`); `turned_off_by` holds 2C `Grantee` values (`agent_instance` | `crew`) in the order of their grants' `granted_at`, without repeats, and is empty when `automatic` is `"on"` (SA-WS-AUTO-03). Example: `{"confluence_link_id":"0190f5a2-…","automatic":"off","turned_off_by":[{"kind":"agent_instance","agent_instance_id":"0190f5a3-…"}]}` (2C `Grantee` is tagged by `kind`, `snake_case`).
  - **Errors.** The owner's codes, passed through with their sentences as for every module code: `WS_LINK_NOT_FOUND` (no link with that id; not retryable), `WS_FILES_FAILED` (the link's place cannot be read; retryable), `WS_DATASTORE_UNAVAILABLE` (retryable). A link in state `proposed` or `paused` answers its value; it is not an error (Internal Logic 8.7 of the owner).
  - **Who may call it.** It reads only and is not added to `PERSON_ONLY_COMMANDS`. No tool, workflow step or block action is given it.
  - **Screens.** `confluencePublishing(backend, confluence_link_id)` in `lib/backend/wiring.ts` becomes `BackendHandle.call` of `workspaces.confluence_publishing { confluence_link_id }`, and the local mirror `ConfluencePublishingView` is replaced by the generated `ConfluencePublishing`; SM-SF-7 is unchanged and draws the publishing line from then on (ScreensModule.md SM-SF-10).

The count line reads **403 commands in 39 groups** (402 after the amendment of 2026-01-10, plus this one).

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `workspaces.confluence_publishing` names a link that does not exist | 200 (`kind: error`) | `WS_LINK_NOT_FOUND` | `{"request_id":…,"kind":"error","error":{"code":"WS_LINK_NOT_FOUND", "message": <the owner's sentence>, "action": <the owner's action>}}` |
| The link's place cannot be read | 200 (`kind: error`), retryable | `WS_FILES_FAILED` | as above, with that code |
| The Workspace Service's tables are unreachable | 200 (`kind: error`), retryable | `WS_DATASTORE_UNAVAILABLE` | as above, with that code |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: workspaces.confluence_publishing (FU-127, command side)

  Scenario: Happy path — a folder with no command-running grant publishes automatically
    Given an active Confluence link L whose linked folder no command-running grantee can reach
    When workspaces.confluence_publishing is called with L's id
    Then the result is {"confluence_link_id": L, "automatic": "on", "turned_off_by": []}

  Scenario: Happy path — a grant turns automatic publishing off
    Given agent instance A may run commands and has a read_and_change grant on L's linked folder
    When workspaces.confluence_publishing is called with L's id
    Then automatic is "off" and turned_off_by is [A as an agent_instance grantee]

  Scenario: Error — an unknown link
    When workspaces.confluence_publishing is called with a confluence_link_id that does not exist
    Then the command fails with WS_LINK_NOT_FOUND

  Scenario: Contract — the catalogue count
    When the catalogue unit test counts the commands
    Then it counts 403
```

### Gaps

None.

## Amendment January 2026 — Commands the screens wait on (FU-138, G-SF-1, G-SF-2)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Every section above stays in force. Counterparts: `components/ScreensModule.md`, amendment January 2026 — *Screens wired to the commands of Phase 11* (SM-WR-1 to SM-WR-9), which closes G-SF-1 and G-SF-2 of that spec; `components/OrdinDesktop.md`, amendment January 2026 — *Phase 11 follow-ups of 2026-01-10* (DSK-REC-33 to DSK-REC-41).
>
> Evidence read: `FileService.md` (`read_version(version_id) -> FileBytes`, `FileBytes`, `Preview`, Internal Logic 4.5, `READ_LIMIT_BYTES`, `FILE_VERSION_NOT_FOUND`); `MemoryService.md` SA-TRUST-04, SA-TRUST-05, SA-CITE-02 and their Error Table rows (`MEM_NOT_HELD`, `MEM_HELD_CHANGED`), with the owner's methods and types as built in `crates/ordin-memory-service/src/held.rs` (`held_writes(place: MemoryPlace)`, `keep_held(place, id, seen_hash) -> RememberReceipt`, `discard_held(place, id)`, `citations_to_confirm(workspace_id) -> Vec<MemoryFactId>`, `confirm_citation(workspace_id, id)`) and `crates/ordin-memory-service/src/model.rs` (`HeldWrite`, `HeldThing`, `HeldLink`); `JournalAndArtefactStore.md` JRNL-REC-1 (`artefact(id) -> Option<Artefact>`); `ProjectService.md` (`project(project_id) -> Project`, `PROJ_NOT_FOUND`); `WorkspaceService.md` (`working_copy_items(link) -> Vec<WorkingCopyItem>`, `read_item(link, origin_path) -> WorkingCopyItem`, §J.4, `WS_ITEM_TOO_LARGE`); 2C `WorkingCopyItem` as built in `crates/ordin-types/src/domain/places.rs` (with `origin_newer_at`); `SetupAssistant.md` SET-UP-25 and this spec's ADR-114 amendment (`UpdateState::AdministratorNeeded`); this spec's ADR-115 amendment (`ConnectionAccount`, `AccountLinkState`); the restated types of `crates/ordin-service-api/src/commands/setup.rs` (two-value `UpdateState`) and the generated `apps/desktop/ui/src/lib/service-api/types.gen.ts` (no `ConnectionAccount`, no `origin_newer_at`).

### Command catalogue

- **SVC-SCR-1 Reading the exact version an approval covers (closes G-SF-1 of ScreensModule.md).** One command is added to the **Files** group:

  | Command | Input | Result | Scope | Effect | Note |
  |---|---|---|---|---|---|
  | `files.read_version` | `{ file_version_id }` | `{ path, preview: Preview }` | Any | Read | the budget of `files.preview`; the program delegates to `FileService::read_version(file_version_id)` and builds the preview from the returned bytes |

  - **Input.** `file_version_id: FileVersionId` (2C, UUIDv7 in its 36-character lowercase form); unknown fields refused.
  - **Result.** `path` is `FileBytes.path` unchanged. `preview` is the File Service's `Preview` (restated as for `files.preview`), built by each program's `ServiceApi` implementation from `FileBytes.bytes` with the rule of FileService.md Internal Logic 4.5, the kind taken from the extension of `path` and the size from the length of `bytes`: an image kind at most 20 MiB → `Image { media_type, bytes_b64 }`; `pdf` at most 64 MiB → `Pdf { bytes_b64 }`; a text-like kind → `Text` with the first `PREVIEW_TEXT_BYTES` decoded as UTF-8 with replacement, `markdown` true for `md` and `markdown`, `truncated` when longer; every other kind, and every kind above its limit → `None` *(chosen here: the operating system's thumbnail of rule 4.5 reads a file on disk, and a version is held only as bytes, so an office kind has no thumbnail here)*. The bytes themselves are never returned outside `preview`.
  - **Errors.** The owner's codes, passed through with their sentences: `FILE_VERSION_NOT_FOUND` (no version with that id; not retryable) and the File Service's refusal of a version above `READ_LIMIT_BYTES` (64 MiB).
  - **Who may call it.** It reads only and is not added to `PERSON_ONLY_COMMANDS`. No tool, workflow step or block action is given it.

- **SVC-SCR-2 Held writes and sources to confirm (closes G-SF-2 of ScreensModule.md).** Five commands are added to the group *What Ordin remembers* (owner `MemoryService.md`), all scope Any, budget `Interactive`. Each takes `workspace_id` and calls the owner with `MemoryPlace::Workspace(workspace_id)` *(chosen here: the screens review a workspace's held writes; a hosted agent's scope is not reviewed through these commands)*:

  | Command | Input | Result | Scope | Effect | Note |
  |---|---|---|---|---|---|
  | `memory.held_writes` | `{ workspace_id }` | `Vec<HeldWrite>` | Any | Read | `held_writes(place)`: at most 200, newest first (SA-TRUST-04) |
  | `memory.keep_held` | `{ workspace_id, id, content_hash }` | `Done` | Any | Change | `keep_held(place, id, content_hash)`; the owner's `RememberReceipt` is not returned *(chosen here: the screens read the list again, SM-SF-4.3)* |
  | `memory.discard_held` | `{ workspace_id, id }` | `Done` | Any | Change | `discard_held(place, id)` |
  | `memory.citations_to_confirm` | `{ workspace_id }` | `Vec<MemoryFactId>` | Any | Read | `citations_to_confirm(workspace_id)`: at most 1,000, newest first (SA-CITE-02) |
  | `memory.confirm_citation` | `{ workspace_id, id: MemoryFactId }` | `Done` | Any | Change | `confirm_citation(workspace_id, id)` |

  - **Inputs.** `id` of `keep_held` and `discard_held` is the held write's `HeldWrite.id`, 1–128 characters with no control character *(chosen here: the owner's id is text; the bound refuses nothing the owner writes)*; `content_hash` is 64 lowercase hexadecimal characters (SHA-256, SA-TRUST-05). Violations → `SVC_INPUT_INVALID` at the field's pointer.
  - **Types restated** field for field from the owner, in `ordin_service_api::commands::memory`, with the owner's `serde` form (no renamed field):

    ```rust
    pub struct HeldThing { pub name: String, pub other_names: Vec<String> }
    pub struct HeldLink { pub from: String, pub kind: String, pub to: String, pub ended: bool,
                          pub confidence_permille: Option<u16> }
    pub struct HeldWrite {
        pub id: String, pub statement: String, pub about: Option<String>, pub fact_class: Option<String>,
        pub cited: Option<String>, pub things: Vec<HeldThing>, pub links: Vec<HeldLink>,
        pub content_hash: String, pub trust_permille: u16, pub written_by: Option<AgentInstanceId>,
        pub held_at: Timestamp, pub needs_check: bool,
    }
    ```

  - **Errors.** The owner's codes, passed through: `MEM_NOT_HELD` (no such held write of the workspace; `keep_held` of a forgotten fact or of a row with no fact id; `confirm_citation` of a fact not waiting), `MEM_HELD_CHANGED` (`keep_held` with a hash that is not the held write's; nothing written), and the existing `MEM_NOT_SUPPORTED`, `MEM_STORE_UNAVAILABLE`, `MEM_MODEL_UNAVAILABLE` and `MEM_INTERNAL` with the meanings of MemoryService.md's Error Table.
  - **Who may call them.** SA-TRUST-04 and SA-CITE-02 say no agent tool reaches these methods. `PERSON_ONLY_COMMANDS` gains `"memory.keep_held"`, `"memory.discard_held"` and `"memory.confirm_citation"`, so the Architecture Test's existing assertion covers them; the two reads are not added.

- **SVC-SCR-3 Reading one artefact by id (FU-138; `ContentRef::Artefact`).** One command is added to the **Artefacts** group:

  | Command | Input | Result | Scope | Effect | Note |
  |---|---|---|---|---|---|
  | `artefacts.get` | `{ artefact_id }` | `{ artefact: Artefact? }` | Any | Read | `Journal::artefact(id)` (JRNL-REC-1); `artefact` absent when no row has the id |

  An unknown id is not an error, because the owner answers `None` and states no code for it; the store error is the Journal's, passed through as for `artefacts.list`. `Artefact` is the 2C type, imported, never restated.

- **SVC-SCR-4 Reading one project by id (FU-138).** One command is added to the **Projects** group:

  | Command | Input | Result | Scope | Effect | Note |
  |---|---|---|---|---|---|
  | `projects.get` | `{ project_id }` | `Project` | Local | Read | `ProjectService::project(project_id)` |

  Scope Local, as `projects.open` and `projects.close`. Errors are the owner's: `PROJ_NOT_FOUND` ("That project is no longer open.") and `PROJ_DATASTORE_UNAVAILABLE`.

- **SVC-SCR-5 A linked issue's items and bringing one in (FU-138; the commands ADR-106 named).** The amendment ADR-106 above says that `workspaces.read_item` brings a Jira item up to date and that `WorkingCopyItem` gains `origin_newer_at`, but no row of the catalogue carries `workspaces.read_item`, and no command returns a `WorkingCopyItem`, so the generated files carry neither. Two commands are added to the **Workspaces** group:

  | Command | Input | Result | Scope | Effect | Note |
  |---|---|---|---|---|---|
  | `workspaces.working_copy_items` | `{ source_link_id }` | `Vec<WorkingCopyItem>` | Any | Read | `WorkspaceService::working_copy_items(source_link_id)` |
  | `workspaces.read_item` | `{ source_link_id, origin_path }` | `WorkingCopyItem` | Any | Change | `WorkspaceService::read_item(source_link_id, origin_path)`: reads an item left unread on first use, and brings a Jira item with `origin_newer_at` up to date (WorkspaceService.md §J.4) |

  `origin_path` is 1–1,024 bytes with no control character, else `SVC_INPUT_INVALID`. `WorkingCopyItem` is the 2C type, imported (with `origin_newer_at`, omitted when absent). Errors are the owner's codes, passed through; WorkspaceService.md's Error Table names `WS_ITEM_TOO_LARGE` for `read_item` of a file above the adapter's limit, and §J.4 maps a failing Jira read as its step 3.5 states. `workspaces.read_item` is not added to `PERSON_ONLY_COMMANDS`: it changes only what the working copy holds to match the origin, as the link's own reads do.

- **SVC-SCR-6 Restated types that lag approved amendments (FU-138; no catalogue change).** Two restatements in `ordin_service_api::commands` do not yet follow amendments already approved, so the generated TypeScript lacks what the screens read. They are corrected to the text above; nothing in this amendment changes their shape:
  1. `UpdateState` is `Downloading | Ready | AdministratorNeeded` (wire `administrator_needed`), as the amendment ADR-114 above states and SetupAssistant.md SET-UP-25 returns. The crate's two-value enum gains the third variant.
  2. `knowledge_agents.details` returns `{ connection, about, accounts: Vec<ConnectionAccount> }`, with `ConnectionAccount { system: SourceSystem, state: AccountLinkState }` and `AccountLinkState` = `linked` | `not_signed_in` | `refused`, as the amendment ADR-115 above states. `AccountNeed` (2C wire, `{ system, host }`) keeps no `state`; the state of a person's account is read from `accounts`, never from `about.accounts`.

The count line reads **413 commands in 39 groups** (403 after the amendment of `workspaces.confluence_publishing`, plus one in Files, five in *What Ordin remembers*, one in Artefacts, one in Projects and two in Workspaces).

### Events

No event kind is added. A kept or discarded held write and a confirmed citation are announced with the existing `workspace_changed`, as the memory commands of the ADR-111 amendment are; the screens re-read what they show.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `files.read_version` names a version that does not exist | 200 (`kind: error`) | `FILE_VERSION_NOT_FOUND` | `{"request_id":…,"kind":"error","error":{"code":"FILE_VERSION_NOT_FOUND","message":"That version is no longer kept."}}` |
| `memory.keep_held`, `memory.discard_held` or `memory.confirm_citation` names nothing waiting | 200 (`kind: error`) | `MEM_NOT_HELD` | `{"request_id":…,"kind":"error","error":{"code":"MEM_NOT_HELD","message":"This fact doesn't need checking."}}` |
| `memory.keep_held` with a hash that is not the held write's | 200 (`kind: error`) | `MEM_HELD_CHANGED` | `{"request_id":…,"kind":"error","error":{"code":"MEM_HELD_CHANGED","message":"This fact changed after you read it. Read it again before keeping it."}}` |
| `memory.keep_held` with `content_hash` not 64 lowercase hexadecimal characters | 200 (`kind: error`) | `SVC_INPUT_INVALID` | `{"request_id":…,"kind":"error","error":{"code":"SVC_INPUT_INVALID","at":"/input/content_hash",…}}` |
| `projects.get` names a project that is unknown | 200 (`kind: error`) | `PROJ_NOT_FOUND` | `{"request_id":…,"kind":"error","error":{"code":"PROJ_NOT_FOUND","message":"That project is no longer open."}}` |
| `workspaces.read_item` of a file above the adapter's limit | 200 (`kind: error`) | `WS_ITEM_TOO_LARGE` | `{"request_id":…,"kind":"error","error":{"code":"WS_ITEM_TOO_LARGE","message":"This document is too large for ordin to read."}}` |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Commands the screens wait on (FU-138, G-SF-1, G-SF-2)

  Scenario: Happy path — the exact version of a text file is read (SVC-SCR-1)
    Given file version V of "notes.md" whose bytes are "# Plan"
    When files.read_version is called with V's id
    Then the result is {"path":"notes.md","preview":{"kind":"text","text":"# Plan","markdown":true,"truncated":false}}

  Scenario: Edge case — an office document's version has no thumbnail (SVC-SCR-1)
    Given file version V of "contract.docx"
    When files.read_version is called with V's id
    Then the preview is {"kind":"none"}

  Scenario: Error path — an unknown version (SVC-SCR-1)
    When files.read_version is called with a file_version_id that does not exist
    Then the command fails with FILE_VERSION_NOT_FOUND

  Scenario: Happy path — a held write is kept by the hash the person saw (SVC-SCR-2)
    Given memory.held_writes for W returns one held write H with content_hash C
    When memory.keep_held is called with W, H's id and C
    Then the result is {} and memory.held_writes for W returns no entry for H

  Scenario: Error path — a held write changed after it was read (SVC-SCR-2)
    Given a held write H whose content_hash is no longer C
    When memory.keep_held is called with W, H's id and C
    Then the command fails with MEM_HELD_CHANGED and nothing is stored

  Scenario: Security — only the person keeps, discards or confirms (SVC-SCR-2)
    When the Architecture Test reads every ToolProvider's tools, every workflow step kind and every block action
    Then none names "memory.keep_held", "memory.discard_held" or "memory.confirm_citation"

  Scenario: Edge case — an artefact id no row has (SVC-SCR-3)
    When artefacts.get is called with an artefact_id that does not exist
    Then the result is {} with no artefact field

  Scenario: Happy path — a project is read by its id (SVC-SCR-4)
    Given an open project P
    When projects.get is called with P's id
    Then the result is the Project P

  Scenario: Happy path — a Jira item is brought up to date (SVC-SCR-5)
    Given an item of a linked issue whose origin_newer_at is set
    When workspaces.read_item is called with its source_link_id and origin_path
    Then the returned WorkingCopyItem has no origin_newer_at

  Scenario: Contract — the generated update state carries the whole-computer case (SVC-SCR-6)
    When the TypeScript generator writes UpdateState
    Then it is "downloading" | "ready" | "administrator_needed"

  Scenario: Contract — the catalogue count
    When the catalogue unit test counts the commands
    Then it counts 413
```

### Open Questions

- **OQ-SCR-1** — *resolved 2026-01-10 as recommended: amendment January 2026 — Resolutions of OQ-SCR-1 and OQ-SCR-2, SVC-SCR-7 and G-SCR-1.* *Reading a crew charter by its id (FU-138; `crew_plan` Start).* The `crew_plan` block carries only `crew_charter_id` and the estimate; `crews.get` takes a crew run id, and no owner method reading a charter by id is public (the Ordin Host keeps charters in its own store and states none in its Public Interface). Options: (a) the Ordin Host gains a public read of a charter by id, carried by a new `crews.charter { crew_charter_id }` command; (b) the Ordin Host puts the charter's purpose, agents and limits into the `crew_plan` block when it shows it. **Recommendation:** (a): the block stays small, and a plan read later (from *Needs you*) shows the charter as it is then. Blocks only the drawing of the crew plan's purpose, agents and limits and its **Start** button; until decided the block draws its estimate only, as now.
- **OQ-SCR-2** — *resolved 2026-01-10 by the short pass: SVC-SCR-8 below and OrdinDesktop.md DSK-REC-42; the field `running` stays open as OrdinDesktop.md OQ-REC-12.* *`workspaces.tree` (Phase 11 "Not built").* WorkspaceService.md names the parts the program composes (`get` and `places`, the Package Catalogue's agents, the Ordin Host's workflows, dashboards and running items, the Knowledge Service's and Memory Service's counts and Home and Search's `needs_you` count), but not the owner method behind each field, and several candidates do not match the field's type: `PackageCatalogue::list(workspace_id) -> Vec<InstalledAgent>` against `agents: Vec<AgentInstance>`; `OrdinHost::workflows(workspace_id) -> Vec<WorkflowInputsView>` against `workflows: Vec<WorkflowId>`; `Journal::list_dashboards` is paged; `Journal::running_runs()` covers every workspace and holds no crew run; no Knowledge Service method counts pages (`list_pages` lists them); `MemoryService::count_workspace_facts` and `HomeSearch::needs_you(Some(workspace_id))` do fit. **Recommendation:** a short scoping pass that names one owner method per field, with the conversion, in OrdinDesktop.md, before the command is built. Blocks the tree's per-workspace groups and counts; the tree's workspace list (`workspaces.list`) is not affected.

## Amendment January 2026 — Resolutions of OQ-SCR-1 and OQ-SCR-2

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Every section above stays in force. Counterparts: `components/ScreensModule.md`, amendment January 2026 — *Resolutions of OQ-WR-1 to OQ-WR-3 and OQ-SCR-1* (SM-WR-11 to SM-WR-14); `components/OrdinDesktop.md`, amendment January 2026 — *Resolutions of 2026-01-10* (DSK-REC-42 to DSK-REC-44).
>
> Evidence read: `CrewOrchestrator.md` *Public Interface* (`check_plan`, `start`, `kill`, `continue_crew`, `get(crew_run_id)`, `running_crew`, `request_take_over`, `recover`, `prune_participant`: none reads a charter by its id); `OrdinHost.md` (*Crew charter (2C domain; written here)*, the `crew_charters` table, steps 10, 14 and 17, `OhError::CharterUnknown` → `OH_CHARTER_UNKNOWN`; its *Public Interface* states no read of a charter by its id); 2C wire `CrewPlanBlock { crew_charter_id, estimate }`; 2C domain `CrewCharter`; the `WorkspaceTree` type of this spec.

### Command catalogue

- **SVC-SCR-7 Reading a crew plan by its charter id (closes OQ-SCR-1 as recommended, option (a)).** One command is specified for the **Crews** group:

  | Command | Input | Result | Scope | Effect | Note |
  |---|---|---|---|---|---|
  | `crews.charter` | `{ crew_charter_id }` | `{ charter: CrewCharter }` | Any | Read | the owner's read of one charter by its id (G-SCR-1) |

  - **Input.** `crew_charter_id: CrewCharterId` (2C); unknown fields refused.
  - **Result.** `charter` is the 2C domain `CrewCharter`, imported, never restated. The screens draw the crew plan's purpose (`purpose`), agents (`members`) and limits (`guardrails`) from it; **Start** sends `crews.start { crew_charter_id }` as now.
  - **Errors.** The owner's codes, passed through. A charter the owner does not hold is `OH_CHARTER_UNKNOWN`, the code the Ordin Host already raises for a charter it does not hold (OrdinHost.md steps 14 and 17).
  - **Who may call it.** It reads only and is not added to `PERSON_ONLY_COMMANDS`.
  - **Entry into the catalogue.** The charters are kept by the Ordin Host (`crew_charters`), and neither its *Public Interface* nor the Crew Orchestrator's states a read of one charter by its id (G-SCR-1). The row enters the catalogue, and the count line moves to **414 commands in 39 groups**, in the same change that adds that owner method. Until then the catalogue holds 413 commands, no program answers `crews.charter`, and the `crew_plan` block draws its estimate only (ScreensModule.md SM-WR-11).

- **SVC-SCR-8 The sources of `workspaces.tree` (closes OQ-SCR-2 by the short pass).** The row and the `WorkspaceTree` type are unchanged. The program composes the result from one owner source per field, named with its conversion in OrdinDesktop.md DSK-REC-42. The field `running` has no single verified source and stays open (OrdinDesktop.md OQ-REC-12); until that is settled, `running` is the empty list.

### Gaps

- **G-SCR-1** *The owner's read of one crew charter.* No public method returns a `CrewCharter` by its `CrewCharterId`: the Crew Orchestrator's `get` takes a crew run id, and the Ordin Host keeps `crew_charters` with no public read in its *Public Interface*. Needed: an amendment of `OrdinHost.md` that states a public read of one charter of a workspace by its id, with its error for an unknown id. Blocks `crews.charter` and the crew plan's purpose, agents, limits and **Start** button; nothing else waits on it.

## Amendment January 2026 — Resolution of G-SCR-1

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Every section above stays in force except where SVC-SCR-9 names what it replaces. Counterparts: `components/OrdinHost.md`, amendment January 2026 — *Reading one crew plan* (OH-WR-2); `components/ScreensModule.md`, SM-WR-11.
>
> Evidence read: SVC-SCR-7 and G-SCR-1 above; `OrdinHost.md` OH-WR-2 (the owner method this command calls, with its error); the 2C domain `CrewCharter`.

### Command catalogue

- **SVC-SCR-9 `crews.charter` enters the catalogue (closes G-SCR-1; replaces the last bullet of SVC-SCR-7).** The row stated in SVC-SCR-7 is in the catalogue:

  | Command | Input | Result | Scope | Effect | Note |
  |---|---|---|---|---|---|
  | `crews.charter` | `{ crew_charter_id }` | `{ charter: CrewCharter }` | Any | Read | `OrdinHost::crews_charter(crew_charter_id)` (OH-WR-2) |

  - **Owner.** The Ordin Host answers it; the program's handler passes `crew_charter_id` to `crews_charter` and returns the charter in `charter`. The error is the owner's: `OH_CHARTER_UNKNOWN` for an id no charter has, and `OH_DATASTORE_UNAVAILABLE`.
  - **The count line** reads **414 commands in 39 groups** (413 after SVC-SCR-1 to SVC-SCR-5, plus this one, in the Crews group). `crews.charter` is not added to `PERSON_ONLY_COMMANDS`.
  - **`workspaces.tree` running items.** SVC-SCR-8's statement that `running` is the empty list ends: OrdinDesktop.md DSK-REC-45 states the source and the conversion.
  - **What it unlocks.** ScreensModule.md SM-WR-11 draws the crew plan's purpose, agents, limits and **Start** when the backend's catalogue lists the command, as that rule states.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Contract — crews.charter is in the catalogue (SVC-SCR-9)
    When the catalogue is listed
    Then it holds 414 commands in 39 groups
    And "crews.charter" is a Read command of scope Any in the Crews group that is not person-only

  Scenario: Happy path — a charter is read by its id (SVC-SCR-9)
    Given the Ordin Host holds charter C
    When crews.charter is called with {"crew_charter_id": C}
    Then the result is {"charter": …} holding C's purpose, members and guardrails

  Scenario: Error path — an unknown charter (SVC-SCR-9)
    When crews.charter is called with an id the Ordin Host does not hold
    Then the reply is an error with code OH_CHARTER_UNKNOWN
```

### Gaps

None. G-SCR-1 above is closed by this amendment and OrdinHost.md OH-WR-2.

## Amendment January 2026 — Answering the rows the program refused (FU-149)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Every section above stays in force. Counterparts: the owners' *Service API commands* tables named in the third column below; `components/OrdinDesktop.md` DSK-REC-48.
>
> Evidence read: the program's `service_api.rs` list of rows it refused with `SVC_NOT_IN_THIS_VERSION`; each owner's *Service API commands* table and public method; SA-CLOSE-01 and SA-CLOSE-06 above; `Checker.md` (the paragraph "The program fills the arguments the screens do not send").

- **SVC-R1-1 Thirteen refused rows are answered.** For each row the program calls the owner method named here, passes the input fields in the owner's serde form, restates the result, and maps an owner error as the Service API maps any module error. No row changes its input, result, scope or effect.

  | Row | Owner call | What the program adds | Errors |
  |---|---|---|---|
  | `brand.get_asset` | `Brand::asset(asset)` | nothing | `BRAND_ASSET_UNKNOWN` for a font index out of range |
  | `ordin.draft_language` | `OrdinHost::draft_language(locale, name)` | the reply `{ accepted: true, messages }` from the returned count | `OH_DRAFT_RUNNING`, `OH_NOTHING_TO_DRAFT` |
  | `craft.open` | `CraftToolkit::open_in_app(place_id, path)` | nothing | the owner's `CRAFT_*` codes |
  | `confluence.find` | `ConfluenceAdapter::find_locations(query, limit)` | nothing | the owner's `CONF_*` codes |
  | `confluence.list` | `ConfluenceAdapter::list_children(location, cursor)` | nothing | the owner's `CONF_*` codes |
  | `confluence.keep_mine` | `ConfluenceAdapter::keep_local(published_copy_id)` | nothing | the owner's `CONF_*` codes |
  | `sharepoint.find` | `SharePointAdapter::find_locations(query, limit)` | nothing | the owner's `SP_*` codes |
  | `sharepoint.list` | `SharePointAdapter::list_children(location, cursor)` | nothing | the owner's `SP_*` codes |
  | `jira.connect_token` | `JiraAdapter::connect_with_token(site, email, token)` | the token as a secret string; it is never logged, journalled or echoed | the owner's `JIRA_*` codes |
  | `jira.find_issues` | `JiraAdapter::search(site, JiraQuery::Text(text), limit)` | `site` from `IdentityService::jira_access()`; no Jira credential → `JIRA_NOT_CONNECTED` | the owner's `JIRA_*` codes |
  | `knowledge.write` | `KnowledgeService::write_page(KnowledgeAuthor::Person, WritePage { …, sources: [], page_id: None })` | a new page with no sources, `type` and `description` from the input | the owner's `KNOW_*` codes |
  | `files.add_place` | `FileService::add_place(AddPlace { path, friendly_name, kind: PersonsFolder })` | the kind `PersonsFolder` (FileService.md, the `add_place` example) | `PLACE_PATH_INVALID` and the owner's other codes |
  | `workspaces.add_email_link` | `WorkspaceService::propose_email_link(workspace_id, link)`, then `confirm_email_link(id)` | the confirmation: the command is the person's act, so the link is proposed and confirmed in one call (WorkspaceService.md, "proposed then confirmed") | the owner's `WS_*` and `CFL_*` codes |

- **SVC-R1-2 Four more rows are answered with arguments the program fills.**

  | Row | Owner call | What the program adds |
  |---|---|---|
  | `agents.installed_files` | `PackageInspector::installed_files(id, folder_path)` | `folder_path` is `PackageCatalogue::get(id)` → `instance.agent_folder_path` (SA-CLOSE-06) |
  | `agents.installed_file` | `PackageInspector::installed_file(id, folder_path, path)` | `folder_path` as for `agents.installed_files`; an unknown instance is `PKG_INSTANCE_UNKNOWN` (SVC-IF-1) |
  | `agents.part_page` | `PackageInspector::part_page(id, part_id, bundle_dir, detail)` | `bundle_dir` is `ResolverAndCompiler::open_bundle(Installers::record(id).plan_hash)` → `Bundle.dir` (SA-CLOSE-06) |
  | `checker.check_installed` | `Checker::check_installed(id, lock, scope)` | `lock` is the same bundle's `lock_file`; `scope` is `PackageCatalogue::source_scope(id)`; `PKG_TRIAL_NO_SCOPE` and `PKG_INSTANCE_UNKNOWN` reach the screen as permanent errors (SA-CLOSE-01) |
  | `checker.check_library_item` | `Checker::check_library_item(scope, kind, identifier)` | `scope` is `CatalogueService::source_scope(home)`, where `home` is the named repository or the bundled catalogue when `agent_repository_id` is absent (SA-CLOSE-01) |

  The program therefore holds the Installers and the Resolver and Compiler as modules of the Service API (`ApiModules.installers`, `ApiModules.resolver`), and the SharePoint and Jira adapters (`ApiModules.sharepoint`, `ApiModules.jira`).

- **SVC-R1-3 Ten rows stay refused with `SVC_NOT_IN_THIS_VERSION`, each for a stated reason.** The refusal is the code `SVC_NOT_IN_THIS_VERSION` and its sentence; no stand-in answer is given.

  | Row | Why it is not answered |
  |---|---|
  | `identity.sign_in` | The spec says the program builds the provider configuration and scopes "for that kind" but states the scopes for none of Microsoft, Google, GitHub, OIDC, Confluence and the mail server; only Slack, GitLab and Jira scopes are fixed inside Identity. Decision D-1 below. |
  | `identity.sign_in_wait`, `identity.complete_relayed_sign_in` | They continue a sign-in that `identity.sign_in` starts. |
  | `identity.save_mail_password`, `identity.save_gitlab_token` | The owner returns a `Credential`; the row's result is a `CredentialView`, and no spec states the conversion. Decision D-1 below. |
  | `identity.import_client_certificate` | It needs the two file pickers in sequence and the profile's `server.client_certificate_name`; the program's shell has no second-picker call. Decision D-2 below. |
  | `identity.relay_sign_in` | OrdinDesktop.md step 14 (a server's relayed Slack sign-in) is not written; it follows `identity.sign_in`. |
  | `confluence.publish_checked` | The owner needs the whole `ConfluenceLink` and its `Place`; the Workspace Service reads a link only by its workspace (`confluence_links(workspace_id)`) and its read of one link by id is private. Decision D-3 below. |
  | `files.save_edit` | The File Service's `write_version` records the write as made by Ordin; no spec states the attribution of a write made by the person. Decision D-4 below. |
  | `setup.help_send` | The owner returns a `HelpRequestRecord` that "the program journals", and no spec names the journal entry's kind or payload. Decision D-5 below. |

- **Decisions the user has not yet made (each with a recommendation).**
  - **D-1** State the scopes per kind for `identity.sign_in` and the conversion `Credential` → `CredentialView`. Recommendation: yes, one short amendment of `IdentityAndCredentials.md` that lists the scopes of each kind and states that the program answers with the credential's row from `IdentityService::list()`.
  - **D-2** Add a "pick a second file" call to the shell. Recommendation: yes, in the same pass; it is small.
  - **D-3** Make the Workspace Service's read of one Confluence link by id public. Recommendation: yes, a one-line amendment of `WorkspaceService.md`.
  - **D-4** State the attribution of a person's edit. Recommendation: a `FileService::write_version` variant for the person (origin `Person`), stated in `FileService.md`.
  - **D-5** Name the journal entry for a help request. Recommendation: `help_request_sent` with the record as its payload, stated in `SetupAssistant.md`.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a delegated row reaches its owner (SVC-R1-1)
    Given the SharePoint Adapter finds the site "Procurement"
    When sharepoint.find is called with {"query": "Procurement", "limit": 10}
    Then the result lists that site

  Scenario: Error path — a draft already runs (SVC-R1-1)
    Given Ordin is drafting a language
    When ordin.draft_language is called with {"locale": "fr-FR", "name": "French"}
    Then the reply is an error with code OH_DRAFT_RUNNING

  Scenario: Error path — Jira is not connected (SVC-R1-1)
    Given no Jira credential exists
    When jira.find_issues is called with {"text": "invoice", "limit": 5}
    Then the reply is an error with code JIRA_NOT_CONNECTED

  Scenario: Happy path — the program fills what the screens never send (SVC-R1-2)
    Given an installed agent whose bundle is on disk
    When agents.part_page is called with the agent's id, a part id and a detail
    Then the Package Inspector is called with the bundle's directory

  Scenario: Error path — a trial of an own draft has no scope (SVC-R1-2)
    When checker.check_installed is called for a trial of an own draft
    Then the reply is an error with code PKG_TRIAL_NO_SCOPE

  Scenario: Error path — a row kept refused (SVC-R1-3)
    When identity.sign_in is called
    Then the reply is an error with code SVC_NOT_IN_THIS_VERSION

  Scenario: Contract — no row changes its shape (SVC-R1-1)
    Then the catalogue still holds 414 commands in 39 groups
```

### Gaps

D-1 to D-5 above: each row stays refused until its owner's spec states what the decision names.

## Amendment January 2026 — The five decisions taken (D-1 to D-5)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). SVC-R1-3 stays in force for the rows SVC-R1-4 does not list. Counterparts: `IdentityAndCredentials.md` (ID-R1-1 to ID-R1-3), `OrdinDesktop.md` (DSK-R1-1), `WorkspaceService.md` (WS-R1-1), `FileService.md` (FS-R1-1), `SetupAssistant.md` (SET-R1-1).

- **SVC-R1-4 Eight more rows are answered; two stay refused.**

  | Row | Owner call | What the program adds |
  |---|---|---|
  | `identity.sign_in` | `IdentityService::begin_sign_in(SignInRequest)` | the request built as ID-R1-1 states; `github`, `google` and `mail_server_single_sign_on` → `CRED_FLOW_NOT_OFFERED` (ID-R1-3) |
  | `identity.sign_in_wait` | `wait_for_sign_in(challenge_id)` | the `CredentialView` of ID-R1-2 |
  | `identity.save_mail_password` | `save_mail_password(account_label, kind, password)` | the `CredentialView` of ID-R1-2; the password is never logged |
  | `identity.save_gitlab_token` | `save_gitlab_token(host, token)` | `host` from the profile's `gitlab.host`; `CRED_FLOW_NOT_OFFERED` when the profile names a `gitlab.client_id` |
  | `identity.import_client_certificate` | `import_client_certificate(name, certificate_pem, key_pem)` | the two picks and the name of DSK-R1-1 |
  | `confluence.publish_checked` | `ConfluenceAdapter::request_publish_checked(link, place, items)` | `link` is `WorkspaceService::confluence_link(confluence_link_id)` (WS-R1-1); `place` is `FileService::place(link.place_id)` |
  | `files.save_edit` | `FileService::write_person_edit(place_id, path, text as UTF-8)` | FS-R1-1 |
  | `setup.help_send` | `SetupAssistant::send_help_request(preview_id, fallback_address)` | the journal entry of SET-R1-1; the outcome answered without `record` |

  `identity.relay_sign_in` and `identity.complete_relayed_sign_in` stay refused with `SVC_NOT_IN_THIS_VERSION`: the second continues a relayed sign-in and returns nothing from which the credential's view can be found, and OrdinDesktop.md step 14 for the first describes a loopback listener on the desktop, a call to a paired server through the Server Connector and two codes (`DSK_RELAY_URL_REFUSED`, `DSK_RELAY_TIMEOUT`) that exist only on a build with a paired server; a build without the `personal-profile` switch has none. It is answered by the build that adds the personal server.

```gherkin
  Scenario: Error path — no Microsoft section (SVC-R1-4)
    When identity.sign_in is called with {"kind": "microsoft"} and the profile has none
    Then the reply is an error with code CRED_PROVIDER_NOT_CONFIGURED

  Scenario: Error path — an unknown Confluence link (SVC-R1-4)
    When confluence.publish_checked is called with an id no link has
    Then the reply is an error with code WS_LINK_NOT_FOUND

  Scenario: Happy path — a person's edit is saved (SVC-R1-4)
    When files.save_edit is called with a place, a path and text
    Then the result is the new FileVersion, whose origin is the person

  Scenario: Error path — the relay row stays refused (SVC-R1-4)
    When identity.relay_sign_in is called
    Then the reply is an error with code SVC_NOT_IN_THIS_VERSION
```

### Gaps

G-ID-1 (`IdentityAndCredentials.md`) for three kinds of `identity.sign_in`; `identity.relay_sign_in` and `identity.complete_relayed_sign_in` wait for the build with a paired server.

## Amendment January 2026 (second) — GitHub and Google sign-in

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Counterpart: `IdentityAndCredentials.md` ID-R1-4 to ID-R1-6.

- **SVC-R1-5** `identity.sign_in` answers `github` and `google` as ID-R1-4 and ID-R1-5 state, in addition to the kinds of SVC-R1-4. `mail_server_single_sign_on` answers `CRED_FLOW_NOT_OFFERED` until OQ-ID-1 is settled; `identity.relay_sign_in` and `identity.complete_relayed_sign_in` stay refused as SVC-R1-4 states.

### Gaps

G-ID-2 (`IdentityAndCredentials.md`).

## Amendment January 2026 (third) — The mail account of `identity.sign_in`

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Counterpart: `IdentityAndCredentials.md` ID-R1-7.

- **SVC-R1-6** The `identity.sign_in` row's input is now `{ kind: CredentialKind, reason: String?, relay_port: u16?, site: String?, mail_account: String? }`; `mail_account` is read for `mail_server_single_sign_on` only and is required for it (ID-R1-7). The result, scope and effect are unchanged, and the catalogue still holds 414 commands in 39 groups. `identity.sign_in` answers `mail_server_single_sign_on` as ID-R1-7 states, which replaces SVC-R1-5's "not offered" for that kind. `identity.relay_sign_in` and `identity.complete_relayed_sign_in` stay refused as SVC-R1-4 states.

- **SVC-R1-7 Commands that wait for the person** (28 January 2026, found when the first-start screen of ScreensModule SM-FS-6 ran in the built program: `setup.run_guided` was answered `SVC_DEADLINE` after 10 s while Claude Code waited for its sign-in). `Budget` gains a fourth value, `WaitsForPerson`, whose deadline in Internal Logic step 3 is 11 minutes (one minute above the owners' 10-minute sign-in wait: SetupAssistant.md §B.2, IdentityAndCredentials.md). The rows `setup.run_guided`, `setup.sign_in_harness` and `identity.sign_in_wait` take it; every other row keeps its budget.

  ```gherkin
  Scenario: Edge case — a guided run waiting for a sign-in is not cut off (SVC-R1-7)
    Given setup.run_guided is waiting for the person to sign in to Claude Code
    When 30 seconds pass
    Then no SVC_DEADLINE is answered and the run goes on until the sign-in ends or 11 minutes pass
  ```

### Gaps

None. G-ID-2 is closed.

## Amendment 2026-01-28 — The memory models download (G-MD-3)

> **Status:** Approved by the user 2026-01-28 ("proceed with recommendations"). Source: the user's decisions of 28 January 2026 (the desktop fetches memory's models on first use, after asking). Every section above stays in force. Counterparts: `components/OrdinDesktop.md` amendment 2026-01-28 (DSK-MD-1 to DSK-MD-12, which answers the commands); `components/ScreensModule.md` amendment 2026-01-28 (SM-MD-1 to SM-MD-6, which sends them).
>
> Evidence read: the `app` rows of the catalogue (`app.quit`, `app.pick_folder`, `app.open_slack_link`: scope `Local`, answered by the desktop program) in `crates/ordin-service-api/src/catalogue/rows.rs`; the event catalogue above (`brand_changed`, journalled "no (desktop-local)"); `crates/ordin-service-api/src/events.rs` (`ServiceEvent`); SVC-R1-6 (414 commands in 39 groups).

- **SVC-MD-1 Four commands in the existing `app` group (chosen here).** All `Local`, `Interactive`, implemented by the desktop program (OrdinDesktop.md DSK-MD-12). *Reason:* the download fills this computer's data folder and is answered by the program, as `app.pick_folder` is; a server's memory is not prepared through it, so the rows are not in `memory`, whose rows are `Any`.

  | Command | Input | Result | Scope | Effect | Note |
  |---|---|---|---|---|---|
  | `app.memory_models` | `{}` | `MemoryModelsView` | Local | Read | the state of memory's models on this computer |
  | `app.download_memory_models` | `{}` | `MemoryModelsView` | Local | Change | **Download** or **Try again**; answers at once, the download runs in the background; refused `DSK_MODELS_NOT_IN_BUILD` in a build with no models input |
  | `app.stop_memory_models_download` | `{}` | `MemoryModelsView` | Local | Change | **Stop**; what was received is kept |
  | `app.decline_memory_models` | `{}` | `MemoryModelsView` | Local | Change | **Not now** |

  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
  #[serde(rename_all = "snake_case")]
  pub enum MemoryModelsState { NotInBuild, Missing, Downloading, Checking, Ready, Failed }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
  #[serde(rename_all = "snake_case")]
  pub struct MemoryModelsView {
      pub state: MemoryModelsState,
      /// The zip's size in bytes; 0 for not_in_build.
      pub size_bytes: u64,
      /// The host name of the zip's address, as written in it (for example "github.com"); empty for not_in_build.
      pub host: String,
      /// Bytes received so far (0..=size_bytes).
      pub received_bytes: u64,
      /// The failure's code (a DSK_MODELS_ code); present only when state is failed.
      #[serde(skip_serializing_if = "Option::is_none")]
      pub failure: Option<String>,
      /// Memory was used while the models were missing, and the person has not answered yet.
      pub ask: bool,
  }
  ```

  The types live beside `AppPickFolderResult` and are generated into the TypeScript types. The catalogue holds **418 commands in 39 groups** (414 + 4; the `app` group is not new).
- **SVC-MD-2 One event kind.** `memory_models_changed`, payload `{ models: MemoryModelsView }`, emitted by the desktop program (OrdinDesktop.md DSK-MD-5 to DSK-MD-11), validation `received_bytes ≤ size_bytes`, journalled: no (desktop-local), as `brand_changed`. `ServiceEvent` gains `MemoryModelsChanged { models: MemoryModelsView }`. It is emitted at most once a second while bytes arrive and on every change of `state` or `ask`; the screens may also read `app.memory_models` at any time.
- **SVC-MD-3 Codes.** The four commands answer the existing `SVC_INPUT_INVALID` for an input other than `{}`, and `DSK_MODELS_NOT_IN_BUILD` as above. The other six `DSK_MODELS_` codes of OrdinDesktop.md's added Error Table rows never come back as a command error: they arrive in `MemoryModelsView.failure`, and the screens show their sentence (EH-SEN-1).

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `app.download_memory_models` in a build with no models input | 200 (`kind: error`) | `DSK_MODELS_NOT_IN_BUILD` | `{"request_id":…,"kind":"error","error":{"code":"DSK_MODELS_NOT_IN_BUILD","message":"This version of ordin can't get memory ready. Install the latest version of ordin."}}` (amended January 2026 (user decision; Approved by the user January 2026)) |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: The memory models commands (SVC-MD-1, SVC-MD-2)

  Scenario: Happy path — the catalogue carries the four rows
    When CATALOGUE is read
    Then it contains "app.memory_models", "app.download_memory_models", "app.stop_memory_models_download" and "app.decline_memory_models", each with scope Local
    And it holds 418 rows

  Scenario: Happy path — the status answers the view
    Given a build with a models input and no models on the computer
    When app.memory_models is sent with input {}
    Then the result has state "missing", size_bytes above 0, host "github.com" for the models-1 input, and no failure

  Scenario: Error path — no models input
    Given a build with no models input
    When app.download_memory_models is sent
    Then the command fails with DSK_MODELS_NOT_IN_BUILD

  Scenario: Edge case — a paired desktop cannot start the download
    When app.download_memory_models arrives with origin PairedDesktop
    Then it is refused with SVC_COMMAND_LOCAL_ONLY

  Scenario: Happy path — progress reaches the screens as events
    Given a download is running
    Then memory_models_changed events arrive on the local backend with received_bytes not decreasing within one attempt and never above size_bytes
```

### Gaps

None.

## Amendment 2026-01-29 — The second-install check kind

> **Status:** Approved 2026-01-29 (user decision: complete the high and medium follow-ups). Source: `components/SetupAssistant.md`, amendment January 2026 (ADR-072), gave `CheckKind` the value `SecondInstall`; the restated `CheckKind` of `ordin_service_api::commands::setup` did not follow it, so the first start report that holds it could not cross the API (SA-RECON-02: the restated type equals the owner's). Every section above stays in force. Counterparts: `components/OrdinDesktop.md` amendment 2026-01-29 (DSK-SC-1, the program runs the start checks); `components/ScreensModule.md` amendment 2026-01-29 (SM-FS-8).
>
> Evidence read: `crates/ordin-setup/src/types.rs` (`CheckKind`, `SecondInstall` after `CraftApps`, wire form `second_install`); `crates/ordin-service-api/src/commands/setup.rs` (the restated `CheckKind`, eight values); `apps/desktop/src/service_api.rs` (`DesktopServiceApi::restate`, which refuses with `INTERNAL` a value the restated type cannot read).

- **SVC-SC-1 `CheckKind` gains `SecondInstall`.** The restated `CheckKind` gains `SecondInstall` after `CraftApps`, wire form `second_install`, exactly as the owner's. It is carried wherever `CheckKind` is: `CheckResult.check` (`setup.run_guided`, `setup.check_now`) and `ProgressStep.check` (`setup.status`, the `setup_progress` event). The generated TypeScript types follow. No command, event, input check or code is added; the catalogue's count of rows is unchanged.

### Acceptance Criteria (Gherkin) — added scenario

```gherkin
Feature: The second-install check kind (SVC-SC-1)

  Scenario: Edge case — a start report with a second install crosses the API
    Given the desktop's start checks found another copy of ordin
    When setup.status is sent
    Then the reply is ok and its progress holds a step whose check is "second_install"
```

### Gaps

None.

## Amendment 2026-01-31 — Commands that open the operating system's dialog wait for the person

> **Status:** Approved 2026-01-31 (user decision: "carry on with the fixes"). Source: the walk through the built program on Windows. In *Add to this workspace › This computer*, **Choose a folder…** sends `app.pick_folder`, which shows the operating system's folder picker and answers when the person has chosen. Its row has the budget `Interactive`, whose deadline in Internal Logic step 3 is 10 s, so a person who took about 12 s to choose a folder got `SVC_DEADLINE` ("That took too long, so ordin stopped waiting. Try again.") and nothing was added. SVC-R1-7 fixed the same fault for the commands that wait for a sign-in. Every section above stays in force.

- **SVC-R1-8 Commands that open a dialog.** The rows `app.pick_folder`, `files.save_export` (the save dialog) and `identity.import_client_certificate` (the file picker for the certificate and its key) take the budget `WaitsForPerson` (11 minutes, SVC-R1-7). Every other row keeps its budget.

  ```gherkin
  Scenario: Edge case — a person who takes time choosing a folder is not cut off (SVC-R1-8)
    Given the person has pressed Choose a folder…
    When 30 seconds pass before they choose one
    Then no SVC_DEADLINE is answered and app.pick_folder answers with the folder chosen
  ```

### Gaps

None.

## Amendment 2026-01-31 — Who a shown block is from

> **Status:** Approved 2026-01-31 (user decision: "agree with all recommendations, fix them"). Source: OrdinHost.md OH-MSG-1 shows the person's message as a block; `block_shown` carried no author, so the screens could not draw it as the person's. Every section above stays in force.

- **SVC-R1-9 `block_shown.author`.** The event `block_shown` gains `author: MessageAuthor?` (`person`, `ordin`, `system`), omitted from the JSON when absent; absent means `ordin`, so events from an earlier backend read as before. `ordin.block` already answers `ConversationBlock.from`.

### Gaps

None.

## Amendment January 2026 — A workspace's Confluence publishing links

> **Status:** amended January 2026 (user decision 2026-01-30, "proceed with recommendations"). Source: Phase 5 of the UI walk on Windows — the folder's screen could not tell which of the workspace's folders publish to Confluence, so it drew no publishing line and no *Ready to publish* list (ScreensModule.md SM-PC-2). The owner's method `WorkspaceService::confluence_links` already exists.

- **SVC-CL-1** One command is added to the **Workspaces** group (owner `WorkspaceService.md`): `workspaces.confluence_links { workspace_id }` → `Vec<ConfluenceLink>`, the workspace's publishing links in the order they were made, any state; effect Read, scope Any, budget Interactive. Errors are the owner's (`WS_WORKSPACE_NOT_FOUND`, `WS_DATASTORE_UNAVAILABLE`). The catalogue counts 419 commands, 408 of them Interactive.

```gherkin
  Scenario: Happy path — a workspace's publishing links are listed (SVC-CL-1)
    Given a workspace whose folder Pricing is linked to the Confluence page "Supplier pages"
    When workspaces.confluence_links is sent for the workspace
    Then the answer holds that one link with folder "Pricing" and its state
```

### Gaps

None.

## Amendment January 2026 — Try again on a run that stopped

> **Status:** amended January 2026 (user decision: "proceed with recommendations"). Source: Phase 5 of the UI walk on Windows (finding FU-061) — the run-state table of ScreensModule.md gives `interrupted` and `failed` the next action **Try again**, but no command started a run's task again, so the run screen drew none. The owner's method is OrdinHost.md OH-TA-1. Every section above stays in force.

- **SVC-TA-1** One command is added to the **Runs** group, owned by `OrdinHost.md` (the other `runs.*` commands stay the Session Supervisor's): `runs.try_again { workspace_id: WorkspaceId, run_id: RunId }` (unknown fields refused) → `RunsTryAgainResult`, tagged by `kind`: `{ kind: "started", run_id: RunId }` when the task started again as a new run, or `{ kind: "waiting", text: String }` when the conversation asks the person something first (folder access, a pack choice or the estimate; nothing has started). Effect Change, scope Any, budget Interactive; it calls `OrdinHost::try_again_run(workspace_id, run_id)`. Errors are the owner's: `OH_RUN_UNKNOWN`, `OH_RUN_NOT_INTERRUPTED`, `OH_RUN_NOT_REPEATABLE`, `OH_WORKSPACE_UNKNOWN`, `OH_DATASTORE_UNAVAILABLE` and `OH_DEPENDENCY_FAILED`. The catalogue counts 420 commands, 409 of them Interactive.

```gherkin
  Scenario: Contract — runs.try_again starts a stopped run's task again (SVC-TA-1)
    Given the catalogue
    Then runs.try_again is a Change command of scope Any with input { workspace_id, run_id }
    And an input with any other field is refused

  Scenario: Happy path — an interrupted run is tried again (SVC-TA-1)
    Given a run of the document agent that ended interrupted
    When runs.try_again is sent for it
    Then the answer is { kind: "started", run_id } with a new run's id
```

### Gaps

None.

## Amendment January 2026 — Whether Ordin is working

> **Status:** Approved by the user, January 2026 ("approve"; decision of January 2026: "go ahead with phase 1"). Source: the person's own use of the first release — nothing showed while Ordin was working. The emitter is OrdinHost.md OH-WK-1; the screens are ScreensModule.md SM-WK-1.

- **SVC-WK-1 `ordin_turn_changed`** *(amended January 2026)*. A new `ServiceEvent` kind `ordin_turn_changed { workspace_id: WorkspaceId, state: "working" | "idle", doing?: String, since: Timestamp }`, emitted by the Ordin Host and written to the Journal like the other kinds; `doing` 1–80 characters; carried to a paired desktop: yes. Row added to the table of kinds: | `ordin_turn_changed` | Ordin Host (OH-WK-1) | `doing` 1–80 | yes |. The generated `types.gen.ts` and `schema.gen.json` carry it.

```gherkin
  Scenario: Happy path — the working state reaches the screens (SVC-WK-1)
    When the Ordin Host emits ordin_turn_changed working for workspace W
    Then an event of kind ordin_turn_changed with workspace_id W and state working reaches every subscriber in seq order
```

### Gaps

None.

## Amendment January 2026 — Items in the conversation cannot be closed

> **Status:** Approved by the user, January 2026 ("Should 'Close finished' go too? yes"; "Do you also want Close and Reopen gone from the backend? yes"). Source: plan 05 Phase 9; ScreensModule.md SM-CL-1 and OrdinHost.md step 37.

- **SVC-CL-2 Three commands removed.** `ordin.close_item`, `ordin.show_item` and `ordin.close_finished` leave the catalogue, with their input and result types. `ConversationBlock` (the result of `ordin.block`) loses its field `closed`. The catalogue holds **417 commands**, three fewer than the 420 the generated file held before this amendment (the count lines above are the history of earlier amendments and are not rewritten). A caller that sends a removed name is answered `SVC_COMMAND_UNKNOWN`, like any name not in the catalogue.

```gherkin
  Scenario: Edge case — a removed command is unknown (SVC-CL-2)
    When a caller sends ordin.close_item
    Then the reply is an error with the code SVC_COMMAND_UNKNOWN
    And the catalogue holds 417 commands with unique names
```

### Gaps

None.

## Amendment January 2026 — The two help commands removed (plan 05 FU-019)

> **Status:** Approved by the user, January 2026 ("close all the follow ups in plan 5"; "Send to IT … should be removed", "yes" to removing it from the whole product). Source: plan 05 FU-019; `components/SetupAssistant.md` SET-R1-1 (removed); ScreensModule.md SM-ER-3.

- **SVC-CL-3 Two commands removed.** `setup.help_preview` and `setup.help_send` leave the catalogue, with `HelpRequestInput`, `HelpRequestPreview`, `HelpDestinationView`, `HelpDestinationKind`, `HelpPreviewId`, `SetupHelpSendInput` and `SetupHelpSendResult`. `PersonAction` and the wire `RepairAction` lose `send_to_it` (and `PersonAction` loses `ask_it_for_claude_access` and `ask_it_for_copilot_access`): an error that had it has no `action`. The catalogue holds **415 commands**, two fewer than after SVC-CL-2. The rows SVC-R1-4 and decision D-5 above (the journal entry `help_request_sent`) describe what existed before this amendment. A caller that sends a removed name is answered `SVC_COMMAND_UNKNOWN`.

```gherkin
  Scenario: Edge case — a removed help command is unknown (SVC-CL-3)
    When a caller sends setup.help_send
    Then the reply is an error with the code SVC_COMMAND_UNKNOWN
    And the catalogue holds 415 commands with unique names
```

### Gaps

None.

## Amendment January 2026 — One command added: delete an earlier conversation (OH-DC-1)

> **Status:** Approved by the user, January 2026 ("no way to remove earlier conversations"). Source: OrdinHost.md OH-DC-1; ScreensModule.md SM-WR-16, SM-WR-17.

- **SVC-CL-4 `ordin.delete_conversation`.** The catalogue gains `ordin.delete_conversation`, owner Ordin Host, input `{ conversation_id }`, result `{}`, scope Any, effect Change, budget `Interactive`. The catalogue then holds 416 commands.

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `ordin.delete_conversation` | `{ conversation_id }` | `{}` | Any | Change | OH-DC-1; refuses the current conversation and one a later conversation continues |

```gherkin
  Scenario: Edge case — the catalogue holds the new command (SVC-CL-4)
    Then the catalogue holds 416 commands with unique names
    And ordin.delete_conversation is among them
```

## Amendment January 2026 — Starting an agent's task from its form (ADR-119)

> **Status:** Approved by the user January 2026. Source: HLD 09 ADR-119 items 3–5 (decision 101): the form's verb "starts a run of that task with that agent, as a task Ordin starts does … Nothing is sent as a chat message on the person's behalf." No command did this: `runs.start` was removed with no owner ("tasks start through `ordin.answer_block` with a form"), and `ordin.answer_block` sends a form's answer to Ordin as its next turn (OrdinHost.md step 38), which is the chat path ADR-119 rejects. The owner's method is OrdinHost.md OH-TF-1; the screen is ScreensModule.md SM-TF-1 to SM-TF-8. Every section above stays in force.
>
> Evidence read: the catalogue's **Runs** group and SVC-TA-1 (`runs.try_again`, owner Ordin Host, result tagged `started` / `waiting`); the removal note for `runs.start`; the **Agents** group (`agents.list` → `Vec<InstalledAgent>`, restated from PackageCatalogue.md); SVC-CL-4 (416 commands); 2C wire `ContentRef::PlaceFile { place_id, relative_path }`, `BlockId`; `PERSON_ONLY_COMMANDS`.

- **SVC-TF-1 `runs.start_task`.** One command is added to the **Runs** group, owned by `OrdinHost.md` (`start_task_from_form`, OH-TF-1): input `RunsStartTaskInput` (unknown fields refused) → `RunsStartTaskResult`. Effect Change, scope Any, budget Interactive, person only. The catalogue holds one command more than before this amendment (417 after SVC-CL-4's 416).

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `runs.start_task` | `RunsStartTaskInput` | `RunsStartTaskResult` | Any | Change | Owner `OrdinHost.md` `start_task_from_form` (OH-TF-1); added by SVC-TF-1; person only |

```rust
// ordin_service_api::commands::runs (chosen here)
pub struct RunsStartTaskInput {
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    /// `TaskForm.command_id` (PackageCatalogue.md PKG-TF-1), 1–300 characters.
    pub command_id: String,
    /// 0–8 values; at most one per input name. A task without inputs takes one value named
    /// `arguments` of kind `text`.
    pub values: Vec<TaskInputValue>,
    /// The waiting form Ordin opened for this task (2C wire WIRE-TF-1), when the form was sent from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<BlockId>,
}
pub struct TaskInputValue {
    /// `^[a-z][a-z0-9_]{0,31}$`.
    pub name: String,
    #[serde(flatten)]
    pub value: TaskValue,
}
/// Short text, long text, number and date travel as `text`; the core reads number and date from it.
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskValue {
    Text { text: String },                         // 0–4,000 characters
    Document { document: ContentRef },             // `PlaceFile` only
    Documents { documents: Vec<ContentRef> },      // 0–20, each `PlaceFile`
    Choice { choice: String },                     // 0–60 characters
}
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunsStartTaskResult {
    /// The task started; the screen opens `run:<run_id>`.
    Started { run_id: RunId },
    /// Nothing started: the conversation asks the person first (folder access, a pack choice or the
    /// estimate), as for `runs.try_again`. `text` is written for Ordin and is not shown.
    Waiting { text: String },
    /// Nothing started: the values break the task's inputs. One entry per field, in input order.
    Refused { problems: Vec<TaskInputProblem> },
}
pub struct TaskInputProblem {
    pub name: String,
    pub problem: TaskProblem,
    /// For `document_gone`: the file name of the document that is no longer in the workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
}
#[serde(rename_all = "snake_case")]
pub enum TaskProblem { RequiredEmpty, TooLong, NotANumber, NotADate, NotAChoice, DocumentGone, TooFewDocuments, TooManyDocuments, WrongKind, UnknownInput }
```

  Field rules checked by the Service API before the owner runs (`SVC_INPUT_INVALID`): the lengths and counts written above, `ContentRef` variants other than `PlaceFile`, and two values with one name. Everything that depends on the task is the owner's and comes back as `Refused` (OH-TF-1 step 3).

  Errors are the owner's: `OH_WORKSPACE_UNKNOWN`, `OH_AGENT_UNKNOWN`, `OH_COMMAND_UNKNOWN`, `OH_TASK_UNREADABLE` (new, OH-TF-1), `OH_BLOCK_UNKNOWN`, `OH_BLOCK_ANSWERED`, `OH_DATASTORE_UNAVAILABLE`, `OH_DEPENDENCY_FAILED`, and the Package Catalogue's or Session Supervisor's code when one refuses (as `delegate` passes it on in OrdinHost.md step 8.3).

  `PERSON_ONLY_COMMANDS` gains `"runs.start_task"`: the command grants an agent read access to folders when the task starts (OH-TF-1 step 7), so only the person's press of a form's verb may send it. No block action carries it: a task's form block holds its `task` as data (WIRE-TF-1), and the task form screen sends the command itself (ScreensModule.md SM-TF-6, SM-TF-8). No `ToolProvider` tool, workflow step kind or block action calls it — Ordin starts tasks with `delegate` (OH-TF-3) — and the Architecture Test's existing assertion over `PERSON_ONLY_COMMANDS` covers it. The folder access it grants is the access the person saw named on the form before pressing its verb (SM-TF-4).

- **SVC-TF-2 `InstalledAgent` gains `task_forms`.** The restated `InstalledAgent` of the **Agents** group gains `task_forms: Vec<TaskForm>` with `TaskForm { command_id, title, description, inputs: Option<Vec<TaskInput>>, argument_label: Option<String>, startable: bool }` and `TaskInput { name, label, kind, required, hint?, choices, max?, default? }` with `kind` ∈ `short_text`, `long_text`, `document`, `documents`, `choice`, `number`, `date`, restated from PackageCatalogue.md PKG-TF-1 and AgentModel.md AM-TF-2 under SA-RECON-02's contract test. *Amended January 2026 (user decision):* `TaskInput` gains `whole: bool` (default false; a number input whose value is a whole number, AgentModel.md AM-TF-2), a field added to a result, so a minor change. A file in a linked code repository travels as `ContentRef::PlaceFile` with the place of that repository's link, as every document does; `TaskValue` is unchanged. A result gaining a field is a minor change. `types.gen.ts` and `schema.gen.json` carry the new types.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Service API — starting a task from its form (ADR-119)

  Scenario: Contract — runs.start_task is in the catalogue (SVC-TF-1)
    Given the catalogue
    Then runs.start_task is a Change command of scope Any with input { workspace_id, agent_instance_id, command_id, values, block_id? }
    And an input with any other field is refused with SVC_INPUT_INVALID
    And the catalogue holds 417 commands with unique names

  Scenario: Happy path — the document agent summarises a document (SVC-TF-1)
    Given the document agent in workspace W and the document "plans/strategy.pdf" in its linked folder "Docs"
    When runs.start_task is sent with command_id "urn:ordin:default:commands:documents:summarise-document" and values document = that file and length = "One page"
    Then the answer is { kind: "started", run_id }

  Scenario: Error — a required field left empty (SVC-TF-1)
    When runs.start_task is sent for "Summarise a document" without a value for "document"
    Then the answer is { kind: "refused", problems: [{ name: "document", problem: "required_empty" }] } and no task starts

  Scenario: Error — a ContentRef that is not a workspace file (SVC-TF-1)
    When runs.start_task is sent with a document value of kind "artefact"
    Then the reply is an error with the code SVC_INPUT_INVALID

  Scenario: Security — starting a task from a form is the person's alone (SVC-TF-1)
    When the tool lists, workflow step kinds and block actions are read
    Then none names "runs.start_task" and PERSON_ONLY_COMMANDS holds it

  Scenario: Contract — agents.list carries task forms (SVC-TF-2)
    When agents.list is called for a workspace holding the document agent
    Then each entry carries task_forms whose inputs use the kinds short_text, long_text, document, documents, choice, number and date only
```

### Gaps

None.

## Amendment January 2026 — Memory: words, conflicts, passages and the diagram (ADR-118, ADR-120)

> **Status:** Approved by the user January 2026. Owner of the rows: `components/MemoryService.md`, amendment January 2026 (ADR-118, ADR-120). Wire shapes: `phase-2c-shared-types-wire.md` §17 (WIRE-TR-5).

- **SVC-TR-1 Six commands added to group `memory`** (catalogue rows, method and errors as the owner's table): `memory.vocabulary`, `memory.rename_term`, `memory.merge_term`, `memory.keep_conflict_side`, `memory.passages`, `memory.diagram`. `memory.rename_term`, `memory.merge_term` and `memory.keep_conflict_side` join `PERSON_ONLY_COMMANDS`; the other three are reads. A command marked *place* in the owner's table takes `workspace_id` on the desktop and the personal server and `scope_id` in the agent profile; `memory.passages` and `memory.diagram` take `workspace_id` only, and answer `MEM_NOT_SUPPORTED` in the agent profile.
- **SVC-TR-2 One command changed.** `memory.correct` takes `{ "id", "object": ObjectInput }`; a request carrying `statement` is refused by the catalogue's input check (`SVC_INPUT_INVALID`, as any unknown field is).
- **SVC-TR-3 Count.** The catalogue grows by six rows; the catalogue test's row count rises by six from its value at the time this amendment lands.

```gherkin
  Scenario: Happy path — the diagram command is a read
    When the screens send memory.diagram { workspace_id: W }
    Then the Service API calls MemoryService::diagram(W, None) and returns its DiagramView

  Scenario: Error — a person-only command from an agent's caller
    When memory.merge_term is sent by a caller that is not the person
    Then it is refused as every PERSON_ONLY_COMMANDS entry is
```

## Amendment January 2026 — A team workspace (ADR-121)

> **Status:** Approved by the user January 2026. Source: decision 103; HLD revision 0.68.0, ADR-121 items 2, 3, 5, 6 and 8. *Amended January 2026 (user decision):* `team_knowledge.results_choices` added to SVC-TW-1 (**Change…** after sharing reads its own choices). Owner of every row: `components/TeamKnowledgeService.md` (amendment January 2026, *Service API commands*); wire shapes: `phase-2c-shared-types-wire.md` §18 (WIRE-TW-1 to WIRE-TW-4). The existing `team_knowledge.*` commands are extended, not duplicated: **Share Ordin's memory and knowledge pages with your team** (label *amended January 2026 (user decision; Approved by the user January 2026)*) (`team_knowledge.share`) and **Get your team's memory and knowledge pages** (label *amended January 2026 (user decision; Approved by the user January 2026)*) (`team_knowledge.get`) carry a team workspace's changes, results and activity with no change to their inputs; `team_knowledge.status` gains `team_workspace`. Every section above stays in force.
>
> Evidence read: the **Team knowledge** group (seven rows of ADR-103, eight of ADR-110), `PERSON_ONLY_COMMANDS` and its Architecture Test, *Command groups by build switch* (the group is answered only by builds with `team-knowledge`), SVC-TF-1 (the count after ADR-119), SVC-TR-3 (a count stated as a rise); `apps/desktop/ui/src/lib/service-api/commands.gen.ts` (the `team_knowledge.*` entries: `add_read_folder`, `adopt`, `adopt_again`, `candidate_links`, `check_now`, `check_recall`, `get`, `keep_as_is`, `set_description`, `share`, `status`, `stop_reading`, `turn_off`, `turn_on`).

- **SVC-TW-1 Ten commands added to the Team knowledge group** (*amended January 2026 (user decision; Approved by the user January 2026)*: eleven, with `team_knowledge.results_choices` for **Change…** after sharing), all scope Local, budget Interactive. Team workspaces are not offered through a personal server's Remote API (ADR-121 item 9): over the Remote API each of the eleven is refused with `SVC_COMMAND_LOCAL_ONLY`, so no workspace of a personal server is shared or opened as a team workspace, and its `team_knowledge.status` never carries `team_workspace`; the long ones answer at once with `busy` set and end with `workspace_changed` (TeamKnowledgeService.md §H):

| Command | Input | Result | Scope | Effect | Note |
|---|---|---|---|---|---|
| `team_knowledge.workspace_share_preview` | `{ ws, choice: TeamFolderChoice }` | `TeamWorkspaceSharePreview` | Local | Read | what **Share as a team workspace…** will share and what stays here |
| `team_knowledge.share_workspace` | `{ ws, choice: TeamFolderChoice, results: TeamResultsChoice }` | `TeamKnowledgeStatus` (`busy: sharing`) | Local | Change | raises the share card; the workspace becomes a team workspace when it is sent |
| `team_knowledge.workspace_open_preview` | `{ url, branch, path }` | `TeamWorkspaceOpenPreview` | Local | Read | reads the description with the person's own sign-in; stores nothing |
| `team_knowledge.open_workspace` | `{ url, branch, path, name }` | `TeamWorkspaceOpened` (`busy: opening`) | Local | Change | person only (SVC-TW-2) |
| `team_knowledge.pending_grants` | `{ ws }` | `Vec<TeamPendingGrantView>` | Local | Read | what colleagues added that waits for **Allow** |
| `team_knowledge.allow_grant` | `{ ws, grant_id: TeamGrantId }` | `TeamKnowledgeStatus` | Local | Change | person only (SVC-TW-2) |
| `team_knowledge.decline_grant` | `{ ws, grant_id: TeamGrantId }` | `TeamKnowledgeStatus` | Local | Change | person only (SVC-TW-2) |
| `team_knowledge.activity` | `{ ws, cursor?: Cursor }` | `Page<TeamActivityView>` | Local | Read | 50 a page, newest first |
| `team_knowledge.results_choices` (*amended January 2026 (user decision; Approved by the user January 2026)*) | `{ ws }` | `TeamResultsChoices` | Local | Read | where results may be saved, and where they are saved now (TeamKnowledgeService.md TW-K) |
| `team_knowledge.set_results_place` | `{ ws, results: TeamResultsChoice }` | `TeamKnowledgeStatus` | Local | Change | shared at the next share as a workspace change |
| `team_knowledge.leave_workspace` | `{ ws }` | `{}` | Local | Change | **Leave the team workspace**; nothing in the team folder changes |

  Every input carries `deny_unknown_fields`; `url` 1–2,048 bytes, `branch` 1–255 bytes, `path` ≤ 1,024 bytes with no `..`, `name` 1–80 characters after trimming, checked here (`SVC_INPUT_INVALID`) before the owner runs; everything that depends on the folder is the owner's (`TEAM_*`, TeamKnowledgeService.md *Error Table*). `team_knowledge.status`'s result gains `team_workspace: Option<TeamWorkspaceStatus>` and `TeamBusy` gains `opening` — both additive. The catalogue's row count rises by ten from its value when this amendment lands, and by one more for `team_knowledge.results_choices` (*amended January 2026 (user decision; Approved by the user January 2026)*: 434 rows); the catalogue test asserts the eleven names.

- **SVC-TW-2 Person-only.** `PERSON_ONLY_COMMANDS` gains `"team_knowledge.open_workspace"`, `"team_knowledge.allow_grant"` and `"team_knowledge.decline_grant"`: opening grants agents access on this computer, and a colleague's added agent or access is granted only by this person's press (ADR-121 item 5). The Architecture Test's check of that list (Reading other teams' knowledge, *Adopting is the person's alone*) covers them, and also asserts that no `ToolProvider` tool, workflow step kind or block action calls the Team Knowledge Service's `open_workspace`, `allow_grant` or `decline_grant`.

- **SVC-TW-3 Events.** No new event kind. Every change of a team workspace — opened, shared, a colleague's change taken in, a waiting grant recorded or decided, an activity entry recorded, left — is announced with `workspace_changed`, which the Team Knowledge Service and the Workspace Service append; the screens re-read `team_knowledge.status`, and the Team activity screen re-reads `team_knowledge.activity`.

- **SVC-TW-4 Generated files.** `commands.gen.ts`, `types.gen.ts` and `schema.gen.json` gain the ten commands and the types of WIRE-TW-1 to WIRE-TW-3 (*amended January 2026 (user decision; Approved by the user January 2026)*: eleven commands, with `team_knowledge.results_choices` and `TeamResultsChoices`).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Service API — a team workspace (ADR-121)

  Scenario: Contract — the ten commands are in the catalogue (SVC-TW-1)
    Given the catalogue of a build with team-knowledge
    Then it contains "team_knowledge.share_workspace", "team_knowledge.open_workspace", "team_knowledge.allow_grant", "team_knowledge.activity" and "team_knowledge.leave_workspace"
    And it contains "team_knowledge.results_choices" as a Local read (amended January 2026 (user decision; Approved by the user January 2026))
    And none duplicates "team_knowledge.share" or "team_knowledge.get"

  Scenario: Security — allowing a colleague's agent is the person's alone (SVC-TW-2)
    When the tool lists, workflow step kinds and block actions are read
    Then none names "team_knowledge.allow_grant", "team_knowledge.decline_grant" or "team_knowledge.open_workspace"

  Scenario: Error — an unknown field (SVC-TW-1)
    When team_knowledge.open_workspace is sent with {"url":"https://github.com/acme/supplier-team","branch":"main","path":"knowledge","name":"Q3","token":"x"}
    Then it is refused with SVC_INPUT_INVALID and the owner is not called

  Scenario: Edge — not offered over the Remote API (SVC-TW-1)
    When a paired desktop sends team_knowledge.share_workspace to a personal server through the Remote API
    Then it is refused with SVC_COMMAND_LOCAL_ONLY and the owner is not called

  Scenario: Edge — a build without the switch (SVC-TW-1)
    Given a build without team-knowledge
    When team_knowledge.workspace_open_preview is sent
    Then it is refused with SVC_NOT_IN_THIS_VERSION
```

### Gaps

None.

## Amendment January 2026 — Clear logs, and the save dialog's budget (amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person found **Save logs for support…** did nothing and that nothing clears the logs. The log showed `runs.list` answered four times and `diagnostics.support_export` never sent: with no run ticked, **Save…** stayed disabled (ScreensModule.md SM-NS-3, amended). The walk also found that the two commands that open the save dialog besides `files.save_export` kept the 10 s `Interactive` budget, so a person who took longer than 10 s in the dialog was cut off. Counterparts: OrdinDesktop.md DSK-LOG-1 to DSK-LOG-4 and DSK-REC-41, ScreensModule.md SM-NS-3a, ErrorHandling.md, phase-2c-shared-types-wire.md §19. Every section above stays in force.

- **SVC-R1-10 The save dialog waits for the person.** The rows `files.save_copy` and `diagnostics.support_export` open the operating system's save dialog (OrdinDesktop.md DSK-REC-41, DSK-REC-43), so they take the budget `WaitsForPerson` (11 minutes, SVC-R1-7), as SVC-R1-8 gave `files.save_export`. The window's own list of commands it never times out names them too (ScreensModule.md SM-FS-6). Every other row keeps its budget. amended January 2026 (user decision; Approved by the user January 2026).

  ```gherkin
  Scenario: Edge case — a person who takes time in the save dialog is not cut off (SVC-R1-10)
    Given the person has pressed Save… under Save logs for support…
    When 30 seconds pass before they choose where to save
    Then no SVC_DEADLINE is answered and diagnostics.support_export answers with the path chosen
  ```

- **SVC-LOG-1 `diagnostics.clear_logs`.** One row is added to the catalogue (435 rows): `diagnostics.clear_logs`, input `{}` (`Done`), result `LogsCleared { deleted_files: u32 }` (this spec's type), scope `Local`, effect `Change`, budget `Interactive`, group `diagnostics`. Owner: the desktop program (OrdinDesktop.md DSK-LOG-4); it deletes every past log file and empties the current one while logging carries on. `PERSON_ONLY_COMMANDS` gains `"diagnostics.clear_logs"` (26 names): clearing the logs removes what support could see, so no run, crew member, workflow step or Ordin may do it; no MCP Front tool, workflow step kind or block action names it, and the Architecture Test's person-only check covers it. Errors: `DSK_LOGS_CLEAR_FAILED` (retryable, action `try_again`), passed through. amended January 2026 (user decision; Approved by the user January 2026).

  ```gherkin
  Scenario: Contract — diagnostics.clear_logs is a local change only the person can send (SVC-LOG-1)
    Given the catalogue
    Then diagnostics.clear_logs is Local, Change and Interactive, in PERSON_ONLY_COMMANDS
    And its input {} is accepted and its result is {"deleted_files": <u32>}
  ```

- **SVC-WEB-1 `app.open_web_page`.** One row is added to the catalogue (436 rows): `app.open_web_page`, input `OpenWebPage { page: WebPage }`, result `{}` (`Done`), scope `Local`, effect `Change`, budget `Interactive`, group `app`. `WebPage` is a closed enum of the pages ordin itself names, written `snake_case`: `telegram_botfather`, `atlassian_api_tokens` and `claude_code_install` (Claude Code's official install page for first start, ScreensModule.md SM-FS-9; amended January 2026 (user decision; Approved by the user January 2026)); any other value is refused at the boundary with `SVC_INPUT_INVALID` at `/page`, and the command takes no address. Owner: the desktop program (OrdinDesktop.md DSK-WEB-1), which maps each page to its fixed `https` address and opens it in the person's browser. `PERSON_ONLY_COMMANDS` gains `"app.open_web_page"` (27 names): it opens a program on the person's computer, so only the person's own press sends it; no MCP Front tool, workflow step kind or block action names it, and the Architecture Test's person-only check covers it. Errors: `CRED_BROWSER_FAILED` (retryable, action `try_again`) when the operating system does not open the browser, the code DSK-REC-38 already uses for `app.open_slack_link`. Closes ScreensModule.md G-SW-1. amended January 2026 (user decision; Approved by the user January 2026).

  ```gherkin
  Scenario: Contract — app.open_web_page takes a named page, never an address (SVC-WEB-1)
    Given the catalogue
    Then app.open_web_page is Local, Change and Interactive, in PERSON_ONLY_COMMANDS
    And the input {"page": "telegram_botfather"} is accepted
    And the input {"page": "https://example.com"} is refused with SVC_INPUT_INVALID at /page
  ```

- **SVC-SRV-1 `ServerView.address`.** `ServerView` gains `address: String`, always present: the server's address as the Server Connector stores it, written `host:port`, or `[host]:port` for an IPv6 host. `servers.list` and `servers.connect` carry it; **Connect again…** fills it in (ScreensModule.md SM-SW-12). It is the address the person typed or pasted, so it is no secret; the pairing code and the key never reach it. Owner: ServerConnector.md (step 10, `list()`). Closes ScreensModule.md G-SW-3. amended January 2026 (user decision; Approved by the user January 2026).

  ```gherkin
  Scenario: Contract — a server row carries its address (SVC-SRV-1)
    Given a server paired at 10.20.4.17 port 8443
    When servers.list is called
    Then its ServerView has address "10.20.4.17:8443"
  ```

### Gaps

None.

## Amendment January 2026 — A dismissal answer (amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Owner of the behaviour: OrdinHost.md OH-DIS-1 to OH-DIS-4; the screens: ScreensModule.md SM-DIS-1; the wire: 2C wire §21 WIRE-DIS-1. No command is added and the command count is unchanged. Every section above stays in force.

- **SVC-DIS-1 `ordin.answer_block` takes a dismissal** *(amended January 2026 (user decision; Approved by the user January 2026))*. The row's input `{ workspace_id, block_id, answer: BlockAnswer }` and result `{ accepted: true }` are unchanged; `BlockAnswer` gains the variant `dismissed`, written `{"kind":"dismissed"}` with no other field (`deny_unknown_fields`, as the other variants). `OrdinAnswerBlockInput::check` accepts it with no field rule. The generated screen types gain `{ kind: "dismissed" }` in `BlockAnswer`; `ordin-service-api-gen check` covers it.

  ```gherkin
  Scenario: Contract — a dismissal is its kind alone (SVC-DIS-1)
    When {"kind":"dismissed"} is read as a BlockAnswer
    Then it is BlockAnswer::Dismissed and is written back as {"kind":"dismissed"}
    And the ordin.answer_block input holding it passes the input check
  ```

### Gaps

None.


## Amendment January 2026 — Topics of things (amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Owner: MemoryService.md *Amendment January 2026 — Topics of things* (MEM-TOP-1 to MEM-TOP-7). HLD: `docs/design/ordin-desktop/11-ux-guidelines.md` §5.10 *Facts at scale*.

- **SVC-TOP-1 Two commands added to group `memory`** (amended January 2026 (user decision; Approved by the user January 2026); the catalogue holds 438 rows, 28 of them person-only). `memory.thing_topics` *place* → `ThingTopicsView` (Read, Interactive; MEM-TOP-4). `memory.set_thing_topic` *place*, `id: MemoryThingId`, `topic: string 1–40 | null` → `ThingTopicsView` (Change, Interactive; MEM-TOP-5); it joins `PERSON_ONLY_COMMANDS`, so no tool, workflow step or block action names it. *place* is `workspace_id` on the desktop and the personal server and `scope_id` in the agent profile, exactly one. The boundary refuses a `topic` that is not 1–40 characters once trimmed (`SVC_INPUT_INVALID` at `/input/topic`); the owner checks the rest (`MEM_TOPIC_INVALID`, `MEM_TOPIC_REFUSED`, `MEM_THING_NOT_FOUND`). Wire types: phase-2c-shared-types-wire.md WIRE-TOP-1.

### Error Table

No code is added here; the owner's rows apply.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Topics of things in the catalogue (SVC-TOP-1)

  Scenario: Happy path — the topic commands are in the catalogue, the set is the person's alone
    When the catalogue is read
    Then memory.thing_topics is a read, and memory.set_thing_topic is a change listed in PERSON_ONLY_COMMANDS

  Scenario: Error — a topic longer than 40 characters is refused at the boundary
    When memory.set_thing_topic arrives with a topic of 41 characters, or of spaces only
    Then it is refused with SVC_INPUT_INVALID at /input/topic
```

### Gaps

None.

## Amendment January 2026 — GitHub with a personal access token (amended January 2026; user decision; Approved by the user January 2026)

- **SVC-GH-PAT-1 `identity.save_github_token`.** One row is added to the catalogue (440 rows with `agents.installed_file`, SVC-IF-1 (amended January 2026; user decision; Approved by the user January 2026)): `identity.save_github_token`, input `IdentitySaveGithubTokenInput { token }` (`deny_unknown_fields`; carries a secret in, so its `Debug` prints `[redacted]`), result `CredentialView`, scope `Any`, effect `Change`, budget `Interactive`, group `identity`. The boundary refuses a `token` of 0 bytes or more than 1,024 bytes with `SVC_INPUT_INVALID` at `/input/token`. Owner: Identity and Credentials (`IdentityService::save_github_token`, IdentityAndCredentials.md ID-GH-PAT-1); the desktop program answers it (OrdinDesktop.md DSK-GH-PAT-1). It is the GitHub counterpart of `identity.save_gitlab_token` for a build that carries no GitHub App client id.
- `WebPage` gains `github_tokens`, GitHub's page for making a personal access token, which `app.open_web_page` opens (SVC-WEB-1; the desktop program's address is `https://github.com/settings/tokens`).

### Error Table

No code is added here; the owner's rows apply (`CRED_PASSWORD_INVALID`, `CRED_PROVIDER_REFUSED`, `CRED_PROVIDER_UNREACHABLE`, `CRED_FLOW_NOT_OFFERED`, `CRED_STORE_FAILED`).

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: A pasted GitHub token in the catalogue (SVC-GH-PAT-1)

  Scenario: Happy path — the command is in the catalogue
    When the catalogue is read
    Then identity.save_github_token is a change returning a CredentialView, and app.open_web_page accepts github_tokens

  Scenario: Error — an empty token is refused at the boundary
    When identity.save_github_token arrives with token ""
    Then it is refused with SVC_INPUT_INVALID at /input/token
```

### Gaps

None.

## Amendment January 2026 — Reading one installed file (amended January 2026; user decision; Approved by the user January 2026)

- **SVC-IF-1 `agents.installed_file`.** One row is added to the catalogue (440 rows): `agents.installed_file`, input `InstalledFileInput { agent_instance_id: AgentInstanceId, path: String }` (`deny_unknown_fields`), result `InstalledFileText { path: String, text: String, cut: bool }`, scope `Any`, effect `Read`, budget `Interactive`, group `agents`. The boundary refuses a `path` of fewer than 1 or more than 1,024 characters with `SVC_INPUT_INVALID` at `/input/path`. Owner: Package Inspector (`PackageInspector::installed_file`, PackageInspector.md PKI-IF-1). The program reads the instance with `PackageCatalogue::get(id)` and passes `instance.agent_folder_path` as `folder_path`, as SA-CLOSE-06 does for `agents.installed_files`; the screen sends only the path `agents.installed_files` listed. It backs the reader of **What it's made of** (ScreensModule.md SM-NS-11).

### Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `path` empty or over 1,024 characters | `Err(permanent)` | `SVC_INPUT_INVALID` | pointer `/input/path` |
| The agent instance is not installed | `Err(permanent)` | `PKG_INSTANCE_UNKNOWN` | passed through from the Package Catalogue |
| The path is missing, a folder, or resolves outside the agent folder | `Err(permanent)` | `PKI_PART_NOT_FOUND` | passed through from the Package Inspector |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Reading one installed file (SVC-IF-1)

  Scenario: Happy path — the command is in the catalogue
    When the catalogue is read
    Then it holds 440 rows and agents.installed_file is a read returning an InstalledFileText

  Scenario: Error — an empty path is refused at the boundary
    When agents.installed_file arrives with path ""
    Then it is refused with SVC_INPUT_INVALID at /input/path

  Scenario: Error — an unknown agent
    When agents.installed_file arrives for an agent instance that is not installed
    Then it is refused with PKG_INSTANCE_UNKNOWN
```

### Gaps

None.

## Amendment January 2026 — Settings › Agents and models (SVC-AGM-1) (amended January 2026; user decision; Approved by the user January 2026)

- **SVC-AGM-1 Two commands in the `app` group (amended January 2026; user decision; Approved by the user January 2026).** The catalogue gains two rows (442 rows), both `Local`, `Interactive`, group `app`, answered by the desktop program (`components/OrdinDesktop.md` DSK-AGM-1): `app.agent_settings` (input `Done`, result `AgentSettingsView`, effect `Read`) and `app.change_agent_setting` (input `ChangeAgentSetting`, result `AgentSettingsView`, effect `Change`). Neither is person-only: as with `settings.change`, they change the person's own preferences only. Types, defined in `commands` (this spec owns them): `HarnessReadiness { ready, needs_set_up, not_found, not_in_this_version }`; `LevelModelView { level: ModelLevel, chosen?: String, default_model?: String }`; `KindEffortView { kind: WorkKind, chosen?: String }`; `AgentHarnessView { harness: HarnessName, readiness, models: Vec<LevelModelView> (smaller, standard, most_capable), effort_levels: Vec<String> (empty: no effort control), effort: Vec<KindEffortView> (empty when effort_levels is) }`; `AgentSettingsView { chosen_harness?: HarnessName, organisation_harness: HarnessName, harness_in_use: HarnessName, harnesses: Vec<AgentHarnessView> }`; `ChangeAgentSetting`, tagged by `change`: `harness { harness: HarnessName | null }`, `model { harness, level: ModelLevel, model: String | null }`, `effort { harness, kind: WorkKind, effort: String | null }`, with `deny_unknown_fields`. `HarnessName` and `WorkKind` are the restated profile types. Input limits, refused with `SVC_INPUT_INVALID`: `model` 1–100 ASCII letters, digits, `.`, `_`, `-` (at `/model`); `level` `automatic` (at `/level`); `effort` 1–16 characters (at `/effort`). Whether an effort is one the harness offers is the owner's check (`DSK_PREFERENCE_VALUE_INVALID`). The catalogue unit tests count 442 rows and 429 `Interactive` rows.

```gherkin
Feature: Settings › Agents and models commands (SVC-AGM-1)

  Scenario: Contract — Settings › Agents and models reads and changes the person's own choices (SVC-AGM-1)
    Given the catalogue
    Then app.agent_settings is a local read and app.change_agent_setting a local change, both interactive
    And a model name with a space, an empty name, a name over 100 characters, a model for the automatic level, an unknown kind or an effort over 16 characters is refused with SVC_INPUT_INVALID
```

## Amendment January 2026 — A dashboard saved as a file, or added to a presentation (SVC-DBX-1) (amended January 2026; user decision; Approved by the user January 2026)

- **SVC-DBX-1 Two commands in the `dashboards` group (amended January 2026; user decision; Approved by the user January 2026).** The catalogue gains two rows (444 rows), both `Local`, effect `Change`, budget `WaitsForPerson` (each opens the operating system's own dialog, SVC-R1-8), answered by the desktop program (`components/OrdinDesktop.md` DSK-DBX-1 to DSK-DBX-4): `dashboards.export`, input `DashboardsExportInput { dashboard_id: DashboardId, format: DashboardExportFormat }` (`deny_unknown_fields`; `DashboardExportFormat` is `pdf`, `pptx`, `xlsx` or `docx`), result `SavedCopy { saved_path }`; and `dashboards.add_to_presentation`, input `DashboardsAddToPresentationInput { dashboard_id: DashboardId }` (`deny_unknown_fields`), result `PresentationUpdated { saved_path: String, as_copy: bool }`. No path or file content crosses the boundary: the program chooses the file through its dialogs and reads the report from the Journal. The window never times either out (`WAITS_FOR_PERSON`). The `Interactive` count stays 429.

### Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `format` missing or not one of the four, or an unknown field | `Err(permanent)` | `SVC_INPUT_INVALID` | pointer under `/input` |
| The dashboard is not in the Journal | `Err(permanent)` | `DSK_DASHBOARD_NOT_FOUND` | — |
| None of the dashboard's refresh runs has a report yet | `Err(permanent)` | `DSK_DASHBOARD_NO_REPORT` | — |
| The person cancels the save dialog or the file chooser | `Err(permanent)` | `DSK_DIALOG_CANCELLED` | — |
| The file chooser cannot open | `Err(retryable)` | `DSK_PICKER_UNAVAILABLE` | — |
| The picked file is not a presentation slides can be added to | `Err(permanent)` | `DSK_PRESENTATION_UNREADABLE` | — |
| The file cannot be written where the person chose | `Err(retryable)` | `DSK_SAVE_COPY_FAILED` | — |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: A dashboard saved on this computer (SVC-DBX-1)

  Scenario: Contract — a dashboard is saved as a file, or added to a presentation, on this computer (SVC-DBX-1)
    Given the catalogue
    Then it holds 444 rows, and dashboards.export and dashboards.add_to_presentation are local changes that wait for the person
    And a format other than pdf, pptx, xlsx or docx, or a path in the input, is refused with SVC_INPUT_INVALID
```

### Gaps

None.

## Amendment January 2026 — GitHub with an SSH key (SVC-SSH-1) (amended January 2026; user decision; Approved by the user January 2026)

- **SVC-SSH-1 Four `app` rows (amended January 2026; user decision; Approved by the user January 2026).** The catalogue gains four rows, each scope `Local`, budget `Interactive`, group `app`, answered by the desktop program (OrdinDesktop.md DSK-SSH-1 to DSK-SSH-3): `app.ssh_keys` (input `{}`, result `SshKeysView`, effect `Read`); `app.create_ssh_key` (input `{}`, result `SshKeysView`, effect `Change`); `app.choose_ssh_key` (input `ChooseSshKey { path: string | null }`, `deny_unknown_fields`, result `SshKeysView`, effect `Change`; the boundary refuses a `path` of 0 bytes or more than 4,096 bytes with `SVC_INPUT_INVALID` at `/input/path`); `app.check_ssh_key` (input `{}`, result `SshKeyCheck { outcome: "works" | "refused" | "unreachable" }`, effect `Read`). `SshKeysView` is `{ keys: SshKey[], chosen: string | null }` and `SshKey` is `{ path, name, public_key }`. `app.create_ssh_key` and `app.choose_ssh_key` are added to `PERSON_ONLY_COMMANDS`. `WebPage` gains `github_ssh_keys` for `app.open_web_page`. The catalogue then holds 448 rows.

```gherkin
Feature: The SSH key commands (SVC-SSH-1)

  Scenario: The commands are on this computer only
    When the catalogue is read
    Then app.ssh_keys, app.create_ssh_key, app.choose_ssh_key and app.check_ssh_key have scope Local
    And app.create_ssh_key and app.choose_ssh_key are person-only

  Scenario: An empty path is refused
    When app.choose_ssh_key is sent with path ""
    Then the reply is SVC_INPUT_INVALID
```

## Amendment January 2026 — A knowledge page saved as a PDF or Word document (SVC-KEX-1) (amended January 2026; user decision; Approved by the user January 2026)

- **SVC-KEX-1 One command in the `knowledge` group (amended January 2026; user decision; Approved by the user January 2026).** The catalogue gains one row (449 rows), `knowledge.export`, scope `Local`, effect `Change`, budget `WaitsForPerson` (it opens the operating system's own save dialog, SVC-R1-8), answered by the desktop program (`components/OrdinDesktop.md` DSK-KEX-1, DSK-KEX-2): input `KnowledgeExportInput { page_id: KnowledgePageId, format: KnowledgeExportFormat }` (`deny_unknown_fields`; `KnowledgeExportFormat` is `pdf` or `docx`), result `SavedCopy { saved_path }`. No content, title or path crosses the boundary: the program reads the page by its id and chooses the file through its dialog. The window never times it out (`WAITS_FOR_PERSON`). The `Interactive` count stays 433.

### Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `format` missing or not `pdf` or `docx`, or an unknown field (a path or the page's text) | `Err(permanent)` | `SVC_INPUT_INVALID` | pointer under `/input` |
| The Knowledge Service does not hold the page | `Err(permanent)` | `KNOW_PAGE_NOT_FOUND` | — |
| The person cancels the save dialog | `Err(permanent)` | `DSK_DIALOG_CANCELLED` | — |
| The file cannot be written where the person chose | `Err(retryable)` | `DSK_SAVE_COPY_FAILED` | — |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: A knowledge page saved on this computer (SVC-KEX-1)

  Scenario: Contract — a knowledge page is saved as a PDF or Word document on this computer (SVC-KEX-1)
    Given the catalogue
    Then it holds 449 rows, and knowledge.export is a local change that waits for the person
    And a format other than pdf or docx, a path or the page's text in the input is refused with SVC_INPUT_INVALID
```

### Gaps

None.


## Amendment January 2026 — Confluence connected with an API token (SVC-CONF-TOKEN-1) (amended January 2026; user decision; Approved by the user January 2026)

- **SVC-CONF-TOKEN-1 Two `confluence` rows (amended January 2026; user decision; Approved by the user January 2026).** The catalogue gains two rows (451 rows), each scope `Local`, effect `Change`, budget `Interactive`, group `confluence`, answered by the desktop program; the `Interactive` count becomes 435. Both are added to `PERSON_ONLY_COMMANDS`.
  - `confluence.connect_token`: input `ConfluenceConnectTokenInput { site, email, token }` (`deny_unknown_fields`; `site` a host name, `email` 3–254 characters, `token` 1–1,024 characters), result `ConfluenceUser { account_id, display_name }`. The program calls `ConfluenceAdapter::connect_with_token` (ConfluenceAdapter.md CONF-TOKEN-1). The input carries a secret: it is never logged, journalled or echoed, and its `Debug` form hides the token.
  - `confluence.connect_with_jira`: input `{}`, result `ConfluenceUser`. The program reads the Jira credential through Identity (`jira_access`); when it is a pasted API token, its site, email and token are checked by `connect_with_token` exactly as if pasted, so Confluence is connected on the same Atlassian site. With no Jira credential, or a Jira browser sign-in, the answer is `JIRA_NOT_CONNECTED`.

### Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Site not a host, email or token out of range, or an unknown field | `Err(permanent)` | `SVC_INPUT_INVALID` | pointer under `/input`; never the token |
| Site not a Confluence Cloud site | `Err(permanent)` | `CONF_SITE_INVALID` | — |
| Confluence refuses the email and token | `Err(permanent)` | `CONF_TOKEN_REFUSED` | — |
| Confluence cannot be reached | `Err(retryable)` | `CONF_UNAVAILABLE` | `action: try_again` |
| `confluence.connect_with_jira` with no Jira API token | `Err(permanent)` | `JIRA_NOT_CONNECTED` | — |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Confluence with an API token (SVC-CONF-TOKEN-1)

  Scenario: Contract — Confluence connected with an API token: two local commands only the person can send, and the token is never echoed (SVC-CONF-TOKEN-1)
    Given the catalogue
    Then it holds 451 rows, and confluence.connect_token and confluence.connect_with_jira are local changes only the person can send
    And a bad site, email or token, or an unknown field, is refused with SVC_INPUT_INVALID and the reply never holds the token
```

### Gaps

None.

## Amendment January 2026 — Meeting minutes (SVC-MIN-1 to SVC-MIN-4) (amended January 2026; user decision; Approved by the user January 2026)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Every section above stays in force. Owners: `components/MinutesService.md` (the group's module), `components/OrdinDesktop.md` DSK-MIN-1 to DSK-MIN-5 (the rows the program answers itself), `components/IdentityAndCredentials.md` ID-OBS-1, `components/ScreensModule.md` SM-MIN-1 to SM-MIN-10 (the callers). Wire types: `phase-2c-shared-types-wire.md` §23.
>
> Evidence read: `crates/ordin-service-api/src/catalogue/rows.rs` (one row per command: domain, verb, input, output, reach, effect, interaction; `dashboards` at :165–:168, `knowledge export` at :196, `files import` at :77, `files save_export` at :13); `crates/ordin-service-api/src/commands/mod.rs` (`WebPage`, SVC-WEB-1); this file's *Command groups by build switch*, SVC-R1-8, SVC-KEX-1 and SVC-CONF-TOKEN-1 (451 rows, 435 `Interactive`).

- **SVC-MIN-1 Twenty `minutes` rows (amended January 2026; user decision; Approved by the user January 2026).** The catalogue gains the group `minutes`, twenty rows, every one scope `Local` (desktop only; never on the Remote API), answered by the desktop program. Inputs are `deny_unknown_fields`; ids are UUIDv7 strings.

| Command | Input | Result | Effect | Budget | Calls |
|---|---|---|---|---|---|
| `minutes.status` | `{ workspace_id }` | `MinutesStatus` | Read | Interactive | `MinutesService::status` |
| `minutes.list` | `{ workspace_id, cursor: Cursor? }` | `Page<MinutesSummary>` (50 per page, newest first) | Read | Interactive | Journal `list_minutes` |
| `minutes.get` | `{ minutes_id }` | `Minutes` | Read | Interactive | Journal `minutes` |
| `minutes.transcript` | `{ minutes_id }` | `Transcript` | Read | Interactive | `MinutesService::transcript` |
| `minutes.save_edit` | `{ minutes_id, body (1 byte–1 MiB), base_revision ≥ 1 }` | `Minutes` | Change | Interactive | `save_edit` |
| `minutes.remove` | `{ minutes_id }` | `{}` | Change | Interactive | `remove` |
| `minutes.import` | `{ workspace_id }` | `MinutesJob` | Change | WaitsForPerson | the program's file dialog, then `import` (DSK-MIN-2) |
| `minutes.jobs` | `{ workspace_id }` | `Vec<MinutesJob>` (at most 50, newest first) | Read | Interactive | `jobs` |
| `minutes.cancel_job` | `{ job_id }` | `MinutesJob` | Change | Interactive | `cancel_job` |
| `minutes.retry` | `{ job_id }` | `MinutesJob` | Change | Interactive | `retry` |
| `minutes.give_model_consent` | `{}` | `{}` | Change | Interactive | `give_model_consent` |
| `minutes.remove_model` | `{}` | `{}` | Change | Interactive | `remove_model` |
| `minutes.accept_notice` | `{}` | `{}` | Change | Interactive | `accept_writer_notice` |
| `minutes.change_settings` | `MinutesSettingsChange { speech_model?, delete_recordings_after_save?, obs_port? }` (at least one) | `MinutesStatus` | Change | Interactive | `change_settings` |
| `minutes.record` | `{ workspace_id }` | `RecordingView` | Change | Interactive | `record` |
| `minutes.stop` | `{ recording_id }` | `MinutesJob` | Change | Interactive | `stop` |
| `minutes.open_obs` | `{}` | `{}` | Change | Interactive | `open_obs` |
| `minutes.save_obs_password` | `{ password (1–256 chars) }` | `{}` | Change | Interactive | Identity `save_obs_password` (DSK-MIN-3) |
| `minutes.delete_recording` | `{ minutes_id }` | `Minutes` | Change | Interactive | `delete_recording` |
| `minutes.export` | `{ minutes_id, format: "pdf" \| "docx" }` | `SavedCopy { saved_path }` | Change | WaitsForPerson | the program (DSK-MIN-4) |

  The catalogue then holds 471 rows; the `Interactive` count becomes 453. `minutes.save_edit` joins the commands whose body may reach 9 MiB (its body is at most 1 MiB of UTF-8). `minutes.save_obs_password` carries a secret: never logged, journalled or echoed, and its `Debug` form hides the password. No path crosses the boundary in any row.

- **SVC-MIN-2 Only the person (amended January 2026; user decision; Approved by the user January 2026).** `PERSON_ONLY_COMMANDS` gains `"minutes.record"`, `"minutes.give_model_consent"`, `"minutes.accept_notice"`, `"minutes.save_obs_password"` and `"minutes.remove"`, so no agent tool, workflow step or block action can start a recording, agree on the person's behalf, or remove minutes; the Architecture Test's existing assertion covers them.
- **SVC-MIN-3 The group behind its build switch (amended January 2026; user decision; Approved by the user January 2026).** *Command groups by build switch* gains the row `minutes` | `minutes` | 82 (with 80 and 81 through it). 49 names: 39 always answered and 10 behind a switch. A build without `minutes` refuses every `minutes.*` command with `SVC_NOT_IN_THIS_VERSION`, and its generated `schema.gen.json` lists no `minutes.*` row and none of the §23 types.
- **SVC-MIN-4 A page Ordin names (amended January 2026; user decision; Approved by the user January 2026).** `WebPage` gains `obs_download`; `app.open_web_page { page: obs_download }` opens `https://obsproject.com/download`.

### Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A field missing, out of range, a path or an unknown field | `Err(permanent)` | `SVC_INPUT_INVALID` | pointer under `/input`; never the password |
| A `minutes.*` command in a build without `minutes` | `Err(permanent)` | `SVC_NOT_IN_THIS_VERSION` | — |
| The person cancels the file or save dialog | `Err(permanent)` | `DSK_DIALOG_CANCELLED` | — |
| The owner's refusals | as owner | `MIN_*`, `TRX_*`, `OBS_*`, `CRED_*` | as MinutesService.md, TranscriptionService.md, ObsRecorderAdapter.md, IdentityAndCredentials.md ID-OBS-1 |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Meeting minutes commands (SVC-MIN-1 to SVC-MIN-4)

  Scenario: Contract — twenty local commands in a build with minutes
    Given the catalogue of a build with the minutes switch
    Then it holds 471 rows and every minutes.* row has scope Local
    And minutes.import and minutes.export wait for the person

  Scenario: Edge — a build without the switch
    Given a build without the minutes switch
    Then schema.gen.json lists no minutes.* command
    And minutes.status is refused with SVC_NOT_IN_THIS_VERSION

  Scenario: Error — the password is never echoed
    When minutes.save_obs_password is sent with password "" or an unknown field
    Then the reply is SVC_INPUT_INVALID and holds no password

  Scenario: Contract — only the person records
    Then minutes.record, minutes.give_model_consent, minutes.accept_notice, minutes.save_obs_password and minutes.remove are in PERSON_ONLY_COMMANDS
```

### Gaps

None.

## Amendment 2026-11-21 — Ordin's minutes tools and the person's own minutes actions (SVC-MIN-2 amended, SVC-MIN-5)

> **Status:** Approved by the user, 2026-11-21 ("approve as written"). Every section above stays in force. Source: the user's decisions of 2026-11-20 (Ordin's minutes and transcription tools; recording, the consents, the OBS password and deletion stay the person's).

- **SVC-MIN-2 amended — Only the person.** `PERSON_ONLY_COMMANDS` also gains `"minutes.delete_recording"` and `"minutes.remove_model"`, so the person's set is: `minutes.record`, `minutes.give_model_consent`, `minutes.accept_notice`, `minutes.save_obs_password`, `minutes.remove`, `minutes.delete_recording`, `minutes.remove_model`. Ordin reaches minutes only through its own tools (OrdinHost.md OH-MIN-4 to OH-MIN-9), which offer none of these; no agent tool, workflow step or block action reaches any `minutes.*` command. The Architecture Test's existing assertion covers the two added names.
- **SVC-MIN-5 Jobs for Ordin.** `minutes.jobs` returns jobs of both purposes; `MinutesJob` carries `purpose` (`minutes` | `transcript`) and `MinutesJobState` the new state `transcript_ready` (2C wire WIRE-MIN-7). The generated `minutes.gen.ts` and `minutes.schema.gen.json` change with them; the switch-off files do not. No row is added, so the catalogue keeps 471 rows.
