# SPEC: Rules Engine

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-rules` · **Build phase:** 3 · **Programs:** D P A
> **Dependencies:** Approval Service, Journal, Organisation Profile

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
- [Amendment January 2026 — Slack for working together (ADR-102)](#amendment-january-2026--slack-for-working-together-adr-102)
- [Amendment January 2026 — Content checks done by ordin (ADR-107)](#amendment-january-2026--content-checks-done-by-ordin-adr-107)

## Purpose

The Rules Engine is ordin's one fixed-code, deterministic evaluator of what an agent may do (02:649–656; ADR-005, ADR-098 item 4). It makes no model call. It decides at three points of every run — the start, before each tool call, and the end — and each decision is allow, deny or ask a person. Its sources are, in this order of strength: the application's own policy (agent-folder and folder-grant limits, the shell-command policy, the approval policy for externally visible actions, and the profile's email and server rules), the package's declaration with wrightrun's ControlEvaluator vocabulary in full (gates, message rules, step gates, argument rules, duty gates, judged loops, bounds), and the agent's own tool policy from crucible (prohibited verbs, forbidden flags, actions sent for approval), which can only narrow (ADR-090 item 1). Every decision is journalled with the rule that produced it, and an *ask* is turned into an approval through the Approval Service before the decision returns. Counters and locks live in the datastore, so a count is exact across instances of the agent profile's scaled shape.

## Approach

A pure, table-driven evaluator over one immutable `ControlSet` per agent instance, compiled at load from the package declaration, the agent configuration's controls (agent profile) and the resolved tool policy; per-run facts (commands, tool counts, set-aside gates, leases) are kept in a `RunFacts` record persisted on every decision so that a run paused on one instance resumes on another (C-80). Facts the engine cannot see itself — work-folder files, settings, runs in progress — and the person-facing wording of approval cards and start-up lines come through one trait the engine owns, `RulesHost`, which the program implements over the File Service, Settings Store, Run Queue and Localisation (ADR-100 item 1 pattern; 2B-3 gives the engine no edge to them). Patterns are compiled with the `regex` crate (ADR-098 item 4). Error prefix: `RULE_` (2C). Rejected: evaluating rules in each caller (Hook Relay, MCP Front, Copilot adapter) — two implementations drift, which ADR-098 removed. Rejected: reading the Run Queue's and Settings Store's tables directly — 2B-1 rule 5 forbids reading another module's tables.

## Shared Context

### Types from `ordin-types` (2C wire §3, §6; copied — do not redefine)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOrigin { BuiltIn, OrdinMcpFront { server: String }, PackageMcpServer { server: String } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolCall {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub point: DecisionPoint,
    pub tool: String,
    pub origin: ToolOrigin,
    /// Never journalled, only its digest.
    pub arguments: serde_json::Value,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionPoint { RunStart, BeforeToolCall, RunEnd }

/// 2C wire `ToolDecision` (a struct), the engine's full decision. The `ToolRequest` entity's stored
/// verdict is the 2C domain enum `ToolRequestDecision` (Allow, Deny, Ask; decision 80, C-2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolDecision { #[serde(flatten)] pub verdict: Verdict, pub rule: RuleRef, pub decided_at: Timestamp }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Verdict { Allow, Deny { reason: String }, Ask { proposal: ActionProposal } }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RuleRef {
    pub source: RuleSource,
    pub rule_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<AgentInstanceId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    DeclarationGate, DeclarationToolRule, DeclarationArgumentRule, AgentToolPolicy,
    AppFolderPolicy, AppShellPolicy, AppApprovalPolicy, FailClosed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ActionProposal { pub action: String, pub destination: String, pub affects: Vec<AffectedItem>, pub consequence: String }
// AffectedItem: File { label, version, place_id } | Record { label } | Recipient { name, address, outside_company }

// 2C wire §6
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartUpReportItem {
    EnforcedRule { rule: RuleRef, plain: String },
    RuleNotEnforced { rule: RuleRef, plain: String, reason: NotEnforcedReason },
    UnavailablePart { part: String, plain: String, reason: UnavailableReason },
    AvailableTool { tool: String, origin: ToolOrigin, plain: String },
    VerbNotOffered { adapter: AdapterKind, verb: AdapterVerb, plain: String },
    OwnScreensNotShown { server: String, plain: String },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotEnforcedReason { NotApplicableToLocalSession, RouteCannotCarry }

// 2C domain
pub enum Authority { DocumentOnly, SystemChanging, RoutingOnly }            // snake_case
pub enum FolderAccess { Read, ReadAndChange }                               // snake_case
pub enum Grantee { AgentInstance { agent_instance_id: AgentInstanceId }, Crew { crew_charter_id: CrewCharterId } }
pub struct ToolRequest {
    pub id: ToolRequestId, pub run_id: RunId, pub tool: String, pub arguments_digest: Sha256Hex,
    pub decision: Option<ToolRequestDecision /* domain enum Allow|Deny|Ask */>,
    pub deciding_rule: Option<String>, pub approval_id: Option<ApprovalId>,
    pub state: ToolRequestState /* Requested|Decided|Executed|Refused */, pub requested_at: Timestamp,
}
pub struct Sha256Hex(pub String);   // 64 lowercase hex characters
pub struct EmailAttachment { pub place_id: PlaceId, pub path: String }   // 2C domain (Mail)
```

Validation copied from 2C: `ToolCall.tool` 1–128 characters → `RULE_TOOL_NAME_INVALID`; `arguments` serialised ≤ 1 MiB → `RULE_ARGUMENTS_TOO_LARGE`; `run_id`/`agent_instance_id` must match the session → `RULE_SESSION_MISMATCH`; `ToolOrigin.server` 1–64 characters `^[a-zA-Z0-9_.-]+$` → `RULE_ORIGIN_INVALID`, and a server name the MCP Front serves is classified `ordin_mcp_front` whatever the hook claims; `Deny.reason` 1–500 characters; `rule_id` 1–128 characters; a rule with `fail_closed` always denies; an agent's own policy never widens (`RULE_POLICY_WIDENS` at load).

### Types from other crates (used)

- `ordin-approval` (Approval Service spec): `ApprovalService::request(ApprovalRequest) -> Result<ApprovalCard, ApprovalError>`, `ApprovalService::get(ApprovalId) -> Result<ApprovalRecord, ApprovalError>`, `ApprovalService::wait(ApprovalId, Option<tokio::time::Instant>) -> Result<WaitOutcome, ApprovalError>` with `WaitOutcome { Decided(ApprovalState), DeadlinePassed }`; `ApprovalRequest { workspace_id, asked_by: ApprovalAsker, question, verb, where_line, affects, changes, content: Option<ApprovalContent>, reject_style, show_details, proposal: ActionProposal, asker: Option<String>, leaves_company: bool }`; `ApprovalContent::Command { label, command, runs_in }`; `ApprovalState { Pending, Approved, Rejected, Expired }`.
- `ordin-journal` (Journal spec, B2 form, R3): `Datastore { Sqlite(SqlitePool), Postgres(PgPool) }` (the program passes `journal.datastore().clone()`), `ModuleMigrations` built from `MigrationStep = (i64, &'static str, &'static str)`; `Journal::record_tool_request(&self, run_id: RunId, tool: &str, arguments_digest: Sha256Hex) -> Result<ToolRequest, JournalError>` (the Journal mints the `ToolRequestId`), `decide_tool_request(&self, id: ToolRequestId, decision: ToolRequestDecision, deciding_rule: &str, approval_id: Option<ApprovalId>) -> Result<ToolRequest, JournalError>`, `finish_tool_request(&self, id: ToolRequestId, executed: bool) -> Result<ToolRequest, JournalError>`; `append_entry(&self, NewJournalEntry { kind, component: "rules", run_id, crew_run_id, workspace_id, summary, data }) -> Result<JournalEntry, JournalError>` for decision records that are not tool requests (run start, run end, command decisions).
- `ordin-organisation-profile`: `OrganisationProfileService::current() -> Arc<ProfileSnapshot>`; the engine reads these 2D-2 keys: `email.company_domains[]`, `email.internal_only`, `email.max_recipients`, `email.no_attach_places[]`, `email.foot_line_required`, `server.external_actions_off[]`. In the agent profile no profile is read (2D Gaps item 1) and `ProfileSnapshot::defaults()` applies. `server.external_actions_off[]` also drives check D.5.1b for the Slack tools (amended January 2026, ADR-102).

### Fixed constants (2D-4, LW limits, and chosen here)

```rust
pub const MAX_PATTERN_CHARS: usize = 200;        // wrightrun A-18, SA-71
pub const MAX_MATCH_CHARS: usize = 4000;         // text a pattern or keyword list is matched against
pub const LEASE_SECONDS: u32 = 30;               // 2D-4 run lease; duty locks use the same figure
pub const LEASE_RENEW_SECONDS: u32 = 10;
pub const PLACE_NO_RUN_SECONDS: u32 = 60;
pub const MAX_ARGUMENT_RULES: usize = 200;       // SA-71
pub const MAX_ARGUMENT_BYTES: usize = 1_048_576; // LW-18
pub const RUN_FACTS_TTL_SECONDS: u64 = 86_400;   // a run record older than a day is dropped from memory
pub const BUNDLE_DIR_PREFIXES: [&str; 4] = ["/.apm/", "/knowledge/", "/acceptance/", "/brand/"];
pub const BUNDLE_FILES: [&str; 3] = ["/AGENTS.md", "/wrightrun-manifest.json", "/wrightrun-declaration.yaml"];
pub const DUTY_ENDINGS: [&str; 5] = ["stop-present", "escalation-present", "nothing-to-do", "overlap", "cycle-limit"];
```

## Public Interface

Everything below is in crate `ordin-rules`. Names are *chosen here* unless marked 2C or wrightrun (the vocabulary of `agent-wright-runtime/docs/spec/components/ControlEvaluator.md`, carried into Rust under ADR-098 item 2, which keeps `wrightrun-declaration.yaml`'s format; YAML keys are the field names with `_` written `-`, which the Package Inspector's parser applies).

