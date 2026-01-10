# SPEC: Trigger Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-triggers` · **Build phase:** 10 · **Programs:** P
> **Dependencies:** Ordin Host, Channel Gateway, NATS Adapter, Slack Service, Organisation Profile, Journal

## Table of contents

1. [Purpose](#purpose)
2. [Approach](#approach)
3. [Shared Context](#shared-context)
4. [Public Interface](#public-interface)
5. [Internal Logic](#internal-logic)
6. [Data Model](#data-model)
7. [Error Table](#error-table)
8. [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
9. [Performance, Security, Observability](#performance-security-observability)
10. [Gaps](#gaps)

## Purpose

The Trigger Service starts saved workflows on the ordin server without the person typing a request (02:632–640, ADR-074). It keeps **triggers** — saved rules, proposed by Ordin as a card and turned on by the person's **Turn on** — and matches against them a file sent to the chat, a **/run** command, a **Run again** press, and each event the NATS Adapter delivers. For events it checks, in fixed code and in this order, that the payload is JSON within the size limit, that its identifier was not seen in the last 7 days, that the trigger's filter holds and that the limits allow a run; it records the outcome before it has the event acknowledged; and it starts the trigger's workflow at its pinned version with inputs filled from payload fields or fixed values. It keeps the limits — one run at a time per trigger, up to 10 waiting, at most 20 triggered runs a day per installation (the profile may set 1–200) — and tells the person in the chat what started, what was missed and when the daily limit stopped event triggers. Nothing is decided by a model turn and nothing is approved in advance: every external action of a triggered run still waits for the person's approval card.

On the personal server it also matches each message the Slack Service keeps from a Slack channel a workspace links against the turned-on triggers of the new kind *Slack message* (ADR-102; amends ADR-074's rejection of group-chat triggers for linked Slack channels only), under the same queue and daily limit as every other trigger. *Amended January 2026 (ADR-102).*

## Approach

One actor task per trigger receives that trigger's events and chat files over a bounded channel, so the per-trigger rules (dedupe, one run at a time, ordered waiting queue) need no lock; a single service task owns the daily count and the midnight reset. The service is a hub *provider*: it implements the Channel Gateway's `ChatTriggerHandler` and the NATS Adapter's `NatsEventSink`, and it starts and stops workflow runs through the Ordin Host, which owns workflows and their versions (2B-2 L7). Rejected: a model turn per event (spends the person's plan and lets event text steer the decision — ADR-074 *Alternatives*); acknowledging when the run finishes (runs wait hours for approvals, beyond the consumer's acknowledgement wait — ADR-074 *Alternatives*).

**Profiles.** The program constructs this service only in the **personal profile**. In the agent profile Ordin is absent (ADR-097 item 8), so no trigger can be proposed, and the agent profile reads no organisation profile (2D *Gaps* 1), so no NATS server is configured; intake routes there belong to the Caller API (02:793–795). Settled by decision D5.

**Reconciliation assumptions** (decision 80): **SA-RECON-01** — personal profile only (D5); the Session Supervisor edge is removed from 2B-3. **SA-RECON-02** — runs start through the Ordin Host's immediate-start option of `workflows.run` (`start_workflow_run`); a trigger the person turned on stands in for **Start** (R11, ADR-074). **SA-RECON-03** — Ordin's proposals arrive through the Ordin Host's tool `propose_trigger` and the program's `TriggerDesk` (row 435). **SA-RECON-04** — the program pauses this service by cancelling its shutdown token (row 434). **SA-CLOSE-01** (*chosen here*; closing decisions K19, K20) — a chat file reaches the run as `RunInputValue::Document { content: ContentRef::PlaceFile { .. } }`, the Workspace Service's saved Inbox file; the Ordin Host reads it through the File Service at start and hands the Crew Orchestrator an artefact (OrdinHost.md SA-CLOSE-02). `started_by: EventTrigger { trigger_id, .. }` and `ChatFileTrigger { trigger_id, .. }` make the Ordin Host start the crew with `started_by_person: false` and `CrewStart.trigger_id = Some(trigger_id)`, which the Crew Orchestrator accepts for a saved workflow; `Person { channel }` (a form the person completed) starts with `started_by_person: true` (OrdinHost.md SA-CLOSE-08).

**Slack messages.** The service subscribes once, at start, to the Slack Service's broadcast of kept messages (`subscribe_kept()`), and reads each matching message with `kept_message(id)`; it never calls the Slack Web API and never sees a message the Slack Service did not keep (only linked channels; never direct messages). A Slack message trigger behaves as a core-NATS event for the queue (no acknowledgement, no redelivery): the Slack Service's own catch-up at start redelivers what arrived while the server was stopped, and the 7-day identifier check of step D.5 removes repeats. Rejected: a second poller in this crate (two readers of one channel double the Slack rate, SA-RATE-02); a model turn per message to decide whether it matters (ADR-074 alternative, rejected for the same reason: it spends the person's plan and lets message text steer the decision). *Amended January 2026 (ADR-102).*

## Shared Context

### Shared types (Phase 2C, copied; not redefined)

```rust
// ordin-types — conventions
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, `Z`
uuid_id!(TriggerId, SlackLinkId, KeptSlackMessageId, TriggerEventId, WorkflowId, WorkspaceId, RunId, CrewRunId, ApprovalId,
         ChannelPairingId, ArtefactId, PlaceId); // UUIDv7 newtypes, #[serde(transparent)]

// ordin-types — Trigger (2C domain §Trigger)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Trigger {
    pub id: TriggerId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub listens_for: TriggerSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<NatsDelivery>,
    pub filter: Vec<FilterCondition>,
    pub input_mapping: Vec<InputMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier_source: Option<IdentifierSource>,
    pub workflow_id: WorkflowId,
    pub workflow_version: u32,
    pub limits: TriggerLimits,
    pub state: TriggerState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turned_on_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turned_on_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerSource {
    ChatFile { file_kind: String, name_pattern: String },
    NatsEvent { subject: String },
    /// ADR-102: a message kept from a Slack channel the trigger's workspace links.
    SlackMessage {
        slack_link_id: SlackLinkId,
        /// 1–200 characters after trimming; matched as a case-insensitive substring of the kept text (NFC, Unicode lower case).
        phrase: String,
        /// When set, only messages whose sender member id equals it (^[UW][A-Z0-9]{8,20}$).
        #[serde(skip_serializing_if = "Option::is_none")]
        from_member: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NatsDelivery { JetStream { consumer_name: String }, CoreNats }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FilterCondition {
    pub field: String,
    pub operator: FilterOperator,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator { Equals, IsOneOf, Exists, LessThan, LessOrEqual, GreaterThan, GreaterOrEqual }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InputMapping { pub input: String, pub from: MappingSource }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MappingSource { PayloadField { path: String }, Fixed { value: String } }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdentifierSource { NatsMsgId, CloudeventsId, Field { name: String } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerLimits { pub max_waiting: u32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerState { Proposed, On, Off, PausedByDailyLimit }

// ordin-types — TriggerEvent (2C domain §TriggerEvent)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TriggerEvent {
    pub id: TriggerEventId,
    pub trigger_id: TriggerId,
    pub identifier: String,
    pub received_at: Timestamp,
    pub outcome: TriggerOutcome,
    /// Set once the workflow run has started (2C C-8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crew_run_id: Option<CrewRunId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerOutcome { Started, SkippedByFilter, Duplicate, RefusedByLimit, Unreadable }

// ordin-types — WorkflowInput (2C domain §Workflow)
pub struct WorkflowInput { pub label: String, pub kind: WorkflowInputKind, pub required: bool }
pub enum WorkflowInputKind { Document, Text, List }   // 2C wire §8 (I-1); re-exported by the domain part

// ordin-types — CrewRunState (2C domain §CrewRun)
pub enum CrewRunState { Starting, Running, Paused, Finished, Escalated, StoppedAtLimit, Killed, Failed }

// ordin-service-api — events read from the Journal (2C wire §1)
pub struct EventEnvelope { pub api_version: ServiceApiVersion, pub backend: BackendId, pub seq: u64, pub at: Timestamp, #[serde(flatten)] pub event: ServiceEvent }
pub enum ServiceEvent { /* … */ CrewRoundChanged { crew_run_id: CrewRunId, round: u32, state: CrewRunState }, /* … */ }

// ordin-service-api — approval asker (2C wire §5)
pub enum ApprovalAsker { Run { run_id: RunId }, CrewRound { crew_run_id: CrewRunId, round: u32 }, Person }
pub enum ContentRef { Artefact { artefact_id: ArtefactId }, PlaceFile { place_id: PlaceId, relative_path: String } }
```

| 2C validation rule | Code |
|---|---|
| `name` 1–80 chars; unique per workspace | `TRIG_NAME_INVALID` |
| `delivery`, `identifier_source` present iff `listens_for.kind = nats_event` (absent for `slack_message`) | `TRIG_NATS_FIELDS_MISMATCH` |
| `SlackMessage.phrase` 1–200 chars after trimming | `TRIG_PHRASE_INVALID` |
| `SlackMessage.slack_link_id` names a link in state `linked` of the trigger's own workspace | `TRIG_SLACK_LINK_UNKNOWN` |
| `SlackMessage.from_member` matches `^[UW][A-Z0-9]{8,20}$` when present | `TRIG_MEMBER_INVALID` |
| subject within the profile's list | `TRIG_SUBJECT_NOT_ALLOWED` |
| `consumer_name` 1–255 `[A-Za-z0-9_-]` | `TRIG_CONSUMER_INVALID` |
| every required workflow input mapped; payload value as text ≤ 4,000 chars | `TRIG_INPUT_UNMAPPED`, `TRIG_PAYLOAD_TOO_LONG` |
| `filter` ≤ 20 conditions | `TRIG_FILTER_TOO_LONG` |
| `limits.max_waiting` 0–10 | `TRIG_WAITING_OUT_OF_RANGE` |
| one run at a time per trigger | `TRIG_RUN_IN_PROGRESS` |
| daily count ≤ limit | `TRIG_DAILY_LIMIT_REACHED` |
| `TriggerEvent.identifier` 1–256 chars | `TEV_IDENTIFIER_INVALID` |
| `crew_run_id` absent unless `outcome = started`; set once the run has started | `TEV_RUN_MISMATCH` |
| payload never stored | `TEV_PAYLOAD_STORED` |

State machine (2C): proposed → on (**Turn on**); on → off (**Turn off**, workflow removed, subject removed from the profile); on → paused_by_daily_limit (daily limit); paused_by_daily_limit → on (next day); off → on (**Turn on**); proposed, off → removed (row deleted).

### Configuration (Phase 2D)

| Source | Key | Default | Use |
|---|---|---|---|
| Organisation profile | `server.triggers.daily_runs` | 20, bounds 1–200 | triggered runs per installation per day |
| Organisation profile | `nats.subjects_allowed[]` | empty | subjects a NATS trigger may use |
| Organisation profile | `nats.servers[]` | empty → event triggers off | whether NATS triggers can be turned on |
| Environment | `TZ` | system zone; UTC in a container without it | midnight for the daily reset |
| 2D-4 | Trigger queue | 1 run at a time per trigger; up to 10 waiting | step E |
| 2D-4 | Daily triggered-run reset | midnight, server time zone | step H |
| Organisation profile | `slack.tools_enabled` | `true` | `false` → Slack message triggers cannot be turned on, and turned-on ones are turned off (step J.4) |
| Organisation profile | `server.external_actions_off[]` | empty | unchanged meaning: external actions of a triggered run stay switched off; a Slack trigger itself posts nothing |
| 2D-4 | Slack message too old to start a run | 24 hours before it is received *(chosen here)* | step M.3 |
| 2D-4 | Workflow input from a Slack message | text ≤ 4,000 characters (as an event, ADR-074 *Inputs*) | step M.5 |
| 2D-4 | Workflow input from an event | text ≤ 4,000 characters | step D.7 |
| 2D-4 | Event identifiers kept | 7 days | step H.3 |
| 2D-4 | Event payload size | the smaller of 1 MB and the server's maximum | enforced by the NATS Adapter |

### Types from the dependencies (as this spec requires them)

```rust
// ordin-nats (NATS Adapter spec)
pub struct FeedCapacity; impl FeedCapacity { pub fn new(free: u32) -> Arc<Self>; pub fn free(&self) -> u32; pub fn set_free(&self, free: u32); }
pub struct FeedSpec { pub trigger_id: TriggerId, pub subject: String, pub capacity: Arc<FeedCapacity>, pub known_delivery: Option<NatsDelivery> }
pub enum FeedKind { JetStream { stream: String, consumer_name: String }, CoreNats }
pub struct NatsEvent { pub trigger_id: TriggerId, pub subject: String, pub msg_id: Option<String>, pub headers: Vec<(String, String)>, pub payload: bytes::Bytes, pub payload_check: PayloadCheck, pub received_at: Timestamp, pub redelivered: bool, pub ack: AckHandle }
pub enum PayloadCheck { WithinLimit, TooLarge { bytes: u64, limit: u32 } }
pub enum ConnectionState { Off, NoCredentials, Connecting { since: Timestamp }, Connected { server: String, since: Timestamp, max_payload: u32 }, Reconnecting { since: Timestamp, attempts: u32 }, CredentialsRefused { since: Timestamp } }
#[async_trait] pub trait NatsEventSink: Send + Sync + 'static { async fn deliver(&self, event: NatsEvent); }
impl NatsAdapter {
    pub async fn open_feed(&self, spec: FeedSpec, sink: Arc<dyn NatsEventSink>) -> Result<FeedHandle, NatsError>;
    pub async fn delete_feed(&self, trigger_id: TriggerId, subject: &str) -> Result<(), NatsError>;
    pub fn subject_allowed(&self, subject: &str) -> bool;
    pub fn state(&self) -> ConnectionState;
    pub fn watch_state(&self) -> tokio::sync::watch::Receiver<ConnectionState>;
}

// ordin-channel-gateway (Channel Gateway spec) — the hub trait this service implements
pub struct ChatContext { pub channel: ChannelKind, pub pairing_id: ChannelPairingId, pub message_id: String, pub workspace: Option<WorkspaceRef> }
pub struct WorkspaceRef { pub id: WorkspaceId, pub name: String }
pub struct SavedChatFile { pub file_name: String, pub media_type: Option<String>, pub size_bytes: u64, pub content: ContentRef, pub display_path: String }
pub enum FileOutcome { Started, NotMatched }
pub struct PersonMessage { pub text: String, pub buttons: Vec<ActionButton>, pub attachment: Option<OutboundFile>, pub route: MessageRoute }
pub enum MessageRoute { LastChannel, AllPaired }
pub struct ActionButton { pub label: String, pub owner: ActionOwner, pub payload: serde_json::Value }
pub enum ActionOwner { Gateway, Triggers }
pub struct ChatForm { pub title: String, pub verb: String, pub fields: Vec<ChatFormField>, pub owner: ActionOwner, pub payload: serde_json::Value }
pub struct ChatFormField { pub name: String, pub label: String, pub required: bool, pub kind: ChatFieldKind, pub value: Option<ChatFieldValue>, pub note: Option<String> }
pub enum ChatFieldKind { Text { max_chars: u32 }, Choice { options: Vec<String> }, Document }
pub enum ChatFieldValue { Text(String), Document(ContentRef) }
pub struct HandlerError { pub code: &'static str, pub retryable: bool }
#[async_trait] pub trait ChatTriggerHandler: Send + Sync + 'static {
    async fn on_file(&self, ctx: &ChatContext, file: &SavedChatFile) -> Result<FileOutcome, HandlerError>;
    async fn on_run_command(&self, ctx: &ChatContext, rest: &str) -> Result<(), HandlerError>;
    async fn on_triggers_command(&self, ctx: &ChatContext) -> Result<(), HandlerError>;
    async fn on_action(&self, ctx: &ChatContext, payload: &serde_json::Value) -> Result<(), HandlerError>;
    async fn on_form_done(&self, ctx: &ChatContext, payload: &serde_json::Value, values: &[(String, ChatFieldValue)]) -> Result<(), HandlerError>;
    async fn on_run_again(&self, ctx: &ChatContext, crew_run_id: CrewRunId) -> Result<(), HandlerError>;
    async fn started_by_line(&self, asker: &ApprovalAsker) -> Option<String>;
}
impl ChannelGateway {
    pub fn register_trigger_handler(&self, handler: Arc<dyn ChatTriggerHandler>) -> Result<(), GatewayError>;
    pub async fn send_to_person(&self, msg: PersonMessage) -> Result<(), GatewayError>;
    pub async fn ask_form(&self, ctx: &ChatContext, form: ChatForm) -> Result<(), GatewayError>;
    pub async fn chat_label(&self, pairing_id: ChannelPairingId) -> Result<String, GatewayError>; // "Telegram", "Slack"
}
```

```rust
// ordin-slack-service (SlackService.md) — the calls this crate makes
pub struct KeptSlackNotice {
    pub workspace_id: WorkspaceId,
    pub slack_link_id: SlackLinkId,
    pub kept_id: KeptSlackMessageId,
    pub ts: String,              // ^[0-9]{10}\.[0-9]{6}$
    pub from_person: bool,
}
pub struct KeptSlackMessage { pub id: KeptSlackMessageId, pub slack_link_id: SlackLinkId, pub ts: String,
    pub thread_ts: Option<String>, pub sender_member_id: String, pub sender_name: String,
    pub sent_at: Timestamp, pub text: String, pub from_person: bool }
pub struct SlackLink { pub id: SlackLinkId, pub workspace_id: WorkspaceId, pub team_id: String,
    pub channel_id: String, pub name: String, pub private: bool, pub ordin_reads: bool,
    pub state: SlackLinkState, pub linked_at: Timestamp, pub read_up_to_ts: Option<String> }
pub enum SlackLinkState { Linked, NoAccess }
impl SlackService {
    /// Capacity 256 per receiver; a slow receiver gets `RecvError::Lagged(n)`.
    pub fn subscribe_kept(&self) -> tokio::sync::broadcast::Receiver<KeptSlackNotice>;
    pub async fn kept_message(&self, id: KeptSlackMessageId) -> Result<Option<KeptSlackMessage>, SlackError>;
    /// The link, or `None` when it does not exist (unlinked). (SlackService.md).
    pub async fn link_of(&self, id: SlackLinkId) -> Result<Option<SlackLink>, SlackError>;
    /// The permalink of a kept message, built without a call (SlackService.md, permalink).
    pub fn permalink(&self, link: &SlackLink, ts: &str) -> String;
}
```

### Calls on the Ordin Host, the Journal and the Organisation Profile (as required here)

| Callee | Signature required | Defined in |
|---|---|---|
| Ordin Host | `workflows(&self, workspace_id: WorkspaceId) -> Result<Vec<WorkflowInputsView>, OhError>`; `workflow_version(&self, workflow_id: WorkflowId, version: u32) -> Result<Option<WorkflowInputsView>, OhError>` with `WorkflowInputsView { workflow_id, name, state, version, latest_version, inputs: Vec<WorkflowInput> }` | OrdinHost.md (in-process interface, X-17) |
| Ordin Host | `start_workflow_run(&self, req: StartWorkflowRun) -> Result<CrewRunId, OhError>` with `StartWorkflowRun { workspace_id, workflow_id, version: u32, inputs: Vec<RunInput>, started_by: RunStartedBy }`, `RunInput { label, value: RunInputValue }`, `RunInputValue { Text { text }, Document { content: ContentRef }, List { items } }`, `RunStartedBy { Person { channel: String }, ChatFileTrigger { trigger_id, file_name }, EventTrigger { trigger_id, event_label } }` — the immediate-start option of `workflows.run` (R11) | OrdinHost.md |
| Ordin Host | `stop_workflow_run(&self, crew_run_id) -> Result<(), OhError>`; `crew_run_state(&self, crew_run_id) -> Result<Option<CrewRunState>, OhError>`; `run_again_inputs(&self, crew_run_id) -> Result<RunAgainInputs, OhError>` with `RunAgainInputs { workspace_id, workflow_id, version, newer_version: Option<u32>, inputs: Vec<WorkflowRunAgainInput { input: WorkflowInput, value: Option<RunInputValue>, newer_input_note: Option<String> }> }`; `OhError::code()` — `OH_WORKFLOW_UNKNOWN` and `OH_VERSION_UNKNOWN` mean the workflow is gone | OrdinHost.md |
| Journal | `Journal::subscribe(&self, sink: Arc<dyn EventSink>) -> SubscriptionId`; `EventSink { fn name(&self) -> &'static str; async fn deliver(&self, event: &JournalEvent); async fn lagged(&self, next_seq: u64); }`; `Journal::events_since(&self, after_seq: u64, limit: u32) -> Result<Vec<JournalEvent>, JournalError>`; `JournalEvent { backend, seq, at, kind: String, data: serde_json::Value }` | Journal and Artefact Store spec |
| Journal | `Journal::append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>` with `NewJournalEntry { kind, component, run_id, crew_run_id, workspace_id, summary, data }` | Journal and Artefact Store spec |
| Journal | `Datastore` (enum `Sqlite(SqlitePool)` / `Postgres(PgPool)`), `MigrationRegistry::register(&mut self, ModuleMigrations { module, sqlite, postgres })` | Journal and Artefact Store spec |
| Organisation Profile | `OrganisationProfileService::current() -> Arc<ProfileSnapshot>`, `subscribe() -> watch::Receiver<Arc<ProfileSnapshot>>`; reads `document.server.triggers.daily_runs` (`u32`, default 20, 1–200; `ServerSection`/`ServerTriggers`, sections not `Option`) and `document.nats` (as the NATS Adapter spec) | OrganisationProfile.md |

## Public Interface

Error prefixes: **`TRIG_`** and **`TEV_`** (2C).

```rust
// crate ordin-triggers

pub struct TriggerDeps {
    pub host: Arc<ordin_host::OrdinHost>,
    pub gateway: Arc<ordin_channel_gateway::ChannelGateway>,
    pub nats: ordin_nats::NatsAdapter,
    pub journal: ordin_journal::Journal,
    pub datastore: ordin_journal::Datastore,
    pub profile: Arc<ordin_organisation_profile::OrganisationProfileService>,
    /// ADR-102: kept messages of linked Slack channels.
    pub slack: Arc<ordin_slack_service::SlackService>,
}

/// What Ordin proposes through the tool `propose_trigger` of MCP server `ordin`, served by the Ordin Host and
/// passed here by the program's `ordin_host::TriggerDesk` (OrdinHost.md SA-RECON-03). Validated by `propose`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TriggerProposal {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub listens_for: TriggerSource,
    pub filter: Vec<FilterCondition>,
    pub input_mapping: Vec<InputMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier_source: Option<IdentifierSource>,
    pub workflow_id: WorkflowId,
    /// Pinned version; must exist.
    pub workflow_version: u32,
    /// 0–10; defaults to 10 when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_waiting: Option<u32>,
}

/// One line of **/triggers**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerLine {
    pub trigger_id: TriggerId,
    pub name: String,
    pub state: TriggerState,
    pub last_event_at: Option<Timestamp>,
    pub today_count: u32,
    /// NATS triggers only: the NATS Adapter is not `Connected`.
    pub not_connected: bool,
}

pub struct TriggerService { /* private: Arc<Inner> */ }

impl TriggerService {
    pub const MODULE: &'static str = "triggers";
    /// `ModuleMigrations { module: "triggers", sqlite: &[(1, "triggers", <Data Model DDL>)], postgres: &[] }`.
    pub fn migrations() -> ordin_journal::ModuleMigrations;

    /// Loads triggers, rebuilds running slots (step I), registers with the Channel Gateway as its
    /// `ChatTriggerHandler`, opens a NATS feed for every `on` NATS trigger, subscribes to the Slack Service's kept messages, and starts the
    /// midnight task and the Journal subscription. Personal profile only (D5).
    /// Cancelling `shutdown` pauses the service for an orderly stop: no new run starts, no event is
    /// acknowledged after that moment, the actors end and the durable consumers are kept (step A.6). There is no
    /// separate `pause` call (row 434).
    pub async fn start(deps: TriggerDeps, shutdown: CancellationToken) -> Result<Arc<Self>, TriggerError>;

    /// JSON Schema of `TriggerProposal` without `workspace_id`, generated with `schemars`; the program's
    /// `TriggerDesk::proposal_schema` returns it as the tool's `input_schema`.
    pub fn proposal_schema() -> serde_json::Value;

    /// Validates and stores a proposal in state `proposed`, then sends the trigger card.
    pub async fn propose(&self, proposal: TriggerProposal) -> Result<Trigger, TriggerError>;

    /// Person's **Turn on** (also reachable from **/triggers**). `by` is the chat label, e.g. "you, in Telegram".
    pub async fn turn_on(&self, id: TriggerId, by: &str) -> Result<Trigger, TriggerError>;
    /// Person's **Turn off**.
    pub async fn turn_off(&self, id: TriggerId) -> Result<Trigger, TriggerError>;
    /// Removes a `proposed` or `off` trigger (row deleted; NATS consumer deleted).
    pub async fn remove(&self, id: TriggerId) -> Result<(), TriggerError>;

    pub async fn list(&self) -> Result<Vec<TriggerLine>, TriggerError>;
}

#[async_trait::async_trait]
impl ordin_channel_gateway::ChatTriggerHandler for TriggerService { /* steps F, G */ }

#[async_trait::async_trait]
impl ordin_nats::NatsEventSink for TriggerService { /* step D */ }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct TriggerError { pub code: &'static str, pub retryable: bool }
```

**Called by.**

| Item | Caller |
|---|---|
| `migrations`, `start` | Ordin Server program, personal profile only |
| `proposal_schema`, `propose` | the Ordin Server program's `TriggerDesk` implementation, when Ordin calls `propose_trigger` (OrdinHost.md X-21) |
| `turn_on`, `turn_off`, `remove`, `list` | this crate's chat steps C and G; the program has no other caller |
| `ChatTriggerHandler` methods | Channel Gateway |
| `NatsEventSink::deliver` | NATS Adapter |

Codes this crate raises beyond the 2C rules (*chosen here*): `TRIG_NOT_FOUND`, `TRIG_STATE_INVALID`, `TRIG_WORKFLOW_NOT_FOUND`, `TRIG_EVENTS_NOT_CONFIGURED`, `TRIG_FILE_KIND_INVALID`, `TRIG_PATTERN_INVALID`, `TRIG_PATH_INVALID`, `TRIG_FILTER_VALUE_INVALID`, `TRIG_START_FAILED`, `TRIG_NATS_FAILED`, `TRIG_DATASTORE_FAILED`, `TRIG_NO_WORKSPACE`, `TRIG_SLACK_OFF`.

### Example

Proposal (Ordin, from "when an invoice-created event arrives, run Invoice check with its PDF"):

```json
{
  "workspace_id": "0192a0f1-1111-7222-8333-444455556666",
  "name": "Invoice check on invoice created",
  "listens_for": {"kind": "nats_event", "subject": "finance.invoice.created"},
  "filter": [{"field": "data.customer", "operator": "equals", "value": "Northwind Group"}],
  "input_mapping": [{"input": "Invoice", "from": {"kind": "payload_field", "path": "data.pdf_url"}},
                    {"input": "Customer", "from": {"kind": "payload_field", "path": "data.customer"}}],
  "identifier_source": {"kind": "cloudevents_id"},
  "workflow_id": "0192a0f3-3333-7444-8555-666677778888",
  "workflow_version": 4
}
```

Trigger card sent (Telegram text form; buttons carry gateway action references):

```text
Run Invoice check when an invoice created event arrives for Northwind Group, with the event's data.pdf_url as Invoice. One at a time, up to 20 a day.
[Turn on] [Not now] [Show details]
```

An event that starts a run → message to the person:

```text
Started Invoice check · invoice created, INV-2291
[Stop]
```

Journal entry written (`NewJournalEntry`, no payload):

```json
{"kind":"trigger_event","component":"triggers","crew_run_id":"0192a1d5-…","workspace_id":"0192a0f1-1111-7222-8333-444455556666",
 "summary":"An invoice created event started Invoice check.",
 "data":{"trigger_id":"0192a1d0-1234-7abc-8def-0123456789ab","trigger_event_id":"0192a1d4-…","identifier_digest":"3f9a0c51d2e7","outcome":"started"}}
```

`TriggerProposal` is unchanged in shape: `listens_for` takes the new variant, so the JSON Schema that `proposal_schema()` returns (and the program's `TriggerDesk::proposal_schema` passes to Ordin's `propose_trigger`) gains the `slack_message` branch with `slack_link_id`, `phrase`, `from_member`. The `propose_trigger` tool row of OrdinHost.md has the `description` "Propose a trigger that runs a saved workflow when a chat file, an event or a message in a linked Slack channel arrives." (OrdinHost.md).

Example of a Slack message trigger proposal:

```json
{
  "workspace_id": "0192a0f1-1111-7222-8333-444455556666",
  "name": "Supplier query on #supplier-desk",
  "listens_for": {"kind": "slack_message", "slack_link_id": "0192a2e0-1111-7222-8333-444455550001", "phrase": "quote request"},
  "filter": [],
  "input_mapping": [{"input": "Request", "from": {"kind": "payload_field", "path": "message"}},
                    {"input": "Asked by", "from": {"kind": "payload_field", "path": "sender"}}],
  "workflow_id": "0192a0f3-3333-7444-8555-666677778888",
  "workflow_version": 2
}
```

Trigger card:

```text
When a message in #supplier-desk says "quote request", run **Supplier query**, with the message as Request and who wrote it as Asked by. One at a time, up to 20 a day.
[Turn on] [Not now] [Show details]
```

## Internal Logic

**A. Start (`TriggerService::start`)**

1. Read all rows of `triggers`; build one actor per trigger in state `on` or `paused_by_daily_limit` (step E). Read today's row of `trigger_daily_counts` (day = server-local date, step H.1).
2. Rebuild running slots (step I).
3. `gateway.register_trigger_handler(self)`. An error (`CHAN_HANDLER_ALREADY_SET`) is a programming error: log `triggers.start.failed` and return `TRIG_STATE_INVALID`.
4. For each `on` NATS trigger: `subject_allowed` false → turn it off (step J.2). Otherwise `nats.open_feed(FeedSpec { trigger_id, subject, capacity: actor.capacity, known_delivery: trigger.delivery }, self)`. The returned `FeedHandle`'s `kind` (`FeedKind`) is written to `triggers.delivery` when it differs. A failure is logged `triggers.feed.failed {trigger_id, code}`; the NATS Adapter retries on connect. For a `slack_message` trigger in `on`: `slack.link_of(slack_link_id)` returning `None`, a link of another workspace, or `state = no_access` → step J.5; `slack.tools_enabled = false` → step J.4.
5. Spawn: the Journal subscription (step I.3), the midnight task (step H), the NATS state watcher (step K), the profile watcher (step J), the Slack task (step M.1).
6. **Stop.** When `shutdown` is cancelled: refuse new starts (`TRIG_STOPPING`, retryable); every actor stops taking items, leaving waiting events unacknowledged so JetStream redelivers them after the next start; each `FeedHandle` is closed with `close` and `nats.delete_feed` is not called, so each durable consumer is kept; the tasks of step 5 end. A run already started continues under the Crew Orchestrator.

**B. Proposal (`propose`)**

1. Validate every 2C rule in the table above, plus:
   - `workflow_version(workflow_id, workflow_version)` returns `Some`; else `TRIG_WORKFLOW_NOT_FOUND`.
   - every input label in `input_mapping` names an input of that version; every `required` input is mapped (`TRIG_INPUT_UNMAPPED`); a `Fixed` value ≤ 4,000 chars (`TRIG_PAYLOAD_TOO_LONG`).
   - `ChatFile`: `file_kind` is `any`, `image`, or 1–10 chars `[a-z0-9]` (a file extension without the dot) → else `TRIG_FILE_KIND_INVALID`; `name_pattern` ≤ 120 chars of any text where `*` and `?` are wildcards (empty = any name) → else `TRIG_PATTERN_INVALID`. A `ChatFile` trigger maps exactly one input from the file: the `input_mapping` row whose `from` is `PayloadField { path: "file" }` (*chosen here*: the file is the input the trigger names, ADR-074); other rows must be `Fixed`.
   - `NatsEvent`: `nats.subject_allowed(subject)` (`TRIG_SUBJECT_NOT_ALLOWED`); `identifier_source` present (`TRIG_NATS_FIELDS_MISMATCH`); every path in `filter`, `input_mapping` and `IdentifierSource::Field` matches the path grammar of step D.3 (`TRIG_PATH_INVALID`); `IsOneOf` has an array `value` of 1–50 items; numeric operators have a numeric `value`; `Equals` has a scalar `value`; `Exists` has no `value` (`TRIG_FILTER_VALUE_INVALID`).
   - `SlackMessage`: `phrase` 1–200 chars after trimming (`TRIG_PHRASE_INVALID`); `slack.link_of(slack_link_id)` is `Some`, `state = linked` and `workspace_id = proposal.workspace_id` (`TRIG_SLACK_LINK_UNKNOWN`; a `SlackError` → `TRIG_SLACK_LINK_UNKNOWN` with `retryable` copied); `from_member` matches `^[UW][A-Z0-9]{8,20}$` when present (`TRIG_MEMBER_INVALID`); `slack.tools_enabled = true` (`TRIG_SLACK_OFF`); `delivery` and `identifier_source` absent (`TRIG_NATS_FIELDS_MISMATCH`); `filter` empty (a filter on a Slack message is not offered, *chosen here*: the phrase and the sender are the filter) → else `TRIG_FILTER_VALUE_INVALID`; every `PayloadField` path in `input_mapping` is one of `message`, `sender`, `link` (`TRIG_PATH_INVALID`).
2. Insert the row with `state = proposed`, `delivery = None`, `limits.max_waiting = proposal.max_waiting.unwrap_or(10)`. Name collision in the workspace → `TRIG_NAME_INVALID`.
3. Send the **trigger card** (step L.1) with buttons **Turn on** (`{"t":"on","id":…}`), **Not now** (`{"t":"not_now","id":…}`), **Show details** (`{"t":"details","id":…}`) through `gateway.send_to_person(route: LastChannel)`.
4. Journal entry `trigger_proposed`. Return the trigger.

**C. Turning on, off, removing**

1. `turn_on(id, by)`: state must be `proposed` or `off` (else `TRIG_STATE_INVALID`). Re-check the workflow version exists (`TRIG_WORKFLOW_NOT_FOUND`) and, for NATS, that the subject is allowed and `nats.servers` is non-empty (`TRIG_EVENTS_NOT_CONFIGURED`). Set `state = on`, `turned_on_by = by`, `turned_on_at = now`. When today's daily count has reached the limit and the trigger is a NATS trigger, set `paused_by_daily_limit` instead. Create the actor; for NATS open the feed (step A.4) and store `delivery`. Edit nothing on the card: the gateway closes the card's buttons when an action is used (Channel Gateway step F). Send "Turned on: {name}." to the last channel. For a Slack message trigger, re-check B.1's Slack bullet; a failure returns its code and leaves the trigger `proposed` or `off`.
2. `turn_off(id)`: state `on` or `paused_by_daily_limit` (else `TRIG_STATE_INVALID`). Set `off`; drop the actor (waiting events are not acknowledged, so JetStream redelivers them only if the trigger is turned on again — the durable consumer is kept); close the feed. A run already going continues.
3. `remove(id)`: state `proposed` or `off` (else `TRIG_STATE_INVALID`); for NATS `nats.delete_feed(id, subject)` (`NATS_NOT_CONNECTED` → keep a row in `trigger_pending_deletes` and retry at each start and each `Connected`); delete the row and its `trigger_events` rows.

**D. A NATS event (`NatsEventSink::deliver`)** — the sink forwards the event to the trigger's actor over a bounded channel (capacity 32); the actor runs D.1–D.10 in order for one event at a time.

1. **Trigger state.** No actor (trigger off or removed) → ack the event and drop it (log `triggers.event.dropped {trigger_id}`; nothing is recorded because the trigger no longer exists or listens).
2. **Payload.** `PayloadCheck::TooLarge` → outcome `unreadable`. Otherwise `serde_json::from_slice::<serde_json::Value>`; failure → `unreadable`.
3. **Path grammar.** A field path is 1–200 chars, segments separated by `.`; a segment is 1–64 chars of `[A-Za-z0-9_-]`, or a decimal array index 0–9999. Resolution walks objects by key and arrays by index from the payload root (for CloudEvents the envelope is the root, so `data.number` reaches the event's data). A missing segment means *absent*; JSON `null` is *absent*.
4. **Identifier.** `NatsMsgId` → `event.msg_id`; `CloudeventsId` → the root object's `id` when the root has string fields `specversion`, `id`, `type` and `source`; `Field { name }` → the value at path `name`, strings as they are, numbers as their JSON text. Absent, empty or longer than 256 chars → `unreadable`. For an `unreadable` event with no identifier, the stored identifier is `"unreadable:" + first 16 hex of SHA-256(payload bytes)` (*chosen here*, so the row satisfies `TEV_IDENTIFIER_INVALID`).
5. **Duplicate.** A row of `trigger_events` with the same `(trigger_id, identifier)`, `received_at` within 7 days and `outcome <> 'duplicate'` → outcome `duplicate`.
6. **Filter** (fixed code, all conditions must hold):
   - `Exists`: value present.
   - `Equals`: both strings → byte equality after NFC; both numbers → numeric equality (integers compared as `i128`, otherwise `f64`); both booleans → equality; any other pairing → false.
   - `IsOneOf`: `Equals` against any element of `value`.
   - `LessThan`, `LessOrEqual`, `GreaterThan`, `GreaterOrEqual`: the field must be a JSON number (a numeric string is not converted); compared as in `Equals`; a non-number → false.
   - A field absent for any operator other than `Exists` → outcome `unreadable` (ADR-074: "JSON that lacks a field the trigger uses, starts nothing; it is counted").
   - Any condition false → outcome `skipped_by_filter`.
7. **Inputs.** For each mapping: `Fixed` → the value; `PayloadField` → strings as they are, numbers and booleans as JSON text, objects and arrays as compact JSON text. Absent → `unreadable`; longer than 4,000 chars → `unreadable` (code `TRIG_PAYLOAD_TOO_LONG` in the log). Values are passed as `RunInputValue::Text { text }`, except for an input of kind `List` (*chosen here*): a `PayloadField` holding a JSON array of strings, or a `Fixed` value split into its non-empty lines, becomes `RunInputValue::List { items }` with 1–100 items of 1–500 characters each; any other value → `unreadable`. Event content reaches agents only as these named inputs (01 trust boundary 4).
8. **Limits.** In order:
   - daily count ≥ limit → outcome `refused_by_limit`; when the count has just reached the limit by another start, step H.2 has already paused event triggers.
   - the trigger's run slot is free → **start** (D.9).
   - waiting queue length < `limits.max_waiting` → push `(event, inputs, identifier, label)` to the queue; start a 20-second ticker that calls `event.ack.in_progress()` for each queued JetStream event; nothing is recorded yet. Set `capacity.set_free(1 + max_waiting − running − queued)`.
   - otherwise → outcome `refused_by_limit` (only core NATS reaches this: JetStream pulls no more than the free places).
9. **Start.**
   1. In one transaction: insert `trigger_events (outcome = 'started', crew_run_id = NULL)`, increment `trigger_daily_counts.started` for today.
   2. `event.ack.ack()`; on `NATS_ACK_FAILED` log and continue (a redelivery is recorded as `duplicate`).
   3. `host.start_workflow_run(StartWorkflowRun { workspace_id, workflow_id, version: trigger.workflow_version, inputs, started_by: EventTrigger { trigger_id, event_label } })` where `event_label` = step L.3.
   4. `Ok(crew_run_id)` → update the row's `crew_run_id`, occupy the slot, journal entry `trigger_event` (outcome `started`), send "Started {workflow} · {event_label}" with **Stop** (`{"t":"stop","run":crew_run_id}`), route `LastChannel`.
   5. `Err` → log `triggers.start.failed {trigger_id, code}`; the row stays `started` with no run; send "Couldn't start {workflow} for {event_label}." (*chosen here*) to the last channel; the slot stays free and the next queued event is tried (D.10). An `OhError` with code `OH_WORKFLOW_UNKNOWN` or `OH_VERSION_UNKNOWN` → also turn the trigger off (step J.3).
   6. If the daily count now equals the limit → step H.2.
10. **Other outcomes.** For `skipped_by_filter`, `duplicate`, `refused_by_limit`, `unreadable`: insert the row (crew_run_id NULL), journal entry `trigger_event` with the outcome, then `event.ack.ack()`. `unreadable` and `refused_by_limit` increment `trigger_notices` for (trigger, today, kind); the first increment of the day schedules the notice of step L.4 one hour later. Nothing is sent for skipped or duplicate events (11 §5.24).

**E. Per-trigger actor and the run slot**

1. Holds: the trigger, `capacity: Arc<FeedCapacity>` (initial `1 + max_waiting`), `running: Option<CrewRunId>`, `queue: VecDeque<Queued>` (≤ `max_waiting`).
2. **Run ended** (step I.3 delivers `CrewRoundChanged` with a terminal state for `running`): clear the slot; pop the oldest queued item and run D.9 for it (its daily-limit check is repeated first: over the limit → outcome `refused_by_limit`, ack); update `capacity`.
3. Terminal crew run states: `finished`, `stopped_at_limit`, `killed`, `failed`. `escalated` and `paused` keep the slot (the run is waiting for the person) (*assumed here*: an escalated crew run is waiting for Ordin or the person, not ended).

**F. Chat items (`ChatTriggerHandler`)**

1. `on_file(ctx, file)`: `ctx.workspace` absent → `NotMatched`. Find the `on` `ChatFile` triggers of that workspace, oldest `turned_on_at` first, whose kind matches (`any`; `image` → `media_type` starts with `image/`; otherwise the file name's extension, lower-cased, equals `file_kind`) and whose `name_pattern` matches the whole file name case-insensitively (`*` = any run of characters, `?` = one character; empty = any). None → `NotMatched` (the gateway gives the file to Ordin).
   For the first match: identifier = `"chat:" + ctx.channel + ":" + ctx.message_id`; then steps D.5 (duplicate), D.8 (limits; a full queue gives `refused_by_limit` and the reply "Invoice check is busy, so I didn't start it for {file}." *chosen here*), D.9 with `inputs` = the mapped file input as `RunInputValue::Document { content: file.content }` plus `Fixed` values, and `started_by: ChatFileTrigger { trigger_id, file_name }`. The started message reads "Started {workflow} with {file name} · **Stop**". Return `Started` (also for `refused_by_limit` and `duplicate`, so the file does not also start an Ordin turn).
2. `on_run_command(ctx, rest)`: `ctx.workspace` absent → send "Open a workspace with Ordin first, then use /run." (*chosen here*) and return. `rest` empty → `host.workflows(ws)`; none → "There are no saved workflows in {workspace}." (*chosen here*); otherwise one button per workflow, alphabetical, at most 20 (`{"t":"run","wf":id}`), and, beyond 20, a last line "and {n} more — type /run and the name". `rest` non-empty → match a workflow name case-insensitively after trimming and collapsing spaces; none → "There's no workflow called “{rest}” in {workspace}." (*chosen here*); found → step F.4.
3. `on_triggers_command(ctx)`: `list()`; no trigger → "You have no triggers yet. Ask me, for example: “when an invoice-created event arrives, run Invoice check”." (*chosen here*). Otherwise one message per trigger with the line of step L.5 and one button: **Turn off** (`{"t":"off","id"}`) for `on`/`paused_by_daily_limit`, **Turn on** (`{"t":"on","id"}`) for `off`.
4. **Starting a workflow from the chat.** `host.workflow_version(id, view.latest_version)` (from `host.workflows`); build `ChatForm { title: name, verb: "Run " + name (cut to 32 chars), fields: one per input in order — Text → `Text { max_chars: 4000 }`, Document → `Document`, List → `Text { max_chars: 4000 }` with note "One item per line" (*chosen here*) —, owner: Triggers, payload: {"t":"form_run","wf":id,"v":version,"ws":workspace} }` and `gateway.ask_form(ctx, form)`. A workflow with no inputs skips the form and starts at once (F.5).
5. `on_form_done(ctx, payload, values)` with `t = form_run` or `form_again`: `host.start_workflow_run(… started_by: Person { channel: <ctx.channel as text> } …)`, each value mapped by its input's kind (Text → `RunInputValue::Text`, Document → `RunInputValue::Document`, List → `RunInputValue::List` with one item per non-empty line; a List outside 1–100 items of 1–500 characters → reply "Give {label} as 1 to 100 lines." (*chosen here*) and no start); send "Started {workflow} · **Stop**". Not counted in the daily limit and not limited to one at a time (*assumed here*: the person asked for this run in the chat; the limits of ADR-074 bound runs that start without the person). Error → "Couldn't start {workflow}." (*chosen here*).
6. `on_run_again(ctx, crew_run_id)`: `host.run_again_inputs(crew_run_id)`; form as F.4 with each field's `value` pre-filled from `WorkflowRunAgainInput.value` and its `note` set to `newer_input_note` (and a first line naming `newer_version` when set) (ADR-060's form: "with newer inputs named"); payload `{"t":"form_again",…}`.
7. `on_action(ctx, payload)` by `payload.t`: `on` → `turn_on(id, "you, in " + chat_label)`; `off` → `turn_off`; `not_now` → `remove` when `proposed`, otherwise nothing; `details` → send step L.2; `run` → F.4; `stop` → `host.stop_workflow_run(run)` then "Stopping {workflow}." (*chosen here*); `turn_off_busiest` → `turn_off(id)`. An unknown `t` → `HandlerError { code: "TRIG_STATE_INVALID", retryable: false }`.
8. `started_by_line(asker)`: for `CrewRound { crew_run_id, .. }` whose `crew_run_id` this service started from an event in the last 7 days → `Some("Started by an event: " + event_label)`; from a chat-file trigger → `Some("Started by a file you sent: " + file name)` (*chosen here*); otherwise `None`.

**G. Words.** Every sentence this service sends is in the *Words* table of step L; texts marked *chosen here* are additions where 11 §5.24 gives none.

**H. Daily count and midnight**

1. "Today" is the server-local date from `chrono::Local` (which follows `TZ`). The midnight task sleeps until the next local midnight (`Local.from_local_datetime(date+1 00:00).earliest()`, so a DST gap still fires), then runs H.3 and H.4.
2. **Limit reached.** When `started` for today reaches the limit: every NATS trigger and every Slack message trigger in `on` becomes `paused_by_daily_limit` (capacity 0; core-NATS events that still arrive are recorded `refused_by_limit`); send once per day "{limit} runs started from events today, the most allowed. Events will start runs again after midnight." with **Turn off this trigger** (`{"t":"turn_off_busiest","id":<trigger with most starts today>}`). Chat-file triggers are not paused by the state change but refuse starts while the count is at the limit (step D.8).
3. **Reset.** Every `paused_by_daily_limit` trigger returns to `on` (capacity restored); insert the new day's count row; delete `trigger_events` rows with `received_at` older than 7 days; delete `trigger_daily_counts` and `trigger_notices` rows older than 7 days. Paused Slack message triggers return to `on` the same way.
4. The limit is read from the profile at each check (`server.triggers.daily_runs`, clamped to 1–200).

**I. Recovery and run ends**

1. On start, for each trigger, take the newest `started` row with a run in the last 7 days; `host.crew_run_state(run)` returning a non-terminal state → the slot is occupied by it.
2. Waiting queues are empty after a restart: JetStream redelivers every unacknowledged event, and core-NATS events received before the restart are lost (ADR-074 *Delivery*, the trigger's "Events sent while the server is off are missed").
3. The service registers an `EventSink` named `triggers` with `journal.subscribe`. For each `JournalEvent` with `kind = "crew_round_changed"`, it reads `data.crew_run_id` and `data.state`; a terminal state for a run held in a slot runs E.2 for that actor. On `lagged(next_seq)` it re-reads with `events_since(next_seq − 1, 500)` until caught up, then re-runs I.1 for every actor, so no end is missed.

**J. Profile changes**

1. On each new profile view: for each NATS trigger in `on` or `paused_by_daily_limit` whose subject is no longer allowed → step J.2.
2. Turn the trigger `off`, close its feed, send "Your company no longer allows ordin to listen for these events, so I turned off {name}." (*chosen here*), journal entry `trigger_turned_off` (`reason: subject_removed`).
3. A workflow that is gone (an `OhError` with code `OH_WORKFLOW_UNKNOWN` or `OH_VERSION_UNKNOWN` from any call) → `off`, "I turned off {name} because its workflow was removed." (*chosen here*), journal entry `trigger_turned_off` (`reason: workflow_removed`).
4. `slack.tools_enabled` becomes `false`: every Slack message trigger in `on` or `paused_by_daily_limit` turns `off`; send once "Your company has turned Slack off for ordin, so I turned off {name}." (*chosen here*); journal entry `trigger_turned_off` (`reason: slack_off`).
5. The link is gone or lost (`link_of` is `None`, or its state is `no_access`, seen at A.4 or at M.2): turn the trigger `off`; send "I turned off {name} because #{channel} is no longer linked to {workspace}." (*chosen here*); journal entry `trigger_turned_off` (`reason: slack_link_removed`).

**K. NATS connection notices**

1. Watch `nats.watch_state()`. When the state is `Reconnecting` or `Connecting` continuously for 10 minutes, or becomes `CredentialsRefused`, and at least one NATS trigger is `on`: send once "I can't reach your company's event system, so events aren't starting anything. Whoever runs your ordin server can check the connection." (*chosen here*; 03:1004, ADR-076 *Before something runs out*). Send again only after a `Connected` state has been seen.
2. **/triggers** shows "Not connected" for NATS triggers whenever the state is not `Connected` (11 §5.24).

**L. Words** (sent through the Channel Gateway, which turns `**…**` into the channel's bold form)

1. *Trigger card*: "Run **{workflow}** when {listens} {filter}, with {inputs}. One at a time, up to {limit} a day." where `listens` = "an *{event plain name}* event arrives" or "you send {a/an} {file kind} file" + (", whose name matches “{pattern}”" when a pattern is set); `filter` = "for {v}" when the only condition is an `Equals` (11 §5.24's "for Northwind Group"); otherwise "when " + conditions joined by " and ", each as: `Equals` "{field} is {v}", `IsOneOf` "{field} is one of {v1, v2}", `Exists` "{field} is given", `LessThan` "{field} is less than {v}", `LessOrEqual` "{field} is at most {v}", `GreaterThan` "{field} is more than {v}", `GreaterOrEqual` "{field} is at least {v}"; no filter → empty; `inputs` = "the event's {path} as *{input}*" or "{value} as *{input}*" or "the file as *{input}*", joined by ", ". The subject itself appears only under **Show details** (11 §5.24 *Words*). For a Slack message trigger, `listens` = "a message in #{channel} says “{phrase}”" + (" from {sender name}" when `from_member` is set, the name read from the newest kept message of that member, else the member id). The card's first words become "When a message in #{channel} says “{phrase}”, run **{workflow}**" for this kind (*chosen here*).
2. *Show details*: "Listens to: {subject}" (or "Files: {kind}, name {pattern}"), one line per filter condition with its raw path and value, one line per input with its source, "Waiting: up to {max_waiting}", and for core NATS "Events sent while the server is off are missed." (ADR-074). For a Slack message trigger, *Show details* reads: "Listens to: #{channel} in {workspace}", "Phrase: {phrase}", "From: {anyone | name}", inputs as today, "Waiting: up to {max_waiting}", and "Messages read more than a day late start nothing."
3. *Event plain name*: the CloudEvents `type` when present, else the last two subject tokens; `.`, `_` and `-` replaced by spaces, lower-cased. *Event label* = plain name + (", " + the first `PayloadField` input's value cut to 40 chars, when there is one).
4. *Missed or refused* (once per trigger per day, sent one hour after the first): "{n} {event plain name} events couldn't be read today, so nothing started for them." or "{n} {event plain name} events arrived while {workflow} was busy, so nothing started for them." (*chosen here* for the second).
5. */triggers line*: "{name} · {On | Off | Paused until midnight}{ · Not connected} · last event {HH:MM | none} · {n} today". For a Slack message trigger the line is the same; "Not connected" is shown for a Slack message trigger when the Slack Service reports the person is signed out of Slack (`slack.status()`'s `signed_in = false`; *chosen here*).
6. *Subject not allowed at proposal*: the proposal is refused with `TRIG_SUBJECT_NOT_ALLOWED`; Ordin shows "Your company hasn't allowed ordin to listen for these events. IT can add them." with **Send to IT** (11 §5.24) — Ordin's words, not sent by this service.

**M. A kept Slack message** (ADR-102)

1. **Slack task.** `let mut rx = slack.subscribe_kept()`. For each `KeptSlackNotice n`: find the `on` and `paused_by_daily_limit` Slack message triggers whose `slack_link_id = n.slack_link_id`; none → next notice. `RecvError::Lagged(k)` → log `triggers.slack_lagged {k}` at `warn` and continue (the skipped messages start nothing; *chosen here*: the Slack Service keeps them, and they stay readable in the workspace). `RecvError::Closed` → log `triggers.slack_closed` and end the task (the Slack Service is stopping).
2. Read `slack.kept_message(n.kept_id)`; `None` (the link was removed meanwhile) → nothing recorded; a `SlackError` → log `triggers.slack_read_failed {code}`, nothing recorded. Read `slack.link_of(n.slack_link_id)` once per notice; `None` or `no_access` → step J.5 for each matching trigger.
3. **Age.** `m.sent_at` before the trigger's `turned_on_at` → nothing recorded (the trigger did not exist for it). `received_at − m.sent_at > 24 h` → outcome `skipped_by_filter` (a message read late at catch-up starts nothing; *chosen here*).
4. **Match** (fixed code, no model turn): NFC-normalise and lower-case (Unicode) both `m.text` and `phrase`; the phrase must be a substring of the text. When `from_member` is set, `m.sender_member_id = from_member`. No match → outcome `skipped_by_filter` (recorded only when the message matched by link; nothing is sent, 11 §5.24). Text that reads as an instruction to ordin is still only data: it can match a phrase and fill an input; it never changes the trigger, its limits or its approvals (R-81, R-98).
5. **Identifier** = `"slack:" + slack_link_id + ":" + m.ts` (1–256 chars). **Inputs**: `message` → `m.text` cut to its first 4,000 characters at a scalar boundary (longer → cut, logged `TRIG_PAYLOAD_TOO_LONG`, *chosen here*: unlike an event field, a message is still readable when cut); `sender` → `m.sender_name`; `link` → `slack.permalink(&link, &m.ts)`; `Fixed` → the value. Every value is `RunInputValue::Text`, except an input of kind `List`, which takes the non-empty lines of the value (1–100 lines of 1–500 characters; else outcome `unreadable`).
6. Then steps D.5 (duplicate within 7 days), D.8 (limits: daily count, run slot, waiting queue up to `limits.max_waiting`, else `refused_by_limit`), D.9 (start; no acknowledgement call, as for core NATS), D.10 (other outcomes and notices), with `started_by: EventTrigger { trigger_id, event_label }` where `event_label` = "a message in #{channel} from {sender}" (cut to 120 chars). The started message reads "Started {workflow} · a message in #{channel} from {sender}" with **Stop**. The approval card of any external action of that run says "Started by an event: a message in #{channel} from {sender}" (step F.8 unchanged: the run is an `EventTrigger` start).
7. Waiting Slack items are kept in memory only; after a restart the queue is empty and the Slack Service's catch-up redelivers unmatched messages, which step 5's identifier check deduplicates (I.2 extended).

## Data Model

Tables owned by this crate (SQLite, server database; personal profile only — no PostgreSQL variant, because the service is not started in the agent profile).

```sql
-- migration ordin-triggers 0001
CREATE TABLE triggers (
    id                 TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id       TEXT NOT NULL CHECK (length(workspace_id) = 36),
    name               TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    listens_for        TEXT NOT NULL,            -- JSON TriggerSource
    delivery           TEXT,                     -- JSON NatsDelivery; NULL for chat triggers
    filter             TEXT NOT NULL DEFAULT '[]',
    input_mapping      TEXT NOT NULL DEFAULT '[]',
    identifier_source  TEXT,                     -- JSON IdentifierSource
    workflow_id        TEXT NOT NULL CHECK (length(workflow_id) = 36),
    workflow_version   INTEGER NOT NULL CHECK (workflow_version >= 1),
    limits             TEXT NOT NULL,            -- JSON TriggerLimits
    state              TEXT NOT NULL CHECK (state IN ('proposed','on','off','paused_by_daily_limit')),
    turned_on_by       TEXT,
    turned_on_at       TEXT,
    UNIQUE (workspace_id, name),
    CHECK ((json_extract(listens_for,'$.kind') = 'nats_event') = (identifier_source IS NOT NULL))
);
CREATE INDEX triggers_state ON triggers(state);

CREATE TABLE trigger_events (
    id            TEXT PRIMARY KEY CHECK (length(id) = 36),
    trigger_id    TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
    identifier    TEXT NOT NULL CHECK (length(identifier) BETWEEN 1 AND 256),
    received_at   TEXT NOT NULL,
    outcome       TEXT NOT NULL CHECK (outcome IN ('started','skipped_by_filter','duplicate','refused_by_limit','unreadable')),
    crew_run_id   TEXT  -- set once the started workflow run exists (2C C-8)
);
CREATE INDEX trigger_events_seen ON trigger_events(trigger_id, identifier, received_at);
CREATE INDEX trigger_events_age ON trigger_events(received_at);

CREATE TABLE trigger_daily_counts (
    day      TEXT PRIMARY KEY,  -- YYYY-MM-DD, server-local
    started  INTEGER NOT NULL DEFAULT 0 CHECK (started >= 0),
    limit_notice_sent INTEGER NOT NULL DEFAULT 0 CHECK (limit_notice_sent IN (0,1))
);

CREATE TABLE trigger_notices (
    trigger_id  TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
    day         TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('unreadable','refused_by_limit')),
    count       INTEGER NOT NULL DEFAULT 0,
    notify_at   TEXT NOT NULL,
    notified    INTEGER NOT NULL DEFAULT 0 CHECK (notified IN (0,1)),
    PRIMARY KEY (trigger_id, day, kind)
);

CREATE TABLE trigger_pending_deletes (
    trigger_id  TEXT PRIMARY KEY,
    subject     TEXT NOT NULL
);
```

No payload is stored anywhere (`TEV_PAYLOAD_STORED`). Each event's outcome is also appended to the journal, which keeps it for the journal's retention; `trigger_events` keeps identifiers for 7 days only. Rollback: the migration adds tables only; restoring the pre-migration backup removes them.

*Amended January 2026 (ADR-102):* no schema change. `listens_for` is JSON `TriggerSource`, which takes the new variant; the existing CHECK `(json_extract(listens_for,'$.kind') = 'nats_event') = (identifier_source IS NOT NULL)` already holds for `slack_message` (no identifier source). No migration step is added; a server that downgrades reads a `slack_message` row as an unknown variant, which *Internal Logic* A.1 of the older build logs and skips (*assumed here*; rollback consideration: turn Slack triggers off before a downgrade).

## Error Table

In-process API; Status is the error and whether the caller may retry. Chat surfaces send the words of step L, never a code.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Proposal name empty, over 80 chars, or taken in the workspace | `Err(permanent)` | `TRIG_NAME_INVALID` | `TriggerError { code, retryable: false }` |
| A start or proposal after the shutdown token was cancelled | `Err(retryable)` | `TRIG_STOPPING` | `TriggerError { code, retryable: true }` |
| `delivery`/`identifier_source` present for a chat trigger or absent for a NATS trigger | `Err(permanent)` | `TRIG_NATS_FIELDS_MISMATCH` | as above |
| Subject outside the profile's list | `Err(permanent)` | `TRIG_SUBJECT_NOT_ALLOWED` | as above |
| A required input not mapped | `Err(permanent)` | `TRIG_INPUT_UNMAPPED` | as above |
| A fixed value over 4,000 chars (proposal); a payload value over 4,000 chars (event → `unreadable`) | `Err(permanent)` / outcome | `TRIG_PAYLOAD_TOO_LONG` | as above / log only |
| More than 20 filter conditions | `Err(permanent)` | `TRIG_FILTER_TOO_LONG` | as above |
| A filter value of the wrong shape for its operator | `Err(permanent)` | `TRIG_FILTER_VALUE_INVALID` | as above |
| A path outside the grammar | `Err(permanent)` | `TRIG_PATH_INVALID` | as above |
| `max_waiting` above 10 | `Err(permanent)` | `TRIG_WAITING_OUT_OF_RANGE` | as above |
| File kind or name pattern invalid | `Err(permanent)` | `TRIG_FILE_KIND_INVALID`, `TRIG_PATTERN_INVALID` | as above |
| Workflow or pinned version not found | `Err(permanent)` | `TRIG_WORKFLOW_NOT_FOUND` | as above |
| Trigger id unknown | `Err(permanent)` | `TRIG_NOT_FOUND` | as above |
| Turn on/off/remove from a state that does not allow it | `Err(permanent)` | `TRIG_STATE_INVALID` | as above |
| Turn on a NATS trigger with no NATS servers in the profile | `Err(permanent)` | `TRIG_EVENTS_NOT_CONFIGURED` | as above |
| `/run` with no workspace in the conversation | message sent in the chat | `TRIG_NO_WORKSPACE` (log) | — |
| Event while the run slot is busy and the queue is full | outcome `refused_by_limit` | `TRIG_RUN_IN_PROGRESS` (log) | — |
| Event after the daily limit | outcome `refused_by_limit` | `TRIG_DAILY_LIMIT_REACHED` (log) | — |
| Identifier absent or over 256 chars | outcome `unreadable` | `TEV_IDENTIFIER_INVALID` (log) | — |
| Ordin Host refuses or fails to start the workflow run | `Err(retryable)` inside the actor; person told | `TRIG_START_FAILED` | log `triggers.start.failed` |
| Opening or deleting a NATS feed fails | `Err(retryable)` | `TRIG_NATS_FAILED` | log `triggers.feed.failed` |
| Datastore read or write fails | `Err(retryable)` | `TRIG_DATASTORE_FAILED` | the event is not acknowledged, so JetStream redelivers it |
| Slack phrase empty or over 200 chars | `Err(permanent)` | `TRIG_PHRASE_INVALID` | `TriggerError { code, retryable: false }` |
| Slack link unknown, unlinked, of another workspace, or without access | `Err(permanent)` (retryable when the Slack Service was unavailable) | `TRIG_SLACK_LINK_UNKNOWN` | `TriggerError { code, retryable }` |
| `from_member` not a Slack member id | `Err(permanent)` | `TRIG_MEMBER_INVALID` | `TriggerError { code, retryable: false }` |
| Slack turned off by the profile (`slack.tools_enabled = false`) at proposal or **Turn on** | `Err(permanent)` | `TRIG_SLACK_OFF` | `TriggerError { code, retryable: false }` |
| The kept-message broadcast lagged | logged only | — (`triggers.slack_lagged`) | — |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Trigger Service

  Background:
    Given the personal profile with Telegram paired
    And workspace "Finance" holds workflow "Invoice check" version 4 with inputs "Invoice" (document, required) and "Customer" (text)
    And the profile allows subjects "finance.invoice.*" and sets server.triggers.daily_runs to 20
    And trigger T "Invoice check on invoice created" is on, listening to "finance.invoice.created" on JetStream,
        with filter data.customer equals "Northwind Group", identifier cloudevents_id,
        and inputs Invoice ← data.pdf_url, Customer ← data.customer

  Scenario: Happy path — an event starts the workflow and is acknowledged after recording
    When a CloudEvents event with id "9f1c" and data {"customer":"Northwind Group","pdf_url":"https://ledger.example/INV-2291.pdf","number":"INV-2291"} arrives
    Then a trigger_events row with outcome "started" and identifier "9f1c" exists before the event is acknowledged
    And Ordin Host is asked to start "Invoice check" version 4 with Invoice "https://ledger.example/INV-2291.pdf" and Customer "Northwind Group"
    And the person receives "Started Invoice check · invoice created, https://ledger.example/INV-22…" with a Stop button
    And today's started count is 1

  Scenario: Edge — duplicate event within 7 days
    Given an event with id "9f1c" was recorded as started 2 days ago
    When the same event is delivered again
    Then a row with outcome "duplicate" is recorded and the event is acknowledged
    And no run is started and nothing is sent to the person

  Scenario: Edge — filter does not hold
    When an event with data.customer "Acme" arrives
    Then the outcome is "skipped_by_filter" and nothing is sent to the person

  Scenario: Edge — one at a time, then the queue
    Given T's run is going
    When 3 events that pass the filter arrive
    Then they wait in arrival order, unacknowledged, each kept alive with in-progress acknowledgements
    And the feed capacity is 7
    When the running crew run finishes
    Then the oldest waiting event is recorded as started, acknowledged, and its run starts

  Scenario: Error — payload missing a field the trigger uses
    When an event without data.pdf_url arrives
    Then the outcome is "unreadable" and the event is acknowledged
    And one hour later the person receives "1 invoice created events couldn't be read today, so nothing started for them."
    And no second such message is sent that day for T

  Scenario: A list input is mapped from a JSON array
    Given trigger T maps the List input "Suppliers" from data.suppliers
    When an event with data.suppliers ["Kestrel", "Northwind"] arrives
    Then the run starts with Suppliers as a list of 2 items
    And an event with data.suppliers "Kestrel" is recorded as "unreadable"

  Scenario: Error — not JSON
    When an event with payload "<invoice/>" arrives
    Then the outcome is "unreadable" with identifier "unreadable:" followed by 16 hex digits

  Scenario: Error — daily limit reached
    Given 19 triggered runs started today
    When an event starts the 20th run
    Then every NATS trigger that is on becomes paused_by_daily_limit
    And the person receives "20 runs started from events today, the most allowed. Events will start runs again after midnight." with "Turn off this trigger"
    When a core-NATS event arrives for another trigger
    Then its outcome is "refused_by_limit"
    When local midnight passes
    Then the paused triggers are on again and today's count is 0

  Scenario: Crash between recording and acknowledgement
    Given an event was recorded as started and the program stopped before the acknowledgement
    When the program starts and JetStream redelivers the event
    Then the outcome is "duplicate" and no second run starts

  Scenario: A file sent to the chat matches a chat trigger
    Given chat trigger F is on for file kind "pdf" with pattern "invoice*" and input Invoice ← the file
    When the person sends "Invoice-0451.pdf"
    Then the run starts with Invoice set to the saved file and the reply reads "Started Invoice check with Invoice-0451.pdf" with Stop
    And Ordin gets no conversation turn for the file

  Scenario: /run with a name asks the inputs one at a time
    When the person sends "/run invoice check"
    Then the gateway is asked to collect "Invoice" then "Customer" with the verb "Run Invoice check"
    When the form is completed
    Then the run starts as the person's run and is not counted towards the daily limit

  Scenario: /triggers lists state and connection
    Given the NATS Adapter is Reconnecting
    When the person sends "/triggers"
    Then one message reads "Invoice check on invoice created · On · Not connected · last event 08:14 · 1 today" with a Turn off button

  Scenario: Subject removed from the profile
    When the profile no longer allows "finance.invoice.*"
    Then T becomes off, its feed is closed, and the person is told it was turned off

  Scenario: Proposal with a subject outside the list
    When Ordin proposes a trigger on "hr.payroll.run"
    Then propose returns TRIG_SUBJECT_NOT_ALLOWED and no row is written

  Scenario: Cancelling the shutdown token pauses the service
    Given trigger T is on with 2 events waiting
    When the program cancels the shutdown token passed to start
    Then no run starts and neither waiting event is acknowledged
    And T's NATS consumer is not deleted

  Scenario: A chat-file trigger hands over the saved Inbox file
    Given a turned-on chat-file trigger for "Invoice check" maps the file to input "Invoice"
    When the paired chat sends "inv-118.pdf" and the Channel Gateway saved it as place_file P "Inbox/inv-118.pdf"
    Then start_workflow_run receives "Invoice" as Document with ContentRef place_file P "Inbox/inv-118.pdf"
    And started_by is ChatFileTrigger with the trigger's id and file name "inv-118.pdf"
```

## Performance, Security, Observability

- **Performance targets:** from `deliver` to the journal record p95 < 50 ms on SQLite (one indexed lookup, one insert); filter evaluation < 1 ms for 20 conditions on a 1 MiB payload; at most 100 triggers × 11 events in memory (≤ 1.1 GiB worst case at 1 MiB each; typical payloads are under 10 KiB); the actor channel of 32 bounds memory per trigger before the NATS Adapter's pull bound applies.
- **Security:** event content is data only — no field reaches a model except as a named workflow input of ≤ 4,000 chars (R-65); the filter is fixed code with no expression language; subjects are checked against the profile's list at proposal, at **Turn on**, at start and on each profile change; only the paired person's chat items reach this service (the Channel Gateway checks the sender); payloads are never stored or logged; identifiers are logged only as the first 12 hex chars of their SHA-256; a triggered run gets no advance approval — approval cards name the event through `started_by_line`.
- **Observability (logs, `tracing`, target `ordin_triggers`):** `triggers.proposed {trigger_id, kind}`, `triggers.turned_on {trigger_id}`, `triggers.turned_off {trigger_id, reason}`, `triggers.removed {trigger_id}`, `triggers.event {trigger_id, outcome, identifier_digest, crew_run_id?}`, `triggers.event.dropped {trigger_id}`, `triggers.queue {trigger_id, waiting, free}`, `triggers.start.failed {trigger_id, code}`, `triggers.feed.failed {trigger_id, code}`, `triggers.daily_limit {count, limit}`, `triggers.reset {day}`, `triggers.notice.sent {trigger_id, kind, count}`. No metrics or spans (personal server, SA-XC-04/05).

- **Slack messages — Performance:** matching one kept message p95 ≤ 5 ms (substring search over at most 40,000 characters per trigger, at most 50 Slack triggers per installation, *chosen here*).
- **Slack messages — Security:** a Slack message is data from people who are not the person (ADR-074 *Messages from other people…*, amended by ADR-102): it fills named inputs of at most 4,000 characters and nothing else; the trigger was approved by the person's **Turn on**; every external action of the run still asks the person; the daily limit and the queue bound the use of the person's plan (R-98).
- **Slack messages — Never logged:** the message text, the sender's name, the phrase.
- **Slack messages — Observability:** log events `triggers.slack_lagged {k}`, `triggers.slack_closed {}`, `triggers.slack_read_failed {code}`; journal entries as for events, `trigger_event` with `outcome` and `identifier_digest`.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

On the personal server the service also starts saved workflows from messages the Slack Service keeps from Slack channels a workspace links (ADR-102; amends ADR-074's rejection of group-chat triggers for linked Slack channels only). The sections above carry the change in place, each marked *Amended January 2026 (ADR-102)* where a paragraph was added:

- **Shared Context:** `TriggerSource::SlackMessage { slack_link_id, phrase, from_member }`; the three Slack validation rules (`TRIG_PHRASE_INVALID`, `TRIG_SLACK_LINK_UNKNOWN`, `TRIG_MEMBER_INVALID`) and the widened `TRIG_NATS_FIELDS_MISMATCH`; configuration rows for `slack.tools_enabled`, the 24-hour age limit and the 4,000-character input; the Slack Service calls (`subscribe_kept`, `kept_message`, `link_of`, `permalink`).
- **Public Interface:** `TriggerDeps.slack`; `start` subscribes to kept messages; `TRIG_SLACK_OFF`; a Slack trigger proposal example and its card.
- **Internal Logic:** A.4, A.5, B.1, C.1, H.2, H.3, J.4, J.5, L.1, L.2, L.5 and the new step M.
- **Data Model:** no schema change. **Error Table:** four rows and a logged lag. **Performance, Security, Observability:** Slack message lines.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a message in a linked Slack channel starts a workflow
    Given workspace "Q3 supplier review" links Slack channel "#supplier-desk"
    And the Slack message trigger "Supplier query on #supplier-desk" with phrase "quote request" is on
    When the Slack Service keeps a message from "Ahmed Khan" in "#supplier-desk" reading "New Quote Request for Northwind"
    Then a trigger event with identifier "slack:<link id>:<ts>" and outcome started is recorded
    And Supplier query starts with Request "New Quote Request for Northwind" and Asked by "Ahmed Khan"
    And the person is told "Started Supplier query · a message in #supplier-desk from Ahmed Khan"

  Scenario: Edge case — a message without the phrase starts nothing
    Given the same trigger is on
    When a message "Lunch at noon?" is kept in "#supplier-desk"
    Then the outcome is skipped_by_filter and nothing is sent to the person

  Scenario: Edge case — the same message delivered again after a restart
    Given a trigger event started for identifier "slack:<link id>:1729500000.000100" today
    When the Slack Service's catch-up delivers that message again
    Then the outcome is duplicate and no run starts

  Scenario: Edge case — a message read more than a day late starts nothing
    Given the server was stopped for 30 hours
    When the catch-up keeps a matching message sent 29 hours ago
    Then the outcome is skipped_by_filter

  Scenario: Error path — the twenty-first triggered run of the day, from Slack
    Given server.triggers.daily_runs is 20 and 20 triggered runs started today
    When a matching message is kept in "#supplier-desk"
    Then the outcome is refused_by_limit with code TRIG_DAILY_LIMIT_REACHED
    And the trigger is paused_by_daily_limit until midnight
    And "20 runs started from events today, the most allowed. Events will start runs again after midnight." was sent exactly once today

  Scenario: Error path — proposing a trigger on a channel of another workspace
    Given "#hr-private" is linked to workspace "HR"
    When Ordin proposes a Slack message trigger on that link for "Q3 supplier review"
    Then the proposal is refused with TRIG_SLACK_LINK_UNKNOWN

  Scenario: Error path — the channel is unlinked while the trigger is on
    When the person removes "#supplier-desk" from the workspace
    And the next kept-message notice for that link is read
    Then the trigger is off and the person is told "I turned off Supplier query on #supplier-desk because #supplier-desk is no longer linked to Q3 supplier review."

  Scenario: Error path — the company turns Slack off
    Given a Slack message trigger is on
    When the profile's slack.tools_enabled becomes false
    Then the trigger is off and a journal entry trigger_turned_off with reason slack_off is written
```

### Gaps

None.
