# SPEC: Service API

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-103) · amended January 2026 (ADR-104; promoted to Approved by the user January 2026) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-108; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (`knowledge.publish_to_confluence` takes the Knowledge Service's shape; renamed result types `DictationAvailability`, `ProjectCheckResult`, `ProjectFetchOutcome`, `SharePointItem`, `SavedAttachment`; the catalogue unit test counts 400) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
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
- `Timestamp` (RFC 3339, UTC, three fractional digits, `Z`), `DecimalString`, `Sha256Hex` (64 lowercase hex), `SetBy` (`person`, `company`), `ordin_types::paging::{Page<T>, PageRequest, Cursor}` (`{"items": [...], "next_cursor": "<opaque>" | null}`, page size 100), `CodedError`, `RepairAction` (`try_again`, `sign_in_again`, `send_to_it`), `EventKind`, `AgentRef`.
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
| `ordin.close_item` | `{ workspace_id, block_id }` | `{}` | Any | Change |  |
| `ordin.show_item` | `{ workspace_id, block_id }` | `{}` | Any | Change |  |
| `ordin.close_finished` | `{ workspace_id }` | `{ closed: u32 }` | Any | Change |  |
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
| `team_knowledge.turn_on` | `{ ws, choice: TeamFolderChoice }` | `TeamKnowledgeStatus` | Any | Change | **Share knowledge with a team…**; `NewFolder` adds the link in the same step |
| `team_knowledge.turn_off` | `{ ws }` | `{}` | Any | Change | **Stop sharing with the team**; local items and shared files stay |
| `team_knowledge.check_now` | `{ ws }` | `TeamKnowledgeStatus` (`busy: checking`) | Any | Read | answers at once; the check runs in the background and ends with `workspace_changed` (owner §H) |
| `team_knowledge.get` | `{ ws }` | `TeamKnowledgeStatus` (`busy: getting`) | Any | Change | **Get them**; answers at once; the outcome arrives in `last_get` or `last_error`, then `workspace_changed` (owner §H) |
| `team_knowledge.share` | `{ ws }` | `TeamKnowledgeStatus` (`busy: sharing`) | Any | Change | **Share what we've learned**; answers at once; gets first, then raises the card; the outcome arrives in `last_share` or `last_error`, then `workspace_changed` (owner §H) |

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
| `setup.help_preview` | `HelpRequestInput` | `HelpRequestPreview` | Any | Change |  |
| `setup.update_offer` | `{}` | `{ offer: UpdateOffer? }` | Local | Read | desktop |
| `setup.install_update` | `{}` | `{}` | Local | Change | desktop |
| `setup.help_send` | `{ preview_id: HelpPreviewId, fallback_address: String? }` | `HelpRequestOutcome` without `record` | Any | Change |  |

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

369 commands in 38 groups (*Amended January 2026 (ADR-109):* 14 added — ten `localisation.*` for languages the person edits, makes, exports and imports, three `ordin.*` for Ordin's drafting of a language, and the desktop's `files.save_export`, which writes an exported text file where the person chooses.) (*Amended January 2026 (ADR-108):* 1 added — `ordin.continue_conversation`, continuing an earlier conversation.) (*Amended January 2026 (ADR-106):* 2 added — `jira.connect_token` and `jira.find_issues` in a group of their own (receivers for a Jira place come from `handover.find_people`); `identity.sign_in` gains optional `site`.) (*Amended January 2026 (ADR-104):* 2 added — `knowledge.accept_suggestion` and `knowledge.dismiss_suggestion`; `knowledge.save_edit` gains optional `tags` and `aliases`.) (*Amended January 2026 (ADR-103):* 8 added — seven `team_knowledge.*` in a group of their own and `memory.keep_checked`.) (*Amended January 2026 (ADR-102).* 20 added: fourteen `slack.*`, two `workspaces.*` Slack grant commands, `handover.open_link`, `identity.complete_relayed_sign_in`, and the desktop's `app.open_slack_link` and `identity.relay_sign_in`.) Rows of the earlier draft of this catalogue that no owner defines and no consumer calls were removed, and rows that duplicated an owner's command under another name were replaced by the owner's command (SA-RECON-01): `home.overview` → `home.view`; `home.mark_opened` → `home.document_opened`; `home.run_again` → `home.run_again_inputs`; `workspaces.remove` → `workspaces.move_to_bin`; `workspaces.add_place` → `files.add_place` (a folder) or `workspaces.add_link` (a remote location); `workspaces.remove_place` → `workspaces.remove_link`; `files.list` → `files.list_folder`; `files.read_text` → `files.preview`; `files.save_text` → `files.save_edit`; `files.open_in_app` → `craft.open_with_system_default`; `push.pending` → `push.list_pending`; `confluence.publish` → `confluence.publish_checked`; `conversation.*` → `ordin.*` (X5: `send` → `send_message`; `answer`, `submit_form`, `answer_offer` → `answer_block` (`BlockAnswer::Choice`/`Other`, `Form`, `Offer`); `block`, `close_item`, `show_item`, `close_finished` → the same verbs under `ordin`; `history` → `read_conversation`; `earlier_list` → `list_conversations`; `clear` → `prepare_start_fresh` then `start_fresh`); `approval.pending` → `approval.list_pending`; `agents.library` → `catalogue.search`; `agents.repositories` → `catalogue.list_repositories`; `agents.repository_add` → `catalogue.probe_repository` then `catalogue.add_repository`; `agents.repository_remove` → `catalogue.remove_repository`; `authoring.drafts` → `authoring.list`; `authoring.draft_get` → `authoring.agent_draft`; `authoring.send_for_review` → `authoring.publish`; `handover.get` → `handover.open`; `handover.accept` → `handover.accept_and_start`; `handover.return` → `handover.return_to`; `mail.message_get` → `mail.open_message`; `mail.draft_get` → `mail.get_draft`; `mail.draft_save` → `mail.edit_draft`; `mail.ask_to_send` → `mail.send`; `signins.list` / `start` / `sign_out` → `identity.list` / `sign_in` / `sign_out`; `setup.retry` → `setup.check_now`; `help.send_to_it` → `setup.help_preview` then `setup.help_send`; `projects.change_set` → `projects.review`; `projects.commit_and_push` → `projects.commit` then `projects.push`; `projects.discard` → `projects.decide_entry`. Removed with no successor: `runs.start` (no owner; tasks start through `ordin.answer_block` with a form), `dashboards.get`, `telegram.draft_save`, `telegram.ask_to_send`, `push.send_back` (no consumer).

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
3. **`handle(...)`.** Calls `accept`; on `Refusal` builds `ServiceError { code, message: sentences.sentence(code), action: sentences.action(code), reference: None }`. Otherwise builds `CallContext { request_id, correlation_id: request_id.to_string(), origin, deadline, cancel }` and runs `dispatch` inside `tokio::time::timeout_at(deadline)` and `std::panic::AssertUnwindSafe(..).catch_unwind()`. The transport chooses `deadline`: `Interactive` 10 s, `Search` 10 s, `Transfer` 60 s after receipt *(assumed here: the p95 targets are 150 ms and 1 s; the deadline only stops a stuck module from holding a connection)*. Timeout → `SVC_DEADLINE` (retryable, action `try_again`); panic → `SVC_INTERNAL` (action `send_to_it`); `cancel` fired → no reply is sent and the transport drops the call. The reply's `http_status` follows the 2C table: 200 for `ok` and domain errors; 422 `SVC_INPUT_INVALID`, `SVC_BODY_TOO_LARGE` *(assumed here: a body refusal is an input refusal)*, `SVC_REQUEST_ID_MALFORMED`, `SVC_VERSION_MALFORMED`, `SRV_BAD_CURSOR`; 409 `SVC_VERSION_UNSUPPORTED`; 404 `SVC_COMMAND_UNKNOWN`, `SVC_COMMAND_LOCAL_ONLY`, `SVC_BACKEND_UNKNOWN`; 503 `SVC_DEADLINE`; 500 `SVC_INTERNAL`. The Remote API adds 401 and 429 itself.
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
| Implementation panicked, or returned an unregistered code | 500 / `Err(permanent)` | `SVC_INTERNAL` | as above, action `send_to_it` |
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

The words shown to the person for `SVC_` codes are Localisation's (07, 11 §6); this crate supplies none. Proposed message identifiers: `svc-input-invalid` "ordin couldn't read that request. Try again."; `svc-deadline` "That took too long. Try again."; `svc-internal` "Something went wrong in ordin. Send to IT."; `svc-version-unsupported` uses the two sentences of 11 §5.31.

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
    Then the reply has code SVC_INTERNAL and action send_to_it
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

`KnowledgeConnectionView` is `{ connection_id, name, url, added_by: SetBy, sources_record: { enabled: bool, retention_days: u16 } | null, last_answered_at: Timestamp | null }`; `sources_record` is `null` until the front door's `about` has been read once. `url` is an `https` address of at most 2,048 bytes (`SVC_INPUT_INVALID` at `/input/url` otherwise, before the owner is called). `knowledge_agents.add` and `knowledge_agents.details` call the front door over the network and may exceed the `Interactive` 150 ms p95 target; they stay inside the transport's 10 s `Interactive` deadline, with their own target in the owner's spec (p95 ≤ 2 s).

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
pub enum UpdateState { Downloading, Ready }
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