### The control vocabulary (wrightrun, in Rust)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Point { Start, BeforeTool, End }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Condition {
    PathExists { path: String },                                  // start, before-tool
    PathAbsent { path: String },
    PathContains { path: String, text: String },                  // text 1–200 characters
    RunInProgress,                                                // start
    StartCount { of: CountOf, more_than: u32, period_seconds: u32 }, // 0..100000; 1..2592000
    RepeatedMessage { period_seconds: u32 },                      // 1..86400
    Keywords { words: Vec<String>, #[serde(default)] r#match: MatchMode }, // 1..50 words of 1–50 chars
    KeywordShare { words: Vec<String>, share: DecimalShare },     // 0.5..1.0
    Pattern { patterns: Vec<String>, #[serde(default)] r#match: MatchMode }, // 1..10, each ≤ 200 chars
    Setting { name: String, equals: serde_json::Value },          // string | number | bool
    Called { target: CallTarget, name: String, #[serde(default = "one")] count: u32 },     // before-tool, end; 1..1000
    NotCalled { target: CallTarget, name: String, #[serde(default = "one")] count: u32 },
    Elapsed { at_least_seconds: u32 },                            // before-tool; 1..86400
    ChangedWorkFolder,                                            // end
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "kebab-case")]
pub enum CountOf { Command, Scope, Asker }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "kebab-case")]
pub enum MatchMode { #[default] Any, All }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "kebab-case")]
pub enum CallTarget { Tool, Subagent }
/// A share written as a decimal string in 0.50–1.00 (no float enters the model, 2C Conventions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] #[serde(transparent)]
pub struct DecimalShare(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DecidingOutcome {
    End { ending: String },
    Reply { text: String },                 // 1–4000 characters
    Command { command: String },
    Tools { tools: Vec<String> },           // may be empty
    SingleCall { command: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AdditiveOutcome { AttachAction { action: String }, Delivery { to: DeliveryTo } }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "kebab-case")]
pub enum DeliveryTo { Asker, Thread }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GateOutcome { Refuse, RefuseEnding { #[serde(default = "three")] pause_after: u8 } } // 1..10

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MessageRule {
    pub name: String,                                   // slug ^[a-z0-9][a-z0-9-]{0,63}$
    #[serde(default)] pub priority: i16,                // -1000..1000
    #[serde(default)] pub command: Option<String>,
    pub conditions: Vec<Condition>,                     // ≥ 1; start-point kinds only
    #[serde(default)] pub decides: Option<DecidingOutcome>,
    #[serde(default)] pub adds: Vec<AdditiveOutcome>,   // at least one of decides / adds
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct StepGate {
    pub name: String,
    pub point: Point,                                   // BeforeTool or End
    #[serde(default)] pub command: Option<String>,
    #[serde(default)] pub tool: Option<String>,         // None = every call
    pub conditions: Vec<Condition>,
    pub outcome: GateOutcome,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AddressCondition {
    #[serde(default)] pub schemes: Vec<String>,
    #[serde(default)] pub hosts: Vec<String>,
    #[serde(default)] pub with_subdomains: bool,
    #[serde(default)] pub host_is_ip: Option<bool>,
    #[serde(default)] pub host_is_single_label: Option<bool>,
    #[serde(default)] pub ports: Vec<u16>,
    #[serde(default)] pub path_prefixes: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ArgumentRule {
    pub name: String,
    pub tool: String,
    pub argument: String,                               // "url", "body.target.href", "items[].url"
    pub form: ArgumentForm,
    #[serde(default)] pub outcome: ArgumentOutcome,
    #[serde(default)] pub present: Option<bool>,
    #[serde(default)] pub equals_any: Vec<String>,      // ≤ 100
    #[serde(default)] pub patterns: Vec<String>,        // ≤ 10
    #[serde(default)] pub max_length: Option<u32>,
    #[serde(default)] pub address: Option<AddressCondition>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "kebab-case")]
pub enum ArgumentForm { RefuseWhen, RefuseUnless }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "kebab-case")]
pub enum ArgumentOutcome { #[default] Refuse, Pause }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Check { pub kind: CheckKind, #[serde(default)] pub condition: Option<Condition>, #[serde(default)] pub command: Option<String> }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "kebab-case")]
pub enum CheckKind { File, Command }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HeartbeatForm { pub entry: String, pub endings: Vec<String> }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DutyGate {
    pub duty: String, pub command: String, pub folder: String,     // folder "/.agent-run/<duty>/"
    #[serde(default)] pub work_check: Option<Check>,
    #[serde(default)] pub done_check: Option<Check>,
    #[serde(default)] pub max_cycles: Option<u32>,
    #[serde(default)] pub period_seconds: Option<u32>,
    #[serde(default)] pub max_failed_cycles: Option<u32>,
    #[serde(default)] pub heartbeat: Option<HeartbeatForm>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct JudgedLoopGate { pub commands: Vec<String>, pub reviewer: String, #[serde(default = "three")] pub pause_after: u8 }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CommandBound {
    pub command: String,
    #[serde(default)] pub wall_clock_minutes: Option<u32>,          // 1..1440, enforced by the Session Supervisor
    #[serde(default)] pub max_rounds: Option<u32>,                  // listed "not enforced"
    #[serde(default)] pub max_repeated_failures: Option<u32>,       // listed "not enforced"
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Gates {
    #[serde(default)] pub duties: Vec<DutyGate>,
    #[serde(default)] pub judged_loops: Vec<JudgedLoopGate>,
    #[serde(default)] pub bounds: Vec<CommandBound>,
    #[serde(default)] pub steps: Vec<StepGate>,
    #[serde(default)] pub argument_rules: Vec<ArgumentRule>,
}
/// The parts of `wrightrun-declaration.yaml` this engine reads; parsed by the Package Inspector.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Declaration {
    pub format_version: u32,
    #[serde(default)] pub gates: Gates,
    #[serde(default)] pub message_rules: Vec<MessageRule>,
    /// YAML `approval-required`: tool names (or `task`, `task:<sub-agent>`) that always ask a
    /// person (wrightrun ToolBinder `approval`). The Package Inspector copies it into
    /// `InstanceRuleSet.approval_list` (SA-CLOSE-01). Default empty.
    #[serde(default)] pub approval_required: Vec<String>,
}
/// Agent profile: the agent configuration's `controls` section (wrightrun keys
/// `controls.message-rules`, `controls.steps`, `controls.argument-rules`), and its schedule entries.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredControls {
    pub message_rules: Vec<MessageRule>,
    pub steps: Vec<StepGate>,
    pub argument_rules: Vec<ArgumentRule>,
    /// (duty, command) of each schedule entry naming a duty.
    pub schedule_entries: Vec<(String, String)>,
}
```

### The agent's own tool policy (ADR-090 item 1; crucible `ResolvedToolPolicy`)

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentToolPolicy {
    /// crucible `toolPermissions.workspaceOverrides.prohibited`, e.g. "git.push".
    pub prohibited_verbs: Vec<String>,
    /// Flags the agent's tool bindings forbid, e.g. "--no-verify".
    pub forbidden_flags: Vec<String>,
    /// crucible `defaultToolPolicy.requiresHumanApproval`: verbs or tool names sent for approval.
    pub requires_approval: Vec<String>,
    /// MCP server names the agent declares; started only through the trust policy (Checker).
    pub mcp_servers: Vec<String>,
}
```

### What one agent instance brings (handed in by the Package Inspector and the Resolver)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstanceRuleSet {
    pub agent_instance_id: AgentInstanceId,
    pub package_name: String,
    pub authority: Authority,
    /// Absolute path of the agent folder (desktop, personal server) or of the bundle (agent profile).
    pub agent_folder: std::path::PathBuf,
    /// Agent profile: the bundle is read-only and the work folder is reached only through ordin's
    /// file tools (ADR-098 item 3), so built-in writes are refused everywhere.
    pub agent_folder_read_only: bool,
    pub declaration: Option<Declaration>,
    pub configured: Option<ConfiguredControls>,
    pub tool_policy: Option<AgentToolPolicy>,
    /// The package's approval list (wrightrun ToolBinder `approval`): tools that always ask.
    pub approval_list: Vec<String>,
    /// Tools of the package's own MCP servers that the package declares as reading only;
    /// every other package MCP tool asks (assumed here, SA-RULE-03).
    pub package_read_only_tools: Vec<String>,
    /// Bound tool descriptions: name and argument places (a place ending ".*" covers every place below it).
    /// On the desktop and personal server the places come from the ToolBinding binding's
    /// `argumentPaths` (JSON Pointers), converted by the Package Inspector (SA-CLOSE-02).
    pub tools: Vec<ToolDescription>,
    /// Operations the package names that no connection maps (agent profile).
    pub unmapped_operations: Vec<String>,
    /// Agent profile: every offered tool name of a connection marked `external: true` (ADR-097
    /// item 4). Each call of one asks a person and its approval carries `leaves_company = true`.
    /// Filled by the Agent Loader from the Connection Binder's `external` list; empty elsewhere.
    pub external: Vec<String>,
    pub commands: Vec<String>,
    pub reviewer: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolDescription {
    pub name: String,
    pub arguments: Vec<String>,
    /// Agent profile: `<connection>/<exposed operation>` for a mapped operation (Connection Binder);
    /// None for every other tool. Shown in the control line's `detail`; never used to decide.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub served_by: Option<String>,
}

/// A place granted in a workspace, resolved to a local root by the Workspace Service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GrantedPlace { pub grantee: Grantee, pub place_id: PlaceId, pub root: std::path::PathBuf, pub access: FolderAccess }
```

### How a caller describes a call (the 2C `ToolCall` plus what only the caller knows)

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CallContext {
    /// The harness's sub-agent that made the call, when the hook input names one (C-78).
    pub subagent: Option<String>,
    /// What the call does, when the caller knows it; `None` lets the engine classify it.
    pub class: Option<ToolClass>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolClass {
    ReadOnly,
    /// Writes inside the agent folder, a granted place or the work folder.
    LocalWrite,
    /// Has an effect outside ordin. `kind` is one of the kinds `server.external_actions_off[]`
    /// may name: "slack_post", "connection_write", "email_send", "origin_push", "copy_publish",
    /// "telegram_post", "github_operation", "hosted_run_start".
    External { kind: String, approval: ApprovalNeed, destination: String, leaves_company: bool },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalNeed {
    /// Ask a person (the default for externally visible actions, ADR-005).
    Always,
    /// The provider already holds the person's own approval for this action (for example the
    /// Mail Service's `email_ask_to_send` opens its own card), or the engineer switched approval
    /// off for this GitHub kind within the profile's ceiling (02:771).
    NotNeeded,
}
```

### Outcomes

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RulesOutcome {
    pub decision: ToolDecision,                   // 2C wire ToolDecision
    pub tool_request_id: ToolRequestId,
    /// Present when the verdict is `ask`: the approval the Approval Service holds open.
    pub approval_id: Option<ApprovalId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunStartFacts {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub crew_charter_id: Option<CrewCharterId>,
    pub command: Option<String>,
    /// The message, or the command's arguments; the engine cuts it to MAX_MATCH_CHARS for matching.
    pub text: String,
    /// Agent profile: `<connection>:<channel>`; None is the agent-wide scope.
    pub scope: Option<String>,
    /// Agent profile chat runs: the asker's chat user identifier.
    pub asker: Option<String>,
    /// "caller", "schedule", "intake", "chat" (agent profile); "person", "crew", "trigger" elsewhere.
    pub started_by: String,
    /// The started-by identifier ("<kind>:<id>" form when no asker).
    pub started_by_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunStartDecision {
    pub outcome: StartOutcome,
    /// A rule started another command: the caller expands it and replaces the first message.
    pub command: Option<String>,
    pub tool_subset: Option<Vec<String>>,
    pub single_call: bool,
    /// Lowered to a command's declared wall-clock bound when a rule started that command.
    pub limit_seconds: Option<u32>,
    pub actions_attached: Vec<String>,
    pub delivery: Option<DeliveryTo>,
    /// (control name, what it did), in order; copied into the run summary.
    pub controls_decided: Vec<(String, String)>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartOutcome {
    Proceed,
    /// A gate or rule ended the run before the model: state `completed` with this named ending.
    Ended { control: String, ending: String },
    /// A rule's stated reply ended the run.
    Replied { control: String, text: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EndDecision {
    End,
    /// Send the run back to the model with this fixed text.
    Continue { control: String, message: String },
    /// The ending was refused `pause_after` times: a person decides whether to set the gates aside.
    Ask { gates: Vec<String>, approval_id: ApprovalId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlLine {
    pub name: String, pub source: ControlSource, pub point: Point, pub command: Option<String>,
    pub enforced: bool, pub reason: String, pub detail: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource { Package, Configuration, Application, AgentPolicy }

/// What `parse_address` returns (wrightrun section E).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub scheme: String, pub host: String, pub port: Option<u16>, pub path: String,
    pub query: Option<String>, pub host_is_ip: bool, pub host_is_single_label: bool,
}
```

### The trait the engine owns, implemented by the program

```rust
#[derive(Debug, Clone)]
pub struct RunKey { pub run_id: RunId, pub agent_instance_id: AgentInstanceId, pub workspace_id: WorkspaceId }

#[derive(Debug, Clone)]
pub enum Proposal {
    ShellCommand { command: String, runs_in: String },
    ExternalTool { tool: String, kind: String, destination: String },
    PackageTool { server: String, tool: String },
    ApprovalListTool { tool: String },
    ArgumentRulePause { rule: String, tool: String },
    AgentPolicyApproval { verb: String, tool: String },
    EndGatesSetAside { gates: Vec<String> },
}
/// The words of one card, in the person's locale, from the message catalogue (never from a model).
#[derive(Debug, Clone)]
pub struct CardWording {
    pub question: String, pub verb: String, pub where_line: String,
    pub affects: Vec<AffectedItem>, pub changes: String,
    /// "See the command" for a shell command; None otherwise.
    pub content_label: Option<String>,
    pub proposal: ActionProposal,
}

#[derive(Debug, thiserror::Error)]
#[error("fact unavailable: {0}")]
pub struct FactError(pub String);

#[async_trait::async_trait]
pub trait RulesHost: Send + Sync + 'static {
    /// Work folder (agent profile: File Service namespace of the agent; elsewhere the agent folder).
    async fn path_exists(&self, run: &RunKey, path: &str) -> Result<bool, FactError>;
    async fn path_contains(&self, run: &RunKey, path: &str, text: &str) -> Result<bool, FactError>;
    async fn read_text(&self, run: &RunKey, path: &str) -> Result<Option<String>, FactError>;
    async fn append_line(&self, run: &RunKey, path: &str, line: &str) -> Result<(), FactError>;
    async fn remove(&self, run: &RunKey, path: &str) -> Result<(), FactError>;
    /// Runs queued, executing or paused with this command (Run Queue in the agent profile;
    /// Session Supervisor elsewhere). Includes the calling run.
    async fn runs_in_progress(&self, command: &str) -> Result<u32, FactError>;
    /// Whether a paused run other than `except` has one of `commands`.
    async fn paused_run_with_command(&self, commands: &[String], except: RunId) -> Result<bool, FactError>;
    /// Whether a run with this identifier exists and has ended; None when it does not exist.
    async fn run_ended(&self, run_id: &str) -> Result<Option<bool>, FactError>;
    /// Settings Store (agent profile): the value of a declared setting in a scope.
    async fn setting_value(&self, scope: Option<&str>, name: &str) -> Result<serde_json::Value, FactError>;
    /// Settings Store: the message rules kept as settings in a scope, with the setting's name.
    async fn setting_rules(&self, scope: &str) -> Result<Vec<(String, MessageRule)>, FactError>;
    /// Localisation: card words for a proposal.
    fn word(&self, run: &RunKey, proposal: &Proposal) -> CardWording;
    /// Localisation: the start-up report sentence for one control line (≤ 200 characters).
    fn plain_line(&self, line: &ControlLine) -> String;
}
```

### The engine

```rust
#[derive(Debug, Clone)]
pub struct RulesConfig {
    /// The MCP server names the MCP Front serves at start (SA-NAME-01..04): "ordin", "work", "sandbox",
    /// "memory", "code", "mail", "telegram", "slack", "knowledge", and each "craft_<app>" served then.
    /// The program passes the first list of the MCP Front's `server_names()`.
    pub ordin_servers: Vec<String>,
    /// The server-name prefixes the MCP Front claims (for example "craft_"), matched by prefix so a
    /// server listed under a claimed prefix after start is still ordin's. The program passes the
    /// second list of the MCP Front's `server_names()`.
    pub ordin_server_prefixes: Vec<String>,
    /// Absolute paths never granted (Internal Logic H.4), plus the application data root.
    pub protected_roots: Vec<std::path::PathBuf>,
    pub app_data_root: std::path::PathBuf,
    pub program: ProgramKind,                 // Desktop | PersonalServer | AgentSingle | AgentScaled
    /// This instance's holder name for locks (agent profile: the pod name; elsewhere "local").
    pub instance_name: String,
}

pub struct RulesEngine { /* private */ }

/// Consumers hold `Arc<ordin_rules::RulesEngine>` (the concrete type, as the Hook Relay and the MCP
/// Front do); a consumer that needs a test double defines its own port trait and the program
/// implements it over this type (SA-RECON-01). The crate defines no `RulesEngine` trait.
impl RulesEngine {
    pub const MODULE: &'static str = "rules";
    pub fn migrations() -> ModuleMigrations;   // module "rules" (ModuleMigrations.module)
    /// Called by: the desktop and server programs at start.
    pub fn new(datastore: Datastore, journal: Arc<Journal>, profile: Arc<OrganisationProfileService>,
               approvals: Arc<ApprovalService>, host: Arc<dyn RulesHost>, config: RulesConfig) -> Arc<Self>;

    /// Returns the start-up lines. Called by: Package Inspector (desktop, personal server) at start
    /// and after each install or update; Agent Loader (agent profile) once at start.
    pub fn load_rule_set(&self, set: InstanceRuleSet) -> Result<Vec<ControlLine>, RulesError>;
    /// Called by: Package Inspector when an instance is removed.
    pub fn unload_rule_set(&self, agent_instance_id: AgentInstanceId);
    /// Called by: File Service, at start and on every grant change (the one path from the
    /// Workspace Service's grants to enforcement, 02:722, 02:732).
    pub fn replace_grants(&self, workspace_id: WorkspaceId, grants: Vec<GrantedPlace>);
    /// Whether a folder may never become a place (H.4, K.6). Called by: File Service (adding a
    /// place), Workspace Service (adding a source link).
    pub fn is_protected_location(&self, path: &std::path::Path) -> bool;
    /// One path against the folder policy (H.2), outside a tool call (K.5). Allow or Deny only;
    /// never Ask. Called by: Craft Toolkit (conversion input and output files).
    pub fn check_path_access(&self, req: &PathAccessRequest) -> ToolDecision;
    /// Whether a reader may read a file of a granted place in a workspace (K.4). Never fails:
    /// in-memory grants only. Called by: Knowledge Service (page sources), Telegram Service (files
    /// attached to a draft).
    pub fn may_read(&self, check: &PlaceRead) -> bool;
    /// The profile's email rules over one email (K.7); the same function D.5.9 uses.
    /// Called by: Mail Service (at draft write and again before sending, SA-MAIL-11).
    pub fn check_email(&self, email: &EmailCheck<'_>) -> EmailVerdict;
    /// Per-project approval settings of the GitHub and GitLab operation kinds (K.3).
    /// Called by: Project Service, at start, on each profile change and on each engineer's change.
    pub fn replace_git_host_settings(&self, project_place: PlaceId, settings: Vec<GitHostSetting>);

    /// DecisionPoint::RunStart. Called by: Claude Local Adapter and Copilot Local Adapter, before
    /// they start the harness (every profile; the agent profile runs the Claude Local Adapter).
    /// They act on `outcome` (`Proceed`, `Ended`, `Replied`), `command`, `tool_subset`,
    /// `single_call` and `limit_seconds`; there is no ask at the start.
    pub async fn begin_run(&self, facts: RunStartFacts) -> Result<RunStartDecision, RulesError>;
    /// DecisionPoint::BeforeToolCall. Never fails: any internal error denies (fail closed).
    /// Called by: Hook Relay (harness tool calls, both harnesses), MCP Front (ordin's own tools).
    pub async fn decide_tool_call(&self, call: &ToolCall, ctx: &CallContext) -> RulesOutcome;
    /// A command ordin runs outside the harness's shell tool (K.1): a check the person or a run
    /// asks for. Never fails; an Ask returns its `approval_id`, and the caller waits with
    /// `wait_for_approval`. Called by: Project Service.
    pub async fn decide_command(&self, req: CommandRequest) -> RulesOutcome;
    /// Waits for the person's decision on an approval this engine opened (K.2). Called by: MCP Front,
    /// Project Service, Hook Relay.
    pub async fn wait_for_approval(&self, approval_id: ApprovalId, until: Option<tokio::time::Instant>)
        -> Result<ApprovalWait, RulesError>;
    /// Records that an allowed call ran or was refused by the harness (ToolRequest executed / refused).
    /// The one owner of the ToolRequest `executed`/`refused` states. Called by: Hook Relay
    /// (after-tool event), MCP Front (after its provider returned).
    pub async fn record_outcome(&self, tool_request_id: ToolRequestId, executed: bool);
    /// DecisionPoint::RunEnd. Called by: Hook Relay, on the harness's stop event (Claude Code by its
    /// hook; Copilot CLI through the relay's in-process path). No adapter calls it directly.
    pub async fn decide_run_end(&self, run_id: RunId) -> EndDecision;
    /// After an `EndDecision::Ask` is decided: approved sets the gates aside. Called by: Hook Relay.
    pub async fn settle_end_gates(&self, run_id: RunId, approved: bool) -> EndDecision;
    /// Restrictions the charter places on a member's run, held until the run's `begin_run` and kept
    /// in its facts. Called by: Crew Orchestrator, right after `start_run`.
    pub fn restrict_run(&self, run_id: RunId, restriction: RunRestriction);
    /// A paused run resumes (any instance): retakes a duty's lock. Some(gate) = refused, overlap.
    /// Called by: the program's `SupervisorHost::run_resuming`, which the Session Supervisor calls
    /// before `adapter.resume` (the Session Supervisor has no edge to this crate).
    pub async fn resume_run(&self, run_id: RunId) -> Result<Option<String>, RulesError>;
    /// The run ended: releases leases and drops its facts. Called by: the program's
    /// `SupervisorHost::run_ended`, which the Session Supervisor calls on every ended state.
    pub async fn end_run(&self, run_id: RunId);
    /// Start-up report lines for an instance, as 2C items. Called by: the program's
    /// `SupervisorHost::startup_items` and `CrewHost::startup_items`.
    pub fn startup_items(&self, agent_instance_id: AgentInstanceId) -> Vec<StartUpReportItem>;

    /// Agent profile: Caller API intake limit (wrightrun take_intake_place). Called by: Caller API.
    pub async fn take_intake_place(&self, intake: &str, run_id: &str, limit: u32) -> Result<bool, RulesError>;
    /// Agent profile: whether a lock is held now (G.3), for the operator's duty routes.
    /// Called by: Caller API (with `duty_lock_key`).
    pub async fn lock_held(&self, key: &str) -> Result<bool, RulesError>;
    /// Agent profile: erasure of one person (ADR-069): deletes their counters and digests.
    /// Called by: the program's `RetainedContent` for `ProvidedContent::ControlState`
    /// (`delete_for_asker`), on Retention and Erasure's behalf.
    pub async fn forget_asker(&self, connection: &str, asker: &str) -> Result<u64, RulesError>;
    /// The rows `forget_asker` would delete. Called by: the same `RetainedContent` (`count_for_asker`).
    pub async fn count_for_asker(&self, connection: &str, asker: &str) -> Result<u64, RulesError>;
    /// Counters and digests of one scope (`<connection>:<channel>` text; the program resolves the
    /// `ScopeId` through Retention and Erasure's `ScopeRegistry`). Called by: the same
    /// `RetainedContent` (`count_scope`, `delete_scope`).
    pub async fn count_scope(&self, scope: &str) -> Result<u64, RulesError>;
    pub async fn forget_scope(&self, scope: &str) -> Result<u64, RulesError>;
    /// Counters and digests recorded before `cutoff`. Called by: the same `RetainedContent`
    /// (`delete_older_than`); `limit` there is applied by the program per call of this method.
    pub async fn prune_before(&self, cutoff: Timestamp) -> Result<u64, RulesError>;
    /// Called by: the program's `SettingRuleCheck` for the Settings Store, before a rule-list setting
    /// is stored (wrightrun section D). The program maps `RULE_SETTING_RULE_NOT_ALLOWED` to the
    /// Settings Store's `RuleRefused { message, place: "<setting>/<rule>" }` and re-serialises the
    /// checked `MessageRule` as the normalised JSON.
    pub fn check_setting_rule(&self, agent_instance_id: AgentInstanceId, setting: &str,
                              allowed_outcomes: &[String], rule: &MessageRule) -> Result<(), RulesError>;
    /// wrightrun section E. Used inside the engine (argument rules); public for wrightrun parity,
    /// no other component calls it.
    pub fn parse_address(text: &str) -> Result<Address, RulesError>;
}

/// "duty:<duty>" — the lock key of a duty (G.3). Called by: Caller API.
pub fn duty_lock_key(duty: &str) -> String;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RunRestriction { pub external_actions_allowed: bool }

/// What `wait_for_approval` returns. `Pending` = `until` passed with no decision; the approval
/// stays open (it is never decided by timeout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalWait { Approved, Rejected, Expired, Pending }

/// A path checked outside a tool call (K.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PathAccessRequest {
    /// The workspace whose grants apply; None = the agent folder only.
    pub workspace_id: Option<WorkspaceId>,
    /// None = ordin's own action (no agent): only the protected roots apply, and ordin's
    /// application data root is allowed.
    pub agent: Option<AgentInstanceId>,
    /// The crew whose grants also apply, when the agent acts as a crew member.
    pub crew_charter_id: Option<CrewCharterId>,
    pub path: std::path::PathBuf,
    pub access: FolderAccess,
}

/// Who reads, for `may_read` (K.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaceReader {
    Agent { agent_instance_id: AgentInstanceId },
    CrewMember { crew_charter_id: CrewCharterId, agent_instance_id: AgentInstanceId },
    /// Ordin itself (ADR-027): it has no file tool, so this governs only what it is shown.
    Ordin,
    /// The hosted agent (agent profile): it has no granted places (ADR-098 item 3).
    HostedAgent,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlaceRead {
    pub workspace_id: WorkspaceId,
    pub reader: PlaceReader,
    pub place_id: PlaceId,
    /// Relative to the place's root, `/`-separated, 1–4,096 characters.
    pub path: String,
}

/// One email for the profile's rules (K.7). Addresses are plain `local@domain` text.
#[derive(Debug, Clone)]
pub struct EmailCheck<'a> {
    pub to: &'a [String], pub cc: &'a [String], pub bcc: &'a [String],
    pub attachments: &'a [EmailAttachment],
    pub drafted_by_agent: bool,
    pub body: &'a str,
    /// The localised foot line the Mail Service adds ("Drafted with an agent in ordin");
    /// empty skips the foot-line test (D.5.9 at draft time).
    pub foot_line: &'a str,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum EmailVerdict { Allowed, Refused { reasons: Vec<EmailViolation> } }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EmailViolation {
    EmailOff,
    RecipientOutsideCompany { address: String },
    TooManyRecipients { count: u32, max: u32 },
    AttachmentFromClosedPlace { place_id: PlaceId, path: String },
    FootLineMissing,
}

/// A command ordin runs itself (K.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CommandRequest {
    pub workspace_id: WorkspaceId,
    pub origin: CommandOrigin,
    /// 1–8,000 characters.
    pub command: String,
    /// Absolute.
    pub working_dir: std::path::PathBuf,
    /// The command is one the project declares as a check (Project Service `set_check_commands`).
    pub declared_check: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandOrigin { Person, Run { run_id: RunId, agent_instance_id: AgentInstanceId } }

/// One GitHub or GitLab operation kind's approval setting for a project (SA-GH-01 kinds: `read`,
/// `comment`, `open_pull_request`, `update_pull_request`, `request_review`, `push_branch`,
/// `create_branch`, `delete_branch`, `label`, `merge`, `release`, `close`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GitHostSetting { pub kind: String, pub approval_required: bool }

#[derive(Debug, thiserror::Error)]
pub enum RulesError {
    #[error("{code}: {message}")]
    Load { code: &'static str, message: String, r#where: String },
    #[error("fact unavailable")] FactUnavailable { control: String },
    #[error("control state unavailable")] State(#[source] sqlx::Error),
    #[error("rule set not loaded")] RuleSetNotLoaded,
    #[error("unknown run")] RunUnknown,
    #[error("not an address: {0}")] NotAnAddress(&'static str),
    #[error("setting rule not allowed: {0}")] SettingRuleNotAllowed(String),
    #[error("approval wait unavailable")] ApprovalUnavailable,
}
impl RulesError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

Not provided here, by decision (each recorded where the consumer is reconciled):
- `sandbox_capabilities` (Sandbox's `SandboxGrants`): the program implements `SandboxGrants` from the Package Inspector's declared connections, the organisation profile and Identity and Credentials (SA-RECON-02); `InstanceRuleSet` holds no connection or credential.
- An approvers list: approvers are the Approval Service's configuration (`ApprovalConfig.approvers`), passed by the program; the Agent Loader does not hand them to this engine.
- A `scope` column in `ControlLine`: the Agent Loader imports `ordin_rules::ControlLine` unchanged.
- The installed `tool_policy`: the Package Inspector maps the install record's resolved tool policy into `InstanceRuleSet.tool_policy` (`AgentToolPolicy`); this engine never reads Installers' records.

Reconciliation assumptions (ruling R5 and the January 2026 reconciliation):
- **SA-RECON-01** — Consumers use the concrete `Arc<ordin_rules::RulesEngine>`; where a consumer wants a seam for tests, it owns a narrow port trait that the program implements over this type. The names above are the contract; callers rename to them (R5).
- **SA-RECON-02** — `SandboxGrants::sandbox_capabilities` is implemented by the program, not by this engine, because the capabilities need the package's connections and their credentials, which `InstanceRuleSet` does not carry.
- **SA-RECON-03** — `may_read` takes a place and a path; a caller that holds a SharePoint or Confluence link (`origin`, `location`) resolves it to the working-copy place through the Workspace Service first (the Knowledge Service's `may_read_source` becomes that resolution plus `may_read`).
- **SA-RECON-04** — `PlaceReader::Ordin` may read a file of any place granted to any grantee in the workspace: Ordin coordinates every crew of the workspace (ADR-027), and it has no file tool, so the check governs only what it is shown.
- **SA-RECON-05** — The GitHub and GitLab command forms of K.3 are *chosen here*: SA-GH-01 names the kinds but not the commands that perform them.
- **SA-RECON-07** — R3: the Journal mints every `ToolRequestId` (`record_tool_request`); this engine updates the request through `decide_tool_request` and `finish_tool_request` and writes its other records with `append_entry`.
- **SA-RECON-08** — R9: the `mcp__<server>__` prefix is removed before matching (D.3); tool names follow wrightrun's pattern.
- **SA-RECON-09** — R20, D4: the engine's tables live in the Journal's `Datastore`, handed over by the program.
- **SA-RECON-06** — A command a person starts outside a run is allowed only when it is a declared check; any other command from the person is refused, because ordin runs no free-form command on the person's behalf outside the harness.
- **SA-CLOSE-01** (*chosen here*) — `Declaration` gains `approval_required` (YAML `approval-required`), the source of `InstanceRuleSet.approval_list` on the desktop and the personal server; `Declaration` states no authority (the Package Inspector derives it). Entries are matched as D.7.2 matches them; an entry naming a tool the agent never calls has no effect, so the load does not refuse it (a harness built-in tool such as `Bash` is a valid entry although it is not in `set.tools`).
- **SA-CLOSE-02** (*chosen here*) — `ToolDescription.arguments` holds places in this engine's form (`.`-separated names, `[]` for list items, a final `.*`); the Package Inspector converts the ToolBinding's JSON Pointers into it. The engine itself never reads a pointer.

### Example

Instance `supplier-analyst#1` (authority `document_only`) declares one argument rule:

```yaml
gates:
  argument-rules:
    - name: fetch-only-docs
      tool: mcp__supplier-db__fetch
      argument: url
      form: refuse-unless
      address: {schemes: [https], hosts: [example.org], with-subdomains: true, path-prefixes: [/api/public/]}
```

`decide_tool_call(ToolCall { tool: "mcp__supplier-db__fetch", origin: PackageMcpServer { server: "supplier-db" }, arguments: {"url": "https://example.org.evil.net/api/public/x"}, … }, CallContext::default())` returns:

```json
{"decision": {"decision": "deny", "reason": "Refused by the control fetch-only-docs: the argument url is not permitted.",
              "rule": {"source": "declaration_argument_rule", "rule_id": "fetch-only-docs",
                       "agent_instance_id": "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"},
              "decided_at": "2026-01-10T09:41:07.305Z"},
 "tool_request_id": "0192a1c3-2200-7a00-8b00-c00d00e00f01", "approval_id": null}
```

The same call with `https://api.example.org/api/public/rates` passes the rule, but the tool belongs to a package MCP server and is not in `package_read_only_tools`, so the engine asks: it builds the card through `RulesHost::word(Proposal::PackageTool {…})`, calls `ApprovalService::request`, and returns `{"decision": {"decision": "ask", "proposal": {…}, "rule": {"source": "app_approval_policy", "rule_id": "app.package_tool"}, …}, "approval_id": "0192a1c3-9a8b-…"}`.

## Internal Logic

**A. `load_rule_set(set)`** — pure apart from replacing the in-memory entry; no store or Journal access.
1. Names a control may use: every `set.tools[].name`; the built-in file tools of *H.1*; `task`; and `task:<s>` for each sub-agent `s` in `{"general-purpose"} ∪ {set.reviewer}`.
2. For each control of the declaration and then of `set.configured`, in declared order, with `where` = its key (`wrightrun-declaration.yaml: gates.steps[2]`, `controls.argument-rules[0]`):
   - a package before-tool step gate or argument rule whose `tool` is in `set.unmapped_operations` is skipped and listed with reason "the operation <tool> is not mapped, so no call of it can be made";
   - a step gate's `tool`, a `called`/`not-called` name, a `tools` outcome name or an argument rule's `tool` that is not a usable name → `RULE_UNBOUND_TOOL`; a judged loop's `reviewer` that is not a sub-agent → `RULE_UNBOUND_TOOL`;
   - an argument rule's `argument` that is neither a place of the tool's description nor under a place ending `.*` → `RULE_UNKNOWN_ARGUMENT`;
   - each `approval_list` entry is trimmed; an empty entry or one longer than 200 characters → `RULE_POLICY_INVALID` with `where` = `approval-required[<i>]`; other entries are kept as written (SA-CLOSE-01);
   - every pattern compiled with `regex::RegexBuilder::new(p).size_limit(1 << 20).build()`; empty, longer than `MAX_PATTERN_CHARS`, or refused by the crate → `RULE_BAD_PATTERN`; more than `MAX_ARGUMENT_RULES` argument rules → `RULE_TOO_MANY_RULES`.
3. A configured control whose name equals a package control name (including the generated names `duty:<d>:stop`, `duty:<d>:escalation`, `duty:<d>:work-check`, `duty:<d>:lock`, `duty:<d>:cycle-limit`, `duty:<d>:done-check`, `duty:<d>:heartbeat`, `duty:<d>:failed-cycles`, `judged-loop:<c>`, `bound:<c>`, `bound:<c>:rounds`, `bound:<c>:repeated-failures`) → `RULE_GATE_OVERRIDE`.
4. Agent tool policy: each verb `^[a-z0-9_-]+(\.[a-z0-9_-]+)?$`, each flag `^--?[A-Za-z0-9][A-Za-z0-9-]*$`; otherwise `RULE_POLICY_INVALID`. The policy is stored as deny and ask lists only, so it cannot widen; a `requires_approval` entry also listed in `prohibited_verbs` is kept in the deny list only.
5. Duty of each command: each `DutyGate.command → duty`, then each configured schedule entry `(duty, command)` whose duty exists and whose command is not mapped yet.
6. Build `ControlSet { agent_instance_id, authority, folder, read_only, message_rules (package then configuration, with compiled patterns), duties, duty_of_command, judged_loops (by command), bounds, steps, argument_rules (by tool name, package first), approval_list, package_read_only_tools, policy, reviewer, skipped }`; replace the map entry atomically (`ArcSwap`-style replacement of an `Arc<ControlSet>`; a decision in flight keeps the set it started with).
7. Build and return the control lines (section F). A load error leaves the previous set in place; the Package Inspector marks the instance unavailable with the code.

**B. `begin_run(facts)`** — DecisionPoint::RunStart (wrightrun section F).
1. Find the instance's `ControlSet`; absent → `RULE_RULESET_NOT_LOADED` (the caller does not start the harness). Create `RunFacts { key, commands: [command] or [], text_cut, scope, asker, crew_charter_id, subset: None, single_call: false, set_aside: [], leases: [], tools_called: {}, tool_calls: [], changed_work_folder: false, ended: false, started_at }` and persist it (section G.1).
2. **Duty gates**, when the command maps to a duty `d` (commands of `d` = every command mapped to it), in order; the first unmet ends the run:
   1. `duty:<d>:stop` unmet when `host.path_exists(d.folder + "STOP")` → ending `stop-present`;
   2. `duty:<d>:escalation` unmet when `d.folder + "escalation.md"` exists → `escalation-present`;
   3. `duty:<d>:work-check` (kind `file` only) unmet when its condition does not hold → `nothing-to-do`;
   4. `duty:<d>:lock`: `take_lock("duty:<d>", run_id)` (section G.3); false → `overlap`. True: add the key to `leases`; sum `host.runs_in_progress(c)` over the duty's commands; a sum above 1 is unmet only when `host.paused_run_with_command(commands, run_id)` is true → `overlap`;
   5. `duty:<d>:cycle-limit` (with `max_cycles`): `count_start(Command, "duty:<d>", period_seconds)`; above `max_cycles` → `cycle-limit`.
3. A duty gate unmet: when `d.heartbeat` lists the ending, compute the entry (`{time}` `YYYY-MM-DDTHH:MM:SSZ`, `{ending}`, `{cycle}` = 1 + the non-blank lines of `heartbeat.md`, 1 when absent) and `host.append_line`; release leases; record `controls_decided (gate, "ended:<ending>")`; return `Ended`. Log `rules_start_ended` {control, ending, command}.
4. Every duty gate met and `d.folder + "lock"` exists: `host.remove` it; log `rules_stale_lock_removed` {duty}.
5. **Message rules, first round.** Rules with `rule.command == facts.command` from the set and, when a scope exists, from `host.setting_rules(scope)` (source `setting`; a rule whose patterns do not compile or whose command or action is unknown is ignored and logged `rules_rule_ignored` {rule, reason}). A rule matches when every condition holds, evaluated in order and stopping at the first that does not (section C). 
6. **The deciding rule**: among matching rules with `decides`, highest `priority`, then source order package, configuration, setting, then name; the first decides.
7. **Additive outcomes** from every matching rule in the same order: `attach-action` appended once; the first `delivery` kept. A named ending empties `actions_attached`.
8. **Apply**: `End` → `Ended { control, ending }`; `Reply` → `Replied`; `Command` → set `command`, add it to `commands`, lower `limit_seconds` to the command's `bound` wall clock when lower, then repeat steps 2–7 once for the new command (a second `command` outcome is ignored, logged with reason `second-command`); `Tools` → `tool_subset`; `SingleCall` → `command` and `single_call = true`. Each appends `(control, what)` to `controls_decided`, where a setting rule is named `setting/<rule>`.
9. Persist `RunFacts`; `journal.append_entry` kind `rules_run_start` {run_id, outcome kind, controls_decided}; return.
10. A `FactError` or a store error while a fact is read: release leases taken so far and return `RULE_FACT_UNAVAILABLE` naming the control. A start is never let through on a fact that could not be read.

**C. Condition evaluation** (wrightrun section I). `path-exists`/`path-absent`/`path-contains`: a bundle path (normalised path starting with a `BUNDLE_DIR_PREFIXES` entry or equal to a `BUNDLE_FILES` entry) is read from `set.folder` on disk; any other path through `host`. `run-in-progress`: `host.runs_in_progress(command) > 1`; never holds without a command. `start-count`: `count_start(of, key, period)` > `more_than`, with key = command, scope text (`agent-wide` when none) or `<connection>/<asker>`; does not hold when the fact is absent. `repeated-message`: `seen_message(scope_text, asker_key, digest, period)`, digest = SHA-256 hex of the text trimmed, inner white space collapsed to one space and lower-cased with Unicode case folding. `keywords`/`keyword-share`: both sides split into words (Unicode word boundaries, lower-cased); a keyword is found when all its words are in the text's set. `pattern`: `Regex::is_match` on the cut text. `setting`: `host.setting_value(scope, name) == equals` (a selection holds when `equals` is one of the selected values). `called`/`not-called`: counts from `tools_called` (`task:<name>` for a sub-agent; the bare `task` sums every `task:*`). `elapsed`: seconds since `started_at` of the current stretch. `changed-work-folder`: `changed_work_folder`. Each counter or digest fact is asked once per start and remembered.

**D. `decide_tool_call(call, ctx)`** — DecisionPoint::BeforeToolCall. Deny wins over ask, ask over allow; within a level the first rule in the order below is the one named.
1. Compute `digest` = SHA-256 hex of the canonical JSON of `call.arguments` (keys sorted, no white space). `journal.record_tool_request(call.run_id, &call.tool, digest)` returns the `ToolRequest` (state `requested`) and its `tool_request_id` (failure: deny `RULE_JOURNAL_UNAVAILABLE`, source `fail_closed`; a decision that cannot be journalled is not made).
2. **Input limits**: tool name length, argument size, origin server form (2C) → deny, source `fail_closed`, rule ids `app.input.tool_name`, `app.input.arguments_size`, `app.input.origin`.
3. **Origin**: a `PackageMcpServer { server }` whose server is in `config.ordin_servers`, or starts with one of `config.ordin_server_prefixes`, is treated as `OrdinMcpFront`. **Name** (R9): a tool named `mcp__<server>__<tool>` is matched against controls, lists and policies by `<tool>` alone; the full name is kept in the `ToolRequest`. Names follow `^[A-Za-z][A-Za-z0-9_-]{0,63}$` after the prefix is removed (ADR-098); another name denies with `RULE_TOOL_NAME_INVALID`.
4. **Run**: load `RunFacts` (memory, else store); absent → deny `RULE_RUN_UNKNOWN` (`fail_closed`, `app.run_unknown`). Its `agent_instance_id` or `workspace_id` different from the call's → deny `RULE_SESSION_MISMATCH`. Instance set absent → deny `RULE_RULESET_NOT_LOADED`. `ended` or `single_call` → deny with the deciding rule's name, reason "the tool is not offered in this run".
5. **Deny checks**, in order:
   1. *Server switches* (`app.server.external_off`, source `app_approval_policy`): `ctx.class` is `External { kind }` and `kind` is in the profile's `server.external_actions_off[]` (personal server only) → deny "This kind of action is switched off on this server.".
   1b. *Server Slack switch* (`app.server.slack_off`, source `app_approval_policy`; ADR-102): on the personal server, when `"slack_post"` is in the profile's `server.external_actions_off[]` and the tool, after the `mcp__slack__` prefix is removed, is `slack_draft_post` or `slack_ask_to_post` with origin `OrdinMcpFront { server: "slack" }` → deny "Slack posts are switched off on this server.". `slack_search` and `slack_read` are not denied by this check (reading posts nothing).
   1a. *Crew restriction* (`app.crew.external_not_allowed`, `app_approval_policy`): the run's `RunRestriction` says `external_actions_allowed = false` and the call is classified `External`, its tool is in `set.external`, or its origin is `PackageMcpServer` → deny "This crew member may not act outside ordin.".
   2. *Folder policy* (`app.folder_grant`, `app_folder_policy`): for a built-in file tool (H.1), every path argument must pass H.2; otherwise deny "the path is outside the places this agent may use".
   3. *Shell policy* (`app.shell`, `app_shell_policy`): a shell tool (H.3) when `authority ≠ system_changing` → deny "this agent may not run commands".
   4. *Agent tool policy* (`agent_tool_policy`, rule id = the verb or flag): for a shell tool or a `gh` operation tool, split the command into segments at `&&`, `||`, `;`, `|` and newlines (outside quotes), tokenise each segment (white space outside single or double quotes; a backslash escapes the next character), take `verb = basename(token0) + "." + first later token not starting with "-"` (or `basename(token0)` alone); deny when a prohibited verb equals the verb or equals its program part, or when any token equals a forbidden flag or starts with `<flag>=`. For other tools, deny when a prohibited verb equals the tool name.
   5. *Tool subset* (rule = deciding rule): `subset` set, tool is a bound tool and not in it → deny "the tool is not offered in this run".
   6. *Reviewer* (`app.reviewer_read_only`, `declaration_tool_rule`), agent profile: `ctx.subagent == reviewer` and the tool is not one of `Read`, `Grep`, `Glob`, `LS` → deny. Conditional on C-78: when the hook input does not name the sub-agent (`ctx.subagent` always `None`), this check never applies and the start-up report lists "reviewer limited to reading" as not enforced (`route_cannot_carry`).
   7. *Step gates* (before-tool, package first): applies when its command is in `commands` (or both none) and its tool is none, the tool, or the call's key (`task:<subagent_type>` for `task`); the first applying gate whose conditions all hold denies "Refused by the control <g>: the declared condition holds.".
   8. *Argument rules* with outcome `refuse` (wrightrun section G.4): walk the place (`.`-separated names; `[]` iterates a list); a leaf is text when a string, or the JSON text of a number or bool; an unreadable place or leaf, or an address the parser refuses, holds against the call. `refuse-when` holds when absent with `present: false`, or the test holds for one leaf; `refuse-unless` holds when absent (unless `present: false`, then when present) or the test fails for one leaf; an empty list passes. The test of a leaf: every stated condition of `equals_any`, `patterns` (search), `max_length`, `address` (scheme in list; host equal, or with subdomains ending in `.host`; IP and single-label flags; port; path equal to a prefix or starting with it, a prefix not ending in `/` followed by `/`). The first holding rule denies "Refused by the control <r>: the argument <place> is not permitted.".
   9. *Email profile rules* (`app.email.*`, `app_approval_policy`): for `ctx.class = External { kind: "email_send" }` and for the mail tools `email_draft` and `email_ask_to_send` (server `mail`), read `to`, `cc`, `bcc` (arrays of `{address}`) and `attachments` (array of `{place_id, relative_path}`) and run K.7 with `drafted_by_agent = true`, `body = ""` and `foot_line = ""` (the foot line is added and tested by the Mail Service before sending). The first violation names the rule: `EmailOff` → `app.email.off`; `TooManyRecipients` → `app.email.max_recipients`; `RecipientOutsideCompany` → `app.email.internal_only`; `AttachmentFromClosedPlace` → `app.email.no_attach`. Unreadable recipient or attachment arrays deny with `app.email.max_recipients` (fail closed).
6. **Approved once** (C-80, ADR-098 item 3): when a `rules_approved_calls` row matches `(run_id, tool, digest)` with `used = 0`: read its approval (`approvals.get`): `approved` → mark used, allow (rule `app.approved_once`); `pending` → ask, returning that same `approval_id` (no second card); `rejected` → deny "the person rejected this action"; `expired` → delete the row and continue.
7. **Ask checks**, in order (the first that holds asks):
   1. *Argument rules* with outcome `pause` that hold (`declaration_argument_rule`, rule id = the rule).
   2. *Approval list* (`declaration_tool_rule`, `approval-list`): the tool (or `task` for any `task:*`) is in `approval_list`.
   3. *Agent policy approval* (`agent_tool_policy`): the verb (D.5.4) or tool name is in `requires_approval`.
   4. *Shell* (`app.shell`): a shell tool for a `system_changing` instance, unless a package argument rule on that tool with form `refuse-unless` and outcome `refuse` passed for this call (the package states which commands it runs without approval, A-06; assumed here, SA-RULE-02), or K.3 classifies every segment of the command as a GitHub or GitLab kind whose project setting has `approval_required = false` (then allow, rule `app.git_host_setting`, source `app_approval_policy`).
   5. *External action* (`app.external_action`): `ctx.class = External { approval: Always }`, or the tool is in `set.external` (agent profile; the approval carries `leaves_company = true`).
   6. *Package tool* (`app.package_tool`): origin `PackageMcpServer` and the tool not in `package_read_only_tools`.
8. **Ask**: `wording = host.word(&key, &proposal)`; build `ApprovalRequest { workspace_id, asked_by: Run { run_id } (or the crew round from the Crew Orchestrator's own requests), question, verb, where_line, affects, changes, content: Command { label, command, runs_in } for a shell command else None, reject_style: Reject, show_details: content present, proposal, asker: facts.asker, leaves_company: from the class }`; `approvals.request(req)`. Insert `rules_approved_calls (run_id, tool, digest, approval_id, used=0)`. An Approval Service error denies with `RULE_APPROVAL_UNAVAILABLE` (`fail_closed`): an ask that cannot be shown to a person is never allowed.
9. **Allow** otherwise (rule `app.default_allow`, source `app_folder_policy` for file tools, `app_approval_policy` for the rest).
10. Every outcome: `journal.decide_tool_request(tool_request_id, ToolRequestDecision, "<source>:<rule_id>", approval_id)` (state `decided`), `deciding_rule = "<source>:<rule_id>"` and the approval; for an allowed call add 1 to `tools_called[key]`, append the tool to `tool_calls`, and set `changed_work_folder` when `ctx.class` is `LocalWrite` or the tool is a built-in write tool; persist `RunFacts`. Log `rules_decided` {run_id, tool, decision, source, rule_id}; never the arguments.
11. Any panic-free internal error not covered above (a poisoned lock, a store error) denies with source `fail_closed`, rule id `app.internal`, and code `RULE_STATE_UNAVAILABLE` in the log.

**D2. `record_outcome(tool_request_id, executed)`.** `journal.finish_tool_request(id, executed)` (state `executed` or `refused`); a Journal error is logged `rules_outcome_unrecorded` {tool_request_id, error_code} and not returned (the call has already run or been refused).

**E. `decide_run_end(run_id)` and `settle_end_gates`** (wrightrun section K).
1. Load `RunFacts`; unknown → `End` (the engine has nothing to hold; logged `rules_run_unknown`). `ended` or `single_call` → release leases, `End`.
2. Unmet end gates, in order: for each command with a judged loop, `judged-loop:<c>` is unmet when `changed_work_folder` and `tools_called["task:<reviewer>"] == 0`; then each `end` step gate that applies and whose conditions hold. Remove gates in `set_aside`.
3. None → release leases, `End`.
4. First gate `g`, `n = refusals[g]`, `limit = pause_after`. `n < limit` → `refusals[g] = n + 1`, persist, return `Continue { control: g, message }` with the fixed text "The run cannot end yet. The control <g> requires a call to the sub-agent <reviewer> before the run ends." (judged loop) or "The run cannot end yet. The control <g> is not met." (step gate).
5. `n ≥ limit` → open an approval with `Proposal::EndGatesSetAside { gates }` (approve = set the gates aside and let the run end; reject = the run goes on) and return `Ask { gates, approval_id }`. The Hook Relay waits on it and calls `settle_end_gates(run_id, approved)`: approved → add every unmet gate to `set_aside`, release leases, `End`; rejected → reset `refusals[first remaining gate] = 0`, return `Continue`.

**F. Control lines and start-up items.** Lines in order: each duty's lines, judged loops, bounds, package step gates, package argument rules, skipped controls, package message rules, then configured step gates, argument rules and message rules (wrightrun section B table: `work-check` of kind `command` and `done-check` are not enforced, "no shell" or "a done-check has no enforcement point in this release"; `failed-cycles`, `bound:<c>:rounds` and `bound:<c>:repeated-failures` are not enforced, "the runtime does not observe the result of a round"). Then the application lines, all enforced: `app.folder_grant`, `app.shell` (or "this agent may not run commands" for other authorities), `app.external_action`, `app.package_tool`, and the agent policy's verbs and flags (source `AgentPolicy`). `startup_items` maps an enforced line to `EnforcedRule { rule: RuleRef { source, rule_id: name, agent_instance_id }, plain: host.plain_line(line) }` and a not-enforced one to `RuleNotEnforced { reason: route_cannot_carry }`; the reviewer line under C-78's fallback uses `route_cannot_carry` too.

**G. Store-backed state** (wrightrun section L, statements in *Data Model*).
1. `RunFacts` persistence: upsert into `rules_run_facts` after every change; memory keeps the record for `RUN_FACTS_TTL_SECONDS`; `end_run` deletes the row and the entry.
2. `count_start`: fixed window `floor(now / period) * period`; upsert adding one, returning the window's count; when it returns 1, delete earlier windows of the same key.
3. Locks: `take_lock(key, holder)` succeeds when no row exists, the row expired, or the holder already holds it; `renew_lock`; `release_lock`. While a run holds leases, a task renews them every `LEASE_RENEW_SECONDS`; a failed renewal tries one take, then logs `rules_lease_lost` {key} and the run goes on. Leases are released at `Ended`, `End` and `end_run`; a paused run stops renewing and its lock lapses within `LEASE_SECONDS`.
4. `resume_run(run_id)`: when the run's first command maps to a duty, `take_lock("duty:<d>", run_id)`; false → log `rules_resume_refused` {control, ending: "overlap"} and return `Some("duty:<d>:lock")`. Reset `started_at` for `elapsed`.
5. `take_intake_place(intake, run_id, limit)`: ensure rows 1..limit exist; select candidate slots in slot order where `run_id IS NULL`, or `host.run_ended(run_id) == Some(true)`, or `host.run_ended(run_id) == None` and `taken_at` older than `PLACE_NO_RUN_SECONDS`; take the first with a compare-and-set `UPDATE … WHERE intake = ? AND slot = ? AND run_id IS ? AND taken_at IS ?`; zero rows → next candidate; none left → false.
6. `forget_asker`: delete `rules_control_starts` rows of kind `asker` with key `<connection>/<asker>` and `rules_control_messages` rows of that asker in scopes starting `<connection>:`; return the count.
7. Any statement error → `RULE_STATE_UNAVAILABLE` (retryable); inside a decision it denies (D.11) or fails the start (B.10).

**H. Application policy tables** (*assumed here*, SA-RULE-01: names of the harnesses' built-in tools as their documentation gives them at the pinned versions; confirmed by the conformance cases of C-26 and C-27).
1. Built-in file tools and their path arguments: Claude Code `Read(file_path)`, `Write(file_path)`, `Edit(file_path)`, `MultiEdit(file_path)`, `NotebookEdit(notebook_path)`, `Glob(path)`, `Grep(path)`, `LS(path)`; Copilot CLI `view(path)`, `create(path)`, `edit(path)`. Write tools: `Write`, `Edit`, `MultiEdit`, `NotebookEdit`, `create`, `edit`. A missing `path` argument of `Glob`, `Grep` or `LS` means the session's working folder, the agent folder.
2. A path passes when, after making it absolute against the agent folder and resolving `.` and `..` lexically (no symbolic link is followed by the engine; the File Service refuses links that leave a place for ordin's own file tools, and the Agent Folder Manager canonicalises each granted root at session start so a root that leads into ordin's data is never applied, 02:479, AgentFolderManager.md SA-CLOSE-01): it is not under any `protected_roots` entry, nor under `app_data_root` except inside the instance's own agent folder; and it is inside the agent folder (reading always; writing unless `agent_folder_read_only`), or inside a `GrantedPlace` of the run's workspace whose grantee is the instance or the run's crew charter, with `read` allowing reading tools and `read_and_change` allowing both. Comparison is by path components, case-insensitive on macOS and Windows, case-sensitive on Linux.
3. Shell tools: `Bash` (Claude Code), `bash` and `powershell` (Copilot CLI); the command argument is `command`.
4. Protected roots the program passes (*assumed here*, SA-RULE-04): macOS `/System`, `/Library`, `/usr`, `/bin`, `/sbin`, `/etc`, `/private`, `/Applications`, and in the home folder `Library/Keychains`, `.ssh`, `.gnupg`, `.aws`, `.config/gh`, `.docker`; Linux `/etc`, `/usr`, `/bin`, `/sbin`, `/boot`, `/proc`, `/sys`, `/var`, and the same home entries plus `.local/share/keyrings`; Windows `C:\Windows`, `C:\Program Files`, `C:\Program Files (x86)`, `C:\ProgramData`, and `%APPDATA%\Microsoft`.

**I. `parse_address(text)`** (wrightrun section E, in Rust): refuse empty, longer than 4,000 characters, or holding a space, backslash, control character or U+007F; match `^([A-Za-z][A-Za-z0-9+.-]*)://([^/?#]*)([^?#]*)(?:\?([^#]*))?(?:#.*)?$`; refuse an authority with `@` or `%`; bracketed IPv6 parsed with `std::net::Ipv6Addr`; port 1–65535 when present; host lower-cased, one trailing full stop removed, non-ASCII hosts encoded with `idna` (*assumed*: the `url` crate's IDNA, already a dependency through `reqwest`); IPv4 by `std::net::Ipv4Addr`; a last label of digits or starting `0x` refused; labels 1–63 of `a-z0-9-`, host ≤ 253; default ports 80 (`http`, `ws`) and 443 (`https`, `wss`); path percent-decoded once as UTF-8 (refusing control characters and backslash), dot segments resolved per RFC 3986 §5.2.4, empty path `/`. Each refusal is `RULE_NOT_AN_ADDRESS` with a reason that never repeats the text.

**J. `check_setting_rule`** (wrightrun section D): name is a slug ≤ 64 and not a declared control name; condition kinds only `keywords`, `keyword-share`, `pattern`, `setting`; outcome kinds in `allowed_outcomes` and never `end` or `tools`; bounds as the vocabulary states; commands and actions exist. Any failure → `RULE_SETTING_RULE_NOT_ALLOWED` with `<setting>/<rule>`.

**K. Calls outside the three decision points** (added in the January 2026 reconciliation, R5).

1. **`decide_command(req)`.** Never fails; every outcome is a `RulesOutcome`.
   1. `command` 1–8,000 characters and `working_dir` absolute, else deny `app.input.command` (`fail_closed`). `working_dir` under a protected root (K.6) → deny `app.folder_grant`.
   2. `Run` origin: `journal.record_tool_request(run_id, "command", digest)` mints the `tool_request_id`, with and the digest of the canonical JSON `{"command","working_dir"}` (D.1 rules apply, including the deny on a journal failure). `Person` origin: no run exists, so no `ToolRequest` (the returned `tool_request_id` is a fresh UUIDv7 with no row); the decision is journalled through `append_entry` as kind `rules_command_decided` {workspace_id, origin: "person", decision, rule}.
   3. `Person` origin: `declared_check` → allow `app.shell.person_check`; otherwise deny `app.shell.person_undeclared` with reason "Only the project's declared checks run from here." (SA-RECON-06).
   4. `Run` origin: load `RunFacts` (absent → deny `RULE_RUN_UNKNOWN`); its `agent_instance_id` or `workspace_id` different from the request's → deny `RULE_SESSION_MISMATCH`; then D.5.3 (authority), D.5.4 (agent tool policy over `command`), and D.6 (approved once, with tool `command`). Then `declared_check` → allow `app.shell.declared_check`; else K.3 with `working_dir` gives a kind whose setting has `approval_required = false` → allow `app.git_host_setting`; else D.7.3 (agent policy approval) or ask `app.shell` with `Proposal::ShellCommand { command, runs_in: working_dir }` through D.8.
   5. Log `rules_command_decided` {origin, decision, rule_id}; never the command text.
2. **`wait_for_approval(approval_id, until)`.** `approvals.wait(approval_id, until)`: `Decided(Approved)` → mark the `rules_approved_calls` row of this approval used (the waiter lets the call run now, so the approved-once record is spent) and return `Approved`; `Decided(Rejected)` → `Rejected`; `Decided(Expired)` → `Expired`; `DeadlinePassed` → `Pending` (the row stays unused, so a resumed session that repeats the call passes D.6 once). An Approval Service error → `RULE_APPROVAL_UNAVAILABLE` (retryable); the caller denies.
3. **GitHub and GitLab kinds** (*chosen here*, SA-RECON-05). `replace_git_host_settings(place, settings)` replaces the in-memory map for that place (no store: the Project Service sends the full list at start). Classification of one command segment, tokenised as D.5.4: program `gh` or `glab` (basename) with sub-command pair — `pr view|list|status|diff|checks`, `issue view|list|status`, `repo view`, `release view|list`, `run view|list`, `mr view|list|diff` → `read`; `pr create`, `mr create` → `open_pull_request`; `pr edit` or `mr update` with `--add-reviewer`, `--reviewer` → `request_review`, with `--add-label`, `--label` → `label`, otherwise `update_pull_request`; `pr comment`, `issue comment`, `pr review`, `mr note`, `issue note` → `comment`; `pr merge`, `mr merge` → `merge`; `pr close`, `issue close`, `mr close` → `close`; `release create` → `release`; `label create|edit` → `label`; program `git` with `push` → `delete_branch` when a later token is `--delete` or `-d` or starts with `:`, else `push_branch`. Any other form → no kind. The project: for `decide_command`, the place among all `replace_grants` entries whose `root` contains `working_dir` and that has settings; for a shell tool call, the one granted place of the run's workspace (grantee the instance or its crew) that has settings — none or more than one → no project. No kind or no project → no setting applies and the call asks as before. A kind of `read` never asks.
4. **`may_read(check)`.** `path` empty, longer than 4,096, absolute, holding a `\0`, or with a `..` segment after lexical normalisation → false. `HostedAgent` → false. Grants of `workspace_id` with `place_id`: `Agent { id }` needs a grant whose grantee is `AgentInstance { id }`; `CrewMember { charter, id }` needs grantee `AgentInstance { id }` or `Crew { charter }`; `Ordin` needs any grant of the place in the workspace (SA-RECON-04). Either access (`read`, `read_and_change`) allows reading. Then `root.join(path)` under a protected root (K.6) → false. Otherwise true. Log `rules_read_refused` {workspace_id, place_id, reader_kind} at debug on false; never the path.
5. **`check_path_access(req)`.** `path` not absolute → deny. `agent = None`: deny when the path is under a protected root (the application data root is allowed: ordin's own files), else allow; rule `app.folder_grant`, source `app_folder_policy`. `agent = Some(id)`: the instance's set must be loaded (else deny `RULE_RULESET_NOT_LOADED`, `fail_closed`); H.2 with the instance's agent folder and, when `workspace_id` is present, its grants to the instance or to `crew_charter_id`; `access = ReadAndChange` needs a writable target. Returns a `ToolDecision` with `decided_at` now; never asks; not journalled (no tool request exists); log `rules_path_checked` {agent?, decision} at debug.
6. **`is_protected_location(path)`.** Make the path absolute and lexically normalised (relative → true, refused); true when it equals or lies under an entry of `protected_roots` or under `app_data_root`, or when it is a file-system root (`/`, a drive root such as `C:\`), since every protected root lies below it; comparison as H.2. Otherwise false. A folder that contains a protected root (the home folder) is not itself protected: each path inside it is checked by H.2 at use.
7. **`check_email(email)`.** Collect every violation, in this order: `email.enabled` false → `EmailOff` (and stop: nothing else is checked); `to + cc + bcc` count above `email.max_recipients` → `TooManyRecipients { count, max }`; with `email.internal_only`, each address whose domain (the text after the last `@`, lower-cased, one trailing full stop removed) is not in `email.company_domains` → `RecipientOutsideCompany { address }`; each attachment whose place matches `email.no_attach_places` → `AttachmentFromClosedPlace { place_id, path }`; `email.foot_line_required`, `drafted_by_agent`, `foot_line` not empty and `body` not containing `foot_line` → `FootLineMissing`. No violation → `Allowed`. Pure: reads the current profile snapshot only.
8. **`lock_held(key)`.** `SELECT 1 FROM rules_control_locks WHERE key = ? AND expires_at > ?now`; a row → true. Statement error → `RULE_STATE_UNAVAILABLE`.
9. **Retained content.** `count_for_asker` counts the rows `forget_asker` (G.6) deletes. `count_scope(scope)` counts `rules_control_starts` rows of kind `scope` with key `scope` plus `rules_control_messages` rows with `scope = scope`; `forget_scope` deletes them and returns the count. `prune_before(cutoff)` deletes `rules_control_messages` rows with `seen_at < cutoff` and `rules_control_starts` rows whose window ended before `cutoff` (`window_start + period_seconds < cutoff`); a counter window still open is kept. Statement errors → `RULE_STATE_UNAVAILABLE`.

## Data Model

Module `rules`. SQLite in every program; PostgreSQL in the scaled shape. Forward-only, additive.

```sql
-- SQLite, migration 1
CREATE TABLE rules_control_starts (
    kind TEXT NOT NULL CHECK (kind IN ('command', 'scope', 'asker')),
    key TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 512),
    period_seconds INTEGER NOT NULL CHECK (period_seconds BETWEEN 1 AND 2592000),
    window_start INTEGER NOT NULL,                 -- Unix seconds
    starts INTEGER NOT NULL CHECK (starts >= 1),
    PRIMARY KEY (kind, key, period_seconds, window_start)
);
CREATE TABLE rules_control_messages (
    scope TEXT NOT NULL CHECK (length(scope) BETWEEN 1 AND 512),
    asker TEXT NOT NULL CHECK (length(asker) BETWEEN 1 AND 512),
    digest TEXT NOT NULL CHECK (length(digest) = 64),
    seen_at INTEGER NOT NULL,                      -- Unix milliseconds
    repeated INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (scope, asker, digest)
);
CREATE INDEX rules_control_messages_seen ON rules_control_messages (scope, asker, seen_at);
CREATE TABLE rules_control_locks (
    key TEXT PRIMARY KEY CHECK (length(key) BETWEEN 1 AND 512),
    holder TEXT NOT NULL,
    expires_at INTEGER NOT NULL                    -- Unix milliseconds
);
CREATE TABLE rules_intake_places (
    intake TEXT NOT NULL, slot INTEGER NOT NULL CHECK (slot >= 1),
    run_id TEXT, taken_at INTEGER,
    PRIMARY KEY (intake, slot)
);
CREATE TABLE rules_run_facts (
    run_id TEXT PRIMARY KEY CHECK (length(run_id) = 36),
    agent_instance_id TEXT NOT NULL,
    facts TEXT NOT NULL,                           -- JSON RunFacts
    updated_at INTEGER NOT NULL
);
CREATE TABLE rules_approved_calls (
    run_id TEXT NOT NULL, tool TEXT NOT NULL, arguments_digest TEXT NOT NULL CHECK (length(arguments_digest) = 64),
    approval_id TEXT NOT NULL, used INTEGER NOT NULL DEFAULT 0 CHECK (used IN (0, 1)),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, tool, arguments_digest, approval_id)
);
CREATE INDEX rules_approved_calls_open ON rules_approved_calls (run_id, tool, arguments_digest) WHERE used = 0;
```

Key statements (SQLite; the PostgreSQL forms are wrightrun's, `agent-wright-runtime/docs/spec/components/ControlEvaluator.md` *Data Model*, with the `rules_` table names and `clock_timestamp()`):

```sql
-- count_start
INSERT INTO rules_control_starts (kind, key, period_seconds, window_start, starts) VALUES (?1, ?2, ?3, ?4, 1)
ON CONFLICT (kind, key, period_seconds, window_start) DO UPDATE SET starts = starts + 1 RETURNING starts;
-- seen_message (?5 = now ms, ?6 = period ms)
INSERT INTO rules_control_messages (scope, asker, digest, seen_at, repeated) VALUES (?1, ?2, ?3, ?5, 0)
ON CONFLICT (scope, asker, digest) DO UPDATE SET repeated = (seen_at > ?5 - ?6), seen_at = excluded.seen_at RETURNING repeated;
-- take_lock (?3 = now ms, ?4 = lease ms)
INSERT INTO rules_control_locks (key, holder, expires_at) VALUES (?1, ?2, ?3 + ?4)
ON CONFLICT (key) DO UPDATE SET holder = excluded.holder, expires_at = excluded.expires_at
WHERE rules_control_locks.expires_at <= ?3 OR rules_control_locks.holder = excluded.holder RETURNING holder;
-- renew_lock
UPDATE rules_control_locks SET expires_at = ?3 + ?4 WHERE key = ?1 AND holder = ?2 AND expires_at > ?3 RETURNING key;
-- release_lock
DELETE FROM rules_control_locks WHERE key = ?1 AND holder = ?2;
```

```sql
-- PostgreSQL, migration 1 (scaled shape)
CREATE TABLE rules_control_starts (kind text NOT NULL CHECK (kind IN ('command','scope','asker')), key text NOT NULL,
  period_seconds integer NOT NULL, window_start timestamptz NOT NULL, starts integer NOT NULL CHECK (starts >= 1),
  PRIMARY KEY (kind, key, period_seconds, window_start));
CREATE TABLE rules_control_messages (scope text NOT NULL, asker text NOT NULL, digest text NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
  seen_at timestamptz NOT NULL, repeated boolean NOT NULL DEFAULT false, PRIMARY KEY (scope, asker, digest));
CREATE TABLE rules_control_locks (key text PRIMARY KEY, holder text NOT NULL, expires_at timestamptz NOT NULL);
CREATE TABLE rules_intake_places (intake text NOT NULL, slot integer NOT NULL CHECK (slot >= 1), run_id text, taken_at timestamptz,
  PRIMARY KEY (intake, slot));
CREATE TABLE rules_run_facts (run_id uuid PRIMARY KEY, agent_instance_id uuid NOT NULL, facts jsonb NOT NULL, updated_at timestamptz NOT NULL);
CREATE TABLE rules_approved_calls (run_id uuid NOT NULL, tool text NOT NULL, arguments_digest text NOT NULL, approval_id uuid NOT NULL,
  used boolean NOT NULL DEFAULT false, created_at timestamptz NOT NULL, PRIMARY KEY (run_id, tool, arguments_digest, approval_id));
```

No table holds a message, an argument value or a path's content: only digests, counts and names. `rules_run_facts` and `rules_approved_calls` rows are deleted by `end_run`; rows older than 30 days are deleted at start (*chosen here*) to clear runs that never ended cleanly.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A control names a tool or sub-agent that is not bound (A.2) | `Err(permanent)` at load | `RULE_UNBOUND_TOOL` | `RulesError::Load { message: "the control fetch-only-docs names the tool http_get, which is not bound", where }` |
| An argument rule names a place the tool does not describe (A.2) | `Err(permanent)` | `RULE_UNKNOWN_ARGUMENT` | `RulesError::Load` naming rule, tool and place |
| A pattern is empty, over 200 characters, or refused by `regex` (A.2) | `Err(permanent)` | `RULE_BAD_PATTERN` | `RulesError::Load` |
| More than 200 argument rules (A.2) | `Err(permanent)` | `RULE_TOO_MANY_RULES` | `RulesError::Load` |
| A configured control takes a package control's name (A.3) | `Err(permanent)` | `RULE_GATE_OVERRIDE` | `RulesError::Load` |
| An agent policy entry is malformed (A.4) | `Err(permanent)` | `RULE_POLICY_INVALID` | `RulesError::Load` |
| A decision for an instance with no loaded set (B.1, D.4) | `Err(permanent)` at start; `deny` before a tool | `RULE_RULESET_NOT_LOADED` | Verdict `deny`, source `fail_closed`, reason "This agent's rules are not loaded." |
| A fact cannot be read at the start (B.10) | `Err(retryable)` | `RULE_FACT_UNAVAILABLE` | `RulesError::FactUnavailable { control }`; the run fails |
| A fact cannot be read before a tool call (D.7) | `deny` | `RULE_FACT_UNAVAILABLE` | reason "Refused by the control <c>: a declared control could not be evaluated." |
| Tool name too long, arguments over 1 MiB, malformed origin (D.2) | `deny` | `RULE_TOOL_NAME_INVALID`, `RULE_ARGUMENTS_TOO_LARGE`, `RULE_ORIGIN_INVALID` | source `fail_closed` |
| The call's run is unknown or belongs to another instance (D.4) | `deny` | `RULE_RUN_UNKNOWN`, `RULE_SESSION_MISMATCH` | source `fail_closed` |
| The ToolRequest cannot be journalled (D.1) | `deny` | `RULE_JOURNAL_UNAVAILABLE` | source `fail_closed` |
| The Approval Service refuses or fails for an ask (D.8) | `deny` | `RULE_APPROVAL_UNAVAILABLE` | source `fail_closed`, reason "waiting for the person's approval could not start" |
| A control-state statement fails (G.7, D.11) | `Err(retryable)`; `deny` inside a decision | `RULE_STATE_UNAVAILABLE` | `RulesError::State` |
| A text is not an address (I) | `Err(permanent)`; inside a decision the rule holds against the call | `RULE_NOT_AN_ADDRESS` | `RulesError::NotAnAddress("the address carries user information")` |
| A rule kept as a setting is not allowed (J) | `Err(permanent)` | `RULE_SETTING_RULE_NOT_ALLOWED` | `RulesError::SettingRuleNotAllowed("kept-answers/r-1917b157949f")` |
| `resume_run` for an unknown run (G.4) | `Err(permanent)` | `RULE_RUN_UNKNOWN` | `RulesError::RunUnknown` |
| A command from a person that is not a declared check (K.1.3) | `deny` | `RULE_COMMAND_NOT_DECLARED` | rule `app.shell.person_undeclared`, reason "Only the project's declared checks run from here." |
| A command request with empty or over-long text, or a relative working folder (K.1.1) | `deny` | `RULE_COMMAND_INVALID` | rule `app.input.command`, source `fail_closed` |
| The Approval Service fails while a caller waits (K.2) | `Err(retryable)` | `RULE_APPROVAL_UNAVAILABLE` | `RulesError::ApprovalUnavailable`; the caller denies the call |
| A path checked outside a call is protected or outside the grants (K.5) | `deny` | `RULE_PATH_NOT_ALLOWED` | rule `app.folder_grant`, reason "the path is outside the places this agent may use" |
| A lock or retained-content statement fails (K.8, K.9) | `Err(retryable)` | `RULE_STATE_UNAVAILABLE` | `RulesError::State` |

The person never sees these codes; deny reasons go to the model and to Diagnostics (07 — Error handling).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Rules Engine

  Scenario: Happy path — a read inside the agent folder is allowed
    Given instance "analyst" with agent folder "/Users/asha/Library/Application Support/ordin/agents/analyst-1"
    And run "R1" began for it in workspace "Sales"
    When decide_tool_call is called for Read with file_path "<agent folder>/notes.md"
    Then the verdict is allow with rule "app.default_allow"
    And a ToolRequest with state decided and the arguments' digest is journalled

  Scenario: Folder grant — a write to a place granted for reading is denied
    Given workspace "Sales" grants place "Quotes" to "analyst" with access read
    When decide_tool_call is called for Write with file_path inside "Quotes"
    Then the verdict is deny with source app_folder_policy and rule "app.folder_grant"

  Scenario: Protected location is never granted
    Given a grant that resolves to the person's home folder with access read_and_change
    When decide_tool_call is called for Read of "~/.ssh/id_ed25519"
    Then the verdict is deny with rule "app.folder_grant"

  Scenario: Approval list from the declaration asks every time
    Given instance "analyst" whose declaration lists approval-required ["email_send"] and binds the tool "email_send"
    When the harness calls "email_send"
    Then the verdict is ask with the control "approval-list"

  Scenario: An empty approval-list entry is refused at load
    Given a rule set whose approval_list holds ""
    When load_rule_set is called
    Then it fails with RULE_POLICY_INVALID naming approval-required[0]

  Scenario: Agent policy — a prohibited verb inside a chained command
    Given instance "eng" with authority system_changing and prohibited verb "git.push"
    When decide_tool_call is called for Bash with command "npm test && git push origin main"
    Then the verdict is deny with source agent_tool_policy and rule "git.push"

  Scenario: Agent policy — a forbidden flag
    Given forbidden flag "--no-verify"
    When decide_tool_call is called for Bash with command "git commit -m 'x' --no-verify"
    Then the verdict is deny naming "--no-verify"

  Scenario: Shell for a document-only agent
    Given instance "analyst" with authority document_only
    When decide_tool_call is called for Bash with any command
    Then the verdict is deny with rule "app.shell"

  Scenario: Ask — an external action opens one approval
    Given a call classified External kind "origin_push" with approval Always
    When decide_tool_call is called
    Then the verdict is ask with rule "app.external_action"
    And the Approval Service holds one pending approval for run "R1"
    When the same call with the same arguments is decided again while pending
    Then the verdict is ask with the same approval_id and no second approval exists

  Scenario: Approved once on resume (C-80)
    Given an ask for tool "mcp__mail__email_ask_to_send" with digest D was approved
    When the resumed session repeats the call with the same arguments
    Then the verdict is allow with rule "app.approved_once"
    When the call is repeated a third time with the same arguments
    Then the verdict is ask again

  Scenario: Argument rule refuses a look-alike host
    Given the rule fetch-only-docs of the example
    When the url is "https://example.org.evil.net/api/public/x"
    Then the verdict is deny "Refused by the control fetch-only-docs: the argument url is not permitted."

  Scenario: Argument rule that cannot be evaluated holds against the call
    Given a refuse-when rule on argument "items[].url" with patterns ["^http:"]
    When the arguments are {"items": "not a list"}
    Then the verdict is deny naming that rule

  Scenario: Pause rule asks instead of refusing
    Given an argument rule with outcome pause on tool "post_message" argument "channel" form refuse-unless equals_any ["#team"]
    When the channel is "#general"
    Then the verdict is ask with source declaration_argument_rule

  Scenario: Message rule ends a run with a stated reply
    Given a configured message rule "holiday-reply" with keywords [holiday, policy] match all and reply "The holiday policy is in the handbook, section 4."
    When begin_run is called with text "Where is the holiday policy?"
    Then the outcome is Replied with control "holiday-reply"

  Scenario: Duty lock — two cycles start at the same instant
    Given duty "triage-inbox" with command "triage" and no other run in progress
    When two runs of "triage" call begin_run at the same instant on two instances
    Then exactly one proceeds
    And the other ends with ending "overlap"

  Scenario: Cycle limit counts within a fixed window
    Given duty "triage-inbox" with max-cycles 2 and period-seconds 86400
    When three runs of "triage" start on the same UTC day
    Then the third ends with ending "cycle-limit"
    And a heartbeat line ending "cycle-limit" is appended when the form lists it

  Scenario: Judged loop refuses the ending, then asks after three refusals
    Given a judged loop on command "draft" with reviewer "checker" and pause-after 3
    And run "R2" of "draft" wrote a work-folder file and never called task:checker
    When decide_run_end is called three times
    Then each returns Continue naming "judged-loop:draft"
    When decide_run_end is called a fourth time
    Then it returns Ask with an approval to set the gate aside
    When the person approves and settle_end_gates is called with true
    Then it returns End

  Scenario: Configuration cannot override a package gate
    Given the package declares a step gate "deploy-needs-plan"
    When a configuration declares a step gate named "deploy-needs-plan"
    Then load_rule_set returns RULE_GATE_OVERRIDE and the previous set stays loaded

  Scenario: Fail closed — no rule set loaded
    Given no rule set is loaded for instance "ghost"
    When decide_tool_call is called for any tool of "ghost"
    Then the verdict is deny with source fail_closed

  Scenario: Fail closed — the Approval Service is unavailable
    Given the approvals store fails
    When a call that must ask is decided
    Then the verdict is deny with source fail_closed and code RULE_APPROVAL_UNAVAILABLE

  Scenario: Server switch — external kind switched off
    Given the profile lists "telegram_post" in server.external_actions_off on a personal server
    When a call classified External kind "telegram_post" is decided
    Then the verdict is deny with rule "app.server.external_off"

  Scenario: Email rules from the profile
    Given email.internal_only is true and company_domains is ["acme.co.uk"]
    When email_ask_to_send is called with a recipient "dan.moss@kestrel.co.uk"
    Then the verdict is deny with rule "app.email.internal_only"

  Scenario: Reviewer kept to reading (agent profile, C-78 confirmed)
    Given the package's reviewer is "checker" and the hook names the sub-agent
    When "checker" calls Write
    Then the verdict is deny with rule "app.reviewer_read_only"

  Scenario: A declared check from a run is allowed without a card
    Given run "R1" of instance "eng" with authority system_changing
    When decide_command is called with origin run "R1", command "cargo test" and declared_check true
    Then the verdict is allow with rule "app.shell.declared_check"

  Scenario: An undeclared command from a run asks, and the caller waits
    Given run "R1" of instance "eng" with authority system_changing
    When decide_command is called with command "make deploy" and declared_check false
    Then the verdict is ask with rule "app.shell" and an approval id
    When wait_for_approval is called and the person approves
    Then it returns Approved and the approved-once record of that approval is spent

  Scenario: A person's undeclared command is refused
    When decide_command is called with origin person and declared_check false
    Then the verdict is deny with rule "app.shell.person_undeclared"

  Scenario: Waiting past the deadline leaves the approval open
    Given an approval opened by decide_tool_call
    When wait_for_approval is called with a deadline and nobody decides
    Then it returns Pending
    And the approved-once record is still unused

  Scenario: GitHub setting lets a pull request comment through
    Given the project place "supplier-portal" has setting comment with approval_required false
    And run "R1" is granted only that place
    When decide_tool_call is called for Bash with command "gh pr comment 12 --body 'done'"
    Then the verdict is allow with rule "app.git_host_setting"
    When the command is "gh pr merge 12"
    Then the verdict is ask with rule "app.shell"

  Scenario: A protected location cannot become a place
    When is_protected_location is called with "/Users/asha/.ssh"
    Then it returns true
    When is_protected_location is called with "/Users/asha/Documents/Quotes"
    Then it returns false

  Scenario: Reading a granted place
    Given workspace "Sales" grants place "Quotes" to instance "analyst" with access read
    When may_read is called for reader agent "analyst", place "Quotes" and path "2026/q3.pdf"
    Then it returns true
    When the path is "../other/secret.txt"
    Then it returns false
    When the reader is the hosted agent
    Then it returns false

  Scenario: Path check for ordin's own conversion files
    When check_path_access is called with no agent for a file under the application data root
    Then the decision is allow
    When it is called with no agent for "/etc/hosts"
    Then the decision is deny with rule "app.folder_grant"

  Scenario: Email rules collect every violation
    Given email.internal_only is true, company_domains ["acme.co.uk"], max_recipients 2 and foot_line_required true
    When check_email is called with 3 recipients, one at "kestrel.co.uk", drafted by an agent, and a body without the foot line
    Then the verdict is refused with TooManyRecipients, RecipientOutsideCompany and FootLineMissing

  Scenario: Duty lock state for the operator
    Given run "R7" holds the lock "duty:triage-inbox" until 20 seconds from now
    When lock_held is called with duty_lock_key("triage-inbox")
    Then it returns true

  Scenario: Address parsing resolves dot segments
    When parse_address is called with "HTTPS://API.Example.org:443/api/public/%2e%2e/admin?x=1"
    Then the address has scheme "https", host "api.example.org", port 443 and path "/api/admin"

  Scenario: A craft server added after start is still ordin's
    Given RulesConfig has ordin_servers ["ordin", "work"] and ordin_server_prefixes ["craft_"]
    When a tool call arrives from PackageMcpServer "craft_slides"
    Then its origin is treated as OrdinMcpFront
```

## Performance / Security / Observability

- **Performance targets:** `decide_tool_call` p95 ≤ 5 ms and p99 ≤ 20 ms on SQLite for a set of 200 argument rules and 50 step gates, excluding an ask's call to the Approval Service (≤ 15 ms more), so the hook answers well inside the 2 s of LW-21. `begin_run` p95 ≤ 30 ms excluding host facts. Pattern matching is linear in the text (`regex`), text cut at 4,000 characters. Memory: ≤ 2 MiB per loaded instance set (compiled regex size limit 1 MiB each, assumed far above real sets); up to 50 instances (07 Scale) → ≤ 100 MiB worst case, typical under 10 MiB.
- **Security:** Every entry point is in-process; callers are authenticated by their own channel (the Hook Relay's session secret; the MCP Front's session). The engine trusts no claim it can check: it reclassifies MCP Front servers, matches run and instance, and denies on any error (fail closed). No model call, no network call. Arguments are never journalled or logged, only their SHA-256 digest; deny reasons name controls and argument places, never values. An agent's policy and a configuration can only narrow the package and the application policy.
- **Observability:** log events (target `ordin_rules`): `rules_decided` {run_id, tool, decision, source, rule_id, tool_request_id}; `rules_start_ended` {control, ending, command}; `rules_start_decided` {control, what}; `rules_ending_refused` {control, refusals}; `rules_set_aside` {control}; `rules_rule_ignored` {rule, reason}; `rules_lease_lost` {key}; `rules_resume_refused` {control, ending}; `rules_stale_lock_removed` {duty}; `rules_fact_unavailable` {control, tool, error_code}; `rules_load_failed` {agent_instance_id, code}. Agent profile only (SA-XC-04): wrightrun's counters unchanged — `wrightrun_runs_ended_by_control_total`, `wrightrun_tool_calls_refused_total`, `wrightrun_tool_calls_paused_by_rule_total`, `wrightrun_run_endings_refused_total`, `wrightrun_controls_set_aside_total`, labels `agent`, `control`, `source` (SA-OBS-02). Spans (agent profile, when traces are on): `rules.decide` with attributes `tool`, `decision`, `source` only.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

### What the engine learns about the Slack tools

The MCP Front calls `decide_tool_call` with `CallContext { class: None }` for every tool it serves (McpFront.md step 17), so the four Slack tools reach the engine as `mcp__slack__<tool>` with no class, exactly as the mail and Telegram tools do. Read today, that gives:

| Tool | Decision today without change | Why that is right |
|---|---|---|
| `slack_search`, `slack_read` | D.9 allow (`app.default_allow`, `app_approval_policy`) | reads of content the session's workspace links; the Slack Service checks grant and workspace itself |
| `slack_draft_post` | D.9 allow | a draft changes nothing outside ordin |
| `slack_ask_to_post` | D.9 allow | it posts nothing: the Slack Service opens its own approval card and posts only on `approved` (the same position as `email_ask_to_send` and `telegram_ask_to_post`, whose provider holds the person's approval — `ApprovalNeed::NotNeeded`, line 400) |

So the engine asks no second card for the Slack tools; one card, the Slack Service's, is the person's approval (ADR-005).

### One new deny check — Slack switched off on the personal server

ADR-074's server switch (D.5.1) applies only when `ctx.class` is `External { kind }`, which the MCP Front never sets. For Slack, the server switch must stop drafts and asks too. Internal Logic **D.5** therefore has item 1b, after item 1:

> 1b. *Server Slack switch* (`app.server.slack_off`, source `app_approval_policy`; ADR-102): on the personal server, when `"slack_post"` is in the profile's `server.external_actions_off[]` and the tool, after the `mcp__slack__` prefix is removed, is `slack_draft_post` or `slack_ask_to_post` with origin `OrdinMcpFront { server: "slack" }` → deny "Slack posts are switched off on this server.". `slack_search` and `slack_read` are not denied by this check (reading posts nothing).

This is defence in depth: the Slack Service also refuses those tools with `SLK_SERVER_SLACK_OFF` and offers none of them when the switch is on, and refuses rule-sent and card-sent posts, which never pass through the engine.

### No standing rule in the engine

"A rule the person set once" is **not** a Rules Engine concept. The engine decides **tool calls** made by sessions; a rule-sent Slack post is made by the Slack Service on the Handover Service's or Ordin's draft and never is a tool call, and a rule must name a channel, a kind of message and a daily cap that only the Slack Service knows. Rules therefore live in the Slack Service (`SlackRule`), which checks `slack.rules_allowed` and the server switch itself. The engine's `ApprovalNeed` is unchanged, and no rule here allows an agent's post without a card.

### Text in place

- `ToolClass::External` doc comment: no change — `"slack_post"` is already one of the kinds `server.external_actions_off[]` may name.
- `RulesConfig.ordin_servers` doc comment lists `"slack"` beside `"mail"` and `"telegram"`.
- The list of profile keys the engine reads (Shared Context, `ordin-organisation-profile`) notes that `server.external_actions_off[]` also drives D.5.1b.

### Error Table

No new code: the deny carries the rule id `app.server.slack_off` and the sentence above, as D.5.1 does.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Slack switched off on the server denies drafting and asking
    Given the program is the personal server and the profile lists "slack_post" in server.external_actions_off
    When a call to "mcp__slack__slack_ask_to_post" from the MCP Front is decided
    Then the verdict is deny with rule "app.server.slack_off"
    And a call to "mcp__slack__slack_draft_post" is denied with the same rule

  Scenario: reading Slack is not switched off
    Given the same profile
    When a call to "mcp__slack__slack_read" is decided
    Then the verdict is allow with rule "app.default_allow"

  Scenario: the Slack tools ask no card of their own on the desktop
    Given the program is the desktop
    When a call to "mcp__slack__slack_ask_to_post" is decided
    Then the verdict is allow and no approval is requested by the engine

  Scenario: the switch is server only
    Given the program is the desktop and the profile lists "slack_post" in server.external_actions_off
    When a call to "mcp__slack__slack_draft_post" is decided
    Then check D.5.1b does not apply and the verdict is allow
```

### Gaps

None.

---

## Amendment January 2026 — Content checks done by ordin (ADR-107)

> Source: decision 87; HLD revision 0.55.0, ADR-107 ("What must happen on every run is configured and done by ordin, never left to the model"). Agent profile only. Counterparts: `components/AgentConfiguration.md` (the `content-check` control), `components/ConnectionBinder.md` (B-5a, the checker operation bound and never offered), `components/SessionSupervisor.md` (J.7, where the checks are asked for and a block becomes `ModelFailureCause::ContentFilter`), `components/ModelPolicy.md` (MP-8).

### Purpose (addition)

A hosted agent that answers in team channels may need every question and every answer checked by a content checker before the person sees anything — whether the model would have chosen to check or not. The engine gains a fourth deterministic decision, the **content check**: it sends the text to a checker the organisation supplies behind a declared HTTP operation and returns allow or block. ordin defines the contract and names no checker or vendor; the organisation supplies the checker, from any moderation service or its own code. A block becomes the existing `ModelFailureCause::ContentFilter`, so the safe-reply and retry-once rules apply unchanged.

### Shared Context (additions)

From the Agent Configuration spec (`ordin_types::agent_profile`, copied): `ContentCheck { name, connection, operation, points: Vec<CheckPoint>, on_block: OnBlock, on_unreachable: OnUnreachable, categories: Vec<String>, timeout_seconds: u32, max_chars: u32 }`; `CheckPoint { Input, Output }`; `OnBlock { End, RetryWithoutContext }`; `OnUnreachable { Block, Allow }`.

`ConfiguredControls` gains one field (the Agent Loader fills it from `ControlsConfig.content_checks`):

```rust
pub struct ConfiguredControls {
    pub message_rules: Vec<MessageRule>,
    pub steps: Vec<StepGate>,
    pub argument_rules: Vec<ArgumentRule>,
    pub schedule_entries: Vec<(String, String)>,
    /// ADR-107; empty everywhere but the agent profile.
    #[serde(default)] pub content_checks: Vec<ContentCheck>,
}
```

### Public Interface (additions)

```rust
/// The checker contract (ADR-107; fixed here). Request body sent to the checker operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ContentCheckRequest {
    pub point: CheckPoint,          // serialised "input" | "output"
    pub run_id: String,
    pub text: String,               // cut to the control's max_chars characters
}
/// The checker's reply. Any other shape counts as unreachable (L.4).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ContentCheckReply {
    pub verdict: CheckerVerdict,
    #[serde(default)] pub category: Option<String>,   // 1..=64 characters when present
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckerVerdict { Allow, Block }

/// Owned by the engine (ADR-100 form); implemented by the server program over the Connection Binder's
/// bound checker operation (ConnectionBinder.md B-5a). Sends the request as a JSON body and parses the reply.
#[async_trait::async_trait]
pub trait ContentChecker: Send + Sync {
    async fn check(&self, connection: &str, operation: &str, request: &ContentCheckRequest,
                   timeout: std::time::Duration) -> Result<ContentCheckReply, CheckerUnreachable>;
}
/// A timeout, a refused connection, a status outside 200–299 or a reply that is not ContentCheckReply.
#[derive(Debug)] pub struct CheckerUnreachable { pub reason: &'static str }  // "timeout" | "refused" | "status" | "reply"

/// The engine's answer for one point of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentCheckOutcome {
    /// No content check covers this point, or every one allowed (or was unreachable with `on-unreachable: allow`).
    Passed,
    /// The first control in configuration order that blocked.
    Blocked { control: String, category: Option<String>, on_block: OnBlock, unreachable: bool },
}

// RulesHost gains one provided method (the program overrides it in the agent profile):
//     /// The checker behind the instance's content-check controls; None outside the agent profile.
//     fn content_checker(&self) -> Option<std::sync::Arc<dyn ContentChecker>> { None }

impl RulesEngine {
    /// ADR-107. Runs every content-check control of the instance that covers `point`, in configuration
    /// order, and stops at the first block. Never fails: an engine error blocks (fail closed, L.6).
    /// Called by: the server program's `SupervisorHost::check_content` (Session Supervisor J.7), for the
    /// question before the run starts and for the final text before it is delivered.
    pub async fn check_content(&self, agent_instance_id: AgentInstanceId, run_id: RunId,
                               point: CheckPoint, text: &str) -> ContentCheckOutcome;
}
```

### Internal Logic (additions)

**A. `load_rule_set`**, step 2 also covers `set.configured.content_checks`: a content check whose name equals another control's name → `RULE_GATE_OVERRIDE` (names are unique across every control, A.3). Each becomes a control line (section F) with `source: Configuration`, `point: Start` for `input` and `End` for `output` (one line per point), `enforced: true`, and `detail` = `"checked by <connection>/<operation>; every question (or answer) is sent there"`, so the start-up report states, beside each connection, that the checker receives every question or every answer (HLD 10, risk on the checker).

**L. `check_content(agent_instance_id, run_id, point, text)`** (ADR-107):

1. Find the instance's `ControlSet`; absent → `Blocked { control: "app.ruleset", category: None, on_block: End, unreachable: false }`, log `RULE_RULESET_NOT_LOADED` (fail closed).
2. `checks` = the set's content checks whose `points` hold `point`, in configuration order. None → `Passed`, no call.
3. `host.content_checker()` is `None` while `checks` is not empty → treat each as unreachable (step 5).
4. For each check: `request = ContentCheckRequest { point, run_id, text: <text cut to max_chars characters, on a character boundary> }`; `checker.check(connection, operation, &request, timeout_seconds)`.
   - `Ok(Allow)` → next check.
   - `Ok(Block { category })` with `categories` empty, or `category` in `categories` → `Blocked { control: name, category, on_block, unreachable: false }`; a category not in a non-empty `categories` counts as allow. A `category` above 64 characters is cut to 64.
5. `Err(CheckerUnreachable)` → log `rules_checker_unreachable` {control, point, reason} with `code = "RULE_CHECKER_UNREACHABLE"` (never the text); `on_unreachable: Block` → `Blocked { control: name, category: None, on_block: End, unreachable: true }` (a retry without gathered context cannot help while the checker does not answer, so the run ends with the safe reply whatever the control's `on-block`); `Allow` → next check.
6. Any other internal error → `Blocked { control: name, category: None, on_block: End, unreachable: true }` with code `RULE_CHECKER_UNREACHABLE` in the log.
7. Every outcome: `journal.append_entry` kind `rules_content_check` {run_id, control, point, verdict: `allow` | `block` | `unreachable`, category} — never the text; metrics family 50 (`wrightrun_content_checks_total{point, verdict}`) through the sink, once per check called. A check's text, the request body and the reply body are never logged, journalled or kept.

The engine makes no model call: the checker is the organisation's own operation, reached over the HTTP connection the configuration declares, and the engine never sees a model's view of the text.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The checker times out, refuses, answers outside 200–299 or with another shape, or no checker is attached (L.3, L.5, L.6) | outcome `Blocked { unreachable: true }` with `on-unreachable: block` (the default); `Passed` with `allow` | `RULE_CHECKER_UNREACHABLE` | logged `rules_checker_unreachable` {control, point, reason}; the run ends with the safe reply (Session Supervisor J.7) |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: The input check blocks
    Given a content-check "screen" on point "input" whose operation answers {"verdict":"block","category":"prompt-attack"}
    When check_content is called for point input
    Then it returns Blocked with control "screen" and category "prompt-attack"
    And the checker received {"point":"input","run_id":"<run>","text":"<the question>"}

  Scenario: An unreachable checker blocks by default
    Given on-unreachable is "block" and the operation times out after timeout-seconds
    When check_content is called
    Then it returns Blocked with unreachable true and on_block End, even when the control's on-block is "retry-without-context"
    And RULE_CHECKER_UNREACHABLE is logged without the text

  Scenario: An unreachable checker with on-unreachable allow lets the text through
    Given on-unreachable is "allow" and the operation answers HTTP 503
    When check_content is called
    Then it returns Passed and RULE_CHECKER_UNREACHABLE is logged

  Scenario: A block outside the listed categories counts as allow
    Given categories ["denied-topic"] and the operation answers {"verdict":"block","category":"profanity"}
    When check_content is called
    Then it returns Passed

  Scenario: No check covers the point
    Given a content-check on point "output" only
    When check_content is called for point input
    Then it returns Passed and the checker is not called

  Scenario: The text is cut to max-chars
    Given max-chars 10 and a question of 25 characters
    When check_content is called
    Then the request's text holds the first 10 characters

  Scenario: The start-up report names the checker
    When a rule set with a content-check on both points is loaded
    Then two control lines with source configuration name the checker's connection and operation
```

### Gaps

None.
