# SPEC: ordin desktop

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-103) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
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
| Log retention | 14 days of daily files | A-09, SA-OBS-01 |
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
pub enum RepairAction { TryAgain, SignInAgain, SendToIt }

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
| `CopilotPort` (Copilot Local Adapter) | implements its one method `start_server` over the bundled Copilot CLI at `DesktopPaths::copilot_cli` (`SdkCopilotPort`) | row 3.33 |
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
# every crate of 2B-3 whose Programs column contains D, plus ordin-types:
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
tauri = { version = "2", features = [] }
tauri-plugin-single-instance = "2"   # 05: Desktop shell plugins
tauri-plugin-notification = "2"      # 05: Desktop shell plugins
tauri-plugin-dialog = "2"            # 05: Desktop shell plugins
tauri-plugin-updater = "2"           # 05: "updated through Tauri's updater"
tauri-plugin-opener = "2"            # 1A SA-RT-01; HLD 05 *Desktop shell plugins* (0.49.1): BrowserOpener, UrlOpener
tauri-plugin-deep-link = "2"         # HLD 05 *Desktop shell plugins* (0.50.0, ADR-102): the `ordin` link scheme, current user only
tokio, tracing, tracing-subscriber, tracing-appender, serde, serde_json, thiserror, uuid # versions per 1A
```

Bundled programs (Tauri `bundle.externalBin`): `binaries/ordin-hook` (built from `apps/hook` before the desktop bundle; `make` copies it to `apps/desktop/binaries/ordin-hook-<target triple>`) and `binaries/copilot` (the pinned Copilot CLI server, 05). At run time each is found beside the executable: `std::env::current_exe()?.parent()?.join("ordin-hook")` (`ordin-hook.exe` on Windows), and the same for `copilot`.

### `tauri.conf.json` (fields this spec fixes)

```json
{
  "productName": "ordin",
  "identifier": "org.ordin.desktop",
  "build": { "frontendDist": "../ui/dist", "devUrl": "http://localhost:5173",
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
  "bundle": { "externalBin": ["binaries/ordin-hook", "binaries/copilot"] },
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
2. **Paths and logging.** In `setup`, resolve `<data root>` with `app.path().app_local_data_dir()`; create it and `logs/`, `run/` (mode `0700` on macOS). Write and delete a probe file `<data root>/.write-probe`; any failure → `StartError::DataRootUnwritable` (step 9). Install the `tracing` subscriber: JSON formatter, level `info`, writing through `tracing_appender::rolling::Builder` with daily rotation, prefix `ordin`, suffix `log`, `max_log_files(14)` (A-09). Install a panic hook that logs `panic` with the location and the first 200 characters of the message, then calls the default hook.
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
    | 3.41 | `ordin-host` (8) | `OrdinHost::new(OrdinHostConfig { profile: Desktop, … }, OrdinHostPolicy { coordinator_name: brand.coordinator_name(), … from the profile }, OrdinHostDeps { supervisor, orchestrator, usage, inspector, catalogue: Package Catalogue, memory, knowledge, push, projects, workspaces, journal, datastore: Datastore, blocks: the same StandardBlockCheck, files: File Service, refs: <RefResolver>, localisation: Localisation (amended January 2026, ADR-109) })` | `register(ordin_host.tool_provider())`; `journal.register_prune_participant(ordin_host.prune_participant())`; `set_policy` on each profile change and each brand change |
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
9. **Start failure.** On any `StartError`: log `start_failed` with the code and module; show a native message box (`tauri_plugin_dialog` `MessageDialogBuilder`, kind error, one button) with `desktop-start-failed` = "ordin couldn't start on this computer. Open it again; if this happens again, ask IT for help." and `desktop-start-failed-reference` = "Reference { $reference }", where the reference is the id of the `start_failed` log line; the text is built from the `en-GB` catalogue compiled into the program when Localisation is not yet constructed. On its button, exit with status 1.
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
| `<data root>` cannot be created or written | `Err(permanent)` — start stops, message box, exit 1 | `DSK_DATA_ROOT_UNWRITABLE` | message box: "ordin couldn't start on this computer. Open it again; if this happens again, ask IT for help." |
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
    Then a message box says "ordin couldn't start on this computer. Open it again; if this happens again, ask IT for help."
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
  - Never logged: notification text (question, agent and workspace names), folder paths chosen in the picker, preferences values, panic messages beyond 200 characters.
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
- **DSK-UP-3** **Bundled programs.** `bundle.externalBin` lists `binaries/copilot` only in builds with `copilot`: the Makefile's bundling target passes the Tauri configuration file that matches the build's switches to the bundling command of P7 *(chosen here)*. `binaries/ordin-hook` is in every build.
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
