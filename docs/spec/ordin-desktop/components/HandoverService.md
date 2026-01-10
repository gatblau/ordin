# SPEC: Handover Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (`handover.list` takes the Service API catalogue's `{ filter }` input) · amended January 2026 (the SharePoint item types renamed `SharePointItem`, `SharePointItemRef`, `SharePointVersion`)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-handover` · **Build phase:** 9 · **Programs:** D P
> **Dependencies:** Catalogue Service, Checker, Approval Service, Push Service, Project Service, SharePoint Adapter, Jira Adapter, Mail Service, Slack Service, Workspace Service, Ordin Host, Identity, Journal

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
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)

## Purpose

The Handover Service hands work from one person to the next through a place both already read and write — a Git repository first, a SharePoint folder second — with no service of ordin's between them (02:737–742; ADR-092, ADR-095, ADR-099). On the sender's side it holds handover drafts, asks Ordin for the four-part note, has the Checker check the handover in fixed code, raises the approval card, and writes the note and the record to the place (in Git through the Push Service as a branch and a pull request labelled `ordin-handover`; in SharePoint through the SharePoint Adapter), emailing a receiver who does not use ordin inside the same approval. On the receiver's side it finds handovers addressed to the person at start, when a workspace opens and every 15 minutes, records **seen**, **Accept and start** and **Return to…** as appended events, and starts the next stage's workspace. It also keeps which lifecycle stage each workspace follows, starts a workspace for a stage in one step, and answers the lifecycle screen from the records it can read — including branches, side-by-side stages, meeting points and declared send-backs with their rounds.

## Approach

Each handover is a persisted state machine (2C `HandoverState`) mirrored by an **append-only event list in the place's `handover.yaml`**, where each installation appends only its own events with a conditional write (GitHub's content SHA; SharePoint's file version — A-44, R-83) and retries from the file as it now reads. Every person-facing step that reaches another person goes through an approval card; every check is the Checker's fixed code, fed with facts this component gathers (readability, headings, reachability) because the Checker reads no external system. Error codes keep 2C's prefix `HAND_` (*chosen here* for codes beyond 2C's). Rejected: *a shared ordin service or board between people* — breaks one person per installation (ADR-092 alternative (d)); *rewrite the record file on each change* — two installations would overwrite each other's events (R-83).

## Shared Context

