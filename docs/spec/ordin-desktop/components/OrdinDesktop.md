# SPEC: ordin desktop

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-103) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · amended January 2026 (ADR-072) · amended 2026-01-10 (as-built readings) · amended January 2026 (FU-087, FU-126: write attribution at start, `SessionSetup.paused`; Approved by the user 2026-01-10) · amended January 2026 (Phase 11 reconciliation, SPEC-GAP-3, 5, 7–24; Approved by the user 2026-01-10, OQ-REC-1 to 5 open) · amended January 2026 (bundled agents and the inputs of a local session, closing OQ-REC-5 and OQ-REC-2; Approved by the user 2026-01-10, OQ-REC-6 and OQ-REC-8 open) · amended January 2026 (follow-ups of 2026-01-10: attachments for every run, closing OQ-REC-7; SPEC-GAP-25, 26, 27; OQ-REC-9 resolved; Approved by the user 2026-01-10) · amended January 2026 (Phase 11 follow-ups: SPEC-GAP-28 to 35, FU-138 program side, DSK-REC-33 to DSK-REC-41; Approved by the user 2026-01-10) · amended January 2026 (OQ-REC-10 and OQ-REC-11 resolved, DSK-REC-39 amended, the sources of `workspaces.tree`, DSK-REC-42 to DSK-REC-44; Approved by the user 2026-01-10, OQ-REC-12 and OQ-REC-13 open) · amended January 2026 (OQ-REC-12 resolved, the tree's running items, DSK-REC-45; Approved by the user 2026-01-10, "proceed with recommendations", OQ-REC-13 open) · amended 2026-01-28 (the memory models download on first use, DSK-MD-1 to DSK-MD-12; Approved by the user 2026-01-28) · amended 2026-01-29 (the Knowledge Service's background task at start, DSK-KS-1; Approved by the user 2026-01-29) · amended January 2026 (a team workspace's wiring and team knowledge on by default, DSK-TW-1 to DSK-TW-3, ADR-121; Approved by the user January 2026) · amended January 2026 (the Package Catalogue's notices loop at start, DSK-PKG-1; user decision) · amended January 2026 (the support file holds the last 3 days of logs, DSK-LOG-5; the bundled catalogue is re-read when its contents change, DSK-REC-39a; `app.open_web_page`, DSK-WEB-1; user decision) · amended January 2026 (user decision: Claude Code found by the Setup Assistant's rule, DSK-CC-1; the page `claude_code_install`, DSK-WEB-1; no sentence sends the person to IT, DSK-IT-1) · amended 2026-02-02 (ADR-123: the Copilot CLI is the person's own install; Approved by the user 2026-02-02) · amended 2026-02-11 (missing memory models offered at start, the document agent's memory, DSK-MD-15, DSK-MD-16; user decision) · amended 2026-02-11 (default memory rules, DSK-MEM-2, DSK-MEM-3; user decision)
> **Mode:** greenfield
> **derivedFromHld:** 0.69.0
> **Crate:** `apps/desktop` (Tauri; package `ordin-desktop`, binary `ordin`) · **Build phase:** 12 · **Programs:** D
> **Dependencies:** every module listed for D

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
- [Amendment January 2026 — Slack for working together (ADR-102)](#amendment-january-2026--slack-for-working-together-adr-102)
- [Amendment January 2026 — Team knowledge (ADR-103)](#amendment-january-2026--team-knowledge-adr-103)
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)
- [Amendment January 2026 — Ordin's language and language files (ADR-109)](#amendment-january-2026--ordins-language-and-language-files-adr-109)
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)
- [Amendment January 2026 — Releases and updates (ADR-114)](#amendment-january-2026--releases-and-updates-adr-114)
- [Amendment January 2026 — Write attribution at start and the paused run of a session (FU-087, FU-126)](#amendment-november-2026--write-attribution-at-start-and-the-paused-run-of-a-session-fu-087-fu-126)
- [Amendment January 2026 — Phase 11 reconciliation (SPEC-GAP-3, 5, 7–24)](#amendment-november-2026--phase-11-reconciliation-spec-gap-3-5-724)
- [Amendment January 2026 — Bundled agents and the inputs of a local session (OQ-REC-5, OQ-REC-2)](#amendment-november-2026--bundled-agents-and-the-inputs-of-a-local-session-oq-rec-5-oq-rec-2)

## Purpose

`apps/desktop` is the ordin desktop program: the Tauri 2 application the person opens on macOS or Windows. It is one of the two composition roots of the workspace (2B-1 rule 3; 09 ADR-100 item 2): it constructs every module the desktop runs in dependency order, registers each provider with its hub (`ToolProvider` with the MCP Front, `RuntimeAdapter` with the Session Supervisor, `PartSource` with the Resolver and Compiler), implements the `ServiceApi` trait over the constructed modules, hands it to the Command Bridge, loads the screens module into one WebView under fixed Tauri capabilities, raises the computer's notifications from Service API events (2B-2 D6), and runs the orderly shutdown on quit (07 — *Graceful shutdown*; SA-XC-13). It owns no table and makes no product decision: every decision stays in the module that owns it.

## Approach

One `main` builds a Tauri `Builder` with five official plugins and, inside its `setup` hook, an async `Core::start` that constructs modules in the fixed order of *Internal Logic* step 3, wiring each by passing `Arc` handles and trait objects; the main window is created only after the Command Bridge exists, so the WebView can never call a command before the core can answer it. Late-bound hub registrations (adapters, part source, tool providers) happen immediately after the provider is constructed and before any session starts. Error-code prefix: `DSK_` (*chosen here*; `DESK_` is taken by the Remote API's desktop pairing codes in 2C). Rejected: a dependency-injection container or service registry (hides the construction order that ADR-100 makes part of each program's specification, and hides dependencies from the reader). Rejected: building the core in a separate OS process behind the WebView (ADR-096 item 6 keeps the desktop's own core in the desktop process; a second process adds a local channel the design does not have).

### Reconciliation assumptions

- **SA-RECON-01** Module construction uses each owner's constructor and handle names as their specs now state them (ruling R6): the Journal is opened as `Datastore::open` → `MigrationRegistry::apply` → `Journal::start` and stopped with `Journal::shutdown` then `Datastore::close`; the MCP Front's registry freezes at `McpFront::start` (no `seal`); the Hook Relay binds at `start` and stops at `shutdown`; the Session Supervisor stops runs with `shutdown(ShutdownReason::Quit)` (reconciliation N5, N25, N64, N74, N90).
- **SA-RECON-02** Only the phase-1 table owners (Organisation Profile, Identity and Credentials) receive the SQLite pool from the program; the Telegram Adapter now depends on the Journal (2B-3, K7) and reaches its tables through it; every other table owner, Localisation, Brand and Setup Assistant included, receives the Journal's `Datastore` (D4, R20).
- **SA-RECON-03** The Catalogue Service is bound as the one `PartSource` of both the Resolver and Compiler and the Checker, right after it is built; both are built without a source (R18).
- **SA-RECON-04** Adapters that need a module built later are late-bound through a `OnceLock` filled right after that module's row; nothing calls them before step 4 (*Internal Logic* step 3).
- **SA-RECON-05** A command sent after quitting began is refused by the Command Bridge with `BRG_CLOSING`; this program raises no code of its own for it (reconciliation row 153).
- **SA-RECON-06** The Server Connector is started once with `start(sender, cancel)` and stopped by cancelling that token (rows 155, 167–169).
- **SA-RECON-07** An error's repair action is the module error's `CodedError::repair()`; `DesktopErrorSentences::action(code)` returns `None`, because no code-to-action map exists (Localisation, N17; ServiceApi step 4).
- **SA-RECON-08** Handover notices read `HomeSearch::needs_you`, whose rows carry `OpenTarget::Handover { workspace_id, handover_id }` (row 176).
- **SA-CLOSE-01** *(K4)* The MCP Front is built without a reference resolver; the program calls `mcp_front.bind_ref_resolver(<RefResolver>)` once, after the Journal, the File Service, the Knowledge Service, the Mail Service and the Telegram Service exist (after row 3.47, before `mcp_front.start`). A tool call that needs a reference before then cannot happen, because nothing runs before step 4.
- **SA-CLOSE-02** *(K5)* For `authoring.publish`, the program asks `AuthoringService::lifecycle_of(&draft)`; `Some(lifecycle_id)` → `PublishContext` from `HandoverService::following(lifecycle_id)`; `None` → no context.
- **SA-CLOSE-03** *(K5, chosen here)* Draft-review notices ("Changes asked for", "Published") come from no new event kind. The Notifier keeps, per backend, the last `ReviewDraftState` of each of the person's drafts, read with `authoring.list { agent_repository_id: None, open_only: false }`, and reads it again on every `needs_you_changed` of that backend (the Authoring Service emits one when a review asks for changes) and, for the local backend, on every `AuthoringNotice::Published` from `AuthoringService::subscribe()`. A draft whose state moved to `changes_asked_for` or to `published` raises its notice. For a server's draft, a publication is noticed at that server's next `needs_you_changed`.
- **SA-CLOSE-04** The Sandbox owns `SandboxGrants` and `PlaceFiles` (Sandbox.md); the program implements both, copied in *Boundary calls* below. `SandboxDeps` carries no secrets, artefact or event handle: the Sandbox takes Identity and the Journal directly.
- **SA-CLOSE-05** Home and Search, the Telegram Adapter, the Server Connector and every other table owner except the two phase-1 owners export `migrations() -> ModuleMigrations` (R3); the program registers each value.
- **SA-CLOSE-06** The program fills what the **Check it** commands do not carry: for `checker.check_installed`, `lock` = `resolver.open_bundle(installers.record(id).plan_hash).lock_file` and `scope` = `PackageCatalogue::source_scope(id)` (K13); for `checker.check_library_item`, `scope` = `SourceScope { home: the repository (or Bundled), profile: the profile repositories in profile order, include_bundled: true }` (Checker.md).
- **SA-CLOSE-07** `identity.import_client_certificate` is answered by this program: it opens the operating system's file picker twice (certificate, then key) and passes both files' bytes with the profile's `server.client_certificate_name` to `IdentityService::import_client_certificate`; no path reaches the screens.
- **SA-CLOSE-08** The desktop asks each hosted agent it reaches by role for the person's approver cards (`HostedAgentAdapter::approver_cards`) every 60 s and whenever *Needs you* is read, and sends the person's decision with `decide_approver_card`; a refusal `HAA_CARD_DECIDED` is shown as "This was already decided." and the card is dropped from the kept list.
- **SA-CLOSE-09** *(chosen here)* The kept cards reach *Needs you* through the program's `JournalReader::waiting_items`, each as `WaitingItem { workspace_id: None, kind: "hosted_approval", title: card.question, since: card.asked_at, opens: OpenTarget::HostedApproval { remote_agent_id: card.remote_agent_id, approval_id: card.approval_id } }` (HomeAndSearch.md SA-CLOSE-05, shown under "Hosted agents"). A press arrives as the Service API command `hosted.decide_card { server_or_agent, approval_id, decision, note }` (ServiceApi.md SA-CLOSE-05), which the program answers by calling `HostedAgentAdapter::decide_approver_card(server_or_agent, approval_id, decision, note)`; `Ok` → `{ state: "approved" }` for `Approve`, `{ state: "rejected" }` for `Reject`, and the card is dropped from the kept list.
- **SA-CLOSE-10** *(chosen here)* The program never sets a project's check commands: the organisation profile has no key for them, and `ProjectSettings` carries none. The engineer sets them on the project settings screen, and `projects.set_check_commands` reaches `ProjectService::set_check_commands` like every other catalogue command (step 5; ProjectService.md SA-CLOSE-03).
- **SA-RECON-09** The desktop builds its port-trait adapters (`SupervisorHost`, `SandboxGrants`, `WorkingCopyStore`, `RefResolver` and the others in *Program-side implementations and calls*) as thin delegations without decisions; the agent-profile-only methods of `SupervisorHost` answer `DSK_AGENT_PROFILE_ONLY`, an empty value or `false`, and are never reached on the desktop.

## Shared Context

### Inputs this program reads

- **No environment variable of its own** (2D *Desktop* item 6; 08:47 revision 0.48.0 "The desktop reads no environment variable of its own"). Proxy settings and trusted certificates come from the operating system (A-39).
- **Installer-carried settings** (2D-2b), read by the Organisation Profile and the Setup Assistant, not by this program: `profile_url`, `profile_signing_keys[]`, `update_feed_url`, and the Tauri updater public key fixed at build.
- **Fixed constants** used here (2D-4, 1A):

| Constant | Value | Source |
|---|---|---|
| Stop deadline per harness process on quit or sleep | 10 s, then killed and the run marked interrupted | SA-STOP-01 |
| Log retention | at most 14 files and 100 MB in all, a new file each UTC day and at 20 MB (DSK-LOG-1, DSK-LOG-2; amended January 2026 (user decision; Approved by the user January 2026)) | A-09, SA-OBS-01 |
| Concurrent local sessions | 8 | A-04 |
| Interface actions that wait for no model | ≤ 150 ms p95 | A-03, 07 NFR |
| Application start to interactive conversation | < 3 s, excluding sign-in | A-03, 07 NFR |
| Local progress event to screen | ≤ 1 s | 07 NFR |
| Server event to the desktop's screens | ≤ 500 ms | A-46, 07 NFR |

### Application data root (desktop)

`<data root>` is Tauri's `app_local_data_dir()` for the bundle identifier: macOS `~/Library/Application Support/<identifier>`, Windows `%LOCALAPPDATA%\<identifier>`, both inside the person's own account (09 ADR-072). The bundle identifier comes from the branded build (ADR-041 build layer); the default build uses `org.ordin.desktop` *(chosen here)*. Sub-paths fixed here: `<data root>/logs/` (log files), `<data root>/run/` (local channels, mode `0700` on macOS), `<data root>/preferences.json` (this program's file, *Data Model*). The hook channel is `<data root>/run/hook.sock` on macOS and `\\.\pipe\ordin-hook-<installation id>` on Windows (2D-1c `ORDIN_HOOK_ENDPOINT`), where the installation id is the local `BackendId` *(assumed here: the one identifier minted once per installation)*.

### Shared types (copied from Phase 2C, wire part §1; crate `ordin-service-api` unless marked)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServiceApiVersion { pub major: u16, pub minor: u16 }
impl ServiceApiVersion { pub const CURRENT: ServiceApiVersion = ServiceApiVersion { major: 1, minor: 0 }; }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BackendId(uuid::Uuid); // minted once at first start, stored by the Journal

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandName(String); // `<module>.<verb>`, `^[a-z][a-z0-9_]{0,30}\.[a-z][a-z0-9_]{0,30}$`

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServiceError {
    pub code: String,                 // never shown to the person
    pub message: String,              // one plain-language sentence, built by Localisation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<RepairAction>, // at most one repair action
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction { TryAgain, SignInAgain }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EventEnvelope {
    pub api_version: ServiceApiVersion,
    pub backend: BackendId,
    pub seq: u64,          // the backend's journal sequence number, strictly increasing
    pub at: Timestamp,     // ordin-types: RFC 3339, UTC, milliseconds, `Z`
    #[serde(flatten)]
    pub event: ServiceEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceEvent {
    RunStateChanged { run_id: RunId, state: RunState },
    RunProgress { run_id: RunId, progress: RunProgress },
    PlanChanged { run_id: RunId, plan_version: u32, steps: Vec<RunStep> },
    StepMarked { run_id: RunId, plan_version: u32, position: u8, mark: StepMark },
    StartUpReportReady { subject: StartUpSubject, items: Vec<StartUpReportItem> },
    ApprovalRequested { card: ApprovalCard },
    ApprovalDecided { approval_id: ApprovalId, state: ApprovalState, decided_on: String },
    CrewRoundChanged { crew_run_id: CrewRunId, round: u32, state: CrewRunState },
    BlockShown { workspace_id: WorkspaceId, block_id: BlockId, block: Block },
    Reset,
    // added in 1.0 by the Service API component spec:
    ServerStateChanged { server_connection_id: ServerConnectionId, state: ServerConnectionState, as_of: Option<Timestamp> },
    WorkspaceChanged { workspace_id: WorkspaceId },
    NeedsYouChanged { workspace_id: WorkspaceId },
    BlockAvailable { workspace_id: WorkspaceId, block_id: BlockId },
}

// The trait this program implements (Service API component spec): one method per catalogue
// command, `async fn <module>_<verb>(&self, ctx: &CallContext, input: I) -> Result<O, ServiceError>`.
#[async_trait::async_trait]
pub trait ServiceApi: Send + Sync + 'static { /* … one method per catalogue row … */ }
pub trait ErrorSentences: Send + Sync {
    fn sentence(&self, code: &str) -> String;
    fn action(&self, code: &str) -> Option<RepairAction>;
}
pub struct CallContext { pub request_id: CommandRequestId, pub correlation_id: String,
    pub origin: CallOrigin, pub deadline: std::time::Instant, pub cancel: tokio_util::sync::CancellationToken }
pub enum CallOrigin { LocalScreens, PairedDesktop { desktop_pairing_id: DesktopPairingId, computer_name: String } }

// home.needs_you result rows (HomeAndSearch.md), used for handover notices:
pub struct HomeRow { pub workspace_id: Option<WorkspaceId>, pub workspace_name: Option<String>, pub line: String,
                     pub at: Timestamp, pub is_new: bool, pub opens: OpenTarget, pub run_again: bool }
pub enum OpenTarget { /* … */ Handover { workspace_id: Option<WorkspaceId>, handover_id: HandoverId },
    HostedApproval { remote_agent_id: RemoteAgentId, approval_id: ApprovalId }, /* … */ }
// the JournalReader's waiting item (HomeAndSearch.md); workspace_id None for a hosted agent's approver card (SA-CLOSE-09)
pub struct WaitingItem { pub workspace_id: Option<WorkspaceId>, pub kind: String, pub title: String,
                         pub since: Timestamp, pub opens: OpenTarget }
// ordin-hosted-agent-adapter (HostedAgentAdapter.md)
pub enum ApproverDecision { Approve, Reject }
pub struct ApproverCard { pub remote_agent_id: RemoteAgentId, pub approval_id: ApprovalId, pub remote_run_id: String,
                          pub asked_by: String, pub question: String, pub scope: String, pub asked_at: Timestamp }
```

From the domain part of 2C (crate `ordin-types`): `RunState { Queued, Running, Paused, Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply }`; `ApprovalState { Pending, Approved, Rejected, Expired }`; `CrewRunState { Starting, Running, Paused, Finished, Escalated, StoppedAtLimit, Killed, Failed }`. From the wire part: `ApprovalCard` (fields used here: `approval_id: ApprovalId`, `backend: BackendId`, `workspace_id: WorkspaceId`, `question: String`, 1–120 characters ending in "?"); `Block::Question(QuestionBlock { question: String, choices: Vec<String>, allow_other: bool })`.

### Hub traits this program wires (owners per ADR-100)

| Trait | Owning crate | Implemented by (desktop) | Received by |
|---|---|---|---|
| `ToolProvider` | `ordin-mcp-front` | Sandbox, Memory Service, Craft Toolkit, Project Service, Knowledge Service, Mail Service, Ordin Host, Authoring Service, Telegram Service, Slack Service | MCP Front (registered by this program) |
| `RuntimeAdapter` | `ordin-runtime-adapter` (2C wire §2) | Claude Local Adapter, Copilot Local Adapter, Hosted Agent Adapter, Slack Thread Adapter | Session Supervisor (registered by this program) |
| `PartSource` | `ordin-agent-model` | Catalogue Service | Resolver and Compiler and Checker (bound by this program, R18) |
| `Coordinator` | `ordin-crew-orchestrator` | Ordin Host (`OrdinHost::coordinator()`) | Crew Orchestrator, passed by the Ordin Host in each `CrewStart` — this program does nothing for it beyond constructing both |
| `EventSink` | `ordin-journal` | Command Bridge, Approval Service, Session Supervisor, Hook Relay, Confluence Adapter, Push Service, Handover Service, Workspace Service, Mail Service, Telegram Service, Slack Service, Sandbox | Journal (`Journal::subscribe`: by this program for the `event_sink()` of the Approval Service, Session Supervisor, Hook Relay, Confluence Adapter, Push Service and Handover Service, and for the Workspace Service itself; the Command Bridge, Mail Service, Telegram Service and Sandbox subscribe themselves) |
| `PruneParticipant` | `ordin-journal` | Approval Service, Session Supervisor, File Service, Usage Planner, Crew Orchestrator, Mail Service, Ordin Host, Authoring Service | Journal (`register_prune_participant`, by this program) |
| `BlockCatalogueCheck` | `ordin-service-api` | `StandardBlockCheck` | MCP Front and Ordin Host (passed by this program) |
| `ServiceApi` | `ordin-service-api` | this program (`DesktopServiceApi`) | Command Bridge |
| `SlackReactor` | `ordin-slack-service` | this program (over `OrdinHost::react_to_slack`) | the Slack Service (`SlackServiceDeps.reactor`) |
| `SlackRulesSource` | `ordin-slack-service` | this program (from the Organisation Profile's `document.slack` (`tools_enabled`, `allowed_channels`, `rules_allowed`) and `document.server.external_actions_off` (always empty on the desktop), following `subscribe()`) | the Slack Service (`SlackServiceDeps.rules`) |
| `TeamMemory`, `TeamPages`, `TeamPush`, `TeamWorkspaces`, `TeamRepos` | `ordin-team-knowledge` | this program (delegations listed in the next table; amended January 2026, ADR-103) | the Team Knowledge Service (`TeamKnowledgeDeps`) |

### Program-side implementations and calls (*Interactions*, owner side)

Each item below is named by its owning spec (reconciliation §3, ruling R6: the owner's names win; the program conforms). Where a module's `…Deps` struct names a port trait that the module owns (`Identity`, `Journal`, `Approvals`, `KnowledgeFiles`, `HandoverGithub` and the like), the program implements it with a thin adapter that delegates to the module the trait's documentation names and adds no decision (RulesEngine SA-RECON-01 pattern). *Internal Logic* step numbers refer to this spec; row numbers to the table of step 3.

| Item (owning spec) | What this program does | When |
|---|---|---|
| `MigrationRegistry::register` (Journal and Artefact Store) | registers `ordin_journal::MIGRATIONS` first, then each table owner's `migrations()` (the Server Connector's included), the `MODULE_NAME` + `SQLITE_MIGRATIONS` pair of Organisation Profile and Identity and Credentials (wrapped in a `ModuleMigrations` with an empty `postgres`); `TelegramAdapter::migrations()` and `HomeSearch::migrations()` are among the `migrations()` values (SA-CLOSE-05) | row 3.1 |
| `Journal::recover_external_actions` (Journal and Artefact Store) | calls it once | step 4, before `session_supervisor.recover()` |
| `Journal::set_company_max_days` (Journal and Artefact Store) | passes the profile's `history.max_days` | step 4, and on each Organisation Profile change |
| `Journal::register_prune_participant` (Journal and Artefact Store) | registers the `prune_participant()` of the Approval Service, Usage Planner, Session Supervisor, File Service, Crew Orchestrator, Mail Service, Ordin Host and Authoring Service | right after rows 3.4, 3.12, 3.19, 3.22, 3.24, 3.39, 3.41, 3.44 |
| `Journal::subscribe` with an `EventSink` (Journal and Artefact Store) | subscribes `approval.event_sink()`, `session_supervisor.event_sink()`, `hook_relay.event_sink()`, `confluence.event_sink()`, `push.event_sink()` and `handover.event_sink()`, and the Workspace Service itself (it implements `EventSink`, name `workspaces`); the Command Bridge (in `start`), the Mail Service, the Telegram Service and the Sandbox subscribe themselves, so the program subscribes nothing for them | right after each module's row |
| `SupervisorHost` (Session Supervisor) | implements `startup_items` over the Rules Engine, the MCP Front's offered tools and the Installers' unavailable parts; `remote_route` over the Organisation Profile's remote agents; `run_resuming` and `run_ended` over the Rules Engine's `resume_run` and `end_run`; `run_abandoned(run_id, adapter)` calls the Slack Thread Adapter's `abandon(run_id)` when `adapter` is `SlackThread` and does nothing for any other kind; `expand_command`, `run_controls`, `reply_for` and `retry_after_block` are agent-profile only and return `HostError { code: "DSK_AGENT_PROFILE_ONLY" }` (*chosen here*), `RunControls::default()`, `None` and `false`; `check_content` (agent-profile only, *amended January 2026, ADR-107*) returns `ContentCheck::Passed`, since the desktop has no content checks | passed at row 3.19 |
| `RulesHost` (Rules Engine) | implements the path facts `path_exists`, `path_contains`, `read_text`, `append_line` and `remove` over the run's agent folder (Agent Folder Manager `get(agent_instance_id)`); `runs_in_progress`, `paused_run_with_command` and `run_ended` over the Session Supervisor's `runs_with_command`, `paused_with_command` and `has_ended`; `setting_value` and `setting_rules` answer no value (Settings Store is agent-profile only); `word` and `plain_line` over Localisation | passed at row 3.14 (late-bound handles, step 3 note) |
| `CrewHost` (Crew Orchestrator) | implements all seven methods: `agent_status` over the Package Catalogue; `saved_agent_versions` and `workflow_steps` over `OrdinHost::{saved_agent_versions, workflow_steps}`; `document_changed` over the File Service; `word_step_approval` over Localisation (the card wording the `RulesHost::word` adapter uses); `startup_items` over the Rules Engine's `startup_items(agent_instance_id)` and the MCP Front's offered tools, for every member of the charter; `remote_route` over the Organisation Profile's remote agents, as `SupervisorHost::remote_route` | passed at row 3.24 (late-bound handles) |
| `Coordinator` (Crew Orchestrator) | none: `OrdinHost::coordinator()` is passed in each `CrewStart` by the Ordin Host | — |
| `HarnessModels` (Runtime Adapter interface) | gives the Claude and Copilot adapters' `harness_models()` to the Usage Planner with `usage.set_harnesses(vec![claude.harness_models(), copilot.harness_models()])` (UsagePlanner late binding) | `UsagePlanner::new` at row 3.12 with an empty list; `set_harnesses` right after row 3.33 |
| `SessionSetupSource` (Claude Local Adapter, Copilot Local Adapter) | implements its one method `session_setup` over the Session Supervisor, the Installers' `record(id).launch`, the Usage Planner, `SetupAssistant::ready_harness`, the Crew Orchestrator's `get(crew_run_id)` (a crew member run's `crew_charter_id`, both adapters) and the Agent Folder Manager (`lock(Session)` held for the run's life, both adapters; `apply_grants` at session start and on resume and `session_access` when a resumed session's setup is rebuilt, Claude Local Adapter only — `CopilotSessionSetup` carries no granted folders, so a Copilot session gets none); `run_starter` is `None` on the desktop | passed at rows 3.32, 3.33 |
| `CopilotPort` (Copilot Local Adapter) | implements its one method `start_server` over the bundled Copilot CLI at `DesktopPaths::copilot_cli` *(superseded 2026-02-02 by DSK-CPX-4)* (`SdkCopilotPort`) | row 3.33 |
| `RemoteAgentDirectory`, `CompanyTokenSource` (Hosted Agent Adapter); `SlackAgentDirectory`, `SlackUserTokenSource` (Slack Thread Adapter) | implements the directories (`resolve_hosted`, `resolve_slack`) over the Organisation Profile's `remote_agents[]`, building each entry's `OidcSettings` from `remote_agent_identity(id)`; calls `mark_remote_agent_enabled(id)` on the adapter's first sign-in and `mark_remote_agent_verified(id)` on its first successful read for that agent's route (OrganisationProfile.md step 7); the token sources over Identity and Credentials: `company_access_token` and `invalidate` over `access_token_for_audience` and `invalidate_audience_token`, `slack_user_token` and `report_refused` over `access_token(CredentialKind::Slack, None)` and `report_refused` | rows 3.10, 3.11 |
| `DesktopKeyStore` (Identity and Credentials) | passes `IdentityService`, which implements it | row 3.13 |
| `BlockCatalogueCheck` (Service API) | builds one `StandardBlockCheck` and passes it to the MCP Front and the Ordin Host | rows 3.20, 3.41 |
| `RefResolver` (Service API), `McpFront::bind_ref_resolver` (MCP Front) | implements all six `RefResolver` methods (McpFront.md: the MCP Front does not implement it): `content` over the File Service; `email_message` over the Mail Service; `entity` over the Telegram Service's `resolve_list_block` and the Knowledge Service's `resolve_block`; `run_agent_name` over the Journal's run index; `plan_state` over the Journal's `current_plan`; `approval_verbs` from the Approval Service's approval verbs and binds it once with `bind_ref_resolver` (SA-CLOSE-01); an error (`MCP_REF_RESOLVER_ALREADY_BOUND`) is `StartError::ModuleFailed` | after row 3.47, before `mcp_front.start` |
| `ToolDispatch` (MCP Front) | passes the MCP Front, which implements it, to the Sandbox | row 3.27 |
| `SandboxGrants`, `PlaceFiles` (Sandbox) | implements `SandboxGrants::sandbox_capabilities` from the Package Inspector's declared connections, the organisation profile and Identity (RulesEngine SA-RECON-02); `PlaceFiles`: `read_for_agent` over the File Service's grant-checked `read_for_agent`, `write_agent_output` into the Agent Folder Manager's `get(agent_instance_id)` folder (SA-CLOSE-04) | row 3.27 |
| `WorkingCopyStore` (Push Service) | implements all five methods (ruling R12): `entry` over `WorkspaceService::working_copy_entry`; `set_state` over `WorkspaceService::set_working_copy_state`; `content` over the File Service's `read_file` (current bytes) or, with `local_version`, `versions` then `read_version` of that version; `restore_version` over the File Service's `versions` then `restore_version`; `local_changes` over `WorkspaceService::subscribe_working_copy_changes`; `PushDeps.github` is the Project Service itself (`GithubAccess`) | row 3.37 |
| `AuthoringHost` (Ordin Host, as `AuthoringService.md` requires it) | passes the Ordin Host, which implements it (`impl AuthoringHost for OrdinHost`) | row 3.44 |
| `HandoverHost` (Ordin Host, as `HandoverService.md` requires it) | passes the Ordin Host, which implements it | row 3.45 |
| `HandoverReach` (Authoring Service) | implements `reachable_places` over the Workspace Service's linked Git repositories and SharePoint folders (AuthoringService.md SA-CLOSE-05); passed as `AuthoringDeps.places` | row 3.44 |
| `PersonIdentity` (Handover Service) | implements `person` over `IdentityService::person()`; passed as `HandoverDeps.identity` | row 3.45 |
| `AgentNames` (Mail Service) | implements `display_name_of` over the Package Catalogue's `agents_in_workspace` (MailService.md); passed as `MailDeps.agent_names` (*late-bound*: the Package Catalogue is built at row 3.40) | row 3.39 |
| `PublishContext`, `AuthoringService::lifecycle_of` (Authoring Service) | for `authoring.publish`, reads `lifecycle_of(&draft)`; `Some(lifecycle_id)` → fills `PublishContext` from `HandoverService::following(lifecycle_id)`; `None` → no context (SA-CLOSE-02) | step 5 |
| `AuthoringService::subscribe`, `authoring.list` (Authoring Service) | the Notifier's draft-review notices (SA-CLOSE-03) | step 7 |
| `PackageInspector::reload_rule_sets` (Package Inspector) | calls it once; each returned failure is logged `rule_set_reload_failed` (warn) with `agent_instance_id` and `code`, and start continues | step 4, before `session_supervisor.recover()` |
| `PackageCatalogue::source_scope`, `ResolverAndCompiler::open_bundle`, `Installers::record` | fill the **Check it** commands' `lock` and `scope` (SA-CLOSE-06) | step 5 |
| `IdentityService::import_client_certificate`, `IdentityService::save_gitlab_token` | answers `identity.import_client_certificate` itself (SA-CLOSE-07); passes `identity.save_gitlab_token` through with the profile's `gitlab.host` (default `gitlab.com`) | step 5 |
| `ResolverAndCompiler::install_part_source`, `Checker::install_part_source` (ruling R18) | binds the Catalogue Service as the one `PartSource` of both, once, right after the Catalogue Service is built | row 3.36 |
| `CatalogueDeps` (Catalogue Service) | passes the Checker (`checker`), the Organisation Profile (`profile`) and the Project Service as `GithubAccess` (`github`) directly; no program adapter | row 3.36 |
| `HarnessSettings`, `InstanceActivity` (Package Catalogue) | implements `HarnessSettings::default_harness` over the Organisation Profile (`ProfileSnapshot::default_harness()`) and `InstanceActivity` over the Session Supervisor (`running`: the instance has a run in progress; `idle`: fired when an instance's last run ends); `ResolveContext.workspace_languages` from the Project Service | row 3.40 |
| `SearchSource` (Home and Search) | wraps the Package Catalogue and the Ordin Host as search sources and calls `HomeSearch::register_source` for each: agents (`group` = `GoToKind::Agent`; `find` over the Package Catalogue's `list`, by display name), workflows (`GoToKind::Workflow`; over the Ordin Host's `workflows.list`) and conversations (`GoToKind::Conversation`; over `ordin.list_conversations`); each `find` returns at most `limit` results | right after row 3.42 |
| `JournalReader` (Home and Search) | implements all four methods as HomeAndSearch.md documents them: `running` over the Journal's `running_runs`, `finished_since` over `runs_ended_since`, both titled from the Session Supervisor's and the Crew Orchestrator's `get`; `run_inputs` over `Run.inputs_used` (Session Supervisor `get`) or a crew run's inputs (Crew Orchestrator `get`); `waiting_items` maps each `OrdinHost::waiting_blocks()` entry to `WaitingItem { workspace_id: Some(workspace_id), kind, title, since, opens: OpenTarget::Block { workspace_id, block_id } }` beside the Approval Service, Journal, Push Service and Handover Service rows (HomeAndSearch SA-CLOSE-01), and each kept hosted approver card to a `WaitingItem` with `workspace_id: None` (SA-CLOSE-09) | | passed at row 3.42 (late-bound: the Push and Handover Services and the Ordin Host exist by then or at 3.45) |
| `HostedAgentAdapter::approver_cards`, `HostedAgentAdapter::decide_approver_card` (Hosted Agent Adapter) | for *Needs you*: every 60 s and on each `home.needs_you`, calls `approver_cards(agent)` for each remote agent of route `hosted_agent` whose organisation-profile entry the person can reach, and keeps the cards in memory; a press on such a card calls `decide_approver_card(agent, approval_id, decision, note)` (SA-CLOSE-08); the cards reach *Needs you* through `waiting_items` and a press arrives as `hosted.decide_card` (SA-CLOSE-09) | step 4 (timer) and step 5 |
| `AgentFolderManager::recover` (Agent Folder Manager), `CrewOrchestrator::recover` (Crew Orchestrator) | calls each once | step 4 |
| `Checker::reevaluate_trust` (Checker) | calls it on each Organisation Profile change | after row 3.36 |
| `ProjectService::workspace_opened`, `HomeSearch::workspace_opened` | calls both when the screens open a workspace (`workspaces.open`) | step 5 |
| `OrdinHost::workspace_removed` (Ordin Host) | calls it when the Workspace Service removes a workspace | step 5 |
| `StreamSink`, `ServerInstaller`, `watch::Receiver<ProjectSettings>` (Project Service) | `StreamSink::send` over the Command Bridge's `ScreenStreams` (`StreamKind::Terminal`, `StreamKind::Lsp`); `ServerInstaller::ensure` over `SetupAssistant::ensure_language_server`; the settings channel from the Organisation Profile and the person's settings (no check commands: they come only from `projects.set_check_commands`, SA-CLOSE-10) | row 3.31 (late-bound: the Setup Assistant is built at 3.43, the Bridge at 3.49) |
| `WordToMarkdown`, `watch::Receiver<ConfluenceSettings>` (Confluence Adapter) | `WordToMarkdown::convert` over the Craft Toolkit's `to_markdown`, passed only once C-13 confirms the conversion (else `None`); the settings channel from the Organisation Profile | row 3.30 |
| `HarnessProbe` (Setup Assistant) | implements the Copilot probe and calls `SetupAssistant::register_probe`: `harness` = the Copilot CLI's `HarnessKind`; `locate` = `Some(HarnessLocation { path: DesktopPaths::copilot_cli, version })`, the version being the first line of `<copilot_cli> --version`, or `None` when that file is absent; `sign_in_status` over `CopilotLocalAdapter::probe_sign_in` (`signed_in` true → `SignedIn { method }`, false → `SignedOut`); `start_sign_in` over `probe_start_sign_in`; `repair` over `probe_repair` (`Ok` → `Repaired`, `Err` → `Failed`); a `CopilotAdapterError::ProfileNotOffered` or `ServerSignInUnconfirmed` → `ProbeError::NotOffered`, any other → `ProbeError::Unreadable` (*chosen here*) | right after row 3.43 |
| `AppUpdater`, `UrlOpener` (Setup Assistant) | `TauriUpdater` implements `AppUpdater` (`check` and `install` over Tauri's updater; *amended January 2026 (ADR-114): see DSK-UP-11*) and is passed with `SetupAssistant::register_updater`; `UrlOpener::open` over `tauri-plugin-opener` (Phase 1A SA-RT-01; HLD 05 *Desktop shell plugins*, revision 0.49.1) | right after row 3.43 |
| `BrowserOpener` (Identity and Credentials) | implements `open` over `tauri-plugin-opener` | row 3.3 |
| `TelegramAppProbe` (Telegram Service) | implements `telegram_app_found` on the desktop by the platform check the trait's comment states | row 3.47 |
| `SlackReactor` (Slack Service) | `react(workspace_id, slack_link_id, channel_name, kept)` → `ordin_host.react_to_slack(workspace_id, slack_link_id, channel_name.to_owned(), kept)` (same order on both sides); an `OhError` is returned to the Slack Service as `SlackReactError { code: e.code().to_owned(), retryable: e.retryable() }` (*chosen here*) | row 3.39a; the Ordin Host is *late-bound* after row 3.41 |
| `SlackRulesSource` (Slack Service) | as the hub-trait row above | row 3.39a |
| `TeamMemory` (Team Knowledge Service) | `export_team`, `import_team`, `forget_team_fact`, `held_for_check` → the Memory Service's methods of the same names, one for one (MemoryService.md amendment January 2026 — Team knowledge) (amended January 2026, ADR-103) | row 3.41a |
| `TeamPages` (Team Knowledge Service) | `export_team_pages`, `import_team_page`, `remove_team_page` → the Knowledge Service's methods of the same names (KnowledgeService.md amendment January 2026 — Team knowledge) | row 3.41a |
| `TeamPush` (Team Knowledge Service) | `propose_team_share`, `resend_team_share`, `withdraw_team_share`, `subscribe` → the Push Service's methods of the same names | row 3.41a |
| `TeamWorkspaces` (Team Knowledge Service) | `get`, `source_links`, `add_link`, `opened`, `deleted`, `links_removed` → the Workspace Service's methods of the same names | row 3.41a |
| `TeamRepos` (Team Knowledge Service) | `git_credentials`, `repository_permission`, `branch_review_rules` → the Project Service as `GithubAccess`; `person` → `IdentityService::person()` | row 3.41a |
| `SlackUserTokenSource` (Slack Thread Adapter) and the Slack Service's token source | both are implemented over the same `IdentityService::access_token_for(CredentialKind::Slack, …)` and `report_refused`; the Slack Service's source asks for the scopes of the call it makes (SA-SLACK-01 or SA-SLACK-02) | rows 3.11, 3.39a |
| `RefResolver::entity` | gains `EntityRef::SlackLink` over the Slack Service's `resolve_list_block` | step 3 tail (SA-CLOSE-01) |
| Deep link (`tauri-plugin-deep-link`) | step 13 | `main`, before the core starts |
| Relayed sign-in for a paired server (decision 82) | step 14 | when a server's `identity.sign_in` answers with a relayed challenge |
| `Brand::coordinator_name` (Brand) | puts it in `OrdinHostPolicy.coordinator_name` at row 3.41 and calls `OrdinHost::set_policy` with a new policy on each `Brand::subscribe` change | row 3.41; each brand change |
| `MemoryService::source_changed` (Memory Service) | forwards the File Service's `subscribe_changes` events to it | after row 3.28 |
| `locale_changed`, `brand_changed` (Localisation, Brand) | appends each to the Journal with `append_event` so the screens receive the event | on each change |
| `Localisation::language_changes` (Localisation; amended January 2026, ADR-109) | appends each `LanguageChange` to the Journal as `language_changed` with `append_event(EventKind::LanguageChanged.as_str(), {locale, change})`, as `locale_changed`; a lagged receiver (`RecvError::Lagged`) is logged `warn` `language_changes_lagged` with the count and the task goes on | on each change, from after row 3.5 until quit |
| Help-request copies, setup reports, `profile_activated` (Journal `append_entry`) | journals each | when produced |
| `DictationService::cancel` (Dictation Service) | cancels the running dictation session | quit (step 10.3) and wake (step 11) |

Traits the personal-server program implements that this program does not wire, each because the module that takes it is not built on the desktop: `SessionRecordStore` (the Claude Local Adapter's `records` is `None`; it is required only in the agent profile); `TriggerDesk` (no Trigger Service, D5); `PersonalCore` and `HostedAgentCore` (no Channel Gateway, ADR-077: the Telegram Adapter is built with `gateway: None`); `LoaderStore`, `WorkFolderOpener` (no Agent Loader); `RunHost`, `ScopeSettings`, `ChannelDocuments`, `SecretSource` (no Connection Binder); `KnowledgeRunContext`, `KnowledgeScopeSettings`, `KnowledgeMetrics`, `ChatHistory` (the Knowledge Service is built with `ProgramKind::Desktop` and no `AgentKnowledgeDeps`); `SettingRuleCheck`, `SelectionLookup` (no Settings Store); `MetricsSink` (no Telemetry); `JobOutcomeNotifier` (no Job Runner); `JobDesk` (no Caller API); `PackageReader`, `HarnessLayoutWriter` (no Release Builder). All seven `CrewHost` methods are wired here and in the personal server.

Called by (this program's own public items):

| Item | Caller |
|---|---|
| `service.hello`, `settings.get`, `settings.change`, `files.save_copy`, `files.save_export` (amended January 2026, ADR-109), `app.quit`, `app.pick_folder` | screens module, through the Command Bridge (the Service API catalogue lists them, `app.*` with scope `Local`) |
| Desktop events `ordin://quit-requested`, `ordin://open-target`, `ordin://appearance` | screens module (listens) |

### Notification rules (11 §7, UX-A4; 2B-2 D6)

Notify outside the window only for: a pending approval; a finished task or crew; a question from Ordin that stops work until the person answers; the review of the person's own agent draft or part draft ("Changes asked for", "Published"); a handover handed to the person or returned. The same kinds come from a connected server's workspaces with "On [server]" in the notice; nothing is notified twice — an item decided in a chat before the desktop shows it raises no notice. A notification names the agent and the decision or result, and opens the view that resolves it. Preferences are per kind, on by default (2D-3).

## Public Interface

### Package and build

```toml
# apps/desktop/Cargo.toml
[package]
name = "ordin-desktop"
edition = "2021"
license = "Apache-2.0"
publish = false

[[bin]]
name = "ordin"
path = "src/main.rs"

[dependencies]
# every module crate of 2B-3 whose Programs column contains D, plus ordin-types (not ordin-hook, a
# program bundled as a binary):
# ordin-types, ordin-service-api, ordin-runtime-adapter, ordin-journal, ordin-organisation-profile,
# ordin-identity, ordin-agent-model, ordin-approval, ordin-localisation, ordin-agent-folders,
# ordin-format-plugins, ordin-resolver, ordin-sharepoint, ordin-hosted-agent-adapter,
# ordin-slack-thread-adapter, ordin-usage-planner, ordin-server-connector, ordin-rules, ordin-brand,
# ordin-dictation, ordin-checker, ordin-installers, ordin-session-supervisor, ordin-command-bridge,
# ordin-mcp-front, ordin-package-inspector, ordin-files, ordin-hook-relay, ordin-crew-orchestrator,
# ordin-sandbox, ordin-memory-service, ordin-craft-toolkit, ordin-confluence, ordin-projects,
# ordin-claude-adapter, ordin-copilot-adapter, ordin-workspaces, ordin-catalogue, ordin-push,
# ordin-knowledge, ordin-mail, ordin-package-catalogue, ordin-host, ordin-home-search, ordin-setup,
# ordin-authoring, ordin-handover, ordin-telegram-adapter, ordin-telegram-service, ordin-slack-service,
# ordin-team-knowledge (amended January 2026, ADR-103), ordin-jira (amended January 2026, ADR-106)
tokio, tracing, tracing-subscriber, tracing-appender, serde, serde_json, thiserror, uuid # versions per 1A

# The program ships for macOS and Windows only, so on any other target Tauri is not built and a
# Linux CI build needs no GTK.
[target.'cfg(any(target_os = "macos", target_os = "windows"))'.dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-single-instance = "2"   # 05: Desktop shell plugins
tauri-plugin-notification = "2"      # 05: Desktop shell plugins
tauri-plugin-dialog = "2"            # 05: Desktop shell plugins
tauri-plugin-updater = "2"           # 05: "updated through Tauri's updater"
tauri-plugin-opener = "2"            # 1A SA-RT-01; HLD 05 *Desktop shell plugins* (0.49.1): BrowserOpener, UrlOpener
tauri-plugin-deep-link = "2"         # HLD 05 *Desktop shell plugins* (0.50.0, ADR-102): the `ordin` link scheme, current user only
```

*(Amended 2026-02-02 by DSK-CPX-1: `binaries/ordin-hook` only.)* Bundled programs (Tauri `bundle.externalBin`): `binaries/ordin-hook` (built from `apps/hook` before the desktop bundle; `make` copies it to `apps/desktop/binaries/ordin-hook-<target triple>`) and `binaries/copilot` (the pinned Copilot CLI server, 05). At run time each is found beside the executable: `std::env::current_exe()?.parent()?.join("ordin-hook")` (`ordin-hook.exe` on Windows), and the same for `copilot`.

### `tauri.conf.json` (fields this spec fixes)

```json
{
  "productName": "ordin",
  "identifier": "org.ordin.desktop",
  "build": { "frontendDist": "ui/dist", "devUrl": "http://localhost:5173",
             "beforeBuildCommand": "npm --prefix ../ui run build" },
  "app": {
    "withGlobalTauri": false,
    "windows": [],
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src ipc: http://ipc.localhost; worker-src 'self' blob:; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'",
      "freezePrototype": true,
      "capabilities": ["main"]
    }
  },
  "bundle": { "externalBin": ["binaries/ordin-hook", "binaries/copilot"] },   // superseded 2026-02-02 by DSK-CPX-1: ["binaries/ordin-hook"]
  "plugins": { "updater": { "pubkey": "<the branded build's update public key>", "endpoints": [] } }
}
```

`productName` and `identifier` are replaced by the branded build (ADR-041). `windows` is empty: the main window is created in code after the core starts (*Internal Logic* step 6). `devUrl` is used only by `tauri dev`.

Under `plugins` the file also holds `"deep-link": { "desktop": { "schemes": ["ordin"] } }` (amended January 2026, ADR-102: the `ordin` link scheme, current user only).

### Tauri capability (`apps/desktop/capabilities/main.json`)

```json
{
  "identifier": "main",
  "description": "The screens module in the main window: Service API commands and events only.",
  "windows": ["main"],
  "permissions": [
    "core:event:allow-listen",
    "core:event:allow-unlisten",
    "core:app:allow-version",
    "ordin:default"
  ]
}
```

`ordin:default` is the permission set the Command Bridge's Tauri plugin `ordin` defines, allowing exactly its `<module>_<verb>` commands (SA-API-01). No `fs`, `shell`, `http`, `dialog`, `notification`, `updater`, `opener` or `path` permission is granted to the WebView: the WebView has no file-system, shell or network access of its own (01 trust boundary 1; 07 — Authorisation layer 1). The five plugins above are called from Rust only.

### Commands this program answers itself (part of `ServiceApi`)

The Service API component spec (`ServiceApi.md`, catalogue version 1.0) names the program as the implementer of six commands, two of them of scope `Local` that this spec introduced (*chosen here*; *Interactions*) and that are now in that catalogue (`ServiceApi.md`, *Command catalogue, version 1.0*). A `Local` command sent for a server's backend is refused by the Service API dispatcher with `SVC_COMMAND_LOCAL_ONLY`, so this program never sees it.

| Command | Input → Result | Scope | What this program does |
|---|---|---|---|
| `service.hello` | `Done` → `ServiceHello { api_version, backend, backend_name, profile: Desktop, last_seq }` | Any | `backend` = the Journal's `BackendId`; `backend_name` = catalogue message `desktop-backend-name` = "This computer"; `last_seq` = `Journal::last_seq()` |
| `settings.get` | `SettingsSectionRef` → `SettingsView` | Local | routes each section's keys to the module that owns them (2D-3); the sections `notifications` and `display` are answered from `<data root>/preferences.json` (*Data Model*) |
| `settings.change` | `ChangeSetting` → `SettingView` | Local | routes by key owner; the keys `notifications.approvals`, `notifications.finished_work`, `notifications.questions`, `notifications.draft_reviews`, `notifications.handovers` (booleans) and `display.appearance` (`system`, `light`, `dark`) (*chosen here*) are written to `preferences.json` |
| `files.save_copy` | `SaveCopy { backend, workspace_id, item }` → `SavedCopy { saved_path }` | Local | opens the operating system's save dialog (`tauri_plugin_dialog`) named after the item; calls `files.download` on `backend` (the local `ServiceApi`, or the Server Connector for a server); decodes the base64url bytes and writes them atomically to the chosen path; cancelled → `DSK_DIALOG_CANCELLED` |
| `files.save_export` | `{ file_name, text }` → `SavedCopy { saved_path }` | Local | *amended January 2026 (ADR-109)*: `file_name` 1–255 characters with no `/`, `\` or NUL, else `SVC_INPUT_INVALID`; opens the operating system's save dialog (`tauri_plugin_dialog`) with `file_name` proposed; writes `text` as UTF-8 atomically to the chosen path (temporary file in the same folder, then rename); cancelled → `DSK_DIALOG_CANCELLED`; a write failure → `DSK_SAVE_COPY_FAILED` (retryable). Used by the screens for `workflows.export` and `localisation.export_language`; the text is never logged |
| `app.quit` *(chosen here)* | `Done` → `AppQuitResult { stopping_sessions: u32 }` | Local | starts the orderly shutdown (*Internal Logic* step 10) |
| `app.pick_folder` *(chosen here)* | `Done` → `AppPickFolderResult { folder: Option<String> }` | Local | opens the operating system's folder picker; the result feeds `files.add_place` with `{ path: folder }` (FileService.md *Service API commands*); cancelled → `folder` absent |
| `app.open_slack_link` | `{ url: String }` → `{}` | Local | opens a Slack permalink the core produced in the person's browser with `UrlOpener` (`tauri-plugin-opener`); the URL must be at most 2,048 bytes and match `^https://[a-z0-9-]{1,80}\.slack\.com/archives/[CG][A-Z0-9]{8,20}/p[0-9]{16}(\?.*)?$` → else `DSK_LINK_REFUSED` (permanent), nothing opened |
| `app.open_web_page` | `OpenWebPage { page: WebPage }` → `{}` | Local | person-only; opens one page ordin names in the person's browser with `UrlOpener`; never a free address (DSK-WEB-1; amended January 2026 (user decision; Approved by the user January 2026); `claude_code_install` amended January 2026 (user decision; Approved by the user January 2026)) |
| `identity.relay_sign_in` | `{ server: ServerRef, reason: Option<String> }` → `CredentialView` | Local | step 14: the whole desktop half of a server's relayed Slack sign-in |

```rust
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AppQuitResult { pub stopping_sessions: u32 }

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AppPickFolderResult {
    /// Absolute path the person chose; absent when the picker was cancelled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// The program's own settings, kept in preferences.json (2D-3, owner App Shell; stored by the
/// program because the core raises the notifications).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DesktopPreferences {
    pub format: u8,                          // 1
    pub notifications: NotificationPreferences,
    pub appearance: Appearance,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NotificationPreferences {
    pub approvals: bool,       // default true
    pub finished_work: bool,   // default true
    pub questions: bool,       // default true
    pub draft_reviews: bool,   // default true
    pub handovers: bool,       // default true
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance { System, Light, Dark } // default System (11 §4 Settings › Display)
```

The program also implements the Service API's `ErrorSentences` trait over Localisation (`DesktopErrorSentences`): `sentence(code)` is `Localisation::error_sentence(code, &MessageArgs::new())`, in the person's locale; `action(code)` returns `None`, because the action of a module error is its `CodedError::repair()` (Localisation *Error sentences and repair actions*; SA-RECON-07).

### Desktop events (core → main window; Tauri events, not Service API events)

Emitted with `AppHandle::emit_to("main", name, payload)`; names *chosen here*. They carry no backend data and do not travel to servers.

| Event name | Payload | When |
|---|---|---|
| `ordin://quit-requested` | `{"running_sessions": <u32>}` | The person closed the window or chose Quit while at least one local session runs; the screens show the quit confirmation and answer with `app.quit`. |
| `ordin://open-target` | `NotificationTarget` (below) | The main window gains focus within 5 minutes after a notification was shown and not yet opened. Also when a handover link was opened (step 13) and `handover.open_link` returned `LinkOpened::Opened` (amended January 2026, ADR-102). |
| `ordin://appearance` | `{"appearance": "system" \| "light" \| "dark"}` | After `settings.change` changed `display.appearance`. |

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotificationTarget {
    Approval { backend: BackendId, workspace_id: WorkspaceId, approval_id: ApprovalId },
    Run { backend: BackendId, run_id: RunId },
    CrewRun { backend: BackendId, crew_run_id: CrewRunId },
    Question { backend: BackendId, workspace_id: WorkspaceId, block_id: BlockId },
    /// `workspace_id` is `None` for a handover opened from a link before it was accepted into a workspace (amended January 2026, ADR-102).
    Handover { backend: BackendId, workspace_id: Option<WorkspaceId>, handover_id: HandoverId },
    /// The person's own agent or part draft whose review changed (SA-CLOSE-03).
    Draft { backend: BackendId, draft: DraftRef },
}
```

### Rust items of the program

```rust
// apps/desktop/src/main.rs
fn main();

// apps/desktop/src/core.rs
pub struct Core { /* one Arc per constructed module, in construction order; private */ }
impl Core {
    pub async fn start(app: &tauri::AppHandle, paths: &DesktopPaths) -> Result<Arc<Core>, StartError>;
    pub async fn shutdown(&self, reason: QuitReason) -> ShutdownSummary;
}

pub struct DesktopPaths {
    pub data_root: std::path::PathBuf,
    pub logs: std::path::PathBuf,
    pub run: std::path::PathBuf,
    pub hook_endpoint: String,          // socket path or pipe name
    pub hook_program: std::path::PathBuf,
    pub copilot_cli: std::path::PathBuf,
    pub preferences: std::path::PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitReason { PersonQuit, OperatingSystemEnding }   // distinct from the Session Supervisor's ShutdownReason

#[derive(Debug, Clone, Copy)]
pub struct ShutdownSummary { pub sessions_stopped: u32, pub sessions_interrupted: u32, pub journal_shut_down: bool }

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("DSK_DATA_ROOT_UNWRITABLE")] DataRootUnwritable { path: std::path::PathBuf, source: std::io::Error },
    #[error("DSK_START_FAILED")] ModuleFailed { module: &'static str, code: String },
}

// apps/desktop/src/service_api.rs
pub struct DesktopServiceApi { /* Arc of every module that answers commands; private */ }
#[async_trait::async_trait]
impl ordin_service_api::ServiceApi for DesktopServiceApi { /* one method per catalogue command */ }

// apps/desktop/src/notifier.rs
pub struct Notifier;
impl Notifier {
    pub fn spawn(app: tauri::AppHandle, events: tokio::sync::broadcast::Receiver<Arc<EventEnvelope>>,
                 deps: NotifierDeps) -> tokio::task::JoinHandle<()>;
}

// apps/desktop/src/updater.rs — implements the Setup Assistant's update trait (boundary below)
pub struct TauriUpdater { app: tauri::AppHandle }

// apps/desktop/src/wake.rs
pub struct WakeWatcher;
impl WakeWatcher { pub fn spawn(on_wake: impl Fn(std::time::Duration) + Send + 'static) -> tokio::task::JoinHandle<()>; }
```

### Boundary calls this program makes into other crates

Copied from each owner's current Public Interface (ruling R6); the owner's spec is authoritative where this copy and it differ.

```rust
// ordin-journal (JournalAndArtefactStore.md)
pub async fn Datastore::open(config: DatastoreConfig) -> Result<Datastore, JournalError>;
//   desktop: DatastoreConfig::Sqlite { path: <data root>/journal.db, backup_dir: <data root>/backups }
pub fn Datastore::sqlite(&self) -> Option<&sqlx::SqlitePool>;   // `.cloned()` handed to Organisation Profile, Identity
pub async fn Datastore::close(&self);
pub fn MigrationRegistry::new() -> MigrationRegistry;
pub fn MigrationRegistry::register(&mut self, migrations: ModuleMigrations) -> Result<(), JournalError>;
pub async fn MigrationRegistry::apply(&self, store: &Datastore, mode: ApplyMode) -> Result<MigrationReport, JournalError>; // ApplyPending
pub const ordin_journal::MIGRATIONS: ModuleMigrations;
pub async fn Journal::start(store: Datastore, options: JournalOptions) -> Result<Journal, JournalError>;
//   desktop: JournalOptions { automatic_pruning: true, prune_interval: 24 h, clock: Arc::new(SystemClock) }
pub fn Journal::backend_uuid(&self) -> uuid::Uuid;        // BackendId(journal.backend_uuid())
pub fn Journal::last_seq(&self) -> u64;                    // service.hello last_seq
pub fn Journal::subscribe(&self, sink: Arc<dyn EventSink>) -> SubscriptionId;
pub fn Journal::register_prune_participant(&self, participant: Arc<dyn PruneParticipant>);
pub async fn Journal::recover_external_actions(&self) -> Result<u32, JournalError>;
pub async fn Journal::set_company_max_days(&self, max_days: Option<u32>) -> Result<RetentionSetting, JournalError>;
pub async fn Journal::append_event(&self, kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>;
pub async fn Journal::append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
pub async fn Journal::shutdown(&self);                     // no flush or close on the Journal
// each table owner: `migrations() -> ModuleMigrations` (Home and Search and the Telegram Adapter
// and the Server Connector included, SA-CLOSE-05), or `MODULE_NAME` + `SQLITE_MIGRATIONS`
// (Organisation Profile, Identity)

// ordin-mcp-front (McpFront.md)
pub fn McpFront::new(config: McpFrontConfig, rules: Arc<RulesEngine>, journal: Journal,
    blocks: Arc<dyn BlockCatalogueCheck>, metrics: Option<Arc<dyn McpMetrics>>) -> McpFront;
/// Once; a second call → MCP_REF_RESOLVER_ALREADY_BOUND (permanent). A tool call needing it
/// before binding → MCP_REF_RESOLVER_NOT_BOUND (permanent) (K4).
pub fn McpFront::bind_ref_resolver(&self, resolver: Arc<dyn RefResolver>) -> Result<(), McpError>;
pub fn McpFront::register(&self, provider: Arc<dyn ToolProvider>) -> Result<(), McpError>;   // only before start
pub async fn McpFront::start(&self, shutdown: CancellationToken) -> Result<(), McpError>;    // freezes the registry

// ordin-session-supervisor (SessionSupervisor.md)
pub fn SessionSupervisor::new(datastore: Datastore, journal: Arc<Journal>, usage: Arc<UsagePlanner>,
    run_queue: Option<Arc<dyn RunQueue>>, host: Arc<dyn SupervisorHost>, config: SupervisorConfig) -> Arc<SessionSupervisor>;
pub fn SessionSupervisor::register_adapter(&self, adapter: Arc<dyn RuntimeAdapter>) -> Result<(), SupervisorError>;
pub fn SessionSupervisor::event_sink(self: &Arc<Self>) -> Arc<dyn EventSink>;
pub fn SessionSupervisor::prune_participant(self: &Arc<Self>) -> Arc<dyn PruneParticipant>;
pub async fn SessionSupervisor::recover(&self) -> Result<RecoveryReport, SupervisorError>; // { resumed, interrupted, ended_remotely, awaiting_decision }
pub fn SessionSupervisor::running_local_sessions(&self) -> u32;
pub async fn SessionSupervisor::shutdown(&self, reason: ShutdownReason) -> ShutdownReport; // Quit | Signal | Sleep → { stopped, interrupted }
pub async fn SessionSupervisor::on_wake(&self, slept_for: Duration);
pub async fn SessionSupervisor::get(&self, run_id: RunId) -> Result<RunView, SupervisorError>; // Run.agent, Run.origin

// ordin-resolver (ResolverAndCompiler.md), ordin-checker (Checker.md) — R18 late binding
pub fn ResolverAndCompiler::new(config: ResolverConfig) -> Result<ResolverAndCompiler, ResolveError>;
pub fn ResolverAndCompiler::install_part_source(&self, source: Arc<dyn PartSource>) -> Result<(), ResolveError>;
pub fn Checker::new(resolver: Arc<ResolverAndCompiler>, profile: Arc<OrganisationProfileService>, journal: Arc<Journal>) -> Checker;
pub fn Checker::install_part_source(&self, source: Arc<dyn PartSource>) -> Result<(), CheckError>;

// ordin-command-bridge (CommandBridge.md)
pub fn CommandBridge::new(cfg: BridgeConfig, api: Arc<dyn ServiceApi>, sentences: Arc<dyn ErrorSentences>,
    connector: Arc<ServerConnector>, journal: ordin_journal::Journal) -> (CommandBridge, ServerEventSender);
pub fn CommandBridge::start(&self, cancel: CancellationToken);           // subscribes its own EventSink
pub fn CommandBridge::subscribe_all(&self) -> tokio::sync::broadcast::Receiver<Arc<EventEnvelope>>;
pub fn CommandBridge::streams(&self) -> ScreenStreams;
pub async fn CommandBridge::close(&self);                                // later commands get BRG_CLOSING
pub fn CommandBridge::plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R>;
pub const IPC_COMMANDS: &[&str];

// ordin-server-connector (ServerConnector.md)
pub fn ServerConnector::new(cfg: ConnectorConfig, journal: ordin_journal::Journal, keys: Arc<dyn DesktopKeyStore>)
    -> Result<ServerConnector, ConnectorError>;
pub async fn ServerConnector::start(&self, events: ServerEventSender, cancel: CancellationToken) -> Result<(), ConnectorError>;
pub fn ServerConnector::on_wake(&self);
pub fn ServerConnector::display_name(&self, backend: BackendId) -> Option<String>;

// ordin-hook-relay (HookRelay.md)
pub async fn HookRelay::start(self: &Arc<Self>) -> Result<(), HookRelayError>;   // binds the hook channel
pub fn HookRelay::event_sink(self: &Arc<Self>) -> Arc<dyn EventSink>;
pub async fn HookRelay::shutdown(&self);

// late binding and host calls (UsagePlanner.md, SlackThreadAdapter.md, CrewOrchestrator.md, SessionSupervisor.md)
pub fn UsagePlanner::set_harnesses(&self, harnesses: Vec<Arc<dyn HarnessModels>>);
pub fn SlackThreadAdapter::abandon(&self, run: RunId);                       // from SupervisorHost::run_abandoned
pub async fn CrewOrchestrator::get(&self, crew_run_id: CrewRunId) -> Result<(CrewRun, Vec<Round>), CrewError>; // SessionSetupSource

// ordin-localisation (Localisation.md)
pub fn Localisation::message(&self, id: MessageId, args: &MessageArgs) -> String;
pub fn Localisation::error_sentence(&self, code: &str, args: &MessageArgs) -> String;

// ordin-setup (SetupAssistant.md)
pub trait AppUpdater: Send + Sync {
    async fn check(&self, feed_url: &url::Url) -> Result<Option<UpdateOffer>, UpdaterError>;
    async fn install(&self, offer: &UpdateOffer) -> Result<(), UpdaterError>;
}
pub fn SetupAssistant::register_updater(&self, updater: Arc<dyn AppUpdater>);
pub fn SetupAssistant::register_probe(&self, probe: Arc<dyn HarnessProbe>);
pub async fn SetupAssistant::start_background_checks(&self);
pub async fn SetupAssistant::close(&self);

// ordin-home-search (HomeAndSearch.md) — handover notices
pub async fn HomeSearch::needs_you(&self, filter: Option<WorkspaceId>) -> Result<Vec<HomeRow>, HomeError>; // rows with OpenTarget::Handover
pub fn HomeSearch::register_source(&self, source: Arc<dyn SearchSource>);

// ordin-dictation (DictationService.md)
pub async fn DictationService::start(&self, field: FieldRef) -> Result<DictationSession, DictationError>;
pub async fn DictationService::cancel(&self, session_id: DictationSessionId) -> Result<(), DictationError>;

// ordin-handover (HandoverService.md), ordin-authoring (AuthoringService.md)
pub async fn HandoverService::following(&self, lifecycle_id: LifecycleId) -> Result<LifecycleFollowing, HandoverError>;
pub async fn AuthoringService::publish(&self, draft: DraftRef, context: Option<PublishContext>) -> Result<(), AuthoringError>;
pub fn AuthoringService::lifecycle_of(&self, draft: &DraftRef) -> Result<Option<LifecycleId>, AuthoringError>;   // K5
pub async fn AuthoringService::list(&self, filter: DraftFilter) -> Result<Vec<DraftSummary>, AuthoringError>;
pub fn AuthoringService::subscribe(&self) -> tokio::sync::broadcast::Receiver<AuthoringNotice>;

// ordin-sandbox (Sandbox.md) — traits the program implements (SA-CLOSE-04)
#[async_trait] pub trait SandboxGrants: Send + Sync {
    async fn sandbox_capabilities(&self, agent_instance_id: AgentInstanceId, workspace_id: WorkspaceId)
        -> Result<Vec<SandboxCapability>, GrantsUnavailable>;
}
#[async_trait] pub trait PlaceFiles: Send + Sync {
    async fn read_for_agent(&self, agent_instance_id: AgentInstanceId, workspace_id: WorkspaceId,
                            place_id: PlaceId, relative_path: &str, max_bytes: u64) -> Result<Vec<u8>, PlaceFileError>;
    async fn write_agent_output(&self, agent_instance_id: AgentInstanceId, relative_path: &str, bytes: &[u8])
        -> Result<Sha256Hex, PlaceFileError>;
}
pub async fn Sandbox::start(deps: SandboxDeps) -> Arc<Sandbox>;   // SandboxDeps { dispatch, grants, identity, journal, place_files, session_idle, data_root, log_level }

// ordin-package-inspector (PackageInspector.md), ordin-package-catalogue (PackageCatalogue.md)
pub async fn PackageInspector::reload_rule_sets(&self) -> Result<Vec<RuleSetReloadFailure>, InspectError>;
pub async fn PackageCatalogue::source_scope(&self, instance: AgentInstanceId) -> Result<SourceScope, PackageError>;   // K13

// ordin-identity (IdentityAndCredentials.md) — commands the program completes
pub async fn IdentityService::import_client_certificate(&self, name: &str, certificate_pem: &[u8],
    key_pem: zeroize::Zeroizing<Vec<u8>>) -> Result<ClientCertificateView, CredError>;
pub async fn IdentityService::save_gitlab_token(&self, host: &str, token: SecretString) -> Result<Credential, CredError>;
pub fn AuthoringService::prune_participant(&self) -> Arc<dyn PruneParticipant>;

// modules that stop on quit: `shutdown()` on the Workspace Service, Craft Toolkit, Project Service,
// Sandbox, File Service, Telegram Service and Organisation Profile; the Mail Service, Catalogue
// Service, Memory Service, Telegram Adapter, MCP Front and Ordin Host stop when the core's
// shutdown token passed to their `start` is cancelled.
```

### Example

Start of a desktop with one paired server, as logged (JSON lines, abbreviated):

```json
{"timestamp":"2026-01-10T08:01:02.114Z","level":"INFO","component":"desktop","message":"start","version":"1.0.0","data_root_ok":true}
{"timestamp":"2026-01-10T08:01:02.391Z","level":"INFO","component":"desktop","message":"journal_open","backend":"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b","migrated":2}
{"timestamp":"2026-01-10T08:01:03.020Z","level":"INFO","component":"desktop","message":"wired","modules":45,"tool_providers":9,"adapters":4}
{"timestamp":"2026-01-10T08:01:03.188Z","level":"INFO","component":"desktop","message":"window_shown","start_ms":1074}
{"timestamp":"2026-01-10T08:14:40.002Z","level":"INFO","component":"desktop","message":"notification","kind":"approval","backend":"0192a1d0-1111-7222-8333-444455556666","shown":true}
```

The approval notification for a server's card reads, in en-GB: title "Northwind server: Publish the revised account brief?", body "Waiting for you · On Northwind server". Pressing it brings the window forward, and the screens open that card's screen.

## Internal Logic

1. **Single instance.** `main` installs `tauri-plugin-single-instance` first. A second launch passes its command-line arguments to the first instance and exits; when an argument is an `ordin:` link, the first instance runs step 13 with it; otherwise it shows, un-minimises and focuses its main window (amended January 2026, ADR-102). Logged: `single_instance_focus`.
2. **Paths and logging.** In `setup`, resolve `<data root>` with `app.path().app_local_data_dir()`; create it and `logs/`, `run/` (mode `0700` on macOS). Write and delete a probe file `<data root>/.write-probe`; any failure → `StartError::DataRootUnwritable` (step 9). Install the `tracing` subscriber: Logging.md's `ordin_types::log::subscriber` (its `JsonLineFormat` and rules R-1 to R-7), level `info` for ordin's crates and `warn` for every other crate (Logging.md V-1; amended January 2026 (user decision; Approved by the user January 2026)), writing through `tracing_appender::non_blocking` over the program's own log files of DSK-LOG-1 and DSK-LOG-2 (A-09; amended January 2026 (user decision; Approved by the user January 2026): `tracing_appender::rolling` starts a new file by time only, so it is no longer the file writer). Install a panic hook that logs `panic` with the location only, never the message, which can quote the text being handled (amended January 2026 (user decision; Approved by the user January 2026)), then calls the default hook.
3. **Construct modules.** `Core::start` constructs, in this order, each module with the owner's constructor; every constructor error stops start with `StartError::ModuleFailed { module, code }` and nothing later is constructed. Phase numbers are 2B-3's. The program constructs a crate only where it exposes a handle; `ordin-types`, `ordin-service-api`, `ordin-runtime-adapter` and `ordin-agent-model` are used as libraries. "Journal" below is the started `Journal` (`Arc` where the owner takes one); "Datastore" is `journal.datastore().clone()`. Port traits a module owns are implemented by program adapters (*Program-side implementations and calls*). An adapter that needs a module built later (marked *late-bound*) holds a `std::sync::OnceLock` the program fills right after that module's row; a call before then returns the adapter's permanent error, and no call can arrive before step 4 because nothing runs until then.

    | # | Crate (phase) | Constructed with | Wiring done right after |
    |---|---|---|---|
    | 3.1 | `ordin-journal` (1) | `Datastore::open(DatastoreConfig::Sqlite { path: <data root>/journal.db, backup_dir: <data root>/backups })`; `MigrationRegistry::new()`, every registration of *Program-side implementations*, `apply(&datastore, ApplyMode::ApplyPending)`; `Journal::start(datastore, JournalOptions { automatic_pruning: true, prune_interval: 24 h, clock: SystemClock })` | read `BackendId(journal.backend_uuid())`; log `journal_open` with `migrated` = `MigrationReport.applied.len()` |
    | 3.2 | `ordin-organisation-profile` (1) | `OrganisationProfileService::start(ProfileMode::Read { pool: <datastore.sqlite().cloned(); the desktop's store is always SQLite, so `None` is `StartError::ModuleFailed`>, data_root, http, build_defaults }, ProfileOptions { refresh_interval: 60 min, clock })` (D4: phase-1 owner, pool from the program) | its refresh loop runs (2D-4) |
    | 3.3 | `ordin-identity` (1) | `IdentityService::new(IdentityConfig { program: Desktop, data_root, keychain_service: <bundle identifier>, secret_backend: OsKeychain, github_app_client_id, http, browser: Some(<BrowserOpener>) }, datastore.sqlite().cloned())` | — |
    | 3.4 | `ordin-approval` (2) | `ApprovalService::new(Datastore, Journal, ApprovalConfig { backend, local_computer_name, approvers: None })` | `journal.subscribe(approval.event_sink())`; `journal.register_prune_participant(approval.prune_participant())` |
    | 3.5 | `ordin-localisation` (2) | `Localisation::start(LocalisationConfig { store: PreferenceStore::Datastore(journal.datastore().clone()), os_locales: sys_locale::get_locales(), app_version: env!("CARGO_PKG_VERSION").to_string() }, Organisation Profile)` (D4: through the Journal's `Datastore`; `app_version` amended January 2026, ADR-109) | the `language_changes()` forwarding task (ADR-109) |
    | 3.6 | `ordin-agent-folders` (2) | `AgentFolderManager::new(Journal, <data root>)` | — |
    | 3.7 | `ordin-format-plugins` (2) | — (library) | — |
    | 3.8 | `ordin-resolver` (2) | `ResolverAndCompiler::new(ResolverConfig { bundle_root: <data root>/bundles, compiler_version })`, without a part source (R18) | source bound at 3.36 |
    | 3.9 | `ordin-sharepoint` (2) | `SharePointAdapter::new(SharePointDeps { identity, journal, http })?` — it returns `Result`; an `Err(e)` is `StartError::ModuleFailed { module: "ordin-sharepoint", code: e.code() }` | — |
    | 3.10 | `ordin-hosted-agent-adapter` (2) | `HostedAgentAdapter::new(HostedAgentConfig::default(), <RemoteAgentDirectory>, <CompanyTokenSource>, http)` | registered at 3.34 |
    | 3.11 | `ordin-slack-thread-adapter` (2) | `SlackThreadAdapter::new(SlackThreadConfig::default(), <SlackAgentDirectory>, <SlackUserTokenSource>, http)` | registered at 3.34 |
    | 3.12 | `ordin-usage-planner` (2) | `UsagePlanner::new(Datastore, Journal, Organisation Profile, vec![])`; the two local adapters' `HarnessModels` follow with `set_harnesses` after row 3.33 | `journal.register_prune_participant(usage.prune_participant())` |
    | 3.13 | `ordin-server-connector` (2) | `ServerConnector::new(ConnectorConfig { computer_name, client_certificate: <profile server.client_certificate_name when server.client_certificate_required>, read_cache_bytes: 32 MiB }, Journal, IdentityService as Arc<dyn DesktopKeyStore>)` | started at step 4 with the sender of 3.49 |
    | 3.14 | `ordin-rules` (3) | `RulesEngine::new(Datastore, Journal, Organisation Profile, Approval Service, <RulesHost> (*late-bound*), RulesConfig { program: Desktop, ordin_servers: <the MCP server names the desktop's providers claim, SA-NAME-01..04>, ordin_server_prefixes: <the claimed prefixes: craft_>, … })` (*chosen here*: the Rules Engine is built before the MCP Front, so the program passes the claimed lists that `McpFront::server_names()` reports after start) | — |
    | 3.15 | `ordin-brand` (3) | `Brand::start(BrandConfig { datastore: <the Journal's Datastore>, data_root, http }, Organisation Profile, Localisation)` (D4) | — |
    | 3.16 | `ordin-dictation` (3) | `DictationService::new(Localisation)` | — |
    | 3.17 | `ordin-checker` (3) | `Checker::new(Resolver and Compiler, Organisation Profile, Journal)`, without a part source (R18) | source bound at 3.36 |
    | 3.18 | `ordin-installers` (3) | `Installers::new(Agent Folder Manager, Resolver and Compiler)` | — |
    | 3.19 | `ordin-session-supervisor` (3) | `SessionSupervisor::new(Datastore, Journal, Usage Planner, None /* no Run Queue on the desktop */, <SupervisorHost> (*late-bound*), SupervisorConfig { program: Desktop, local_session_limit: 8, stop_deadline: 10 s, … })` | `journal.subscribe(supervisor.event_sink())`; `journal.register_prune_participant(supervisor.prune_participant())`; adapters registered at 3.32–3.34 |
    | 3.20 | `ordin-mcp-front` (4) | `McpFront::new(McpFrontConfig { app_data_root, installation_id, hook_program, profile: Desktop, … }, Rules Engine, Journal, Arc::new(StandardBlockCheck), None)` | providers registered at 3.27–3.47; `bind_ref_resolver` and `start` at 3.47 |
    | 3.21 | `ordin-package-inspector` (4) | `PackageInspector::new(Rules Engine, <data root>/inspections)` | — |
    | 3.22 | `ordin-files` (4) | `FileService::new(FileServiceDeps { journal, rules, agent_folders, data_root })` | `journal.register_prune_participant(files.prune_participant())` |
    | 3.23 | `ordin-hook-relay` (4) | `HookRelay::new(Datastore, Journal, Rules Engine, Approval Service, Session Supervisor, HookRelayConfig { app_data_root, installation_id })` | `journal.subscribe(hook_relay.event_sink())`; `start` at step 4 |
    | 3.24 | `ordin-crew-orchestrator` (4) | `CrewOrchestrator::new(Datastore, Journal, Session Supervisor, Rules Engine, Approval Service, Usage Planner, Organisation Profile, <CrewHost> (*late-bound*))` | `journal.register_prune_participant(orchestrator.prune_participant())` |
    | 3.27 | `ordin-sandbox` (5) | `Sandbox::start(SandboxDeps { dispatch: MCP Front, grants: <SandboxGrants>, identity, journal, place_files: <PlaceFiles>, session_idle, data_root, log_level })` | `mcp_front.register(sandbox.tool_provider())` |
    | 3.28 | `ordin-memory-service` (5) | `MemoryService::start(MemoryServiceConfig { program: Desktop, models_dir, agent_wide_scope: None, log_level }, Journal, shutdown token)` | `register(memory.tool_provider())` |
    | 3.29 | `ordin-craft-toolkit` (5) | `CraftToolkit::new(CraftDeps { journal, registry: MCP Front, rules, files, profile, http, data_root, platform, definitions })` | `register(craft.tool_provider())` |
    | 3.30 | `ordin-confluence` (5) | `ConfluenceAdapter::new(ConfluenceDeps { journal, identity, approvals, files, http, settings, word_to_markdown })` | `journal.subscribe(confluence.event_sink())` |
    | 3.30a | `ordin-jira` (5) | `JiraAdapter::new(JiraDeps { journal, identity, http })` (amended January 2026, ADR-106) | — (no tables, no event sink) |
    | 3.31 | `ordin-projects` (5) | `ProjectService::new(ProjectDeps { journal, tool_registry: MCP Front, files, rules, identity, installer: <ServerInstaller> (*late-bound*), streams: <StreamSink> (*late-bound*), settings, data_root })` | `register(projects.tool_provider())` |
    | 3.32 | `ordin-claude-adapter` (5) | `ClaudeLocalAdapter::new(ClaudeAdapterConfig { claude_path, hook_program, app_data_root, profile: Desktop, sign_in: <person's own sign-in>, stop_deadline: 10 s, … }, MCP Front, Hook Relay, Rules Engine, <SessionSetupSource>, None)` | `session_supervisor.register_adapter(claude)` |
    | 3.33 | `ordin-copilot-adapter` (5) | `CopilotLocalAdapter::new(CopilotAdapterConfig { cli_path: copilot_cli, … }, <CopilotPort>, MCP Front, Hook Relay, Rules Engine, Identity, <SessionSetupSource>)` | `register_adapter(copilot)` |
    | 3.34 | — | — | `register_adapter(hosted_agent)`, `register_adapter(slack_thread)` |
    | 3.35 | `ordin-workspaces` (6) | `WorkspaceService::new(WorkspaceDeps { journal, rules, files, sharepoint, confluence: Some(..), jira: Some(..) /* amended January 2026, ADR-106 */, data_root, documents_dir, on_server: false })` | `journal.subscribe(workspaces.clone())` (the service implements `EventSink`) |
    | 3.36 | `ordin-catalogue` (6) | `CatalogueService::new(CatalogueDeps { datastore, profile: Organisation Profile, github: Project Service as Arc<dyn GithubAccess>, formats, checker: Checker }, CatalogueConfig { root: <data root>/catalogue, bundled_catalogue, ordin_version })` | `resolver.install_part_source(catalogue)` then `checker.install_part_source(catalogue)` (R18; an error here is `StartError::ModuleFailed`) |
    | 3.37 | `ordin-push` (6) | `PushService::new(PushDeps { journal, approvals, working_copy: <WorkingCopyStore>, files, projects, github: Project Service as Arc<dyn GithubAccess>, sharepoint, confluence: Some(..), craft })` | `journal.subscribe(push.event_sink())` |
    | 3.38 | `ordin-knowledge` (7) | `KnowledgeService::new(ProgramKind::Desktop, KnowledgeDeps { journal, files, workspaces, rules, memory, push, agent: None })` | `register(knowledge.tool_provider())` |
    | 3.39 | `ordin-mail` (7) | `MailService::start(MailDeps { workspaces, agent_names: <AgentNames> (*late-bound*), identity, rules, approvals, files, profile, journal, tools: MCP Front }, shutdown token)` | `register(mail.tool_provider())`; `journal.register_prune_participant(mail.prune_participant())` |
    | 3.39a | `ordin-slack-service` (7) | `SlackService::start(SlackServiceDeps { identity, approvals, workspaces, rules: <SlackRulesSource>, journal, registry: MCP Front, reactor: None, http, api_base: https://slack.com/api/, program: Desktop })` and, after row 3.41 builds the Ordin Host, `slack.set_reactor(<SlackReactor over OrdinHost::react_to_slack>)`` | `mcp_front.register(slack.tool_provider())`; its migrations are registered at 3.1 (`SlackService::migrations()`) |
    | 3.40 | `ordin-package-catalogue` (7) | `PackageCatalogue::new(PackageCatalogueDeps { datastore, journal, catalogue, resolver, checker, installers, folders, inspector, harness, activity })` | — |
    | 3.41 | `ordin-host` (8) | `OrdinHost::new(OrdinHostConfig { profile: Desktop, … }, OrdinHostPolicy { coordinator_name: brand.coordinator_name(), … from the profile }, OrdinHostDeps { supervisor, orchestrator, usage, inspector, catalogue: Package Catalogue, memory, knowledge, push, projects, workspaces, journal, datastore: Datastore, blocks: the same StandardBlockCheck, files: File Service, refs: <RefResolver>, localisation: Localisation (amended January 2026, ADR-109) })` | `register(ordin_host.tool_provider())`; `mcp_front.bind_block_store(ordin_host.block_store())` (McpFront.md SA-MCP-BLOCK-02; amended 2026-01-10, approved by the user on 2026-01-10; an error, `MCP_BLOCK_STORE_ALREADY_BOUND`, is `StartError::ModuleFailed`); `journal.register_prune_participant(ordin_host.prune_participant())`; `set_policy` on each profile change and each brand change |
    | 3.41a | `ordin-team-knowledge` (8) | `TeamKnowledgeService::new(TeamKnowledgeDeps { journal, memory: <TeamMemory>, pages: <TeamPages>, push: <TeamPush>, workspaces: <TeamWorkspaces>, repos: <TeamRepos>, clones_dir: <data root>/team-knowledge })` (amended January 2026, ADR-103) | its migrations are registered at 3.1 (`TeamKnowledgeService::migrations()`); `run(shutdown token)` spawned at step 4 |
    | 3.42 | `ordin-home-search` (8) | `HomeSearch::new(HomeSearchDeps { journal, reader: <JournalReader>, knowledge: knowledge.reader(), files, workspaces, sharepoint, confluence: Some(..) })` | `register_source` for each search source |
    | 3.43 | `ordin-setup` (8) | `SetupAssistant::new(SetupDeps { profile, identity, catalogue, packages, craft }, SetupConfig { program: Desktop, bundled_programs: [ordin-hook, copilot], application_version, http, url_opener }, <the Journal's Datastore>)` (D4) | `register_updater(TauriUpdater)`; `register_probe(<Copilot HarnessProbe>)` |
    | 3.44 | `ordin-authoring` (9) | `AuthoringService::new(AuthoringDeps { datastore, journal, model, catalogue, formats, checker: Checker, host: Ordin Host as Arc<dyn AuthoringHost>, push, places: <HandoverReach> }, <data root>/authoring)` | `register(authoring.tool_provider())`; `journal.register_prune_participant(authoring.prune_participant())` |
    | 3.45 | `ordin-handover` (9) | `HandoverService::new(HandoverDeps { slack: <the Slack Service>, journal, catalogue, checker: Checker, approvals, push, github: Project Service as Arc<dyn GithubAccess>, sharepoint, mail, workspaces, host: Ordin Host as Arc<dyn HandoverHost>, identity: <PersonIdentity>, jira: Some(..) /* amended January 2026, ADR-106 */ })` | `journal.subscribe(handover.event_sink())` |
    | 3.46 | `ordin-telegram-adapter` (10) | `TelegramAdapter::new(TelegramDeps { mode: Desktop, secrets: identity.secrets(), journal, gateway: None })?` (no Channel Gateway on the desktop, ADR-077; tables through the Journal's `Datastore`, K7) | `start(shutdown token)` at step 4 |
    | 3.47 | `ordin-telegram-service` (11) | `TelegramService::start(TelegramServiceDeps { bot, approvals, workspaces, files, places, rules, journal, registry: MCP Front, app_probe: Some(<TelegramAppProbe>), program: Desktop })` | `register(telegram.tool_provider())`; then `mcp_front.bind_ref_resolver(<RefResolver>)` (SA-CLOSE-01), then `mcp_front.start(shutdown token)`, which freezes the registry |
    | 3.48 | `DesktopServiceApi`, `DesktopErrorSentences` | an `Arc` of every module above that implements a catalogue command (the *Implemented by* column of the Service API catalogue), Localisation, `<data root>/preferences.json`, `AppHandle` | — |
    | 3.49 | `ordin-command-bridge` (3) | `CommandBridge::new(BridgeConfig { local_backend, server_replay_events: 1000 }, DesktopServiceApi as Arc<dyn ServiceApi>, DesktopErrorSentences as Arc<dyn ErrorSentences>, Server Connector, Journal)` | `app.manage(bridge)`; keep the returned `ServerEventSender` for step 4; `bridge.start(cancel)` (it subscribes itself to the Journal) |

    Rows 3.25 and 3.26 are unused (numbering kept stable for the Generation Playbook). After 3.49 log `wired` with the counts of modules, tool providers and adapters.
4. **Start background work, in order.** `journal.recover_external_actions()`; `journal.set_company_max_days(<profile history.max_days>)`; `agent_folders.recover()`; `package_inspector.reload_rule_sets()` (each failure logged `rule_set_reload_failed`, start continues; an instance whose set did not load is refused by the Rules Engine, fail-safe); `hook_relay.start()` (a failure is `StartError::ModuleFailed { module: "ordin-hook-relay" }`); `session_supervisor.recover()` → `RecoveryReport` (runs resume or are marked); `crew_orchestrator.recover()` (crews wait for the person, 03 *Recovery after restart*); `start(shutdown)` of the Catalogue Service, Telegram Adapter and Ordin Host, and `start()` of the File, Workspace, Project, Confluence, Craft and Home and Search services; `Notifier::spawn(app, bridge.subscribe_all(), …)` (step 7); `WakeWatcher::spawn` (step 11); `server_connector.start(sender, connector_cancel)` with the sender of 3.49 (not awaited); `setup_assistant.start_background_checks()` (not awaited: profile fetch, harness sign-in checks, repository clones run in the background, 03 *First start*). Each sidecar path from `DesktopPaths` that does not exist is logged as `DSK_SIDECAR_MISSING` with the file name; start continues and the Setup Assistant reports it to the person. `slack.start_reading(shutdown token)` reads every linked channel while ordin is open (SA-RATE-02: every 60 s; at start it first reads what arrived since `read_up_to_ts`, at most 1,000 messages per channel). Nothing is read while ordin is closed: no login item, no background service (ADR-011, ADR-072). (amended January 2026, ADR-102)
5. **Commands the program answers.** Every other catalogue method of `DesktopServiceApi` calls the module its *Implemented by* column names, with the `CallContext`, and maps the module's error `e` to `ServiceError { code: e.code(), message: sentences.sentence(e.code()), action: e.repair().or(sentences.action(e.code())) }` (ServiceApi *Error mapping*). `authoring.publish` first reads `AuthoringService::lifecycle_of(&draft)` and, for `Some(lifecycle_id)`, fills `PublishContext` from `HandoverService::following(lifecycle_id)` (SA-CLOSE-02); `checker.check_installed` and `checker.check_library_item` get `lock` and `scope` as SA-CLOSE-06 states (a `PackageError` from `source_scope` maps as any module error); `identity.save_gitlab_token` passes the profile's `gitlab.host` (default `gitlab.com`); `workspaces.open` also calls `ProjectService::workspace_opened` and `HomeSearch::workspace_opened`; a workspace removal also calls `OrdinHost::workspace_removed`; `dictation.start` follows step 12; `hosted.decide_card` calls `HostedAgentAdapter::decide_approver_card` as SA-CLOSE-09 states (an `AdapterError` maps as any module error, so `HAA_CARD_DECIDED`, `HAA_NO_ACCESS` and `HAA_CARD_UNKNOWN` reach the screens with their codes; `HAA_CARD_DECIDED` and `HAA_CARD_UNKNOWN` also drop the card from the kept list). The program's own: `service.hello` and `settings.*` as in the table of *Public Interface*; `files.save_copy` (save dialog, `files.download` on the source backend, atomic local write; a write failure → `DSK_SAVE_COPY_FAILED`); `app.quit` (step 10); `app.pick_folder` (`tauri_plugin_dialog` `FileDialogBuilder::pick_folder` on the main window; a picker failure → `DSK_PICKER_UNAVAILABLE`); `identity.import_client_certificate` (SA-CLOSE-07): profile `server.client_certificate_name` absent → `CRED_PROVIDER_NOT_CONFIGURED`; `FileDialogBuilder::pick_file` with the filter `pem`, `crt`, `cer` for the certificate, then with `pem`, `key` for the key (a picker failure → `DSK_PICKER_UNAVAILABLE`; either cancelled → `{ certificate: None }`, nothing read); each file read whole, refused above 64 KiB with `CRED_CERTIFICATE_INVALID`; then `IdentityService::import_client_certificate(name, cert, Zeroizing(key))`, the key buffer zeroised after the call. The Tauri plugin `ordin` is added to the `Builder` in `main` with `CommandBridge::plugin()`, and the build script registers `IPC_COMMANDS` with `tauri_build::Attributes::new().plugin("ordin", InlinedPlugin::new().commands(IPC_COMMANDS).default_permission(DefaultPermissionRule::AllowAllCommands))` (the `ordin:default` set, CommandBridge.md); the plugin's commands find the bridge in Tauri's managed state, which exists before the window is created.
6. **Window.** Create the window `main` with `WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))`: title from the brand's product name, 1280 × 800, minimum 480 × 560, `focused(true)`, drag-and-drop enabled. Log `window_shown` with the milliseconds since process start. On `WindowEvent::CloseRequested`, call `api.prevent_close()` and run step 10.
7. **Notifications.** The `Notifier` reads every `EventEnvelope` from the Command Bridge's merged stream (local and server events):
    1. Map the event to a candidate notice:

        | Event | Condition | Preference | Title (message id) | Body (message id) | Target |
        |---|---|---|---|---|---|
        | `ApprovalRequested { card }` | — | `approvals` | `notification-approval-title` = "{ $source }: { $question }" | `notification-approval-body` = "Waiting for you" (plus "· On { $server }" for a server) | `Approval` |
        | `RunStateChanged { run_id, state: Completed }` | — | `finished_work` | `notification-run-done-title` = "{ $agent } finished" | `notification-run-done-body` = "Done · { $workspace }" | `Run` |
        | `CrewRoundChanged { state: Finished }` | — | `finished_work` | `notification-crew-done-title` = "Crew finished" | `notification-crew-done-body` = "Done · { $workspace }" | `CrewRun` |
        | `BlockShown { block: Block::Question(q) }` | — | `questions` | `notification-question-title` = "Ordin has a question" | the question text, first 120 characters | `Question` |
        | `NeedsYouChanged { workspace_id }` | `home.needs_you { workspace_id }` on that backend returns a row whose `opens` is `OpenTarget::Handover` and whose `handover_id` was not seen before in this program session (title from the row's `line`) | `handovers` | `notification-handover-title` = "Handed to you: { $title }" | `notification-handover-body` = "Waiting for you" (plus "· On { $server }") | `Handover` |
        | `NeedsYouChanged` (any backend); `AuthoringNotice::Published` (local) | a draft's `ReviewDraftState` changed to `changes_asked_for` or `published` (step 7.6) | `draft_reviews` | `notification-draft-changes-title` / `notification-draft-published-title` | `notification-draft-changes-body` / `notification-draft-published-body` | `Draft` |
        | any other | — | — | no notice | — | — |

        `$source` is the agent's or Ordin's name for a local card and the server's display name for a server's card (`ServerConnector::display_name`); `$agent` and `$workspace` come, for the local backend, from `SessionSupervisor::get(run_id)` (`RunView.run.agent`) and the workspace's name from `WorkspaceService::get` when the run's `origin` names a workspace; for a server's run, or when the label is absent, the title is `notification-run-done-title-unnamed` = "A task finished" and "On { $server }" is added to the body. "Ordin" in every message is the brand's coordinator name (ADR-041). All text is built with `Localisation::message` in the person's locale.
    2. Drop the notice when its preference is off, or when the main window is focused and visible *(assumed here: the person is already looking at the window, where the item appears in Needs you or on its screen)*.
    3. Hold each approval notice for 1.5 s *(assumed here)*; if `ApprovalDecided` for the same `approval_id` arrives meanwhile, drop it (11 §7 "Nothing is notified twice"). Hold no other kind.
    4. Show it with `tauri_plugin_notification` (`app.notification().builder().title(..).body(..).show()`). Record `{target, shown_at}` as the pending target. An OS refusal is logged as `DSK_NOTIFY_FAILED` and never retried.
    5. When the main window next gains focus within 5 minutes of the last shown notice, emit `ordin://open-target` with that notice's target and clear it *(assumed here: Tauri 2's notification plugin reports no click on desktop platforms, so focus after a notice is how "opens the view that resolves it" is met)*.
    6. **Draft reviews** (SA-CLOSE-03). At step 4 the Notifier reads `authoring.list { agent_repository_id: None, open_only: false }` on each backend (local, and each server when it connects) and keeps `DraftRef → ReviewDraftState`. On every `NeedsYouChanged` of a backend, and for the local backend on every `AuthoringNotice::Published` from `AuthoringService::subscribe()`, it reads the list again (at most once per second per backend; reads coalesce) and, for each draft whose state differs from the kept one: now `changes_asked_for` → notice `notification-draft-changes-title` = "Changes asked for: { $name }", body `notification-draft-changes-body` = "Waiting for you" (plus "· On { $server }"), preference `draft_reviews`, target `Draft { backend, draft }`; now `published` → `notification-draft-published-title` = "Published: { $name }", body `notification-draft-published-body` = "In { $repository }", same preference and target. Then it keeps the new states. A read that fails is logged `DSK_DRAFT_LIST_FAILED` (warn) and no notice is raised; the next event reads again. A handover returned to the person arrives as the same `Handover` item and uses the same notice *(assumed here: `OpenTarget::Handover` does not distinguish handed from returned)*.
8. **Updates.** `TauriUpdater` implements the Setup Assistant's `AppUpdater` and is given to it with `register_updater` (row 3.43); the Setup Assistant calls it with its `effective_update_feed()` (the installer's feed, or a verified profile's `updates.feed_url`, D10). `TauriUpdater::check` builds `app.updater_builder().endpoints(vec![feed_url])?.build()?` and calls `check()`; it maps a found update to `UpdateOffer { version, notes }`. `install` downloads and installs with the plugin's signature check against the build's public key (05, A-02). Errors → `DSK_UPDATE_CHECK_FAILED` (retryable) and `DSK_UPDATE_INSTALL_FAILED` (retryable). The Setup Assistant decides when to check and what the person sees.
9. **Start failure.** On any `StartError`: log `start_failed` with the code and module; show a native message box (`tauri_plugin_dialog` `MessageDialogBuilder`, kind error, one button) with `desktop-start-failed` = "ordin couldn't start on this computer. Open it again; if this happens again, reinstall ordin." (*amended January 2026 (user decision; Approved by the user January 2026)*) and `desktop-start-failed-reference` = "Reference { $reference }", where the reference is the id of the `start_failed` log line; the text is built from the `en-GB` catalogue compiled into the program when Localisation is not yet constructed. On its button, exit with status 1.
10. **Quit.** On `CloseRequested`, the application menu's Quit, or `app.quit`:
    1. If a shutdown is already running, do nothing more.
    2. When the trigger is not `app.quit` and `session_supervisor.running_local_sessions() > 0`, emit `ordin://quit-requested { running_sessions }` and stop here; the screens ask (07: "the application asks before ending running sessions") and answer with `app.quit` or nothing.
    3. Otherwise run `Core::shutdown(QuitReason::PersonQuit)`: `command_bridge.close()` (later commands are refused by the Bridge with `BRG_CLOSING`); `dictation.cancel(session_id)` for the session the program is forwarding, if any (DictationService *On application quit*); `session_supervisor.shutdown(ShutdownReason::Quit)` (SA-STOP-01; runs past the 10 s stop deadline are ended and marked interrupted; crews pause and wait for the person after the next start); cancel the Server Connector's token (remote runs continue remotely, 07); `setup_assistant.close()`; `shutdown()` on the Slack Service, Telegram Service, Workspace Service, Project Service, Craft Toolkit, Sandbox, File Service and Organisation Profile; cancel the core's shutdown token (Mail Service, Catalogue Service, Memory Service, Telegram Adapter, MCP Front, Ordin Host, Team Knowledge Service — amended January 2026, ADR-103: a share waiting for its card or its send stays recorded and resumes at the next start); `hook_relay.shutdown()`; `journal.shutdown()` then `datastore.close()`. Each step is bounded: 15 s for the Session Supervisor's shutdown, 2 s for every other step; a step that overruns is logged with `DSK_SHUTDOWN_STEP_TIMEOUT` and the next step runs.
    4. Log `shutdown` with the summary and call `app.exit(0)`.
    5. When the operating system ends the session (`RunEvent::ExitRequested` without a person's quit), run step 3 without asking (`QuitReason::OperatingSystemEnding`).
11. **Sleep and wake.** `WakeWatcher` ticks every 5 s on a Tokio interval with `MissedTickBehavior::Skip` and compares the wall clock (`SystemTime`) and the monotonic clock (`Instant`) since the last tick. When either the tick arrives more than 30 s late by `Instant`, or the wall clock advanced more than 30 s further than `Instant`, the computer slept *(assumed here: no operating-system power notification library is named in 1A or 05; the two tests cover macOS, whose monotonic clock pauses in sleep, and Windows, whose timer fires late)*. It then calls `session_supervisor.on_wake(slept_for)` (local sessions that cannot continue are marked interrupted, 07) and `server_connector.on_wake()` (reconnect at once), and calls `dictation.cancel(session_id)` for the dictation session the program is forwarding, if any (the microphone stream did not survive the sleep; DictationService *system sleep*). Logged: `wake` with `slept_for_s`.
12. **Dictation sessions.** `dictation.start` reaches `DesktopServiceApi`, which calls `DictationService::start(field)`, keeps the returned `session_id` as the current session, spawns one task that forwards every `DictationUpdate` from `DictationSession.updates` to `ScreenStreams::send(session_id, StreamKind::Dictation, update)`, and clears the current session after `Ended`. `ScreenStreams::send` returning `BRG_STREAM_NO_WINDOW` cancels the session. A new `dictation.start` replaces the current session (the Dictation Service ends the earlier one with `Ended { Replaced }`).

13. **Handover links (`ordin://handover/open`)** (amended January 2026, ADR-102).
    1. `tauri-plugin-deep-link` delivers each `ordin:` URL the operating system hands to the program: at start (`get_current()`, a URL the program was launched with) and while running (`on_open_url`). Registration is for the current user only, made by the plugin at first start without administrator rights — macOS: the bundle's `CFBundleURLTypes` (registered by Launch Services when the bundle in the user's Applications folder is first opened); Windows: `HKCU\Software\Classes\ordin` (`register_all()`) (ADR-072; confirmation C-85). A failure to register is logged `DSK_LINK_REGISTER_FAILED` and start continues; links then do not open ordin, and the place link in the message still works.
    2. Each URL, before anything else: at most 4,096 bytes; scheme `ordin`; host `handover`; path `/open` → else log `DSK_LINK_REFUSED {reason}` (no content) and do nothing. The URL is data: it is handed whole to `HandoverService::open_link(url)` (through `DesktopServiceApi`, local backend), which checks every part (HandoverService.md: `HAND_LINK_INVALID`, `HAND_LINK_PLACE_REFUSED`).
    3. The command is `handover.open_link { link }` (scope Local, owner Handover Service), result `LinkOpened`:
   - `LinkOpened::Opened { handover_id }` → show, un-minimise and focus the main window and emit `ordin://open-target` with `NotificationTarget::Handover { backend: local, workspace_id: None, handover_id }` (or `Some` when the handover was accepted into a workspace, read from `handover.open`); the screens open the handover screen (ScreensModule step 13).
   - `LinkOpened::NotFoundYet { place }` → focus the window and open Home with the line `home-handover-link-following` = "ordin now follows { $place }; the handover will appear when it's readable."; *Waiting for you* shows the handover when the next check reads it (the place is followed now and checked at once, so it appears within one minute).
   - Any error (`HAND_LINK_INVALID`, `HAND_LINK_PLACE_REFUSED`, others) → focus the window and show the error's sentence on Home (`ErrorLine`).
    4. Links before the core has started are held (at most 5, *chosen here*) and run after step 5.

14. **Relayed sign-in for a paired server** (amended January 2026, ADR-102; IdentityAndCredentials.md, relayed browser sign-in).
    1. The screens call the local `identity.relay_sign_in { server, reason }` when the person signs in to Slack for a paired server (Settings › Slack of that server's backend).
    2. This program binds the first free loopback port of 47821–47823 on `127.0.0.1` (all busy → `CRED_LOOPBACK_BUSY`, the Identity code, passed through) and calls the server's `identity.sign_in { kind: slack, reason, relay_port: <that port> }` through the Server Connector.
    3. The answer must be `SignInChallenge::RelayedBrowser { challenge_id, authorisation_url, expires_at }`; this program checks that `authorisation_url` starts with `https://slack.com/oauth/v2/authorize?` and that its `redirect_uri` is `http://127.0.0.1:<that port>/callback` (else `DSK_RELAY_URL_REFUSED`, nothing opened), then opens it with `BrowserOpener`.
    4. The listener answers one `GET /callback`; `code` and `state` are forwarded at once with the Server Connector to the server's `identity.complete_relayed_sign_in { challenge_id, code, state }`; the desktop keeps neither (never logged, never stored). The page shown in the browser reads "You can close this page and go back to ordin." The server checks `state` and exchanges the code with the PKCE verifier it holds; the token exists only on the server. The command returns the server's `CredentialView`.
    5. No callback before `expires_at` (10 minutes) → the listener closes; `DSK_RELAY_TIMEOUT` is returned and shown as the sign-in's timeout sentence; the server's challenge expires on its own.

## Data Model

The program owns no table. It owns one file:

**`<data root>/preferences.json`** (mode `0600` on macOS), UTF-8 JSON, written atomically (write `preferences.json.tmp`, `fsync`, rename):

```json
{
  "format": 1,
  "notifications": { "approvals": true, "finished_work": true, "questions": true, "draft_reviews": true, "handovers": true },
  "appearance": "system"
}
```

- Absent file → defaults as shown; nothing is written until the first `settings.change` of one of its keys.
- Unreadable or invalid file (bad JSON, unknown `format`, unknown field) → defaults are used, the file is left in place, and `DSK_PREFERENCES_UNREADABLE` is logged once; the next `preferences.write` replaces it.
- `format` other than `1` written by a newer program → treated as unreadable (no downgrade).
- Removal: deleted with the data root when ordin is removed; not exported, not journalled.

Side-panel widths, the tree's show switches and other view preferences "remembered on that computer only" (11 §4) are kept by the screens module, not here.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `<data root>` cannot be created or written | `Err(permanent)` — start stops, message box, exit 1 | `DSK_DATA_ROOT_UNWRITABLE` | message box: "ordin couldn't start on this computer. Open it again; if this happens again, reinstall ordin." (*amended January 2026 (user decision; Approved by the user January 2026)*) |
| A module's constructor, a part-source binding or `hook_relay.start()` fails (database newer than the program, migration failure, hook channel in use) | `Err(permanent)` — start stops, message box, exit 1 | `DSK_START_FAILED` | as above; the log line names the module and the module's own code |
| A command arrives after quitting began | `Err(permanent)`, raised by the Command Bridge | `BRG_CLOSING` | `{"request_id":…,"kind":"error","error":{"code":"BRG_CLOSING",…}}` |
| An agent-profile-only `SupervisorHost` method is reached on the desktop | `Err(permanent)`, never expected | `DSK_AGENT_PROFILE_ONLY` | `HostError { code: "DSK_AGENT_PROFILE_ONLY" }`, logged at `error` |
| `settings.change` of a `notifications.*` or `display.appearance` key cannot write the file | `Err(retryable)` | `DSK_PREFERENCES_WRITE_FAILED` | `{"kind":"error","error":{"code":"DSK_PREFERENCES_WRITE_FAILED","message":"Your choice couldn't be saved.","action":"try_again"}}` |
| `settings.change` of one of those keys with a value of the wrong type | `Err(permanent)` | `DSK_PREFERENCE_VALUE_INVALID` | `{"kind":"error","error":{"code":"DSK_PREFERENCE_VALUE_INVALID","message":"That choice isn't one of the options."}}` |
| `preferences.json` unreadable at `settings.get` | logged; defaults returned | `DSK_PREFERENCES_UNREADABLE` | the defaults as a normal result |
| The person cancels the save dialog of `files.save_copy` or `files.save_export` | `Err(permanent)`, no action | `DSK_DIALOG_CANCELLED` | `{"kind":"error","error":{"code":"DSK_DIALOG_CANCELLED","message":"Nothing was saved."}}` |
| The copy, or an exported file of `files.save_export` (amended January 2026, ADR-109), cannot be written where the person chose | `Err(retryable)` | `DSK_SAVE_COPY_FAILED` | `{"kind":"error","error":{"code":"DSK_SAVE_COPY_FAILED","message":"The copy couldn't be saved there.","action":"try_again"}}` |
| The operating system's folder picker fails to open | `Err(retryable)` | `DSK_PICKER_UNAVAILABLE` | `{"kind":"error","error":{"code":"DSK_PICKER_UNAVAILABLE","message":"The folder chooser couldn't open.","action":"try_again"}}` |
| `hosted.decide_card` refused by the hosted agent (card already decided, person not an approver there, card unknown) or the agent unreachable | `Err(permanent)` for the refusals, `Err(retryable)` when unreachable; the module's error mapped as any module error | `HAA_CARD_DECIDED` / `HAA_NO_ACCESS` / `HAA_CARD_UNKNOWN` / `RA_UNREACHABLE` | `{"kind":"error","error":{"code":"HAA_CARD_DECIDED","message":"This was already decided."}}` |
| The operating system refuses a notification (permission off) | logged only, never retried | `DSK_NOTIFY_FAILED` | — |
| The `ordin` link scheme could not be registered for this user | logged; start continues | `DSK_LINK_REGISTER_FAILED` | — |
| An `ordin:` URL is not a handover link (scheme, host, path or length) | logged; nothing opened | `DSK_LINK_REFUSED` | — |
| A relayed sign-in URL is not Slack's authorise URL with a loopback redirect of this computer | `Err(permanent)` | `DSK_RELAY_URL_REFUSED` | `{"kind":"error","error":{"code":"DSK_RELAY_URL_REFUSED","message":"This sign-in can't be finished from this computer."}}` *(chosen here)* |
| No browser callback within 10 minutes of a relayed sign-in | `Err(retryable)` | `DSK_RELAY_TIMEOUT` | the sign-in timeout sentence |
| `authoring.list` fails while the Notifier compares draft states | logged (warn); no notice; the next event reads again | `DSK_DRAFT_LIST_FAILED` | — |
| `bind_ref_resolver` refused at start (already bound) | `Err(permanent)` — start stops, message box, exit 1 | `DSK_START_FAILED` (module `ordin-mcp-front`, code `MCP_REF_RESOLVER_ALREADY_BOUND`) | as for any module failure |
| A bundled program (`ordin-hook`, `copilot`) is missing beside the executable | logged; start continues; the Setup Assistant reports it | `DSK_SIDECAR_MISSING` | — |
| A shutdown step overruns its bound | logged; the next step runs | `DSK_SHUTDOWN_STEP_TIMEOUT` | — |
| The update feed cannot be read | `Err(retryable)` to the Setup Assistant | `DSK_UPDATE_CHECK_FAILED` | — (the Setup Assistant owns the words) |
| Download or signature check of an update fails | `Err(retryable)` to the Setup Assistant | `DSK_UPDATE_INSTALL_FAILED` | — |

Every `message` above is a catalogue message (`desktop-quitting`, `desktop-preferences-not-saved`, `desktop-preference-invalid`, `desktop-nothing-saved`, `desktop-copy-not-saved`, `desktop-picker-failed`; ids *chosen here*) in the person's locale; the person never sees a code (07).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: ordin desktop

  Scenario: Happy path — start wires every module and shows the window
    Given a writable data root and a journal at the current schema
    When the person opens ordin
    Then the modules are constructed in the order of Internal Logic step 3
    And nine tool providers are registered with the MCP Front before McpFront.start
    And four runtime adapters are registered with the Session Supervisor
    And the Catalogue Service is bound as the part source of the Resolver and Compiler and of the Checker
    And bind_ref_resolver is called once on the MCP Front after the Telegram Service is built and before McpFront.start
    And reload_rule_sets is called before session_supervisor.recover
    And no SQLite pool is handed to the Telegram Adapter, and TelegramAdapter::migrations() and HomeSearch::migrations() are registered
    And the window "main" is created only after the Command Bridge plugin is added
    And "window_shown" is logged within 2000 ms of process start

  Scenario: Happy path — a server's approval raises one notification
    Given the main window is not focused and approval notifications are on
    When an ApprovalRequested event arrives from Northwind server
    Then after 1.5 seconds one notification titled "Northwind server: Publish the revised account brief?" is shown
    When the person brings the window forward within 5 minutes
    Then "ordin://open-target" is emitted with kind "approval" and that card's identifiers

  Scenario: Edge case — an approval decided in a chat raises no notice
    Given the main window is not focused
    When ApprovalRequested and, 400 ms later, ApprovalDecided for the same approval arrive from a server
    Then no notification is shown

  Scenario: Edge case — notification preference off
    Given preferences.notifications.finished_work is false
    When RunStateChanged with state "completed" arrives
    Then no notification is shown

  Scenario: Edge case — quit with running sessions asks first
    Given two local sessions are running
    When the person closes the main window
    Then the window stays open
    And "ordin://quit-requested" is emitted with running_sessions 2
    When the screens send app.quit
    Then each session is given 10 seconds to end its turn
    And Journal.shutdown and Datastore.close run before the process exits with status 0

  Scenario: Edge case — quit with nothing running does not ask
    Given no local session is running
    When the person chooses Quit
    Then no "ordin://quit-requested" is emitted
    And the program exits with status 0 after Journal.shutdown and Datastore.close

  Scenario: Edge case — waking from sleep
    Given the computer slept for 40 minutes
    When the next 5-second tick arrives
    Then SessionSupervisor.on_wake is called with about 2400 seconds
    And ServerConnector.on_wake is called

  Scenario: Edge case — quitting while dictating
    Given a dictation session 0192b3c4-1111-7222-8333-444455556666 is running
    When the person quits and no local session is running
    Then DictationService.cancel is called with 0192b3c4-1111-7222-8333-444455556666 before Journal.shutdown runs

  Scenario: Edge case — waking from sleep while dictating
    Given a dictation session is running and the computer slept for 10 minutes
    When the next 5-second tick detects the sleep
    Then DictationService.cancel is called for that session

  Scenario: Edge case — second launch
    Given ordin is already running
    When the person opens ordin again
    Then the second process exits
    And the first instance's main window is shown and focused

  Scenario: Error path — data root not writable
    Given the data root cannot be written
    When the person opens ordin
    Then a message box says "ordin couldn't start on this computer. Open it again; if this happens again, reinstall ordin."
    And the log holds "start_failed" with code DSK_DATA_ROOT_UNWRITABLE
    And the program exits with status 1

  Scenario: Error path — a module fails to start
    Given the journal database is newer than the program
    When the person opens ordin
    Then no module after the Journal is constructed
    And the log holds "start_failed" with code DSK_START_FAILED and module "ordin-journal"

  Scenario: Happy path — save a copy of a server's document
    When the screens send files.save_copy for a document of Northwind server
    And the person chooses a folder in the save dialog
    Then files.download is called on Northwind server's backend
    And the file is written there and the reply holds its path

  Scenario: Error path — save dialog cancelled
    When the person cancels the save dialog of files.save_copy
    Then the reply is an error with code DSK_DIALOG_CANCELLED and no file is written

  Scenario: Error path — a command after quitting began
    Given Core.shutdown is running
    When the screens send any command
    Then the reply is an error with code BRG_CLOSING from the Command Bridge

  Scenario: Error path — preferences file damaged
    Given preferences.json holds invalid JSON
    When the screens send settings.get for the section notifications
    Then the reply holds the defaults
    And DSK_PREFERENCES_UNREADABLE is logged once

  Scenario: Error path — preferences cannot be saved
    Given the data root has become read-only
    When the screens send settings.change for notifications.approvals
    Then the reply is an error with code DSK_PREFERENCES_WRITE_FAILED and action "try_again"

  Scenario: Error path — a shutdown step hangs
    Given the Telegram Service's shutdown does not return
    When the person quits
    Then DSK_SHUTDOWN_STEP_TIMEOUT is logged after 2 seconds
    And the Journal is still shut down and the datastore closed

  Scenario: Error path — a missing bundled program
    Given the copilot sidecar is missing beside the executable
    When the person opens ordin
    Then DSK_SIDECAR_MISSING is logged naming "copilot"
    And the window is still shown

  Scenario: Happy path — a review asks for changes on the person's own draft
    Given the Notifier keeps the local draft "Contract clause checker" in state waiting_for_review
    And the window is not focused
    When needs_you_changed arrives from the local backend and authoring.list now shows that draft in changes_asked_for
    Then one notice "Changes asked for: Contract clause checker" with body "Waiting for you" is shown
    And on the next focus within 5 minutes ordin://open-target carries kind "draft" and that draft

  Scenario: Edge case — the same draft state raises no second notice
    Given the Notifier keeps a draft in changes_asked_for
    When needs_you_changed arrives again and the draft is still in changes_asked_for
    Then no notice is shown

  Scenario: Error path — the draft list cannot be read
    Given authoring.list fails on the local backend
    When needs_you_changed arrives
    Then DSK_DRAFT_LIST_FAILED is logged and no notice is shown

  Scenario: Happy path — publishing a lifecycle draft fills the publish context
    Given a draft for which AuthoringService::lifecycle_of returns lifecycle "0192a1e0-1111-7222-8333-444455556666"
    When the screens send authoring.publish with that draft
    Then HandoverService::following is called with that lifecycle id and publish receives Some(PublishContext)
    And for a draft whose lifecycle_of returns None, publish receives None

  Scenario: Happy path — Check it on an installed agent
    When the screens send checker.check_installed for agent instance "0192a1c9-aaaa-7bbb-8ccc-ddddeeee0002"
    Then the program calls PackageCatalogue::source_scope for that instance and Checker::check_installed with the bundle's lock file and that scope

  Scenario: Edge case — importing a client certificate is cancelled
    Given the profile names server.client_certificate_name "ACME desktop"
    When the screens send identity.import_client_certificate and the person cancels the key file picker
    Then the reply is {"certificate": null} and nothing is stored

  Scenario: Happy path — a hosted agent's approver card is listed and pressed (SA-CLOSE-09)
    Given the hosted agent "support-agent" returns one approver card with approval A and question "Send the refund to the customer?"
    When home.needs_you is called with no workspace
    Then one row has workspace_id None and opens hosted_approval with approval A
    When the screens send hosted.decide_card { server_or_agent: "support-agent"'s id, approval_id: A, decision: "approve" }
    Then decide_approver_card("support-agent", A, Approve, None) is called and the result is { state: "approved" }
    And the next home.needs_you holds no row for approval A

  Scenario: Error path — another approver decided the card first
    Given decide_approver_card returns Refused with code HAA_CARD_DECIDED
    When the screens send hosted.decide_card for approval A
    Then the reply's error code is HAA_CARD_DECIDED with message "This was already decided."
    And the card is dropped from the kept list

  Scenario: Error path — the SharePoint Adapter refuses to start
    Given SharePointAdapter::new returns an error with code SP_DATASTORE_UNAVAILABLE
    When the person opens ordin
    Then start stops with DSK_START_FAILED, module "ordin-sharepoint"
```

Integration test stubs (one each): construction with fake modules records the order of 3.1–3.49; a fake `ServiceApi` call through the real Command Bridge plugin under the `main` capability; the WebView cannot call a command outside `ordin:default` (Tauri refuses it).

## Performance, Security, Observability

- **Performance targets:** process start to `window_shown` ≤ 2.0 s p95 on a supported machine, so the conversation is interactive within 3 s (A-03); module construction does no network call and no repository clone (those run in the background). Event to notification ≤ 1 s, except approvals, held 1.5 s by design. Quit with no running session ≤ 3 s; with sessions ≤ 20 s. Idle core resident memory, excluding child processes and the memory component's models: ≤ 250 MB *(assumed here)*.
- **Security:**
  - The WebView is limited by the `main` capability to the `ordin` plugin's Service API commands and to listening for events; it has no file-system, shell, HTTP, dialog or notification permission (07 Authorisation layer 1). The CSP allows no remote origin; `connect-src` is the IPC origin only, so the WebView makes no network call (ADR-096 item 2).
  - Input validation for every command is the Command Bridge's (schema check, unknown fields refused, SA-API-02); this program additionally refuses desktop-only commands for a server's backend.
  - No credential passes through this program: keys and tokens stay in Identity and Credentials; the desktop reads no environment variable of its own and builds no harness environment.
  - Never logged: notification text (question, agent and workspace names), folder paths chosen in the picker, preferences values, panic messages (amended January 2026 (user decision; Approved by the user January 2026)).
  - User space only (ADR-072): everything under `<data root>`; no administrator rights; no login item and no background service (ADR-011).
- **Observability:** JSON log lines (SA-OBS-01) with `timestamp`, `level`, `component` = `desktop`, and these events: `start` (`version`, `data_root_ok`), `journal_open` (`backend`, `migrated`), `wired` (`modules`, `tool_providers`, `adapters`), `window_shown` (`start_ms`), `start_failed` (`code`, `module`), `notification` (`kind`, `backend`, `shown`), `wake` (`slept_for_s`), `single_instance_focus`, `shutdown` (`reason`, `sessions_stopped`, `sessions_interrupted`, `journal_shut_down`, `duration_ms`), and one line per error code above. No metrics and no traces (SA-XC-04, SA-XC-05).
- **Security (handover links, relayed sign-in):** the link scheme is registered for the current user only; a link is never trusted: its length, scheme, host and path are checked here and every parameter by the Handover Service; a link can make ordin read a place with the person's own sign-in, never write or post (HandoverService.md). The relayed sign-in never stores or logs a code; the redirect URL must be this computer's loopback.
- **Observability (handover links, relayed sign-in):** log events `link_opened {kind: "handover"}`, `DSK_LINK_REFUSED {reason}`, `relay_sign_in {server, outcome}` — never the URL's parameters.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

The desktop builds and wires the Slack Service and opens handover links; the sections above carry the change in place, each marked where amended:

- **Shared Context:** the Slack Service implements `ToolProvider` and `EventSink`; the hub traits `SlackReactor` (this program over `OrdinHost::react_to_slack`) and `SlackRulesSource` (from the profile's `document.slack`; `server.external_actions_off` is always empty on the desktop); the Slack token source over `IdentityService::access_token_for` and `report_refused`; `RefResolver::entity` for `EntityRef::SlackLink`.
- **Package and build:** crate `ordin-slack-service`; `tauri-plugin-deep-link` 2 with the `ordin` scheme for the current user only; the WebView is granted no deep-link permission.
- **Desktop events:** `ordin://open-target` is raised for an opened handover link; `NotificationTarget::Handover.workspace_id` is `Option<WorkspaceId>` (`None` for a link to a handover not yet accepted; the screens then open the handover screen in front of the current view).
- **Commands:** `app.open_slack_link` and `identity.relay_sign_in` (Local).
- **Internal Logic:** step 1 passes a second launch's link to step 13; row 3.39a builds the Slack Service and row 3.45 passes it to the Handover Service; step 4 starts reading linked channels while ordin is open (SA-RATE-02), never while it is closed (ADR-011); step 10.3 stops it first; step 13 opens handover links; step 14 is the desktop half of a paired server's relayed Slack sign-in.
- **Error Table:** `DSK_LINK_REGISTER_FAILED`, `DSK_LINK_REFUSED`, `DSK_RELAY_URL_REFUSED`, `DSK_RELAY_TIMEOUT`.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a handover link opens the handover and follows its place
    Given the receiver's ordin does not follow the SharePoint place "Supplier onboarding handovers"
    When the receiver opens "ordin://handover/open?h=0192a3f0-…&t=sharepoint&p=https%3A%2F%2Fkestrel.sharepoint.com%2Fsites%2Fsupplier%2FHandovers%2FJanuary 2026%20Northwind%20assessment" from a Slack message
    Then handover.open_link is called with that URL
    And it returns Opened, the main window is focused and ordin://open-target carries kind "handover" with that handover id and no workspace id
    And the place is followed and the handover appears in Waiting for you within one minute

  Scenario: Edge case — ordin is not running when the link is opened
    Given ordin is closed
    When the operating system starts ordin with an ordin://handover/open link
    Then the link is held until the core has started and then opened as above

  Scenario: Edge case — a second launch carries a link
    Given ordin is running
    When a second launch passes an ordin://handover/open link
    Then the first instance opens the link and the second exits

  Scenario: Edge case — Slack is read only while ordin is open
    Given workspace "Q3 supplier review" links "#supplier-desk"
    And ordin was closed from 18:00 to 09:00 while 3 messages were posted there
    When ordin starts at 09:00
    Then the Slack Service reads those 3 messages, shown with their original times
    And no reaction or post happened between 18:00 and 09:00

  Scenario: Error path — a link of another kind is refused
    When the operating system hands ordin "ordin://settings/open?x=1"
    Then DSK_LINK_REFUSED is logged and nothing opens

  Scenario: Wiring — the Slack Service is built before the Handover Service and registered with the MCP Front
    When the core starts
    Then row 3.39a builds the Slack Service before row 3.45
    And the MCP Front holds the provider for server "slack" before it starts
```

### Gaps

None.

## Amendment January 2026 — Team knowledge (ADR-103)

> Source: decision 83 (the user's decisions D1–D10 of January 2026); HLD revision 0.51.0, ADR-103. The module: `components/TeamKnowledgeService.md`.

The desktop builds and wires the Team Knowledge Service; the sections above carry the change in place, each marked where amended:

- **Shared Context:** the hub traits `TeamMemory`, `TeamPages`, `TeamPush`, `TeamWorkspaces` and `TeamRepos`, owned by `ordin-team-knowledge` and implemented by this program as one-for-one delegations to the Memory Service, Knowledge Service, Push Service, Workspace Service, Project Service (as `GithubAccess`) and `IdentityService::person()`.
- **Package and build:** crate `ordin-team-knowledge`.
- **Internal Logic:** row 3.41a builds the module after the Ordin Host (build phase 8; it needs none of row 3.41's outputs) with `clones_dir = <data root>/team-knowledge`; its migrations are registered at 3.1; `run` is spawned at step 4 with the core's shutdown token; step 10.3 cancels it with that token.
- **Commands:** `DesktopServiceApi` (row 3.48) answers the seven `team_knowledge.*` commands over the module and `memory.keep_checked` over the Memory Service (ServiceApi.md amendment January 2026 — Team knowledge).
- **Error Table:** no new row; a construction failure of row 3.41a is `StartError::ModuleFailed` naming `ordin-team-knowledge`, as for every module row.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — the Team Knowledge Service on the desktop
    When the desktop starts
    Then row 3.41a builds the Team Knowledge Service after the Memory, Knowledge, Push, Workspace and Project Services
    And its migrations ran at step 3.1 and its run task is started at step 4
    And team_knowledge.status reaches TeamKnowledgeService::status through DesktopServiceApi

  Scenario: Edge case — quitting while a share waits for its card
    Given a team share waiting for its approval card
    When the person quits
    Then the Team Knowledge Service stops with the core's shutdown token and the share is still proposed after the next start
```

### Gaps

None.

## Amendment January 2026 — Jira issues and handovers (ADR-106)

> Source: decision 86 (the user's decisions of January 2026: Jira as a handover place and Jira issues as workspace links); HLD revision 0.54.0, ADR-106. The module: `components/JiraAdapter.md`.

The desktop builds and wires the Jira Adapter; the sections above carry the change in place, each marked where amended:

- **Package and build:** crate `ordin-jira`.
- **Internal Logic:** row 3.30a builds `JiraAdapter::new(JiraDeps { journal, identity, http })` after the Confluence Adapter (build phase 5; it needs only the Journal, Identity and the shared HTTP client of SA-HTTP-02). Rows 3.35 and 3.45 pass it as `WorkspaceDeps.jira` and `HandoverDeps.jira` (`Some`). It has no migrations, no event sink and no background task, so steps 3.1, 4 and 10 are unchanged.
- **Commands:** `DesktopServiceApi` (row 3.48) answers `jira.connect_token` and `jira.find_issues` over the Jira Adapter (ServiceApi.md amendment January 2026 — Jira issues and handovers). `jira.connect_token` reaches `JiraAdapter::connect_with_token` with the token wrapped in `SecretString` at the bridge; the program logs no part of its input. For `identity.sign_in` with `kind: jira`, the program builds `ProviderConfig::Jira { site, client_id }` from the input's `site` and the profile's `confluence.oauth_client_id`; with no such key it returns `CRED_PROVIDER_NOT_CONFIGURED` and starts no sign-in.
- **Handover links:** unchanged; a link with `t=jira` is handed whole to `handover.open_link` (step 13.2), which checks it.
- **Error Table:** no new row; a construction failure of row 3.30a is `StartError::ModuleFailed` naming `ordin-jira`, as for every module row.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — the Jira Adapter on the desktop
    When the desktop starts
    Then row 3.30a builds the Jira Adapter with the Journal, Identity and the shared HTTP client
    And WorkspaceDeps.jira and HandoverDeps.jira are Some
    And jira.find_issues reaches JiraAdapter::search through DesktopServiceApi

  Scenario: Edge case — Atlassian sign-in with no company app
    Given the organisation profile has no confluence section
    When the screens send identity.sign_in with kind jira and site "https://acme.atlassian.net"
    Then the reply is CRED_PROVIDER_NOT_CONFIGURED and no browser is opened

  Scenario: Error path — the token never reaches a log line
    When jira.connect_token is sent and the Jira Adapter returns JIRA_TOKEN_REFUSED
    Then the program's log lines for that command contain the command name and the code only
```

### Gaps

None.

## Amendment January 2026 — Ordin's language and language files (ADR-109)

> Source: decision 89 (the user's decisions of January 2026); HLD revision 0.58.0, ADR-109 (amends ADR-042 and ADR-063), 11 §4, §5.19a and UX-A77. The modules: `components/Localisation.md` and `components/OrdinHost.md` (each amendment January 2026 — Ordin's language and language files).

The desktop wires the person's languages and Ordin's knowledge of the language; the sections above carry the change in place, each marked where amended:

- **Internal Logic:** row 3.5 passes `LocalisationConfig.app_version` = the program's package version (`env!("CARGO_PKG_VERSION")`, the `version` of the `start` log line), which Localisation compares with each person language's `checked_against` to re-check its wording after an update. Row 3.41 passes the same `Arc<Localisation>` as `OrdinHostDeps.localisation`; the Ordin Host reads the active locale itself, so `set_policy` carries no locale and is called as before.
- **Events:** a task started after row 3.5 forwards each item of `Localisation::language_changes()` as `language_changed` through `Journal::append_event`, exactly as `locale_changed` is forwarded (*Program-side implementations and calls*). It ends at quit (step 10), with the other forwarding tasks.
- **Commands:** `DesktopServiceApi` (row 3.48) answers the ten new `localisation.*` commands over the Localisation methods of the same names and the three new `ordin.*` drafting commands over the Ordin Host (ServiceApi.md amendment January 2026). One program command is added, `files.save_export` (Local): the screens pass it the `file_name` and text of `localisation.export_language` or `workflows.export`, and the program opens the save dialog and writes the file (row in *Commands the program answers*). `localisation.import_language` carries the file's text, which the screens read from the file the person chooses, as for `workflows.import`. The program never logs the `ftl` input or result.
- **Error Table:** the `DSK_DIALOG_CANCELLED` row covers `files.save_export`; the `DSK_SAVE_COPY_FAILED` row covers its write failure; module errors map to `ServiceError` by step 5 as every module's do.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — Ordin Host receives Localisation
    When the desktop starts
    Then row 3.41 builds the Ordin Host with OrdinHostDeps.localisation set to the Localisation of row 3.5
    And row 3.5 passes app_version equal to the version of the "start" log line

  Scenario: Happy path — a language change reaches the screens
    Given the desktop is running
    When the person saves an edit to one fr-FR message with localisation.edit_message
    Then the Journal receives an event of kind "language_changed" with data {"locale":"fr-FR","change":"edited"}
    And the screens receive it on service_events

  Scenario: Error path — an import refused by Localisation
    When the screens send localisation.import_language with a file that is not a language file
    Then the reply is an error with code LOC_FILE_UNREADABLE and the sentence "That file isn't an ordin language file."
    And the program's log lines for that command contain the command name and the code only
  Scenario: Happy path — an exported language file is saved where the person chooses
    Given the screens hold the reply of localisation.export_language for "fr-FR" with file_name "ordin-language-fr-FR.ftl"
    When the screens send files.save_export with that file_name and text
    And the person chooses a folder in the save dialog
    Then the file is written there as UTF-8 with exactly that text
    And the reply is a SavedCopy holding its path, and no log line holds the text

  Scenario: Error path — save dialog of an export cancelled
    When the person cancels the save dialog of files.save_export
    Then the reply is an error with code DSK_DIALOG_CANCELLED and no file is written

  Scenario: Error path — an export file name with a path separator
    When the screens send files.save_export with file_name "../ordin.ftl"
    Then the reply is an error with code SVC_INPUT_INVALID and no dialog opens

  Scenario: Error path — the exported file cannot be written
    Given the chosen folder refuses writes
    When the person chooses it in the save dialog of files.save_export
    Then the reply is an error with code DSK_SAVE_COPY_FAILED and action "try_again"
```

### Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111 (amends ADR-030). The modules: `components/MemoryService.md`, `components/KnowledgeService.md`, `components/SessionSupervisor.md` and `components/OrdinHost.md` (each amendment January 2026 — Agentic memory as a core capability).

The desktop wires memory as every agent's capability; the sections above carry the change in place, each marked where amended:

- **Program-side implementations and calls (rows added):**
  - `MemoryPages` (Memory Service): `page_explaining(workspace_id, names)` → `KnowledgeService::page_for_names(workspace_id, names)`; `Ok(Some(m))` → `Some(PageLabel { page_id: m.page_id, title: m.title })`; `Ok(None)` or `Err` → `None` (an error logged `warn` `desktop.page_explaining_failed {code}`). Passed with `MemoryService::set_pages` after row 3.28 and before row 3.48 answers any command.
  - `KnowledgeMemory` (Knowledge Service): `index_page_sections` and `index_read_page_sections` pass the added `graph: PageGraph` argument to the Memory Service's methods of the same names, one for one.
  - `SupervisorHost` (Session Supervisor): `run_starting(start)` (*amended January 2026, ADR-111*): for `RunTarget::AgentInstance { agent_instance_id }`, `PackageInspector::memory_contract(agent_instance_id)` → `Some(c)` → `MemoryService::set_agent_rules(agent_instance_id, AgentMemoryRules { retains: c.retains, excludes: c.excludes, max_age_days: if c.unbounded { None } else { c.max_age_days }, store_location: c.store_location })` (the Ordin Host's mapping, OrdinHost.md SA-CLOSE-04); `None` → `MemoryService::clear_agent_rules(agent_instance_id)`; an `InspectError` or `MemoryError` → logged `warn` `desktop.memory_rules_failed {agent_instance_id, code}` and nothing else (the session then gets the memory tools only if earlier rules stand). `run_ended` does not touch memory rules (SessionSupervisor.md amendment January 2026).
  - `MemoryService::set_recall_log_days`: called after row 3.28 with Settings › History's days (default 90) and again whenever that setting changes.
- **Commands:** `DesktopServiceApi` (row 3.48) answers the thirteen new `memory.*` commands over the Memory Service methods named in its command table, with `workspace_id` (ServiceApi.md amendment January 2026); a command with `scope_id` is the Memory Service's `MEM_NOT_SUPPORTED`.
- **Error Table:** no new row; module errors map to `ServiceError` by step 5 as every module's do.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — "Explained in" is served by the Knowledge Service
    When the desktop starts
    Then MemoryService::set_pages is called once after row 3.28 with the program's MemoryPages
    And page_explaining(W, ["Borealis"]) returns the PageLabel of the page whose alias is "Borealis"

  Scenario: Wiring — an agent task gets its memory rules before it starts
    Given agent instance A whose package has a Memory part retaining ["supplier_terms"]
    When the Session Supervisor launches a task run of A
    Then set_agent_rules(A, retains ["supplier_terms"]) is called before the MCP Front opens A's session

  Scenario: Edge — a package without a Memory part clears earlier rules
    Given agent instance B with rules registered and an update that removed its Memory part
    When a task run of B is launched
    Then clear_agent_rules(B) is called and B's session is offered no memory server

  Scenario: Error path — the package cannot be read
    Given memory_contract(A) fails with an InspectError
    When a task run of A is launched
    Then "desktop.memory_rules_failed" is logged and the run starts
```

### Gaps

None.

---

## Amendment January 2026 — Knowledge agents and the Open Knowledge Format (ADR-112, ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-112 (ordin's wiki is an Open Knowledge Format bundle) and ADR-113 (knowledge agents; desktops ask, never answer). The modules: `components/KnowledgeQueryClient.md` (new), `components/MemoryService.md`, `components/KnowledgeService.md` and `components/OrganisationProfile.md` (each amendment January 2026 — Knowledge agents and the Open Knowledge Format).

The desktop builds the Knowledge Query Client and nothing of the answering side: the Knowledge Query Server and the Knowledge Reader are agent-profile modules and are never constructed here, so a person's memory is never reachable by another installation (ADR-113). The sections above are unchanged in substance; the rows below add to them.

- **Construction (row added to step 3):**

  | # | Crate (phase) | Constructed with | Wiring done right after |
  |---|---|---|---|
  | 3.13a | `ordin-knowledge-query-client` (2) | `KnowledgeQueryClient::new(KnowledgeClientDeps { journal, identity: Some(identity), http, mode: KnowledgeClientMode::Person { sign_in: CompanySignIn { issuer: <profile identity_provider.issuer>, client_id: <profile identity_provider.client_id> } } })` — the person's company sign-in through Identity and Credentials, as the Hosted Agent Adapter of row 3.10 gets its token | `knowledge_client.apply_company_front_doors(<per workspace: the profile's knowledge_agents.front_doors[] entries whose applies_to matches it>)` with the profile of row 3.2 |

- **Program-side implementations and calls (rows added):**
  - **Memory Service ← client.** After row 3.28 and before row 3.48 answers any command, `MemoryService::set_knowledge_client(<the client of row 3.13a>)` (MemoryService.md amendment January 2026; the edge Memory Service → Knowledge Query Client of Phase 2B). Until it is called, recall behaves as before this amendment.
  - **Company front doors.** On each verified profile from `OrganisationProfileService::subscribe()`, and when a workspace is created or links a code repository, the program builds, for every workspace, the `knowledge_agents.front_doors[]` entries whose `applies_to` matches it (`team_knowledge`: a Team Knowledge workspace; `code`: also a workspace that links a code repository — the rule `team_knowledge.read_folders[]` uses, ADR-110) and passes them to `KnowledgeQueryClient::apply_company_front_doors(per_workspace)`. An absent key is an empty list for every workspace: company entries leave, connections the person added stay.
  - **Events.** The client's `knowledge_agents_changed` and the Memory Service's `more_found` are written by those modules with `Journal::append_event`; the program adds no forwarding row (the Command Bridge carries every journalled event, step 4).
  - **Knowledge Service.** `KnowledgeDeps` gains `identity: Arc<dyn KnowledgeIdentity>` and `app_version` (KnowledgeService.md amendment January 2026): the program implements `KnowledgeIdentity::git_login` over Identity and Credentials (`person()`'s `github_login`, else `None`) and passes `app_version = env!("CARGO_PKG_VERSION")`, as row 3.5 does. `KnowledgeMemory::index_page_sections` already passes the page graph; the page's tier and status ride in the same call. The workspace `knowledge/index.md` is written by the Knowledge Service itself.
- **Commands:** `DesktopServiceApi` (row 3.48) answers the four `knowledge_agents.*` commands over the Knowledge Query Client's methods named in its command table, and the changed `knowledge.save_edit` and `knowledge.write` with their new fields (ServiceApi.md amendment January 2026).
- **Orderly stop (step 10):** `knowledge_client.close()` right after the Memory Service stops taking recalls: open connections to front doors are closed and late results not yet arrived are dropped; a recall in flight returns what it holds (bounded by the client's 5 s second deadline).
- **Error Table:** no new row; module errors map to `ServiceError` by step 5 as every module's do.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — the desktop asks and never answers
    When the desktop starts
    Then the Knowledge Query Client is constructed at row 3.13a
    And no Knowledge Query Server and no Knowledge Reader is constructed
    And MemoryService::set_knowledge_client is called once after row 3.28

  Scenario: Wiring — a front door named by the company
    Given the verified profile names knowledge_agents.front_doors [{"key":"supplier","display_name":"Supplier knowledge","url":"https://knowledge.example.org","applies_to":"team_knowledge"}]
    And workspace T is a Team Knowledge workspace and workspace P is not
    When the desktop starts
    Then apply_company_front_doors is called with that entry for T and with no entry for P
    When a newer verified profile names no front door
    Then apply_company_front_doors is called with an empty list

  Scenario: Edge — a desktop with no front door behaves as before
    Given no workspace has a connection and the profile names no front door
    When Ordin recalls in workspace W
    Then the Knowledge Query Client sends nothing

  Scenario: Wiring — the Knowledge Service learns the person's Git host login
    Given the person is signed in to GitHub as "jsmith"
    When the Knowledge Service asks KnowledgeIdentity::git_login
    Then it receives Some("jsmith")

  Scenario: Wiring — the knowledge_agents commands
    When the screens send knowledge_agents.list for workspace W
    Then DesktopServiceApi calls the Knowledge Query Client for W and returns its connections
```

### Assumptions

- **SA-DESK-KA-01** The client is constructed at row 3.13a (phase 2), after Identity and Credentials and the Organisation Profile, so the profile's company front doors are applied before any workspace recalls.
- **SA-DESK-KA-02** The Memory Service receives the client by a setter after its start, so its constructor and the order of rows 3.1–3.28 are unchanged.

### Gaps

None.

---

## Amendment January 2026 — Releases and updates (ADR-114)

> Source: decision 93 (the user's decisions of January 2026); HLD revision 0.62.0, ADR-114 (each build answers its own module groups; ordin updates itself from one signed release feed). The modules: `components/ServiceApi.md` (the groups a backend answers, `SVC_NOT_IN_THIS_VERSION`, `SettingsSection::About`, five `setup.*` rows), `components/SetupAssistant.md` (the new `AppUpdater`, `UpdateOffer.state`, **What's new**), `components/ScreensModule.md` (absent groups not drawn; Settings › About; the update line), `components/ArchitectureTest.md` (metadata read with `--all-features`), each amendment January 2026 — Releases and updates. Where this amendment and the sections above differ, this amendment wins.
> *Decoupled January 2026: build switches replace release-numbered features; releases are planned in the maintainers' release roadmap, kept outside the repository.*

### Public Interface — package and build (replaces the `[dependencies]` comment for the crates behind a switch)

**DSK-UP-1** Every crate of the `[dependencies]` comment not named below stays a normal dependency and is in every build. The crates below are `optional = true` and enabled by the build switch (Cargo feature) named, except `knowledge-agents`, which makes no crate optional: the always-built Memory Service depends on `ordin-knowledge-query-client` (ArchitectureTest.md, Memory Service → Knowledge Query Client), so that crate is in every build and the switch gates only row 3.13a and `MemoryService::set_knowledge_client`. This spec turns no switch on by default; the manifest of a shipped version lists the switches that version turns on, and which version turns on which switch is planned outside this spec (kept by the maintainers outside the repository).

```toml
[features]
default          = []
sandbox          = ["dep:ordin-sandbox"]                                         # Step 41
dictation        = ["dep:ordin-dictation"]                                       # Step 29
copilot          = ["dep:ordin-copilot-adapter"]                                 # Step 47
slack            = ["dep:ordin-slack-service", "dep:ordin-slack-thread-adapter"] # Steps 73, 18
mail             = ["dep:ordin-mail"]                                            # Step 54
team-knowledge   = ["dep:ordin-team-knowledge"]                                  # Step 74
authoring        = ["dep:ordin-authoring"]                                       # Step 61
handover         = ["mail", "slack", "dep:ordin-handover"]                       # Step 63
telegram         = ["dep:ordin-telegram-adapter", "dep:ordin-telegram-service"]  # Steps 66, 69
hosted-agents    = ["dep:ordin-hosted-agent-adapter"]                            # Step 17
knowledge-agents = []                                                            # row 3.13a; the crate stays a normal dependency
```

| Switch | Crates made optional under it | Rows of *Internal Logic* step 3 it adds |
|---|---|---|
| `sandbox` | `ordin-sandbox` | 3.27 |
| `dictation` | `ordin-dictation` | 3.16 |
| `copilot` | `ordin-copilot-adapter` | 3.33 |
| `slack` | `ordin-slack-service`, `ordin-slack-thread-adapter` | 3.39a, 3.11 |
| `mail` | `ordin-mail` | 3.39 |
| `team-knowledge` | `ordin-team-knowledge` | 3.41a |
| `authoring` | `ordin-authoring` | 3.44 |
| `handover` (turns on `mail` and `slack`, whose services `HandoverDeps` takes) | `ordin-handover` | 3.45 |
| `telegram` | `ordin-telegram-adapter`, `ordin-telegram-service` | 3.46, 3.47 |
| `hosted-agents` | `ordin-hosted-agent-adapter` | 3.10 and its registration in 3.34 |
| `knowledge-agents` | none: `ordin-knowledge-query-client` is always built, because the Memory Service depends on it; the switch gates construction only (row 3.13a of the amendment January 2026 — Knowledge agents, and `MemoryService::set_knowledge_client`) | 3.13a |

- **DSK-UP-2** If the C-17 spike fails (Playbook §7.8), the `sandbox` switch is not turned on; nothing in this spec changes. `ordin-sandbox`'s own `extra-guests` feature is unchanged (Playbook §5).
- **DSK-UP-3** *(withdrawn 2026-02-02 by DSK-CPX-1)* **Bundled programs.** `bundle.externalBin` lists `binaries/copilot` only in builds with `copilot`: the Makefile's bundling target passes the Tauri configuration file that matches the build's switches to the bundling command of P7 *(chosen here)*. `binaries/ordin-hook` is in every build.
- **DSK-UP-4** The code of a row behind a switch is compiled only with that switch (for `knowledge-agents`, the program's wiring of row 3.13a only; the client crate itself is always compiled) — `#[cfg(feature = "…")]` on the row, on its wiring and on the program-side implementations used only by it. A build without a switch contains no stub of the absent module: the seams below say what each present module receives instead.

### Internal Logic — step 3 by build switch

**DSK-UP-5** `Core::start` constructs, in the order of step 3, the rows that are always built (every row not in the table above) and the rows of the switches the build has. The numbering of step 3 is unchanged, and an absent row is skipped. The seams of the present modules:

| Seam | Without the switch | With it |
|---|---|---|
| Usage Planner `set_harnesses` (row 3.12, after row 3.33) | `copilot` off: called after row 3.32 with `vec![claude.harness_models()]` | `vec![claude.harness_models(), copilot.harness_models()]`, after row 3.33, as before |
| Setup Assistant (row 3.43) | `copilot` off: `bundled_programs: [ordin-hook]`; no `register_probe` (no Copilot `HarnessProbe` exists) — the Setup Assistant then skips `copilot`, as it does on the server (OrdinServer.md, traits not wired) | `bundled_programs: [ordin-hook, copilot]` and `register_probe(<Copilot HarnessProbe>)`, as before |
| `DesktopPaths::copilot_cli` | `copilot` off: not resolved, and no `DSK_SIDECAR_MISSING` is logged for `copilot` | as before |
| Session Supervisor adapters (rows 3.32–3.34) | always the Claude Local Adapter; + Copilot with `copilot`; + Slack Thread Adapter with `slack`; + Hosted Agent Adapter with `hosted-agents` | — |
| Memory Service `set_folders` (the program's `MemoryFolders` over the Team Knowledge Service, MemoryService.md amendment January 2026) | `team-knowledge` off: never called — the Memory Service then returns no folder labels and refuses a recall naming folders with `MEM_FOLDER_UNKNOWN`, as MemoryService.md states for an absent `MemoryFolders` | called after row 3.41a, as before |
| Team Knowledge Service `run(shutdown token)` (step 4) | `team-knowledge` off: not spawned | as before |
| Slack Service and `set_reactor` (row 3.39a) | `slack` off: neither exists; `SlackReactor` is not implemented; `slack.start_reading` (step 4) does not run | as before |
| `WorkspaceDeps`, `HandoverDeps`, `HomeSearchDeps` | unchanged: every crate they take is always built (Jira, Confluence, SharePoint), and `handover` turns on `mail` and `slack` | — |
| Telegram rows 3.46–3.47 | `telegram` off: neither exists; `start(shutdown token)` of the Telegram Adapter (step 4) and its `shutdown()` (step 10) are skipped | as before |
| `bind_ref_resolver` and `McpFront::start` (written at row 3.47) | run right after the last row from 3.27 to 3.47 that the build constructs, and before row 3.48 (SA-CLOSE-01 unchanged: once, before `start`) | right after row 3.47, as before |
| Handover links (step 13) | `handover` off: the `ordin` scheme is not registered and step 13 does not run (no Handover Service answers `handover.open_link`); `NeedsYouChanged` rows with `OpenTarget::Handover` cannot occur | as before |
| Notifier draft reviews (step 7.6) | `authoring` off: not run (no Authoring Service) | as before |
| Knowledge Query Client (row 3.13a) and `MemoryService::set_knowledge_client` | `knowledge-agents` off: neither; recall behaves as before the amendment January 2026 — Knowledge agents | as that amendment states |
| Orderly stop (step 10.3) | each `shutdown()` and token of an absent module is skipped; the bounds are unchanged | — |

**DSK-UP-6** **MCP providers by build switch.** The providers registered before `McpFront::start` are always the Memory Service, Craft Toolkit, Project Service, Knowledge Service and Ordin Host (5); + Sandbox with `sandbox`; + Mail Service with `mail`; + Slack Service with `slack`; + Authoring Service with `authoring`; + Telegram Service with `telegram` (10 with all five). The fixed count "nine" in the Gherkin above and in the `wired` log example predates the Slack Service (ADR-102); from this amendment the count follows the build's switches, and `wired` logs the number actually registered.

**DSK-UP-7** **Groups this build answers.** `DesktopServiceApi` (row 3.48) implements `ServiceApi::groups()` (ServiceApi.md amendment January 2026 — Releases and updates, SVC-RL-1), a list fixed at build time by the switches: every `CommandMeta.module` of a catalogue row whose *Implemented by* module is constructed in this build, and the groups of the commands this program answers itself (`service`, `settings`, `files`, `app`, `identity`). The same list, sorted, is `ServiceHello.groups` in the reply to `service.hello`. A command of a group not in the list is refused by the Service API with `SVC_NOT_IN_THIS_VERSION` before it reaches this program, so `DesktopServiceApi` has no method body for an absent module (DSK-UP-4). A command of a present group that concerns a module behind a switch the build lacks (for example `workspaces.set_telegram_grant` without `telegram`) is answered by its owner as the owner specifies; this program adds no refusal.

### Commands this program answers itself (changed)

| Command | Change |
|---|---|
| `service.hello` | **DSK-UP-8** adds `groups` (DSK-UP-7) |
| `settings.get` | **DSK-UP-9** `{ section: about }` → `SettingsView { section: About, settings: [] }`: the section holds no setting of its own; the screens draw it from `service.hello`, `setup.update_offer`, `setup.check_update` and `setup.whats_new` (ScreensModule.md amendment January 2026). `settings.change` names no key of `About`, so an `about.*` key is refused as any unknown key is |
| `setup.install_update` | **DSK-UP-10** calls `SetupAssistant::install_update` (SetupAssistant.md SET-UP-5): with an offer in state `Ready` it returns `Ok(())` and the program answers `{}`, then runs step 15 below; otherwise the Setup Assistant's `SETUP_UPDATE_NONE`. Step 15 runs the quit flow of step 10, which asks first when local sessions run (step 10.2) |

### Internal Logic — updates (step 8 replaced; steps 10 and 15)

8. **Updates.** **DSK-UP-11** `TauriUpdater` implements the Setup Assistant's `AppUpdater` as SetupAssistant.md's amendment January 2026 defines it, and is given with `register_updater` (row 3.43) as before. It keeps at most one downloaded update, in memory, behind a `tokio::sync::Mutex`:
    1. `check(feed)`: `app.updater_builder().endpoints(vec![feed_url])?.build()?.check()`, as before; a found update is kept as the *pending* update and mapped to `UpdateOffer { version, notes, state: Downloading }`.
    2. `download(&offer)`: the pending update's download (Tauri's updater plug-in), which checks the signature against the build's public key (2D-2b) before it returns; the verified bytes are kept with the pending update. An unreachable feed or file → `DSK_UPDATE_CHECK_FAILED` (`UpdaterError::Unreachable`); a refused signature → `DSK_UPDATE_INSTALL_FAILED` (`UpdaterError::SignatureInvalid`); both are logged and returned to the Setup Assistant, which clears the offer (SetupAssistant.md). An offer for a different version replaces the kept bytes.
    3. `install_and_restart()`: install the kept bytes with the plug-in's install, then `app.restart()`. Called only by step 15, after the core has stopped.
    4. `install_on_exit()`: install the kept bytes with the plug-in's install and return; the caller then exits. Called only by step 10.3 below.
    5. No kept bytes when 3 or 4 is called → `UpdaterError::NothingDownloaded` (SetupAssistant.md), which the Setup Assistant maps to `SETUP_UPDATE_NONE`; an install that fails → `UpdaterError::InstallFailed`, logged as `DSK_UPDATE_INSTALL_FAILED`.
    6. 3 and 4 are called only through `SetupAssistant::finish_update` (SET-UP-6); the program never calls them itself.
    7. **Bounds** *(chosen here: 512 MiB as the server's archive, and 30 minutes; Configuration.md 2D-4 row *Desktop update download*)*: a download whose announced or received size passes 512 MiB is stopped and refused, and a download not finished 30 minutes after it started is abandoned; both drop the partial bytes, are logged as `DSK_UPDATE_CHECK_FAILED` with `reason` `too_large` or `timed_out`, and return `UpdaterError::Unreachable`, so the Setup Assistant clears the offer and tries again at its next check. No command and no screen waits for a download: `setup.check_update` returns as soon as the offer exists, in state `Downloading`.

10. **Quit (step 10.3 extended).** **DSK-UP-12** On every quit other than step 15 and other than `QuitReason::OperatingSystemEnding`, after `datastore.close()` and before `app.exit(0)`, call `setup_assistant.finish_update(UpdateFinish::OnExit)`, bounded at 120 s *(chosen here: the installer's own run)*. It uses no store, so it is valid after `setup_assistant.close()` and after the datastore is closed (SetupAssistant.md SET-UP-6, SET-UP-7). `Ok(false)` (no update `Ready`, including one still downloading) installs nothing; `Ok(true)` installed the update on the way out; an error or overrun is logged with `DSK_UPDATE_INSTALL_FAILED` and the program exits as before. The person's next start is the new version, or the current one when the install failed (the Setup Assistant offers it again at its next check). When the operating system is ending the session (step 10.5), nothing is installed, because the system may end the installer part-way; the downloaded update is not kept across the restart, and the Setup Assistant's check 60 s after the next start offers it again. The quit bound of the smoke test (Playbook §7.3: quitting with nothing running ends within 3 s) holds whenever no update is `Ready`; with an update `Ready`, quitting takes the installer's time as well, at most the 120 s above.

15. **Update restart.** **DSK-UP-13** After `setup.install_update` succeeds with an offer `Ready` (DSK-UP-10):
    1. Run step 10 with the trigger *update*. When `session_supervisor.running_local_sessions() > 0`, step 10.2 emits `ordin://quit-requested { running_sessions }` and stops, and the program keeps *update restart requested* until the person answers the quit question *(chosen here)*; the screens' existing quit confirmation answers with `app.quit`, and that `app.quit` continues here instead of ending as a plain quit. A quit the person declines leaves the offer `Ready` and clears the flag: the flag is cleared when the screens answer `ordin://quit-requested` with anything other than `app.quit`, and when the main window next gains focus without an `app.quit` having arrived, so a later plain quit is never turned into an update restart *(chosen here)*.
    2. Run `Core::shutdown(QuitReason::Update)` *(variant added here)*: the steps and bounds of step 10.3.
    3. Call `setup_assistant.finish_update(UpdateFinish::Restart)` (SetupAssistant.md SET-UP-6, which calls `install_and_restart()`), bounded at 120 s. On macOS the plug-in replaces the bundle under `~/Applications` and `app.restart()` starts the new version. On Windows the plug-in runs the per-user installer, which ends this process and starts the new version *(assumed here; confirmed by C-46)*.
    4. On `Ok(false)`, an error or an overrun: log `DSK_UPDATE_INSTALL_FAILED` (for an error or overrun) and call `app.restart()`, so the person is never left without ordin; the current version starts, and the Setup Assistant offers the update again at its next check.
    5. `QuitReason::Update` is logged on the `shutdown` line as `reason = "update"`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitReason { PersonQuit, OperatingSystemEnding, Update }   // Update added (ADR-114)
```

16. **Older version started.** **DSK-UP-14** At start, after row 3.43, the program compares its own version (`CARGO_PKG_VERSION`) with the last version the Setup Assistant recorded as started (SetupAssistant.md, the what's-new record). When the running version is lower by semantic-version order, it logs `update_downgrade_seen` (`warn`, with `running` and `last_started`) *(chosen here)*, and **What's new** is not shown for the running version: `setup.whats_new` returns `None` until a version higher than the last one started runs. Nothing else changes; the journal's schema is never older than the program, because migrations only go forward and a lower version that finds a newer schema stops with the Journal's own refusal. The update signature does not bind a version number, so this line is the record that an older signed build was installed (R-124).

### Error Table (rows changed or added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A command of a group this build does not answer | `Err(permanent)`, raised by the Service API | `SVC_NOT_IN_THIS_VERSION` | `{"kind":"error","error":{"code":"SVC_NOT_IN_THIS_VERSION","message":"This needs a newer version of ordin."}}` |
| Download of an update, or the feed, unreachable | `Err(retryable)` to the Setup Assistant; no pop-up | `DSK_UPDATE_CHECK_FAILED` | — (the Setup Assistant owns the words) |
| An update's signature is refused, or installing it fails at quit or at the update restart | `Err(retryable)` to the Setup Assistant; logged; ordin restarts or exits on its current version | `DSK_UPDATE_INSTALL_FAILED` | — |

| A download passes 512 MiB or 30 minutes | `Err(retryable)` to the Setup Assistant; partial bytes dropped; retried at the next check | `DSK_UPDATE_CHECK_FAILED` (`reason` `too_large` or `timed_out`) | — |
| The program starts on a version lower than the last one started | logged `warn` `update_downgrade_seen`; start continues; no What's new | — | — |

No new `DSK_` code is added.

### Acceptance Criteria (Gherkin) — added scenarios

The scenario *Happy path — start wires every module and shows the window* above holds for a build with every switch on, except that its count of tool providers is ten (DSK-UP-6). The scenarios below state it for builds with fewer switches.

```gherkin
  Scenario: Happy path — a build with no switch wires its modules and shows the window
    Given a build with no build switch, a writable data root and a journal at the current schema
    When the person opens ordin
    Then the rows of Internal Logic step 3 that are always built are constructed in order
    And rows 3.10, 3.11, 3.13a, 3.16, 3.27, 3.33, 3.39, 3.39a, 3.41a, 3.44, 3.45, 3.46 and 3.47 are not constructed
    And five tool providers are registered with the MCP Front before McpFront.start
    And one runtime adapter, the Claude Local Adapter, is registered with the Session Supervisor
    And usage.set_harnesses is called with the Claude Local Adapter's harness models only
    And bind_ref_resolver is called once, after row 3.43 and before McpFront.start
    And service.hello returns groups that include "ordin" and "setup" and do not include "mail", "telegram" or "dictation"
    And "window_shown" is logged within 2000 ms of process start

  Scenario: Happy path — a build with the team switches adds the team modules
    Given a build with the switches sandbox, dictation, copilot, team-knowledge, authoring and handover
    When the person opens ordin
    Then rows 3.11, 3.16, 3.27, 3.33, 3.39, 3.39a, 3.41a, 3.44 and 3.45 are constructed
    And nine tool providers are registered before McpFront.start
    And three runtime adapters are registered with the Session Supervisor
    And MemoryService.set_folders is called once after row 3.41a
    And rows 3.10, 3.13a, 3.46 and 3.47 are not constructed

  Scenario: Error path — a command of a group the build lacks
    Given a build without the switch mail
    When the screens send mail.list_drafts
    Then the reply is an error with code SVC_NOT_IN_THIS_VERSION
    And no module is called

  Scenario: Edge case — Copilot is not offered without its switch
    Given a build without the switch copilot
    When the Setup Assistant runs its start checks
    Then no Copilot harness probe is registered and no DSK_SIDECAR_MISSING is logged for copilot

  Scenario: Happy path — restart to update
    Given the Setup Assistant holds an offer for "0.3.0" in state ready and no local session runs
    When the screens send setup.install_update
    Then Core::shutdown runs with reason update
    And setup_assistant.finish_update is called with Restart, the updater installs the kept bytes and the application restarts
    And the shutdown line is logged with reason "update"

  Scenario: Happy path — an update installs on the way out
    Given the Setup Assistant holds an offer for "0.3.0" in state ready
    When the person quits ordin
    Then the journal is shut down and the datastore closed
    And then setup_assistant.finish_update is called with OnExit, updater.install_on_exit is called once and the program exits with status 0

  Scenario: Edge case — quitting while the update still downloads
    Given the Setup Assistant holds an offer for "0.3.0" in state downloading
    When the person quits ordin
    Then finish_update with OnExit returns false, install_on_exit is not called and the program exits with status 0

  Scenario: Edge case — restart to update while sessions run
    Given the Setup Assistant holds an offer for "0.3.0" in state ready and two local sessions run
    When the screens send setup.install_update
    Then the reply is {} and ordin://quit-requested is emitted with running_sessions 2
    And nothing is shut down
    When the screens answer with app.quit
    Then Core::shutdown runs with reason update and finish_update is called with Restart

  Scenario: Error path — the install fails at the update restart
    Given the Setup Assistant holds an offer for "0.3.0" in state ready
    And the updater's install fails
    When the screens send setup.install_update
    Then DSK_UPDATE_INSTALL_FAILED is logged
    And the application restarts on its current version

  Scenario: Edge case — the operating system ends the session with an update ready
    Given the Setup Assistant holds an offer for "0.3.0" in state ready
    When the operating system ends the session
    Then Core::shutdown runs with reason operating_system_ending
    And finish_update is not called and nothing is installed

  Scenario: Edge case — a declined update restart does not turn a later quit into an update
    Given the Setup Assistant holds an offer for "0.3.0" in state ready and one local session runs
    And the screens sent setup.install_update and ordin://quit-requested was emitted
    When the screens do not answer with app.quit and the main window next gains focus
    And the person later quits ordin from the application menu with no session running
    Then Core::shutdown runs with reason person_quit
    And finish_update is called with OnExit, not Restart

  Scenario: Error path — a download that passes its size bound is refused
    Given the Setup Assistant holds an offer for "0.3.0" in state downloading
    When the download passes 512 MiB
    Then the partial bytes are dropped and DSK_UPDATE_CHECK_FAILED is logged with reason too_large
    And the Setup Assistant clears the offer

  Scenario: Edge case — an older version is started
    Given the Setup Assistant recorded "0.4.0" as the last version started
    When ordin "0.3.0" starts
    Then update_downgrade_seen is logged with running "0.3.0" and last_started "0.4.0"
    And setup.whats_new returns no notes

  Scenario: Happy path — Settings › About has no setting of its own
    When the screens send settings.get with section "about"
    Then the reply is a SettingsView with section "about" and no settings
```

### Performance, Security, Observability (additions)

- **Performance:** a build with no switch constructs every row of step 3 except the thirteen behind switches (DSK-UP-5); a download is bounded at 512 MiB and 30 minutes (DSK-UP-11.7); quitting with no update ready keeps the 3 s bound, and with one ready adds at most 120 s and starts within the same 2,000 ms to `window_shown`; the download of an update runs on the Tokio runtime outside every interactive path.
- **Security:** a build contains no code of a module it does not ship (DSK-UP-4), so the surface of a module behind a switch the build lacks cannot be reached (the Knowledge Query Client's code is present in every build but is never constructed without `knowledge-agents`, so it opens no connection); an update is installed only after Tauri's updater has checked its signature against the build's public key (2D-2b); the WebView still has no `updater` permission (capability `main` unchanged), so only the core installs.
- **Observability:** `start` gains `features` (the build switches of the build, for example `["copilot","dictation","sandbox"]`); `wired` logs the counts actually constructed; `update_install_on_exit` and `update_restart` (each with `version` and `outcome`) are logged *(chosen here)*.

### Assumptions

- **SA-DESK-UP-01** Tauri's updater plug-in keeps the download and the install as two steps, so a verified download can wait for the person's restart or quit (C-46 confirms the per-user install and update on both platforms).
- **SA-DESK-UP-02** On Windows the per-user installer the plug-in runs starts the new version after installing; if C-46 shows it does not, step 15.2 starts the installed executable itself after the installer returns.

### Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (knowledge inherits its readers; a person's own ordin links their accounts at each front door from the sign-ins it already holds, with no step for the person). The modules: `components/KnowledgeQueryClient.md` and `components/IdentityAndCredentials.md` (amendments January 2026).

The desktop still asks and never answers; it constructs no Source Access. Row 3.13a is unchanged: the client is given Identity and Credentials and links accounts itself, watching `IdentityService::subscribe()` for sign-in changes (KnowledgeQueryClient.md amendment January 2026, PD-A). One wiring addition:

- **Microsoft scopes for a sign-in.** When the program builds the scope set for `identity.sign_in` with kind `microsoft` (the scopes the SharePoint Adapter needs, as before), it adds every scope `KnowledgeQueryClient::microsoft_check_scopes()` returns — the `about.sharepoint_check_scope` of each live connection that has one. The person consents on Microsoft's own page in the same sign-in; no screen passes a scope (SA-DSK-PD-01). Until the person signs in again, `access_token_for_scope` returns `CRED_SCOPE_MISSING` and that front door withholds restricted SharePoint facts (IdentityAndCredentials.md SA-ID-PD-04).

A scope longer than 512 characters or not starting `api://` is left out and logged as `knowledge_scope_ignored {length}` (WARN), never the scope itself.

- **Confluence `read:me`.** When it builds the Confluence scope set for `identity.sign_in`, the program adds `read:me` only while a live connection's `about.accounts` lists Confluence (IdentityAndCredentials.md SA-ID-PD-02); the Confluence Adapter's scopes are unchanged.

**SA-DSK-PD-01** *(chosen here)* The program owns the Microsoft and Confluence scope unions, so a screen or an agent cannot ask Identity for an arbitrary resource; the only resources added are front doors the person connected or the company named.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Desktop wiring for per-document read access (ADR-115)

  Scenario: Happy path — the Microsoft sign-in carries the front door's check scope
    Given a live connection whose about has sharepoint_check_scope "api://7c1e-app/knowledge.check"
    When the person starts a Microsoft sign-in
    Then the requested scopes are the SharePoint Adapter's scopes plus "api://7c1e-app/knowledge.check"

  Scenario: Edge — no front door checks SharePoint
    Given no live connection has a sharepoint_check_scope
    When the person starts a Microsoft sign-in
    Then the requested scopes are the SharePoint Adapter's scopes only

  Scenario: Error — a check scope longer than 512 characters
    Given microsoft_check_scopes returns a scope of 600 characters
    When the person starts a Microsoft sign-in
    Then that scope is left out of the requested scopes
    And knowledge_scope_ignored is logged with its length only

  Scenario: Edge — the knowledge-agents switch is off
    Given a build without the knowledge-agents switch
    When the person starts a Microsoft sign-in
    Then no KnowledgeQueryClient call is made and the scopes are the SharePoint Adapter's only
```


## Amendment January 2026 — The install choice (ADR-072)

> Source: decision 95 (the user's decisions of January 2026); HLD revision 0.64.0, ADR-072 as amended; `components/SetupAssistant.md` amendment January 2026. Every section above stays in force except where this amendment states otherwise.

### Internal Logic (changed)

8. **Updates.** **DSK-UP-15 Install kind.** `TauriUpdater` works out `InstallKind` once, before `register_updater` (row 3.43), and keeps it. The install folder is the folder holding the running executable on Windows, and the folder holding the `.app` bundle on macOS. The program creates and removes one uniquely named empty file in that folder: success → `PerUser`; failure → `WholeComputer`. The answer depends only on that write, so an administrator's own account, which can write `/Applications`, reads as `PerUser` and updates itself. Log `install_kind` (`kind`) at `info` *(chosen here)*.

   **DSK-UP-16 Other install.** `TauriUpdater::other_install` looks in the other kind's folder for this build's product name: when this copy is `PerUser`, `/Applications` on macOS and `%ProgramFiles%` on Windows; when it is `WholeComputer`, `~/Applications` on macOS and `%LOCALAPPDATA%\Programs` on Windows. A copy found at a path other than the running one → `Some(path)`; otherwise `None`. Neither copy is changed.

   **DSK-UP-17 Both at once.** The two copies share one bundle identifier, so they share the account's `app_local_data_dir`, and the single-instance plug-in lets one run at a time. Each installer writes its own shortcut, and the person starts one of them. A copy older than the data root's schema stops with the Journal's own refusal (`JRNL_SCHEMA_NEWER`), which is why the Setup Assistant names the other copy.

### Installer settings

- **DSK-UP-18** The Windows installer is NSIS, asking **Only for me (no administrator needed)**, the default, or **Everyone on this computer (needs an administrator)**. No MSI is shipped. The macOS disk image's folder shortcut points at `~/Applications`; a copy dragged to `/Applications` also works.
- **DSK-UP-19** A whole-computer install writes machine-wide registry keys only for the install and uninstall entries. Uninstalling either kind removes the program and no person's data.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a folder the program can write is a per-user install
    Given the program's install folder can be written
    When the program starts
    Then the install kind is PerUser and the folder holds no new file

  Scenario: Edge — a folder the program cannot write is a whole-computer install
    Given the program's install folder cannot be written
    When the program starts
    Then the install kind is WholeComputer

  Scenario: Edge — the other install is found and left alone
    Given a per-user copy is running and a copy exists in the whole-computer folder
    When other_install is called
    Then it returns that copy's path and the copy is unchanged
```

### Assumptions

- **SA-DESK-UP-03** Tauri's bundler can produce an NSIS installer that asks between per-user and whole-computer install with per-user as the default, and a macOS disk image whose folder shortcut points at `~/Applications` (C-46). If the disk image cannot, the install guide tells the person where to drag the app.

### Gaps

None.

## Amendment January 2026 — Write attribution at start and the paused run of a session (FU-087, FU-126)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations").
>
> Source: the follow-ups FU-087 (program side: refuse to start when `HookRelay::attributes_writes()` is false in a build with places; fill `SessionSetup.paused` from the run's row) and FU-126 (how the program fills `SessionSetup.paused`), and the two `SPEC-GAP` markers of `apps/desktop/src/core.rs`. Evidence read for this amendment: `HookRelay.md` SA-RECON-05; `crates/ordin-hook-relay/src/writes.rs` (`AgentWrites::attributes_writes`, `NoPlaces`) and `crates/ordin-hook-relay/src/relay.rs` (`HookRelay::new` takes `writes: Arc<dyn AgentWrites>`; `HookRelay::attributes_writes`); `FileService.md` SA-RECON-14; `apps/desktop/src/error.rs` (`StartError`, `ALL_CODES`); `ClaudeLocalAdapter.md` SA-RECON-08 and `SessionSetup.paused`, `PausedRun`; `SessionSupervisor.md` H.3, H.4 and the `runs` table; `crates/ordin-types/src/domain/runs.rs` (`Run`, which carries no pause approval id); `crates/ordin-run-queue/src/types.rs` (`RunOutcome.pending`, `PendingItem.approval_id`); rows 3.19, 3.22, 3.23 and 3.32 of *Internal Logic* step 3 and the list of traits this program does not wire. Every section above stays in force except where this amendment states otherwise.

### Internal Logic (additions)

3. **Construct modules — row 3.23.**

   **DSK-WIRE-1 The Hook Relay's writes.** Row 3.23 passes, as `HookRelay::new`'s `writes` argument, the program's `AgentWrites` adapter over the File Service of row 3.22, a delegation without decisions (SA-RECON-09; FileService.md SA-RECON-14: "the program joins the two"). The desktop never passes `NoPlaces`. Every desktop build has places: `ordin-files` (row 3.22) is behind no build switch (DSK-UP-1), so DSK-WIRE-2 applies to every build.

   **DSK-WIRE-2 Refuse to start without attribution.** Right after row 3.23's construction, before its wiring and before row 3.24, the program calls `hook_relay.attributes_writes()`. `false` → start stops with `StartError::ModuleFailed { module: "ordin-hook-relay", code: "DSK_WRITES_NOT_ATTRIBUTED" }`; nothing later is constructed, and step 9 runs: the `start_failed` log line carries code `DSK_START_FAILED` and module `ordin-hook-relay` with the code `DSK_WRITES_NOT_ATTRIBUTED`, and the person sees step 9's message box, `desktop-start-failed` with `desktop-start-failed-reference`. No message of its own *(chosen here)*: the cause is a build wired with `NoPlaces`, which the person cannot repair, and the reference lets support find the log line. `true` → row 3.23's wiring runs as before. Reason (HookRelay.md SA-RECON-05): a relay with `NoPlaces` records no claim, so every file an agent writes through its harness would count as the person's own.

**DSK-WIRE-3 `SessionSetup.paused` on the desktop.** The program's `SessionSetupSource` (rows 3.32, 3.33) sets `SessionSetup.paused` to `None` for every run. It reads no run row and calls no Session Supervisor getter for it. Reasons, each read in the specs named:

- `SessionSetup.paused` is an agent-profile field, `None` for any other run (ClaudeLocalAdapter.md, *Public Interface*), and adoption by a second instance happens only in the agent profile (SA-RECON-08).
- The desktop runs one instance (step 1, the single-instance plug-in), builds the Session Supervisor with no Run Queue and `program: Desktop` (row 3.19), and passes the Claude Local Adapter no `SessionRecordStore` (the list of traits this program does not wire). A paused desktop run is resumed by the instance that paused it, through the Supervisor's F path, and never adopted.

So FU-126 needs no new Session Supervisor getter and no Run Queue record on the desktop; `SessionSupervisor.md` is unchanged. How the agent-profile program fills the field is OQ-WIRE-1.

### Error Table (row added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The Hook Relay was built with an `AgentWrites` that records nothing (`attributes_writes()` false), checked right after row 3.23 (DSK-WIRE-2) | `Err(permanent)` — start stops, message box, exit 1 | `DSK_START_FAILED` (module `ordin-hook-relay`, code `DSK_WRITES_NOT_ATTRIBUTED`) | as for any module failure |

`DSK_WRITES_NOT_ATTRIBUTED` is *chosen here*, in this program's prefix (*Approach*), because the program raises it, not the Hook Relay. It is added to `ALL_CODES` in `apps/desktop/src/error.rs` and to the registry (ErrorHandling.md, amended 2026-01-10).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Error path — a build whose Hook Relay attributes no writes does not start (DSK-WIRE-2)
    Given the Hook Relay of row 3.23 is built with NoPlaces
    When the person opens ordin
    Then no module after the Hook Relay is constructed
    And the log holds "start_failed" with code DSK_START_FAILED, module "ordin-hook-relay" and code DSK_WRITES_NOT_ATTRIBUTED
    And a message box says "ordin couldn't start on this computer. Open it again; if this happens again, reinstall ordin."
    And the program exits with status 1

  Scenario: Happy path — the Hook Relay over the File Service lets start continue (DSK-WIRE-1, DSK-WIRE-2)
    Given the Hook Relay of row 3.23 is built with the program's AgentWrites over the File Service
    When the person opens ordin
    Then attributes_writes is true
    And the Crew Orchestrator of row 3.24 is constructed

  Scenario: Edge case — a paused desktop run is given no paused run in its session setup (DSK-WIRE-3)
    Given a run paused on approval "A1" on the desktop
    When the Claude Local Adapter asks the program for the run's session setup
    Then SessionSetup.paused is None
```

### Open Questions

- **OQ-WIRE-1** *Where the agent-profile program reads `SessionSetup.paused`.* FU-087's exit criterion "a paused run is adopted on a second instance" is agent-profile behaviour, which the desktop program cannot show (DSK-WIRE-3); it belongs to the ordin server program in the agent profile (OrdinServer.md, `SessionSetupSource`). `PausedRun` needs `approval_id` and `caller`. Options: (a) a new Session Supervisor getter over the `runs` row, whose `pause_approval_id` the Supervisor stores at a pause (SessionSupervisor.md C, H.3); (b) the Run Queue record, whose `RunOutcome.pending` items carry `approval_id` (`crates/ordin-run-queue/src/types.rs`) and which the program already reads for `run_starter`. Neither source is shown to hold the run's `ToolCaller`: the public `Run` and the Run Queue's `RunRecord` carry none. **Recommendation:** (a), because the Supervisor already owns the pause and its approval id and builds the `StartRun` that holds the caller; the getter's signature needs the caller's source settled first, in `SessionSupervisor.md`. Blocks only FU-087's adoption exit criterion; nothing on the desktop waits for it.

### Gaps

None for the desktop. OQ-WIRE-1 concerns the ordin server program.

## Amendment January 2026 — Phase 11 reconciliation (SPEC-GAP-3, 5, 7–24)

> **Status:** Approved (user, 2026-01-10), except OQ-REC-1 to OQ-REC-5, which stay open (OQ-REC-5 recommendation: the Package Catalogue adds the two instances on first use; their bundled ids still need naming).
>
> Source: the `// SPEC-GAP-<N>:` markers that Phase 11 Step 72 left in `apps/desktop/src` (`hosts.rs`, `hosts_run.rs`, `hosts_home.rs`, `hosts_knowledge.rs`, `wiring.rs`, `wiring_tests.rs`, `service_api.rs`), N = 3, 5 and 7 to 24. SPEC-GAP-4 and SPEC-GAP-6 are outside this amendment. Evidence read: `crates/ordin-rules/src/types.rs` (`Proposal`, `ControlLine`, `ControlSource`, `CardWording`, `RulesHost::word`, `RulesHost::plain_line`); `crates/ordin-approval/src/validate.rs` (every card field must be non-empty: `affects` 1–50 items, `changes` 1–1,000, `proposal.consequence` at least 1 character); `crates/ordin-localisation/catalogues/en-GB/approval.ftl` and `Localisation::message`, `message_in`, `format_list`, `bundled_locales`; `crates/ordin-organisation-profile/src/service.rs` (`remote_agents`, `remote_agent` asynchronous; `remote_agent_identity` synchronous over an in-memory id-to-name map); `RemoteAgent.route: RemoteRoute { Http, Slack }`, `RemoteAgentStatus { Listed, Enabled, Verified, Withdrawn }`; `crates/ordin-session-supervisor/src/sql/sqlite_v1.sql` (`runs.workspace_id NOT NULL`, the `tasks` table with `attached_artefacts`); `RunView { run, startup }`; `crates/ordin-agent-folders` (`lock(id, LockMode::Session) -> FolderLease`, released on drop); `SupervisorHost::run_ended`; `crates/ordin-knowledge/src/ports.rs` (`KnowledgeMemory`) and `crates/ordin-knowledge/src/sql/sqlite_v5.sql` (`knowledge_read_pages_removed` carries `workspace_id` and `folder_id`); `crates/ordin-setup/src/assistant.rs` (`LANGUAGE_SERVERS_JSON`, private); `Journal::artefact_versions`, `list_artefacts` (no read by `ArtefactId`); `MemoryService::get_facts`; `CatalogueService::repository_ref`; `OrdinHost.md` tool table and `OrdinHostConfig`; `McpFront.md` SA-RECON-04 and step 3 of `register`. Every section above stays in force except where this amendment states otherwise. Owner changes this amendment needs are drafted in the owner specs, each marked with the same status: `OrdinHost.md`, `SessionSupervisor.md`, `KnowledgeService.md`, `JournalAndArtefactStore.md`, `SetupAssistant.md`, `ClaudeLocalAdapter.md`.

### Resolution per marker

| Marker | Outcome | Assertion |
|---|---|---|
| SPEC-GAP-3 | Resolved: catalogue messages for card words and rules lines | DSK-REC-1 to DSK-REC-4 |
| SPEC-GAP-5 | Open question | OQ-REC-1 |
| SPEC-GAP-7 | Resolved: `desktop-backend-name` added to the catalogue list | DSK-REC-5 |
| SPEC-GAP-8 | Open question (with SPEC-GAP-5) | OQ-REC-1 |
| SPEC-GAP-9 | Resolved as an accepted limit of this build | DSK-REC-6 |
| SPEC-GAP-10 | (a) resolved by an owner change; (b) and (c) resolved; (d) open | DSK-REC-7 to DSK-REC-9, OQ-REC-2 |
| SPEC-GAP-11 | Resolved over the run register of DSK-REC-8 | DSK-REC-10 |
| SPEC-GAP-12 | Resolved by an owner change (Setup Assistant) | DSK-REC-11 |
| SPEC-GAP-13 | Resolved by an owner change (Knowledge Service) | DSK-REC-12 |
| SPEC-GAP-14 | Models resolved; the place-file `origin_ref` open | DSK-REC-13, OQ-REC-3 |
| SPEC-GAP-15 | Open question | OQ-REC-4 |
| SPEC-GAP-16 | Resolved: the verbs of DSK-REC-2 and DSK-REC-3 | DSK-REC-14 |
| SPEC-GAP-17 | Resolved | DSK-REC-15 |
| SPEC-GAP-18 | Resolved by an owner change (Journal) | DSK-REC-16 |
| SPEC-GAP-19 | Resolved for `Run`, `MemoryFact`, `AgentRepository`; `CrewCharter` stays a gap | DSK-REC-17, G-REC-1 |
| SPEC-GAP-20 | Resolved | DSK-REC-18 |
| SPEC-GAP-21 | Resolved by an owner change (Session Supervisor) | DSK-REC-19 |
| SPEC-GAP-22 | Resolved | DSK-REC-20 |
| SPEC-GAP-23 | Version and maxima resolved; the two instance ids open | DSK-REC-21, OQ-REC-5 |
| SPEC-GAP-24 | Resolved by an owner change (Ordin Host) | DSK-REC-22 |

### Internal Logic (additions)

**DSK-REC-1 Card words for a Rules Engine proposal (`RulesHost::word`, SPEC-GAP-3).** The program's `RulesHost::word(run, proposal)` builds every text of the `CardWording` from the catalogue messages of DSK-REC-4 in the active locale (`Localisation::message`), never from a model. A proposal field passed as an argument (`$tool`, `$server`, `$destination`, `$rule`, `$verb`, `$runs_in`) is the field's own text, trimmed and cut to 60 characters with "…" as the 60th, so `question` stays within 120 characters and ends in "?"; an empty field is replaced by the text of `approval-rules-not-stated`. `where_line` is cut to 200 characters. The fields are, per variant:

| `Proposal` | `question` | `verb` | `where_line` | `affects` | `changes` | `content_label` |
|---|---|---|---|---|---|---|
| `ShellCommand { command, runs_in }` | `approval-rules-shell-question` | `approval-rules-verb-run` | `runs_in` | `[Record { label: runs_in }]` | `approval-rules-shell-changes` | `Some("See the command")` |
| `ExternalTool { tool, kind, destination }` | `approval-rules-external-question` | `approval-rules-verb-allow` | `destination` | `[Record { label: destination }]` | `approval-rules-external-changes` | `None` |
| `PackageTool { server, tool }` | `approval-rules-package-question` | `approval-rules-verb-allow` | `server` | `[Record { label: tool }]` | `approval-rules-package-changes` | `None` |
| `ApprovalListTool { tool }` | `approval-rules-listed-question` | `approval-rules-verb-allow` | `approval-rules-where-this-computer` | `[Record { label: tool }]` | `approval-rules-listed-changes` | `None` |
| `ArgumentRulePause { rule, tool }` | `approval-rules-argument-question` | `approval-rules-verb-allow` | `approval-rules-where-rule` | `[Record { label: tool }]` | `approval-rules-argument-changes` | `None` |
| `AgentPolicyApproval { verb, tool }` | `approval-rules-policy-question` | `approval-rules-verb-allow` | `approval-rules-where-this-computer` | `[Record { label: tool }]` | `approval-rules-policy-changes` | `None` |
| `EndGatesSetAside { gates }` | `approval-rules-gates-question` | `approval-rules-verb-finish` | `approval-rules-where-this-run` | one `Record { label: gate }` per gate, the first 50 | `approval-rules-gates-changes`, `$gates` = `Localisation::format_list(gates)`, cut to 1,000 characters | `None` |

`ExternalTool.kind` is not shown. `CardWording.proposal` is `ActionProposal { action: <question without its final "?">, destination: <where_line>, affects: <affects>, consequence: <changes> }`. Every field is non-empty, so the Approval Service's checks (`APPR_QUESTION_INVALID`, `APPR_VERB_INVALID`, `APPR_WHERE_INVALID`, `APPR_AFFECTS_INVALID`, `APPR_CHANGES_INVALID`, `APPR_ACTION_INVALID`, `APPR_DESTINATION_INVALID`, `APPR_CONSEQUENCE_INVALID`) pass for every proposal the Rules Engine raises. The words the code wrote while the gap was open (empty `affects`, `changes` and `consequence`) fail those checks, and the Rules Engine then denies with `RULE_APPROVAL_UNAVAILABLE`; this assertion replaces them.

**DSK-REC-2 Card words for a crew step (`CrewHost::word_step_approval`, SPEC-GAP-3).** `word_step_approval(view, step_label, member, line)` returns `question` = `approval-crew-step-question` with `$step` = `step_label` cut to 60 characters; `verb` = `approval-crew-verb-carry-on`; `where_line` = `approval-crew-where` with `$purpose` = `view.purpose` cut to 190 characters; `affects` = `[Record { label: step_label }]`; `changes` = `line` cut to 1,000 characters, or `approval-crew-step-changes` when `line` is empty; `content_label: None`; `proposal` built as DSK-REC-1 says. `line` is the workflow's own text, shown as data and not translated. `member` is not used.

**DSK-REC-3 Rules lines (`RulesHost::plain_line`, SPEC-GAP-3).** `plain_line(line)` returns, by `line.source`: `Package` → `rules-line-package`; `Configuration` → `rules-line-configuration`; `Application` → `rules-line-application`; `AgentPolicy` → `rules-line-agent-policy`; with `$rule` = `line.detail` when it is non-empty after trimming, otherwise `line.name`. The result is cut to 200 characters with "…" as the 200th (RulesEngine.md: "≤ 200 characters").

**DSK-REC-4 Catalogue messages to add (SPEC-GAP-3, SPEC-GAP-7; chosen here).** The en-GB texts below are *chosen here* and are added to the bundled en-GB catalogue of `ordin-localisation`: the `approval-` messages beside the existing card wording in `crates/ordin-localisation/catalogues/en-GB/approval.ftl`, so they are safety wording (ADR-109) like the other `approval-` messages; the `rules-line-` and `desktop-` messages in the catalogue's own files for them. The catalogue change itself is a Patch step, not part of this amendment. Each other bundled locale carries them by the Localisation component's existing rule for a message added in en-GB.

| Message id | en-GB text |
|---|---|
| `approval-rules-not-stated` | Not stated |
| `approval-rules-verb-run` | Run |
| `approval-rules-verb-allow` | Allow |
| `approval-rules-verb-finish` | Finish anyway |
| `approval-rules-where-this-computer` | This computer |
| `approval-rules-where-this-run` | This run |
| `approval-rules-where-rule` | Rule { $rule } |
| `approval-rules-shell-question` | Run this command in { $runs_in }? |
| `approval-rules-shell-changes` | The command can read and change files in { $runs_in }. |
| `approval-rules-external-question` | Let the agent use { $tool } to reach { $destination }? |
| `approval-rules-external-changes` | Information leaves this computer for { $destination }. |
| `approval-rules-package-question` | Let the agent use { $tool } from { $server }? |
| `approval-rules-package-changes` | A tool from the agent's package runs with what { $server } can reach. |
| `approval-rules-listed-question` | Let the agent use { $tool }? |
| `approval-rules-listed-changes` | Your company asks you to approve each use of { $tool }. |
| `approval-rules-argument-question` | Let the agent use { $tool } as it asked? |
| `approval-rules-argument-changes` | The rule { $rule } holds this call until you check it. |
| `approval-rules-policy-question` | Let the agent { $verb } with { $tool }? |
| `approval-rules-policy-changes` | This agent's settings ask for your approval before it can { $verb }. |
| `approval-rules-gates-question` | Finish without the checks that did not pass? |
| `approval-rules-gates-changes` | The run ends although these checks did not pass: { $gates }. |
| `approval-crew-step-question` | Carry on with “{ $step }”? |
| `approval-crew-verb-carry-on` | Carry on |
| `approval-crew-where` | Crew: { $purpose } |
| `approval-crew-step-changes` | The crew waits here until you choose. |
| `rules-line-package` | From the agent's package: { $rule } |
| `rules-line-configuration` | From this agent's settings: { $rule } |
| `rules-line-application` | Always on: { $rule } |
| `rules-line-agent-policy` | From your company's policy for this agent: { $rule } |
| `desktop-backend-name` | This computer |

**DSK-REC-5 `service.hello` (SPEC-GAP-7).** `backend_name` = `Localisation::message` of `desktop-backend-name` (DSK-REC-4) in the active locale. Until the catalogue carries the message, the en-GB text "This computer" is returned, as now.

**DSK-REC-6 `CrewHost::document_changed` (SPEC-GAP-9; accepted limit of this build).** No owner keeps the versions an input document had at the workflow's previous run, and the File Service has no "changed since" query. On the desktop `document_changed` answers `true` for every input, so an application condition `document_changed` always takes its first path and never skips work the person asked for. Log `crew_document_changed_assumed` at `debug` with `workspace_id` and the input's `label` (*chosen here*). A full answer needs an owner for the previous run's versions; that is listed in *Gaps* (G-REC-2).

**DSK-REC-7 Model for a local session (SPEC-GAP-10 (a)).** The program's `SessionSetupSource` sets `SessionSetup.model` and `SessionSetup.effort` to `None` on the desktop. The Usage Planner's choice reaches the harness through `StartRun.model`, which the Session Supervisor already fills: the Claude Local Adapter passes `--model <StartRun.model.model>` when `SessionSetup.model` is `None` and `StartRun.model` carries a model (ClaudeLocalAdapter.md, amendment January 2026, CLA-REC-1). No `WorkKind` is needed on the program side.

**DSK-REC-8 The run register and the session lease (SPEC-GAP-10 (b)).** The program keeps one in-memory register `RunId → (AgentInstanceId, FolderLease)`, owned by its `SessionSetupSource` and shared with its `SupervisorHost`:
1. `session_setup(run_id, agent_instance_id)` calls `AgentFolderManager::lock(agent_instance_id, LockMode::Session)` before anything else. An `AfmError` → `SessionSetupError { code: <the AfmError's code> }` (the adapter then answers `CLA_SETUP_FAILED`). The lease is stored under `run_id`; a second call for the same `run_id` replaces the entry, and the earlier lease is dropped.
2. `SupervisorHost::run_ended(run_id)` removes the entry and drops its lease, after the Rules Engine's `end_run`. An absent entry (a remote run, or a run whose setup failed) is no error.
3. On quit (step 10), every entry is dropped after `session_supervisor.shutdown(ShutdownReason::Quit)` returns.

A `Session` lease admits other `Session` leases and blocks `Write` (install, update, repair, removal) only while a session runs, which is the reason the lease is held for the run's life.

**DSK-REC-9 The workspace of a run (SPEC-GAP-10 (c)).** The program reads the workspace of any run, a task run included, from `RunView.workspace_id` (SessionSupervisor.md, amendment January 2026, SUP-REC-1), and a task run's task from `RunView.task`. `session_setup` applies the folder grants of that workspace for every run, as it does for a conversation.

**DSK-REC-10 `InstanceActivity` (SPEC-GAP-11).** Over the run register of DSK-REC-8: `running(id)` is true while an entry with that `AgentInstanceId` exists; `idle()` returns a receiver of a `tokio::sync::broadcast` channel of capacity 64 *(chosen here)*, sent the instance's id by `run_ended` when the removed entry was the instance's last. A send with no receiver is no error.

**DSK-REC-11 Language servers with no profile entry (SPEC-GAP-12).** `ProjectSettings.language_servers` takes, for a language with no profile entry, the entries returned by `ordin_setup::bundled_language_servers()` (SetupAssistant.md, amendment January 2026, SET-REC-1). The function is free, so the program calls it at row 3.31, before the Setup Assistant exists.

**DSK-REC-12 `KnowledgeMemory` (SPEC-GAP-13).** The Knowledge Service passes the workspace to `check_content(workspace_id, text)` and the workspace and read folder to `remove_page(workspace_id, folder_id, page_id)` (KnowledgeService.md, amendment January 2026, KNOW-REC-1). The program's implementation forwards both to the Memory Service one for one, keeps no record of index calls, and no longer answers `MEM_NOT_SUPPORTED` for `check_content`.

**DSK-REC-13 The memory models (SPEC-GAP-14, first part; chosen here).** The shell fills `CoreInputs.models_dir` with the `models` folder of Tauri's resource directory, where the bundle places `embedding/` and `reranker/` (MemoryService.md amendment, item 13), and `CoreInputs.model_manifest` with the `ModelManifest` compiled into the program by the release build. A build whose manifest lists no file starts, and recall answers `MEM_MODEL_UNAVAILABLE`, as now. The models' file names and digests are release-build data that no document records, as for the language-server table (SetupAssistant.md SA-CLOSE-01).

**DSK-REC-14 `RefResolver::approval_verbs` (SPEC-GAP-16).** At construction the program fills the list once with `Localisation::message_in(locale, id, &MessageArgs::new())` for each locale of `bundled_locales()` and each of the four approve-button messages `approval-rules-verb-run`, `approval-rules-verb-allow`, `approval-rules-verb-finish` and `approval-crew-verb-carry-on`, duplicates removed. A form's `verb` equal to any of them is refused with `BLOCK_FORM_INVALID` (ServiceApi.md, block check step 6), so a form button never reads as an approve button in any bundled language. A language file the person imports later is not in the list until the next start.

**DSK-REC-15 `RefResolver::run_agent_name` (SPEC-GAP-17).** Replaces "over the Journal's run index": `SessionSupervisor::get(run_id)` → `Run.agent`; `AgentInstance { agent_instance_id }` → `PackageCatalogue::get(agent_instance_id)`'s `instance.display_name`; `RemoteAgent { remote_agent_id }` → `OrganisationProfileService::remote_agent(remote_agent_id)`'s `display_name`. Any error → `None`.

**DSK-REC-16 `RefResolver::content` of an artefact (SPEC-GAP-18).** `ContentRef::Artefact { artefact_id }` → `Journal::artefact(artefact_id)` (JournalAndArtefactStore.md, amendment January 2026, JRNL-REC-1). `None` or an error → `false`. `Some(artefact)` → `true` only when the artefact belongs to the block's workspace: `ArtefactLocation::Place { place_id }` → `place_id` is among `WorkspaceService::places(workspace_id)`; `ArtefactLocation::AgentFolder { agent_instance_id }` → `PackageCatalogue::get(agent_instance_id).workspaces` contains `workspace_id`.

**DSK-REC-17 `RefResolver::entity` (SPEC-GAP-19).** In addition to the Telegram, Slack and Knowledge rows: `EntityRef::Run(run_id)` → `SessionSupervisor::get(run_id)` is `Ok` and its `workspace_id` (SUP-REC-1) equals the block's workspace; `EntityRef::MemoryFact(id)` → `MemoryService::get_facts(workspace_id, vec![id])` is `Ok`; `EntityRef::AgentRepository(id)` → `CatalogueService::repository_ref(id)` is `Ok`. `EntityRef::CrewCharter` answers `false` (G-REC-1); `Draft` answers `false` while the Mail Service is not built.

**DSK-REC-18 `JournalReader::waiting_items` (SPEC-GAP-20).** Beside the rows SA-CLOSE-09 and HomeAndSearch.md SA-CLOSE-01 already fix:

| Source | `kind` | `title` | `since` | `opens` | `workspace_id` |
|---|---|---|---|---|---|
| `ApprovalService::list_pending(None, …)`, each record's `card` | `approval` | `card.question` | `card.created_at` | `OpenTarget::Approval { workspace_id: card.workspace_id, approval_id: card.approval_id }` | `Some(card.workspace_id)` |
| `Journal::list_external_actions(Some(UnknownOutcome), …)` | `unknown_outcome` | `action.destination` | `action.sent_at` | `OpenTarget::Run { workspace_id, run_id: action.run_id }` | the run's `RunView.workspace_id` (SUP-REC-1) |
| `PushService::list_pending(workspace_id)` with `state = Held`, per workspace not in the Bin | `held_push` | `pending.request` | `pending.created_at` | `OpenTarget::Workspace { workspace_id }` | `Some(pending.workspace_id)` |

An external action with no `run_id`, or whose run cannot be read, is left out and logged `waiting_item_left_out` at `debug` with the action's id (*chosen here*).

**DSK-REC-19 `JournalReader::running` and `finished_since` (SPEC-GAP-21).** Each run-index entry becomes `RunSummary { workspace_id: RunView.workspace_id, run_id: Some(run_id), crew_run_id: <Some for a CrewRound origin>, title, state, at: ended_at or first_seen_at, can_run_again }`, with `title` = the agent's display name as DSK-REC-15 reads it, or the agent reference's id when no name can be read; `can_run_again` = `true` for a `Task` or `CrewRound` origin and `false` for a `Conversation`. The Supervisor's `list(Some(workspace))` paging of the code is no longer needed.

**DSK-REC-20 `OpenTarget::Item` kinds (SPEC-GAP-22).** The program's search sources set `item_kind` to the snake_case name of their `GoToKind` group: `agent` (id: the `AgentInstanceId`, `workspace_id: None`), `workflow` (id: the `WorkflowId`, `workspace_id: Some`), `conversation` (id: the conversation id, `workspace_id: Some`). `ScreensModule.md` does not yet name how an `item` target opens (G-REC-3).

**DSK-REC-21 `OrdinHostConfig.fixed_instructions_version` and `OrdinHostPolicy.crew_maxima` (SPEC-GAP-23, part).** `fixed_instructions_version` = `ordin_host::FIXED_INSTRUCTIONS_VERSION` (OrdinHost.md, amendment January 2026, OH-REC-2), not the application version: the instructions change with Ordin Host amendments, not with each release. `crew_defaults` and `crew_maxima` are both built from the profile's one `limits.crew`, which is both the company default and the company maximum (CrewOrchestrator.md A.4: "the company maximum (`limits.crew.*`)"), with `failure_repeat_limit` 2 and 5 as OrdinHost.md states. The two instance ids are OQ-REC-5.

**DSK-REC-22 The Ordin Host's provider registers (SPEC-GAP-24).** The Ordin Host's `remember`, `recall` and `write_page` descriptions are 117, 118 and 117 characters (OrdinHost.md, amendment January 2026, OH-REC-1), within McpFront.md SA-RECON-04's 1–120, so `mcp_front.register(ordin_host.tool_provider())` succeeds and start continues past row 3.41. The program does not shorten an owner's text.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `lock(agent_instance_id, Session)` fails in `session_setup` (DSK-REC-8) | `Err` to the adapter, retryable as the Agent Folder Manager says | `SessionSetupError { code: <AfmError code> }`, surfaced by the adapter as `CLA_SETUP_FAILED` | the run ends `failed` as for any setup failure |
| A proposal field is empty (DSK-REC-1) | `Ok` | — | the field reads `approval-rules-not-stated` |
| `Journal::artefact` errors or returns `None` (DSK-REC-16) | `false` | — | the block check refuses the reference as unresolved |
| A card-word argument exceeds its cut length (DSK-REC-1, DSK-REC-2) | `Ok` | — | the argument is cut with "…" so the field stays within the Approval Service's limit |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a shell command card passes the Approval Service's checks (DSK-REC-1)
    Given the Rules Engine asks to approve ShellCommand { command: "make test", runs_in: "supplier-portal" }
    When the program words the card
    Then the question is "Run this command in supplier-portal?" and the verb is "Run"
    And affects holds one record "supplier-portal" and changes is "The command can read and change files in supplier-portal."
    And ApprovalService::request accepts the card

  Scenario: Edge case — an empty proposal field reads "Not stated" (DSK-REC-1)
    Given the Rules Engine asks to approve ShellCommand { command: "ls", runs_in: "" }
    When the program words the card
    Then where_line is "Not stated" and the card is accepted

  Scenario: Edge case — a long tool name is cut so the question fits (DSK-REC-1)
    Given ApprovalListTool with a tool name of 300 characters
    When the program words the card
    Then the question is at most 120 characters and ends in "?"

  Scenario: Happy path — a rules line names where the rule comes from (DSK-REC-3)
    Given an enforced ControlLine with source Application and detail "this agent may not run commands"
    When the program words the line
    Then the line is "Always on: this agent may not run commands"

  Scenario: Error path — a form may not use an approve verb (DSK-REC-14)
    Given Ordin shows a form whose verb is "Allow"
    When the block check runs
    Then the form is refused with BLOCK_FORM_INVALID

  Scenario: Happy path — a session holds its lease until the run ends (DSK-REC-8, DSK-REC-10)
    Given agent A has no run
    When a run of A is set up
    Then InstanceActivity.running(A) is true and an update of A waits for the folder
    When the Session Supervisor reports the run ended
    Then the lease is dropped, running(A) is false and idle() receives A

  Scenario: Happy path — a task run's workspace comes from the run view (DSK-REC-9, DSK-REC-19)
    Given a task run R in workspace W
    When Home lists running work
    Then R's row is in W with can_run_again true

  Scenario: Happy path — the Ordin Host's provider registers on the desktop (DSK-REC-22)
    Given the Ordin Host's tool table as amended in January 2026
    When start reaches the Ordin Host's registration
    Then the MCP Front accepts the provider and start continues to row 3.42

  Scenario: Edge case — an application document condition takes its first path (DSK-REC-6)
    Given a saved workflow step "if the contract changed" with condition document_changed
    When the crew reaches the step
    Then the first path is taken
```

### Open Questions

- **OQ-REC-1** *A remote agent's route and presence, read without waiting (SPEC-GAP-5, SPEC-GAP-8).* `SupervisorHost::remote_route` and `CrewHost::remote_route` are synchronous; the remote agents' ids exist behind `OrganisationProfileService::remote_agents()`, which is asynchronous, and `RemoteAgent` has no version for `CrewHost::agent_status`. Options: (a) the Organisation Profile gains a synchronous `remote_agent_route(id: RemoteAgentId) -> Option<RemoteRoute>` over the same in-memory id-to-name map `remote_agent_identity` already reads, `None` for an unknown or `Withdrawn` agent; the program maps `Http` → `AdapterKind::HostedAgent` and `Slack` → `AdapterKind::SlackThread`; `agent_status` of a remote member reads `remote_agent(id)` (asynchronous, as `agent_status` is) and answers `Present { version: "" }` unless the agent is `Withdrawn`, and the Ordin Host saves `""` as a remote member's version so it never reads as changed; (b) both `remote_route` methods become `async` in `SessionSupervisor.md` and `CrewOrchestrator.md`. **Recommendation:** (a): one accessor in the module that already holds the map, against a signature change in two hub traits and every implementation. Blocks only the builds with the `hosted-agents` or `slack` switch on; a build with neither switch on registers no remote adapter and is not affected.
- **OQ-REC-2** *Attachments, shell and package servers of a local session (SPEC-GAP-10 (d)).* No spec names the source of `SessionSetup.attachment_paths` (a task's `attached_artefacts` are `ArtefactId`s; no owner turns one into an absolute path), `shell_allowed` ("a system-changing package on a project with approved shell commands": no desktop owner answers it) or `package_servers` (the Installers' `HarnessLaunch::ClaudeCode` carries none). Until settled they are empty, `false` and empty: a session sees no attachments, runs no shell command and starts no package-declared MCP server. **Recommendation:** a scoping pass with the Installers, Package Inspector and File Service owners before choosing; the question is not yet ripe for a yes or no. Blocks agents whose packages declare MCP servers or need shell commands, and tasks with attachments.
- **OQ-REC-3** *The `origin_ref` of a place file (SPEC-GAP-14, second part).* "After row 3.28" forwards File Service changes to `MemoryService::source_changed(origin_ref, current_version)`, but agents write `origin_ref` as free text ("SharePoint — Procurement/…"), so no form of a place file matches the facts that cite it. Options: (a) forward nothing on the desktop until a canonical form exists, and remove that row's forwarding; (b) fix a canonical form for place files that the Ordin Host and the memory tools write and the program forwards. **Recommendation:** (a) now, (b) as its own change: (b) needs every writer of `origin_ref` to change together. Blocks only the "may be out of date" mark on facts from place files.
- **OQ-REC-4** *Where the Claude Local Adapter's `claude` path comes from (SPEC-GAP-15).* `ClaudeAdapterConfig.claude_path` is fixed at row 3.32; the Setup Assistant, which finds the path (`ready_harness().location`), is built at row 3.43. Options: (a) `SessionSetup` gains `claude_path: Option<PathBuf>`, filled from `ready_harness().location` per session (as SetupAssistant.md's consumer table already says), with `ClaudeAdapterConfig.claude_path` kept for the adapter's sign-in checks; (b) the shell finds the path before start by the Setup Assistant's own rule and passes it in `CoreInputs.claude_path`, as the code does now. **Recommendation:** (a), because a repaired or moved harness is used without a restart; (b) is acceptable for this build if the change to `ClaudeLocalAdapter.md` must wait. Nothing is blocked today: the code runs (b).
- **OQ-REC-5** *The bundled Ordin and agent-maker instances (SPEC-GAP-23).* `OrdinHostConfig` needs `ordin_agent_instance_id` and `agent_maker_instance_id` "installed at first start", but no spec installs them or names their identifiers in the bundled catalogue, so the code passes the nil id and `PackageCatalogue::ensure_ready` refuses: **Ordin's conversation never starts.** Options: (a) the program adds both agents from the bundled catalogue with `PackageCatalogue::add` at first start and keeps their ids in `<data root>/preferences.json`; (b) the Package Catalogue owns them: an accessor that returns the instance of a named bundled agent, adding it on first call. **Recommendation:** (b): the catalogue already owns instances and their repair, and a lost preferences file would otherwise orphan Ordin's instance. Either option also needs the two agents' identifiers in the bundled catalogue stated in `PackageCatalogue.md` or `CatalogueService.md`. **Blocks the build with no switch on**: Ordin's conversation and the agent maker.

### Gaps

- **G-REC-1** No owner reads a crew charter by `CrewCharterId` (the Crew Orchestrator's public `get` takes a `CrewRunId`), so `EntityRef::CrewCharter` answers `false`, and a block that cites a charter is refused as unresolved. Owner to name: the Ordin Host (which holds crew plans) or the Crew Orchestrator.
- **G-REC-2** No owner keeps the versions an input document had at a workflow's previous run (DSK-REC-6).
- **G-REC-3** `ScreensModule.md` states no screen for an `OpenTarget::Item` of kind `agent`, `workflow` or `conversation` (DSK-REC-20).

## Amendment January 2026 — Bundled agents and the inputs of a local session (OQ-REC-5, OQ-REC-2)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"), except OQ-REC-6 and OQ-REC-8, which stay open. DSK-REC-26 step 1 is replaced by DSK-REC-29 of the next amendment (Approved by the user 2026-01-10).
>
> Closes OQ-REC-5 of the Phase 11 reconciliation above, as the user chose it on 2026-01-10 (option (b): the Package Catalogue adds Ordin's own agent and the agent maker on first use), and scopes OQ-REC-2 with its owners. Evidence read: `PackageCatalogue.md` (`add` §A, `ensure_ready` §F, `get` §H, `source_scope` §M, the `agent_instances` table, `AgentInstance.commit` "except for the bundled catalogue (`BUNDLED_REPOSITORY_ID`)"); `CatalogueService.md` (`start` §A step 1, `find_part`, `source_scope`, `CatalogueConfig.bundled_catalogue` "the frozen reference `models/`, ADR-082"); `AgentModel.md` (`BUNDLED_REPOSITORY_ID`, the identifier grammar `urn:ordin:<publisher>:<kind-dir>[:<category>]*:<slug>`, `McpServerDecl`); `OrdinHost.md` (`OrdinHostConfig`, step 42.1 `start_agent_maker`); HLD `docs/design/ordin-desktop/09-decisions.md` ADR-027 ("Ordin is an agent package … bundled with the application at a pinned version, and always present. The person does not install, update or remove it") and ADR-083 (the agent maker is the advisor workspace, `models/@default/workspaces/advisor`); `phase-1-analysis.md` glossary row *WorkspaceProfile*, example `urn:ordin:default:workspaces:advisor`; `ClaudeLocalAdapter.md` and `CopilotLocalAdapter.md` (`SessionSetupSource`, `SessionSetup`, `CopilotSessionSetup`, `PackageMcpServer`, the `--tools … [,Bash]` line); `RuntimeAdapterInterface.md` (`attachments` at most 50, `RA_ATTACHMENT_NOT_GRANTED`; `check_start` does not check grants); `SessionSupervisor.md` (`StartRequest.attachments`, `StartRun.attachments`, SUP-REC-1 `RunView.task`); `JournalAndArtefactStore.md` JRNL-REC-1 (`artefact(id)`); 2C `Artefact { location, path, producing_run_id, .. }`, `ArtefactLocation`; `FileService.md` (`place(place_id)`, `Place.path`); `WorkspaceService.md` (`places(id)`); `ProjectService.md` (`project_for_place`: "the project not `closed` on a place"); `RulesEngine.md` D1 step 7.4 (*Shell*, `app.shell`); HLD `07-cross-cutting.md` and `10-risks.md` A-06, ADR-037 *Consequences*; `phase-1-analysis.md` SA-CRED-01; `apps/desktop/src/hosts_run.rs` (`assemble`: `attachment_paths` empty, `shell_allowed` false, `package_servers` empty). The bundled catalogue folder itself is not in this repository. Owner changes are drafted in `PackageCatalogue.md`, `CatalogueService.md`, `OrdinHost.md` and `cross-cutting/ErrorHandling.md`, each marked with the same status. Every section above stays in force except where this amendment states otherwise.

### Resolution

| Question | Outcome | Assertion |
|---|---|---|
| OQ-REC-5 | Resolved: the Package Catalogue owns both instances at fixed ids and adds each on first use; the agent maker's identifier is taken from the HLD; Ordin's own identifier is still to be supplied | DSK-REC-23 to DSK-REC-25 (with PKG-REC-1 to PKG-REC-4, CAT-REC-1, OH-REC-3); OQ-REC-6 |
| OQ-REC-2, attachments | Resolved: how an artefact becomes a checked absolute path; the list read from the task row until OQ-REC-7 is decided | DSK-REC-26; OQ-REC-7 |
| OQ-REC-2, shell | Resolved | DSK-REC-27 |
| OQ-REC-2, package tool servers | Resolved as an accepted limit of this build; a change to the Agent Model is needed before any can start | DSK-REC-28; OQ-REC-8 |

### Internal Logic (additions)

**DSK-REC-23 The two instance ids of `OrdinHostConfig` (OQ-REC-5).** At row 3.41 the program sets `ordin_agent_instance_id = BundledAgent::Ordin.instance_id()` and `agent_maker_instance_id = BundledAgent::AgentMaker.instance_id()` (PackageCatalogue.md, amendment January 2026, PKG-REC-1). Both are fixed values, so row 3.41 reads nothing and needs no catalogue that has finished indexing. The program keeps no copy of either id in `<data root>/preferences.json`: a lost or reset preferences file cannot orphan either instance.

**DSK-REC-24 Adding the bundled instances at start (OQ-REC-5).** In step 4, once `catalogue.start(shutdown)` has returned `Ok`, the program spawns one task (not awaited; start goes on to the next item of step 4) that calls, in this order, `packages.bundled_instance(BundledAgent::Ordin, &localisation.active().locale)` and `packages.bundled_instance(BundledAgent::AgentMaker, &localisation.active().locale)` (PKG-REC-2). For each call:
1. `Ok` → log `bundled_agent_ready` at `debug` with `agent` (`ordin` or `agent_maker`) and `agent_instance_id`.
2. `Err` with `retryable = true` → log `bundled_agent_unavailable` at `warn` with `agent`, `code` and `attempt`; call again after 30 s, at most 5 attempts in all per agent per start *(chosen here)*; the fifth failure is logged at `error` and the task moves on to the next agent.
3. `Err` with `retryable = false` → log `bundled_agent_unavailable` at `error` with `agent` and `code`; no further attempt until the next start.
4. A cancelled shutdown token ends the task between attempts.

Because `bundled_instance` returns the stored instance without writing when it is already installed (PKG-REC-2 step 2), every later start costs one row read per agent. Until the call for Ordin has succeeded, `catalogue.ensure_ready(ordin_agent_instance_id)` in the Ordin Host fails as `PackageCatalogue.md` §F states for an instance that is not ready, and Ordin's conversation shows its pause with `OH_DEPENDENCY_FAILED` (OrdinHost.md step 3.1); the person's message waits, as for any dependency failure.

**DSK-REC-25 An artefact in a bundled agent's folder (OQ-REC-5).** A bundled instance serves no workspace (PKG-REC-3), so DSK-REC-16's test "`PackageCatalogue::get(agent_instance_id).workspaces` contains `workspace_id`" would refuse every artefact Ordin or the agent maker writes in its own folder. For `ArtefactLocation::AgentFolder { agent_instance_id }` where `agent_instance_id` is one of the two `BundledAgent` ids, `RefResolver::content` answers `true` only when the artefact's `producing_run_id` is `Some(run_id)` and `SessionSupervisor::get(run_id)`'s `workspace_id` (SUP-REC-1) equals the block's workspace; no producing run, or any error, answers `false`. Every other artefact keeps DSK-REC-16's rule.

**DSK-REC-26 `SessionSetup.attachment_paths` and `CopilotSessionSetup.attachment_paths` (OQ-REC-2).** The program's `SessionSetupSource`, for both local adapters:
1. **Which artefacts.** Until OQ-REC-7 is decided, the list is `RunView.task.attached_artefacts` of `SessionSupervisor::get(run_id)` (SUP-REC-1) for a run with a `Task` origin, and empty for every other origin. A run whose `StartRequest.attachments` were not saved in a task row (a crew round, which passes its input artefacts, CrewOrchestrator.md step 4) therefore sees none of them (OQ-REC-7). Duplicate ids are removed, first occurrence kept; more than 50 ids → `SessionSetupError { code: "RA_INPUT_TOO_LARGE" }` (the Runtime Adapter Interface's code for the count, RuntimeAdapterInterface.md step 3).
2. **The artefact.** For each id, in order: `journal.artefact(id)` (JRNL-REC-1). `Err(e)` → `SessionSetupError { code: <e's code> }`. `Ok(None)` → `SessionSetupError { code: "RA_ATTACHMENT_NOT_GRANTED" }`.
3. **Its folder.** `ArtefactLocation::Place { place_id }` → `files.place(place_id)`'s `Place.path`; a `FileError` → `SessionSetupError { code: <its code> }`. `ArtefactLocation::AgentFolder { agent_instance_id }` → `packages.get(agent_instance_id)`'s `instance.agent_folder_path`; a `PackageError` → `SessionSetupError { code: <its code> }`.
4. **The path.** The folder joined with `artefact.path` (2C: relative to its folder, at most 4096 bytes), then resolved through symbolic links. The resolved path must name an existing regular file that lies under the run's own agent folder (`SessionSetup.agent_folder` or `CopilotSessionSetup.agent_folder`) or, for the Claude Local Adapter only, under one of the folders of `SessionSetup.extra_dirs` (the folder grants already applied for this run, DSK-REC-9; resolved the same way). Otherwise → `SessionSetupError { code: "RA_ATTACHMENT_NOT_GRANTED" }`. A Copilot session has no granted folder outside its agent folder (`CopilotSessionSetup` carries none), so only artefacts in its own folder pass.
5. `attachment_paths` = the resolved paths in list order. Log `session_attachments` at `debug` with `run_id` and the count; never a path.

A failure in steps 1 to 4 fails the whole setup, so the run ends `failed` as for any setup failure (DSK-REC-8 row): a session never starts with an attachment silently missing. The Claude Local Adapter then names each path in the first turn (`Attached: <path>`, ClaudeLocalAdapter.md step 11), and the attachments are readable because they are inside the agent folder or a `--add-dir` folder.

**DSK-REC-27 `SessionSetup.shell_allowed` (OQ-REC-2).** The ClaudeLocalAdapter field reads "True only for a system-changing package on a project with approved shell commands (A-06; 09 ADR-037)". The program sets it to `true` exactly when both hold:
1. `packages.get(agent_instance_id)`'s `instance.authority` is `Authority::SystemChanging` (the Package Inspector's reading, stored by the Package Catalogue); and
2. the run's workspace (`RunView.workspace_id`, SUP-REC-1) has at least one place, among `workspaces.places(workspace_id)`, for which `projects.project_for_place(place.id)` returns `Some` (a project that is not `closed`). This is the reading *chosen here* of ADR-037's "shell commands remain only for system-changing packages working on a project".

"Approved" is not decided here: every shell command of such a session is still put to the person by the Rules Engine unless the package declares a rule that allows it (RulesEngine.md D1 step 7.4, rule `app.shell`; A-06). `shell_allowed` only decides whether `Bash` is on the `--tools` list. Any error from the three reads → `false`, logged `shell_allowed_unread` at `warn` with `run_id` and `code`; the session still starts, without a shell. `CopilotSessionSetup` has no such field and is unchanged.

**DSK-REC-28 `package_servers` (OQ-REC-2; accepted limit of this build).** Both setups carry `package_servers` empty in this build. The reason is in the owners' types, not in the program: a package declares a server as `McpServerDecl { name, server_type, scope, env_ref, transport, declared_by }` (AgentModel.md), with no program and no arguments, while `PackageMcpServer` needs `command` and `args` (ClaudeLocalAdapter.md); and the card on which the person enters a server's credential once (SA-CRED-01) has no owner flow in any component spec. No program can be started from what is declared, so none is. An agent whose plan names such a server is still added and runs, with ordin's own servers (the MCP Front) and without that server; its agent page lists the connection as the Package Inspector reads it. OQ-REC-8 holds the change that would lift the limit.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `bundled_instance` fails with a retryable error at start (DSK-REC-24) | retried after 30 s, at most 5 attempts | the `PackageError` code, logged `bundled_agent_unavailable` | none; until it succeeds, Ordin's conversation shows its pause with `OH_DEPENDENCY_FAILED` |
| `bundled_instance` fails with a permanent error (DSK-REC-24), for example `PKG_BUNDLED_AGENT_MISSING` | not retried until the next start | the `PackageError` code, logged at `error` | as above; Diagnostics shows the log line |
| An attached artefact is unknown, outside the agent folder and the run's granted folders, or not an existing file (DSK-REC-26) | `Err` to the adapter | `SessionSetupError { code: "RA_ATTACHMENT_NOT_GRANTED" }`, surfaced by the Claude Local Adapter as `CLA_SETUP_FAILED` | the run ends `failed` as for any setup failure |
| More than 50 attached artefacts (DSK-REC-26) | `Err` to the adapter | `SessionSetupError { code: "RA_INPUT_TOO_LARGE" }` | as above |
| The Journal, File Service or Package Catalogue read for an attachment fails (DSK-REC-26) | `Err` to the adapter, retryable as the owner says | `SessionSetupError { code: <owner code> }` | as above |
| A read for `shell_allowed` fails (DSK-REC-27) | `Ok`; `shell_allowed = false` | logged `shell_allowed_unread` | the session runs without a shell |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — Ordin's conversation starts on a first start (DSK-REC-23, DSK-REC-24)
    Given a new data root and a bundled catalogue that holds Ordin's agent and the agent maker
    When ordin starts and the person sends a first message
    Then the Ordin Host receives BundledAgent::Ordin.instance_id() as ordin_agent_instance_id
    And ensure_ready for that id succeeds and Ordin's session starts

  Scenario: Edge case — a later start adds nothing (DSK-REC-24)
    Given both bundled instances are ready
    When ordin starts again
    Then each bundled_instance call returns the stored instance and no install is recorded

  Scenario: Error path — the bundled install is retried (DSK-REC-24)
    Given the Checker is unavailable on the first bundled_instance call for Ordin
    When start reaches step 4
    Then bundled_agent_unavailable is logged with attempt 1
    And the call is made again 30 seconds later

  Scenario: Happy path — a task's attachment reaches the session (DSK-REC-26)
    Given a task in workspace W attaches artefact A in place P, and P is granted to the run
    When the session is set up
    Then attachment_paths holds the absolute path of A under P's folder

  Scenario: Error path — an attachment outside the run's folders (DSK-REC-26)
    Given a task attaches an artefact in a place that is not granted to the run
    When the session is set up
    Then setup fails with RA_ATTACHMENT_NOT_GRANTED and the run ends failed

  Scenario: Happy path — a system-changing agent on a project gets a shell (DSK-REC-27)
    Given a system-changing agent and a workspace with an open project
    When the session is set up
    Then shell_allowed is true and each command it runs is still put to the person unless its package allows it

  Scenario: Edge case — no project, no shell (DSK-REC-27)
    Given a system-changing agent and a workspace with no open project
    When the session is set up
    Then shell_allowed is false

  Scenario: Edge case — a declared package server is not started (DSK-REC-28)
    Given an agent whose plan declares the MCP server "jira-reader"
    When the session is set up
    Then package_servers is empty and the session starts
```

### Open Questions

- **OQ-REC-6** *The identifiers of the two bundled agents (blocks the build with no switch on: Ordin's conversation).* The Package Catalogue reads each bundled agent from the bundled catalogue by its WorkspaceProfile identifier (PKG-REC-1). The bundled catalogue folder is not in this repository, and no document names Ordin's own agent as a WorkspaceProfile: ADR-027 describes Ordin as "an agent package built with AgentWright". The agent maker's identifier is taken as `urn:ordin:default:workspaces:advisor`, from ADR-083's path `models/@default/workspaces/advisor` and the glossary example of a WorkspaceProfile; it is not read from the folder that ships. **The maintainer must supply:** (1) the `metadata.id` of Ordin's own WorkspaceProfile in the bundled catalogue, in the form `urn:ordin:<publisher>:workspaces:<slug>` (AgentModel.md identifier grammar); (2) confirmation that this part, and the advisor workspace under the identifier above, are in the bundled catalogue the release ships, offered and passing their checks; (3) if Ordin's agent is not yet a part of the bundled catalogue, that part itself. **Recommendation:** supply (1) and (3) as one change to the bundled catalogue; the spec then needs only the one constant filled in. Until then `bundled_instance(BundledAgent::Ordin, …)` answers `PKG_BUNDLED_AGENT_MISSING` and Ordin's conversation does not start; the agent maker is not blocked if (2) holds for it. **Status 2026-01-10:** open; waits for the maintainer. **Resolved 2026-01-10 (identifier):** the user supplied `urn:ordin:default:workspaces:coordinator` as Ordin's own WorkspaceProfile identifier. Still to confirm by the maintainer: that this entry exists in the bundled catalogue the release ships, offered and passing its checks (point 2 above); otherwise `PKG_BUNDLED_AGENT_MISSING` stands.
- **OQ-REC-7** *Attachments of runs that are not tasks.* `StartRequest.attachments` is filled for tasks and for crew rounds (their input artefacts), but the `SessionSetupSource` receives only `run_id` and `agent_instance_id`, and the Session Supervisor keeps the attachments only in the task row. Options: (a) both adapters' `SessionSetupSource::session_setup` gains the start request's attachments as a third argument, `attachments: &[ArtefactId]`, copied from `StartRun.attachments`, and DSK-REC-26 step 1 reads that list for every origin (a change to `ClaudeLocalAdapter.md`, `CopilotLocalAdapter.md` and both programs); (b) keep the task row as the only source; a crew member then reads its inputs through its tools (the brief already names each input's location, CrewOrchestrator.md step 2). **Recommendation:** (a): the start request already carries the list for every origin, and one added argument is cheaper than members that cannot read the documents the crew was given. Blocks only crew members' direct access to their input documents; tasks work now. **Resolved 2026-01-10:** the user chose (a), crew members receive the documents they are given as attachments, not only tasks; specified as DSK-REC-29 with ClaudeLocalAdapter.md CLA-REC-2 and CopilotLocalAdapter.md CPA-REC-1 (Approved by the user 2026-01-10).
- **OQ-REC-8** *Package-declared tool servers.* To start a server a package declares, three things are missing: the program and arguments to run (`McpServerDecl` has neither), a trust decision on that program (today only ordin's trusted, pinned programs run outside the sandbox, ADR-037), and the credential card of SA-CRED-01. Options: (a) the Agent Model's `McpServerDecl` gains `command` and `args`, the Checker's trust policy decides on the program, and an owner (the Identity and Credentials service is the candidate) gains the SA-CRED-01 card; (b) packages never start their own programs on the desktop, and an agent's tool servers are always ordin's own. **Recommendation:** decide (a) or (b) before any agent repository publishes an agent that relies on its own server; until then the limit of DSK-REC-28 holds. A yes to (a) is a change to four component specs; (b) closes the question with no build work. Blocks agents whose work depends on a server their package declares. **Status 2026-01-10:** open; the user accepted the recommendation: (a) or (b) is decided before any agent repository publishes an agent that depends on its own server.

### Gaps

None.

## Amendment January 2026 — Follow-ups of 2026-01-10 (OQ-REC-7; SPEC-GAP-25, 26, 27)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"), with OQ-REC-9 resolved as recommended (DSK-REC-29 below).
>
> The user's instruction of 2026-01-10 ("proceed with recommendations") chose OQ-REC-7 (a). This amendment specifies it and settles three markers of `apps/desktop`: `SPEC-GAP-25` (`apps/desktop/src/words.rs`), `SPEC-GAP-26` (`apps/desktop/src/service_api.rs`) and `SPEC-GAP-27` (`apps/desktop/src/wiring.rs`, `apps/desktop/src/wiring_tests.rs`). Evidence read: DSK-REC-1 and the DSK-REC-4 message table above; `crates/ordin-localisation/catalogues/en-GB/approval.ftl`; `ApprovalService.md` (`question` 1–120 characters ending in "?", `APPR_QUESTION_INVALID`); `Localisation.md` B1 (`set_use_isolating(true)`), B5, B6, D and the `localisation.get_settings` row; `CrewOrchestrator.md` step 4 (`attachments: input artefacts`); `SessionSupervisor.md` C.2 (`StartRun { … attachments … }`); `RuntimeAdapterInterface.md` (attachments at most 50); `OrdinHost.md` OH-REC-4. Owner changes are drafted in `ClaudeLocalAdapter.md` (CLA-REC-2), `CopilotLocalAdapter.md` (CPA-REC-1), `OrdinHost.md` (OH-REC-4) and `Localisation.md` (LOC-REC-1), each marked with the same status. Every section above stays in force except where this amendment states otherwise.

### Resolution

| Question or marker | Outcome | Assertion |
|---|---|---|
| OQ-REC-7 | Resolved as the user chose: every run's attachments reach the session | DSK-REC-29 (with CLA-REC-2, CPA-REC-1) |
| OQ-REC-9 | Resolved as recommended (user, 2026-01-10): a crew round leaves out an input that fails the folder check and logs it; a task still fails | DSK-REC-29 (crew-round rule) |
| `SPEC-GAP-25` | Resolved: the cut length of a question's arguments depends on how many it has and on the locale's text | DSK-REC-30 |
| `SPEC-GAP-26` | Resolved in its owner | DSK-REC-32 (LOC-REC-1) |
| `SPEC-GAP-27` | Resolved in its owner | DSK-REC-31 (OH-REC-4) |

### Internal Logic (additions)

**DSK-REC-29 Which artefacts a session is given (OQ-REC-7; replaces DSK-REC-26 step 1).** The program's `SessionSetupSource::session_setup(run_id, agent_instance_id, attachments)` (ClaudeLocalAdapter.md CLA-REC-2, CopilotLocalAdapter.md CPA-REC-1) takes the list from its `attachments` argument, for every origin: a task's attached artefacts, a crew round's input artefacts (CrewOrchestrator.md step 4) and any other run's `StartRequest.attachments`, as the Session Supervisor copied them into `StartRun.attachments` (SessionSupervisor.md C.2). It no longer reads `RunView.task.attached_artefacts` for this. Duplicate ids are removed, first occurrence kept; more than 50 ids → `SessionSetupError { code: "RA_INPUT_TOO_LARGE" }`. Steps 2 to 5 of DSK-REC-26 and its failure rule are unchanged, with one exception for crew rounds (OQ-REC-9, resolved 2026-01-10):

- **Crew-round rule.** When `RunView.origin` of `SessionSupervisor::get(run_id)` (SUP-REC-1) is `RunOrigin::CrewRound`, an artefact whose resolved path is an existing regular file but lies outside the run's agent folder and, for the Claude Local Adapter, outside every folder of `SessionSetup.extra_dirs` (the folder test of DSK-REC-26 step 4) is left out of `attachment_paths` instead of failing the setup. The program logs `crew_input_not_attached` at `info` with `run_id` and `artefact_id`, never a path, and the setup goes on with the next id. The member reads that document through its tools at the location the brief names (CrewOrchestrator.md step 2). This is the case of every crew input in a place for a Copilot member, which has no granted folder outside its agent folder (DSK-REC-26 step 4).
- **Everything else still fails.** For a crew round, an id that is unknown (step 2), whose folder cannot be read (step 3) or whose path is not an existing regular file (step 4) still fails the setup with `RA_ATTACHMENT_NOT_GRANTED` or the owner's code, and the member's run ends `failed`; the Crew Orchestrator sees that run end as for any failed round. For every other origin (a task, a conversation), the folder test still fails the setup with `RA_ATTACHMENT_NOT_GRANTED`: the person attached a task's documents directly and sees the failure.
- **The origin read.** `SessionSupervisor::get(run_id)` is read once per setup, before step 2, only when `attachments` is not empty; its error → `SessionSetupError { code: <its code> }`.

**DSK-REC-30 The cut length of a question's arguments (`SPEC-GAP-25`; amends DSK-REC-1).** DSK-REC-1's fixed cut of 60 characters per argument does not keep `question` within the Approval Service's 120 characters for a question with two arguments: with both arguments 60 characters long, `approval-rules-external-question`, `approval-rules-package-question` and `approval-rules-policy-question` reach 141 to 149 characters in en-GB, and the card is refused with `APPR_QUESTION_INVALID`. For the `question` field only, `RulesHost::word` and `CrewHost::word_step_approval` build the text as follows, counting characters as the Approval Service counts `question`:
1. Format the message with every argument cut to 60 characters as DSK-REC-1 and DSK-REC-2 state. At most 120 characters → that is `question`.
2. Otherwise let `F` be the length of the same message formatted with every argument the empty string (this counts the message's own words and any marks the formatter adds around arguments, Localisation.md B1), `n` the number of its arguments, and `L = min(60, floor((120 − F) / n))`. Cut each argument longer than `L` to `L` characters with "…" as the `L`-th, and format again. The result is at most 120 characters and ends in "?" because the message does.
3. When `L < 2` (a locale whose message leaves fewer than 2 characters per argument), format the en-GB message instead with the rule of step 2 and log `card_words_too_long` at `warn` with `message_id` and `locale`, never an argument value.

An argument that fits within `L` is not cut, and `where_line`, `affects`, `changes` and `verb` keep the cuts DSK-REC-1 states. With the en-GB messages, `L` is at least 43 characters for every two-argument question.

**DSK-REC-31 The Ordin Host's provider registers (`SPEC-GAP-27`).** OrdinHost.md OH-REC-4 publishes `decide` and `change_setting` as one object schema each, so the MCP Front's check of step 2.3 passes and the registration of the Ordin Host's provider (DSK-REC-22) succeeds. The program does not rewrite an owner's schema; it registers the provider as DSK-REC-22 states. Once OH-REC-4 is built, the start test runs every step with no exception for the Ordin Host.

**DSK-REC-32 `following_operating_system` (`SPEC-GAP-26`).** The program answers `localisation.get_settings` with `{ active, offered, following_operating_system }` where `active = localisation.active()`, `offered = localisation.offered()` and `following_operating_system = (active.source == LocaleSource::OperatingSystem)`, as Localisation.md LOC-REC-1 states.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A crew member's input artefact is unknown, its folder cannot be read, or it is not an existing file (DSK-REC-29) | `Err` to the adapter | `SessionSetupError { code: "RA_ATTACHMENT_NOT_GRANTED" }` or the owner's code | the member's run ends `failed`, as for a task |
| A crew member's input artefact is an existing file outside the member's agent folder and its run's granted folders (DSK-REC-29, crew-round rule) | `Ok`; the artefact is left out of `attachment_paths` | — | logged `crew_input_not_attached`; the member finds the document from its brief |
| A task's attached artefact is outside the agent folder and the run's granted folders (DSK-REC-29) | `Err` to the adapter | `SessionSetupError { code: "RA_ATTACHMENT_NOT_GRANTED" }` | the run ends `failed` |
| The run's origin cannot be read (DSK-REC-29) | `Err` to the adapter | `SessionSetupError { code: <owner code> }` | the run ends `failed` |
| A translated question leaves fewer than 2 characters per argument (DSK-REC-30 step 3) | `Ok`; en-GB words used | — | logged `card_words_too_long` |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a crew member is given its input document (DSK-REC-29)
    Given a crew round whose input artefact A is in place P, and P's folder grant is applied to the member's run (DSK-REC-9)
    When the member's Claude session is set up
    Then attachment_paths holds the absolute path of A under P's folder

  Scenario: Edge case — a crew member's input outside its folders is left out (DSK-REC-29, OQ-REC-9)
    Given a crew round run by a Copilot member whose input artefact is an existing file in a place
    When the member's session is set up
    Then the setup succeeds, attachment_paths does not hold that artefact's path
    And crew_input_not_attached is logged with the run id and the artefact id

  Scenario: Error path — a task's attachment outside its folders still fails (DSK-REC-29)
    Given a task whose attached artefact is in a place not granted to the run
    When the run's session is set up
    Then setup fails with RA_ATTACHMENT_NOT_GRANTED and the run ends failed

  Scenario: Error path — a crew member's unknown input still fails (DSK-REC-29)
    Given a crew round whose input artefact id is not in the journal
    When the member's session is set up
    Then setup fails with RA_ATTACHMENT_NOT_GRANTED and the member's run ends failed

  Scenario: Edge case — two long arguments still fit the question (DSK-REC-30)
    Given an ExternalTool proposal whose tool and destination are each 100 characters
    When the program words the card in en-GB
    Then the question is at most 120 characters, ends in "?" and the Approval Service accepts it

  Scenario: Edge case — short arguments are not cut (DSK-REC-30)
    Given a PackageTool proposal with tool "read_issue" and server "jira"
    When the program words the card in en-GB
    Then the question is "Let the agent use read_issue from jira?" apart from marks around the arguments

  Scenario: Happy path — the settings say the language follows the computer (DSK-REC-32)
    Given no language was chosen by the person
    When localisation.get_settings is called
    Then following_operating_system is true
```

### Open Questions

- **OQ-REC-9** *Crew members on Copilot and input documents in places.* With DSK-REC-29, a crew member is given the round's input documents and its setup fails when one is outside its own folders. A Claude member reaches a place whose folder grant is applied to its run (`--add-dir`, DSK-REC-9); a Copilot member has no granted folder outside its agent folder, so any input document in a place fails its round. Options: (a) keep the one rule: the member's run fails with `RA_ATTACHMENT_NOT_GRANTED`, visibly; (b) for a crew round only, leave out an input that fails the folder check, log it, and let the member read it through its tools as the brief names its location (CrewOrchestrator.md step 2). **Recommendation:** (b) for crew rounds, keeping (a) for tasks: the person attached a task's documents directly and must see a failure, whereas a crew's inputs are also named in the brief, and no grant can fix the Copilot case. Blocks only crew rounds with input documents in places run by a Copilot member; until decided, DSK-REC-29 applies (a). **Resolved 2026-01-10:** the user accepted the recommendation, (b) for crew rounds and (a) for tasks; specified as the crew-round rule of DSK-REC-29, Approved by the user 2026-01-10.

### Gaps

None.

## Amendment January 2026 — Phase 11 follow-ups of 2026-01-10 (SPEC-GAP-28 to 35; FU-138 program side)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). The user's instruction of 2026-01-10 ("proceed with recommendations") accepted three recommendations, specified here: SPEC-GAP-35 (the macOS menu's Quit asks first, DSK-REC-35), SPEC-GAP-31 (a new `installation_id` at each start, DSK-REC-33) and SPEC-GAP-32 (the computer's name is the operating system's device name, DSK-REC-34). Every section above stays in force except where this amendment states otherwise.
>
> Evidence read: the markers `SPEC-GAP-28` (`apps/desktop/src/core.rs`), `SPEC-GAP-29` and `SPEC-GAP-30` (`apps/desktop/src/service_api.rs`), `SPEC-GAP-31`, `32`, `34` and `35` (`apps/desktop/src/window.rs`) and `SPEC-GAP-33` (`apps/desktop/src/updater.rs`); steps 4, 5, 7, 10, 11 and 14 and DSK-UP-9, DSK-UP-12 to DSK-UP-14 and DSK-REC-13 above; `McpFront.md` (`McpFrontConfig.installation_id`, `ORDIN_MCP_ENDPOINT`); `HookRelay.md` (`HookRelayConfig.installation_id`, `ORDIN_HOOK_ENDPOINT`); `OrdinHook.md` (a hook denies on any cause); `ApprovalService.md` (`decided_on`: an empty computer name is stored as `unknown`, 80 characters); `RemoteApi.md` (`PairRequest.computer_name` 1–63 characters, `DESK_NAME_INVALID`); `JournalAndArtefactStore.md` (`recover_external_actions` runs "before any session resumes", `support_export`, `SupportBundle`); `OrganisationProfile.md` (`installer.json`, `build_defaults`: "the branded build may compile the same three values in as defaults"); `cross-cutting/Configuration.md` 2D-2b; `CatalogueService.md` (`CatalogueConfig.bundled_catalogue`, CAT-REC-1); `ScreensModule.md` SM-CU-3 (the Settings sections and the commands each reads); `cross-cutting/ErrorHandling.md` (`CRED_BROWSER_FAILED`); `SetupAssistant.md` SET-UP-4, SET-UP-8 (`setup_whats_new`, `whats_new`); `ServerConnector.md` (`display_name`, `on_wake`); `SessionSupervisor.md` (`get`, `on_wake`, `running_local_sessions`); `CommandBridge.md` (`subscribe_all`); `ServiceApi.md`, amendment January 2026 — *Commands the screens wait on* (SVC-SCR-1 to SVC-SCR-6), Draft with this one.

### Resolution

| Item | Outcome | Assertion |
|---|---|---|
| `SPEC-GAP-31` | Resolved as the user accepted | DSK-REC-33 |
| `SPEC-GAP-32` | Resolved as the user accepted | DSK-REC-34 |
| `SPEC-GAP-35` | Resolved as the user accepted | DSK-REC-35 |
| `SPEC-GAP-28` | Resolved from the Journal's ordering rule | DSK-REC-36 |
| `SPEC-GAP-29` | Resolved from DSK-UP-9 and ScreensModule.md SM-CU-3 | DSK-REC-37 |
| `SPEC-GAP-30` | Resolved with an existing code of the same meaning | DSK-REC-38 |
| `SPEC-GAP-34` | Resolved from DSK-REC-13 and OrganisationProfile.md `build_defaults`; the feed address waits on the release build's inputs | DSK-REC-39 |
| `SPEC-GAP-33` | **Phase 12, not settled here.** The update public key is the maintainer's key, committed with the release build's inputs; until then `tauri.conf.json` carries an empty key, every download fails its signature check (`DSK_UPDATE_INSTALL_FAILED`) and nothing unsigned is installed. No key is named in this spec | — |
| FU-138, program side | The program's delegations of the new catalogue rows | DSK-REC-40 |
| `diagnostics.support_export` | Resolved from the Journal's row and `files.save_export` | DSK-REC-41 |
| `files.save_copy` | Already specified for a `place_file` item (*Commands this program answers itself*, step 5); the other two kinds of item are open | OQ-REC-10 |
| `identity.relay_sign_in` | Already specified (step 14); nothing to add. The owner methods it calls are the Server Connector's and Identity and Credentials' as step 14 states | — |
| Notifier, WakeWatcher | Already specified (steps 7 and 11); the owner methods they call are verified: `CommandBridge::subscribe_all`, `ServerConnector::display_name`, `SessionSupervisor::get`, `WorkspaceService::get`, `SessionSupervisor::on_wake`, `ServerConnector::on_wake` | — |
| `workspaces.tree` | Open | ServiceApi.md OQ-SCR-2 |
| DSK-UP-14 | Open: the Setup Assistant records no version as started | OQ-REC-11 |

### Internal Logic (additions)

**DSK-REC-33 `installation_id` (`SPEC-GAP-31`).** At step 3, before row 3.20, the program takes one new UUIDv7 for this process. Row 3.20 is given it as `McpFrontConfig.installation_id` and row 3.23 as `HookRelayConfig.installation_id`, in its 36-character lowercase hyphenated form, so both Windows pipe names (`\\.\pipe\ordin-mcp-<installation id>`, `\\.\pipe\ordin-hook-<installation id>`) carry the same value. It is not written to any file and is taken again at every start, so **the pipe names change at every start, and nothing relies on their being stable across starts**: each harness session receives its endpoints in its environment when it starts (`ORDIN_MCP_ENDPOINT`, McpFront.md; `ORDIN_HOOK_ENDPOINT`, HookRelay.md), sessions end at quit (step 10.3), and a run resumed after a restart is started again with the new endpoints. A harness process left behind by an earlier process cannot reach the new pipes; its hook denies, as a hook does on any cause (OrdinHook.md). On macOS the endpoints are socket paths under `<app data root>/run/`, and the value is unused there.

**DSK-REC-34 The computer's name (`SPEC-GAP-32`).** `ApprovalConfig.local_computer_name` (row 3.4) and `ConnectorConfig.computer_name` (row 3.13) are the operating system's device name (`whoami::devicename()`), read once at step 3. The program removes control characters, trims spaces at both ends and cuts the result to 63 characters at a character boundary, so it passes the Remote API's check of `PairRequest.computer_name` (1–63 characters). When the read fails or the result is empty, the value is `unknown`, the word the Approval Service already stores for an empty name. A name changed in the operating system while ordin runs takes effect at the next start.

**DSK-REC-35 The macOS menu's Quit asks first (`SPEC-GAP-35`; amends step 10).** On macOS the application menu's Quit is a menu item the program builds itself, labelled `desktop-menu-quit` with the platform's quit shortcut (⌘Q), in place of the predefined Quit item; every other item of the default application menu stays. Choosing it runs step 10 with the trigger *menu Quit*, which step 10.2 treats as it treats a closed window: when `session_supervisor.running_local_sessions() > 0`, it emits `ordin://quit-requested { running_sessions }` and stops, and the screens ask as for a closed window. With the predefined item gone, `RunEvent::ExitRequested` without an exit code and without a quit the program asked for is the operating system ending the session, and step 10.5 runs without asking, as now. Windows has no application menu; closing the window (step 6) is its quit. New message (en-GB, `desktop.ftl`, *chosen here*): `desktop-menu-quit` = "Quit { $product }", `$product` the brand's product name.

**DSK-REC-36 A failure in step 4 (`SPEC-GAP-28`; amends step 4).** Step 4 stated the outcome only for `reload_rule_sets` (logged, start continues) and `hook_relay.start()` (start stops). The rest:
1. `journal.recover_external_actions()` failing stops start with `StartError::ModuleFailed { module: "ordin-journal", code }` and step 9 runs: the Journal requires it to run "before any session resumes" (JournalAndArtefactStore.md, Internal Logic 3), and `session_supervisor.recover()` resumes sessions later in this step.
2. Every other call of step 4 that returns an error (`set_company_max_days`, `agent_folders.recover`, `session_supervisor.recover`, `crew_orchestrator.recover`, and the `start` of each service) is logged `background_step_failed` at `warn` with the module and its code, never a path or a value, and start continues with the next call. The module whose call failed answers its own commands with its own codes; the Setup Assistant's start checks run as before.

**DSK-REC-37 Settings sections the program does not hold (`SPEC-GAP-29`; amends the `settings.get` and `settings.change` rows).** `settings.get` answers `notifications` and `display` from `preferences.json`, and `about` as DSK-UP-9 states. For every other `SettingsSection` it answers `SettingsView { section, settings: [] }`, as DSK-UP-9 does for `about`: the screens read and change those sections through their owners' commands (ScreensModule.md SM-CU-3, which names the command of each section), and the program routes no key of theirs. `settings.change` accepts only the keys of the table of *Commands this program answers itself*; any other key that passes the `ChangeSetting.key` pattern → `SVC_INPUT_INVALID`, nothing written (DSK-REC-48: no `at` in this build).

**DSK-REC-38 The operating system refuses to open a link (`SPEC-GAP-30`).** When `UrlOpener` or `BrowserOpener` fails on a link the program has checked — `app.open_slack_link` after its pattern check, and step 14.3 after its `authorisation_url` check — the command answers `CRED_BROWSER_FAILED` (retryable, action `try_again`), the existing code whose meaning is "System browser could not be opened" (cross-cutting/ErrorHandling.md), with its registered sentence. The link is logged nowhere; the log line is `link_open_failed` *(chosen here)* with the command name.

**DSK-REC-39 What the bundle holds and the build's installer values (`SPEC-GAP-34`).** *(The first sentence's source of the catalogue's files is amended by DSK-REC-39a below, 2026-01-10.)* `CatalogueConfig.bundled_catalogue` (row 3.36) is the `catalogue` folder of Tauri's resource directory, as DSK-REC-13 places the models in its `models` folder; the release build puts the catalogue's files there (CatalogueService.md CAT-REC-1 states what it must hold). `CoreInputs.installer_defaults`, the Organisation Profile's `build_defaults`, are compiled into the program, as OrganisationProfile.md allows ("the branded build may compile the same three values in as defaults"); `<app data root>/installer.json`, when present, still wins. The open-source build compiles `profile_url` absent, `profile_signing_keys` empty and `update_feed_url` = the project's release feed address (Configuration.md 2D-2b as amended by ADR-114). That address is fixed with the release build's inputs (OA-6); until then the build compiles it absent, and the Setup Assistant checks no feed.

**DSK-REC-40 The program's delegations of the new rows (FU-138; ServiceApi.md SVC-SCR-1 to SVC-SCR-5).** Each row is answered by the owner named in its row, with the module's code passed through as step 5 states:
1. `files.read_version`: `FileService::read_version(file_version_id)`, then the preview built from `FileBytes` as SVC-SCR-1 states; `FileBytes.bytes` is dropped after the preview is built and never logged.
2. `memory.held_writes`, `memory.keep_held`, `memory.discard_held`: the Memory Service's methods with `MemoryPlace::Workspace(workspace_id)`; `memory.citations_to_confirm` and `memory.confirm_citation` with `workspace_id`. `keep_held`'s `RememberReceipt` is dropped and `{}` returned.
3. `artefacts.get`: `Journal::artefact(artefact_id)`, `None` answered as `{}`.
4. `projects.get`: `ProjectService::project(project_id)`.
5. `workspaces.working_copy_items` and `workspaces.read_item`: `WorkspaceService::working_copy_items(source_link_id)` and `WorkspaceService::read_item(source_link_id, origin_path)`.

**DSK-REC-41 `diagnostics.support_export`.** The program calls `Journal::support_export(run_ids)`; a Journal error is passed through (`JRNL_EXPORT_INVALID` for no run or more than 20). It then opens the operating system's save dialog (`tauri_plugin_dialog`) with `SupportBundle.suggested_file_name` proposed, writes `bytes` atomically to the chosen path (temporary file in the same folder, then rename) and returns `SavedCopy { saved_path }`, as `files.save_export` does. Cancelled → `DSK_DIALOG_CANCELLED`, nothing written; a write failure → `DSK_SAVE_COPY_FAILED` (retryable). The bytes are never logged; `SupportBundle.truncated` is logged with the line `support_export_saved` *(chosen here)*. The save dialog waits for the person, so the row's budget is `WaitsForPerson`, as `files.save_copy`'s is (ServiceApi.md SVC-R1-10; amended January 2026 (user decision; Approved by the user January 2026)). Before the save dialog the program adds the last 3 days of its log files to the Journal's bytes (DSK-LOG-5; amended January 2026 (user decision; Approved by the user January 2026)).

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `journal.recover_external_actions()` fails at start (DSK-REC-36) | start stops; step 9 | `DSK_START_FAILED` (`StartError::ModuleFailed { module: "ordin-journal" }`) | the start-failed message box |
| Any other step-4 call fails (DSK-REC-36) | logged `warn`; start continues | the module's code | — |
| `settings.change` names a key the program does not hold (DSK-REC-37) | `Err(permanent)` | `SVC_INPUT_INVALID` | `{"kind":"error","error":{"code":"SVC_INPUT_INVALID",…}}` |
| The operating system refuses to open a checked link (DSK-REC-38) | `Err(retryable)` | `CRED_BROWSER_FAILED` | `{"kind":"error","error":{"code":"CRED_BROWSER_FAILED","action":"try_again",…}}` |
| The person cancels the support export's save dialog (DSK-REC-41) | `Err(permanent)`, no action | `DSK_DIALOG_CANCELLED` | `{"kind":"error","error":{"code":"DSK_DIALOG_CANCELLED","message":"Nothing was saved."}}` |
| The support export cannot be written (DSK-REC-41) | `Err(retryable)` | `DSK_SAVE_COPY_FAILED` | — |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — both pipes carry one id that changes at each start (DSK-REC-33)
    Given ordin started on Windows and the MCP Front's pipe is \\.\pipe\ordin-mcp-<X>
    Then the Hook Relay's pipe is \\.\pipe\ordin-hook-<X>
    When ordin quits and starts again
    Then both pipes carry a new id Y different from X, and no file holds X or Y

  Scenario: Edge case — an empty device name (DSK-REC-34)
    Given the operating system's device name is empty
    When ordin starts
    Then ApprovalConfig.local_computer_name and ConnectorConfig.computer_name are "unknown"

  Scenario: Edge case — a long device name is cut for pairing (DSK-REC-34)
    Given the device name is 80 characters long
    When the person pairs with a server
    Then PairRequest.computer_name holds its first 63 characters

  Scenario: Happy path — ⌘Q with a session running asks first (DSK-REC-35)
    Given ordin runs on macOS with one local session running
    When the person presses ⌘Q
    Then ordin://quit-requested is emitted with running_sessions 1 and nothing stops

  Scenario: Edge case — the computer shutting down does not ask (DSK-REC-35)
    Given ordin runs on macOS with one local session running
    When the operating system ends the session
    Then step 10.3 runs with QuitReason::OperatingSystemEnding and no question is asked

  Scenario: Error path — the journal's recovery fails at start (DSK-REC-36)
    Given recover_external_actions fails with JRNL_DATASTORE_UNAVAILABLE
    When ordin starts
    Then start stops with StartError::ModuleFailed for "ordin-journal" and no session is resumed

  Scenario: Edge case — another step-4 call fails (DSK-REC-36)
    Given crew_orchestrator.recover fails
    When ordin starts
    Then background_step_failed is logged with module and code and the window is shown

  Scenario: Happy path — a section the program does not hold (DSK-REC-37)
    When settings.get is called with section "using_your_plan"
    Then the result is {"section":"using_your_plan","settings":[]}

  Scenario: Error path — an unknown key (DSK-REC-37)
    When settings.change is called with key "usage.plan_mode"
    Then the reply is an error with code SVC_INPUT_INVALID and preferences.json is unchanged

  Scenario: Error path — the browser cannot be opened (DSK-REC-38)
    Given the operating system refuses to open links
    When app.open_slack_link is called with a valid Slack permalink
    Then the reply is an error with code CRED_BROWSER_FAILED

  Scenario: Happy path — a support export is saved (DSK-REC-41)
    Given the person chooses "/Users/asha/Desktop/ordin-support.jsonl" in the save dialog
    When diagnostics.support_export is called with one run id
    Then the file holds the bundle's bytes and the result is {"saved_path":"/Users/asha/Desktop/ordin-support.jsonl"}
```

### Open Questions

- **OQ-REC-10** — *resolved 2026-01-10 as recommended (a): DSK-REC-43.* *Saving a copy of a version or of an artefact (`files.save_copy`).* `SaveCopy.item` is a `ContentRef`, and the program's rule calls `files.download { place_id, path }`, which reads only a `place_file`. No command returns the bytes of a `file_version` (`files.read_version` returns a preview), and an `artefact` may lie in an agent folder, which the screens never read. Options: (a) `place_file`, and an `artefact` in a place (through `artefacts.get`, then `files.download` of its place and path), are saved; a `file_version` and an artefact in an agent folder are refused with `SVC_INPUT_INVALID`; (b) a new command returns a version's bytes so a version can be saved too. **Recommendation:** (a): it needs no new command, and no screen of this build offers **Save a copy** on an approval's version. Blocks only saving a version or an agent-folder artefact; until decided the program answers `place_file` items and refuses the other two kinds with `SVC_INPUT_INVALID`.
- **OQ-REC-11** — *resolved 2026-01-10 as recommended (a): DSK-REC-44 and SetupAssistant.md SET-REC-2.* *Seeing that an older version started (DSK-UP-14).* DSK-UP-14 compares the running version with "the last version the Setup Assistant recorded as started", but the Setup Assistant records no such version: `setup_whats_new` keeps release notes by version (SET-UP-4) and `whats_new` reads the running version's row (SET-UP-8). Options: (a) the Setup Assistant keeps the highest version started, written at each start, with a read the program calls after row 3.43; (b) the program keeps it in `preferences.json`. **Recommendation:** (a): the Setup Assistant already owns What's new, which DSK-UP-14 suppresses, and its store survives a reinstall of the same data root as `preferences.json` does. Blocks only the `update_downgrade_seen` log line and the suppression of What's new after a downgrade; nothing else waits on it.

### Gaps

None in this amendment's own rules; `SPEC-GAP-33` waits on the release build's inputs, and OQ-REC-10 and OQ-REC-11 are decisions, each with its recommendation.

## Amendment January 2026 — Resolutions of 2026-01-10 (OQ-REC-10, OQ-REC-11, DSK-REC-39, `workspaces.tree`)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Every section above stays in force except where a rule below names what it replaces. Counterparts: `components/ServiceApi.md`, amendment January 2026 — *Resolutions of OQ-SCR-1 and OQ-SCR-2* (SVC-SCR-8); `components/SetupAssistant.md`, amendment January 2026 — *The highest version started* (SET-REC-2).
>
> Evidence read: OQ-REC-10, OQ-REC-11, DSK-REC-39 and DSK-UP-14 above; the `files.save_copy` row of *Public Interface* and step 5; `ServiceApi.md` (`WorkspaceTree`, `RunningItem`, SVC-SCR-3 `artefacts.get`, OQ-SCR-2); `WorkspaceService.md` (`get(id) -> Workspace`, `places(id) -> Vec<Place>`); `PackageCatalogue.md` (`list(workspace_id: Option<WorkspaceId>) -> Vec<InstalledAgent>`, `InstalledAgent.instance`); `OrdinHost.md` (`workflows(workspace_id) -> Vec<WorkflowInputsView>`, `WorkflowInputsView.workflow_id`); `JournalAndArtefactStore.md` (`list_dashboards(workspace_id, cursor) -> Page<Dashboard>`, `running_runs() -> Vec<RunIndexEntry>`); 2C domain `Page { items, next_cursor }`; `KnowledgeService.md` (`list_pages(workspace_id, filter) -> PageListing { this_workspace, from_agents }`); `MemoryService.md` (`count_workspace_facts(workspace_id) -> u32`); `HomeAndSearch.md` (`needs_you(filter) -> Vec<HomeRow>`, `JournalReader::running`, `RunSummary`); `CatalogueService.md` CAT-REC-1; `SetupAssistant.md` SET-UP-8.

### Internal Logic (additions)

**DSK-REC-39a The bundled catalogue is a pinned commit of the bundled agents repository (amends DSK-REC-39).** The `catalogue` folder of Tauri's resource directory (`CatalogueConfig.bundled_catalogue`, row 3.36) holds the files of one pinned commit of the **bundled agents repository**, in Ordin's own format: the WorkspaceProfiles and parts with their `urn:ordin:` identifiers, as CAT-REC-1 requires them. The commit is named by its full hash in the release's build input and is frozen into the application when the release is built: the release build copies the files of that commit into the folder, and nothing at run time fetches, updates or changes them; a new commit reaches people only in a new release. Identifiers are used exactly as the repository writes them: no identifier is rewritten at build time or at run time, and neither the build nor the program reads, converts from, or depends on the earlier implementation or its repository. The repository's name and host, and the file of the build input that names the commit, are not yet decided (OQ-REC-13); until they are, the release build has no commit to copy, the folder holds what the build finds there, and a missing bundled agent is answered `PKG_BUNDLED_AGENT_MISSING` (CAT-REC-1). The Catalogue Service reads the folder again when its contents change as well as when the version changes (CatalogueService.md §A step 1), so a build of the same version with other bundled agents, such as `make local-build` with `AGENTS=worktree`, reaches the person at its first start; the build passes no commit into the program (amended January 2026 (user decision; Approved by the user January 2026)). The rest of DSK-REC-39 (the installer values) is unchanged.

**DSK-REC-42 The sources of `workspaces.tree` (closes ServiceApi.md OQ-SCR-2 by the short pass).** The program composes `WorkspaceTree` for `workspace_id` from one owner source per field, with the conversion stated:

| Field | Source | Conversion |
|---|---|---|
| `workspace` | `WorkspaceService::get(workspace_id)` | unchanged |
| `places` | `WorkspaceService::places(workspace_id)` | unchanged, in the order returned |
| `agents` | `PackageCatalogue::list(Some(workspace_id))` | `InstalledAgent.instance` of each entry, in the order returned |
| `workflows` | `OrdinHost::workflows(workspace_id)` | `WorkflowInputsView.workflow_id` of each entry, in the order returned (every workflow with a saved version) |
| `dashboards` | `Journal::list_dashboards(workspace_id, cursor)` | every page in turn, from `cursor` `None` until `next_cursor` is null; `Dashboard.id` of each item, in the order returned |
| `running` | none verified (OQ-REC-12) | the empty list until OQ-REC-12 is settled |
| `knowledge_pages` | `KnowledgeService::list_pages(workspace_id, None)` | the number of entries of `this_workspace` (pages from agents are not counted) |
| `memory_facts` | `MemoryService::count_workspace_facts(workspace_id)` | unchanged |
| `waiting_for_person` | `HomeSearch::needs_you(Some(workspace_id))` | the number of rows |

1. `workspace` and `places` are read first; an error of either is the command's reply, with the owner's code (step 5), and nothing else is read.
2. The other sources are read after them, at the same time. *(Chosen here: the tree is drawn on every `workspace_changed`, so one unavailable group must not hide the workspace.)* An error of one of them, or a source module this build does not construct, gives that field the empty list or 0; an error is logged `tree_source_failed` *(chosen here)* at `warn` with the module and its code, never a name or a path.

**DSK-REC-43 Saving a copy of each kind of item (closes OQ-REC-10 as recommended, option (a); amends the `files.save_copy` row).** By the kind of `SaveCopy.item`:
1. `place_file { place_id, relative_path }`: as the row states — the save dialog named after the last segment of `relative_path`, `files.download { place_id, path: relative_path }` on `backend`, the atomic write.
2. `artefact { artefact_id }`: `artefacts.get { artefact_id }` on `backend` first (ServiceApi.md SVC-SCR-3). An artefact whose `location` is a place is saved as 1 with that place and `Artefact.path`. No artefact with that id, or an artefact whose `location` is an agent folder → `SVC_INPUT_INVALID`, no dialog opened, nothing written.
3. `file_version { file_version_id }` → `SVC_INPUT_INVALID`, no dialog opened, nothing written. In this build no screen offers **Save a copy** on an exact version, and an agent folder is never read by the screens (FileService.md *Security*).
4. An error reply of `artefacts.get` or `files.download` is the command's reply, with the owner's code; the dialog's and the write's outcomes are as the row and the Error Table state.

**DSK-REC-44 Seeing that an older version started (closes OQ-REC-11 as recommended, option (a); amends DSK-UP-14).** After row 3.43 the program calls `SetupAssistant::record_started()` (SetupAssistant.md SET-REC-2), which answers the highest version recorded as started before this start and records the running version when it is higher. When the answer is a version higher than the running version (`CARGO_PKG_VERSION`) by semantic-version order, the program logs `update_downgrade_seen` at `warn` with `running` and `last_started`, as DSK-UP-14 states. **What's new** is suppressed by the Setup Assistant itself (SET-REC-2). A `SetupError` is logged `background_step_failed` at `warn` with the module and its code, and start continues (DSK-REC-36.2).

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `files.save_copy` of a `file_version`, of an artefact in an agent folder, or of an artefact no row has (DSK-REC-43) | `Err(permanent)` | `SVC_INPUT_INVALID` | `{"kind":"error","error":{"code":"SVC_INPUT_INVALID",…}}` |
| `workspaces.tree` for a workspace the Workspace Service does not hold (DSK-REC-42.1) | `Err` | the Workspace Service's code | its sentence |
| One of the later sources of `workspaces.tree` fails (DSK-REC-42.2) | `Ok`, the field empty or 0 | — (logged `tree_source_failed`) | — |
| `record_started` fails at start (DSK-REC-44) | logged `warn`; start continues | the Setup Assistant's code | — |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the tree counts a workspace's parts (DSK-REC-42)
    Given workspace W has 2 places, 1 installed agent, 1 saved workflow, 3 knowledge pages and 4 remembered facts
    When workspaces.tree is called for W
    Then places has 2 entries, agents 1, workflows 1, knowledge_pages is 3, memory_facts is 4 and running is empty

  Scenario: Edge case — one source fails and the tree is still drawn (DSK-REC-42)
    Given the Memory Service answers MEM_STORE_UNAVAILABLE
    When workspaces.tree is called for W
    Then the result has memory_facts 0 and tree_source_failed is logged with the module and the code

  Scenario: Error path — saving a copy of an exact version is refused (DSK-REC-43)
    When files.save_copy is called with item {"kind":"file_version","file_version_id":"0192a1d0-0000-7000-8000-000000000001"}
    Then the reply is an error with code SVC_INPUT_INVALID and no save dialog opens

  Scenario: Happy path — an artefact in a place is saved (DSK-REC-43)
    Given artefact A lies in place P at "out/summary.md"
    When files.save_copy is called with item artefact A and the person chooses a path
    Then files.download is called with P and "out/summary.md" and the file is written

  Scenario: Edge case — an older version starts (DSK-REC-44)
    Given the highest version recorded as started is "0.3.0"
    When version "0.2.0" starts
    Then update_downgrade_seen is logged with running "0.2.0" and last_started "0.3.0"
```

### Open Questions

- **OQ-REC-12** — *resolved 2026-01-10 as recommended (a): DSK-REC-45.* *The source of `WorkspaceTree.running` (DSK-REC-42).* No single verified source gives one workspace's running runs and crew runs with a title and a typed state: `Journal::running_runs()` covers every workspace and holds no crew run, `RunIndexEntry` carries no workspace or title, and the program's own `JournalReader::running()` (HomeAndSearch.md) titles each run from the Session Supervisor and the Crew Orchestrator but gives `RunSummary.state` as text, not `RunState` or `CrewRunState`. Options: (a) filter the program's `JournalReader::running()` by `workspace_id` and map `state` back to the typed state; (b) a new owner read of one workspace's running items. **Recommendation:** (a): it reuses what Home already draws and needs no new owner method, at the cost of one text-to-state mapping in the program. Blocks only the tree's *Running now* group under a workspace; Home's *Running now* is not affected. Until decided, `running` is the empty list.
- **OQ-REC-13** *(resolved 2026-01-10; amended 2026-01-28, user decision: the bundled agents come from `gatblau/ordin-agents` on GitHub, `https://github.com/gatblau/ordin-agents`; the memory model files and the releases come from `gatblau/ordin-releases`, see OQ-REL-3. The pinned commit's hash is still to be written in `release/release.conf`.)* *The bundled agents repository's name and host (DSK-REC-39a).* The repository from which the release build copies the pinned commit has no decided name or host, and so the build input that names the commit has no file yet. Options: a name and host the maintainer chooses. **Recommendation:** none can be derived from the evidence; it is the maintainer's choice. Blocks only the release build's copy of the bundled agents; a build without it ships without them, and the Package Catalogue answers `PKG_BUNDLED_AGENT_MISSING`.

### Gaps

None in this amendment's own rules; OQ-REC-12 and OQ-REC-13 are decisions.

## Amendment January 2026 — The tree's running items (closes OQ-REC-12)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations", option (a) of OQ-REC-12). Every rule above stays in force except where DSK-REC-45 names what it replaces. Counterpart: `components/ServiceApi.md`, SVC-SCR-8.
>
> Evidence read: OQ-REC-12 and DSK-REC-42 above; `HomeAndSearch.md` `JournalReader::running() -> Result<Vec<RunSummary>, HomeSourceError>`, `RunSummary { workspace_id, run_id: Option<RunId>, crew_run_id: Option<CrewRunId>, title, state: String, at, can_run_again }`; the program's `JournalReader` composite of row 3.42 (DSK-REC-19), which gives `state` as the serde name of the run's `RunState`; `ServiceApi.md` `RunningItem::Run { workspace_id, run_id, title, state: RunState }` and `RunningItem::CrewRun { workspace_id, crew_run_id, title, state: CrewRunState }`; `CrewOrchestrator.md` `get(crew_run_id) -> Result<(CrewRun, Vec<Round>), CrewError>`, `CrewRun.state`.

### Internal Logic (additions)

**DSK-REC-45 `WorkspaceTree.running` (closes OQ-REC-12 as recommended, option (a); replaces the `running` row of DSK-REC-42).**
1. **Source.** The program's `JournalReader::running()` of row 3.42, the list Home's *Running now* is drawn from, keeping the summaries whose `workspace_id` is the workspace's, in the order returned.
2. **Runs.** A summary with a `run_id` and no `crew_run_id` becomes `RunningItem::Run { workspace_id, run_id, title, state }`, `title` unchanged and `state` the `RunState` whose serde name is the summary's `state` text.
3. **Crew runs.** The summaries that carry a `crew_run_id` become one `RunningItem::CrewRun { workspace_id, crew_run_id, title, state }` per distinct `crew_run_id`, at the place of its first summary: `title` is that summary's `title`, and `state` is `CrewRun.state` read with `CrewOrchestrator::get(crew_run_id)`. When that read fails, or the Crew Orchestrator is not built, each of the crew's summaries that has a `run_id` becomes a `Run` item as in 2.
4. **Left out.** A summary whose `state` text names no `RunState` (for example `replied_in_slack`), or that has neither a usable `run_id` nor a crew run, is left out and logged `tree_running_item_left_out` at `debug` *(chosen here)* with the workspace id, never the title.
5. **Failure.** When the reader is not built, or `running()` fails with `HomeSourceError`, `running` is the empty list and `tree_source_failed` is logged as DSK-REC-42.2 states.
6. A crew run is listed while at least one of its rounds is running; the tree does not list a crew run between two rounds, as Home's *Running now* does not.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the tree lists what is running in the workspace (DSK-REC-45)
    Given workspace W has run R "Kestrel buyer" running and workspace X has run S running
    When workspaces.tree is called for W
    Then running holds one Run item for R with state "running" and none for S

  Scenario: Happy path — a crew run is one item (DSK-REC-45)
    Given crew run C of W has two rounds running
    When workspaces.tree is called for W
    Then running holds one CrewRun item for C whose state is the crew run's state

  Scenario: Edge case — a state the tree cannot type (DSK-REC-45)
    Given Home's running list holds a summary of W whose state is "replied_in_slack"
    When workspaces.tree is called for W
    Then that summary is not in running and tree_running_item_left_out is logged

  Scenario: Error path — the running list is unavailable (DSK-REC-45)
    Given JournalReader::running fails
    When workspaces.tree is called for W
    Then running is empty and the other fields are filled
```

### Gaps

None.

## Amendment January 2026 — The release build's inputs

> **Status:** written 2026-01-10 under the user's standing instruction ("proceed with recommendations"); the user has not read it. Every section above stays in force except where DSK-REC-46 names what it replaces. Counterpart: `phase-5-playbook.md`, §6.5 amendment January 2026 (PB-REL-1 to PB-REL-10, OQ-REL-1 to OQ-REL-3).
>
> Evidence read: DSK-REC-39 and DSK-REC-39a above; `apps/desktop/src/window.rs` (`installer_defaults: Default::default()`); `apps/desktop/src/wiring.rs` (`option_env!("GITHUB_APP_CLIENT_ID")`, the existing build-time input); `crates/ordin-organisation-profile/src/installer.rs` (`InstallerSettings.update_feed_url`, checked to be https when read from a file).

### Internal Logic (additions)

**DSK-REC-46 The feed address is a build input (replaces the last two sentences of DSK-REC-39, and closes its "absent until Phase 12" for the build that has one).** `CoreInputs.installer_defaults.update_feed_url` is the value of the build-time environment variable `ORDIN_UPDATE_FEED_URL` when it is set at compile time and begins with `https://`, and absent otherwise; `profile_url` stays absent and `profile_signing_keys` empty. `release-build` sets the variable to the real channel's feed address (PB-REL-5) when that is set. An `installer.json` in the data root still wins (DSK-REC-39), which is how a build is pointed at the test feed.

**DSK-REC-47 The update public key and the bundle's extra resources (closes `SPEC-GAP-33` as far as the program is concerned).** The program holds no key of its own. `tauri.conf.json` keeps an empty `plugins.updater.pubkey`; the release build supplies the key and the `catalogue` resource through `--config` (PB-REL-4, PB-REL-6, PB-REL-7), so a build made any other way refuses every downloaded update with `DSK_UPDATE_INSTALL_FAILED`, as now. `SPEC-GAP-33` stays marked in `updater.rs` until the maintainer commits `release/ordin-update.pub`.

### Open Questions

- **OQ-REC-13** is resolved (2026-01-10; amended 2026-01-28): `gatblau/ordin-agents` on GitHub for the bundled agents; PB-REL-7 gives the build input its names, `RELEASE_BUNDLED_AGENTS_SOURCE` and `RELEASE_BUNDLED_AGENTS_COMMIT`, in `release/release.conf`.
- **OQ-REL-1 to OQ-REL-3** are in `phase-5-playbook.md`; OQ-REL-1 and OQ-REL-2 are resolved there (2026-01-10); OQ-REL-3's source is decided (2026-01-10) and its file names and digests stay open.

### Gaps

`SPEC-GAP-33` waits on the maintainer's key.

## Amendment January 2026 — Phase 13A: the refusals without `at`, and four questions settled

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Every section above stays in force except where DSK-REC-48 names what it replaces. Counterpart: `components/ServiceApi.md`, amendment January 2026 — *Answering the rows the program refused* (SVC-R1-1 to SVC-R1-3).
>
> Evidence read: DSK-REC-37 and DSK-REC-43 above; `ServiceError` in `ordin-service-api` (code, message, action, reference; no `at`); OQ-REC-1, OQ-REC-3, OQ-REC-4 and OQ-REC-8 above.

- **DSK-REC-48 A refusal the program makes itself carries no `at` in this build (closes `SPEC-GAP-36`).** `ServiceError` has the fields `code`, `message`, `action` and `reference`. The pointers `at = /input/key` of DSK-REC-37 and `at = /input/item` of DSK-REC-43 are not sent: the refusal is `SVC_INPUT_INVALID` with its sentence, and nothing is written or opened, as those rules state. Adding `at` to `ServiceError` would change the Service API contract and every generated type; it is a change for a later build, not this one.

- **OQ-REC-1 resolved as recommended (a), scheduled with the switches that need it.** The Organisation Profile gains the synchronous `remote_agent_route(id)` accessor and the program maps `Http` to `AdapterKind::HostedAgent` and `Slack` to `AdapterKind::SlackThread`, in the build that has the `hosted-agents` or `slack` switch on. a build with neither switch on registers no remote adapter, so nothing in it changes; `SPEC-GAP-5` and `SPEC-GAP-8` stay marked with this reason.
- **OQ-REC-3 resolved as recommended (a).** The desktop forwards no File Service change to `MemoryService::source_changed` for a place file until a canonical `origin_ref` exists. `SPEC-GAP-14` stays marked with this reason.
- **OQ-REC-4 resolved as recommended (a), with (b) accepted for this build.** The target is that `SessionSetup` carries `claude_path` from `ready_harness().location`. This build keeps (b), the path found before start and passed in `CoreInputs.claude_path`, because the change to `ClaudeLocalAdapter.md` waits. `SPEC-GAP-15` stays marked with this reason.
- **OQ-REC-8 resolved as (b) for this build.** Packages do not start their own programs on the desktop; an agent's tool servers are ordin's own. DSK-REC-28 holds. Before any agent repository publishes an agent that depends on a server its package declares, this question is asked again.

### Gaps

None added. `SPEC-GAP-5`, `SPEC-GAP-8`, `SPEC-GAP-14` and `SPEC-GAP-15` stay marked in code with the reasons above; `SPEC-GAP-33` waits on the maintainer's key.

## Amendment January 2026 — The file picker for a certificate (D-2)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Counterpart: `components/IdentityAndCredentials.md`, the `identity.import_client_certificate` row.

- **DSK-R1-1** The window shell gains `pick_file(title)`: the operating system's file picker on the main window, `Ok(None)` when the person cancels, `DSK_PICKER_UNAVAILABLE` when it cannot open (as `pick_folder`). `identity.import_client_certificate` calls it twice, first for the certificate file, then for the key file, with the titles "Choose the certificate file" and "Choose the key file". Either one cancelled → `{ certificate: absent }`, nothing stored. Each file is read in memory (at most 64 KiB, else `CRED_CERTIFICATE_INVALID`); the key's bytes are zeroised after the call and no path reaches the screens. The name is the profile's `server.client_certificate_name`; absent → `CRED_PROVIDER_NOT_CONFIGURED`. Identity's `import_client_certificate(name, certificate_pem, key_pem)` does the rest.

```gherkin
  Scenario: Happy path — a certificate is imported (DSK-R1-1)
    Given the profile names a client certificate name
    When the person picks a certificate file and then a key file
    Then identity.import_client_certificate returns the certificate's name, subject and end date

  Scenario: Edge case — the second picker is cancelled (DSK-R1-1)
    When the person picks the certificate file and cancels the key picker
    Then the result has no certificate and nothing is stored
```

## Amendment 2026-01-28 — The memory models download on first use (resolves G-MD-1 to G-MD-3)

> **Status:** Approved by the user 2026-01-28 ("proceed with recommendations"). Source: the user's decisions of 28 January 2026 — the models are fetched the first time memory is used, not bundled; they come from one zip on the releases repository that carries both licences; the person is asked first, in one plain sentence naming the size (174 MB) and the host, with **Download** and **Not now**, and **Not now** leaves memory unavailable and asks again the next time memory is used. It changes how ADR-033's models reach the computer; the HLD amendment of ADR-033 is `phase-5-playbook.md` OQ-REL-4. Every section above stays in force except where an item below names what it replaces. Counterparts: `phase-5-playbook.md` §6.5 amendment 2026-01-28 (PB-REL-11 to PB-REL-14); `components/MemoryService.md` amendment 2026-01-28 (MEM-MD-1 to MEM-MD-4); `components/ServiceApi.md` amendment 2026-01-28 (SVC-MD-1 to SVC-MD-3); `components/ScreensModule.md` amendment 2026-01-28 (SM-MD-1 to SM-MD-6).
>
> Evidence read: DSK-REC-13 and DSK-REC-46 above; *Application data root (desktop)* above (`app_local_data_dir()`, macOS `~/Library/Application Support/<identifier>`, Windows `%LOCALAPPDATA%\<identifier>`); `apps/desktop/src/window.rs` (`.app_local_data_dir()`; `models_dir: resources.join("models")`; `model_manifest: Default::default()`; `installer_defaults_from(option_env!("ORDIN_UPDATE_FEED_URL"))`); `apps/desktop/src/wiring.rs` (`CoreInputs.models_dir`, `CoreInputs.model_manifest`, `CoreInputs.http`); `apps/desktop/src/http.rs` (`outbound_client()`: connect timeout 10 s, `redirect_policy()` allowing at most 3 redirects and only to `https`); `crates/ordin-memory-service/src/config.rs` (`ModelManifest`, `ModelFileDigest { path, sha256 }`); `crates/ordin-memory-service/src/providers.rs` (`check_models`, `bundled_files()`, `read_checked` refusing a file whose digest differs at load); `crates/ordin-memory-service/src/service.rs` (`models_missing`, `model_gate` answering `MEM_MODEL_UNAVAILABLE`); `release/memory-models/files.txt`; workspace dependencies `zip` 9.0.5 (`deflate`), `sha2`, `rand`, `reqwest` (`rustls`).

### Internal Logic (additions and replacements)

**DSK-MD-1 Where the files live (replaces the first sentence of DSK-REC-13; resolves G-MD-1; chosen here).** `CoreInputs.models_dir` is `<data root>/models`: macOS `~/Library/Application Support/<identifier>/models`, Windows `%LOCALAPPDATA%\<identifier>\models`, inside the person's own account and writable without administrator rights on both a per-person and a whole-computer install (ADR-072). It holds `embedding/` and `reranker/` with the ten files of the manifest, and `LICENSES.md`. The download's working folder is `<data root>/models.download`. Both are removed with the data root when ordin is removed. *Reason:* the bundle's resource folder cannot be written on a whole-computer install, and the data root is the one folder the program already owns and creates at step 2.

**DSK-MD-2 The models input (replaces the second and third sentences of DSK-REC-13; chosen here).** The program reads four build-time environment variables, as DSK-REC-46 reads `ORDIN_UPDATE_FEED_URL`: `ORDIN_MEMORY_MODELS_URL`, `ORDIN_MEMORY_MODELS_SHA256`, `ORDIN_MEMORY_MODELS_BYTES` and `ORDIN_MEMORY_MODELS_MANIFEST` (PB-REL-13). The input is valid when all four are set at compile time, the address begins with `https://`, the SHA-256 is 64 lowercase hexadecimal characters, the size is a whole number above 0, and the manifest is exactly ten `<path>=<sha256>` pairs separated by `,` whose paths are the ten files `check_models` checks, each once, each digest 64 lowercase hexadecimal characters. A valid input fills `CoreInputs.model_manifest` with one `ModelFileDigest { path, sha256 }` per pair and `CoreInputs.models_source` (new field) with `Some(ModelsSource { url, sha256, bytes })`. Any variable absent, or the input invalid, gives the empty `ModelManifest` and `models_source: None` ("no models input"); an invalid input is logged once at start as `memory_models.input_invalid { variable }`, naming the first failing variable and never its value. With no models input, memory behaves as before this amendment: recall and remember answer `MEM_MODEL_UNAVAILABLE`, and nothing is downloaded. *Reason:* a development or test build has no models input, so no test reaches the network, and a release always has one (PB-REL-12).

```rust
/// The zip a release build names (PB-REL-11). Program-internal; not a Service API type.
pub struct ModelsSource {
    pub url: reqwest::Url,   // https only
    pub sha256: String,      // 64 lowercase hexadecimal characters
    pub bytes: u64,          // the zip's exact size; also the download's size cap (DSK-MD-8)
}
```

**DSK-MD-3 The models' state.** The program keeps one `MemoryModelsView` (Service API type, SVC-MD-1) for the local backend, in memory only:

| `state` | Meaning |
|---|---|
| `not_in_build` | no models input (DSK-MD-2); never changes in this run |
| `missing` | the input is valid and the files are not all present with their digests; no download runs |
| `downloading` | a download runs (DSK-MD-6 to DSK-MD-8) |
| `checking` | the zip is complete and is being checked, unpacked and checked again (DSK-MD-9, DSK-MD-10) |
| `ready` | every file matches the manifest and the Memory Service has re-checked them |
| `failed` | the last download ended with the code in `failure`; the partial file is kept where DSK-MD-7 says so |

`size_bytes` is `ModelsSource.bytes` (0 for `not_in_build`); `host` is the host name of `ModelsSource.url` as written in the address (for `models-1`: `github.com`; empty for `not_in_build`); `received_bytes` is the partial file's length while `missing`, `downloading` or `failed`, `size_bytes` while `checking` or `ready`; `failure` is set only in `failed`; `ask` is DSK-MD-5's flag.

**DSK-MD-4 At start.** After row 3.28 builds the Memory Service: with no models input → `not_in_build`. Otherwise `MemoryService::models_ready()` (MEM-MD-3) true → `ready`; false → `missing`. Then, in the background, every entry of `<data root>/models.download` whose name begins with `unpack-` or `old-` is deleted, and every `*.zip.part` file whose name is not `<ModelsSource.sha256>.zip.part` is deleted (a partial of another zip is never resumed). Nothing is downloaded at start.

**DSK-MD-5 Asking the person (G-MD-3 trigger; OQ-MD-1; chosen here).** The program holds `MemoryService::models_wanted()` (MEM-MD-3) and, on each change, sets `ask = true` and emits `memory_models_changed` when all hold: `state` is `missing` or `failed`; no download runs; `ask` is false; and at least 60 seconds have passed since the person last answered **Not now** (`app.decline_memory_models`) in this run. `ask` becomes false on `app.download_memory_models`, on `app.decline_memory_models` (which also records the time) and when `state` becomes `ready`. `ask` and the time of **Not now** are not stored: after a restart the person is asked again at the next use. *Reason for the 60 seconds:* one answer of Ordin recalls several times within seconds; counting those as one use keeps **Not now** from returning at once, and a later question is the "next time memory is used" the user named.

**DSK-MD-6 The download.** `app.download_memory_models` in `missing` or `failed` starts one background task under the core's shutdown token and answers at once with the view in `downloading`; in `downloading` or `checking` it answers the current view and starts nothing; in `ready` it answers the view; in `not_in_build` it is refused `DSK_MODELS_NOT_IN_BUILD`. The task writes to `<data root>/models.download/<sha256>.zip.part` (folder created if absent). Each attempt is one `GET` of `ModelsSource.url` through `CoreInputs.http` (connect timeout 10 s, at most 3 redirects, `https` only). When the partial file holds `n > 0` bytes, the attempt sends `Range: bytes=<n>-`. The answer:
- `206` with a `Content-Range` starting at `n` → the body is appended. A `206` starting elsewhere → the partial file is emptied and the next attempt starts at 0.
- `200` → the partial file is emptied and the body written from byte 0 (the host ignored the range).
- `416` → the partial file is deleted and the next attempt starts at 0.
- `401`, `403`, `404`, `407`, `410`, `451`, a redirect refused by the redirect rule, or a certificate the TLS layer refuses → `DSK_MODELS_BLOCKED`, no retry; the partial file is kept.
- `408`, `429`, any `5xx`, a connection failure, a timeout of DSK-MD-7, or a body that ends before `size_bytes` (a short file) → a retry under DSK-MD-7.
- Any other status → `DSK_MODELS_BLOCKED`.

The view's `received_bytes` follows the file, and `memory_models_changed` is emitted at most once a second while bytes arrive, and on every change of `state`. Logs: `memory_models.download_started { resumed_from }`, `memory_models.attempt_failed { attempt, status }` (status is the HTTP status or `connect`, `timeout`, `short`), `memory_models.download_failed { code }`; never the address's query or a byte of the file.

**DSK-MD-7 Timeouts and retries (chosen here).** Every attempt has the connect timeout of 10 s, at most 30 s from the sent request to the answer's status line, and at most 30 s between two received chunks of the body; the whole download has no total limit (174 MB over a slow line). A download makes at most 6 attempts; the wait before attempt `k` (`k` ≥ 2) is a random duration, uniform between 0 and `min(30 s, 1 s × 2^(k−2))` (exponential backoff with full jitter, from `rand`). An attempt that appended at least 1 MiB resets the count to 1, so a line that drops every few minutes still finishes. When the attempts are used up: no byte of this zip ever received (the partial file is empty) and the last failure was a connection failure or a connect timeout → `DSK_MODELS_OFFLINE`; otherwise `DSK_MODELS_INTERRUPTED`. In both cases the partial file is kept, so the person's next **Download** resumes from its length.

**DSK-MD-8 The size cap.** The download never writes more than `size_bytes`. A `Content-Length` (for `200`) or a `Content-Range` total (for `206`) other than `size_bytes`, or a body that would carry the file past `size_bytes`, stops the attempt at once, deletes the partial file and ends the download with `DSK_MODELS_MISMATCH`, no retry. A write that fails with the operating system's "storage full" error (`std::io::ErrorKind::StorageFull`) ends the download with `DSK_MODELS_DISK_FULL`, keeping the partial file; any other write error ends it with `DSK_MODELS_WRITE_FAILED`, keeping the partial file. *Chosen here:* free space is not measured before the download, because the workspace carries no dependency that reads it; a full disk is found at the first write that fails, and nothing already received is lost.

**DSK-MD-9 The fingerprints.** When the partial file holds `size_bytes` bytes, `state` becomes `checking` and, on a blocking thread:
1. The SHA-256 of the whole file is computed and compared with `ModelsSource.sha256`. Different → the partial file is deleted, `DSK_MODELS_MISMATCH`. Nothing is unpacked from a zip whose fingerprint differs.
2. The zip is opened with the `zip` crate and unpacked into a new folder `<data root>/models.download/unpack-<UUIDv7>/`. Only entries whose name equals one of the ten manifest paths, and the entry `LICENSES.md`, are written; every other entry (`SHA256SUMS` among them) is skipped, so no entry name can place a file outside the folder. A manifest path missing from the zip or present twice → `DSK_MODELS_MISMATCH`.
3. The SHA-256 of each of the ten written files is compared with the manifest. Any difference → `DSK_MODELS_MISMATCH`.

A failure in 2 or 3 deletes the `unpack-` folder and the partial file. A storage-full error in 2 → `DSK_MODELS_DISK_FULL`, any other write error → `DSK_MODELS_WRITE_FAILED`; both delete the `unpack-` folder and keep the partial file, which is complete and passed step 1, so the next **Download** goes straight to `checking`.

**DSK-MD-10 Into place, and the re-check (resolves G-MD-2 on the program side).** After DSK-MD-9: when `<data root>/models` exists, it is renamed to `<data root>/models.download/old-<UUIDv7>`; the `unpack-` folder is renamed to `<data root>/models`; then the `old-` folder and the partial file are deleted. Both renames stay inside the data root's volume, so each is one operating-system rename, and a stop between them leaves either the old folder or the new one under its final name, never a half-written `models`. A rename failure → `DSK_MODELS_WRITE_FAILED` (the `old-` folder, if any, is renamed back first). The program then calls `MemoryService::recheck_models()` (MEM-MD-3): `true` → `state` `ready`, `ask` false, log `memory_models.ready`, emit `memory_models_changed`; `false` → `DSK_MODELS_MISMATCH`. Memory then answers recall and remember without a restart of ordin or of the Memory Service.

**DSK-MD-11 Stop, quit and one download at a time.** `app.stop_memory_models_download` while `downloading` cancels the task at its next await, keeps the partial file and sets `state` `missing` (`ask` false); in any other state it answers the current view and does nothing (a `checking` step is not stopped: it ends within the time of hashing and unpacking 174 MB). Quitting (step 10) cancels the task through the shutdown token without waiting for it; the partial file stays and the next run's **Download** resumes. At most one download task exists per process.

**DSK-MD-12 Commands this program answers itself (additions to the table above).** All four are `Local`, `Interactive` and answered without a module (SVC-MD-1):

| Command | Input → Result | What this program does |
|---|---|---|
| `app.memory_models` | `Done` → `MemoryModelsView` | answers DSK-MD-3's view |
| `app.download_memory_models` | `Done` → `MemoryModelsView` | DSK-MD-6 |
| `app.stop_memory_models_download` | `Done` → `MemoryModelsView` | DSK-MD-11 |
| `app.decline_memory_models` | `Done` → `MemoryModelsView` | **Not now**: `ask` false, the time recorded (DSK-MD-5); memory stays unavailable |

The program emits the Service API event `memory_models_changed { models: MemoryModelsView }` (SVC-MD-2) on the local backend only, as it emits `brand_changed`.

**DSK-MD-13 A zip that matches its fingerprint but cannot be unpacked (added 2026-01-28, user decision).** When the whole zip matched `sha256` but the zip cannot be read, or one of its entries fails to decompress, the outcome is `DSK_MODELS_MISMATCH`, and the partial file and the `unpack-` folder are deleted as for the other mismatches of DSK-MD-9: the download is not usable as it is, and the person can act on it no differently from a fingerprint difference.

**DSK-MD-14 A refused certificate (added 2026-01-28, user decision).** The HTTP client reports a certificate the TLS layer refuses only as a connection error. The program tells it apart by the word "certificate" in the error's chain of causes and answers `DSK_MODELS_BLOCKED`, as DSK-MD-6 requires (on a company network this is usually a filtering proxy, which only IT can change); every other connection error is retried.

**DSK-REC-13 (amended 2026-01-28).** Its first three sentences are replaced by DSK-MD-1 and DSK-MD-2; its sentence on release-build data no document records no longer holds for the models: `release/memory-models/files.txt` records them (PB-REL-11).

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `app.download_memory_models` in a build with no models input | `Err(permanent)` | `DSK_MODELS_NOT_IN_BUILD` | `{"kind":"error","error":{"code":"DSK_MODELS_NOT_IN_BUILD","message":"This version of ordin can't get memory ready. Install the latest version of ordin."}}` (*amended January 2026 (user decision; Approved by the user January 2026)*) |
| No connection to the models' host after 6 attempts, and no byte received | `failed`, retryable by **Try again** | `DSK_MODELS_OFFLINE` | `failure` in `MemoryModelsView`; sentence "ordin couldn't connect to get memory ready. Check you're online, then try again." |
| The host refuses the address (401, 403, 404, 407, 410, 451, another unexpected status), a redirect is refused, or the certificate is refused | `failed`, not retried automatically | `DSK_MODELS_BLOCKED` | sentence "Your network stopped the memory download. Try again later, or on another network." (*amended January 2026 (user decision; Approved by the user January 2026)*) |
| The zip's or a file's fingerprint differs, a manifest file is missing or twice in the zip, the size announced or received is not the expected size, or the re-check fails | `failed`; partial file deleted | `DSK_MODELS_MISMATCH` | sentence "The memory download arrived damaged, so ordin didn't use it. Try again; if it happens again, choose Save logs for support… in Settings › Diagnostics." (*amended January 2026 (user decision; Approved by the user January 2026)*) |
| The disk is full while writing the zip or unpacking it | `failed`; partial file kept | `DSK_MODELS_DISK_FULL` | sentence "There isn't enough free space on this computer to get memory ready. Free up about 350 MB, then try again." |
| The connection kept dropping and 6 attempts were used up after bytes had arrived | `failed`, retryable; partial file kept | `DSK_MODELS_INTERRUPTED` | sentence "The memory download stopped before it finished. Try again; it carries on from where it stopped." |
| Writing, unpacking or renaming fails for a reason other than a full disk | `failed`, retryable; partial file kept unless DSK-MD-9 deleted it | `DSK_MODELS_WRITE_FAILED` | sentence "ordin couldn't save what memory needs on this computer. Try again; if it happens again, choose Save logs for support… in Settings › Diagnostics." (*amended January 2026 (user decision; Approved by the user January 2026)*) |

All seven codes are new (none is in `crates/ordin-types/src/error_registry/table.rs` or `cross-cutting/ErrorHandling.md` on 2026-01-28); owner `ordin-desktop`, HTTP status `200 (kind: error)`, retryable yes for `DSK_MODELS_OFFLINE`, `DSK_MODELS_INTERRUPTED` and `DSK_MODELS_WRITE_FAILED`, no for the other four. Their `en-GB` messages (EH-SEN-1, EH-SEN-2) go in `crates/ordin-localisation/catalogues/en-GB/errors-desktop.ftl` as `error-dsk-models-not-in-build`, `error-dsk-models-offline`, `error-dsk-models-blocked`, `error-dsk-models-mismatch`, `error-dsk-models-disk-full`, `error-dsk-models-interrupted` and `error-dsk-models-write-failed`, with the sentences above. The *Error-code registry* of `cross-cutting/ErrorHandling.md` gains the seven rows (`DSK_` 20 → 27) when this amendment is approved.

### Acceptance Criteria (Gherkin) — added scenarios

The download scenarios run against a stand-in host serving a test zip of ten small files and `LICENSES.md`, built with test values of the four inputs; no scenario reaches the real network.

```gherkin
Feature: The memory models download on first use (DSK-MD-1 to DSK-MD-12)

  Scenario: Happy path — the models download, are checked and memory works (DSK-MD-6, DSK-MD-9, DSK-MD-10)
    Given a build with a valid models input and an empty <data root>/models
    And memory was used once, so app.memory_models answers state "missing" and ask true
    When the screens send app.download_memory_models
    Then the answer has state "downloading"
    And memory_models_changed events report received_bytes rising to size_bytes
    And then state "checking", then state "ready"
    And <data root>/models holds the ten manifest files and LICENSES.md, and no SHA256SUMS
    And <data root>/models.download holds no .zip.part file
    And a recall now returns a result instead of MEM_MODEL_UNAVAILABLE

  Scenario: Edge case — a second start downloads nothing (DSK-MD-4)
    Given the models are in place from an earlier run
    When ordin starts
    Then app.memory_models answers state "ready" and the stand-in host receives no call

  Scenario: Edge case — interrupted, then resumed by byte range (DSK-MD-6, DSK-MD-7)
    Given the stand-in host closes the connection after 40% of the zip on the first attempt
    When the download runs
    Then the second attempt sends "Range: bytes=<the partial file's length>-"
    And the host answers 206 with the rest
    And the state becomes "ready"

  Scenario: Edge case — stopped by the person, resumed after a restart (DSK-MD-11)
    Given a download has written 50 MB
    When the person presses Stop, and ordin is quit and started again
    Then app.memory_models answers state "missing" with received_bytes 50 MB
    When app.download_memory_models is sent
    Then the first attempt sends "Range: bytes=50000000-"

  Scenario: Error path — a short file every time (DSK-MD-7)
    Given the stand-in host ends every body 1,000 bytes early and answers 206 to every range
    When the download runs
    Then 6 attempts are made, each waiting a random time no longer than the backoff cap
    And the state is "failed" with failure DSK_MODELS_INTERRUPTED
    And the partial file is kept

  Scenario: Error path — the zip's fingerprint is wrong (DSK-MD-9)
    Given the stand-in host serves a zip of the expected size whose SHA-256 differs
    When the download completes
    Then the state is "failed" with failure DSK_MODELS_MISMATCH
    And nothing was unpacked and the partial file is deleted
    And <data root>/models is unchanged

  Scenario: Error path — one file's fingerprint is wrong inside a matching zip (DSK-MD-9)
    Given a models input whose manifest digest for "reranker/tokenizer.json" differs from the file in the zip
    When the download completes
    Then the state is "failed" with failure DSK_MODELS_MISMATCH
    And no unpack- folder remains and <data root>/models was not replaced

  Scenario: Error path — the host announces a larger file (DSK-MD-8)
    Given the stand-in host answers 200 with a Content-Length one byte above size_bytes
    When the download runs
    Then no byte is written past size_bytes and the failure is DSK_MODELS_MISMATCH

  Scenario: Error path — the disk is full (DSK-MD-8)
    Given writing the partial file fails with "storage full" after 10 MB
    When the download runs
    Then the state is "failed" with failure DSK_MODELS_DISK_FULL
    And the 10 MB already written are kept for the next Download

  Scenario: Error path — offline (DSK-MD-7)
    Given the stand-in host's address refuses every connection
    When the download runs
    Then the state is "failed" with failure DSK_MODELS_OFFLINE after 6 attempts
    And every other part of ordin goes on working

  Scenario: Error path — the address is blocked (DSK-MD-6)
    Given the stand-in host answers 403
    When the download runs
    Then the state is "failed" with failure DSK_MODELS_BLOCKED after one attempt

  Scenario: Edge case — Not now, then asked again at the next use (DSK-MD-5)
    Given state "missing" and ask true
    When the screens send app.decline_memory_models
    Then ask is false and nothing is downloaded
    And a recall refused 10 seconds later does not set ask
    And a recall refused 2 minutes later sets ask true and emits memory_models_changed

  Scenario: Edge case — a build with no models input (DSK-MD-2)
    Given a build made without the four ORDIN_MEMORY_MODELS_ variables
    Then app.memory_models answers state "not_in_build"
    And app.download_memory_models is refused with DSK_MODELS_NOT_IN_BUILD
    And a recall answers MEM_MODEL_UNAVAILABLE, as before this amendment
```

### Gaps

- **G-MD-4** *Renaming over files another program holds open (Windows).* A virus scanner may hold a just-written file open for a moment, which can make DSK-MD-10's rename fail on Windows. The rule answers `DSK_MODELS_WRITE_FAILED` and **Try again** works once the scanner lets go; whether a short wait before the rename is needed has not been measured on a Windows computer.
- **G-MD-5** *The privacy page.* The note that ordin contacts the models' host once, when the person agrees, belongs in `docs/privacy.md`, which is outside the spec set and is the maintainer's text (OQ-REL-2).

## Amendment 2026-02-11 — Missing memory models are offered at start; the document agent keeps what it reads

> **Status:** user decision 2026-02-11 ("go with b and give the document agent memory"). Source: the 0.2.1-test.13 smoke test. All day, nothing called recall or remember, so DSK-MD-5 never asked; `memory.models_not_downloaded` is logged at `info` only (MEM-MD-6), so the person saw no sign that memory was off. Amends DSK-MD-4 and DSK-MD-5 and the 2026-01-28 decision "fetched on first use"; the download itself, its consent and its checks are unchanged.

**DSK-MD-15 Asked at start (amends DSK-MD-4, DSK-MD-5).** When DSK-MD-4 sets `state` to `missing`, it also sets `ask = true`, so the first `app.memory_models` answer and the card of SM-MD-1 offer the download as soon as the local backend's conversation is shown. Nothing is downloaded until the person presses **Download** (DSK-MD-6). **Not now** sets `ask` false and records the time as before, and DSK-MD-5 still asks again at the next refused recall or remember at least 60 seconds later. `ask` is not stored, so each start with the models missing asks again. A build with no models input (`not_in_build`) and a start with the models `ready` set `ask` false, as before. *Reason:* asking only on first use leaves memory silently empty whenever no agent happens to call it, which is what the smoke test found.

**DSK-MD-16 The document agent's Memory part.** The bundled catalogue holds a Memory part `urn:ordin:default:memories:documents:document-agent` (`apps/desktop/catalogue/models/@default/memories/documents/document-agent.yaml`): `retains` `document-facts`, `decisions`, `open-questions`; `excludes` `secrets`; `retention.maxAgeDays` 365; `store.location` `local`. The document agent's WorkspaceProfile names it as `memory` and moves to version 0.2.1, so installed copies are offered the update. The four content classes are the terms of a new vocabulary `urn:ordin:default:vocabulary:memory-content-class` (`apps/desktop/catalogue/models/@default/vocabulary/memory-content-class.yaml`, axis `memory-content-class`). With the part, the desktop sets the agent's memory rules before each run (ADR-111), and the Memory Service offers it `recall` and `remember`. Its System Role (version 0.4.0) tells it to keep the facts, decisions and open questions of each document it reads, each with its source and `fact_class`, and never a secret. *Assumption:* 365 days, because a document's facts go stale over months; the person can forget a fact sooner.

| Condition | Outcome |
|---|---|
| Models missing at start, build with a models input | `ask` true at once; the card offers **Download** |
| The person answers **Not now** | `ask` false; asked again at the next refused use ≥ 60 s later, or at the next start |
| The document agent calls `remember` with the models missing | `MEM_MODEL_UNAVAILABLE` (MEM-MD-4); the agent carries on; DSK-MD-5 applies |
| The document agent names no `fact_class` | `MEM_CLASS_REQUIRED`, naming the three classes it keeps |

```gherkin
Feature: Missing memory models are offered at start (DSK-MD-15)

  Scenario: Happy path — missing models are offered at start
    Given a build with a valid models input and an empty <data root>/models
    When ordin starts
    Then app.memory_models answers state "missing" and ask true
    And nothing is downloaded until the person presses Download

  Scenario: Edge case — Not now holds for this run
    Given the person was offered the models at start
    When the person answers Not now and a recall is refused within 60 seconds
    Then ask stays false
```

## Amendment 2026-02-11 — Default memory rules; Ordin asks agents to keep what they read (DSK-MEM-2)

> **Status:** user decision 2026-02-11 ("go with defaults"). Source: the 0.2.1-test.14 log — the document agent was offered `recall` and `remember`, read a PDF in five parts and made no `remember` call, because Ordin's brief did not ask for it. The user weighed Ordin writing memory itself against agents writing it, on turns and on future agents: agents write, because they hold the document's text and Ordin's long-lived conversation does not grow with fact lists; Ordin orchestrates through the brief; an agent with no memory part still gets memory. Amends ADR-111's `run_starting` (an agent with no memory part had its rules cleared, so the Memory Service offered it no memory tools).

- **DSK-MEM-2 Default rules.** In `SupervisorHost::run_starting`, an agent instance whose package declares no memory part (`memory_contract` → `None`) gets `AgentMemoryRules { retains: [], excludes: ["secrets"], max_age_days: Some(365), store_location: "local" }` instead of none: any class may be kept except `secrets`, for 365 days. A memory part replaces the defaults entirely. A failure setting them is logged `desktop.memory_rules_failed { code }` as for a declared part, and the run starts.
- **DSK-MEM-3 The brief asks.** The bundled coordinator's System Role (`ordin-agents`, `workspaces/coordinator/system-role.md`, "Handing work to an agent") adds a fifth part to every brief whose work reads documents: keep the documents' facts, decisions and open questions in memory, one claim each, with the document they came from. Ordin does not write the agent's facts itself; it recalls them to answer.
- **DSK-PLAN-1 The brief asks for a plan when the work has stages** *(amended February 2026, the user's review of the progress card: "Actions taken so far" gives no sense of how far along the work is; user decision: Ordin decides whether a plan is needed from the request)*. The bundled coordinator's System Role ("Handing work to an agent") adds a sixth part to a brief whose work has more than one stage — it reads several documents, reads and then writes, compares, drafts, or is something the person expects to take minutes: "Plan first: state 3 to 7 steps in the person's words with `plan`, then mark each step now when you start it and done when it is finished." A brief for one question about one document has no such part. The agent's steps then reach the progress card through the MCP Front's `plan` tool (MCP-PLAN-START, MCP-PLAN-REMIND), whatever its harness. *Status: Approved by the user, February 2026 ("approve phase 3 as written, build it"). Source: plan 10 Phase 3.*

```gherkin
Feature: Default memory rules (DSK-MEM-2)

  Scenario: Edge case — an agent with no memory part
    Given an installed agent whose package declares no memory part
    When a run of it starts
    Then its memory rules exclude "secrets", keep facts for 365 days and require no class
    And the Memory Service offers it recall and remember
```

## Amendment 2026-02-11 — Save only the last few minutes for support (DSK-LOG-6)

> **Status:** user decision 2026-02-11 ("export only last 5 mins (configurable)"; plan 11). Amends DSK-LOG-5 and DSK-REC-41: the support file held the last 3 days of logs and the newest 20 runs' journal, tens of megabytes for a test of a few minutes.

- **DSK-LOG-6 A window.** `diagnostics.support_export` takes an optional `last_minutes`, 1 to 4,320; outside that range it is `SVC_INPUT_INVALID` at `/last_minutes`. Given, `since` = now − `last_minutes`: the Journal's bundle keeps only lines whose time is at or after `since` (`Journal::support_export_since`), and the log part keeps only lines whose `time` field is at or after `since` written in the same fixed-width UTC form, plus every line with no `time`; the log part starts with `{"record":"log_window","since":<since>}`. Absent, the file is as DSK-LOG-5 and DSK-REC-41 describe. The 40 MB cap and its `log_lines_cut` record are unchanged. Screens: **Save logs for support…** gains a choice of Last 5 minutes (the default), Last 15 minutes, Last hour and Last 3 days, kept while the window is open (ScreensModule.md SM-NS-3b).

```gherkin
Feature: Save only the last few minutes for support (DSK-LOG-6)

  Scenario: Happy path — the last 5 minutes
    Given log lines from 10:00, 10:05 and 10:07:30 and one with no time
    When the support file is saved at 10:10 for the last 5 minutes
    Then it holds the log_window record and the lines of 10:05, 10:07:30 and the one with no time

  Scenario: Error path — a window outside 1 to 4,320 minutes
    When diagnostics.support_export is sent with last_minutes 0
    Then it is refused with SVC_INPUT_INVALID at /last_minutes
```

## Amendment 2026-02-11 — Closing says it is closing (DSK-QT-1)

> **Status:** user decision 2026-02-11 (plan 11 Phase 2). After **Close** the window stayed as it was while step 10.3 ran, up to the bounds of its steps (the 27 November log: `copilot.cli.stop_timed_out { deadline_ms: 10000 }`), with nothing to say it was closing.

- **DSK-QT-1 The window is told first.** When step 10's decision is `Proceed`, the shell emits `ordin://quitting { running_sessions }` to the main window — the count of local sessions running at the decision — and only then starts `Core::shutdown`. `Ask` and `AlreadyRunning` emit nothing new. The shutdown's steps and bounds are unchanged; the screens' line is ScreensModule.md SM-QT-1.

## Amendment 2026-01-29 — The Knowledge Service's background task at start

> **Status:** Approved by the user 2026-01-29 ("proceed with recommendations"). Source: the end-to-end job found that no saved knowledge page reaches memory, because step 4 does not name the Knowledge Service's background task and the program never starts it. Every section above stays in force; DSK-KS-1 adds one item to step 4 and names when it stops in step 10.3. Counterpart: `components/KnowledgeService.md` (`KnowledgeService::run(shutdown)`: the freshness listeners, the memory-index task, housekeeping and the start-up scan; in the agent profile it starts nothing).
>
> Evidence read: step 4 and step 10.3 above; row 3.38; `apps/desktop/src/core.rs` (`Core::start_background`, `Core::shutdown`: the core's shutdown token is cancelled after the File Service's shutdown); `crates/ordin-knowledge/src/service.rs` (`KnowledgeService::run`); `crates/ordin-knowledge/src/indexer.rs` (`index_task`, `index_pass`, `record_attempt`); `apps/desktop/src/hosts_tests.rs` (the one place that spawned `run` before this amendment, a test).

### Internal Logic (additions)

**DSK-KS-1 Step 4 starts the Knowledge Service's background task; quitting stops it.** Step 4 gains one item, after `start()` of Home and Search: the program spawns `knowledge.run(<the core's shutdown token>)`, not awaited. It runs the Knowledge Service's file and origin listeners, its memory-index task, its housekeeping and its start-up scan, so a page the person or an agent saves is handed to memory's index (`KnowledgeMemory::index_page_sections`) with no other call, and Ordin's recall can find it. The program spawns it once and nothing else in the program spawns it. It runs while ordin is open and stops when ordin quits: step 10.3 cancels the core's shutdown token (after the File Service's shutdown), and `run` returns once its tasks have ended. A failure inside it is the Knowledge Service's to log; it never stops start.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: ordin desktop — the Knowledge Service's background task

  Scenario: Happy path — a saved page is handed to memory after start (DSK-KS-1)
    Given the core has started on an empty data root
    When the person saves a knowledge page in a workspace
    Then within 10 seconds the Knowledge Service has handed the page to memory once, with no other call
    And with memory's models present, Ordin's recall finds the page's text

  Scenario: Edge case — quitting stops the task (DSK-KS-1)
    Given the core has started
    When the person quits
    Then the core's shutdown token is cancelled and the Knowledge Service's task ends with it
```

### Gaps

None.

## Amendment 2026-01-29 — The Setup Assistant's start checks

> **Status:** Approved 2026-01-29 (user decision: complete the high and medium follow-ups). Source: step 4 names `setup_assistant.start_background_checks()` but not the start checks, while `components/SetupAssistant.md` (*Who calls each public item*) has the programs call `run_checks(Start)` at start, before `start_background_checks`; the program never called it, so the second-install check (SetupAssistant.md SET-UP-27) never ran. Every section above stays in force; DSK-SC-1 replaces the item `setup_assistant.start_background_checks()` of step 4. Counterparts: `components/ServiceApi.md` amendment 2026-01-29 (SVC-SC-1, `CheckKind::SecondInstall` restated); `components/ScreensModule.md` amendment 2026-01-29 (SM-FS-8, the step's words).
>
> Evidence read: step 4 above; `apps/desktop/src/core.rs` (`Core::start_background`: the `setup_progress` listener, then `start_background_checks` spawned); `crates/ordin-setup/src/checks.rs` (`checks`: with trigger `Start` it adds the `SecondInstall` result on the desktop and publishes every result as the progress); `apps/desktop/src/service_api_rows.rs` (`setup.status` restates `progress()`).

### Internal Logic (additions)

**DSK-SC-1 Step 4 runs the start checks, then the background checks.** In step 4, at the place of `setup_assistant.start_background_checks()`, the program spawns one task, not awaited, that calls `setup_assistant.run_checks(CheckTrigger::Start)` and, when that call returns, `setup_assistant.start_background_checks()`. Start does not wait for the checks, as it did not wait for the background checks before (03 *First start*).

1. **Where the report goes.** The program does not keep or journal the start report. The Setup Assistant publishes its results as its progress (`running: false`, one step per result, a failed step with its `problem`, SET-R1-3); the program's existing `setup_progress` listener of step 4 turns that change into a `setup_progress` event, and `setup.status` answers the same progress. So a `SecondInstall` result reaches the screens through both (ScreensModule.md SM-UP-9).
2. **A failing check never stops start.** A result with `outcome = Failed` is part of the report and changes nothing in the program. An `Err` from `run_checks` is logged `warn` as `setup.start_checks_failed` with its code, and the background checks start all the same. Neither stops start, quits the program or shows a message box.
3. **Order.** The listener is subscribed before the task is spawned, so the start report's progress is not missed. The background checks never start before the start checks have ended, so the Setup Assistant's first background run is 60 s after `Start` (SetupAssistant.md D.4). When ordin quits before the start checks end, step 10.3's `setup_assistant.close()` has already run and the later `start_background_checks` starts nothing (SetupAssistant.md SET-SC-1).

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: ordin desktop — the Setup Assistant's start checks

  Scenario: Edge case — a second install is reported at start (DSK-SC-1)
    Given the desktop's updater names another copy of ordin in "/Applications"
    When the core has started
    Then within 10 seconds setup.status answers a progress with a second_install step that failed
    And its problem is "setup-second-install" with folder "/Applications"
    And the core is running

  Scenario: Edge case — a failed start check does not stop start (DSK-SC-1)
    Given no organisation profile and no Claude Code on this computer
    When the core starts
    Then start succeeds and the background checks are started
```

### Gaps

None.

## Amendment 2026-01-30 — The person decides what happens to data the program does not recognise

> **Status:** Approved 2026-01-30 (user decision: all three changes, no "ask IT" when the saved data does not match). Source: after an update, a database whose applied step differed from the program's stopped start at row 3.1 with step 9's message box, which tells the person to ask IT; nobody else can repair it, and the only way out was deleting the data by hand. Step 9 stays in force for every other `StartError`. Counterparts: `cross-cutting/DatabaseMigrations.md` and `components/JournalAndArtefactStore.md`, amendments 2026-01-30.

### Internal Logic (additions)

**DSK-DATA-1 Which failures ask the person.** When start stops with `StartError::ModuleFailed { module: "ordin-journal", code }` and `code` is `JRNL_MIGRATION_CHANGED` or `JRNL_SCHEMA_NEWER`, the program logs `start_failed` as step 9 does, then shows, in place of step 9's message box, a native message box (kind warning) with the words below, compiled into the program in `en-GB` like step 9's (Localisation is not constructed yet):

| Code | Text | Buttons |
|---|---|---|
| `JRNL_MIGRATION_CHANGED` | "ordin's saved data was set up by a different version of ordin, and part of it doesn't match this one. You can keep your data and carry on (ordin saves a copy first), or start with empty data (your current data is moved to a folder, not deleted)." | **Keep my data** · **Start with empty data** · **Quit** |
| `JRNL_SCHEMA_NEWER` | "ordin's saved data comes from a newer version of ordin. Install the newer version to open it, or start with empty data (your current data is moved to a folder, not deleted)." | **Start with empty data** · **Quit** |

Each text is followed by step 9's reference line. Closing the box is **Quit**: exit with status 1, nothing changed.

**DSK-DATA-2 Keep my data.** The program opens the datastore at `<data root>/journal.db` with `backups` as its backup folder, registers this build's modules (row 3.1's registrations), and calls `MigrationRegistry::accept_changed`. It logs `info` `data_kept` with `accepted` (the number of rows) and restarts the program. On an error it logs `start_failed` with that code and shows step 9's message box.

**DSK-DATA-3 Start with empty data.** The program creates `<data root>/set-aside-<YYYYMMDDTHHMMSSZ>` (mode `0700`) and moves into it every entry of the data root except `logs`, `backups` and earlier `set-aside-*` folders. It logs `info` `data_set_aside` with the folder's name and restarts the program, which starts as on a first start. Nothing is deleted. A failure to create the folder or move an entry stops there, logs `start_failed` with `DSK_DATA_ROOT_UNWRITABLE`, and shows step 9's message box; entries already moved stay in the new folder.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the person keeps data an update did not recognise
    Given the data root holds a database whose applied step differs from this build's
    When the program starts
    Then a message box offers "Keep my data", "Start with empty data" and "Quit"
    When the person chooses "Keep my data"
    Then the program restarts and opens the main window with the person's data

  Scenario: Happy path — the person starts with empty data
    Given the data root holds a database written by a newer ordin
    When the person chooses "Start with empty data"
    Then every entry of the data root but logs and backups is in a new set-aside folder
    And the program restarts as on a first start
```

### Gaps

None.

## Amendment 2026-01-31 — The agent maker is added only in a build that has it

> **Status:** Approved 2026-01-31 (user decision: "carry on with the fixes"). Source: DSK-REC-24 adds Ordin's agent and then the agent maker on every start. The agent maker (`urn:ordin:default:workspaces:advisor`) comes with the `authoring` build switch and is not in the bundled catalogue of a build without it (the catalogue's own README says so). So every start of such a build logged `agent.bundled_missing` and `bundled_agent_unavailable` with `PKG_BUNDLED_AGENT_MISSING` at `error`, and Diagnostics showed it to the person. DSK-REC-24 stays in force for a build with `authoring`.
>
> Evidence read: DSK-REC-24 and its error rows; `apps/desktop/src/bundled.rs` (`add_bundled_agents`, `add_each`); `apps/desktop/src/groups.rs` (the `authoring` switch); the bundled catalogue's README; safety net C (`apps/desktop/tests/first_start.rs`) on the release's bundled agents.

- **DSK-REC-24a The bundled agents of this build.** The task of DSK-REC-24 calls `bundled_instance` for `BundledAgent::Ordin`, then, only when the program is built with the `authoring` switch, for `BundledAgent::AgentMaker`. Without the switch nothing is logged about the agent maker. Each call keeps DSK-REC-24's retry rule.

### Acceptance Criteria (Gherkin) — added scenario

```gherkin
  Scenario: Edge case — a build without authoring adds only Ordin's agent (DSK-REC-24a)
    Given a build without the authoring switch and a bundled catalogue that holds only Ordin's agent
    When ordin starts on a new data root
    Then bundled_instance is called for Ordin's agent only
    And nothing is logged at warn or error about the agent maker
```

### Gaps

None.

## Amendment 2026-01-31 — A test build that journey tests can drive

> **Status:** Approved 2026-01-31 (user decision: "use playwright on windows"). Source: the journey tests drive the real window with Playwright over WebView2's debugging port. The Windows window's WebView2 is started with the window shell's own browser arguments, so the `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` variable has no effect, and the port can be opened only by the program itself. Every section above stays in force; a program built without the switch below is unchanged.
>
> Evidence read: `apps/desktop/src/window.rs` (step 6, `WebviewWindowBuilder`); the WebView2 command line of a running program on the Windows build machine (`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`, no debugging port with the variable set); the window shell's WebView2 environment code (`wry` 0.57, `additional_browser_args`, which replaces the variable).

- **DSK-JRN-1 The `journeys` switch.** The program has a build switch `journeys`, off by default. With it, on Windows only, step 6 creates the window with the browser arguments `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required --remote-debugging-port=9222` (the window shell's defaults plus the debugging port, which WebView2 opens on the loopback address only). Without it, or on macOS, step 6 is unchanged. Nothing else changes with the switch, apart from DSK-JRN-3.
- **DSK-JRN-3 The system dialogs' answer.** With the `journeys` switch, each of the three operating-system dialogs (the folder picker of `app.pick_folder`, the file picker of `identity.import_client_certificate`, the save dialog of `files.save_export`) first reads `<data root>/journeys-dialog-answer.txt`: when it exists and holds a path, the file is deleted and that path is the person's choice, and no dialog opens; otherwise the dialog opens as without the switch. The journey runner writes the file just before the step that opens the dialog. Windows does not let a test drive another program's dialog reliably, so this is how a journey chooses a folder.
- **DSK-JRN-2 Never in a release.** The release build (`scripts/release/build.sh`) builds the program with no build switch named on the command line, so `journeys` is never in an installer. The switch is named only by the journey runner's own build.
- **DSK-JRN-4 The online services' stand-ins** *(amended January 2026; user decision 2026-01-30)*. With the `journeys` switch, the program also compiles the online services' test seams, and at start each service row reads `<data root>/journeys-stand-ins.json`, a JSON object with one optional entry per service. `confluence: { gateway, site, token }` → `ConfluenceAdapter::use_test_seam(ConfluenceTestSeam { gateway, site, token })` (ConfluenceAdapter.md CONF-SEAM-2), logged `journeys.stand_in` with the service's name at `info`. `sharepoint: { graph, token_endpoint, client_id }` → the SharePoint row builds its adapter `with_stand_in(graph, 1.0, 120 s)`, and the Workspaces row, before it builds the Workspace Service, signs a Microsoft account in with `IdentityService::seed_microsoft_for_tests("journeys-refresh", client_id, token_endpoint)` when no Microsoft account is signed in, so its tokens come from the stand-in (a `graph` that is not a URL is refused; one the adapter refuses stops the start, which only a journeys build can do). `jira: { site_base_url, gateway_base_url }` → the Jira row builds `JiraAdapter::with_endpoints(deps, JiraEndpoints { site_base_url, gateway_base_url, request_timeout: None })`; the person then connects Jira with an API token as without the switch. Each is logged `journeys.stand_in` with its service's name. An absent file or entry changes nothing; a file that is not a JSON object, a site that is not a URL, or a seam the adapter refuses is logged `journeys.stand_in_refused` (or `journeys.stand_ins_unreadable`) at `warn` and the service keeps its production settings. The journey runner writes the file before it starts the program. Without the switch nothing reads the file, and DSK-JRN-2 keeps the switch, and so the seams, out of every installer.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a journeys build opens the debugging port (DSK-JRN-1)
    Given the program built with the journeys switch on Windows
    When it starts
    Then http://127.0.0.1:9222/json/version answers

  Scenario: Happy path — a journeys build reaches the Confluence stand-in (DSK-JRN-4)
    Given the program built with the journeys switch
    And journeys-stand-ins.json names a Confluence gateway on 127.0.0.1, a site and a token
    When it starts and the person links a Confluence page
    Then every Confluence request goes to the stand-in with that token

  Scenario: Edge case — a release build does not (DSK-JRN-2)
    Given the program built by the release build
    When it starts on Windows
    Then nothing listens on port 9222
```

### Gaps

None.

## Amendment 2026-01-31 — Ordin's own conversation is not work on Home

> **Status:** Approved 2026-01-31 (user decision: "continue with phase 5" of the walk through the built program). Source: the walk on Windows: after a message to Ordin, Home listed Ordin's conversation run under **Running now** as "Ordin — paused · Waiting for you", and again under **Since you last looked**. The run is Ordin's conversation (`RunOrigin::Conversation`, caller `Ordin`), paused between the person's messages by design; nothing waits for the person. Every section above stays in force.

- **DSK-HOME-1 Ordin's conversation run.** The desktop's Home reader (`JournalReader::running`, `finished_since`) leaves out every run whose origin is `RunOrigin::Conversation`. Runs of agents, crews and workflows are listed as before.

```gherkin
  Scenario: Edge case — Ordin waiting for the next message is not on Home (DSK-HOME-1)
    Given Ordin has answered the person and its run is paused for the next message
    When Home is drawn
    Then Running now and Since you last looked list no row for Ordin's conversation
```

### Gaps

None.

## Amendment January 2026 — A linked Confluence page can be read

> **Status:** amended January 2026 (user decision 2026-01-30, "proceed with recommendations"). Source: Phase 5 of the UI walk on Windows — a linked page, kept as `<title>.confluence.xhtml` in Confluence's storage format (ConfluenceAdapter.md, *Working-copy file names*), opened with no preview, because the File Service has none for that format and cannot depend on the Confluence Adapter.

- **DSK-CFP-1 The preview of a linked Confluence page.** The program's `files.preview` row answers a path ending `.confluence.xhtml` (any case) by reading the file (`files.read_file`) and converting it with `ConfluenceAdapter::storage_to_markdown`, as `Preview::Text { text, markdown: true, truncated }`, cut at `PREVIEW_TEXT_BYTES` on a character boundary; a page that does not convert is `Preview::None`. Every other path is the File Service's `preview` as before. The file is never changed.

```gherkin
  Scenario: Happy path — a linked page reads as Markdown (DSK-CFP-1)
    Given the working copy holds Rates.confluence.xhtml with "<h1>Rates</h1><p>Kestrel Freight: 1,240 GBP.</p>"
    When files.preview is sent for it
    Then the preview is Markdown text holding "# Rates" and "Kestrel Freight: 1,240 GBP."
```

### Gaps

None.

## Amendment January 2026 — The Push Service runs

> **Status:** amended January 2026 (user decision 2026-01-30, "proceed with recommendations"). Source: Phase 5 of the UI walk on Windows — Ordin proposed sending a changed Confluence page back, the person pressed **Send back**, `approval_decided` was journalled, and nothing was sent: the pending push stayed `proposed`. Step 4 starts the other services' loops but never the Push Service's, whose `run` (PushService.md: recovery §G, then the decision loop §B and the change loops §D, §E) is what carries out an approved push; its sink only queues decisions.

- **DSK-PUSH-1 Start the Push Service** *(amended January 2026)*. Step 4 also spawns `push.run(shutdown token)` (row 3.37's service), after `start()` of the File, Workspace, Project, Confluence, Craft and Home and Search services and beside the Knowledge Service's `run`; it is not awaited, and it stops when step 10.3 cancels the core's shutdown token. An error it returns is logged `push.run_failed` at `warn` with its code; start does not stop.

```gherkin
  Scenario: Happy path — an approved send-back is carried out (DSK-PUSH-1)
    Given Ordin proposed sending a changed Confluence page back and the person pressed Send back
    Then the Push Service updates the page in Confluence and the pending push is no longer proposed
```

### Gaps

None.


## Amendment January 2026 — The document agent in every workspace

> **Status:** Approved by the user, January 2026 ("approve"; decisions of January 2026: "yes, plan both", "go ahead with phase 1"). Source: the person's own use of the first release — only Ordin was installed, so a question about a document could not be given to an agent. The catalogue's side is PackageCatalogue.md PKG-DOC-1 to PKG-DOC-3.
>
> Evidence read: DSK-REC-24 (bundled instances at start, its retry rule); ServiceApi.md `workspaces.create`; PackageCatalogue.md `AddAgentInput.workspace_languages` ("filled by the program from the Project Service before the call").

- **DSK-DOC-1 Adding the starter agents** *(amended January 2026)*. (1) At start, in DSK-REC-24's task, after both `bundled_instance` calls, the program lists the active workspaces and calls `packages.add_starter_agents(workspace_id, <languages as for add>, &localisation.active().locale)` for each, one at a time; DSK-REC-24's retry rule applies per workspace (retryable → again after 30 s, at most 5 attempts; permanent → logged `starter_agents_unavailable` at `error` with `workspace_id` and `code`). (2) After `workspaces.create` succeeds, the program calls it for the new workspace in a spawned task, with the same rule; the command's reply does not wait for it. A failure never stops ordin starting or the workspace being created. Each successful add raises the catalogue's notice as any add does, so Ordin's next turn names the agent (OrdinHost.md OH-DIR-1).

```gherkin
  Scenario: Happy path — an upgrade adds the document agent to existing workspaces (DSK-DOC-1)
    Given a data root from an earlier version with the workspaces "Strategy" and "Suppliers" and only Ordin installed
    When this version starts
    Then the document agent serves "Strategy" and "Suppliers"

  Scenario: Happy path — a new workspace has the document agent (DSK-DOC-1)
    When the person creates the workspace "Board papers"
    Then the document agent serves "Board papers" without the person adding it
```

### Gaps

None.

## Amendment January 2026 — A team workspace, and team knowledge on by default (ADR-121)

> **Status:** Approved by the user January 2026. Source: decision 103; HLD revision 0.68.0, ADR-121 (consequence: "The build that carries this turns on the team-knowledge build switch"). The modules: `components/TeamKnowledgeService.md` (amendment January 2026: the traits `TeamAgents` and `TeamRuns`, the `TeamWorkspaces` and `TeamPush` additions, the `EventSink`), `components/WorkspaceService.md` WS-TW-1 to WS-TW-9, `components/OrdinHost.md` OH-TW-2. Every section above stays in force; where this amendment and DSK-UP-1 differ, this amendment wins.
>
> Evidence read: DSK-UP-1 (the `[features]` table, `default = []`, "This spec turns no switch on by default"); the hub-trait delegation table (rows `TeamMemory` to `TeamWorkspaces`, row 3.41a); `apps/desktop/Cargo.toml` (`[features] default = []`, `team-knowledge = ["dep:ordin-team-knowledge"]`).

- **DSK-TW-1 Team knowledge is on by default in the desktop program** (replaces DSK-UP-1's sentence "This spec turns no switch on by default" for this one switch). `apps/desktop/Cargo.toml`:

  ```toml
  [features]
  default          = ["team-knowledge"]
  ```

  Every other line of DSK-UP-1's table is unchanged, and every other switch stays off by default. The Team Knowledge Service this switch builds is its **first build** (ADR-121 item 9; `phase-5-playbook.md` Step 74, PB-TW-1): the workspace's own folder and the team workspace. Reading other teams' folders and company-named folders (ADR-110) and the index and log (ADR-112) are not in it, so the program wires none of their calls: no company folders reach the service from the Organisation Profile, and `DesktopServiceApi` refuses the seven commands of the ADR-110 amendment — `team_knowledge.add_read_folder`, `stop_reading`, `adopt`, `adopt_again`, `keep_as_is`, `set_description` and `check_recall` — with `SVC_NOT_IN_THIS_VERSION`, as it refuses a command of a module the build lacks, so the screens draw no entry for them (ScreensModule.md SM-UP-1). The crate is new code in that build; nothing of it is built before. A build made with `--no-default-features` has no Team Knowledge Service, answers no `team_knowledge.*` command (`SVC_NOT_IN_THIS_VERSION`) and shows no team entry, as DSK-UP-4 states. The personal server's switches (OrdinServer.md) are unchanged.
- **DSK-TW-2 Delegations added** (to the hub-trait table, row 3.41a):

  | Trait (owner) | Methods → implementation | Row |
  |---|---|---|
  | `TeamWorkspaces` (Team Knowledge Service), additions | `create`, `rename`, `move_to_bin`, `delete_from_bin`, `remove_link`, `working_copy_items`, `grants`, `set_grant`, `check_origin`, `add_link_from_origin`, `make_results_folder`, `results_folder`, `results_of_run`, `unsent_results`, `read_result`, `store_team_result`, `remove_team_result`, `mark_results_sent`, `keep_both_results`, `record_pending_grant`, `pending_grants`, `decide_pending_grant`, `end_team`, `resolve_origin` (*amended January 2026 (user decision; Approved by the user January 2026)*, WorkspaceService.md WS-TW-6 step 3) → the Workspace Service's methods of the same names | 3.41a |
  | `TeamAgents` (Team Knowledge Service), new | `agents_in_workspace` → `PackageCatalogue::agents_in_workspace` then `agent_identity`; `find_agent` → `match_workflow_agent`, then `CatalogueService::repository_by_ref` and `find_part`; `add_agent` → `find_part` then `PackageCatalogue::add` with the workspace languages and interface locale the Service API fills for `agents.add`; `remove_agent` → `PackageCatalogue::remove` (TeamKnowledgeService.md amendment January 2026, Shared Context 3) | 3.41a |
  | `TeamRuns` (Team Knowledge Service), new | `ended_task` → `SessionSupervisor::get`, then `PackageCatalogue::get` for the display name and the Package Inspector's `directory` for the task's title | 3.41a |
  | `TeamPush` (Team Knowledge Service), addition | `publish_result` → `PushService::publish_copy` | 3.41a |

  `TeamKnowledgeDeps` gains `agents` and `runs` with these implementations. Row 3.41a also calls `journal.subscribe(<the service's event sink>)` after construction, as the Push Service's sink is registered. The Ordin Host's X-7 and X-27 additions (OH-TW-2) are met by the Workspace Service and File Service objects that row 3.41 already holds.
- **DSK-TW-3 Commands.** `DesktopServiceApi` (row 3.48) answers the ten `team_knowledge.*` commands of ServiceApi.md SVC-TW-1 over the module (*amended January 2026 (user decision; Approved by the user January 2026)*: eleven, with `team_knowledge.results_choices` → `TeamKnowledgeService::results_choices`).

```gherkin
  Scenario: Happy path — a default build carries team knowledge (DSK-TW-1)
    Given the desktop program built with its default features
    When the screens send "team_knowledge.status" for a workspace
    Then the command is answered and not refused with SVC_NOT_IN_THIS_VERSION

  Scenario: Edge — the first build answers no read-folder command (DSK-TW-1)
    Given the desktop program built with its default features in the build that carries Step 74's first build
    When the screens send "team_knowledge.add_read_folder"
    Then it is refused with SVC_NOT_IN_THIS_VERSION and the Team Knowledge Service is not called

  Scenario: Edge — a build without default features (DSK-TW-1)
    Given the desktop program built with --no-default-features
    When the screens send "team_knowledge.status"
    Then it is refused with SVC_NOT_IN_THIS_VERSION
```

### Gaps

None.

## Amendment January 2026 — The Package Catalogue's notices loop runs

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person's own Mac after an upgrade — the bundled catalogue was indexed again (`catalogue.bundled_indexed`) and holds document-agent 0.2.0 and cspa-431 1.2.0, yet the installed instances stayed at 0.1.0 and 1.0.0 and no update waited for the person. PackageCatalogue.md names the program as the caller of `run` (its callers table) and PKG-REC-3 item 4 relies on §C's start sweep, but step 4 never spawned `run`, so neither the start sweep nor any later `RepositoryUpdated` notice was acted on.

- **DSK-PKG-1 Start the Package Catalogue's notices loop** *(amended January 2026 (user decision; Approved by the user January 2026))*. In step 4, once `catalogue.start(shutdown)` has returned `Ok`, the program spawns `packages.run(<the core's shutdown token>)` (PackageCatalogue.md §C, §D), not awaited, before DSK-REC-24's task. Its start sweep then re-plans every `ready` instance against the catalogue `catalogue.start` has just indexed: an instance from the bundled repository whose catalogue version is newer is updated between runs, or waits for the person's choice, as §C and PKG-REC-3 item 4 state. The loop stops when step 10.3 cancels the core's shutdown token. An error it returns is logged `packages.run_failed` at `warn` with its code; start does not stop. When `catalogue.start` fails, the loop is not spawned.

```gherkin
  Scenario: Happy path — start re-plans an agent of the bundled repository (DSK-PKG-1)
    Given an agent installed from the bundled repository by an earlier version is ready
    When ordin starts and the bundled catalogue has been indexed
    Then the Package Catalogue's start sweep re-plans that agent

  Scenario: Happy path — an agent installed at an old version is updated to the new one (DSK-PKG-1)
    Given the document agent is installed at version 0.1.0 from the bundled repository
    And the bundled catalogue now holds version 0.2.0, which changes nothing it may do or read
    When the Package Catalogue's notices loop starts
    Then the document agent is ready at version 0.2.0 and updated_between_runs_at is set
```

### Gaps

None.

## Amendment January 2026 — The log files' limits and Clear logs (amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person found **Save logs for support…** did nothing, and that nothing clears the logs; they chose a size limit per file, a limit for the folder and a **Clear logs…** button. Counterparts: cross-cutting/Logging.md L-1 and *Data Model* (amended), ServiceApi.md SVC-LOG-1 and SVC-R1-10, ScreensModule.md SM-NS-3 and SM-NS-3a, ErrorHandling.md (two `DSK_` rows), phase-2c-shared-types-wire.md §19. Every section above stays in force except step 2's file writer and the *Log retention* constant, both amended in place.

### Internal Logic (additions)

**DSK-LOG-1 The files.** Step 2 hands `tracing_appender::non_blocking` the program's own writer, `log_files::LogFiles` in `apps/desktop/src/log_files.rs`, because `tracing-appender` 0.2 starts a new file by time only. Files are `<data root>/logs/ordin.<YYYY-MM-DD>.log`, the date in UTC, and `ordin.<YYYY-MM-DD>.<n>.log` (n = 1, 2, …) for the further files of one day. A new file starts at the first line of a new UTC day, and before a line that would take a file already holding bytes past 20,000,000 bytes (20 MB); a line is never split, so a single line over 20 MB goes whole into a file of its own. At start the program continues the day's newest file while it is a file under 20 MB, else it starts the day's next number. amended January 2026 (user decision; Approved by the user January 2026).

**DSK-LOG-2 The limits.** At most 14 files are kept, the current one included, and the folder's log files together hold at most 100,000,000 bytes (100 MB). At start and each time a new file starts, the program deletes the oldest files other than the current one, oldest first by date then number, until both limits hold. A file it cannot delete stays and is tried again at the next new file; it is logged `warn` as `log_file_not_deleted` with `code: DSK_LOG_FILE_NOT_DELETED` and the file's name. Only names of the two forms above count; every other file in `logs/` is left alone. amended January 2026 (user decision; Approved by the user January 2026).

**DSK-LOG-3 The bound per day.** Logging.md *Performance targets* puts a normal day under 50 MB, so a day takes up to three files; the 100 MB limit, not the 14 files, then decides how many days are kept. amended January 2026 (user decision; Approved by the user January 2026).

**DSK-LOG-4 `diagnostics.clear_logs`.** A person-only command (ServiceApi.md SVC-LOG-1). The program deletes every log file but the current one and empties the current one, keeping its writer, so logging never stops and the next line goes at the start of the emptied file. It answers `LogsCleared { deleted_files }` and logs `logs_cleared` with `deleted_files`, which is the first line of the emptied file. Lines still waiting in the non-blocking writer's queue may be written after the emptying; they are the lines of the moment of clearing. A past file that cannot be deleted, a current file that cannot be emptied, an unreadable folder, or no log open at all (step 2 could not open the folder) → `DSK_LOGS_CLEAR_FAILED` (retryable, action `try_again`), logged `warn` as `logs_clear_failed` with the count of files not deleted; every file that could go has gone, and the current one is emptied whenever it can be. amended January 2026 (user decision; Approved by the user January 2026). *Amended January 2026 (user decision; Approved by the user January 2026):* when the log files were cleared, the program then calls the Journal's `clear_diagnostic_lines` (JournalAndArtefactStore.md JA-DIAG-CLEAR-1), so the Diagnostics event list is empty from that moment, also after the window is reopened; no journal entry is deleted. A journal refusal is passed through as its own code (`JRNL_DATASTORE_UNAVAILABLE`, retryable); the log files stay cleared.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| **Clear logs** cannot delete a past log file, cannot empty the current one, cannot read the folder, or no log is open (DSK-LOG-4) | `Err(retryable)` | `DSK_LOGS_CLEAR_FAILED` | `{"kind":"error","error":{"code":"DSK_LOGS_CLEAR_FAILED","action":"try_again",…}}` |
| **Clear logs** cleared the files but the Journal cannot store the clear moment (DSK-LOG-4, JA-DIAG-CLEAR-1) (amended January 2026; user decision; Approved by the user January 2026) | `Err(retryable)` | `JRNL_DATASTORE_UNAVAILABLE` | `{"kind":"error","error":{"code":"JRNL_DATASTORE_UNAVAILABLE","action":"try_again",…}}` |
| A log file the limits would delete cannot be deleted (DSK-LOG-2) | logged `warn`; logging continues | `DSK_LOG_FILE_NOT_DELETED` | — |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: The log files' limits and Clear logs (DSK-LOG-1 to DSK-LOG-4)

  Scenario: A new file starts each UTC day and at 20 MB (DSK-LOG-1)
    Given the program writes to ordin.2026-01-28.log
    When a line would take that file past 20 MB
    Then the line is written whole to ordin.2026-01-28.1.log
    And the first line of 2026-01-29 is written to ordin.2026-01-29.log

  Scenario: The oldest files go first (DSK-LOG-2)
    Given logs/ holds 15 log files and the program starts a 16th
    Then the 2 oldest are deleted and 14 remain
    And when the log files together hold more than 100 MB the oldest are deleted until they hold at most 100 MB

  Scenario: Error path — a file the limits cannot delete (DSK-LOG-2)
    Given the oldest log file cannot be deleted
    When the limits are checked
    Then the next oldest files are deleted instead and log_file_not_deleted is logged with DSK_LOG_FILE_NOT_DELETED

  Scenario: Happy path — Clear logs (DSK-LOG-4)
    Given logs/ holds 3 past log files and the current one
    When diagnostics.clear_logs is called
    Then the 3 past files are deleted, the current file is empty and the reply is {"deleted_files":3}
    And the next line is written to the current file

  Scenario: Error path — a past file cannot be deleted (DSK-LOG-4)
    Given one past log file cannot be deleted
    When diagnostics.clear_logs is called
    Then every other past file is deleted, the current file is emptied and the reply is DSK_LOGS_CLEAR_FAILED with action try_again
```

### Gaps

None.

## Amendment January 2026 — The support file holds the last 3 days of logs (amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person chose that **Save logs for support…** should also hold ordin's log files, which support needs to see what happened. Counterparts: cross-cutting/Logging.md E-1 (amended), ScreensModule.md SM-NS-3 and its string `diagnostics-support-note` (amended), ErrorHandling.md `DSK_SUPPORT_LOG_UNREADABLE` (added). DSK-REC-41 is amended in place by one sentence; every other section above stays in force.

### Internal Logic (additions)

**DSK-LOG-5 The log part of the support file.** After `Journal::support_export(run_ids)` answers and before the save dialog opens (DSK-REC-41), the program appends to `SupportBundle.bytes`, in the same JSON Lines form, the lines of the log files of the last 3 days: every file named `ordin.<day>.log` or `ordin.<day>.<n>.log` (DSK-LOG-1) whose `<day>` is the current UTC day, as the writer's own clock names it, or one of the two UTC days before it. It reads them through the process-wide handle of DSK-LOG-1 (`log_files::installed()`): under the handle's lock it flushes the current file and takes its length, then reads that file only up to that length, so a line being written is never read in part and logging is held only for the flush. Files are read oldest first, by day then number. Each line that is not blank becomes one record `{"record":"log_line","file":<file name>,"line":<the line's text>}` *(field names chosen here, as the Journal's `record` field)*; bytes that are not UTF-8 are replaced with U+FFFD. The log part is at most 40,000,000 bytes *(chosen here: DSK-LOG-3 puts a day under 50 MB, and the file is written only where the person chooses)*; when the lines do not fit, the newest lines are kept and one record `{"record":"log_lines_cut","left_out":<lines left out>}` comes first in the log part. A log file of those days that cannot be read is left out and named by one record `{"record":"log_file_unreadable","file":<file name>}`, logged `warn` as `support_log_file_unreadable` with `code: DSK_SUPPORT_LOG_UNREADABLE` and the file's name; the export goes on. With no log open (step 2 could not open the folder), the log part is the one record `{"record":"logs_not_open"}`. The line `support_export_saved` gains `log_files` (files read) and `log_files_unreadable` (files left out). Every desktop line is written by `JsonLineFormat` under Logging.md's rules R-2 to R-7 and the `panic` line holds its location only (step 2), so the log part holds no document or conversation text and the screen says so (ScreensModule.md SM-NS-3) (amended January 2026 (user decision; Approved by the user January 2026)). amended January 2026 (user decision; Approved by the user January 2026).

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A log file of the last 3 days cannot be read for the support file (DSK-LOG-5) | logged `warn`; the export succeeds without it, with a `log_file_unreadable` record naming it | `DSK_SUPPORT_LOG_UNREADABLE` | — |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: The log part of the support file (DSK-LOG-5)

  Scenario: Happy path — the last 3 days of logs are in the support file
    Given today is 2026-01-28 in UTC and logs/ holds ordin.2026-01-25.log, ordin.2026-01-26.log, ordin.2026-01-27.log, ordin.2026-01-28.log and ordin.2026-01-28.1.log
    When the person saves logs for support
    Then the saved file holds the journal's lines followed by a log_line record for each line of the 2026-01-26, 2026-01-27 and 2026-01-28 files, oldest first
    And no line of ordin.2026-01-25.log is in it

  Scenario: Edge case — the file being written is read up to its last whole line
    Given the program is writing to ordin.2026-01-28.1.log
    When the person saves logs for support
    Then every log_line record of that file is a whole line

  Scenario: Edge case — the newest lines are kept
    Given the log files of the last 3 days hold more than 40,000,000 bytes of lines
    When the person saves logs for support
    Then the log part starts with a log_lines_cut record and holds the newest lines

  Scenario: Error path — a log file cannot be read
    Given ordin.2026-01-27.log cannot be read
    When the person saves logs for support
    Then the file is saved with a log_file_unreadable record naming ordin.2026-01-27.log
    And support_log_file_unreadable is logged with DSK_SUPPORT_LOG_UNREADABLE
```

### Gaps

None.

## Amendment January 2026 — A web page ordin names (amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the Settings walk-through's Telegram and Jira steps send the person to a page they must find by hand (ScreensModule.md G-SW-1). Counterparts: ServiceApi.md SVC-WEB-1, phase-2c-shared-types-wire.md WIRE-WEB-1, ScreensModule.md SM-SW-5 and SM-SW-11, ErrorHandling.md `CRED_BROWSER_FAILED`. Every section above stays in force.

### Internal Logic (additions)

**DSK-WEB-1 `app.open_web_page`.** A person-only command (ServiceApi.md SVC-WEB-1). The input names a page, never an address; the program maps it to a fixed address compiled into it: `telegram_botfather` → `https://t.me/BotFather`, `atlassian_api_tokens` → `https://id.atlassian.com/manage-profile/security/api-tokens`, `claude_code_install` → `https://claude.com/claude-code` (Claude Code's official install page, ScreensModule.md SM-FS-9; amended January 2026 (user decision; Approved by the user January 2026)). It opens the address with the `UrlOpener` over `tauri-plugin-opener` that `app.open_slack_link` uses (`DesktopModules.url_opener`) and answers `{}`. When the operating system does not open it, the answer is `CRED_BROWSER_FAILED` (retryable, action `try_again`), as DSK-REC-38 states for `app.open_slack_link`, logged `warn` as `link_open_failed` with the command name and the page's name; the address is logged nowhere. Without a `UrlOpener` (a program built with no window) the answer is the module's `not_wired` refusal, as for `app.open_slack_link`. amended January 2026 (user decision; Approved by the user January 2026).

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The operating system does not open the page's address (DSK-WEB-1) | `Err(retryable)` | `CRED_BROWSER_FAILED` | `{"kind":"error","error":{"code":"CRED_BROWSER_FAILED","message":"ordin couldn't open your web browser. Open it yourself and try again.","action":"try_again"}}` |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: A web page ordin names (DSK-WEB-1)

  Scenario: Happy path — BotFather opens
    When app.open_web_page is called with {"page": "telegram_botfather"}
    Then https://t.me/BotFather is opened in the person's browser and the reply is {}

  Scenario: Error path — the browser does not open
    Given the operating system refuses to open a link
    When app.open_web_page is called with {"page": "atlassian_api_tokens"}
    Then the reply is CRED_BROWSER_FAILED with action try_again
```

### Gaps

None.

## Amendment January 2026 — Claude Code found by the Setup Assistant's rule, and no sentence sends the person to IT

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: ordin opened from Finder or the Dock gets the system's short `PATH`; the desktop program found the Claude adapter's `claude` in the person's own folders with its own copy of the folder list, while the Setup Assistant's probe searched `PATH` only and reported Claude Code missing (SetupAssistant.md, amendment January 2026, SET-R1-7). Separately, the person's rule that every feature is self-service. Every section above stays in force except where this amendment changes it.

- **DSK-CC-1 One rule for where `claude` is.** Before start, the shell sets `CoreInputs.claude_path` (OQ-REC-4 option (b)) to `ordin_setup::find_claude_code(PATH, home)`, with `PATH` and the home folder (`HOME`, `USERPROFILE` on Windows) of the program's own environment: `claude` on `PATH`, else in the person's own folders of SetupAssistant.md SET-R1-7. The desktop program keeps no folder list of its own. When nothing is found the path stays the bare name `claude` and start shows the Claude Code message box as before. The Claude Local Adapter puts the folder of that path on the `PATH` of every child it starts (ClaudeLocalAdapter.md CLA-PATH-1), as the Setup Assistant's probe does (SET-R1-8).
- **DSK-IT-1 The program's own sentences.** The start-failure message box (step 9) reads "ordin couldn't start on this computer. Open it again; if this happens again, reinstall ordin."; the memory models' sentences of the 2026-01-28 amendment that told the person to ask or tell IT read as its Error Table now states; `error-dsk-start-failed` reads "ordin couldn't start. Close any other ordin window, then try again; if it keeps happening, reinstall ordin." and `error-dsk-writes-not-attributed` "ordin couldn't start because it can't record who made changes. Reinstall ordin." No sentence of the program tells the person to ask, tell or wait for IT (SetupAssistant.md SET-R1-9). Codes do not change.

```gherkin
  Scenario: Happy path — Claude Code in the person's own folder is used when ordin is opened from Finder (DSK-CC-1)
    Given claude is only in <home>/.local/bin and PATH is "/usr/bin:/bin:/usr/sbin:/sbin"
    When ordin starts
    Then CoreInputs.claude_path is <home>/.local/bin/claude
    And the Setup Assistant's Harness step finds the same file

  Scenario: Happy path — Install Claude Code opens the official page (DSK-WEB-1)
    When app.open_web_page is called with page "claude_code_install"
    Then the UrlOpener opens "https://claude.com/claude-code"
```

### Gaps

None.

## Amendment January 2026 — Settings › Memory's settings (DSK-MEM-1; amended January 2026; user decision; Approved by the user January 2026)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). It amends DSK-REC-37 for the section `what_ordin_remembers` only; every other section stays in force. Callers: `ScreensModule.md` SM-MS-2 and SM-MS-4. Enforcers: `MemoryService.md` MEM-SET-1 to MEM-SET-5, `OrdinHost.md` OH-MS-1.
>
> Evidence read: `apps/desktop/src/prefs.rs` (`DesktopPreferences`, `Preferences::section`, `change`, `write_atomically`); `apps/desktop/src/service_api.rs` (`settings_get`, `settings_change`); `apps/desktop/src/wiring.rs` (`memory_row`); `crates/ordin-memory-service/src/service.rs` (`set_recall_log_days`, the one other setting the program gives the service).

- **DSK-MEM-1 The section `what_ordin_remembers` (amended January 2026; user decision; Approved by the user January 2026).** `preferences.json` gains `memory: { keep: "on_its_own" | "ask_first", learn_from_conversations: bool, learn_from_documents: bool, suggest_removal_after_days: null | 1–3650, use_everywhere: bool, say_which_facts: bool }`, absent in an older file and then read as the defaults `on_its_own`, `true`, `true`, `null`, `true`, `false` (today's behaviour); `format` stays 1. `settings.get { section: what_ordin_remembers }` answers six `SettingView`s in the order `memory.keep_mode`, `memory.learn_from_conversations`, `memory.learn_from_documents`, `memory.suggest_removal_after_days`, `memory.use_everywhere`, `memory.say_which_facts`, each `set_by: person`, `person_may_change: true`. `settings.change` of one of those keys checks the value (a wrong type, an unknown `keep`, or a period outside 1–3,650 days → `DSK_PREFERENCE_VALUE_INVALID`, nothing written), writes the file as every change does, then gives the whole `MemorySettings` to the Memory Service (`MemoryService::set_settings`), so the next write and recall follow it. At start, `memory_row` gives the saved settings to the service before the first write or recall. A refusal by the service there is not expected (the bounds are the same); it is logged `memory_settings_not_applied { code }` and the saved value is given again at the next start. The personal server and the agent profile do not answer this section; their Memory Service keeps the defaults.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `settings.change` of a `memory.*` key with a value of the wrong type, an unknown `keep`, or a period outside 1–3,650 days (DSK-MEM-1) | `Err(permanent)` | `DSK_PREFERENCE_VALUE_INVALID` | as for the other keys |
| `preferences.json` cannot be written after a `memory.*` change (DSK-MEM-1) | `Err(retryable)` | `DSK_PREFERENCES_WRITE_FAILED` | as for the other keys; the Memory Service keeps the settings it had |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Settings › Memory's settings (DSK-MEM-1)

  Scenario: Happy path — the defaults, and an older file
    Given a preferences.json written before this amendment
    When settings.get of what_ordin_remembers is sent
    Then the six settings answer today's behaviour and the notifications keep their values

  Scenario: Happy path — a change is saved and handed to the Memory Service
    When settings.change { key: "memory.use_everywhere", value: false } is sent
    Then preferences.json holds it and the Memory Service's next recall uses each workspace's own facts only

  Scenario: Error — a value out of bounds
    When settings.change { key: "memory.suggest_removal_after_days", value: 0 } is sent
    Then it fails with DSK_PREFERENCE_VALUE_INVALID and nothing is written
```

### Gaps

None.

## Amendment January 2026 — GitHub with a personal access token (amended January 2026; user decision; Approved by the user January 2026)

- **DSK-GH-PAT-1 `identity.save_github_token`.** The program passes the token, wrapped in `SecretString` at the bridge, to `IdentityService::save_github_token` and answers with the stored credential's `CredentialView`, as `identity.save_gitlab_token` does; an Identity error maps as any module error. The GitHub App client id stays a build input (`option_env!("GITHUB_APP_CLIENT_ID")`, set by the release pipeline from `RELEASE_GITHUB_APP_CLIENT_ID`, phase-5-playbook.md PB-REL-31). `app.open_web_page { page: github_tokens }` opens `https://github.com/settings/tokens` (DSK-WEB-1).

### Error Table

No code is added; Identity's rows apply.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: GitHub's token page (DSK-GH-PAT-1)

  Scenario: Happy path — the token page opens at its fixed address
    When app.open_web_page is called with {"page": "github_tokens"}
    Then the browser opens https://github.com/settings/tokens
```

### Gaps

None.

## Amendment January 2026 — Settings › Agents and models (DSK-AGM-1) (amended January 2026; user decision; Approved by the user January 2026)

- **DSK-AGM-1 The person's harness, models and efforts (amended January 2026; user decision; Approved by the user January 2026).** `preferences.json` gains `agents: { harness?: "claude_code" | "copilot", models?: { <harness>: { <level>: <model name> } }, effort?: { <harness>: { <kind>: <effort> } } }`, absent in an older file and then read as every choice on its default; `format` stays 1. Each absent entry means "use the default". Precedence, for every choice: the person's, then the organisation profile's, then the built-in default. The program answers `app.agent_settings` and `app.change_agent_setting` (`components/ServiceApi.md` SVC-AGM-1) from `apps/desktop/src/agent_settings.rs`:
  - **Checks.** A model name is 1–100 ASCII letters, digits, `.`, `_` and `-`, for the levels `smaller`, `standard` and `most_capable` only. An effort is one the harness offers: Claude Code's `--effort` values `low`, `medium`, `high`, `xhigh`, `max`; Copilot offers none in this version. Anything else → `DSK_PREFERENCE_VALUE_INVALID`, nothing written. A `null` value removes the entry. The file is written as every change is (`DSK_PREFERENCES_WRITE_FAILED` when it cannot be).
  - **Models and efforts.** At start (`usage_row`) and after every change, the program gives `PersonalModelChoices` to the Usage Planner (`set_personal_choices`, `components/UsagePlanner.md` SA-AGM-01), so the next step of every run uses them.
  - **Harness.** At start (`supervisor_row`) the harness is the person's when this version can run it, else `ProfileSnapshot::default_harness()`; the Session Supervisor's `local_adapter`, the Ordin Host's `harness` and the Package Catalogue's `HarnessSettings` (`ProfileHarness.chosen`) all take it. Only Claude Code runs in this version: the Copilot adapter is behind the `copilot` build switch and not wired, so a person's choice of Copilot is kept but the organisation's harness is used. A changed harness takes effect when the program next starts; the view's `harness_in_use` names the one in use now.
  - **Readiness.** From the Setup Assistant's `harness_attachments()`: a harness this version cannot run → `not_in_this_version`; no attachment → `not_found`; an attachment in state `ready` → `ready`; otherwise `needs_set_up`. An unreadable list reads as no attachment; it never blocks the section.
  - **Defaults shown.** Each level's `default_model` is `OrganisationProfileDocument::level_model(harness, level)`: the profile's `usage.level_models`, else Claude Code's `haiku`, `sonnet`, `opus`; absent for Copilot, whose own selection then applies.

| Condition | Result | Code | Retry |
|---|---|---|---|
| `app.change_agent_setting` with an effort the harness does not offer, or a model for the `automatic` level (DSK-AGM-1) | `Err(permanent)` | `DSK_PREFERENCE_VALUE_INVALID` | as for the other keys |
| `preferences.json` cannot be written after an agents change (DSK-AGM-1) | `Err(retryable)` | `DSK_PREFERENCES_WRITE_FAILED` | as for the other keys; the Usage Planner keeps the choices it had |

```gherkin
Feature: Settings › Agents and models (DSK-AGM-1)

  Scenario: Happy path — a choice is kept and reaches the next step
    Given the person chose "high" effort for Code on Claude Code
    Then preferences.json holds it under agents.effort.claude_code.code
    And the Usage Planner is given it at once and at every start

  Scenario: Error path — an effort the harness does not offer
    When the person sends effort "extreme", or any effort for Copilot
    Then the answer is DSK_PREFERENCE_VALUE_INVALID and nothing is written

  Scenario: Edge case — Copilot chosen in a version that cannot run it
    Given the person chose Copilot
    When the program starts
    Then agents run with the organisation's harness and the choice is kept
```
- **DSK-NTF-1 Notifications are raised, and only their switches are listed (amended January 2026; user decision; Approved by the user January 2026).** Step 7 was not built: the five `notifications.*` switches were saved to `preferences.json` and read by nothing. The program now runs step 7 for the kinds whose events exist in every build: `ApprovalRequested` (`approvals`), `RunStateChanged` to `completed` and `CrewRoundChanged` to `finished` (`finished_work`), and a `BlockShown` question (`questions`), from `CommandBridge::subscribe_all()` on the local backend and each connected server. `apps/desktop/src/notifier.rs` maps the event (`Notifier::notice`); `apps/desktop/src/window.rs` holds an approval for 1.5 s and drops it when `ApprovalDecided` arrived meanwhile, reads the switch from `preferences.json` at the moment of showing, skips the notice while the main window is focused, visible and not minimised, and shows it with `tauri_plugin_notification`. A run's title is `notification-run-done-title-unnamed` and its body `notification-done-body` (the agent and workspace lookups of step 7.1 are not made); a crew's is `notification-crew-done-title`; an approval's is `notification-approval-title` with the brand's coordinator name as `$coordinator` and the card's question, body `notification-approval-body`; a question's is `notification-question-title`, body the question's first 120 characters; a server's notice wraps its body in `notification-on-server`. A refusal by the operating system is logged `DSK_NOTIFY_FAILED` and not retried. A shown notice is kept as the pending target; when the main window next gains focus within five minutes (pressing the notice brings the program forward), the program un-minimises, shows and focuses the window and emits `ordin://open-target` once with that notice's `NotificationTarget`: `approval { backend, workspace_id, approval_id }`, `run { backend, run_id }`, `crew_run { backend, crew_run_id }` or `question { backend, workspace_id, block_id }`. The screens (`openNotificationTarget` in `apps/desktop/ui/src/lib/shell/open-item.ts`, registered by `App.svelte`) open the approval's card in its workspace, the run's or the crew's screen, or the question's workspace, whose conversation shows it; any other payload opens nothing. The handover (step 7.1, `NeedsYouChanged`) and draft-review (step 7.6) notices are not raised: their modules are not built. `settings.get { section: notifications }` therefore answers only `notifications.approvals`, `notifications.finished_work` and `notifications.questions` (`notifier::RAISED`), so no switch is drawn for a notice that is never shown; `settings.change` still accepts all five keys, and the stored values are kept.

## Amendment January 2026 — A dashboard saved as a file, or added to a presentation (DSK-DBX-1 to DSK-DBX-4) (amended January 2026; user decision; Approved by the user January 2026)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Owners: `components/ServiceApi.md` SVC-DBX-1 (the commands), `components/ScreensModule.md` SM-DB-5 to SM-DB-8 (the menus), `components/JournalAndArtefactStore.md` JA-DB-2 (what is read). Everything runs on this computer, with no other program and no service. Code: `apps/desktop/src/dashboard_export/` and `apps/desktop/src/service_api/dashboards.rs`.

- **DSK-DBX-1 `dashboards.export`.** The program reads `Journal::dashboard(dashboard_id)` (absent → `DSK_DASHBOARD_NOT_FOUND`) and the report of the last of its `refresh_runs` that has one (`Journal::report_for_run`, newest first; none → `DSK_DASHBOARD_NO_REPORT`), the report the dashboard's screen shows (ScreensModule.md SM-DB-4). It writes the file in memory, then opens the save dialog with `<dashboard name>.<pdf|pptx|xlsx|docx>` proposed (each of `/ \ : * ? " < > |` and control characters replaced by a space; `Dashboard` when nothing is left) and writes it atomically, as `files.save_export` does. Cancelled → `DSK_DIALOG_CANCELLED`; a write failure → `DSK_SAVE_COPY_FAILED` (retryable). What each file holds — every part is a table: a table part as it is; a chart part as the table of the rows it is drawn from (the screen draws no chart image the program could reuse); key figures as one row per figure with the columns `dashboard-export-figure`, `dashboard-export-value`, `dashboard-export-unit`. A money value is shown with two decimal places and its currency code, a percentage as a percentage of 100, a number with its column's decimals; the title is the dashboard's name and the line under it is `dashboard-as-of` with the report's `as_of`, in the active locale.
  - **Excel (`.xlsx`).** One sheet per part, named after its heading (`[ ] : * ? / \` removed, no apostrophe at either end, at most 31 characters, ` (2)`, ` (3)`… added to a name already used regardless of case, `Sheet N` when empty); row 1 the column names in bold, then one row per data row; numbers stored as numbers, a percentage column in Excel's percentage format, a money or decimals column with two decimal places (none for zero decimals); an empty value is an empty cell. A report with no part gives one sheet holding the title and the as-of line.
  - **PowerPoint (`.pptx`).** 16:9. A title slide with the name and the as-of line, then one slide per part with its heading and a table; a part of more than 12 rows goes on over as many slides as it needs, each repeating the column names.
  - **Word (`.docx`).** A4. The name in the `Title` style, the as-of line in `Subtitle`, then per part its heading in `Heading 1` and a table whose first row repeats on each page.
  - **PDF.** A4 portrait, 50-point margins, Helvetica and Helvetica-Bold (standard fonts, none embedded, WinAnsi encoding; a character outside it is shown as `?`). The title, the as-of line, then each part's heading and its table; a cell wider than its column is cut and ends with `...`; a table that reaches the foot of a page goes on to the next with its column names again.
- **DSK-DBX-2 `dashboards.add_to_presentation`.** As DSK-DBX-1 for the report. The program opens the file chooser titled `dashboard-export-pick-presentation` (cancelled → `DSK_DIALOG_CANCELLED`; it cannot open → `DSK_PICKER_UNAVAILABLE`). A file over 1 GiB, one that cannot be read, is not a zip package (a presentation protected with a password is not), holds an encrypted entry or lacks `ppt/presentation.xml`, its relationships, `[Content_Types].xml`, `<p:sldSz>` or any slide layout → `DSK_PRESENTATION_UNREADABLE`, nothing written. Otherwise the new slides — DSK-DBX-1's PowerPoint slides, sized to the presentation's own slide size — are added after its last slide: numbered after its highest `ppt/slides/slideN.xml`, with `<p:sldId>` ids after its highest and relationship ids it does not use, each using its layout of type `blank`, else its first layout by number. Every other part is copied unchanged; only `ppt/presentation.xml`, `ppt/_rels/presentation.xml.rels` and `[Content_Types].xml` gain the new entries. The result is `PresentationUpdated { saved_path, as_copy }`.
- **DSK-DBX-3 Writing in place, or a copy.** The presentation is replaced atomically (temporary file in the same folder, then rename), so it is either exactly as it was or holds every new slide: `as_copy: false`. When PowerPoint has it open — its owner file `~$<file name>` lies beside it — or the replacement fails, the original is left untouched and the save dialog opens with `dashboard-export-copy-name` and `.pptx` proposed; the copy is written atomically: `as_copy: true`, logged `info` as `presentation_saved_as_copy`. That dialog cancelled → `DSK_DIALOG_CANCELLED`; its write failure → `DSK_SAVE_COPY_FAILED`.
- **DSK-DBX-4 Nothing is logged of the report or the file.** No name, path, cell or byte of the report or of a file is logged.

| Condition | Result | Code | Response |
|---|---|---|---|
| The dashboard is not in the Journal (DSK-DBX-1) | `Err(permanent)` | `DSK_DASHBOARD_NOT_FOUND` | `{"kind":"error","error":{"code":"DSK_DASHBOARD_NOT_FOUND","message":"That dashboard can't be found. It may have been removed."}}` |
| No refresh run of the dashboard has a report (DSK-DBX-1) | `Err(permanent)` | `DSK_DASHBOARD_NO_REPORT` | `{"kind":"error","error":{"code":"DSK_DASHBOARD_NO_REPORT","message":"This dashboard has no report to save yet. Refresh it, then try again."}}` |
| The picked file is not a presentation slides can be added to (DSK-DBX-2) | `Err(permanent)` | `DSK_PRESENTATION_UNREADABLE` | `{"kind":"error","error":{"code":"DSK_PRESENTATION_UNREADABLE","message":"ordin can't add slides to that file. Choose a PowerPoint presentation (.pptx) that isn't protected with a password."}}` |
| The save dialog or the file chooser is cancelled (DSK-DBX-1 to DSK-DBX-3) | `Err(permanent)` | `DSK_DIALOG_CANCELLED` | as for `files.save_export` |
| The file chooser cannot open (DSK-DBX-2) | `Err(retryable)` | `DSK_PICKER_UNAVAILABLE` | as for `app.pick_folder` |
| The file or the copy cannot be written (DSK-DBX-1, DSK-DBX-3) | `Err(retryable)` | `DSK_SAVE_COPY_FAILED` | as for `files.save_export` |

```gherkin
Feature: A dashboard saved on this computer (DSK-DBX-1 to DSK-DBX-4)

  Scenario: Happy path — every format opens with its structure (DSK-DBX-1)
    Given the dashboard "Prices" whose report has a table, a chart and key figures
    When it is saved as Excel, Word, PowerPoint and PDF
    Then the workbook has three sheets, the document a title and three tables, the presentation a title slide and three table slides, and the PDF one page

  Scenario: Happy path — slides added to an existing presentation keep its own (DSK-DBX-2)
    Given a presentation of two slides
    When "Prices" is added to it
    Then it holds six slides, its first two unchanged and in their order, and every part it had is unchanged

  Scenario: Edge case — the presentation is open in PowerPoint (DSK-DBX-3)
    Given ~$Board pack.pptx lies beside Board pack.pptx
    Then Board pack.pptx is not changed and the save dialog offers a copy

  Scenario: Error path — a file that is not a presentation (DSK-DBX-2)
    When the person picks a workbook
    Then the answer is DSK_PRESENTATION_UNREADABLE and nothing is written

  Scenario: Error path — no report yet (DSK-DBX-1)
    Given a dashboard whose refresh has not made a report
    Then the answer is DSK_DASHBOARD_NO_REPORT and no dialog opens
```

### Gaps

None.

## Amendment January 2026 — A knowledge page saved as a PDF or Word document (DSK-KEX-1, DSK-KEX-2) (amended January 2026; user decision; Approved by the user January 2026)

> **Status:** amended January 2026; user decision; Approved by the user January 2026. Owners: `components/ServiceApi.md` SVC-KEX-1 (the command), `components/ScreensModule.md` SM-KN-3 (the menu). Everything runs on this computer, with no other program and no service, and with no crate added: the page's Markdown is read with `pulldown-cmark`, the reader the workspace already uses, and written by the dashboard's PDF and Word writers (DSK-DBX-1). Code: `apps/desktop/src/dashboard_export/page.rs`, `apps/desktop/src/dashboard_export/docx.rs`, `apps/desktop/src/dashboard_export/pdf.rs` and `apps/desktop/src/service_api/knowledge_export.rs`.

- **DSK-KEX-1 `knowledge.export`.** The program reads the page by its id with the Knowledge Service's `get_page`, as `knowledge.get` does (a page it does not hold → its own `KNOW_PAGE_NOT_FOUND`; no code is added). The screen sends no content, title or path. It writes the file in memory, then opens the save dialog with `<page title>.<pdf|docx>` proposed (each of `/ \ : * ? " < > |` and control characters replaced by a space; `Page` when nothing is left) and writes it atomically, as `dashboards.export` does. Cancelled → `DSK_DIALOG_CANCELLED`; a write failure → `DSK_SAVE_COPY_FAILED` (retryable). The result is `SavedCopy { saved_path }`. The page's title is the document's title (the first line, and the Word file's `dc:title`); the Markdown body (CommonMark with tables, task lists, page links `[[…]]` and YAML front matter) is mapped as follows:
  - a heading of level 1, 2 or 3 → `Heading 1`, `Heading 2` or `Heading 3`; a deeper one → `Heading 3`;
  - a paragraph → a paragraph, keeping bold, italic and inline code (monospace); a line break stays a line break;
  - a bulleted or numbered list → a real list, each nested list a level down, a numbered list starting at its own first number;
  - a table → a real table whose first row is its header row (repeated on each page);
  - a code block → monospace lines;
  - a link → its text followed by its address in brackets, `text (https://…)`; a page link `[[Page]]` or `[[Page|shown]]`, an address-only link and an in-page `#` link → their text only; an image → its words and its address;
  - a block quote → its paragraphs in italic; a horizontal rule → a rule; raw HTML and the front matter are left out.
  - **Word (`.docx`).** A4, the dashboard file's styles with `Heading 2`, `Heading 3`, `List Paragraph` and `Code` (Courier New) added; the lists numbered by `word/numbering.xml`, one bulleted and one numbered definition of nine levels, and one numbering per list.
  - **PDF.** A4 portrait, 50-point margins, the dashboard PDF's standard fonts (none embedded, WinAnsi encoding; a character outside it is shown as `?`) with Helvetica-Oblique, Helvetica-BoldOblique and Courier added for italic, bold italic and code. Lines wrap at the margin and a page goes on to the next; a list item is indented by its depth with a bullet or its number; a table cell wider than its column is cut and ends with `...`, and a table that reaches the foot of a page goes on to the next with its header row again; a code line longer than the width goes on to the next line.
- **DSK-KEX-2 Nothing is logged of the page or the file.** No title, path, word or byte of the page or of the file is logged.

| Condition | Result | Code | Response |
|---|---|---|---|
| The Knowledge Service does not hold the page (DSK-KEX-1) | `Err(permanent)` | `KNOW_PAGE_NOT_FOUND` | as for `knowledge.get` |
| The save dialog is cancelled (DSK-KEX-1) | `Err(permanent)` | `DSK_DIALOG_CANCELLED` | as for `files.save_export` |
| The file cannot be written (DSK-KEX-1) | `Err(retryable)` | `DSK_SAVE_COPY_FAILED` | as for `files.save_export` |

```gherkin
Feature: A knowledge page saved on this computer (DSK-KEX-1, DSK-KEX-2)

  Scenario: Happy path — the Word file has real headings, lists and tables (DSK-KEX-1)
    Given a page "Supplier notes" with headings of three levels, a numbered list holding a bulleted one, a bulleted list, a table and a code block
    When it is saved as Word
    Then the document has the title, one paragraph in each of Heading 1, Heading 2 and Heading 3, five numbered list paragraphs over three lists, one table with a repeated header row, and the code in Courier New

  Scenario: Happy path — the PDF is a paged document (DSK-KEX-1)
    When the page is saved as PDF
    Then the file starts with %PDF-1.4, has one page, and draws bold, italic and code in their own fonts

  Scenario: Edge case — a long page goes on over several pages (DSK-KEX-1)
    Given a page of 300 paragraphs
    Then its PDF has at least five pages

  Scenario: Error path — the person cancels the save dialog (DSK-KEX-1)
    Then the answer is DSK_DIALOG_CANCELLED and nothing is written
```

### Gaps

None.

## Amendment January 2026 — GitHub with an SSH key (DSK-SSH-1 to DSK-SSH-3) (amended January 2026; user decision; Approved by the user January 2026)

- **DSK-SSH-1 The choice (amended January 2026; user decision; Approved by the user January 2026).** `preferences.json` gains `github_ssh_key`, the path of the chosen key, absent when none is chosen; `format` stays 1. The key itself is never stored, read or logged. At start (the Project Service's row) the stored path is handed to `ordin_projects::ssh::set_chosen_key` (ProjectService.md PROJ-SSH-1). `app.ssh_keys` answers the keys of the person's SSH folder (`$HOME/.ssh`, else `%USERPROFILE%\.ssh`; ProjectService.md PROJ-SSH-3) and the stored path. `app.choose_ssh_key` accepts only the path of a listed key, else `DSK_SSH_KEY_UNKNOWN`, or `null`; it stores the choice (`DSK_PREFERENCES_WRITE_FAILED` when the file cannot be written) and hands it to git at once. The answers are built in `apps/desktop/src/service_api/ssh_keys.rs`.
- **DSK-SSH-2 Making a key (amended January 2026; user decision; Approved by the user January 2026).** `app.create_ssh_key` makes a key in the SSH folder (ProjectService.md PROJ-SSH-4) on a blocking thread and chooses it. No `ssh-keygen` → `DSK_SSH_KEYGEN_MISSING`; a failed run → `DSK_SSH_KEY_NOT_MADE`. Each is logged as `ssh_key_not_made` with its code only.
- **DSK-SSH-3 Checking a key (amended January 2026; user decision; Approved by the user January 2026).** `app.check_ssh_key` with no key chosen → `DSK_SSH_NO_KEY`; otherwise it runs ProjectService.md PROJ-SSH-5 with a limit of 8 seconds, within the command's 10-second budget, and answers the outcome. `app.open_web_page { page: github_ssh_keys }` opens `https://github.com/settings/ssh/new`.

| Condition | Result | Code | Notes |
|---|---|---|---|
| `app.create_ssh_key` on a computer with no `ssh-keygen` (DSK-SSH-2) | `Err(permanent)` | `DSK_SSH_KEYGEN_MISSING` | the person chooses an existing key |
| `ssh-keygen` ran and made no key (DSK-SSH-2) | `Err(retryable)` | `DSK_SSH_KEY_NOT_MADE` | **Try again** |
| `app.choose_ssh_key` with a path that is not a listed key (DSK-SSH-1) | `Err(permanent)` | `DSK_SSH_KEY_UNKNOWN` | — |
| `app.check_ssh_key` with no key chosen (DSK-SSH-3) | `Err(permanent)` | `DSK_SSH_NO_KEY` | — |


## Amendment January 2026 — Meeting minutes (DSK-MIN-1 to DSK-MIN-6) (amended January 2026; user decision; Approved by the user January 2026)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Every section above stays in force. Modules: `components/TranscriptionService.md`, `components/ObsRecorderAdapter.md`, `components/MinutesService.md`; amendments `components/ServiceApi.md` SVC-MIN-1 to SVC-MIN-4, `components/OrdinHost.md` OH-MIN-1 to OH-MIN-3, `components/IdentityAndCredentials.md` ID-OBS-1, `components/JournalAndArtefactStore.md` JA-MIN-1 to JA-MIN-3, `components/ScreensModule.md` SM-MIN-1 to SM-MIN-10.
>
> Evidence read: `apps/desktop/Cargo.toml:19-32` (`[features]`: `default = ["team-knowledge"]`, `dictation = ["dep:ordin-dictation"]` at :22, `handover` composing `mail` and `slack` at :28); `crates/ordin-dictation/Cargo.toml` (depends on `ordin-types`, `ordin-localisation`, `tracing` only); `apps/desktop/src/window.rs` (`tauri_plugin_dialog` :17; the journeys answer file :60–:62); this file's DSK-UP-1 to DSK-UP-7, step 3's rows 3.16 and 3.41, DSK-KEX-1.

- **DSK-MIN-1 The `minutes` build switch (amended January 2026; user decision; Approved by the user January 2026).** `[features]` gains

```toml
minutes = ["dictation", "ordin-dictation/transcription", "ordin-host/minutes", "dep:ordin-minutes", "dep:ordin-obs"]  # Steps 80, 81, 82
```

  It turns on `dictation`, because the Transcription Service lives in `ordin-dictation`; it is not in `default`. DSK-UP-1's table gains the row `minutes` (turns on `dictation`) | `ordin-minutes`, `ordin-obs` | 3.16a, 3.16b, 3.41a. Step 3 gains, each behind `#[cfg(feature = "minutes")]`: **3.16a** `TranscriptionService::new(<data_root>/speech-models, Localisation, <the program's reqwest client>)`; **3.16b** `ObsRecorder::new(Identity, Localisation)`; **3.41a**, right after the Ordin Host (3.41): `MinutesService::new(<data_root>/minutes, transcription, recorder, journal, localisation, <MinutesWriterOverHost>)`, then `run(shutdown token)` spawned in step 4. `MinutesWriterOverHost` implements `MinutesWriter`: `writer_name` → `OrdinHost::minutes_writer_name`; `write` → `OrdinHost::write_minutes` with `WriteMinutesRun` from the request, mapping `OhError` to `MinutesError::WriteFailed` (logged with the host's code). The workspace removal path calls `MinutesService::workspace_removed` after the Ordin Host's `workspace_removed`. A build without `minutes` constructs none of them, registers no `minutes` group (DSK-UP-7) and draws no Minutes entry (SM-UP-1).
- **DSK-MIN-2 `minutes.import` (amended January 2026; user decision; Approved by the user January 2026).** The program opens the operating system's file dialog (`tauri_plugin_dialog`, with the journeys answer file in journey builds) titled with `minutes-import-title`, one file, filters "Transcripts and recordings" (`vtt`, `docx`, `txt`, `m4a`, `mp3`, `wav`). Cancelled → `DSK_DIALOG_CANCELLED`. Otherwise `MinutesService::import(workspace_id, path)`; its reply is the result.
- **DSK-MIN-3 `minutes.save_obs_password` (amended January 2026; user decision; Approved by the user January 2026).** Calls `IdentityService::save_obs_password(SecretString::new(password))` (ID-OBS-1); an empty password is refused at the boundary with `SVC_INPUT_INVALID`. A new password replaces the stored one.
- **DSK-MIN-4 `minutes.export` (amended January 2026; user decision; Approved by the user January 2026).** Reads the minutes with the Journal's `minutes(id)` (absent → `MIN_NOT_FOUND`), then writes the PDF or Word file with the same writer as `knowledge.export` (DSK-KEX-1): the minutes' `title` is the document's title and its `body` the Markdown mapped as DSK-KEX-1 maps a page's body; the save dialog proposes `<title>.<pdf|docx>` with DSK-KEX-1's character replacement. Cancelled → `DSK_DIALOG_CANCELLED`; a write failure → `DSK_SAVE_COPY_FAILED`. Result `SavedCopy { saved_path }`. The writer is called through one function shared by both rows, so a fix to one is a fix to both.
- **DSK-MIN-5 Quit and sleep (amended January 2026; user decision; Approved by the user January 2026).** On quit (step 10) the program calls `MinutesService::shutdown()` before the Journal shuts down, inside the existing quit bounds; on sleep it does nothing, so a recording continues in OBS while the computer is awake again.
- **DSK-MIN-6 Minutes on by default, without dictation (user decision 2026-11-22: meeting minutes ships in release 1).** Replaces DSK-MIN-1's switch line and its sentence "It turns on `dictation` … it is not in `default`". `[features]` reads

```toml
default = ["team-knowledge", "minutes"]
minutes = ["dep:ordin-dictation", "ordin-dictation/transcription", "ordin-host/minutes", "ordin-service-api/minutes", "dep:ordin-minutes", "dep:ordin-obs"]  # Steps 80, 81, 82
```

  `minutes` depends on the `ordin-dictation` crate for the Transcription Service only and does not turn the `dictation` switch on, so a default build constructs no row 3.16, registers no `dictation` group and logs no `dictation` feature. DSK-UP-1's row `minutes` loses "(turns on `dictation`)". A build with `--no-default-features` (or one naming its switches without `minutes`) constructs none of the minutes rows, as DSK-MIN-1 says for a build without the switch.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The file or save dialog is cancelled | `Err(permanent)` | `DSK_DIALOG_CANCELLED` | as for `files.save_export` |
| The exported file cannot be written | `Err(retryable)` | `DSK_SAVE_COPY_FAILED` | as for `files.save_export` |
| The minutes to export are not in the Journal | `Err(permanent)` | `MIN_NOT_FOUND` | as MinutesService.md |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Meeting minutes in the desktop program (DSK-MIN-1 to DSK-MIN-5)

  Scenario: Happy path — a build with minutes wires the three modules
    Given a build with the minutes switch (the default build, DSK-MIN-6)
    Then the start log's features include "minutes" and do not include "dictation"
    And service.hello's groups include "minutes"

  Scenario: Edge — a build without the switch
    Given a build with --no-default-features
    Then no Minutes Service, Transcription Service or OBS Recorder is constructed and "minutes" is not in groups

  Scenario: Happy path — minutes saved as Word
    Given minutes titled "Supplier call"
    When minutes.export is sent with format docx and the dialog saves
    Then a Word document titled "Supplier call" with Heading 2 paragraphs Summary, Decisions, Actions and Open questions is written

  Scenario: Error — the import dialog is cancelled
    When minutes.import is sent and the person cancels the dialog
    Then the reply is DSK_DIALOG_CANCELLED and no job is created
```

### Gaps

None.

## Amendment 2026-11-21 — The minutes port for Ordin's tools (DSK-MIN-6)

> **Status:** Approved by the user, 2026-11-21 ("approve as written"). Every section above stays in force. Built only with `minutes`.

- **DSK-MIN-6 `MinutesPort`.** The program implements OrdinHost.md's `MinutesPort` (OH-MIN-5) and gives it to the Ordin Host at start (row 3.41a's wiring, beside `MinutesWriter`). `list`, `get` → the Journal's `list_minutes` and `minutes` (as `minutes.list` and `minutes.get`); `transcript`, `save_edit`, `jobs`, `cancel_job`, `retry`, `transcription` → the Minutes Service's methods of those names; `import` and `transcribe` → the Minutes Service's `import` and `transcribe` with the absolute path. A `LocalFile { place_id, path }` becomes an absolute path by joining `path` to the place's `path` (WorkspaceService.md `place`), refusing with `OH_MINUTES_PATH_INVALID` a result that, once its links are resolved, is not inside the place's folder, or a place that is not a folder on this computer (OH-MIN-6). `export` writes with DSK-MIN-4's writer, to the joined path with OH-MIN-8's naming and never over an existing file; a write failure → `DSK_SAVE_COPY_FAILED`. Errors keep their owners' codes in `DepError`.

## Amendment 2026-02-02 — The Copilot CLI is the person's own, not a bundled program (DSK-CPX-1 to DSK-CPX-5)

> **Status:** Approved by the user, 2026-02-02 ("approve, and allow Copilot without a company profile"). Source: user decision 2026-02-02, "copilot installation should be a pre-requisite to enable copilot harness in ordin"; HLD ADR-123. **derivedFromHld:** 0.73.0. Every section above stays in force except where this amendment changes it.

- **DSK-CPX-1 One bundled program.** *Bundled programs* reads: `binaries/ordin-hook` only, found beside the executable as before. `tauri.conf.json` reads `"bundle": { "externalBin": ["binaries/ordin-hook"] }` in every build. DSK-UP-3 is withdrawn: no build carries `binaries/copilot`, so the bundling target needs no configuration per switch for it.
- **DSK-CPX-2 No Copilot path beside the executable.** `DesktopPaths` loses `copilot_cli`. The Error Table row *A bundled program is missing beside the executable* names `ordin-hook` only. DSK-UP-5's row `DesktopPaths::copilot_cli` is withdrawn. The Gherkin scenario *Error path — a missing bundled program* reads: Given the `ordin-hook` program is missing beside the executable; When the person opens ordin; Then `DSK_SIDECAR_MISSING` is logged naming "ordin-hook"; And the window is still shown.
- **DSK-CPX-3 The Copilot `HarnessProbe`'s `locate`.** In the *traits the program implements* table, the Copilot probe's `locate` is `ordin_setup::find_copilot_cli(PATH, home, LOCALAPPDATA, APPDATA)` over the program's own environment (SetupAssistant.md SET-CPX-2), then `<found> --version` (SET-CPX-3): `Some(HarnessLocation { path: <found>, version })`, or `Ok(None)` when nothing is found. `sign_in_status`, `start_sign_in` and `repair` are unchanged.
- **DSK-CPX-4 The Copilot adapter asks for the CLI.** The program implements the Copilot Local Adapter's `CopilotCliSource` (CopilotLocalAdapter.md CPA-CPX-2): `current()` runs the same `locate` as DSK-CPX-3 and answers `CopilotCli { path, version }` from its `HarnessLocation`, or `None`. Row 3.33 reads `CopilotLocalAdapter::new(CopilotAdapterConfig { tested_versions, app_data_root, profile: Desktop, stop_deadline: 10 s }, <CopilotPort>, <CopilotCliSource>, MCP Front, Hook Relay, Rules Engine, Identity, <SessionSetupSource>)`. `CopilotPort` (`SdkCopilotPort`) starts the server with the `cli_path` of the `CliServerSpec` the adapter passes, not with a path of `DesktopPaths`.
- **DSK-CPX-5 Setup Assistant row 3.43.** `SetupConfig.bundled_programs` is `[ordin-hook]` in every build (SetupAssistant.md SET-CPX-1). DSK-UP-5's Setup Assistant row reads: `copilot` off — no `register_probe`; `copilot` on — `register_probe(<Copilot HarnessProbe>)`. With `copilot` on and no Copilot CLI installed, start completes, nothing is logged for Copilot at start, and the Setup Assistant reports Copilot as not installed only when the organisation profile lists it; Settings › Agents and models marks it *Not found on this computer* (ScreensModule.md SM-AGM-1).

```gherkin
  Scenario: Edge case — Copilot on, no Copilot CLI installed (DSK-CPX-2, DSK-CPX-5)
    Given a build with copilot on and no copilot executable on PATH or in the person's own folders
    When the person opens ordin
    Then the window is shown
    And no DSK_SIDECAR_MISSING is logged
    And the Copilot probe's locate answers None

  Scenario: Happy path — the person's Copilot CLI reaches the adapter (DSK-CPX-4)
    Given a build with copilot on and a copilot executable at <home>/.local/bin/copilot printing "1.0.98"
    When a Copilot session is started
    Then CopilotPort::start_server receives cli_path <home>/.local/bin/copilot
```

### Gaps

None.

## Amendment 2026-02-04 — The first-start check's secret store (DSK-CHK-1 to DSK-CHK-3)

> **Status:** Approved by the user, 2026-02-04 ("apply recommendations"). Source: plan 08 Phase 11 — the first-start check runs in a Linux container with no keychain, so the Sandbox could not create its audit key (`SBX_AUDIT_KEY_UNAVAILABLE`) and the check could never start it. Every section above stays in force except where this amendment changes it.

- **DSK-CHK-1 The `check-secrets` switch.** The program has a build switch `check-secrets`, off by default. With it, row 3.3 builds the Identity service with `SecretBackendConfig::EncryptedFile { key_source: KeySource::KeyFile(<data_root>/check-secrets.key) }` in place of `SecretBackendConfig::OsKeychain`. If that file is absent, row 3.3 first writes 32 random bytes to it (mode `0600` on Unix). A failure to write it stops row 3.3 with `ModuleFailed` and the code `CRED_SECRET_KEY_INVALID`. Without the switch, row 3.3 is unchanged. Nothing else changes with the switch.
- **DSK-CHK-2 Never shipped.** No release or local build names `check-secrets`. `make first-start-check` is its only user.
- **DSK-CHK-3 The check starts the Sandbox.** `make first-start-check` builds with `check-secrets`, so the Sandbox's audit key is created in the encrypted file and the first start logs no `SBX_AUDIT_KEY_UNAVAILABLE`.

```gherkin
  Scenario: A check build keeps secrets in an encrypted file (DSK-CHK-1)
    Given a build with the check-secrets switch and an empty data root
    When the core starts
    Then <data_root>/check-secrets.key holds 32 bytes
    And the Sandbox's audit key is stored and the Sandbox is available

  Scenario: A normal build uses the keychain (DSK-CHK-1)
    Given a build without the check-secrets switch
    When the core starts
    Then row 3.3 uses the operating system's keychain
```

### Gaps

None.

## Amendment 2026-02-04 — The assistant chosen at first start, and a choice of Copilot honoured (DSK-CHO-1 to DSK-CHO-4)

> **Status:** Approved by the user, 2026-02-04 ("approve as written"). Source: HLD ADR-125; user decision 2026-02-04 ("go with your recommendations": ordin restarts itself once after the first-start answer). **derivedFromHld:** 0.75.0. Every section above stays in force except where this amendment changes it.

Evidence read: `apps/desktop/src/agent_settings.rs:139-151` (`in_this_version` accepts Claude Code only, so `harness_to_use` replaces a person's choice of Copilot with the organisation's harness); `apps/desktop/src/wiring.rs:788-798` (row 3.19 fixes the Session Supervisor's `local_adapter` at start), `:1463-1466` (Package Catalogue `ProfileHarness.chosen`), `:1494` (Ordin Host `harness`); `apps/desktop/src/core.rs:29-33`, `:400-404` (`QuitReason`, `AfterQuit::Restart`); `apps/desktop/Cargo.toml` (the `copilot` switch is on by default).

- **DSK-CHO-1 A choice of Copilot is honoured (amends DSK-AGM-1, *Harness*).** `in_this_version(harness)` is true for `claude_code` in every build, and true for `copilot` exactly when the program is built with the `copilot` switch. In DSK-AGM-1, *Harness*, the sentence "Only Claude Code runs in this version: the Copilot adapter is behind the `copilot` build switch and not wired, so a person's choice of Copilot is kept but the organisation's harness is used." is replaced by: "In a build with the `copilot` switch, a person's choice of Copilot is the harness in use: the Session Supervisor's `local_adapter` is `AdapterKind::CopilotLocal`. In a build without it, the choice is kept and the organisation's harness is used, and the harness's readiness is `not_in_this_version`."
- **DSK-CHO-2 `setup.choose_harness` and the one restart.** The program registers a `HarnessChoice` (SetupAssistant.md SET-CHO-1) with the Setup Assistant at the Setup Assistant's row:
  - `chosen()` reads `preferences.json` `agents.harness` and maps `claude_code` to `HarnessKind::ClaudeCode` and `copilot` to `HarnessKind::CopilotCli`. An absent entry, or an unreadable file, gives `None`.
  - `choose(h)` writes `agents.harness` exactly as `app.change_agent_setting` `harness { harness }` does (the same checks, `format` stays 1). If the write fails → `ChoiceError`.

  It answers `setup.choose_harness { harness }` (ServiceApi.md SVC-CHO-1) in these steps:
  1. Call `SetupAssistant::choose_harness`. An error is returned as it is.
  2. Work out `harness_to_use` again from the stored preferences. When it equals `harness_in_use` (the harness wired at row 3.19), answer `{ restarting: false }`; the guided run goes on in this process.
  3. Otherwise, answer `{ restarting: true }`. Then shut down through the ordinary quit path with the new `QuitReason::HarnessChosen` (log name `harness_chosen`), whose `AfterQuit` is `Restart`. No session has run, because first start is still drawn (ScreensModule.md SM-FS-4), so nothing is stopped or interrupted. On the next start, row 3.19 wires the chosen harness and the Setup Assistant runs the guided sequence again with that harness first and no question (SET-CHO-5).
- **DSK-CHO-3 Settings keeps its rule.** A harness changed in Settings › Agents and models still takes effect when the program next starts (DSK-AGM-1); only the first-start answer restarts the program.
- **DSK-CHO-4 The answer is sent before the restart.** The program sends the `setup.choose_harness` answer to the window before it starts shutting down, so the window can draw `setup-choose-restarting` (ScreensModule.md SM-FS-10).

| Condition | Result | Code | Retry |
|---|---|---|---|
| `setup.choose_harness` with no question waiting (DSK-CHO-2) | `Err(permanent)` | `SETUP_NO_CHOICE_PENDING` | no |
| `setup.choose_harness` with a harness not offered (DSK-CHO-2) | `Err(permanent)` | `SETUP_HARNESS_NOT_OFFERED` | no |
| `preferences.json` cannot be written for the choice (DSK-CHO-2) | `Err(retryable)` | `SETUP_CHOICE_NOT_SAVED` | yes; nothing restarts |

```gherkin
Feature: The assistant chosen at first start (DSK-CHO)

  Scenario: Happy path — choosing Copilot restarts ordin with the Copilot Local Adapter (DSK-CHO-1, DSK-CHO-2)
    Given a desktop build with copilot, no company profile, no agents.harness, and stub claude and copilot both signed in
    And the guided run is waiting at harness_choice
    When setup.choose_harness {"harness": "copilot_cli"} is called
    Then preferences.json holds agents.harness "copilot"
    And the answer is {"restarting": true}
    And the program shuts down with reason harness_chosen and restarts
    And after the restart the Session Supervisor's local_adapter is CopilotLocal and no harness_choice step is drawn

  Scenario: Happy path — choosing Claude Code goes on without a restart (DSK-CHO-2)
    Given the same start, waiting at harness_choice, with Claude Code in use
    When setup.choose_harness {"harness": "claude_code"} is called
    Then the answer is {"restarting": false} and the guided run goes on

  Scenario: Edge case — a stored choice of Copilot is used at start (DSK-CHO-1)
    Given a build with copilot and agents.harness "copilot"
    When the program starts
    Then harness_in_use is copilot and the local adapter is CopilotLocal

  Scenario: Edge case — a build without copilot keeps the choice and uses Claude Code (DSK-CHO-1)
    Given a build without copilot and agents.harness "copilot"
    When the program starts
    Then harness_in_use is claude_code and Copilot's readiness is not_in_this_version

  Scenario: Edge case — Settings still decides at the next start (DSK-CHO-3)
    Given agents.harness "copilot" chosen at first start
    When the person chooses Claude Code in Settings › Agents and models
    Then the program does not restart, and at the next start harness_in_use is claude_code

  Scenario: Error path — the choice cannot be saved (DSK-CHO-2)
    Given preferences.json cannot be written
    When setup.choose_harness {"harness": "copilot_cli"} is called
    Then the error is SETUP_CHOICE_NOT_SAVED and the program does not restart
```

### Gaps

None.


## Amendment 2026-02-05 — The agents follow the harness at start (DSK-HAR-1)

> **Status:** Approved by the user, 2026-02-05 ("approve as written"). Source: as PackageCatalogue.md PKG-HAR-1 to PKG-HAR-4. **derivedFromHld:** 0.75.0. Every section above stays in force except where this amendment changes it.

- **DSK-HAR-1 At start.** In the start task of DSK-REC-24 (`apps/desktop/src/core.rs`, the task that calls `add_bundled_agents` and then `add_starter_agents_everywhere`), after `add_bundled_agents` and before `add_starter_agents_everywhere`, the program calls `packages.follow_harness()` once. The harness it follows is `HarnessSettings::default_harness()` (DSK-AGM-1: the person's stored choice, else the profile's), the same one row 3.19 uses to pick the local adapter, so every installed agent is laid out for the adapter that will run it. It runs on every start; with nothing to follow it installs nothing. Its counts are logged as `agents.harness_followed` (`followed`, `failed`, `unchanged`). A first start that chose another harness reaches this through the restart of DSK-CHO-2; a change in Settings › Agents and models reaches it at the next start, as DSK-AGM-1 already says a changed harness does.

```gherkin
Feature: The agents follow the harness at start (DSK-HAR-1)

  Scenario: Happy path — choosing Copilot, then the agents run on Copilot
    Given an agent installed for Claude Code and agents.harness "copilot" stored
    When the desktop core starts
    Then the agent's install record is HarnessLaunch::Copilot
    And a Copilot session setup for it succeeds
```

### Gaps

None.


## Amendment February 2026 — Copilot sessions get the folder grants (DSK-REC-49)

> **Status:** Approved by the user, February 2026 ("yes to both, go ahead"). Source: the 0.2.1-test.9 smoke test, where a Document agent on Copilot could not read a PDF the person had attached and allowed. Every section above stays in force except where this amendment changes it.

- **DSK-REC-49 Folder grants for a Copilot session.** `DesktopCopilotSessionSetup::session_setup` applies the folder grants as the Claude setup does (DSK-REC-9): `apply_grants(run_id, agent_instance_id, workspace_id, crew_charter_id)` at session start, and `session_access(run_id)` first when the run resumes a persisted session (falling back to `apply_grants` when that read fails). The grants' paths become `CopilotSessionSetup.extra_dirs` (CopilotLocalAdapter.md CPA-DIR-1), and the attachment check of DSK-REC-26 step 4 accepts a file under one of them, as for `SessionSetup.extra_dirs`. A failed `apply_grants` fails the setup with that owner's code. This replaces, for Copilot, the "Claude Local Adapter only" and "a Copilot session gets none" wording of the `SessionSetupSource` row and of DSK-REC-26 step 4.

```gherkin
Feature: Copilot sessions get the folder grants (DSK-REC-49)

  Scenario: Happy path — an allowed place reaches the Copilot session
    Given a Copilot agent granted read access to the place "Docs"
    And a run of it with a PDF from "Docs" attached
    When its session is set up
    Then extra_dirs holds the folder of "Docs"
    And attachment_paths holds the PDF
```

### Gaps

None.


## Amendment February 2026 — The File Service's `documents` server is registered (DSK-REC-50)

> **Status:** Approved by the user, February 2026. Source: FileService.md SA-FS-DOC-1. Every section above stays in force.

- **DSK-REC-50** Right after row 3.22 (File Service), and so before `McpFront::start`, the program registers `document_tool_provider(&files)` (`apps/desktop/src/document_tools.rs`, over `files.document_reader()`; amended 2026-02-12, FileService.md SA-FS-DOC-1) with the MCP Front (`Provider::Files`, step 3 with DSK-UP-6). It is the first provider registered: the providers before the MCP Front starts are, in order, the File Service, the Sandbox (when built), the Memory Service, the Craft Toolkit, Projects, Knowledge and the Ordin Host.

```gherkin
Feature: The documents server is registered (DSK-REC-50)

  Scenario: Happy path
    When the desktop core starts
    Then the File Service's provider is registered with the MCP Front before any other
```

### Gaps

None.

## Amendment February 2026 — ordin moves itself to the person's Applications folder (DSK-MOVE-1 to DSK-MOVE-4) (amended February 2026, plan 08 Phase 19; user decision; Approved by the user February 2026)

Plan 08 Phase 19 (user decision 2026-02-11, "Ordin should move itself to ~/Applications on first start, with the DMG saying so instead of showing the /Applications link"): copying ordin into `/Applications` asks for an administrator's password on a Mac without administrator rights. macOS only; on Windows and Linux every command below answers as if ordin were installed and changes nothing.

- **DSK-MOVE-1 Is ordin installed?** `app.install_place {}` answers `{ installed: bool, target: String }`. `installed` is true when `install_folder(current_exe())` (`apps/desktop/src/install_kind.rs`), canonicalised, equals `/Applications` or `$HOME/Applications`, canonicalised; false otherwise (a mounted disk image, `~/Downloads`, a translocated copy under `/private/var/folders`, anywhere else). `target` is `$HOME/Applications/<product name>.app`. The rule is a pure function of the two paths and `$HOME`, unit-tested.
- **DSK-MOVE-2 Move.** `app.move_to_applications {}`, refused with `DSK_MOVE_NOT_NEEDED` when `installed` is true. Otherwise: create `$HOME/Applications` when missing; when `target` exists, move it to the Bin (renamed to `$HOME/.Trash/<product name> <yyyy-mm-dd hh.mm.ss>.app`); copy the running bundle to `target` with `/usr/bin/ditto <bundle> <target>`; remove `com.apple.quarantine` from the copy only (`/usr/bin/xattr -dr com.apple.quarantine <target>`); start a detached `/bin/sh` that waits until this program's process has ended (checking every 200 ms, for at most 60 seconds) and then runs `/usr/bin/open <target>`; answer `{ moved: true }`; then the program quits through the ordinary shutdown (DSK-REC) with the quit reason `moved_to_applications`, exiting rather than restarting. *Corrected 2026-02-12 while building it: the copy is started after this program has ended, because the single-instance plug-in (step 1) would otherwise hand the copy's start to the running ordin and the copy would close; and the older copy is moved to the Bin by renaming it into `~/.Trash`, which needs no new library.* No step runs with administrator rights. Each tool runs with a 120-second limit and its output is not logged; each step logs `move.<step>` with `outcome` and, on failure, the exit code.
- **DSK-MOVE-3 Errors.** A folder that cannot be made, an older copy that cannot be moved to the Bin, or `ditto` failing → `DSK_MOVE_COPY_FAILED` (retryable; any partial copy is removed); the detached shell that cannot be started → `DSK_MOVE_START_FAILED` (not retryable; the copy stays, the running ordin carries on). Nothing is changed outside `$HOME/Applications`. Both codes, and `DSK_MOVE_NOT_NEEDED`, go in the Error Table and ErrorHandling.md's registry, with en-GB sentences in `errors-desktop.ftl`.
- **DSK-MOVE-4 Updates.** Updates go on replacing the bundle where it lives (DSK-UP-15: `~/Applications` is a per-user install), with no administrator.

```gherkin
Feature: ordin moves itself to the person's Applications folder (DSK-MOVE-1 to DSK-MOVE-4)

  Scenario: Happy path — opened from the disk image
    Given ordin runs from /Volumes/ordin/ordin.app
    When the person presses Move ordin
    Then ~/Applications/ordin.app is a copy without the quarantine attribute
    And the copy is started and the running ordin quits

  Scenario: Edge case — already in the person's Applications folder
    Given ordin runs from ~/Applications/ordin.app
    Then app.install_place answers installed true and nothing is asked

  Scenario: Edge case — an older copy is there
    Given ~/Applications/ordin.app exists and ordin runs from ~/Downloads/ordin.app
    When the person presses Move ordin
    Then the older copy is in the Bin and the new one replaces it

  Scenario: Error path — the copy fails
    Given ditto exits with an error
    Then the answer is DSK_MOVE_COPY_FAILED and no partial copy is left
```

### Gaps

None.