### Shared types used (copied from Phase 2C, unchanged)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);
// UUIDv7 newtypes: HandoverId, HandoverPlaceId, LifecycleId, WorkspaceStageId, WorkspaceId,
// AgentRepositoryId, PartId, ApprovalId, ExternalActionKey, PlaceId, RunId

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Handover {
    pub id: HandoverId,
    pub title: String,                        // 1–120 (HAND_TITLE_INVALID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_id: Option<LifecycleId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_name: Option<String>,           // with lifecycle_id or neither (HAND_STAGE_MISMATCH)
    pub sender: String,
    pub receiver: HandoverReceiver,
    pub results: Vec<String>,                 // 1–50 links, readable by the receiver (HAND_RESULT_UNREADABLE)
    pub note: HandoverNote,
    pub checks_passed: Vec<String>,
    pub handover_place_id: HandoverPlaceId,
    pub round: u32,                           // ≥ 1; past the stage's max_rounds it is marked, never refused (C-21)
    /// True once a declared send-back takes `round` past the stage's agreed `max_rounds` (ADR-099 item 5; 2C C-21).
    pub beyond_agreed_rounds: bool,
    pub events: Vec<HandoverEvent>,           // append-only (HAND_EVENT_NOT_APPEND)
    pub state: HandoverState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_workspace_id: Option<WorkspaceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_into_workspace_id: Option<WorkspaceId>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HandoverReceiver {
    pub name: String,
    pub address: String,                      // RFC 5321 ≤ 254 (HAND_RECEIVER_INVALID)
    pub uses_ordin: bool,
    /// For a Git handover place, the receiver's GitHub login (ADR-092 item 3; 2C C-21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HandoverNote { pub sections: [NoteSection; 4] }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NoteSection { pub heading: String, pub body: String /* ≤ 4000 (HAND_NOTE_TOO_LONG) */, pub drafted_by_ordin: bool }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HandoverEvent {
    pub kind: HandoverEventKind,
    pub by: String,
    pub at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,               // required for sent_back; ≤ 1000 (HAND_REASON_MISSING)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoverEventKind { Sent, Seen, Accepted, SentBack, TakenBack }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoverState { Draft, Sent, Seen, Accepted, SentBack, TakenBack }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HandoverPlace {
    pub id: HandoverPlaceId,
    pub place_type: HandoverPlaceType,
    pub address: String,                      // https ≤ 2048, unique (HPLACE_ADDRESS_INVALID)
    pub display_name: String,                 // 1–120 (HPLACE_NAME_INVALID)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoverPlaceType { Git, Sharepoint }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceStage {
    pub id: WorkspaceStageId,
    pub workspace_id: WorkspaceId,            // unique (LIFE_WORKSPACE_FOLLOWS_TWO)
    pub lifecycle_id: LifecycleId,
    pub stage_name: String,
    pub set_at: Timestamp,
}

// Lifecycle, LifecycleStage (with `hands_over: Option<String>`, `handover_place_id`, `days_to_answer: u32` — 0 = not set),
// StageNext { One, OneOf, SideBySide, Nothing }, StageChoice { stage, when }, SendBack { to_stage, when, max_rounds }: 2C domain.

// 2C wire §5 parts used to build cards: ApprovalAsker (`Person` for every card here, C-19), AffectedItem, ApprovalContent,
// RejectStyle, ActionProposal { action, destination, affects, consequence }.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState { Pending, Approved, Rejected, Expired }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction { TryAgain, SignInAgain, SendToIt }
```

2C state machine: — →`draft` (sender starts it); `draft`→`sent` (after the approval card); `sent`→`seen` (receiver opens it); `seen`→`accepted` (**Accept and start**); `sent`, `seen`→`sent_back` (**Return to…** with reason, after its card); `sent_back`→`draft` (next round drafted); `draft`, `sent`, `seen`, `sent_back`→`taken_back`. `accepted` and `taken_back` are final. A draft is visible to the sender only. The interface label for `sent_back` is **Return to [name]…**, never "Send back" (11 §3; 1C *Return to…*).

### The note's four headings (ADR-092; LD-37 fixed here)

`What's done` · `Decisions made` · `Open questions` · `What's needed next`, in this order, written in en-GB in the files (*assumed here*: the record is read by another person's ordin whose locale may differ; the screens show the headings from the Fluent catalogue `handover-note-done`, `handover-note-decisions`, `handover-note-open`, `handover-note-next`).

### Files in the handover place (08 *Handover record*; names and schema confirmed here)

`handover.md` — the note: `# [title]`, then the four headings as `## ` lines each followed by its body, then `## Results` with one Markdown link per result.

`handover.yaml` — the record. Keys are `snake_case` (decision D7: the handover record is ordin's own format under SA-JSON-01; 08's camelCase spellings `recordSchema`, `nextStage`, `sentWith`, `returnedTo`, `sendBackRound`, `beyondAgreedRounds` are replaced, H08-18):

```yaml
record_schema: ordin.handover/v1
id: 0192a3d0-1111-7aaa-8bbb-cccc0000d001
title: Northwind assessment
lifecycle: { repository: github.com/acme/agents, identifier: "urn:ordin:acme:journeys:supplier-onboarding", version: "1.2.0" }  # optional
stage: Analysis                     # optional; the receiver's stage
next_stage: Contracting             # optional; the stage chosen at a choice (ADR-099 item 3)
sent_with: { handovers: [0192a3d0-1111-7aaa-8bbb-cccc0000d002], meet_at: Approval }  # optional; side by side
sender: { name: Priya Shah, address: priya.shah@acme.co.uk, github_login: pshah }
receiver: { name: Sam Lee, address: sam.lee@acme.co.uk, github_login: slee }
results:
  - { label: Supplier assessment, link: "https://acme.sharepoint.com/sites/proc/Shared%20Documents/assessment.docx" }
contract: { identifier: "urn:ordin:acme:output-contracts:supplier-assessment", version: "1.0.0" }  # optional
checks_passed: [results_readable, contract_sections, receiver_reachable]
round: 1
events:
  - { kind: sent, by: priya.shah@acme.co.uk, at: "2026-01-10T09:00:00.000Z", round: 1 }
  - { kind: seen, by: sam.lee@acme.co.uk, at: "2026-01-10T09:20:00.000Z", round: 1 }
  - kind: sent_back
    by: sam.lee@acme.co.uk
    at: "2026-01-10T11:00:00.000Z"
    round: 1
    reason: Payment terms are missing
    returned_to: { stage: Analysis, person: { name: Priya Shah, address: priya.shah@acme.co.uk } }  # only past the sender
    send_back_round: { round: 2, max_rounds: 3, beyond_agreed_rounds: false }
```

Parsing rules: the file is parsed with Shared types' restricted reader `ordin_types::yaml::read_restricted::<T>(text, 262_144)` (over `saphyr`; 05 *YAML parsing*, decisions D2 and P4-1; anchors, aliases, custom tags and over-size input refused), where `T` is this crate's private serde type for the keys above, and written by this crate's fixed code; bytes that are not UTF-8, or any `YamlError` (`YAML_FEATURE_REFUSED`, `YAML_MALFORMED`, `YAML_TOO_LARGE`), → `HAND_RECORD_UNREADABLE`; `record_schema` must equal `ordin.handover/v1`; unknown keys are kept and ignored (a newer minor may add them); `events` are read in order; an event whose `by` is the person reading is theirs. Each file ≤ 256 KiB (*chosen here*). Git: files under `handovers/<YYYY-MM-DD>-<slug>/` on branch `ordin-handover/<YYYY-MM-DD>-<slug>-<first 8 of id>` (*chosen here*). SharePoint: folder `<YYYY-MM-DD> <title>` in the place folder (ADR-092 item 3).

### Configuration

| Item | Value | Source |
|---|---|---|
| Receiving check | at start, on workspace open, every 15 minutes | 2D-4, A-43 |
| Open handovers a person is asked to review | up to 50 | A-43 |
| Conditional-write retries | 3, then the person is told what happened first | A-44, R-83 (*count chosen here*) |
| GitHub label | `ordin-handover` | 08, ADR-092 |

### Boundary interfaces this spec requires (each provider's current signature)

```rust
// ordin-journal — concrete `ordin_journal::Journal` (D4): `datastore()`; `append_entry(NewJournalEntry { kind, component:
//   "handover", workspace_id, summary, data, .. })`; `append_event("needs_you_changed", data)`;
//   `begin_external_action(NewExternalAction { action_key, run_id: None, kind: HandoverWritten, destination })`,
//   `confirm_external_action(key)`, `unsettled_external_actions(&[HandoverWritten])` -> Vec<ExternalAction { action_key,
//   destination, state, .. }>; trait `EventSink { name, deliver,
//   lagged }` (this service's sink reads `approval_decided`); `ModuleMigrations` from `MigrationStep` tuples.

// ordin-catalogue — `CatalogueService`: lifecycles(Option<AgentRepositoryId>) -> Vec<Lifecycle>; lifecycle(LifecycleId)
//   -> Lifecycle; lifecycle_place(HandoverPlaceId) -> LifecyclePlace { id, place_type, address };
//   find_part(repository, PartKind::OutputContract, identifier, None) -> PartView; read_part(PartId) -> PartDocument;
//   errors `CatalogueError { code, retryable, action, .. }`.

// ordin-checker — `Checker::check_handover(&self, input: &HandoverCheckInput) -> Result<HandoverCheckReport, CheckError>`:
//   HandoverCheckInput { results: Vec<HandoverResult>, contract: Option<HandoverContract>, receiver: ReceiverFacts }
//   HandoverResult { link, label, local_only: bool, readable_by_receiver: Option<bool>, headings: Option<Vec<String>> }
//   HandoverContract { scope: SourceScope, identifier: String }      // SourceScope re-exported by the Checker
//   ReceiverFacts { is_person: bool, reachable: bool }
//   HandoverCheckReport { passed, findings: Vec<HandoverFinding { code, passed, item: Option<String>, missing_sections, .. }> }

// ordin-workspaces — `WorkspaceService`: create(name: String) -> Workspace (its `.id`); get(id) -> Workspace;
//   source_links(id) -> Vec<SourceLink>; linked_repositories(id) -> Vec<LinkedRepository { host, repository, host_name, .. }>;
//   add_link(id, LinkLocation, display_name: Option<String>) -> SourceLink (LinkLocation::{Sharepoint { location },
//   GitRemote { url, branch, path }}, the latter the 2C addition of K21); opened() -> broadcast::Receiver<WorkspaceId>;
//   errors `WorkspaceError` (`WS_*`).

// ordin-host — `HandoverHost` (OrdinHost.md, X-19), imported, not restated: draft_note, recommend_next_stage,
//   start_from_handover, each returning `Result<_, OhError>`.

// ordin-approval — `ApprovalService`: request(ApprovalRequest) -> ApprovalCard (`card.approval_id`); withdraw(approval_id);
//   get(approval_id) -> ApprovalRecord (`.state`); errors `ApprovalError`.

// ordin-push — `PushService::send_approved_pull_request(ApprovedPullRequest { approval_id, proposal: PullRequestProposal })
//   -> Result<PullRequestRef, PushError>`; PullRequestProposal { workspace_id, origin: SubmissionOrigin::Handover { id },
//   host, repository, content: PullRequestContent::Files { files }, branch, base, title, body, labels, reviewers,
//   existing: None, card: CardText }. The Push Service records the `PullRequestOpened` external action.
//   subscribe() -> broadcast::Receiver<PushNotice>; this service reads PushNotice::PullRequestOpened { submission, origin,
//   pull_request: PullRequestRef } and PushNotice::PullRequestFailed { submission, origin, code } whose `origin` is
//   SubmissionOrigin::Handover { id }; other notices are ignored.

// ordin-projects — `GithubAccess` (ProjectService.md, *Repository hosts in process*), imported: review_requests(label),
//   file_at(host, repository, git_ref, path) -> (bytes, blob sha), update_file(host, repository, branch, path, bytes,
//   expected_sha, message) -> commit, pull_request, review(host, repository, number, ReviewVerdict, body), merge, close,
//   reviewers(host, repository, query) -> Vec<GithubPerson>, default_branch(host, repository); types ReviewRequest,
//   ReviewVerdict, GithubPerson, PullRequestView, PullRequestRef; errors `GithubError` (`GH_*`). GitHub only.

// ordin-sharepoint — `SharePointAdapter` (ruling R17: no directory search): list_children(&SharePointLocation, cursor)
//   -> Page<SharePointItem>; item_at(&folder, path) -> Option<SharePointItem>; fetch(&SharePointItemRef) -> FetchedItem { bytes,
//   version: SharePointVersion { tag, .. } }; upload_version(&item, bytes, expected_version: &str /* cTag */, ExternalWrite)
//   -> SharePointVersion (`SP_ORIGIN_CHANGED` on a stale tag); create_file(&folder, name, bytes, ExternalWrite);
//   create_folder(&parent, name, ExternalWrite) -> SharePointLocation; audience(&SharePointLocation) -> Audience;
//   ExternalWrite { key, run_id: None, kind: HandoverWritten }; errors `SharePointError` (`SP_*`).
//   resolve_web_url(web_url: &str) -> Result<SharePointLocation, SharePointError> (SharePointAdapter.md).
//   resolve_origin(origin_location: &str) -> Result<SharePointLocation, SharePointError> (SharePointAdapter.md).

// ordin-mail — `MailService`: send_under_approval(NewDraft, approval_id, run_id: None) -> SentEmail (sends only when the
//   approval is Approved and unused); draft_for_person(NewDraft) -> EmailDraft (the person sends it with `mail.send`);
//   NewDraft { workspace_id, kind: EmailDraftKind::New, answers: None, to, cc, bcc, subject, body, attachments };
//   errors `MailError` (`MAIL_*`).

/// Mine; implemented by the program over Identity and Credentials' `IdentityService::person()` (its *Callers* table:
/// "Handover Service, through the program's `PersonIdentity`").
#[async_trait::async_trait]
pub trait PersonIdentity: Send + Sync {
    async fn person(&self) -> Result<PersonProfile, CredError>;   // PersonProfile { name, email: Option, github_login: Option }
}
```

### Reconciliation assumptions

- **SA-RECON-01** Ruling R3, D4: tables live in the Journal's `Datastore`; migrations are `migrations() -> ModuleMigrations` (module `handover`, SQLite only) registered by the program.
- **SA-CLOSE-01** (*chosen here*; closing decisions K21, K22) A Git place or Git result is linked to the receiving workspace as `LinkLocation::GitRemote { url, branch, path }`, read from its `https` address: `https://<host>/<owner>/<name>` → `url` = that, `branch` = `HEAD` (the repository's default branch), `path` = ""; `…/<name>/tree/<branch>/<path>` (GitHub) or `…/<name>/-/tree/<branch>/<path>` (GitLab) → those parts; a `…/blob/…` file address links its folder. Any other address stays a link in the note. Confluence results stay links in the note: this crate has no Confluence edge and Confluence links exist on the desktop only. The Catalogue Service's caller table lists this service for `find_part` and `read_part`.
- **SA-CLOSE-02** (*chosen here*) The Handover Service withdraws an approval card in one place only: `take_back` (Internal Logic F.6) withdraws the handover's still-pending hand-over card (C.3) with `ApprovalService::withdraw` before anything else, so the person who takes a handover back is not left with a card that would still send it. ApprovalService.md names this caller and step.
- **SA-RECON-02** Ruling R6 (rows 653, 647): GitHub calls go through the Project Service's one `GithubAccess`; `HandoverGithub` is dropped.
- **SA-RECON-03** Ruling R17 (row 724): no SharePoint directory search; `HandoverSharePoint` maps to the adapter's `list_children`, `item_at`, `fetch`, `upload_version`, `create_file`, `create_folder`.
- **SA-RECON-04** Single external-action bracket: SharePoint writes are begun and confirmed by the SharePoint Adapter with this service's `ExternalWrite` key, and the pull request by the Push Service; this service brackets only its own GitHub API writes (`update_file`, `review`, `merge`, `close`) with `begin_external_action`/`confirm_external_action`, kind `HandoverWritten`.
- **SA-RECON-05** Ruling R6 (rows 218, 220, 221): the Approval Service's names are used — `request` returns an `ApprovalCard`, `withdraw`, and `get(id)?.state` in place of a `state` call.
- **SA-RECON-06** Ruling R6 (rows 346, 347): the receiver email is `send_under_approval` with `run_id: None`; the reminder is `draft_for_person`, sent by the person through the Mail Service's own card.
- **SA-RECON-07** Ruling R6 (row 684): result facts (location, readability by the receiver, headings) are composed here from the GitHub, SharePoint and Workspace calls (Internal Logic B.1); the Workspace Service offers no `describe_results`.
- **SA-RECON-08** Decision D3 pattern: `HandoverHost` is implemented by the Ordin Host and `PersonIdentity` by the program over Identity and Credentials.
- **SA-RECON-09** Decision D7: `handover.yaml` keys are snake_case.

## Public Interface

```rust
pub struct HandoverService { /* private */ }

pub struct HandoverDeps {
    /// Datastore owner (D4): tables through `journal.datastore()`; entries, events and external actions.
    pub journal: std::sync::Arc<ordin_journal::Journal>,
    pub catalogue: std::sync::Arc<ordin_catalogue::CatalogueService>,
    pub checker: std::sync::Arc<ordin_checker::Checker>,
    pub approvals: std::sync::Arc<ordin_approval::ApprovalService>,
    pub push: std::sync::Arc<ordin_push::PushService>,
    pub github: std::sync::Arc<dyn ordin_projects::GithubAccess>,
    pub sharepoint: std::sync::Arc<ordin_sharepoint::SharePointAdapter>,
    pub mail: std::sync::Arc<ordin_mail::MailService>,
    pub workspaces: std::sync::Arc<ordin_workspaces::WorkspaceService>,
    pub host: std::sync::Arc<dyn ordin_host::HandoverHost>,
    pub identity: std::sync::Arc<dyn PersonIdentity>,
}

impl HandoverService {
    pub const MODULE: &'static str = "handover";
    /// Data Model as `MigrationStep` tuples (version 1), `postgres: &[]`; the program registers it (SA-RECON-01).
    pub fn migrations() -> ordin_journal::ModuleMigrations;
    pub fn new(deps: HandoverDeps) -> std::sync::Arc<Self>;
    /// The sink the program registers with `Journal::subscribe`; reads `approval_decided` for this service's cards.
    pub fn event_sink(self: &std::sync::Arc<Self>) -> std::sync::Arc<dyn ordin_journal::EventSink>;
    pub async fn run(&self, shutdown: tokio_util::sync::CancellationToken) -> Result<(), HandoverError>;
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<HandoverNotice>;

    // Sending
    pub async fn open_draft(&self, input: OpenDraftInput) -> Result<HandoverDraftView, HandoverError>;
    pub async fn update_draft(&self, input: UpdateDraftInput) -> Result<HandoverDraftView, HandoverError>;
    pub async fn find_people(&self, input: FindPeopleInput) -> Result<Vec<PersonChoice>, HandoverError>;
    pub async fn hand_over(&self, draft: DraftGroupId) -> Result<ApprovalId, HandoverError>;
    pub async fn take_back(&self, id: HandoverId) -> Result<ApprovalId, HandoverError>;
    pub async fn remind(&self, id: HandoverId) -> Result<(), HandoverError>;
    pub async fn change_and_hand_over_again(&self, id: HandoverId) -> Result<HandoverDraftView, HandoverError>;

    // Receiving
    pub async fn check_now(&self) -> Result<u32 /* new handovers */, HandoverError>;
    pub async fn open(&self, id: HandoverId) -> Result<HandoverScreen, HandoverError>;
    pub async fn accept_and_start(&self, input: AcceptInput) -> Result<WorkspaceId, HandoverError>;
    pub async fn return_options(&self, id: HandoverId) -> Result<Vec<ReturnOption>, HandoverError>;
    pub async fn return_to(&self, input: ReturnInput) -> Result<ApprovalId, HandoverError>;

    // Lifecycles and stages
    pub async fn follow_stage(&self, input: FollowStageInput) -> Result<WorkspaceStage, HandoverError>;
    pub async fn stop_following(&self, workspace: WorkspaceId) -> Result<(), HandoverError>;
    pub async fn start_workspace_for_stage(&self, input: StartForStageInput) -> Result<WorkspaceId, HandoverError>;
    pub async fn stage_of(&self, workspace: WorkspaceId) -> Result<Option<StageView>, HandoverError>;
    pub async fn lifecycle_view(&self, id: LifecycleId) -> Result<LifecycleView, HandoverError>;
    pub async fn list(&self, filter: HandoverFilter) -> Result<Vec<HandoverLine>, HandoverError>;
    /// Who follows a lifecycle and how many of its handovers are open (Authoring Service's `PublishContext`).
    pub async fn following(&self, lifecycle_id: LifecycleId) -> Result<LifecycleFollowing, HandoverError>;
}
```

Inputs and results (*chosen here*; Service API inputs carry `deny_unknown_fields`):

```rust
pub struct DraftGroupId(pub uuid::Uuid);          // one per draft; side-by-side handovers share it
pub struct OpenDraftInput { pub workspace_id: WorkspaceId, pub results: Vec<String> }
pub struct UpdateDraftInput {
    pub draft: DraftGroupId,
    pub title: Option<String>,
    pub next_stage: Option<String>,               // at a choice; the person chooses (ADR-099 item 3)
    pub receivers: Option<Vec<StageReceiver>>,    // one per stage (side by side), else one
    pub results: Option<Vec<String>>,
    pub note: Option<[NoteSectionEdit; 4]>,
    pub place: Option<HandoverPlaceId>,           // outside a lifecycle: a place the workspace links
}
pub struct StageReceiver { pub stage: Option<String>, pub person: PersonChoice }
pub struct PersonChoice { pub name: String, pub address: String, pub github_login: Option<String>, pub outside_company: bool }
pub struct NoteSectionEdit { pub body: String }
pub struct FindPeopleInput { pub draft: DraftGroupId, pub query: String /* 1–100 */ }
pub struct HandoverDraftView {
    pub draft: DraftGroupId,
    pub handovers: Vec<Handover>,                 // one per receiver
    pub choices: Vec<StageChoice>,                // when the stage leads to a choice
    pub recommended: Option<(String, String)>,    // Ordin's recommendation and reason (P5)
    pub suggested_receiver: Option<PersonChoice>, // who took the stage last
    pub checks: Vec<HandoverFinding>,
    pub can_hand_over: bool,
}
pub struct HandoverScreen {
    pub handover: Handover,
    pub marked_lines: Vec<String>,                // note lines that read as instructions (R-81)
    pub arrived: Option<(u32, u32)>,              // "1 of 2 handed over" at a meeting point
    pub still_under_way: Vec<String>,
    pub results: Vec<ResultLine>,
    pub actions: Vec<HandoverAction>,
}
pub struct ResultLine { pub label: String, pub link: String, pub cannot_open: Option<String> }
#[serde(rename_all = "snake_case")]
pub enum HandoverAction { AcceptAndStart, ReturnTo, AskOrdin, Close }
pub struct AcceptInput { pub id: HandoverId, pub into: AcceptTarget }
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AcceptTarget { NewWorkspace { name: String }, Existing { workspace_id: WorkspaceId } }
pub struct ReturnOption { pub stage: String, pub person: PersonChoice, pub when: Option<String>, pub round: u32, pub max_rounds: Option<u32>, pub declared: bool }
pub struct ReturnInput { pub id: HandoverId, pub to: ReturnOption, pub reason: String /* 1–1000 */ }
pub struct FollowStageInput { pub workspace_id: WorkspaceId, pub lifecycle_id: LifecycleId, pub stage_name: String }
pub struct StartForStageInput { pub lifecycle_id: LifecycleId, pub stage_name: String, pub workspace_name: String }
pub struct StageView { pub lifecycle: Lifecycle, pub stage_name: String, pub next: Vec<String>, pub hands_on: Vec<String> /* OutputContract sections */ }
pub struct LifecycleView { pub lifecycle: Lifecycle, pub stages: Vec<StageLine> }
pub struct StageLine { pub stage: LifecycleStage, pub handovers: Vec<HandoverLine>, pub you_are_here: bool }
pub struct HandoverLine { pub id: HandoverId, pub title: String, pub other_person: String, pub state: HandoverState, pub round: u32, pub direction: Direction }
#[serde(rename_all = "snake_case")]
pub enum Direction { HandedToYou, HandedOver }
pub struct HandoverFilter { pub workspace_id: Option<WorkspaceId>, pub open_only: bool }
/// `following_workspaces`: display names of this person's workspaces that follow a stage of the lifecycle;
/// `open_handovers`: this person's handovers of the lifecycle in `draft`, `sent`, `seen` or `sent_back`.
pub struct LifecycleFollowing { pub following_workspaces: Vec<String>, pub open_handovers: u32 }

#[derive(Debug, Clone)]
pub enum HandoverNotice {
    HandedToYou { id: HandoverId, title: String, from: String },          // Needs you, Home, notice (11 §7)
    ReturnedToYou { id: HandoverId, title: String, by: String, reason: String },
    Answered { id: HandoverId, state: HandoverState },
    NoAnswer { id: HandoverId, receiver: String, days: u32 },            // Home: "No answer from [name] in [N] days"
    SendFailed { draft: DraftGroupId, code: &'static str },
}

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct HandoverError { pub code: &'static str, pub retryable: bool, pub action: Option<RepairAction>, pub detail: String }
```

Service API commands (*chosen here*): `handover.open_draft`, `handover.update_draft`, `handover.find_people`, `handover.hand_over`, `handover.take_back`, `handover.remind`, `handover.change_and_hand_over_again`, `handover.check_now`, `handover.open`, `handover.accept_and_start`, `handover.return_options`, `handover.return_to`, `handover.follow_stage`, `handover.stop_following`, `handover.start_workspace_for_stage`, `handover.stage_of`, `handover.lifecycle_view`, `handover.list`. These are the catalogue's names (R1); the Service API takes this module's inputs and results (`handover.open` → `HandoverScreen`, `handover.accept_and_start` with `AcceptInput` → `WorkspaceId`, `handover.return_to` with `ReturnInput`, `handover.list` with `{ filter: HandoverFilter }` → `Vec<HandoverLine>`, *amended January 2026*: the catalogue's wrapper, unwrapped to `list(filter)`). Events: this service appends `needs_you_changed { workspace_id }` through the Journal (Internal Logic H).

### Callers of the public interface (reconciliation §3)

| Item | Called by |
|---|---|
| Service API commands above | Service API catalogue (screens: handover draft, handover screen, lifecycle view, Home) |
| `subscribe` (`HandoverNotice`) | desktop program (notices, 11 §7) |
| `list` | program's `JournalReader` for Home and Search (`waiting_items`: waiting handovers) |
| `following` | program, filling the Authoring Service's `PublishContext` |
| `run` | program, at start |
| `migrations()`, `event_sink()` | program (Journal `MigrationRegistry::register`, `Journal::subscribe`) |

### Example

`handover.open` result (abridged): `{"handover":{"id":"0192a3d0-1111-7aaa-8bbb-cccc0000d001","title":"Northwind assessment","stage_name":"Analysis","sender":"priya.shah@acme.co.uk","receiver":{"name":"Sam Lee","address":"sam.lee@acme.co.uk","uses_ordin":true},"state":"seen","round":1,…},"marked_lines":[],"arrived":null,"still_under_way":[],"results":[{"label":"Supplier assessment","link":"https://acme.sharepoint.com/…/assessment.docx","cannot_open":null}],"actions":["accept_and_start","return_to","ask_ordin"]}`.

## Internal Logic

**A. Open a draft.**
1. Read the workspace's `workspace_stages` row (if any) and its lifecycle with `catalogue.lifecycle(id)`; `CatalogueError` code for an unknown id → `HAND_LIFECYCLE_UNKNOWN`.
2. Mint `DraftGroupId`; title = the workspace's name (≤ 120, trimmed). Results default to the given links.
3. `host.draft_note(workspace, results, stage)` → four sections with `drafted_by_ordin = true`; headings forced to the four fixed headings. A host failure leaves the four bodies empty and the draft opens anyway (*assumed here*).
4. Next stage: `StageNext::One` → that stage; `OneOf` → `choices` with `host.recommend_next_stage` first and marked; `SideBySide` → one receiver slot per stage; `Nothing` → `HAND_STAGE_IS_LAST` (the lifecycle ends; no handover from a last stage, *assumed here*). No lifecycle → one slot, no stage.
5. Suggested receiver per stage: the receiver of the last `sent` or `accepted` handover this person sent for that (lifecycle, stage).
6. Place: the next stage's `handover_place_id` (`catalogue.lifecycle_place(id)` → `LifecyclePlace { id, place_type, address }`, inserted into `handover_places` with the same id when new, `display_name` = the address's last path segment); no lifecycle → none until the person picks a place the workspace links: `workspaces.linked_repositories(ws)` gives Git places (`https://{host_name}/{repository}`, GitHub hosts only) and `workspaces.source_links(ws)` of kind `sharepoint_location` gives SharePoint places (their `origin_location` resolved with `sharepoint.resolve_origin` to its `web_url`).
7. Insert one `Handover` row per slot (`state = draft`, `round = 1`, same `draft_group_id`); run §B. (amended January 2026, ADR-102: each row also gets its `DraftSlackView`, §A.7 of the *Amendment January 2026* below)

**B. Check (fixed code, ADR-092 item 2; ADR-099 item 4).** On every change:
1. Result facts, composed here (*chosen here*, SA-RECON-07), one `HandoverResult { link, label = last path segment, .. }` per link:
   - A link that is not `https` (a local path or `file:` address) → `local_only = true`, which fails `results_readable` with the fix **Put it in [place]…**.
   - `github.com` → Git: `readable_by_receiver` = `Some(true)` when `github.reviewers(host, repository, receiver login)` lists the login, `Some(false)` when it does not, `None` without a login; a `.md` file → `github.file_at(host, repository, ref, path)` and its `#` heading lines as `headings`; other files → `headings = None`.
   - A SharePoint web address → `sharepoint.resolve_web_url(link)` then `sharepoint.audience(location)`: `link_scope` `Organization` or `Anonymous` → `Some(true)` for a receiver inside the company; otherwise `None` (the adapter's principals are directory ids, not addresses); a `.md` file → `item_at` + `fetch` and its `#` heading lines; other files → `headings = None`.
   - Any other `https` address (Confluence included) → `readable_by_receiver = None`, `headings = None`.
2. Contract: `HandoverContract { scope: SourceScope { home: RepositoryRef::Repository(lifecycle.agent_repository_id), profile: vec![], include_bundled: false }, identifier: stage.hands_over }` (*chosen here*), resolved by the Checker (this crate does not depend on the Agent Model; the types are the Checker's re-exports). Headings: **conditional on C-68** — Word and Confluence headings come from the Workspace Service's conversion; until C-68 is confirmed, a Word or Confluence result in a stage with a contract that returns `headings = None` fails `contract_sections` with the plain reason "ordin can't read this document's sections yet" and the fix **Ask Ordin to add it** (*assumed here*: C-68 states no fallback, and ADR-090 forbids waiving a check).
3. Receiver: Git place → the receiver must have a GitHub login with read access to the place (`github.reviewers(host, repository, login)` finds them); SharePoint place → the receiver is the email address the person typed (no directory search, ruling R17): it must be one address of RFC 5321 form, else `receiver_reachable` fails ("Type the person's email address.").
4. `checker.check_handover(&input)` → findings (`CheckError` → `HAND_STORE_FAILED` when retryable is false, else the draft keeps its previous findings and `can_hand_over = false`, *chosen here*); `checks_passed` = codes passed; `can_hand_over` = report `passed` ∧ a place is set ∧ every slot has a receiver ∧ (at a choice) a stage is chosen. Nothing can be waived (ADR-090). A handover outside any lifecycle has no contract check.

**C. Hand over (approval and write).**
1. `can_hand_over` false → `HAND_CHECKS_FAILING`.
2. `uses_ordin`: true when an earlier record shows an event written by the receiver; otherwise false for SharePoint places (the email is the notice) and true for Git places (GitHub's review request is the notice) (*assumed here*: the HLD gives no way to know; ADR-092 item 3).
3. One card for the whole draft group, `approvals.request(ApprovalRequest { workspace_id, asked_by: ApprovalAsker::Person, .., reject_style: RejectStyle::NotNow, show_details: true, proposal: ActionProposal { action: "hand over", destination: place address, affects, consequence: changes }, asker: None, leaves_company: false })`: question "Hand "[title]" over to [name]?" (side by side: "Hand "[title]" over to [n] people?"), verb "Hand over", where the place's display name (each stage's place, side by side), affects one `Recipient { name, address, outside_company }` per receiver plus each stage, changes "[name] is asked to take this over. They see the note and the results; nothing else in this workspace.", content `Message { label: "See what will be sent", text: handover.md, destination_note }` (ADR-092 item 2, UX-A53). Store `card.approval_id` on each row. (amended January 2026, ADR-102: Slack posts are prepared before the card and listed on it, steps 3a–3c of the *Amendment January 2026* below)
4. On `approval_decided` = approved, per handover (side by side: `sent_with` names the others and the meeting stage, if any, where the stages' `next` lead to one stage): (amended January 2026, ADR-102: once `sent`, the prepared Slack posts are sent, §C.4 of the *Amendment January 2026* below)
   - Git: render the files with the `sent` event; `push.send_approved_pull_request(ApprovedPullRequest { approval_id, proposal: PullRequestProposal { origin: SubmissionOrigin::Handover { id }, content: PullRequestContent::Files { files }, base: github.default_branch(host, repository), branch as above, title: "Handover: [title]", body: handover.md, labels: ["ordin-handover"], reviewers: [receiver github_login], existing: None, card: the card's text } })`. The Push Service records the external action (03 *Hand work over*: "the Push Service's unique key stops a second pull request").
   - SharePoint: `folder = sharepoint.create_folder(&place_location, folder name, ExternalWrite { key: k1, run_id: None, kind: HandoverWritten })`, then `create_file(&folder, "handover.md", …, ExternalWrite { key: k2, .. })` and `create_file(&folder, "handover.yaml", …, ExternalWrite { key: k3, .. })`, each key a fresh UUIDv7; the adapter begins and confirms each external action (SA-RECON-04). `SP_NAME_EXISTS` → the folder name gets " (2)", " (3)" until free.
   - `uses_ordin = false` → `mail.send_under_approval(NewDraft { workspace_id, kind: EmailDraftKind::New, answers: None, to: [receiver address], cc: [], bcc: [], subject: "Handover: [title]", body: note + links, then "Open in ordin: {ordin_link}" and "Where it is: {place_link}" (amended January 2026, ADR-102; the handover link of the *Amendment January 2026* below), attachments: [] }, approval_id, None)`.
   - `state = sent`, event `sent` appended locally; journal `handover.sent`.
5. A refused write (`SP_ACCESS_DENIED`, `SP_NOT_FOUND`, `PUSH_NO_REVIEW_RIGHT`, `PUSH_FORBIDDEN`, `GH_NOT_FOUND`) → the draft stays `draft`, nothing is recorded as sent, notice `SendFailed` with `HAND_PLACE_REFUSED` (03 error posture). Rejected or expired card → stays `draft`.
6. Push notices and resubmission (decision E3). `run` reads `push.subscribe()` and keeps only notices whose `origin` is `SubmissionOrigin::Handover { id }` for a local handover in `draft` or `sent`:
   - `PullRequestOpened { pull_request, .. }` for a handover still in `draft` (the Push Service recovered the submission after a restart, PushService.md §G.3) → store `pull_request`, append the `sent` event, `state = sent`; journal `handover.sent`.
   - `PullRequestFailed { code, .. }` (the Push Service could not send the files, or a restart lost them before the send, PushService.md Data Model) → the handover is set back to `draft` with nothing recorded as sent, and notice `SendFailed { draft, code }` with the Push code mapped as in the Error Table (an unmapped code → `HAND_PLACE_UNREACHABLE`). The draft screen offers **Try again**, which calls `hand_over(draft)`: §C.1–C.4 run again with a new approval card and the files rendered again from the local row, so the Push Service never resends the earlier submission on its own.

**D. Receiving (A-43).** At start, on every `workspaces.opened()`, every 15 minutes and on `check_now`:
1. Git: `github.review_requests("ordin-handover")` (≤ 50 open, A-43); for each pull request, `file_at(head, …/handover.yaml)`; parse; the receiver's address or login must be the person's (`identity.person()` → `PersonProfile.email`, `.github_login`), else skip.
2. SharePoint: for every place of a stage the person's workspaces follow and every place their workspaces link, `sharepoint.list_children(&place_location, cursor)` (folders only, every page) and, for each folder new since the stored list or whose `handover.yaml` `RemoteVersion.tag` differs from `record_version`, `item_at(&place_location, "<folder>/handover.yaml")` + `fetch`; receiver must be the person. (amended January 2026, ADR-102: and every place in `followed_handover_places`, §L of the *Amendment January 2026* below)
3. Upsert a local `Handover` (receiver side, `direction = handed_to_you`) with the record's fields and events; a new one → notice `HandedToYou` ("Handed to you: [title] — from [name]"). A record whose last event is `taken_back` → `taken_back` (the screen then offers only **Close**). (amended January 2026, ADR-102: the record's `slack_messages` are merged into the local row by `(notice, permalink)`)
4. Sender side, for each `sent`/`seen` handover this person sent: re-read the record and adopt events appended by the receiver; for a receiver without ordin in Git, read the pull request's review state as the answer (approved or merged → `accepted`; changes requested → `sent_back` with the review body as the reason; 08 *GitHub review requests*). Notices `Answered` / `ReturnedToYou`.
5. No answer in time (ADR-092 item 7, amended by ADR-102): `days_to_answer > 0` and no answer `days_to_answer` days after `sent` → notice `NoAnswer` once. `remind(id)`:
   a. The handover's receiving stage has `slack_channel` → §S2 with `notice = Reminder`. `covered_by_rule` is `Some` → §S3 now. Otherwise `slack.ask_to_post(post_id, from_workspace_id)` — the Slack Service's own card ("Post in #{name}?"); its decision is the Slack Service's, and the post is sent by it on **Post**. Insert `handover_slack_posts`.
   b. No stage channel, or §S2 refused → `mail.draft_for_person(NewDraft { to: [receiver address], subject: "Handover: [title]", body: a reminder with the note's link, the ordin link and the place link, .. })`; the person reviews and sends that draft through the Mail Service's own card (SA-RECON-06), as before.
   `remind` returns `Ok(())` in both cases; the screen shows which one was used.
6. Every record read is data; nothing in it is followed (R-81).

**E. Appending an event (A-44, R-83).** Read the file with its version (Git: blob SHA on the pull request branch; SharePoint: file version); append exactly one event at the end; write conditionally — Git: `begin_external_action(HandoverWritten, "<repository>/<path>")`, `github.update_file(host, repository, branch, path, bytes, expected_sha, message)`, `confirm_external_action`; SharePoint: `sharepoint.upload_version(&item, bytes, expected_version = tag read, ExternalWrite { key, run_id: None, kind: HandoverWritten })`. A conflict (`GH_CONFLICT`, `SP_ORIGIN_CHANGED`) → re-read and retry, at most 3 times; then `HAND_PLACE_BUSY` and the screen shows what happened first. An event already present (same kind, by, round) is not appended twice. (amended January 2026, ADR-102: step E3 appends a Slack message to the record, §E3 of the *Amendment January 2026* below)

**E2. After a restart.** At `run` start, after the program's `Journal::recover_external_actions`: `journal.unsettled_external_actions(&[HandoverWritten])`. For each action whose `destination` (`<repository>/<path>`) names the Git record of a local handover (the GitHub writes this service brackets itself, SA-RECON-04): re-read that record as §D.4 does. The event the action was writing is present → the local row adopts it and `handover.recovered_arrived` is logged; the Journal row stays `unknown_outcome` for the person's **It arrived — mark as done**. The event is absent → the local state is left as it was before the action, `handover.unknown_outcome` is logged, and the screen offers the same action again. Nothing is written again automatically. Actions whose destination names no local Git record (the SharePoint writes, which the SharePoint Adapter brackets) are left to their owner.

**F. Open, accept, return.**
1. `open`: append `seen` (first open only) through §E; `state = seen`. Mark note lines that read as instructions to an assistant (case-insensitive phrases: "ignore previous", "ignore the above", "you are now", "system prompt", "as an ai", "disregard", "run this command", "execute") (*chosen here*; the marking only labels, it never removes, R-81). Results that cannot be opened are listed "Can't be opened: [reason]". At a meeting stage, `arrived` counts the handovers of the same `sent_with` group already here.
2. `accept_and_start` (`seen` only, else `HAND_STATE_INVALID`): append `accepted` (§E); Git: `github.review(host, repository, number, ReviewVerdict::Approve, "")` then `github.merge(…)`, each inside its own `HandoverWritten` external action (SA-RECON-04), where the repository allows it (a refusal to merge leaves the pull request open; the acceptance still stands); create the workspace (`AcceptTarget::NewWorkspace` → `workspaces.create(name)?.id`) or use the chosen one; `follow_stage` (the handover's stage); link inputs: each SharePoint result → `workspaces.add_link(ws, LinkLocation::Sharepoint { location }, None)`; each Git result → `add_link(ws, LinkLocation::GitRemote { url, branch, path }, None)` by the Git address rule of SA-CLOSE-01; Confluence and other results stay links in the note (SA-CLOSE-01); link the place: a SharePoint place → `add_link` the same way, a Git place → `add_link(ws, LinkLocation::GitRemote { .. }, Some(display_name))` (SA-CLOSE-01); a `WorkspaceError` on a link is logged `handover.link_failed {code}` and the acceptance continues; `host.start_from_handover(workspace, note, stage agent)` — the agent is offered, asking before installing (ADR-012). `accepted_into_workspace_id` set; journal `handover.accepted`. (amended January 2026, ADR-102: with a stage Slack channel, an *Accepted* notice follows, §F.2 of the *Amendment January 2026* below)
3. `return_options`: the declared send-backs of the current stage first, each with its round ("round 2 of 3"), the round counted from `send_back_round` events of earlier rounds in the record; then the sender; in a lifecycle that branches, every earlier stage on the way and whoever did it, read from the records (ADR-099 item 5).
4. `return_to` (`sent` or `seen`; reason 1–1000 → else `HAND_REASON_MISSING`): card naming the sender (and the earlier person when it goes back past the sender: affects both, ADR-099 item 5), question "Return "[title]" to [name]?", verb "Return", changes the reason; past `max_rounds` the card adds "round [n]; the lifecycle allows [m]" and the event carries `beyond_agreed_rounds: true` — never blocked. On approval: append `sent_back` with reason, `returned_to`, `send_back_round` (§E); Git: `github.review(…, ReviewVerdict::RequestChanges, reason)` inside a `HandoverWritten` external action; `state = sent_back`, and `beyond_agreed_rounds` set from the event. (amended January 2026, ADR-102: with a stage Slack channel, a *Returned* notice rides on the card, §F.4 of the *Amendment January 2026* below)
5. Sender: `change_and_hand_over_again` (`sent_back` only) → `state = draft`, `round + 1` (declared send-backs past the limit are marked with `beyond_agreed_rounds`, never refused — 2C C-21); the record keeps every round.
6. `take_back` (`draft`, `sent`, `seen`, `sent_back`; else `HAND_ALREADY_ACCEPTED`): first, when the handover's `approval_id` names a card still pending (`approvals.get(approval_id)?.state` is `Pending`: the hand-over card of C.3 not yet decided), it is withdrawn with `approvals.withdraw(approval_id)` (SA-CLOSE-02); the card covers the whole draft group, so every handover of the group stays a draft; `APPR_CARD_ALREADY_DECIDED` → the person decided first: re-read the handover and continue with its current state. Then a draft is deleted locally without a card; otherwise a card ("Take back "[title]" from [name]?", verb "Take back") (*assumed here*: it changes what another person sees, G8), then append `taken_back` and, in Git, `github.close(host, repository, number)` inside a `HandoverWritten` external action. (amended January 2026, ADR-102: with a stage Slack channel, a *Taken back* notice rides on the card, §F.6 of the *Amendment January 2026* below)

**G0. Finding people (`find_people`).** `query` trimmed, 1–100 characters (else `HAND_QUERY_INVALID`). Git place → `github.reviewers(host, repository, query)`, each as a `PersonChoice` with its login. SharePoint place, or no place yet → no directory is read (ruling R17): a `query` of email-address form returns one `PersonChoice { name: query, address: query, github_login: None, outside_company }` with `outside_company` true when its domain is not one of the organisation's domains; any other query returns an empty list and the screen asks for the email address.

**G. Stages and lifecycles.**
1. `follow_stage`: the lifecycle and stage must exist (`HAND_LIFECYCLE_UNKNOWN`, `HAND_STAGE_UNKNOWN`); one row per workspace (`LIFE_WORKSPACE_FOLLOWS_TWO` replaced by an update — following another stage replaces the row, *assumed here*). (amended January 2026, ADR-102: following a stage whose place is in `followed_handover_places` deletes that row, §G.1 of the *Amendment January 2026* below)
2. `start_workspace_for_stage` (ADR-095 item 1): `workspaces.create(name)?.id`; `follow_stage`; link the place as in F.2 for receiving — an unreachable place does not stop the start (03 *Start a workspace for a stage* error posture: created without the link); `host.start_from_handover(workspace, empty note, stage agent)`. `stage_of` returns **What this stage hands on** from the contract's sections: `catalogue.find_part(lifecycle.agent_repository_id, PartKind::OutputContract, hands_over, None)` then `read_part(id)` and the section names of its `body`.
3. `lifecycle_view`: the Catalogue's lifecycle with, per stage, the handovers this person can read in that stage's place (local rows plus records read in §D), and "You are here" for stages the person's workspaces follow. It is not a shared board.
4. `following(lifecycle_id)`: `workspace_stages` rows of that lifecycle → their workspaces' display names (`workspaces.get(id)?.name`), sorted; `open_handovers` = local `handovers` rows of that lifecycle in `draft`, `sent`, `seen` or `sent_back`. An unknown lifecycle returns an empty list and 0.

**H. Needs-you events (reconciliation row 455).** When a handover enters or leaves a state that waits for the person — notices `HandedToYou`, `ReturnedToYou`, `NoAnswer`, and the person's `accept_and_start`, `return_to`, `take_back` or close of such a handover — append `journal.append_event("needs_you_changed", {"workspace_id": id})` for the workspace concerned (the accepting or sending workspace; a handed-to-you handover not yet accepted uses the workspace following the stage, or none, in which case the event carries the person's most recently opened workspace — *assumed here*). A failed append is logged and changes nothing else.

**S. Slack notices** and **L. Handover links** (amended January 2026, ADR-102): sections S and L of the *Amendment January 2026* below.

## Data Model

Module `handover`, journal database. Not in the agent profile, so no PostgreSQL variant.

```sql
-- migrations(): module `handover`, step (1, "handover tables", <this SQL>)
CREATE TABLE handover_places (
  id TEXT PRIMARY KEY,
  place_type TEXT NOT NULL CHECK (place_type IN ('git','sharepoint')),
  address TEXT NOT NULL UNIQUE CHECK (length(address) <= 2048),
  display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120)
);

CREATE TABLE handovers (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
  lifecycle_id TEXT, stage_name TEXT,
  sender TEXT NOT NULL,
  receiver TEXT NOT NULL,                    -- JSON HandoverReceiver
  results TEXT NOT NULL,                     -- JSON Vec<String>
  note TEXT NOT NULL,                        -- JSON HandoverNote
  checks_passed TEXT NOT NULL,
  handover_place_id TEXT NOT NULL REFERENCES handover_places(id),
  round INTEGER NOT NULL CHECK (round >= 1),
  events TEXT NOT NULL,                      -- JSON Vec<HandoverEvent>, append-only
  state TEXT NOT NULL CHECK (state IN ('draft','sent','seen','accepted','sent_back','taken_back')),
  from_workspace_id TEXT, accepted_into_workspace_id TEXT,
  draft_group_id TEXT NOT NULL,              -- chosen here: side-by-side handovers share it
  direction TEXT NOT NULL CHECK (direction IN ('handed_over','handed_to_you')),   -- chosen here
  beyond_agreed_rounds INTEGER NOT NULL DEFAULT 0,   -- 2C C-21
  approval_id TEXT,                          -- chosen here
  next_stage TEXT, sent_with TEXT,           -- chosen here: ADR-099 fields of the record
  pull_request TEXT,                         -- chosen here: JSON {host, repository, number, branch}
  place_folder TEXT,                         -- chosen here: SharePoint folder name
  record_version TEXT,                       -- chosen here: last blob sha or file version read
  sent_at TEXT, no_answer_notified INTEGER NOT NULL DEFAULT 0,
  CHECK ((lifecycle_id IS NULL) = (stage_name IS NULL))
);
CREATE INDEX ix_handovers_state ON handovers(direction, state);
CREATE INDEX ix_handovers_group ON handovers(draft_group_id);

CREATE TABLE workspace_stages (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL UNIQUE,
  lifecycle_id TEXT NOT NULL,
  stage_name TEXT NOT NULL,
  set_at TEXT NOT NULL
);

CREATE TABLE sharepoint_place_reads (        -- chosen here: last folder list per place
  handover_place_id TEXT PRIMARY KEY REFERENCES handover_places(id) ON DELETE CASCADE,
  folders TEXT NOT NULL, read_at TEXT NOT NULL
);
```

The local `events` column mirrors the place's record; the place stays the record shared by both people (04 Data Lifecycle). Rollback: forward-only; no destructive change.

Migration step 2 (amended January 2026, ADR-102) adds four columns to `handovers` and the tables `handover_slack_posts` and `followed_handover_places`: *Data Model* of the *Amendment January 2026* below.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `find_people` query empty or over 100 characters | `Err(permanent)` | `HAND_QUERY_INVALID` | `{"error":"Type a name or an email address.","code":"HAND_QUERY_INVALID","action":null}` |
| Title empty or over 120 chars | `Err(permanent)` | `HAND_TITLE_INVALID` | `{"error":"Give the handover a title of up to 120 characters.","code":"HAND_TITLE_INVALID","action":null}` |
| Receiver address invalid | `Err(permanent)` | `HAND_RECEIVER_INVALID` | `{"error":"That isn't an address ordin can reach.","code":"HAND_RECEIVER_INVALID","action":null}` |
| Note section over 4000 chars | `Err(permanent)` | `HAND_NOTE_TOO_LONG` | `{"error":"This part of the note is too long.","code":"HAND_NOTE_TOO_LONG","action":null}` |
| More than 50 results | `Err(permanent)` | `HAND_RESULTS_TOO_MANY` | `{"error":"Hand over at most 50 results.","code":"HAND_RESULTS_TOO_MANY","action":null}` |
| A check fails | `Err(permanent)` | `HAND_CHECKS_FAILING` | screens show *Checked by ordin* |
| Lifecycle or stage unknown | `Err(permanent)` | `HAND_LIFECYCLE_UNKNOWN` / `HAND_STAGE_UNKNOWN` | `{"error":"That lifecycle is no longer in the library.","code":"HAND_LIFECYCLE_UNKNOWN","action":null}` |
| Handing over from a last stage | `Err(permanent)` | `HAND_STAGE_IS_LAST` | `{"error":"This is the last stage of the lifecycle.","code":"HAND_STAGE_IS_LAST","action":null}` |
| Action not allowed in the state | `Err(permanent)` | `HAND_STATE_INVALID` | `{"error":"This handover has already been answered.","code":"HAND_STATE_INVALID","action":null}` |
| Take back after acceptance | `Err(permanent)` | `HAND_ALREADY_ACCEPTED` | `{"error":"It has been accepted and can't be taken back.","code":"HAND_ALREADY_ACCEPTED","action":null}` |
| Return without a reason | `Err(permanent)` | `HAND_REASON_MISSING` | `{"error":"Say why you're returning it.","code":"HAND_REASON_MISSING","action":null}` |
| The place refuses the write | outcome | `HAND_PLACE_REFUSED` | `{"error":"The handover place didn't accept it; the draft is still yours.","code":"HAND_PLACE_REFUSED","action":null}` |
| Conditional write keeps conflicting | `Err(retryable)` | `HAND_PLACE_BUSY` | `{"error":"Someone else answered at the same moment.","code":"HAND_PLACE_BUSY","action":"try_again"}` |
| Sign-in for the place expired | `Err(permanent)` | `HAND_SIGN_IN_NEEDED` | `{"error":"Sign in again to reach the handover place.","code":"HAND_SIGN_IN_NEEDED","action":"sign_in_again"}` |
| Place unreachable | `Err(retryable)` | `HAND_PLACE_UNREACHABLE` | `{"error":"Couldn't reach the handover place.","code":"HAND_PLACE_UNREACHABLE","action":"try_again"}` |
| Record unreadable (`record_schema` other; the restricted reader's `YAML_FEATURE_REFUSED`, `YAML_MALFORMED` or `YAML_TOO_LARGE`) | outcome | `HAND_RECORD_UNREADABLE` | Diagnostics; the item is not shown |
| Unknown handover | `Err(permanent)` | `HAND_UNKNOWN` | `{"error":"That handover is no longer here.","code":"HAND_UNKNOWN","action":null}` |
| Datastore failure | `Err(retryable)` | `HAND_STORE_FAILED` | `{"error":"Something went wrong saving the handover.","code":"HAND_STORE_FAILED","action":"try_again"}` |

Provider errors map as follows: `GH_SIGN_IN_NEEDED`, `SP_SIGN_IN_NEEDED`, `PUSH_SIGN_IN_NEEDED` → `HAND_SIGN_IN_NEEDED`; `GH_NOT_FOUND`, `GH_FAILED`, `SP_ACCESS_DENIED`, `SP_NOT_FOUND`, `PUSH_NO_REVIEW_RIGHT`, `PUSH_FORBIDDEN` → `HAND_PLACE_REFUSED`; `GH_RATE_LIMITED`, `GH_UNREACHABLE`, `SP_THROTTLED`, `SP_UNAVAILABLE`, `SP_OUTCOME_UNKNOWN` → `HAND_PLACE_UNREACHABLE`; `GH_CONFLICT`, `SP_ORIGIN_CHANGED` → retry (Internal Logic E), then `HAND_PLACE_BUSY`; `JournalError` and `ApprovalError` with retryable true → `HAND_STORE_FAILED`.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Handover Service

  Scenario: Happy path — hand over through Git
    Given a workspace following stage "Analysis" whose next stage "Design" uses a Git handover place
    And every result is in a place Sam Lee can read and holds the contract's sections
    When Priya opens a handover draft, chooses Sam Lee and approves the card
    Then a branch with handover.md and handover.yaml and a pull request labelled ordin-handover requesting Sam's review exist
    And the handover is sent

  Scenario: A result only on this computer
    Given one result exists only on Priya's computer
    When the draft is checked
    Then results_readable fails with the fix Put it in the handover place and Hand over is unavailable

  Scenario: Receiver address not in email form
    Given a draft whose place is a SharePoint folder
    When the receiver is typed as "Procurement team"
    Then receiver_reachable fails with "Type the person's email address."

  Scenario: Receiving and accepting
    Given a pull request requesting Sam's review with label ordin-handover
    When Sam's ordin checks for handovers
    Then "Handed to you: Northwind assessment — from Priya Shah" is notified
    When Sam opens it
    Then a seen event by Sam is appended to handover.yaml
    When Sam accepts into a new workspace
    Then an accepted event is appended, the pull request is approved, and the new workspace follows "Design" with the results linked

  Scenario: Return to an earlier stage with declared rounds
    Given stage "Review" declares a send-back to "Analysis" at most 3 rounds and this is the third return
    When Sam returns it with reason "Payment terms are missing"
    Then the card says round 4; the lifecycle allows 3
    And after approval the sent_back event has beyond_agreed_rounds true

  Scenario: Two installations write at once (SharePoint)
    Given the receiver accepts while the sender takes it back
    When both append to handover.yaml
    Then one conditional write is refused and that side re-reads and retries from the file as it now reads

  Scenario: Side by side
    Given stage "Assessment" leads to "Security" and "Legal" side by side
    When Priya hands over to two people on one card
    Then two handovers are written, each naming the other in sent_with and the meeting stage

  Scenario: Taking back a draft withdraws its pending hand-over card (SA-CLOSE-02)
    Given a draft handover whose hand-over card is pending
    When Priya takes it back
    Then withdraw is called for that card before the draft is deleted
    And the card's state is expired

  Scenario: Taken back before it was opened
    Given a sent handover not yet seen
    When Priya takes it back after its card
    Then a taken_back event is appended and the pull request is closed
    And Sam's screen offers only Close

  Scenario: No answer in time
    Given a stage with days_to_answer 5 and a handover sent 6 days ago with no answer
    When the 15-minute check runs
    Then NoAnswer is sent once

  Scenario: The place refuses the write
    Given Priya has no write access to the SharePoint place
    When the approved handover is written
    Then the draft stays a draft and SendFailed carries HAND_PLACE_REFUSED

  Scenario: A failed pull request is resubmitted with Try again
    Given an approved Git handover whose files the Push Service could not send
    When the Push Service sends PullRequestFailed with origin Handover for it
    Then the handover is a draft again and SendFailed is notified
    When Priya presses Try again
    Then a new approval card is raised and, once approved, the files are sent again

  Scenario: A Git write left unsettled by a restart
    Given ordin stopped after an accepted event was written to handover.yaml but before the action was confirmed
    When the Handover Service starts
    Then the record is re-read, the local handover adopts the accepted event and nothing is written again

  Scenario: Word result before C-68 is confirmed
    Given a stage with a contract and a Word result whose headings cannot be read
    When the draft is checked
    Then contract_sections fails with the fix Ask Ordin to add it

  Scenario: A SharePoint receiver is the typed email address (R17)
    Given a draft whose place is a SharePoint folder
    When find_people is called with "sam.lee@acme.co.uk"
    Then one PersonChoice with that address is returned and no directory is searched
    And find_people with "Sam" returns no choices

  Scenario: A handover handed to the person is announced as needing them
    Given a new handover.yaml names the person as receiver
    When the receiving check runs
    Then HandedToYou is sent and the Journal receives needs_you_changed

  Scenario: Following a lifecycle is counted for publishing
    Given two workspaces follow stages of lifecycle L and one handover of L is sent
    When following(L) is called
    Then it returns both workspace names and open_handovers 1

  Scenario: Accepting a handover links its Git place without a clone
    Given a handover whose place is "https://github.com/acme/northwind/tree/main/handover"
    When the receiver accepts it into a new workspace
    Then the workspace gains a GitRemote link with url "https://github.com/acme/northwind", branch "main" and path "handover"
    And no folder is cloned
```

## Performance, Security, Observability

- **Performance targets:** one receiving check across 50 Git handovers and 20 SharePoint places ≤ 30 s, in the background (*chosen here*; one GitHub search call per check, A-43); `open` ≤ 500 ms p95 from the local mirror plus one conditional write; draft checks ≤ 2 s p95 for 50 results excluding the Workspace Service's conversions (*chosen here*); memory ≤ 32 MiB.
- **Security:** only the person sends, returns or takes back a handover, through its approval card; agents and crews may prepare a draft and stop (ADR-092 item 9); the note and results from another person are data, marked where a line reads as an instruction, and given to agents only after the receiver accepts (R-81); events are append-only and each installation writes only its own, conditionally (R-83); the record holds no secret and no content beyond the note and links; place addresses are validated `https` URLs ≤ 2048 bytes.
- **Observability (D, P; logs only):** `component = "handover"`; `handover.draft_opened` {draft_group_id, lifecycle_id, stage}; `handover.checked` {draft_group_id, passed, failed_codes}; `handover.sent` {handover_id, place_type, result}; `handover.received` {handover_id, place_type}; `handover.event_appended` {handover_id, kind, retries}; `handover.accepted` {handover_id, workspace_id}; `handover.returned` {handover_id, round, beyond_agreed_rounds}; `handover.check_run` {git_found, sharepoint_found, duration_ms}. Never logged: note text, reasons, result links, receiver addresses (the handover id identifies them).

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

> Source: decision 82; HLD revision 0.50.0, ADR-102 (amends ADR-092 items 2, 4 and 7; settles OQ-55 with option (a)).
> Every rule below adds to the sections above, and each step it extends names it in place; step D.5 is quoted here as it reads above. Behaviour without Slack is unchanged: with no Slack sign-in, a stage without a Slack channel and *Also tell them in Slack* off, every step runs exactly as it does without Slack and no Slack Service call reaches Slack.

### Purpose (addition)

When the sender is signed in to Slack, the Handover Service also tells people in Slack, as the sender: a Slack direct message to the receiver on the hand-over card, and one message in the Slack channel a lifecycle stage names for each of the handover's moments — handed over, accepted, returned, taken back and the overdue reminder — mentioning the other person. Every Slack message carries a link that opens the handover in the receiver's ordin and, on a computer without ordin, the handover's place. Opening that link follows the place when the receiver's ordin does not follow it yet (OQ-55 (a)). Slack is additive: whenever Slack cannot carry a notice, the email or GitHub review request of today is the notice.

### Approach (addition)

The Handover Service writes the words of each Slack notice and decides when it is due; the Slack Service owns the post, its approval binding, the rules and the Slack Web API. A Slack post rides on the card this service raises anyway (hand over, return, take back) and is sent only after this service's write to the handover place succeeded, so Slack never announces a handover that was not written. Actions without a card of their own (accept, remind) post under the person's rule for that stage's channel, or the Slack Service's own card. Rejected: *Slack posts as a separate card after each action* — the person would approve twice for one moment (ADR-005 asks once per action); *posting before the place write* — a refused write would leave a false notice in the channel.

### Shared Context (additions)

```rust
// 2C domain (amended January 2026) — copied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StageSlackChannel {
    pub channel_id: String,   // ^[CG][A-Z0-9]{8,20}$
    pub name: String,         // 1–80, without '#'
}
// LifecycleStage also has (2C domain, amended):
//   #[serde(skip_serializing_if = "Option::is_none")] pub slack_channel: Option<StageSlackChannel>,

// Handover also has (2C domain, amended): `pub slack_messages: Vec<HandoverSlackMessage>` (default empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HandoverSlackMessage {
    pub notice: HandoverNoticeKind,
    pub permalink: String,    // https://<domain>.slack.com/archives/<channel_id>/p<ts without the dot>, ≤ 2048
    pub by: String,           // the address of the person who posted
    pub at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoverNoticeKind { HandedOver, Accepted, Returned, TakenBack, Reminder }

// uuid newtypes added: SlackPostId, SlackRuleId (2C domain).
```

**Record file (addition to *Files in the handover place*).** `handover.yaml` has one optional top-level key, appended by the installation that posted (record schema stays `ordin.handover/v1`; older readers keep and ignore it, as the parsing rules above state):

```yaml
slack_messages:
  - { notice: handed_over, permalink: "https://acme.slack.com/archives/C04ABCDEF12/p1761037200000100", by: priya.shah@acme.co.uk, at: "2026-01-10T09:00:05.000Z" }
```

Each entry is validated on read: `notice` one of the five kinds; `permalink` an `https` URL whose host ends `.slack.com`, ≤ 2048 bytes; `by` an RFC 5321 address; `at` RFC 3339. An invalid entry is dropped and logged `handover.slack_entry_ignored`; the record stays readable. Entries are append-only and each installation appends only its own (R-83).

**The handover link** (*fixed here*). Every Slack notice and the email to a receiver without ordin carry:

```
ordin://handover/open?h=<HandoverId>&t=<git|sharepoint>&p=<percent-encoded handover place address>
```

`p` is the handover place's `address` (`handover_places.address`). Beside it, each message carries the place link a browser opens without ordin: for a SharePoint place, the place address itself; for a Git place, the folder of the handover files on its branch, `https://github.com/<owner>/<repo>/tree/<branch>/handovers/<YYYY-MM-DD>-<slug>` (*chosen here*: the pull request's number is not known when the card freezes the message text; GitHub shows the open pull request on that branch's page).

**Message words** (message catalogue ids; en-GB; filled by fixed code; each ≤ 600 characters after filling; never contains the note, the results or a send-back reason the person left out):

| Id | Text (`{…}` filled; `<mention:addr|Name>` is resolved by the Slack Service) |
|---|---|
| `handover-slack-direct` | `I've handed "{title}" over to you in ordin. <{ordin_link}\|Open in ordin> · <{place_link}\|Where it is>` |
| `handover-slack-handed-over` | `<mention:{receiver_address}\|{receiver_name}> I've handed "{title}" over to you ({stage}). <{ordin_link}\|Open in ordin> · <{place_link}\|Where it is>` |
| `handover-slack-accepted` | `<mention:{sender_address}\|{sender_name}> I've accepted "{title}" and started on it. <{ordin_link}\|Open in ordin>` |
| `handover-slack-returned` | `<mention:{to_address}\|{to_name}> I've returned "{title}" to you.{ reason_line} <{ordin_link}\|Open in ordin>` — `reason_line` is ` Reason: {reason}` (reason cut to 300 characters) only when the person kept it |
| `handover-slack-taken-back` | `<mention:{receiver_address}\|{receiver_name}> I've taken "{title}" back; there's nothing for you to do.` |
| `handover-slack-reminder` | `<mention:{receiver_address}\|{receiver_name}> A reminder: "{title}" has been waiting for you for {days} days. <{ordin_link}\|Open in ordin> · <{place_link}\|Where it is>` |

The mention token `<mention:address|Name>` is *chosen here* and is passed in `SlackPostRequest.text`; the Slack Service turns it into `<@member>` when it finds the member and into the plain name otherwise (Slack Service spec).

**Boundary interface — Slack Service** (`ordin-slack-service`, SlackService.md; each provider's current signature):

```rust
// ordin-slack-service — `SlackService` (direct edge, build phase 7):
//   reach(&self, address: &str) -> Result<SlackReach, SlackError>;
//   prepare_post(&self, req: SlackPostRequest) -> Result<PreparedSlackPost, SlackError>;
//   bind_approval(&self, post_id: SlackPostId, approval_id: ApprovalId) -> Result<(), SlackError>;
//   send(&self, post_id: SlackPostId) -> Result<SlackPostOutcome, SlackError>;
//   ask_to_post(&self, post_id: SlackPostId, workspace_id: WorkspaceId) -> Result<ApprovalId, SlackError>;
//   post_record(&self, post_id: SlackPostId) -> Result<SlackPost, SlackError>;
//   rule_for(&self, key: &SlackRuleKey, kind: SlackRuleKind) -> Result<Option<SlackRuleId>, SlackError>;   // chosen here: read-only, no post
// SlackPostRequest { target: SlackTarget, thread_ts: Option<String>, text, origin: SlackPostOrigin, rule_key: Option<SlackRuleKey> }
// SlackTarget::{Channel { channel_id, channel_name }, PersonByAddress { address, person_name }}
// SlackPostOrigin::Handover { handover_id, notice: HandoverNoticeKind }
// SlackRuleKey::Stage { lifecycle_id, stage_name }
// PreparedSlackPost { post_id, destination_line, text_as_posted, covered_by_rule: Option<SlackRuleId> }
// SlackReach::{Found, NotFound, NotSignedIn, ScopeNeeded, NotAllowed}
// SlackPostOutcome::{Posted { ts, permalink }, Refused { code }, UnknownOutcome}
// SlackPost { id, destination, state: SlackPostState, permalink, refusal_code, .. }
// errors `SlackError { code: &'static str (SLK_*), retryable }`.
// Required of the Slack Service (interaction): `reach` and `prepare_post` answer without any Slack HTTP request when
// there is no Slack sign-in, when the profile or the server switches Slack off, and when the stage channel is not in
// `slack.allowed_channels`; this keeps "no Slack HTTP request" true for a person who never connected Slack.
```

### Public Interface (additions)

```rust
pub struct HandoverDeps {
    // … every field above, unchanged …
    pub slack: std::sync::Arc<ordin_slack_service::SlackService>,
}

impl HandoverService {
    /// `handover.open_link` — a handover link opened from Slack, an email or a paste (OQ-55 (a)).
    pub async fn open_link(&self, link: &str) -> Result<LinkOpened, HandoverError>;
}

pub struct UpdateDraftInput {
    // … every field above, unchanged …
    /// *Also tell them in Slack* (per draft group: applies to every receiver Slack can reach). None = unchanged.
    pub tell_in_slack: Option<bool>,
}
pub struct PersonChoice {
    // … every field above, unchanged …
    /// Filled by `find_people` for a choice with an email address (at most 5 per call); None otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack: Option<SlackReach>,
}
pub struct HandoverDraftView {
    // … every field above, unchanged …
    /// One per handover of the draft group, in the order of `handovers` (*chosen here*: a side-by-side draft has
    /// several receivers, so `DraftSlackView` is a list here, each naming its handover).
    pub slack: Vec<DraftSlackView>,
}
pub struct DraftSlackView {
    pub handover_id: HandoverId,                  // chosen here
    pub reach: SlackReach,                        // the receiver, by address
    pub tell_in_slack: bool,
    pub stage_channel: Option<StageSlackChannel>, // the receiving stage's channel
    pub stage_rule: Option<SlackRuleId>,          // the person's rule for that channel, when on
    /// The sentence the draft screen shows under the receiver (message catalogue id `handover-slack-reach-*`).
    pub line: String,
}
pub struct ReturnInput {
    // … every field above, unchanged …
    /// Whether the stage-channel message carries the reason; default true.
    #[serde(default = "yes")]
    pub reason_in_slack: bool,
}
pub struct HandoverScreen {
    // … every field above, unchanged …
    /// Slack messages about this handover: those this installation posted, and the record's `slack_messages`.
    pub slack: Vec<SlackLine>,
}
pub struct SlackLine { pub notice: HandoverNoticeKind, pub destination_line: String, pub state: SlackLineState, pub permalink: Option<String> }
#[serde(rename_all = "snake_case")]
pub enum SlackLineState { Waiting, Posted, NotSent }      // chosen here: the post record's state as the screen shows it
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkOpened {
    /// The handover is here; the program raises `ordin://open-target` for it.
    Opened { handover_id: HandoverId },
    /// The place is now followed; the record is not readable yet (the next check retries). Outcome `HAND_NOT_FOUND_YET`.
    NotFoundYet { place: String },
}
```

`DraftSlackView.line` words (en-GB): `Found` → "They'll also get a Slack message from you." (when `tell_in_slack`) / "Also tell them in Slack" (the tick, off); `NotFound` → "Not found in Slack — they'll get an email" for a SharePoint place, "Not found in Slack — GitHub will ask them to review" for a Git place; `NotSignedIn` → "Sign in to Slack to tell them there too"; `ScopeNeeded` → "Allow ordin to find people in Slack…" (starts the Slack Service's incremental consent, SA-SLACK-02); `NotAllowed` → no line and no tick.

Service API command added (catalogue owner: this module): `handover.open_link` with `{ "link": string 1–4096 }` → `LinkOpened`, scope Local, effect Change. `handover.update_draft` and `handover.return_to` take the new optional fields; `handover.open` returns the new `slack` field.

Callers added: `open_link` — the desktop program (its `ordin` link handler, OrdinDesktop.md) and the screens (Home's **Open a handover link…** paste box, the C-85 fallback).

### Internal Logic (changes)

**A.7 (Slack):** for each handover row, compute its `DraftSlackView` (§S1). `tell_in_slack` starts **true** when `reach` is `Found`, else false.

**B. Check — unchanged.** Slack reach is never a check: a receiver Slack cannot reach gets the email or the review request, so nothing about Slack can stop **Hand over…** (ADR-090 is not touched).

**C.3a–3c (Slack), before `approvals.request`:**
- 3a. For each handover of the group, prepare its posts (§S2): the direct message when `tell_in_slack` is true and `reach` is `Found`; the receiving stage's channel message (`notice = HandedOver`) when that stage has `slack_channel`.
- 3b. For each prepared post: `changes` has one more line — channel: "Also posts as you in #{name}."; direct message: "Also sends {receiver name} a Slack message from you." — and `affects` has one more `AffectedItem::Record { label: destination_line }`. `content.text` (`handover.md`) gains, after the note, a part "\n\n---\nSlack, as you — {destination_line}:\n{text_as_posted}" per post, so **See what will be sent** shows every word that will leave. When `changes` with the added lines would exceed 1,000 characters, the lines are joined into one: "Also posts as you in Slack: {destinations, comma-separated}." (*chosen here*).
- 3c. After `approvals.request` returns `card.approval_id`: `slack.bind_approval(post_id, approval_id)` for each prepared post, then insert `handover_slack_posts (handover_id, slack_post_id, notice)`. A `bind_approval` error leaves that post unbound (it is never sent; its line shows **Not sent**) and is logged `handover.slack_bind_failed {code}`; the handover continues.

**C.4 (Slack), at its end:** once a handover is `sent` (here for SharePoint and for the email path, or on `PullRequestOpened` in §C.6 for Git), run §S3 for every post of that handover in `handover_slack_posts` with state `prepared` or `waiting`. **Step 5 gains:** a refused write sends nothing to Slack; the prepared posts stay unsent and are dropped with the draft's next card (`try_again` prepares new ones).

**C.4 — the receiver email** (`uses_ordin = false`): the body carries, after the note's links, "Open in ordin: {ordin_link}" and "Where it is: {place_link}" (OQ-55 (a); the email stays the notice exactly as before).

**D.2 and D.3 (Slack):** step 2 also reads every place in `followed_handover_places`; step 3 copies the record's `slack_messages` into the local row (append-only merge by `(notice, permalink)`).

**D.5, as it reads above:**
> 5. No answer in time (ADR-092 item 7, amended by ADR-102): `days_to_answer > 0` and no answer `days_to_answer` days after `sent` → notice `NoAnswer` once. `remind(id)`:
>    a. The handover's receiving stage has `slack_channel` → §S2 with `notice = Reminder`. `covered_by_rule` is `Some` → §S3 now. Otherwise `slack.ask_to_post(post_id, from_workspace_id)` — the Slack Service's own card ("Post in #{name}?"); its decision is the Slack Service's, and the post is sent by it on **Post**. Insert `handover_slack_posts`.
>    b. No stage channel, or §S2 refused → `mail.draft_for_person(NewDraft { to: [receiver address], subject: "Handover: [title]", body: a reminder with the note's link, the ordin link and the place link, .. })`; the person reviews and sends that draft through the Mail Service's own card (SA-RECON-06), as before.
>    `remind` returns `Ok(())` in both cases; the screen shows which one was used.

**E3. Appending a Slack message (best effort).** After §S3 returns `Posted { permalink, .. }`: read the record with its version, append one `slack_messages` entry `{notice, permalink, by: the person's address, at: now}` and write conditionally exactly as §E (Git: inside a `HandoverWritten` external action; SharePoint: `upload_version` with `ExternalWrite`). An entry already present (same `notice` and `permalink`) is not appended twice. A conflict retries at most 3 times; then, or on any refusal (a merged pull request whose branch is gone, `GH_NOT_FOUND`; `SP_ACCESS_DENIED`), the entry is kept locally only and `handover.slack_link_not_recorded {handover_id, code}` is logged. It never surfaces as `HAND_PLACE_BUSY` and never blocks the handover.

**F. Open, accept, return (Slack):**
- **F.2, after `handover.accepted` is journalled:** when the handover's lifecycle stage has `slack_channel` (`catalogue.lifecycle(lifecycle_id)`; an unknown lifecycle on the receiver's side → no post), §S2 with `notice = Accepted`, target the stage channel, mention the sender. `covered_by_rule` is `Some` → §S3 now; else `slack.ask_to_post(post_id, accepted_into_workspace_id)`. The acceptance never waits for Slack and never fails because of it.
- **F.4:** before the card, §S2 with `notice = Returned` when the stage has `slack_channel`; the reason is in the text only when `reason_in_slack` is true. Card lines as §C.3b; `bind_approval` as §C.3c. After the `sent_back` event is appended (§E), §S3.
- **F.6:** a handover deleted as a draft posts nothing. Otherwise, before the take-back card, §S2 with `notice = TakenBack` when the stage has `slack_channel`; card lines and binding as §C.3b–c; after `taken_back` is appended, §S3. Any prepared post of an earlier action still `prepared` or `waiting` for this handover is left unsent (its card was withdrawn or decided).

**S. Slack notices (new section).**
1. **Reach** (`reach_for(handover)`): `slack.reach(receiver.address)`; any `SlackError` → `NotSignedIn` and log `handover.slack_reach_failed {code}`. Result stored in `handovers.slack_reach` with `slack_reach_checked_at`; re-read when the receiver changes or the stored value is older than 10 minutes (*chosen here*). `stage_rule` = `slack.rule_for(&SlackRuleKey::Stage { lifecycle_id, stage_name }, SlackRuleKind::HandoverNotices)` when the stage has a channel (error → `None`).
2. **Prepare** (`prepare(handover, notice, target)`): build the text from the message table above (title, names and links of this handover; `place_link` per *The handover link*); `slack.prepare_post(SlackPostRequest { target, thread_ts: None, text, origin: SlackPostOrigin::Handover { handover_id, notice }, rule_key: Some(SlackRuleKey::Stage { lifecycle_id, stage_name }) for a channel post, None for a direct message })`. A `SlackError` (any `SLK_*`: Slack switched off, not signed in, channel not allowed, member not found, scope needed) → no post for this notice, log `handover.slack_skipped {notice, code}`; the handover action continues unchanged and its email or review request is the notice.
3. **Send** (`send(post)`): `slack.send(post_id)`.
   - `Posted { ts, permalink }` → journal `handover.slack_posted {handover_id, notice, slack_post_id}`; run §E3.
   - `Refused { code }` → the screen line shows **Not sent**; log `handover.slack_not_sent {notice, code}`. For a direct message to a receiver without ordin the email of §C.4 has already gone; for a channel post nothing else is sent (*assumed here*: the place write and the email or review request remain the record of the moment).
   - `UnknownOutcome` → the line shows "Not sure this was posted" (external-action wording of 00); nothing is resent (04:200).
   - A `SlackError` → as `Refused`.
4. **After a restart** (added to §E2): every `handover_slack_posts` row whose handover is `sent`, `seen`, `sent_back`, `accepted` or `taken_back` and whose post record is `prepared` or `waiting` with an approved card (`slack.post_record(id)`) is sent by §S3 once; one whose card was rejected or expired is left. The Slack Service's own crash rules decide `posting` rows (never resent).

**L. Handover links (new section; `open_link`).**
1. Parse with the `url` crate: length 1–4,096; scheme `ordin`; host `handover`; path `/open`; query exactly the keys `h`, `t`, `p` once each, no other key; `h` a UUIDv7; `t` `git` or `sharepoint`; `p` percent-decoded to an `https` URL ≤ 2,048 bytes with no user information and no fragment. Any failure → `HAND_LINK_INVALID` (permanent; the screen says "That isn't a handover link ordin can open.").
2. Place host: `t = git` → the host is `github.com` (GitHub only, as the Git place rule of §A.6); `t = sharepoint` → the host matches `^[a-z0-9-]{1,63}\.sharepoint\.com$`. Otherwise `HAND_LINK_PLACE_REFUSED` (permanent; "ordin opens handover links to GitHub and SharePoint places only."). The link's text is data: nothing in it is fetched except through the SharePoint Adapter with the person's own sign-in (§L.4) or GitHub's own search (§L.5); `p` is never requested over plain HTTP.
3. A local `handovers` row with `id = h` → `LinkOpened::Opened { handover_id: h }` (journal `handover.link_opened {handover_id, found: true}`).
4. `t = sharepoint`: `sharepoint.resolve_web_url(p)` (`SP_SIGN_IN_NEEDED` → `HAND_SIGN_IN_NEEDED`; `SP_ACCESS_DENIED`, `SP_NOT_FOUND` → `HAND_PLACE_REFUSED`; unreachable → `HAND_PLACE_UNREACHABLE`). Upsert `handover_places` (`place_type = sharepoint`, `address = p`, `display_name` = the last path segment, 1–120); when the place is neither a place of a stage the person's workspaces follow nor a place a workspace links, insert `followed_handover_places (handover_place_id, added_at = now, from_link_handover_id = h)` (journal `handover.place_followed {handover_place_id}`). At most 50 rows: inserting the 51st deletes the oldest row whose place holds no local handover (*chosen here*). Then run §D.2 for **that place only**, with a 60-second time limit.
5. `t = git`: run §D.1 now, with a 60-second time limit (Git handovers are found through the person's review requests; the place needs no following).
6. After §L.4 or §L.5: a local row with `id = h` now exists → `Opened { handover_id: h }` and the `HandedToYou` notice of §D.3 is sent as usual; else → `NotFoundYet { place: display_name }` (outcome `HAND_NOT_FOUND_YET`; the screen says "ordin now follows {place}. The handover isn't readable there yet; ordin looks again every 15 minutes."). A record found whose receiver is not the person is skipped exactly as §D.2 states, and the result is `NotFoundYet`.

**G.1 (Slack):** following a stage whose place is in `followed_handover_places` deletes that row (the stage now covers it).

**H. Needs-you events — unchanged;** a Slack card the Slack Service opens is the Slack Service's (it appends its own `needs_you_changed`).

### Data Model (additive migration)

```sql
-- migrations(): module `handover`, step (2, "slack notices and followed places", <this SQL>)
-- spec: ordin-desktop/components/HandoverService#handover_slack_posts
ALTER TABLE handovers ADD COLUMN tell_in_slack INTEGER NOT NULL DEFAULT 0;
ALTER TABLE handovers ADD COLUMN slack_reach TEXT;                       -- JSON SlackReach, chosen here
ALTER TABLE handovers ADD COLUMN slack_reach_checked_at TEXT;
ALTER TABLE handovers ADD COLUMN slack_messages TEXT NOT NULL DEFAULT '[]';   -- JSON Vec<HandoverSlackMessage>, append-only

CREATE TABLE handover_slack_posts (
  handover_id   TEXT NOT NULL REFERENCES handovers(id) ON DELETE CASCADE,
  slack_post_id TEXT NOT NULL UNIQUE,                                     -- SlackPostId (Slack Service's record)
  notice        TEXT NOT NULL CHECK (notice IN ('handed_over','accepted','returned','taken_back','reminder')),
  direct        INTEGER NOT NULL DEFAULT 0,                               -- 1 = direct message, 0 = stage channel
  created_at    TEXT NOT NULL,
  PRIMARY KEY (handover_id, slack_post_id)
);

CREATE TABLE followed_handover_places (
  handover_place_id     TEXT PRIMARY KEY REFERENCES handover_places(id) ON DELETE CASCADE,
  added_at              TEXT NOT NULL,
  from_link_handover_id TEXT NOT NULL                                     -- HandoverId the link named
);
CREATE INDEX ix_followed_places_added ON followed_handover_places(added_at);
```

Rollback (sql-safety): additive only — four columns with constant defaults (SQLite `ADD COLUMN` with a constant default rewrites no row) and two new tables; no `DROP`, no narrowing, no backfill. Forward-only per SA-XC-07; a build without this step reads none of the new columns. `ON DELETE CASCADE` removes post links and followed places only with their handover or place.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `open_link` text is not a handover link of the fixed form | `Err(permanent)` | `HAND_LINK_INVALID` | `{"error":"That isn't a handover link ordin can open.","code":"HAND_LINK_INVALID","action":null}` |
| The link's place is not on GitHub or SharePoint | `Err(permanent)` | `HAND_LINK_PLACE_REFUSED` | `{"error":"ordin opens handover links to GitHub and SharePoint places only.","code":"HAND_LINK_PLACE_REFUSED","action":null}` |
| Place followed, handover not readable there yet | outcome (`LinkOpened::NotFoundYet`) | `HAND_NOT_FOUND_YET` | `{"kind":"not_found_yet","place":"Procurement handovers"}` |

Slack outcomes are never errors of this service: every `SLK_*` from `reach`, `prepare_post`, `bind_approval`, `send` or `ask_to_post` is logged and shown as the Slack line's state (**Not sent**, "Not sure this was posted"), and the handover action returns as it would without Slack.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Handover Service — Slack notices

  Background:
    Given Priya is signed in to Slack and the Slack Service finds "sam.lee@acme.co.uk" as member "U07SAMLEE1"
    And lifecycle "Supplier onboarding" stage "Design" names the Slack channel "C04ABCDEF12" "design-reviews"

  Scenario: Receiver told by direct message on the hand-over card
    Given a SharePoint handover draft to Sam Lee with Also tell them in Slack ticked
    When Priya approves the hand-over card
    Then the card's changes said "Also sends Sam Lee a Slack message from you."
    And after handover.yaml is written exactly one direct message to Sam is sent through slack.send
    And handover_slack_posts holds that post with notice handed_over and direct 1

  Scenario: Stage channel post on hand-over, after the write
    Given a handover to stage "Design"
    When Priya approves the hand-over card
    Then slack.prepare_post was called with target channel "C04ABCDEF12" before the card and bound to the card's approval
    And slack.send is called only after the record write succeeded
    And the record's slack_messages gains one entry with the post's permalink

  Scenario: A refused place write sends nothing to Slack
    Given Priya has no write access to the SharePoint place
    When the approved handover is written
    Then SendFailed carries HAND_PLACE_REFUSED
    And slack.send is not called for any post of that handover

  Scenario: Accepting posts by the person's rule, or asks
    Given Sam has a handover-notices rule on for stage "Design"
    When Sam accepts the handover
    Then one message mentioning Priya is sent with slack.send without a card
    Given Sam has no rule for stage "Design"
    When Sam accepts another handover of that stage
    Then slack.ask_to_post is called and the acceptance is not delayed

  Scenario: Return keeps the reason only when the person kept it
    When Sam returns the handover with reason "Payment terms are missing" and reason_in_slack false
    Then the prepared text contains no reason
    And after the sent_back event is appended the post is sent

  Scenario: Overdue reminder goes to the stage channel
    Given stage "Design" has days_to_answer 2 and a handover sent 2 days ago with no answer
    When Priya presses Remind Sam Lee…
    Then slack.ask_to_post is called for a reminder post mentioning Sam
    And mail.draft_for_person is not called

  Scenario: Reminder without a stage channel drafts the email as before
    Given a stage with no Slack channel
    When Priya presses Remind Sam Lee…
    Then mail.draft_for_person is called and no Slack Service method is called

  Scenario: Opening the link follows the SharePoint place
    Given Sam's ordin follows no stage whose place is "https://acme.sharepoint.com/sites/proc/Handovers"
    When open_link is called with "ordin://handover/open?h=0192a3d0-1111-7aaa-8bbb-cccc0000d001&t=sharepoint&p=https%3A%2F%2Facme.sharepoint.com%2Fsites%2Fproc%2FHandovers"
    Then followed_handover_places gains that place
    And that place is read within 60 seconds
    And the result is opened with the handover's id and HandedToYou is sent

  Scenario: Every message carries a place link for a computer without ordin
    When a Git handover's direct message is prepared
    Then its text holds "https://github.com/acme/northwind/tree/ordin-handover/2026-01-10-northwind-assessment-0192a3d0/handovers/2026-01-10-northwind-assessment"

  Scenario: No Slack configured means no Slack request
    Given Priya has no Slack sign-in and no stage names a channel
    When she opens, checks and hands over a draft
    Then the draft view's Slack line is "Sign in to Slack to tell them there too"
    And no post is prepared and the handover tests above pass unchanged

  Scenario: Receiver not in Slack falls back to email
    Given the Slack Service returns NotFound for "dan.moss@acme.co.uk"
    When Priya opens a SharePoint draft to Dan Moss
    Then the Slack line is "Not found in Slack — they'll get an email"
    And after approval the email of step C.4 is sent with the ordin link and the place link

  Scenario: Slack switched off on the server
    Given the personal server's profile lists slack_post in server.external_actions_off
    When a draft opens
    Then reach is NotAllowed, no Slack tick is offered, and the card carries no Slack line

  Scenario: A link to another host is refused
    When open_link is called with p "https://evil.example.com/handovers"
    Then it fails with HAND_LINK_PLACE_REFUSED and no place is followed

  Scenario: A malformed link is refused
    When open_link is called with "ordin://handover/open?h=42"
    Then it fails with HAND_LINK_INVALID
```

### Performance, Security, Observability (additions)

- **Performance:** `reach` is cached per handover for 10 minutes, so typing in the draft causes at most one lookup per receiver per 10 minutes; preparing up to three posts adds ≤ 1 s p95 to opening the hand-over card, excluding Slack's response (*chosen here*); `open_link` answers within 60 s, bounded by the single-place check.
- **Security:** a Slack post leaves only through the Slack Service, after this service's write succeeded and under the card the person approved or the person's own rule (ADR-102); the message words are fixed templates, never the note or results (the note stays in the place, ADR-092 item 3); the reason goes to Slack only when the person kept it; a handover link is untrusted text — fixed form, two allowed hosts, read only with the person's own sign-in, at most 50 places followed from links; a record's `slack_messages` from another person are data, shown as links and never opened by ordin.
- **Observability (logs only):** `handover.slack_skipped {handover_id, notice, code}`, `handover.slack_bind_failed {handover_id, code}`, `handover.slack_posted {handover_id, notice, slack_post_id}`, `handover.slack_not_sent {handover_id, notice, code}`, `handover.slack_link_not_recorded {handover_id, code}`, `handover.slack_reach_failed {code}`, `handover.slack_entry_ignored {handover_id}`, `handover.link_opened {handover_id, found}`, `handover.place_followed {handover_place_id}`. Never logged: message text, Slack member ids, permalinks, the link's `p`.

### Gaps

None.

---

## Amendment January 2026 — Jira issues and handovers (ADR-106)

> Source: decision 86; HLD revision 0.54.0, ADR-106 (closes OQ-57; amends ADR-092 with a third handover place type). Settled with ADR-106: the record on Jira is one comment per event; a receiver without ordin answers by reassigning the issue or finishing it; ordin labels, comments, assigns and may create an issue, and never changes an issue's status or fields; results are linked, never uploaded; Jira Cloud only; desktop only.
> Every rule below adds to the sections above. Git and SharePoint handovers are unchanged: with no Jira place in a lifecycle and no Jira issue linked to a workspace, every step runs exactly as it does above and no Jira Adapter call is made.

### Purpose (addition)

A handover can also travel on **one Jira issue** in a Jira Cloud project both people can see. Handing over then writes the note and the record to that issue as comments, labels it `ordin-handover` and assigns it to the receiver; Jira's own notification is the notice, so no email is sent. The receiver's ordin finds issues assigned to them with that label on the existing receiving schedule, **Accept and start** links the issue to the receiving workspace, and **Return to…** and **Take back** record the answer on the issue and assign it back to the sender. A sender whose receiver does not use ordin reads the receiver's answer from the issue itself.

### Approach (addition)

Jira has no conditional write, so the A-44 guarantee is kept by appending only: the note and the record's fixed part are the **opening comment**, and each event is **its own comment**, which no other installation can overwrite. Events are read in comment creation order (C-86); an event counts only when its Jira author is the person the event names; an edited record comment is ignored as data (R-104). Retries are de-duplicated by an `event_id` on each event. All Jira calls go through the Jira Adapter (`ordin-jira`), which builds every query itself and brackets every write as an external action under the key this service passes. Rejected: *an issue entity property re-read after each write* — two installations writing in the same second would overwrite each other, the loss R-83 forbids; *one attachment per event* — attachments are heavier to read and appear in the issue's file list as clutter; *moving the issue's status* — workflows differ per project and each status change would need its own card (ADR-106).

### Shared Context (additions)

```rust
// 2C domain (amended January 2026, ADR-106) — copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoverPlaceType { Git, Sharepoint, Jira }           // stored 'git' | 'sharepoint' | 'jira'

// HandoverReceiver and PersonChoice each also have (2C domain, amended):
//   #[serde(skip_serializing_if = "Option::is_none")] pub jira_account_id: Option<String>,   // 1–128, [A-Za-z0-9:_-]
// For a Jira place `jira_account_id` is required and `address` is the address Jira shows, or "" when the person's
// Jira privacy setting hides it (C-88); HAND_RECEIVER_INVALID then applies to `jira_account_id`, not to `address`.

// HandoverEvent also has (2C domain, amended):
//   #[serde(skip_serializing_if = "Option::is_none")] pub event_id: Option<uuid::Uuid>,   // UUIDv7; always set on a Jira place
// On a Jira place an event's `by` is `jira:<accountId>` (chosen here: an address may be hidden, the account id never is).
```

Jira Adapter types used (JiraAdapter.md; copied): `JiraProjectRef { site, key }`, `JiraIssueRef { site, key }`, `JiraUser { account_id, display_name, email: Option<String> }`, `JiraStatusCategory { ToDo, InProgress, Done }`, `JiraIssueSummary { issue, id, summary, assignee: Option<JiraUser>, status_category, labels, updated }`, `JiraComment { id, author: JiraUser, created, updated, body_markdown, code_blocks: Vec<JiraCodeBlock> }`, `JiraCodeBlock { language: Option<String>, text }`, `JiraCommentBody { markdown, code_block: Option<JiraCodeBlock> }`, `JiraQuery::{HandoversFor { label, also_keys }, Text(String)}`.

**The handover place address** for a Jira place is the Jira Adapter's canonical project address `https://{site}.atlassian.net/projects/{KEY}`; any form `JiraAdapter::parse_project_address` accepts is stored as `JiraAdapter::project_address` returns it. `display_name` = the project's name from `jira.project(&project)` cut to 120 characters, else `{KEY}`.

**Record keys added to `ordin.handover/v1`** (all optional; an installation that does not know them keeps and ignores them, as the parsing rules above state):

```yaml
jira: { site: acme.atlassian.net, issue: ABC-12 }                   # only on a Jira place
sender:   { name: Priya Shah, address: priya.shah@acme.co.uk, jira_account_id: "712020:5c1e…" }
receiver: { name: Sam Lee, address: "", jira_account_id: "712020:9a7b…" }
events:
  - { kind: sent, by: "jira:712020:5c1e…", at: "2026-01-10T09:00:00.000Z", round: 1, event_id: 0192a3d0-2222-7aaa-8bbb-cccc0000e001 }
```

**The record on a Jira issue** (*fixed here*; the files of *Files in the handover place* are not written for a Jira place):

1. **Opening comment** — the note exactly as `handover.md` reads (`# [title]`, the four en-GB headings, `## Results` with one link per result), then one code block (`language: yaml`) whose first line is `# ordin.handover/v1 record` and whose remaining lines are the record of *Files in the handover place* **without** `events`. Written by the sender's installation once per round 1; a later round (`change_and_hand_over_again`) writes a new opening comment whose record has the new `round`, and the newest valid opening comment by the sender is the record's fixed part.
2. **Event comment** — one plain line, then one code block (`language: yaml`) whose first line is `# ordin.handover/v1 event` and whose remaining lines are exactly one event mapping. Plain lines (en-GB, message catalogue ids): `handover-jira-sent` "Handed over to {receiver_name} in ordin."; `handover-jira-seen` "Seen by {name} in ordin."; `handover-jira-accepted` "Accepted by {name} in ordin."; `handover-jira-sent-back` "Returned to {to_name} in ordin. Reason: {reason}"; `handover-jira-taken-back` "Taken back by {name} in ordin.".
3. **Reading rules.** A comment is a record comment when it has a code block whose first line is one of the two markers; its code block (marker line removed) is parsed with `ordin_types::yaml::read_restricted::<T>(text, 262_144)` as `handover.yaml` is. Comments are taken in the order `comments()` returns them (creation order, C-86). An event comment counts only when (a) its `by` equals `"jira:" + comment.author.account_id`, (b) `by` names the record's sender or receiver (or `returned_to.person`), (c) `updated == created`, and (d) its `event_id` was not seen earlier in the list; otherwise it is ignored and logged `handover.jira_comment_ignored {handover_id, reason}` (reason `author`, `edited`, `duplicate`, `unreadable`) and shown in Diagnostics (R-104). An opening comment that is edited, unreadable or not written by the record's sender → `HAND_RECORD_UNREADABLE` (the handover is not shown). Everything read is data (R-81); note lines that read as instructions are marked as §F.1 states.
4. **Size.** Each comment ≤ 32,000 characters after conversion (*assumed here*: Jira Cloud refuses a comment over 32,767 characters; the margin covers conversion). A draft whose opening comment would exceed it has `can_hand_over = false` with the plain line "This handover is too long for one Jira comment. Shorten the note or hand over fewer results." (`HAND_JIRA_TOO_LONG`).

### Configuration (rows added)

| Item | Value | Source |
|---|---|---|
| Jira label | `ordin-handover` (the GitHub label's text) | ADR-106 |
| Jira receiving check | one `search` per check on the connected Jira site, plus one `comments` read per issue whose `updated` changed since the last read; up to 50 issues | A-54 |
| Comment size limit | 32,000 characters | *assumed here* (Shared Context item 4) |
| New issue type | the project's issue type named "Task", else its first issue type that is not a sub-task (Jira Adapter `create_issue`) | ADR-106; OQ-69 |

### Boundary interfaces (additions)

```rust
// ordin-jira — `JiraAdapter` (JiraAdapter.md; build phase 2; programme D), each provider's signature as that spec states:
//   JiraAdapter::parse_project_address(&str) -> Result<JiraProjectRef, JiraError>;   // associated, sync
//   JiraAdapter::parse_issue_address(&str) -> Result<JiraIssueRef, JiraError>;        // associated, sync
//   JiraAdapter::project_address(&JiraProjectRef) -> String; JiraAdapter::issue_address(&JiraIssueRef) -> String;
//     // the canonical forms, associated, sync
//   myself(site: &str) -> JiraUser;  project(&JiraProjectRef) -> JiraProject { key, name, .. };
//   find_assignable(&JiraProjectRef, query: &str, limit: u32 /* ≤ 20 */) -> Vec<JiraUser>;
//   is_assignable(&JiraProjectRef, account_id: &str) -> bool;
//   search(site: &str, &JiraQuery, limit: u32 /* ≤ 50 */) -> Vec<JiraIssueSummary>;
//   comments(&JiraIssueRef) -> Vec<JiraComment>;                                  // every page, creation order
//   create_issue(&JiraProjectRef, summary: &str, action_key: ExternalActionKey) -> JiraIssueRef;
//   add_label(&JiraIssueRef, label: &str, action_key) -> ();
//   add_comment(&JiraIssueRef, &JiraCommentBody, action_key) -> JiraComment;
//   assign(&JiraIssueRef, account_id: &str, action_key) -> ();
//   each async, returning Result<_, JiraError> (`code()` JIRA_*, `retryable()`). Writes (JiraAdapter.md SA-JIRA-01,
//   SA-JIRA-02): this service mints the `ExternalActionKey` (UUIDv7) and passes it; the adapter alone begins and
//   confirms the journal's external action, `kind: HandoverWritten`, `run_id: None`, destination
//   `Jira › {site} › {KEY-N}` for a write to an issue and `Jira › {site} › {KEY}` for `create_issue`.
//   On a timeout the adapter retries `add_label` and `assign` once with the same key; `create_issue` makes one search
//   for an issue it may have created, else returns `JIRA_OUTCOME_UNKNOWN`; `add_comment` returns
//   `JIRA_OUTCOME_UNKNOWN` at once and is never resent by the adapter — this service re-reads the comments for the
//   `event_id` (§E (Jira)). The connected site is the one Jira credential's site (IdentityAndCredentials.md,
//   `CredentialKind::Jira`).

// ordin-workspaces — additions used (WorkspaceService.md, amended January 2026, ADR-106):
//   SourceLinkKind::JiraIssue (its `origin_location` is the canonical issue address
//   `https://{site}.atlassian.net/browse/{KEY}-{N}`); LinkLocation::JiraIssue { address: String } for `add_link`.
```

`HandoverDeps` gains `pub jira: Option<std::sync::Arc<ordin_jira::JiraAdapter>>` — `None` in `ordin-server` (Jira is programme D only); with `None`, a Jira place is refused at §A.6 with `HAND_PLACE_TYPE_UNAVAILABLE` and no Jira search runs.

### Public Interface (additions)

```rust
pub struct UpdateDraftInput {
    // … every field above, unchanged …
    /// The issue a Jira handover rides on. None = unchanged.
    pub jira_issue: Option<JiraIssueSelection>,
}
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JiraIssueSelection {
    /// One of the workspace's linked issues in the place's project.
    Linked { key: String },                       // ^[A-Z][A-Z0-9_]{1,9}-[1-9][0-9]{0,9}$
    /// A new issue in the place's project, named after the handover's title; stated on the card.
    CreateNew,
}
pub struct HandoverDraftView {
    // … every field above, unchanged …
    /// Set only when the draft's place is a Jira place.
    pub jira: Option<DraftJiraView>,
}
pub struct DraftJiraView {
    pub project: String,                          // the place's display name
    pub options: Vec<JiraIssueOption>,            // the workspace's linked issues in that project, newest link first
    pub selection: JiraIssueSelection,
    /// The line the draft shows (message catalogue `handover-jira-rides-on` / `handover-jira-creates`).
    pub line: String,
}
pub struct JiraIssueOption { pub key: String, pub summary: String /* ≤ 255 */ }
```

`DraftJiraView.line` words (en-GB): `Linked` → "Through {key} in Jira: {summary}."; `CreateNew` → "A new issue will be created in {project}." (11 §5.28 as amended) While the handover is in `draft` with `jira_issue_created = 1` (an issue made at §C.4.1 whose later writes failed), `line` is instead "ordin created {key}; it is still in {project}." and `selection` is `Linked { key }` of that issue, so the next send rides on it and creates no second issue. The screens draw `line` as sent and add no Jira sentence of their own.

Service API: no new command; `handover.update_draft` takes the new optional `jira_issue`, `handover.open_draft` and `handover.update_draft` return `jira`, and `handover.find_people` returns `jira_account_id` on each choice. `PersonChoice.address` may be "" for a Jira choice; the screens show the name only then.

### Internal Logic (changes)

**A.6 (Jira):** a lifecycle stage's place with `place_type = jira` is inserted as above (canonical address, display name from `jira.project`). Outside a lifecycle, Jira places are the projects of the workspace's `SourceLinkKind::JiraIssue` links (`JiraAdapter::parse_issue_address(origin_location)` → `JiraProjectRef { site, key: the issue key's project part }`), offered beside the Git and SharePoint places. `deps.jira` is `None` → `HAND_PLACE_TYPE_UNAVAILABLE`.

**A.7 (Jira):** with a Jira place, compute `DraftJiraView`: `options` = the workspace's `JiraIssue` links whose project key and site match the place, ordered by `added_at` descending; `selection` = `Linked { key }` of the first option, else `CreateNew`.

**B.1 (Jira):** result facts are composed as above. A Jira place gives no GitHub login, so a Git result's `readable_by_receiver` is `None`; a local-only result still fails `results_readable` with **Put it in [place]…**, where [place] is the first Git or SharePoint place the workspace links, else "a shared folder" (*chosen here*: a result is never put in Jira, ADR-106).

**B.3 (Jira):** `receiver.reachable` = the receiver has `jira_account_id` ∧ `jira.is_assignable(&project, account_id)`. A `JiraError` → `reachable = false` with the error's mapped plain sentence as the finding's reason. Nothing can be waived (ADR-090). `can_hand_over` additionally requires a `selection` and the size rule of Shared Context item 4.

**C.2 (Jira):** `uses_ordin` = true when an earlier record shows an event written by the receiver; otherwise false. No email is sent for a Jira place in either case: Jira's assignment notification is the notice (ADR-106).

**C.3 (Jira):** the card is raised as above with: destination the issue's address (or the project's address for `CreateNew`); `changes` "[name] is assigned [key] in Jira. Everyone who can see [key] sees the note and the results' links." and, for `CreateNew`, first the line "ordin creates a new issue in [project] named "[title]"." (R-105); `content.text` the opening comment's Markdown.

**C.4 (Jira), on approval**, per handover, in this order, each write with a fresh UUIDv7 `ExternalActionKey` recorded in `jira_pending_writes` before the call and deleted after it returns:
1. `CreateNew` → `jira.create_issue(&project, title, k1)`; store `jira_issue` and `jira_issue_created = 1`. `JIRA_ISSUE_TYPE_NONE` → `HAND_ISSUE_TYPE_NONE`, the draft stays a draft. `JIRA_OUTCOME_UNKNOWN` (the adapter's one search found no issue it may have created) → the draft stays a draft with `SendFailed` `HAND_PLACE_UNREACHABLE`, and its `jira_pending_writes` row stays for §E2 (Jira).
2. `myself(site)` → the sender's `jira_account_id` and `by`; `add_comment(&issue, opening, k2)`; store `jira_opening_comment_id`.
3. `add_comment(&issue, event sent, k3)` with a new `event_id`.
4. `add_label(&issue, "ordin-handover", k4)`; `assign(&issue, receiver.jira_account_id, k5)`.
5. `state = sent`; journal `handover.sent`; then the Slack steps of §C.4 (Slack) as for any place, with `place_link` = the issue's address and the handover link of §L with `t=jira`.
A `JiraError` at step 1, 2 or 3 → the draft stays a draft, nothing is recorded as sent, `SendFailed` with the mapped code (`HAND_PLACE_REFUSED` for a refusal). An opening comment left without its `sent` comment is ignored by every reader (no `sent` event, no handover). A failure at step 4 after step 3 succeeded (the adapter has already retried a timed-out `add_label` or `assign` once with the same key) → the handover is `sent`; the missing label or assignment is retried by §D.6 at the next check, at most 3 times, then the screen shows "ordin couldn't assign [key] to [name] in Jira; assign it there." (`HAND_JIRA_ASSIGN_FAILED`, outcome).

**D.6 Jira receiving (new step; A-54).** When `deps.jira` is `Some` and a Jira credential is connected, on the schedule of §D:
1. `also_keys` = the issue keys of local handovers on Jira places with `direction = handed_over` in `sent`, `seen` or `sent_back` (at most 50, newest first). `jira.search(site, &JiraQuery::HandoversFor { label: "ordin-handover", also_keys }, 50)` — one call.
2. For each summary whose `updated` differs from the local `record_version` (or no local row): `jira.comments(&issue)`; read the record (Shared Context item 3). `record_version` = `updated`.
3. Receiver side: the record's `receiver.jira_account_id` must equal `myself(site).account_id`, the issue's assignee must be the person, and it must hold a counted `sent` event; else skip. Upsert as §D.3 (`direction = handed_to_you`), upserting the place from `jira.site` and the issue's project.
4. Sender side: adopt counted events by the receiver (§D.4). For a receiver who has not written an ordin event after the latest `sent` (settled with ADR-106): `status_category = Done` → `accepted` (notice `Answered`); assignee is the sender ∧ a newer comment that is not a record comment, written by the receiver after the `sent` event → `sent_back` with that comment's `body_markdown` cut to 1,000 characters as the reason (notice `ReturnedToYou`); assignee is the sender without such a comment → `sent_back` with the reason "Assigned back to you in Jira without a comment." These answers are read only: nothing is written to Jira for them, and `uses_ordin` stays false.
5. A `JiraError` stops this step for this check (logged `handover.jira_check_failed {code}`); Git and SharePoint receiving are unaffected. The label and assignment retries of §C.4 run here.

**E (Jira). Appending an event.** No conditional write: `add_comment(&issue, event, key)` with a new `event_id`, recorded in `jira_pending_writes` first. `HAND_PLACE_BUSY` never arises on a Jira place. When two installations answer in the same second, both comments are kept; the first counted event in creation order decides the state, and the person whose event came second is shown what happened first ("[name] had already [answered] in Jira."). `JIRA_OUTCOME_UNKNOWN` (the adapter never resends a comment) → `comments(&issue)`; the `event_id` is present → adopted; absent → the local state is left as it was, `handover.unknown_outcome` is logged and the screen offers the action again (nothing is written again automatically; readers drop a later duplicate `event_id`). `JIRA_ACTION_KEY_USED` → re-read as for an unknown outcome.

**E2 (Jira). After a restart:** after the program's `Journal::recover_external_actions`, `journal.unsettled_external_actions(&[HandoverWritten])`; each action whose `destination` starts `Jira › ` and whose `action_key` is a `jira_pending_writes` row is this service's (SA-JIRA-02): a comment write is resolved by re-reading the issue's comments (an event comment whose `event_id`, or an opening comment whose record `id`, matches is adopted); `create_issue` (destination `Jira › {site} › {KEY}`) by `search(site, Text(title), 5)` filtered to issues whose summary equals the title and whose assignee is unset or the sender; `add_label` and `assign` by the next §D.6 check, which reads the issue's labels and assignee. An adopted write deletes its row and logs `handover.recovered_arrived`; the Journal row stays `unknown_outcome` for the person's **It arrived — mark as done**, as §E2 states. An unresolved one leaves the local state as before the write, logs `handover.unknown_outcome` and offers the action again; nothing is written again automatically. `jira_pending_writes` rows whose action the Journal shows confirmed are deleted.

**F.1 (Jira):** the first `open` appends `seen` by §E (Jira).

**F.2 (Jira):** `accept_and_start` appends `accepted` by §E (Jira) and links, besides the Git and SharePoint results as above, the issue: `workspaces.add_link(ws, LinkLocation::JiraIssue { address: canonical issue address }, Some(key))`; a `WorkspaceError` is logged `handover.link_failed {code}` and the acceptance continues. No GitHub review or merge is made for a Jira place.

**F.4 (Jira):** on approval of the return card: append `sent_back` by §E (Jira); then `assign(&issue, to, key)` where `to` is `returned_to.person.jira_account_id` when the return goes past the sender and that id is known, else the sender's `jira_account_id`. The card's `changes` adds "[key] is assigned to [name] in Jira."

**F.5 (Jira):** `change_and_hand_over_again` runs §C.3–C.4 with step C.4.1 skipped (the issue exists) and a new opening comment for the new round.

**F.6 (Jira):** a draft is deleted locally as above (an issue created at §C.4.1 for a draft whose later writes failed is left in Jira and named on the draft screen through `DraftJiraView.line`: "ordin created {key}; it is still in {project}."). Otherwise, after the take-back card: append `taken_back` by §E (Jira), then `assign(&issue, sender jira_account_id, key)`.

**G0 (Jira):** a Jira place → `jira.find_assignable(&project, query, 20)`, each `JiraUser` as `PersonChoice { name: display_name, address: email or "", github_login: None, outside_company: email's domain not one of the organisation's domains (false when email is None), jira_account_id: Some(account_id) }`. No typed address is accepted for a Jira place: an address that is not an assignable person returns no choice.

**L (Jira):** the handover link's `t` also takes `jira`; its place host must match `^[a-z0-9-]{1,63}\.atlassian\.net$` (else `HAND_LINK_PLACE_REFUSED`, whose sentence becomes "ordin opens handover links to GitHub, SharePoint and Jira places only."); §L.4 for `t = jira` runs §D.6 now with a 60-second time limit (a Jira handover is found by assignment; the place needs no following).

**S (Jira):** for a receiver whose `address` is "", `slack.reach` is not called and `reach` is `NotFound`.

**Jira error mapping** (added to the provider mapping above): `JIRA_NOT_CONNECTED`, `JIRA_SIGN_IN_NEEDED`, `JIRA_TOKEN_REFUSED` → `HAND_SIGN_IN_NEEDED`; `JIRA_ACCESS_DENIED`, `JIRA_NOT_FOUND`, `JIRA_QUERY_INVALID` → `HAND_PLACE_REFUSED`; `JIRA_ADDRESS_INVALID` → `HPLACE_ADDRESS_INVALID`; `JIRA_SITE_MISMATCH` → `HAND_PLACE_SITE_OTHER`; `JIRA_ISSUE_TYPE_NONE` → `HAND_ISSUE_TYPE_NONE`; `JIRA_TOO_LARGE` → `HAND_JIRA_TOO_LONG`; `JIRA_THROTTLED`, `JIRA_UNAVAILABLE`, `JIRA_RESPONSE_INVALID` → `HAND_PLACE_UNREACHABLE`; `JIRA_OUTCOME_UNKNOWN`, `JIRA_ACTION_KEY_USED` → re-read (§E (Jira)).

### Data Model (migration step 3)

```sql
-- migrations(): module `handover`, step (3, "jira handover places", <this SQL>)
PRAGMA defer_foreign_keys = ON;   -- inside the registry's one transaction; reset at commit

CREATE TEMP TABLE keep_sharepoint_place_reads AS SELECT * FROM sharepoint_place_reads;
CREATE TEMP TABLE keep_followed_handover_places AS SELECT * FROM followed_handover_places;

CREATE TABLE handover_places_v3 (
  id TEXT PRIMARY KEY,
  place_type TEXT NOT NULL CHECK (place_type IN ('git','sharepoint','jira')),
  address TEXT NOT NULL UNIQUE CHECK (length(address) <= 2048),
  display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120)
);
INSERT INTO handover_places_v3 (id, place_type, address, display_name)
  SELECT id, place_type, address, display_name FROM handover_places;
DROP TABLE handover_places;
ALTER TABLE handover_places_v3 RENAME TO handover_places;

INSERT OR IGNORE INTO sharepoint_place_reads SELECT * FROM keep_sharepoint_place_reads;
INSERT OR IGNORE INTO followed_handover_places SELECT * FROM keep_followed_handover_places;
DROP TABLE temp.keep_sharepoint_place_reads;
DROP TABLE temp.keep_followed_handover_places;

ALTER TABLE handovers ADD COLUMN jira_issue TEXT;                         -- JSON {site, key}; Jira places only
ALTER TABLE handovers ADD COLUMN jira_issue_created INTEGER NOT NULL DEFAULT 0;
ALTER TABLE handovers ADD COLUMN jira_opening_comment_id TEXT;
-- record_version holds the issue's `updated` for a Jira place (its meaning above: the last version read).

CREATE TABLE jira_pending_writes (
  action_key  TEXT PRIMARY KEY,                                           -- ExternalActionKey this service minted (SA-JIRA-01)
  handover_id TEXT NOT NULL REFERENCES handovers(id) ON DELETE CASCADE,
  write_kind  TEXT NOT NULL CHECK (write_kind IN ('create_issue','opening_comment','event_comment','label','assign')),
  event_id    TEXT,                                                       -- for event_comment
  created_at  TEXT NOT NULL
);
CREATE INDEX ix_jira_pending_handover ON jira_pending_writes(handover_id);
```

Rollback (sql-safety): this step rebuilds `handover_places` to widen its `CHECK` list, the only way SQLite changes a `CHECK`. It is a widening rebuild under DatabaseMigrations rule 12a, made safe as follows: every row is copied before the old table is dropped; `PRAGMA defer_foreign_keys` defers the `handovers.handover_place_id` reference check to commit, when every id exists again; the two child tables whose `ON DELETE CASCADE` the drop fires are copied to temporary tables first and restored after the rename. The registry applies the step inside its one transaction after the backup copy (DatabaseMigrations), so any failure rolls back to the unchanged file and the backup copy stays as the rollback. The `handovers` columns are additive with constant defaults; `jira_pending_writes` is new. Forward-only per SA-XC-07; no data is narrowed or lost. Migration test (*required here*): a database at step 2 holding places, handovers, `sharepoint_place_reads` and `followed_handover_places` rows migrates to step 3 with every row present and `PRAGMA foreign_key_check` empty.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A Jira place in a programme without the Jira Adapter (the server) | `Err(permanent)` | `HAND_PLACE_TYPE_UNAVAILABLE` | `{"error":"Jira handovers are available in ordin on your computer only.","code":"HAND_PLACE_TYPE_UNAVAILABLE","action":null}` |
| The Jira place is on a different Jira site from the one connected | `Err(permanent)` | `HAND_PLACE_SITE_OTHER` | `{"error":"This Jira project is on a different Jira site from the one you connected.","code":"HAND_PLACE_SITE_OTHER","action":null}` |
| ordin cannot create an issue in the project (no issue type it may use) | outcome (`SendFailed`) | `HAND_ISSUE_TYPE_NONE` | `{"error":"ordin can't create an issue in that project. Link an issue to this workspace first.","code":"HAND_ISSUE_TYPE_NONE","action":null}` |
| The opening comment would exceed 32,000 characters | `Err(permanent)` (draft line) | `HAND_JIRA_TOO_LONG` | `{"error":"This handover is too long for one Jira comment. Shorten the note or hand over fewer results.","code":"HAND_JIRA_TOO_LONG","action":null}` |
| Label or assignment still refused after 3 checks | outcome (screen line) | `HAND_JIRA_ASSIGN_FAILED` | `{"error":"ordin couldn't assign the issue in Jira; assign it there.","code":"HAND_JIRA_ASSIGN_FAILED","action":null}` |
| A record comment edited, written by someone else or repeated | outcome | — | Diagnostics (`handover.jira_comment_ignored`); the event is not counted |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Handover Service — Jira handovers

  Background:
    Given a Jira Cloud test double for "acme.atlassian.net" behind the Jira Adapter
    And Priya has connected Jira with account "712020:priya" and Sam is assignable in project "ABC" as "712020:sam"

  Scenario: A lifecycle stage with a Jira place
    Given stage "Design" of lifecycle "Supplier onboarding" names the place "https://acme.atlassian.net/jira/software/projects/ABC/boards/1"
    When Priya opens a draft in a workspace following "Analysis"
    Then the draft's place is "https://acme.atlassian.net/projects/ABC" with place_type jira

  Scenario: Receiver search lists only assignable people
    Given a draft whose place is the Jira project "ABC"
    When find_people is called with "sam"
    Then every choice comes from find_assignable for project "ABC" and carries a jira_account_id
    And find_people with "dan.moss@acme.co.uk", who is not assignable, returns no choices

  Scenario Outline: Checks fail and Hand over is unavailable
    Given a Jira draft where <condition>
    When the draft is checked
    Then <check> fails and can_hand_over is false
    Examples:
      | condition                                  | check              |
      | the receiver is no longer assignable       | receiver_reachable |
      | one result is a file on Priya's computer   | results_readable   |

  Scenario: Handing over on a linked issue
    Given the workspace links "https://acme.atlassian.net/browse/ABC-12"
    When Priya approves the hand-over card to Sam
    Then ABC-12 has an opening comment with the four headings and the results as links and a record code block
    And ABC-12 has one event comment with kind sent, by "jira:712020:priya" and an event_id
    And ABC-12 carries the label ordin-handover and is assigned to "712020:sam"
    And create_issue is not called and no email is sent

  Scenario: Handing over with no linked issue creates one
    Given the workspace links no issue in project "ABC"
    When Priya opens the hand-over card
    Then its changes say "ordin creates a new issue in ABC named "Northwind assessment"."
    When she approves it
    Then exactly one issue named "Northwind assessment" exists in "ABC"

  Scenario: The Jira place refuses the write
    Given the test double refuses add_comment with JIRA_ACCESS_DENIED
    When Priya approves the hand-over card
    Then the draft stays a draft, no sent event is recorded and SendFailed carries HAND_PLACE_REFUSED

  Scenario: Receiving a Jira handover
    Given ABC-12 is assigned to Sam, labelled ordin-handover and holds a valid record with a sent event by Priya
    And ABC-13 is assigned to Sam and labelled ordin-handover but holds no record comment
    When Sam's ordin checks for handovers
    Then search is called once for "acme.atlassian.net"
    And "Handed to you: Northwind assessment — from Priya Shah" is notified for ABC-12 only

  Scenario: Accepting links the issue
    Given Sam has opened the handover on ABC-12
    When Sam accepts into a new workspace
    Then an accepted event comment by "jira:712020:sam" is added
    And the new workspace has a JiraIssue link to "https://acme.atlassian.net/browse/ABC-12" and every Git or SharePoint result linked
    And handover.accepted is journalled

  Scenario: Returning assigns the issue back
    When Sam returns the handover with reason "Payment terms are missing" and approves the card
    Then a sent_back event comment with that reason is added and ABC-12 is assigned to "712020:priya"
    And at Priya's next check ReturnedToYou is notified

  Scenario: Two installations answer in the same second
    Given Priya takes the handover back while Sam accepts it, both comments created in the same second
    When either ordin next reads ABC-12
    Then both event comments are present in creation order, the first decides the state, and no event_id repeats

  Scenario: A receiver without ordin answers in Jira
    Given Dan has no ordin and is assigned ABC-14 by Priya's handover
    When Dan assigns ABC-14 to Priya with the comment "Need the signed copy"
    Then Priya's ordin shows the handover returned with reason "Need the signed copy"
    And nothing is written to ABC-14 by Priya's ordin

  Scenario: An edited or impersonating record comment is ignored
    Given an event comment with kind accepted whose by is "jira:712020:sam" but whose author is "712020:dan"
    And an event comment edited after it was created
    When the record is read
    Then neither event is counted and handover.jira_comment_ignored is logged twice

  Scenario: An unknown outcome is never resent blindly
    Given add_comment for the seen event returns JIRA_OUTCOME_UNKNOWN
    When Sam's ordin re-reads ABC-12's comments and finds that event_id
    Then the seen event is adopted and add_comment is not called again

  Scenario: Too long for one comment
    Given a Jira draft whose opening comment would be 33,000 characters
    When the draft is checked
    Then can_hand_over is false and the line says it is too long for one Jira comment

  Scenario: Git and SharePoint handovers are unchanged
    Given no lifecycle names a Jira place and no workspace links a Jira issue
    When every scenario of the Handover Service feature above runs
    Then each passes unchanged and no Jira Adapter method is called
    And a handover.yaml carrying jira, jira_account_id and event_id keys is read by an installation without this amendment, which keeps and ignores those keys

  Scenario: Migration step 3 keeps every row
    Given a database at handover step 2 with places, handovers, sharepoint_place_reads and followed_handover_places rows
    When step 3 is applied
    Then every row is present, place_type accepts 'jira' and PRAGMA foreign_key_check returns no row
```

### Performance, Security, Observability (additions)

- **Performance:** the Jira receiving step is one `search` plus one `comments` read per changed issue, ≤ 10 s for 50 open handovers on one site in the background (*chosen here*), inside the 30 s target above; sending makes at most six Jira calls (`myself`, `create_issue`, two `add_comment`, `add_label`, `assign`), ≤ 5 s p95 excluding Jira's response time (*chosen here*). Jira rate-limit responses are retried by the Jira Adapter with back-off and never lose an event: an event not written stays unwritten locally and is offered again.
- **Security:** only the person sends, returns or takes back, through the card, which states that everyone who can see the issue sees the note and the result links, and that a new issue is created when it is (R-105). Every comment, description and record read from Jira is data (R-81); an event counts only when its Jira author is the person it names, so a third person cannot answer for the receiver (R-104). The Jira token never passes through this service (the Jira Adapter obtains it from Identity and Credentials). Queries are built by the Jira Adapter from typed values; this service never writes JQL.
- **Observability (logs only):** `handover.sent`, `handover.received` and `handover.check_run` carry `place_type = jira`; added `handover.jira_check_failed {code}`, `handover.jira_comment_ignored {handover_id, reason}`, `handover.jira_answer_read {handover_id, kind}` (a receiver without ordin), `handover.jira_assign_retry {handover_id, attempt}`. Never logged: issue keys with summaries, comment text, account ids, reasons.

### Gaps

None.
