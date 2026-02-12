# SPEC: App Shell, Block Renderer (drawing side) · amended January 2026 (user decision; Approved by the user January 2026): one what-you-see editor wherever a person writes Markdown — the document editor, a knowledge page's body and an adopted page's body (SM-NS-4.6, the Edit of the ADR-104 amendment's item 3, SM-NS-13); an email draft keeps a plain text box

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-103) · amended January 2026 (ADR-104; promoted to Approved by the user January 2026) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-108; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (the dictation type is `DictationAvailability`) · amended January 2026 (ADR-115) · amended January 2026 (ADR-072) · amended January 2026 (FU-089, FU-127; approved by the user 2026-01-10) · amended January 2026 (FU-135, FU-136, FU-142; approved by the user 2026-01-10) · amended January 2026 (run-state message ids; Approved by the user 2026-01-10) · amended January 2026 (`confluencePublishing` wired to `workspaces.confluence_publishing`, SM-SF-10, closing G-SF-3; Approved by the user 2026-01-10) · amended January 2026 (screens wired to the commands of Phase 11, SM-WR-1 to SM-WR-10; Approved by the user 2026-01-10) · amended January 2026 (OQ-WR-1, OQ-WR-2 and OQ-WR-3 resolved, SM-WR-11 to SM-WR-14; Approved by the user 2026-01-10) · amended January 2026 (the crew plan's words and Start, SM-WR-15, closing SPEC-GAP-4 now that ServiceApi.md G-SCR-1 is closed; Approved by the user 2026-01-10, "proceed with recommendations") · amended January 2026 (ADR-116; promoted to Approved by the user January 2026) · amended January 2026 (ADR-117, icons; promoted to Approved by the user January 2026) · amended 2026-01-28 (the memory models download on first use, SM-MD-1 to SM-MD-6; Approved by the user 2026-01-28) · amended 2026-01-29 (the labels of a crew run's states, SM-RS-6 and SM-RS-7; Approved 2026-01-29, user decision: complete the high and medium follow-ups) · amended 2026-01-29 (the second-install step's words, SM-FS-8; Approved 2026-01-29, user decision: complete the high and medium follow-ups) · amended January 2026 (a task's form opens from the tree, SM-AT-2 and SM-TF-1 to SM-TF-8, ADR-119; Approved by the user January 2026) · amended January 2026 (ADR-118, ADR-120: facts as triples, words, conflicts, passages and the memory diagram; Approved by the user January 2026) · amended January 2026 (a team workspace: share and open dialogs, the access card, Team activity, Leave, SM-TW-1 to SM-TW-12, ADR-121; Approved by the user January 2026) · amended January 2026 (the screen is called Memory and its Words tab Kinds of fact, SM-SF-25; user decision) · amended January 2026 (the support note names the logs, SM-NS-3; a kind of fact named by a kind of note, SM-SF-26; the walk-through's web pages and server address, G-SW-1 and G-SW-3 closed; user decision) · amended January 2026 (Install Claude Code and Check again on first start, SM-FS-9; no sentence sends the person to IT; user decision) · amended January 2026 (user decision; Approved by the user January 2026): one way into sharing, the **Share** link on the workspace's tree row (SM-TW-13); knowledge-only sharing is no longer offered on its own, a workspace with nothing shared shows one pointer line (SM-TK-2) and a workspace that shares its knowledge only is offered **Share the whole workspace** (SM-TW-14) · amended January 2026 (user decision; Approved by the user January 2026) (Skip, Dismiss and Not now on what waits, SM-DIS-1) · amended January 2026 (user decision; Approved by the user January 2026): actions in menus, content in panes — sharing moves into the workspace's ⋯ menu and the row keeps a team icon (SM-TW-1, SM-TW-13); a document's actions move into its ⋯ menu in the tree and the document page shows its name, location and preview (SM-NS-4, SM-RL-4, SM-ASK-1); a PDF is drawn with pdf.js (SM-PDF-1) · amended January 2026 (user decision; Approved by the user January 2026) (the Facts list at scale and topics of things: SM-SF-4, SM-SF-12, SM-SF-27) · amended 2026-02-02 (ADR-123: the Copilot CLI is the person's own install; Approved by the user 2026-02-02)
> **Mode:** greenfield
> **derivedFromHld:** 0.72.0
> **Crate:** `apps/desktop/ui/` (screens module, TypeScript; npm package `@ordin/screens`, private) · **Build phase:** 12 · **Programs:** D
> **Dependencies:** Service API (generated TypeScript types)

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
- [Amendment January 2026 — Page links (ADR-104)](#amendment-january-2026--page-links-adr-104)
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)
- [Amendment January 2026 — Continue an earlier conversation (ADR-108)](#amendment-january-2026--continue-an-earlier-conversation-adr-108)
- [Amendment January 2026 — Ordin's language and language files (ADR-109)](#amendment-january-2026--ordins-language-and-language-files-adr-109)
- [Amendment January 2026 — Reading other teams' knowledge (ADR-110)](#amendment-january-2026--reading-other-teams-knowledge-adr-110)
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)
- [Amendment January 2026 — Releases and updates (ADR-114)](#amendment-january-2026--releases-and-updates-adr-114)
- [Amendment January 2026 — Screens for Phase 7A decisions and automatic publishing (FU-089, FU-127)](#amendment-november-2026--screens-for-phase-7a-decisions-and-automatic-publishing-fu-089-fu-127)
- [Amendment January 2026 — Catch-up of words and screen identifiers, the screens Phase 10 left out, and when an approval card opens (FU-135, FU-136, FU-142)](#amendment-november-2026--catch-up-of-words-and-screen-identifiers-the-screens-phase-10-left-out-and-when-an-approval-card-opens-fu-135-fu-136-fu-142)
- [Amendment January 2026 — Run-state message ids](#amendment-november-2026--run-state-message-ids)
- [Amendment January 2026 — The look of the screens (ADR-116)](#amendment-november-2026--the-look-of-the-screens-adr-116)
- [Amendment January 2026 — Icons (ADR-117)](#amendment-november-2026--icons-adr-117)
- [Amendment January 2026 — First start, Add to this workspace and removing a link](#amendment-november-2026--first-start-add-to-this-workspace-and-removing-a-link)

## Purpose

The screens module is everything the person sees in the ordin desktop window: the **App Shell** — side rail with Home, *Search or go to…* and the workspace tree, the screen area with one tab per open screen, the conversation panel with *Needs you* as its fixed first tab, Settings, Diagnostics and engineering mode's fixed views (02:313–330; 11 §4) — and the **Block Renderer's drawing side**, which draws every block of the fixed catalogue with the application's own code (02:332–339; ADR-028, ADR-046, ADR-067, ADR-068, ADR-070). It runs in the Tauri WebView, depends only on TypeScript types generated from `ordin-service-api` (2B-1 rule 6), holds one backend handle per backend it shows — the desktop's own core and each paired ordin server — and never knows where a backend runs (ADR-096 item 2). It holds no state that is not also in the journal, apart from view preferences (02:328).

## Approach

Svelte 5 components with runes (`$state`, `$derived`) over one event store per backend: the store applies Service API events in sequence order, reloads through commands on a gap or a `reset`, and every view derives what it draws from the store and from command results; nothing is drawn from markup supplied by data. All calls leave through one file, `lib/backend/tauri.ts`, the only importer of `@tauri-apps/api` (SA-UI-01). No component library (SA-UI-02): ordin's own components, styled only from design tokens. Words come from Fluent catalogues (`@fluent/bundle`, SA-I18N-01), never literals. Error-code prefix for the module's own diagnostics: `UI_` (*chosen here*; the person never sees one). Rejected: a global store library or a client-side router (Svelte's runes and one `ScreenArea` keyed by screen identifiers cover both, with no added dependency). Rejected: drawing blocks from HTML produced in the core (ADR-028: data, never markup).

### Reconciliation assumptions

- **SA-RECON-01** The views call the owners' command names that the Service API catalogue adopted (rulings R1, R6; reconciliation rows 177, 180; X5): `home.view`, `home.document_opened`, `home.run_again_inputs`, `home.needs_you`, the `ordin.*` conversation group, `approval.list_pending`, `identity.*` sign-ins, `catalogue.*` agent repositories, `setup.check_now`; there is no help command (plan 05 FU-019).
- **SA-RECON-02** Dictation (row 178) uses `dictation.get_availability`, `dictation.start { field }`, `dictation.stop` and `dictation.cancel { session_id }`; its `DictationUpdate` values arrive on the Command Bridge's `service_streams` channel as `StreamMessage { stream: <session_id>, kind: "dictation", payload: DictationUpdate }`, never as journalled events; terminal bytes and language-server messages arrive on the same channel with `kind` `terminal` and `lsp`.
- **SA-RECON-03** *Amended January 2026 (ADR-109; SM-LG-1):* message catalogue text comes from the core with `localisation.get_catalogue`, never bundled into the screens at build time, as Localisation's spec states (row 76, N16) — the earlier wording here ("stay bundled at build time", O19) contradicted it and could not show a language the person edits, makes or imports. At start the module reads the active locale from `localisation.get_settings` and the brand's string overrides and design tokens from `brand.get`, and re-reads them on `locale_changed` and `brand_changed` (N16, N34, row 179).
- **SA-CLOSE-01** **Check it** on *What [agent] is made of* and on every part page calls the Checker's commands as the Service API restates them: `checker.check_installed { agent_instance_id }` for an installed agent, `checker.check_library_item { agent_repository_id, kind, identifier }` for a library part (`agent_repository_id` absent for the bundled catalogue), and `checker.report { check_run_id }` to show an earlier result again (closes O165).
- **SA-CLOSE-02** `sign_in_changed` carries `credential_kind` (ServiceApi.md SA-CLOSE-02); the store reads `credential_id`, `credential_kind` and `state`.
- **SA-CLOSE-03** Settings › Sign-ins offers GitLab by `identity.sign_in { kind: "gitlab" }` when the organisation profile names a GitLab OAuth application, else by a token box that sends `identity.save_gitlab_token { token }` (the box is a password field, cleared after sending); and, on the desktop when the profile requires a client certificate, **Import a certificate…**, which sends `identity.import_client_certificate {}` (the desktop program opens the file pickers). Which GitLab form to show comes from `identity.sign_in` itself: a reply `CRED_PROVIDER_NOT_CONFIGURED` for `gitlab` switches the row to the token box *(chosen here)*.
- **SA-CLOSE-04** *(chosen here)* A project's check commands are set on the project's settings screen, which sends `projects.set_check_commands { project_id, commands }` (ServiceApi.md SA-CLOSE-07, ProjectService.md SA-CLOSE-03) when the engineer presses **Save**; the screen shows the project's current `check_commands` from the `Project` the command returns (or from `projects.open`), one command per line, and, when the list is empty, `project-checks-none` = "No checks set". No other screen and no program sets them.
- **SA-RECON-04** The version sentence follows `ServerView.older_side` (`desktop` or `server`), which the Server Connector fills (N36); the error line shows `ServiceError.reference` when present (N35).

## Shared Context

### Libraries (1A; versions fixed there)

`svelte` 5.57.2, `vite` 8.3.4, `@sveltejs/vite-plugin-svelte` 7.3.1, `typescript` 6.0.3 (no `svelte-check` release supports TypeScript 7; the peer range is `^5 || ^6`), `dompurify` 3.4.17 through `overrides` in `package.json` (the release that fixes every advisory `monaco-editor` 0.56.0 carries), `@tauri-apps/api` 2.12.4, `@fluent/bundle` 0.19.1, `echarts` 6.1.0 (SA-UI-03, provisional until C-40), `@xterm/xterm` 6.0.0 (SA-TERM-01), Monaco Editor (05 *Code editor*; version pinned at lock time, 05 *Build / test toolchain*). Build-time only (05 *Build / test toolchain*): `vitest` (tests), `svelte-check` (type check of `.svelte` files). TypeScript types are generated by the Service API crate's own generator (`ordin-service-api-gen`, Service API spec), so no npm generator is added.

### Shared types (generated from `ordin-service-api`; copied here as TypeScript; JSON is `snake_case`)

```ts
export type BackendId = string;          // UUIDv7, 36 characters
export type CommandName = `${string}.${string}`;
export interface ServiceApiVersion { major: number; minor: number }   // CURRENT = { major: 1, minor: 0 }

export interface CommandEnvelope<I> {
  api_version: ServiceApiVersion; request_id: string; command: CommandName; backend: BackendId; input: I;
}
export type CommandOutcome<O> = { kind: 'ok'; result: O } | { kind: 'error'; error: ServiceError };
export interface CommandReply<O> { request_id: string; /* flattened */ }   // CommandReply<O> = { request_id } & CommandOutcome<O>
export interface ServiceError {
  code: string; message: string; action?: 'try_again' | 'sign_in_again';
  reference?: string;        // Diagnostics line id, 1–64 characters, shown as "Reference r-81"
}

export interface EventEnvelope {
  api_version: ServiceApiVersion; backend: BackendId; seq: number; at: string;  // RFC 3339 ms Z
}                                                                                // & ServiceEvent (flattened, tag "kind")
export type ServiceEvent =
  | { kind: 'run_state_changed'; run_id: string; state: RunState }
  | { kind: 'run_progress'; run_id: string; progress: { line: string; count?: { done: number; total?: number } } }
  | { kind: 'plan_changed'; run_id: string; plan_version: number; steps: RunStep[] }
  | { kind: 'step_marked'; run_id: string; plan_version: number; position: number; mark: 'next' | 'now' | 'done' | 'skipped' }
  | { kind: 'start_up_report_ready'; subject: StartUpSubject; items: StartUpReportItem[] }
  | { kind: 'approval_requested'; card: ApprovalCard }
  | { kind: 'approval_decided'; approval_id: string; state: ApprovalState; decided_on: string }
  | { kind: 'crew_round_changed'; crew_run_id: string; round: number; state: CrewRunState }
  | { kind: 'block_shown'; workspace_id: string; block_id: string; block: Block }
  | { kind: 'reset' }
  // added in 1.0 by the Service API component spec:
  | { kind: 'server_state_changed'; server_connection_id: string; state: 'connected' | 'not_connected' | 'disconnected' | 'refused_for_version'; as_of?: string }
  | { kind: 'workspace_changed'; workspace_id: string }
  | { kind: 'needs_you_changed'; workspace_id: string }
  | { kind: 'block_available'; workspace_id: string; block_id: string }
  | { kind: 'locale_changed'; active: ActiveLocale }
  | { kind: 'language_changed'; locale: string; change: LanguageChangeKind }   // amended January 2026, ADR-109
  | { kind: 'brand_changed'; brand_identifier: string; version: string; source: 'build' | 'organisation_profile' }   // desktop-local
  | { kind: 'setup_progress'; progress: unknown }                     // read only as a signal: reload with setup.status
  | { kind: 'sign_in_changed'; credential_id: string; credential_kind: CredentialKind; state: 'signed_out' | 'signed_in' | 'needs_sign_in' };  // reload with identity.list

// Localisation.md, restated by the Service API (event locale_changed, command localisation.get_settings)
export interface ActiveLocale {
  locale: string; source: 'operating_system' | 'person'; direction: 'ltr' | 'rtl'; fallback_chain: string[];
}
export interface OfferedLocale { locale: string; native_name: string; direction: 'ltr' | 'rtl' }

// Server Connector / Service API servers.* result
export interface ServerView {
  server_connection_id: string; display_name: string;
  state: 'connected' | 'not_connected' | 'disconnected' | 'refused_for_version';
  as_of?: string; backend?: BackendId;
  older_side?: 'desktop' | 'server';   // present only when state is refused_for_version
}

// Dictation Service (desktop only), and the Command Bridge's unjournalled stream
export type FieldRef = string;          // chosen by the screens, ≤ 128 characters
export type DictationAvailability =   // amended January 2026: named apart from the Package Catalogue's Availability
  | { state: 'available'; locale: string }
  | { state: 'needs_assets'; locale: string }
  | { state: 'unavailable'; reason: string; message: string };
export type DictationUpdate =
  | { kind: 'preparing'; session_id: string }
  | { kind: 'listening'; session_id: string }
  | { kind: 'partial'; session_id: string; text: string }   // replaces the provisional tail
  | { kind: 'final'; session_id: string; text: string }     // appended, no longer replaced
  | { kind: 'ended'; session_id: string; reason: 'stopped' | 'cancelled' | 'replaced' | 'silence' | 'time_limit' | 'locale_changed' | 'failed' };
export interface StreamMessage { stream: string; kind: 'dictation' | 'terminal' | 'lsp'; payload: unknown }

export type RunState = 'queued' | 'running' | 'paused' | 'completed' | 'failed' | 'stopped_at_limit'
  | 'cancelled' | 'interrupted' | 'abandoned' | 'no_reply';
export type ApprovalState = 'pending' | 'approved' | 'rejected' | 'expired';
export type CrewRunState = 'starting' | 'running' | 'paused' | 'finished' | 'escalated' | 'stopped_at_limit' | 'killed' | 'failed';
export type StartUpSubject = { kind: 'run'; run_id: string } | { kind: 'crew_run'; crew_run_id: string };
export type StartUpReportItem =
  | { kind: 'enforced_rule'; rule: unknown; plain: string }
  | { kind: 'rule_not_enforced'; rule: unknown; plain: string; reason: 'not_applicable_to_local_session' | 'route_cannot_carry' }
  | { kind: 'unavailable_part'; part: string; plain: string; reason: 'not_trusted' | 'missing_dependency' | 'harness_cannot_run' }
  | { kind: 'available_tool'; tool: string; origin: unknown; plain: string }
  | { kind: 'verb_not_offered'; adapter: string; verb: string; plain: string }
  | { kind: 'own_screens_not_shown'; server: string; plain: string };

export interface ApprovalCard {
  approval_id: string; backend: BackendId; workspace_id: string;
  asked_by: { kind: 'run'; run_id: string } | { kind: 'crew_round'; crew_run_id: string; round: number } | { kind: 'person' };
  question: string;          // "[Verb] [object]?", 1–120 characters
  verb: string;              // approve button label, 1–32 characters
  where_line: string;
  affects: Array<{ kind: 'file'; label: string; version?: string; place_id?: string }
               | { kind: 'record'; label: string }
               | { kind: 'recipient'; name: string; address: string; outside_company: boolean }>;
  changes: string;
  content?: ApprovalContent;
  reject_style: 'reject' | 'not_now';
  buttons: { approve: string; reject: string; show_details?: string };   // CardRef, 22 characters
  created_at: string;
}
export type ContentRef = { kind: 'artefact'; artefact_id: string } | { kind: 'place_file'; place_id: string; relative_path: string }
  | { kind: 'file_version'; file_version_id: string };   // amended January 2026 (FU-089): as the generated type carries it
export type ApprovalContent =
  | { kind: 'message'; label: string; text: string; destination_note?: string }
  | { kind: 'command'; label: string; command: string; runs_in: string }
  | { kind: 'changes'; label: string; change_set: ContentRef }
  | { kind: 'page'; label: string; page: ContentRef }
  | { kind: 'files'; label: string; files: ContentRef[] }
  | { kind: 'email'; label: string; email: EmailApproval };
export interface ApprovalPress { card_ref: string; decision: 'approve' | 'reject' }   // command approval.decide
```

`Block` is the tagged union of 2C wire §7 with `kind` ∈ `message`, `question`, `form`, `documents`, `preview`, `document`, `changes`, `plan_progress`, `table`, `chart`, `key_figures`, `report`, `email_list`, `email`, `telegram_list`, `result`, `agent_proposal`, `agent_library`, `crew_plan`, `memory`, `knowledge` (and `access`, SM-FA-3); its fields are those 2C fixes and the generated file carries them (for example `TableBlock { title, columns: { name, type, … }[], rows: Cell[][], source: PartSource }`, cells `null | number | string | ContentRef`; `ChartBlock { title, chart, …, columns, rows, x, series, x_title?, y_title?, source }`; `ReportBlock { title, as_of, parts: { heading, content }[] }`; `PartSource` = `documents` | `company_system` | `earlier_result` | `worked_out`). Limits the drawing side relies on (A-34, LW-29–LW-42): table ≤ 10,000 rows × 30 columns; chart ≤ 8 series and ≤ 5,000 points, pie ≤ 6 slices; key figures 1–8; report 1–12 parts, ≤ 2 MB; plan 3–7 steps of ≤ 80 characters; question 2–4 choices.

Slack types (amended January 2026, ADR-102):

```ts
type SlackLink = { id: string; workspace_id: string; team_id: string; channel_id: string; name: string;
  private: boolean; ordin_reads: boolean; state: "linked" | "no_access"; linked_at: string; read_up_to_ts?: string };
type SlackRule = { id: string; kind: "handover_notices" | "ordin_answers";
  scope: { kind: "stage"; lifecycle_id: string; stage_name: string } | { kind: "link"; slack_link_id: string };
  channel_id: string; channel_name: string; daily_cap: number; state: "on" | "off"; created_at: string; turned_off_at?: string };
type SlackReach = { kind: "found" } | { kind: "not_found" } | { kind: "not_signed_in" }
  | { kind: "scope_needed" } | { kind: "not_allowed" };
type StageSlackChannel = { channel_id: string; name: string };
type DraftSlackView = { reach: SlackReach; tell_in_slack: boolean; stage_channel?: StageSlackChannel; stage_rule?: string };
// HandoverDraftView.slack: DraftSlackView[] — one per handover of the draft group (side by side: one per receiver and stage), in the draft group's order
// ReturnInput.reason_in_slack: boolean (default true) — whether the stage-channel post of a return carries the reason
// HandoverScreen.slack: SlackLine[] — the Slack notices of this handover, as the Handover Service (F2) defines SlackLine
type LinkOpened = { kind: "opened"; handover_id: string } | { kind: "not_found_yet"; place: string };
type SlackListBlock = { slack_link_id: string };     // Block kind "slack_list"
```

The generated `SlackStatus`, `OfferedSlackChannel`, `SlackLinkSummary` and `SlackMessageOut` are drawn as their owner (`SlackService.md`) defines them; this module reads only the fields named in the table below.

### Rules from the HLD the module enforces on screen

- No block kind approves, sends, posts or publishes; approval cards come only from `ApprovalRequested` (Approval Service), never from a block (02:333; 11 §5.15).
- Text is always drawn as text; a string that looks like markup is shown literally; links are only `ContentRef`s, never URLs (2C §7).
- The person never sees: block kinds, codes, paths, tool names, model names, versions, rule identifiers, verb names, port numbers, keys, certificates, the words *API*, *stream*, *tunnel*, *pairing* (11 §3, §5.31). Diagnostics is the one view that shows technical names (02:326).
- Status is never carried by colour alone: every run state has a label and an icon (11 §10).
- Run state labels and next actions (11 §8; stored states from 04, SA-HLDCONFLICT-01 reading 1D-2):

| Stored state (or condition) | Label | Next action |
|---|---|---|
| `queued` | Waiting to start | Cancel |
| `running` | Working | Stop |
| `paused` with an approval pending for the run | Waiting for you | Review |
| `stopped_at_limit` | Stopped — reached its limit | See results so far |
| `failed` | Couldn't finish | Try again |
| `interrupted` | Interrupted | Try again |
| `no_reply` | No answer yet | Open the conversation |
| `paused` with an approval pending in Slack | Waiting for you in Slack | Open in Slack |
| `abandoned` | Not used (stopped crew) | — |
| external action of unknown outcome | Not sure this happened | Review what was sent |
| `completed` | Done | Open results |
| `paused` after ordin was closed | Paused while ordin was closed | Continue |
| `cancelled` | Stopped *(assumed here: 11 §8 has no row; the label of the person's own Stop)* | — |

- "Posting in Slack as the person always says so" (11 §3, ADR-008): every Slack line on a card and on the handover draft names "as you".
- A Slack message is from other people: text through Svelte text interpolation only; a message with `asks_agent_to_act` true carries the note `slack-message-instruction-note` (as `telegram_list`, R-81).
- Fixed sentence of Settings › Slack, always shown first.

### Responsiveness and accessibility targets (11 §9, §10; 07 NFR)

Actions that wait for no model respond in ≤ 150 ms p95; the conversation is interactive within 3 s of opening; a local progress event reaches the screen within 1 s; a server event within 500 ms of the server's journal recording it (A-46). WCAG 2.2 AA outside the code editor; every action by keyboard; F6 / Shift+F6 cycle tree, screen and conversation; WAI-ARIA tabs, tree and combobox patterns; reduced motion and light/dark from the operating system unless Settings › Display says otherwise; right-to-left locales mirrored.

## Public Interface

The module's public interface is what it requires from the core (it exposes nothing to other modules).

### Module structure (*chosen here*)

```text
apps/desktop/ui/
  package.json                 name "@ordin/screens", private; scripts: dev, build, check, test, gen:api, gen:check
  index.html  vite.config.ts  svelte.config.js  tsconfig.json
  src/
    main.ts                    mounts App into #app
    App.svelte                 shell layout; F6 regions; narrow-window mode
    lib/service-api/schema.gen.json   written by ordin-service-api-gen (committed, never edited)
    lib/service-api/types.gen.ts      one type per schema definition (generated)
    lib/service-api/commands.gen.ts   API_VERSION, Commands map, IPC_NAMES, ServiceEvent (generated)
    lib/backend/backend.ts     BackendHandle interface, Backends registry
    lib/backend/tauri.ts       the only file importing @tauri-apps/api
    lib/store/backend-state.svelte.ts   BackendState (one per backend)
    lib/store/view-prefs.ts    localStorage view preferences
    lib/i18n/fluent.ts         catalogue loading, t(), brand overlay
    lib/format/intl.ts         numbers, money, percentages, dates, lists, plurals (Intl)
    lib/blocks/BlockView.svelte         dispatch on block.kind
    lib/blocks/kinds/<Kind>.svelte      one per block kind (21)
    lib/blocks/OneLineCard.svelte       the conversation's card for a block shown on a screen
    lib/approval/ApprovalScreen.svelte  the fixed approval card
    lib/shell/SideRail.svelte  WorkspaceTree.svelte  ScreenArea.svelte  ScreenTabs.svelte
    lib/shell/ConversationPanel.svelte  NeedsYouTab.svelte  MessageBox.svelte
    lib/views/Home.svelte  SearchDialog.svelte  Settings/*.svelte  Diagnostics.svelte
    lib/views/ConfirmDialog.svelte  QuitDialog.svelte  ErrorLine.svelte  EarlierConversations.svelte
    lib/views/engineering/CodeEditor.svelte  ChangesWaiting.svelte  Terminal.svelte
    lib/views/fixed/*.svelte   the other fixed screens of 11 §5.15
    styles/tokens.css          design tokens (brand pack values injected at start)
  tests/                       vitest
```

Generation: `npm run gen:api` runs `cargo run -p ordin-service-api --bin ordin-service-api-gen -- write .`; `npm run gen:check` runs the same binary with `check .` and fails on stale files (also run by `make check`). `BackendHandle.call` is typed by the generated `Commands` map and sends to `IPC_NAMES[command]`.

### Backend handle

```ts
// lib/backend/backend.ts
export interface BackendHandle {
  readonly id: BackendId;
  readonly kind: 'local' | 'server';
  call<C extends CommandName>(command: C, input: InputOf<C>): Promise<CommandOutcome<ResultOf<C>>>;
}
export class Backends {
  readonly local: BackendHandle;
  servers(): BackendHandle[];                  // from the paired servers the local core reports
  get(id: BackendId): BackendHandle | undefined;
}
```

Every handle — local or server — calls the local Command Bridge; the Bridge routes a server's command to the Server Connector (ADR-096 item 2). The WebView makes no network call and holds no server key.

### What the module requires from the Command Bridge (Tauri plugin `ordin`)

| Need | Form (as this module requires it) | Defined in |
|---|---|---|
| Send a command | `invoke('plugin:ordin|<ipc_name>', { envelope })` → the `CommandReply` JSON; `ipc_name` is `<module>_<verb>` (SA-API-01); the reply always resolves, errors travel inside it | Command Bridge spec |
| Receive events | `invoke('plugin:ordin|service_events', { after: { <backend id>: <last seq drawn> }, onEvents: Channel<EventBatch> })` → `EventsOpened { local_backend, reset: BackendId[] }`; each `EventBatch.events` element is one `EventEnvelope`, in delivery order; a backend listed in `reset` is reloaded by command | Command Bridge spec |
| Server connection state | `server_state_changed` events on the local backend, and `servers.list` → `ServerView { server_connection_id, display_name, state, as_of?, backend? }` to map a connection to its backend | Service API spec |
| Which side to update when versions differ | `ServerView.older_side` (`desktop` \| `server`), present when `state` is `refused_for_version` | Service API spec; filled by the Server Connector |
| Unjournalled streams (dictation, terminal, language server) | `invoke('plugin:ordin|service_streams', { onStream: Channel<StreamMessage> })`, opened once at start by the main window; messages are matched to their command by `stream` (the `session_id` or `StreamId` the command returned) | Command Bridge spec |
| Desktop-only events | `listen('ordin://quit-requested')`, `listen('ordin://open-target')`, `listen('ordin://appearance')` | OrdinDesktop |
| Desktop-only commands | `app.quit`, `app.pick_folder` (scope `Local`) | OrdinDesktop; Service API catalogue |
| The active locale and the offered locales | `localisation.get_settings` → `{ active: ActiveLocale, offered: OfferedLocale[], following_operating_system }`; event `locale_changed` | Localisation spec; Service API catalogue |
| The brand's string overrides and design tokens | `brand.get` → `{ brand_identifier, version, source, strings, design_tokens, links, font_count, refusal }` (scope `Local`); event `brand_changed` | Brand spec; Service API catalogue |
| Everything the views read or start | the Service API catalogue 1.0 (`ServiceApi.md`), by the command names listed per view in *Internal Logic* step 9 | Service API spec |

### Message catalogue entries this module draws (en-GB source; ids *chosen here* under SA-I18N-00's pattern)

Wording is 11's where 11 quotes it; rows marked ‡ are *chosen here* where 11 quotes none. Rows marked § are **safety wording** (amended January 2026, ADR-109): the en-GB file carries a comment line `safety` attached to each, so Localisation's `SAFETY_IDS` holds them beside every `approval-` message, and a drafted or imported language shows them only after the person checks them. "Ordin" and "ordin" are brand strings (ADR-041), never literals.

| Id | en-GB text | Source |
|---|---|---|
| `rail-home` | Home | 11 §4 |
| `rail-home-count` | Home, { $count } new | 11 §10 |
| `rail-search` | Search or go to… | 11 §4 |
| `rail-new-workspace` | New workspace | 11 §4 |
| `rail-collapse-all` / `rail-expand` | Collapse all / Expand | 11 §4 |
| `rail-back` | Back to your workspace | 11 §4 |
| `rail-server-on` | On { $server } | 11 §5.31 |
| `rail-server-not-connected` | Not connected · as of { $time } | 11 §5.31 |
| `rail-show-documents-code` | Show: Documents · Code | 11 §4 |
| `tabs-more` | More tabs ({ $count }) | 11 §4 |
| `conv-tab-close` | Close tab · the conversation is kept | 11 §4 |
| `conv-needs-you` | Needs you | 11 §4 |
| `conv-header-conversation` | Conversation | 11 §4 |
| `conv-summarise` / `conv-clear` / `conv-earlier` | Summarise / Clear / Earlier | 11 §5.12 |
| `conv-earlier-name` | Earlier conversations | 11 §5.12 |
| `conv-summarised` | Earlier messages ({ $count }) — summarised | 11 §5.12 |
| `conv-new-started` | New conversation · started { $time } | 11 §5.12 |
| `conv-long-nudge` | This conversation is long — summarise or clear it to keep Ordin quick | 11 §5.12 |
| `card-close` | Close | 11 §3, §4 |
| `card-open` | Open | 11 §4 |
| `card-waiting` | Waiting for you | 11 §4 |
| `card-check-and-press` § | Check it and press “{ $verb }” | 11 §5.5 |
| `card-decided` § | Approved: { $verb }. | 11 §5.5 |
| `check-it` | Check it | 11 §5.30, UX-A63 |
| `check-checked-by-ordin` | Checked by ordin | 11 §5.30 |
| `check-checked-at` | Checked { $time }, against the version in use | 11 §5.30 |
| `check-all-passed` | Every check passed. | 11 §5.30 |
| `signin-gitlab-paste-token` | Paste a GitLab access token | *(chosen here, SA-CLOSE-03)* |
| `signin-import-certificate` | Import a certificate… | *(chosen here, SA-CLOSE-03)* |
| `project-checks-title` | Check commands | *(chosen here, SA-CLOSE-04)* |
| `project-checks-none` | No checks set | *(chosen here, SA-CLOSE-04)* |
| `project-checks-add` | Add a check command | *(chosen here, SA-CLOSE-04)* |
| `approval-from-ordin` | From ordin — built from the action, not written by an agent | 11 §5.5 |
| `approval-where` / `approval-affects` / `approval-changes` | Where: / Affects: / Changes: | 11 §5.5 |
| `approval-reject` / `approval-not-now` | Reject / Not now | 11 §5.5 |
| `approval-last-line` | Nothing is sent or changed outside ordin until you choose. | 11 §5.5 |
| `approval-show-whole` | Show the whole message | 11 §5.5 |
| `approval-verb-read-start` | { $verb } — you've read the start | 11 §5.5 |
| `approval-decided-on` | Decided on { $place }, { $time } | 11 §5.31 |
| `approval-decided-in` | Decided in { $chat }, { $time } | 11 §5.31 |
| `approval-outside-company` | Outside the company | 11 §3 |
| `confirm-cancel` | Cancel | 11 §5.5 |
| `confirm-clear-title` | Clear this conversation? | 11 §5.5, §5.12 |
| `confirm-clear-keep-points` | Keep the key points first | 11 §5.12 |
| `confirm-remove-workspace-title` | Remove workspace? | 11 §5.5 |
| `confirm-remove-workspace-body` | Its links, conversation and history are removed. Your documents stay where they are. | 11 §5.5 |
| `confirm-stop-crew-title` | Stop crew? | 11 §5.5 |
| `quit-title` ‡ | Quit { $product }? | — |
| `quit-body` ‡ | { $count } agents are still working. They stop now; what they have written so far is kept. | — |
| `quit-verb` ‡ | Quit | — |
| `progress-now` | Now: { $sentence } — step { $n } of { $total } | 11 §5.4 |
| `progress-waiting` | Waiting for you: { $what } | 11 §5.4 |
| `progress-running-for` | Running for { $minutes } min | 11 §5.4 |
| `progress-last-checked` | last checked { $seconds } seconds ago | 11 §5.4 |
| `progress-step-done` / `-now` / `-next` / `-skipped` | done / now / next / skipped | 11 §5.4 |
| `progress-plan-changed` | Plan changed: { $what } | 11 §5.4 |
| `progress-show-what-happened` | Show what happened | 11 §5.4 |
| `progress-no-activity` | Done — the agent reports no activity for this step | 11 §5.4 |
| `progress-count-of` / `progress-count-so-far` | Items worked through: { $done } of { $total } / Actions taken so far: { $done } — the count is the agent's own tally of items or actions, never the number of steps, so it names what it counts and is not confused with the step list beside it (amended January 2026; user decision; Approved by the user January 2026) | 11 §5.4 |
| `progress-stop-note` | Stop — files written so far are kept | 11 §5.4 |
| `progress-run-again` / `progress-every-week` | Run again… / Every week… | 11 §5.4, §5.17 |
| `state-<stored state>` | the labels of the run-state table above | 11 §8 |
| `startup-heading` | What this agent will and won't do | 11 §3 |
| `report-as-of` | as of { $time } | 11 §3 |
| `report-sources` | Where these numbers come from | 11 §3 |
| `report-show-as-table` | Show as table | 11 §5.15 |
| `home-waiting` / `home-running` / `home-since` | Waiting for you / Running now / Since you last looked | 11 §5.17 |
| `home-new` | new | 11 §5.17 |
| `home-all-workspaces` | All workspaces | 11 §5.17 |
| `home-nothing-running` | Nothing running now | 11 §5.17 |
| `home-show-progress` | Show progress | 11 §5.17 |
| `search-go-to` / `search-found` / `search-actions` | Go to / Found in pages and documents / Actions | 11 §5.17 |
| `search-footer` | ↑↓ to move · Enter to open · Esc to close | 11 §5.17 |
| `search-other-workspace` | To use this here, add it to this workspace. | 11 §5.17 |
| `server-connect` | Connect to a server… | 11 §5.31 |
| `server-address` | Its address | 11 §5.31 |
| `server-code` | The code from whoever runs your ordin server | 11 §5.31 |
| `server-connect-verb` | Connect | 11 §5.31 |
| `server-code-failed` | That code didn't work. Ask for a new one from whoever runs your ordin server. | 11 §5.31 |
| `server-waiting-for` | Waiting for { $server } | 11 §5.31 |
| `server-disconnect` | Disconnect this computer… | 11 §5.31 |
| `server-disconnect-title` | Disconnect this computer from { $server }? | 11 §5.31 |
| `server-disconnect-body` | The server keeps working. You can connect again with a new code. | 11 §5.31 |
| `server-remove` | Remove this server | 11 §5.31 |
| `server-version-server-older` | { $server } is older than ordin. Some things won't work until it's updated. | 11 §5.31; amended January 2026 (user decision; Approved by the user January 2026) |
| `server-version-desktop-older` | Update ordin on this computer to use { $server }. | 11 §5.31 |
| `server-no-local-folders` | Folders on this computer can't be added to a workspace on { $server }. | 11 §5.31 |
| `server-open-copy` | Open a copy in Word | 11 §5.31 |
| `server-open-copy-note` | This saves a copy on this computer. Changes to the copy stay here. | 11 §5.31 |
| `server-no-code-views` | Code views aren't available for workspaces on a server yet. | 11 §5.31 |
| `server-cannot-reach` | Can't reach { $server } right now. ordin will keep trying while it's open. | 11 §6 |
| `server-ended` | This computer is no longer connected to { $server }. | 11 §6 |
| `server-connect-again` | Connect again… | 11 §6 |
| `error-try-again` / `error-sign-in-again` | Try again / Sign in again | 11 §6 |
| `error-reference` | Reference { $reference } | 11 §6 |
| `dictation-preparing` | Getting dictation ready… | DictationService (`Preparing`) |
| `dictation-failed` | Dictation stopped. You can try again. | DictationService step C6 |
| `settings-section-<name>` | Sign-ins, Agent repositories, Email, Telegram, Slack, Jira, Servers, Dictation, How Ordin talks to you, Language, Using your plan, Notifications, History, Display, Code, Memory, Diagnostics | 11 §4; amended by ADR-102, ADR-106 and ADR-109; `settings-section-what-ordin-remembers` reads Memory (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25) |
| `settings-history-line` | Keep conversations and history for { $days } days | 11 §5.12 |
| `settings-history-invalid` | Enter a whole number of days, 1 or more. | 11 §5.12 |
| `settings-history-only-way` | This is the only way earlier conversations are removed. | 11 §5.12 |
| `settings-appearance` | Appearance: Same as this computer / Light / Dark | 11 §4 |
| `block-cannot-show` ‡ | This part can't be shown. | — (11 §5.15 "the person never sees a broken or empty item") |
| `settings-slack-fixed` ‡ | Every post is shown to you before it is posted, unless you set a rule for it. | decision 82 |
| `settings-slack-signed-in` ‡ | Signed in to { $workspace } as { $name }. | — |
| `settings-slack-sign-in` ‡ | Sign in to Slack… | 11 §5.5 sign-in pattern |
| `settings-slack-own-app` ‡ | Connect your own Slack app… | decision 82 |
| `settings-slack-own-app-steps` ‡ | Create an app in your Slack workspace from this description, then paste its client ID here. | — |
| `settings-slack-not-approved` ‡ | Your Slack workspace needs an administrator to approve apps, so ordin can't connect. Handovers still go by email. | R-06 response (amended) |
| `settings-slack-rules` ‡ | Posted without asking | — |
| `settings-slack-rule-line` ‡ | { $kind } in #{ $channel } · up to { $cap } a day · { $state } | — |
| `settings-slack-rule-off` ‡ | Turn off | — |
| `settings-slack-rules-off-by-company` ‡ | Your company asks that every Slack post is shown to you first. | `slack.rules_allowed = false` |
| `settings-slack-more-permissions` ‡ | To send someone a Slack message and find them by their email address, ordin needs two more Slack permissions. | SA-SLACK-02 |
| `slack-add-to-workspace` ‡ | Slack channel… | Add to this workspace |
| `slack-link-kept-from` ‡ | Messages from { $date }, when you linked it | ADR-075 pattern |
| `slack-link-older-not-read` ‡ | Some older messages weren't read. | SA-RATE-02 |
| `slack-link-no-access` ‡ | You're no longer in #{ $channel }, so ordin can't read it. | `state = no_access` |
| `slack-ordin-reads` ‡ | Let Ordin look at new messages | ADR-102 |
| `slack-ordin-reads-note` ‡ | Ordin reads new messages while ordin is open and asks you before it posts. | ADR-011 |
| `slack-message-instruction-note` ‡ | This message asks ordin to do something. It's shown as information only. | R-81 |
| `handover-slack-tell` ‡ | Also tell them in Slack | HLD 11 §5.25a |
| `handover-slack-not-found` | Not found in Slack — they'll get an email | ADR-102 |
| `handover-slack-not-signed-in` ‡ | Sign in to Slack to tell them there too. | — |
| `handover-slack-not-allowed` ‡ | Slack isn't available here, so they'll get an email. | HLD 11 §5.25a |
| `handover-slack-stage-channel` ‡ | Also posted as you in #{ $channel } | HLD 11 §5.25a |
| `handover-slack-stage-rule` ‡ | Posted in #{ $channel } without asking (your rule) | HLD 11 §5.25a |
| `handover-slack-set-rule` ‡ | Post handover notices in #{ $channel } without asking… | HLD 11 §5.25a |
| `handover-slack-set-rule-confirm` ‡ | Post notices of { $stage } handovers in #{ $channel } as you, without asking, up to 20 a day? You can turn this off in Settings › Slack. | HLD 11 §5.25a |
| `slack-ordin-answers-rule-confirm` ‡ | Post Ordin's answers in #{ $channel } as you, without asking, up to 20 a day? You can turn this off in Settings › Slack. | HLD 11 §5.25a |
| `handover-slack-posted` ‡ | Posted in #{ $channel } · Open in Slack | — |
| `home-open-handover-link` ‡ | Open a handover link… | C-85 fallback |
| `home-open-handover-link-hint` ‡ | Paste the link from the Slack message or email. | — |
| `home-handover-link-following` | ordin now follows { $place }; the handover will appear when it's readable. | HandoverService.md |
| `handover-slack-reason-in-post` ‡ | Include the reason in the Slack post | HLD 11 §5.25a |
| `lifecycle-stage-slack-channel` ‡ | Slack channel for this stage (optional) | ADR-102 |
| `team-share-with-team` ‡ | Share knowledge with a team… (*removed — amended January 2026 (user decision; Approved by the user January 2026), SM-TW-13: knowledge-only sharing is no longer offered on its own*) | ADR-103 |
| `team-share-dialog-title` ‡ | Share this workspace's knowledge with a team (*removed — amended January 2026 (user decision; Approved by the user January 2026), SM-TW-13*) | ADR-103 |
| `team-share-dialog-pick` ‡ | Choose a folder in a GitHub or GitLab repository | ADR-103 |
| `team-share-dialog-address` ‡ | Or paste the folder's address from GitHub or GitLab | ADR-103 |
| `team-share-dialog-address-invalid` ‡ | Paste the address of a folder as GitHub or GitLab shows it. | ADR-103 |
| `team-share-dialog-all` ‡ | Everything Ordin remembers in this workspace and every knowledge page are shared with the people who can open this folder. (*removed — amended January 2026 (user decision; Approved by the user January 2026), SM-TW-13; the share dialog of SM-TW-2 says what is shared*) | ADR-103 (D6) |
| `team-share-dialog-history` ‡ | What you share stays in the repository's history, even after you delete it. | ADR-103 (D9) |
| `team-share-dialog-verb` ‡ | Start sharing (*removed — amended January 2026 (user decision; Approved by the user January 2026), SM-TW-13*) | ADR-103 |
| `team-header` ‡ | Team knowledge: { $folder } | ADR-103 |
| `team-available` ‡ | { $facts } new facts and { $pages } pages from { $people } | ADR-103 (Fluent plurals give "1 page", "8 new facts") |
| `team-get` ‡ | Get your team's memory and knowledge pages (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-1; was "Get them") | ADR-103 |
| `team-share` ‡ | Share Ordin's memory and knowledge pages with your team (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-1; was "Share what we've learned") | ADR-103 |
| `team-share-what` | Facts, kinds of fact, the links between them, where each fact came from, and knowledge pages. Your conversations and your documents themselves stay on this computer. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-1) | TeamKnowledgeService §E steps 3–4 |
| `team-share-invite` | Your team can use this workspace's knowledge pages and what Ordin remembers too. (*removed — amended January 2026 (user decision; Approved by the user January 2026), SM-TK-2: replaced by `team-share-from-tree`*) | ADR-103 |
| `team-share-outcome` | Your share of { $facts } facts and { $pages } pages is waiting for your approval in Needs you. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-3; Fluent plurals give "1 fact", "1 page") | `ShareProposed.counts` |
| `team-get-outcome` | Got { $facts } facts and { $pages } pages from your team. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-3; Fluent plurals give "1 fact", "1 page") | `GetReport.counts` |
| `team-nothing-to-share` ‡ | Nothing new to share. | `TEAM_NOTHING_TO_SHARE` |
| `team-waiting-review` ‡ | Waiting for review in { $repository } | ADR-103 |
| `team-waiting-review-open` ‡ | Open the review | ADR-103 |
| `team-held-for-check` ‡ | { $count } facts wait for you to check them | ADR-103 (D10) |
| `team-check-keep` ‡ | Keep it | ADR-103 (D10) |
| `team-check-forget` ‡ | Forget | ADR-103 (D10) |
| `team-not-taken-title` ‡ | Not taken in | SA-TEAM-04 |
| `team-not-taken-file` ‡ | { $file } could not be read. | `TEAM_FILE_UNREADABLE` |
| `team-not-taken-fact` ‡ | A shared fact was not taken in: it reads like an instruction or breaks a rule. | `TEAM_FACT_NOT_TAKEN` |
| `team-shared-by` ‡ | Shared by { $name }, { $date } | ADR-103 |
| `team-cites-their-files` ‡ | Cites files on { $name }'s computer | SA-TEAM-05 |
| `team-folder-large` ‡ | This team's folder is very large; getting it may take a while. | SA-TEAM-07 |
| `team-stop` ‡ | Stop sharing with the team | ADR-103 |
| `team-stop-confirm` ‡ | Stop sharing with the team? Everything stays as it is here and in the team's folder; Share and Get are no longer offered. | ADR-103 |
| `team-stop-verb` ‡ | Stop sharing | ADR-103 |
| `team-busy-checking` ‡ | Checking your team's knowledge… | TeamKnowledgeService §H |
| `team-busy-getting` ‡ | Getting what the team has learned… | TeamKnowledgeService §H |
| `team-busy-sharing` ‡ | Preparing your share… | TeamKnowledgeService §H |

### Example

An approval on the local backend, from the event that raises it to the decision. Values are illustrative; identifiers are UUIDv7.

1. **Event in** (one `EventEnvelope` of an `EventBatch` on `service_events`; the store's `lastSeq` for this backend is 40):

```json
{ "api_version": { "major": 1, "minor": 0 }, "backend": "0192f100-0000-7000-8000-00000000000a", "seq": 41,
  "at": "2026-01-10T10:43:58.120Z", "kind": "approval_requested",
  "card": { "approval_id": "0192f1a0-5b6c-7d8e-9f00-112233445566", "backend": "0192f100-0000-7000-8000-00000000000a",
            "workspace_id": "0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d",
            "asked_by": { "kind": "run", "run_id": "0192f19f-0000-7000-8000-0000000000b1" },
            "question": "Publish the revised account brief?", "verb": "Publish",
            "where_line": "Northwind team site — Account briefs", "affects": [ { "kind": "file", "label": "Account brief.docx", "version": "v7" } ],
            "changes": "Replaces version 6 with the revised brief", "reject_style": "reject",
            "buttons": { "approve": "7Hq2mVx9LpA3dRk5sT0uWa", "reject": "0bN8wE4yJfG6hZ1iQ2o3Pc", "show_details": "Mm5Ve8Ux2Tw4Sy6Rz9Q1Kd" },
            "created_at": "2026-01-10T10:43:58.101Z" } }
```

Store state after `apply` (Internal Logic 4): `lastSeq = 41`; `pendingApprovals` holds `0192f1a0-5b6c-7d8e-9f00-112233445566` → that card; `needsYou` lists it under its workspace and the *Needs you* count is 1. The card opens as a screen in front, with focus on its heading (step 7), and the conversation shows its one-line card "Waiting for you" with **Open**.

2. **Command out** when the person presses **Publish** (`invoke('plugin:ordin|approval_decide', { envelope })`):

```json
{ "api_version": { "major": 1, "minor": 0 }, "request_id": "0192f1a1-2222-7333-8444-555566667777",
  "command": "approval.decide", "backend": "0192f100-0000-7000-8000-00000000000a",
  "input": { "card_ref": "7Hq2mVx9LpA3dRk5sT0uWa", "decision": "approve" } }
```

Reply (`CommandReply<ApprovalOutcome>`):

```json
{ "request_id": "0192f1a1-2222-7333-8444-555566667777", "kind": "ok",
  "result": { "approval_id": "0192f1a0-5b6c-7d8e-9f00-112233445566", "state": "approved", "decided_on": "Asha's laptop" } }
```

3. **Event in**: `{ "api_version": { "major": 1, "minor": 0 }, "backend": "0192f100-0000-7000-8000-00000000000a", "seq": 42, "at": "2026-01-10T10:44:03.877Z", "kind": "approval_decided", "approval_id": "0192f1a0-5b6c-7d8e-9f00-112233445566", "state": "approved", "decided_on": "Asha's laptop" }`.

Store state after `apply`: `lastSeq = 42`; the card has left `pendingApprovals`; `decided` holds it with `{ state: "approved", decided_on: "Asha's laptop", at: "2026-01-10T10:44:03.877Z" }`; the *Needs you* count is 0. The card's buttons are replaced by "Decided on Asha's laptop, 10:44" and the conversation card reads "Approved: Publish.".

Had the reply been `{ "request_id": "0192f1a1-2222-7333-8444-555566667777", "kind": "error", "error": { "code": "APPR_CARD_ALREADY_DECIDED", "message": "This was already decided on another computer." } }`, the screen would show that message with no action button (step 11), and the store would change only when the matching `approval_decided` event arrives. An event with `seq` 44 arriving after 42 would record `SVC_EVENT_GAP` and reload this backend (step 5).

## Internal Logic

1. **Start** (`main.ts`). Call `localisation.get_settings` and `brand.get` on the local backend; call `localisation.get_catalogue { locale: active.locale }` and build one `@fluent/bundle` per returned resource, in `fallback_chain` order (SA-I18N-01, SA-RECON-03; amended January 2026, ADR-109: nothing is bundled at build time); call it again on every `locale_changed`, and on every `language_changed` whose `locale` is in `active.fallback_chain` (SM-LG-1), and lay the brand's `strings` over them per message id; set `<html lang>` from `active.locale` and `dir` from `active.direction`; inject the brand's token values into `:root` (brand values pass the contrast check in the core, ADR-041; approval-card structure, danger and waiting colours are not overridable). Call `settings.get { section: display }` and apply `display.appearance`. Open the unjournalled stream (`service_streams`) and the event stream (`service_events`, with an empty `after`) before the first data command, buffering events until each backend's first load completes. Mount `App`; place focus in the message box of the open workspace's conversation. Target: interactive ≤ 3 s after process start (A-03).
2. **Backends.** Create the local `BackendHandle` with `EventsOpened.local_backend`; call `servers.list` and create one handle per `ServerView` that has a `backend`. A `server_state_changed` event updates that server's connection state and `as_of` and, when the server is new or removed, `servers.list` is called again.
3. **Store — one `BackendState` per backend** (`lib/store/backend-state.svelte.ts`), all fields `$state`:
    - `lastSeq: number` (0 until the first load), `loaded: boolean`;
    - `connection` and `asOf` (servers only);
    - `runs: Map<RunId, RunView>` — `state`, `progressLines` (the last 200 `RunProgress` lines, older ones folded), `count`, `plan` (`plan_version`, `steps`, marks), `startUp` items;
    - `crews: Map<CrewRunId, { round, state }>`;
    - `pendingApprovals: Map<ApprovalId, ApprovalCard>` and `decided: Map<ApprovalId, { state, decided_on, at }>`;
    - `conversation: Map<WorkspaceId, ConversationItem[]>` — blocks shown, in `seq` order, each with its one-line card state (SM-CL-1: an item cannot be closed);
    - derived (`$derived`): `needsYou` (pending approvals plus the waiting items the catalogue's *Needs you* command returns, grouped by workspace, open workspace first), counts per workspace and the *Needs you* count.
4. **Applying an event** (`apply(env: EventEnvelope)`):
    1. Find the `BackendState` of `env.backend`; unknown backend → drop, diagnostic `UI_BACKEND_UNKNOWN`.
    2. `env.api_version.major` ≠ 1 → drop, diagnostic `UI_EVENT_VERSION`.
    3. `seq ≤ lastSeq` → drop (a repeat after reconnection). `seq > lastSeq + 1` and `loaded` → diagnostic `SVC_EVENT_GAP` and a reload of that backend (step 5), then continue with this event.
    4. By `kind`: `run_state_changed` updates `runs`; `run_progress` appends a line (≤ 1 s from emission to screen, no batching beyond one animation frame); `plan_changed` replaces the plan and adds the line `progress-plan-changed`; `step_marked` updates one mark; `start_up_report_ready` stores the items; `approval_requested` adds the card to `pendingApprovals` and opens it (step 7); `approval_decided` moves it to `decided` and updates every place it is drawn; `crew_round_changed` updates `crews`; `block_shown` adds a conversation item (step 6); `block_available` fetches the item with `ordin.block` and adds it; `workspace_changed` reloads that workspace's tree with `workspaces.tree`; `needs_you_changed` reloads *Needs you* with `home.needs_you`; `server_state_changed` updates the server's connection (step 10); `locale_changed` re-runs the locale part of step 1 with the event's `active` (catalogues, `lang`, `dir`) and redraws; `brand_changed` calls `brand.get` again and re-applies strings and tokens; `setup_progress` re-reads `setup.status` for the getting-ready view; `sign_in_changed` re-reads `identity.list` for Settings › Sign-ins; `reset` clears the backend's state and reloads (step 5); an unknown `kind` (newer minor version) → ignored, diagnostic `UI_EVENT_KIND_UNKNOWN`.
    5. Set `lastSeq = seq`.
5. **Load and reload** of a backend: call `service.hello` (its `last_seq` becomes the baseline), then `workspaces.list`, `workspaces.tree` for each expanded workspace, `home.needs_you`, `approval.list_pending`, `ordin.read_conversation` for each open conversation tab, and the commands of each open screen, with a new `request_id` each; while they run, the backend's screens keep their last state, read-only. On completion set `loaded = true` and apply buffered events whose `seq` is greater than the loaded state's sequence number.
6. **Drawing a block** (`BlockView` dispatches on `block.kind`). Where each kind appears and what it offers (11 §5.15, UX-A15, UX-A35, UX-A53):

    | Kind | Where | Component draws | Actions (each sends a command, or opens a screen, form or card; none approves) |
    |---|---|---|---|
    | `message` | conversation, in full | the text as paragraphs split on blank lines; attachments as one-line cards | — |
    | `question` | conversation, in full; in *Needs you* until answered | the question, 2–4 choice buttons, an "other" box when `allow_other` | the choices |
    | `form` | screen at once, cursor in the first empty field; **Waiting for you** card in the conversation; in *Needs you* | labelled fields by `input`: text, document picker, place picker, choice; a mark on fields `filled_by_ordin` | the form's `verb` |
    | `documents` | screen + one-line card | tree and list of the referenced items; 25 at a time with **Show more** | Ask Ordin about these, Open |
    | `preview` | screen + card | one document, page or image from the core's data for the reference | Open in [app], Ask Ordin about this |
    | `document` | screen + card | a Markdown document as a formatted page, read view of 11 §4 *Editing fills the centre* | Edit, Publish a copy…, Send back…, Ask Ordin about this |
    | `changes` | screen + card | `text_differences` as a two-column diff; `before_and_after_pages` / `_images` side by side | Undo, Ask for more changes |
    | `plan_progress` | screen + card | three layers: Now, steps, What happened (11 §5.4) from the store's run or crew | Stop, Review, Show what happened |
    | `table` | screen + card | title, typed columns, rows; sort by any column; search box; source line | Save as…, Ask Ordin about this |
    | `chart` | screen + card | the chart (step 8) and **Show as table** drawing exactly `columns` and `rows` | Show as table, Save as…, Ask Ordin about this |
    | `key_figures` | screen + card | 1–8 figures: label, value with unit or currency, change against `against` when present | Ask Ordin about this |
    | `report` | screen + card | title, "as of", 1–12 parts each a table, chart or key figures under its heading; a part refused by the core shows `block-cannot-show` and **Ask Ordin why** | Keep as dashboard…, Refresh, Save as…, Ask Ordin about this |
    | `email_list` | screen + card | sender (with **Outside the company**, word and icon), subject, date, attachments, from the core's data | Open, Ask Ordin about this |
    | `email` | screen + card | one message, or one draft with who drafted it | message: Reply with Ordin, Save attachment, Ask Ordin about this; draft: Send…, Edit, Save to mail drafts, Discard |
    | `telegram_list` | screen + card | sender, time, text, files; a note on a message that tries to instruct the agent | Reply with Ordin, Save file |
    | `slack_list` | screen + card | sender, time, text, thread replies count; `slack-message-instruction-note` on a message that tries to instruct the agent | Reply with Ordin, Open in Slack |
    | `result` | `produced`: screen + card; `offer`: conversation, in full, waiting; `checklist`: screen + card | what was produced with where it is; or the offer; or a list to tick | Open; Save as workflow…, Not now; Correct; Publish checked items… |
    | `agent_proposal` | the agent library screen with the agent chosen | from the catalogue's library command | Add it, Ask IT, Ask Ordin about this agent |
    | `agent_library` | screen | library with search and filters | Add it, Ask IT, Ask Ordin about this agent |
    | `crew_plan` | screen at once; **Waiting for you** card; in *Needs you* | purpose, agents, limits, estimated use, from the charter the core holds | Start |
    | `memory` | screen + card | facts with their sources, agents' own pages | Correct, Forget, Remember everywhere, Open Knowledge |
    | `knowledge` | screen | the workspace's pages with sources | Bring up to date, Edit, Ask Ordin about this, Publish to Confluence…, Remove page |

    Rules applied to every kind: text through Svelte text interpolation only; a `ContentRef` is resolved through a command, never opened as a URL; an action whose label ends in "…" opens something first (11 §4); an unknown kind → not drawn, diagnostic `UI_BLOCK_KIND_UNKNOWN`, and the conversation shows nothing broken (the core already told Ordin, 11 §5.15). A one-line card (`OneLineCard`) shows the item's title, the Now sentence for progress (11 §5.4 "the same words everywhere"), and **Open**; an item stops offering **Close** while it waits for the person.
    **Reply with Ordin** sends the conversation message "Draft a reply to { $sender }'s message in #{ $channel }." with the message id attached as a reference line (ordin.send_message), exactly as `telegram_list`'s action does. **Open in Slack** opens the message's permalink with `app.open_slack_link { url }` (*chosen here*; scope `Local`, answered by the desktop program): only a link from the core, never a link inside message text; the program refuses any URL that is not `https://<name>.slack.com/archives/…`. (amended January 2026, ADR-102)
7. **Approval cards** (`ApprovalScreen`, 11 §5.5, UX-A53, ADR-080). On `approval_requested`: open the card as a screen in its workspace's screen area — in front, with focus on its heading, only when it answers what the person just did; otherwise as a new tab marked *new* (11 §4 "Ordin does not move the person's place"); a card from another workspace opens there without switching (scene `approvalOther`). Draw, in this order: the question as heading; `approval-from-ordin`; Where, Affects (files by name and version, recipients with **Outside the company**), Changes; the content button labelled with `content.label`, or, for `message` and `command`, the content open on the card (a command in full in a code block with where it runs; a message over 12 lines folded to its first 6 with `approval-show-whole`, and the approve button reading `approval-verb-read-start` until it is opened); then the approve button labelled `verb` and the reject button (`approval-reject` or `approval-not-now`); then `approval-last-line`. The approve button is never the default button, so Enter on the card does not approve (UX-A2); focus never lands on a button when the card opens. Pressing sends `approval.decide { card_ref, decision }` to the card's backend; the buttons are disabled while it is in flight; the reply's error (`APPR_CARD_ALREADY_DECIDED`, `APPR_CARD_CLOSED`) is shown as its one sentence. `approval_decided` replaces the buttons with `approval-decided-on` or `approval-decided-in` and the conversation's card with `card-decided`. The fixed lines come only from the catalogue; no field of the card can change them. A card from a server whose connection is not `connected` shows `server-waiting-for` and disabled buttons.
8. **Charts** (`kinds/Chart.svelte`, SA-UI-03; conditional on C-40). Map `chart` to an ECharts option: `bar` (vertical or horizontal, grouped or stacked), `line`, `area` (line with area style), `scatter` (numeric `x`), `pie` (one series, ≤ 6 slices); one series per name in `series`; axis titles from `x_title`, `y_title`; colours only from the theme's chart palette tokens `--chart-1` … `--chart-8` (11 §5.21); ECharts `aria.enabled = true` with the title as label; values formatted by `lib/format/intl.ts`; no ECharts toolbox, no data zoom that changes data, no rich-text formatter (text only). Render with the canvas renderer; re-render on theme change. A render error → draw **Show as table** in its place and record `UI_CHART_RENDER_FAILED`. *If C-40 shows ECharts misses 300 ms for the six chart kinds within A-34's limits, Observable Plot replaces ECharts inside `Chart.svelte` only (SA-UI-03); nothing else changes.*
9. **Fixed views** (11 §4, §5.15; drawn by the application, with no content from Ordin). Each reads its data through the catalogue commands named for it and re-reads on the events listed.

    | View | Where | Content and rules | Commands | Events that refresh it |
    |---|---|---|---|---|
    | Side rail and tree (`WorkspaceTree`) | left, always | brand; **Home** with its count; **Search or go to…**; the person's workspaces as one WAI-ARIA tree (never folded behind a menu), each with icons for where its documents live and its waiting count; under each: Documents by where they live, Agents and their tasks, Workflows, Dashboards, Email, Telegram, Running now, Knowledge, What Ordin remembers, Earlier conversations; each node's main action on hover/focus/selection, the rest in its **⋯** menu (Shift+F10, Menu key); folders 25 at a time with **Show more**; each connected server as a group below, under its display name with "On [server]"; **Hosted agents** for a person with the operator role (ADR-097 item 9, scene `hostedOps`) Slack (amended January 2026, ADR-102): under each workspace, **Slack** after **Telegram**: one row per linked channel, "#name" with its new-message count; a channel in `no_access` shows `slack-link-no-access` | `workspaces.list`, `workspaces.tree`, `workspaces.create`, `workspaces.rename`, `workspaces.move_to_bin`, `files.add_place` (a folder, with `app.pick_folder`), `workspaces.add_link` (a remote location), `workspaces.remove_link`, `home.changed_marks`, `home.workspace_opened`, `servers.list` | `workspace_changed`, `run_state_changed`, `approval_*`, `needs_you_changed`, `server_state_changed` |
    | Screen area and tabs (`ScreenArea`, `ScreenTabs`) | centre | one row of tabs per workspace; something already open comes to the front; at most 6 open — opening a seventh closes the oldest tab neither in front nor showing running work; tabs out of view under `tabs-more`; Delete closes the focused tab; closing never deletes; every tab, Home included, has **Close** and closes: a workspace first shown with nothing open opens Home, and once the person has closed every tab the area stays empty, drawing the brand's logo large and faint as a watermark with the version (`about-version`) and a small **Check for updates** button (`about-check`, the check of SM-UP-2.1) under it, until something is opened (the side rail's Home opens Home again); a right press on a tab, Shift+F10 or the menu key opens its menu `screen-tab-menu` (WAI-ARIA menu, arrows, Home, End, Escape returns the focus to the tab) holding **Close** (`screen-tab-menu-close`), **Close others** (`screen-tab-menu-close-others`, off with one tab), **Close tabs to the right** (`screen-tab-menu-close-right`, off on the last tab) and **Close all** (`screen-tab-menu-close-all`); when the front tab is closed with others the front moves to the nearest tab left open after it, else before it (amended January 2026; user decision; Approved by the user January 2026); when an item is removed, every tab open for it closes straight away, wherever the removal was made (SM-OS-3) (amended January 2026; user decision; Approved by the user January 2026) | — (view preference) | — |
    | Conversation panel (`ConversationPanel`) | right | *Needs you* as the fixed first tab (never closed), then one tab per opened workspace with a new mark and waiting count; switching a tab switches the whole window to that workspace; tools **Summarise**, **Clear**, **Earlier**; the long-conversation nudge; the message box with the dictation button only where `dictation.get_availability` is not `unavailable` (macOS 26+; step 16); **Clear** asks first, then sends `ordin.prepare_start_fresh` and, with the points the person keeps, `ordin.start_fresh` | `ordin.read_conversation`, `ordin.send_message`, `ordin.answer_block` (a choice, other text, a form or an offer as its `BlockAnswer`), `ordin.summarise`, `ordin.prepare_start_fresh`, `ordin.start_fresh`, `ordin.continue_conversation` (amended January 2026, ADR-108: the conversation picker in the header and **Continue this conversation**, SM-CC-1…7), `ordin.list_conversations`, `ordin.block`, `dictation.get_availability`, `dictation.start`, `dictation.stop`, `dictation.cancel` | `block_shown`, `block_available`, `approval_*` |
    | *Needs you* (`NeedsYouTab`) | conversation panel, first tab | every waiting item grouped by workspace, the open workspace first, "On [server]" for a server's; choosing one opens its workspace and item with focus on its heading, never on a button | `home.needs_you`, `approval.list_pending`, `approval.get` | `needs_you_changed`, `approval_*` |
    | Home (`Home`) | centre, fixed view | Waiting for you (only the oldest keeps a primary **Open**), Running now (state label and **Show progress**), Since you last looked (newest first, **Open**, **Run again…** on finished work); filter **All workspaces** or one; **new** marks with a label; empty sections say so in one line; a row opens the item in its own workspace; no button approves, sends, posts or publishes *Waiting for you* header has **Open a handover link…** (opens a one-field dialog; paste → `handover.open_link { link }`; errors drawn with `ErrorLine`) (amended January 2026, ADR-102). | `home.view`, `home.opened`, `home.document_opened`, `home.run_again_inputs` | `run_state_changed`, `approval_*`, `crew_round_changed`, `needs_you_changed` |
    | Search or go to… (`SearchDialog`) | dialog over the window; Ctrl+K / ⌘K anywhere; rail control | WAI-ARIA combobox with listbox; groups Go to, Found in pages and documents, Actions; result count announced as it changes; a result in another workspace names it and, on use, switches first; footer `search-footer`; Escape returns focus where it was; queries debounced 120 ms | `search.query` | — |
    | Settings (`Settings/*`) | centre, fixed view | sections in 11 §4's order: Sign-ins, Agent repositories, Email, Telegram, Slack (amended January 2026, ADR-102), Jira (amended January 2026, ADR-106), Servers, Dictation, How Ordin talks to you, Language (amended January 2026, ADR-109; SM-LG-2), Using your plan, Notifications, History, Display, Code, What Ordin remembers, Diagnostics; "Set by your company" on locked values; Notifications and Display are `settings.get` / `settings.change` of the sections `notifications` and `display`, answered by the desktop program; History validates in place (`settings-history-invalid` under the box, tied by `aria-describedby`, focus kept, last value kept until a valid one); Servers lists each server with its state, **Connect to a server…** (address and code, or the one pasted line), **Disconnect this computer…** (confirmation), **Remove this server** | `settings.get`, `settings.change` (sections `notifications` and `display`, answered by the desktop program), `identity.list`, `identity.sign_in`, `identity.sign_in_wait`, `identity.sign_in_cancel`, `identity.sign_out`, `identity.save_mail_password`, `identity.save_gitlab_token`, `identity.import_client_certificate`, `jira.connect_token` (amended January 2026, ADR-106), `catalogue.list_repositories`, `catalogue.probe_repository` then `catalogue.add_repository`, `catalogue.rename_repository`, `catalogue.remove_repository`, `localisation.get_settings`, `localisation.set_locale`, `usage.get_settings`, `usage.update_settings`, `usage.today`, `servers.list`, `servers.connect`, `servers.disconnect`, `servers.remove`, `setup.status`, `setup.check_now`, `setup.sign_in_harness`, `dictation.get_availability`; Language (amended January 2026, ADR-109): `localisation.list_languages`, `localisation.export_language`, `localisation.import_language`, `localisation.remove_language`, `ordin.estimate_language_draft`, `ordin.draft_language`, `ordin.stop_language_draft` | `server_state_changed`, `sign_in_changed`, `setup_progress`, `locale_changed`, `language_changed` (amended January 2026, ADR-109) |
    | Diagnostics (`Diagnostics`) | Settings › Diagnostics | journal events, versions and technical names, for support; the only view showing codes; also this window's own diagnostics ring (step 12) | `diagnostics.lines`, `diagnostics.support_export` | — |
    | Confirmation (`ConfirmDialog`) | modal dialog | title is the question; one or two lines; the verb and **Cancel**; a verb that removes, discards or stops is never the default button; focus to the heading, Tab trapped, Escape cancels, focus returns to the opener (11 §5.5 *Confirmations*, §10) | the command of the confirmed action (for example `workspaces.move_to_bin`, `crews.stop`, `ordin.prepare_start_fresh`; removing a workflow is this confirmation followed by `workflows.remove { workflow_id, confirmed: true }`, never an approval card, D11) | — |
    | Quit (`QuitDialog`) | modal dialog on `ordin://quit-requested` | `quit-title`, `quit-body` with the count, **Quit** (not default) and **Cancel**; **Quit** sends `app.quit` | `app.quit` | — |
    | Earlier conversations | screen | each earlier conversation's first request, date, number of messages; one opens read-only on its own tab, with **Ask Ordin about it** and, from January 2026 (ADR-108), **Continue this conversation** | `ordin.list_conversations`, `ordin.read_conversation`, `ordin.ask_about_conversation`, `ordin.continue_conversation` | — |
    | Engineering views | screens in a workspace that links a code repository | code editor (Monaco, its diff editor for change review), *Changes waiting*, terminal (xterm.js); editor and terminal traffic is never journalled: input goes by `projects.lsp_send` and `projects.terminal_input`, output arrives on `service_streams` as `StreamMessage` of kind `lsp` or `terminal` (payload `{ bytes_b64 }`) matched by the `StreamId` the open command returned; on a server's workspace: `server-no-code-views` | `projects.review`, `projects.decide_entry`, `projects.commit` then `projects.push`, `projects.open_in_ide`, `projects.editor_open`, `projects.lsp_send`, `projects.editor_close`, `projects.terminal_open`, `projects.terminal_input`, `projects.terminal_resize`, `projects.terminal_close` | — |
    | Project settings (`ProjectSettings`) | screen opened from a project in the Code group of a workspace that links a code repository | **Check commands**: one text box per command (at most 20, each ≤ 1000 characters), **Add a check command**, a remove control per line and **Save**; an empty list reads `project-checks-none`; **Save** sends the list, and the reply's `Project.check_commands` is redrawn; a refusal (`PROJ_CHECKS_INVALID`, `PROJ_NOT_FOUND`) is the command's error line, the boxes keep what was typed (SA-CLOSE-04) | `projects.set_check_commands` | — |
    | Check it (`CheckItPanel`, 11 §5.30, UX-A63) | on *What [agent] is made of* and on every part page, in place | **Check it** (`check-it`); while it runs, a quiet status line; the result as *Checked by ordin* (`check-checked-by-ordin`) with `check-checked-at` from `checked_at`, then one line per finding drawn from its `message_id` and `args` (Fluent), naming the part by `part_identifier`, with its fixes in order (**Fix it**, **Ask Ordin to fix it**, **Change it yourself**); no finding → `check-all-passed`; a failure finding is marked before warnings; the panel keeps the last `check_run_id` for the open page and reopens it with `checker.report` (SA-CLOSE-01) | `checker.check_installed`, `checker.check_library_item`, `checker.report` | `workspace_changed` (the agent's workspace: the result is kept, its time line stays) |
    | Other fixed screens of 11 §5.15 | screens | Add to this workspace; the document page; the workflow screen and editor; the agent draft; Add an agent repository…; Parts of [agent repository], part draft, Withdraw; handover draft, handover screen, Lifecycle: [name]; What [agent] is made of, the part page, Show the installed files; the import screen; each drawn as the prototype scene of its 11 section shows it (12 §2, UX-A55) | `files.*`, `craft.open_with_system_default`, `push.*`, `confluence.publish_checked`, `workflows.*`, `reports.get`, `dashboards.*`, `knowledge.*`, `memory.*`, `mail.*`, `telegram.*`, `agents.*`, `authoring.*`, `handover.*` as each screen needs | `workspace_changed`, `run_state_changed` Also `slack.*`, `workspaces.set_slack_grant`, `workspaces.slack_grants`, `handover.open_link` (amended January 2026, ADR-102). Also `team_knowledge.*` and `memory.keep_checked` (amended January 2026, ADR-103). |
    | Settings › Slack (`Settings/Slack`) | Settings | `settings-slack-fixed` first; sign-in line (`signed_in`, workspace, name) with **Sign in to Slack…** (`identity.sign_in { kind: slack }`) or **Sign out**; when the profile names no Slack app, **Connect your own Slack app…** (dialog with the manifest from `slack.status`, a client-ID field, `slack.connect_own_app`); **Posted without asking**: one line per `SlackRule` with **Turn off** (`slack.turn_off_rule`); `settings-slack-rules-off-by-company` instead when the company turned rules off; **more permissions** line with **Allow…** when `scope_needed` was last met | `slack.status`, `slack.connect_own_app`, `slack.rules`, `slack.turn_off_rule`, `identity.sign_in`, `identity.sign_in_wait`, `identity.sign_out` | `workspace_changed`, `sign_in_changed` |
    | Add to this workspace › Slack channel… (`AddSlackChannel`) | dialog | channels the person belongs to, not yet linked, private ones marked; a channel outside `slack.allowed_channels` is not listed; **Add** links it (`slack.link`) and shows `slack-link-kept-from`; the first link may ask for the `users:read` permission (sign-in sentence) | `slack.offered_channels`, `slack.link` | — |
    | Slack channel screen (`SlackChannel`) | screen | the `slack_list` block for the link; **Let Ordin look at new messages** switch (`slack.set_ordin_reads`) with `slack-ordin-reads-note`; **Ordin answers in #name without asking…** sets an `ordin_answers` rule after the confirmation `slack-ordin-answers-rule-confirm` (`slack.add_rule`); agent access per agent: **Read Slack** / **Read and draft Slack** / none (`workspaces.set_slack_grant`); **Remove from this workspace** after the §5.5 confirmation (`slack.unlink`) | `slack.messages`, `slack.set_ordin_reads`, `slack.add_rule`, `slack.unlink`, `workspaces.slack_grants`, `workspaces.set_slack_grant` | `workspace_changed` |

    *Slack screens (amended January 2026, ADR-102).*

    **Handover draft** (11 §5.28): under each receiver, drawing `HandoverDraftView.slack[i]` for the i-th handover of the draft group (side by side: one block per receiver and stage):
    - When `reach` is `found`: the tick **Also tell them in Slack** (`handover-slack-tell`), on by default, sending `handover.update_draft` with that handover's `tell_in_slack` on change (the input's shape is the Handover Service's).
    - `not_found` → `handover-slack-not-found`; `not_signed_in` → `handover-slack-not-signed-in` with **Sign in to Slack…**; `scope_needed` → `settings-slack-more-permissions` with **Allow…** (`identity.sign_in { kind: slack }`; declining leaves the tick off and email on); `not_allowed` → `handover-slack-not-allowed`, no tick.
    - When `stage_channel` is present: `handover-slack-stage-channel`, or `handover-slack-stage-rule` when `stage_rule` is set; a link **Post handover notices in #channel without asking…** opens the confirmation `handover-slack-set-rule-confirm` → `slack.add_rule { kind: handover_notices, scope: stage }`.

    **Return to [name]…** card input (11 §5.28) — when the stage has a Slack channel, a tick **Include the reason in the Slack post** (`handover-slack-reason-in-post` ‡), on by default, sent as `ReturnInput.reason_in_slack`.

    **Handover screen** (both sides) — one line per entry of `HandoverScreen.slack` (`SlackLine`): `handover-slack-posted` with **Open in Slack** (the permalink, through `app.open_slack_link`); a failed or refused post: the post's sentence and **Try again** (`slack.try_again`) where the Slack Service offers it.

    **Approval card** (step 7) — unchanged drawing; the Slack lines arrive in `changes` and `affects` as the Handover and Slack Services build them (`#name · Slack · will appear as you`), and **See what will be sent** shows the whole text.

    **Lifecycle draft** (11 §5.28; the Authoring Service's lifecycle editor) — per stage, an optional field **Slack channel for this stage** (`lifecycle-stage-slack-channel`): a picker over `slack.offered_channels` (channel id and name), cleared to remove it.

    **Open a handover link…** (Home) — `handover.open_link { link }`: `opened` → open the handover screen as step 13 with no workspace; `not_found_yet` → close the dialog and show on Home `home-handover-link-following` = "ordin now follows { $place }; the handover will appear when it's readable."; an error → its sentence on the dialog.
10. **Server workspaces** (11 §5.31). A server's group and its workspaces' headers read `rail-server-on`; when its connection is `not_connected`, they read `rail-server-not-connected` with `asOf`; everything shown stays visible, read-only, and every button that would send something is disabled with `server-waiting-for`; on `connected` the line goes, the backend reloads (step 5) and *Needs you* catches up. `refused_for_version` shows `server-version-server-older` when `ServerView.older_side` is `server` and `server-version-desktop-older` when it is `desktop`. `disconnected` shows `server-ended` with **Connect again…**. Adding a folder on this computer to a server's workspace is refused in place with `server-no-local-folders`; opening a server document in the person's own app offers `server-open-copy` with `server-open-copy-note`. Never shown: port numbers, keys, certificates, *API*, *stream*, *tunnel*, *pairing*.
11. **Errors** (`ErrorLine`, 11 §6, P7). A command's `ServiceError` is shown as its `message` and at most one action: `try_again` re-sends the same command with a new `request_id`; `sign_in_again` opens Settings › Sign-ins at the account concerned; **SM-ER-3 No help request** *(amended January 2026, the user's decision: "Send to IT … should be removed", "yes" to removing it from the whole product; every feature is self-service; plan 05 Phase 10)*: an action `send_to_it` or `ask_it_for_…` that a backend still names draws **no** button, so the line shows its sentence and its reference only; the screens never call `setup.help_preview` or `setup.help_send`. Under the action, `error-reference` with `ServiceError.reference` when the error carries one; none → the line is not drawn. Background retries show as a quiet status line, never repeated pop-ups. Diagnostics is never offered as the way forward. An `invoke` that rejects (Bridge unavailable) → `UI_IPC_FAILED`, shown as `server-cannot-reach` for a server's backend, otherwise `error-try-again` with no further detail; a command that waits for no model and has no reply after 10 s → `UI_COMMAND_TIMEOUT`, shown the same way.
12. **Diagnostics ring.** Every `UI_…` and `SVC_EVENT_GAP` diagnostic is kept in memory, last 500 entries (`{ at, code, backend?, detail }` with no content — no text of a block, card or message), shown under Settings › Diagnostics as "This window" *(assumed here: the screens module writes no log file of its own)*.
13. **Notifications opened from outside.** On `ordin://open-target`, open the target's workspace and item exactly as *Needs you* does (approval card, run progress, crew progress, or the question in the conversation). `ordin://open-target` with kind `handover` also follows a handover link (OrdinDesktop step 13). Its `workspace_id` is optional (OrdinDesktop.md): when present, open that workspace and the handover screen as today; when absent (a handover not yet accepted), open the handover screen in front of the current view, as *Needs you* opens a handover it lists. (amended January 2026, ADR-102)
14. **Keyboard and focus** (11 §10). F6 / Shift+F6 move between the tree, the open screen and the conversation; screen and conversation tabs follow WAI-ARIA tabs (arrow keys move and show); the side panels' edges are separators operable with arrow keys, Home, End and Enter (restore), widths stored as view preferences; screen-reader announcements (`aria-live="polite"`) for a new approval and a finished task, using the Now sentence; visible focus on every control.
15. **Narrow windows.** (From January 2026: at 1,100 CSS pixels or less — amendment "The look of the screens", item 3.) Below 1,024 CSS pixels wide *(assumed here: three columns of at least 240, 480 and 300 pixels)*, the conversation slides over from the side, opened from **Conversation** in the header, which shows a mark when something new arrives. At 700 pixels or less, an editing page keeps at least 60 % of the window's height (11 §4).
16. **Dictation** (DictationService §E; ADR-043). At start and on `locale_changed`, `dictation.get_availability`; `unavailable` → no microphone button anywhere; `needs_assets` → the button shows and starting shows "Getting dictation ready…" on `preparing`. Pressing the button sends `dictation.start { field }` with the field's `FieldRef` (*chosen here*: `conversation-message-box` or `form:<block id>:<field name>`, ≤ 128 characters); the reply's `session_id` selects the `StreamMessage`s of kind `dictation` whose `stream` equals it. `partial` replaces the provisional tail after the last `final` (drawn dimmed); `final` appends to the field and is no longer replaced; `ended` removes the provisional tail and resets the button (reason `failed` shows `dictation-failed` with **Try again**). Pressing again sends `dictation.stop { session_id }`; Escape or leaving the field sends `dictation.cancel { session_id }`. Nothing is sent until the person presses the field's own send or submit control; dictated words never act on a card, and an approval card has no dictation control.

## Data Model

The module owns no table and no file. View preferences "remembered on that computer only" live in the WebView's `localStorage`, keys prefixed `ordin.view.` *(chosen here)*:

| Key | Value | Default |
|---|---|---|
| `ordin.view.panel.rail` | width in CSS pixels, 200–480 | 290 (280 before the January 2026 amendment, ADR-116) |
| `ordin.view.panel.conversation` | width in CSS pixels, 280–640 | 420 (380 before the January 2026 amendment, ADR-116) |
| `ordin.view.tree.show` | `{ "<workspace id>": { "documents": bool, "code": bool } }` (one always true) | both true |
| `ordin.view.tabs` | `{ "<backend id>:<workspace id>": ["<screen key>", …] }`, at most 6 each | empty |
| `ordin.view.panel.collapsed` | `{ "rail": boolean, "conversation": boolean }`, which side panes are collapsed to a strip (SM-PC-1 (amended January 2026; user decision; Approved by the user January 2026)) | both `false` |
| `ordin.view.tree.expanded` | list of node keys | the open workspace |

Nothing else is stored: no block content, no card, no message text, no path, no identifier other than the backend, workspace, block and screen keys above. A value that fails to parse is replaced by its default (`UI_STORAGE_UNAVAILABLE` recorded once). Removing ordin removes the WebView's storage with the data root.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `invoke` rejects: the Command Bridge is not reachable | `Err(retryable)` | `UI_IPC_FAILED` | person sees "Try again" with no detail; for a server's backend, "Can't reach Northwind server right now. ordin will keep trying while it's open." |
| A command that waits for no model has no reply after 10 s | `Err(retryable)` | `UI_COMMAND_TIMEOUT` | as above |
| A command replies with `kind: "error"` | `Err` per the error's `action` | the core's code (for example `APPR_CARD_ALREADY_DECIDED`) | the error's `message` and at most one action button |
| An event's `seq` skips numbers | reload of that backend | `SVC_EVENT_GAP` | nothing shown; screens keep their last state until reloaded |
| `reset` event | clear and reload of that backend | — | nothing shown beyond the reload |
| An event of an unknown `kind` (newer minor version) | ignored | `UI_EVENT_KIND_UNKNOWN` | nothing shown |
| An event of another major version | dropped | `UI_EVENT_VERSION` | nothing shown |
| An event for an unknown backend | dropped | `UI_BACKEND_UNKNOWN` | nothing shown |
| A block of an unknown `kind` | not drawn | `UI_BLOCK_KIND_UNKNOWN` | nothing broken is shown |
| A chart fails to render | table drawn instead | `UI_CHART_RENDER_FAILED` | the data as a table |
| A catalogue message missing in the active locale | en-GB message drawn | `UI_MESSAGE_MISSING` | the en-GB text (a missing en-GB message fails the build, SA-I18N-00) |
| `localStorage` throws or holds an unparsable value | defaults used | `UI_STORAGE_UNAVAILABLE` | nothing shown |
| A pasted handover link is not a handover link | `Err(permanent)` from the core | `HAND_LINK_INVALID` | the error's `message` on the dialog |
| A Slack permalink from the core is not `https://*.slack.com/archives/…` | not opened | `UI_LINK_REFUSED` *(chosen here)* | nothing opens; diagnostic only |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: App Shell and Block Renderer (drawing side)

  Scenario: Happy path — an approval card opens as a screen with focus on its heading
    Given the person just asked Ordin to publish the account brief
    When an approval_requested event arrives with question "Publish the revised account brief?" and verb "Publish"
    Then the card opens as a screen in front, with focus on "Publish the revised account brief?"
    And it reads "From ordin — built from the action, not written by an agent", then Where, Affects and Changes
    And its buttons are "See the changes", "Publish" and "Reject", and "Publish" is not the default button
    And its last line is "Nothing is sent or changed outside ordin until you choose."
    And the conversation shows a one-line card with "Waiting for you" and "Open"
    And Needs you lists it under its workspace

  Scenario: Happy path — pressing Publish
    When the person presses "Publish"
    Then approval.decide is sent with the approve card reference
    And when approval_decided arrives the buttons are replaced by "Decided on Asha's laptop, 10:44"
    And the conversation card reads "Approved: Publish."

  Scenario: Happy path — progress in three layers
    Given a plan of 4 steps and step 2 marked now
    When run_progress "Read 12 of 40 quotes" arrives
    Then the screen reads "Now: comparing the Q3 rates — step 2 of 4" with "Running for 6 min"
    And the steps show done, now, next, next with a symbol and a word each
    And "Read 12 of 40 quotes" is under "Show what happened"
    And the line reaches the screen within 1 second of the event

  Scenario: Happy path — a report with a chart shown as a table
    Given a report block with key figures, two charts and a table
    When the person opens it and presses "Show as table" on a chart
    Then the table shows exactly the chart's columns and rows
    And each part shows "Where these numbers come from"

  Scenario: Edge case — the seventh tab
    Given six screen tabs are open in a workspace and the oldest shows no running work
    When the person opens a seventh screen
    Then the oldest tab that is neither in front nor showing running work is closed

  Scenario: Edge case — a question with choices stays in the conversation
    When a block_shown event with a question block of 3 choices arrives
    Then the question and its 3 choices appear in the conversation in full
    And Needs you lists it until it is answered
    And it has no Close while it waits

  Scenario: Edge case — server link drops
    Given Northwind server is connected
    When server_state_changed arrives for Northwind server with state not_connected and as_of 10:42
    Then the server's group reads "Not connected · as of 10:42"
    And the server's screens stay visible and read-only
    And buttons that would send something are disabled with "Waiting for Northwind server"
    When server_state_changed arrives with state connected
    Then the line goes and the server's backend is reloaded

  Scenario: Edge case — server of a different version
    When server_state_changed arrives with state refused_for_version
    Then the person reads "Northwind server is older than ordin. Some things won't work until it's updated."

  Scenario: Happy path — Check it on an installed agent
    Given What "Contract clause checker" is made of is open for agent instance "0192a1c9-aaaa-7bbb-8ccc-ddddeeee0002"
    When the person presses Check it
    Then checker.check_installed is sent with that agent_instance_id
    And on a report with passed true and no findings the panel reads "Checked by ordin" and "Every check passed."

  Scenario: Happy path — Check it on a library part shows each finding
    Given the part page of "urn:ordin:acme:tool-bindings:jira" in the bundled catalogue is open
    When the person presses Check it
    Then checker.check_library_item is sent with no agent_repository_id, kind "ToolBinding" and that identifier
    And a finding with message_id "check-trust-needs-it-approval" is drawn as one line naming the part

  Scenario: Error path — Check it cannot read the repository
    Given checker.check_library_item replies with an error whose action is try_again
    When the panel draws the reply
    Then the error line shows the message and "Try again", and no "Every check passed." is shown

  Scenario: Happy path — the engineer sets a project's check commands (SA-CLOSE-04)
    Given the project settings screen of project "supplier-portal" shows "No checks set"
    When the engineer adds "cargo test" and presses Save
    Then projects.set_check_commands is sent with commands ["cargo test"]
    And the screen lists "cargo test" from the returned project

  Scenario: Error path — check commands refused
    Given the project settings screen lists 21 check commands
    When the engineer presses Save and the reply is PROJ_CHECKS_INVALID
    Then the error line shows the message and the 21 boxes keep their text

  Scenario: Edge case — sign_in_changed with credential_kind
    Given Settings › Sign-ins is open
    When a sign_in_changed event arrives with credential_kind "gitlab" and state "needs_sign_in"
    Then identity.list is sent again and the GitLab row is redrawn

  Scenario: Edge case — a gap in the sequence
    Given the local backend's last sequence number is 120
    When an event with sequence number 124 arrives
    Then SVC_EVENT_GAP is recorded in the diagnostics ring
    And the local backend is reloaded through commands before event 124 is applied

  Scenario: Edge case — a repeated event after reconnection
    Given the server's last sequence number is 48213
    When an event with sequence number 48213 arrives again
    Then it is dropped and nothing changes on screen

  Scenario: Edge case — reset
    When a reset event arrives from Northwind server
    Then that backend's state is cleared and reloaded
    And the person sees no message about it

  Scenario: Edge case — history setting refused in place
    Given Settings › History shows 90
    When the person enters "0"
    Then "Enter a whole number of days, 1 or more." appears under the box, tied to it for screen readers
    And the box keeps focus and the stored value stays 90

  Scenario: Edge case — quitting with agents working
    When "ordin://quit-requested" arrives with running_sessions 2
    Then a dialog asks "Quit ordin?" with focus on its heading
    And "Quit" is not the default button
    When the person presses Cancel
    Then nothing is sent and focus returns to where it was

  Scenario: Edge case — search across workspaces
    When the person presses Ctrl+K and types "Kestrel"
    Then results appear under "Go to", "Found in pages and documents" and "Actions"
    And the number of results is announced
    And a result from "Q4 forecast" names that workspace

  Scenario: Error path — an approval already decided elsewhere
    When the person presses "Publish" and the reply is an error with code APPR_CARD_ALREADY_DECIDED and message "This was already decided on Asha's laptop."
    Then that sentence is shown on the card with no code
    And the buttons are not offered again

  Scenario: Error path — command error with an action
    When a command replies with message "Can't reach the company's agent library right now" and action try_again
    Then the person sees that sentence and one "Try again" button
    And pressing it sends the same command with a new request_id

  Scenario: Error path — the bridge cannot be reached
    Given invoke rejects
    When the person presses any button that sends a command
    Then UI_IPC_FAILED is recorded and the person sees one "Try again"

  Scenario: Error path — unknown block kind
    When a block_shown event carries kind "timeline"
    Then nothing is drawn for it and UI_BLOCK_KIND_UNKNOWN is recorded

  Scenario: Error path — chart fails to render
    Given the chart library throws while drawing a chart block
    Then the block's data is drawn as a table
    And UI_CHART_RENDER_FAILED is recorded

  Scenario: Error path — markup in data is shown as text
    When a message block's text is "<img src=x onerror=alert(1)>"
    Then the conversation shows those characters literally and no element is created

  Scenario: Happy path — dictated text replaces its provisional tail
    Given dictation is available and the person pressed the microphone in the message box
    And dictation.start replied with session_id 0192b3c4-1111-7222-8333-444455556666
    When service_streams delivers a dictation partial "Please summarise the" and then a partial "Please summarise the supplier"
    Then the message box shows "Please summarise the supplier" once, dimmed as provisional
    When a dictation final "Please summarise the supplier quotes." arrives
    Then the text is kept and no message is sent until the person presses Send

  Scenario: Edge case — dictation unavailable
    Given dictation.get_availability returns state unavailable with reason windows
    When the conversation panel is drawn
    Then no microphone button is shown

  Scenario: Edge case — the version sentence names the side to update
    Given servers.list returns a server in refused_for_version with older_side "desktop"
    When its workspaces are drawn
    Then their header reads "Update ordin on this computer to use Northwind server."

  Scenario: Happy path — an error with a reference
    Given a command replies with an error whose action is send_to_it and whose reference is "r-81"
    When the error line is drawn
    Then it shows the message and "Reference r-81" and no button (SM-ER-3)

  Scenario: Happy path — a locale change redraws the window
    Given the window is drawn in en-GB
    When a locale_changed event arrives with active locale "fr-FR" and direction "ltr"
    Then html lang becomes "fr-FR" and every catalogue string is redrawn from the bundled fr-FR catalogue
```

Tests (vitest): one per error-table row; one per block kind with the 2C JSON example as input; store tests table-driven over event sequences (in order, repeat, gap, reset, unknown kind); a source scan test that fails on `{@html`, `innerHTML`, `fetch(`, `XMLHttpRequest`, `WebSocket`, `EventSource`, and on any import of `@tauri-apps/api` outside `lib/backend/tauri.ts`; a catalogue test that every id used in the source exists in the en-GB `.ftl` files; an accessibility test of the approval screen and the confirmation dialog (focus on open, Tab trapped, Escape). Coverage ≥ 70 % (patch-mode rule C3).

## Performance, Security, Observability

- **Performance targets:** an action that waits for no model updates the screen ≤ 150 ms p95 (A-03); first paint of the shell ≤ 1 s after the window is created, interactive conversation ≤ 3 s after process start; initial JavaScript ≤ 600 KB gzipped, with Monaco, xterm.js and ECharts loaded on first use *(assumed here)*; applying one event ≤ 2 ms p95 and a burst of 500 events ≤ 100 ms; a chart of 5,000 points drawn ≤ 300 ms (C-40); sorting or searching a 10,000-row table ≤ 200 ms, with only the visible rows (± 50) in the document; the tree draws 25 documents per folder at a time.
- **Security:**
  - The WebView's only route out is the `ordin` plugin under the `main` capability (OrdinDesktop); the module makes no network call (no `fetch`, `XMLHttpRequest`, `WebSocket`, `EventSource`), loads no remote asset (CSP), and holds no key, token or server address beyond the display name.
  - Every string from data is drawn as text; `{@html}` and `innerHTML` are banned by test; Monaco and xterm.js receive text only; ECharts receives no rich-text formatter.
  - Approval cards are drawn only from `approval_requested` events; no block kind can draw a button that approves, sends, posts or publishes, and the card's fixed lines come from the catalogue.
  - Input validation: form fields check their own limits before sending (lengths of 2C: label, text ≤ 16,384 characters); the core validates every command again (SA-API-02).
  - `localStorage` holds view preferences only (*Data Model*).
- **Observability:** no logging and no metrics of its own (SA-XC-04/05). Diagnostics are the in-memory ring of step 12, with codes and identifiers only, shown in Settings › Diagnostics.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

The screens draw Slack for working together; the sections above carry it in place, each marked where amended:

- **Shared Context:** the generated Slack types (`SlackLink`, `SlackRule`, `SlackReach`, `StageSlackChannel`, `DraftSlackView`, `LinkOpened`, `SlackListBlock`) and three rules on screen: every Slack line names "as you"; a Slack message is drawn as text and marked when it reads as an instruction; Settings › Slack opens with its fixed sentence.
- **Message catalogue:** the `slack-*`, `settings-slack-*`, `handover-slack-*`, `home-handover-link-*` and `lifecycle-stage-slack-channel` rows; `settings-section-<name>` lists Slack after Telegram.
- **Internal Logic:** step 6 draws `slack_list`; step 9 adds Slack to the side rail, Settings, Home and the fixed screens, the views Settings › Slack, Add to this workspace › Slack channel… and the Slack channel screen, the handover draft's **Also tell them in Slack**, the return card's **Include the reason in the Slack post**, the handover screen's Slack lines, the lifecycle draft's stage channel and Home's **Open a handover link…**; step 13 opens a handover from a link with or without a workspace.
- **Error Table:** `HAND_LINK_INVALID` on the paste dialog and `UI_LINK_REFUSED`.

### Acceptance Criteria (Gherkin) — added scenarios


```gherkin
  Scenario: Happy path — the receiver is found in Slack
    Given the handover draft's receiver is "Priya Shah" and handover.update_draft returns reach found
    Then the draft shows the tick "Also tell them in Slack", on
    And unticking it sends handover.update_draft with tell_in_slack false

  Scenario: Edge case — the receiver is not in Slack
    Given reach is not_found
    Then the draft shows "Not found in Slack — they'll get an email" and no tick

  Scenario: Edge case — Settings › Slack opens with its fixed sentence
    When the person opens Settings › Slack
    Then the first line reads "Every post is shown to you before it is posted, unless you set a rule for it."
    And each rule is listed with Turn off

  Scenario: Edge case — a Slack message that instructs ordin
    Given a slack_list message with asks_agent_to_act true
    Then it is drawn with "This message asks ordin to do something. It's shown as information only."

  Scenario: Edge case — a pasted link whose handover is not readable yet
    When the person pastes a handover link and handover.open_link returns not_found_yet for "Supplier onboarding handovers"
    Then Home shows "ordin now follows Supplier onboarding handovers; the handover will appear when it's readable."

  Scenario: Edge case — a side-by-side draft shows Slack per receiver
    Given a draft group of two handovers, one receiver found in Slack and one not
    Then the first shows the tick "Also tell them in Slack" and the second "Not found in Slack — they'll get an email"

  Scenario: Error path — a pasted link that is not a handover link
    When the person pastes "https://example.com/x" into Open a handover link…
    Then the dialog shows the HAND_LINK_INVALID sentence and nothing opens
```

### Gaps

None.

## Amendment January 2026 — Team knowledge (ADR-103)

> Source: decision 83 (the user's decisions D1–D10 of January 2026); HLD revision 0.51.0, ADR-103. Owners of the data: `components/TeamKnowledgeService.md`, `components/MemoryService.md` and `components/KnowledgeService.md` (their amendments January 2026 — Team knowledge). The sections above carry the message rows `team-*` (marked ‡, *chosen here*) and the commands in step 9's fixed-screens row; everything else is below.

### Shared Context (additions)

The generated types `TeamKnowledgeStatus`, `TeamKnowledgeBinding`, `TeamAvailable`, `TeamCounts`, `TeamProblem`, `TeamFolderChoice`, `GetReport` and `ShareProposed` (TeamKnowledgeService.md), and the `FactView` and page-view fields that carry who shared an item and when (the Memory Service's and Knowledge Service's amendments). Three rules on screen: **no Git word** — no team-knowledge text, button or card says branch, commit, push, pull, merge, rebase or conflict; **the sentence, never the code** — every `TEAM_*` refusal is drawn as its owner's sentence through `ErrorLine`; **what is shared is said before the first share** — the dialog's `team-share-dialog-all` and `team-share-dialog-history` lines are always shown.

### Internal Logic (additions to step 9, fixed views)

1. **Workspace header.** While `team_knowledge.status { ws }` has a `binding`, the workspace header shows `team-header` with `header_line` ("Team knowledge: acme/supplier-team › knowledge"), and in its **⋯** menu **Stop sharing with the team** (`team-stop`). Without a binding, the workspace's **⋯** menu offers no share (*amended January 2026 (user decision; Approved by the user January 2026)*: knowledge-only sharing is no longer offered on its own; a workspace is shared from the **Share** link on its tree row, SM-TW-13). The status is read when the workspace opens and re-read with `team_knowledge.status` on every `workspace_changed` for that workspace, which is how the outcome of a check, get or share reaches the screen.
2. **Share knowledge with a team… (dialog).** *amended January 2026 (user decision; Approved by the user January 2026): this dialog is no longer offered and **Start sharing** is not drawn, so no screen sends `team_knowledge.turn_on`; its folder choices and the address-splitting rule below are kept, used by the share dialog (SM-TW-2) and by **Read another team's knowledge…**.* One list of `team_knowledge.candidate_links { ws }` (each by its display name) and a second choice, one field `team-share-dialog-address` for the folder's web address as the host shows it (`TeamFolderChoice::NewFolder`). The screen splits the address, so the person never types a branch: `https://<host>/<owner>/<name>/tree/<branch>/<path>` (GitHub) or `https://<host>/<group>/<name>/-/tree/<branch>/<path>` (GitLab) → `url = https://<host>/<owner or group>/<name>`, `branch` = the segment after `tree/`, `path` = the rest (empty for the repository root); a branch name containing `/` is read as its first segment (*chosen here*; a folder on such a branch is chosen from the linked folders instead). Any other text → `team-share-dialog-address-invalid` under the field, nothing sent. The lines `team-share-dialog-all` and `team-share-dialog-history` sit above the verb **Start sharing**, which sends `team_knowledge.turn_on`. A refusal is drawn under the fields with `ErrorLine` (`TEAM_LINK_NOT_SUITABLE`, `TEAM_NO_ACCESS`, `TEAM_SIGN_IN_NEEDED` with **Sign in again**, `TEAM_REMOTE_UNREACHABLE` with **Try again**, `TEAM_FORMAT_NEWER`); focus stays in the dialog.
3. **Team knowledge panel** (under the header, one line each, shown only when it applies):
   - `busy` → one progress line in place of the panel's buttons: `team-busy-checking`, `team-busy-getting` or `team-busy-sharing`; **Get your team's memory and knowledge pages** (`team-get`) and **Share Ordin's memory and knowledge pages with your team** (`team-share`) are disabled while it shows (labels *amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-1). `team_knowledge.check_now`, `team_knowledge.get` and `team_knowledge.share` answer at once with the status (`busy` set); the screen draws that status and waits for `workspace_changed`.
   - `last_error` → its owner's sentence with its action through `ErrorLine` (**Try again** re-sends the command that failed; **Sign in again** opens the sign-in), until the next status without it.
   - `available` → `team-available` with the counts and up to five names, and **Get your team's memory and knowledge pages** (`team-get`, `team_knowledge.get`); when `last_get` arrives the line is replaced by the new status, which no longer carries those items.
   - **Share Ordin's memory and knowledge pages with your team** (`team-share`) → `team_knowledge.share`; when `last_share` arrives, the card it raised is an ordinary approval card (step 7, drawn unchanged), also listed in *Needs you*. When `share_unavailable` is set the button is disabled and its sentence (for `TEAM_READ_ONLY`: "You can read this team's knowledge but not add to it.") is shown beside it, tied by `aria-describedby`. `TEAM_NOTHING_TO_SHARE` in `last_error` is drawn as `team-nothing-to-share`, not as an error.
   - `waiting_for_review` → `team-waiting-review` with **Open the review** (the request's address, through the system browser as other external links are opened).
   - `held_for_check > 0` → `team-held-for-check`; choosing it opens What Ordin remembers for the workspace filtered to the facts waiting for a check, each with **Keep it** (`memory.keep_checked { id }`) and **Forget** (`memory.forget { id }`).
   - `problems` non-empty → a **Not taken in** list (`team-not-taken-title`), one row per problem with `team-not-taken-file` or `team-not-taken-fact`.
   - `folder_large` → `team-folder-large`.
   - **SM-TK-1 Memory and knowledge pages travel together** (*amended January 2026 (user decision; Approved by the user January 2026)*). The share and get verbs name both: `team-share` and `team-get`. Under the share verb, while the workspace has an own folder and is not a team workspace (`team_workspace` absent), the panel shows `team-share-what` in secondary text. Its words follow what one share carries (TeamKnowledgeService §E steps 3–4): every fact, entity, source reference, relationship and term of the workspace, and every knowledge page; a source travels as its reference (`origin_ref`), never as the document's content; conversations are not exported. A team workspace says what it shares in its own share dialog (SM-TW-2), so the line is not drawn there. The sentences that named the old verb follow it: `team-ws-result-not-here` and `team-ws-problem-result-not-taken`.
   - **SM-TK-2 One line where nothing is shared** (*amended January 2026 (user decision; Approved by the user January 2026)*; it replaces the invitation on the Knowledge screen, `team-share-invite`). Memory and Knowledge draw the same panel; the Knowledge screen no longer passes `invite`. While the status has no `binding`, no entry in `folders` and no `team_workspace`, the panel is one line, `team-share-from-tree` ("Share this workspace from the side tree to work on it with your team."), with no button, and **Get your team's memory and knowledge pages** is not drawn; the line is drawn only for the local backend when it answers `team_knowledge.share_workspace` (SM-UP-1, SM-TW-12), and nothing is drawn otherwise. **Get your team's memory and knowledge pages** and the `team-shared-by` marks stay for a workspace that shares (a team workspace, or one that shares its knowledge only, SM-TW-14). The panel's section is named `team-panel-label`. The line `team-share-from-tree` is removed: while nothing of the workspace is shared the panel draws nothing at all, and sharing stays offered on the side tree's workspace row (SM-TW-13) (amended January 2026; user decision; Approved by the user January 2026).
   - **SM-TK-3 What a share or get carried** (*amended January 2026 (user decision; Approved by the user January 2026)*). When the person presses `team-share`, or `team-get` of the own folder, the panel keeps the `last_share` or `last_get` it had. The first status that arrives afterwards with `busy` cleared, no `last_error` and a different report draws one line: after a share `team-share-outcome`, after a get `team-get-outcome`, with `facts` = `new_facts + changed_facts` and `pages` = `new_pages + changed_pages` of the report's `counts`; the same sentence is announced politely. Removed items appear on the approval card only; a report with no new or changed item, a failed command, and a get of another team's folder (`folder_id` set; `last_get` reports the own folder) draw no line. A share is not sent until its approval card is approved (step 7), so the share line says it waits for approval. The line is replaced by the next share or get the person starts and is not kept across a reopening of the screen. No command or wire type changes: the counts are `ShareProposed.counts` and `GetReport.counts` of TeamKnowledgeService.md.
4. **Stop sharing with the team** opens a confirmation (`team-stop-confirm`, verb `team-stop-verb`, **Cancel** first in focus); confirming sends `team_knowledge.turn_off`.
5. **Who shared it.** In What Ordin remembers and in the knowledge page list and page screen, an item that came from a colleague shows `team-shared-by` with the sharer's name and the date in the person's locale; a page that cites files only on the sharer's computer also shows `team-cites-their-files`.
6. A workspace on a connected server shows the same header, panel and dialog through the Remote API; nothing differs on screen. (*amended January 2026 (user decision; Approved by the user January 2026)*: no dialog turns knowledge-only sharing on, and the pointer line of SM-TK-2 is not drawn there, SM-TW-12; a server workspace that shares its knowledge shows its header and panel as before.)

### Error Table (rows added)

N/A — no new code; the `TEAM_*` and `MEM_*` refusals are drawn by `ErrorLine` as their owners' sentences.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  # Two scenarios below: amended January 2026 (user decision; Approved by the user January 2026) (SM-TW-13).
  Scenario: Happy path — a workspace that shares its knowledge shows the header
    Given workspace "Q3 supplier review" whose team_knowledge.status has a binding with header_line "acme/supplier-team › knowledge"
    Then the header shows "Team knowledge: acme/supplier-team › knowledge"
    And no screen sends team_knowledge.turn_on

  Scenario: Edge case — the dialog says what will be shared before the first share
    When the share dialog of SM-TW-2 opens
    Then it shows "What you share stays in the repository's history, even after you delete it."

  Scenario: Happy path — new items from colleagues
    Given team_knowledge.status has available 8 new facts and 1 new page from "Priya" and "Sam"
    Then the panel shows "8 new facts and 1 page from Priya and Sam" with Get your team's memory and knowledge pages
    When Get your team's memory and knowledge pages is pressed
    Then team_knowledge.get is sent and the panel shows "Getting what the team has learned…"
    And on workspace_changed the screen re-reads team_knowledge.status and, with busy cleared and last_get set, the line is gone

  Scenario: Edge case — read-only access
    Given share_unavailable is TEAM_READ_ONLY
    Then Share Ordin's memory and knowledge pages with your team is disabled and "You can read this team's knowledge but not add to it." is shown beside it

  # Amended January 2026 (user decision): SM-TK-1, SM-TK-2, SM-TK-3.
  Scenario: Happy path — the panel says what a share carries
    Given a workspace with an own folder that is not a team workspace
    Then the panel shows "Facts, kinds of fact, the links between them, where each fact came from, and knowledge pages. Your conversations and your documents themselves stay on this computer."

  # Two scenarios below: amended January 2026 (user decision; Approved by the user January 2026) (SM-TK-2).
  Scenario: Happy path — Memory and Knowledge point an unshared workspace at the side tree
    Given team_knowledge.status has no binding, no folders and no team_workspace
    When the Knowledge screen or the Memory screen opens
    Then the panel shows only "Share this workspace from the side tree to work on it with your team."
    And it has no "Share knowledge with a team…" and no "Get your team's memory and knowledge pages"

  Scenario: Edge case — no pointer once shared, or without the team group
    Given team_knowledge.status has a binding or a team_workspace, or the backend does not answer team_knowledge.share_workspace
    Then the pointer line is not shown

  Scenario: Happy path — a share says what it carried
    Given the person presses Share Ordin's memory and knowledge pages with your team
    When the status arrives with busy cleared and last_share counts 42 new facts and 6 new pages
    Then the panel shows "Your share of 42 facts and 6 pages is waiting for your approval in Needs you."

  Scenario: Happy path — a get says what it brought
    Given the person presses Get your team's memory and knowledge pages for the own folder
    When the status arrives with busy cleared and last_get counts 10 new facts, 2 changed facts and 2 new pages
    Then the panel shows "Got 12 facts and 2 pages from your team."

  Scenario: Error path — a failed get says nothing about counts
    Given the status after a get has busy cleared and last_error TEAM_REMOTE_UNREACHABLE
    Then no count line is shown

  Scenario: Edge case — facts waiting for a check
    Given held_for_check is 2
    When the person chooses "2 facts wait for you to check them" and presses Keep it on one fact
    Then memory.keep_checked is sent for that fact

  Scenario: Edge case — no Git words
    When every team-* message is rendered in en-GB
    Then none contains "branch", "commit", "push", "pull", "merge", "rebase" or "conflict"

  Scenario: Error path — the repository cannot be reached
    Given team_knowledge.status after a get has busy cleared and last_error TEAM_REMOTE_UNREACHABLE
    Then the panel shows "Your team's knowledge can't be reached just now." with Try again
```

### Gaps

None.

---

## Amendment January 2026 — Page links (ADR-104)

> Source: decision 84 (the user's decisions K1–K9 of January 2026); HLD revision 0.52.0, ADR-104 and 11 §5.10 as amended. Owners of the data: `components/KnowledgeService.md` (amendment January 2026 — Page links and editing in other apps: `PageView`, `PageListing`, `SuggestionView`, the two suggestion commands, the filter words) and `components/MemoryService.md` (amendment January 2026 — `FactView.page_section`). Every rule below adds to the Knowledge screen and the memory block of step 9; nothing above changes.

### Shared Context (additions)

The generated types `PageRef`, `PageLinkView`, `SuggestionView`, `SuggestionKind`, `DuplicateReason`, `NotAdoptedFile`, `NotAdoptedReason` and the fields added to `PageView`, `PageSummary`, `PageListing` and `FactView`. Vocabulary (11 §2 as amended): links are "links", backlinks are "Pages that link here", aliases are "Other names", tags are "Tags"; never "vault", "wiki", "backlink", "alias" or "graph" on screen.

### Internal Logic (additions to step 9, the Knowledge screen)

1. **Links in the page.** The page body is drawn as any Markdown document (unchanged), with wiki links drawn as links: `[[target|shown]]` shows `shown`, `[[target#section]]` shows `target › section`, `[[target]]` shows `target`, `![[target]]` is drawn as a link (no embedding). Activating a link whose `PageLinkView.target` is set opens that page (at the section when one is named and a heading of that text exists, else at the top); a link with no target is drawn in the theme's muted style with the tooltip `knowledge-link-missing` and opens nothing. The link's position in `links_out` order matches its position in the body.
   1. **SM-KN-1 The page drawn formatted, and which links are page links** *(amended January 2026)*. Item 1's "drawn as any Markdown document" means the page body goes through the same Markdown reader as a Markdown document (`apps/desktop/ui/src/lib/blocks/markdown.ts`: headings, paragraphs, lists, quotes, code, bold, italic and code spans become elements; markup in the text stays text; nothing is set as HTML) and is never shown as its Markdown source. The links paired with `links_out`, in body order, are exactly the ones KnowledgeService.md §P keeps: wiki links (§P.2, an empty target or one naming another extension is not one) and Markdown links whose path ends in `.md`, carries no scheme and stays under `knowledge/` when resolved against the page's folder (§P.3, the form ordin itself writes, KnowledgeService SA-OKF-11). A kept Markdown link shows its words and acts as a wiki link does (opens its target page, or is drawn muted with `knowledge-link-missing`). No link is looked for in a code block or code span. Every other Markdown link — a web address, or a path that is not a `.md` file such as `[Rates](Rates)` — is drawn as its words only (**Where a link goes**), opens nothing and is not under **Links on this page**, because the service keeps no link for it. *(Found in the Phase 5 walk on Windows: the page "Paying suppliers", written by Ordin with the body "# Paying suppliers … [Rates](Rates)", was shown as its source text; its empty **Links on this page** was correct, since `Rates` names no `.md` file.)*
   2. **SM-KN-2 A knowledge page can be removed, and the removal undone** *(amended January 2026; user report on 0.1.1: there was no way to remove a page from the knowledge base)*. An open page that is not read-only and not being edited shows **Remove…** (`knowledge-remove`, icon `i-remove`) beside **Edit**. It asks with `ConfirmDialog` (`knowledge-remove-title` with `$title`, `knowledge-remove-body`, verb `knowledge-remove-verb`); confirming sends `knowledge.remove { page_id }`, closes the page, reads `knowledge.list` again and shows the status line `knowledge-removed` with `$title` and **Undo** (`knowledge-undo-remove`), which sends `knowledge.undo_remove { page_id }` and reads the list again. An error (`KNOW_PAGE_REMOVED`, `KNOW_TITLE_DUPLICATE`, `KNOW_PAGE_NOT_FOUND`) is drawn by `ErrorLine`. Cancel sends nothing. *Amended January 2026 (user decision; Approved by the user January 2026):* the open page's head is one line: **Back** (`fixed-back`, `i-back`), the page's title, and, while the page is not read-only and not being edited, **Edit** (`knowledge-edit`, `i-edit`) and **Remove…** (`knowledge-remove`, `i-bin`). The three are small icon buttons in the editor toolbar's style (SM-NS-4.6, 1.75 rem square), each with its word as `aria-label` and `title`; the head wraps on a narrow pane, and the page's lines below it are spaced more tightly than the list.
   3. **SM-KN-3 A knowledge page saved as a PDF or Word document** *(amended January 2026; user decision; Approved by the user January 2026)*. On this computer only — the backend is local and answers `knowledge.export` (ServiceApi.md SVC-KEX-1) — an open page that is not being edited shows a ⋯ icon button (`i-more`, `aria-label` `more-actions-for` with the page's title, `title` `more-actions-tip`, `aria-haspopup="menu"`) at the end of its head, after **Edit** and **Remove…**, and also on a page that may only be read. It opens the tree menu's markup (`tree-menu-list`, `role="menu"` named `tree-node-menu`, focus on its first item, Escape closes it) holding **Export as PDF…** (`knowledge-export-pdf`, icon `i-pdf`) and **Export as Word…** (`knowledge-export-docx`, icon `i-word`). Choosing one closes the menu and sends `knowledge.export { page_id, format: "pdf" | "docx" }`; the screen sends no content, title or path — the program reads the page and opens the save dialog (OrdinDesktop.md DSK-KEX-1). Saved → the shell announces `knowledge-exported` with `$title`; `DSK_DIALOG_CANCELLED` → nothing is shown; any other error (`DSK_SAVE_COPY_FAILED`, `KNOW_PAGE_NOT_FOUND`) is drawn by `ErrorLine` under the head. Knowledge pages are not rows of the side tree (the tree has one **Knowledge** row), so the tree gains no item. Messages (en-GB, `screens-fixed.ftl`): `knowledge-export-pdf`, `knowledge-export-docx`, `knowledge-exported`.
2. **Pages that link here.** Below **Where this comes from**, `knowledge-links-in-title` and one row per `links_in` page (its title; activating opens it); none → `knowledge-links-in-none`. **Links on this page** (`knowledge-links-out-title`) lists `links_out` once per target, unresolved ones with `knowledge-link-missing`; `links_truncated > 0` adds `knowledge-links-truncated`.
3. **Other names and tags.** Under the title: `aliases` as `knowledge-other-names` and `tags` as chips. In **Edit**, two fields below the title: **Other names** (one per line, `knowledge-other-names-hint`) and **Tags** (comma-separated, `knowledge-tags-hint`); **Save** sends `knowledge.save_edit` with `tags` and `aliases` only when either field changed. *amended January 2026 (user decision; Approved by the user January 2026):* in **Edit** the body (`knowledge-body`) is written in the what-you-see editor of SM-NS-4.6, never as Markdown codes; **Save** sends the same `knowledge.save_edit` with `body_markdown` as the editor gives it. Other names stay a plain one-per-line box and Tags a plain field. *amended January 2026 (user decision; Approved by the user January 2026):* **Edit** also draws, under the title, **Kind of page** (`knowledge-type`; a choice of `Explanation`, `How-to`, `Decision`, `Reference`, `Glossary` as `knowledge-type-explanation`, `-how-to`, `-decision`, `-reference`, `-glossary`, plus the page's own `type` as written when it is none of them) and **Summary in one sentence** (`knowledge-description`, at most 200 characters, `knowledge-description-hint`), filled from the page view's `type` and `description`; `knowledge.save_edit` carries `type` only when the kind was changed and `description` only when the summary was changed and is not empty, so the file's header is otherwise written back as it was. **Done** (`edit-done`) saves and goes back to the page; **Cancel** (`edit-cancel`) and Escape go back without saving, asking `edit-discard-title` (**Discard** / **Keep editing**, `ConfirmDialog`) first when a field was changed. `unregistered_aliases` shows `knowledge-name-in-use` per name; `title_refused` shows `knowledge-title-in-use`. *Amended January 2026 (user decision; Approved by the user January 2026):* in **Edit**, the page's details — **Kind of page**, **Summary in one sentence** with its hint, **Other names** and **Tags** (the Open Knowledge Format fields) — are hidden when the form opens and shown on request by a small icon button under the title (`i-info`, in the editor toolbar's style, `aria-expanded`, its words as `aria-label` and `title`: `knowledge-details-show` ("Show the page's details") while hidden, `knowledge-details-hide` ("Hide the page's details") while shown). Hiding them keeps what was typed; **Done** saves them on the terms above whether shown or hidden.
4. **Suggestions.** When `suggestions` is non-empty, a **Suggestions** group (`knowledge-suggestions-title`) under the page actions: a `link` row shows `knowledge-suggest-link` with the `context` (the matched words emphasised) and **Link it** (`knowledge.accept_suggestion`) and **Dismiss** (`knowledge.dismiss_suggestion`); a `duplicate` row shows `knowledge-suggest-duplicate` (reason `names`) or `knowledge-suggest-duplicate-text` (reason `text`), **Open** (the other page) and **Dismiss**. Neither row changes anything until its button is pressed. `KNOW_SUGGESTION_STALE` is drawn by `ErrorLine` and the page is re-read.
5. **Rename notice.** After a save that changed the title, when the page has `links_in`, one line `knowledge-renamed-links` with the number of pages whose links were updated (the number of `links_in` before the save), with **Show them** (lists them).
6. **Filter box.** The search box of the page list sends its text as `knowledge.list { filter }`; its placeholder is `knowledge-filter-hint`. Tags in the list rows are chips; activating a chip puts `tag:<tag>` in the box.
7. **Files that are not pages.** When `not_adopted` is non-empty, one line under the list, `knowledge-not-adopted`, expanding to one row per file with its reason (`knowledge-not-adopted-limit`, `-large`, `-header`, `-other-workspace`) and **Show in folder** (the platform file manager, as other files are shown).
8. **A fact's section.** In What Ordin remembers, a fact with `page_id` shows **Open the page** (unchanged); with `page_section` set it opens the page scrolled to the first heading whose text equals `page_section`, and to the top when none does.

### Message catalogue rows (en-GB, *chosen here* unless 11 quotes them)

| Key | en-GB | Source |
|---|---|---|
| `knowledge-link-missing` ‡ | No page called { $name } | 11 §5.10 (amended) |
| `knowledge-links-in-title` ‡ | Pages that link here | 11 §5.10 (amended) |
| `knowledge-links-in-none` ‡ | No other page links here yet. | — |
| `knowledge-links-out-title` ‡ | Links on this page | — |
| `knowledge-links-truncated` ‡ | { $count } more links are not listed. | — |
| `knowledge-other-names` ‡ | Also called { $names } | 11 §5.10 (amended) |
| `knowledge-other-names-hint` ‡ | Other names for this page, one per line. Links using them find this page. | — |
| `knowledge-tags-hint` ‡ | Tags, separated by commas | — |
| `knowledge-name-in-use` ‡ | "{ $name }" is already the name of another page, so it isn't used for this one. | — |
| `knowledge-title-in-use` ‡ | The file was given the title "{ $title }", which another page already has. This page keeps its title. | — |
| `knowledge-suggestions-title` ‡ | Suggestions | — |
| `knowledge-suggest-link` ‡ | Link "{ $words }" to { $page }? | 11 §5.10 (amended) |
| `knowledge-suggest-duplicate` ‡ | This page and { $page } have nearly the same name. They may cover the same subject. | 11 §5.10 (amended) |
| `knowledge-suggest-duplicate-text` ‡ | This page and { $page } say much the same. They may cover the same subject. | — |
| `knowledge-renamed-links` ‡ | Links on { $count } other pages were updated to the new title. | 11 §5.10 (amended) |
| `knowledge-filter-hint` ‡ | Filter pages — try tag:suppliers or state:out-of-date | — |
| `knowledge-not-adopted` ‡ | { $count } files in the knowledge folder aren't pages | — |
| `knowledge-not-adopted-limit` ‡ | This workspace already keeps 500 pages. | — |
| `knowledge-not-adopted-large` ‡ | The file is too long to keep as a page. | — |
| `knowledge-not-adopted-header` ‡ | The block at the top of the file can't be read. | — |
| `knowledge-not-adopted-other-workspace` ‡ | It's a copy of a page that belongs to another workspace. | — |

### Error Table (rows added)

N/A — no new code; `KNOW_TAG_INVALID`, `KNOW_ALIAS_INVALID`, `KNOW_SUGGESTION_NOT_FOUND` and `KNOW_SUGGESTION_STALE` are drawn by `ErrorLine` as their owner's sentences.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a link opens the page it names
    Given page "Q3 review notes" whose PageView has links_out [{target "Supplier rates", name "Supplier rates", section "Review cycle"}]
    When the person activates the link "Supplier rates › Review cycle"
    Then the page "Supplier rates" opens scrolled to its heading "Review cycle"

  Scenario: Happy path — a page is drawn formatted (SM-KN-1)
    Given a page whose body is "# Paying suppliers\n\nSupplier invoices are due within 30 days. See [Rates](Rates) and [Day rates](Day%20rates.md)."
    And its PageView has links_out [{target "Day rates", name "Day rates"}]
    Then the body shows the heading "Paying suppliers" and the paragraph as text, with no "#" and no "[Rates](Rates)"
    And "Rates" is drawn as words only and "Day rates" as a link that opens the page "Day rates"

  Scenario: Edge case — a link to no page
    Given links_out holds {name "Purchasing policy"} with no target
    Then the link is drawn muted with the tooltip "No page called Purchasing policy" and activating it opens nothing

  Scenario: Happy path — accepting a suggestion
    Given suggestions holds a link suggestion to "Supplier rates" for the words "supplier rates"
    When the person presses Link it
    Then knowledge.accept_suggestion is sent with that suggestion id and the page is re-read

  Scenario: Edge case — no forbidden words
    When every knowledge-* message is rendered in en-GB
    Then none contains "vault", "wiki", "backlink", "alias" or "graph"

  Scenario: Error path — a tag with a space
    When the person saves the page with tags "supplier rates"
    Then knowledge.save_edit returns KNOW_TAG_INVALID and the owner's sentence is shown beside Tags, with the edit kept

  Scenario: Edge case — a fact opens at its section
    Given a fact in What Ordin remembers with page_id P and page_section "Exceptions"
    When the person chooses Open the page
    Then page P opens scrolled to its heading "Exceptions"
```

### Gaps

None.

## Amendment January 2026 — Jira issues and handovers (ADR-106)

> Source: decision 86 (the user's decisions of January 2026: Jira as a handover place and Jira issues as workspace links); HLD revision 0.54.0, ADR-106, and 11 §5.13 and §5.28 as amended. Owners of the data: `components/JiraAdapter.md` (`jira.*` commands, `JiraUser`, `JiraIssueSummary`), `components/HandoverService.md` (`DraftJiraView`, `JiraIssueOption`, `JiraIssueSelection`, `PersonChoice.jira_account_id`, the card's lines), `components/WorkspaceService.md` (`LinkLocation::JiraIssue`, `SourceLinkKind` `jira_issue`, `WorkingCopyItem.origin_newer_at`) and `components/IdentityAndCredentials.md` (`identity.sign_in` with `site`). Jira is drawn on the desktop's own backend only: every `jira.*` command is scope Local, and a server's workspace never offers Jira.

### Shared Context (additions)

The generated types `JiraUser`, `JiraIssueSummary`, `DraftJiraView`, `JiraIssueOption`, `JiraIssueSelection`, the variant `LinkLocation` `jira_issue`, the field `WorkingCopyItem.origin_newer_at` and `PersonChoice.jira_account_id`. Vocabulary: Jira's own words for its things — "issue", "project", "assigned"; never "ticket" on screen *(chosen here: the words Jira shows the person)*. Rule on screen: every text read from Jira is drawn as text, never as markup or a link the screens follow on their own.

### Internal Logic (additions to step 9)

1. **Settings › Jira** (`settings-section-jira`, after Slack). Not connected: the line `settings-jira-intro`; three fields — site (`settings-jira-site`, the host only, `name.atlassian.net`), email (`settings-jira-email`) and API token (`settings-jira-token`, a password field cleared after sending); the guide line `settings-jira-token-guide`; the verb **Connect** (`settings-jira-connect`) sends `jira.connect_token { site, email, token }`. Beside it **Sign in with Atlassian…** (`settings-jira-oauth`) sends `identity.sign_in { kind: jira, site: "https://" + <site field> }` and then `identity.sign_in_wait` as every browser sign-in does; a reply `CRED_PROVIDER_NOT_CONFIGURED` replaces the button with `settings-jira-oauth-unavailable` for the rest of the session (the precedent of SA-CLOSE-03). Connected (an `identity.list` row of kind `jira`): `settings-jira-connected` with the site and the name `jira.connect_token` returned (or the account label after a restart), and **Disconnect Jira…** (`settings-jira-disconnect`) → confirmation → `identity.sign_out`. Errors from `jira.connect_token` are drawn under the fields with `ErrorLine` (`JIRA_TOKEN_REFUSED`, `JIRA_ADDRESS_INVALID`, `JIRA_UNAVAILABLE` with **Try again**, `CRED_DUPLICATE` as `settings-jira-other-site`); focus stays in the form and the token field is cleared. A `sign_in_changed` with `credential_kind` `jira` re-reads `identity.list`.
2. **Add to this workspace › Jira issue…** (`jira-add-to-workspace`, dialog, desktop backend only). One field `jira-add-paste` for an issue's address; below it a search box `jira-add-search` sending `jira.find_issues { text, limit: 25 }` 300 ms after the last keystroke, one row per `JiraIssueSummary` (`<key> <summary>`, status category); empty → `jira-add-none`. **Add** sends `workspaces.add_link { workspace_id, location: { kind: "jira_issue", address } }` with the pasted text, or with the picked row's address `https://<site>/browse/<key>`. Not connected (`JIRA_NOT_CONNECTED` from the search, or `WS_SOURCE_SIGN_IN_NEEDED` from `add_link`) → `jira-add-not-connected` with **Open Settings › Jira**. `WS_JIRA_ADDRESS_INVALID`, `WS_JIRA_SITE_MISMATCH`, `SRC_DUPLICATE` are drawn under the field with `ErrorLine`.
3. **A linked issue in the workspace tree.** The link shows its display name and the line `jira-item-read-only`; its items are listed as for any remote link. An item with `origin_newer_at` set shows `jira-item-changed` and **Bring in the new version** (`jira-item-bring-in`), which sends `workspaces.read_item { link, origin_path }`; nothing changes the item before the person presses it. `jira-item-changed` takes `$time` from `origin_newer_at`. Removing the link uses the existing confirmation of 11 §5.6, with the added line `jira-remove-line`.
4. **Handover draft with a Jira place** (11 §5.28 as amended). When `HandoverDraftView.jira` is present:
   - **Which issue?** (`handover-jira-issue-label`): a single choice with one option per `options` entry (`<key> <summary>`) and a last option `handover-jira-issue-new` naming the project; the chosen option is `selection`; a change sends `handover.update_draft` with `jira_issue` = `{ kind: "linked", key }` or `{ kind: "create_new" }`. Below it, `line` drawn as text. When `options` is empty, **Use an existing issue…** (`handover-jira-use-existing`) opens the dialog of step 2 for the draft's workspace; after a link is added the screens send `handover.open_draft` again, so the new issue appears in `options`. Every Jira sentence of the draft, including the one naming an issue ordin created when later writes failed, is the `line` the Handover Service sends; the screens add none of their own.
   - **Receiver search**: the field's hint is `handover-jira-receiver-hint`; it sends `handover.find_people` as for every place. A choice whose `address` is empty shows the name only. No email address can be typed as a receiver for a Jira place: text that matches no choice shows `handover-jira-receiver-none` and **Hand over** stays unavailable.
   - **Checks**: drawn unchanged (*Checked by ordin*), with the reasons the Checker returns.
5. **Approval card** (step 7) — unchanged drawing; the Jira lines ("[name] is assigned [key] in Jira. Everyone who can see [key] sees the note and the results' links." and, for a new issue, "ordin creates a new issue in [project] named "[title]".") arrive in `changes` as the Handover Service builds them, and **See what will be sent** shows the opening comment's text.
6. **Open a handover link…** — unchanged: a link with `t=jira` is passed whole to `handover.open_link`, which checks it.

### Message catalogue rows (en-GB, *chosen here* unless 11 quotes them)

| Key | en-GB | Source |
|---|---|---|
| `settings-section-jira` ‡ | Jira | 11 §4 (amended) |
| `settings-jira-intro` ‡ | Connect Jira so Ordin can read the issues you link and hand work over on them. ordin writes to Jira only when you approve a handover. | ADR-106 |
| `settings-jira-site` ‡ | Your Jira site, for example acme.atlassian.net | — |
| `settings-jira-email` ‡ | The email address you sign in to Jira with | — |
| `settings-jira-token` ‡ | API token | — |
| `settings-jira-token-guide` ‡ | In your Atlassian account, open Security, then API tokens, create a token and paste it here. You don't need anyone's permission. | ADR-106 |
| `settings-jira-connect` ‡ | Connect | — |
| `settings-jira-oauth` ‡ | Sign in with Atlassian… | — |
| `settings-jira-oauth-unavailable` ‡ | Your company hasn't set up Atlassian sign-in. Use an API token instead. | — |
| `settings-jira-connected` ‡ | Connected to { $site } as { $name } | — |
| `settings-jira-disconnect` ‡ | Disconnect Jira… | — |
| `settings-jira-other-site` ‡ | ordin is already connected to another Jira site. Disconnect it first. | — |
| `jira-add-to-workspace` ‡ | Jira issue… | 11 §5.13 (amended) |
| `jira-add-paste` ‡ | Paste the issue's address from Jira | — |
| `jira-add-search` ‡ | Or search your Jira issues | — |
| `jira-add-none` ‡ | No issues match. | — |
| `jira-add-not-connected` ‡ | Connect Jira in Settings to link an issue. | — |
| `jira-item-read-only` ‡ | From Jira · read only | — |
| `jira-item-changed` | Changed in Jira, { $time } | 11 §5.13 (amended) |
| `jira-remove-line` | Nothing in Jira changes. | 11 §5.13 (amended) |
| `jira-item-bring-in` | Bring in the new version | 11 §5.13 (amended) |
| `handover-jira-issue-label` | Which issue? | 11 §5.28 (amended) |
| `handover-jira-issue-new` | A new issue will be created in { $project } | 11 §5.28 (amended) |
| `handover-jira-use-existing` | Use an existing issue… | 11 §5.28 (amended) |
| `handover-jira-receiver-hint` ‡ | People who can be assigned issues in { $project } | — |
| `handover-jira-receiver-none` ‡ | Nobody who can be assigned issues in { $project } matches. | — |

### Error Table (rows added)

N/A — no new code; `JIRA_*`, `WS_JIRA_*`, `CRED_DUPLICATE`, `CRED_PROVIDER_NOT_CONFIGURED` and the new `HAND_*` codes are drawn by `ErrorLine` as their owners' sentences.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — connecting Jira with an API token
    Given Settings › Jira is open and not connected
    When the person enters "acme.atlassian.net", "sam.lee@acme.co.uk" and a token, and presses Connect
    Then jira.connect_token is sent with those three fields
    And the token field is empty afterwards and the section shows "Connected to acme.atlassian.net as Sam Lee"

  Scenario: Edge case — no company Atlassian sign-in
    When the person presses "Sign in with Atlassian…" and identity.sign_in returns CRED_PROVIDER_NOT_CONFIGURED
    Then the button is replaced by "Your company hasn't set up Atlassian sign-in. Use an API token instead."

  Scenario: Happy path — linking an issue from search
    Given Jira is connected
    When the person types "rates" in Add to this workspace › Jira issue… and jira.find_issues returns ABC-12
    And the person picks ABC-12 and presses Add
    Then workspaces.add_link is sent with location {kind "jira_issue", address "https://acme.atlassian.net/browse/ABC-12"}

  Scenario: Edge case — a changed issue waits for the person
    Given an item of a linked issue has origin_newer_at set to 10:40 today
    Then it shows "Changed in Jira, 10:40" and "Bring in the new version"
    When the person presses Bring in the new version
    Then workspaces.read_item is sent for that item

  Scenario: Happy path — choosing the issue a handover rides on
    Given HandoverDraftView.jira has options ABC-12 and ABC-20 and selection linked ABC-12
    When the person chooses "A new issue will be created in Supplier onboarding" under "Which issue?"
    Then handover.update_draft is sent with jira_issue {kind "create_new"}
    And the draft shows the line the reply carries

  Scenario: Edge case — no linked issue in the project
    Given HandoverDraftView.jira has no options and selection create_new
    When the person presses Use an existing issue…
    Then the Jira issue dialog opens for the draft's workspace
    And after workspaces.add_link succeeds, handover.open_draft is sent again

  Scenario: Error path — typing an email address as a Jira receiver
    Given a draft with a Jira place
    When the person types "sam@example.com" and handover.find_people returns no choice
    Then the field shows "Nobody who can be assigned issues in Supplier onboarding matches." and Hand over is unavailable

  Scenario: Error path — a token Atlassian refuses
    When jira.connect_token returns JIRA_TOKEN_REFUSED
    Then its sentence is shown under the fields, the token field is cleared and focus stays in the form

  Scenario: Edge case — no Jira on a server's workspace
    Given the workspace shown belongs to a connected ordin server
    When Add to this workspace opens
    Then "Jira issue…" is not offered
```

### Gaps

None.

## Amendment January 2026 — Continue an earlier conversation (ADR-108)

> Source: decision 88 (the user's decisions of January 2026); HLD revision 0.57.0, ADR-108, 11 §5.12 and UX-A76. Owner of the data: `components/OrdinHost.md` (`ordin.continue_conversation`, `ConversationSummary.continues`, `.first_request`, `ConversationMessage.carried`, `ConversationEnd` `put_away`). The rows *Conversation panel* and *Earlier conversations* of step 9's table name the added command and button.

### Shared Context (additions)

The generated fields and variant above, and `MessageAuthor` `system` with `ConversationMessage.kind` (a Slack turn is drawn as the ADR-102 amendment says). Words: "conversation" only, never "session", "thread" or "context" (11 §3).

### Internal Logic (additions to the conversation panel and the Earlier conversations screen)

1. **SM-CC-1 Conversation picker** (`ConversationPicker`), in the conversation panel's header before **Summarise**, **Clear** and **Earlier**. A button showing the live conversation's name — its `first_request` cut to 48 characters at a word end with "…", or `conv-picker-new` before the first message — with accessible name `conv-picker-label`. WAI-ARIA menu button: `aria-haspopup="menu"` and `aria-expanded`; Enter, Space or Down Arrow opens it on the first item, Up Arrow on the last; Up and Down Arrow move and wrap; Home and End; a typed character moves to the next item that starts with it; Escape closes it and returns focus to the button; Tab closes it. On opening it sends `ordin.list_conversations` (first page). Items, in order: the live conversation (`menuitemradio`, `aria-checked="true"`, its name and `conv-picker-current`); a separator; up to 20 earlier entries, newest first, each `menuitemradio` `aria-checked="false"` with its `first_request` and the line `conv-picker-date`, accessible name "<first request>, <date>"; more than 20 → a last item `conv-picker-all`, which opens the Earlier conversations screen; none → `conv-picker-none`, disabled; a separator; **Clear**, which behaves as the Clear tool. Choosing an earlier entry opens it read-only on its own tab, as the Earlier conversations screen does; it sends nothing that changes state. On a narrow window the picker is in the header of the sliding conversation panel.
2. **SM-CC-2 Read-only tab** of an earlier conversation: beside **Ask Ordin about it**, **Continue this conversation** (`conv-continue`). Its tree node's menu (⋯): **Read it**, **Ask Ordin about it**, **Continue this conversation**; the main action stays **Read it**. The live conversation offers it nowhere.
3. **SM-CC-3 Asking first.** When the live conversation has at least one message, `ConfirmDialog` with `confirm-continue-title`, the line `confirm-continue-body`, **Keep the key points first** (`confirm-clear-keep-points`, drawn as in Clear's confirmation), the verb **Continue** (`confirm-continue-verb`) and **Cancel**. **Cancel** sends nothing. **Continue** sends `ordin.continue_conversation { workspace_id, conversation_id, kept_points: [] }`. **Keep the key points first** follows Clear's flow, sending `ordin.prepare_start_fresh` and then, with the points the person keeps, `ordin.continue_conversation` in place of `ordin.start_fresh`. With no live conversation, or one with no messages, the command is sent at once with no dialog.
4. **SM-CC-4 After the reply.** The continued conversation's read-only tab closes; the workspace's conversation tab reads `ordin.read_conversation` of the new conversation; focus goes to the message box; a polite live region says `conv-continued-announce`.
5. **SM-CC-5 Waiting for a crew.** A reply `OH_CREW_RUNNING` makes **Continue this conversation** unavailable, with `conv-continue-waits` beside it, until the next `run_state_changed` event; other errors are drawn by `ErrorLine` as their owner's sentences.
6. **SM-CC-6 A continued conversation.** When the live conversation's `ConversationSummary.continues` is set, a fold above its own messages reads `conv-continued-line` with **Show** / **Hide** (`conv-continued-show`, `conv-continued-hide`); `$count` is the chain's `message_count` less the live conversation's own messages. Showing it reads `ordin.read_conversation` of `continues`, then of each earlier link, drawn read-only, oldest first, 100 at a time with **Show more**, with `conv-continued-from` between two conversations. The Earlier conversations screen and the tree group list one entry per chain, with its message count, and a read-only tab draws the whole chain the same way.
7. **SM-CC-7 Moved items.** Entries with `carried: true` are drawn at the top of the live conversation as ordinary waiting items. In an earlier conversation, a message whose block's `conversation_id` (from `ordin.block`) is not the conversation drawn is drawn as one line, `conv-item-moved`. Approval cards are drawn as today.

### Message catalogue rows (en-GB, *chosen here* unless 11 quotes them)

| Key | en-GB | Source |
|---|---|---|
| `conv-picker-label` ‡ | Conversation: { $name } | — |
| `conv-picker-new` | New conversation | 11 §5.12 (amended) |
| `conv-picker-current` ‡ | Current | — |
| `conv-picker-date` ‡ | { $date } · { $count } messages | — |
| `conv-picker-all` | All earlier conversations ({ $count }) | 11 §5.12 (amended) |
| `conv-picker-none` ‡ | No earlier conversations | — |
| `conv-continue` | Continue this conversation | 11 §5.12 (amended) |
| `conv-continue-waits` | Waits until the crew finishes | 11 §5.12 (amended) |
| `confirm-continue-title` | Continue this conversation? | 11 §5.12 (amended) |
| `confirm-continue-body` | Ordin stops working from the current conversation and works from “{ $name }” instead. Both are kept, and nothing is deleted. | 11 §5.12 (amended) |
| `confirm-continue-verb` | Continue | 11 §5.12 (amended) |
| `conv-continued-line` | Continued { $time } · earlier messages ({ $count }) | 11 §5.12 (amended) |
| `conv-continued-show` / `conv-continued-hide` ‡ | Show / Hide | — |
| `conv-continued-from` ‡ | Continued { $time } | — |
| `conv-continued-announce` ‡ | Continuing “{ $name }” | — |
| `conv-item-moved` | Moved to the current conversation | 11 §5.12 (amended) |

### Error Table (rows added)

N/A — no new code of this component; `OH_CONVERSATION_UNKNOWN`, `OH_CONVERSATION_NOT_CONTINUABLE`, `OH_CREW_RUNNING` and the other Ordin Host codes are drawn by `ErrorLine` as the Ordin Host's sentences.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Continue is offered on an earlier conversation only
    Given workspace W has a live conversation and earlier conversation X
    Then X's read-only tab and X's tree node menu offer "Continue this conversation"
    And the live conversation offers it nowhere

  Scenario: Asking first, and Cancel changes nothing
    Given the live conversation has 2 messages
    When the person presses Continue this conversation on X
    Then the dialog "Continue this conversation?" shows "Keep the key points first", Continue and Cancel
    When the person presses Cancel
    Then no command is sent

  Scenario: Nothing to ask when the live conversation is empty
    Given the live conversation has no messages
    When the person presses Continue this conversation on X
    Then ordin.continue_conversation is sent at once with no dialog

  Scenario: Keeping key points before continuing
    When the person chooses Keep the key points first and sends Ordin's form keeping 2 points
    Then ordin.prepare_start_fresh was sent, then ordin.continue_conversation with those 2 points

  Scenario: The picker by keyboard
    Given focus is on the picker button showing "Q3 supplier rates"
    When the person presses Down Arrow
    Then the menu opens with focus on the live conversation
    And each earlier entry's accessible name ends with its date
    When the person presses Escape
    Then the menu closes and focus is on the picker button

  Scenario: Choosing an earlier conversation opens it to read
    When the person chooses earlier entry X in the picker
    Then X opens read-only on its own tab with Continue this conversation
    And no command that changes state is sent

  Scenario: A crew is running
    When ordin.continue_conversation returns OH_CREW_RUNNING
    Then Continue this conversation is unavailable with "Waits until the crew finishes"

  Scenario: A continued conversation shows where it came from
    Given the live conversation continues X, which has 12 messages
    Then above its messages the line "Continued 14:05 · earlier messages (12)" shows with Show

  Scenario: A moved item shows once
    Given a question moved from conversation A into the live conversation
    Then the live conversation draws the question as waiting
    And A's read-only tab draws "Moved to the current conversation" in its place

  Scenario: Narrow window
    Given the window is narrower than the two-panel layout
    When the conversation panel slides over
    Then the picker is in its header and works the same way
```

### Gaps

None.

## Amendment January 2026 — Ordin's language and language files (ADR-109)

> Source: decision 89 (the user's decisions of January 2026); HLD revision 0.58.0, ADR-109 (amends ADR-042 and ADR-063), 11 §4, §5.19a and UX-A77. Owners of the data: `components/Localisation.md` (`localisation.list_languages`, `.list_messages`, `.edit_message`, `.restore_message`, `.restore_language`, `.mark_reviewed`, `.confirm_language`, `.remove_language`, `.export_language`, `.import_language`; event `language_changed`; the checks and their sentences) and `components/OrdinHost.md` (`ordin.estimate_language_draft`, `ordin.draft_language`, `ordin.stop_language_draft`; the draft offer in the conversation). The sections above carry the change in place: SA-RECON-03; the event union (`language_changed`); Internal Logic step 1; the Settings row of step 9's table and `settings-section-<name>`; the safety marks (§) on `card-check-and-press` and `card-decided`.

### Shared Context (additions)

Generated types (`ordin-service-api`, from Localisation's and the Ordin Host's specs), read as they are:

- `LanguageSummary { locale, native_name?, direction, kind: 'shipped' | 'made' | 'imported', state: 'drafting' | 'needs_review' | 'in_use', choosable, offered_by_company, edited, in_english, needs_review, needs_changing, made_for_version?, draft?: { done, total } }`.
- `MessageRow { message_id, area, english, text?, shipped?, source: 'shipped' | 'edited' | 'drafted' | 'imported' | 'english', safety, reviewed, needs_changing }`.
- `MessageFilter` fields sent with `localisation.list_messages`: `text?` (1–200 characters), `area?`, `only?: 'needs_review' | 'in_english' | 'edited' | 'needs_changing'`, plus `locale` and `cursor?`.
- `ImportReport { locale, stored, dropped_unknown, in_english, left_out, left_out_ids (≤ 100), summary: LanguageSummary }`.
- `LanguageFile { file_name, ftl }` (≤ 2 MiB); `LanguageChangeKind` `'edited' | 'restored' | 'reviewed' | 'confirmed' | 'imported' | 'removed' | 'draft_started' | 'draft_progress' | 'draft_ended'`.
- `UsageEstimate { band: 'light' | 'medium' | 'heavy', min_turns, max_turns, … }` (UsagePlanner.md), in `ordin.estimate_language_draft`'s reply `{ messages, estimate? }`.

Words (11 §3): "language" and "wording", never "locale", "catalogue", "Fluent" or "message identifier" outside Diagnostics; the language tag is shown only after the language's name ("Japanese — ja"). Safety wording is called "approval wording" on screen.

### Internal Logic (additions)

1. **SM-LG-1 Catalogue text from the core.** Step 1 as amended: `localisation.get_catalogue` for `active.locale` at start, again on `locale_changed`, and again on `language_changed` whose `locale` is in `active.fallback_chain`; the new bundles replace the old in one store update, so every mounted view redraws its words in the same frame, without a restart. The brand's `strings` are laid over them again. While the call is in flight the old words stay; an error keeps them and records `UI_CATALOGUE_RELOAD_FAILED` in the diagnostics ring (step 12), retried on the next of those events.
2. **SM-LG-2 Settings › Language** (`Settings/Language.svelte`), after **How Ordin talks to you**. Reads `localisation.get_settings` and `localisation.list_languages`; re-reads on `locale_changed` and `language_changed`.
    - **Language** picker (`language-picker-label`): a WAI-ARIA listbox of `language-follow-computer` (sends `localisation.set_locale { locale: null }`) and every `offered` language by `native_name` with its tag ("Français — fr-FR"); a person's language carries `language-made-by-you` or `language-imported`. A language with `choosable: false` is not listed, and a bundled language whose catalogue does not hold every message is not in `offered` (Localisation LOC-CMP-1, amended January 2026). Choosing one sends `localisation.set_locale { locale }`.
    - Buttons **Edit wording** (`language-edit`, opens SM-LG-3 for the active language), **Export…** (SM-LG-6), **Import…** (SM-LG-5), **Make a new language…** (SM-LG-4).
    - **Your languages** (`language-yours`): one row per `LanguageSummary` with `kind` `made` or `imported`, and per shipped language with `edited > 0`; each names the language and one state line, first match: `draft` present → `language-state-drafting` with **Stop drafting** (`ordin.stop_language_draft { locale }`); `needs_review > 0` → `language-state-needs-checking`; `state = needs_review` → `language-state-ready-to-use`; `needs_changing > 0` → `language-state-needs-changing`; `in_english > 0` → `language-state-in-english` with **Ask Ordin to draft them** (SM-LG-4 from its estimate step, tag and name filled); else `language-state-in-use`. **Open** on each row opens SM-LG-3. Counts in the state line are announced politely when they change (11 §10).
3. **SM-LG-3 Language screen** (`LanguageScreen`, centre, fixed view, one tab per language; title `language-screen-title`). Reads `localisation.list_messages` (page 100, **Show more** for the next cursor) and the language's `LanguageSummary`; re-reads on `language_changed` for its `locale`.
    - **Header**: the language's name and state line (SM-LG-2); **Use this language** (`language-use`, `localisation.confirm_language`) shown while `state = needs_review`, unavailable with `language-use-check-first` while `needs_review > 0`; **Restore all** (`language-restore-all`, shipped languages with `edited > 0`; `ConfirmDialog` `confirm-restore-all-title` / `-body` / verb `language-restore-all-verb` → `localisation.restore_language`); **Remove this language** (`language-remove`, made and imported only; `ConfirmDialog` `confirm-remove-language-title` / `-body` / verb `language-remove-verb` → `localisation.remove_language`, then the tab closes). Neither verb is the default button.
    - **Find**: a search box (`language-search-label`; debounced 200 ms → `text`), an **Area** select (`language-area-label`, `language-area-all` then `areas` from `localisation.list_languages`), and filter toggles `language-only-needs-checking`, `language-only-in-english`, `language-only-edited`, `language-only-needs-changing` (one at a time, `aria-pressed`); the result count is announced politely (`language-results`).
    - **Rows**, one per `MessageRow`: the English text, the language's text (or `language-shows-english`), the source label (`language-source-*`), and `language-approval-wording` when `safety`. Text is drawn as text, never as markup. Actions: **Edit** (`language-edit-message`); **Restore** (`language-restore`, shown when `shipped` is present and `source` is not `shipped`; sends `localisation.restore_message`); **Mark as checked** (`language-mark-checked`, on a row with `safety` and not `reviewed`; sends `localisation.mark_reviewed { locale, message_ids: [id] }`); with the *Needs checking* filter on, **Mark all shown as checked** (`language-mark-all-checked`) sends the shown ids, at most 200 per call.
    - **Editing**: **Edit** turns the row's text into a multi-line text box labelled `language-edit-label`, filled with `text` (else `english`), with **Save** and **Cancel**; Escape cancels and returns focus to **Edit**. **Save** sends `localisation.edit_message { locale, message_id, ftl }`. A refusal (`LOC_MESSAGE_REFUSED` or any other) is drawn by `ErrorLine` under the box, tied by `aria-describedby`, the box keeps the person's text and focus; the saved text is unchanged. On success the row redraws from the reply and focus returns to **Edit**; the words on other screens change on the `language_changed` that follows (SM-LG-1).
    - **Keyboard and focus** (11 §10): the tab opens with focus on the heading; Tab order is header actions, find controls, rows; each row is a group named by its English text; F6 order is unchanged (step 14).
    - **Right to left**: the language's text and its edit box carry `dir` from the summary's `direction` and `lang` from its tag; the English column keeps `dir="ltr"` and `lang="en-GB"`.
4. **SM-LG-4 Make a new language…** (`MakeLanguageDialog`, modal): fields `language-make-name` (1–60 characters) and `language-make-tag` (the language's short code, 2–35 characters, hint `language-make-tag-hint`); **Next** sends `ordin.estimate_language_draft { locale }` and shows `language-make-estimate` (or `language-make-no-estimate` when `estimate` is absent), `language-make-review-line`, **Draft it** (`language-offer-draft`, not the default button) and **Cancel**. Nothing is drafted before **Draft it**; **Cancel** sends nothing more. **Draft it** sends `ordin.draft_language { locale, name }`; the dialog closes and the row of SM-LG-2 shows `language-state-drafting`. Errors (`OH_DRAFT_RUNNING`, `OH_NOTHING_TO_DRAFT`, `LOC_TAG_INVALID`, `LOC_LANGUAGE_EXISTS`, `LOC_TOO_MANY_LANGUAGES`, a paused plan) are drawn by `ErrorLine` in the dialog with focus on the field it concerns, or on the first field. The draft offer Ordin shows in the conversation is an ordinary question block (Ordin Host), drawn as any question.
5. **SM-LG-5 Import…** The screens read the person's chosen file with the WebView's standard file input (`<input type="file" accept=".ftl,text/plain">`, *chosen here*: the screens hold no open-file command of their own; no path crosses to the core, as SA-CLOSE-03's certificate import keeps paths out of the screens). A file larger than 2,097,152 bytes is not read: the sentence of `error-loc-file-too-large`, served by the core, is drawn and nothing is sent. Otherwise the file's text is sent as `localisation.import_language { file_name, ftl, replace: false }`. A `LOC_LANGUAGE_EXISTS` reply opens `ConfirmDialog` `confirm-replace-language-title` / `-body` with the verb `language-replace-verb` and **Cancel**; the verb resends with `replace: true`; **Cancel** sends nothing. On an `ImportReport`, a polite announcement and one line `language-import-report` (with `language-import-left-out` when `left_out > 0`, folding the `left_out_ids` behind **Show which**), then SM-LG-3 opens for its `locale` with the *Needs checking* filter on when `summary.needs_review > 0`. Any other error is drawn by `ErrorLine` and nothing changes.
6. **SM-LG-6 Export…** asks which language (the listbox of SM-LG-2, every language but English (UK)), sends `localisation.export_language { locale }`, then sends `files.save_export { file_name, text: ftl }` to the local backend, which opens the save dialog and writes the file (OrdinDesktop.md; the workflow screen's **Export** sends `workflows.export`'s `file_name` and `yaml` the same way). The person cancelling the choice writes nothing and shows nothing.
7. **SM-LG-7 A personal server's languages.** In Settings › Servers, each connected personal server's row has **Languages…** (`server-languages`), which opens SM-LG-2's content as a screen bound to that server's `BackendHandle` (its title names the server); every command of SM-LG-2…6 goes to that backend. A server reply `LOC_LANGUAGES_UNAVAILABLE` (agent profile) is drawn as its sentence and the actions are not offered.
8. **SM-LG-8 Right-to-left languages.** A made or imported language whose `ActiveLocale.direction` is `rtl` sets `<html dir="rtl">` as step 1 does for any language; nothing else differs.

### Message catalogue rows (en-GB, *chosen here* unless 11 quotes them)

| Key | en-GB | Source |
|---|---|---|
| `settings-section-language` | Language | 11 §4 (amended) |
| `language-picker-label` | Language ordin uses | 11 §5.19a |
| `language-follow-computer` | Follow this computer | 11 §5.19a |
| `language-made-by-you` / `language-imported` | Made by you / Imported | 11 §5.19a |
| `language-edit` | Edit wording | 11 §5.19a |
| `language-export` / `language-import` / `language-make` | Export… / Import… / Make a new language… | 11 §5.19a |
| `language-yours` ‡ | Your languages | — |
| `language-state-drafting` | Drafting { $done } of { $total } | 11 §5.19a |
| `language-stop-drafting` ‡ | Stop drafting | — |
| `language-state-needs-checking` | Needs checking: { $count } approval { $count -> [one] message *[other] messages } | 11 §5.19a |
| `language-state-ready-to-use` ‡ | Checked — ready to use | — |
| `language-state-needs-changing` ‡ | { $count } { $count -> [one] message needs *[other] messages need } changing after the update | — |
| `language-state-in-english` | { $count } { $count -> [one] message *[other] messages } still in English | 11 §5.19a |
| `language-ask-ordin-draft` | Ask Ordin to draft them | 11 §5.19a |
| `language-state-in-use` ‡ | In use | — |
| `language-open` ‡ | Open | — |
| `language-screen-title` ‡ | Wording: { $language } | — |
| `language-use` | Use this language | 11 §5.19a |
| `language-use-check-first` ‡ | Check the approval wording first | — |
| `language-restore-all` / `language-restore-all-verb` ‡ | Restore all… / Restore all | — |
| `confirm-restore-all-title` ‡ | Restore all of { $language } to the wording ordin came with? | — |
| `confirm-restore-all-body` ‡ | Your { $count } changes are removed. | — |
| `language-remove` / `language-remove-verb` ‡ | Remove this language… / Remove | — |
| `confirm-remove-language-title` ‡ | Remove { $language }? | — |
| `confirm-remove-language-body` ‡ | Its wording is deleted from this computer. Export it first if you may want it again. | — |
| `language-search-label` ‡ | Find wording | — |
| `language-area-label` / `language-area-all` ‡ | Part of ordin / All parts | — |
| `language-only-needs-checking` / `language-only-in-english` / `language-only-edited` / `language-only-needs-changing` ‡ | Needs checking / In English / Edited / Needs changing | — |
| `language-results` ‡ | { $count } { $count -> [one] message *[other] messages } | — |
| `language-shows-english` ‡ | Shows in English | — |
| `language-source-shipped` / `-edited` / `-drafted` / `-imported` / `-english` | Came with ordin / Edited by you / Drafted by Ordin / Imported / In English | 11 §5.19a |
| `language-approval-wording` | Approval wording | 11 §5.19a |
| `language-edit-message` / `language-restore` ‡ | Edit / Restore | — |
| `language-edit-label` ‡ | Wording in { $language } | — |
| `language-mark-checked` | Mark as checked | 11 §5.19a |
| `language-mark-all-checked` ‡ | Mark all shown as checked | — |
| `language-make-name` / `language-make-tag` ‡ | Language name / Language code | — |
| `language-make-tag-hint` ‡ | For example ja for Japanese, or pt-BR for Portuguese as spoken in Brazil | — |
| `language-make-estimate` | Estimated use: { $band }, about { $min } to { $max } model turns, for { $count } messages. | 11 §5.19a |
| `language-make-no-estimate` ‡ | { $count } messages to draft. | — |
| `language-band-light` / `-medium` / `-heavy` ‡ | light / medium / heavy | — |
| `language-make-review-line` | Before you use it, you check the wording on approval cards. | 11 §5.19a |
| `language-offer-draft` / `language-offer-not-now` | Draft it / Not now | 11 §5.19a |
| `confirm-replace-language-title` ‡ | Replace your { $language }? | — |
| `confirm-replace-language-body` ‡ | The wording in the file replaces the wording you have now. | — |
| `language-replace-verb` ‡ | Replace | — |
| `language-import-report` | Imported { $stored } messages; { $unknown } not known here were left out; { $english } show in English. | 11 §5.19a |
| `language-import-left-out` ‡ | { $count } { $count -> [one] message didn't pass the checks and was *[other] messages didn't pass the checks and were } left out. | — |
| `language-show-which` ‡ | Show which | — |
| `server-languages` ‡ | Languages… | — |

`language-offer-draft` and `language-offer-not-now` are the same keys the Ordin Host's draft offer uses. The problem sentences of a refused edit are Localisation's (`language-problem-*` inside `LOC_MESSAGE_REFUSED`'s sentence); the screens draw the reply's `message` and hold none of them.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `localisation.get_catalogue` fails while reloading after `locale_changed` or `language_changed` | recovered: the previous words stay; retried on the next such event | `UI_CATALOGUE_RELOAD_FAILED` | — (diagnostics ring only, step 12) |

Every other code is its owner's and is drawn by `ErrorLine` as the owner's sentence: `LOC_*` (Localisation), `OH_DRAFT_RUNNING`, `OH_NOTHING_TO_DRAFT`, `OH_DEPENDENCY_FAILED` (Ordin Host).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: The picker lists a confirmed made language and not an unconfirmed one
    Given the person has made language "ja" in state "in_use" and imported "pt-BR" in state "needs_review"
    When Settings › Language opens
    Then the picker lists "日本語 — ja" marked "Made by you"
    And it does not list "pt-BR"
    And "Your languages" shows pt-BR with "Needs checking: 14 approval messages"

  Scenario: An edit that is refused keeps the person's text
    Given the Language screen of "fr-FR" is open on message "run-progress-documents-read"
    When the person removes "{ $count }" and presses Save
    And the reply is an error with code LOC_MESSAGE_REFUSED
    Then the error's sentence is drawn under the text box, tied by aria-describedby
    And the text box still holds the person's text and has focus

  Scenario: An edit shows on screen without a restart
    Given the active locale is "fr-FR" and Home shows "12 documents lus"
    When the person saves new wording for "run-progress-documents-read" and the event language_changed {"locale":"fr-FR","change":"edited"} arrives
    Then localisation.get_catalogue is called for "fr-FR"
    And Home shows the new wording without the window reloading

  Scenario: Restoring one message
    Given message "card-open" of "fr-FR" has source "edited"
    When the person presses Restore on it
    Then localisation.restore_message is sent for "fr-FR" and "card-open"
    And the row shows "Came with ordin"

  Scenario: An import reports what it did and opens on Needs checking
    When the person imports "ordin-language-pt-BR.ftl" and the reply is an ImportReport with stored 1180, dropped_unknown 3, in_english 20 and summary.needs_review 14
    Then the line "Imported 1180 messages; 3 not known here were left out; 20 show in English." is shown and announced
    And the Language screen of "pt-BR" opens with the "Needs checking" filter on

  Scenario: Importing a language the person already has asks first
    When localisation.import_language replies LOC_LANGUAGE_EXISTS
    Then the dialog "Replace your Português?" offers Replace and Cancel, Replace not the default button
    When the person presses Cancel
    Then no further command is sent

  Scenario: The estimate is shown before drafting, and Cancel drafts nothing
    When the person fills "Japanese" and "ja" in Make a new language… and presses Next
    Then ordin.estimate_language_draft is sent and "Estimated use: medium, about 12 to 15 model turns, for 1200 messages." is shown
    When the person presses Cancel
    Then ordin.draft_language is never sent

  Scenario: Use this language waits for the approval wording
    Given language "ja" has state "needs_review" and needs_review 3
    Then "Use this language" is unavailable with "Check the approval wording first"
    When the person marks the 3 rows as checked
    Then "Use this language" becomes available and sends localisation.confirm_language

  Scenario: Export cancelled saves nothing
    When the person chooses Export… for "fr-FR" and cancels where to save it
    Then nothing is written and no message is shown

  Scenario: A right-to-left made language
    Given the person has made language "ar" and chooses it
    When locale_changed arrives with active {"locale":"ar","source":"person","direction":"rtl","fallback_chain":["ar","en-GB"]}
    Then the html element has dir "rtl" and lang "ar"

  Scenario: The Language screen by keyboard
    Given the Language screen of "fr-FR" opens
    Then focus is on its heading
    When the person types "document" in Find wording
    Then the result count is announced
    When the person presses Edit on the first row and then Escape
    Then the edit is cancelled and focus is on that row's Edit button

  Scenario: A large file is not read
    When the person picks a 3 MB file in Import…
    Then the sentence "That file is too large to be a language file." is shown
    And localisation.import_language is not sent

  Scenario: A personal server's languages
    Given personal server "Home server" is connected
    When the person chooses Languages… on its row in Settings › Servers
    Then a screen titled with "Home server" opens and localisation.list_languages is sent to that server's backend
```

### Gaps

None.

---

## Amendment January 2026 — Reading other teams' knowledge (ADR-110)

> Source: decision 90 (the user's decisions of January 2026); HLD revision 0.59.0, ADR-110 (amends ADR-103: one own folder, any number of read folders; adds Adopt and folders the company names), 11 §5.31a as amended and UX-A78. Owners of the data: `components/TeamKnowledgeService.md`, `components/MemoryService.md` and `components/KnowledgeService.md` (their amendments January 2026). Every rule below adds to the Team knowledge amendment above; a workspace that reads no other folder looks exactly as it does there.

### Shared Context (additions)

The generated types `TeamKnowledgeStatus` (with `folders`, `own_folder_id`, `workspace_facts`, `workspace_large`, `last_recall_check`, `descriptions`), `TeamFolderStatus`, `TeamHeld`, `ReviewState`, `RecallCheckReport`, `FolderDescription`, `AdoptItem`, `AdoptEdit`, `AdoptResult` and `TeamBusy::CheckingRecall` (TeamKnowledgeService.md); the `FactView` fields `folder`, `folder_description`, `set_by_company`, `adopted_from`, `adoption_mark` (MemoryService.md); the `PageView`/`PageSummary` fields `folder`, `folder_description`, `set_by_company`, `folder_id`, `read_only`, `adopted_from`, `adoption_mark` and `PageListing.from_other_teams` (KnowledgeService.md). The three rules of the Team knowledge amendment hold for every new line: no Git word, the sentence never the code, and what is shared said before it is shared.

New messages (en-GB; *chosen here*; ids follow the `team-*` pattern; `[…]` are Fluent arguments):

| Id | en-GB text |
|---|---|
| `team-also-reads` | Also reads |
| `team-read-another` | Read another team's knowledge… |
| `team-read-dialog-intro` | Agents in this workspace will use this team's knowledge. Nothing from it is shared to [own folder] unless you adopt it. |
| `team-read-verb` | Start reading |
| `team-stop-reading` | Stop reading |
| `team-stop-reading-confirm` | Stop reading [folder]? Its knowledge leaves this workspace. Anything you adopted from it stays. |
| `team-set-by-company` | Set by your company |
| `team-folder-counts` | [facts] facts and [pages] pages |
| `team-folder-no-access` | You can't open [display name] yet. |
| `team-folder-review-not-required` | [display name] is set to need review, but its changes aren't reviewed. |
| `team-folder-review-cannot-tell` | [display name] is set to need review, but ordin can't see its review rules with your access. |
| `team-item-elsewhere` | [file] is already here from another folder, so it wasn't taken in again. |
| `team-description-not-taken` | A folder description wasn't used: it reads like an instruction or may hold personal data. |
| `team-workspace-large` | This workspace now holds more than 100,000 facts. Searches may slow down; consider reading only the parts of a folder you need. |
| `team-describe` | Describe this folder… |
| `team-describe-help` | A short description helps people and agents know what this folder holds. It's shared the next time you share. |
| `team-adopt` | Adopt into [own folder]… |
| `team-adopt-title` | Adopt into [own folder] |
| `team-adopt-from` | From [folder], shared by [name] |
| `team-adopt-help` | You can change the text first, for example to make it a rule for everyone. It's shared the next time you share. |
| `team-adopt-verb` | Adopt |
| `team-adopted` | Adopted. It's shared the next time you share. |
| `team-adopted-from` | Adopted from [folder] |
| `team-changed-at-source` | Changed at its source since you adopted it |
| `team-removed-at-source` | Removed at its source |
| `team-adopt-again` | Adopt again |
| `team-keep-as-is` | Keep as it is |
| `team-from-folder` | From [folder] |
| `team-read-only-page` | This page belongs to another team's folder. Adopt it to change it. |
| `team-check-recall` | Check how well ordin finds this knowledge |
| `team-busy-checking-recall` | Checking how well ordin finds this knowledge… |
| `team-recall-result` | [folder]: finds [percent]% of the expected facts among its first 5 answers, across [checks] questions |
| `team-recall-no-checks` | No folder has questions to check yet. |
| `team-no-own-folder` | This workspace reads knowledge your company names but doesn't share to a team folder. |

### Internal Logic (additions to step 9, fixed views)

1. **Header.** A workspace with an own folder keeps `team-header`. A workspace with no own folder but at least one folder in `folders` (company-named folders, `applies_to: code`) shows `team-no-own-folder` in the panel in place of **Share Ordin's memory and knowledge pages with your team** (`team-share`, label *amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-1); the **⋯** menu offers no share (*amended January 2026 (user decision; Approved by the user January 2026)*: the workspace is shared from its tree row, SM-TW-13).
2. **Also reads** (`team-also-reads`), under the own folder's lines, one block per entry of `folders`, in the order given:
   - the folder's `name`, its `description` in secondary text when present, and the `team-set-by-company` tag when `set_by_company`;
   - `team-folder-counts` with `held.facts` and `held.pages`;
   - `available` → `team-available` and **Get your team's memory and knowledge pages** (`team-get`, label *amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-1), which sends `team_knowledge.get { ws, folder_id }`; `busy` and `last_error` draw as for the own folder;
   - `blocked` → its sentence through `ErrorLine`: `TEAM_COMPANY_FOLDER_NO_ACCESS` as `team-folder-no-access` with no action (SM-ER-3); `TEAM_REVIEW_NOT_REQUIRED` and `TEAM_REVIEW_CANNOT_TELL` as their messages, with no action;
   - `problems` → the **Not taken in** list as for the own folder, `TEAM_ITEM_HELD_ELSEWHERE` drawn as `team-item-elsewhere` and `TEAM_DESCRIPTION_NOT_TAKEN` as `team-description-not-taken`;
   - `folder_large` → `team-folder-large`;
   - **Stop reading** (`team-stop-reading`) only when `may_stop`; it opens a confirmation (`team-stop-reading-confirm`, **Cancel** first in focus) and sends `team_knowledge.stop_reading { ws, folder_id }`. A folder set by the company with `may_stop` false shows no **Stop reading** and no explanation beyond its tag.
   `workspace_large` → `team-workspace-large` once, under the list.
3. **Read another team's knowledge…** (`team-read-another`), in the panel and the **⋯** menu, shown only when the workspace has an own folder. The dialog is the *Share knowledge with a team…* dialog's two choices (a linked folder, or the folder's web address split by the same rule, any folder depth), with `team-read-dialog-intro` above the verb **Start reading**, which sends `team_knowledge.add_read_folder { ws, choice }`. Refusals under the fields through `ErrorLine`: `TEAM_LINK_NOT_SUITABLE`, `TEAM_NO_ACCESS`, `TEAM_SIGN_IN_NEEDED`, `TEAM_REMOTE_UNREACHABLE`, `TEAM_FORMAT_NEWER`, `TEAM_FOLDER_ALREADY_READ`, `TEAM_FOLDER_IS_OWN`, `TEAM_READ_LIMIT`.
4. **Describe this folder…** (`team-describe`), on the own folder only, when its access is read-write: a list of the own folder's `descriptions` (the folder itself first, then each sub-folder given) with one text field each (1–500 characters, `team-describe-help` below), **Save** sending `team_knowledge.set_description { ws, path, text }`; an emptied field sends `text: null`.
5. **Where a fact or page came from.** In What Ordin remembers, in recall results shown in a conversation, and in the page list and page screen:
   - an item with `folder` shows `team-from-folder` and, when `set_by_company`, the `team-set-by-company` tag; `folder_description` is the line's tooltip and its accessible description;
   - an item with `adopted_from` shows `team-adopted-from`;
   - an item with `adoption_mark` shows `team-changed-at-source` or `team-removed-at-source` as a notice with **Adopt again** (opens the Adopt dialog of step 6 on the source's current text, sending `team_knowledge.adopt_again`) and **Keep as it is** (`team_knowledge.keep_as_is { ws, item }`); with `removed_at_source` only **Keep as it is** is offered.
   The page list draws `from_other_teams` under a heading per folder name, after the workspace's own pages. A read page (`read_only`) opens without **Edit**, **Remove**, **Copy to another workspace** or **Publish to Confluence**, and shows `team-read-only-page` with **Adopt into [own folder]…**.
6. **Adopt into [own folder]…** (`team-adopt`) is offered on every fact or page with `folder` set, when the workspace has an own folder with read-write access (otherwise not shown). It opens a dialog titled `team-adopt-title` with `team-adopt-from`, the text in an editable field (a fact's statement; a page's title and body), `team-adopt-help`, **Cancel** and **Adopt** (`team-adopt-verb`). **Adopt** sends `team_knowledge.adopt { ws, item, edit }`, with `edit` carrying only the fields the person changed (none → `edit: null`). On success the dialog closes and `team-adopted` is announced (polite live region). A refusal stays in the dialog as one sentence through `ErrorLine` (`TEAM_ADOPT_PERSONAL_DATA`, `TEAM_ADOPT_REFUSED`, `TEAM_ADOPT_INVALID`, `TEAM_ADOPT_NOT_READ_ITEM`), the text kept so the person can change it and try again. Adopting is only ever this dialog: no conversation, block or card adopts.
7. **Check how well ordin finds this knowledge** (`team-check-recall`), in the panel's **⋯**: sends `team_knowledge.check_recall { ws }`; while `busy` is `checking_recall` the line `team-busy-checking-recall` shows; `last_recall_check` is drawn one line per folder as `team-recall-result` (`percent` = `recall_at_5_permille / 10`, rounded; `checks` as reported), or `team-recall-no-checks` when `folders` is empty.
8. On a connected server's workspace the same lines and dialogs are drawn through the Remote API.

### Error Table (rows added)

N/A — no new code; the new `TEAM_*`, `MEM_*` and `KNOW_*` refusals are drawn by `ErrorLine` as their owners' sentences.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — reading another team's folder
    Given workspace W with own folder "acme/supplier-team › knowledge"
    When the person chooses Read another team's knowledge…, enters "https://github.com/acme/platform-team/tree/main/knowledge" and presses Start reading
    Then team_knowledge.add_read_folder is sent with url "https://github.com/acme/platform-team", branch "main", path "knowledge"
    And the panel lists "acme/platform-team › knowledge" under Also reads with Stop reading

  Scenario: Edge case — a folder set by the company cannot be stopped
    Given folders holds "Security standards" with set_by_company true and may_stop false
    Then its block shows "Set by your company" and no Stop reading

  Scenario: Edge case — no access to a folder set by the company
    Given folders holds "Security standards" with blocked TEAM_COMPANY_FOLDER_NO_ACCESS
    Then its block shows "You can't open Security standards yet." with no button
    And every other folder's block is drawn as before

  Scenario: Happy path — adopting a fact with a change
    Given a recalled fact F with folder "acme/platform-team › knowledge"
    When the person chooses Adopt into acme/supplier-team › knowledge…, changes the statement and presses Adopt
    Then team_knowledge.adopt is sent with item {kind: "fact", id: F} and edit.statement set
    And "Adopted. It's shared the next time you share." is announced

  Scenario: Error path — the adoption is refused for personal data
    Given team_knowledge.adopt fails with TEAM_ADOPT_PERSONAL_DATA
    Then the dialog stays open with "This may hold personal data, so it wasn't adopted. Change the text and try again." and the edited text kept

  Scenario: Edge case — a source changed after adoption
    Given an own fact with adopted_from "acme/platform-team › knowledge" and adoption_mark changed_at_source
    Then it shows "Changed at its source since you adopted it" with Adopt again and Keep as it is
    When the person presses Keep as it is
    Then team_knowledge.keep_as_is is sent for that fact

  Scenario: Edge case — a read page is read-only
    Given knowledge.get returns a page with read_only true
    Then the page screen shows no Edit or Remove and shows "This page belongs to another team's folder. Adopt it to change it."

  Scenario: Edge case — no adopt without an own folder
    Given a workspace that reads only company-named folders
    Then no fact or page offers Adopt into… and the panel shows "This workspace reads knowledge your company names but doesn't share to a team folder."

  Scenario: Edge case — no Git words in the new messages
    When every message of this amendment is rendered in en-GB
    Then none contains "branch", "commit", "push", "pull", "merge", "rebase", "conflict" or "clone"
```

### Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026: every agent gets memory, the person sees the whole of it in this step, every open question settled with its proposed default); HLD revision 0.60.0, ADR-111 (amends ADR-030), 11 §5.10 as amended and UX-A79. Owners of the data: `components/MemoryService.md` (amendment January 2026 — Agentic memory as a core capability), `components/OrdinHost.md` (the answer block's `memory_used`) and `components/KnowledgeService.md` (the page a thing is explained in). Every rule below adds to the sections above; a workspace whose memory holds no things or links looks as it does there, with the fact lines gaining only their kind and "true since".

### Shared Context (additions)

The generated types `FactView` (with `kind`, `true_from`, `true_until`, `replaces`, `fading`, `not_used_since`, `things`, `made_from_count`), `MemoryKind`, `ThingRef`, `ThingView`, `ThingDetail`, `LinkView`, `LinkBy`, `PageLabel`, `FactHistoryView`, `OriginsView`, `MemoryUsedView`, `RememberedInRun`, `ThingCount`, `ThingForgetReceipt`, `MemoryRecallId`, `MemoryThingId`, `MemoryLinkId` (MemoryService.md); the answer block's `memory_used: [MemoryRecallId]` (OrdinHost.md, 2C wire §7). Commands used (Service API catalogue, MemoryService.md): `memory.things`, `memory.thing`, `memory.fact_history`, `memory.fact_origins`, `memory.keep`, `memory.used`, `memory.remembered_in_run`, `memory.correct_link`, `memory.forget_link`, `memory.thing_count`, `memory.forget_thing`, `memory.same_as`, `memory.forget_count`, with the existing `memory.list`, `memory.get`, `memory.correct`, `memory.forget`, `memory.remember_everywhere`, `memory.keep_checked`.

Three rules hold for every new line: **no store words** — never "entity", "relationship", "graph", "node", "edge", "episodic", "semantic", "consolidated", "salience", "decay", "embedding" or "score" on a screen a business user sees (11 §3 vocabulary); **the sentence, never the code** (`ErrorLine`); **nothing changes without the person's choice** — every action here is a button the person presses, and none is offered to Ordin or an agent.

New messages (en-GB; *chosen here* unless 11 §5.10 quotes them; ids follow a `memory-*` pattern; `[…]` are Fluent arguments):

| Id | en-GB text |
|---|---|
| `memory-tab-facts` | Facts |
| `memory-tab-things` | Things |
| `memory-kind-happened` | Something that happened |
| `memory-kind-known` | Something known |
| `memory-kind-summary` | A summary |
| `memory-true-since` | True since [date] |
| `memory-true-between` | True from [from] to [until] |
| `memory-replaced` | Replaced "[statement]" |
| `memory-fading` | Fading — not used since [date] |
| `memory-keep` | Keep |
| `memory-kept` | Kept. Ordin will keep using this. |
| `memory-history` | History |
| `memory-history-title` | How this fact changed |
| `memory-history-version` | [statement] · [when] |
| `memory-made-from` | Made from |
| `memory-made-from-count` | Made from [count] facts |
| `memory-origin-no-longer-true` | No longer true |
| `memory-forget-confirm` | Forget this fact? |
| `memory-forget-confirm-summaries` | This also forgets [count] summary made from it. / This also forgets [count] summaries made from it. (Fluent plural) |
| `memory-forgotten` | Forgotten. Ordin won't use this again. |
| `memory-things-search` | Find a thing |
| `memory-thing-counts` | [facts] facts · [links] links |
| `memory-thing-also-called` | Also called [names] |
| `memory-explained-in` | Explained in [page] |
| `memory-open-page` | Open the page |
| `memory-link-line` | [from] [kind] [to] · since [date] |
| `memory-link-line-ended` | [from] [kind] [to] · from [since] to [until] |
| `memory-link-by-person` | Set by you |
| `memory-link-by-page` | From the page [page] |
| `memory-link-correct` | Correct |
| `memory-link-correct-title` | Correct this link |
| `memory-link-correct-help` | Say how these two things are related, in a few words. |
| `memory-link-forget` | Forget |
| `memory-link-forget-confirm` | Forget that [from] [kind] [to]? The facts stay. |
| `memory-thing-forget` | Forget everything about this… |
| `memory-thing-forget-confirm` | Forget everything about [thing]? This forgets [facts] facts and [links] links. |
| `memory-thing-forget-confirm-summaries` | It also forgets [summaries] summaries made from them. |
| `memory-thing-forget-verb` | Forget all of it |
| `memory-thing-forgotten` | Forgotten. Ordin won't use anything about [thing] again. |
| `memory-thing-count-changed` | Something about [thing] changed while you were deciding. Check the numbers again. |
| `memory-same-as` | Same as… |
| `memory-same-as-title` | Which thing is [thing] the same as? |
| `memory-same-as-help` | The two become one. [thing] becomes another name for the one you choose. |
| `memory-same-as-verb` | Make them one |
| `memory-same-as-done` | [merged] is now another name for [kept]. |
| `memory-things-empty` | Ordin hasn't noted any things in this workspace yet. When it learns who or what a fact is about, they appear here. |
| `memory-used` | What Ordin used |
| `memory-used-title` | What Ordin used for this answer |
| `memory-used-gone` | [count] item was forgotten since. / [count] items were forgotten since. (Fluent plural) |
| `memory-used-page` | [page] › [section] |
| `memory-remembered-in-run` | Remembered during this run |
| `memory-remembered-by` | Remembered by [agent] |
| `memory-remembered-by-ordin` | Remembered by Ordin |
| `memory-remembered-none` | Nothing was remembered during this run. |
| `memory-hosted-scope-channel` | In #[channel] |
| `memory-hosted-scope-direct` | In direct chats with [person] |
| `memory-hosted-scope-agent` | Everywhere this agent works |
| `memory-hosted-title` | What [agent] remembers |

### Internal Logic (additions)

1. **What Ordin remembers — two tabs** (the `memory` view of step 6 and the fixed view of step 9, Settings › What Ordin remembers, and the tree's *What Ordin remembers* node): **Facts** (`memory-tab-facts`, first, as before) and **Things** (`memory-tab-things`), a WAI-ARIA tab list; the chosen tab is a view preference per workspace. Agents' own pages, **Open Knowledge** and the Team knowledge lines stay under both tabs as before. The tabs are drawn as tabs, not buttons: words on the pane's line, the shown tab underlined in the accent colour; ArrowLeft and ArrowRight move and show (wrapping), Home and End go to the first and the last tab, and the focus follows the tab shown; only the shown tab is in the tab order (amended January 2026; user decision; Approved by the user January 2026).
2. **Each fact line** (Facts tab, a thing's facts, **What Ordin used**, **Remembered during this run**), drawn by one component `FactLine`, adds to the existing statement, origin and actions:
   - its kind in words from `kind` (`memory-kind-happened`, `memory-kind-known`, `memory-kind-summary`) as secondary text;
   - `memory-true-since` with `true_from`, or `memory-true-between` when `true_until` is present;
   - when `replaces` is present: `memory-replaced` with the replaced fact's statement, read through `memory.get` for that id within the same workspace (a `MEM_FACT_NOT_FOUND` answer drops the line, never an error);
   - when `fading`: the notice `memory-fading` with `not_used_since` and **Keep** (`memory-keep`), which sends `memory.keep { id }`; on success the notice goes, the kind reads `memory-kind-known`, and `memory-kept` is announced (polite live region); `MEM_ALREADY_KEPT` redraws the line from the reply's fact with no message;
   - **History** (`memory-history`) in the line's **⋯** menu: opens a dialog titled `memory-history-title` listing `memory.fact_history { id }`'s `versions` oldest first as `memory-history-version` (`when` = `memory-true-since` or `memory-true-between` of that version), with the links of the reply under them as link lines (step 4, without actions); History opens inside the fact's own line, under its details, never at the foot of the list or as a dialog: the title `memory-history-title` (a level 4 heading), the versions and the links; only one fact's history is open at a time, and pressing **History** again on that fact closes it (its button carries `aria-expanded`) (amended January 2026; user decision; Approved by the user January 2026).
   - for `kind = summary`: `memory-made-from-count` with `made_from_count`, a disclosure that on opening sends `memory.fact_origins { id }` and lists each `OriginItem`: a present `fact` as a `FactLine` without actions; `state = no_longer_true` marked `memory-origin-no-longer-true`;
   - `things` as small links under the statement, each opening that thing on the Things tab (step 3).
   **Forget** on a fact first sends `memory.forget_count { id }` and opens a confirmation (`ConfirmDialog`) titled `memory-forget-confirm`, with `memory-forget-confirm-summaries` when `summaries > 0`, the verb **Forget** (not the default button) and **Cancel**; confirming sends `memory.forget` as before and announces `memory-forgotten` (SA-AMEM-02).
3. **Things tab.** A search field (`memory-things-search`, debounced 150 ms) over `memory.things { workspace_id, search?, cursor? }`; a list, 100 a page with **Show more** while `next_cursor` is present, each row: the thing's `name`, `memory-thing-also-called` with `other_names` when not empty, `memory-thing-counts`, and when `explained_in` is present `memory-explained-in` with **Open the page** (`memory-open-page`), which opens the Knowledge screen at `explained_in.page_id`. Empty → `memory-things-empty`. A row opens the thing's detail (`memory.thing { workspace_id, id }`) in place, with a back control to the list: the name, other names, the explained-in line, its facts as `FactLine`s (newest first) and its links as link lines (step 4); actions **Forget everything about this…** and **Same as…** (steps 5 and 6).
4. **Link lines** (`LinkLine`): `memory-link-line` with `from.name`, `kind`, `to.name` and `since`, or `memory-link-line-ended` when `until` is present; the other thing's name is a link to its detail. Secondary text from `by`: `agent` → `memory-remembered-by` with the agent's name; `ordin` → `memory-remembered-by-ordin`; `person` → `memory-link-by-person`; `page` → `memory-link-by-page` with **Open the page**; `team` → `team-from-folder` or the own folder's line as the Team knowledge amendment draws it. Actions on a live link:
   - **Correct** (`memory-link-correct`) opens a dialog titled `memory-link-correct-title` with `memory-link-correct-help`, three fields — the first thing (a picker over `memory.things`, preset to `from`), the words between them (1–64 characters, preset to `kind`), the second thing (preset to `to`) — and **Save**, which sends `memory.correct_link { id, kind, from, to }`; the dialog closes and the detail re-reads. A refusal stays in the dialog through `ErrorLine`.
   - **Forget** (`memory-link-forget`) opens a confirmation `memory-link-forget-confirm` (**Cancel** first in focus) and sends `memory.forget_link { id }`; then `memory-forgotten` is announced.
   An ended link has no actions.
5. **Forget everything about this…** (`memory-thing-forget`): sends `memory.thing_count { workspace_id, id }`, then opens a confirmation titled with `memory-thing-forget-confirm` (`facts`, `links` from the reply) and `memory-thing-forget-confirm-summaries` when `summaries > 0`, the verb **Forget all of it** (`memory-thing-forget-verb`, never the default button) and **Cancel** (default, first in focus). Confirming sends `memory.forget_thing { workspace_id, id, expected }` with `expected` = the counts shown. `MEM_THING_COUNT_CHANGED` re-reads the counts and redraws the same dialog with `memory-thing-count-changed` above the new numbers, still waiting for the person; any other refusal shows through `ErrorLine` (`MEM_DELETE_UNVERIFIED` with **Try again**). On success: back to the list, `memory-thing-forgotten` announced.
6. **Same as…** (`memory-same-as`): a dialog titled `memory-same-as-title` with `memory-same-as-help`, a picker over `memory.things` excluding this thing, and **Make them one** (`memory-same-as-verb`), which sends `memory.same_as { workspace_id, keep: <the chosen thing>, merge: <this thing> }`; on success the kept thing's detail opens and `memory-same-as-done` is announced. `MEM_SAME_THING` cannot arise from the picker; it is drawn through `ErrorLine` if it does.
7. **What Ordin used** (conversation panel). An answer whose `message` block carries a non-empty `memory_used` shows a quiet link `memory-used` under the answer (keyboard-reachable, not a button style). Choosing it sends `memory.used { workspace_id, recall_ids: memory_used }` and opens a side panel titled `memory-used-title`: the facts as `FactLine`s and the links as `LinkLine`s, each with **Correct** and **Forget** as on the Facts and Things tabs; a fact with `page_id` shows `memory-used-page` with the page's title and `page_section`, and **Open the page** (opening the page at that section, as the ADR-104 amendment does); `gone > 0` → `memory-used-gone` at the foot. Nothing is fetched until the person chooses the link. An answer with no `memory_used` shows nothing. The existing in-words mention by Ordin ("as you asked last week…", 11 §5.10) is unchanged.
8. **Remembered during this run** (`plan_progress` block, the *What happened* layer of 11 §5.4): a disclosure `memory-remembered-in-run`, closed by default, that on opening sends `memory.remembered_in_run { run_id }` and lists each item as a `FactLine` with `memory-remembered-by` (agent or crew member, named from the workspace's agents the screens already hold, looked up by the reply's `agent_instance_id`; the reply carries no name, MemoryService.md SA-AMEM-07) or `memory-remembered-by-ordin`; empty → `memory-remembered-none`. It re-reads on `run_progress` events while open and the run is running.
9. **Hosted agents** (open question 6; the operator screen under **Hosted agents** in the side rail, UX-A67). The operator screen gains a section `memory-hosted-title` drawn only when the hosted agent's Caller API operator route (`POST /operator/memory/<verb>`, CallerApi.md route 33) answers the memory commands for the signed-in person (it answers only the agent's owners; for any other person the call is refused, `AUTH_FORBIDDEN`, and the section is not drawn, with no message). The section lists the agent's scopes, each headed `memory-hosted-scope-channel`, `memory-hosted-scope-direct` or `memory-hosted-scope-agent`, and opens on choice the same Facts and Things tabs, with every command sent with `scope_id` in place of `workspace_id`, through that operator route. **Remember everywhere** is not shown there. Channel members never see this section; what they see is the agent's **What I used** answer in their channel (ChannelGateway.md amendment January 2026), not drawn by this module.
10. On a connected personal server's workspace the same tabs, lines and dialogs are drawn through the Remote API, as step 10 does for every view.

### Error Table (rows added)

N/A — no new code; the new `MEM_*` refusals (`MEM_THING_NOT_FOUND`, `MEM_LINK_NOT_FOUND`, `MEM_THING_COUNT_CHANGED`, `MEM_SAME_THING`, `MEM_ALREADY_KEPT`, `MEM_RECALL_IDS_INVALID`, `MEM_LINKS_INVALID`) are drawn by `ErrorLine` as the Memory Service's sentences; `MEM_THING_COUNT_CHANGED` additionally re-reads the counts (step 5).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the Things tab lists every thing with its counts (criterion 9)
    Given memory.things returns "Borealis" (12 facts, 4 links, other names ["Borealis Ltd"]) and "packaging" (3 facts, 1 link)
    When the person opens What Ordin remembers and chooses Things
    Then two rows are drawn: "Borealis · Also called Borealis Ltd · 12 facts · 4 links" and "packaging · 3 facts · 1 link"
    When the person types "Borealis Ltd" in Find a thing
    Then memory.things is sent with search "Borealis Ltd" and the row "Borealis" is drawn

  Scenario: Happy path — a thing's links read as words
    Given memory.thing returns a live link from "Borealis" kind "supplies" to "packaging" since 8 Jan
    Then its detail shows "Borealis supplies packaging · since 8 Jan" with Correct and Forget

  Scenario: Happy path — a fact corrected twice shows its kind, truth and history (criterion 10)
    Given a FactView with kind known, true_from 10 Jan, replaces F2 whose statement is "Prefers pricing in dollars"
    Then its line shows "Something known", "True since 10 Jan" and "Replaced "Prefers pricing in dollars""
    When the person chooses History
    Then memory.fact_history is sent and three versions are listed oldest first, the first two with "True from … to …"

  Scenario: Happy path — a fading fact can be kept (criterion 11)
    Given a FactView with fading true and not_used_since 4 Jan
    Then its line shows "Fading — not used since 4 Jan" with Keep
    When the person presses Keep
    Then memory.keep is sent for that fact and "Kept. Ordin will keep using this." is announced
    And the line shows "Something known" and no fading notice

  Scenario: Happy path — What Ordin used lists the facts and links of an answer (criterion 12)
    Given an answer whose message block has memory_used [R1]
    And memory.used returns three facts and one link
    When the person chooses What Ordin used under the answer
    Then the panel lists the three facts and the one link, each with Correct and Forget
    When the person presses Forget on the first fact and confirms
    Then memory.forget is sent for that fact

  Scenario: Edge case — nothing is fetched for an answer that used no memory
    Given an answer whose message block has no memory_used
    Then no What Ordin used link is drawn and memory.used is never sent

  Scenario: Happy path — remembered during this run (criterion 13)
    Given memory.remembered_in_run returns two facts by the crew member "Supplier analyst" and one by Ordin
    When the person opens Remembered during this run on the run's progress
    Then three lines are listed with "Remembered by Supplier analyst" twice and "Remembered by Ordin" once

  Scenario: Happy path — forget everything about a thing names the counts (criterion 14)
    Given memory.thing_count returns facts 12, links 4, summaries 0 for "Borealis"
    When the person chooses Forget everything about this…
    Then the confirmation reads "Forget everything about Borealis? This forgets 12 facts and 4 links."
    And Cancel has focus and Forget all of it is not the default button
    When the person presses Forget all of it
    Then memory.forget_thing is sent with expected {facts: 12, links: 4, summaries: 0}

  Scenario: Error path — the counts changed while deciding
    Given memory.forget_thing fails with MEM_THING_COUNT_CHANGED
    Then memory.thing_count is sent again and the same dialog shows "Something about Borealis changed while you were deciding. Check the numbers again." above the new numbers

  Scenario: Happy path — two things made one (criterion 15)
    Given the things "Borealis" and "Borealis Ltd"
    When the person opens "Borealis Ltd", chooses Same as…, picks "Borealis" and presses Make them one
    Then memory.same_as is sent with keep "Borealis" and merge "Borealis Ltd"
    And "Borealis Ltd is now another name for Borealis." is announced

  Scenario: Happy path — a thing explained in a page (criterion 20)
    Given memory.things returns "Borealis" with explained_in {page_id: P, title: "Supplier Borealis"}
    Then its row shows "Explained in Supplier Borealis" with Open the page
    When the person presses Open the page
    Then the Knowledge screen opens at page P
    And a fact from P listed in What Ordin used shows "Supplier Borealis › Contract terms"

  Scenario: Edge case — Forget names the summaries that go with a fact
    Given memory.forget_count returns summaries 1
    When the person presses Forget on that fact
    Then the confirmation reads "Forget this fact?" and "This also forgets 1 summary made from it."

  Scenario: Edge case — no store words on these screens
    When every message of this amendment is rendered in en-GB
    Then none contains "entity", "relationship", "graph", "node", "edge", "episodic", "semantic", "consolidated", "salience", "decay", "embedding" or "score"

  Scenario: Agent profile — only owners see a hosted agent's memory
    Given the hosted agent's Caller API operator route refuses memory.things with AUTH_FORBIDDEN for the signed-in person
    Then the operator screen draws no "What Support agent remembers" section and shows no message
```

### Gaps

None.

---

## Amendment January 2026 — Knowledge agents and the Open Knowledge Format (ADR-112, ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-112 (ordin's wiki is an Open Knowledge Format bundle) and ADR-113 (knowledge agents: a team's front door to the organisation's knowledge), 11 as amended and UX-A80. Owners of the data: `components/KnowledgeQueryClient.md` (front doors of a workspace), `components/MemoryService.md` (agent facts in **What Ordin used**, `more_found`), `components/KnowledgeService.md` (page type, description and marks) and `components/CallerApi.md` (the hosted agent's knowledge status on the operator route). Every rule below adds to the sections above; a workspace with no front door and pages with no type look as they do there.

### Shared Context (additions)

The generated types `KnowledgeConnectionView`, `KnowledgeAbout` and `KnowledgeConnectionId` (KnowledgeQueryClient.md); `MemoryUsedView.wider` with `WiderUsed`, `UnansweredSource` (MemoryService.md, 2C wire); the `PageView`/`PageSummary` fields `type`, `description`, `status` and `past_stale_after` (KnowledgeService.md), and `FactView.page_status` with `may_be_out_of_date` (MemoryService.md). Commands used: `knowledge_agents.list`, `knowledge_agents.add`, `knowledge_agents.remove`, `knowledge_agents.details`, with the existing `memory.used`, `knowledge.save_edit`, `knowledge.write`. Events used: `knowledge_agents_changed` (re-read `knowledge_agents.list`) and `more_found` (re-read `memory.used` for an open panel of that recall).

The rules of the earlier amendments hold for every new line: no store words; **no Git word** on a business user's screen (never "repository", "commit", "branch", "clone" or "fetch": a repository is drawn as its place name, a commit as "version [short id]", a fetch as "read"); the sentence, never the code; nothing changes without the person's choice. **A front door is never connected by Ordin or an agent**: connecting and removing are buttons of this module only (`PERSON_ONLY_COMMANDS`, ServiceApi.md amendment January 2026).

New messages (en-GB; *chosen here* unless 11 quotes them; ids follow a `knowledge-agents-*`, `memory-*` and `knowledge-*` pattern; `[…]` are Fluent arguments):

| Id | en-GB text |
|---|---|
| `settings-section-knowledge-agents` | Knowledge agents |
| `knowledge-agents-intro` | A knowledge agent answers from your organisation's knowledge. When ordin's own memory isn't enough, it asks the agents you connect here. Only your question is sent, never your conversation or files. |
| `knowledge-agents-workspace` | For workspace |
| `knowledge-agents-none` | This workspace isn't connected to a knowledge agent. It works as it always has. |
| `knowledge-agents-connect` | Connect a knowledge agent… |
| `knowledge-agents-connect-title` | Connect a knowledge agent |
| `knowledge-agents-address` | The knowledge agent's address |
| `knowledge-agents-address-help` | Ask the team that runs it for its address. You sign in with your company account. |
| `knowledge-agents-connect-verb` | Connect |
| `knowledge-agents-record-on` | This knowledge agent keeps a record of which sources each answer used, for [days] days. The record never holds your question or your name. |
| `knowledge-agents-record-off` | This knowledge agent keeps no record of which sources its answers used. |
| `knowledge-agents-connected` | Connected. Ordin will ask [name] when its own memory isn't enough. |
| `knowledge-agents-set-by-company` | Set by your company |
| `knowledge-agents-last-answered` | Last answered [when] |
| `knowledge-agents-never-answered` | Not asked yet |
| `knowledge-agents-details` | Details |
| `knowledge-agents-places` | Reads [count] places |
| `knowledge-agents-as-of` | Up to date as of [when] |
| `knowledge-agents-remove` | Remove |
| `knowledge-agents-remove-confirm` | Stop asking [name]? Ordin will answer from this workspace's own memory only. |
| `knowledge-agents-removed` | Removed. Ordin won't ask [name] again. |
| `memory-used-from-agent` | From [agent] · [place] › [file] · version [version] |
| `memory-used-as-of` | As of [when] |
| `memory-used-answer-id` | Answer reference [id] |
| `memory-more-found` | More found |
| `memory-more-found-added` | [count] more result arrived. / [count] more results arrived. (Fluent plural) |
| `memory-did-not-answer` | [agent] did not answer in time. |
| `memory-agent-unreachable` | [agent] could not be reached. |
| `knowledge-type` | Type |
| `knowledge-type-explanation` | Explanation |
| `knowledge-type-how-to` | How-to |
| `knowledge-type-decision` | Decision |
| `knowledge-type-reference` | Reference |
| `knowledge-type-glossary` | Glossary |
| `knowledge-description` | Description |
| `knowledge-description-help` | One sentence saying what this page explains. Others see it in lists. |
| `knowledge-mark-draft` | Draft |
| `knowledge-mark-deprecated` | Deprecated |
| `knowledge-mark-out-of-date` | May be out of date |
| `hosted-knowledge-title` | What [agent] reads |
| `hosted-knowledge-place` | [place] · read [when] |
| `hosted-knowledge-place-unreadable` | [place] can't be read with this agent's access. Its answers are left out. |
| `hosted-knowledge-unread` | [count] documents are waiting to be read; today's reading limit is used up. |
| `hosted-knowledge-conflict` | "[name]" means different things in [first] and [second]. [first] is used until the owners settle it. |

### Internal Logic (additions)

1. **Settings › Knowledge agents** (`settings-section-knowledge-agents`, after Team knowledge in Settings; a fixed view `Settings/KnowledgeAgents`). `knowledge-agents-intro` first, then a workspace picker (`knowledge-agents-workspace`, preset to the workspace open in the centre, every workspace of the backend listed). For the chosen workspace it sends `knowledge_agents.list { workspace_id }` and draws one row per connection, in the order given: `name`; `knowledge-agents-set-by-company` as a tag when `added_by` is `company`; `knowledge-agents-last-answered` with `last_answered_at`, or `knowledge-agents-never-answered`; the sources-record sentence (`knowledge-agents-record-on` with `retention_days`, or `knowledge-agents-record-off`; nothing when `sources_record` is `null`); actions **Details** and **Remove**. No connection → `knowledge-agents-none`. Below the list, **Connect a knowledge agent…** (`knowledge-agents-connect`). A `knowledge_agents_changed` event for the chosen workspace re-reads the list.
2. **Connect dialog** (`knowledge-agents-connect-title`). One field `knowledge-agents-address` (checked in the screen for the `https://` prefix before sending, `UI_KNOWLEDGE_ADDRESS_NOT_HTTPS` otherwise), `knowledge-agents-address-help` below it, **Cancel** and **Connect** (`knowledge-agents-connect-verb`). **Connect** sends `knowledge_agents.add { workspace_id, url }`; the client reads the front door's `about` with the person's company sign-in before it keeps anything (KnowledgeQueryClient.md). On success the dialog stays open on a second stage that draws `about.agent_name` and the sources-record sentence from `about.sources_record` (`knowledge-agents-record-on` or `knowledge-agents-record-off`), with two buttons: **Keep it** (closes the dialog; `knowledge-agents-connected` announced in a polite live region) and **Remove** (sends `knowledge_agents.remove { connection_id }` at once, with no further confirmation, because nothing has been asked of the front door yet). Refusals stay in the dialog as one sentence through `ErrorLine`, the address kept; a sign-in refusal offers **Sign in again** (`sign_in_again`, Settings › Sign-ins at the company account); `KQ_NOT_IN_AUDIENCE` is drawn as its owner's sentence with no action.
3. **Details** opens a side panel: `knowledge_agents.details { connection_id }`; draws `about.agent_name`, `knowledge-agents-places` with the number of places it reads, `knowledge-agents-as-of`, the sources-record sentence, and the address. An unreachable front door shows its sentence through `ErrorLine` above the stored view.
4. **Remove** opens a `ConfirmDialog` titled with `knowledge-agents-remove-confirm` (**Cancel** first in focus; **Remove** never the default button), then sends `knowledge_agents.remove { connection_id }` and announces `knowledge-agents-removed`. A company entry removes the same way; it is not added back.
5. **Ordin asked in words.** When the person asks Ordin to connect a knowledge agent, Ordin answers in words and names Settings › Knowledge agents; the conversation draws no button that connects (SA-UI-KA-02). Asking Ordin to "look wider" needs no screen: the answer that follows carries its own **What Ordin used**.
6. **What Ordin used, agent facts** (adds to step 7 of the ADR-111 amendment). The panel draws `MemoryUsedView.wider` after the workspace's own facts and links, one source line each without **Correct** or **Forget** (they belong to another installation), and with no statement — the recall record keeps an outside fact's source, never its text (MemoryService.md SA-MKA-07): `memory-used-from-agent` with `agent`, the place's display name from `repository` (the repository key's last two path segments before `@`), `path`'s file name and the first 7 characters of `commit`; `memory-used-page` with `page_title` and `section` when `page_title` is present; `memory-used-as-of` with `as_of`; `memory-used-answer-id` with `answer_id` when present (selectable text, so the person can quote it to an auditor); the marks of step 8. The order is the view's.
7. **More found and late agents.** While a panel of **What Ordin used** for recall R is open, a `more_found` event with that `recall_id` re-sends `memory.used` and draws a notice `memory-more-found` with `memory-more-found-added`; each entry of `unanswered` is drawn once: reason `not_in_time` as `memory-did-not-answer`, reason `refused` or `unreachable` as `memory-agent-unreachable` ("[agent] could not be reached."). Under an answer, a quiet line `memory-more-found` appears beside **What Ordin used** when a `more_found` event arrives for one of the answer's `memory_used` ids, so the person knows to look; nothing is fetched until the person opens the panel. When the view's `wider` facts were incomplete at the first deadline, the names the Memory Service records as not answered are drawn as `memory-did-not-answer` at the panel's foot.
8. **Page marks.** Wherever a page or a page-backed fact is drawn (page list, page screen, **What Ordin used**, a thing's facts, recall results in a conversation): `status = draft` → `knowledge-mark-draft`; `deprecated` → `knowledge-mark-deprecated`; a page's `past_stale_after` or a fact's `may_be_out_of_date` → `knowledge-mark-out-of-date`, the same notice the page screen already draws when a source changed. Marks are text tags, never colour alone.
9. **Page editor.** The edit view gains **Type** (`knowledge-type`, a select of the five types in their message ids; a value read from a file that is not one of the five is shown as written, selected, and kept unless the person picks another) and **Description** (`knowledge-description`, one line, 1–200 characters, `knowledge-description-help` below), beside the tags and other names of the ADR-104 amendment. **Save** sends `knowledge.save_edit` with `type` and `description` only when the person changed them. A new page (`knowledge.write`) is created with the type the person picked (default Explanation) and the description when given. The page screen shows `description` under the title in secondary text, and the page list shows it as each row's second line.
10. **Hosted agents, knowledge status** (the operator screen under **Hosted agents**, UX-A67). When the hosted agent is a knowledge agent, the screen reads its knowledge status from the operator route `GET /operator/knowledge` (CallerApi.md amendment January 2026, route 40; a `ReaderStatus`) and the screen draws `hosted-knowledge-title`: one line per place it reads, `hosted-knowledge-place` with the place's display name and its last read time, or `hosted-knowledge-place-unreadable`; `hosted-knowledge-unread` when documents wait for the next day's reading limit; one `hosted-knowledge-conflict` per glossary conflict, naming the two glossaries' display names. The section is drawn only for the agent's operators, as the memory section is; it has no actions in this step.
11. On a connected personal server's workspace, steps 1–9 are drawn through the Remote API, as step 10 does for every view.

### Error Table (rows added)

| Code | When | Screen behaviour |
|---|---|---|
| `UI_KNOWLEDGE_ADDRESS_NOT_HTTPS` | the address in the connect dialog does not start with `https://` | the field shows "Use an address that starts with https://." under it and nothing is sent; a diagnostic of the ring (step 12), never shown as a code |

The `KQ_*` refusals of the Knowledge Query Client (sign-in, unreachable, not in the audience, too many connections, already connected) and the new `KNOW_*` refusals (type, description) are drawn by `ErrorLine` as their owners' sentences.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — connecting a front door shows what it keeps
    Given Settings › Knowledge agents is open for workspace W with no connection
    When the person chooses Connect a knowledge agent…, enters "https://knowledge.example.org" and presses Connect
    Then knowledge_agents.add is sent with workspace_id W and url "https://knowledge.example.org"
    And the dialog shows "This knowledge agent keeps a record of which sources each answer used, for 365 days. The record never holds your question or your name."
    When the person presses Keep it
    Then "Connected. Ordin will ask Supplier knowledge when its own memory isn't enough." is announced

  Scenario: Edge case — a workspace with no front door
    Given knowledge_agents.list returns no connection for W
    Then the section shows "This workspace isn't connected to a knowledge agent. It works as it always has."

  Scenario: Error path — an address that is not https
    When the person enters "http://knowledge.example.org" and presses Connect
    Then "Use an address that starts with https://." is shown under the field and no command is sent

  Scenario: Error path — the person is outside the agent's audience
    Given knowledge_agents.add fails with KQ_NOT_IN_AUDIENCE
    Then the dialog shows the Knowledge Query Client's sentence for KQ_NOT_IN_AUDIENCE and no action

  Scenario: Happy path — removing a front door asks first
    Given a connection "Supplier knowledge" set by the company
    When the person presses Remove
    Then the confirmation reads "Stop asking Supplier knowledge? Ordin will answer from this workspace's own memory only." with Cancel in focus
    When the person presses Remove in the confirmation
    Then knowledge_agents.remove is sent for that connection

  Scenario: Happy path — What Ordin used names the agent and the source
    Given memory.used returns a wider source from agent "Supplier knowledge", repository "git.example.org/acme/contracts@main", path "suppliers/borealis.md", commit "9f2c1e7a0b", page_title "Supplier Borealis", section "Contract terms", as_of 18 Jan 09:40, answer_id A
    When the person opens What Ordin used
    Then the line shows "From Supplier knowledge · acme/contracts › borealis.md · version 9f2c1e7", "Supplier Borealis › Contract terms", "As of 18 Jan 09:40" and "Answer reference A"
    And the line shows no statement and has no Correct and no Forget

  Scenario: Happy path — late results arrive as More found
    Given What Ordin used is open for recall R
    When a more_found event arrives with recall_id R, added 2 and unanswered [{agent: "Finance knowledge", reason: not_in_time}]
    Then memory.used is sent again and "More found" with "2 more results arrived." is shown
    And "Finance knowledge did not answer in time." is shown once

  Scenario: Happy path — a page's type and description are saved
    Given the page "Supplier Borealis" of type Explanation is being edited
    When the person picks Type "Reference", writes the description "Who Borealis is and what we buy from them." and presses Save
    Then knowledge.save_edit is sent with type "Reference" and that description
    And the page list shows the description as the page's second line

  Scenario: Edge case — an unknown type read from a file is kept
    Given a page whose file has type "Runbook"
    When the person edits only the body and presses Save
    Then knowledge.save_edit is sent with no type field

  Scenario: Edge case — marks are drawn as words
    Given a page with status deprecated and past_stale_after true
    Then its row shows the tags "Deprecated" and "May be out of date"

  Scenario: Agent profile — the operator sees what a knowledge agent reads
    Given the operator status of hosted agent "Supplier knowledge" lists 3 places, 120 unread documents and a conflict on "BRL" between "Sales glossary" and "Finance glossary"
    When an operator opens the agent under Hosted agents
    Then "What Supplier knowledge reads" lists 3 places, "120 documents are waiting to be read; today's reading limit is used up." and ""BRL" means different things in Sales glossary and Finance glossary. Sales glossary is used until the owners settle it."

  Scenario: Edge case — no Git words on the new screens
    When every message of this amendment is rendered in en-GB
    Then none contains "repository", "commit", "branch", "clone" or "fetch"
```

### Assumptions

- **SA-UI-KA-01** The connect dialog shows what a front door keeps right after **Connect**, with **Keep it** or **Remove**, because the client reads the front door's `about` as part of the add and offers no separate check; nothing is asked of the front door before the person chooses, since no recall runs while the dialog is open.
- **SA-UI-KA-02** No conversation block connects a front door; Ordin names the Settings section in words. A later change may add an offer that only opens this dialog.
- **SA-UI-KA-03** A repository is drawn as its host-less last two path segments and a commit as its first 7 characters, so no Git word reaches a business user; the full values are kept in the line's accessible description for support.
- **SA-UI-KA-04** Agent facts in **What Ordin used** carry no **Correct** or **Forget**: they are another installation's knowledge, changed through Git by its owners.

### Gaps

None.

---

## Amendment January 2026 — Releases and updates (ADR-114)

> Source: decision 93 (the user's decisions of January 2026); HLD revision 0.62.0, ADR-114 (each build answers the module groups of its build switches; ordin updates itself from one signed release feed, and a personal server updates itself when the person asks), 11 as amended and UX-A81. Owners of the data: `components/ServiceApi.md` (`ServiceHello.groups`, `SVC_NOT_IN_THIS_VERSION`, `SettingsSection::About`) and `components/SetupAssistant.md` (`UpdateOffer`, `UpdateState`, `WhatsNew`, `ServerUpdate`, the five `setup.*` commands of this amendment and the `SETUP_*` refusals). Every rule below adds to the sections above.
>
> *Decoupled January 2026: build switches replace release-numbered features; releases are planned in the maintainers' release roadmap, kept outside the repository.*

### Shared Context (additions)

The generated types `ServiceHello` with `groups: string[]`, `UpdateOffer` with `state: 'downloading' | 'ready'`, `WhatsNew { version, notes, seen }`, `ServerUpdate { version, notes, self_update }`, and the `SettingsSection` value `about`. Commands used: `service.hello` (existing; now also read for `groups`), `setup.update_offer` and `setup.install_update` (existing), `setup.check_update`, `setup.whats_new`, `setup.whats_new_seen`, `setup.server_update`, `setup.update_server`. No event is added: the update line re-reads `setup.update_offer` as step SM-UP-3 states.

The rules of the earlier amendments hold for every new line: the sentence, never the code; nothing changes without the person's choice; no technical word on a business user's screen (never "feed", "signature", "archive", "binary", "migration" or "unit").

New messages (en-GB; *chosen here* unless 11 quotes them; ids follow an `about-*`, `update-*`, `whats-new-*` and `server-update-*` pattern; `{ $… }` are Fluent arguments):

| Id | en-GB text | Source |
|---|---|---|
| `settings-section-about` | About ordin | 11 §4 (UX-A81) |
| `about-version` | Version { $version } | |
| `about-check` | Check for updates | 11 §4 (UX-A81) |
| `about-checking` | Checking for updates… | |
| `about-up-to-date` | ordin is up to date. | |
| `about-downloading` | Getting ordin { $version } ready… | |
| `about-ready` | ordin { $version } is ready. | |
| `about-could-not-check` | ordin couldn't check for updates. Try again later. | |
| `about-whats-new` | What's new | 11 §4 (UX-A81) |
| `about-whats-new-none` | There are no notes for this version. | |
| `update-ready-line` | ordin { $version } is ready. | ADR-114 item 4 |
| `update-restart` | Restart to update | ADR-114 item 4 |
| `update-later` | Not now | |
| `update-restarting` | Restarting to update… | |
| `whats-new-title` | What's new in ordin { $version } | ADR-114 item 4 |
| `whats-new-got-it` | Got it | |
| `server-update-available` | { $server } can update to ordin { $version }. | ADR-114 item 5 |
| `server-update-button` | Update server… | ADR-114 item 5 |
| `server-update-confirm-title` | Update { $server } to ordin { $version }? | |
| `server-update-confirm-body` | Work running on { $server } stops and is marked as interrupted. The server restarts within a minute and then continues. | ADR-114 item 5 |
| `server-update-confirm-verb` | Update server | |
| `server-update-started` | { $server } is updating. It will be back within a minute. | |
| `server-update-notes` | What's new in this version | |

### Internal Logic (additions)

1. **SM-UP-1 Groups a backend answers.** At each load and reload of a backend (step 5), `service.hello`'s `groups` is kept in that backend's store as a set. Every entry that sends a command — a menu item, a Settings section, a button, a tree node, a tab, a block action, a command of *Search or go to…* — is drawn for a backend only when the group of each command it sends (the part of the command name before the first `.`, which is `CommandMeta.module`) is in that backend's set. An entry whose group is absent is not drawn at all: never greyed out, never labelled as coming later, never counted in a badge. The rule is applied once, in `lib/backend/groups.ts` (*chosen here*), through `answers(backend, command): boolean`; views call it, and no view keeps its own list. A Settings section whose every command is absent is left out of the section list, and the next present section takes its place in keyboard order. When a reply is nonetheless `SVC_NOT_IN_THIS_VERSION` (a backend updated between hello and the command), `ErrorLine` draws its sentence ("This needs a newer version of ordin."), and the backend is reloaded so the entry disappears.
2. **SM-UP-2 Settings › About** (`settings-section-about`), the last section after Diagnostics, a fixed view `Settings/About`, local backend only. It draws, in order: the brand's product name and `about-version` with the application's version, read once through `lib/backend/tauri.ts` from the application-version call of `@tauri-apps/api` 2.12.4 (*chosen here*; SA-UI-UP-04); **Check for updates** (`about-check`); one status line in a polite live region; **What's new** (`about-whats-new`). *Amended (amended January 2026; user decision; Approved by the user January 2026):* the section is named **Updates** (`settings-section-about` and `about-title` both "Updates"; the product name is no longer in the heading). It draws the version, **Check for updates** and the status line, and below them, inside the section, this version's release notes: when the backend answers `setup.whats_new` the section reads it on opening and draws `whats-new-title` with the version and the notes, or `about-whats-new` with `about-whats-new-none` when there are none. There is no **What's new** button and no side panel to close.
   1. **Check for updates** sends `setup.check_update`. While it runs the button is disabled and the status reads `about-checking`. Result `offer: null` → `about-up-to-date`. `offer.state = 'downloading'` → `about-downloading` with `offer.version`, and the view re-sends `setup.update_offer` every 5 s *(chosen here)* while it is open until the state is `ready`. `offer.state = 'ready'` → `about-ready` with **Restart to update** (`update-restart`) beside it, which acts as SM-UP-3.2. A refusal `SETUP_UPDATE_UNREACHABLE` or `SETUP_UPDATE_SIGNATURE_INVALID` → `about-could-not-check`, with no code and no technical detail; other refusals through `ErrorLine`.
   2. On opening, the view sends `setup.update_offer` once and draws the same states, so a download that started in the background is shown without pressing.
   3. **What's new** sends `setup.whats_new`; `whats_new` present → a side panel titled `whats-new-title` with its version, the notes drawn as SM-UP-6 states; absent → the panel shows `about-whats-new-none`. Opening it here never sends `setup.whats_new_seen`.
3. **SM-UP-3 The update line.** At start, after the local backend loads, the shell sends `setup.update_offer`, and again every 30 minutes *(chosen here)* and each time Settings › About reads it; an offer with `state = 'ready'` adds one line at the top of the conversation panel's *Needs you* tab — not a pop-up, dialog or notification — reading `update-ready-line` with **Restart to update** (`update-restart`) and **Not now** (`update-later`).
   1. **Not now** hides the line until the next start of the program; nothing is sent and nothing is stored.
   2. **Restart to update** sends `setup.install_update`. The desktop program then runs its quit flow (OrdinDesktop.md DSK-UP-13), so when sessions are running the screens receive `ordin://quit-requested` and ask exactly as quitting does (step 10 of OrdinDesktop.md, drawn by the existing quit confirmation); the confirmation answers with `app.quit` as it always does, and the program ends that quit as the update restart. Nothing else is sent. While the command runs the line reads `update-restarting` and its buttons are disabled. `SETUP_UPDATE_NONE` (the offer was cleared meanwhile) removes the line; `SETUP_UPDATE_INSTALL_FAILED` is drawn by `ErrorLine` with **Try again**.
   3. The line is drawn only for the local backend; a server's version is shown in Settings › Servers (SM-UP-5).
4. **SM-UP-4 What's new at start.** After the local backend's first load in a program session, the shell sends `setup.whats_new`. `whats_new` present with `seen = false` → a card at the top of *Needs you*, above the update line, titled `whats-new-title` with `whats_new.version`, the notes drawn as SM-UP-6 states, and one button **Got it** (`whats-new-got-it`). **Got it** sends `setup.whats_new_seen { version: whats_new.version }` and removes the card; a refusal is drawn by `ErrorLine` and the card stays. The card is not repeated as a notification and is not counted in *Needs you*'s waiting count. Closing the window without pressing **Got it** shows the card again at the next start.
5. **SM-UP-5 A personal server's update, Settings › Servers.** For each server row whose backend is connected and loaded, the view sends `setup.server_update` to that backend once when Settings › Servers opens and again after `server_state_changed` for that server.
   1. `update` present and `self_update = true` → under the row, `server-update-available` with the server's `display_name` and `update.version`, **Update server…** (`server-update-button`) and **What's new in this version** (`server-update-notes`, which opens the SM-UP-2.3 side panel with `update.version` and `update.notes`).
   2. `update` present and `self_update = false` → `server-update-available` followed by the sentence of `SETUP_SELF_UPDATE_UNAVAILABLE` ("This server can't update itself. Whoever runs it installs new versions."), with no button.
   3. `update` absent, or the refusal `SETUP_SELF_UPDATE_UNAVAILABLE` (an agent-profile backend) → nothing is drawn under the row. Any other refusal is drawn by `ErrorLine` under the row.
   4. **Update server…** opens a `ConfirmDialog` titled `server-update-confirm-title`, body `server-update-confirm-body`, verbs **Cancel** (first in focus) and **Update server** (`server-update-confirm-verb`, never the default button). Confirming sends `setup.update_server { version: update.version }` to that server's backend. `{ accepted: true }` → the row's line reads `server-update-started`, announced in a polite live region; the server's connection then drops and returns as the Server Connector reports it (step 10). Refusals are drawn by `ErrorLine` under the row with their owners' sentences: `SETUP_UPDATE_IN_PROGRESS`, `SETUP_UPDATE_VERSION_CHANGED` (the view re-sends `setup.server_update` after drawing it), `SETUP_UPDATE_NO_ARCHIVE`, `SETUP_UPDATE_SIGNATURE_INVALID`, `SETUP_UPDATE_UNREACHABLE` and `SETUP_UPDATE_REPLACE_FAILED` (with **Try again**, which re-opens the confirmation).
   5. A server in `refused_for_version` with `older_side = 'server'` keeps the row sentence `server-version-server-older`; when its `setup.server_update` cannot be read because the versions differ, nothing more is drawn. With `older_side = 'desktop'` the row keeps `server-version-desktop-older`, and the local update line of SM-UP-3 is the way forward.
6. **SM-UP-6 Release notes are plain text.** `UpdateOffer.notes`, `WhatsNew.notes` and `ServerUpdate.notes` are drawn as text nodes only: line breaks are kept (`white-space: pre-line`), a line starting with `- ` or `* ` is drawn as a list item, and nothing else is interpreted — no Markdown links, no images, no HTML. The source-scan test of §6.3 already refuses `{@html` and `innerHTML`; the notes view adds no exception. Notes longer than the 4,000 characters the Setup Assistant keeps are not expected; if a longer string arrives it is drawn whole.

### Error Table (rows added)

| Code | When | Screen behaviour |
|---|---|---|
| `SVC_NOT_IN_THIS_VERSION` | a command whose group the backend does not answer | `ErrorLine` draws "This needs a newer version of ordin." with no action; the backend is reloaded (SM-UP-1) |

The `SETUP_*` refusals named in SM-UP-2, SM-UP-3 and SM-UP-5 are drawn by `ErrorLine` as their owners' sentences; this module adds no `UI_` code.

### Tests (vitest) — added

- `groups.ts`: table-driven over (`groups`, command) pairs — present group true; absent group false; a command with no `.` false; an empty set hides every entry but those of `service` (the hello itself is never gated).
- Settings list: with `groups` lacking `mail` and `telegram`, the Email and Telegram sections are absent and Tab moves from Slack to Jira.
- About: one test per result of `setup.check_update` (null, downloading then ready through `setup.update_offer`, each refusal); the button is disabled while the check runs.
- Update line: ready offer → line with two buttons; **Not now** hides it until the store is re-created; **Restart to update** sends `setup.install_update`; `SETUP_UPDATE_NONE` removes the line.
- What's new: card drawn when `setup.whats_new` returns notes with `seen` false, and not drawn when `seen` is true (About still shows them); **Got it** sends `setup.whats_new_seen` with the version; not counted in *Needs you*.
- Server row: one test per SM-UP-5 branch; the confirmation has **Cancel** in focus and Escape cancels.
- Notes rendering: a string holding `<img src=x onerror=alert(1)>` and `[link](https://example.org)` is drawn as literal text; `- item` lines become list items.
- Catalogue: every message id of this amendment exists in the en-GB `.ftl` files; none contains "feed", "signature", "archive", "binary", "migration" or "unit".

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — an entry of a group the build lacks is not drawn
    Given service.hello of the local backend returns groups without "mail"
    When the person opens Settings
    Then the section list has no Email section
    And no workspace shows "Add to this workspace › Email folder…"

  Scenario: Error path — a command meets an older backend
    Given an entry was drawn for a group the backend no longer answers
    When the person uses it and the reply is SVC_NOT_IN_THIS_VERSION
    Then "This needs a newer version of ordin." is shown with no action
    And the backend is reloaded and the entry disappears

  Scenario: Happy path — checking for updates finds a new version
    Given Settings › About is open on version 1.0.0
    When the person presses Check for updates
    Then "Checking for updates…" is announced
    And when setup.check_update returns an offer 1.1.0 in state downloading
    Then the status reads "Getting ordin 1.1.0 ready…"
    And when setup.update_offer returns state ready
    Then the status reads "ordin 1.1.0 is ready." with Restart to update

  Scenario: Edge case — ordin is up to date
    When setup.check_update returns no offer
    Then the status reads "ordin is up to date."

  Scenario: Error path — the check cannot reach the updates
    When setup.check_update fails with SETUP_UPDATE_UNREACHABLE
    Then the status reads "ordin couldn't check for updates. Try again later." and no code is shown

  Scenario: Happy path — the update line restarts ordin
    Given setup.update_offer returns 1.1.0 in state ready
    Then Needs you shows "ordin 1.1.0 is ready." with Restart to update and Not now
    When the person presses Restart to update
    Then setup.install_update is sent and the line reads "Restarting to update…"

  Scenario: Edge case — Not now hides the line until the next start
    When the person presses Not now on the update line
    Then the line is not shown again while ordin stays open

  Scenario: Happy path — What's new appears once after an update
    Given setup.whats_new returns version 1.1.0 with notes "- Copilot can now run your work" and seen false
    Then Needs you shows "What's new in ordin 1.1.0" with a list item "Copilot can now run your work"
    When the person presses Got it
    Then setup.whats_new_seen is sent with version "1.1.0" and the card is removed

  Scenario: Happy path — updating a personal server asks first
    Given Settings › Servers lists "Northwind server" and setup.server_update returns 1.1.0 with self_update true
    Then the row shows "Northwind server can update to ordin 1.1.0." with Update server…
    When the person presses Update server…
    Then the confirmation reads "Update Northwind server to ordin 1.1.0?" with Cancel in focus
    And its body reads "Work running on Northwind server stops and is marked as interrupted. The server restarts within a minute and then continues."
    When the person presses Update server in the confirmation
    Then setup.update_server is sent with version "1.1.0"
    And "Northwind server is updating. It will be back within a minute." is announced

  Scenario: Edge case — a server that cannot update itself
    Given setup.server_update returns 1.1.0 with self_update false
    Then the row shows "Northwind server can update to ordin 1.1.0." and "This server can't update itself. Whoever runs it installs new versions."
    And no Update server… button is shown

  Scenario: Error path — a newer version came out meanwhile
    When setup.update_server fails with SETUP_UPDATE_VERSION_CHANGED
    Then the row shows "A newer version came out meanwhile. Check again." and setup.server_update is sent again

  Scenario: Security — release notes are never markup
    Given setup.whats_new returns notes "<img src=x onerror=alert(1)>"
    Then the card shows that text literally and no image element is created
```

### Accessibility (additions)

The About status line and the server row's started line are polite live regions; the update line and the **What's new** card are ordinary regions in *Needs you*'s reading order, each with a heading (`update-ready-line` is the line's accessible name; `whats-new-title` the card's heading level 3). **Restart to update**, **Not now**, **Got it** and **Update server…** are buttons reachable by Tab in reading order; none takes focus on its own when it appears. The confirmation follows `ConfirmDialog` (focus on open, Tab trapped, Escape cancels).

### Assumptions

- **SA-UI-UP-01** The group of a command is the part of its name before the first `.`, which equals `CommandMeta.module` for every catalogue row (ServiceApi.md); the screens do not read the catalogue at run time.
- **SA-UI-UP-02** The update line lives in *Needs you* and not in a banner, because 11 keeps pop-ups for approvals and a pending update needs no answer; **Not now** stores nothing, so the line returns at the next start, when restarting costs the person nothing.
- **SA-UI-UP-03** A server's update is offered only in Settings › Servers, never as a line in *Needs you*, because updating stops the server's running work and is a choice made at a quiet moment.
- **SA-UI-UP-04** The application version is read from the Tauri application API, not from the Service API, because `ServiceHello` carries the catalogue version and not the program's; the call goes through `lib/backend/tauri.ts`, so the source-scan rule on `@tauri-apps/api` imports holds.

### Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (knowledge inherits its readers; nothing about a document the person may not open is ever shown, named or counted; the person takes no step to link accounts). Owners of the data: `components/KnowledgeQueryClient.md` (`ConnectionAccount`), `components/MemoryService.md` (`MemoryUsedView.wider_unchecked`), both amendments January 2026; ServiceApi.md amendment January 2026 for the result shapes.

Two additions to the knowledge-agent screens of the January 2026 amendment; nothing else changes. The rules of the earlier amendments hold: no Git word, the sentence never the code, nothing changes without the person's choice.

1. **What Ordin used, unchecked sources** (adds to step 6 of the January 2026 amendment). When `MemoryUsedView.wider_unchecked` is true, the panel draws the sentence of key `memory-used-unchecked` once at its foot, after any `memory-did-not-answer` lines. It names no document, place or number.
2. **Details, accounts** (adds to step 3 of the January 2026 amendment). Below the address, one line per entry of `accounts`, in the order given:
   - `linked` → `knowledge-agents-account-linked` with the system's name;
   - `not_signed_in` → `knowledge-agents-sign-in-to-include` with the system's name, and the action **Sign in** (`sign_in_again`, Settings › Sign-ins at that kind);
   - `refused` → `knowledge-agents-account-refused` with the system's name, and the same **Sign in** action (signing in again gives the front door a fresh token to check).
   No entry → nothing drawn. The system's name is its product name as Settings › Sign-ins draws it (GitHub, GitLab, Confluence); an entry whose system has no name there (SharePoint is never linked) is not drawn.

### Localisation keys (added to the bundled en-GB catalogue)

| Key | en-GB |
|---|---|
| `memory-used-unchecked` | Some sources couldn't be checked. |
| `knowledge-agents-sign-in-to-include` | Sign in to { $system } in ordin to include what you can read there. |
| `knowledge-agents-account-linked` | { $system }: your account is linked. |
| `knowledge-agents-account-refused` | { $system } didn't accept ordin's sign-in for this knowledge agent. |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Screens for per-document read access (ADR-115)

  Scenario: Happy path — What Ordin used says some sources couldn't be checked
    Given memory.used returns a view with wider_unchecked true
    When the person opens What Ordin used
    Then the panel's foot shows "Some sources couldn't be checked."
    And no place, file name or number of withheld sources is drawn

  Scenario: Edge — every source was checked
    Given memory.used returns a view with wider_unchecked false
    When the person opens What Ordin used
    Then "Some sources couldn't be checked." is not drawn

  Scenario: Happy path — Details shows a system to sign in to
    Given knowledge_agents.details returns accounts [{"system":"confluence","state":"not_signed_in"}]
    When the person opens Details for that connection
    Then the panel shows "Sign in to Confluence in ordin to include what you can read there."
    And choosing Sign in opens Settings › Sign-ins at Confluence

  Scenario: Error — a system the catalogue has no name for
    Given knowledge_agents.details returns accounts [{"system":"sharepoint","state":"linked"}]
    When the person opens Details
    Then no line is drawn for that entry and the other lines are drawn as usual

  Scenario: Happy path — Details shows a linked account
    Given knowledge_agents.details returns accounts [{"system":"github","state":"linked"}]
    When the person opens Details
    Then the panel shows "GitHub: your account is linked."
```


## Amendment January 2026 — The install choice (ADR-072)

> Source: decision 95 (the user's decisions of January 2026); HLD revision 0.64.0, ADR-072 as amended, UX-A81; `components/SetupAssistant.md` amendment January 2026 (`UpdateState::AdministratorNeeded`, `CheckKind::SecondInstall`). Every section above stays in force except where this amendment states otherwise.

### Shared Context (additions)

`UpdateOffer.state` gains `'administrator_needed'`. New messages (en-GB):

| id | en-GB | Source |
|---|---|---|
| `update-ask-administrator` | ordin { $version } is available. This copy was installed for everyone on this computer, so install the new version the same way. | ADR-072; amended January 2026 (user decision; Approved by the user January 2026) |
| `setup-second-install` | ordin is also installed in { $folder }. Remove one copy, because updates reach only the copy installed for you. | ADR-072 |

### Internal Logic (additions)

1. **SM-UP-7 Whole-computer update line.** Where SM-UP-3 reads an offer with `state = 'administrator_needed'`, the line in *Needs you* reads `update-ask-administrator` with **Not now** (`update-later`) and no **Restart to update**. Nothing is sent when the line shows.
2. **SM-UP-8 About.** In SM-UP-2.1 and SM-UP-2.2, `offer.state = 'administrator_needed'` draws `update-ask-administrator` in the status line, with no **Restart to update** and no repeated `setup.update_offer`. *Amended (amended January 2026; user decision; Approved by the user January 2026):* the section is **Updates** (SM-UP-2); the rule is unchanged.
3. **SM-UP-9 Second install.** A `SecondInstall` check result is drawn among the start checks as any other result, by its message. It has no action.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a whole-computer install asks for the administrator
    Given setup.update_offer returns 1.1.0 in state administrator_needed
    Then Needs you shows "ordin 1.1.0 is available. This copy was installed for everyone on this computer, so install the new version the same way." with Not now
    And Restart to update is not shown

  Scenario: Edge case — About on a whole-computer install
    When setup.check_update returns 1.1.0 in state administrator_needed
    Then the status reads "ordin 1.1.0 is available. This copy was installed for everyone on this computer, so install the new version the same way."
    And Restart to update is not shown
```

### Gaps

None.

## Amendment January 2026 — Screens for Phase 7A decisions and automatic publishing (FU-089, FU-127)

> **Status:** Approved by the user, 2026-01-10 ("go with your recommendations"); Gaps G-SF-1 and G-SF-2 are to be closed by a later amendment, so the screens they name are built against `lib/backend/wiring.ts` and draw nothing until the commands exist.
>
> Source: the follow-ups FU-089 (screens for what the Phase 7A security fixes put behind a person's review) and FU-127 (the folder's automatic-publishing state, *Ready to publish* items of kind `changed`, and the Actions group of *Search or go to…*), and the user's decision of 2026-01-10 "Home search actions get their own result type", settled against these screens. Owners of the data: `components/ConfluenceAdapter.md` (step 6.1, `ContentRef::FileVersion` on the **Publish checked items…** card; SA-CONF-AUTO-01 to 05, `ReadyKind::Changed`), `components/FileService.md` (`read_version`), `components/ProjectService.md` (SA-RECON-11, `CodingTask.problems_unchecked`, `EditorSession.asks_to_start_servers`, Internal Logic review step 2), `components/MemoryService.md` (SA-TRUST-04, SA-TRUST-05, SA-CITE-02, `FactOrigin::AgentCiting`), `components/WorkspaceService.md` (SA-WS-AUTO-01 to 03, `ConfluencePublishing`, G-AUTO-1) and `components/HomeAndSearch.md` (the Actions group, its *Gaps* entry of 2026-01-10). Every rule below adds to the sections above; nothing above changes except the `ContentRef` copy in *Shared Context*, corrected as stated next.

### Shared Context (additions)

**Correction.** The `ContentRef` copy in *Shared types* lacked the variant `{ kind: 'file_version'; file_version_id: string }` that the generated `ContentRef` (`types.gen.ts`) carries; the copy is corrected in place. The generated file was always the source; only this restatement was stale.

Generated types read by this amendment (each seen in `apps/desktop/ui/src/lib/service-api/types.gen.ts`): `ContentRef` (variant `file_version`), `Preview` (`image`, `pdf`, `text` with `truncated`, `thumbnail`, `none`), `ReviewTask { task, change_set?, new_problems: Problem[] }`, `CodingTask { new_problems: number, problems_unchecked?: boolean, … }`, `Problem { file, line, message, severity, code? }`, `EditorSession { asks_to_start_servers, language?, server_line, stream }`, `Project`, `FactView`, `FactOrigin` (variant `agent_citing { agent_instance_id, origin_ref, version, run_id? }`), `MemoryGetResult`, `InstalledAgent` (`instance.id`, `instance.display_name`), `Grantee` (`agent_instance` | `crew`), `ConfluenceLink`, `ReadyItem { confluence_link_id, kind, path, from_path?, published_copy_id? }`, `ReadyKind` (`new` | `renamed` | `removed` | `changed`), `SearchAnswer { actions: GoToResult[], … }`, `GoToResult`, `OpenTarget` (variant `screen { name }`).

Commands used that the catalogue already carries (each seen in `commands.gen.ts`): `projects.review`, `projects.editor_open`, `projects.editor_close`, `projects.language_servers_answer` (`{ project_id }` → `boolean | null`), `projects.answer_language_servers` (`{ project_id, allowed }` → `Done`), `memory.get` (`{ ids, workspace_id }`, 1–100 ids), `agents.list` (`{ workspace_id? }`), `confluence.ready_to_publish`, `search.query`.

Owner methods the screens need that **no Service API command carries yet** (see *Gaps*): `FileService::read_version(version_id) -> FileBytes`; `held_writes(place) -> Vec<HeldWrite>`, `keep_held(place, id, content_hash)`, `discard_held(place, id)`, `citations_to_confirm(workspace_id) -> Vec<MemoryFactId>`, `confirm_citation(workspace_id, id)` (`crates/ordin-memory-service/src/held.rs`; `HeldWrite`, `HeldThing` and `HeldLink` in `crates/ordin-memory-service/src/model.rs`); `WorkspaceService::confluence_publishing(link) -> ConfluencePublishing`.

**One wiring file** (*chosen here*): `lib/backend/wiring.ts` is the only file that reaches those methods. Until the Service API catalogue carries a command for one, its function returns `{ kind: 'not_wired' }` without sending anything, and each view below states what it draws then. When the catalogue gains the command, the function becomes a `BackendHandle.call` of it, the screens' local mirror types below are replaced by the generated ones, and no view changes. Its functions and their mirror types, field for field from the owners' Rust structs (none of which renames a field):

```ts
// lib/backend/wiring.ts  (chosen here)
type NotWired = { kind: 'not_wired' };
interface VersionContent { path: string; preview: Preview }            // from FileBytes { place_id, path, bytes, hash, media_type }
interface HeldThing { name: string; other_names: string[] }
interface HeldLink { from: string; kind: string; to: string; ended: boolean; confidence_permille?: number | null }
interface HeldWrite {
  id: string; statement: string; about?: string | null; fact_class?: string | null; cited?: string | null;
  things: HeldThing[]; links: HeldLink[]; content_hash: string; trust_permille: number;
  written_by?: string | null; held_at: string; needs_check: boolean;
}
interface ConfluencePublishingView { confluence_link_id: string; automatic: 'on' | 'off'; turned_off_by: Grantee[] }

readFileVersion(b: BackendHandle, file_version_id: string): Promise<CommandOutcome<VersionContent> | NotWired>;
heldWrites(b: BackendHandle, workspace_id: string): Promise<CommandOutcome<HeldWrite[]> | NotWired>;
keepHeld(b: BackendHandle, workspace_id: string, id: string, content_hash: string): Promise<CommandOutcome<unknown> | NotWired>;
discardHeld(b: BackendHandle, workspace_id: string, id: string): Promise<CommandOutcome<unknown> | NotWired>;
citationsToConfirm(b: BackendHandle, workspace_id: string): Promise<CommandOutcome<string[]> | NotWired>;
confirmCitation(b: BackendHandle, workspace_id: string, id: string): Promise<CommandOutcome<unknown> | NotWired>;
confluencePublishing(b: BackendHandle, confluence_link_id: string): Promise<CommandOutcome<ConfluencePublishingView> | NotWired>;
```

The rules of the earlier amendments hold for every new line: the sentence, never the code; nothing changes without the person's choice; no technical word on a business user's screen and no Git word anywhere in this amendment (never "branch", "commit", "push", "pull", "merge", "repository", "hash" or "language server" in the person's text). Text an agent or a colleague's file wrote (a held statement, a thing's name, a link's kind, a cited source) is drawn through Svelte text interpolation only.

New messages (en-GB; *chosen here* unless a source is named; `{ $… }` are Fluent arguments; rows marked § are **safety wording** as defined above *Message catalogue entries*). Every id below is new to this file:

| Id | en-GB text | Source |
|---|---|---|
| `approval-bound-version-heading` § | { $name } — the version this approval covers | FU-089 |
| `approval-bound-version-note` § | Changes made to this file after the card was raised are not included. | ConfluenceAdapter.md step 6.1 |
| `approval-bound-version-cut` § | Only the start of this file is shown. | |
| `approval-bound-version-unavailable` § | ordin can't show the exact version this approval covers. Reject the card if you need to see it first. | |
| `approval-approve-needs-version` § | Approve is off because ordin can't show the exact version this approval covers. You can still reject it. | |
| `changes-problems-none` | No new problems | 11 UX-A25 |
| `changes-problems-count` | { $count } new problem / { $count } new problems (Fluent plural) | 11 UX-A25 |
| `changes-problems-unchecked` | Not checked for new problems: some changed files had no code checking. | ProjectService.md review step 2 |
| `changes-problems-unchecked-also` | Some changed files had no code checking, so there may be more. | ProjectService.md review step 2 |
| `code-checking-label` | Code checking | |
| `code-checking-ask-title` | Turn on code checking for { $project }? | ProjectService.md SA-RECON-11 |
| `code-checking-ask-body` § | Code checking runs this project's own build scripts and add-ons on your computer. Turn it on only if you trust where this project came from. | ProjectService.md SA-RECON-11 |
| `code-checking-yes` | Turn on | |
| `code-checking-no` | Keep off | |
| `code-checking-turn-off` | Turn off | |
| `code-checking-on` | Code checking is on for this project. | |
| `code-checking-off` | Code checking is off for this project. | |
| `code-checking-unanswered` | Code checking hasn't been turned on for this project. | |
| `memory-tab-to-check` | To check | |
| `memory-to-check-line` | { $count } item waiting for your check / { $count } items waiting for your check (Fluent plural) | |
| `memory-held-title` | Written by agents, waiting for you | MemoryService.md SA-TRUST-04 |
| `memory-held-intro` | Ordin doesn't use these until you keep them. Anything you leave here is removed after 30 days. | MemoryService.md SA-TRUST-04, SA-QUAR-01 |
| `memory-held-by-agent` | Written by { $agent } | |
| `memory-held-by-an-agent` | Written by an agent | |
| `memory-held-from-colleague` | From a file a colleague shared | |
| `memory-held-about` | About { $about } | |
| `memory-held-cites` | Says it comes from { $source } | |
| `memory-held-names` | Names | |
| `memory-held-links` | Links | |
| `memory-held-link` | { $from } { $kind } { $to } | |
| `memory-held-link-ended` | { $from } { $kind } { $to } · no longer true | |
| `memory-held-link-sure` | { $percent } sure | |
| `memory-held-needs-check` | May hold personal details. If you keep it, it isn't shared with your team until you check it. | MemoryService.md SA-TRUST-04 (`needs_check`) |
| `memory-held-discard` | Don't keep | |
| `memory-held-kept` | Kept. Ordin will use this from now on. | |
| `memory-held-discarded` | Not kept. Ordin won't use this. | |
| `memory-held-empty` | Nothing written by agents is waiting for you. | |
| `memory-citations-title` | Sources to confirm | MemoryService.md SA-CITE-02 |
| `memory-citations-intro` | An agent named the source of each fact below. Until you confirm it, the fact stays on this computer and isn't shared with your team. | MemoryService.md SA-CITE-02 |
| `memory-cited-by-agent` | { $agent } says this comes from { $source } (version { $version }) | MemoryService.md SA-CITE-01 |
| `memory-cited-by-an-agent` | An agent says this comes from { $source } (version { $version }) | MemoryService.md SA-CITE-01 |
| `memory-citation-confirm` | Confirm source | |
| `memory-citation-confirmed` | Confirmed. This fact can now be shared with your team. | |
| `memory-citations-empty` | No sources are waiting for you to confirm. | |
| `memory-citation-select` | Select “{ $statement }” | SM-SF-29 |
| `memory-citations-select-all` | Select all | SM-SF-29 |
| `memory-citations-select-none` | Select none | SM-SF-29 |
| `memory-citations-confirm-selected` | Confirm selected ({ $count }) | SM-SF-29 |
| `memory-citations-confirmed-count` | { $count } sources confirmed. These facts can now be shared with your team. | SM-SF-29 |
| `confluence-auto-on` | Your own changes here are published to Confluence automatically. | 11 §5.14; WorkspaceService.md SA-WS-AUTO-01 |
| `confluence-auto-off` | Changes here are not published automatically. They wait in Ready to publish, because { $names } can run programs that could change these files. | WorkspaceService.md SA-WS-AUTO-01, SA-WS-AUTO-03 |
| `confluence-auto-off-more` | { $names } and { $count } more | |
| `confluence-auto-an-agent` | an agent | |
| `confluence-auto-a-crew` | a crew of agents | |
| `confluence-auto-how-to-turn-on` | To publish automatically again, remove their access to this folder. | WorkspaceService.md SA-WS-AUTO-02 |
| `ready-kind-changed` | Changed since it was published | ConfluenceAdapter.md SA-CONF-AUTO-03 |
| `search-action-home` | Home | 11 §5.17; HomeAndSearch.md *Actions group* |
| `search-action-new-workspace` | New workspace | 11 §5.17 |
| `search-action-add-documents` | Add documents | 11 §5.17 |
| `search-action-summarise` | Summarise | 11 §5.17 |
| `search-action-clear` | Clear | 11 §5.17 |
| `search-action-earlier-conversations` | Earlier conversations | 11 §5.17 |
| `search-action-settings` | Settings | 11 §5.17 |

#### Part kind words (G-CU-2 follow-up)

The ids are AgentModel.md's kind table; the AgentModel spec gives no English, so the words are *chosen here* from each id's own words and approved with this amendment:

| Id | en-GB text |
|---|---|
| `part-kind-ability` | Ability |
| `part-kind-agent` | Agent |
| `part-kind-code-workflow` | Code workflow |
| `part-kind-group-of-abilities` | Group of abilities |
| `part-kind-hand-over` | Hand-over |
| `part-kind-language` | Language |
| `part-kind-lifecycle` | Lifecycle |
| `part-kind-memory-rules` | Memory rules |
| `part-kind-not-offered` | Not offered |
| `part-kind-reference-material` | Reference material |
| `part-kind-rehearsal-checks` | Rehearsal checks |
| `part-kind-result-must-hold` | Result must hold |
| `part-kind-role` | Role |
| `part-kind-rule` | Rule |
| `part-kind-task` | Task |
| `part-kind-tool` | Tool |
| `part-kind-way-of-working` | Way of working |
| `part-kind-words` | Words |
| `part-kind-working-mode` | Working mode |
| `part-kind-working-practice` | Working practice |
| `part-kind-writing-guide` | Writing guide |
| `part-kind-writing-style` | Writing style |

Existing ids reused: `memory-keep` (Keep), `memory-kind-happened`, `memory-kind-known`, `memory-kind-summary`, `memory-thing-also-called`.

### Internal Logic (additions)

1. **SM-SF-1 The exact version an approval covers** (FU-089; adds to step 7). Wherever an approval card's content holds a `ContentRef` of kind `file_version` — `files[i]` of `files`, `change_set` of `changes`, `page` of `page` — the content view opened by the card's content button resolves it through `readFileVersion(backend, file_version_id)` on the card's backend, never through `files.preview` of the file as it is now, because the file may have moved on since the card was raised (ConfluenceAdapter.md step 6.1).
   1. `ok` → heading `approval-bound-version-heading` with `name` = the last segment of `VersionContent.path`; under it `approval-bound-version-note`; then the `Preview` drawn as the document page draws a `files.preview` result (text as text nodes, an image or PDF from its bytes, a thumbnail, or nothing for `none`). A `text` preview with `truncated: true` ends with `approval-bound-version-cut`.
   2. `not_wired`, or an error reply → `approval-bound-version-unavailable` in place of the preview; an error reply's own sentence is drawn under it by `ErrorLine`. **Approve** is turned off while the version cannot be shown (SM-SF-1.4); **Reject** is not changed.
   3. A `file_version` reference outside an approval card (a table cell, a `documents` block) resolves through the same function and draws the same preview, without `approval-bound-version-note`.
   4. **Nothing is approved unseen** (secscan F4, the user's decision of 2026-01-10). When the card opens, the screen resolves every `file_version` reference of its content as 1 does, without waiting for the content button. While any of them is still being read, the **Approve** button is disabled. If any of them is `not_wired` or an error reply, **Approve** stays disabled and the line `approval-approve-needs-version` is drawn under the buttons. **Reject** (or **Not now**) stays enabled. The card's own text, `verb` and `reject_style` are not changed. This is the only way the screens alter the buttons of an approval card: they never add a button, never reorder them and never press one.
   5. **Where a link goes** (secscan F3, the same decision). In an approval's content view, a link in Markdown text is drawn as its words followed by its target in brackets, both as plain text: `words (target)`. A link is still never an element the person can follow (2C §7). Outside an approval's content view a link is drawn as its words only, as before.
2. **SM-SF-2 New problems of a coding task** (FU-089; adds to the Engineering views of step 9). In *Changes waiting* and in the side-by-side attempts, the problems line of each `ReviewTask` reads, by `task.problems_unchecked` (absent reads as `false`) and `task.new_problems`:

   | `problems_unchecked` | `new_problems` | Line | Icon |
   |---|---|---|---|
   | `false` | 0 | `changes-problems-none` | tick |
   | `false` | ≥ 1 | `changes-problems-count`, then one line per `ReviewTask.new_problems` entry: file, line and message | warning |
   | `true` | 0 | `changes-problems-unchecked` | question mark |
   | `true` | ≥ 1 | `changes-problems-count` and its problem lines, then `changes-problems-unchecked-also` | warning |

   `changes-problems-none` is drawn only on the first row: no other combination, and no other screen, says there are no new problems (ProjectService.md review step 2: "the screens must then not say there are no new problems"). A conversation card or *Needs you* row for the task carries the same line as the table gives.
3. **SM-SF-3 The code-checking question** (FU-089; ProjectService.md SA-RECON-11).
   1. When `projects.editor_open` returns `asks_to_start_servers = true`, the code editor draws a bar above the text — not a dialog, not an approval card, not a notification — with the heading `code-checking-ask-title` (`project` = `Project.display_name`), the line `code-checking-ask-body`, and two buttons **Turn on** (`code-checking-yes`) and **Keep off** (`code-checking-no`). Neither is the default button and neither takes focus when the bar appears. The editor stays usable with highlighting while the bar is shown.
   2. Either button sends `projects.answer_language_servers { project_id, allowed }` (`true` for **Turn on**, `false` for **Keep off**); both buttons are disabled while it is in flight. `ok` → the bar is removed from every open editor of that project and `code-checking-on` or `code-checking-off` is announced in a polite live region. After **Turn on**, each open editor of that project is closed with `projects.editor_close { session }` and opened again with `projects.editor_open { path, project_id }`, so code checking starts through the Project Service's own check and nothing in the screens starts it. A refusal (`PROJ_NOT_FOUND` or any other) is drawn by `ErrorLine` on the bar and the bar stays.
   3. Project settings (`ProjectSettings`) gains a **Code checking** section (`code-checking-label`) above **Check commands**. On opening it sends `projects.language_servers_answer { project_id }`: `true` → `code-checking-on` with **Turn off** (`code-checking-turn-off`); `false` → `code-checking-off` with **Turn on**; `null` → `code-checking-unanswered` with **Turn on**. Each button sends `projects.answer_language_servers` as in 3.2 and the section re-reads the answer after `ok`.
   4. A later `projects.editor_open` that again returns `asks_to_start_servers = true` (the folder now holds a different project, or ordin restarted on a folder whose answer is not kept) draws the bar again; the screens keep no answer of their own.
4. **SM-SF-4 Written by agents, waiting for you** (FU-089; MemoryService.md SA-TRUST-04, SA-TRUST-05; adds to What Ordin remembers of the January 2026 memory amendment). What Ordin remembers for a workspace gains a third tab **To check** (`memory-tab-to-check`) after Facts and Things, holding two sections: this one and SM-SF-5. The tab shows the number of items in both sections beside its label. The Facts tab starts with `memory-to-check-line` when that number is above 0; choosing it opens the **To check** tab.
   1. On opening the tab, and on each `workspace_changed` for the workspace while it is open, the view calls `heldWrites(backend, workspace_id)`. `not_wired` → the section is not drawn at all (as SM-UP-1 treats an absent group); when SM-SF-5 is `not_wired` too, the tab and the Facts line are not drawn.
   2. `ok` → heading `memory-held-title`, the line `memory-held-intro`, then one entry per `HeldWrite` in the order given (newest first), or `memory-held-empty` when there is none. Each entry draws, in order: the `statement`; `memory-held-about` when `about` is present; the kind label `memory-kind-happened`, `memory-kind-known` or `memory-kind-summary` when `fact_class` is `episodic`, `semantic` or `consolidated` (no kind line for any other value); who wrote it — `memory-held-by-agent` with the `instance.display_name` of the `agents.list { workspace_id }` entry whose `instance.id` equals `written_by`, `memory-held-by-an-agent` when no entry matches, `memory-held-from-colleague` when `written_by` is absent; `memory-held-cites` with `cited` when present; under `memory-held-names`, each `HeldThing` as its `name` followed by `memory-thing-also-called` with `other_names` when that list is not empty; under `memory-held-links`, each `HeldLink` as `memory-held-link` (or `memory-held-link-ended` when `ended`), followed by `memory-held-link-sure` with `confidence_permille / 1000` formatted as a percentage by `lib/format/intl.ts` when present; `memory-held-needs-check` when `needs_check`; the date `held_at` in the person's locale; then **Keep** (`memory-keep`) and **Don't keep** (`memory-held-discard`). Nothing a keep applies is folded away or left out (SA-TRUST-04: everything a keep applies is shown).
   3. **Keep** calls `keepHeld(backend, workspace_id, id, content_hash)` with the `content_hash` of the entry as drawn — the screens never compute, change or cache a hash apart from the entry they drew. When a re-read replaces an entry, the drawn entry and its hash are replaced together. `ok` → the entry is removed and `memory-held-kept` is announced. `MEM_HELD_CHANGED` → its owner's sentence ("This fact changed after you read it. Read it again before keeping it.") is drawn by `ErrorLine` on the entry and the list is read again; nothing is kept. `MEM_NOT_HELD` → its owner's sentence and the list is read again.
   4. **Don't keep** calls `discardHeld(backend, workspace_id, id)` with no confirmation dialog, because nothing has been stored for it (SA-UI-SF-03). `ok` → the entry is removed and `memory-held-discarded` is announced; `MEM_NOT_HELD` as in 4.3.
5. **SM-SF-5 Sources to confirm** (FU-089; MemoryService.md SA-CITE-01, SA-CITE-02). The second section of **To check**.
   1. The view calls `citationsToConfirm(backend, workspace_id)` with SM-SF-4.1's timing. `not_wired` → the section is not drawn. `ok` → the ids are read with `memory.get { ids, workspace_id }` in chunks of 100, in the order given, and the section draws heading `memory-citations-title`, the line `memory-citations-intro`, then one entry per returned `FactView`, or `memory-citations-empty` when the list is empty. An id `memory.get` does not return (forgotten meanwhile) is not drawn.
   2. Each entry draws the fact as the Facts tab draws it, its source line as SM-SF-6 states, and **Confirm source** (`memory-citation-confirm`). Pressing it calls `confirmCitation(backend, workspace_id, id)`; `ok` → the entry is removed and `memory-citation-confirmed` is announced; `MEM_NOT_HELD` → its owner's sentence ("This fact doesn't need checking.") and the section is read again.
   3. The existing `team-held-for-check` route (Team knowledge amendment, step 4) opens **To check**, because `held_for_check` counts both the facts waiting for a personal-data check and the citations waiting here (SA-CITE-02). Its **Keep it** and **Forget** entries for personal-data checks stay on the Facts tab filter as that amendment states.
6. **SM-SF-6 An agent's citation reads as the agent's** (MemoryService.md SA-CITE-01). Wherever a `FactView` is drawn (What Ordin remembers, What Ordin used, the memory block), a fact whose `came_from.kind` is `agent_citing` shows its source line as `memory-cited-by-agent` with the agent's `instance.display_name` (looked up as in SM-SF-4.2), `origin_ref` as `source` and `version`, or `memory-cited-by-an-agent` when no agent matches — never as a plain document source. `origin_ref` is drawn as text and is not a link. **Confirm source** is offered only in SM-SF-5, for the ids that section lists.
7. **SM-SF-7 A linked folder's automatic publishing** (FU-127; WorkspaceService.md SA-WS-AUTO-01 to 03; closes the words half of G-AUTO-1).
   1. For each `ConfluenceLink` of a workspace in state `proposed`, `active` or `paused`, the view that shows the linked folder (the folder's screen in Documents, and the link's line under the folder in the tree's **⋯** details) calls `confluencePublishing(backend, link.id)` when it opens and again on each `workspace_changed` for that workspace (SA-WS-AUTO-02). `not_wired` → no publishing line is drawn.
   2. `automatic = 'on'` → `confluence-auto-on`.
   3. `automatic = 'off'` → `confluence-auto-off` with `names` built from `turned_off_by` in the order given: an `agent_instance` grantee as the `instance.display_name` of the matching `agents.list` entry (no workspace filter), or `confluence-auto-an-agent` when none matches; a `crew` grantee as `confluence-auto-a-crew`. Repeated words are drawn once. Up to 3 names are joined by `Intl.ListFormat` of the active locale (`lib/format/intl.ts`); with more, the first 3 and `confluence-auto-off-more` with the remaining count. Under it, `confluence-auto-how-to-turn-on`. The line has an information icon and a label; it is never carried by colour alone.
   4. A refusal (`WS_LINK_NOT_FOUND`, `WS_FILES_FAILED`, `WS_DATASTORE_UNAVAILABLE`) draws no publishing line; `ErrorLine` draws the owner's sentence in its place.
   5. The line states a fact and offers no switch: automatic publishing follows the folder's access and nothing else (WorkspaceService.md *Approach*, "Rejected: a setting the person turns on and off").
8. **SM-SF-8 Ready to publish, changed items** (FU-127; ConfluenceAdapter.md SA-CONF-AUTO-03, SA-CONF-AUTO-05). In the *Ready to publish* checklist (the `result` block of kind `checklist`, and the list read through `confluence.ready_to_publish`), a `ReadyItem` of kind `changed` is drawn as the other kinds are — its file name from `path`, a tick, the item's line — with the line `ready-kind-changed`. It is ticked and sent through **Publish checked items…** as the other kinds are; the card's `changes` line comes from the Confluence Adapter unchanged. A kind the screens do not know is drawn with its file name and no kind line, and stays tickable, so a newer backend's item is never hidden from the person.
9. **SM-SF-9 The Actions group of Search or go to…** (FU-127; HomeAndSearch.md *Gaps*, 2026-01-10; the user's decision "Home search actions get their own result type"). The screens settle the seven screen names; they are the ones the built service already answers (`crates/ordin-home-search/src/service.rs`, `ACTIONS`), so no rename follows:

   | Screen name | Label drawn | What it opens |
   |---|---|---|
   | `home` | `search-action-home` | Home, as the rail's **Home** |
   | `new_workspace` | `search-action-new-workspace` | the rail's **New workspace** (`workspaces.create`) |
   | `add_documents` | `search-action-add-documents` | *Add to this workspace* for the workspace in front |
   | `summarise` | `search-action-summarise` | the conversation panel's **Summarise** for the workspace in front |
   | `clear` | `search-action-clear` | the conversation panel's **Clear** for the workspace in front, which asks first (step 9) |
   | `earlier_conversations` | `search-action-earlier-conversations` | *Earlier conversations* of the workspace in front |
   | `settings` | `search-action-settings` | Settings |

   1. The screens read an Actions entry through one function, `actionScreen(entry): SearchActionName | null` in `lib/search/actions.ts` (*chosen here*), where `SearchActionName` is the union of the seven names above. Today an entry is a `GoToResult` and the name is `opens.name` when `opens.kind = 'screen'`; when HomeAndSearch.md gives the group its own result type, only this function changes.
   2. The label is drawn from the screens' catalogue by name, never from the entry's `label`, so it follows the person's language.
   3. `null` (another `opens` kind, or a name not in the table) → the entry is not drawn and diagnostic `UI_SEARCH_ACTION_UNKNOWN` is recorded (step 12).
   4. The four entries that act on the workspace in front (`add_documents`, `summarise`, `clear`, `earlier_conversations`) are not drawn when no workspace is in front.
   5. Every entry passes SM-UP-1: an entry whose action sends a command of a group the backend does not answer is not drawn.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| An Actions entry of *Search or go to…* names no screen of SM-SF-9 | not drawn | `UI_SEARCH_ACTION_UNKNOWN` *(chosen here)* | nothing shown; diagnostic only |
| **Keep** of a held write whose content changed after it was drawn | `Err(permanent)` from the core | `MEM_HELD_CHANGED` | the owner's sentence on the entry; the list is read again (SM-SF-4.3) |
| **Keep**, **Don't keep** or **Confirm source** of an item no longer waiting | `Err(permanent)` from the core | `MEM_NOT_HELD` | the owner's sentence; the section is read again |
| The publishing state of a linked folder cannot be read | `Err` from the core | `WS_LINK_NOT_FOUND`, `WS_FILES_FAILED`, `WS_DATASTORE_UNAVAILABLE` | no publishing line; the owner's sentence (SM-SF-7.4) |
| The exact version an approval covers cannot be read, or its command is not wired | `Err` from the core, or `not_wired` | the core's code, or none | `approval-bound-version-unavailable`; **Approve** disabled with `approval-approve-needs-version`, **Reject** unchanged (SM-SF-1.2, SM-SF-1.4) |

The `PROJ_*` refusals of `projects.answer_language_servers` and `projects.language_servers_answer` are drawn by `ErrorLine` as their owner's sentences; no other `UI_` code is added.

### Tests (vitest) — added

- Content view: a card whose `files` holds `{ kind: 'file_version', file_version_id }` calls `readFileVersion` and never `files.preview`; `ok` with a text preview draws the text, `approval-bound-version-heading` with the last path segment and `approval-bound-version-note`; `truncated: true` adds `approval-bound-version-cut`; `not_wired` and an error reply each draw `approval-bound-version-unavailable` and leave both card buttons enabled; a preview text holding `<img src=x onerror=alert(1)>` is drawn literally.
- Problems line: table-driven over the four rows of SM-SF-2, plus `problems_unchecked` absent read as `false`; the text `No new problems` appears only for (`false`, 0).
- Code-checking bar: `asks_to_start_servers = true` draws the bar and `false` does not; neither button has focus after drawing; **Turn on** sends `projects.answer_language_servers` with `allowed: true`, then `projects.editor_close` and `projects.editor_open` for each open editor of that project; **Keep off** sends `allowed: false` and reopens nothing; a refusal keeps the bar. Project settings: one test per answer `true`, `false`, `null`.
- Held writes: an entry with every field present draws the statement, about, kind, agent name, cited source, each thing with its other names, each link with its percentage, the personal-details line and the date; `written_by` absent draws `memory-held-from-colleague`; an unknown `fact_class` draws no kind line; **Keep** sends the drawn entry's `content_hash` exactly; after a re-read that changes an entry's hash, **Keep** sends the new hash; `MEM_HELD_CHANGED` draws the sentence and re-reads the list; **Don't keep** opens no dialog.
- Sources to confirm: ids are read with `memory.get` in chunks of 100; an id not returned is not drawn; **Confirm source** calls `confirmCitation` with the fact id; `MEM_NOT_HELD` re-reads the section.
- Citation line: a `FactView` with `came_from.kind = 'agent_citing'` draws `memory-cited-by-agent` in What Ordin remembers, What Ordin used and the memory block; `origin_ref` is not an element with an `href`.
- To check tab: both sections `not_wired` → no tab and no Facts line; one section wired → the tab with that section only.
- Publishing line: `on` → `confluence-auto-on`; `off` with one agent, with a crew, with an agent not in `agents.list`, and with five grantees (three names and "and 2 more"); `not_wired` → no line; `WS_LINK_NOT_FOUND` → the sentence and no line; a `workspace_changed` re-reads it.
- Ready to publish: an item of kind `changed` draws `ready-kind-changed` and is tickable; an item of an unknown kind is drawn with its file name, no kind line, and is tickable.
- Actions group: table-driven over the seven names (label from the catalogue, the entry's `label` ignored); an `opens.kind` other than `screen` and an unknown name are not drawn and record `UI_SEARCH_ACTION_UNKNOWN`; the four workspace actions are absent with no workspace in front.
- Catalogue: every id of this amendment exists in the en-GB `.ftl` files; the four `approval-bound-version-*` ids and `code-checking-ask-body` carry the `safety` comment; no text of this amendment contains "branch", "commit", "push", "pull", "merge", "repository", "hash" or "language server".

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Screens for Phase 7A decisions and automatic publishing (FU-089, FU-127)

  Scenario: Happy path — an approval shows the exact version it covers
    Given an approval card "Publish 1 checked items to Confluence?" whose files content holds a file version of "Quote — Kestrel.md"
    When the person opens See the files
    Then the heading reads "Quote — Kestrel.md — the version this approval covers"
    And the text of that version is shown, not the file as it is now
    And "Changes made to this file after the card was raised are not included." is shown

  Scenario: Error path — the version cannot be shown
    Given readFileVersion returns not_wired
    When the person opens See the files
    Then "ordin can't show the exact version this approval covers. Reject the card if you need to see it first." is shown
    And the card's Publish and Not now buttons are unchanged

  Scenario: Security — an unchecked task never reads as clean
    Given projects.review returns a task with new_problems 0 and problems_unchecked true
    When the person opens Changes waiting
    Then the task reads "Not checked for new problems: some changed files had no code checking."
    And "No new problems" is not shown anywhere on the screen

  Scenario: Happy path — a checked task with no problems
    Given projects.review returns a task with new_problems 0 and problems_unchecked false
    Then the task reads "No new problems"

  Scenario: Edge case — problems found and some files unchecked
    Given a task with new_problems 2 and problems_unchecked true
    Then the task reads "2 new problems" with both problems listed
    And "Some changed files had no code checking, so there may be more." is shown

  Scenario: Happy path — the person turns on code checking
    Given projects.editor_open returns asks_to_start_servers true for project "Billing"
    Then the editor shows "Turn on code checking for Billing?" with Turn on and Keep off
    And no button has focus
    When the person presses Turn on
    Then projects.answer_language_servers is sent with allowed true
    And each open editor of Billing is closed and opened again
    And "Code checking is on for this project." is announced

  Scenario: Edge case — the person keeps code checking off
    When the person presses Keep off on the code-checking bar
    Then projects.answer_language_servers is sent with allowed false
    And the bar is removed and no editor is reopened

  Scenario: Happy path — project settings show the answer
    Given projects.language_servers_answer returns null
    When the person opens the project's settings
    Then Code checking reads "Code checking hasn't been turned on for this project." with Turn on

  Scenario: Happy path — the person keeps a held write
    Given heldWrites returns one write "Pay this supplier first." by agent "Quote checker" naming "Kestrel", also called "Kestrel Ltd", with a link "Kestrel supplies Northwind" at 850
    When the person opens What Ordin remembers › To check
    Then the entry shows the statement, "Written by Quote checker", "Kestrel", "Also called Kestrel Ltd" and "Kestrel supplies Northwind" with "85% sure"
    When the person presses Keep
    Then keepHeld is sent with that write's content hash
    And "Kept. Ordin will use this from now on." is announced

  Scenario: Security — a held write changed after the person read it
    Given the person is reading a held write from a colleague's file
    When the person presses Keep and the reply is MEM_HELD_CHANGED
    Then "This fact changed after you read it. Read it again before keeping it." is shown
    And the list is read again and nothing is kept

  Scenario: Happy path — the person discards a held write
    When the person presses Don't keep on a held write
    Then discardHeld is sent without a confirmation
    And "Not kept. Ordin won't use this." is announced

  Scenario: Happy path — the person confirms an agent's source
    Given citationsToConfirm returns one fact that agent "Quote checker" cited from "quote.pdf" at version "1"
    When the person opens What Ordin remembers › To check
    Then the entry reads "Quote checker says this comes from quote.pdf (version 1)"
    When the person presses Confirm source
    Then confirmCitation is sent with that fact's id
    And "Confirmed. This fact can now be shared with your team." is announced

  Scenario: Security — an agent's citation is never drawn as a plain source
    Given memory.used returns a fact whose came_from is agent_citing by "Quote checker" from "quote.pdf"
    When the person opens What Ordin used
    Then its source line reads "Quote checker says this comes from quote.pdf (version 1)"
    And "quote.pdf" is not a link

  Scenario: Edge case — nothing to check is wired yet
    Given heldWrites and citationsToConfirm both return not_wired
    When the person opens What Ordin remembers
    Then no To check tab is drawn and the Facts tab has no waiting line

  Scenario: Happy path — a linked folder publishes automatically
    Given confluencePublishing returns automatic on for the link of "Supplier quotes"
    When the person opens "Supplier quotes"
    Then "Your own changes here are published to Confluence automatically." is shown

  Scenario: Error path — an agent that can run programs turns automatic publishing off
    Given confluencePublishing returns automatic off, turned off by agent "Build helper"
    When the person opens "Supplier quotes"
    Then "Changes here are not published automatically. They wait in Ready to publish, because Build helper can run programs that could change these files." is shown
    And "To publish automatically again, remove their access to this folder." is shown
    And no switch to turn automatic publishing on is offered

  Scenario: Edge case — many agents are named
    Given confluencePublishing returns automatic off, turned off by five agents
    Then the line names the first three and "and 2 more"

  Scenario: Edge case — the publishing state is re-read when access changes
    Given the folder shows automatic publishing off
    When a workspace_changed event arrives for the workspace
    And confluencePublishing returns automatic on
    Then "Your own changes here are published to Confluence automatically." is shown

  Scenario: Happy path — a changed file waits in Ready to publish
    Given confluence.ready_to_publish returns an item of kind changed for "Quote — Acme.pdf"
    When the person opens Ready to publish
    Then the item reads "Changed since it was published" and can be ticked

  Scenario: Happy path — an action opens its screen
    Given search.query returns an Actions entry opening the screen "earlier_conversations"
    And workspace "Q4 forecast" is in front
    When the person chooses it
    Then Earlier conversations of "Q4 forecast" opens
    And the entry's label was drawn from the person's language catalogue

  Scenario: Error path — an action the screens do not know
    Given search.query returns an Actions entry opening the screen "reports"
    Then no Actions entry is drawn for it
    And UI_SEARCH_ACTION_UNKNOWN is recorded in this window's diagnostics
```

### Accessibility (additions)

The code-checking bar is a region with the heading `code-checking-ask-title` (level 3), placed before the editor in reading order; its buttons are reachable by Tab and none takes focus when the bar appears; the answer is announced in a polite live region. **To check** is a third tab of What Ordin remembers' WAI-ARIA tab list, its count part of the tab's accessible name; each held write and each source to confirm is a list item whose accessible name is its statement, with **Keep**, **Don't keep** and **Confirm source** as buttons in reading order; outcomes are announced in a polite live region and focus moves to the next entry, or to the section heading when the list is empty. The problems line carries its icon with a text label (never colour alone). The publishing line is plain text with an icon labelled by its sentence. The content view's heading `approval-bound-version-heading` is level 2 and receives focus when the view opens, as other content views do (step 7).

### Assumptions

- **SA-UI-SF-01** The screens reach the five memory methods, `read_version` and `confluence_publishing` through one wiring file that returns `not_wired` until the Service API catalogue names a command for each, because a command name is the catalogue's to give (identifier rule) and the screens can then be built and tested now against the owners' shapes; the views draw nothing, or the stated fallback, while `not_wired`.
- **SA-UI-SF-02** A file version is shown through its own read and never through `files.preview` of the current file, because the approval binds the version's bytes and the current file may differ (ConfluenceAdapter.md step 6.1).
- **SA-UI-SF-03** **Don't keep** asks no confirmation: a held write is not yet remembered, so discarding it removes nothing Ordin uses, and the owner journals the discard (`memory.held_discarded`).
- **SA-UI-SF-04** Held writes and sources to confirm do not enter *Needs you* and are not counted there: they block no running work and expire after 30 days (MemoryService.md SA-QUAR-01). They are found through **To check**, the Facts tab's line and the existing `team-held-for-check` line.
- **SA-UI-SF-05** An agent's name is the `instance.display_name` of the `agents.list` entry whose `instance.id` equals the agent instance id the owner gives; a crew is named "a crew of agents" because no command returns a crew charter's name by its id.
- **SA-UI-SF-06** After a yes to code checking, the screens close and reopen each open editor of that project instead of starting anything themselves, so every start passes the Project Service's single check (SA-RECON-11).
- **SA-UI-SF-07** The person's text says "code checking" for what the Project Service calls a language server, and "programs" for shell commands, following 11 §3's rule that the person never sees tool or technical names outside Diagnostics.
- **SA-UI-SF-08** The Actions group's labels come from the screens' catalogue keyed by screen name, so the result type HomeAndSearch.md gives the group needs to carry only the screen name; the seven names are those the built service already answers.

### Gaps

Each gap below blocks only the part named; the rest of this amendment can be built and tested now against `lib/backend/wiring.ts`.

- **G-SF-1** *Reading the exact version an approval covers.* No Service API command reads one file version by its id; the owner method is `FileService::read_version(version_id) -> FileBytes`. Until the catalogue (`components/ServiceApi.md`) names one and Phase 11 wires it, `readFileVersion` returns `not_wired` and the card shows `approval-bound-version-unavailable`. This blocks FU-089's exit criterion "an approval card whose content is `ContentRef::FileVersion` shows those bytes". Recommended: a read command of the `files.` group taking `{ file_version_id }` and returning the version's path and a `Preview`, the shape `files.preview` already returns.
- **G-SF-2** *Held writes and sources to confirm.* No Service API command, and no generated type, carries `held_writes`, `keep_held`, `discard_held`, `citations_to_confirm` or `confirm_citation` (MemoryService.md states them as "Rust methods for the person's surface"). Until the catalogue names them and Phase 11 wires them, SM-SF-4 and SM-SF-5 are not drawn. This blocks FU-089's exit criteria "held memory writes are listed with their names and links and kept by their `content_hash`" and "agent citations can be confirmed". Recommended: five commands of the `memory.` group with the owner's inputs, `keep_held` taking `content_hash`, and `HeldWrite`, `HeldThing` and `HeldLink` restated by the Service API field for field.
- **G-SF-3** *The command carrying `ConfluencePublishing`* (the command half of WorkspaceService.md G-AUTO-1; the words half is settled by SM-SF-7). No Service API command carries `ConfluencePublishing`; it is follow-up FU-127 (command side). Until it is added `confluencePublishing` returns `not_wired` and no publishing line is drawn. This blocks the drawing of SM-SF-7 only.

## Amendment January 2026 — Catch-up of words and screen identifiers, the screens Phase 10 left out, and when an approval card opens (FU-135, FU-136, FU-142)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations").
>
> Source: the follow-ups FU-136 (every message id and screen identifier the Phase 10 screens chose in code, and every `SPEC-GAP-<N>` marker in `apps/desktop/ui/src`), FU-135 (the screens the spec names that Phase 10 did not build) and FU-142 (the approval card always opens as a new tab). Evidence read for this amendment: the four en-GB files `crates/ordin-localisation/catalogues/en-GB/screens.ftl`, `screens-blocks.ftl`, `screens-settings.ftl` and `screens-fixed.ftl`; every `register.ts` under `apps/desktop/ui/src/lib/views/`; `apps/desktop/ui/src/lib/shell/screens.ts`; `apps/desktop/ui/src/lib/views/Settings/sections.ts`; `apps/desktop/ui/src/lib/app/bootstrap.ts`; the generated `apps/desktop/ui/src/lib/service-api/commands.gen.ts` and `types.gen.ts`; HLD `docs/design/ordin-desktop/11-ux-guidelines.md` §4, §5.6, §5.17 to §5.22, §5.25 to §5.30 and §7. Every command named below was seen in `commands.gen.ts`, and every field in `types.gen.ts`; what was not seen is in *Gaps*. Every rule below adds to the sections above; nothing above changes except where a numbered assertion of this amendment says it replaces a sentence.

The rules of the earlier amendments hold for every line below: the sentence, never the code; nothing changes without the person's choice; text an agent, a colleague or an outside system wrote is drawn through Svelte text interpolation only; no Git word ("branch", "commit", "push", "pull request", "repository" as a technical noun, "hash") on a screen a business user sees, outside the engineering views and the views for people who code that 11 §5.11, §5.26 and §5.29 name; SM-UP-1 applies to every entry.

### Shared Context (additions)

1. **SM-CU-1 Block kinds.** The generated `Block` union carries 22 kinds, the 22 rows of step 6's table; `slack_list` (added by the January 2026 Slack amendment, ADR-102) is the 22nd. The module structure line "`lib/blocks/kinds/<Kind>.svelte` one per block kind (21)" reads "(22)". The test "one per block kind using its 2C JSON example" covers all 22.

2. **SM-CU-2 Screen identifiers** (FU-136; *chosen in code, Phase 10; stated here*). A screen key is `<id>` or `<id>:<instance>`, split at the first `:` (`parseKey` in `lib/shell/screens.ts`); the key is what `ordin.view.tabs` stores (Data Model). The identifiers, their instance and their registered tab title:

   | Identifier | Instance | Tab title id | Registered in |
   |---|---|---|---|
   | `home` | — | `rail-home` | `lib/views/screens-home/register.ts` |
   | `settings` | — | `screen-settings` | `lib/views/Settings/register.ts` |
   | `diagnostics` | — | `screen-diagnostics` | `lib/views/Settings/register.ts` |
   | `language` | `<locale>`, `<backend id>@<locale>` for a server, either followed by `?only=needs_review` | `settings-section-language` | `lib/views/Settings/register.ts` |
   | `server-languages` | `<backend id>` | `settings-section-language` | `lib/views/Settings/register.ts` |
   | `earlier-conversations` | — | `conv-earlier-name` | `lib/views/screens-home/register.ts` |
   | `earlier-conversation` | `<conversation id>` | `conv-earlier-name` | `lib/views/screens-home/register.ts` |
   | `documents` | — | `tree-documents` | `lib/views/fixed/register.ts` |
   | `agents` | — | `tree-agents` | `lib/views/fixed/register.ts` |
   | `workflows` | — | `tree-workflows` | `lib/views/fixed/register.ts` |
   | `dashboards` | — | `tree-dashboards` | `lib/views/fixed/register.ts` |
   | `telegram` | — | `tree-telegram` | `lib/views/fixed/register.ts` |
   | `slack` | — | `tree-slack` | `lib/views/fixed/register.ts` |
   | `slack-channel` | `<Slack link id>` | `tree-slack` | `lib/views/fixed/register.ts` |
   | `knowledge` | none, or `<page id>` | `tree-knowledge` | `lib/views/fixed/register.ts` |
   | `memory` | — | `tree-memory` | `lib/views/fixed/register.ts` |
   | `approval` | `<approval id>` | `screen-approval` | `lib/views/approval/register.ts` |
   | `block` | `<block id>` | `screen-unknown` (the opener passes the block's title) | `lib/views/blocks/register.ts` |
   | `code-editor` | `<project id>:<path>` (a project id holds no `:`) | `engineering-editor-title` | `lib/views/engineering/register.ts` |
   | `changes-waiting` | `<project id>` | `engineering-changes-title` | `lib/views/engineering/register.ts` |
   | `terminal` | `<project id>` | `engineering-terminal-title` | `lib/views/engineering/register.ts` |
   | `project-settings` | `<project id>` | `engineering-settings-title` | `lib/views/engineering/register.ts` |
   | `email` | `<email link id>` | `tree-email` | reserved in `SCREEN_IDS`; drawn by SM-NS-12 |
   | `running` | — | `tree-running` | reserved in `SCREEN_IDS` |
   | `document` | `<place id>` | `screen-document` | reserved in `SCREEN_IDS`; drawn by SM-NS-4 |
   | `run` | `<run id>` | `screen-run` | SM-RS-1 (amendment *The run screen*) |
   | `task-form` | `<agent instance id>:<command id>` | `screen-task-form` | SM-TF-1 (amendment *A task's form opens from the tree*) |
   | `crew` | `<crew run id>` | `screen-crew` | reserved in `SCREEN_IDS` |
   | `handover` | `<handover id>` | `screen-handover` | opened by Home today; drawn by SM-NS-8 |

   The screens of FU-135 take these identifiers (*chosen here*): `workflow` (`<workflow id>`, SM-NS-5), `agent-draft` (`<agent draft id>`, SM-NS-6), `repository-parts` (`<agent repository id>`, SM-NS-7), `part-draft` (`<part draft id>`, SM-NS-7), `withdraw-part` (`<part id>`, SM-NS-7), `lifecycle` (`<workspace id>`, SM-NS-9), `import` (no instance, SM-NS-10), `installed-files` (`<agent instance id>`, SM-NS-11), `email-message` (`<email link id>:<message ref>`, SM-NS-12), `email-draft` (`<draft id>`, SM-NS-12), `part-page` (`<agent instance id>:<part id>` for an installed agent's part, `library:<part id>` for a library part, SM-NS-15).

   Three further registrations are not screens: the overlay `search` (*Search or go to…*, `lib/views/screens-home/register.ts`), and the *Needs you* slots `whats-new` and `update-line` (`lib/views/Settings/register.ts`, What's new above the update line, SM-UP-4).

   **Correction (FU-134 item, stated here).** `document:<place id>` carries no path, so it cannot name one document. The document screen's instance is `<place id>:<path>` (a place id holds no `:`), and `screenKeyFor` of an `OpenTarget` of kind `document` returns that key.

3. **SM-CU-3 Settings section identifiers** (FU-136; *chosen in code, Phase 10; stated here*; `lib/views/Settings/sections.ts`). Each section has a kebab identifier; its heading is `settings-section-<identifier>`; the wire value is the `SettingsSection` value it reads with `settings.get` where it reads one. Order is 11 §4's:

   | Identifier | `SettingsSection` | Shown when the backend answers (SM-UP-1) |
   |---|---|---|
   | `sign-ins` | `sign_ins` | `identity.list` |
   | `agent-repositories` | `agent_repositories` | `catalogue.list_repositories` |
   | `email` | `email` | `mail.settings` |
   | `telegram` | `telegram` | `telegram.status` |
   | `slack` | — | `slack.status` |
   | `jira` | — | `jira.connect_token` |
   | `servers` | `servers` | `servers.list` |
   | `dictation` | `dictation` | `dictation.get_availability` |
   | `how-ordin-talks` | `how_ordin_talks` | `voice.get` (SM-NS-1.5; the code's `settings.get` is replaced) |
   | `language` | — | `localisation.get_settings`, `localisation.list_languages` |
   | `using-your-plan` | `using_your_plan` | `usage.get_settings` |
   | `notifications` | `notifications` | `settings.get`, `settings.change` |
   | `history` | `history` | `history.get`, `history.set_days` |
   | `display` | `display` | `settings.get`, `settings.change` |
   | `code` | `code` | `projects.ides`, `projects.language_plugins` (SM-NS-1.8; the code's `projects.set_check_commands` is replaced) |
   | `what-ordin-remembers` | `what_ordin_remembers` | `settings.get`, `settings.change` (SM-NS-1.9; amended January 2026; user decision; Approved by the user January 2026: settings only, `memory.list` is no longer sent) |
   | `knowledge-agents` | — | `knowledge_agents.list` |
   | `diagnostics` | `diagnostics` | `diagnostics.lines` |
   | `about` | `about` | `setup.check_update`; local backend only |

4. **SM-CU-4 Message ids chosen in code** (FU-136). Each id below is defined in the named en-GB file and drawn by the Phase 10 screens; until now the spec did not list it. The text is the file's, copied without change; `{ $coordinator }` and `{ $product }` are filled from the brand (ADR-041). The ids the spec already wrote in shortened form (`progress-step-now`, `-next`, `-skipped`; `language-source-edited`, `-drafted`, `-imported`, `-english`; `language-band-medium`, `-heavy`) are not repeated. `approval-affects-version` is safety wording by its `approval-` prefix (ADR-109), marked §. `check-ask-ordin-to-fix` and `conv-ask-about` carry the literal word "Ordin" where the other ids of their files use `{ $coordinator }`; see *Gaps* G-CU-9.

Ids defined in `crates/ordin-localisation/catalogues/en-GB/screens.ftl`:

| Id | en-GB text |
|---|---|
| `rail-new-workspace-create` | Create |
| `rail-workspaces` | Workspaces |
| `rail-resize` | Resize the side panel |
| `tree-documents` | Documents |
| `tree-folder-empty` | Nothing in here yet |
| `tree-agents` | Agents |
| `tree-workflows` | Workflows |
| `tree-dashboards` | Dashboards |
| `tree-email` | Email |
| `tree-telegram` | Telegram |
| `tree-slack` | Slack |
| `tree-running` | Running now |
| `tree-knowledge` | Knowledge |
| `tree-memory` | Memory (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25) |
| `screen-settings` | Settings |
| `screen-diagnostics` | Diagnostics |
| `screen-approval` | Approval |
| `screen-document` | Document |
| `screen-run` | Progress |
| `screen-crew` | Crew progress |
| `screen-unknown` | Page |
| `conv-resize` | Resize the conversation |
| `conv-send` | Send |
| `error-needs-newer-version` | This needs a newer version of ordin. |

Ids defined in `crates/ordin-localisation/catalogues/en-GB/screens-blocks.ftl`:

| Id | en-GB text |
|---|---|
| `approval-affects-version` § | version { $version } |
| `telegram-message-instruction-note` | This message asks { $product } to do something. It's shown as information only. |
| `block-ask-about-this` | Ask { $coordinator } about this |
| `block-ask-about-these` | Ask { $coordinator } about these |
| `block-ask-about-agent` | Ask { $coordinator } about this agent |
| `block-ask-why` | Ask { $coordinator } why |
| `block-ask-more-changes` | Ask for more changes |
| `block-review` | Review |
| `block-show-more` | Show more |
| `block-close-preview` | Hide |
| `block-not-now` | Not now |
| `block-ask-text-this` | Tell me more about “{ $title }”. |
| `block-ask-text-these` | Tell me more about these documents. |
| `block-ask-text-agent` | Tell me more about the agent “{ $agent }”. |
| `block-ask-text-figures` | Tell me more about these figures. |
| `block-ask-text-more-changes` | I'd like more changes to this. |
| `block-ask-text-progress` | How is this going? |
| `block-ask-text-why` | Why can't “{ $title }” be shown? |
| `block-reply-text` | Draft a reply to { $sender }'s message in #{ $channel }. |
| `block-reply-email-text` | Draft a reply to { $sender }'s email. |
| `block-reply-reference` | Message reference: { $id } |
| `block-title-preview` | A document |
| `block-title-document` | A document |
| `block-title-changes` | Changes |
| `block-title-key-figures` | Key figures |
| `block-title-offer` | Something to decide |
| `block-title-crew-plan` | A crew of agents |
| `block-file-unnamed` | A file |
| `block-attachments` | Attachments |
| `block-file-saved` | Saved. |
| `block-save-file` | Save file |
| `block-save-attachment` | Save attachment |
| `block-no-messages` | No messages yet. |
| `block-answer-other` | Something else |
| `block-answer-send` | Answer |
| `block-filled-by-ordin` | Filled in by { $coordinator } |
| `block-table-search` | Search this table |
| `block-table-no-rows` | No rows match. |
| `block-figure-up` | Up { $percent } on { $against } |
| `block-figure-down` | Down { $percent } on { $against } |
| `block-figure-same` | Unchanged on { $against } |
| `block-figure-was` | { $against }: { $value } |
| `block-source-documents` | { $documents } |
| `block-source-document` | { $name } (version { $version }) |
| `block-source-company-system` | { $system }, read { $date } at { $time } |
| `block-source-earlier-result` | An earlier result |
| `block-source-worked-out` | Worked out by { $agent } from { $files } |
| `block-changes-before` | Before |
| `block-changes-after` | After |
| `block-changes-removed` | removed |
| `block-changes-added` | added |
| `block-reply-with-ordin` | Reply with { $coordinator } |
| `block-open-in-slack` | Open in Slack |
| `block-thread-replies` | 1 reply / { $count } replies (Fluent plural on `count`) |
| `block-email-from` | From |
| `block-email-to` | To |
| `block-email-send` | Send… |
| `block-email-edit` | Edit |
| `block-email-save-to-drafts` | Save to mail drafts |
| `block-email-saved-to-drafts` | Saved to your mail drafts. |
| `block-email-discard` | Discard |
| `block-email-discard-title` | Discard this draft? |
| `block-email-drafted-by` | Drafted by { $agent } |
| `block-email-drafted-by-an-agent` | Drafted by an agent |
| `block-offer-save-question` | Save this as a workflow so you can run it again? |
| `block-offer-update-question` | Update the workflow with what changed this time? |
| `block-offer-save` | Save as workflow… |
| `block-offer-update` | Update the workflow… |
| `block-publish-checked` | Publish checked items… |
| `block-agent-search` | Search the agents |
| `block-agent-none` | No agents match. |
| `block-estimate-light` | Light use |
| `block-estimate-medium` | Medium use |
| `block-estimate-heavy` | Heavy use |
| `block-estimate-turns` | about { $min } to { $max } turns |
| `block-estimate-day` | about { $percent } of a typical day's use |
| `block-estimate-ask-first` | { $coordinator } asks before starting work of this size. |
| `block-open-knowledge` | Open Knowledge |

Ids defined in `crates/ordin-localisation/catalogues/en-GB/screens-settings.ftl`:

| Id | en-GB text |
|---|---|
| `settings-section-sign-ins` | Sign-ins |
| `settings-section-agent-repositories` | Agent repositories |
| `settings-section-email` | Email |
| `settings-section-telegram` | Telegram |
| `settings-section-slack` | Slack |
| `settings-section-servers` | Servers |
| `settings-section-dictation` | Dictation |
| `settings-section-how-ordin-talks` | How { $coordinator } talks to you |
| `settings-section-using-your-plan` | Using your plan |
| `settings-section-notifications` | Notifications |
| `settings-section-history` | History |
| `settings-section-display` | Display |
| `settings-section-code` | Code |
| `settings-section-what-ordin-remembers` | What { $coordinator } remembers |
| `settings-section-diagnostics` | Diagnostics |
| `knowledge-agents-address-not-https` | Use an address that starts with https://. |
| `knowledge-agents-keep-it` | Keep it |
| `language-save` | Save |
| `language-make-next` | Next |
| `settings-slack-sign-out` | Sign out |
| `settings-slack-allow` | Allow… |
| `settings-show-more` | Show more |
| `conv-ask-about` | Ask { $coordinator } about it |

Ids defined in `crates/ordin-localisation/catalogues/en-GB/screens-fixed.ftl`:

| Id | en-GB text |
|---|---|
| `engineering-editor-title` | Code editor |
| `engineering-changes-title` | Changes waiting |
| `engineering-terminal-title` | Terminal |
| `engineering-settings-title` | Project settings |
| `engineering-save` | Save |
| `engineering-discard-edits` | Discard my edits |
| `engineering-entry-accept` | Accept |
| `engineering-entry-reject` | Reject |
| `engineering-keep-this-one` | Keep this one |
| `engineering-keep-yours` | Keep mine |
| `engineering-use-the-agents` | Use the agent's |
| `engineering-commit-message` | Message for this commit |
| `engineering-commit-and-push` | Commit and push |
| `engineering-check-remove` | Remove |
| `engineering-tests-passed` | Tests passed |
| `engineering-tests-failed` | Tests failed |
| `check-running` | Checking… |
| `check-fix-it` | Fix it |
| `check-ask-ordin-to-fix` | Ask { $coordinator } to fix it |
| `check-change-it-yourself` | Change it yourself |
| `agents-made-of` | What { $agent } is made of |
| `fixed-show-more` | Show more |
| `fixed-back` | Back |
| `fixed-nothing-here` | Nothing here yet. |
| `knowledge-title` | Title |
| `knowledge-body` | Page |
| `knowledge-edit` | Edit |
| `knowledge-details-show` | Show the page's details (amended January 2026; user decision; Approved by the user January 2026) |
| `knowledge-details-hide` | Hide the page's details (amended January 2026; user decision; Approved by the user January 2026) |
| `knowledge-remove` | Remove… |
| `knowledge-remove-title` | Remove “{ $title }”? |
| `knowledge-remove-body` | The page leaves this workspace and goes to this computer's Bin, where you can still get it back. |
| `knowledge-remove-verb` | Remove |
| `knowledge-removed` | Removed “{ $title }”. |
| `knowledge-undo-remove` | Undo |
| `knowledge-link-it` | Link it |
| `knowledge-dismiss` | Dismiss |
| `knowledge-show-them` | Show them |
| `team-error-unreachable` | Your team's knowledge can't be reached just now. |
| `team-error-sign-in` | Sign in again to reach your team's knowledge. |
| `team-error-read-only` | You can read this team's knowledge but not add to it. |
| `slack-open-in-slack` | Open in Slack |
| `slack-remove-from-workspace` | Remove from this workspace |
| `slack-remove-confirm` | Remove this channel from the workspace? |
| `jira-add` | Add |
| `jira-open-settings` | Open Settings › Jira |


### Internal Logic (additions)

#### Markers left in the code (FU-136)

Each worker of Phase 10 numbered its own `SPEC-GAP-<N>` markers, so a number repeats across files; a marker is named here by its file and number. The blocks worker reported six markers; three are in the tree (`screens-blocks.ftl` 1, `refs.ts` 2, `CrewPlan.svelte` 4) and the other three numbers appear nowhere in `apps/desktop/ui/src` or the `.ftl` files, so they have nothing to state.

| Marker | File (under `apps/desktop/ui/src/lib/` unless a path is given) | Disposition |
|---|---|---|
| SPEC-GAP-1 | `crates/ordin-localisation/catalogues/en-GB/screens-blocks.ftl` | **Stated**: SM-CU-4 lists every `block-` id |
| SPEC-GAP-2 | `blocks/refs.ts` | **Kept**: no Service API command reads one artefact by its id; needs a Service API change (FU-138, Phase 11) |
| SPEC-GAP-4 | `blocks/kinds/CrewPlan.svelte` | **Stated**: SM-WR-15 (`crews.charter`, ServiceApi.md SVC-SCR-9, closed G-SCR-1; the marker is removed) |
| SPEC-GAP-1 | `views/Settings/updates.svelte.ts` | **Kept**: the generated `UpdateState` lacks `administrator_needed`; needs the Service API types regenerated from SetupAssistant.md's amendment (FU-138, Phase 11). The string comparison stays until then |
| SPEC-GAP-2 | `views/Settings/servers-model.svelte.ts` | **Stated**: SM-CU-7 |
| SPEC-GAP-3 | `views/Settings/Servers.svelte` | **Stated**: SM-CU-8 |
| SPEC-GAP-4 | `views/Settings/SignIns.svelte` | **Stated**: SM-NS-2 |
| SPEC-GAP-5 | `views/Settings/Slack.svelte` | **Stated**: SM-CU-9 |
| SPEC-GAP-6 | `views/Settings/knowledge-agents.svelte.ts` | **Kept**: ADR-115 reads `AccountNeed.state`, the generated `AccountNeed` carries `host` and `system` only; FU-138, Phase 11 |
| SPEC-GAP-7 | `crates/ordin-localisation/catalogues/en-GB/screens-settings.ftl` | **Stated**: SM-CU-4 adopts the proposed ids |
| SPEC-GAP-8 | `views/Diagnostics.svelte` | **Stated**: SM-NS-3 |
| SPEC-GAP-9 | `views/Settings/Settings.svelte` | **Stated**: SM-NS-1 |
| SPEC-GAP-10 | `views/Home.svelte` | **Stated**: SM-CU-5 |
| SPEC-GAP-11 | `views/SearchDialog.svelte` | **Stated**: SM-CU-6 |
| SPEC-GAP-1 | `views/engineering/projects.ts`, `views/engineering/editor-hub.svelte.ts` (and the test named after it in `apps/desktop/ui/tests/engineering-code-checking.test.ts`) | **Kept**: no command returns a `Project` by its id alone; FU-138, Phase 11. Until then the opener calls `rememberProject` with the `Project` it holds (FU-134), and the code-checking bar names the project from the first folder of the file's path |
| SPEC-GAP-2 | `views/engineering/register.ts` | **Stated**: SM-CU-2 |
| SPEC-GAP-3 | `views/fixed/team.svelte.ts` | **Stated**: SM-CU-10 |
| SPEC-GAP-4 | `views/fixed/LinkLine.svelte` | **Stated**: SM-CU-11 |
| SPEC-GAP-5 | `views/fixed/jira.svelte.ts` | **Kept**: the linked issue's changed-item notice needs `WorkingCopyItem.origin_newer_at` and `workspaces.read_item`, neither generated; FU-138, Phase 11 |
| SPEC-GAP-6 | `crates/ordin-localisation/catalogues/en-GB/screens-fixed.ftl` | **Stated**: SM-CU-4 |
| SPEC-GAP-7 | `views/fixed/memory-used.svelte.ts` | **Stated**: SM-CU-11 |

A marker marked **Stated** is removed from the code by the Patch that follows this amendment; a marker marked **Kept** stays, with its comment naming FU-138.

5. **SM-CU-5 Home's empty sections and groups** (closes `Home.svelte` SPEC-GAP-10; adds to the Home row of step 9). Each of the three sections, when it has no row after the filter, draws one line: Waiting for you `home-nothing-waiting`, Running now `home-nothing-running` (existing), Since you last looked `home-nothing-since`. Rows are grouped by `HomeRow.workspace_name` in the order the reply gives. A row with no `workspace_id` whose `opens.kind` is `hosted_approval` is grouped last under `home-group-hosted-agents`; any other row with no `workspace_id` is grouped under `home-group-no-workspace`, before it.

6. **SM-CU-6 Search result count** (closes `SearchDialog.svelte` SPEC-GAP-11). The dialog's polite live region reads `search-result-count` with the number of entries drawn in all three groups after SM-SF-9's filtering, updated once per answer of `search.query` (after the 120 ms debounce), and `search-no-results` when the number is 0 and the box is not empty. Nothing is announced while the box is empty.

7. **SM-CU-7 Connect to a server…, the pasted line** (closes `servers-model.svelte.ts` SPEC-GAP-2; adds to the Settings row of step 9). The dialog has two fields, `server-address` and `server-code`, and the hint `server-paste-line-hint` under the address field. **Connect** sends `servers.connect` with `{ kind: 'address_and_code', address, code }` when both fields hold text after trimming, and with `{ kind: 'pairing_line', line: <address field> }` when the code field is empty. An empty address field leaves **Connect** disabled. A refusal is drawn by `ErrorLine` in the dialog, the fields keeping what was typed.

8. **SM-CU-8 A server that cannot update itself** (closes `Servers.svelte` SPEC-GAP-3; replaces the quoted sentence of SM-UP-5.2 by an id). When `setup.server_update` returns `update` with `self_update = false`, the line under the server's row is `server-update-available` followed by `server-update-self-unavailable`, with no button. The sentence is the Setup Assistant's own (SetupAssistant.md SET-UP-14), held by the screens' catalogue because no refusal carries it in this case.

9. **SM-CU-9 Slack rule words** (closes `Slack.svelte` SPEC-GAP-5; adds to the Settings › Slack row of step 9). `settings-slack-rule-line` takes `kind` = `settings-slack-rule-kind-handover-notices` or `settings-slack-rule-kind-ordin-answers` by `SlackRule.kind`, and `state` = `settings-slack-rule-state-on` or `settings-slack-rule-state-off` by `SlackRule.state`. Rules in state `off` are listed too, under the rules in state `on`, without **Turn off**. **Sign out** is `settings-slack-sign-out` and **Allow…** is `settings-slack-allow` (SM-CU-4).

10. **SM-CU-10 Team knowledge, the status's last error** (closes `team.svelte.ts` SPEC-GAP-3; adds to step 4 of the Team knowledge amendment). `TeamKnowledgeStatus` carries a `last_error` code without its sentence, so the panel draws, by code: `TEAM_NOTHING_TO_SHARE` → `team-nothing-to-share` with no action; `TEAM_REMOTE_UNREACHABLE` → `team-error-unreachable` with **Try again**; `TEAM_SIGN_IN_NEEDED` → `team-error-sign-in` with **Sign in again**; `TEAM_READ_ONLY` → `team-error-read-only` with no action; any other code → the catalogue's `error-svc-internal` with **Try again**. The sentences are the Team Knowledge Service's (TeamKnowledgeService.md *Error Table*), except that `team-error-sign-in` omits the host the owner's sentence names, because the status carries no host.

11. **SM-CU-11 A page named by its id** (closes `memory-used.svelte.ts` SPEC-GAP-7 and `LinkLine.svelte` SPEC-GAP-4). `FactView` and `LinkBy` of kind `page` carry a page id only. A view that draws them reads each distinct page id once with `knowledge.get { page_id }` while it is open, and draws `PageView.page.title` in `memory-used-page` and `memory-link-by-page`. A refusal (the page was removed) draws `memory-page-gone` in place of the line, with no **Open the page**. The titles are held for the view's lifetime only, never in `localStorage`.

#### When an approval card opens (FU-142)

12. **SM-AP-1 Always a new tab; in-front rule deferred** (replaces, until SM-AP-2 is built, step 7's clause "in front, with focus on its heading, only when it answers what the person just did; otherwise", and the Example's sentence "The card opens as a screen in front, with focus on its heading (step 7)"). On `approval_requested` the card opens in its workspace's screen area as a new tab marked *new*, never in front, whatever the person did last; focus does not move, the person's place does not move, and the polite live region announces the new approval (step 14). A card from another workspace opens in that workspace's screen area without switching. Opening it from *Needs you*, from its one-line card or from a notification puts it in front with focus on its heading, as before. This is the behaviour of `lib/app/bootstrap.ts` today (`screens.open(…, { inFront: false })`).
13. **SM-AP-2 The in-front rule, deferred.** When it is built, a card opens in front, with focus on its heading, only when its `approval_requested` event arrives within 5 seconds (SA-UI-CU-01) after the reply to an `ordin.send_message` or an `approval.*` command that the person sent from the card's workspace to the same backend; in every other case SM-AP-1 applies. The 5-second window starts at the reply, not at the press. No other signal (a pointer position, a focused tab) counts as "what the person just did". Until it is built, no screen implements SM-AP-2 in part.

#### The screens Phase 10 left out (FU-135)

Each screen below is a fixed screen of the application (11 §5.15). It opens as a tab in the open workspace's screen area unless it is a Settings section; focus goes to its heading (level 2) when it opens; a refusal of any command it sends is drawn by `ErrorLine` as the owner's sentence with at most one action (step 11), in the place the refusal concerns; the person never sees a code. The message ids each one draws are in the table *New messages* below, grouped by screen.

14. **SM-NS-1 Settings section bodies** (closes `Settings.svelte` SPEC-GAP-9). A value with `set_by = 'company'`, or a `SettingView` with `person_may_change = false`, is drawn disabled with `settings-set-by-company` beside it. A change is sent when the control changes (no **Save** in these sections, except where named); the control keeps the person's choice while the command is in flight and returns to the stored value on a refusal. Each section below is amended in place by the Settings walk-through amendment, SM-SW-1 to SM-SW-17 (amended January 2026 (user decision; Approved by the user January 2026)).
    1. **Agent repositories** (11 §4, §5.2, §5.27). Commands: `catalogue.list_repositories`, `catalogue.probe_repository`, `catalogue.add_repository`, `catalogue.rename_repository`, `catalogue.remove_repository`. One row per `RepositorySummary` in the order given: `repository.display_name`; `settings-repos-format-ordin` or `settings-repos-format-apm` by `repository.format`; `settings-repos-agent-count` with `agent_count`; `settings-repos-locked` ("Can't be removed"; amended January 2026, the user's decision "don't say set by your company": the program's own repository is locked too, and no company is named) when `set_by_company`; **Show its parts** (`settings-repos-show-parts`) when `repository.person_can_publish`, opening `repository-parts:<repository.id>` in the open workspace (SM-NS-7; never in Settings, 11 §4 "Settings is never the path to finish a task"); **Rename…** (`settings-repos-rename`) and **Remove…** (`settings-repos-remove`) on rows not set by the company. **Rename…** opens a dialog with one field (`settings-repos-rename-field`, 1–80 characters) and **Rename** (`settings-repos-rename-verb`) / **Cancel** (`confirm-cancel`), sending `catalogue.rename_repository { id, display_name }`. **Remove…** opens `ConfirmDialog` titled `settings-repos-remove-title` with `settings-repos-remove-body`, verbs **Remove** (`settings-repos-remove-verb`, never the default) and **Cancel** (`confirm-cancel`), then `catalogue.remove_repository { id }`. **Add an agent repository…** (`settings-repos-add`) opens a dialog: the field `settings-repos-add-address` (at most 2,048 bytes); **Look it up** (`settings-repos-add-look`) sends `catalogue.probe_repository { web_address }`; its `RepositoryProbe` draws `settings-repos-add-found` (`count` = `agent_count`, `format` = the format words above), a name field preset to `suggested_name` (`settings-repos-add-name`), `settings-repos-add-cannot-publish` when `person_can_publish` is false, and **Add** (`settings-repos-add-verb`), which sends `catalogue.add_repository { probe_id, display_name }`. A probe past its `expires_at` is looked up again on **Add** before sending.
    2. **Email** (11 §5.22, UX-A44). Commands: `mail.settings`, `mail.set_settings`, `mail.verify_sign_in`, `identity.list`, `identity.sign_out`, `identity.save_mail_password`, `workspaces.list`, `workspaces.email_links`. Drawn in order: `settings-email-fixed`; **Your mailbox** (`settings-email-mailbox`): the `account_label` of the `identity.list` entry whose `kind` is `microsoft`, `google`, `mail_server_single_sign_on`, `mail_server_password` or `mail_server_app_password`, then `settings-email-signed-in-password` (kinds `mail_server_password`, `mail_server_app_password`) or `settings-email-signed-in-account` (the other three), with **Change password** (`settings-email-change-password`, password kinds only; opens the sign-in screen of SM-NS-2.3) and **Sign out of email** (`settings-email-sign-out`, `identity.sign_out { credential_id }`, at once); `mail.verify_sign_in`'s answer `refused` or `not_reachable_securely` draws `settings-email-check-refused` or `settings-email-check-not-secure`. **Add my signature** (`settings-email-add-signature`, switch) with the signature text box (`settings-email-signature`, at most 2,000 characters, saved on leaving the box); **Say at the bottom that an agent drafted it** (`settings-email-foot-line`, switch; on and disabled with `settings-email-foot-line-required` when `agent_foot_line_locked`); **Check for new email** (`settings-email-check`) with the two choices `settings-email-check-every-15` (`every15_minutes`) and `settings-email-check-on-open` (`only_on_workspace_open`). Each change sends `mail.set_settings` with the whole `MailSettingsUpdate`. **Email in your workspaces** (`settings-email-workspaces`): per workspace of `workspaces.list`, its `workspaces.email_links` by `display_name`; a workspace with none is not listed.
    3. **Telegram** (11 §5.25, UX-A51). Commands: `telegram.status`, `telegram.connect_bot`, `telegram.remove`. Drawn in order: `settings-telegram-fixed`; when `offered` is false, the sentence for `reason_not_offered` (`settings-telegram-not-allowed`, `settings-telegram-tools-off`, `settings-telegram-no-app`, `settings-telegram-no-bot` for `not_allowed_by_company`, `tools_turned_off`, `app_not_on_computer`, `no_bot`) and nothing more; when `connected`, `settings-telegram-bot` with `bot.display_name` and `bot.username`, `settings-telegram-admin-note` when `bot.can_read_all_group_messages` is false, and **Remove Telegram** (`settings-telegram-remove`, `telegram.remove {}` at once, 11 §5.25); when offered and not connected, **Set up Telegram** (`settings-telegram-set-up`) with the field `settings-telegram-key` (a password field, cleared after sending) and **Connect** (`settings-telegram-connect`, `telegram.connect_bot { key }`); the reply's `BotInfo` draws `settings-telegram-bot`.
    4. **Dictation** (ADR-078, step 16). Command: `dictation.get_availability`. `available` → `settings-dictation-available` with the locale's name (`Intl.DisplayNames` of the active locale); `needs_assets` → `settings-dictation-needs-assets`; `unavailable` → its `message` as text. The section offers no control: dictation is started from the message box and form fields only.
    5. **How Ordin talks to you** (11 §5.19, UX-A34). Commands: `voice.get`, `voice.update`. Drawn in order: `settings-voice-fixed`; **Tone** (`settings-voice-tone`) with `settings-voice-tone-friendly`, `-neutral`, `-warm`, `-direct`; **Formality** (`settings-voice-formality`) with `settings-voice-formality-everyday`, `-formal` (field `register`); **Length** (`settings-voice-length`) with `settings-voice-length-brief`, `-shortest`; **Call me** (`settings-voice-call-me`, field `name_to_use`, saved on leaving the box, empty sends `""`); **Notes for Ordin** (`settings-voice-notes`, at most 300 characters, hint `settings-voice-notes-example`, line `settings-voice-notes-limit`); a choice with `set_by = 'company'` reads `settings-voice-company-default` beside it and stays changeable. Each change sends `voice.update` with the one field changed. The preview is not drawn in this build (G-CU-5).
    6. **Using your plan** (11 §5.18, UX-A32). Commands: `usage.today`, `usage.get_settings`, `usage.update_settings`. Drawn in order: **Today** (`settings-plan-today`): `settings-plan-today-line` with `typical_day_permille / 10` as a percentage, and under **Details** (`settings-plan-details`) `settings-plan-today-details` with `model_turns`, `smaller_share_permille / 10` and `answered_without_model`; **How Ordin uses your plan** (`settings-plan-choice`) with `save_my_plan`, `balanced`, `best_results` as `settings-plan-choice-save`, `-balanced`, `-best` each with its line `settings-plan-choice-save-line`, `settings-plan-choice-balanced-line`, `settings-plan-choice-best-line`, and `custom` drawn as `settings-plan-choice-custom` (not choosable); the switches `settings-plan-without-model`, `settings-plan-reuse`, `settings-plan-only-needed`, `settings-plan-continue-after-break` (fields `answer_simple_requests_without_model`, `reuse_answers`, `give_agents_only_needed_parts`, `continue_from_summary_after_break`); **Summarise long conversations** (`settings-plan-summarise`) with `settings-plan-summarise-early`, `-nearly-full`, `-only-asked`; **Ask before work estimated as** (`settings-plan-ask-before`) with `settings-plan-ask-heavy`, `-medium-or-heavy`, `-never`; **Agents working at the same time** (`settings-plan-agents-at-once`, a number with − and +); **More choices** (`settings-plan-more`, folded): **Show usage on each run** (`settings-plan-show-usage`) and **Thinking** (`settings-plan-thinking`) with `settings-plan-thinking-low`, `-automatic`, `-high`. Each change sends `usage.update_settings` with the one field changed. The model level per kind and the daily pause are not drawn in this build (G-CU-6).
    7. **Notifications** (11 §7). Commands: `settings.get { section: notifications }`, `settings.change`. One switch per `SettingView` whose `key` is one of `notifications.approvals`, `notifications.finished_work`, `notifications.questions`, `notifications.draft_reviews`, `notifications.handovers` (OrdinDesktop.md), labelled `settings-notify-approvals`, `-finished-work`, `-questions`, `-draft-reviews`, `-handovers`, in that order, under the line `settings-notify-intro`; a key not in this list is not drawn. A change sends `settings.change { key, value }` with a boolean. `DSK_PREFERENCES_WRITE_FAILED` returns the switch and draws its sentence.
    8. **Code** (11 §4, §5.11, UX-A25, UX-A26). Commands: `projects.ides`, `projects.set_ide_enabled`, `projects.language_plugins`, `projects.configure_language`, `projects.restart_language_server`. Drawn in order: **Open code in** (`settings-code-open-in`): one switch per `Ide` by `name`, on when `enabled`, sending `projects.set_ide_enabled { ide_id, enabled }`; `settings-code-no-ide` when the list is empty; **Code checking by language** (`settings-code-languages`): one row per `LanguagePlugin` by `language`, with the server in use (`chosen_server`, else `default_server`), `settings-code-pinned` with `pinned_version` when present, `settings-code-from-company` when `source = organisation_profile` or `settings-code-from-agent` when `agent_package`, a switch **On** (`settings-code-on`) sending `projects.configure_language { language, enabled, chosen_server, server_settings }` with the row's current `chosen_server` and `server_settings` unchanged, a choice of server among `approved_servers` when it holds more than one, and **Restart** (`settings-code-restart`, `projects.restart_language_server { language }`) while `enabled`. `server_settings` is not drawn (G-CU-7).
    9. **What Ordin remembers**, titled *Memory* (11 §4, §5.10; amended January 2026; user decision; Approved by the user January 2026). The section holds settings only, never a list of facts; facts are seen and changed on the `memory` screen in the side tree. Its body is SM-MS-1 to SM-MS-5 of the amendment *Settings › Memory holds settings only*. Commands: `settings.get { section: what_ordin_remembers }` and `settings.change`.

15. **SM-NS-2 Sign-ins** (closes `SignIns.svelte` SPEC-GAP-4; adds to SA-CLOSE-03). Commands: `identity.list`, `identity.sign_in`, `identity.sign_in_wait`, `identity.sign_in_cancel`, `identity.sign_out`, `identity.save_gitlab_token`, `identity.save_github_token`, `identity.save_mail_password`, `identity.import_client_certificate`, `profile.get_document`, `app.open_web_page`, `confluence.connect_token`, `confluence.connect_with_jira` (SM-NS-2.11). Every service ordin can use is always listed, and a service with nothing saved signs in through the same `identity.sign_in` (SM-SW-3, SM-NS-2.5; amended January 2026 (user decision; Approved by the user January 2026)). A service this version cannot sign in to is greyed out (SM-NS-2.9), and Jira is connected in Settings › Jira only (SM-NS-2.10) (amended January 2026; user decision; Approved by the user January 2026).
    1. One row per `CredentialView` in the order given: the account's product name by `kind` (`signin-kind-<kind>`, the kinds of the generated `CredentialKind`, with `_` written `-`), then `account_label` in secondary text; the state by `state`: `signed_in` → `signin-state-signed-in`, with `signin-expires` and `expires_at` when present, and **Sign out** (`signin-sign-out`, `identity.sign_out { credential_id }` at once); `needs_sign_in` → `signin-state-needs-sign-in` and **Sign in** (`signin-sign-in`); `signed_out` → `signin-state-signed-out` and **Sign in**.
    2. **Sign in** sends `identity.sign_in { kind }` (with `site` for `jira`, from Settings › Jira). By `SignInChallenge.kind`: `browser_opened` → `signin-browser-opened` and **Cancel** (`identity.sign_in_cancel { challenge_id }`), then `identity.sign_in_wait { challenge_id }` until it answers; `device_code` → `signin-device-code` with `verification_uri` drawn as text, the `user_code` in a large monospaced line with **Copy the code** (`signin-copy-code`, the system clipboard through the WebView), then `identity.sign_in_wait`; `already_signed_in` → the row redraws from `identity.list`; `relayed_browser` arises only on a personal server (Slack) and is drawn as `browser_opened`. A reply `CRED_PROVIDER_NOT_CONFIGURED` for `gitlab` draws no sentence, since the row's block already offers the token (SA-CLOSE-03); for `github` it draws no sentence and the block offers the token in place of one-click sign-in (SM-NS-2.6) (amended January 2026; user decision; Approved by the user January 2026); for `jira` it is answered by Settings › Jira; for any other kind (`slack`, the others) its sentence is drawn under the list with `ErrorLine`, so **Sign in** never does nothing (amended January 2026; user decision; Approved by the user January 2026). `CRED_SIGN_IN_CANCELLED` draws nothing; `CRED_SIGN_IN_TIMEOUT` draws its sentence with **Try again**.
    3. A mail password kind opens the sign-in screen of 11 §5.22 in a dialog: title `signin-mail-title`; `signin-mail-password-note`; **Email address** (`signin-mail-address`, preset to the `account_label` of the credential when one exists) with `signin-mail-address-hint`; **App password** (`signin-mail-app-password`) or **Password** (`signin-mail-password`) by `MailPasswordKind`, a password field with **Show** (`signin-show`); `signin-mail-keychain-note`; **Sign in** sends `identity.save_mail_password { account_label, kind, password }`; the password field is cleared after sending.
    4. **Import a certificate…** (`signin-import-certificate`) is drawn on the local backend only, and only when `profile.get_document` returns `document.server.client_certificate_required = true` (Configuration.md: "Whether desktops must present a client certificate"). It sends `identity.import_client_certificate {}`; the desktop program opens the pickers. A result with `certificate` draws `signin-certificate-imported` with `certificate.name` and `not_after` as a date; a result without it (cancelled) draws nothing. `CRED_CERTIFICATE_EXPIRED` and `CRED_CERTIFICATE_INVALID` draw their sentences.
    6. **SM-NS-2.6 GitHub personal access token** (amended January 2026; user decision; Approved by the user January 2026). When the build carries no `GITHUB_APP_CLIENT_ID` (`identity.list` answers `github_one_click` false or absent, or `identity.sign_in { kind: github }` answers `CRED_PROVIDER_NOT_CONFIGURED`, which draws no sentence), GitHub's shown block (SM-NS-2.7) offers **With a token** (amended January 2026; user decision; Approved by the user January 2026), drawn in order: `signin-github-token-why`; the three steps `signin-github-token-step-1` to `signin-github-token-step-3` (a fine-grained token with Repository permissions Contents: Read and write and Pull requests: Read and write, the GitHub calls the agent library and GitHub documents make); **Open GitHub's token page** (`signin-github-open-token-page`), which sends `app.open_web_page { page: github_tokens }` to the local backend, a refusal drawn under it with **Try again**; **Paste a GitHub personal access token** (`signin-github-paste-token`), a password field with `signin-github-token-hint`; **Save token** (`signin-github-save`) sends `identity.save_github_token { token }` with the spaces at the ends removed, and the field is cleared whatever the reply. An empty field sends nothing and the hint becomes `signin-github-token-empty`. `CRED_PROVIDER_REFUSED` draws `signin-github-token-refused` in place of the hint; any other refusal is drawn with `ErrorLine`. Success re-reads `identity.list`; the block stays shown, now with **Sign out**. A build that carries a GitHub App refuses a pasted token (`CRED_FLOW_NOT_OFFERED`), so the box is not drawn there.
    7. **SM-NS-2.7 One chevron per row** (amended January 2026; user decision; Approved by the user January 2026). Every available service row (SM-NS-2.9) carries exactly one control, an icon button with a chevron that points right while the row's ways to sign in are hidden and turns to point down while they are shown. Each press toggles them. The button carries `aria-expanded` (`true` while shown), `aria-controls` naming the shown block (`signin-ways-<row key>`, the credential id or, for a service with nothing saved, the service id), and an accessible name and title of `signin-row-hide` while shown and `signin-row-show` while hidden, each with the service's product name as `service`. The shown block takes the row's full width and holds, in order: when the row's account is `signed_in`, its `account_label` and state with **Sign out** (`identity.sign_out { credential_id }` at once); then the ways to sign in, each a sub-section with its heading: for GitHub, **With one click** (`signin-way-one-click`, **Sign in** sending `identity.sign_in { kind: github }`) only when `identity.list` answers `github_one_click: true`, otherwise **With a token** (`signin-way-token`, the box of SM-NS-2.6), then **With an SSH key** (`signin-way-ssh`, SM-NS-2.8) on a backend that answers `app.ssh_keys`; for GitLab, **With one click** only when the profile names `gitlab.client_id`, then **With a token** (the box of SA-CLOSE-03); for every other service, **Sign in** (SM-NS-2.2, SM-NS-2.5) while the account is not `signed_in`. A sign-in that waits (SM-NS-2.2) is drawn under the row whether or not the block is shown. Nothing else in a row is a control; **Sign in** and **Sign out** are never drawn outside the block.
    8. **SM-NS-2.8 GitHub with an SSH key** (amended January 2026; user decision; Approved by the user January 2026). On a backend that answers `app.ssh_keys`, GitHub's shown block (SM-NS-2.7) holds the sub-section **With an SSH key** (`signin-way-ssh`, `id` `signin-ssh-github`) (amended January 2026; user decision; Approved by the user January 2026); it has no chevron of its own, and showing GitHub's block reads `app.ssh_keys` again. The sub-section is drawn in order: the heading; `signin-github-ssh-why`, which says in one line that some features, such as pull requests, still need the token; the key list (`signin-github-ssh-choose`), a choice of `signin-github-ssh-none` and each key by name, whose change sends `app.choose_ssh_key { path }` (`null` for none), or `signin-github-ssh-no-keys` when the folder holds none; **Make a new key** (`signin-github-ssh-create`), which sends `app.create_ssh_key`. With a listed key chosen: `signin-github-ssh-public` with a Copy icon button (`signin-github-ssh-copy`, the system clipboard; `signin-github-ssh-copied` after), the public key in a monospaced block, `signin-github-ssh-add`, **Open GitHub's SSH key page** (`signin-github-ssh-open-page`, `app.open_web_page { page: github_ssh_keys }`) and **Check it works** (`signin-github-ssh-check`, `app.check_ssh_key`), whose answer is drawn as one polite status line: `signin-github-ssh-checking` while it runs, then `signin-github-ssh-works`, `signin-github-ssh-refused` or `signin-github-ssh-unreachable`. A refusal of any of these commands is drawn with `ErrorLine`. A new choice clears the last check.
    9. **SM-NS-2.9 Services not available in this version** (amended January 2026; user decision; Approved by the user January 2026). A service row is available when this version can complete its sign-in, decided from the build and the profile: GitHub and GitLab always (a token the person makes); Confluence whenever the backend answers `confluence.connect_token` (an API token the person makes, SM-NS-2.11; amended January 2026; user decision; Approved by the user January 2026), and otherwise when `profile.get_document` names `confluence`; Microsoft 365 and Google when `profile.get_document` names `microsoft` or `google`; Slack when the backend answers `slack.status` (build switch `slack`) and the profile's `slack` names `workspace` and `client_id` with `tools_enabled` not `false`; the company's mail server when the backend answers `mail.settings` (build switch `mail`) and the profile's `email` is enabled with `system = company_server`; the company account when the profile names `identity_provider` (otherwise it is not listed). With no profile read, the profile's part counts as present. A row with a saved account is always available, so it can be signed out. Any other row is drawn greyed out with `aria-disabled="true"`, its product and `signin-for-*` line, and the one line `signin-not-available`; it has no control. This replaces `signin-not-used`, and nothing on the screen names the organisation or its IT staff.
    10. **SM-NS-2.10 Jira in one place** (amended January 2026; user decision; Approved by the user January 2026). Sign-ins has no Jira row: Jira's only way in needs the site, and Settings › Jira holds the site, the API token and **Sign in with Atlassian…** (ADR-106). Settings › Jira draws **Sign in with Atlassian…** only when `profile.get_document` names `confluence` (the Atlassian app); with no app named only the API token is offered. A `CRED_PROVIDER_NOT_CONFIGURED` reply draws `settings-jira-oauth-unavailable`, which names no company or IT staff.
    11. **SM-NS-2.11 Confluence with an API token** (amended January 2026; user decision; Approved by the user January 2026). Confluence is no longer "not available in this version" with the default profile: its row is active wherever the backend answers `confluence.connect_token`. Its shown block (SM-NS-2.7) holds, while the account is not `signed_in`: **With one click** (`signin-way-one-click`, **Sign in** sending `identity.sign_in { kind: confluence }`) only when the profile names `confluence.oauth_client_id`; then **With an API token** (`signin-way-api-token`), a form `#signin-confluence-token` with `signin-confluence-token-why`, the site field (`signin-confluence-site`, placeholder `signin-confluence-site-placeholder`), the email field (`signin-confluence-email`), the one line `signin-confluence-token-how`, **Open Atlassian's token page** (`signin-confluence-open-token-page`, `app.open_web_page { page: atlassian_api_tokens }`, which opens id.atlassian.com's API token page), the token field (a password field, `signin-confluence-token`, hint `signin-confluence-token-hint`), **Connect** (`signin-confluence-connect`) and, when a Jira account is `signed_in`, **Use my Jira sign-in for {site}** (`signin-confluence-use-jira`, `confluence.connect_with_jira {}`). **Connect** reads the site as Settings › Jira does (SM-JS-1) and sends `confluence.connect_token { site, email, token }`; the token field is cleared whatever the reply. An empty site sends nothing and draws `signin-confluence-site-needed`; an empty token sends nothing and the hint becomes `signin-confluence-token-empty`. A refusal is drawn as its sentence under the form (`CONF_TOKEN_REFUSED`, `CONF_SITE_INVALID`, `CONF_UNAVAILABLE` with **Try again**, which re-sends the same input). On success the list is read again; the signed-in block reads `signin-confluence-connected` (site and name) with **Disconnect** (`signin-confluence-disconnect`), which sends `identity.sign_out { credential_id }` at once and draws `signin-confluence-disconnected`. The token is never drawn and nothing on the screen names the organisation or its IT staff.
    12. **SM-NS-2.12 Microsoft 365 without a company profile** (amended February 2026, from ADR-126; user decision; Approved by the user February 2026). SM-NS-2.9's Microsoft 365 row is also available when `identity.list` answers `microsoft_built_in: true`, whatever the profile says; its block (SM-NS-2.7) offers **Sign in** as any other service. While a sign-in through the build's app waits (no profile `microsoft` section), the waiting line gains `signin-microsoft-work-account` (sign in with your work or school account; a personal Microsoft account won't work). `CRED_ADMIN_CONSENT_REQUIRED` is drawn with `ErrorLine` under the row as its own sentence (`error-cred-admin-consent-required`: your organisation has to approve ordin before you can sign in; ask whoever manages your Microsoft 365, or use a company profile), with no **Try again** (its action is `null`). The Add screen's **Sign in with Microsoft** (SM-AW) draws the same refusal the same way. Words: `signin-microsoft-work-account` in `screens-settings.ftl`, the error's line in `errors-identity.ftl` (en-GB).

16. **SM-NS-3 Diagnostics, support export** (closes `Diagnostics.svelte` SPEC-GAP-8; adds to the Diagnostics row of step 9). Diagnostics is the one view that shows codes and technical names (02:326), so its words are plain but its rows are technical. Headings: `diagnostics-journal-title` above `diagnostics.lines`, 100 lines a page with **Show more** while `next_cursor` is present, each line `at`, `level`, `code`, `summary`, `reference`, and no **Send to IT** or other button *(amended January 2026, the user's decision "send to IT in diagnostics should be removed": every feature is self-service; plan 05 Phase 9)*; `diagnostics-window-title` ("This window", step 12) above the window's ring. **Save logs for support…** (`diagnostics-support-export`) opens a panel: `diagnostics-support-pick` above the runs of `runs.list {}` (newest first, 50 a page), each with a tick, its `id`, `state` and `queued_at`; the newest runs, up to 20, start ticked when the panel opens, so **Save…** works at once (amended January 2026 (user decision; Approved by the user January 2026): with every tick off at first, **Save…** stayed disabled and the person saw nothing happen); at most 20 ticked (`diagnostics-support-limit` shown when the 21st is ticked, which stays unticked); `diagnostics-support-note`, which names the last 3 days of ordin's logs as part of the file (OrdinDesktop.md DSK-LOG-5; amended January 2026 (user decision; Approved by the user January 2026)); **Save…** (`diagnostics-support-save`, disabled with no tick) sends `diagnostics.support_export { run_ids }`; `SavedCopy` draws `diagnostics-support-saved` with `saved_path`; `DSK_DIALOG_CANCELLED` draws nothing. Local backend only (scope `Local`). *Amended January 2026 (user decision; Approved by the user January 2026):* the journal's lines are one compact table with the column headings `diagnostics-col-time` ("Time"), `diagnostics-col-level` ("Level") and `diagnostics-col-summary` ("What happened") and a last column named `diagnostics-col-details` for screen readers; each row draws `at`, `level` and `summary` and a small icon button (`i-chev`, `aria-expanded`, `diagnostics-line-more` "Show the details" / `diagnostics-line-less` "Hide the details" as `aria-label` and `title`) that opens, under the row, its `code` and `reference`. No line is in the journal → `diagnostics-journal-none` ("No events to show."). `DiagnosticLine` carries no workspace or agent, so the table has no such column. The screen's buttons — **Save logs for support…** (`i-save`, `aria-expanded` while its panel is open), **Clear logs…** (`i-bin`), **Save…** (`i-check`) and each **Show more** (`settings-show-more`, `i-unfold`) — are small icon buttons in the editor toolbar's style (SM-NS-4.6, 1.75 rem square), each with its words as `aria-label` and `title`. *Superseded in part (amended January 2026; user decision; Approved by the user January 2026):* **Save logs for support…** is one action with no panel and no ticks: pressing it reads `runs.list {}`, takes the newest runs, up to 20, and sends `diagnostics.support_export { run_ids }`, whose file also holds the last 3 days of ordin's logs; `diagnostics-support-note` ("The file holds your newest 20 runs' records and the last 3 days of ordin's logs. It holds no document or conversation text.") is the button's second `title` line; the button is disabled while saving; no run → `diagnostics-support-nothing` ("There is nothing to save yet.") as a status line and nothing is sent; `SavedCopy` draws `diagnostics-support-saved`; `DSK_DIALOG_CANCELLED` draws nothing; any other refusal is `ErrorLine`. `diagnostics-support-pick`, `diagnostics-support-limit` and `diagnostics-support-save` are withdrawn, and so are the 21-tick error row and the scenario *a 21st run is refused*. The journal table gains a first column `diagnostics-col-reference` ("Reference"), each cell the line's `reference` — the first 8 characters of the entry's `JournalEntryId` — with `diagnostics-col-reference-hint` ("A short code for this event. Quote it to support so they can find it.") as the heading's and the cell's `title`, drawn quieter on `info` rows; the row's details now show `code` only.

17. **SM-NS-4 The document page and its editor** (11 §5.6, UX-A52, UX-A59; screen `document:<place id>:<path>`). Commands: `files.preview`, `files.versions`, `files.restore_version`, `files.save_edit`, `files.published_copies`, `craft.open`, `craft.open_with_system_default`, `files.move_to_bin`, `home.document_opened`.
    1. On opening: `files.preview { place_id, path }`, `files.versions { place_id, path }` and `home.document_opened` for the document. Heading: the last segment of `path`.
    2. **At a glance**: `doc-glance` with the newest `FileVersion`'s `created_at`, and who changed it by `origin.kind`: `person` → `doc-origin-person`, `run` → `doc-origin-agent`, `art_craft_edit` → `doc-origin-app` with `origin.app`. Then **Open in [app]** (`doc-open-in-app`, `craft.open { place_id, path }`): `opened_in_app` and `opened_with_system_default` draw nothing; `no_editor` draws the question `doc-no-editor` with **Open in your usual app** (`doc-open-usual-app`, `craft.open_with_system_default`); and **Ask Ordin about this** (`block-ask-about-this`, the conversation message `block-ask-text-this` with the document's name, and the document itself as one attachment `{ kind: 'place_file', place_id, relative_path }`, SM-ASK-1). *amended January 2026 (user decision; Approved by the user January 2026):* **Open in [app]** and **Ask Ordin about this** are no longer drawn on the page; they are entries of the document row's ⋯ menu in the tree (SM-RL-4), sending the same commands. Under the heading the page draws `doc-location` with where the document lives: the linked location's `display_name` from `workspaces.links { workspace_id }` (the link whose `place_id` is the page's), then the folders of `path`, joined by " › "; the line is left out when neither is known. The `no_editor` question is drawn under the document's row when the menu's **Open in [app]** raised it, and on the page when the **Open in its app** of a long PDF (SM-PDF-1) raised it.
    3. **The page**: the `Preview` drawn as step 6 draws `preview` (text as text nodes; a `text` preview with `markdown = true` as a formatted page from the Markdown, never as Markdown symbols outside a workspace that links code; image, PDF, thumbnail or nothing), `truncated` adding `doc-cut`.
    4. **Versions** (`doc-versions`, a folded heading): one line per `FileVersion`, newest first, `doc-version-line` with `version`, `created_at` and the origin words of 2; **Restore** (`doc-restore`) on every line but the newest, sending `files.restore_version { file_version_id }` after `ConfirmDialog` `doc-restore-title` / `doc-restore-body`.
    5. **Published as** (`doc-published-as`, a folded heading with its count; shown only for a Markdown document): `files.published_copies { original: { kind: 'place_file', place_id, path } }`, one line per `PublishedCopy` with its `audience` and `doc-copy-kept` when `keep_up_to_date`.
    6. **Edit** (`doc-edit`, only on a `text` preview with `markdown = true` and `truncated = false`) turns the page into its editor on the full pane (UX-A59): a bar with **Save** (`doc-save`), **Cancel** (`doc-cancel`), the save status, and under it the what-you-see editor (*amended January 2026 (user decision; Approved by the user January 2026)*, replacing the five-button toolbar over the raw text): the document is shown and edited as formatted text and the person never sees a Markdown code. The editor (`apps/desktop/ui/src/lib/editor/MarkdownEditor.svelte`) takes and gives Markdown; its toolbar (`role="toolbar"`, named `editor-toolbar`, one tab stop, arrow keys, Home and End between buttons) holds Heading, Bold, Italic, Bulleted list, Numbered list, Link, Quote, Table, Undo and Redo (`editor-heading`, `-bold`, `-italic`, `-bullets`, `-numbers`, `-link`, `-quote`, `-table`, `-undo`, `-redo`), with Cmd/Ctrl+B, I, K, Z and Shift+Z; the editable area is named by the document's name. Link opens a small box (`editor-link-address`, **Apply**, **Remove link**, **Cancel**) that accepts an http(s) or mailto address only, else `editor-link-invalid`. Headings, bold, italic, strikethrough, lists, links, quotes, tables, code and rules come back as Markdown; a part it cannot show (raw HTML, a picture, front matter, a task list, a link definition, a link of another scheme) is kept word for word, drawn as its words with `editor-kept`, and written back unchanged. Pasted HTML (from Word, a web page or a mail) is cleaned before it is read: no scripts, styles, event handlers or other attributes, links only when http(s) or mailto. The same editor is used wherever a person writes Markdown (UX guidelines §5.33). *amended January 2026 (user decision; Approved by the user January 2026):* the toolbar is a row of icon-only buttons in four groups divided by a `role="separator"` line — Heading, Bold, Italic | Bulleted list, Numbered list | Link, Quote, Table | Undo, Redo — drawing `i-heading`, `i-bold`, `i-italic`, `i-bullets`, `i-numbers`, `i-link`, `i-quote`, `i-sheet`, `i-undo` and `i-redo` (VisualDesign.md V8); each button carries its name as `aria-label` and a `title` tooltip, `editor-tip` with the shortcut where it has one ("Bold (⌘B)" on macOS, "Bold (Ctrl+B)" elsewhere), and Bold, Italic, Bulleted list, Numbered list, Link and Quote carry `aria-pressed` for the format at the cursor. Heading is a menu button (`aria-haspopup="menu"`, `aria-expanded`) opening a `role="menu"` of four `menuitemradio`s, `editor-heading-normal`, `editor-heading-1`, `-2`, `-3`, the level at the cursor `aria-checked`; arrow keys move in it and Escape closes it alone, focus going back to Heading. **Done** (`edit-done`, the default button) sends `files.save_edit { place_id, path, text }`, draws `doc-saved-not-sent` and goes back to the page; **Cancel** (`edit-cancel`) and Escape anywhere in the editor go back without saving, and when the text was changed first open `ConfirmDialog` titled `edit-discard-title` with **Discard** (`edit-discard-verb`) and **Keep editing** (`edit-keep-editing`) (*amended January 2026 (user decision; Approved by the user January 2026)*, replacing **Save**, **Cancel** and the question on the bar). `FILE_CHANGED_MEANWHILE` keeps the editor open with its sentence and the text typed.
    7. *amended January 2026 (user decision; Approved by the user January 2026): the page has no ⋯; **Move to Bin…** is the document row's menu entry of SM-RL-4, which also closes the document's tab on `ok`.* Formerly, in the page's **⋯**: **Move to Bin…** (`doc-move-to-bin`); `ConfirmDialog` `doc-bin-title` / `doc-bin-body`, verbs **Move to Bin** (`doc-bin-verb`) and **Cancel**, then `files.move_to_bin { place_id, path }`; the tab closes on `ok`; a refusal for a place that is not on this computer draws its owner's sentence.
    8. **Send back…**, **Publish a copy…** and a copy's own actions are not drawn in this build (G-CU-3).

18. **SM-NS-5 The workflow screen and its editor** (11 §5.20; screen `workflow:<workflow id>`). Commands: `workflows.get`, `workflows.check`, `workflows.save`, `workflows.run`, `workflows.rename`, `workflows.remove`, `workflows.duplicate`, `workflows.export`, `workflows.restore`, `workflows.discard_draft`, `files.save_export`, `workspaces.list`.
    1. On opening, `workflows.get { workflow_id }`. Drawn top to bottom: `file.name` as heading; `file.purpose`; `file.origin` as text; when `workflow.state = not_saved_yet`, `workflow-imported-band` with **Save to this workspace** (`workflow-save-to-workspace`, `workflows.save` with `change_note` `workflow-imported-note-text`) and **Discard** (`workflow-discard`, `workflows.discard_draft`); **Asks for** (`workflow-asks-for`): each `file.inputs` entry; **Agents** (`workflow-agents`): each `file.agents` entry, marked `workflow-agent-missing` when its `AgentPresence.presence` is `missing` and `workflow-agent-other-version` when `other_version`; **Steps** (`workflow-steps`): `file.steps` as a nested numbered list; **Limits** (`workflow-limits`); **Versions** (`workflow-versions`): each `VersionLine`, `workflow-version-line` with `version`, `saved_at` and `change_note`, with **Restore** (`workflow-restore`) on earlier ones (`workflows.restore`).
    2. Actions at the foot, in this order: **Run…** (`workflow-run`, unavailable with `workflow-run-missing-agent` while an agent is `missing`), **Change** (`workflow-change`), **Duplicate…** (`workflow-duplicate`), **Export…** (`workflow-export`), **Rename…** (`workflow-rename`), **Remove…** (`workflow-remove`). **Run…** sends `workflows.run { workflow_id }`; its `crew_charter_id` opens the crew plan as Ordin Host shows it. **Rename…**: a panel with `workflow-rename-field` (1–80 characters), **Rename** (`workflow-rename-verb`) / **Cancel** (`confirm-cancel`); an empty name is refused in place with `workflow-rename-empty`; `WFF_NAME_DUPLICATE` draws its sentence under the field; `ok` draws `workflow-renamed-from`. **Duplicate…**: a panel with the name preset to `workflow-copy-of`, a workspace choice `workflow-duplicate-to` (this one first, then `workspaces.list`) and **Duplicate** (`workflow-duplicate-verb`), sending `workflows.duplicate { workflow_id, version, name, to_workspace_id }`. **Export…**: `workflow-export-note`, then `workflows.export { workflow_id, version }` and `files.save_export { file_name, text: yaml }`. **Remove…**: `ConfirmDialog` `workflow-remove-title` with `workflow-remove-body`, then `workflows.remove { workflow_id, confirmed: true }` (D11).
    3. **Change** turns the same screen into its editor: each step's line a text field with **Move up**, **Move down**, **Remove step** (`workflow-move-up`, `workflow-move-down`, `workflow-remove-step`); limits with − and +; inputs renamed or removed; then **Save** (`doc-save`) sends `workflows.check { workspace_id, file }`: each `CheckProblem` of `problems` is drawn from its `message_id` with `args` on the step or field `at` names, and nothing is saved; with no problems, `workflows.save { workflow_id, base_version, change_note, file }` with the change note from the field `workflow-change-note` (required), then `workflow-saved-as` with the returned `version`. `notices` are drawn above the steps. **Cancel** (`doc-cancel`) with changes asks on the screen, `doc-discard-question` with **Discard changes** (`doc-discard-verb`) and **Keep changing** (`doc-keep-changing`).
    4. Control blocks, **Show as diagram**, shared workflows and **Share…** are not drawn in this build (G-CU-4).

19. **SM-NS-6 The agent draft** (11 §5.26; screen `agent-draft:<agent draft id>`). Commands: `authoring.agent_draft`, `authoring.update_agent_draft`, `authoring.apply_fix`, `authoring.rehearse`, `authoring.try_it`, `authoring.set_publish_to`, `authoring.publish`, `authoring.send_change`, `authoring.discard`, `catalogue.list_repositories`.
    1. On opening, `authoring.agent_draft` with the draft id. Heading `draft-agent-title` with `draft.name`. Drawn in order: the band by `draft.state` (`draft-band-draft`, `-waiting`, `-changes-asked`, `-approved-not-signed`, `-published`); `reviewer_words` as quoted text under the band; the **Next** bar (`draft-next`) holding the one next action: **Fix [N] checks** (`draft-next-fix`) while a `Finding` of severity failure remains, else **Rehearse** (`draft-next-rehearse`) while `draft.rehearsal_id` is absent, else **Try it here — only you** (`draft-try`) while `can_try` and not `draft.trial`, else **Publish…** (`draft-publish`) while `can_publish`; `draft.purpose`; **Checked by ordin** (`check-checked-by-ordin`) with `draft-checked-line`, the failed `checks` first, each from its `message_id` and `args` with its `fixes` as buttons (**Fix it** `check-fix-it` → `authoring.apply_fix`; **Ask Ordin to fix it** `check-ask-ordin-to-fix`; **Change it yourself** `check-change-it-yourself`), the passed ones folded under `draft-passed` with the count; `tidied` under **ordin tidied N things in the wording** (`draft-tidied`); **What it is** (`draft-what-it-is`): `will`, `wont`, `needs`, `tasks` as lists under `draft-will`, `draft-wont`, `draft-needs`, `draft-tasks`; **Publish to** (`draft-publish-to`) listing `publish_to` by the `display_name` of the matching `catalogue.list_repositories` entry, the choice sending `authoring.set_publish_to`; at the foot **Discard** (`draft-discard`, `ConfirmDialog` `draft-discard-title` / `draft-discard-body`, then `authoring.discard { kind: 'agent', id }`).
    2. **Rehearse** sends `authoring.rehearse { id, workspace_id, situations: [] }`, so the rehearsal runs the situations the Authoring Service adds from the draft's own expectations (AuthoringService.md §I; SA-UI-CU-04) and draws `draft-rehearsal-result` with the number of situations whose `expectations` are all `met` and the number of situations, each situation's `description` opening its `expectations` (`text`, then `draft-expectation-met` or `draft-expectation-not-met` by `outcome`), and its `reviewer_opinion` under `draft-reviewer-opinion`. **Try it here — only you** sends `authoring.try_it { id, workspace_id }`. **Publish…** sends `authoring.publish { kind: 'agent', id }`; the card that follows is the Approval Service's (step 7). With state `changes_asked_for`, **Send the change…** (`draft-send-change`) sends `authoring.send_change`.
    3. **Talk to the agent maker** and `files` (people who code) are not drawn in this build (G-CU-8).

20. **SM-NS-7 Agent repository screens** (11 §5.27).
    1. **Add an agent repository…** is SM-NS-1.1's dialog; the same dialog opens from the agent library.
    2. **Parts of [display name]** (screen `repository-parts:<agent repository id>`; shown only when that repository's `person_can_publish`). Commands: `catalogue.list_repositories`, `catalogue.search`, `catalogue.part`, `catalogue.used_by`, `catalogue.part_history`, `catalogue.vocabulary`, `catalogue.layout`, `catalogue.health`, `authoring.new_part`. Heading `repo-parts-title`; **Agent repository:** (`repo-parts-switch`) among the person's repositories with `person_can_publish`. Tabs **Parts**, **Words**, **Layout**, **Health** (`repo-tab-parts`, `-words`, `-layout`, `-health`); an APM-format repository draws **Parts** only, with `repo-apm-note`. **Parts**: a search box (`repo-parts-search`) over `catalogue.search { repositories: [id], kinds: [], include_not_for_new, text }`, **Show: In use · Not for new agents · Withdrawn** (`repo-show-in-use`, `repo-show-not-for-new`, `repo-show-withdrawn`), one row per `LibraryEntry`: `part.part.name`, the kind in plain words (the `part-kind-*` id that AgentModel.md's kind table gives the part's `PartKind`), and `part.state_label` as `repo-state-current`, `-not-for-new`, `-withdrawn` or `-not-offered`; choosing a row draws `part.summary`, **Read it** (`repo-read-it`, SM-NS-15), **Ask Ordin about this part** (`repo-ask-about-part`), and the folded **Used by** (`repo-used-by`, `catalogue.used_by`) and **History of this part** (`repo-history`, `catalogue.part_history`); **More** (`repo-more`) holds **Withdraw…** (`repo-withdraw`), which opens `withdraw-part:<part id>`. **Words**: `catalogue.vocabulary { id }` listed. **Layout**: `catalogue.layout { id }`, one line per `KindLine` with its kind in plain words and `count`. **Health**: `catalogue.health { id }`, `repo-health-checked` with `computed_at`, one group per `HealthLine.kind` (`repo-health-unused`, `-look-alike`, `-shared-identifier`, `-broken-reference`, `-used-deprecated`, `-used-withdrawn`, `-uncited-term`, `-journey-not-offered`) with its count, then `repo-health-line`. **New part…** (`repo-new-part`) asks the kind and a name and sends `authoring.new_part { agent_repository_id, kind, name, workspace_links_code }`, opening `part-draft:<id>`.
    3. **The part draft** (screen `part-draft:<id>`). Commands: `authoring.part_draft`, `authoring.update_part_draft`, `authoring.publish`, `authoring.discard`. Heading `draft-part-title` with the part's name; the band as SM-NS-6; **Used by** first, `draft-part-used-by` with the count and names of `used_by`; **Checked by ordin** as SM-NS-6; `writing_guide` under **How to write it** (`draft-part-how-to-write`) as text; the question `draft-part-changes-what` with **No, it's a correction** (`draft-part-correction`) and **Yes** (`draft-part-yes`), each sending `authoring.update_part_draft { id, changes_what_part_does, fields }` with `fields` = the draft's `draft.fields` unchanged; **Publish…** and **Discard** as SM-NS-6. The part's own form (`PartDraftView.form`) is not drawn in this build (G-CU-2).
    4. **Withdraw [part]** (screen `withdraw-part:<part id>`). Commands: `catalogue.part`, `catalogue.used_by`, `catalogue.search`, `authoring.withdraw`. Drawn in order: heading `withdraw-title`; `withdraw-used-by` with the agents of `catalogue.used_by`; **Why is it withdrawn?** (`withdraw-reason`, required, 1–1,000 characters); **Replace it with** (`withdraw-replace`, optional, parts of the same kind from `catalogue.search`); **Withdraw…** (`withdraw-verb`, never the default button) sends `authoring.withdraw { part, reason, replacement }`; its `PartDraftView` opens `part-draft:<draft.id>`, whose **Publish…** raises the card.

21. **SM-NS-8 The handover screen** (11 §5.28; screen `handover:<handover id>`, title `screen-handover`). Commands: `handover.open`, `handover.accept_and_start`, `handover.return_options`, `handover.return_to`, `handover.take_back`, `handover.remind`, `workspaces.list`.
    1. On opening, `handover.open { id }`. Heading `handover-screen-title` with `handover.title`. Drawn in order: `handover-screen-from` with `handover.sender` and the time of the first `handover.events` entry; `handover-screen-stage` with `handover.stage_name` when present; `handover-screen-arrived` with `arrived[0]` and `arrived[1]` when present; the note: each `handover.note.sections` entry as its `heading` and `body` (text, paragraphs split on blank lines), `handover-drafted-by-ordin` under a section with `drafted_by_ordin`; each `marked_lines` entry repeated with `handover-line-instruction` under it; **What's handed over** (`handover-results`): each `ResultLine` as its `label`, with `cannot_open` drawn as text when present; **Checked by [name]'s ordin** (`handover-checked-by`): `handover.checks_passed` read only; the Slack lines of the January 2026 Slack amendment.
    2. Buttons from `actions`, in this order: `accept_and_start` → **Accept and start** (`handover-accept`), `return_to` → **Return to [name]…** (`handover-return`), `ask_ordin` → **Ask Ordin about this** (`block-ask-about-this`), `close` → **Close** (`card-close`). **Accept and start** asks `handover-accept-question` with **New workspace** (`handover-accept-new`, named from the title) and the person's workspaces, then sends `handover.accept_and_start { id, into }`; the returned workspace id opens. `HAND_ALREADY_ACCEPTED` draws its sentence. **Return to [name]…** opens a form: `handover-return-why` (required), the choices of `handover.return_options { id }` each as `handover-return-option` with `person.name`, `stage` and `round`, then `handover.return_to { id, reason, to }`.
    3. The sender's side: when `actions` holds neither `accept_and_start` nor `return_to` and `handover.state` is `sent` or `seen` (SA-UI-CU-06), the screen draws `handover-state-<state>` with `handover.receiver.name`, **Take it back…** (`handover-take-back`, `ConfirmDialog` `handover-take-back-title`, verbs **Take it back** (`handover-take-back-verb`) and **Cancel**) → `handover.take_back { id }`, and **Remind [name]…** (`handover-remind`) → `handover.remind { id }`; a refusal of either draws its owner's sentence.

22. **SM-NS-9 The lifecycle screen** (11 §5.28; screen `lifecycle:<workspace id>`, opened by **Show the lifecycle**). Commands: `handover.stage_of`, `handover.lifecycle_view`. `handover.stage_of { workspace }` returns the `StageView` the workspace follows (`null` → the screen draws `lifecycle-none` only); `handover.lifecycle_view { id: stage.lifecycle.id }` returns the `LifecycleView` (HandoverService.md `lifecycle_view(id: LifecycleId)`). Heading `lifecycle-title` with `lifecycle.name`; `lifecycle-readable-note`; the stages as a path in order, each with its number, `stage.name`, `stage.role`, `lifecycle-handover-count` with the number of `handovers`, and `lifecycle-you-are-here` on the stage with `you_are_here`; then, per stage, `lifecycle-stage-agent` with `agent` when present, `lifecycle-stage-hands-over` with `hands_over` when present, and its `HandoverLine`s (`title`, `other_person`, the state words of `handover-state-<state>`), each opening `handover:<id>`. The draft of a lifecycle and **Show as diagram** are not drawn in this build (G-CU-4).

23. **SM-NS-10 The import screen** (11 §5.30; screen `import`). Commands: `authoring.import`, `catalogue.list_repositories`, `app.pick_folder`. Drawn in order: heading `import-title`; `import-what-ordin-reads`; **What are you bringing in?** (`import-what`) with **An agent** (`import-agent`) and **One part** (`import-part`), the latter adding **What kind of part is it?** (`import-part-kind`), a choice among the `PartKind` values in plain words (the `part-kind-*` ids of AgentModel.md), sent as `as_part`; **Which agent repository is it for?** (`import-which-repository`) among `catalogue.list_repositories` entries with `person_can_publish`; **Choose a folder…** (`import-choose-folder`, `app.pick_folder`), the chosen folder drawn by its last segment; **Import** (`import-verb`) sends `authoring.import { agent_repository_id, source: <folder>, workspace_id, as_part }` with `as_part` absent for an agent; the `DraftRef` opens `agent-draft:<id>` or `part-draft:<id>`. A file (an APM package) as the source is not offered in this build (G-CU-10).

24. **SM-NS-11 Show the installed files** (11 §5.29; screen `installed-files:<agent instance id>`; offered on *What [agent] is made of* in a workspace that links code only). Command: `agents.installed_files { agent_instance_id }`. Heading `installed-files-title` with that name; `installed-files-note`; one line per `InstalledFileEntry`, grouped by folder (the path up to its last `/`), each with `path`, `size` formatted by `lib/format/intl.ts` and the first 12 characters of `sha256`, in the order the reply gives within each folder. The tab title is the agent's name, passed by *What [agent] is made of*, which holds it (`InstalledMadeOf.display_name`). Read only; no line opens anything in this build. `INST_NOT_INSTALLED` draws its sentence. *Amended (amended January 2026; user decision; Approved by the user January 2026):* the screen is titled `installed-files-title` ("What { $agent } is made of") and is a folder tree beside a reader (the reader goes under the tree when the pane is narrower than 40 rem). The tree (`installed-files-tree`, "The files of { $agent }") is built from the paths of `agents.installed_files`; every folder starts open, and two icon buttons **Open all folders** (`installed-files-open-all`) and **Close all folders** (`installed-files-close-all`) sit above it. Choosing a file sends `agents.installed_file { agent_instance_id, path }` (ServiceApi.md SVC-IF-1); until one is chosen the reader shows `installed-files-choose`. The reader shows the path as its heading, an icon button **Copy the text** (`installed-files-copy`, reading `installed-files-copied` for a moment after copying), then the text: a `.md` file is drawn formatted by the read-only Markdown view, any other file as it is in monospace. `cut = true` draws `installed-files-cut` ("Only the start of this file is shown."; the file is cut at 100 KiB). **Details** (`installed-files-details`, a closed disclosure) holds the size and the first 12 characters of `sha256`. A refusal of `agents.installed_file` is `ErrorLine` with **Try again**. Read only; nothing here changes the agent.

25. **SM-NS-12 Email screens** (11 §5.22; corrects `lib/views/fixed/register.ts`'s note that no command names a mail link: `workspaces.email_links` returns `EmailLink.display_name`).
    1. **Email list** (screen `email:<email link id>`). Commands: `workspaces.email_links`, `mail.list_messages`, `mail.mark_looked`. On opening, `mail.mark_looked { link: <email link id> }`. Heading: the link's `display_name`; `email-link-note`; one row per `MessageSummary`, newest first, 50 a page with **Show more** while `next_cursor` is present: `from.name` (else `from.address`), **Outside the company** (`approval-outside-company`, word and icon) when `from.outside_company`, `subject`, `date`, `attachment_names` joined; a row opens `email-message:<email link id>:<message_ref>`. Empty → `email-list-empty`.
    2. **Email message** (screen `email-message:…`). Commands: `mail.open_message`, `mail.save_attachment`, `ordin.send_message`. Drawn in order: `subject` as heading; `block-email-from` with `from` (and **Outside the company** when it applies), `block-email-to` with `to`, `email-cc` with `cc` when not empty, `date`; `email-hidden-text` when `hidden_text`; `text` as paragraphs; `email-cut` when `truncated`; each `attachments` entry with `file_name` and **Save attachment** (`block-save-attachment`, `mail.save_attachment { workspace_id, message_ref, index }`, then `block-file-saved`); **Reply with Ordin** (`block-reply-with-ordin`, the conversation message `block-reply-email-text` with the sender's name, and `block-reply-reference` with `message_ref`); **Ask Ordin about this** (`block-ask-about-this`).
    3. **Email draft** (screen `email-draft:<draft id>`; the `email` block's draft, in full). Commands: `mail.get_draft`, `mail.edit_draft`, `mail.send`, `mail.save_to_mail_drafts`, `mail.discard_draft`. Drawn as the `email` block draws a draft (step 6) with `block-email-drafted-by` / `block-email-drafted-by-an-agent`, **Send…** (`block-email-send`, `mail.send { id }`, which raises the card), **Edit** (`block-email-edit`, a multi-line plain text box over `body` with To and Subject above it — *amended January 2026 (user decision; Approved by the user January 2026): an email body is plain text, not Markdown, so it keeps a plain text box rather than the what-you-see editor of SM-NS-4.6*; **Save** sending `mail.edit_draft`), **Save to mail drafts** (`block-email-save-to-drafts`, then `block-email-saved-to-drafts`) and **Discard** (`block-email-discard`, `ConfirmDialog` `block-email-discard-title` with `email-discard-body`). By `state`: `sent` → `email-sent`; `not_sure_it_was_sent` → `email-not-sure`; `not_sent` → `failure_reason` as text. `MAIL_DRAFT_STATE_INVALID` draws its sentence.

26. **SM-NS-13 Adopt, Describe this folder and the recall check** are specified in the Team knowledge amendment of January 2026 (ADR-110), *Internal Logic* steps 4, 6 and 7, with their ids in its message table; nothing is added but the Fluent argument names, which that table wrote as `[…]`: `team-adopt` and `team-adopt-title` take `folder` (the own folder's name); `team-adopt-from` takes `folder` and `name`; `team-adopted-from` and `team-from-folder` take `folder`; `team-read-dialog-intro` takes `folder`; `team-stop-reading-confirm` takes `folder`; `team-folder-counts` takes `facts` and `pages`; `team-folder-no-access`, `team-folder-review-not-required` and `team-folder-review-cannot-tell` take `name`; `team-item-elsewhere` takes `file`; `team-recall-result` takes `folder`, `percent` and `checks`. Commands, all seen: `team_knowledge.adopt`, `team_knowledge.adopt_again`, `team_knowledge.keep_as_is`, `team_knowledge.set_description`, `team_knowledge.check_recall`. *amended January 2026 (user decision; Approved by the user January 2026):* in **Adopt into [own folder]…** for a page, the body is written in the what-you-see editor of SM-NS-4.6 (the title stays a one-line field and a fact's statement a plain text box); **Adopt** sends the same command with `edit.body` as the editor gives it. *amended January 2026 (user decision; Approved by the user January 2026):* **Adopt** is the dialog's Done; **Cancel** and Escape close it without adopting, asking `edit-discard-title` (**Discard** / **Keep editing**, `ConfirmDialog`) first when the text was changed.

27. **SM-NS-14 A hosted agent's memory section** (ADR-111 amendment step 9) is **not buildable**: it is reached through the Caller API's operator route (`POST /operator/memory/<verb>`), and no Service API command in `commands.gen.ts` carries it (the `hosted` group holds `hosted.decide_card` only). It stays as that amendment states it, and is not drawn (G-CU-1).

28. **SM-NS-15 Part pages for Check it** (11 §5.29, §5.30; screen `part-page:…`).
    1. An installed agent's part (`part-page:<agent instance id>:<part id>`). Command: `agents.part_page { agent_instance_id, part_id, detail }` with `detail` = `links_code` in a workspace that links code, else `business_user`. Drawn: `line.name` as heading; the kind from `plain_kind_message_id`; `part-page-from` with `line.from` when present, else `part-page-from-ordin`; `line.what_it_does`; the part's `text` as paragraphs split on blank lines, read only, never as instructions to the reader; `file` under **Details** (`part-page-details`) in a workspace that links code; **Ask Ordin about this part** (`repo-ask-about-part`); the **Check it** panel of step 9 (`CheckItPanel`, `checker.check_installed { agent_instance_id }`).
    2. A library part (`part-page:library:<part id>`). Commands: `catalogue.part`, `catalogue.used_by`. Drawn: `part.name`; the kind in plain words (the `part-kind-*` id of AgentModel.md's kind table); `part-page-from` with `from`; `summary`; **Used by** (`part-page-used-by`): each agent of `catalogue.used_by` by `part.name`; the **Check it** panel with `checker.check_library_item`. The part's text laid out by kind (11 §5.29) is not drawn in this build, because `catalogue.read_part` returns `body` with no generated shape (G-CU-2).

#### New messages

New messages (en-GB; *chosen here* unless a source is named; a source "11 §…" means the HLD quotes the words; `{ $… }` are Fluent arguments; `a` / `-b` rows name ids that share the first id's stem, as earlier tables do). Every id below is new to this file and to the en-GB `.ftl` files; the Patch that follows adds each to the `.ftl` file of its area.

| Id | en-GB text | Source |
|---|---|---|
| `home-nothing-waiting` | Nothing is waiting for you | 11 §5.17 ("each section says so in one line") |
| `home-nothing-since` | Nothing new since you last looked | 11 §5.17 |
| `home-group-hosted-agents` | Hosted agents | 11 §4 (ADR-097 item 9) |
| `home-group-no-workspace` | Not in a workspace | |
| `search-result-count` | { $count } result / { $count } results (Fluent plural) | 11 §5.17 ("The number of results is announced as it changes") |
| `search-no-results` | No results | |
| `server-paste-line-hint` | Or paste the one line you were given here and leave the code empty. | 11 §5.31 |
| `server-update-self-unavailable` | This server can't update itself. Whoever runs it installs new versions. | SetupAssistant.md SET-UP-14 |
| `settings-slack-rule-kind-handover-notices` / `settings-slack-rule-kind-ordin-answers` | Handover notices / Ordin's answers | 11 §5.25a |
| `settings-slack-rule-state-on` / `settings-slack-rule-state-off` | on / off | |
| `memory-page-gone` | From a page that is no longer here | |
| `settings-set-by-company` | Set by your company | 11 §4, §5.18 |
| `settings-repos-locked` | Can't be removed | plan 05 Phase 9 |
| `settings-repos-format-ordin` / `settings-repos-format-apm` | ordin format / APM format | 11 §4 |
| `settings-repos-agent-count` | 1 agent / { $count } agents (Fluent plural) | |
| `settings-repos-show-parts` | Show its parts | 11 §4 |
| `settings-repos-rename` | Rename… | |
| `settings-repos-rename-field` | New name for this agent repository | |
| `settings-repos-rename-verb` | Rename | |
| `settings-repos-remove` | Remove… | |
| `settings-repos-remove-title` | Remove { $name }? | |
| `settings-repos-remove-body` | ordin stops reading this agent repository. You can add it again later. | |
| `settings-repos-remove-verb` | Remove | |
| `settings-repos-add` | Add an agent repository… | 11 §4 |
| `settings-repos-add-address` | The agent repository's web address | |
| `settings-repos-add-look` | Look it up | |
| `settings-repos-add-found` | 1 agent found · { $format } / { $count } agents found · { $format } (Fluent plural) | |
| `settings-repos-add-name` | Name shown in ordin | |
| `settings-repos-add-cannot-publish` | You can add agents from it, but you can't publish to it. | |
| `settings-repos-add-verb` | Add | |
| `settings-email-fixed` | Every email is shown to you before it is sent. This can't be turned off. | 11 §5.22 |
| `settings-email-mailbox` | Your mailbox | 11 §5.22 |
| `settings-email-signed-in-password` | Signed in with your password, kept in this computer's keychain | 11 §5.22 |
| `settings-email-signed-in-account` | Signed in with your company account | 11 §5.22 |
| `settings-email-change-password` | Change password | 11 §5.22 |
| `settings-email-sign-out` | Sign out of email | 11 §5.22 |
| `settings-email-check-refused` | Your mail server didn't accept this password. Check it and try again. | 11 §5.22 |
| `settings-email-check-not-secure` | Your company's mail server can't be reached securely, so ordin didn't connect. Try again later. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-FS-9) | 11 §5.22 |
| `settings-email-add-signature` | Add my signature | 11 §5.22 |
| `settings-email-signature` | Your signature | |
| `settings-email-foot-line` | Say at the bottom that an agent drafted it | 11 §5.22 |
| `settings-email-foot-line-required` | Required by your company | 11 §5.22 |
| `settings-email-check` | Check for new email | 11 §5.22 |
| `settings-email-check-every-15` / `settings-email-check-on-open` | Every 15 minutes / Only when I open the workspace | 11 §5.22 |
| `settings-email-workspaces` | Email in your workspaces | 11 §5.22 |
| `settings-telegram-fixed` | Every post is shown to you before it is posted. This can't be turned off. | 11 §5.25 |
| `settings-telegram-not-allowed` | Your company doesn't allow Telegram here. | |
| `settings-telegram-tools-off` | Telegram isn't available here. | |
| `settings-telegram-no-app` | Telegram isn't on this computer. Install it to link Telegram groups here. | 11 §5.25 |
| `settings-telegram-no-bot` | No Telegram bot is set up for ordin. | |
| `settings-telegram-bot` | Your bot: { $name } (@{ $username }) | |
| `settings-telegram-admin-note` | Your bot sees only messages addressed to it in groups where it isn't an administrator. | 11 §5.25 |
| `settings-telegram-remove` | Remove Telegram | 11 §5.25 |
| `settings-telegram-set-up` | Set up Telegram | 11 §5.25 |
| `settings-telegram-key` | Paste the key BotFather sent you | 11 §5.25 |
| `settings-telegram-connect` | Connect | 11 §5.25 |
| `settings-dictation-available` | Dictation is ready in { $language }. | |
| `settings-dictation-needs-assets` | Dictation gets ready the first time you use it. | |
| `settings-voice-fixed` | Ordin always uses plain words and keeps it short. Choose how it sounds. | 11 §5.19 |
| `settings-voice-tone` | Tone | 11 §5.19 |
| `settings-voice-tone-friendly` / `-neutral` / `-warm` / `-direct` | Friendly / Neutral / Warm / Direct | 11 §5.19 |
| `settings-voice-formality` | Formality | 11 §5.19 |
| `settings-voice-formality-everyday` / `-formal` | Everyday / Formal | 11 §5.19 |
| `settings-voice-length` | Length | 11 §5.19 |
| `settings-voice-length-brief` / `-shortest` | Brief / Shortest | 11 §5.19 |
| `settings-voice-call-me` | Call me | 11 §5.19 |
| `settings-voice-notes` | Notes for Ordin | 11 §5.19 |
| `settings-voice-notes-example` | I'm new to procurement; explain a term the first time you use it | 11 §5.19 |
| `settings-voice-notes-limit` | Notes change how Ordin explains, never what it may do | 11 §5.19 |
| `settings-voice-company-default` | Your company's default | 11 §5.19 |
| `settings-plan-today` | Today | 11 §5.18 |
| `settings-plan-today-line` | Today you've used about { $percent } of a typical day's plan. | 11 §5.18 (as a percentage) |
| `settings-plan-details` | Details | 11 §5.18 |
| `settings-plan-today-details` | { $turns } model turns · { $percent } on smaller models · { $answered } answered by ordin without the model | 11 §5.18 |
| `settings-plan-choice` | How Ordin uses your plan | 11 §5.18 |
| `settings-plan-choice-save` / `-balanced` / `-best` | Save my plan / Balanced / Best results | 11 §5.18 |
| `settings-plan-choice-save-line` | Smaller models wherever they do the job; asks before medium or heavy work | 11 §5.18 |
| `settings-plan-choice-balanced-line` | Ordin picks the model for each step; the most capable only where it matters | 11 §5.18 |
| `settings-plan-choice-best-line` | The most capable models for talking, planning and writing | 11 §5.18 |
| `settings-plan-choice-custom` | Custom | 11 §5.18 |
| `settings-plan-without-model` | Answer simple requests without the model | 11 §5.18 |
| `settings-plan-reuse` | Reuse an answer when nothing has changed | 11 §5.18 |
| `settings-plan-only-needed` | Give agents only the parts they need | 11 §5.18 |
| `settings-plan-continue-after-break` | After a break of more than an hour, continue from a summary | 11 §5.18 |
| `settings-plan-summarise` | Summarise long conversations | 11 §5.18 |
| `settings-plan-summarise-early` / `-nearly-full` / `-only-asked` | Early / When the conversation is nearly full / Only when I ask | |
| `settings-plan-ask-before` | Ask before work estimated as | 11 §5.18 |
| `settings-plan-ask-heavy` / `-medium-or-heavy` / `-never` | Heavy / Medium or heavy / Never ask | |
| `settings-plan-agents-at-once` | Agents working at the same time | 11 §5.18 |
| `settings-plan-more` | More choices | 11 §5.18 |
| `settings-plan-show-usage` | Show usage on each run | 11 §5.18 |
| `settings-plan-thinking` | How much Ordin thinks first | |
| `settings-plan-thinking-low` / `-automatic` / `-high` | Less / Ordin decides / More | |
| `settings-notify-intro` | ordin tells you outside its window only about these. | 11 §7 |
| `settings-notify-approvals` | Approvals waiting for you | 11 §7 |
| `settings-notify-finished-work` | Finished tasks and crews | 11 §7 |
| `settings-notify-questions` | Questions that stop work until you answer | 11 §7 |
| `settings-notify-draft-reviews` | Reviews of your drafts on GitHub | 11 §7 |
| `settings-notify-handovers` | Handovers to you and returned to you | 11 §7 |
| `settings-code-open-in` | Open code in | 11 §4 (UX-A25) |
| `settings-code-no-ide` | No IDE was found on this computer. | |
| `settings-code-languages` | Code checking by language | 11 §4 (UX-A26) |
| `settings-code-pinned` | version { $version } | 11 UX-A26 |
| `settings-code-from-company` | From your company | 11 UX-A26 |
| `settings-code-from-agent` | From an agent | 11 UX-A26 |
| `settings-code-on` | On | 11 UX-A26 |
| `settings-code-restart` | Restart | 11 UX-A26 |
| `settings-memory-intro` … `settings-memory-say-which-hint` | the 27 messages of SM-MS-5 (amended January 2026; user decision; Approved by the user January 2026) | 11 §4 |
| `signin-kind-github` / `-gitlab` / `-slack` / `-confluence` / `-microsoft` / `-google` / `-jira` | GitHub / GitLab / Slack / Confluence / Microsoft / Google / Jira | 11 §4 (product names, never translated) |
| `signin-kind-oidc` | Your company account | |
| `signin-kind-mail-server-single-sign-on` / `signin-kind-mail-server-password` / `signin-kind-mail-server-app-password` | Company email / Company email / Company email | 11 §5.22 |
| `signin-state-signed-in` | Signed in | 11 §4 |
| `signin-state-needs-sign-in` | Sign in again to keep using it | |
| `signin-state-signed-out` | Asked for the first time Ordin needs it | 11 §4 |
| `signin-expires` | until { $date } | |
| `signin-sign-in` | Sign in | 11 §4 |
| `signin-sign-out` | Sign out | |
| `signin-browser-opened` | Finish signing in in your browser. | |
| `signin-device-code` | Open { $address } and enter this code: | IdentityAndCredentials.md (`DeviceCode`) |
| `signin-copy-code` | Copy the code | |
| `signin-mail-title` | Sign in to your company's email | 11 §5.22 |
| `signin-mail-password-note` | Your company's mail server doesn't use a browser sign-in, so this is the one password ordin asks for. It goes only to your company's mail server and is kept in this computer's keychain. | 11 §5.22, with "[server]" written as "your company's mail server" (SA-UI-CU-03) |
| `signin-mail-address` | Email address | 11 §5.22 |
| `signin-mail-address-hint` | Filled in from your company account. Change it only if you use another address for email. | 11 §5.22 |
| `signin-mail-app-password` / `signin-mail-password` | App password / Password | 11 §5.22 |
| `signin-show` | Show | 11 §5.22 |
| `signin-mail-keychain-note` | ordin keeps this in this computer's keychain and uses it only to reach your company's mail server. | 11 §5.22 |
| `signin-certificate-imported` | Certificate { $name } imported. It's valid until { $date }. | |
| `diagnostics-journal-title` | Journal events | |
| `diagnostics-window-title` | This window | step 12 |
| `diagnostics-support-export` | Save logs for support… | 00-overview *Support export* |
| `diagnostics-support-pick` | Choose up to 20 runs to include. | ServiceApi.md (`SupportExport.run_ids`, at most 20) |
| `diagnostics-support-limit` | You can include up to 20 runs. | |
| `diagnostics-support-note` | The file holds the chosen runs' records and the last 3 days of ordin's logs. It holds no document or conversation text. | 08-interfaces *Support export*; 07 *Logging*; OrdinDesktop.md DSK-LOG-5; amended January 2026 (user decision; Approved by the user January 2026): the logs are in the file, and the line no longer says what the file does not hold, because a log line is not shown to hold no document or conversation text |
| `diagnostics-support-save` | Save… | |
| `diagnostics-support-saved` | Saved to { $path } | |
| `diagnostics-clear` | Clear logs… | SM-NS-3a; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-clear-title` | Clear ordin's logs? | SM-NS-3a; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-clear-body` | This deletes ordin's log files and empties the list of events below. A file saved for support still includes earlier events. (amended January 2026; user decision; Approved by the user January 2026) | SM-NS-3a; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-clear-verb` | Clear logs | SM-NS-3a; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-clear-done` | Logs cleared. | SM-NS-3a; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-journal-none` | No events to show. | SM-NS-3; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-col-time` | Time | SM-NS-3; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-col-level` | Level | SM-NS-3; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-col-summary` | What happened | SM-NS-3; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-col-details` | Details | SM-NS-3; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-line-more` | Show the details | SM-NS-3; amended January 2026 (user decision; Approved by the user January 2026) |
| `diagnostics-line-less` | Hide the details | SM-NS-3; amended January 2026 (user decision; Approved by the user January 2026) |
| `screen-handover` | Handover | |
| `doc-glance` | Changed { $date } · { $who } | 11 §5.6 |
| `doc-origin-person` / `doc-origin-agent` / `doc-origin-app` | saved by you / changed by an agent / changed in { $app } | |
| `doc-open-in-app` | Open in its app | 11 §5.6 |
| `doc-no-editor` | No editor for this kind of file — open it in your usual app? | 11 §5.6 |
| `doc-open-usual-app` | Open in your usual app | 11 §5.6 |
| `doc-cut` | Only the start of this document is shown. | |
| `doc-versions` | Versions | 11 §5.6 |
| `doc-version-line` | Version { $version } · { $date } · { $who } | |
| `doc-restore` | Restore | |
| `doc-restore-title` | Go back to version { $version }? | |
| `doc-restore-body` | The document goes back to this version. | |
| `doc-published-as` | Published as ({ $count }) | 11 §5.6 |
| `doc-copy-kept` | Kept up to date | 11 §5.6 |
| `doc-edit` / `doc-save` / `doc-cancel` | Edit / Save / Cancel | 11 §5.6 |
| `editor-toolbar` (`$name`) / `editor-heading` / `-bold` / `-italic` / `-bullets` / `-numbers` / `-link` / `-quote` / `-table` / `-undo` / `-redo` | Formatting for { $name } / Heading / Bold / Italic / Bulleted list / Numbered list / Link / Quote / Table / Undo / Redo | 11 §5.33; amended January 2026 (user decision; Approved by the user January 2026), replacing `doc-tool-*` |
| `editor-placeholder` / `editor-kept` | Start writing… / Kept as it was written — this part can't be changed here | 11 §5.33; amended January 2026 (user decision; Approved by the user January 2026) |
| `editor-link-address` / `-apply` / `-remove` / `-cancel` / `-invalid` | Link address / Apply / Remove link / Cancel / Use a web address (https://…) or an email address. | 11 §5.33; amended January 2026 (user decision; Approved by the user January 2026) |
| `doc-saved-not-sent` | Changed by you — not yet sent back | 11 §5.6 |
| `doc-discard-question` | Discard your changes? | 11 §5.20 |
| `doc-discard-verb` / `doc-keep-changing` | Discard changes / Keep changing | 11 §5.20 |
| `doc-move-to-bin` | Move to Bin… | 11 §5.6 |
| `doc-bin-title` | Move “{ $name }” to the Bin? | 11 §5.6 |
| `doc-bin-body` | The file goes to your computer's own Bin (or Trash), not to the Bin in this app, which lists workspaces only. You can undo it straight afterwards in the tree. | 11 §5.6 |
| `doc-bin-verb` | Move to Bin | 11 §5.6 |
| `workflow-imported-band` | Imported — not saved yet | 11 §5.20 |
| `workflow-save-to-workspace` / `workflow-discard` | Save to this workspace / Discard | 11 §5.20 |
| `workflow-imported-note-text` | Imported from a file | 11 §5.20 |
| `workflow-asks-for` / `workflow-agents` / `workflow-steps` / `workflow-limits` / `workflow-versions` | Asks for / Agents / Steps / Limits / Versions | 11 §5.20 |
| `workflow-agent-missing` | Not in this workspace | 11 §5.20 |
| `workflow-agent-other-version` | Another version is installed here | |
| `workflow-version-line` | Version { $version } · { $date } · { $note } | 11 §5.20 |
| `workflow-restore` | Restore | 11 §5.20 |
| `workflow-run` / `workflow-change` / `workflow-duplicate` / `workflow-export` / `workflow-rename` / `workflow-remove` | Run… / Change / Duplicate… / Export… / Rename… / Remove… | 11 §5.20 |
| `workflow-run-missing-agent` | An agent this workflow uses isn't in this workspace. | 11 §5.20 |
| `workflow-rename-field` | New name for the workflow | 11 §5.20 |
| `workflow-rename-verb` | Rename | 11 §5.20 |
| `workflow-rename-empty` | Give the workflow a name. | 11 §5.20 |
| `workflow-renamed-from` | Renamed from “{ $name }”. | 11 §5.20 |
| `workflow-copy-of` | Copy of { $name } | 11 §5.20 |
| `workflow-duplicate-to` | Workspace | |
| `workflow-duplicate-verb` | Duplicate | |
| `workflow-export-note` | The file holds the steps, the agents' names and the limits. It holds no documents, links, folder access or history. | 11 §5.20 |
| `workflow-remove-title` | Remove the workflow “{ $name }”? | 11 §5.20 |
| `workflow-remove-body` | The workflow and its versions on this computer are removed. Its agents, its past runs in history and any copy already shared stay. | 11 §5.20 |
| `workflow-move-up` / `workflow-move-down` / `workflow-remove-step` | Move up / Move down / Remove step | 11 §5.20 |
| `workflow-change-note` | What changed | |
| `workflow-saved-as` | Saved as version { $version } | 11 §5.20 |
| `draft-agent-title` | Agent draft: { $name } | 11 §5.26 |
| `draft-band-draft` / `-waiting` / `-changes-asked` / `-approved-not-signed` / `-published` | Draft — only you can see it / Waiting for review on GitHub / Changes asked for / Approved on GitHub — not signed yet / Published | 11 §5.26 |
| `draft-next` | Next | 11 §5.26 |
| `draft-next-fix` | Fix 1 check / Fix { $count } checks (Fluent plural) | 11 §5.26 |
| `draft-next-rehearse` | Rehearse | 11 §5.26 |
| `draft-try` | Try it here — only you | 11 §5.26 |
| `draft-publish` | Publish… | 11 §5.26 |
| `draft-checked-line` | Fixed rules, the same answer every time. No agent decides these. | 11 §5.26 |
| `draft-passed` | { $count } passed | 11 §5.26 |
| `draft-tidied` | ordin tidied { $count } things in the wording | 11 §5.26 |
| `draft-what-it-is` | What it is | 11 §5.26 |
| `draft-will` / `draft-wont` / `draft-needs` / `draft-tasks` | It will / It won't / It needs / Its tasks | 11 §5.26, §5.29 |
| `draft-publish-to` | Publish to | 11 §5.26 |
| `draft-discard` | Discard | 11 §5.26 |
| `draft-discard-title` | Discard the draft “{ $name }”? | 11 §5.26 |
| `draft-discard-body` | The draft and its rehearsal results are deleted. Nothing was published, so nobody else is affected. | 11 §5.26 |
| `draft-rehearsal-result` | { $passed } of { $total } situations passed | 11 §5.26 |
| `draft-expectation-met` / `draft-expectation-not-met` | Passed / Failed | 11 §5.26 |
| `draft-reviewer-opinion` | Reviewing agent's opinion — from a model, so it passes or fails nothing | 11 §5.26 |
| `draft-send-change` | Send the change… | 11 §5.26 |
| `draft-part-title` | Part draft: { $name } | 11 §5.27 |
| `draft-part-used-by` | Changing it changes { $count } agents once it's published: { $names } | 11 §5.27 |
| `draft-part-how-to-write` | How to write it | 11 §5.27 |
| `draft-part-changes-what` | Does this change what the part does? | 11 §5.27 |
| `draft-part-correction` / `draft-part-yes` | No, it's a correction / Yes | 11 §5.27 |
| `repo-parts-title` | Parts of { $name } | 11 §5.27 |
| `repo-parts-switch` | Agent repository: | 11 §5.27 |
| `repo-tab-parts` / `-words` / `-layout` / `-health` | Parts / Words / Layout / Health | 11 §5.27 |
| `repo-apm-note` | { $name } is in APM format, so ordin can withdraw parts from it but not change them. | 11 §5.27 |
| `repo-parts-search` | Search the parts | |
| `repo-show-in-use` / `repo-show-not-for-new` / `repo-show-withdrawn` | In use / Not for new agents / Withdrawn | 11 §5.27 |
| `repo-state-current` / `-not-for-new` / `-withdrawn` / `-not-offered` | In use / Not for new agents / Withdrawn / Read, not offered | 11 §5.27 |
| `repo-read-it` | Read it | 11 §5.27 |
| `repo-ask-about-part` | Ask Ordin about this part | 11 §5.27 |
| `repo-used-by` | Used by | 11 §5.27 |
| `repo-history` | History of this part | 11 §5.27 |
| `repo-more` | More | 11 §5.27 |
| `repo-withdraw` | Withdraw… | 11 §5.27 |
| `repo-new-part` | New part… | 11 §5.27 |
| `repo-health-checked` | Checked { $time }, after the last update. | 11 §5.27 |
| `repo-health-unused` | Not used by any agent | 11 §5.27 |
| `repo-health-look-alike` | Look alike | 11 §5.27 |
| `repo-health-shared-identifier` | Same name in two repositories | 11 §5.27 |
| `repo-health-broken-reference` | Uses a part that can't be found | 11 §5.27 |
| `repo-health-used-deprecated` | Used while not for new agents | 11 §5.27 |
| `repo-health-used-withdrawn` | Used while withdrawn | 11 §5.27 |
| `repo-health-uncited-term` | Words no part uses | 11 §5.27 |
| `repo-health-journey-not-offered` | Lifecycles not offered | |
| `repo-health-line` | Health suggests; it decides nothing. | 11 §5.27 |
| `withdraw-title` | Withdraw { $name } | 11 §5.27 |
| `withdraw-used-by` | These agents stop on every computer that uses them once this is published: | 11 §5.27 |
| `withdraw-reason` | Why is it withdrawn? | 11 §5.27 |
| `withdraw-replace` | Replace it with | 11 §5.27 |
| `withdraw-verb` | Withdraw… | 11 §5.27 |
| `handover-screen-title` | Handed to you: { $title } | 11 §5.28 |
| `handover-screen-from` | From { $name }, { $time } | 11 §5.28 |
| `handover-screen-stage` | Stage: { $stage } | |
| `handover-screen-arrived` | { $done } of { $total } handed over. | 11 §5.28 |
| `handover-drafted-by-ordin` | Drafted by Ordin | 11 §5.28 |
| `handover-line-instruction` | This line asks an agent to do something. Agents won't follow it. | 11 §5.28 |
| `handover-results` | What's handed over | 11 §5.28 |
| `handover-checked-by` | Checked by { $name }'s ordin | 11 §5.28 |
| `handover-accept` | Accept and start | 11 §5.28 |
| `handover-accept-question` | Start a new workspace for this, or use one you have? | 11 §5.28 |
| `handover-accept-new` | New workspace | 11 §5.28 |
| `handover-return` | Return to { $name }… | 11 §5.28 |
| `handover-return-why` | Why are you returning it? | 11 §5.28 |
| `handover-return-option` | { $name } — { $stage } (round { $round }) | 11 §5.28 |
| `handover-take-back` | Take it back… | 11 §5.28 |
| `handover-take-back-title` | Take back the handover to { $name }? | |
| `handover-take-back-verb` | Take it back | |
| `handover-remind` | Remind { $name }… | 11 §5.28 |
| `handover-state-draft` | Draft | 11 §5.28 |
| `handover-state-sent` / `-seen` / `-accepted` / `-sent-back` | Waiting for { $name } / Seen by { $name } / Accepted by { $name } / Returned by { $name } | 11 §5.28 |
| `handover-state-taken-back` | Taken back | |
| `lifecycle-title` | Lifecycle: { $name } | 11 §5.28 |
| `lifecycle-readable-note` | Shows the handovers in places you can read. | 11 §5.28 |
| `lifecycle-handover-count` | 1 handover / { $count } handovers (Fluent plural) | 11 §5.28 |
| `lifecycle-you-are-here` | You are here | 11 §5.28 |
| `lifecycle-stage-agent` | Agent: { $agent } | |
| `lifecycle-stage-hands-over` | Hands over: { $what } | 11 §5.28 |
| `lifecycle-none` | This workspace doesn't follow a stage of a lifecycle. | |
| `import-title` | Import an agent or a part | 11 §5.30 |
| `import-what-ordin-reads` | Agents made in ordin, APM packages, and Claude Code and GitHub Copilot set-ups. | 11 §5.30 |
| `import-what` | What are you bringing in? | 11 §5.30 |
| `import-agent` / `import-part` | An agent / One part | 11 §5.30 |
| `import-part-kind` | What kind of part is it? | |
| `import-which-repository` | Which agent repository is it for? | 11 §5.30 |
| `import-choose-folder` | Choose a folder… | |
| `import-verb` | Import | |
| `installed-files-title` | The installed files of { $agent } | 11 §5.29 |
| `installed-files-note` | Read only. ordin wrote these from the agent; to change them, change the agent. | 11 §5.29 |
| `email-link-note` | Ordin and the agents you allow can read these messages. Nothing is sent without you. | 11 §5.22 |
| `email-list-empty` | No messages here yet. | |
| `email-cc` | Cc | 11 §5.22 |
| `email-hidden-text` | This message holds hidden text, which isn't shown. | 11 §5.22 |
| `email-cut` | Only the start of this message is shown. | |
| `email-discard-body` | The draft is deleted from this computer. Nothing was sent. | 11 §5.22 |
| `email-sent` | Sent. It's in your Sent folder. | 11 §5.22 |
| `email-not-sure` | Not sure this was sent. It isn't in your Sent folder, but it may still have gone. Check in your mail app. | 11 §5.22 |
| `part-page-from` | From: { $name } | 11 §5.29 |
| `part-page-from-ordin` | From: ordin | 11 §5.29 |
| `part-page-details` | Details | 11 §5.29 |
| `part-page-used-by` | Used by | 11 §5.29 |

Existing ids reused: `confirm-cancel`, `card-close`, `check-checked-by-ordin`, `check-fix-it`, `check-ask-ordin-to-fix`, `check-change-it-yourself`, `block-ask-about-this`, `block-ask-text-this`, `block-reply-with-ordin`, `block-reply-email-text`, `block-reply-reference`, `block-save-attachment`, `block-file-saved`, `block-email-from`, `block-email-to`, `block-email-send`, `block-email-edit`, `block-email-save-to-drafts`, `block-email-saved-to-drafts`, `block-email-discard`, `block-email-discard-title`, `block-email-drafted-by`, `block-email-drafted-by-an-agent`, `approval-outside-company`, `server-address`, `server-code`, `server-update-available`, `signin-import-certificate`, `memory-used-page`, `memory-link-by-page`, `team-nothing-to-share`, `team-error-unreachable`, `team-error-sign-in`, `team-error-read-only`, `settings-show-more`, `settings-slack-sign-out`, `settings-slack-allow`, `home-nothing-running`, and the `part-kind-*` ids of AgentModel.md's kind table.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Any command of SM-NS-1 to SM-NS-15 is refused | `Err` from the core | the owner's code | the owner's sentence by `ErrorLine` at the place it concerns, with at most one action; the person's typed text is kept |
| A name another workflow in the workspace has, on **Rename…** | `Err(permanent)` | `WFF_NAME_DUPLICATE` | its sentence under the field; the panel stays open (SM-NS-5.2) |
| An empty workflow name on **Rename** | not sent | — | `workflow-rename-empty` under the field (SM-NS-5.2) |
| **Save** of a workflow whose check finds problems | not saved | `CheckProblem.code` (diagnostic only) | each problem's message on the step or field it names (SM-NS-5.3) |
| The document changed where it lives while it was being edited | `Err` from the core | `FILE_CHANGED_MEANWHILE` | its sentence on the editor's bar; the editor stays open with the text typed (SM-NS-4.6) |
| The person cancels a save or file dialog | `Err` from the core | `DSK_DIALOG_CANCELLED` | nothing is drawn (SM-NS-3, SM-NS-5.2) |
| A notification switch cannot be written | `Err(retryable)` | `DSK_PREFERENCES_WRITE_FAILED` | the switch returns to the stored value; its sentence with **Try again** (SM-NS-1.7) |
| GitLab has no OAuth application in the profile | `Err` from the core | `CRED_PROVIDER_NOT_CONFIGURED` | no sentence; the row's block offers the token (SA-CLOSE-03, SM-NS-2.2) (amended January 2026; user decision; Approved by the user January 2026) |
| The build carries no GitHub App client id | `Err` from the core | `CRED_PROVIDER_NOT_CONFIGURED` | no sentence; GitHub's block offers the token in place of one-click sign-in (SM-NS-2.6) (amended January 2026; user decision; Approved by the user January 2026) |
| GitHub does not accept a pasted token | `Err` from the core | `CRED_PROVIDER_REFUSED` | `signin-github-token-refused` under the field, which is cleared (SM-NS-2.6; amended January 2026; user decision; Approved by the user January 2026) |
| GitHub cannot be reached while a token is checked | `Err` from the core | `CRED_PROVIDER_UNREACHABLE` | its sentence with `ErrorLine` (SM-NS-2.6; amended January 2026; user decision; Approved by the user January 2026) |
| Any other service (Slack, the others) has no sign-in set up in this build or profile | `Err` from the core | `CRED_PROVIDER_NOT_CONFIGURED` | its sentence is drawn with `ErrorLine`; never silent (SM-NS-2.2; amended January 2026; user decision; Approved by the user January 2026) |
| The person cancels a sign-in | `Err` from the core | `CRED_SIGN_IN_CANCELLED` | nothing is drawn (SM-NS-2.2) |
| A sign-in is not finished in time | `Err` from the core | `CRED_SIGN_IN_TIMEOUT` | its sentence with **Try again** (SM-NS-2.2) |
| An imported certificate is expired or unreadable | `Err` from the core | `CRED_CERTIFICATE_EXPIRED`, `CRED_CERTIFICATE_INVALID` | its sentence under **Import a certificate…** (SM-NS-2.4) |
| A support export names more than 20 runs | not sent | — | `diagnostics-support-limit`; the 21st tick stays off (SM-NS-3) |
| **Clear logs** cannot delete a past log file or empty the current one | `Err(retryable)` | `DSK_LOGS_CLEAR_FAILED` | its sentence with **Try again** inside the open question (SM-NS-3a; amended January 2026 (user decision; Approved by the user January 2026)) |
| A handover is accepted on another computer first | `Err` from the core | `HAND_ALREADY_ACCEPTED` | its sentence; the screen re-reads with `handover.open` (SM-NS-8.2) |
| An email draft that may no longer be edited or sent | `Err` from the core | `MAIL_DRAFT_STATE_INVALID` | its sentence; the screen re-reads with `mail.get_draft` (SM-NS-12.3) |
| Show the installed files of an agent that is no longer installed | `Err` from the core | `INST_NOT_INSTALLED` | its sentence, with no list (SM-NS-11) |

### Tests (vitest) — added

- Catalogue: every id of SM-CU-4 and of *New messages* exists in exactly one en-GB `.ftl` file under `crates/ordin-localisation/catalogues/en-GB/`; the text of each SM-CU-4 id equals the text in this amendment.
- Screen identifiers: table-driven over SM-CU-2 — `parseKey(makeKey(id, instance))` returns the pair; `screenKeyFor` of an `OpenTarget` of kind `document` returns `document:<place id>:<path>`.
- Settings sections: table-driven over SM-CU-3 — a section is listed exactly when the backend answers its commands; the order is the table's.
- Markers: a source scan finds no `SPEC-GAP-` text in `apps/desktop/ui/src` other than the markers the table above marks **Kept**, each naming FU-138.
- Approval card (SM-AP-1): an `approval_requested` event opens `approval:<id>` as a tab marked new, not in front, with focus unchanged, both when the person sent `ordin.send_message` in that workspace 1 second earlier and when they did not.
- Home (SM-CU-5), Search (SM-CU-6), Connect (SM-CU-7), server update (SM-CU-8), Slack rules (SM-CU-9), team error (SM-CU-10), page names (SM-CU-11): one test per branch each assertion states.
- Each SM-NS screen: a recorded reply of each command it sends draws the order its assertion gives; one test per error row above; SM-UP-1 hides each entry whose group is absent.
- Sign-ins: `device_code` draws the address as text, never as an element with an `href`; the password and token fields are empty after sending.
- Email message: a `text` holding `<img src=x onerror=alert(1)>` is drawn literally; `hidden_text = true` draws `email-hidden-text`.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Catch-up of words and identifiers, the screens Phase 10 left out, and when an approval card opens (FU-135, FU-136, FU-142)

  Scenario: Happy path — a new approval does not move the person's place (SM-AP-1)
    Given the person is reading "Supplier summary.docx" in workspace "Q4 forecast"
    And they sent a message in that workspace 2 seconds ago
    When an approval_requested event arrives for workspace "Q4 forecast"
    Then the approval opens as a new tab marked new
    And "Supplier summary.docx" stays in front with the same focus

  Scenario: Edge case — Home with nothing waiting (SM-CU-5)
    Given home.view returns no waiting rows
    When the person opens Home
    Then Waiting for you reads "Nothing is waiting for you"

  Scenario: Happy path — the search box announces its results (SM-CU-6)
    When the person types "Kestrel" and search.query returns 3 entries
    Then the live region reads "3 results"

  Scenario: Happy path — connecting with one pasted line (SM-CU-7)
    Given the person pastes a line into the address box and leaves the code empty
    When they press Connect
    Then servers.connect is sent with kind pairing_line and that line

  Scenario: Error path — a server that cannot update itself (SM-CU-8)
    Given setup.server_update returns 1.1.0 with self_update false for "Northwind server"
    Then the row reads "Northwind server can update to ordin 1.1.0." and "This server can't update itself. Whoever runs it installs new versions."
    And no Update server button is shown

  Scenario: Happy path — a Slack rule names its kind (SM-CU-9)
    Given slack.rules returns an ordin_answers rule in #supplier-review, cap 20, state on
    Then Settings › Slack reads "Ordin's answers in #supplier-review · up to 20 a day · on" with Turn off

  Scenario: Happy path — Settings › Email shows the mailbox (SM-NS-1.2)
    Given identity.list returns a mail_server_password credential "you@company.com" signed in
    When the person opens Settings › Email
    Then the section starts "Every email is shown to you before it is sent. This can't be turned off."
    And Your mailbox reads "you@company.com" and "Signed in with your password, kept in this computer's keychain"

  Scenario: Happy path — Using your plan shows today (SM-NS-1.6)
    Given usage.today returns typical_day_permille 200
    Then the section reads "Today you've used about 20% of a typical day's plan."

  Scenario: Edge case — a notification choice set by the company (SM-NS-1.7)
    Given settings.get returns notifications.handovers with person_may_change false
    Then its switch is disabled with "Set by your company"

  Scenario: Happy path — a device-code sign-in (SM-NS-2)
    Given identity.sign_in for github returns device_code "WDJB-MJHT" at "https://github.com/login/device"
    Then the row reads "Open https://github.com/login/device and enter this code:" with "WDJB-MJHT" and Copy the code
    And the address is not a link

  Scenario: Edge case — no certificate import unless the company requires one (SM-NS-2)
    Given profile.get_document returns server.client_certificate_required false
    Then Settings › Sign-ins shows no Import a certificate…

  Scenario: Happy path — saving logs for support (SM-NS-3)
    Given the person opens Save logs for support… over 3 runs, all 3 ticked
    And unticks 1 of them
    When they press Save… and choose a place
    Then diagnostics.support_export is sent with the other 2 run ids
    And "Saved to" with the chosen path is shown

  Scenario: Error path — a 21st run is refused (SM-NS-3)
    Given 20 runs are ticked
    When the person ticks a 21st
    Then it stays unticked and "You can include up to 20 runs." is shown

  Scenario: Happy path — editing a Markdown document (SM-NS-4)
    Given the document page of "rates.md" shows a Markdown text preview
    When the person presses Edit, changes a line and presses Save
    Then files.save_edit is sent with the new text
    And "Changed by you — not yet sent back" is shown

  Scenario: Error path — the document changed meanwhile (SM-NS-4)
    When files.save_edit is refused with FILE_CHANGED_MEANWHILE
    Then its sentence is shown on the editor's bar and the typed text is kept

  Scenario: Error path — a workflow name already used (SM-NS-5)
    When the person renames the workflow to "Supplier rates refresh" and the reply is WFF_NAME_DUPLICATE
    Then its sentence is shown under the field and the panel stays open

  Scenario: Happy path — a workflow saved after its check (SM-NS-5)
    Given workflows.check returns no problems
    When the person presses Save with the change note "Check prices first"
    Then workflows.save is sent and "Saved as version 4" is shown

  Scenario: Happy path — the agent draft's next step (SM-NS-6)
    Given authoring.agent_draft returns a draft with 2 failed checks
    Then the Next bar reads "Fix 2 checks"
    And Publish… is not the next action

  Scenario: Happy path — a part withdrawn with a reason (SM-NS-7)
    Given the person opens Withdraw on "Reading notice periods"
    When they write the reason and press Withdraw…
    Then authoring.withdraw is sent with that part and reason
    And the part draft opens

  Scenario: Happy path — accepting a handover into a new workspace (SM-NS-8)
    Given handover.open returns actions accept_and_start and return_to
    When the person presses Accept and start and chooses New workspace
    Then handover.accept_and_start is sent with into new_workspace

  Scenario: Edge case — a workspace with no lifecycle (SM-NS-9)
    Given handover.stage_of returns null
    When the person opens the lifecycle screen
    Then it reads "This workspace doesn't follow a stage of a lifecycle."

  Scenario: Happy path — importing an agent from a folder (SM-NS-10)
    When the person chooses An agent, an agent repository and a folder, and presses Import
    Then authoring.import is sent without as_part
    And the agent draft opens

  Scenario: Happy path — the installed files are read only (SM-NS-11)
    Given agents.installed_files returns 3 files
    Then 3 lines are drawn and none opens anything

  Scenario: Happy path — an email list names its link (SM-NS-12)
    Given workspaces.email_links returns "Inbox › Suppliers" and mail.list_messages returns 2 messages
    When the person opens that email link
    Then the heading reads "Inbox › Suppliers" and 2 rows are drawn newest first

  Scenario: Security — an email's hidden text (SM-NS-12)
    Given mail.open_message returns hidden_text true
    Then "This message holds hidden text, which isn't shown." is drawn

  Scenario: Happy path — an installed agent's part page (SM-NS-15)
    Given agents.part_page returns a rule with its text
    Then the page shows the part's name, its kind in plain words, "From: ordin" and its text
    And Check it is offered
```

### Accessibility (additions)

Every new screen has one level-2 heading that receives focus on opening; every list of rows is a list with one accessible name per row; every switch and choice is a native control with its label; disabled controls set by the company carry `settings-set-by-company` in their accessible description; the support-export ticks are checkboxes whose limit message is announced in a polite live region; the device code is selectable text with **Copy the code** as a button; a new approval's announcement (SM-AP-1) uses the existing polite live region and never moves focus.

### Assumptions

- **SA-UI-CU-01** *(flagged)* The in-front window of SM-AP-2 is 5 seconds after the reply. Chosen because an approval that answers the person's own request is raised by the same turn; a longer window would pull later, unrelated cards to the front. It applies only when SM-AP-2 is built.
- **SA-UI-CU-02** This build keeps the card a new tab (SM-AP-1) because the screens hold no signal that a card answers the person's last action, and opening in front without one would move the person's place (11 §4).
- **SA-UI-CU-03** Where the HLD names a value no generated field carries (the mail server's name, the app a document opens in before `craft.open` answers), the words say it in general terms rather than inventing a field.
- **SA-UI-CU-04** **Rehearse** sends no situations of its own in this build; the Authoring Service adds the situations of the draft's own expectations (AuthoringService.md §I), so a rehearsal still runs. Situations the person or the agent maker add are G-CU-8.
- **SA-UI-CU-05** A `FileVersionOrigin` of kind `person` is a save by the person using this ordin, so it reads "saved by you".
- **SA-UI-CU-06** A handover's sender is recognised on its screen by `actions` holding neither `accept_and_start` nor `return_to`; `HandoverScreen` carries no direction of its own. A command the backend refuses for the wrong side draws its sentence.

### Gaps

Each gap blocks only the part named; everything else in this amendment can be built against the commands and types seen in `commands.gen.ts` and `types.gen.ts`.

- **G-CU-1** *A hosted agent's memory section* (SM-NS-14). No Service API command reaches the Caller API's operator route; the screens cannot draw the section. Needs a Service API command of the `hosted` group carrying the memory verbs, or a decision that the section stays out of this build. **Settled by the user 2026-01-10:** the section stays out of this build.
- **G-CU-2** *A part's own form and text by kind.* `PartDraftView.form`, `PartDraft.fields`, `UpdatePartDraftInput.fields` and `PartDocument.body` are `unknown` in the generated types, so the part draft's form (SM-NS-7.3) and a library part's text laid out by kind (SM-NS-15.2) cannot be specified field by field. Needs the Service API to restate their shapes.
- **G-CU-3** *(Publish a copy… closed by SM-PUB-1 and SM-PUB-2 (amended January 2026; user decision; Approved by the user January 2026); Send back… and a copy's own actions stay open.)* *Send back… and Publish a copy… on the document page*, and a published copy's own actions (SM-NS-4.8). They need the destination picker and the Push Service's copy commands specified on screen; not stated here.
- **G-CU-4** *Workflow control blocks, Show as diagram, shared workflows and Share…* (SM-NS-5.4), and the lifecycle draft and its diagram (SM-NS-9). `WorkflowFileStep` and the lifecycle draft's form were not read for this amendment.
- **G-CU-5** *The preview "How Ordin would say it"* in How Ordin talks to you: no command returns the example message.
- **G-CU-6** *The model level per kind and the daily pause* in Using your plan: `model_level_per_kind` is a `Record<string, string>` whose kinds and levels are not named in the generated types, and the daily pause is stored in requests while 11 §5.18 says it is shown as a share of a typical day, with no conversion visible.
- **G-CU-7** *A language server's own settings* (`LanguagePlugin.server_settings`, `unknown`) in Settings › Code.
- **G-CU-8** *Talk to the agent maker*, rehearsal situations the person adds, and the files shown to people who code on the agent draft (SM-NS-6.3).
- **G-CU-9** *Two ids with a literal "Ordin".* **Settled by the user 2026-01-10:** `check-ask-ordin-to-fix` and `conv-ask-about` take `{ $coordinator }` like the other ids of their files, so a renamed coordinator shows the right name. The `.ftl` files and the two call sites change in Phase 10B.
- **G-CU-10** *Importing an APM package as a file* (SM-NS-10). `ImportInput.source` is a string and only `app.pick_folder` exists to choose one; no command opens a file picker for a package.

## Amendment January 2026 — Run-state message ids

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations").
>
> Source: the screens' second review ("nine `state-*` messages") and FU-136. Evidence read for this amendment: the run-state table in *Shared Context* above, the row `state-<stored state>` in *Message catalogue entries this module draws*, the en-GB file `crates/ordin-localisation/catalogues/en-GB/screens.ftl` (section "Run-state labels"), the generated `RunPauseReason` and `Run.pause_reason` in `apps/desktop/ui/src/lib/service-api/types.gen.ts`, and the two call sites `apps/desktop/ui/src/lib/blocks/kinds/PlanProgress.svelte` and `apps/desktop/ui/src/lib/views/home-model.svelte.ts`. This amendment replaces the localisation row `state-<stored state>` with the rule and rows below; nothing else above changes.

### Message catalogue entries (row `state-<stored state>` replaced)

**SM-RS-1 Mapping rule.** A Fluent message id cannot contain `_`, so the id of a stored run state is `state-` followed by the stored state with every `_` replaced by `-`. The stored states are those of the run-state table above; their text is that table's *Label* column, unchanged.

**SM-RS-2 The nine ids of a stored state with one label.**

| Id | en-GB text | Stored state | Source |
|---|---|---|---|
| `state-queued` | Waiting to start | `queued` | 11 §8 |
| `state-running` | Working | `running` | 11 §8 |
| `state-stopped-at-limit` | Stopped — reached its limit | `stopped_at_limit` | 11 §8 |
| `state-failed` | Couldn't finish | `failed` | 11 §8 |
| `state-interrupted` | Interrupted | `interrupted` | 11 §8 |
| `state-no-reply` | No answer yet | `no_reply` | 11 §8 |
| `state-abandoned` | Not used (stopped crew) | `abandoned` | 11 §8 |
| `state-completed` | Done | `completed` | 11 §8 |
| `state-cancelled` | Stopped | `cancelled` | *assumed* in the run-state table above (11 §8 has no row) |

**SM-RS-3 `paused` has no plain id.** The run-state table gives `paused` three labels, one per condition, so `state-paused` is not a message and SM-RS-1 is not applied to it. The three ids are *chosen here* *(flagged)*, each named after the table's own condition:

| Id ‡ | en-GB text | Condition (the table's words) | Next action (the table's) |
|---|---|---|---|
| `state-paused-approval` | Waiting for you | `paused` with an approval pending for the run | Review |
| `state-paused-slack` | Waiting for you in Slack | `paused` with an approval pending in Slack | Open in Slack |
| `state-paused-closed` | Paused while ordin was closed | `paused` after ordin was closed | Continue |

**SM-RS-4 Choosing the paused id.** A run whose `Run.pause_reason` is `approval` draws `state-paused-approval`. Which runs draw `state-paused-slack` and `state-paused-closed` is G-RS-1. Until G-RS-1 is settled, every `paused` run draws `state-paused-approval`, whatever its `pause_reason` (*chosen here*: the label that asks the person to look, which never hides a pause).

**SM-RS-5** The row "external action of unknown outcome" of the run-state table is not a stored state, so it has no `state-` id; this amendment gives it none.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a stored state with an underscore finds its message (SM-RS-1)
    Given a run whose stored state is stopped_at_limit
    When its state is drawn in en-GB
    Then the message id is state-stopped-at-limit
    And the label reads "Stopped — reached its limit"

  Scenario: Happy path — a run paused on an approval (SM-RS-4)
    Given a run whose stored state is paused and whose pause_reason is approval
    When its state is drawn in en-GB
    Then the message id is state-paused-approval
    And the label reads "Waiting for you"

  Scenario: Edge case — no message is looked up as state-paused (SM-RS-3)
    Given a run whose stored state is paused
    When its state is drawn
    Then no message with the id state-paused is requested
```

### Tests (vitest) — added

- Every id of SM-RS-2 and SM-RS-3 resolves in the en-GB bundle to the text in its row.
- The id built from each of the nine stored states of SM-RS-2 by SM-RS-1 equals the id in its row.

### Assumptions

- **SA-UI-RS-01** *(flagged)* The three paused ids of SM-RS-3 are chosen here; the HLD gives labels for the three conditions and no ids.

### Gaps

- **G-RS-1** *Telling the three paused conditions apart.* The generated `RunPauseReason` is `approval`, `sign_in` or `person`; no generated field says that an approval is pending in Slack or that a run was paused because ordin was closed, and the run-state table has no row for `paused` with `pause_reason` `sign_in` or `person`. Needs the Session Supervisor's or the Service API's spec to name the field that carries each condition, or a decision that `state-paused-slack` and `state-paused-closed` stay out of this build.

## Amendment January 2026 — The command behind a linked folder's publishing line (FU-127, command side)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Counterpart: `components/ServiceApi.md` SVC-AUTO-1 (`workspaces.confluence_publishing`). Closes G-SF-3 of the amendment *Screens for Phase 7A decisions and automatic publishing (FU-089, FU-127)*. Every rule above stays in force.

- **SM-SF-10** `confluencePublishing(b, confluence_link_id)` in `lib/backend/wiring.ts` stops returning `not_wired`: it becomes a `BackendHandle.call` of `workspaces.confluence_publishing` with `{ confluence_link_id }`, as the wiring rule of that amendment states, and returns its `CommandOutcome`. The local mirror `ConfluencePublishingView` is removed and the generated `ConfluencePublishing` of `types.gen.ts` is used in its place, once the generated files carry it (they are regenerated from the catalogue; no hand edit). SM-SF-7 is unchanged: it draws `confluence-auto-on` or `confluence-auto-off` from the result, and on `WS_LINK_NOT_FOUND`, `WS_FILES_FAILED` or `WS_DATASTORE_UNAVAILABLE` draws no publishing line and the owner's sentence through `ErrorLine` (SM-SF-7.4). No message id is added.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the folder shows that automatic publishing is off (SM-SF-10)
    Given workspaces.confluence_publishing answers automatic "off" with one agent_instance grantee named "Report writer"
    When the linked folder's screen opens
    Then the publishing line confluence-auto-off is drawn with the name "Report writer"

  Scenario: Error path — the link is gone (SM-SF-10)
    Given workspaces.confluence_publishing answers WS_LINK_NOT_FOUND
    When the linked folder's screen opens
    Then no publishing line is drawn and the owner's sentence is shown in its place
```

### Gaps

None.

## Amendment January 2026 — Screens wired to the commands of Phase 11 (FU-138, G-SF-1, G-SF-2, FU-144)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Every rule above stays in force except where a rule below names what it replaces. Counterpart: `components/ServiceApi.md`, amendment January 2026 — *Commands the screens wait on* (SVC-SCR-1 to SVC-SCR-6). Closes G-SF-1 and G-SF-2 of the amendment *Screens for Phase 7A decisions and automatic publishing (FU-089, FU-127)* once those commands are in the catalogue.
>
> Evidence read: the markers of `apps/desktop/ui/src/lib/backend/wiring.ts` (the six `not_wired` functions), `apps/desktop/ui/src/lib/blocks/refs.ts` (SPEC-GAP-2), `apps/desktop/ui/src/lib/blocks/kinds/CrewPlan.svelte` (SPEC-GAP-4), `apps/desktop/ui/src/lib/views/engineering/projects.ts` and `apps/desktop/ui/src/lib/views/engineering/editor-hub.svelte.ts` (SPEC-GAP-1), `apps/desktop/ui/src/lib/views/fixed/jira.svelte.ts` (SPEC-GAP-5), `apps/desktop/ui/src/lib/views/Settings/updates.svelte.ts` (SPEC-GAP-1), `apps/desktop/ui/src/lib/views/Settings/knowledge-agents.svelte.ts` (SPEC-GAP-6); follow-ups FU-138 and FU-144; this file's SM-CC-2, SM-CC-3, SM-CC-6, SM-SF-1, SM-SF-4, SM-SF-5, SM-NS-11, SM-AP-1, SM-RS-3, SM-RS-4 and G-RS-1; `components/OrdinHost.md` (`ordin.list_conversations`, `ordin.prepare_start_fresh`, step 33); the en-GB catalogue `crates/ordin-localisation/catalogues/en-GB/screens-settings.ftl` (`conv-ask-about`, `conv-continue`, `conv-picker-all`).

### Internal Logic (additions)

1. **SM-WR-1 The exact version an approval covers** (closes G-SF-1). `readFileVersion(b, file_version_id)` in `lib/backend/wiring.ts` becomes `BackendHandle.call` of `files.read_version { file_version_id }` and returns its `CommandOutcome`; the mirror `VersionContent` is replaced by the generated result type once the generated files carry it (regenerated from the catalogue; no hand edit). SM-SF-1 is unchanged: an approval card whose content holds a `file_version` reference draws the version's preview, and SM-SF-1.4 keeps **Approve** disabled until every such reference is read, so a card that cannot read one (`FILE_VERSION_NOT_FOUND`, or a version above the read limit) still draws `approval-bound-version-unavailable` with the owner's sentence and keeps **Approve** off.
2. **SM-WR-2 Held writes and sources to confirm** (closes G-SF-2). In `lib/backend/wiring.ts`: `heldWrites` → `memory.held_writes { workspace_id }`; `keepHeld` → `memory.keep_held { workspace_id, id, content_hash }`; `discardHeld` → `memory.discard_held { workspace_id, id }`; `citationsToConfirm` → `memory.citations_to_confirm { workspace_id }`; `confirmCitation` → `memory.confirm_citation { workspace_id, id }`. The mirrors `HeldWrite`, `HeldThing` and `HeldLink` are replaced by the generated types. SM-SF-4 and SM-SF-5 are drawn from then on, unchanged; `NotWired` and `isNotWired` stay for a backend whose catalogue lacks a command (a server on an older version), which then draws as `not_wired` did.
3. **SM-WR-3 A block that cites an artefact** (closes `refs.ts` SPEC-GAP-2). `resolveRef` resolves a `ContentRef` of kind `artefact` with `artefacts.get { artefact_id }` on the block's backend:
   1. `artefact` absent → `unresolvable`, drawn as today.
   2. `location` of kind `place` → the name is the last segment of `Artefact.path`, and the preview is `files.preview { place_id, path }`, exactly as a `place_file` reference is resolved (the file as it is now).
   3. `location` of kind `agent_folder` → the name as in 2 and the preview `{ kind: 'none' }` *(chosen here: an agent folder is never listed or read by the screens, FileService.md Security)*.
   4. An error reply → `{ kind: 'error' }` with its sentence through `ErrorLine`.
   SM-SF-1.4 does not apply to an `artefact` reference; it applies only to `file_version`.
4. **SM-WR-4 A project screen reads its project** (closes `projects.ts` and `editor-hub.svelte.ts` SPEC-GAP-1, and FU-144's `rememberProject` item). When a project screen or the editor's bar needs a `Project` that the screens have not seen on that backend, it sends `projects.get { project_id }` and keeps the result with `rememberProject`; `PROJ_NOT_FOUND` draws its sentence through `ErrorLine` in place of the project's name. The screen no longer depends on its opener having called `rememberProject`, so no opener in the tree is needed for it; the first folder of a file's path is no longer drawn as a stand-in name.
5. **SM-WR-5 A linked issue's changed item** (closes `jira.svelte.ts` SPEC-GAP-5; replaces the input of ADR-106 step 3). The items of a remote link in the tree are read with `workspaces.working_copy_items { source_link_id }` when the link's node is expanded and again on each `workspace_changed` for its workspace. An item with `origin_newer_at` draws `jira-item-changed` (`$time` from `origin_newer_at`) and **Bring in the new version** (`jira-item-bring-in`), which sends `workspaces.read_item { source_link_id, origin_path }` (ADR-106 step 3 wrote `{ link, origin_path }`; the catalogue's key is `source_link_id`). `ok` → the item is redrawn from the returned `WorkingCopyItem`, with no notice; an error → its sentence through `ErrorLine` beside the item, and the notice stays. Nothing changes the item before the person presses the button. Jira stays offered on the desktop's own backend only.
   - *Amended January 2026 (user decision, "go with A"): a Jira issue in the tree.* A Jira issue whose link has a place (`place_id` set) is opened in the tree as any other linked location is (SM-RL-3): pressing its row opens the place's documents screen and lists, under the row, its documents, each opening the document page. Under those documents the tree draws only the issue's items with `origin_newer_at`, each with `jira-item-changed` and **Bring in the new version** as above; an item that has not changed is not drawn twice. A remote link with no place yet still lists all its items under its row. *(Found by the journeys on Windows: with the Documents screen no longer listing places, a Jira issue's documents could not be opened anywhere.)*
6. **SM-WR-6 Generated types read as they are** (closes `updates.svelte.ts` SPEC-GAP-1 and `knowledge-agents.svelte.ts` SPEC-GAP-6, after ServiceApi.md SVC-SCR-6). `offerState` reads `UpdateOffer.state`, whose generated type then holds `administrator_needed`, without a string comparison outside the type. Settings › Knowledge agents draws each account line from `knowledge_agents.details`'s `accounts` (`ConnectionAccount { system, state }`), one line per entry in the order given, with `state` choosing the line as ADR-115's amendment above states; it never reads a `state` from `about.accounts`.
7. **SM-WR-7 Earlier conversations open in the screen area** *(amended January 2026, the user's decision "earlier conversations should open in a central pane not in the treeview"; replaces the list in the tree of FU-144)*. Under a workspace the tree draws one row, *Earlier conversations* (`conv-earlier-name`, icon `history`), drawn when the backend answers `ordin.list_conversations`. Pressing it opens the Earlier conversations screen (`earlier-conversations`) in the workspace's screen area; the row has no children and sends no command itself. The screen lists the chains as SM-CC-6 states, and each opens read-only on its own tab (`earlier-conversation:<id>`), where **Ask Ordin about it** and **Continue this conversation** (SM-CC-2 to SM-CC-5) are offered. The tree has no **⋯** menu for an earlier conversation.
8. **SM-WR-8 A block that opens a screen** (FU-144; extends SM-AP-1 to blocks). When a `block_shown` event carries a block whose row of step 9's block table says "screen at once", the screen `block:<id>` opens in its workspace's screen area as a new tab marked *new*, never in front, whatever the person did last; focus and the person's place do not move. This is SM-AP-1's rule; when SM-AP-2 is built for approval cards, the same 5-second rule applies to these screens with the same signal.
9. **SM-WR-9 The button to the installed files** (FU-144, SM-NS-11). *What [agent] is made of* offers **Show the installed files** (`installed-files-show`), which opens `installed-files:<agent instance id>` with the agent's name as SM-NS-11 states. It is offered only in a workspace that links code; until OQ-WR-3 settles how the screens tell that, the button is not drawn.
10. **SM-WR-10 Paused runs** (FU-144, `state-paused`). Settled by SM-RS-3 and SM-RS-4 (Approved 2026-01-10): no message `state-paused` is looked up, and every `paused` run draws `state-paused-approval` until G-RS-1 is settled. No change here.

### Message catalogue rows (en-GB; every id below is new; *chosen here*)

| Id | en-GB text | Source |
|---|---|---|
| `tree-node-menu` ‡ | Actions for { $name } | — *(flagged: the words are chosen here)* |
| `installed-files-show` | Show the installed files | 11 §5.29 (SM-NS-11) |

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A project screen opens for a project that is gone | `Err` from the core | `PROJ_NOT_FOUND` | its sentence in place of the project's name (SM-WR-4) |
| **Bring in the new version** fails | `Err` from the core | the owner's code (for example `WS_ITEM_TOO_LARGE`) | its sentence beside the item; the notice stays (SM-WR-5) |
| A block cites an artefact no row has | `ok` with no `artefact` | — | drawn as `unresolvable` (SM-WR-3) |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — an approval card shows the exact version (SM-WR-1)
    Given an approval card whose changes content is file_version V of "rates.md"
    And files.read_version answers V with a text preview "Rate: 120"
    When the card opens
    Then "Rate: 120" is drawn under approval-bound-version-heading and Approve is enabled

  Scenario: Error path — the version is gone (SM-WR-1)
    Given files.read_version answers FILE_VERSION_NOT_FOUND
    When the card opens
    Then approval-bound-version-unavailable and "That version is no longer kept." are drawn and Approve is disabled

  Scenario: Happy path — held writes are listed (SM-WR-2)
    Given memory.held_writes for W answers one held write "Pay this supplier first."
    When the To check tab of W opens
    Then that statement is drawn with Keep and Discard

  Scenario: Edge case — an older server lacks the commands (SM-WR-2)
    Given a server backend whose catalogue has no memory.held_writes
    When the To check tab of a workspace on that server opens
    Then the held writes section is not drawn

  Scenario: Happy path — an artefact in a place is previewed (SM-WR-3)
    Given a documents block citing artefact A whose location is place P and path "out/summary.md"
    When the block is drawn
    Then artefacts.get then files.preview with P and "out/summary.md" are sent and the name is "summary.md"

  Scenario: Happy path — a project screen opened first reads its project (SM-WR-4)
    Given the screens have seen no Project for id J
    When the project screen of J opens
    Then projects.get is sent with J and the project's display name is drawn

  Scenario: Happy path — bringing in a changed Jira item (SM-WR-5)
    Given an item of a linked issue with origin_newer_at set to 10:40 today
    When the person presses Bring in the new version
    Then workspaces.read_item is sent with the link's source_link_id and the item's origin_path
    And the notice is no longer drawn

  Scenario: Happy path — Earlier conversations opens in the screen area (SM-WR-7)
    Given ordin.list_conversations answers a conversation A
    When the person presses Earlier conversations in the tree
    Then the Earlier conversations screen opens in the screen area
    And the tree shows no conversation under the row

  Scenario: Edge case — a block screen does not take the person's place (SM-WR-8)
    Given the person is typing in a document screen
    When a block_shown event carries a block that opens a screen
    Then block:<id> opens as a tab marked new and focus stays in the document screen
```

### Open Questions

- **OQ-WR-1** — *resolved 2026-01-10 as recommended (a): SM-WR-12 below and OrdinHost.md OH-WR-1.* *Keep the key points first (SM-CC-3, and Clear's flow).* `ordin.prepare_start_fresh` answers `{ accepted: true }` and Ordin then shows its key points as a form (OrdinHost.md step 33), but nothing marks that form as the key-points form, and nothing says who turns the person's answer into `ordin.start_fresh` or `ordin.continue_conversation`, so the screens cannot finish either flow. Options: (a) the Ordin Host does it: `ordin.prepare_start_fresh` takes an optional `conversation_id` to continue, and the Ordin Host, on the answer to its own form, calls the start-fresh or continue path itself; (b) the form block carries a marker the screens read, and the screens send the command. **Recommendation:** (a): the Ordin Host already owns the form and both paths, and the screens then send nothing new; it needs an amendment of OrdinHost.md before this file can state the screens' part. Blocks **Keep the key points first** in both Clear and Continue; **Continue** and **Clear** without key points work as now.
- **OQ-WR-2** — *resolved 2026-01-10 as recommended (a): SM-WR-13 below and the 2C wire amendment of `ResultBlock::Checklist`.* *The Ready to publish checklist block (FU-144, `ReadyChecklist` mount).* The `result` block of kind `checklist` is `{ title, items: Vec<ContentRef> }`; it carries no `confluence_link_id` and no `ReadyItem`, so the screens cannot draw SM-SF-8's changed items or send **Publish checked items…** for it. Options: (a) the block gains `confluence_link_id` (2C wire §7, an additive optional field), and the screens read `confluence.ready_to_publish` for it; (b) the checklist block stays a plain list of documents and **Publish checked items…** is offered only on the linked folder's screen. **Recommendation:** (a): the card in *Needs you* is where the person is asked, and an optional field keeps older clients working. Blocks the checklist card's publish action; the linked folder's own *Ready to publish* list is not affected.
- **OQ-WR-3** — *resolved 2026-01-10 as recommended (a): SM-WR-14 below.* *Which workspace "links code" (SM-NS-11, and `detail` of `agents.part_page`).* No command or field states it. Options: (a) a workspace links code when `workspaces.links` holds an active link of kind `git_repository_folder` or `git_remote` (2C `SourceLinkKind`); (b) when a project is open on one of its places. **Recommendation:** (a): it is readable from one existing command and matches the kinds that hold a repository; a workspace whose project was closed still holds code. Blocks **Show the installed files** and the `links_code` detail of a part page; until decided, the button is not drawn and part pages use `business_user`.
- The crew plan's purpose, agents, limits and **Start** wait on ServiceApi.md OQ-SCR-1 — *resolved 2026-01-10: SM-WR-11 below; the command waits on ServiceApi.md G-SCR-1.*

### Gaps

None in this amendment's own rules; the three Open Questions above and OQ-SCR-1 are decisions, each with its recommendation.

## Amendment January 2026 — Resolutions of OQ-WR-1 to OQ-WR-3 and OQ-SCR-1

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Every rule above stays in force except where a rule below names what it replaces. Counterparts: `components/OrdinHost.md`, amendment January 2026 — *Keep the key points first* (OH-WR-1); `phase-2c-shared-types-wire.md`, amendment January 2026 (`ResultBlock::Checklist.confluence_link_id`); `components/ServiceApi.md`, amendment January 2026 — *Resolutions of OQ-SCR-1 and OQ-SCR-2* (SVC-SCR-7, G-SCR-1).
>
> Evidence read: this file's SM-CC-3, SM-SF-8, SM-NS-11, SM-WR-9 and OQ-WR-1 to OQ-WR-3; `apps/desktop/ui/src/lib/views/parts/part-page.svelte.ts` (`DETAIL_FOR_PART_PAGE`, SPEC-GAP-1); `apps/desktop/ui/src/lib/blocks/kinds/CrewPlan.svelte` (SPEC-GAP-4); 2C domain `SourceLink { kind, state }`, `SourceLinkKind` (`git_repository_folder`, `git_remote`), `SourceLinkState` (`proposed`, `active`); the catalogue rows `workspaces.links`, `confluence.ready_to_publish` and `ordin.prepare_start_fresh`.

### Internal Logic (additions)

11. **SM-WR-11 The crew plan block** (OQ-SCR-1, ServiceApi.md SVC-SCR-7). When the block's backend lists `crews.charter` in its catalogue, `crew_plan` sends `crews.charter { crew_charter_id }` when it is drawn and draws, above the estimate, the charter's `purpose`, one line per entry of `members` and the limits of `guardrails`, with **Start** sending `crews.start { crew_charter_id }`; an error reply draws its sentence through `ErrorLine` and the estimate stays. When the catalogue lacks the command (ServiceApi.md G-SCR-1 is open, or the backend is older), the block draws its estimate only, as now.
12. **SM-WR-12 Keep the key points first** (closes OQ-WR-1; replaces the part of SM-CC-3 and of Clear's flow that names a second command). **Keep the key points first** sends `ordin.prepare_start_fresh { workspace_id }` from Clear's confirmation, and `ordin.prepare_start_fresh { workspace_id, conversation_id }` from Continue's confirmation (SM-CC-3), `conversation_id` being the conversation to continue. The screens send nothing more for that flow: Ordin's key-points form arrives as any `form` block and is answered with `ordin.answer_block` as any form; the Ordin Host then starts fresh or continues (OrdinHost.md OH-WR-1). The next `ordin.read_conversation` after `workspace_changed` shows the new conversation, and SM-CC-4 runs when its conversation id differs from the one shown. An error reply to `ordin.prepare_start_fresh` is drawn as SM-CC-5 states.
13. **SM-WR-13 The Ready to publish checklist card** (closes OQ-WR-2). A `result` block of kind `checklist` that carries `confluence_link_id` sends `confluence.ready_to_publish { confluence_link_id }` when drawn and draws its items as SM-SF-8 states, with **Publish checked items…** as on the linked folder's screen; an error reply draws its sentence through `ErrorLine` in place of the list. A checklist without the field is drawn as now, as a plain list of its `items`.
14. **SM-WR-14 A workspace that links code** (closes OQ-WR-3). A workspace links code when `workspaces.links { workspace_id }` returns at least one `SourceLink` with `state` `active` and `kind` `git_repository_folder` or `git_remote`. The screens read it when *What [agent] is made of* or a part page opens, and again on each `workspace_changed` for the workspace while one is open. Then:
    1. SM-WR-9's **Show the installed files** is drawn in that workspace, and not drawn in any other.
    2. A part page of an installed agent sends `agents.part_page` with `detail` `links_code` in that workspace and `business_user` in any other, replacing the fixed `business_user`.
    3. An error reply to `workspaces.links` is taken as "does not link code"; nothing else is drawn for it.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — Keep the key points first while continuing (SM-WR-12)
    Given the live conversation of W has messages and the person chose to continue conversation X
    When the person presses Keep the key points first
    Then ordin.prepare_start_fresh is sent with W and conversation_id X and no other command is sent

  Scenario: Happy path — a checklist card for a linked folder (SM-WR-13)
    Given a checklist block carrying confluence_link_id L
    When the block is drawn
    Then confluence.ready_to_publish is sent with L and Publish checked items… is offered

  Scenario: Edge case — a checklist card without a linked folder (SM-WR-13)
    Given a checklist block with no confluence_link_id
    When the block is drawn
    Then its items are drawn as a plain list and no command is sent

  Scenario: Happy path — a workspace with a linked repository folder links code (SM-WR-14)
    Given workspaces.links for W returns an active link of kind git_repository_folder
    When What the agent is made of opens in W
    Then Show the installed files is drawn

  Scenario: Edge case — a proposed repository link does not count (SM-WR-14)
    Given workspaces.links for W returns only a proposed link of kind git_remote
    When a part page of an installed agent opens in W
    Then agents.part_page is sent with detail business_user
```

### Gaps

None in this amendment's own rules. SM-WR-11's drawing of the charter waits on ServiceApi.md G-SCR-1, and until then the block draws its estimate only.

## Amendment January 2026 — The crew plan's words and Start (closes SPEC-GAP-4)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Every rule above stays in force except where SM-WR-15 names what it replaces. Counterparts: `components/OrdinHost.md`, amendment January 2026 — *Reading one crew plan* (OH-WR-2); `components/ServiceApi.md`, amendment January 2026 — *Resolution of G-SCR-1* (SVC-SCR-9).
>
> Evidence read: SM-WR-11 above; `apps/desktop/ui/src/lib/blocks/kinds/CrewPlan.svelte` (SPEC-GAP-4); `apps/desktop/ui/src/lib/blocks/kinds/Question.svelte` (the busy, offline and waiting rules a block that sends a command follows); `apps/desktop/ui/src/lib/views/fixed/facts.ts` (`AgentNames` over an `agents.list` result); 2C domain `CrewCharter { name, purpose, members: Vec<CrewMember { member: AgentRef, role }>, guardrails: Guardrails { max_rounds, wall_clock_secs, request_budget, max_members_running, members_allowed_external_actions, approval_points, failure_repeat_limit }, state }`, `CrewCharterState`; the catalogue rows `crews.charter` and `crews.start`.

### Internal Logic (additions)

15. **SM-WR-15 The crew plan block, completed (replaces the last sentence of SM-WR-11).**
    1. **Reading.** When the block is drawn on a backend whose `service.hello` groups include `crews`, `crew_plan` sends `crews.charter { crew_charter_id }` and `agents.list { workspace_id }` (when the group `agents` is present, for the agents' names), then draws the result above the estimate. While the replies are awaited the estimate is drawn alone. An error reply of `SVC_NOT_IN_THIS_VERSION` (a backend whose catalogue lacks the command) is not drawn: the block draws its estimate only, as before. Any other error reply draws its sentence through `ErrorLine` and the estimate stays. A failed `agents.list` gives every agent the name `block-crew-agent-unknown`.
    2. **Purpose.** `purpose` is drawn as text, in a paragraph above the estimate.
    3. **Agents.** Under the heading `block-crew-members-heading`, one line per entry of `members`, in order, `block-crew-member` with `agent` the display name of `member` when it is an `agent_instance` that `agents.list` holds and `block-crew-agent-unknown` otherwise, and `role` the entry's `role` as text.
    4. **Limits.** Under the heading `block-crew-limits-heading`, in this order: `block-crew-limit-rounds` (`max_rounds`), `block-crew-limit-time` (`wall_clock_secs` divided by 60, rounded up, as minutes), `block-crew-limit-requests` (`request_budget`; not drawn when it is 0, which is no budget: CrewOrchestrator.md, `limits.crew.request_budget` none until OQ-3), `block-crew-limit-running` (`max_members_running`) and `block-crew-limit-repeat` (`failure_repeat_limit`), each with `count` (or `minutes`) formatted by `formatNumber`; then, when `approval_points` is not empty, `block-crew-approval-points` with `points` the entries joined by ", ". `members_allowed_external_actions` is not drawn: the plan screen of the Ordin Host adds those, not this block.
    5. **Start.** When `state` is `valid`, the plan is still waiting (the conversation's item for the block is waiting, as `Question` reads it), the backend is not waiting for its server and `groups` include `crews`, the block draws **Start** (`block-crew-start`). Pressing it sends `crews.start { crew_charter_id }` once at a time (the button is disabled while the reply is awaited). An `ok` reply marks the item answered, as `Question` does, and the block then draws `block-crew-started` in place of the button; an error reply draws its sentence through `ErrorLine` and the button stays. When `state` is `started` the block draws `block-crew-started`; when `state` is `written`, `discarded`, `kept` or `archived` it draws `block-crew-not-startable`; in both cases there is no button.
    6. **Words (en-GB, `screens-blocks.ftl`).**

       | Message id | Text |
       |---|---|
       | `block-crew-members-heading` | Agents in this crew |
       | `block-crew-member` | `{ $agent }: { $role }` |
       | `block-crew-agent-unknown` | An agent |
       | `block-crew-limits-heading` | Limits |
       | `block-crew-limit-rounds` | Rounds: up to `{ $count }` |
       | `block-crew-limit-time` | Time: up to `{ $minutes }` min |
       | `block-crew-limit-requests` | Requests: up to `{ $count }` |
       | `block-crew-limit-running` | Agents at the same time: up to `{ $count }` |
       | `block-crew-limit-repeat` | Same failure allowed: `{ $count }` times |
       | `block-crew-approval-points` | Asks you first: `{ $points }` |
       | `block-crew-start` | Start |
       | `block-crew-started` | Started |
       | `block-crew-not-startable` | This plan can't be started. |

    7. SPEC-GAP-4 (*Gaps* above) is closed: the marker in `CrewPlan.svelte` is removed and its row is replaced by this rule.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a crew plan shows its purpose, agents and limits (SM-WR-15)
    Given a crew_plan block whose charter has purpose "Compare the three quotes", members "Kestrel buyer" as Finder and "Borealis" as Checker, and max_rounds 6
    When the block is drawn
    Then the purpose, "Kestrel buyer: Finder", "Borealis: Checker" and "Rounds: up to 6" are drawn above the estimate
    And Start is offered

  Scenario: Happy path — Start sends crews.start once (SM-WR-15)
    Given the plan above is waiting and its state is "valid"
    When the person presses Start twice quickly
    Then crews.start is sent once with the crew_charter_id
    And "Started" is drawn in place of the button

  Scenario: Error path — Start is refused (SM-WR-15)
    Given crews.start answers OH_CREW_RUNNING
    When the person presses Start
    Then the error's sentence is drawn and Start stays

  Scenario: Edge case — an older backend has no crews.charter (SM-WR-15)
    Given the backend answers SVC_NOT_IN_THIS_VERSION to crews.charter
    When the block is drawn
    Then only the estimate is drawn, with no error line and no Start

  Scenario: Edge case — a plan that was started elsewhere (SM-WR-15)
    Given the charter's state is "started"
    When the block is drawn
    Then "Started" is drawn and there is no Start button

  Scenario: Edge case — an agent that agents.list does not hold (SM-WR-15)
    Given a member that is a remote agent
    When the block is drawn
    Then its line reads "An agent: <role>"
```

### Gaps

None.

## Amendment January 2026 — The look of the screens (ADR-116)

> Source: decision 98 (the user's decision of January 2026); HLD revision 0.65.0, ADR-116 (the prototype's appearance is the reference for how the desktop's screens look; accessibility wins over the prototype; one token vocabulary from brand pack to screen; checked by exact values and side-by-side review), UX-A82, 12 §2. Values: `cross-cutting/VisualDesign.md` (Draft, January 2026). Owner of the token names and the default pack: `components/Brand.md`, amendment January 2026.

What a screen shows and does is unchanged; this amendment fixes how it looks. Where it disagrees with an earlier part of this spec on a value, this amendment wins.

1. **Values.** `src/styles/tokens.css` holds every property of `cross-cutting/VisualDesign.md` V1 to V6, in the three theme scopes of its P3, and nothing else; the earlier properties are removed and each use is replaced as follows, then adjusted to its scene in the review of item 6: `--color-bg` → `--color-background`; `--color-surface` keeps its name (new values); `--color-border` → `--color-line` for dividers and outlines, `--color-border-control` for buttons, text fields and selects; `--color-text` keeps its name; `--color-muted` → `--color-text-muted`; `--color-accent` keeps its name; `--color-accent-text` (text on an accent fill) → `--color-on-accent`; `--danger-color` → `--color-danger`; `--waiting-color` → `--color-waiting-text`; `--focus-ring` → `3px solid var(--color-focus)` with offset 2 px; `--space-1` (4 px) → `--space-04`; `--space-2` (8 px) → `--space-sm`; `--space-3` (12 px) → `--space-12`; `--space-4` (16 px) → `--space-md`; `--radius` (6 px) → `--radius-md`; `--font-body` keeps its name with the new stack; `--chart-1` … `--chart-8` keep their names. No new property reuses a removed name. `body` takes `--color-background`, `--color-text`, `--font-body` and the *Body* step (16 px / 1.45), replacing 14 px.
2. **Brand tokens and fonts.** The module ships Jost (`apps/desktop/ui/src/assets/fonts/Jost-wght.ttf` and `apps/desktop/ui/src/assets/fonts/OFL.txt`) and adds a pack's font files as `cross-cutting/VisualDesign.md` P4 states; a file that fails to load records `UI_BRAND_FONT_FAILED`. `brandTokens` and `applyBrandToDocument` follow `cross-cutting/VisualDesign.md` P2 (replacing the rule that wrote a token `color.text.dark` as the property `--color.text.dark`, which no style rule read); a token P2 does not map, or with a refused prefix, is not applied and records `UI_BRAND_TOKEN_IGNORED` in the diagnostics ring (step 12) with the token's name. `PROTECTED_TOKEN` becomes the four refused prefixes `color.danger.`, `color.waiting.`, `color.run-state.`, `color.approval.`.
3. **Panel widths and the narrow window.** The defaults of the two view preferences change: `ordin.view.panel.rail` 290 (was 280), `ordin.view.panel.conversation` 420 (was 380); ranges and keys unchanged; a stored value is kept. Step 15's narrow mode starts at a window of 1,100 CSS pixels or less (was: below 1,024), where the conversation slides over at `min(420px, 92vw)` and the side rail is at most 40 % of the window's width (prototype `@media (max-width:1100px)`).
4. **Charts.** Step 8 keeps `--chart-1` … `--chart-8`, now with the values of V2, light and dark; the chart re-renders on a theme change.
5. **Code editor and terminal.** Monaco's themes `ordin-light` and `ordin-dark`, and the terminal's colours, follow `cross-cutting/VisualDesign.md` Internal Logic step 6.
6. **Scenes.** Each block kind of step 6 and each view of step 9 is drawn as the scene or scenes below show it (`docs/design/ordin-desktop/user-interface.html#scene=<id>`), with the values of `cross-cutting/VisualDesign.md`; a change to its drawing passes that spec's side-by-side review (Internal Logic step 7). Where the screen's data differs from the scene's sample data, the arrangement, palette roles, type steps, spacing and radii still match. The prototype's own chrome — the dark bar with the **Show** menu and the caption line — is never drawn.

   | Block kind (step 6) | Scene ids |
   |---|---|
   | `message` | `free`, `goBetween`, `goBetweenWords`, `translate` |
   | `question` | `ambiguous`, `agentAsks` |
   | `form` | `form`, `docsDrag` |
   | `documents` | `docs`, `find` |
   | `preview` | `cfPreview`, `docs` |
   | `document` | `md`, `mdNew` |
   | `changes` | `changes`, `sendBack` |
   | `plan_progress` | `progress`, `progressChanged`, `progressAdded`, `runs` |
   | `table` | `reportTable`, `tableLong` |
   | `chart` | `report`, `chartKinds` |
   | `key_figures` | `report` |
   | `report` | `report`, `reportCannot`, `dashKeep` |
   | `email_list` | `mailRead` |
   | `email` | `mailDraft`, `mailSteer` |
   | `telegram_list` | `tgRead` |
   | `slack_list` | `skChannel` |
   | `result` | `produced`: `free`; `offer`: `flowSave`; `checklist`: no scene (UI-D116) — drawn as a `documents` list with a tick per row, with V6's values |
   | `agent_proposal` | `addAgent` |
   | `agent_library` | `library`, `libraryRepos` |
   | `crew_plan` | `crew`, `useEstimate` |
   | `memory` | `memory` |
   | `knowledge` | `know`, `knLinks` |
   | Approval card (step 7) | `reject`, `typedYes`, `slack`, `hoCard`, `mailSend` |

   | View (step 9 and others) | Scene ids |
   |---|---|
   | Side rail and tree (`WorkspaceTree`) | `tree`, `catchup`, `srvTree` |
   | Screen area and tabs (`ScreenArea`, `ScreenTabs`) | `switch`, `docs` |
   | Conversation panel (`ConversationPanel`) | `long`, `direct`, `needsTab` |
   | *Needs you* (`NeedsYouTab`) | `needsTab` |
   | Home (`Home`) | `home`, `homeGrouped` |
   | Search or go to… (`SearchDialog`) | `findAll`, `findCount` |
   | Settings (`Settings/*`) | `settings`, `useSettings`, `voiceSettings`, `historySetting` |
   | Diagnostics (`Diagnostics`) | `settings` |
   | Confirmation (`ConfirmDialog`) | `removeWs`, `repoRemove`, `jiraRemove` |
   | Quit (`QuitDialog`) | no scene (UI-D116) — drawn as the Confirmation's dialog |
   | Earlier conversations | no scene (UI-D109) — drawn as a `documents` list, one row per conversation |
   | Engineering views | `eng`, `engEdit`, `engTwo` |
   | Project settings (`ProjectSettings`) | `eng` |
   | Check it (`CheckItPanel`) | `checkAgent` |
   | Add to this workspace | `newWs`, `addCode` |
   | The workflow screen and editor | `flow`, `flowEdit` |
   | The agent draft | `makeDraft` |
   | Add an agent repository… | `repoAdd` |
   | Parts of [agent repository], part draft, Withdraw | `partsBiz`, `partDraft`, `partWithdraw` |
   | Handover draft, handover screen, Lifecycle: [name] | `hoDraft`, `hoScreen`, `hoLifecycle` |
   | What [agent] is made of, the part page, Show the installed files | `madeOf`, `partRead`, `installedFiles` |
   | The import screen | `importStart` |
   | Settings › Slack (`Settings/Slack`) | `settingsSlack` |
   | Add to this workspace › Slack channel… (`AddSlackChannel`) | `skLink` |
   | Slack channel screen (`SlackChannel`) | `skChannel` |
   | Errors (`ErrorLine`, step 11) | `errors` |
   | First start (setup, the Setup Assistant's screens) | `setup`, `signinFirst` |
   | Narrow windows (step 15) | any scene above with the window 1,100 pixels wide or less |

   A view or block kind added by a later amendment names its scene in that amendment, or states that none exists and the divergence row that records it.

### Error Table — added row

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A token of the active brand pack is not mapped, or has a refused prefix | internal (diagnostics ring only; not applied) | `UI_BRAND_TOKEN_IGNORED` | `{ at, code, backend, detail: "<token name>" }` |
| A font file of the active brand pack fails to load or decode | internal (diagnostics ring only; skipped) | `UI_BRAND_FONT_FAILED` | `{ at, code, backend, detail: "font <index>" }` |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: The look of the screens (ADR-116)

  Scenario: Happy path — Home in the light theme takes the specified values
    Given ordin's own brand pack is active and display.appearance is "light"
    When the window shows Home
    Then the computed background-color of <body> is rgb(242, 244, 247)
    And the computed background-color of the side rail is rgb(255, 255, 255)
    And the computed font-size of <body> is 16px
    And the first family of the computed font-family of <body> is "Jost"

  Scenario: Happy path — a brand pack's accent reaches the tree
    Given an active pack whose color.accent.light is "#0B5FFF"
    And display.appearance is "light"
    When the person opens a workspace from the tree
    Then the computed border-color of its selected tree row is rgb(11, 95, 255)

  Scenario: Edge — the side rail opens at the prototype's width
    Given no stored value for ordin.view.panel.rail
    When the window opens at 1,440 pixels wide
    Then the side rail is 290 pixels wide and the conversation panel 420 pixels wide

  Scenario: Edge — the conversation slides over at 1,100 pixels
    When the window is 1,100 pixels wide
    Then the conversation is not a column and opens over the screen from Conversation in the header

  Scenario: Error — a token with a refused prefix is not applied
    Given brand.get returns design_tokens with color.approval.light "#000000"
    When an approval card is drawn
    Then the computed border-color of the card is rgb(31, 79, 184)
    And the diagnostics ring holds UI_BRAND_TOKEN_IGNORED with detail "color.approval.light"

  Scenario: Edge — the prototype's chrome is not drawn
    When the window shows any screen
    Then no element carries the Show menu or the caption line of the prototype
```

### Tests (vitest) — added

One per scenario above; plus the tests named in `cross-cutting/VisualDesign.md` *Acceptance Criteria*, which live in this module. The review of step 6's scenes is recorded in each change's description and is not a vitest test.

## Amendment January 2026 — Icons (ADR-117)

> Source: decision 99 (the user's decision of January 2026); HLD revision 0.66.0, ADR-117 (the prototype's icons, shipped with ordin and replaceable by name), UX-A83, UX-A14. Names, drawing, sizes, placement and the replacement rule: `cross-cutting/VisualDesign.md` V8 and Internal Logic step 8. The build pack's icon set: `components/Brand.md`, amendment *The prototype's icons in the build pack*.

What a screen shows and does is unchanged; this amendment adds icons beside its words.

1. **SM-IC-1 The sprite.** The module ships `apps/desktop/ui/src/assets/icons/ordin-icons.svg`, byte for byte the build pack's `crates/ordin-brand/build-brand/icons-light.svg`, and `apps/desktop/ui/src/lib/icons/sprite.ts` exports `ICON_NAMES` (the 50 icon names of V8 without their `i-` prefix, in V8's order, as a `readonly` tuple: `'chev'`, `'ws'`, … `'unfold'`), `type IconName = typeof ICON_NAMES[number]`, `installSprite()` and `applyBrandIcons(backend)`, which carry out `cross-cutting/VisualDesign.md` Internal Logic step 8. Start-up (step 1) calls `installSprite()` before the first screen is drawn and `applyBrandIcons` after `brand.get` answers; the `brand_changed` handler and a change of the effective theme call `applyBrandIcons` again. A test compares the two files' bytes, so a change to one without the other fails the module's gate.
2. **SM-IC-2 The icon part.** `apps/desktop/ui/src/lib/icons/Icon.svelte` takes `name: IconName` and `size?: 'default' | 'small' | 'twist' | 'mark' | 'needs-tab' | 'close' | 'heading'` (the sizes of V8 *Drawing*, default `'default'`) and draws `<svg class="icon icon-<size>" aria-hidden="true" focusable="false"><use href="#i-<name>"/></svg>`, nothing else. The stroke, size and direction rules live in `src/styles/tokens.css` under `.icon` (V8). No component draws one of the 50 icons any other way, and no component writes a character (such as `›`, `×`, `⋯`) in place of an icon V8 names. The status glyphs of V8 *Status glyphs are not icons* — the run-state dot's glyphs, the problems and tests line's ✓, ⚠ and ?, and the **Outside the company** mark's ↗ — stay text in `aria-hidden` spans; they are the icons step 9's run states, SM-SF-2's problems line and the Email kinds' **Outside the company** mark require.
3. **SM-IC-3 Where.** Each place in V8 *Where icons are drawn* and *Actions* that the screens draw today puts the icon part before its words, in the tile colour V8 names. In particular: the tree's toggle draws `twist` `i-chev` in place of the character `›` and keeps its `aria-label`; each tree group row draws its group's icon; the one-line block card (step 6) draws its kind's icon; a screen tab draws its kind's icon and its **Close** button `i-remove` in `close` size. The only buttons whose sole content is an icon are those V8 *Accessibility* lists, with these catalogue messages (`crates/ordin-localisation/catalogues/en-GB/screens.ftl`; `aria-label` / `title`):

   | Button | `aria-label` | `title` |
   |---|---|---|
   | A screen tab's **Close** | `screen-tab-close` = `Close { $title }` (new) | `screen-tab-close` |
   | A conversation tab's **Close** | `conv-tab-close-named` = `Close the conversation tab for { $name }` (new) | `conv-tab-close` (existing: `Close tab · the conversation is kept`) |
   | **Hide the conversation** | `conv-hide` = `Hide the conversation` (new) | `conv-hide` |
   | **Dictate** | `message-dictate` = `Dictate` (new) | `message-dictate-tip` = `Speak instead of typing` (new) |
   | A tree row's or a document screen's **More actions** | `more-actions-for` = `More actions for { $name }` (new) | `more-actions-tip` = `More actions` (new) |
   | A tree row's action | the action's words (its message in the node's actions menu) | the same |

   A tooltip that differs from the accessible name (**Dictate**, a conversation tab's **Close**, **More actions**) follows the prototype's `title`; the accessible name keeps the verb. The screen tab's **Close** stops using `conv-tab-close`, whose words are about a conversation; its tooltip is `screen-tab-close`, not the prototype's "Close · Delete", because no Delete shortcut for tabs is specified (a deliberate difference from the scene). An icon-only button anywhere else is a defect.
4. **SM-IC-4 Direction.** Under `[dir="rtl"]` the `.icon` rule mirrors `i-chev`, `i-send`, `i-open` and `i-history` (V8 *Direction*); an open twist turns 90° without mirroring.

### Error Table — added row

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The active pack's icon set for a theme is a PNG, cannot be parsed, carries no icon name, or cannot be fetched | internal (diagnostics ring only; ordin's own icons are drawn) | `UI_BRAND_ICONS_IGNORED` | `{ at, code, backend, detail: "icon set <theme>: png" \| "…: unreadable" \| "…: no named icons" \| "…: unavailable" }` |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Icons (ADR-117)

  Scenario: Happy path — the tree draws its icons
    Given a workspace "Supplier review" with documents, agents and Knowledge
    When the person opens it in the tree
    Then the workspace row draws i-ws, the Documents row i-docs, the Agents row i-agent and the Knowledge row i-know
    And each icon is aria-hidden and not focusable

  Scenario: Happy path — a one-line block card draws its kind's icon
    Given a plan_progress block shown in the conversation as a one-line card
    Then the card draws i-run before its title in the accent text colour

  Scenario: Edge — the tree toggle draws the twist icon
    Given a closed workspace row
    Then its toggle holds the icon i-chev and no text character
    And when the row is opened the icon is turned 90 degrees

  Scenario: Edge — a run state keeps its glyph
    Given a run in the state "finished"
    Then its run-state dot holds the text glyph ✓ in an aria-hidden span beside the label
    And no icon of the sprite is drawn for the state

  Scenario: Edge — the two sprite files are the same
    When the module's tests run
    Then apps/desktop/ui/src/assets/icons/ordin-icons.svg has the same bytes as crates/ordin-brand/build-brand/icons-light.svg

  Scenario: Error — a company icon set that cannot be fetched
    Given an active company pack and brand.get_asset for icon_set_light answers with an error
    When the screens apply the pack in the light theme
    Then every symbol of the sprite is ordin's drawing
    And the diagnostics ring holds UI_BRAND_ICONS_IGNORED with detail "icon set light: unavailable"

  Scenario: Edge — no icon-only button outside the allowed places
    When the review page draws every scene of step 6's tables
    Then every button whose only content is an icon is a tree row's action, a tree row's or document screen's More actions, a screen tab's or conversation tab's Close, Hide the conversation or Dictate
    And it has a non-empty accessible name and a non-empty title
```

### Tests (vitest) — added

One per scenario above; plus the icon tests named in `cross-cutting/VisualDesign.md` *Acceptance Criteria*. Each changed screen part passes the side-by-side review of `cross-cutting/VisualDesign.md` Internal Logic step 7 with its icons.


## Amendment January 2026 — First start, Add to this workspace and removing a link

> **Status:** Approved (user, 28 January 2026: "proceed with recommendations" — the same window, and no GitHub or GitLab entries in this build). Source: the user's decisions of 28 January 2026 (first start, *Add to this workspace* and removing a link are in scope now; first start is drawn before the main window); 11 §5.1 *Getting ready*, 11 §5.13 *Workspaces* (UX-A23, UX-A57); scenes `setup`, `setupReady`, `noseat`, `newWs`, `addCode`, `jiraRemove` of `docs/design/ordin-desktop/user-interface.html`. The commands are the ones the Service API already carries (`components/SetupAssistant.md` §A–C, `components/WorkspaceService.md` §3, `components/SharePointAdapter.md`, `components/ConfluenceAdapter.md`, `components/IdentityAndCredentials.md`); this amendment adds no command, event or stored value.

Step 9's view table names *First start (setup, the Setup Assistant's screens)* and *Add to this workspace* but states no view for either, and its tree row lists `files.add_place`, `workspaces.add_link` and `workspaces.remove_link` without a control that sends them. This amendment states the three.

### First start

1. **SM-FS-1 When it is drawn.** Start-up (step 1), after the catalogue is loaded and before the window's three regions are drawn, sends `setup.status {}` to the local backend. When `needs_guided` is `true`, the window draws only `FirstStart` (`apps/desktop/ui/src/lib/views/setup/FirstStart.svelte`), centred, at most 440 pixels wide (scene `setup`), on the ground colour; the side rail, the screen area and the conversation are not drawn and receive no focus. When `needs_guided` is `false`, the regions are drawn as before and `FirstStart` is never drawn in this run. A failed `setup.status` is drawn by step 11 inside `FirstStart`'s frame with **Try again**, which sends `setup.status` again. *(Chosen here, from the user's decision "before the main window": the program keeps its one window (`components/OrdinDesktop.md`) and draws first start in it in place of the regions; no second window is opened.)*
2. **SM-FS-2 The guided run.** `FirstStart` sends `setup.run_guided {}` once, then draws the progress it receives: the `progress` of the `setup.status` answer first, then each `setup_progress` event's `progress` (event filter of step 3: `setup_progress` from the local backend only). It draws:
   - the brand tile (step 9's side-rail brand, V6 *Brand tile*) and the heading `setup-getting-ready`;
   - `setup-progress-count` with `$done` and `$total`;
   - one row per `ProgressStep`, in order: a run-state dot (V6) for its `state` (`waiting` → not yet, `running` → now, `done` → done, `needs_you` → waiting, `failed` → failed), its words `t(step.message_id, { harness })`, where `harness` is `harness-claude-code` or `harness-copilot` for the `harness` of the answer's first attachment (`harness-claude-code` when there is none), and, for a screen reader, the state's word before the step (`setup-step-state-<state with _ as ->`, visually hidden);
   - `setup-sends-to` with `$harness`, as one small line. Nothing about accounts, passwords or administrators is shown: a business user is never told about technical set-up (user, 28 January 2026).
3. **SM-FS-3 Signing in.** While a step `harness_sign_in` is in state `needs_you`, `FirstStart` draws **Sign in to { $harness }** (`setup-sign-in`, primary) with `setup-sign-in-note` under it. Pressing it sends `setup.sign_in_harness { harness }` (the first attachment's `harness`, `claude_code` when there is none) and disables the button with `setup-signing-in` until the answer. The guided run resumes by itself when the attachment becomes `ready` (SetupAssistant.md §A.4); `FirstStart` sends nothing more. An error answer is drawn by step 11 under the button, and the button is enabled again.
4. **SM-FS-4 The end.** When `setup.run_guided` answers:
   - `ready = true` → `FirstStart` is removed, the three regions are drawn as at an ordinary start, and Home is in front (step 9).
   - `ready = false` → `FirstStart` stays and draws, under the steps, each `CheckResult` whose `outcome` is `failed` as `t(message_id, message_args)` with its `action` as one button: `try_again` → **Try again** (`setup-try-again`, sends `setup.run_guided {}` again), `sign_in` → as SM-FS-3. A failed result with any other action, including `send_to_it`, `ask_it_for_claude_access` and `ask_it_for_copilot_access` (SM-ER-3), draws its sentence only. Nothing lets the person go past first start while no harness is ready: Ordin cannot work without one (OrdinHost.md).
   - An error answer (a `ServiceError`) → step 11 inside the frame, with its own action.
5. **SM-FS-5 Words of the steps and results.** Each `CheckKind` has the step message `setup-step-<kind with _ as ->` and each Setup Assistant result code its message `<code lower-cased, _ as ->` (the rule of `crates/ordin-setup/src/checks.rs`), in the catalogue table below.
6. **SM-FS-6 Commands that wait for the person** (added 28 January 2026, found while building SM-FS-3). Step 11's 10-second limit for a command that waits for no model does not apply to `setup.run_guided`, `setup.sign_in_harness` and `identity.sign_in_wait`: each waits for the person to sign in in the browser (up to 10 minutes, SetupAssistant.md §B.2; IdentityAndCredentials.md), so each is awaited without a limit, as the `ordin.*` commands are.
7. **SM-FS-7 Why a step failed, at once** (user, 28 January 2026: "proceed with recommendations"). While the guided run goes on, a step whose `state` is `failed` and whose `problem` is set (SetupAssistant.md SET-R1-3) draws, under its words, `t(problem.message_id, problem.message_args)` in small muted type, so no failure mark stands without its reason. Once SM-FS-4 draws the failed results with their actions, the line under the step is no longer drawn, so the sentence is not shown twice.

### Add to this workspace

6. **SM-AW-1 The screen.** *Add to this workspace* is a screen of the workspace's screen area (`apps/desktop/ui/src/lib/views/add/AddToWorkspace.svelte`, screen id `add-to-workspace`, one per workspace, title `add-title`), opened by the workspace row's **⋯** menu item **Add to this workspace…** (`tree-add`, icon `i-plus`), by the **+** action of the Documents row (`tree-add-documents`, icon `i-plus`) and by step 9's `add_documents` search action. It opens on its first step, *Where it lives*: `add-intro` (muted) and one row per entry, each a button with its icon (V8), its name in bold and its line in small muted type, and `add-linked` (a state label, `done` tone) when the workspace already holds a link of that kind:

   | Entry | Name / line ids | Icon | Opens |
   |---|---|---|---|
   | SharePoint | `add-sharepoint`, `add-sharepoint-line` | `i-sp` | SM-AW-2 with `sharepoint.find` |
   | Confluence | `add-confluence`, `add-confluence-line` | `i-cf` | SM-AW-2 with `confluence.find` (desktop backend only) |
   | This computer | `add-local`, `add-local-line` | `i-folder` | SM-AW-3, kind `local_folder` |
   | A code repository on this computer | `add-code`, `add-code-line` | `i-code` | SM-AW-3, kind `git_repository_folder` |
   | Jira issue… | `jira-add-to-workspace` | `i-task` | the Jira dialog (Jira amendment item 2) |
   | Email… | `add-email`, `add-email-line` | `i-mail` | the existing Email link dialog; only with `mail` on |
   | Slack channel… | `slack-add-to-workspace` | `i-chat` | the existing Slack dialog; only with `slack` on |

   *Not in this build (G-AW-2):* the prototype's GitHub and GitLab document entries and its list of code repositories a sign-in can reach. The Service API has no command that lists or clones a remote repository, and a `git_remote` link is never read (WorkspaceService.md §3.2).
7. **SM-AW-2 SharePoint and Confluence.** A search field (`add-search`, label `add-search-label`) sends `sharepoint.find` or `confluence.find` `{ query, limit: 25 }` 300 ms after the last keystroke (2 to 200 characters; empty → the list is cleared). Each answer is one row: the location's name in bold (`site_name › path` or `space_name › title`) and, small and muted, its address, with **Add** (`add-add`, small). **Add** sends `workspaces.add_link { workspace_id, location: { kind: "sharepoint" | "confluence", location } }`. **Back** (`add-back`, small) returns to *Where it lives*.
   - An answer or error with code `SP_SIGN_IN_NEEDED`, `CONF_SIGN_IN_NEEDED` or `WS_SOURCE_SIGN_IN_NEEDED` replaces the list with the sign-in step: heading `add-sign-in-microsoft` or `add-sign-in-confluence`, the line `add-sign-in-line`, and **Sign in with Microsoft** / **Sign in to Confluence** (primary) sending `identity.sign_in { kind: "microsoft" | "confluence", reason: t('add-sign-in-reason') }` then `identity.sign_in_wait`, as Settings › Sign-ins does; a successful wait sends the last search again.
   - `CONF_NOT_CONFIGURED` or `WS_SOURCE_NOT_CONFIGURED` → `add-confluence-not-configured` with **Open Settings** (`add-open-settings`), which opens Settings › Sign-ins.
   - Any other error → step 11 under the field.
8. **SM-AW-3 A folder on this computer.** `add-choose-folder` (primary) sends `app.pick_folder {}`; no folder chosen → nothing changes. A folder → `workspaces.add_link { workspace_id, location: { kind, path: folder } }` with the entry's kind. `WS_NOT_A_REPOSITORY`, `WS_PLACE_REFUSED` and every other error → step 11 under the button. This entry is not drawn for a server's workspace (`WS_LOCAL_FOLDER_ON_SERVER`).
9. **SM-AW-4 Added.** A successful `workspaces.add_link` draws the step *Added*: heading `add-added`, the line `add-added-line` with `$name` (the `SourceLink.display_name`), and **Show documents** (`add-show-documents`, opens the workspace's Documents screen) and **Add something else** (`add-more`, back to *Where it lives*). While `workspaces.link_progress { source_link_id }` answers `done = false` (sent every 2 s while the screen is in front, stopping at `done`), the line `add-reading` with `$read` and `$listed` is drawn under it. The tree's Documents children follow the `workspace_changed` event (step 3).

### Removing a link

10. **SM-RL-1 Documents by where they live.** Under each workspace's Documents row, the tree draws one child row per active `SourceLink` of `workspaces.links { workspace_id }` (sent when the Documents row is first opened and again on each `workspace_changed` for the workspace while it is open), grouped and ordered by kind: SharePoint, Confluence, This computer, code repositories, Jira, each row with its kind's icon (V8) and `display_name`; `proposed` links are not drawn. A row's main action opens the link's documents (step 9's documents list for its place); a Jira link's opens its issue's item list.
11. **SM-RL-2 The row's menu.** Each link row has the tree's **⋯** menu (`tree-node-menu`, step 9) holding **Remove from this workspace…** (`tree-remove-link`, icon `i-remove`, danger). It opens step 9's confirmation (`ConfirmDialog`): title `remove-link-title` with `$name`, body `remove-link-body`, plus `jira-remove-line` for a `jira_issue` link (Jira amendment item 3), verb `remove-link-verb` in the danger style. Confirming sends `workspaces.remove_link { source_link_id }`; success adds the line `remove-link-done` with `$name` to the conversation's status line (step 9) and the row goes on the next `workspace_changed`; an error is drawn by step 11 in the dialog, which stays open. Cancel sends nothing.

12. **SM-RL-3 A location opens to its folders and documents** *(amended January 2026; replaces the narrowing of SM-RL-1 to one row per link; closes plan 05 FU-009)*. A link row whose link has a place (any kind but a Jira issue) shows, once pressed, under it: the place's sub-folders and documents, read with `files.list_folder { place_id, folder, sort: 'name_asc' }` (the root with `folder` empty), folders first as the command orders them, each row with its icon (`i-folder`, `i-page`) and `name`, indented one step deeper than its parent. A folder row opens or closes its own contents, read when it first opens and again on each `workspace_changed` for the workspace; a document row opens `document:<place id>:<path>` (step 9's document page). A link's main action still opens `documents:<place id>` (SM-RL-1) as well as showing its contents. When `next_cursor` is present **Show more** (`settings-show-more`) reads the next page; a folder with no items shows `tree-folder-empty`. A failed read draws the error line under the row, with Try again. A Jira link keeps its item list (SM-WR-5). *Status: Approved by the user, January 2026 ("close all the follow ups in plan 5 including FU-009"). Source: plan 05 FU-009; the prototype's `wsNodeOf` (user-interface.html) and 11 §4 draw each location with its sub-folders and documents; SM-RL-1 had been narrowed to one row per link. The prototype's count per location and its **Changed since you looked** row are not part of this amendment.*

13. **SM-RL-4 A document in the tree can be moved to the Bin** *(amended January 2026; user report on 0.1.1: "an md file was created but there is no way to delete it")*. *amended January 2026 (user decision; Approved by the user January 2026):* the row's ⋯ menu, also opened by a right-click on the row, holds, in this order, **Ask Ordin about this** (`block-ask-about-this`, icon `i-chat`, when the backend has the `ordin` group and answers `ordin.send_message`; it sends the message and attachment of SM-ASK-1 for the row's place and path, disabled while a server is not connected), **Open in its app** (`doc-open-in-app`, icon `i-open`, when it answers `craft.open`; `no_editor` draws `doc-no-editor` with **Open in your usual app** under the row, as SM-NS-4.2) and **Move to Bin…**; a confirmed move also closes the document's tab when it is open. *amended January 2026 (user decision; Approved by the user January 2026), again:* after **Ask Ordin about this** the document's menu holds **Add to knowledge** (`tree-add-to-knowledge`, icon `i-know`) and **Add to memory** (`tree-add-to-memory`, icon `i-memory`), and a folder row of SM-RL-3 gets a ⋯ menu (and right-click) holding **Add to knowledge** only; all are drawn under the same condition as **Ask Ordin about this**. No command makes a knowledge page or memory from a document (`knowledge.write` takes a finished page body; the `memory.*` commands act on what is already remembered), so each sends `ordin.send_message` as SM-ASK-1 does, with the row's place and path attached as one `place_file` and the text `tree-ask-text-knowledge` or `tree-ask-text-memory` with the row's name (Ordin hands a memory request to the Document agent, which remembers through its `remember` tool); the workspace's conversation is brought into view first, and folder access is asked on the usual **Allow** card. **Show in folder** and **Copy path** are not offered: no command reveals a file in the system's file manager, and a place-relative path is not a path the person can paste. Each document row of SM-RL-3 has the tree's **⋯** menu (`tree-node-menu`) holding **Move to Bin…** (`doc-move-to-bin`, icon `i-remove`, danger) when the backend answers `files.move_to_bin`. It asks with `ConfirmDialog` (`doc-bin-title`, `doc-bin-body`, verb `doc-bin-verb`); confirming sends `files.move_to_bin { place_id, path }`, announces `tree-bin-done` with `$name`, reads the folder again and shows `tree-bin-done` with **Undo** (`tree-bin-undo`) under the rows, which sends `files.restore_from_bin { ticket }` with the `BinTicket` the move returned and reads the folder again (the app's own Bin screen lists workspaces only, so this is the way back from the app; the file is also in the computer's own Bin); an error is drawn in the dialog, which stays open. Cancel sends nothing. A folder row has no menu.

### Catalogue — added (`crates/ordin-localisation/catalogues/en-GB/screens-setup.ftl`, new, and `screens.ftl`)

| Id | Words |
|---|---|
| `setup-getting-ready` | Getting you ready… |
| `setup-progress-count` | { $done } of { $total } done |
| `setup-step-state-waiting` / `-running` / `-done` / `-needs-you` / `-failed` | Not yet: / In progress: / Done: / Needs you: / Didn't work: |
| `setup-step-company-settings` | Checking your company's settings |
| `setup-step-harness` | Checking { $harness } is on this computer |
| `setup-step-harness-sign-in` | Signing you in to { $harness } |
| `setup-step-tokens` | Checking your sign-ins |
| `setup-step-agent-repositories` | Connecting the company's agent library |
| `setup-step-agent-folders` | Preparing your agents' folders |
| `setup-step-programs` | Checking the programs ordin uses |
| `setup-step-craft-apps` | Checking the company's apps |
| `setup-sends-to` | To do the work, Ordin sends what it reads to { $harness }. Your documents stay where they live. |
| `setup-sign-in` | Sign in to { $harness } |
| `setup-sign-in-note` | Opens { $harness }'s sign-in page in your browser. This is the only sign-in needed now. |
| `setup-signing-in` | Waiting for you to sign in in your browser… |
| `setup-try-again` | Try again |
| `harness-claude-code` / `harness-copilot` | Claude Code / GitHub Copilot |
| `setup-no-profile-address` | ordin can't reach your company's settings. |
| `setup-profile-unreachable` | Your company's settings can't be reached just now. ordin goes on with the last ones it has. |
| `setup-harness-missing` | Claude Code isn't on this computer yet. Install it, then choose Check again. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-FS-9) |
| `setup-harness-untested` | This version of Claude Code hasn't been checked with ordin yet. Install a checked version, then try again. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-FS-9) |
| `setup-harness-check-failed` | Claude Code couldn't be checked. |
| `setup-no-harness-access` | You don't have Claude access yet. |
| `setup-sign-in-timeout` | The sign-in wasn't finished in time. |
| `setup-sign-in-on-server` | Sign in on the computer this server runs on. |
| `setup-no-ready-harness` | ordin needs Claude Code, signed in, before it can start. |
| `setup-catalogue-failed` | Can't reach the company's agent library right now. |
| `setup-agent-folder-failed` | Your agents' folders couldn't be prepared. |
| `setup-program-refused` | This computer stopped a program ordin needs. Check the computer's security settings, then try again. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-FS-9) |
| `setup-program-damaged` | A program ordin needs is damaged. |
| `setup-program-failed` | A program ordin needs couldn't be set up. |
| `setup-craft-app-failed` | One of the company's apps couldn't be set up. |
| `add-title` | Add to this workspace |
| `add-intro` | Nothing is copied. Ordin keeps a link to where things live and only reads them until you approve a change. |
| `add-sharepoint` / `add-sharepoint-line` | SharePoint / Company sites and libraries, typically Word and PowerPoint |
| `add-confluence` / `add-confluence-line` | Confluence / Spaces and page trees |
| `add-local` / `add-local-line` | This computer / A folder on this computer |
| `add-code` / `add-code-line` | A code repository on this computer / A folder that holds a Git repository; it appears under Code |
| `add-email` / `add-email-line` | Email… / Messages from people or companies, or a folder in your mailbox |
| `add-linked` | Linked |
| `add-search-label` | Find a site, library, space or page |
| `add-add` | Add |
| `add-back` | Back |
| `add-sign-in-microsoft` / `add-sign-in-confluence` | Sign in with Microsoft / Sign in to Confluence |
| `add-sign-in-line` | Sign in once with your work account so Ordin can reach it; your browser opens. ordin keeps the sign-in on this computer. |
| `add-sign-in-reason` | To list the places you can add to this workspace |
| `add-confluence-not-configured` | Confluence isn't set up on this computer yet. |
| `add-open-settings` | Open Settings |
| `add-choose-folder` | Choose a folder… |
| `add-added` | Added |
| `add-added-line` | { $name } is linked to this workspace. |
| `add-reading` | Reading { $read } of { $listed } documents… |
| `add-show-documents` | Show documents |
| `add-more` | Add something else |
| `tree-add` | Add to this workspace… |
| `tree-add-documents` | Add documents… |
| `tree-bin-done` | Moved “{ $name }” to your computer's Bin. |
| `tree-bin-undo` | Undo |
| `tree-remove-link` | Remove from this workspace… |
| `remove-link-title` | Remove “{ $name }” from this workspace? |
| `remove-link-body` | Your documents stay where they are. Agents in this workspace can no longer use them. |
| `remove-link-verb` | Remove |
| `remove-link-done` | Removed the link to “{ $name }”. The documents are untouched where they live. |

The step 9 view table's row *First start* now names `FirstStart` (SM-FS-1 to SM-FS-5), and its row *Add to this workspace* names `AddToWorkspace` (SM-AW-1 to SM-AW-4).

### Error Table — added rows

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `setup.status` fails at start | the error is drawn in `FirstStart`'s frame with **Try again**; the regions are not drawn | (the reply's code) | — |
| The guided run ends with no ready harness | `FirstStart` stays, with each failed result and its action | (each `CheckResult.code`) | — |
| A search needs a sign-in | the sign-in step replaces the list | `SP_SIGN_IN_NEEDED`, `CONF_SIGN_IN_NEEDED`, `WS_SOURCE_SIGN_IN_NEEDED` | — |
| Confluence has no site | `add-confluence-not-configured` with **Open Settings** | `CONF_NOT_CONFIGURED`, `WS_SOURCE_NOT_CONFIGURED` | — |
| A folder that holds no repository is chosen for a code repository | step 11 under the button | `WS_NOT_A_REPOSITORY` | — |
| A protected folder is chosen | step 11 under the button | `WS_PLACE_REFUSED` | — |
| The same location is added twice | step 11 under **Add** | `SRC_DUPLICATE` | — |
| Removing a link fails | step 11 in the dialog, which stays open | (the reply's code) | — |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a first start signs in and reaches Home (SM-FS-1 to SM-FS-4)
    Given setup.status answers needs_guided true with Claude Code found and signed out
    When ordin starts
    Then the window draws only "Getting you ready…" and no side rail, screen area or conversation
    And setup.run_guided is sent once
    When a setup_progress event marks harness_sign_in as needs_you
    Then "Sign in to Claude Code" is drawn
    When the person presses it
    Then setup.sign_in_harness is sent with harness "claude_code"
    When setup.run_guided answers ready true
    Then the three regions are drawn and Home is in front

  Scenario: Edge case — already signed in through Claude Code, nothing is asked (scene setupReady)
    Given the guided run's harness_sign_in step passes with no needs_you state
    When ordin starts
    Then no sign-in button is drawn and Home follows when setup.run_guided answers ready true

  Scenario: Edge case — not a first start (SM-FS-1)
    Given setup.status answers needs_guided false
    When ordin starts
    Then FirstStart is not drawn and setup.run_guided is not sent

  Scenario: Error path — no Claude access (scene noseat, SM-FS-4)
    Given setup.run_guided answers ready false with a failed result SETUP_NO_HARNESS_ACCESS and no action
    Then FirstStart stays with "You don't have Claude access yet." and no button to ask anyone
    And the side rail is not drawn

  Scenario: Happy path — a SharePoint library is added (SM-AW-1, SM-AW-2, SM-AW-4)
    Given the workspace Q3 supplier review is in front
    When the person chooses Add to this workspace… and SharePoint and types "Suppliers"
    Then sharepoint.find is sent with query "Suppliers" and limit 25
    When the person presses Add on "Procurement › Suppliers"
    Then workspaces.add_link is sent with location kind "sharepoint"
    And "Added" is drawn with "Suppliers is linked to this workspace."

  Scenario: Error path — SharePoint needs a sign-in first (scene newWs, SM-AW-2)
    Given sharepoint.find fails with SP_SIGN_IN_NEEDED
    Then "Sign in with Microsoft" replaces the list
    When the person signs in and identity.sign_in_wait succeeds
    Then sharepoint.find is sent again with the last query

  Scenario: Error path — a folder that is not a repository (SM-AW-3)
    Given the person chooses A code repository on this computer and picks a folder with no .git
    When workspaces.add_link fails with WS_NOT_A_REPOSITORY
    Then its message is drawn under "Choose a folder…" and nothing is added

  Scenario: Happy path — removing a Jira link says nothing in Jira changes (scene jiraRemove, SM-RL-2)
    Given the workspace holds the Jira link "ABC-12"
    When the person chooses Remove from this workspace… on its row
    Then the confirmation reads "Remove “ABC-12” from this workspace?", "Your documents stay where they are. Agents in this workspace can no longer use them." and "Nothing in Jira changes."
    When the person confirms
    Then workspaces.remove_link is sent with the link's id

  Scenario: Edge case — Cancel removes nothing (SM-RL-2)
    When the person cancels the confirmation
    Then no command is sent
```

### Tests (vitest) — added

One per scenario above and per added error row, with the fake transport of `tests/support/fake-backend.ts`. Each new view passes the side-by-side review of `cross-cutting/VisualDesign.md` Internal Logic step 7 against scenes `setup`, `setupReady`, `noseat`, `newWs`, `addCode` and `jiraRemove`.

### Gaps

- **G-AW-1** *Opened by Ordin with a best match.* Scene `newWs` has Ordin open *Add to this workspace* at SharePoint with its best match first. No event carries such a request to the screens, and the Ordin Host makes no link call (WorkspaceService.md, OrdinHost.md X-7), so in this build the screen is opened by the person only.
- **G-AW-2** *Repositories on GitHub and GitLab.* No command lists the repositories a sign-in can reach or clones one, and a `git_remote` link is never read; the entries wait for those commands.

## Amendment 2026-01-28 — Getting memory ready (G-MD-3; OQ-MD-1)

> **Status:** Approved by the user 2026-01-28 ("proceed with recommendations"). Source: the user's decisions of 28 January 2026 — memory's models are fetched the first time memory is used, and the person is asked first in one plain sentence naming the size (174 MB) and the host, with **Download** and **Not now**; **Not now** leaves memory unavailable and asks again the next time memory is used. Counterparts: `components/ServiceApi.md` amendment 2026-01-28 (SVC-MD-1 to SVC-MD-3: `app.memory_models`, `app.download_memory_models`, `app.stop_memory_models_download`, `app.decline_memory_models`, the event `memory_models_changed`); `components/OrdinDesktop.md` amendment 2026-01-28 (DSK-MD-1 to DSK-MD-12 and its `DSK_MODELS_` codes).
>
> Evidence read: steps 1, 3, 9 and 11 above; SM-IC-2 and SM-IC-3 (icon part); `cross-cutting/VisualDesign.md` V8 (icon names `i-memory`, `i-stop`); `cross-cutting/ErrorHandling.md` EH-SEN-1 and EH-SEN-2.

1. **SM-MD-1 When the card is drawn.** At start (step 1), the screens send `app.memory_models {}` to the local backend and keep the answer; step 3's event filter adds `memory_models_changed` from the local backend only, whose `models` replaces it. The *Getting memory ready* card (`MemoryModelsCard`) is drawn at the top of the conversation region of every open conversation of the local backend, above the messages, while one of these holds: `ask` is `true`; `state` is `downloading` or `checking`; `state` is `failed` and the person pressed **Download** or **Try again** in this run; or `state` became `ready` less than 5 seconds ago. It is not a dialog: the conversation, the tree and the screens stay usable, and nothing takes focus from where the person is typing. A screen reader hears its heading once when it appears (`aria-live="polite"`). With `state` `not_in_build` it is never drawn. It is never drawn for a server's conversation.
2. **SM-MD-2 Asking.** With `ask` true and `state` `missing` or `failed`, the card draws the icon `i-memory` and the heading `memory-models-title`, then the sentence `memory-models-ask` with `$size` = `size_bytes` ÷ 1,000,000 rounded to the nearest whole number (174 for `models-1`) and `$host` = `host`, then **Download** (`memory-models-download`, primary) and **Not now** (`memory-models-not-now`). When `received_bytes` is above 0, the primary button reads `memory-models-resume` instead. **Download** sends `app.download_memory_models {}`; **Not now** sends `app.decline_memory_models {}` and the card goes when the answer's `ask` is false. Nothing else in the screens downloads.
3. **SM-MD-3 Progress.** With `state` `downloading`: the heading, the line `memory-models-progress` with `$received` and `$size` in whole MB (rounded down for `$received`), a progress bar (`role="progressbar"`, `aria-valuemin` 0, `aria-valuemax` 100, `aria-valuenow` = `received_bytes` × 100 ÷ `size_bytes` rounded down), and **Stop** (`memory-models-stop`, icon `i-stop`, secondary), which sends `app.stop_memory_models_download {}`. With `state` `checking`: the heading and `memory-models-checking`, no button. With `state` `ready` (for 5 seconds): `memory-models-ready`, then the card goes.
4. **SM-MD-4 Failure.** With `state` `failed` (SM-MD-1): the heading, the sentence of `failure` from the catalogue (`error-<code in kebab-case>`, EH-SEN-1; for example `DSK_MODELS_OFFLINE` → `error-dsk-models-offline`), and **Try again** (`memory-models-try-again`, primary), which sends `app.download_memory_models {}`, and **Not now**, as SM-MD-2. A `ServiceError` answered to any of the four commands is drawn by step 11 inside the card.
5. **SM-MD-5 Words.** No word of the card names a file, a fingerprint, a folder or a model: the person sees "memory", the size and the host.
6. **SM-MD-6 What memory's own screens show.** What Ordin remembers (step 9) is unchanged: it lists what memory holds, which needs no model.

### Catalogue — added (`crates/ordin-localisation/catalogues/en-GB/screens.ftl`)

| Id | Words |
|---|---|
| `memory-models-title` | Getting memory ready |
| `memory-models-ask` | To remember things for you, Ordin needs a one-time download of { $size } MB from { $host }. |
| `memory-models-download` | Download |
| `memory-models-resume` | Carry on downloading |
| `memory-models-not-now` | Not now |
| `memory-models-progress` | Downloading { $received } of { $size } MB… |
| `memory-models-stop` | Stop |
| `memory-models-checking` | Checking the download… |
| `memory-models-ready` | Memory is ready. |
| `memory-models-try-again` | Try again |

The failure sentences are the seven `error-dsk-models-…` messages of OrdinDesktop.md's added Error Table rows, in `errors-desktop.ftl`.

### Error Table — added rows

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The download ends in `failed` | the card shows the code's sentence with **Try again** and **Not now** | `DSK_MODELS_OFFLINE`, `DSK_MODELS_BLOCKED`, `DSK_MODELS_MISMATCH`, `DSK_MODELS_DISK_FULL`, `DSK_MODELS_INTERRUPTED`, `DSK_MODELS_WRITE_FAILED` | — |
| A command of the card is refused | step 11 inside the card | (the reply's code, for example `DSK_MODELS_NOT_IN_BUILD`) | — |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — asked, downloaded, ready (SM-MD-1 to SM-MD-3)
    Given app.memory_models answers state "missing", size_bytes 173982577, host "github.com", ask false
    When a memory_models_changed event arrives with ask true
    Then the conversation shows "Getting memory ready" and "To remember things for you, Ordin needs a one-time download of 174 MB from github.com."
    And the buttons "Download" and "Not now"
    When the person presses Download
    Then app.download_memory_models is sent
    When an event arrives with state "downloading" and received_bytes 87000000
    Then "Downloading 87 of 174 MB…" is shown with a progress bar at 50 and a "Stop" button
    When an event arrives with state "ready"
    Then "Memory is ready." is shown and the card goes 5 seconds later

  Scenario: Edge case — Not now (SM-MD-2)
    Given the card asks
    When the person presses Not now
    Then app.decline_memory_models is sent, the card goes and no download starts
    And the person can go on typing in the conversation

  Scenario: Edge case — resumed after a stop (SM-MD-2, SM-MD-3)
    Given the person pressed Stop at 50 MB and memory is used again later
    When the card asks
    Then its primary button reads "Carry on downloading"

  Scenario: Error path — offline (SM-MD-4)
    Given the person pressed Download
    When an event arrives with state "failed" and failure "DSK_MODELS_OFFLINE"
    Then the card shows "ordin couldn't connect to get memory ready. Check you're online, then try again." with "Try again" and "Not now"
    When the person presses Try again
    Then app.download_memory_models is sent

  Scenario: Error path — damaged download (SM-MD-4)
    When an event arrives with state "failed" and failure "DSK_MODELS_MISMATCH"
    Then the card shows "The memory download arrived damaged, so ordin didn't use it. Try again; if it happens again, choose Save logs for support… in Settings › Diagnostics."

  Scenario: Error path — full disk (SM-MD-4)
    When an event arrives with state "failed" and failure "DSK_MODELS_DISK_FULL"
    Then the card shows "There isn't enough free space on this computer to get memory ready. Free up about 350 MB, then try again."

  Scenario: Edge case — a build with no models (SM-MD-1)
    Given app.memory_models answers state "not_in_build"
    Then the card is never drawn
```

### Tests (vitest) — added

One per scenario above and per added error row, with the fake transport of `tests/support/fake-backend.ts`.

### Gaps

None. The card has no scene in `docs/design/ordin-desktop/user-interface.html`; it follows V6 and V8 without a side-by-side review until one is drawn.

## Amendment 2026-01-29 — The labels of a crew run's states

> **Status:** Approved 2026-01-29 (user decision: complete the high and medium follow-ups). Every rule above stays in force; SM-RS-1 to SM-RS-5 are unchanged and SM-RS-6 adds rows to them.
>
> Evidence read: the amendment *Run-state message ids* above (SM-RS-1 to SM-RS-5); `crates/ordin-types/src/domain/runs.rs` (`RunState`, `CrewRunState`: `starting`, `running`, `paused`, `finished`, `escalated`, `stopped_at_limit`, `killed`, `failed`); the generated `CrewRunState` and `RunState` in `apps/desktop/ui/src/lib/service-api/types.gen.ts`; `components/CrewOrchestrator.md` (the crew run's transitions; `escalate` stops the crew and hands it back to the person with a question; `kill` is **Stop crew**); HLD `04-domain.md` (crew run lifecycle) and `11-ux-guidelines.md` §8, which labels a run's states and none of a crew run's own; the call sites `apps/desktop/ui/src/lib/blocks/kinds/PlanProgress.svelte` (a `plan_progress` block whose subject is a crew run) and `apps/desktop/ui/src/lib/i18n/run-state.ts`.

### Message catalogue entries (added)

**SM-RS-6 The four states only a crew run has.** A crew run's state is drawn with the same rule as a run's (SM-RS-1, `state-` and the state with `_` as `-`). `running`, `paused`, `stopped_at_limit` and `failed` take the ids of SM-RS-2 and SM-RS-4 unchanged (a paused crew run draws `state-paused-approval`, as SM-RS-4 states for a run). The four states below have no id above; their words are *chosen here* *(flagged, SA-UI-RS-02)*, in the tone of the run-state table:

| Id | en-GB text | Stored crew state | Meaning (CrewOrchestrator.md) |
|---|---|---|---|
| `state-starting` | Starting | `starting` | the crew is validated and its first step not yet running |
| `state-finished` | Done | `finished` | Ordin called `finish`, or the saved steps ended |
| `state-escalated` | Handed back to you | `escalated` | Ordin called `escalate`: the crew stopped with a question for the person |
| `state-killed` | Stopped | `killed` | the person pressed **Stop crew** |

`finished` reads as a run's `completed` and `killed` as a run's `cancelled`, so the person sees one word for one outcome whether a task or a crew did the work.

**SM-RS-7 Every state the screens receive has a message.** Every value of the generated `RunState` and `CrewRunState` maps by SM-RS-1, SM-RS-4 and SM-RS-6 to an id that exists in the en-GB catalogue. A state added to either enum without an id fails the test below rather than showing its id on screen.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a crew run handed back to the person (SM-RS-6)
    Given a crew run whose stored state is escalated
    When its state is drawn in en-GB
    Then the message id is state-escalated
    And the label reads "Handed back to you"

  Scenario: Edge case — every state the screens receive has a label (SM-RS-7)
    Given each value of RunState and CrewRunState
    When its message id is built
    Then the en-GB catalogue holds that message
```

### Tests (vitest) — added

- Table-driven over the four rows of SM-RS-6: the id built from the state, and its en-GB text.
- SM-RS-7: every value of `RunState` and `CrewRunState` (listed in the test and checked against the generated types by the type checker) resolves in the en-GB bundle, with no `UI_MESSAGE_MISSING` recorded.

### Assumptions

- **SA-UI-RS-02** *(flagged)* The four crew-run labels of SM-RS-6 are chosen here; the HLD's run-state table (11 §8) has no row for a crew run's own states.

## Amendment 2026-01-29 — The second-install step's words

> **Status:** Approved 2026-01-29 (user decision: complete the high and medium follow-ups). Source: the desktop now runs the Setup Assistant's start checks (`components/OrdinDesktop.md` DSK-SC-1), whose progress holds a `second_install` step on the desktop (`components/SetupAssistant.md` SET-UP-27; `components/ServiceApi.md` SVC-SC-1). SM-FS-5 gives every `CheckKind` a step message, but its table has none for `second_install`, and the catalogue holds neither that step's words nor `setup-second-install` of the amendment January 2026 (ADR-072). Every section above stays in force.
>
> Evidence read: SM-FS-5 and its catalogue table; SM-UP-9 and the `setup-second-install` row of the amendment January 2026 (ADR-072); `crates/ordin-localisation/catalogues/en-GB/screens-setup.ftl`; `apps/desktop/ui/src/lib/views/setup/first-start-model.svelte.ts` (`stepMessageId`, `resultMessageId`).

- **SM-FS-8 The second-install step.** SM-FS-5's catalogue table gains the row below; `setup-second-install` (ADR-072 amendment, wording unchanged) is the result's message, so a failed `second_install` step draws it under its words as SM-FS-7 says, with the `folder` argument. Both ids live in `screens-setup.ftl`. The step has no action and no button (SM-UP-9).

| id | en-GB |
|---|---|
| `setup-step-second-install` | Checking for another copy of ordin |

### Acceptance Criteria (Gherkin) — added scenario

```gherkin
  Scenario: Edge case — the second-install step has its words (SM-FS-8)
    Given the en-GB catalogue
    Then it holds "setup-step-second-install" and "setup-second-install"
    And a failed second_install step with folder "/Applications" draws "ordin is also installed in /Applications. Remove one copy, because updates reach only the copy installed for you."
```

### Gaps

None.

## Amendment 2026-01-31 — A Jira site pasted as an address

> **Status:** Approved 2026-01-31 (user decision: "accept pasted Jira addresses"). Source: the Settings › Jira form of the amendment January 2026 (ADR-106), Internal Logic step 1, takes the site as "the host only, `name.atlassian.net`", and sends it as typed. `jira.connect_token` checks `site` as a host name (ServiceApi.md row `jira.connect_token`; JiraAdapter.md), so a person who pastes the address their browser shows, `https://acme.atlassian.net/jira/your-work`, is refused before the Jira Adapter is reached, and sees the Service API's general sentence for an input that is not valid. An empty site is refused the same way. Every section above stays in force; this amendment changes only what the form sends.
>
> Evidence read: ScreensModule.md amendment January 2026 (ADR-106) Internal Logic step 1 and its catalogue rows; `apps/desktop/ui/src/lib/views/Settings/signins-model.svelte.ts` (`JiraModel.connect`, `signInWithAtlassian`); `crates/ordin-service-api/src/commands/jira.rs` (`JiraConnectTokenInput::check`, `rules::host`).

- **SM-JS-1 The site is read as a host.** Before **Connect** sends `jira.connect_token` and before **Sign in with Atlassian…** sends `identity.sign_in`, the screens read the site field as follows, and use the result as `site` (and in `"https://" + <site>`):
  1. Spaces at either end are removed.
  2. A leading `http://` or `https://` (any letter case) is removed.
  3. Everything from the first `/`, `?` or `#` on is removed.
  4. A trailing `.` is removed, and the rest is put in lower case.
  The field keeps what the person typed; only what is sent changes. The Jira Adapter's own checks of the site (`JIRA_ADDRESS_INVALID` for a site that is not a Jira Cloud site) are unchanged and drawn as before.
- **SM-JS-2 No site.** When the result of SM-JS-1 is empty, nothing is sent: the line `settings-jira-site-needed` is drawn under the fields as an error line, focus moves to the site field, and the token field keeps its text (nothing was sent, so nothing needs typing again).

| id | en-GB |
|---|---|
| `settings-jira-site-needed` | Type your Jira site, for example acme.atlassian.net. |

### Error Table (rows added)

| Condition | Code | Shown |
|---|---|---|
| The site field is empty after SM-JS-1 | none (no command is sent) | `settings-jira-site-needed` under the fields |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a pasted address is sent as its host (SM-JS-1)
    Given Settings › Jira is open and not connected
    When the person enters "https://Acme.atlassian.net/jira/your-work?x=1", an email address and a token, and presses Connect
    Then jira.connect_token is sent with site "acme.atlassian.net"
    And the site field still shows what the person typed

  Scenario: Edge case — Sign in with Atlassian uses the same host (SM-JS-1)
    When the site field holds "https://acme.atlassian.net/" and the person presses "Sign in with Atlassian…"
    Then identity.sign_in is sent with site "https://acme.atlassian.net"

  Scenario: Error path — no site (SM-JS-2)
    When the site field holds "  " and the person presses Connect
    Then no command is sent
    And "Type your Jira site, for example acme.atlassian.net." is shown under the fields
    And the token field keeps its text
```

### Gaps

None.

## Amendment 2026-01-31 — The Documents screen with no place

> **Status:** Approved 2026-01-31 (user decision: "carry on with the fixes"). Source: the screen `documents` is registered with no instance (the screen table of step 9: `documents` | — | `tree-documents`), and the Documents row's main action opens it so (`DocumentsTreeGroup.svelte`, `openDocuments`). A link row of SM-RL-1 opens `documents:<place id>`, "step 9's documents list for its place". No assertion says what the screen draws with no place. The built screen sent `files.list_folder` with `place_id: ""`, which the Service API refuses (`SVC_INPUT_INVALID`), so the person saw the general sentence for an input that is not valid. Every section above stays in force.
>
> Evidence read: step 9's screen table; SM-RL-1; `apps/desktop/ui/src/lib/views/fixed/Documents.svelte`, `lists.svelte.ts` (`loadFolder`), `apps/desktop/ui/src/lib/shell/DocumentsTreeGroup.svelte`, `apps/desktop/ui/src/lib/views/add/links-model.svelte.ts` (`orderedLinks`); the Mac log of test.1 (`files.list_folder` → `SVC_INPUT_INVALID`).

- **SM-DS-1 Documents with no place.** *Superseded by SM-DS-4 (amended January 2026; user decision; Approved by the user January 2026): the list of places is the side tree's (SM-RL-1); the screen with no place is the overview of recently changed documents.* The screen `documents` with no instance lists where the workspace's documents live: it sends `workspaces.links { workspace_id }` and draws one row per link that `orderedLinks` keeps (active, in SM-RL-1's order) and that has a `place_id`, with the link's `display_name`. A row opens `documents:<place id>` with the link's `display_name` as the tab title, as the link's tree row does. It is read again on each `workspace_changed` for the workspace. It never sends `files.list_folder`.
- **SM-DS-2 No place yet.** With no such row, the screen draws `documents-no-places`.
- **SM-DS-3 A place's documents.** The screen `documents:<place id>` is unchanged: `files.list_folder { place_id, folder, sort: "name_asc" }`, a folder row opens the folder, and a file row opens its document page (SM-NS-4).

| id | en-GB |
|---|---|
| `documents-no-places` | No documents are linked to this workspace yet. Use Add to this workspace to link a folder, a site or a space. |

### Error Table (rows added)

| Condition | Code | Shown |
|---|---|---|
| `workspaces.links` fails | the owner's code | its sentence on the screen, by `ErrorLine` |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — Documents from the tree lists where documents live (SM-DS-1)
    Given a workspace with an active local folder "Quotes" (place p1) and an active Confluence space "Rates" (place p2)
    When the person opens Documents from the workspace's tree row
    Then workspaces.links is sent and files.list_folder is not
    And the screen lists "Rates" then "Quotes"
    And choosing "Quotes" opens the screen documents:p1 titled "Quotes"

  Scenario: Edge case — no documents linked yet (SM-DS-2)
    Given a workspace with no active link that has a place
    When the person opens Documents from the workspace's tree row
    Then "No documents are linked to this workspace yet. Use Add to this workspace to link a folder, a site or a space." is shown
```

### Gaps

None.

## Amendment 2026-01-31 — A command that gets no answer on this computer says so in words

> **Status:** Approved 2026-01-31 (user decision: "carry on with the fixes"). Source: the walk through the built program on Windows. **Choose a folder…** was cut off by the window's own 10-second limit while the folder picker was open (ServiceApi.md SVC-R1-8), and step 11 drew `UI_COMMAND_TIMEOUT` on this computer's backend as a bare **Try again**, with no sentence. The catalogue already holds `error-ui-command-timeout` and `error-ui-ipc-failed`. Every section above stays in force.

- **SM-ER-1 Words for a local failure.** In step 11, `UI_COMMAND_TIMEOUT` on this computer's backend is drawn as `error-ui-command-timeout` ("That took too long and got no answer. Try again.") and `UI_IPC_FAILED` as `error-ui-ipc-failed` ("The window couldn't reach the rest of ordin. Close ordin and open it again."), each with **Try again**. A server's backend keeps `server-cannot-reach`.
- **SM-ER-2 The window waits as long as the backend.** The window never times out a command whose Service API budget is `WaitsForPerson` (ServiceApi.md SVC-R1-7, SVC-R1-8): `app.pick_folder`, `files.save_export`, `identity.import_client_certificate`, `identity.sign_in_wait`, `setup.run_guided`, `setup.sign_in_harness`. A test of the Service API's catalogue compares the window's list with those rows.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Error path — a local command with no answer says so (SM-ER-1)
    Given a command to this computer's backend has had no reply for 10 seconds
    Then "That took too long and got no answer. Try again." is shown with Try again

  Scenario: Edge case — choosing a folder slowly is not cut off (SM-ER-2)
    Given the person pressed Choose a folder… and the folder picker is open
    When 30 seconds pass before they choose
    Then the window still waits and the chosen folder is added
```

### Gaps

None.

## Amendment 2026-01-31 — A PDF is read in its own app

> **Status:** Approved 2026-01-31 (user decision: "agree with all recommendations, fix them"). Source: the walk through the built program on Windows. SM-NS-4.3 draws a `pdf` preview in the WebView's own PDF viewer, but the window's content security policy (OrdinDesktop.md; cross-cutting/Cors.md) is `object-src 'none'; frame-src 'none'`, so the viewer never loads and the page showed "This part can't be shown." The policy stays as it is. Every section above stays in force.

- **SM-PDF-1 A PDF is drawn on the page** (*amended January 2026 (user decision; Approved by the user January 2026); replaces "PDFs open in their own app"*). Wherever a `Preview` of kind `pdf` is drawn (the document page of SM-NS-4, an approval card's content, a `preview` block), the screens decode `bytes_b64` as the other previews' bytes are read and draw the document with pdf.js (`pdfjs-dist`, Apache-2.0, an exact version pinned in `apps/desktop/ui/package.json`, legacy build): each page on a canvas, with its text in a selectable text layer over it, the pages in a scrolling column, with **Zoom out** and **Zoom in** (`doc-pdf-zoom-out`, `doc-pdf-zoom-in`, from 50% to 300%, `doc-pdf-zoom`) and each page named `doc-pdf-page`. While it opens it draws `doc-pdf-loading`; a document pdf.js cannot read draws `doc-pdf-failed`. At most the first 50 pages are drawn; a longer document ends with `doc-pdf-more-pages` and, on the document page when the backend answers `craft.open`, **Open in its app** (`doc-open-in-app`). The window's content security policy (OrdinDesktop.md) is unchanged: pdf.js parses in a bundled same-origin module worker (`worker-src 'self' blob:`), the bytes are handed over in memory, range requests, streaming and auto-fetch are off, the character maps and standard fonts are bundled and read without a fetch (`connect-src` names the IPC bridge only), WebAssembly is off (`useWasm: false`; the policy has no `wasm-unsafe-eval`), XFA is off (`enableXfa: false`), `isEvalSupported` is false, and no scripting manager or annotation layer is created, so a PDF's scripts and links never run. pdf.js loads under the window's frozen `Object.prototype` (`freezePrototype`). No `object`, `embed` or `iframe` element is drawn. **Zoom out** and **Zoom in** are small square buttons in the editor toolbar's style drawing "−" and "+" (the characters hidden from assistive technology), named by `aria-label` and `title` `doc-pdf-zoom-out` and `doc-pdf-zoom-in`, either side of the `doc-pdf-zoom` percentage (amended January 2026; user decision; Approved by the user January 2026).

  Formerly (Approved 2026-01-31): wherever a `Preview` of kind `pdf` is drawn, the screens drew the line `doc-pdf-opens-in-app` instead of a viewer; the document page's **Open in its app** (`doc-open-in-app`) stayed beside it.

| id | en-GB |
|---|---|
| `doc-pdf-opens-in-app` | PDFs open in their own app. Use Open in its app to read this one. (*removed — amended January 2026 (user decision; Approved by the user January 2026), SM-PDF-1*) |
| `doc-pdf-loading` | Opening the PDF… (*amended January 2026 (user decision; Approved by the user January 2026), SM-PDF-1*) |
| `doc-pdf-failed` | This PDF can't be shown here. Open it in its app from its ⋯ menu. (*amended January 2026 (user decision; Approved by the user January 2026), SM-PDF-1*) |
| `doc-pdf-page` | Page { $page } of { $total } (*amended January 2026 (user decision; Approved by the user January 2026), SM-PDF-1*) |
| `doc-pdf-zoom-in` | Zoom in (*amended January 2026 (user decision; Approved by the user January 2026), SM-PDF-1*) |
| `doc-pdf-zoom-out` | Zoom out (*amended January 2026 (user decision; Approved by the user January 2026), SM-PDF-1*) |
| `doc-pdf-zoom` | { $percent }% (*amended January 2026 (user decision; Approved by the user January 2026), SM-PDF-1*) |
| `doc-pdf-more-pages` | Only the first { $shown } of { $total } pages are shown. (*amended January 2026 (user decision; Approved by the user January 2026), SM-PDF-1*) |
| `tree-add-to-knowledge` | Add to knowledge (*amended January 2026 (user decision; Approved by the user January 2026), SM-RL-4*) |
| `tree-add-to-memory` | Add to memory (*amended January 2026 (user decision; Approved by the user January 2026), SM-RL-4*) |
| `tree-ask-text-knowledge` | Write a knowledge page from “{ $title }”. (*amended January 2026 (user decision; Approved by the user January 2026), SM-RL-4*) |
| `tree-ask-text-memory` | Add the facts in “{ $title }” to memory. (*amended January 2026 (user decision; Approved by the user January 2026), SM-RL-4*) |
| `doc-location` | In { $location } (*amended January 2026 (user decision; Approved by the user January 2026), SM-NS-4*) |

```gherkin
  # Scenarios below: amended January 2026 (user decision; Approved by the user January 2026) (SM-PDF-1); they replace "a PDF's document page".
  Scenario: Happy path — a PDF's document page draws its pages (SM-PDF-1)
    Given files.preview answers a preview of kind pdf with 3 pages
    When the document page of "Quarterly plan.pdf" is drawn
    Then it draws 3 pages, each a canvas with a text layer, named "Page 1 of 3" to "Page 3 of 3"
    And no embedded viewer is drawn and no Open in its app is drawn

  Scenario: Edge case — a long PDF shows its first 50 pages (SM-PDF-1)
    Given files.preview answers a preview of kind pdf with 120 pages
    When the document page is drawn
    Then it draws 50 pages and "Only the first 50 of 120 pages are shown." with Open in its app
    When the person presses Open in its app
    Then "craft.open" is sent with the page's place and path
```

### Gaps

None.

## Amendment 2026-01-31 — The person's own messages in the conversation

> **Status:** Approved 2026-01-31 (user decision: "agree with all recommendations, fix them"). Source: OrdinHost.md OH-MSG-1 and ServiceApi.md SVC-R1-9; the prototype's conversation scenes draw the person's turns as `.turn.you .bubble`. Every section above stays in force.

- **SM-CV-1 The person's message.** A conversation item whose author is `person` (`block_shown.author`, or `ConversationBlock.from` when read back) and whose block is a `message` is drawn at the end side of the conversation column as a bubble on `--color-accent-soft`, its paragraphs as text, preceded by the visually hidden `conv-you-said`; it has no **Close**. Every other item is drawn as before.

| id | en-GB |
|---|---|
| `conv-you-said` | You said |

```gherkin
  Scenario: Happy path — the person sees what they sent (SM-CV-1)
    When the person sends "What do the meeting notes say about rates?"
    Then the conversation shows that text as the person's own bubble, then Ordin's answer
```

### Gaps

None.

## Amendment 2026-01-31 — Naming a new workspace

> **Status:** Approved 2026-01-31 (user decision: "agree with all recommendations, fix them"). Source: the walk through the built program on Windows: after **New workspace** the name field did not take the cursor, so typing went nowhere, and **Create** with an empty field sent nothing and said nothing. Every section above stays in force.

- **SM-WS-1 The name field.** When the side rail's **New workspace** opens its name field (`rail-new-workspace`), the field takes the cursor. **Create** (`rail-new-workspace-create`) is unavailable while the trimmed name is empty, so it never does nothing when pressed; Enter in an empty field sends nothing either.

```gherkin
  Scenario: Happy path — naming a workspace (SM-WS-1)
    When the person presses New workspace and types "Supplier review"
    Then the text is in the name field and Create sends workspaces.create with that name

  Scenario: Edge case — no name yet (SM-WS-1)
    When the name field is empty
    Then Create is unavailable
```

### Gaps

None.

## Amendment 2026-01-31 — A screen with no workspace opens where the person is

> **Status:** Approved 2026-01-31 (user decision: "continue with phase 5" of the walk through the built program). Source: the walk on Windows: with a workspace open, choosing **Settings** in *Search or go to…* closed the dialog and showed nothing. A screen with no workspace (`settings`, `language`, `server-languages`, `home` from the side rail) was opened in the no-workspace screen area while the screen area drew the open workspace's, so it opened out of sight; the same held for **Edit wording** and **Open** in Settings › Language. Every section above stays in force.

- **SM-OS-1 Where a screen with no workspace opens.** A screen opened with no workspace for the backend the window shows opens in the screen area the person sees (the open workspace's, or the no-workspace area when none is open); the window keeps its workspace and conversation. A screen of another backend switches the window to that backend with no workspace, then opens there.

```gherkin
  Scenario: Happy path — Settings from Search with a workspace open (SM-OS-1)
    Given the workspace "Supplier review" is open
    When the person chooses Settings in Search or go to…
    Then Settings is the screen in front, and "Supplier review" and its conversation stay open
```

### Gaps

None.

## Amendment 2026-01-31 — Running now in a workspace, and empty screens that say what to do

> **Status:** Approved 2026-01-31 (user decision: "agree with all, fix them and commit"). Source: the walk through the built program on Windows: the workspace tree's **Running now** opened the `running` screen, which step 9's screen table only reserves, so it drew its heading and nothing else; an empty **Knowledge** drew only its filter box; an empty **Agents** said "Nothing here yet." with no hint that agents are added by asking Ordin (11 §5.2). Every section above stays in force.

- **SM-RN-1 Running now in a workspace.** The screen `running` (instance none, title `tree-running`, command `home.view`) draws the open workspace's running work as Home's **Running now** section draws it: `home.view { workspace_id }`'s `running` rows, each with its state label and **Show progress** (`home-show-progress`), read again on Home's refresh events; `home-nothing-running` when there is none. It never sends `home.opened`, so Home's new marks are unchanged.
- **SM-EM-1 Empty screens say what to do.** An empty Agents screen draws `agents-none-yet`; an empty Knowledge screen with no filter text draws `knowledge-none-yet`. Other fixed lists keep `fixed-nothing-here`.

| id | en-GB |
|---|---|
| `agents-none-yet` | No agents in this workspace yet. Ask Ordin to find one for the work. |
| `knowledge-none-yet` | No pages yet. Ask Ordin to write one, or keep what it learns. |

```gherkin
  Scenario: Happy path — a workspace's running work (SM-RN-1)
    Given an agent is running in the workspace "Supplier review"
    When the person chooses Running now under "Supplier review"
    Then the run is listed with its state and Show progress

  Scenario: Edge case — empty screens (SM-EM-1)
    Given a workspace with no agent and no knowledge page
    Then Agents shows "No agents in this workspace yet. Ask Ordin to find one for the work."
    And Knowledge shows "No pages yet. Ask Ordin to write one, or keep what it learns."
```

### Gaps

None.


## Amendment 2026-01-31 — Adding an agent: the library screen and Add it

> **Status:** Approved 2026-01-31 (user decisions: "build the missing Add it button for agents"; "Ask IT stays out"). Source: the walk through the built program on Windows. Ordin found the document agent and proposed it, but the `agent_proposal` card's **Open** went to the Agents screen, which listed the workspace's agents and not the proposed one, and neither the card nor the `agent_library` block offered **Add it**: step 9's block table names the actions (Add it, Ask IT, Ask Ordin about this agent; HLD 11 §5.2, UX-A33) and no assertion said how they work, so nothing called `agents.preview_add` or `agents.add` (PackageCatalogue.md *Service API commands*). Every section above stays in force; this amendment replaces the actions of the `agent_proposal` and `agent_library` rows of step 9's table.

- **SM-AL-1 The agent library screen.** The screen `library` (title `library-title`; commands `catalogue.search`, `agents.preview_add`, `agents.add`) opens in the workspace it is asked from. Its instance, when present, is the identifier of the agent to choose (`library:<identifier>`). It is drawn as the `agent_proposal` block's screen (instance `block.agent`, SM-AL-5), opened by **Open** on an `agent_library` block (no instance) and by **Browse the agent library** (`library-browse`) on the Agents screen, beside its list (no instance). It draws a search box (`block-agent-search`) and the list of `catalogue.search { kinds: [WorkspaceProfile], include_not_for_new: false, repositories: [], text? }`: each row the agent's name, its one-sentence `summary` and `library-from` with the entry's `from`. The agent named by the instance comes first, marked `library-suggested`, and is chosen when the screen opens. `block-agent-none` when nothing matches. The Agents screen no longer draws **Browse the agent library**; the library is opened from the Agents row's ⋯ menu in the side tree (SM-AT-1), from an `agent_library` block and from an `agent_proposal` block (amended January 2026; user decision; Approved by the user January 2026).
- **SM-AL-2 The chosen agent.** Choosing a row sends `agents.preview_add { workspace_id, agent: <the entry's part id>, choices: [], interface_locale: <the active locale> }` and draws, under the agent's name as a heading: the one action beside the name (SM-AL-3), the summary, `library-from`, and `library-uses` with the `label` of each `uses` item joined by ", " when `uses` is not empty. Nothing is asked before **Add it**: the `asks` of the preview are choices the agent asks the first time its work needs them (decision 64), and the screen shows none of them. `available` decides the action:
  - `available` → **Add it** (`library-add`);
  - `pack_missing` → no action and `library-unavailable` (one sentence, never a pack name or class);
  - `needs_it_approval` → no action and `library-needs-approval`;
  - `unsafe` → no action and `library-unsafe`.
  A preview refused with `PKG_ALREADY_ADDED` draws `library-already-added` and no action; any other refusal is the error line with **Try again**. **Ask Ordin about this agent** (`block-ask-about-agent`) is drawn for every chosen agent.
- **SM-AL-3 Add it.** **Add it** sends `agents.add` with the same input as the preview. While it is in flight the action reads `library-adding` with the agent's name and is disabled. `ready` → `library-added` with the agent's name, and the action is gone; `pack_missing`, `needs_it_approval` and `unsafe` → the sentence SM-AL-2 gives for each, and the action is gone; a refusal → the error line, and **Add it** stays. The agent then appears on the Agents screen; Ordin reads the change at the person's next message ("Agent directory changed", OrdinHost.md step 4).
- **SM-AL-5 The proposal's screen is the library.** An `agent_proposal` block's screen draws Ordin's `reason`, then the agent library of SM-AL-1 with the block's `agent` as its instance, so **Open** on the card in the conversation shows the proposed agent chosen with **Add it** at once, with no second **Open**. *(Found on Windows: the block's screen drew the proposal and its own **Open**, one step more than step 9's table, which places the block on "the agent library screen with the agent chosen".)*
- **SM-AL-6 Ordin's own agents are not listed.** The library leaves out ordin's own agents: an entry whose part is in the bundled catalogue (`agent_repository_id` `01900000-0000-7000-8000-000000000001`) with the identifier `urn:ordin:default:workspaces:coordinator` (Ordin) or `urn:ordin:default:workspaces:advisor` (the agent maker), as PackageCatalogue.md PKG-OWN-1 refuses them. *(Found on Windows: the library listed Ordin itself as an agent to add.)*
- **SM-AL-4 No IT actions.** The library screen offers no **Ask IT** and no **Send to IT** (user decision, January 2026: every feature is self-service). An agent the company's trust policy does not allow, or one that failed its safety check, is shown with its one sentence and no action.

| id | en-GB |
|---|---|
| `library-title` | Agent library |
| `library-browse` | Browse the agent library |
| `library-from` | From: { $from } |
| `library-suggested` | Ordin suggests this one |
| `library-uses` | Uses: { $uses } |
| `library-add` | Add it |
| `library-adding` | Adding { $agent }… |
| `library-added` | { $agent } is added to this workspace. Tell Ordin to go ahead in the conversation. |
| `library-already-added` | Added to this workspace. |
| `library-unavailable` | This agent can't be set up for this workspace. |
| `library-needs-approval` | This agent needs your company's approval before it can be added. |
| `library-unsafe` | This agent could not be added safely. |

```gherkin
  Scenario: Happy path — adding the agent Ordin proposed (SM-AL-1, SM-AL-2, SM-AL-3)
    Given Ordin proposed the document agent in the workspace "Supplier review"
    When the person chooses Open on the proposal
    Then the agent library opens with the document agent first, marked "Ordin suggests this one", and chosen
    And its detail shows what it does, "From: ordin" and Add it
    When the person chooses Add it
    Then the action reads "Adding Document agent…" until agents.add answers
    And "Document agent is added to this workspace. Tell Ordin to go ahead in the conversation." is shown

  Scenario: Edge case — an agent already in the workspace (SM-AL-2)
    Given agents.preview_add answers PKG_ALREADY_ADDED
    When the person chooses the agent
    Then "Added to this workspace." is shown and there is no Add it

  Scenario: Edge case — an agent that needs approval offers no IT action (SM-AL-2, SM-AL-4)
    Given agents.preview_add answers available needs_it_approval
    When the person chooses the agent
    Then "This agent needs your company's approval before it can be added." is shown
    And there is no Add it, no Ask IT and no Send to IT

  Scenario: Error path — Add it refused (SM-AL-3)
    Given agents.add answers PKG_INSTALL_FAILED
    When the person chooses Add it
    Then the error line is shown and Add it stays
```

### Gaps

None.

## Amendment 2026-01-31 — The run screen

> **Status:** Approved 2026-01-31 (user decision: "proceed": build the result screen and the conversation card). Source: the walk through the built program on Windows. After Ordin delegated to the document agent and its run completed, Home listed it under *Since you last looked*, and its **Open** showed the screen `run`, which step 9's screen table only reserves: a heading and nothing else, with no command sent. The design's golden path (03-sequences *Run a task with one local agent*) ends with the run's final message and artefacts on its screen. The card in the conversation is OrdinHost.md OH-RC-1. Every section above stays in force; this amendment replaces the `run` row's "reserved in `SCREEN_IDS`".

- **SM-RS-1 The run screen.** The screen `run` (instance the run id, title `screen-run`, command `runs.get`) draws one run. It sends `runs.get { run_id }` and draws, as a heading, the agent's display name from `agents.get` (for an agent instance, when the backend answers it; `screen-run` otherwise), the run's state word (`stateMessageId`), and `run-screen-asked` with the task's `free_text`, else its `command`, when the run has a task. While the run's state is `queued`, `running` or `paused`, it draws the `plan_progress` block for the run. Once it has ended, it draws a `produced` result for the run: the final message and the artefacts of the journal's `run_result` entry for the run (`journal.list_entries { filter: { run_id, kind: "run_result" } }`, when the backend answers it), else the run's `end_reason` with no artefacts; `run-screen-no-message` when there is neither. A refused `runs.get` is the error line with **Try again**.

| id | en-GB |
|---|---|
| `run-screen-asked` | Asked: { $brief } |
| `run-screen-no-message` | It left no message. |

```gherkin
  Scenario: Happy path — a finished run shows what it produced (SM-RS-1)
    Given the document agent's run ended Completed with a final message and one file
    When the person chooses Open on the run under Since you last looked
    Then the run screen shows "Document agent", its state, what it was asked, the final message and the file

  Scenario: Edge case — a running run shows its progress (SM-RS-1)
    Given a run that is running
    When the person opens it
    Then the run screen shows its progress and no result
```

### Gaps

None.

## Amendment 2026-01-31 — Who may use which folder

> **Status:** Approved 2026-01-31 (user decision: "do both": the per-folder choice on screen and Ordin's tick box). Source: the walk through the built program on Windows. The document agent's write to the linked folder was refused because it could use no folder, and no screen could give it one: nothing sent `workspaces.set_grant` (WorkspaceService.md). The design gives the choice per agent and folder (11 vocabulary: "can't use" / "can read" / "can read and change"; 03-sequences *Add a folder and work with files*) and Ordin's tick box when access is missing (11 *Missing access is shown, not assumed*; OrdinHost.md OH-ACC-1). Every section above stays in force.

- **SM-FA-1 What an agent can use.** On the Agents screen, the chosen agent's section draws `folder-access-heading` and one row per active link of the workspace with a place (`workspaces.links`): the link's display name and a choice of `folder-access-none`, `folder-access-read` and `folder-access-read-and-change` (a `jira_issue` link offers the first two only), set from `workspaces.grants` (the agent's grant on that place, else `folder-access-none`). A change sends `workspaces.set_grant { workspace_id, grantee: { kind: agent_instance, agent_instance_id }, place_id, access }` at once (`access: null` for `folder-access-none`); a refusal is the error line. Drawn only when the backend answers `workspaces.grants` and there is at least one such link.
- **SM-FA-2 When a folder is added.** On *Add to this workspace*, the *Added* step draws `folder-access-agents-heading` and, for each agent of the workspace (`agents.list { workspace_id }`), the same choice for the new link, sent the same way. Nothing is drawn when the workspace has no agent.
- **SM-FA-3 Ordin's access card.** The block kind `access` (2C wire §7 as amended; OrdinHost.md OH-ACC-1) is drawn in full in the conversation: its `text`, then one tick box per place labelled `access-can-read-and-change` (or `access-can-read` for a `read_only` place) with the agent's and the folder's names, ticked when `workspaces.grants` holds the agent's grant on that place. Ticking sends `workspaces.set_grant` with `read_and_change` (or `read`), unticking with `access: null`, at once; the tick is the decision and nothing goes to *Needs you*.

| id | en-GB |
|---|---|
| `folder-access-heading` | What it can use |
| `folder-access-agents-heading` | Which agents can use it |
| `folder-access-none` | Can't use |
| `folder-access-read` | Can read |
| `folder-access-read-and-change` | Can read and change |
| `access-can-read-and-change` | { $agent } can read and change { $folder } |
| `access-can-read` | { $agent } can read { $folder } |

```gherkin
  Scenario: Happy path — giving an agent a folder on the Agents screen (SM-FA-1)
    Given the workspace links "Sample documents" and the document agent can use no folder
    When the person chooses the document agent and sets "Sample documents" to Can read and change
    Then workspaces.set_grant is sent with access read_and_change

  Scenario: Happy path — ticking Ordin's access card (SM-FA-3)
    Given Ordin's access card for the document agent and "Sample documents"
    When the person ticks "Document agent can read and change Sample documents"
    Then workspaces.set_grant is sent with access read_and_change at once
```

### Gaps

None.

## Amendment January 2026 — Publishing a folder to Confluence from its screen

> **Status:** amended January 2026 (user decision 2026-01-30, "proceed with recommendations"). Source: Phase 5 of the UI walk on Windows — `workspaces.propose_confluence_link` was sent by no screen, and the folder's publishing line and *Ready to publish* list were never drawn, because `Documents.svelte` gave `PublishingFolder` no links and nothing read them. Every rule above stays in force; SM-SF-7.1's "the view that shows the linked folder" is this one.

- **SM-PC-1 Publish this folder to Confluence…** *Moved (amended January 2026; user decision; Approved by the user January 2026): the folder screen draws no publishing button; the same flow is the folder menu's **Publish this folder automatically…** in the side tree (SM-FP-1), and a document is published from its own menu, **Publish to…** (SM-PUB-1). This row keeps the identifier it had; the pane-collapse rule of the same identifier further down is a separate rule.* The folder screen of Documents (a place's folder, SM-DS-3) offers **Publish this folder to Confluence…** (`publish-confluence-open`) when the place is the person's folder or the workspace's own folder, no publishing link of the workspace is on this place and folder, and the backend answers `confluence.find` and `workspaces.propose_confluence_link`. It opens, in place, the heading `publish-confluence-heading`, a search field labelled `publish-confluence-find` with **Find** (`publish-confluence-search`, at least 2 characters) and **Cancel**. **Find** sends `confluence.find { query, limit: 10 }` and lists the pages found — a whole space is left out, since a folder publishes under a page — each as "space › page" with **Publish here** (`publish-confluence-here`); none → `publish-confluence-nothing`. **Publish here** sends `workspaces.propose_confluence_link { workspace_id, place_id, folder, location }` with the folder shown (empty for the place itself); `ok` closes the search, reads the links again and opens the approval card (`approval:<approval_id>`) at once; an error draws its sentence through `ErrorLine`. Nothing is published from here: the card is the person's decision (ConfluenceAdapter.md).
- **SM-PC-2 A linked folder's publishing on its screen.** The same screen reads `workspaces.confluence_links { workspace_id }` (ServiceApi.md SVC-CL-1) when it opens, when the folder shown changes and on each `workspace_changed` for the workspace. For each link whose `place_id` and `folder` are those shown: `proposed` → `publish-confluence-waiting`; `active` or `paused` → the publishing line (SM-SF-7, SM-SF-10); `active` → under the heading `publish-confluence-ready-heading`, the link's *Ready to publish* list read with `confluence.ready_to_publish` and drawn with **Publish checked items…** as SM-WR-13 states, or `publish-confluence-ready-none` when it is empty. An item an approval held stays in this list (ConfluenceAdapter.md step 6.2), so the person sees it is still waiting.

### Message catalogue rows (en-GB; *chosen here*)

| Id | en-GB text |
|---|---|
| `publish-confluence-open` | Publish this folder to Confluence… |
| `publish-confluence-heading` | Publish this folder to Confluence |
| `publish-confluence-find` | Find the Confluence page to publish under |
| `publish-confluence-search` | Find |
| `publish-confluence-here` | Publish here |
| `publish-confluence-nothing` | No Confluence page matches that. |
| `publish-confluence-waiting` | Waiting for you to approve publishing this folder to Confluence. |
| `publish-confluence-ready-heading` | Ready to publish |
| `publish-confluence-ready-none` | Nothing is waiting to be published. |

```gherkin
  Scenario: Happy path — an active link shows its Ready to publish list (SM-PC-2)
    Given the folder Pricing of Sample documents publishes to Confluence and Pricing notes.md is ready
    When the person opens Pricing in Documents
    Then "Ready to publish" lists Pricing notes.md with Publish checked items…

  Scenario: Happy path — a folder is published under a page it finds (SM-PC-1)
    Given the folder Pricing of Sample documents publishes nowhere
    When the person chooses Publish this folder to Confluence…, finds "Supplier" and chooses Publish here on "Procurement › Supplier pages"
    Then workspaces.propose_confluence_link is sent for Pricing and the approval card opens
```

### Gaps

None.

## Amendment January 2026 — Try again on the run screen

> **Status:** amended January 2026 (user decision: "proceed with recommendations"). Source: Phase 5 of the UI walk on Windows (finding FU-061) — the run-state table above gives `failed` and `interrupted` the next action **Try again**, but the run screen (SM-RS-1 of the amendment *The run screen*) drew none, and no command could start the run's task again. The command is ServiceApi.md SVC-TA-1, owned by OrdinHost.md OH-TA-1. Every section above stays in force.

- **SM-RS-8 Try again on a run that stopped.** On the run screen, when the run's state is `interrupted` or `failed`, the run has a task, and the backend answers `runs.try_again`, the screen draws **Try again** (`run-screen-try-again`) below what the run produced. Pressing it sends `runs.try_again { workspace_id: <the RunView's workspace_id>, run_id }`, with the button disabled until the reply. `{ kind: "started", run_id }` opens the new run's screen (`run:<run_id>`) in the run's workspace; this screen stays as it was. `{ kind: "waiting", text }` draws, in a polite live region under the button, `run-screen-waiting` alone (`text` is written for Ordin, not the person, and is not shown): nothing has started, and the question waits in the conversation and in *Needs you*. A refusal is the error line under the button (`OH_RUN_UNKNOWN`, `OH_RUN_NOT_INTERRUPTED`, `OH_RUN_NOT_REPEATABLE`, or the owner's other codes, with their action). No other state draws the button.

| id | en-GB |
|---|---|
| `run-screen-try-again` | Try again |
| `run-screen-waiting` | Nothing has started yet: the conversation needs you first. |

```gherkin
  Scenario: Happy path — an interrupted run is tried again and the new run opens (SM-RS-8)
    Given the document agent's run ended interrupted
    When the person opens it and chooses Try again
    Then runs.try_again is sent for the run and its workspace
    And the new run's screen opens

  Scenario: Edge case — when the conversation asks first, the screen says so (SM-RS-8)
    Given runs.try_again answers waiting with "Waiting for the person to start it."
    When the person chooses Try again on a failed run
    Then the screen shows "Nothing has started yet: the conversation needs you first." and not that text

  Scenario: Edge case — a completed run offers no Try again (SM-RS-8)
    Given a run that ended completed
    When the person opens it
    Then no Try again is drawn

  Scenario: Error path — a refusal is the error line (SM-RS-8)
    Given runs.try_again answers OH_RUN_NOT_REPEATABLE
    When the person chooses Try again
    Then the error line shows the code's sentence
```

### Gaps

None.

## Amendment January 2026 — Renaming, archiving and the Bin for workspaces

> **Status:** amended January 2026 (user decision: "proceed with recommendations"). Source: 11 §4 (UX-A13, UX-A14), §5.13 (UX-A68), §5.5 *Confirmations* (UX-A57) and the prototype's scenes `removeWs` and `binWs`; the commands and their states are WorkspaceService.md steps 4–9 and *Service API commands*. *(Found in the Phase 5 walk on Windows: no screen offered archive, bin, restore or rename.)* Every section above stays in force; this amendment states how the side rail's `workspaces.rename` and `workspaces.move_to_bin` (step 9's fixed-views table) are drawn, and adds the Archived and Bin screens.

- **SM-WS-2 The workspace's actions.** On a workspace row of the local backend, the **⋯** menu holds, after **Add to this workspace…** when that is offered: **Rename…** (`ws-rename`), **Archive** (`ws-archive`) and, last, **Remove workspace…** (`ws-remove`), each drawn only when the backend answers its commands (`workspaces.rename`; `workspaces.archive`; `workspaces.bin_impact` and `workspaces.move_to_bin`). The **⋯** button is drawn when the menu holds at least one item. A workspace of a connected server offers none of the three in this build.
- **SM-WS-3 Rename…** Replaces nothing: under the row it opens a form with one field, `ws-rename-field`, holding the current name with the text selected and the cursor in it (at most 80 characters), **Rename** (`ws-rename-verb`) and **Cancel** (`confirm-cancel`). **Rename** is unavailable while the trimmed name is empty or unchanged; Enter renames and Escape cancels. **Rename** sends `workspaces.rename { workspace_id, name }` with the trimmed name; `ok` closes the form, reads the active workspaces again (`workspaces.list { filter: "active" }`) and announces `ws-renamed` in the polite live region. A refusal (`WS_NAME_EMPTY`, `WS_NAME_TOO_LONG`, `WS_NAME_DUPLICATE`, `WS_NOT_FOUND`) is the error line under the field, and the form keeps what was typed.
- **SM-WS-4 Archive.** **Archive** sends `workspaces.archive { workspace_id }` at once, with no confirmation: nothing is lost and **Restore** undoes it. `ok` reads the active, archived and binned workspaces again; the workspace leaves the tree, its conversation tab closes, and when it was the open workspace the window shows no workspace. `ws-archived` is announced. A refusal (`WS_STATE_INVALID`, `WS_NOT_FOUND`) is the error line under the tree.
- **SM-WS-5 Archived workspaces.** Under the tree of the local backend, **Archived** (`rail-archived`, with its count, aria-label `rail-archived-count`) is drawn while `workspaces.list { filter: "archived" }` holds at least one workspace. It opens the screen `archived` (no workspace, no instance, title `archived-title`; commands `workspaces.list`, `workspaces.restore`): the heading, `archived-lead`, then one row per archived workspace, its name with **Restore** (`ws-restore`) and **Remove workspace…** (`ws-remove`, as SM-WS-6); `archived-none` when there is none. **Restore** sends `workspaces.restore { workspace_id }`; `ok` reads the three lists again, the workspace is back in the tree, and `ws-restored` is announced; a refusal is the error line on that row.
- **SM-WS-6 Remove workspace…** First sends `workspaces.bin_impact { workspace_id }`; a refusal is the error line where the action was chosen, and no dialog opens. Then `ConfirmDialog` (UX-A57) with the title `ws-bin-title`, the body `ws-bin-body`, and, when `unsent_changes` is above 0, the further line `ws-bin-unsent` with the count and the `documents` joined as a list; the verb **Move to Bin** (`ws-bin-verb`), which is not the default button, and **Cancel**. Cancel or Escape sends nothing more. **Move to Bin** sends `workspaces.move_to_bin { workspace_id }`; `ok` closes the dialog, reads the three lists again, closes the workspace's conversation tab (the window shows no workspace when it was open) and draws under the tree `ws-binned` with **Undo** (`ws-undo`, which sends `workspaces.restore_from_bin` as SM-WS-7's **Restore**); the line goes when another lifecycle action is taken. A refusal keeps the dialog open with the error line in it.
- **SM-WS-7 The Bin.** Under the tree of the local backend, after **Archived**, **Bin** (`rail-bin`, with its count when above 0, aria-label `rail-bin-count`) is always drawn when the backend answers `workspaces.list`. It opens the screen `bin` (no workspace, no instance, title `bin-title`; commands `workspaces.list`, `workspaces.restore_from_bin`, `workspaces.delete_from_bin`, `workspaces.empty_bin`, `workspaces.bin_impact`), read with `workspaces.list { filter: "in_bin" }` when it opens and again on each `workspace_changed`: the heading, `bin-lead`, one row per workspace in the Bin, its name, `bin-removed-on` with its `moved_to_bin_at` as a short date, **Restore** (`bin-restore`) and **Delete now…** (`bin-delete`); after the rows, **Empty Bin…** (`bin-empty`); `bin-none` and no **Empty Bin…** when the Bin is empty. **Restore** sends `workspaces.restore_from_bin { workspace_id }`; `ok` reads the three lists again and announces `bin-restored`; a refusal (`WS_NAME_DUPLICATE` when an active workspace took its name, `WS_STATE_INVALID`) is the error line on that row. Nothing leaves the Bin by age. The word is "Bin" on every system in this build (G-WS-1).
- **SM-WS-8 Deleting from the Bin.** **Delete now…** first sends `workspaces.bin_impact { workspace_id }` (a refusal is the error line on the row), then `ConfirmDialog` with `bin-delete-title`, `bin-delete-body` and, when `unsent_changes` is above 0, `bin-delete-unsent`; the verb **Delete** (`bin-delete-verb`) is never the default button. **Delete** sends `workspaces.delete_from_bin { workspace_id }`; `ok` closes the dialog, reads the Bin again and announces `bin-deleted`. **Empty Bin…** opens `ConfirmDialog` with `bin-empty-title` and `bin-empty-body`, which names the count and the workspaces; the verb **Empty Bin** (`bin-empty-verb`) is never the default button. It sends `workspaces.empty_bin {}`; `ok` closes the dialog, reads the Bin again and announces `bin-emptied` with `deleted`. A refusal of either keeps the dialog open with the error line in it. Cancel or Escape sends nothing.

| id | en-GB |
|---|---|
| `ws-rename` | Rename… |
| `ws-rename-field` | New name for the workspace |
| `ws-rename-verb` | Rename |
| `ws-renamed` | Renamed the workspace to “{ $name }”. |
| `ws-archive` | Archive |
| `ws-archived` | Archived “{ $name }”. It's under Archived, below your workspaces. |
| `ws-restore` | Restore |
| `ws-restored` | Restored “{ $name }”. |
| `ws-remove` | Remove workspace… |
| `ws-bin-title` | Move this workspace to the Bin? |
| `ws-bin-body` | It goes to the Bin, with its links, conversation and history. You can restore it from there. Your documents stay where they are. |
| `ws-bin-unsent` | It also takes { $count } { $count -> [one] change *[other] changes } not yet sent back: { $documents }. |
| `ws-bin-verb` | Move to Bin |
| `ws-binned` | Moved “{ $name }” to the Bin. Your documents are where they were. |
| `ws-undo` | Undo |
| `rail-archived` | Archived |
| `rail-archived-count` | Archived, { $count } |
| `rail-bin` | Bin |
| `rail-bin-count` | Bin, { $count } |
| `archived-title` | Archived workspaces |
| `archived-lead` | Workspaces you archived. Restore one to work in it again. |
| `archived-none` | No workspace is archived. |
| `bin-title` | Bin |
| `bin-lead` | Workspaces you removed. Each keeps its links, conversation, history and changes not yet sent back until you empty the Bin. Your documents were never moved. A document you move to the Bin goes to your computer's own Bin or Trash, not to this one. |
| `bin-removed-on` | Removed { $date } |
| `bin-restore` | Restore |
| `bin-delete` | Delete now… |
| `bin-empty` | Empty Bin… |
| `bin-none` | The Bin is empty. |
| `bin-restored` | Restored “{ $name }”, with its conversation, history and changes not yet sent back. |
| `bin-delete-title` | Delete “{ $name }” now? |
| `bin-delete-body` | Its links, conversation and history are deleted. Your documents stay where they are. This can't be undone. |
| `bin-delete-unsent` | { $count } { $count -> [one] change *[other] changes } not yet sent back { $count -> [one] is *[other] are } lost: { $documents }. |
| `bin-delete-verb` | Delete |
| `bin-deleted` | Deleted “{ $name }”. |
| `bin-empty-title` | Empty the Bin? |
| `bin-empty-body` | { $count -> [one] One workspace is *[other] { $count } workspaces are } deleted, with their links, conversation and history: { $names }. Your documents stay where they are. This can't be undone. |
| `bin-empty-verb` | Empty Bin |
| `bin-emptied` | Emptied the Bin: { $count -> [one] one workspace *[other] { $count } workspaces } deleted. |

```gherkin
  Scenario: Happy path — renaming a workspace (SM-WS-2, SM-WS-3)
    Given the workspace "Supplier review"
    When the person chooses Rename… in its ⋯ menu, types "Q3 supplier review" and chooses Rename
    Then workspaces.rename is sent with that name and the tree shows "Q3 supplier review"

  Scenario: Error path — the name is taken (SM-WS-3)
    Given workspaces.rename answers WS_NAME_DUPLICATE
    When the person renames "Supplier review" to "Onboarding"
    Then the error line under the field says "Another workspace already has that name." and the field keeps "Onboarding"

  Scenario: Happy path — archiving and restoring (SM-WS-4, SM-WS-5)
    When the person chooses Archive in the ⋯ menu of "Supplier review"
    Then workspaces.archive is sent, the workspace leaves the tree and Archived shows 1
    When the person opens Archived and chooses Restore on "Supplier review"
    Then workspaces.restore is sent and the workspace is back in the tree

  Scenario: Happy path — moving a workspace to the Bin names its unsent changes (SM-WS-6)
    Given "Supplier review" has 2 changes not yet sent back, in "Supplier summary" and "rates.md"
    When the person chooses Remove workspace…
    Then workspaces.bin_impact is sent and the dialog "Move this workspace to the Bin?" says "It also takes 2 changes not yet sent back: Supplier summary and rates.md."
    When the person chooses Move to Bin
    Then workspaces.move_to_bin is sent and the line "Moved “Supplier review” to the Bin. Your documents are where they were." offers Undo

  Scenario: Edge case — the confirmation cancelled (SM-WS-6, SM-WS-8)
    When the person chooses Remove workspace… and then Cancel
    Then workspaces.move_to_bin is not sent and the workspace stays in the tree

  Scenario: Happy path — the Bin restores and deletes (SM-WS-7, SM-WS-8)
    Given "Supplier review" and "Old pilot" are in the Bin
    When the person opens Bin and chooses Restore on "Supplier review"
    Then workspaces.restore_from_bin is sent and it is back in the tree
    When the person chooses Delete now… on "Old pilot" and then Delete
    Then workspaces.delete_from_bin is sent and the Bin is empty

  Scenario: Happy path — Empty Bin (SM-WS-8)
    Given two workspaces are in the Bin
    When the person chooses Empty Bin… and then Empty Bin
    Then workspaces.empty_bin is sent and "Emptied the Bin: 2 workspaces deleted." is announced

  Scenario: Error path — a restore refused (SM-WS-7)
    Given workspaces.restore_from_bin answers WS_NAME_DUPLICATE
    When the person chooses Restore on "Supplier review"
    Then the error line on its row says "Another workspace already has that name."
```

### Gaps

- **G-WS-1** *Recycle Bin on Windows.* 11 §5.13 names ordin's Bin as the computer's own, **Recycle Bin** on Windows. The screens learn no operating system from the backend (no command or `service.hello` field carries it), so this build says "Bin" everywhere, as the document's Move to Bin (SM-NS-4.7) does. Closing it needs the platform in `service.hello` or `brand.get`, then a Fluent selector on the Bin's messages.

## Amendment January 2026 — A file with no preview says so

> **Status:** amended January 2026 (user decision 2026-01-30, "proceed with recommendations"). Source: the journeys on Windows — a document page whose `files.preview` answered `None` was empty below its buttons, which reads as a failure. FileService.md FS-PV-1 now gives Word, Excel and PowerPoint files their text, so `None` remains for the kinds ordin cannot show on this computer.

- **SM-PV-1 No preview, one line** *(amended January 2026)*. On the document page of SM-NS-4, a `Preview` of kind `none` draws the line `doc-no-preview` ("There's no preview of this kind of file. It opens in its own app.") in place of nothing, beside the page's **Open in its app** (*amended January 2026 (user decision; Approved by the user January 2026): **Open in its app** is in the document's ⋯ menu in the tree, SM-RL-4*). Elsewhere (an approval card's content, a `preview` block), where no such button is offered, `none` still draws nothing (SM-SF-1.1).

```gherkin
  Scenario: Edge case — a file with no preview says so (SM-PV-1)
    Given files.preview answers none for "Old terms.doc"
    When its document page is drawn
    Then it shows "There's no preview of this kind of file. It opens in its own app."

  Scenario: Edge case — an approval card's content with no preview draws nothing (SM-PV-1)
    Given an approval covers "Old terms.doc", whose preview is none
    When the card's content is drawn
    Then nothing is drawn for the preview
```

### Gaps

None.

## Amendment January 2026 — Formatted messages, the working line, feedback within a second, scrolling and Close

> **Status:** Approved by the user, January 2026 ("approve"; decisions of January 2026: "yes, plan both", "yes to both, go ahead with phase 1"). Source: the person's own use of the first release on a Mac — Ordin's answers showed `**` and `- ` literally; nothing showed while Ordin was working; the conversation did not move to the newest item after **Send**; every item carried **Close**, which cluttered the conversation.
>
> Evidence read: step 3 (`conversation` items with `closed`), step 4.4 (events), step 6 (row `message`), the conversation panel row of step 7's layout table, the strings `conv-show-closed` and the setting `ordin.view.closed`; `apps/desktop/ui/src/lib/blocks/kinds/Message.svelte` (paragraphs through text interpolation); `apps/desktop/ui/src/lib/blocks/MarkdownView.svelte` (a parsed tree, no HTML; a link is drawn as its words); the store keeps a `block_shown` item's author as `from` (`apps/desktop/ui/src/lib/store/backend-state.svelte.ts`); OrdinHost.md OH-WK-1 (the event `ordin_turn_changed`, added in the same change).

- **SM-MSG-1 Ordin's messages are formatted** *(amended January 2026)*. Step 6's row `message` reads: for an item whose author is `ordin` or `system`, the text is drawn through `MarkdownView` (headings, bold, italics, inline code, bulleted and numbered lists, quotes, code blocks, rules; a link is drawn as its words, never as a link; no HTML is ever produced, so markup in the text is shown as text); for an item whose author is `person`, the text is drawn as today, paragraphs split on blank lines, so what the person typed is shown as typed. Attachments stay one-line cards. Step 6's rule "text through Svelte text interpolation only" holds: `MarkdownView` draws every piece of text by interpolation.
- **SM-WK-1 The working line** *(amended January 2026)*. Step 4.4 handles `ordin_turn_changed {workspace_id, state, doing?, since}` (OrdinHost.md OH-WK-1): the store keeps, per workspace, `ordinWorking: { since, doing? } | null` — set on `working`, cleared on `idle`, and cleared when a `block_shown` from Ordin arrives for that workspace or the backend reloads (step 5). While it is set, the conversation draws one line below its last item, never as an item and never stored: `ordin-working` ("Ordin is working…"); after 10 s of `since`, `ordin-working-for` ("Ordin is working… { $seconds } s"); with `doing`, `ordin-working-doing` ("Ordin is { $doing }… { $seconds } s"). The line has `role="status"` and `aria-live="polite"`, and its seconds are updated once a second. The message box stays usable: a message sent meanwhile is queued as today (OrdinHost step 4).
- **SM-FB-1 Feedback within a second** *(amended January 2026)*. Every action of the person that waits on the backend for more than one second shows, within one second of the action, what is happening, on the control or in the place the result will appear; and every failure says what failed and what the person can do next (ErrorHandling.md). The places this release covers: a message sent to Ordin (SM-WK-1); adding an agent (the library's **Adding { $agent }…**, unchanged); the one-time memory download (SM-MD-3, unchanged); a task Ordin hands to an agent, from its start to its end (OrdinHost.md OH-DLG-2 and OH-RC-1). A new screen or action is checked against this rule before it is approved.
- **SM-SCR-1 The conversation follows the newest item** *(amended January 2026)*. When the person sends a message, the conversation scrolls so the person's message and the working line are in view. When a new item or the working line appears and the conversation was scrolled to its end (its bottom within 48 px of the end) before it appeared, it scrolls to the new end; when the person had scrolled up to read, it does not move, and a button `conv-jump-newest` ("New messages ↓") appears at the bottom of the panel, which scrolls to the end and goes away. Scrolling respects `prefers-reduced-motion` (no smooth scrolling then).
- **SM-CL-1 Items cannot be closed** *(amended January 2026; replaces the earlier "Close only where it helps")*. No item in the conversation carries **Close**, is folded to a one-line "· closed" form, or is counted or listed as closed. The conversation panel draws no **Show again**, **Show closed items** or **Close finished items**; the strings `conv-show-closed`, `conv-close-finished`, `card-show-again` and `card-closed`, the view preference `ordin.view.closed`, the `closed` flag of a conversation item and the commands `ordin.close_item`, `ordin.show_item` and `ordin.close_finished` do not exist. `card-close` stays: other screens use it. *Status: Approved by the user, January 2026 ("I do not like show again, show closed items and the button to hide answers, this is confusing as should be removed"; "Close finished": yes; "Close and Reopen gone from the backend": yes). Source: plan 05 Phase 9.*

- **SM-RES-1 A finished task's final message is drawn as Markdown** *(amended January 2026)*. In a `result` block with `of: 'produced'` (the run screen's result, step 4.4 and the progress card), `summary` is the agent's own final message, drawn like an Ordin message (SM-MSG-1): formatted headings, lists and bold, never the marks themselves; text through Svelte text interpolation only, a link drawn as its words. *Status: Approved by the user, January 2026 ("I saw raw markdown in progress tab"; same 0.1.1). Source: plan 05 Phase 9.*

- **SM-CF-1 Clear leaves the panel empty** *(amended January 2026)*. When `ordin.start_fresh` answers `ok` after the person confirmed **Clear**, the panel at once reads the workspace's new live conversation (`ordin.list_conversations`, then `ordin.read_conversation` of the live one, as in SM-CC-4), so the items of the conversation that ended are no longer drawn; the picker's list reads again and focus goes to the message box. On an `error` the items stay and the error line shows the message. *Status: Approved by the user, January 2026 ("Clear this conversation does not work!"). Source: plan 05 Phase 9; before this the panel kept drawing the ended conversation until the next restart.*

- **SM-PG-1 A working run shows movement** *(amended January 2026, the user's report on the 0.1.1 build: \"Working in progress should show some movement?\")*. Wherever a run's state is `running` and its label `state-running` (\"Working\") is drawn, a small ring (`ActivityDot`, class `activity`, `aria-hidden`) turns beside it: on the progress card (`plan_progress` of a run, before its *Now* line or its state label), on **Home**'s *Running now* rows and on the *Running now* screen. It carries no words, and other states draw none. When the person has asked for reduced motion (`prefers-reduced-motion: reduce`) the ring stands still. *Status: Approved by the user, January 2026. Source: plan 05 Phase 9.*
- **SM-PG-2 A completed run shows every step done** *(amended January 2026, the user's report on the 0.1.1 build: "the next items are never changing nor the bullets next to them showing a check")*. On the progress card of a run whose state is `completed`, a step whose stored mark is `next` or `now` is drawn as `done` (✓, `progress-step-done`), because the agent finished its work without marking it; the stored marks are not changed, and a run in any other state draws its steps as stored. *Status: Approved by the user, January 2026. Source: plan 05 Phase 9; with MCP-PLAN-START (McpFront.md), which marks step 1 now when the steps are stated.*
- **SM-PG-3 Review is drawn only where it leads somewhere** *(amended January 2026, the user's report on the 0.1.1 build: "the Review button does nothing")*. The progress card's **Review** (`block-review`) opens the run's screen (`run:<run id>`, or `crew:<crew run id>`) in the screen area. When that screen is already the front tab of the area the card is drawn in, as on the run screen itself, the card draws no **Review**, because pressing it would change nothing. *Status: Approved by the user, January 2026. Source: plan 05 Phase 9.*
- **SM-PG-4 The progress card says where the work is, what it is doing and for how long** *(amended February 2026, the user's review of the progress card on 0.2.1-test.14 to test.16: "Actions taken so far" gives no sense of how far along the work is; user decision: steps, not a percentage, for agent work)*. The `plan_progress` card of a run draws, top to bottom:
  1. **Now** — as today (`progress-now`, or the state label), with the elapsed time beside it: `progress-running-for-seconds` ("Running for { $seconds } s") under 60 seconds, then `progress-running-for` ("Running for { $minutes } min", whole minutes, rounded down). Elapsed is measured from the time (`at`) of the first event the window received for the run to the time of the `run_state_changed` that ended it (`completed`, `failed`, `stopped_at_limit`, `cancelled`, `interrupted`, `abandoned` or `no_reply`), or to now while the run is not ended; while the run is `running` it is redrawn once a second. A run for which the window has received no event draws no elapsed time. *(Corrected while building, February 2026: the window's run view carries no `started_at`; the events' own times stand in for it.)*
  2. **The step bar** — when the run has a plan: one segment per step, in step order, each drawn in the colour of its mark (done: `--color-done`; now: `--color-accent`; next and skipped: `--color-line-strong`), with the marks of SM-PG-2. The bar is `aria-hidden`: the step list below carries the same facts in words. No percentage is drawn for a plan.
  3. **Doing** — the run's newest progress line (the last `line` of its progress lines), as `progress-doing` ("Doing: { $line }"), in the muted style, while the run is `running`; nothing in any other state.
  4. **The count line** — as today when no total is known (`progress-count-so-far`); when a total is known and above zero, `progress-count-percent` ("Items worked through: { $done } of { $total } · { $percent }%"), the percentage rounded down to a whole number and never above 100. When the run has no plan and no count, only Now, the elapsed time and Doing are drawn.
  The steps list, **Show what happened** and the actions are unchanged. New en-GB words: `progress-running-for-seconds`, `progress-doing`, `progress-count-percent`; `progress-count-of` is no longer drawn by the card. *Status: Approved by the user, February 2026 ("approve as written, build it"). Source: plan 10 Phase 2.*

- **SM-FL-1 Form fields stand one to a line** *(amended January 2026; first for Settings, then for every form after the user's report on the knowledge page's edit form: "fix the layout when editing knowledge around line returns")*. In every form of the window, a `label` that holds a text, email or password field, a selection list or a text area is drawn as its own block: the label's words above the control, with a gap of `--space-sm` below. A field is at most 460 px wide; a text area takes the width of its form up to 860 px, keeps the line returns the person types and shows them as typed. A label that holds a check box, a radio button or a slider stays in the line it is in. The rule is one style of the window (`tokens.css`), so a form added later follows it. *Status: Approved by the user, January 2026 ("this is not displaying correctly, line returns are not there", the Jira form; "also fix the layout when editing knowledge around line returns"). Source: plan 05 Phase 9.*

- **SM-ASK-1 Ask Ordin about this on an opened document sends the file** *(amended January 2026)*. On the document page (step 6, Document page row 2), **Ask Ordin about this** sends `ordin.send_message { workspace_id, text, attachments: [ { kind: 'place_file', place_id, relative_path } ] }` with the page's own place and path, as the `document` and `preview` cards already do, so Ordin's turn carries the location line of OrdinHost.md OH-ATT-1. When the page has no workspace the button is not drawn, as today. *amended January 2026 (user decision; Approved by the user January 2026):* the entry is the document row's ⋯ menu entry in the tree (SM-RL-4), sending the same message and attachment for the row's place and path; the page draws no button. *Status: Approved by the user, January 2026 ("yes"). Source: plan 05 FU-016, found while writing journey 10: the button sent the name only, so Ordin could not say which file to read.*

Strings added (en-GB): `ordin-working` = "Ordin is working…"; `ordin-working-for` = "Ordin is working… { $seconds } s"; `ordin-working-doing` = "Ordin is { $doing }… { $seconds } s"; `conv-jump-newest` = "New messages ↓". The `doing` phrases are the core's (OrdinHost.md OH-WK-1) and are localised there.

```gherkin
  Scenario: Happy path — Ordin's answer is formatted (SM-MSG-1)
    Given Ordin's message "**Done.**\n\n- one\n- two"
    When it is drawn
    Then "Done." is bold and "one" and "two" are items of a list

  Scenario: Edge case — the person's own text is shown as typed (SM-MSG-1)
    Given the person's message "**not bold**"
    When it is drawn
    Then it reads "**not bold**"

  Scenario: Error path — markup in Ordin's text never becomes an element (SM-MSG-1)
    Given Ordin's message "<img src=x onerror=alert(1)> and [a link](https://example.com)"
    When it is drawn
    Then no img element exists, the text "<img src=x onerror=alert(1)>" is shown, and "a link" is not a link

  Scenario: Happy path — Ask Ordin about an opened document sends the file (SM-ASK-1)
    Given the document page of "Quarterly plan.pdf" in the folder place P of workspace W
    When the person presses "Ask Ordin about this"
    Then ordin.send_message is sent for W with the text "Tell me more about “Quarterly plan.pdf”." and one attachment { kind: "place_file", place_id: P, relative_path: "Quarterly plan.pdf" }

  Scenario: Happy path — the working line comes and goes (SM-WK-1)
    Given the person sends "Summarise the strategy"
    When ordin_turn_changed working arrives for the workspace
    Then "Ordin is working…" shows below the last item
    And after 12 s it reads "Ordin is working… 12 s"
    When Ordin's reply is shown
    Then the line is gone

  Scenario: Edge case — the working line names what Ordin is doing (SM-WK-1)
    When ordin_turn_changed working arrives with doing "searching the library"
    Then the line reads "Ordin is searching the library… <seconds> s"

  Scenario: Happy path — sending scrolls to the newest item (SM-SCR-1)
    Given the conversation is scrolled to the top
    When the person sends a message
    Then the person's message is in view

  Scenario: Edge case — reading earlier items is not interrupted (SM-SCR-1)
    Given the person has scrolled up
    When Ordin's reply arrives
    Then the conversation does not move and "New messages ↓" shows

  Scenario: Happy path — nothing in the conversation can be closed (SM-CL-1)
    When a message and a documents card are drawn
    Then neither has a Close
    And the panel shows no Show again, no Show closed items and no Close finished items

  Scenario: Happy path — a finished task's final message is drawn formatted (SM-RES-1)
    Given a task that ended with the message "# Summary" and "**Done**" on separate lines
    When its run screen is opened
    Then the result shows a heading and bold text
    And the marks "#" and "**" are not drawn
```

### Gaps

None.

## Amendment January 2026 — Check it: the part's name, and fix buttons that act

> **Status:** Approved by the user, January 2026 ("approve, go ahead with phase 3A code"). Source: the person's own use of the first release — **Ask Ordin to fix it** under **Check it** did nothing.
>
> Evidence read: `apps/desktop/ui/src/lib/views/fixed/CheckItPanel.svelte` (each fix button calls the optional `onfix`; the finding's sentence is drawn with `part` overwritten by `part_identifier`); its users `apps/desktop/ui/src/lib/views/fixed/Agents.svelte`, `apps/desktop/ui/src/lib/views/parts/InstalledPartPageView.svelte` and `apps/desktop/ui/src/lib/views/parts/LibraryPartPageView.svelte` (none passes `onfix`); `apps/desktop/ui/src/lib/views/authoring/PartDraft.svelte` (passes no `onfix` to `DraftChecks`; `AgentDraft.svelte` does, through `authoring.apply_fix`); `apps/desktop/ui/src/lib/blocks/ask.ts` (`askOrdin` sends `ordin.send_message`).

- **SM-CHK-1 A finding names the part by its name** *(amended January 2026)*. In the Check it row of step 7's layout table, a finding is drawn from its `message_id` and `args` as they are; `part_identifier` is used only to open the part's page (**Read it**), never put into the sentence.
- **SM-CHK-2 Ask Ordin to fix it sends Ordin the problem** *(amended January 2026)*. In Check it (`CheckItPanel`), **Ask Ordin to fix it** (fix `AskOrdin`) sends `ordin.send_message` to the workspace the page is open in, with the text `check-ask-ordin-text` ("Check it found a problem with { $subject }: { $finding } Can you fix it?"), where `$subject` is the agent's or part's name shown on the page and `$finding` is the finding's sentence. While it is sent the button is disabled; on success the line shows `check-asked-ordin` ("Sent to Ordin; the answer is in the conversation.") and the conversation shows the message as the person's (SM-SCR-1 scrolls to it); an error shows its sentence with `ErrorLine`. With no workspace open, or when the backend does not answer `ordin.send_message`, the button is not drawn.
- **SM-CHK-3 A fix button always acts** *(amended January 2026)*. A fix button is drawn only when the screen has its action: in Check it, **Fix it** and **Change it yourself** are drawn only when the page passes a handler (they belong to the authoring screens); in a part draft, `PartDraft` passes its fixes to `authoring.apply_fix { draft: { kind: 'part', id } }` as `AgentDraft` does for an agent.

```gherkin
Feature: Check it's fixes (SM-CHK-1..3)

  Scenario: Happy path — Ask Ordin to fix it
    Given Check it on the document agent shows a finding
    When the person presses Ask Ordin to fix it
    Then Ordin receives "Check it found a problem with Document agent: " followed by the finding's sentence
    And the line "Sent to Ordin; the answer is in the conversation." is shown

  Scenario: Error path — the message cannot be sent
    Given ordin.send_message answers an error
    When the person presses Ask Ordin to fix it
    Then the error's sentence is shown and the button can be pressed again

  Scenario: Edge case — no handler for a fix
    Given Check it on an installed agent with a finding whose fixes are FixIt and AskOrdin
    Then only Ask Ordin to fix it is drawn
```

### Gaps

None.

## Amendment January 2026 — Settings at the foot of the side rail

> **Status:** Approved by the user, January 2026 ("proceed with recommendations"; decision of January 2026: "yes, add Settings to the rail in Phase 6"). Source: the person's own use of the first release — "I cannot see where the configuration system is". The side rail has no **Settings**, so Settings is reached only through *Search or go to…*.
>
> Evidence read: the Purpose (the App Shell's side rail lists Home, *Search or go to…* and the workspace tree); step 7's layout row *Side rail and tree*; SM-OS-1 (a screen with no workspace opens where the person is); the prototype `docs/design/ordin-desktop/user-interface.html` (the rail ends with `<button class="railbtn" id="settingsBtn" aria-pressed="false">Settings</button>` below a growing gap); `docs/design/ordin-desktop/11-ux-guidelines.md` (Back to your workspace assumes the rail's Settings); `apps/desktop/ui/src/lib/shell/SideRail.svelte` (brand, Home, *Search or go to…*, the workspaces and their foot, no Settings); `apps/desktop/ui/src/lib/views/SearchDialog.svelte` (Settings opened as `settings` with no workspace).

- **SM-RAIL-1 Settings in the side rail** *(amended January 2026)*. The side rail ends with one entry **Settings** (`rail-settings` = "Settings"), drawn after the workspace tree and its foot (Archived, the Bin) and held at the bottom of the rail; when the rail is taller than the window it scrolls inside itself (VisualDesign.md V5-SCR-1) and **Settings** follows the tree. Pressing it opens the screen `settings` of the local backend with no workspace, where the person is (SM-OS-1); the open workspace and its conversation stay open. It has `aria-pressed="true"` while `settings` is the screen in front, as **Home** does. It is drawn only when the local backend answers `settings.get` (SM-UP-1). Step 7's row *Side rail and tree* adds "**Settings** at the foot".

```gherkin
  Scenario: Happy path — Settings from the side rail (SM-RAIL-1)
    Given the workspace "Supplier review" is open
    When the person presses Settings at the foot of the side rail
    Then Settings is the screen in front and the rail's Settings is pressed
    And "Supplier review" and its conversation stay open

  Scenario: Edge case — a backend that does not answer settings.get (SM-RAIL-1)
    Given the local backend's groups do not include settings.get
    When the side rail is drawn
    Then it has no Settings
```

### Gaps

None.

## Amendment January 2026 — Remove and update an agent; no button that does nothing (FU-010, FU-011)

> **Status:** Approved by the user, January 2026 ("approve"). Source: the parity check of the first release and the user's decision ("proceed with recommendations") to put FU-010 and FU-011 in 0.1.1. Without a **Remove** on the Agents screen the document agent that 0.1.1 installs for every workspace (PKG-DOC-1) cannot be taken out of a workspace, which DSK-DOC-1's mitigation and PKG-DOC-3 assume.
>
> Evidence read: PackageCatalogue.md *Service API commands* (`agents.accept_update`, `agents.decline_update` → `InstalledAgent`; `agents.remove` with `RemoveAgentInput { agent_instance_id, workspace_id? }` → `Done`), §E (`remove` with a `workspace_id` drops that workspace's membership and uninstalls only when no membership remains), PKG-REC-3 (`PKG_BUNDLED_AGENT_FIXED` for a bundled instance); the generated types `InstalledAgent.update?: PendingUpdate { may_do_changes: MayDoChange[], to_commit }` and `MayDoChange { kind, message_id, name }` (`apps/desktop/ui/src/lib/service-api/types.gen.ts`); `apps/desktop/ui/src/lib/views/fixed/Agents.svelte` (the chosen agent's section holds *What [agent] is made of*, **Show the installed files**, folder access and **Check it**, and no Remove or Update); `apps/desktop/ui/src/lib/views/ConfirmDialog.svelte` (props `title`, `body`, `verb`, `destructive`, `initialFocus`); `apps/desktop/ui/src/lib/views/authoring/RepositoryParts.svelte` (**More › Withdraw…** is drawn whatever the backend answers).

- **SM-AG-1 Remove an agent from the workspace.** In the Agents screen's chosen-agent section, below **Check it**, one button **Remove [agent]…** (`agent-remove`, with `$agent`), drawn only when `answers(backend, 'agents.remove')` (SM-UP-1) and a workspace is open. It opens a `ConfirmDialog` titled `agent-remove-title` ("Remove { $agent } from this workspace?"), with the body `agent-remove-body` ("{ $agent } will no longer work in this workspace. Other workspaces that use it keep it."), the verb `agent-remove-verb` ("Remove") with `destructive` true (so **Remove** is never the default button) and `initialFocus` `cancel`. **Remove** sends `agents.remove { agent_instance_id, workspace_id }`. On `ok` the dialog closes, the Agents list is read again, the section closes and `agent-removed` ("{ $agent } is removed from this workspace.") is announced. **Cancel**, Escape or a click outside sends nothing. A refusal is drawn through `ErrorLine` inside the dialog, which stays open; `PKG_BUNDLED_AGENT_FIXED` therefore reads "This agent comes with ordin and can't be changed or removed." (its row in PackageCatalogue.md), and the bundled instances never reach the list (PKG-REC-3), so the button is not offered for them.
- **SM-AG-2 An update the person can accept or decline.** When the chosen agent's `InstalledAgent.update` is present, the section draws, above **Remove [agent]…**: `agent-update-heading` ("A new version of { $agent } is ready."), one line per item of `update.may_do_changes` through the message id the item carries (`MayDoChange.message_id`, with `$name` set to its `name`), then **Update** (`agent-update`) and **Not now** (`agent-update-decline`). **Update** sends `agents.accept_update { agent_instance_id }`; **Not now** sends `agents.decline_update { agent_instance_id }`. Each result is the agent's new `InstalledAgent`: the section is drawn again from it (`update` absent once the offer is gone, so the block disappears). While a button's command is in flight both are disabled. A refusal (`PKG_NO_UPDATE` among them) is drawn through `ErrorLine` and the block stays. Both buttons are drawn only when `answers(backend, 'agents.accept_update')` and `answers(backend, 'agents.decline_update')` (SM-UP-1). The desktop does not subscribe to agent notices and draws no *Needs you* entry for an update: that stays in plan 06.
- **SM-AG-3 The words of a change.** The nine `MayDoChange.message_id` values the Package Catalogue emits have no screen strings today. `screens-fixed.ftl` (en-GB) gains, *chosen here*:

  | Message id | en-GB |
  |---|---|
  | `may-do-tool-added` | It can now use { $name }. |
  | `may-do-tool-removed` | It can no longer use { $name }. |
  | `may-do-read-added` | It can now read more than before. |
  | `may-do-read-removed` | It can no longer read some of what it could. |
  | `may-do-connection-added` | It can now connect to { $name }. |
  | `may-do-connection-removed` | It can no longer connect to { $name }. |
  | `may-do-policy-changed` | What it may do without asking has changed. |
  | `may-do-declaration-changed` | What it says it does has changed. |
  | `may-do-authority-changed` | How much it may decide has changed. |

  A `message_id` the file does not hold is drawn as `agent-update-change-unknown` ("Something about what it may do has changed."), never as the id. The new strings are en-GB only: de-DE and fr-FR ship no screen strings and fall back to English (the Phase 3A correction recorded in Checker.md).
- **SM-AG-4 No entry that does nothing.** SM-UP-1 restated for **Withdraw…** on *Parts of [agent repository]*: the entry **More › Withdraw…** (`repo-withdraw`) is drawn only when `answers(backend, 'authoring.withdraw')`. When it is absent, **More** holds no other entry, so **More** is not drawn either. A backend that answers `authoring.withdraw` draws it as today.
- **SM-AG-5 The library never offers Ordin.** SM-AL-1's list holds no row for the bundled coordinator, the WorkspaceProfile `urn:ordin:default:workspaces:coordinator` of the bundled catalogue (PackageCatalogue.md PKG-REC-1, `BundledAgent::Ordin.identifier()`). The exclusion is made in `catalogue.search` and `catalogue.facets` (CatalogueService.md CAT-LIB-1), so the library screen and Ordin's `find_agent` both leave it out; the screen keeps no list of its own.

```gherkin
  Scenario: Happy path — removing an agent from the workspace (SM-AG-1)
    Given the workspace "Supplier review" has the agent "Document agent"
    And the person has chosen "Document agent" on the Agents screen
    When the person presses "Remove Document agent…" and then Remove
    Then agents.remove is sent with that agent and "Supplier review"
    And the Agents list is read again and no longer holds "Document agent"
    And the section for "Document agent" is closed

  Scenario: Edge case — cancelling the removal (SM-AG-1)
    Given the person has pressed "Remove Document agent…"
    When the person presses Cancel
    Then agents.remove is not sent
    And "Document agent" is still in the list

  Scenario: Edge case — Remove is not the default button (SM-AG-1)
    When the confirmation for "Document agent" opens
    Then Cancel has the focus
    And pressing Enter without moving does not send agents.remove

  Scenario: Error path — the removal is refused (SM-AG-1)
    Given agents.remove is answered with an error
    When the person confirms the removal
    Then the error line is drawn inside the dialog
    And the dialog stays open and the agent stays in the list

  Scenario: Happy path — accepting an update (SM-AG-2)
    Given "Document agent" has an update whose may_do_changes hold one "tool_added" for "web_search"
    When the person chooses "Document agent"
    Then the section says "It can now use web_search."
    When the person presses Update
    Then agents.accept_update is sent for that agent
    And the update block is gone

  Scenario: Happy path — declining an update (SM-AG-2)
    Given "Document agent" has an update
    When the person presses "Not now"
    Then agents.decline_update is sent for that agent
    And the update block is gone

  Scenario: Error path — an update that is no longer there (SM-AG-2)
    Given agents.accept_update is answered with PKG_NO_UPDATE
    When the person presses Update
    Then the error line is drawn and the update block stays

  Scenario: Edge case — a change the file has no words for (SM-AG-3)
    Given an update item whose message_id is not in the string file
    When the section is drawn
    Then it says "Something about what it may do has changed."
    And the id is not shown

  Scenario: Edge case — Withdraw… on a backend without authoring (SM-AG-4)
    Given the backend's groups do not include authoring
    When "Parts of Northwind agents" is drawn
    Then there is no Withdraw… and no More

  Scenario: Edge case — the document agent stays removed after a restart (SM-AG-1, PKG-DOC-3)
    Given the person removed "Document agent" from "Supplier review"
    When ordin is restarted
    Then "Document agent" is not in "Supplier review"

  Scenario: Edge case — the library does not offer Ordin (SM-AG-5)
    When the person opens the agent library and searches for "ordin"
    Then no row is Ordin, the coordinator
```

### Gaps

None.

## Amendment January 2026 — Delete an earlier conversation (SM-WR-16, SM-WR-17)

> **Status:** Approved by the user, January 2026 ("no way to remove earlier conversations"). Source: OrdinHost.md OH-DC-1; ServiceApi.md SVC-CL-4.

- **SM-WR-16 A ⋯ menu on each earlier conversation.** Each row of the Earlier conversations list, when the backend answers `ordin.delete_conversation`, has the tree's **⋯** menu (`more-actions-for`, `tree-node-menu`) holding **Delete…** (`conv-delete`, icon `i-remove`, danger). It opens `ConfirmDialog` with `conv-delete-title` (`$name` the conversation's first request), `conv-delete-body` and the verb `conv-delete-verb`, never the default button. Confirming sends `ordin.delete_conversation { conversation_id }` with the id of the row's entry; `ok` closes the dialog, reads `ordin.list_conversations` again and announces `conv-deleted` with `$name`; an error is drawn by `ErrorLine` in the dialog, which stays open. Cancel sends nothing.
- **SM-WR-17 Delete on the open earlier conversation.** The screen of an open earlier conversation (`EarlierConversation`) has **Delete…** (`conv-delete`) when the backend answers `ordin.delete_conversation`, with the same confirmation as SM-WR-16. On `ok` the screen's tab closes and the earlier conversations list, if open, reads again on its next view.

### Strings added (`crates/ordin-localisation/catalogues/en-GB/screens.ftl`)

| Id | Text |
|---|---|
| `conv-delete` | Delete… |
| `conv-delete-title` | Delete “{ $name }”? |
| `conv-delete-body` | The conversation and its messages are deleted from this computer. This can't be undone. Pages Ordin kept from it stay. |
| `conv-delete-verb` | Delete |
| `conv-deleted` | Deleted “{ $name }”. |

```gherkin
  Scenario: Happy path — Delete… on an earlier conversation (SM-WR-16)
    When the person presses ⋯ then Delete… on a row and confirms
    Then ordin.delete_conversation is sent with that conversation's id and the list is read again

  Scenario: Edge case — Cancel (SM-WR-16)
    When the person presses Delete… and then Cancel
    Then nothing is sent

  Scenario: Happy path — Delete… on the open conversation closes its tab (SM-WR-17)
    When the person presses Delete… on the open earlier conversation and confirms
    Then ordin.delete_conversation is sent and the tab closes
```

## Amendment — a fact line matches the prototype (SM-SF-11)

> **Status:** Approved by the user, 2026-01-31 ("build all four gaps"). Source: the prototype's `memory` scene (`docs/design/ordin-desktop/user-interface.html`); MemoryService.md §9.

- **SM-SF-11 The fact line.** Wherever a `FactView` is drawn by `FactLine` it shows, under the statement, its kind, when it was true, its **scope** (`memory-scope-workspace` when `workspace_id` is set, otherwise `memory-scope-everywhere`) and, when `may_be_out_of_date`, the mark `memory-out-of-date`; and on its own line where it came from, by `came_from.kind`: `conversation` → `memory-from-conversation`; `agent` → `memory-from-agent` with the agent's name (`memory-from-an-agent` when none matches); `document` → `memory-from-document` with `origin_ref` as text; `knowledge_page` → `memory-from-page`; `team` → `memory-from-team`; `person` → `memory-from-you`; `agent_citing` keeps SM-SF-6. On the Facts tab the line also offers **Correct** (`memory-correct`): it turns the statement into a text box of at most 500 characters (`memory-correct-label`), with **Save** (`memory-correct-save`, off while the text is empty, unchanged or longer than 500) and **Cancel** (`memory-correct-cancel`; Escape does the same); Save sends `memory.correct { id, statement }`, announces `memory-corrected` and reads the list again. For a fact with a `workspace_id` it offers **Remember everywhere** (`memory-everywhere`), which sends `memory.remember_everywhere { id }`, announces `memory-everywhere-done` and reads the list again. An error reply draws the owner's sentence on that fact through `ErrorLine`.

### Strings added (`crates/ordin-localisation/catalogues/en-GB/screens-fixed.ftl`)

| Id | Text |
|---|---|
| `memory-scope-workspace` | This workspace |
| `memory-scope-everywhere` | All workspaces |
| `memory-out-of-date` | may be out of date |
| `memory-from-conversation` | From our conversation |
| `memory-from-agent` | From { $agent } |
| `memory-from-an-agent` | From an agent |
| `memory-from-document` | From { $source } |
| `memory-from-page` | From a knowledge page |
| `memory-from-team` | From your team |
| `memory-from-you` | Written by you |
| `memory-correct` | Correct |
| `memory-correct-label` | Correct this fact |
| `memory-correct-save` | Save |
| `memory-correct-cancel` | Cancel |
| `memory-corrected` | Corrected. Ordin will use the new wording. |
| `memory-everywhere` | Remember everywhere |
| `memory-everywhere-done` | Remembered in every workspace. |

- **SM-SF-12 The Things list is tidy.** A thing with no fact and no link is not listed (`fact_count` and `link_count` both 0). A thing is drawn under its stored name written as it was found, that is, the other name that differs from the stored name only in letter case when there is one (`thingTitle`); an other name that is the title again in other letters is not listed under `memory-thing-also-called`, and the line is not drawn when none is left. The same applies to the open thing's heading.

## Amendment — fewer things around the conversation (SM-CC-12, SM-WR-18)

> **Status:** Approved by the user, 2026-01-31 ("what ordin used is not necessary and clutters the window"; "earlier conversations link is not needed in treeview because it exists as button on top of ordin chat"). The fact-line wording of SM-SF-11 also draws the source in the one small line, as the prototype does.

- **SM-CC-12 No What Ordin used under an answer.** A `message` block no longer draws the quiet **What Ordin used** link, whatever its `memory_used` holds. The panel and the `memory.used` command stay; the memory screen's own tabs are unchanged. This replaces the placement of ADR-111 step 7 in the conversation.
- **SM-WR-18 No Earlier conversations row in the tree.** The tree no longer draws the *Earlier conversations* row of SM-WR-7. The **Earlier conversations** button above the conversation (`conv-earlier`) and *Search or go to…* open the same screen; SM-WR-7's screen, SM-WR-16 and SM-WR-17 are unchanged.

## Amendment — the conversation header and the Agents tree (SM-CC-13, SM-CC-14, SM-AT-1)

> **Status:** Approved by the user, January 2026 ("do all"). Source: the prototype's `chatTools` row and its `ag:` tree group (`docs/design/ordin-desktop/user-interface.html`); user findings that the header's "New conversation" drop-down was confusing, that the Clear popup did not say what keeping key points does, and that the Agents row did not behave like the prototype.

- **SM-CC-13 The conversation header.** The conversation panel's tools are **Summarise**, **Clear** and **Earlier** only, as in the prototype. The conversation picker drop-down of SM-CC-1 is no longer drawn there (its model still supplies the count). **Earlier** (`conv-earlier`) reads `conv-earlier-count` with the number of earlier conversations when it is above 0, has the accessible name `conv-earlier-label` (or `conv-earlier-name` with none), and opens the Earlier conversations screen.
- **SM-CC-14 The Clear popup says what it does.** The popup shows `confirm-clear-body`: that Ordin forgets what was said here, that the conversation stays readable in Earlier conversations, that **Keep the key points first** lets the person choose which points go to What Ordin remembers, and that **Summarise** keeps the conversation going instead. Showing the points as ticked boxes inside the popup, as the prototype does, needs Ordin to propose them before the popup opens and is not part of this amendment.
- **SM-AT-1 Agents in the tree.** When the backend answers `agents.list`, the Agents row (`tree-agents`) is drawn by `AgentsTreeGroup` in place of a single leaf: its main action opens the Agents screen and shows, under it, one row per agent of the workspace (`agents.list { workspace_id }`, read again on each `workspace_changed`). The row draws no number of agents: a plain count is clutter, and a root row of the side tree draws a count only when it calls for the person, such as a workspace's waiting count (amended January 2026; user decision; Approved by the user January 2026). The group's ⋯ menu holds **Browse the agent library** (`library-browse`). Each agent row's main action shows or hides its tasks; its ⋯ menu holds **What it's made of** (`tree-agent-made-of`, opens the installed-files screen, when `agents.installed_files` is answered) and **Remove {agent}…** (`agent-remove`, when `agents.remove` is answered), which asks with `agent-remove-title`, `agent-remove-body` and `agent-remove-verb`, then sends `agents.remove { agent_instance_id, workspace_id }`, announces `agent-removed` and reads the list again; an error reply is drawn in the dialog. A task row, when `ordin.send_message` is answered, is a button (`tree-task-run`) that sends `tree-task-run-request` to Ordin as the person's message; otherwise it is plain text.

### Strings added (`crates/ordin-localisation/catalogues/en-GB/screens.ftl`)

| Id | Text |
|---|---|
| `conv-earlier-count` | Earlier ({ $count }) |
| `conv-earlier-label` | Earlier conversations ({ $count }) |
| `confirm-clear-body` | Ordin forgets what was said here and starts a new conversation. This one stays readable in Earlier conversations. To carry decisions over, choose the key points first: Ordin lists them and you pick which ones go to What Ordin remembers. Summarise instead keeps this conversation going. |
| `tree-agent-made-of` | What it's made of |
| `tree-task-run` | Ask Ordin to run { $task } |
| `tree-task-run-request` | Run { $task } with { $agent }. |

## Amendment January 2026 — A task's form opens from the tree (SM-AT-2, SM-TF-1 to SM-TF-8)

> **Status:** Approved by the user January 2026. Source: HLD 09 ADR-119 items 3–5 (decision 101); HLD 11 §5.3 and UX-A35; the prototype's `ag:` tree group and `startTask` / `taskForm` (`docs/design/ordin-desktop/user-interface.html`: the form opens as a screen, nothing is written to the conversation, fields are labelled, a line says which files the agent will be given, and the verb starts the task). Counterparts: ServiceApi.md SVC-TF-1 (`runs.start_task`) and SVC-TF-2 (`InstalledAgent.task_forms`), OrdinHost.md OH-TF-1 to OH-TF-4, 2C wire WIRE-TF-1 and WIRE-TF-2. Every section above stays in force, except that SM-AT-2 replaces the last sentence of SM-AT-1.
>
> Evidence read: SM-AT-1 and `apps/desktop/ui/src/lib/shell/AgentsTreeGroup.svelte` (a task row sends `ordin.send_message` with `tree-task-run-request`); `apps/desktop/ui/src/lib/blocks/kinds/Form.svelte` (the document picker: a link, then a file from `files.list_folder`; the cursor in the first empty field; `block-filled-by-ordin`); `apps/desktop/ui/src/lib/shell/screens.ts` (a screen key is `<id>` or `<id>:<instance>`, the instance being everything after the first `:`); SM-RS-1 (`run:<run id>`) and SM-RS-8 (`started` opens the run's screen; `waiting` draws `run-screen-waiting`); SM-UP-1 (a control is drawn only when the backend answers its commands); `workspaces.grants`.

- **SM-AT-2 Pressing a task opens its form** (replaces SM-AT-1's last sentence). Under each agent, the task rows are the agent's `task_forms` (PackageCatalogue.md PKG-TF-1), in order, each drawn by its `title`. When the backend answers `runs.start_task`, a task row is a button whose main action opens the screen `task-form:<agent instance id>:<command id>` in the workspace, with the task's title as the tab's title and focus on the screen (UX-A35); it writes nothing to the conversation and sends no command. Otherwise the row is plain text. `tree-task-run` and `tree-task-run-request` are no longer drawn; their strings are removed from `screens.ftl`.

- **SM-TF-1 The task form screen.** Registered as `task-form` (`lib/views/tasks/register.ts`, title id `screen-task-form`, commands `agents.list` and `runs.start_task`). Its instance is `<agent instance id>:<command id>` (an agent instance id holds no `:`; the command id, which does, is the rest). It reads `agents.list { workspace_id }` (again on `workspace_changed`) and draws the `TaskForm` with that `command_id` of the agent with that id. None found → `task-form-gone` with **Close** (`task-form-close`). `startable` false → the title, then `task-form-unreadable`, and **Close**; no field is drawn.

- **SM-TF-2 What the form shows,** top to bottom: the title as the heading; the `description` as one line; one field per input in input order (SM-TF-3); for a task with `inputs` None, one long-text field named `arguments`, labelled `argument_label` or else `task-form-details-label`, not required; the access line (SM-TF-4); then the task's own verb, a primary button whose label is the title (ADR-119 item 3, as the prototype's verb), and **Cancel** (`task-form-cancel`). On opening, the cursor is in the first field without a value (the first field when all have one), as `Form.svelte` does.

- **SM-TF-3 Fields by kind.** Every field has its `label` as a visible label, drawn above the field in the normal reading size (14 px, the body text's size); a required input adds `task-form-required-mark` after the label, and an input with `required: false` adds `task-form-optional` (*amended January 2026 (user decision; Approved by the user January 2026)*); a `hint` is drawn below the label, above the field, in smaller type (12 px, `--color-text-muted`) and joined to the field with `aria-describedby`. Each field starts from the input's `default`, else empty.
  - *short text*: a one-line text box, at most 200 characters.
  - *long text*: a text area of six lines that grows with its text, at most 4,000 characters.
  - *document* (*amended January 2026 (user decision; Approved by the user January 2026)*): one button, **Choose a document…** (`task-form-choose-document`), opens the document browser (SM-TF-9). Once a file is chosen the field shows it as `task-form-chosen-document` ("03-nats-key-rotation.md, in docs/wip/plan of ordin": the file name, the folder it sits in inside its place, and the place's name), or `task-form-chosen-document-top` when the file sits at the top of its place, with **Choose another** (`task-form-change-document`), which opens the browser again.
  - *documents* (*amended January 2026 (user decision; Approved by the user January 2026)*): **Choose a document…** while the list is empty and **Add** (`task-form-add-document`) once it holds one, each opening the document browser; each chosen document is listed as the document field shows it, with **Remove** (`task-form-remove-document`), followed by `task-form-documents-count`; **Add** is off once the list holds `max`, and a file already in the list is not added twice. A documents field that is filled holds at least two documents (`DOCUMENTS_MIN`, AgentModel.md AM-TF-2; ADR-119 item 1).
  - *choice*: two to four choices as radio buttons, more as a drop-down list, in the order written; an input with `required: false` and no default offers `task-form-no-choice` first.
  - *number* (*amended January 2026 (user decision; Approved by the user January 2026)*): a short box about six characters wide (`size="6"`) with the numeric keyboard hint (`inputmode="decimal"`), and beside it two step buttons, `task-form-step-up` and `task-form-step-down`, each changing the value by 1 (an empty box steps from 0); the Up and Down arrow keys in the box do the same. The person's locale's decimal separator is accepted and sent as a full stop. For an input with `whole: true` (AgentModel.md AM-TF-2) the box has `inputmode="numeric"` and takes only digits and a leading minus sign: any other character typed or pasted is dropped.
  - *date*: the platform's date field; its value is sent as `YYYY-MM-DD`.

- **SM-TF-4 The access line.** When at least one chosen document lies in a linked folder on which the agent holds no grant (`workspaces.grants { workspace_id }`, read when the screen opens and on `workspace_changed`; when the backend does not answer it, every folder of a chosen document counts), the form draws, above the buttons, `task-form-access` with the agent's name and those folders' names joined by the locale's list format. It changes as documents are chosen and removed. This is the access the task's start grants (OH-TF-1 step 7): nothing is granted while the form is open or while the conversation asks a question first, and once granted the access lasts, as a grant made on the access card does, which the line says (ADR-119 item 3).

- **SM-TF-5 Checked before sending** (ADR-119 item 4). Pressing the verb first checks each field as OrdinHost.md OH-TF-1 step 3 does, except that a document's presence is checked by the core: required and empty → `task-form-required` (a document field `task-form-required-document`; a documents field `task-form-required-documents`); over the length → `task-form-too-long`; a number that does not read → `task-form-not-a-number`; a number input with `whole: true` holding a fraction → `task-form-not-a-whole-number` (*amended January 2026 (user decision; Approved by the user January 2026)*); a date that does not read → `task-form-not-a-date`; a filled documents field with one document → `task-form-too-few-documents`; more documents than `max` → `task-form-too-many-documents`. Each problem is drawn under its field, the field carries `aria-invalid="true"` and the message through `aria-describedby`, focus moves to the first field with a problem, and nothing is sent. A `refused` answer from the core draws its problems the same way: `required_empty`, `too_long`, `not_a_number`, `not_a_date`, `too_few_documents` and `too_many_documents` as above (`not_a_number` on a whole-number input → `task-form-not-a-whole-number`, *amended January 2026 (user decision; Approved by the user January 2026)*); `not_a_choice` → `task-form-not-a-choice`; `document_gone` → `task-form-document-gone` with the file name; `wrong_kind` and `unknown_input` → `task-form-changed` on the first field, since the task changed since the screen read it, and the screen reads `agents.list` again.

- **SM-TF-6 Starting.** The verb sends `runs.start_task { workspace_id, agent_instance_id, command_id, values, block_id? }` with one value per field that has one (text and number as `text`, date as `text` in `YYYY-MM-DD`, document as `document`, documents as `documents`, choice as `choice`); the button reads `task-form-starting` and is off until the reply. `started` → announce `task-form-started` with the agent's name and the title, open `run:<run_id>` in the workspace with focus (SM-RS-1), and close the form's tab. `waiting` → `run-screen-waiting` in a polite live region under the buttons, the form stays open and unchanged (the question waits in the conversation and in *Needs you*). An error reply → `ErrorLine` under the buttons with the owner's sentence and action.

- **SM-TF-7 Cancel.** **Cancel** and Escape close the form's tab and send nothing. What was typed is not kept.

- **SM-TF-8 A form Ordin opened** (11 §5.3, UX-A35, WIRE-TF-1). A `form` block whose `task` is set opens at once as a screen, as every form does, with its **Waiting for you** card, and is drawn by the task form of SM-TF-2 to SM-TF-6 instead of the general form: the definition from `agents.list` as SM-TF-1, each field starting from the block's field of the same name (`Text` into short text, long text, number and date; `Document`; `Documents`; `Choice`), and fields with `filled_by_ordin` marked `block-filled-by-ordin`. Its verb sends `runs.start_task` with `block_id`; `started` also marks the block answered, as the general form does. **Cancel** closes the screen only: the card keeps waiting and opens the form again, with Ordin's values. A block no longer waiting draws its fields read-only and `task-form-sent` in place of the buttons.

- **SM-TF-9 The document browser** (amended January 2026 (user decision; Approved by the user January 2026); replaces the two drop-down lists of the document picker). **Choose a document…**, **Choose another** and **Add** open a small dialog (`role="dialog"`, `aria-modal="true"`, title `task-form-browser-title`, `aria-describedby` the field's label) that works as the side tree's Documents group does (SM-RL-1, SM-RL-3):
  1. **At the top**, the places of the workspace: first each place of `workspaces.tree` that no link names (the workspace's own folder), by its `friendly_name`; then the active links of `workspaces.links` that have a `place_id`, in the side tree's order (`orderedLinks`: SharePoint, Confluence, This computer, then the linked code repositories, kind `git_repository_folder`), by their `display_name`. A Jira issue and a link with no place yet are left out, since they hold no file to choose.
  2. **Folders open** one level at a time: a place or folder row opens to its folders and files, read with `files.list_folder { place_id, folder, sort: "name_asc" }` when it is first opened, folders first as the core returns them, with **Show more** (`settings-show-more`) while `next_cursor` is set and `task-form-browser-empty` for an empty folder. A file row's button chooses it.
  3. **A search box** (`task-form-browser-search` as its label) above the list, drawn when the backend answers `files.search`: from one character typed (after trimming), 250 ms after the last key, `files.search { place_ids: <every place of step 1>, query, limit: 50 }` replaces the list with the hits whose `name_match` is true, each drawn by its file name with `task-form-chosen-document`'s folder and place below it; none → `task-form-browser-none`. Clearing the box shows the places again.
  4. **Choosing a file** closes the dialog, sets the field (a document field) or adds the file (a documents field), clears the field's problem and puts focus back on the field's button. **Cancel** (`task-form-cancel`) and Escape close the dialog only: the form stays open and its tab is not closed.
  5. An error reply to `workspaces.links`, `files.list_folder` or `files.search` is drawn by `ErrorLine` in the dialog, with **Try again**.

  A file in a linked code repository is chosen as any file is: the value sent is `place_file { place_id: <the repository link's place_id>, relative_path }` (ServiceApi.md SVC-TF-2; OrdinHost.md OH-TF-1 step 3), so no new value kind is needed.

### Strings added (`crates/ordin-localisation/catalogues/en-GB/screens.ftl`)

| Id | Text |
|---|---|
| `screen-task-form` | Task |
| `task-form-gone` | This task isn't available any more. |
| `task-form-close` | Close |
| `task-form-unreadable` | This task can't be started from here. Ask Ordin in the conversation instead. |
| `task-form-details-label` | What should the agent do? |
| `task-form-cancel` | Cancel |
| `task-form-optional` | (optional) |
| `task-form-required-mark` | (required) — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-choose-document` | Choose a document… — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-chosen-document` | { $file }, in { $folder } of { $place } — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-chosen-document-top` | { $file }, in { $place } — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-browser-title` | Choose a document — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-browser-search` | Search by file name — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-browser-none` | No file has that name. — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-browser-empty` | This folder is empty. — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-step-up` | One more — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-step-down` | One less — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-change-document` | Choose another |
| `task-form-add-document` | Add |
| `task-form-remove-document` | Remove { $name } |
| `task-form-documents-count` | { $count } of { $max } documents |
| `task-form-no-choice` | None |
| `task-form-access` | When this task starts, { $agent } can read { $folders }, for this task and later ones. |
| `task-form-required` | Fill this in to start. |
| `task-form-required-document` | Choose a document to start. |
| `task-form-required-documents` | Choose at least two documents to start. |
| `task-form-too-long` | This is too long. Keep it to { $max } characters. |
| `task-form-not-a-number` | Enter a number, such as 2 or 3.5. |
| `task-form-not-a-whole-number` | Enter a whole number, such as 2. — amended January 2026 (user decision; Approved by the user January 2026) |
| `task-form-not-a-date` | Enter a whole date. |
| `task-form-too-few-documents` | This field takes two or more documents. |
| `task-form-too-many-documents` | Choose at most { $max } documents. |
| `task-form-not-a-choice` | Choose one of the options. |
| `task-form-document-gone` | { $name } isn't in this workspace any more. Choose another document. |
| `task-form-changed` | This task has changed since the form opened. Check the fields and try again. |
| `task-form-starting` | Starting… |
| `task-form-started` | Started. { $agent } is working on { $task }. |
| `task-form-sent` | Sent. The work has started. |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Screens — a task's form (ADR-119)

  Scenario: Happy path — a task from the tree opens its form (SM-AT-2, SM-TF-1)
    Given the document agent in the workspace and a backend that answers runs.start_task
    When the person opens the Agents row, opens the document agent and presses "Summarise a document"
    Then the screen "task-form:<document agent id>:urn:ordin:default:commands:documents:summarise-document" opens with focus
    And it shows the fields "Document to summarise", "Who it is for (optional)", "Length" set to "One page" and "Anything to bring out (optional)"
    And no command is sent and nothing is added to the conversation

  Scenario: Error — a required document left empty (SM-TF-5)
    When the person presses "Summarise a document" on the form without choosing a document
    Then "Choose a document to start." is shown under "Document to summarise", focus moves to it and nothing is sent

  Scenario: Happy path — starting opens the run (SM-TF-4, SM-TF-6)
    Given the person chose "strategy.pdf" in the folder "Docs", on which the document agent holds no grant
    Then the form shows "When this task starts, Document agent can read Docs, for this task and later ones."
    When the person presses "Summarise a document"
    Then runs.start_task is sent and, on started, the run's screen opens and the form's tab closes

  Scenario: Edge — the conversation asks first (SM-TF-6)
    Given runs.start_task answers waiting
    When the person presses the verb
    Then "Nothing has started yet: the conversation needs you first." is shown and the form stays open

  Scenario: Error — a document that left the workspace (SM-TF-5)
    Given runs.start_task answers refused with document_gone for "reference" naming "q2.pdf"
    Then "q2.pdf isn't in this workspace any more. Choose another document." is shown under "Earlier or reference document"

  Scenario: Edge — a form Ordin opened (SM-TF-8)
    Given a waiting form block for "Compare documents" with reference filled in by Ordin
    When it opens
    Then it is drawn as the task form with "Earlier or reference document" marked as filled in by Ordin
    And Cancel closes the screen while the conversation card still waits

  Scenario: Happy path — a document deep in a code repository (SM-TF-3, SM-TF-9; amended January 2026 (user decision; Approved by the user January 2026))
    Given the workspace links the code repository "ordin", which holds "docs/wip/plan/03-nats-key-rotation.md"
    When the person presses "Choose a document…", opens "ordin", then "docs", "wip" and "plan", and chooses "03-nats-key-rotation.md"
    Then the browser closes and the field reads "03-nats-key-rotation.md, in docs/wip/plan of ordin" with "Choose another"
    And the value sent is place_file with the repository link's place and "docs/wip/plan/03-nats-key-rotation.md"

  Scenario: Happy path — searching by file name (SM-TF-9; amended January 2026 (user decision; Approved by the user January 2026))
    When the person types "rotation" in "Search by file name"
    Then the hits of files.search whose name matches are listed, and choosing one sets the field

  Scenario: Edge — a whole number (SM-TF-3, SM-TF-5; amended January 2026 (user decision; Approved by the user January 2026))
    Given the input "phase" of kind number with whole true
    When the person types "2.5"
    Then the box holds "25", and "One more" makes it "26"
    And a refused not_a_number for "phase" reads "Enter a whole number, such as 2."

  Scenario: Edge — labels say what is required (SM-TF-3; amended January 2026 (user decision; Approved by the user January 2026))
    Then "Document to summarise (required)" and "Who it is for (optional)" are the labels, each hint below its label

  Scenario: Edge — a backend without the command (SM-AT-2)
    Given a backend that does not answer runs.start_task
    Then each task row is plain text
```

### Gaps

None. Dragging a document onto a field is not in scope: ADR-119 item 3 makes it a later change, and the pickers of SM-TF-3 are how documents are chosen.

## Amendment January 2026 — Facts as triples, words, conflicts, passages and the memory diagram (SM-SF-13 to SM-SF-24)

> **Status:** Approved by the user January 2026. Source: decision 100, HLD revision 0.68.0, ADR-118 items 4–8 and 11; decision 102, ADR-120 items 1–7. Backend: `components/MemoryService.md`, amendment January 2026 (ADR-118, ADR-120: `FactView.triple`, `older_note`, `conflicts_with`, `memory.vocabulary`, `memory.rename_term`, `memory.merge_term`, `memory.keep_conflict_side`, `memory.passages`, `memory.diagram`, `memory.correct` with an object). Wire shapes: `phase-2c-shared-types-wire.md` §17. Every rule below adds to the sections above; where one replaces a rule, it says so. On screen the words are *thing*, *word* and *value*; "triple", "predicate" and "term" are never shown.

- **SM-SF-13 A fact reads as its three parts** (ADR-118 item 6; replaces the statement line of SM-SF-11 for a fact with `triple`). `FactLine` draws `triple.subject.name`, `·`, `triple.predicate.label`, `·`, and the object: a thing's name; a text as written; a number as `value` then `unit`; money formatted in the active locale by `Intl.NumberFormat` with `style: "currency"` and the fact's currency (a new helper `formatMoney` in `lib/format/intl`); a date by its precision (`YYYY` as the year, `YYYY-MM` as month and year, `YYYY-MM-DD` by `formatDate`); yes/no as `memory-value-yes` / `memory-value-no`. The subject and a thing object are link buttons that open the thing (as `fact.things` do today). The line's accessible name is the three parts joined by spaces, never `·`. The source line of SM-SF-11 stays beneath, unchanged. A fact with `triple` absent and `older_note` true draws its `statement` as before, and the mark `memory-older-note` (a tag, with the tooltip `memory-older-note-help`) after the kind. *Amended January 2026 (user decision; Approved by the user January 2026):* a fact line is concise and collapsed by default. Collapsed, its first line is the word and the value, prominent (`triple.predicate.label`, `·`, the object as above), and its second line is the subject in normal text, still the link button that opens (or, beside the diagram, centres on) the thing; a fact with no `triple` shows its `statement` alone. A small button **Details** (`memory-fact-more`; **Hide details**, `memory-fact-less`, while open) with `aria-expanded`, reached with Tab and pressed with Enter or Space, expands the line. Expanded, the line adds the evidence line led by since when the fact is true (`memory-true-since` / `memory-true-between`), then the source, the kind and the scope; the **Older note** mark and its explanation; the conflict line of SM-SF-14; the replaced, fading, shared and folder lines; and the actions **Correct**, **History**, **Forget**, **Keep**, **Remember everywhere** and the Adopt actions, where they were. Collapsing closes an open correction unsaved. The accessible name stays the three parts joined by spaces, subject first. This applies in the Facts tab, a thing's page and the lists beside the diagram (SM-SF-22). *Amended January 2026 (user decision; Approved by the user January 2026):* the three parts are one line that reads as a sentence, subject · word · value ("Manchester zero carbon goal · target year · 2038"), the value in semibold; the source line, scope, conflicts and actions sit under **Details**, closed by default; a conflict keeps a small mark while closed. The line's secondary actions are small icon buttons in the editor toolbar's style (SM-NS-4.6, 1.75 rem square), each with its verb as `aria-label` and `title`: **Details** / **Hide details** `i-chev` (turned 90° while open, `aria-expanded`), **Correct** `i-edit`, **History** `i-history` (`aria-expanded` while its history is open), **Keep it** (`team-check-keep`) `i-eye`, **Remember everywhere** `i-ws`, **Forget** `i-bin` and **Open the page** `i-open`; **Keep** of a fading fact and **Keep this one** keep their words. *Amended January 2026 (user decision; Approved by the user January 2026):* **Save** (`memory-correct-save`) and **Cancel** (`memory-correct-cancel`) of an open correction are icon buttons of the same style, `i-check` (Save, off on the terms of SM-SF-11) and `i-remove` (Cancel), each with its word as `aria-label` and `title`. **History** opens the fact's history inside its own line (step 6 *History*) (amended January 2026; user decision; Approved by the user January 2026).
- **SM-SF-14 Conflicts with** (ADR-118 item 5). A fact with `conflicts_with` non-empty draws, under its source line, `memory-conflicts-with` followed by each conflicting fact's three parts (read with `memory.get` once per list for ids not already on screen), each as a link that scrolls to and focuses that fact's line when it is in the list. The line is marked by the text and by the icon `info` (`ICON_NAMES` in `apps/desktop/ui/src/lib/icons/sprite.ts`; symbol `i-info` of `apps/desktop/ui/src/assets/icons/ordin-icons.svg`), never by colour alone. On the Facts tab and a thing's page each side offers **Keep this one** (`memory-keep-this`): it asks with `memory-keep-this-title` and `memory-keep-this-body` (naming the other fact's three parts), verb `memory-keep-this-verb`, then sends `memory.keep_conflict_side { keep, other }` (for two or more conflicts the dialog lists them as radio buttons and sends one call per other fact, stopping at the first error), announces `memory-keep-this-done` and reads the list again. `MEM_CONFLICT_NOT_FOUND` draws its sentence through `ErrorLine` and reads the list again. *Amended January 2026 (user decision; Approved by the user January 2026):* while the line is collapsed (SM-SF-13) a fact in conflict keeps a small mark under its subject, the icon `info` and the words `memory-fact-conflict-mark` ("Conflicts with another fact"), so a conflict is never hidden; the full line above and **Keep this one** appear when the line is expanded.
- **SM-SF-15 Correct a value** (replaces SM-SF-11's text box for a fact with `triple`). **Correct** opens one field for the object by its kind: a thing → a text box for the name (1–200); text → a text box (1–1,000); number → a decimal box and a unit box (0–32); money → an amount box and a currency box (three letters, upper-cased as typed); date → a text box taking `YYYY`, `YYYY-MM` or `YYYY-MM-DD` with the hint `memory-correct-date-hint`; yes/no → a two-option radio group. Labelled `memory-correct-value-label { word }`. **Save** is off while the value is empty, unchanged or not of its form; it sends `memory.correct { id, object }`, announces `memory-corrected` and reads the list again. An older note does not offer **Correct**; it offers **Forget** as before.
- **SM-SF-16 The Things list** (replaces SM-SF-12's first sentence). The list draws the things `memory.things` returns; the backend lists only things named by at least one live fact that is not a passage, so a thing with no such fact is never drawn, whatever its link count. The rest of SM-SF-12 (the title as found, other names) stands.
- **SM-SF-17 The Kinds of fact tab** (ADR-118 item 4; the tab's name and its person-facing words amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25). *Memory* gains a tab **Kinds of fact** (`memory-tab-words`) after Things. It reads `memory.vocabulary { workspace_id }` when first shown and after each change, and lists each word: its label (for a word with `seed_unrenamed`, the catalogue's `memory-term-<key>` when the active locale has it, else the label), its meaning, what it links to (`memory-word-object-thing`, `-text`, `-number`, `-money`, `-date`, `-yes-no`), `memory-word-one` or `memory-word-several`, who added it (`memory-word-by-seed`, `memory-word-by-ordin`, `memory-word-by-agent { agent }` / `memory-word-by-an-agent`, `memory-word-by-hosted-agent` for `added_by` `hosted_agent`, `memory-word-by-team`, `memory-word-by-you`) and `memory-word-count { count }`. Above the list one line `memory-words-intro` says that each one is a kind of fact Ordin can keep (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25). Each word's ⋯ menu holds:
  1. **Rename** (`memory-word-rename`): a text box (1–64) in place, Save/Cancel as SM-SF-11's; sends `memory.rename_term { workspace_id, key, label }`; announces `memory-word-renamed`. `MEM_TERM_LABEL_TAKEN` draws its sentence under the box.
  2. **Merge into…** (`memory-word-merge`): a dialog `memory-word-merge-title` with a list of the other words of the same object kind (words of another kind are not offered), the body `memory-word-merge-body { count }` and the verb `memory-word-merge-verb`; sends `memory.merge_term { workspace_id, key, into }`; announces `memory-word-merged { count }`. Errors are drawn in the dialog.
  A word whose label names a kind of note (SM-SF-26) also shows the line `memory-word-not-a-kind` under its meaning (amended January 2026 (user decision; Approved by the user January 2026)).
- **SM-SF-18 What Ordin read** (ADR-118 item 8). A knowledge page's screen gains a collapsed section **What Ordin read** (`memory-read-title`) after the page body. Opening it reads `memory.passages { workspace_id, page_id }` and draws each passage's heading and the first 300 characters of its text, with `memory-read-version { version }`; none → `memory-read-empty`. Passages are never drawn by `FactLine`, and a recall result with `passage` true is not drawn in any fact list. *Amended January 2026 (user decision; Approved by the user January 2026):* a knowledge page draws no **What Ordin read** section. A page's own sections are its passages, so the section only repeated the page; the passages stay in memory for recall, and the page's sources stay where the page lists them.
- **SM-SF-19 A thing's summaries.** A thing's page draws `ThingDetail.summaries` under **What Ordin summed up** (`memory-summaries-title`), each with its date and `memory-made-from-count`, and **Forget** as a fact's; they are no longer in the Facts tab.
- **SM-SF-20 Show as diagram** (ADR-120 items 1, 4). The Things tab and each thing's page offer **Show as diagram** (`memory-diagram-show`). From the Things tab it opens the diagram screen without a centre (`memory.diagram { workspace_id }`); from a thing's page, centred on that thing (`memory.diagram { workspace_id, centre }`). The screen has the heading `memory-diagram-title`, then the explanatory line `memory-diagram-intro` (ADR-120 item 5), then, when `left_out > 0`, `memory-diagram-left-out { shown, total }` with a search box (`memory-things-search`) whose choice centres the diagram on a thing; then the diagram and, beside it, the centred thing's list (SM-SF-22); and **Back to the list** (`memory-diagram-back`), which returns to the Things tab (or to the thing's page when opened from it). Escape does the same as Back. *Amended January 2026 (user decision; Approved by the user January 2026):* the drawing and the lists sit side by side when the window is at least 900 px wide, the drawing first and taking three fifths of the width; under 900 px the lists go below the drawing. The lists column holds, top to bottom, the panel of SM-SF-22, **Facts about {thing}**, then **Things in the diagram**, spaced with the theme's spacing tokens.
- **SM-SF-21 What is drawn** (ADR-120 item 2). The diagram is drawn by `MemoryDiagram.svelte` with ECharts' `graph` series, loaded through `chart-draw.ts`'s loader with `charts.GraphChart` added to its `core.use` list (no new dependency; ADR-120 item 7), layout `force` with `roam: true` (move and zoom) and `draggable: false`. Each `DiagramView.things` entry is a node: a rounded box with the thing's name (`symbol: 'roundRect'`), the centre drawn larger and in the brand's accent; each `joins` entry is an edge from `from` to `to` with an arrow at the end (`edgeSymbol: ['none', 'arrow']`) and the label `word.label`; each `values` entry is a small node joined to the centre by an edge labelled `word.label`, its text the value as SM-SF-13 writes it. An edge whose `conflict` is true is dashed (`lineStyle.type: 'dashed'`) and its label is `memory-diagram-conflicts { word }`, so a conflict is never told by colour alone. Colours come from the theme's CSS variables (ADR-116), read once when drawn and again on a theme change. Ended facts, older notes and passages are never in `DiagramView`, so never drawn. A drawing failure follows `chart-draw.ts`'s rule: the list of SM-SF-22 is shown alone and `UI_CHART_RENDER_FAILED` is recorded. *Amended January 2026 (user decision; Approved by the user January 2026):* when the drawing fails, a line under the hidden canvas says `memory-diagram-not-drawn` ("The diagram could not be drawn. The lists show the same facts."), as a status, so the person knows why only the lists are there. ECharts is loaded under the window's frozen `Object.prototype` (OrdinDesktop `tauri.conf.json` `freezePrototype: true`): the build rewrites the one assignment in `echarts/lib/core/Scheduler.js` that cannot run under it (`apps/desktop/ui/echarts-frozen-prototype.ts`), and stops when that assignment is no longer found.
- **SM-SF-22 Using it** (ADR-120 items 4, 6). Pressing a thing node sends `memory.diagram { workspace_id, centre: <that thing> }` and redraws. Beside the diagram, the list **Facts about {thing}** (`memory-diagram-facts-title`) holds the centre's facts, read with `memory.thing` and drawn by `FactLine` (with SM-SF-13 to SM-SF-15). Under it, the list **Things in the diagram** (`memory-diagram-things-title`) holds every thing of `DiagramView.things` as a button, in the order of `things` (the centre first, then step 1, then step 2); without a centre it is the only list. Inside a fact line beside the diagram, the subject and a thing object centre the diagram on that thing instead of opening its page (ADR-120 item 6), and the thing's page is reached from the line's thing menu as **Open {thing}** (`memory-diagram-open-thing`). Pressing an edge or a value node opens that fact's line (by `fact_id`, read with `memory.get`) in a panel under the diagram, with its source line and **Correct**, **Forget** and **History**; the diagram itself changes nothing, and after any change on the line the diagram is read again. Accessibility: the list beside the diagram is the diagram's equivalent; the canvas has `aria-hidden="true"`, ECharts' `aria` is off for it, and each drawn thing, up to two steps away, is a button of **Things in the diagram** reachable with Tab in the list's order whose **Enter** centres on it (the same call as pressing the node), so every box of the drawing can be reached from the keyboard; the drawn edges are the facts of the lists. The screen announces `memory-diagram-centred { thing }` after each centring. *Amended January 2026 (user decision; Approved by the user January 2026):* the fact lines beside the diagram and in the panel under it are collapsed and expanded as SM-SF-13 says; **Correct**, **Forget** and **History** are among the expanded line's actions. *Amended January 2026 (user decision; Approved by the user January 2026):* the fact line opened in the panel under the diagram, by pressing an edge or a value node, starts expanded. *Amended January 2026 (user decision; Approved by the user January 2026):* under **Facts about {thing}** a fact whose subject is the centre drops the subject, since the heading names it, and reads word · value; its accessible name keeps the three parts. The facts are grouped by word, each word's group in the order the word is first met: first the groups whose value is written (a text, number, money, date or yes/no), then the groups whose value is another thing, and last the facts whose subject is another thing. **Things in the diagram** is a compact run of small buttons that wraps, the centre marked by `aria-current` and bold text. The panel opened by pressing an edge or a value node is no longer under the diagram: it opens at the top of the lists column (beside the drawing, or directly under it in a narrow window), framed, with the title **The fact you pressed** (`memory-diagram-fact-title`), which takes the focus, and a **Close** button (`memory-diagram-fact-close`) that closes the panel only. The diagram screen is simplified: the drawing spans the whole width of the pane; directly under it, **Things in the diagram** stays as the compact run of buttons that is the drawing's keyboard equivalent; under that the screen draws only what the person selected — the fact pressed (its panel, titled **The fact you pressed**) and the facts of the thing pressed or centred (**Facts about {thing}**, grouped as above). With nothing selected (no centre and no fact pressed) it draws the one line `memory-diagram-select-hint` in its place. No column is drawn beside the drawing in any window width (amended January 2026; user decision; Approved by the user January 2026).
- **SM-SF-23 Errors.** `MEM_THING_NOT_FOUND` on centring (the thing was forgotten) draws its sentence through `ErrorLine` and reads the diagram again without a centre; `MEM_STORE_UNAVAILABLE` and `MEM_STORE_TIMEOUT` draw their sentences and **Try again** (`memory-diagram-retry`); `not_wired` (no `memory.diagram` in the backend) hides **Show as diagram**.
- **SM-SF-24 Bounds shown.** When `left_out` is 0 the left-out line is not drawn; `DiagramView.joins` is at most 200 and `values` at most 40, so the drawing holds at most 240 edges (200 joins and 40 value edges) and 80 nodes (40 things and 40 values).
- **SM-SF-28 The selected fact stands out in the diagram** *(amended February 2026, the user's request: "when you select a fact in the diagram of what Ordin remembers, the selected fact changes colour")*. `MemoryDiagram` takes `selected: string | null`, the id of the fact whose panel is open (`c.diagramFact?.id`, SM-SF-22). The edge of that fact is drawn in `--color-accent` with a line width of 3 (the others keep their line colour and width 1); when the fact is a value, its small box has a `--color-accent` border of width 2 and its text in `--color-text`. Pressing another fact moves the mark to it; **Close** on the panel, centring on another thing, or a diagram that no longer holds the fact clears it. The drawing is redrawn with the mark. The canvas stays hidden from assistive technology: the open panel's heading names the selected fact. *Status: Approved by the user, February 2026 ("approve phase 4 as written, build it"). Source: plan 10 Phase 4.*
- **SM-SF-29 Confirm several sources at once** *(amended February 2026, the user's request: "in memory, have a way to select multiple sources to confirm them together")*. Extends SM-SF-5.2. When **Sources to confirm** lists two or more entries, each entry gains a tick box before its **Confirm source**, whose accessible name is `memory-citation-select { statement }` with the fact's statement, and the section gains a line under `memory-citations-intro` with **Select all** (`memory-citations-select-all`; **Select none**, `memory-citations-select-none`, while every entry is ticked) and **Confirm selected** (`memory-citations-confirm-selected { count }`, "Confirm selected ({ $count })"), off while nothing is ticked. Pressing **Confirm selected** turns both off and sends one `confirmCitation(backend, workspace_id, id)` per ticked entry, in the list's order, one after the other (no new command). Each `ok` removes its entry as SM-SF-5.2 says, without its own announcement. The first refusal stops the run: its sentence is drawn on that entry through `ErrorLine` (`MEM_NOT_HELD` reads the section again, as SM-SF-5.2 says), the entries not yet sent stay ticked, and the line's actions turn on again. At the end one announcement, in the polite live region, says `memory-citations-confirmed-count { count }` ("{ $count } sources confirmed. These facts can now be shared with your team.") with the number confirmed, when at least one was; focus moves to the first entry left, or to the section heading when the list is empty. Ticks are kept only while the section is drawn; reading the section again keeps the ticks of the ids it still lists. With one entry, no tick box and no line are drawn. Tab reaches, in reading order, **Select all**, **Confirm selected**, then each entry's tick box and **Confirm source**; Space toggles a tick box. *Status: Approved by the user, February 2026 ("approve phase 5 as written, build it"). Source: plan 10 Phase 5.*

### Strings added (`crates/ordin-localisation/catalogues/en-GB/screens-fixed.ftl`)

| Id | Text |
|---|---|
| `memory-value-yes` | yes |
| `memory-value-no` | no |
| `memory-older-note` | Older note |
| `memory-older-note-help` | Kept before Ordin wrote each fact as a thing, a kind of fact and a value. Ordin still uses it. (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25) |
| `memory-conflicts-with` | Conflicts with: |
| `memory-keep-this` | Keep this one |
| `memory-keep-this-title` | Keep this fact? |
| `memory-keep-this-body` | Ordin will stop using “{ $other }”. You can still see it in this fact's history. |
| `memory-keep-this-verb` | Keep this one |
| `memory-keep-this-done` | Kept. Ordin no longer uses the other fact. |
| `memory-correct-value-label` | New value for “{ $word }” |
| `memory-correct-date-hint` | A year, a month (2026-04) or a day (2026-04-15) |
| `memory-tab-words` | Kinds of fact (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25) |
| `memory-words-intro` | Each one is a kind of fact Ordin can keep, for example price, supplies or works for, as in “Kestrel · supplies · packaging”. Ordin and your agents add a new kind when none fits. (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25) |
| `memory-word-object-thing` | links to a thing |
| `memory-word-object-text` | links to text |
| `memory-word-object-number` | links to a number |
| `memory-word-object-money` | links to an amount of money |
| `memory-word-object-date` | links to a date |
| `memory-word-object-yes-no` | links to yes or no |
| `memory-word-one` | one at a time |
| `memory-word-several` | can have several |
| `memory-word-by-seed` | Built in |
| `memory-word-by-ordin` | Added by Ordin |
| `memory-word-by-agent` | Added by { $agent } |
| `memory-word-by-an-agent` | Added by an agent |
| `memory-word-by-hosted-agent` | Added by your organisation's agent |
| `memory-word-by-team` | From your team |
| `memory-word-by-you` | Added by you |
| `memory-word-count` | { $count -> [one] Used by 1 fact *[other] Used by { $count } facts } |
| `memory-word-rename` | Rename |
| `memory-word-renamed` | Renamed. Facts of this kind read the new name. (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25) |
| `memory-word-merge` | Merge into… |
| `memory-word-merge-title` | Merge into another kind of fact (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25) |
| `memory-word-merge-body` | { $count -> [one] The 1 fact of this kind will become the kind you choose. *[other] The { $count } facts of this kind will become the kind you choose. } (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25) |
| `memory-word-merge-verb` | Merge |
| `memory-word-not-a-kind` | Not a kind of fact. Use Merge into… to move these facts to a better kind. (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-26) |
| `memory-word-merged` | { $count -> [one] Merged. 1 fact is now of the other kind. *[other] Merged. { $count } facts are now of the other kind. } (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-25) |
| `memory-read-title` | What Ordin read |
| `memory-read-version` | From version { $version } |
| `memory-read-empty` | Ordin has not read this page yet. |
| `memory-summaries-title` | What Ordin summed up |
| `memory-diagram-show` | Show as diagram |
| `memory-diagram-title` | Diagram of what Ordin remembers |
| `memory-diagram-intro` | Each box is something Ordin knows about. Each arrow is a fact that joins two of them. Press an arrow to see where Ordin learnt it. |
| `memory-diagram-left-out` | Showing { $shown } of { $total } things — search to centre on one |
| `memory-diagram-conflicts` | { $word } (Conflicts) |
| `memory-diagram-facts-title` | Facts about { $thing } |
| `memory-diagram-centred` | Centred on { $thing } |
| `memory-diagram-things-title` | Things in the diagram |
| `memory-diagram-open-thing` | Open { $thing } |
| `memory-diagram-back` | Back to the list |
| `memory-diagram-fact-title` | The fact you pressed |
| `memory-diagram-fact-close` | Close |
| `memory-diagram-retry` | Try again |

`memory-term-<key>` and `memory-term-<key>-meaning` for the 26 seed words take, in en-GB, the label and meaning of MemoryService.md's seed table (52 ids, not repeated here).

### Acceptance Criteria (added)

```gherkin
  Scenario: Happy path — a fact reads as its three parts (SM-SF-13)
    Given memory.list returns a fact with triple {subject Kestrel, predicate "account manager", object {kind money, amount "3000000", currency "GBP"}}
    When the Facts tab is drawn in en-GB
    Then the line reads "Kestrel · account manager · £3,000,000.00" and its source line is beneath it

  Scenario: Edge — an older note is marked and cannot be corrected
    Given a fact with older_note true
    Then its line shows "Older note" and offers Forget but not Correct

  Scenario: Happy path — keep one of two conflicting facts (SM-SF-14)
    Given facts A and B with conflicts_with naming each other
    When the person presses Keep this one on A and confirms
    Then memory.keep_conflict_side { keep: A, other: B } is sent and the list is read again

  Scenario: Happy path — rename and merge a word (SM-SF-17)
    Given the Kinds of fact tab lists "provides" (3 facts) and "supplies", both linking to a thing
    When the person merges "provides" into "supplies"
    Then memory.merge_term is sent and "Merged. 3 facts now use the other word." is announced
    And "price" is not offered as a target, its object being money

  Scenario: Edge — page sections are under What Ordin read (SM-SF-18)
    When the person opens What Ordin read on page P
    Then memory.passages for P is sent and its sections are listed, and none appears in the Facts tab

  Scenario: Happy path — the diagram around a thing (SM-SF-20 to SM-SF-22)
    Given a thing page for Kestrel
    When the person presses Show as diagram
    Then memory.diagram { centre: Kestrel } is sent, the intro line is shown, and the list beside the diagram is "Facts about Kestrel"
    When the person presses the node Tom Reid
    Then memory.diagram { centre: Tom Reid } is sent and "Centred on Tom Reid" is announced

  Scenario: Edge — conflicts are dashed and labelled (SM-SF-21)
    Given a DiagramView with two joins whose conflict is true
    Then both edges are dashed and their labels end with "(Conflicts)"

  Scenario: Edge — the left-out line (SM-SF-20)
    Given a DiagramView with 40 things, total_things 212
    Then "Showing 40 of 212 things — search to centre on one" is shown

  Scenario: Edge — keyboard users centre through the list (SM-SF-22)
    Given the diagram without a centre
    When the person tabs to "Northwind" in Things in the diagram and presses Enter
    Then memory.diagram { centre: Northwind } is sent

  Scenario: Edge — a thing two steps away is reachable from the keyboard (SM-SF-22, ADR-120 item 6)
    Given the diagram centred on Kestrel, with Tom Reid at step 1 and Northwind at step 2
    Then Things in the diagram lists Kestrel, Tom Reid and Northwind as buttons
    When the person tabs to "Northwind" and presses Enter
    Then memory.diagram { centre: Northwind } is sent

  Scenario: Edge — a thing in a fact line beside the diagram centres it (SM-SF-22)
    Given the diagram centred on Kestrel and the fact line "Kestrel · account manager · Tom Reid" in Facts about Kestrel
    When the person presses "Tom Reid" in that line
    Then memory.diagram { centre: Tom Reid } is sent and the thing's page is not opened

  Scenario: Edge — a conflict line is marked with an icon from the sprite (SM-SF-14)
    Given a fact with conflicts_with non-empty
    Then its "Conflicts with:" line carries the icon info and the words, not only a colour

  Scenario: Error — the centred thing was forgotten (SM-SF-23)
    Given memory.diagram answers MEM_THING_NOT_FOUND
    Then "Ordin no longer remembers that." is shown and the diagram is read again without a centre
```

### Gaps

None.

## Amendment January 2026 — A team workspace (SM-TW-1 to SM-TW-12)

> **Status:** Approved by the user January 2026. *Amended January 2026 (user decision):* SM-TW-5's **Change…** reads `team_knowledge.results_choices`; one scenario added. Source: decision 103; HLD revision 0.68.0, ADR-121 items 2, 3, 5, 6 and 8 and its consequence ("the Screens module gains the two cards, **Team activity**, the access card and **Leave the team workspace**"); 11 §5.31a (no Git words; one sentence per refusal; what is shared is said before it is shared). Backend: `components/TeamKnowledgeService.md` amendment January 2026 (TW-A to TW-J) through ServiceApi.md SVC-TW-1 to SVC-TW-4; wire shapes `phase-2c-shared-types-wire.md` §18. The prototype has no team-workspace scene; the cards reuse the look of its team-knowledge panel and dialogs (`docs/design/ordin-desktop/user-interface.html`, §5.31a scenes `tkOn`, `tkShare`, `tkPanel`). Every rule below adds to the Team knowledge amendments above; a workspace that is not a team workspace looks exactly as it does there.
>
> Evidence read: the Team knowledge amendment (ADR-103) steps 1–6 and its message rows `team-*` (`team-share-dialog-pick`, `team-share-dialog-address` and its address-splitting rule, `team-share-dialog-history`, `team-header`, `team-stop`); SM-CU-10 (a `last_error` code drawn by id); SM-UP-1 (a control is drawn only when the backend answers its commands); `apps/desktop/ui/src/lib/shell/WorkspaceTree.svelte` (the `GROUPS` rows under a workspace, each filtered by the commands it needs; the row's ⋯ menu with `tree-add`, `ws-rename`, `ws-archive`, `ws-remove`; the waiting count drawn as `.count`); `lib/shell/SideRail.svelte` (**New workspace**, `rail-new-workspace`, on the local backend when it answers `workspaces.create`); `lib/shell/screens.ts` (`SCREEN_IDS`); `lib/icons/sprite.ts` (`ICON_NAMES`: `history`, `publish`, `open`, `stop` among them); `crates/ordin-localisation/catalogues/en-GB/screens.ftl` and `screens-fixed.ftl` (the `tree-*`, `rail-*` and `team-*` ids).

**Three rules**, as for team knowledge: **no Git word** — no `team-ws-*` text, button or card says branch, commit, push, pull, merge, rebase, conflict or clone (the existing no-Git-words test covers every `team-ws-*` id); **the sentence, never the code** — every `TEAM_*` and `WS_*` refusal is drawn as its owner's sentence through `ErrorLine`; **what is shared is said before it is shared** — the share dialog lists the places, what stays on this computer, the agents and their access, the knowledge and the results place before **Share** is pressed. A colleague is named by the login the activity and grants carry, as the host shows it. **A name the repository's history does not record** (`by_confirmed`, `replaced_by_confirmed` or `added_by_confirmed` false; TeamKnowledgeService.md TW-J) is never drawn as fact: `team-ws-someone` stands in its place and `team-ws-says-it-was` with the login follows the line (ADR-121 item 10).

- **SM-TW-1 Share as a team workspace…** (`team-ws-share`, icon `publish`). *amended January 2026 (user decision; Approved by the user January 2026): this menu entry is removed from the workspace's ⋯ menu, the panel and the tree row's ⋯ menu, and `team-ws-share` with it; the **Share** link on the tree row (SM-TW-13) is the one way in, under the same condition.* *amended January 2026 (user decision; Approved by the user January 2026), again: the one way in is now the tree row's ⋯ menu entry **Share with your team…** (`tree-share`, icon `publish`), SM-TW-13; the row carries no Share link.* Formerly in the workspace's ⋯ menu beside **Share knowledge with a team…** and in the tree row's ⋯ menu after `tree-add`, shown for a workspace of the local backend when it answers `team_knowledge.share_workspace` and `team_knowledge.status` has no `team_workspace`. It opens the dialog of SM-TW-2.
- **SM-TW-2 The share dialog** (title `team-ws-share-title` with the workspace's name). Step one is the folder: when the workspace already shares knowledge, its folder is shown as `team-header` and cannot be changed; otherwise the two folder choices of the Team knowledge amendment's step 2 (*amended January 2026 (user decision; Approved by the user January 2026)*: kept for this dialog though that dialog is no longer offered) (`team-share-dialog-pick`, `team-share-dialog-address`, the same address-splitting rule and `team-share-dialog-address-invalid`). Then **Where results are saved** (`team-ws-results-label`): radio buttons, the team's folder first (`team-ws-results-team-folder`), then each `results_choices` entry by its name. Choosing a folder sends `team_knowledge.workspace_share_preview { ws, choice }` and draws, in this order: `team-ws-preview-places` with the places' names (none → `team-ws-preview-no-places`); `team-ws-preview-stays` with the names that stay (only when any); `team-ws-preview-agents`, then one line per agent, `team-ws-agent-access` with its access (SM-TW-6's access phrase); `team-ws-preview-knowledge` with the counts; `team-ws-preview-results` with the chosen results place; then `team-ws-share-intro` and `team-share-dialog-history`. **Share** (`team-ws-share-verb`) sends `team_knowledge.share_workspace { ws, choice, results }`; the dialog closes on the reply and the panel shows `team-busy-sharing`; the card arrives in *Needs you* as any approval card. A refusal stays in the dialog through `ErrorLine` (`TEAM_WS_ALREADY_TEAM`, `TEAM_WS_FOLDER_TAKEN`, `TEAM_WS_RESULTS_NOT_SUITABLE`, `TEAM_READ_ONLY`, `TEAM_ALREADY_ON`, `TEAM_LINK_NOT_SUITABLE`, `TEAM_NO_ACCESS`, `TEAM_SIGN_IN_NEEDED` with **Sign in again**, `TEAM_REMOTE_UNREACHABLE` with **Try again**). **Cancel** first in focus; Escape is **Cancel**.
- **SM-TW-3 Open a team workspace…** (`rail-open-team-workspace`, icon `open`). In the side rail under **New workspace**, on the local backend, when it answers `team_knowledge.workspace_open_preview`. It opens a dialog (title `team-ws-open-title`) with one field, `team-ws-open-address` (the folder's address as GitHub or GitLab shows it, split by the rule of SM-TW-2; any other text → `team-share-dialog-address-invalid`), and **Continue** (`team-ws-open-continue`), which sends `team_knowledge.workspace_open_preview { url, branch, path }`. The preview card then shows: the name in an editable field (`team-ws-open-name`, 1–80 characters, set from `name`; `name_in_use` → `team-ws-open-name-in-use` under it and **Open** off until the name changes); `team-ws-preview-places` with each place and, after it, its reach (`will_link` → nothing; `no_access` → `team-ws-reach-no-access`; `sign_in_needed` → `team-ws-reach-sign-in`; `not_set_up` → `team-ws-reach-not-set-up`; `cannot_tell` → `team-ws-reach-cannot-tell`); `team-ws-preview-agents` with each agent and its access (SM-TW-6), and after it `team-ws-agent-already-here` or `team-ws-agent-not-found` when so; `team-ws-preview-knowledge`; `team-ws-preview-results`; `team-ws-open-intro`. **Open** (`team-ws-open-verb`) sends `team_knowledge.open_workspace { url, branch, path, name }`; on the reply the dialog closes, the new workspace appears in the tree and opens in the centre with focus, and its panel shows `team-ws-busy-opening` until `workspace_changed` brings the status without `busy`. Refusals stay in the dialog through `ErrorLine` (`TEAM_WS_NOT_A_TEAM_WORKSPACE`, `TEAM_WS_FORMAT_NEWER`, `TEAM_WS_DESCRIPTION_UNREADABLE`, `TEAM_WS_ALREADY_OPEN`, `TEAM_WS_NAME_IN_USE`, `TEAM_FORMAT_NEWER`, `TEAM_LINK_NOT_SUITABLE`, `TEAM_NO_ACCESS`, `TEAM_SIGN_IN_NEEDED`, `TEAM_REMOTE_UNREACHABLE`, `TEAM_WS_OPEN_FAILED`). Opening from a link is not offered in this amendment: the desktop registers no link handler for it.
- **SM-TW-4 The header.** A team workspace's header shows `team-ws-header` with `header_line` in place of `team-header`; while `team_workspace.state` is `proposed` it also shows `team-ws-proposed`.
- **SM-TW-5 The team panel** (additions under the header, one line each, shown only when it applies): `places` with state `no_access` → `team-ws-place-no-access` per place; `removal_waits` → `team-ws-place-removal-waits`; `waiting` → `team-ws-place-waiting`; `agents_not_here` → `team-ws-agent-not-here` per agent; always `team-ws-results-in` with `results`, followed by **Change…** (`team-ws-change-results`), which opens the results radio buttons of SM-TW-2 alone and sends `team_knowledge.set_results_place`. *Amended January 2026 (user decision):* **Change…** does not send the share preview, which refuses a team workspace (`TEAM_WS_ALREADY_TEAM`); it sends `team_knowledge.results_choices { ws }` and draws one radio button per entry of `choices` (the team folder as `team-ws-results-team-folder`, a link by its `name`), the entry equal to `current` checked (`current` absent → the first); heading `team-ws-results-label`; **Save** (`engineering-save`) sends `team_knowledge.set_results_place { ws, results }` with the checked entry and closes on the reply; a refusal of either command stays in the dialog through `ErrorLine` (`TEAM_WS_NOT_TEAM`, `TEAM_WS_RESULTS_NOT_SUITABLE`, `TEAM_WORKSPACE_NOT_FOUND` with **Try again**); **Cancel** first in focus; Escape is **Cancel**. No string is added; `results_here_only` → `team-ws-results-here-only`; `unsent_results > 0` → `team-ws-unsent-results`; `pending_grants > 0` → `team-ws-grants-waiting` with **Review** (`team-ws-grants-review`), which opens Team activity (SM-TW-8). **Share Ordin's memory and knowledge pages with your team** (`team-share`) and **Get your team's memory and knowledge pages** (`team-get`) are unchanged but for their labels (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-1); their outcome now also carries the workspace's changes, results and activity.
- **SM-TW-6 The access card** (ADR-121 item 5). One card per `team_knowledge.pending_grants { ws }` entry, drawn at the top of Team activity and in the panel under `team-ws-grants-waiting`, with the look of an approval card: `team-ws-grant-new-agent` (`new_agent` true) or `team-ws-grant-more-access` (false), with `person` = `added_by` (or `team-ws-someone`, with `team-ws-says-it-was` after the line, when `added_by_confirmed` is false), `agent` = `agent_name` and `access` the **access phrase**: the entries grouped by access, `team-ws-access-change` with the place names of `read_and_change` entries and `team-ws-access-read` with those of `read` entries, joined by `team-ws-access-and`, place names joined by the locale's list format ("Priya added Supplier analyst, which can read and change Contracts — Allow?"). `left_out` non-empty → `team-ws-grant-left-out` under it. Buttons **Don't allow** (`team-ws-grant-decline`, first in focus, never the default) and **Allow** (`team-ws-grant-allow`), which send `team_knowledge.decline_grant` / `team_knowledge.allow_grant { ws, grant_id }`; the card closes on the reply and `team-ws-grant-allowed` or `team-ws-grant-declined` is announced in a polite live region. `TEAM_WS_AGENT_NOT_ADDED`, `TEAM_WS_ALLOW_FAILED` (with **Try again**) and `TEAM_WS_GRANT_NOT_FOUND` are drawn in the card through `ErrorLine`; after `TEAM_WS_GRANT_NOT_FOUND` the list is read again. No other screen, block or conversation allows a grant.
- **SM-TW-7 Team activity in the tree** (consistent with `WorkspaceTree.svelte`, where every group under a workspace is a row that opens a screen). A row **Team activity** (`tree-team-activity`, icon `history`) after **Running now** and before **Knowledge**, drawn only when the backend answers `team_knowledge.activity` and the workspace's `team_knowledge.status` has `team_workspace` (read when the workspace node opens and on `workspace_changed`). When `pending_grants > 0` the row carries the count, drawn as the workspace node's waiting count is, with the accessible name `team-ws-grants-waiting`. Pressing it opens the screen of SM-TW-8.
- **SM-TW-8 The Team activity screen.** Registered as `team-activity` (`SCREEN_IDS.teamActivity`, title id `screen-team-activity`, commands `team_knowledge.activity` and `team_knowledge.pending_grants`). Top to bottom: the access cards (SM-TW-6); then the entries of `team_knowledge.activity { ws }`, newest first, each one line with its time in the person's locale:
  - `run` → `team-ws-activity-run` (`person` = `by`, or `team-ws-activity-you` when `mine` and `by` is absent, or `team-ws-someone` with `team-ws-says-it-was` at the end of the line when `by_confirmed` is false; `task`, or `team-ws-activity-untitled` when `task` is null; `agent`), then the outcome as a tag (`team-ws-outcome-completed`, `team-ws-outcome-failed`, `team-ws-outcome-stopped`, told by text, never colour alone), then each result's `name` as a link that opens its `file` as any document of the workspace opens (the `document` screen); a result with `file` absent is plain text with `team-ws-result-not-here` as its tooltip and accessible description;
  - `replaced` → `team-ws-activity-replaced` with `person` = `by`, `other` = `replaced_by` and `subject`; an unconfirmed `by` or `replaced_by` is drawn as `team-ws-someone`, each followed by its own `team-ws-says-it-was`;
  Activity lists task runs and replaced changes only (ADR-121 items 5 and 6); a setting someone changed is not an entry.
  - an entry of this computer not yet shared (`mine`, `by` absent) carries `team-ws-activity-not-shared`.
  At most 50 entries are drawn; **Show earlier** (`team-ws-activity-more`) sends the next page with `cursor`. None → `team-ws-activity-empty`. The screen reads both commands again on `workspace_changed` for the workspace. A workspace that stops being a team workspace closes the screen.
- **SM-TW-9 No conversation text in activity.** The screen draws only the fields of `TeamActivityView`; it never fetches or shows a run's conversation, inputs or approvals from an activity entry.
- **SM-TW-10 Leave the team workspace** (`team-ws-leave`, icon `stop`). In a team workspace the ⋯ menu shows it in place of **Stop sharing with the team**. It asks with `ConfirmDialog` (`team-ws-leave-title` with the workspace's name, `team-ws-leave-body`, verb `team-ws-leave-verb`, **Cancel** first in focus); confirming sends `team_knowledge.leave_workspace { ws }` and announces `team-ws-left`. `TEAM_SHARE_PENDING` is drawn through `ErrorLine`.
- **SM-TW-11 The status's last error** (adds to SM-CU-10). `last_error` `TEAM_WS_FOLDER_TAKEN` → `team-ws-error-folder-taken`; `TEAM_WS_RESULTS_NO_ACCESS` and `TEAM_WS_RESULT_TOO_LARGE` arrive as `problems` and are drawn in the **Not taken in** list as `team-ws-problem-results-no-access` and `team-ws-problem-result-too-large` (with `file`); `TEAM_WS_RESULT_KEPT` → `team-ws-problem-result-kept` (with the new name); `TEAM_WS_RESULT_NOT_TAKEN` → `team-ws-problem-result-not-taken`; `TEAM_WS_DESCRIPTION_UNREADABLE` and `TEAM_WS_FORMAT_NEWER` as problems → `team-ws-problem-description` and `team-ws-problem-format-newer`; `TEAM_WS_NAME_IN_USE` as a problem → `team-ws-problem-name-in-use`. Any other code keeps SM-CU-10's rule.
- **SM-TW-12 Not on a connected server** (ADR-121 item 9: "Team workspaces are not offered through a personal server's Remote API"). The ten commands are local only (ServiceApi.md SVC-TW-1), so for a workspace on a connected server none of SM-TW-1 to SM-TW-11 is drawn: no **Share as a team workspace…** and no **Share** link or **Shared** mark on the tree row (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TW-13; *amended January 2026 (user decision; Approved by the user January 2026): no sharing entry in the row's ⋯ menu and no team icon*), no **Team activity** row, no access card; **Open a team workspace…** is offered for the local backend only. Its team knowledge panel stays as the Team knowledge amendment draws it.
- **SM-TW-13 Sharing from the workspace's ⋯ menu** (*amended January 2026 (user decision; Approved by the user January 2026); replaces the Share link on the tree row, below*). Actions on a workspace live in its ⋯ menu (also opened by a right-click on the row), never as controls on the row (11-ux-guidelines.md §4, "Actions in menus, content in panes"). For a workspace of the local backend that answers `team_knowledge.status` and `team_knowledge.share_workspace` (SM-UP-1, SM-TW-12), the row reads `team_knowledge.status` when it is drawn and again on every `workspace_changed` for the workspace, and once the status is read the row's ⋯ menu holds, before **Rename…**: while the status has no `team_workspace`, **Share with your team…** (`tree-share`, icon `publish`), which opens the share dialog of SM-TW-2 (for a workspace that shares its knowledge already, with the folder fixed to its own, SM-TW-14); with a `team_workspace`, **Shared with your team** (`tree-shared`, icon `crew`, `aria-expanded`), which shows under the row `team-ws-header` and `team-ws-shared-with` with `header_line` (Escape closes it), and **Leave the team workspace** (`team-ws-leave`, icon `remove`), which asks as SM-TW-10 does. A team workspace's row draws a small team icon (`crew`) after the waiting count; it is not a control, and its accessible name is `tree-shared` ("Shared with your team"). The row has no Share link and no Shared mark; `tree-share-name` and `tree-shared-name` are removed. **Open a team workspace…** (SM-TW-3) is unchanged.
- Formerly, **SM-TW-13 The Share link on the tree row** (*amended January 2026 (user decision; Approved by the user January 2026)*; *replaced as above*). Every workspace row of the side tree (`WorkspaceTree.svelte`, the rail's tree) carries, after the waiting count and before the ⋯ button, a small text link, drawn for a workspace of the local backend when it answers `team_knowledge.status` and `team_knowledge.share_workspace` (SM-UP-1, SM-TW-12); the row reads `team_knowledge.status` when it is drawn and again on every `workspace_changed` for the workspace. While the status has no `team_workspace` the link reads **Share** (`tree-share`), with the accessible name `tree-share-name` ("Share Supplier review with your team"); pressing it opens the share dialog of SM-TW-2, which for a workspace that shares its knowledge already fixes the folder to its own (SM-TW-14). With a `team_workspace` the link is a small **Shared** mark (`tree-shared`, accessible name `tree-shared-name`, `aria-expanded`); pressing it shows, under the row, `team-ws-header` with `header_line`, `team-ws-shared-with` with `header_line` (everyone who can open the team folder), and **Leave the team workspace** (`team-ws-leave`), which asks as SM-TW-10 does; Escape closes it. Both are buttons in the tab order (keyboard-reachable; Enter and Space press them); a press is the link's own and does not also open the workspace, and a key pressed in the link or its dialog is not taken by the row's tree keys. This is the only way into sharing: the ⋯ menus offer no share (SM-TW-1), and **Open a team workspace…** (SM-TW-3) is unchanged.
- **SM-TW-14 Share the whole workspace** (*amended January 2026 (user decision; Approved by the user January 2026)*). A workspace of the local backend that shares its knowledge only (`binding` present, no `team_workspace`; turned on before this amendment) keeps its panel as the Team knowledge amendment draws it — **Get your team's memory and knowledge pages**, **Share Ordin's memory and knowledge pages with your team**, **Stop sharing with the team**, the "Shared by…" marks — and the panel adds one line, `team-ws-share-whole-line`, followed by **Share the whole workspace** (`team-ws-share-whole`, icon `publish`), drawn when the backend answers `team_knowledge.share_workspace`. It opens the share dialog of SM-TW-2 with the workspace's own folder fixed, so the workspace is upgraded in place: `team_knowledge.workspace_share_preview` and `team_knowledge.share_workspace` take a `choice` naming the bound link (TeamKnowledgeService.md TW-A step 2, which accepts an own folder only when `choice` names it, else `TEAM_ALREADY_ON`). The tree row's **Share** (SM-TW-13) opens the same dialog.

### Strings added

In `crates/ordin-localisation/catalogues/en-GB/screens.ftl`:

| Id | Text |
|---|---|
| `rail-open-team-workspace` | Open a team workspace… |
| `tree-team-activity` | Team activity |
| `tree-share` | Share with your team… (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TW-13; formerly "Share") |
| `tree-share-name` | Share { $workspace } with your team (*removed — amended January 2026 (user decision; Approved by the user January 2026)*, SM-TW-13) |
| `tree-shared` | Shared with your team (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TW-13; formerly "Shared") |
| `tree-shared-name` | { $workspace } is shared with your team (*removed — amended January 2026 (user decision; Approved by the user January 2026)*, SM-TW-13) |
| `screen-team-activity` | Team activity |

In `crates/ordin-localisation/catalogues/en-GB/screens-fixed.ftl`:

| Id | Text |
|---|---|
| `team-ws-share` | Share as a team workspace… (*removed — amended January 2026 (user decision; Approved by the user January 2026), SM-TW-1, SM-TW-13*) |
| `team-ws-share-title` | Share { $workspace } as a team workspace |
| `team-ws-results-label` | Where results are saved |
| `team-ws-results-team-folder` | In the team's folder |
| `team-ws-preview-places` | Places everyone will link, each with their own sign-in: { $places } |
| `team-ws-preview-no-places` | No places are linked yet. |
| `team-ws-preview-stays` | Stays on this computer: { $names } |
| `team-ws-preview-agents` | Agents |
| `team-ws-agent-access` | { $agent }, which can { $access } |
| `team-ws-preview-knowledge` | { $facts -> [one] 1 fact *[other] { $facts } facts } and { $pages -> [one] 1 page *[other] { $pages } pages } |
| `team-ws-preview-results` | Results will be saved in { $place } |
| `team-ws-share-intro` | Everyone who can open this folder can open this workspace with the same places and agents, see its results and see what each person did. Your conversations stay yours. |
| `team-ws-share-verb` | Share |
| `team-ws-open-title` | Open a team workspace |
| `team-ws-open-address` | Paste the address of the team's folder from GitHub or GitLab |
| `team-ws-open-continue` | Continue |
| `team-ws-open-name` | Name on this computer |
| `team-ws-open-name-in-use` | Another workspace already has this name. Choose another. |
| `team-ws-reach-no-access` | You don't have access |
| `team-ws-reach-sign-in` | Sign in to link this |
| `team-ws-reach-not-set-up` | Not set up on this computer |
| `team-ws-reach-cannot-tell` | Can't check just now; ordin tries again later |
| `team-ws-agent-already-here` | Already on this computer |
| `team-ws-agent-not-found` | Not in your agent library, so it won't be added |
| `team-ws-open-intro` | Opening links these places with your own sign-ins and gives these agents the access shown. Your conversations stay yours. |
| `team-ws-open-verb` | Open |
| `team-ws-busy-opening` | Opening the team workspace… |
| `team-ws-header` | Team workspace: { $folder } |
| `team-ws-proposed` | Waiting for your approval to share |
| `team-ws-place-no-access` | { $place }: you don't have access |
| `team-ws-place-removal-waits` | { $place } was removed by your team; it stays until your changes in it are sent. |
| `team-ws-place-waiting` | { $place } can't be reached just now; ordin tries again later. |
| `team-ws-agent-not-here` | { $agent } isn't on this computer. |
| `team-ws-results-in` | Results are saved in { $place } |
| `team-ws-change-results` | Change… |
| `team-ws-results-here-only` | Your results stay on this computer: you can read this team workspace but not add to it. |
| `team-ws-unsent-results` | { $count -> [one] 1 result waits to be shared *[other] { $count } results wait to be shared } |
| `team-ws-grants-waiting` | { $count -> [one] 1 change by your team waits for you *[other] { $count } changes by your team wait for you } |
| `team-ws-grants-review` | Review |
| `team-ws-grant-new-agent` | { $person } added { $agent }, which can { $access } — Allow? |
| `team-ws-grant-more-access` | { $person } gave { $agent } more access: it can { $access } — Allow? |
| `team-ws-access-change` | read and change { $places } |
| `team-ws-access-read` | read { $places } |
| `team-ws-access-and` | { $first }, and { $second } |
| `team-ws-grant-left-out` | Left out because you can't open them: { $places } |
| `team-ws-grant-allow` | Allow |
| `team-ws-grant-decline` | Don't allow |
| `team-ws-grant-allowed` | Allowed. { $agent } can use what your team gave it. |
| `team-ws-grant-declined` | Not allowed. Nothing changed on this computer. |
| `team-ws-activity-run` | { $person } ran { $task } with { $agent } |
| `team-ws-activity-you` | You |
| `team-ws-activity-untitled` | a task |
| `team-ws-outcome-completed` | Done |
| `team-ws-outcome-failed` | Didn't finish |
| `team-ws-outcome-stopped` | Stopped |
| `team-ws-result-not-here` | Not on this computer yet. Get your team's memory and knowledge pages to bring it here. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-1) |
| `team-ws-activity-replaced` | { $person }'s change to { $subject } was replaced by { $other }'s later share |
| `team-ws-activity-not-shared` | Not shared yet |
| `team-ws-someone` | Someone |
| `team-ws-says-it-was` | (says it was { $person }) |
| `team-ws-activity-more` | Show earlier |
| `team-ws-activity-empty` | Nothing done here yet. Tasks your team runs in this workspace appear here. |
| `team-ws-leave` | Leave the team workspace |
| `team-ws-leave-title` | Leave { $workspace }? |
| `team-ws-leave-body` | It stays on this computer as your own workspace, with everything in it. You stop sharing with the team and getting what they share. Nothing changes in the team's folder. |
| `team-ws-leave-verb` | Leave |
| `team-ws-left` | You left the team workspace. Everything is still here. |
| `team-ws-error-folder-taken` | This folder already holds a team workspace. Open it with Open a team workspace… instead. |
| `team-ws-problem-results-no-access` | You can't open the place where the team saves results, so your results stay on this computer. |
| `team-ws-problem-result-too-large` | { $file } is too large to share; it stays on this computer. |
| `team-ws-problem-result-kept` | A colleague saved a result with the same name, so both are kept; yours is now { $file }. |
| `team-ws-problem-result-not-taken` | A colleague's { $file } wasn't brought here because yours has the same name. Rename yours, then get your team's memory and knowledge pages again. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-1) |
| `team-ws-problem-description` | This team workspace's settings can't be read, so your team's latest changes to it weren't taken in. |
| `team-ws-problem-format-newer` | This team workspace needs a newer ordin to take in your team's latest changes. |
| `team-ws-problem-name-in-use` | Your team renamed this workspace, but another workspace here already has that name. |
| `team-ws-shared-with` | Shared with everyone who can open { $folder }. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TW-13) |
| `team-ws-share-whole` | Share the whole workspace (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TW-14) |
| `team-ws-share-whole-line` | Only this workspace's knowledge is shared with your team. (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TW-14) |
| `team-share-from-tree` | *Removed — SM-TK-2 (amended January 2026; user decision; Approved by the user January 2026): the panel draws nothing while nothing is shared.* |
| `team-panel-label` | Your team (*amended January 2026 (user decision; Approved by the user January 2026)*, SM-TK-2; the panel section's accessible name, which was `team-share-with-team`) |

The word "repository" appears in none of these strings; the folder is named by `header_line`, as the existing team-knowledge strings name it.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Screens — a team workspace (ADR-121)

  Scenario: Happy path — the share dialog says what is shared before it is shared (SM-TW-2)
    Given workspace "Q3 supplier review" with the SharePoint link "Contracts", the local folder "Notes" and the agent "Supplier analyst"
    When the person presses Share with your team… in the workspace's ⋯ menu in the tree and picks the folder "acme/supplier-team › knowledge" (amended January 2026 (user decision; Approved by the user January 2026), SM-TW-13)
    Then the dialog shows "Places everyone will link, each with their own sign-in: Contracts" and "Stays on this computer: Notes"
    And it shows "Supplier analyst, which can read and change Contracts" and "Results will be saved in acme/supplier-team › knowledge › results"
    When the person presses Share
    Then team_knowledge.share_workspace is sent and the panel shows "Preparing your share…"

  Scenario: Happy path — opening a team workspace (SM-TW-3)
    When the person chooses Open a team workspace…, pastes "https://github.com/acme/supplier-team/tree/main/knowledge" and presses Continue
    Then team_knowledge.workspace_open_preview is sent with url "https://github.com/acme/supplier-team", branch "main", path "knowledge"
    And a place this person cannot open shows "You don't have access"
    When the person presses Open
    Then team_knowledge.open_workspace is sent and the new workspace opens with focus

  Scenario: Security — the access card asks before anything is granted (SM-TW-6)
    Given team_knowledge.pending_grants lists Supplier analyst added by "priya" with read and change on "Contracts"
    When Team activity opens
    Then the card reads "priya added Supplier analyst, which can read and change Contracts — Allow?"
    And "Don't allow" has focus and is not the default button
    When the person presses Allow
    Then team_knowledge.allow_grant is sent with that grant's id

  Scenario: Happy path — Team activity in the tree (SM-TW-7, SM-TW-8)
    Given a team workspace with two activity entries and one waiting grant
    Then the tree shows "Team activity" with the count 1 under the workspace
    When the person opens it
    Then the access card comes first and the entries follow, newest first, each result a link that opens the file

  Scenario: Edge — an entry with no title (SM-TW-8)
    Given an activity entry with task null by "sam"
    Then it reads "sam ran a task with Supplier analyst"

  Scenario: Happy path — leaving (SM-TW-10)
    When the person chooses Leave the team workspace and confirms
    Then team_knowledge.leave_workspace is sent and "You left the team workspace. Everything is still here." is announced

  Scenario: Security — a name the history does not record says so (SM-TW-8)
    Given an activity entry by "priya" with by_confirmed false, task "Compare documents" and agent "Supplier analyst"
    Then it reads "Someone ran Compare documents with Supplier analyst (says it was priya)"

  Scenario: Security — an unconfirmed name on the access card (SM-TW-6)
    Given a waiting grant added by "priya" with added_by_confirmed false for "Contract checker"
    Then the card reads "Someone added Contract checker, which can read and change Contracts — Allow? (says it was priya)"

  Scenario: Edge — a workspace on a connected server (SM-TW-12)
    Given a workspace of a connected personal server
    Then its menu has no "Share with your team…", its row has no team icon and its tree has no "Team activity" row (amended January 2026 (user decision; Approved by the user January 2026), SM-TW-13)

  Scenario: Edge — no Git words (SM-TW rules)
    When every team-ws-* message is rendered in en-GB
    Then none contains "branch", "commit", "push", "pull", "merge", "rebase", "conflict" or "clone"

  # Scenario below: amended January 2026 (user decision; Approved by the user January 2026)

  Scenario: Happy path — Change… after sharing (SM-TW-5)
    Given W is a team workspace whose results are saved in the team's folder, and W links the SharePoint folder "Supplier results"
    When the person presses Change… on the team panel
    Then the screens send "team_knowledge.results_choices" and not "team_knowledge.workspace_share_preview"
    And the dialog shows "In the team's folder", checked, and "Supplier results"
    When the person checks "Supplier results" and presses Save
    Then "team_knowledge.set_results_place" is sent with the link "Supplier results"

  # Scenarios below: amended January 2026 (user decision; Approved by the user January 2026) (SM-TW-13, SM-TW-14)

  # Two scenarios below: amended January 2026 (user decision; Approved by the user January 2026) (SM-TW-13); they replace the Share link and the Shared mark.
  Scenario: Happy path — Share with your team… in an unshared workspace's menu opens the share dialog (SM-TW-13)
    Given workspace "Supplier review" of the local backend with no team_workspace
    Then its tree row has no Share button and no team icon
    When the person opens the row's ⋯ menu, or right-clicks the row
    Then the menu holds "Share with your team…"
    When the person presses it
    Then the dialog "Share Supplier review as a team workspace" opens and the workspace does not open behind it

  Scenario: Happy path — a team workspace's menu says it is shared (SM-TW-13)
    Given workspace "Supplier review" whose team_knowledge.status has team_workspace and header_line "acme/supplier-team › knowledge"
    Then its tree row draws a team icon named "Shared with your team", which is not a button
    When the person opens the row's ⋯ menu
    Then it holds "Shared with your team" and "Leave the team workspace" and no "Share with your team…"
    When the person presses Shared with your team
    Then it shows "Team workspace: acme/supplier-team › knowledge" and "Shared with everyone who can open acme/supplier-team › knowledge."
    When the person presses Leave the team workspace in the menu
    Then the Leave confirmation of SM-TW-10 opens

  Scenario: Happy path — a knowledge-only workspace is offered the whole workspace (SM-TW-14)
    Given workspace "Supplier review" whose team_knowledge.status has a binding to the link L and no team_workspace
    Then its panel shows "Only this workspace's knowledge is shared with your team." with Share the whole workspace
    When the person presses Share the whole workspace
    Then team_knowledge.workspace_share_preview is sent with choice link L
```

### Gaps

None.

## Amendment January 2026 — The screen is called Memory, and its Words tab Kinds of fact (SM-SF-25)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person found "Words" did not say what the tab lists, and "What Ordin remembers" was long for a screen's name. Only person-facing text changes: message ids, screen ids, Settings section ids, Service API commands and code identifiers stay as they are.

- **SM-SF-25 Memory and Kinds of fact** *(amended January 2026 (user decision; Approved by the user January 2026))*. (1) Wherever a rule above names the screen *What Ordin remembers* — the tree entry, the screen's heading and its tab list's name, the Settings section, the block that shows memory, and any button, link, help text or empty-state line that opens or names it — the person reads **Memory**: `tree-memory` and `settings-section-what-ordin-remembers` read "Memory", and a sentence that names the screen is rewritten to read naturally ("Open Memory"). A sentence about what Ordin remembers in the ordinary sense ("Open a workspace to see what Ordin remembers in it.", `memory-diagram-title` "Diagram of what Ordin remembers") is unchanged. (2) SM-SF-17's tab reads **Kinds of fact**; its intro line says that each one is a kind of fact Ordin can keep, for example price, supplies or works for; and its rename and merge texts speak of a kind of fact, not a word (the string table above). The en-GB catalogue holds these texts; the other locales' catalogues do not hold these ids and fall back to en-GB as before.

```gherkin
  Scenario: Edge — the screen reads Memory and its tab Kinds of fact (SM-SF-25)
    When the person opens Memory for a workspace
    Then the heading reads "Memory", the tabs include "Kinds of fact" and no text reads "What Ordin remembers"
```

### Gaps

None.

## Amendment January 2026 — Save logs for support… works at once, and Clear logs… (amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person found **Save logs for support…** did nothing. The log on their computer showed `runs.list` answered four times, one for each press, and `diagnostics.support_export` never sent: the panel opened with every tick off, so **Save…** stayed disabled. They also found nothing clears the logs. Counterparts: OrdinDesktop.md DSK-LOG-1 to DSK-LOG-4, ServiceApi.md SVC-LOG-1 and SVC-R1-10, ErrorHandling.md `DSK_LOGS_CLEAR_FAILED`. SM-NS-3 is amended in place (the newest runs start ticked); every other section above stays in force.

- **SM-NS-3 (amended in place).** When the panel opens, the newest runs of the first page, up to 20, are ticked; unticking still works and the 21st tick is still refused. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-FS-6 (extended).** The window never times out `files.save_copy` and `diagnostics.support_export` either: they wait for the save dialog (ServiceApi.md SVC-R1-10). amended January 2026 (user decision; Approved by the user January 2026).
- **SM-NS-3a Clear logs….** Beside **Save logs for support…**, on the same terms (local backend only, drawn only when the build answers the command's group `diagnostics`), a button **Clear logs…** (`diagnostics-clear`). It opens the confirmation pattern (`ConfirmDialog`): title `diagnostics-clear-title` ("Clear ordin's logs?"), line `diagnostics-clear-body` ("This deletes ordin's log files and empties the list of events below. A file saved for support still includes earlier events."; amended January 2026; user decision; Approved by the user January 2026), the verb **Clear logs** (`diagnostics-clear-verb`, a removing verb, so not the default button) and **Cancel**. **Cancel** or Escape closes it and sends nothing. **Clear logs** sends `diagnostics.clear_logs {}`; `LogsCleared` closes the question and draws `diagnostics-clear-done` ("Logs cleared.") as a status line under the two buttons; *amended January 2026 (user decision; Approved by the user January 2026):* `diagnostics.clear_logs` also empties the Diagnostics event list in the backend from that moment (JournalAndArtefactStore.md JA-DIAG-CLEAR-1), so after `LogsCleared` the screen empties the journal table and reads `diagnostics.lines` again, which lists only events recorded since; the list stays empty after the screen or the window is reopened. This window's own lines (the ring of step 12) recorded before the clear are no longer drawn while the window stays open, since they are not kept anywhere else; any refusal stays inside the open question as `ErrorLine` (its sentence, **Try again** when retryable, never its code). amended January 2026 (user decision; Approved by the user January 2026).

```gherkin
Feature: Clear logs… (SM-NS-3a)

  Scenario: Happy path — clearing the logs
    Given Settings › Diagnostics on this computer
    When the person presses Clear logs… and then Clear logs
    Then diagnostics.clear_logs is sent with {}
    And the question closes and "Logs cleared." is shown

  Scenario: Edge case — cancelled
    When the person presses Clear logs… and then Cancel
    Then nothing is sent

  Scenario: Error path — a log file cannot be deleted
    Given diagnostics.clear_logs answers DSK_LOGS_CLEAR_FAILED
    When the person presses Clear logs
    Then the question stays open with the sentence and Try again
```

### Gaps

None.

## Amendment January 2026 — The support note names the logs, and a kind of fact named by a kind of note (SM-NS-3, SM-SF-26; amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person chose that the support file holds the last 3 days of ordin's logs, and that a word naming a kind of note is refused when used, not only when proposed. Counterparts: OrdinDesktop.md DSK-LOG-5, cross-cutting/Logging.md E-1, MemoryService.md MEM-TR-13 (d). SM-NS-3's note and SM-SF-17 are amended in place; every other section above stays in force.

- **SM-NS-3 (amended in place).** `diagnostics-support-note` reads "The file holds the chosen runs' records and the last 3 days of ordin's logs. It holds no document or conversation text." (amended January 2026 (user decision; Approved by the user January 2026)) It no longer says the file holds no tokens, message text or file content: the logs are now part of the file, and no evidence shows a log line never holds document or conversation text. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SF-26 A kind of fact named by a kind of note.** On the Kinds of fact tab (SM-SF-17), a word whose stored `label`, read as MemoryService.md MEM-TR-13 (c) reads a proposed label (`_` and `-` as spaces, lower case, spaces collapsed, ASCII punctuation trimmed at both ends, a final `s` or `ies` for `y` read as the singular), is `key fact`, `known issue`, `note`, `issue`, `fact`, `summary`, `detail`, `point` or `info`, and for which **Merge into…** is offered (another word of the same object kind exists), shows one line `memory-word-not-a-kind` ("Not a kind of fact. Use Merge into… to move these facts to a better kind.") under its label and meaning. The line is text only; the word's ⋯ menu is unchanged. A word with nothing to merge into shows no line, so the line never names an action the menu does not hold. amended January 2026 (user decision; Approved by the user January 2026).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: The support note and a kind of fact named by a kind of note (SM-NS-3, SM-SF-26)

  Scenario: Happy path — the support note names the logs (SM-NS-3)
    When the person opens Save logs for support…
    Then the panel reads "The file holds the chosen runs' records and the last 3 days of ordin's logs."

  Scenario: Happy path — a word named "Key facts" shows the line (SM-SF-26)
    Given the Kinds of fact tab lists "Key facts" and "target year", both linking to text
    Then "Key facts" shows "Not a kind of fact. Use Merge into… to move these facts to a better kind."
    And "target year" shows no such line

  Scenario: Edge case — a word with nothing to merge into shows no line (SM-SF-26)
    Given the Kinds of fact tab lists "note" and no other word that links to text
    Then "note" shows no such line

  Scenario: Happy path — Open BotFather in Telegram (SM-SW-5; amended January 2026 (user decision; Approved by the user January 2026))
    Given Settings › Telegram shows Set up Telegram
    When the person presses Open BotFather in Telegram
    Then app.open_web_page is sent to the local backend with {"page": "telegram_botfather"}

  Scenario: Error path — the browser does not open (SM-SW-11; amended January 2026 (user decision; Approved by the user January 2026))
    Given app.open_web_page answers CRED_BROWSER_FAILED
    When the person presses Open Atlassian's token page
    Then its sentence is drawn on the token page's line with Try again, and Copy the address is still there

  Scenario: Happy path — Connect again… fills in the address (SM-SW-12; amended January 2026 (user decision; Approved by the user January 2026))
    Given servers.list answers a server "Team server" with address "10.20.4.17:8443"
    When the person presses Connect again… on its row
    Then the address box holds "10.20.4.17:8443" and the code box is empty
```

## Amendment January 2026 — The Settings walk-through (SM-SW-1 to SM-SW-17; amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: a usability walk-through of every Settings section, read from the code, and the person's decisions on it: one section at a time, with a side list of sections on a window at least 700 px wide and a **Section** menu below that; **Sign out of email** and **Remove Telegram** act at once and say what they did, **Remove this server…** asks first; new words come from the prototype (`docs/design/ordin-desktop/user-interface.html`) and `docs/design/ordin-desktop/11-ux-guidelines.md`, and plain en-GB in their style where neither has them; Sign-ins always lists the services ordin can use; **Open in [IDE]** is built. The rules named below are amended in place by the bullets of this section; every other rule above stays in force. Counterparts: none outside this module — every command used is already in ServiceApi.md, and no error code is added.

- **SM-SW-1 One section at a time, beside its list (amends step 9's Settings row and SM-NS-1).** The Settings screen draws its heading, then, on a window at least 700 px wide, the present sections (SM-CU-3) as a column of buttons at the start with the open section beside them; below 700 px the column is not drawn and a **Section** menu (`settings-section-menu`, a `select` of the same sections in the same order) sits above the open section. The open section's button is drawn as a pressed toggle. *Amended January 2026 (user decision; Approved by the user January 2026):* the 700 px are the Settings screen's own width, not the window's: with the side panel and the conversation open, a wide window can leave the screen area too narrow for the column and a section side by side, which squeezed **Using your plan** into a strip where its lines and controls ran into each other. In **Using your plan** a row whose words and control do not fit side by side draws its control under its words, a choice box is never narrower than its chosen words allow while there is room and never wider than the row, and a long word breaks rather than running out of its card. Settings › Memory draws one heading only, the section's, inside its card: the embedded memory view is given `embedded` and hides its own heading. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-2 Shared section styles (amends SM-NS-1).** No section is left with the browser's own styles. The classes several sections draw — `.tag`, `.secondary`, `.hint`, `.invalid`, `.side-panel`, `.setting-row`, `.switch-line`, `.choice-group`, `.field-stack` and `.settings-section h4` — are styled once in `apps/desktop/ui/src/styles/tokens.css`, never in one component (Svelte scopes a component's styles to that component). A list of rows uses `.list-rows` / `.list-row`; a group of radio choices is a `fieldset.choice-group` with no box; a form has small labels above full-width fields. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-3 Sign-ins lists every service (amends SM-NS-2.1 to SM-NS-2.3; adds SM-NS-2.5).** Under `signin-intro`, Sign-ins lists, in this order, GitHub, GitLab, Microsoft 365, Google, Confluence, Slack, the company's mail server, and the company account (Jira is in Settings › Jira, SM-NS-2.10), each as `signin-service` (its `signin-kind-*` name and its `signin-for-*` line). A service with saved accounts has one row per `CredentialView` of its kinds; a service with none has one row of its own reading `signin-state-signed-out`. A service the organisation profile does not use (`profile.get_document`: `microsoft`, `google` or `confluence` absent; `slack.tools_enabled = false`; `email` absent, not enabled or not `company_server`) and that has no saved account is greyed out with `signin-not-available` and has no control (SM-NS-2.9 (amended January 2026; user decision; Approved by the user January 2026)); the company account is listed only when the profile names `identity_provider` or an account of kind `oidc` is saved. With no profile read, every service but the company account counts as used. A row's small line is the account, then `signin-signed-in-until` with `expires_at` (or `signin-state-signed-in`), `signin-state-needs-sign-in` as a waiting state label with a mark, or `signin-state-signed-out`.
    1. **SM-NS-2.5 Sign in with nothing saved.** **Sign in** on a service row with no account sends `identity.sign_in { kind }` for GitHub, GitLab, Microsoft 365, Google, Confluence, Slack and the company account, exactly as for a saved account (IdentityAndCredentials.md accepts a kind with nothing saved); Jira has no row here and is connected in Settings › Jira (SM-NS-2.10) (amended January 2026; user decision; Approved by the user January 2026); the company's mail server opens the mail dialog of SM-NS-2.3 for the profile's first `email.sign_in_methods` entry (a password when none is named). For `single_sign_on` the dialog asks for the address only, under `signin-mail-sso-note`, and **Sign in** sends `identity.sign_in { kind: mail_server_single_sign_on, mail_account }`; a saved single sign-on account sends its `account_label` as `mail_account`.
    2. The mail dialog of SM-NS-2.3 is a modal dialog (`.dialog`, `aria-modal`): focus starts in the address, Tab stays inside it, Escape closes it and focus returns to its opener; **Show** reads **Hide** (`signin-hide`) while the password is shown; an empty password is not sent and `signin-mail-password-empty` is drawn under the field.
    3. The GitLab token box has its own **Save token** (`signin-gitlab-save`) and the hint `signin-gitlab-token-hint`; an empty box is not sent and the hint reads `signin-gitlab-token-empty`.
    4. **Sign out** draws `signin-signed-out-of` with the service's name in a polite status line. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-4 Email (amends SM-NS-1.2; adds the commands `profile.get_document` and `identity.sign_in`).** The fixed sentence is bold. **Your mailbox** is always one row: with a mailbox, its `account_label` over its sign-in line, **Change password** and **Sign out of email** at the end; with none, `settings-email-not-signed-in` and **Sign in** (`signin-sign-in`), which signs in by the profile's `email.system` — `microsoft365` sends `identity.sign_in { kind: microsoft }`, `google_workspace` sends `{ kind: google }`, `company_server` opens the mail dialog as SM-SW-3.1. **Sign out of email** acts at once, then draws `settings-email-signed-out` (password kinds) or `settings-email-signed-out-account` (the other kinds) in a polite status line. The signature box is disabled while **Add my signature** is off, and draws `settings-saved` under it after it is saved on leaving it. **Email in your workspaces** is always drawn; with no workspace linking email it reads `settings-email-workspaces-none`. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-5 Telegram (amends SM-NS-1.3).** **Set up Telegram** draws `settings-telegram-set-up-line`, then three numbered steps: **Make your bot** (`settings-telegram-step-make`, `settings-telegram-step-make-line`), **Paste the key BotFather sent you** (`settings-telegram-step-paste`, the field `settings-telegram-key`, its hint `settings-telegram-key-hint`, **Connect**) and **Add your bot to a group** (`settings-telegram-step-group`, `settings-telegram-step-group-line`). **Connect** with an empty key sends nothing and the hint reads `settings-telegram-key-empty`. **Remove Telegram** acts at once, then draws `settings-telegram-removed` in a polite status line. Under **Make your bot**, **Open BotFather in Telegram** (`settings-telegram-open-botfather`) sends `app.open_web_page { page: "telegram_botfather" }` to the local backend; a refusal is drawn under the button through `ErrorLine` with **Try again** (amended January 2026 (user decision; Approved by the user January 2026), closing G-SW-1). amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-6 Using your plan (amends SM-NS-1.6).** **Today** is one sentence, `settings-plan-today-line` with the share in words (`settings-plan-share-small`, `-tenth`, `-fifth`, `-third`, `-half`, `-most` for a share of a typical day under 0.07, 0.15, 0.27, 0.42, 0.7 and under 1; `settings-plan-today-over` from a whole day up), never a percentage; the percentages stay under **Details**. **Balanced** carries `settings-default`. **Custom** is drawn only while it is the current choice, with `settings-plan-choice-custom-line`. Every row has one line under its name: `settings-plan-without-model-line`, `settings-plan-reuse-line`, `settings-plan-only-needed-line`, `settings-plan-continue-after-break-line`, `settings-plan-summarise-line`, `settings-plan-ask-before-line`, `settings-plan-agents-at-once-line`, and under **More choices** `settings-plan-show-usage-line` and `settings-plan-thinking-line`; `settings-set-by-company` is drawn in that line, in bold, for every row the company set, **Show usage on each run** and **Thinking** included. The rows sit in one wrapper with no gap, each with its own padding and divider; a switch sits before its words at the start, a choice at the end. **More choices** is drawn at 13 px, weight 600, below the section heading's 15 px. **−** and **+** are named `settings-plan-agents-fewer` and `settings-plan-agents-more`, and the number is in a polite live region. amended January 2026 (user decision; Approved by the user January 2026). *Layout (amended January 2026; user decision; Approved by the user January 2026):* the section is drawn in cards, as Settings › Memory draws its groups (1 px `--color-line` border, `--radius-md`). First a usage card holding `settings-plan-today-line`, with `settings-plan-details` behind a small chevron icon button (`aria-expanded`, `aria-controls`, named `settings-plan-details`), closed at first. Then the card **How Ordin uses your plan** (`settings-plan-choice`): one radio group (`fieldset`, name `plan-choice`), each choice a bordered box with its radio at the start, its name in bold and its line muted under it; the chosen box is outlined in `--color-accent` on `--color-accent-soft`. Then the card **Ways to use less** (`settings-plan-group-less`): `settings-plan-without-model`, `settings-plan-reuse`, `settings-plan-only-needed`, `settings-plan-continue-after-break` as switches and `settings-plan-summarise` as a choice box. Then the card **Limits** (`settings-plan-group-limits`): `settings-plan-ask-before` with its choice box and `settings-plan-agents-at-once` with **−**, the number and **+**; **−** stays disabled at 1. Then the card **More choices** (`settings-plan-more`), whose name is followed by a chevron icon button (`aria-expanded`, named by the card's name) that folds and unfolds its rows inside the card. Every row is one element holding its name and line at the start and its control at the end of the same line, a switch included; the control is named by the row's name (`aria-labelledby`) and described by its line (`aria-describedby`); the row's height follows its words, and when the card is too narrow for both the control drops below the words. A switch is drawn as a track with a knob, accent-filled when on. This replaces "a switch sits before its words at the start" above.
- **SM-SW-7 Code and Open in [IDE] (amends SM-NS-1.8; 11 §5.11, UX-A25).** Under **Open code in**, when an IDE was found, `settings-code-open-in-line`. **Code checking by language** draws `settings-code-languages-line` and, with no language plugin, `settings-code-no-languages`. Each row's switch is named `settings-code-on-for` and its server choice `settings-code-server-for` with the language; **Restart** draws `settings-code-restarted` in a polite status line.
    1. **Open in [IDE].** Where a code repository or one of its files is drawn, one action per IDE that `projects.ides` returns with `enabled = true` (the IDEs found on this computer and ticked under **Open code in**): one IDE is one button `open-in-ide` with its `name`; two or more are items of a ⋯ menu named `open-in-ide-menu`. It sends `projects.open_in_ide { project_id, ide_id, path }` (`path` absent for the repository), writes nothing to the conversation, and draws `open-in-ide-opened` in a polite status line; `PROJ_IDE_NOT_FOUND` and `PROJ_IDE_START_FAILED` draw their sentences. With no IDE ticked nothing is drawn. In this build it is drawn on the code editor's bar for its file and on *Changes waiting* for its repository; the side tree's Code group is not drawn in this build (G-SW-2). amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-8 History (amends the History row of step 9; 11 §5.12).** `settings-history-only-way` gains its second sentence. A number lower than the one kept is not sent at once: the confirmation pattern opens, titled `settings-history-lower-title` with the number, the line `settings-history-lower-body`, the verb `settings-history-lower-verb` (a removing verb, never the default) and **Cancel**; **Cancel** sends nothing and the box shows the number kept. The journal removes older conversations at its next clean-up (JournalAndArtefactStore.md: at start and every 24 hours), which the line says. A higher number is sent at once. `RetentionSetting.company_max_days`, when present, draws `settings-history-company-max`; a saved number draws `settings-history-saved`. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-9 Agent repositories (amends SM-NS-1.1; 11 §5.27).** `settings-repos-intro` first; with no repository, `settings-repos-none`. A row is the display name over one small line: `settings-repos-on` with the host (`github.com` written GitHub), the format words only when the workspace in front links code (SM-WR-14), the agent count, and `settings-repos-locked` when set by the company; the buttons at the end. When a repository the person can publish to is listed and no workspace is open, `settings-repos-show-parts-needs-workspace` says why **Show its parts** is missing. **Rename…** opens a modal dialog titled `settings-repos-rename-title` with the name. **Add an agent repository…** is a modal dialog; its address field has the hint `settings-repos-add-address-hint`; after **Add**, `settings-repos-added` with the name and count in a polite status line. Both dialogs keep Tab inside, close on Escape and put the verb before **Cancel**. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-10 Slack (amends SM-CU-9 and the Slack row of step 9).** The rule line is drawn with its kind and state as SM-CU-9 states; rules in state `off` are listed after those `on`, without **Turn off**. `reason_not_offered` draws one sentence: `settings-slack-tools-off`, `settings-slack-no-app`, `settings-slack-sign-in-not-here`, `settings-slack-not-approved`. **Connect your own Slack app…** labels its boxes `settings-slack-description` (read only, with **Copy**, `settings-slack-copy`, then `settings-slack-copied`) and `settings-slack-client-id`; its verb is `settings-slack-connect`. **Posted without asking** is drawn at 13 px, weight 600. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-11 Jira (amends the ADR-106 rows and SM-JS-2).** The fields are stacked, each label above its field. `settings-jira-site-needed` is drawn under the site field and tied to it (`aria-invalid`, `aria-describedby`). The token guide is three numbered lines, `settings-jira-token-step-1`, `-2`, `-3`, then Atlassian's token page address as text with **Copy the address** (`settings-jira-copy-token-page`, then `settings-jira-token-page-copied`), and `settings-jira-token-note` under the token field. Connected, the row reads `settings-jira-connected` with the site and name when this session connected, otherwise `settings-jira-connected-site` with the credential's `account_label` (the site). **Disconnect Jira…** opens the confirmation pattern titled `settings-jira-disconnect-title`, line `settings-jira-disconnect-body`, verb `settings-jira-disconnect-verb`; done, `settings-jira-disconnected` in a polite status line. Beside **Copy the address**, which stays as the fallback, **Open Atlassian's token page** (`settings-jira-open-token-page`) sends `app.open_web_page { page: "atlassian_api_tokens" }` to the local backend; a refusal is drawn on the token page's line through `ErrorLine` with **Try again** (amended January 2026 (user decision; Approved by the user January 2026), closing G-SW-1). amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-12 Servers (amends SM-UP-5, SM-CU-7 and SM-CU-8).** With no server, `server-none`. Each server is a row: its name over its state — `server-state-connected`, `rail-server-not-connected` with `as_of` or `server-state-not-connected` without it, the version sentences, `server-ended` — and its update line, with the buttons at the end. A server whose `setup.server_update` has `self_update = false` draws `server-update-self-unavailable` after `server-update-available` (SM-CU-8). **Remove this server…** (`server-remove`) no longer acts at once: it opens the confirmation pattern titled `server-remove-title`, line `server-remove-body`, verb `server-remove-verb`; `servers.remove` (which disconnects, forgets the key and deletes the server's rows) is sent on the verb, then `server-removed` in a polite status line. The connect form has the hint `server-address-hint` followed by `server-paste-line-hint` (SM-CU-7). **Connect again…** opens the form under that server's row, titled `server-connect-again-title`, with focus in the address, which holds that server's `ServerView.address` (ServiceApi.md SVC-SRV-1; amended January 2026 (user decision; Approved by the user January 2026), closing G-SW-3); the code box is empty. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-13 Language (amends SM-LG-2).** The picker is a radio group (`fieldset.choice-group`) under `language-picker-label`, the language in use checked, not a listbox. **Export…** draws `language-export-pick` above the languages to export. **Your languages** with none reads `language-yours-none`; each language is a row with its state line under its name. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-14 Knowledge agents and the dialogs (amends the ADR-112 Settings rules and SM-LG-4).** With no workspace, Knowledge agents draws its intro and `knowledge-agents-no-workspace`, and no picker, list or **Connect a knowledge agent…**. Its connect dialog and **Make a new language…** keep Tab inside, close on Escape and put the verb before **Cancel**, as the confirmation pattern does. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-15 Display, How Ordin talks to you, Dictation, Notifications, About (amends SM-NS-1.4, SM-NS-1.5, SM-NS-1.7 and SM-UP-2).** Display set by the company draws `settings-set-by-company` with the appearance choices disabled. How Ordin talks to you marks the program's default of each choice (Friendly, Everyday, Brief) with `settings-default` when the company set none; **Call me** has the hint `settings-voice-call-me-hint`; **Notes for Ordin** has `settings-voice-notes-limit` under its name and the count `settings-voice-notes-count` tied to the box; each box draws `settings-saved` after it is saved on leaving it. Dictation adds `settings-dictation-where` when available or getting ready. Notifications' line is `settings-notify-intro` as amended below. About is headed `about-title` with the brand's product name, and the separate product-name line is not drawn; **Read the terms of reference** is not drawn (G-SW-4). amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-16 One word per idea.** An account is signed out of (**Sign out**), a connection is disconnected (**Disconnect**); "Same as this computer" is the one name of following the computer, in Display and Language alike; a sentence names ordin (the program) or Ordin (the coordinator), never both forms for the same thing. amended January 2026 (user decision; Approved by the user January 2026).
- **SM-SW-17 The form block's document field (extends SM-TF-3 and SM-TF-9 to the `form` block).** A `form` block's field of input `document` is one button, **Choose a document…** (`task-form-choose-document`), or, once chosen, the chosen file's line (`task-form-chosen-document`, `-top`) with **Choose another** (`task-form-change-document`); either opens the document browser of SM-TF-9 over the workspace's places and linked repositories (`workspaces.links`). It replaces the two drop-down lists of the block's document picker. amended January 2026 (user decision; Approved by the user January 2026).

### New messages (en-GB)

| Id | en-GB text | Source |
|---|---|---|
| `settings-section-menu` | Section | SM-SW-1; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-default` | (default) | prototype `drawSetUse`, `drawSetVoice`; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-saved` | Saved | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-intro` | Ordin uses these to read and publish your documents, code and messages. Your passwords stay in this computer's keychain. | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-service` | { $name }, { $for } | prototype "Microsoft, for SharePoint"; amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-for-github` / `-gitlab` / `-microsoft` / `-google` / `-jira` / `-confluence` / `-slack` / `-mail` / `-oidc` | for your agent library and GitHub documents / for GitLab repositories and documents / for SharePoint and email / for email / for the issues you link / for your company's pages / for handovers and messages in Slack / for your company's email / for knowledge agents and your company's servers | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-not-used` | Your organisation doesn't use this | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-signed-in-until` | Signed in until { $date } | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-set-up` | Set up… | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-signed-out-of` | Signed out of { $service }. | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-gitlab-token-hint` | Make one in GitLab under Preferences, then Personal access tokens. It's kept in this computer's keychain. | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-gitlab-token-empty` | Paste a token first. | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-gitlab-save` | Save token | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-hide` | Hide | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-mail-password-empty` | Type your password first. | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-mail-sso-note` | Your company's mail server uses your company account. Type your email address, then finish signing in in your browser. | amended January 2026 (user decision; Approved by the user January 2026) |
| `signin-kind-microsoft` | Microsoft 365 (was "Microsoft") | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-email-not-signed-in` | Not signed in — asked the first time a workspace links email | prototype `drawSetMail`; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-email-signed-out` | Signed out of email, and your password is removed from this computer's keychain. Workspaces keep their email links; nothing is read until you sign in again. | 11 §5.22; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-email-signed-out-account` | Signed out of email. Workspaces keep their email links; nothing is read until you sign in again. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-email-workspaces-none` | No workspace links email yet. Add it from a workspace's "Add to this workspace". | prototype `drawSetMail`; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-telegram-set-up-line` | ordin posts through a Telegram bot of your own, so posts come from the bot, never from you. Three steps, once on this computer. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-telegram-step-make` / `-make-line` | Make your bot / In Telegram, open a chat with @BotFather, Telegram's own helper for making bots. Send /newbot, then a name. BotFather answers with a key. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-telegram-step-paste` | Paste the key BotFather sent you | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-telegram-key` | The bot's key (was "Paste the key BotFather sent you", now the step's title) | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-telegram-key-hint` | ordin keeps it in this computer's keychain. Never paste it into a chat. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-telegram-key-empty` | Paste the key first. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-telegram-step-group` / `-group-line` | Add your bot to a group / Add your bot to a Telegram group as an administrator, so it can read the group. Then link the group from a workspace's "Add to this workspace". | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-telegram-removed` | Removed Telegram from ordin on this computer, and deleted the bot's key from the keychain. Workspaces keep their Telegram links; nothing is read until you connect again. | 11 §5.25; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-today-line` | Today you've used { $share } of a typical day's plan. (was a percentage) | 11 §5.18; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-today-over` | Today you've used more than a typical day's plan. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-share-small` / `-tenth` / `-fifth` / `-third` / `-half` / `-most` | a small part / about a tenth / about a fifth / about a third / about half / most | prototype `shareWords`; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-summarise-early` | Early (saves most) (was "Early") | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-choice-custom-line` | You chose the model for each kind of work under More choices. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-without-model-line` | Opening, finding, going to and "what's new" are done by ordin itself and use none of your plan. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-reuse-line` | The same question over the same versions of the same documents, within 7 days, shows the earlier answer, with Ask again. Never an action or a change. | prototype; UsagePlanner.md (7 days); amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-only-needed-line` | Long documents and long results are cut to the parts that matter; an agent can still read the rest. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-continue-after-break-line` | After an hour the next message would reread the whole conversation; a summary costs less. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-summarise-line` | Summarising is one large message itself, so early pays off only in a conversation that goes on. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-ask-before-line` | Crews and tasks over many documents show their estimated use first. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-agents-at-once-line` | More at once finishes sooner and uses your plan faster. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-agents-fewer` / `-more` | Fewer agents at the same time / More agents at the same time | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-show-usage-line` | How much of your plan each run used, on its progress. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-plan-thinking-line` | More thinking gives better answers to hard questions and uses more of your plan. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-code-open-in-line` | IDEs found on this computer. Each one ticked appears as "Open in …" on the Code group, its repository and each file. | prototype `drawSetCode`; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-code-languages-line` | They give the code editor and agents completion, errors and go to definition. Every language is highlighted in the editor with or without one. | prototype `drawSetCode`; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-code-no-languages` | No language has code checking yet. Every language is still highlighted in the editor. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-code-on-for` / `settings-code-server-for` | Code checking for { $language } / Language server for { $language } | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-code-restarted` | Restarted the language server for { $language }. | amended January 2026 (user decision; Approved by the user January 2026) |
| `open-in-ide` | Open in { $ide } | 11 §5.11 (UX-A25); amended January 2026 (user decision; Approved by the user January 2026) |
| `open-in-ide-menu` | Open in an IDE | amended January 2026 (user decision; Approved by the user January 2026) |
| `open-in-ide-opened` | Opened { $name } in { $ide }. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-history-only-way` | This is the only way earlier conversations are removed. An approval still waiting and work still running are never removed. | 11 §5.12; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-history-lower-title` | Remove conversations older than { $days } days? | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-history-lower-body` | They are removed from this computer at the next clean-up, within a day. An approval still waiting and work still running are never removed. | 11 §5.12; JournalAndArtefactStore.md prune schedule; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-history-lower-verb` | Remove older conversations | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-history-company-max` | Your company keeps history for at most { $days } days. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-history-saved` | Saved. Conversations and history are kept for { $days } days. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-repos-intro` | Where the agents in your agent library come from. | prototype `drawSetRepo`; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-repos-none` | No agent repositories yet. Add one to bring its agents into your library. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-repos-on` | On { $host } | 11 §5.27 ("on GitHub"); amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-repos-show-parts-needs-workspace` | Open a workspace to see the parts of an agent repository you publish to. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-repos-rename-title` | Rename { $name } | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-repos-add-address-hint` | For example https://github.com/northwind/finance-agents | 11 §5.27; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-repos-added` | Added { $name }. Its agent is / Its { $count } agents are (Fluent plural) in your agent library. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-slack-tools-off` / `-no-app` / `-sign-in-not-here` | Slack isn't available here. / Your company hasn't set up a Slack app for ordin. / Signing in to Slack isn't offered on this computer. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-slack-client-id` | Client ID (on your app's Basic Information page) | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-slack-description` / `-copy` / `-copied` | App description to copy / Copy / Copied. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-slack-connect` | Connect | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-jira-intro` | Connect Jira so ordin can read the issues you link and hand work over on them. ordin writes to Jira only when you approve a handover. (one name form) | SM-SW-16; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-jira-connected-site` | Connected to { $site } | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-jira-disconnect-title` / `-body` / `-verb` | Disconnect Jira? / The token is deleted from this computer's keychain. Linked issues stay in their workspaces; nothing is read until you connect again. / Disconnect | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-jira-disconnected` | Disconnected Jira on this computer and deleted the token from the keychain. Linked issues stay in their workspaces; nothing is read until you connect again. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-jira-token-step-1` / `-2` / `-3` | Choose Open Atlassian's token page below and sign in to your Atlassian account. If the page doesn't open, choose Copy the address and paste it into your browser. / Choose Create API token, give it a name such as ordin, and copy the token. / Paste the token below. You don't need anyone's permission. | 11 §5.22a (UX-A75); amended January 2026 (user decision; Approved by the user January 2026); step 1 names **Open Atlassian's token page**, with **Copy the address** as the fallback (SM-SW-11), amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-jira-copy-token-page` / `settings-jira-token-page-copied` | Copy the address / Copied. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-jira-open-token-page` | Open Atlassian's token page | 11 (UX-A75); SM-SW-11; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-telegram-open-botfather` | Open BotFather in Telegram | prototype `tgOpenApp`; SM-SW-5; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-jira-token-note` | The token is kept in this computer's keychain. ordin uses it only to read the issues you link and to hand work over after you approve. | 11 (UX-A75); amended January 2026 (user decision; Approved by the user January 2026) |
| `server-none` | No servers yet. Connect to one with the code from whoever runs it. | amended January 2026 (user decision; Approved by the user January 2026) |
| `server-state-connected` / `server-state-not-connected` | Connected / Not connected | amended January 2026 (user decision; Approved by the user January 2026) |
| `server-address-hint` | For example ordin.northwind-group.com. | amended January 2026 (user decision; Approved by the user January 2026) |
| `server-connect-again-title` | Connect again to { $server } | amended January 2026 (user decision; Approved by the user January 2026) |
| `server-remove` | Remove this server… (was "Remove this server") | amended January 2026 (user decision; Approved by the user January 2026) |
| `server-remove-title` / `-body` / `-verb` | Remove { $server } from this computer? / This computer disconnects and forgets { $server }, with the key it used to connect. The server keeps working. To connect again you need a new code from whoever runs it. / Remove | ServerConnector.md `remove`; amended January 2026 (user decision; Approved by the user January 2026) |
| `server-removed` | Removed { $server } from this computer. | amended January 2026 (user decision; Approved by the user January 2026) |
| `language-follow-computer` | Same as this computer (was "Follow this computer") | SM-SW-16; amended January 2026 (user decision; Approved by the user January 2026) |
| `language-yours-none` | You haven't made or imported a language yet. | amended January 2026 (user decision; Approved by the user January 2026) |
| `language-export-pick` | Choose a language to save its wording as a file. | amended January 2026 (user decision; Approved by the user January 2026) |
| `knowledge-agents-intro` | … When Ordin's own memory isn't enough, … (was "ordin's") | SM-SW-16; amended January 2026 (user decision; Approved by the user January 2026) |
| `knowledge-agents-no-workspace` | Open or make a workspace to connect a knowledge agent to it. | amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-voice-call-me-hint` | Used in greetings only. Leave empty for no name. | prototype `drawSetVoice`; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-voice-notes-count` | { $count } of { $max } | prototype `drawSetVoice`; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-dictation-where` | Start dictation with the microphone button in the message box or a form field. Your speech stays on this computer and is never kept; you check the words before you send them. | prototype; amended January 2026 (user decision; Approved by the user January 2026) |
| `settings-notify-intro` | ordin shows a notification on your computer only for what's switched on here. (was "ordin tells you outside its window only about these.") | amended January 2026 (user decision; Approved by the user January 2026) |
| `about-title` | About { $product } | amended January 2026 (user decision; Approved by the user January 2026) |

### Error Table (rows added)

| Condition | Result | Code | Drawn |
|---|---|---|---|
| **Save token** with an empty GitLab box | not sent | — | `signin-gitlab-token-empty` under the box (SM-SW-3.3) |
| **Sign in** of the mail dialog with an empty password | not sent | — | `signin-mail-password-empty` under the field (SM-SW-3.2) |
| **Connect** with an empty Telegram key | not sent | — | `settings-telegram-key-empty` under the field (SM-SW-5) |
| A lower History number, then **Cancel** | not sent | — | the box shows the number kept (SM-SW-8) |
| **Open in [IDE]** and the IDE cannot be found or started | `Err` from the core | `PROJ_IDE_NOT_FOUND`, `PROJ_IDE_START_FAILED` | its sentence beside the action; no notice (SM-SW-7.1) |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: The Settings walk-through (SM-SW-1 to SM-SW-17)

  Scenario: Happy path — Sign-ins lists the services with nothing saved
    Given no account is saved and the profile names a Microsoft section only
    When the person opens Settings › Sign-ins
    Then GitHub, GitLab, Microsoft 365, Jira and Slack each have a row with what they are for
    And Google and Confluence read "Your organisation doesn't use this" with no Sign in

  Scenario: Happy path — signing in to a service with nothing saved
    When the person presses Sign in on Microsoft 365's row
    Then identity.sign_in is sent with { kind: "microsoft" }

  Scenario: Edge case — lowering History asks first
    Given History keeps 90 days
    When the person types 9
    Then "Remove conversations older than 9 days?" is asked and nothing is sent
    And Remove older conversations sends history.set_days with { days: 9 }

  Scenario: Happy path — Remove this server… asks first
    When the person presses Remove this server… and then Remove
    Then servers.remove is sent and "Removed Northwind from this computer." is shown

  Scenario: Happy path — Open in IntelliJ IDEA
    Given IntelliJ IDEA is found and ticked, and no other IDE is ticked
    When the person presses Open in IntelliJ IDEA on a file in the code editor
    Then projects.open_in_ide is sent with the project, the IDE and the file's path
    And a short notice says it opened, and nothing is written to the conversation

  Scenario: Edge case — Sign out of email acts at once and says what it did
    When the person presses Sign out of email
    Then identity.sign_out is sent with no question
    And a line says the password is removed and that workspaces keep their email links
```

### Gaps

- **G-SW-1** *Opening a web page or another app.* Closed, amended January 2026 (user decision; Approved by the user January 2026): `app.open_web_page` (ServiceApi.md SVC-WEB-1) opens the two pages ordin names, and SM-SW-5 and SM-SW-11 draw **Open BotFather in Telegram** and **Open Atlassian's token page**.
- **G-SW-2** *The side tree's Code group.* 11 §5.11 puts **Open in [IDE]** on the Code group, its repository and each file; the Code group is not drawn in this build, so SM-SW-7.1 draws it on the code editor's bar and on *Changes waiting*. When the Code group is built, its group, repository and file rows carry it as SM-SW-7.1 states.
- **G-SW-3** *Connect again with the address filled in.* Closed, amended January 2026 (user decision; Approved by the user January 2026): `ServerView` carries `address` (ServiceApi.md SVC-SRV-1), and **Connect again…** fills it in (SM-SW-12).
- **G-SW-4** *Read the terms of reference* in About (11 §4, UX-A70). No terms document or command that returns one exists in this build.

## Amendment January 2026 — Claude Code not on this computer: Install Claude Code and Check again

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person's rule that every feature is self-service — the person never waits on IT or company set-up. First start drew "Claude Code needs to be installed by IT on this computer." for `SETUP_HARNESS_MISSING` with no button, and other catalogue sentences told the person to ask or tell IT. Counterparts: SetupAssistant.md SET-R1-9 (the sentences), ServiceApi.md SVC-WEB-1 and OrdinDesktop.md DSK-WEB-1 (the page `claude_code_install`). Every section above stays in force; SM-FS-4 gains the buttons below.

- **SM-FS-9 Install Claude Code, then Check again.** When SM-FS-4 draws a failed result with code `SETUP_HARNESS_MISSING` and the harness of the answer's first attachment is `claude_code` (or there is no attachment), `FirstStart` draws under the failed results two buttons: **Install Claude Code** (`setup-install-claude-code`, primary), which sends `app.open_web_page { page: "claude_code_install" }` to the local backend, and **Check again** (`setup-check-again`), which sends `setup.run_guided {}` again. An error answer of `app.open_web_page` (`CRED_BROWSER_FAILED`) is drawn by step 11 under the buttons, with **Try again**, which asks for the same page again. A missing GitHub Copilot draws neither button. No sentence of first start, of the setup steps or of the catalogue the screens draw tells the person to ask, tell or wait for IT (SetupAssistant.md SET-R1-9).

| id | en-GB |
|---|---|
| `setup-install-claude-code` | Install Claude Code |
| `setup-check-again` | Check again |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — Claude Code is installed from first start (SM-FS-9)
    Given setup.run_guided answers ready false with a failed result SETUP_HARNESS_MISSING
    Then FirstStart shows "Claude Code isn't on this computer yet. Install it, then choose Check again."
    And the buttons "Install Claude Code" and "Check again"
    When the person presses Install Claude Code
    Then app.open_web_page is sent with page "claude_code_install" and no address
    When the person presses Check again
    Then setup.run_guided is sent again

  Scenario: Edge case — a missing GitHub Copilot offers no Claude Code install (SM-FS-9)
    Given the first attachment's harness is copilot_cli and the result SETUP_HARNESS_MISSING failed
    Then no "Install Claude Code" button is drawn

  Scenario: Error path — the browser does not open (SM-FS-9)
    Given app.open_web_page answers CRED_BROWSER_FAILED
    When the person presses Install Claude Code
    Then "ordin couldn't open your web browser. Open it yourself and try again." is drawn under the buttons with Try again
```

### Gaps

None.

## Amendment January 2026 — Ordin's folder question (SM-FA-4; amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: OrdinHost.md OH-ACC-2 — when the person's request names a file in a folder the agent cannot read, Ordin asks with one card in the conversation, **Allow** or **Not now**; HLD 11 *Missing access is shown, not assumed* as amended. No block kind, wire type or command is added. Every section above stays in force.
>
> Evidence read: the block kinds table (`question` — conversation, in full; in *Needs you* until answered; the question, 2–4 choice buttons); scenes `ambiguous` and `agentAsks`; SM-FA-3 (the `access` card); `apps/desktop/ui/src/lib/blocks/kinds/Question.svelte` (one button per choice, sent with `ordin.answer_block`, disabled once answered).

- **SM-FA-4 Ordin's folder question** *(amended January 2026 (user decision; Approved by the user January 2026))*. The folder question of OrdinHost.md OH-ACC-2 is a `question` block and is drawn as every question is: in full in the conversation, its question ("Document agent needs to read CVs to answer."), then one button per choice, **Allow** and **Not now**, in that order. A press sends `ordin.answer_block { workspace_id, block_id, answer: { kind: choice, choice } }`; once answered the buttons are disabled and the item leaves *Needs you*. A refusal is the error line under the buttons. It is not the `access` card of SM-FA-3: it has no tick boxes, and it waits.

### String table (keys added)

None. The question and its two choices are Ordin Host's own text (OrdinHost.md OH-ACC-2), as for OH-PACK-1's questions; the card adds no wording of its own.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — Ordin's folder question is drawn with Allow and Not now (SM-FA-4)
    Given the conversation holds a waiting question "Document agent needs to read CVs to answer." with choices "Allow" and "Not now"
    Then the card shows that question and the buttons "Allow" and "Not now", in that order, with no tick box
    When the person presses Allow
    Then ordin.answer_block is sent with the choice "Allow"
    And the buttons are disabled

  Scenario: Edge case — Not now is sent the same way (SM-FA-4)
    When the person presses Not now
    Then ordin.answer_block is sent with the choice "Not now"
```

### Gaps

None.

## Amendment January 2026 — Setting aside what waits (SM-DIS-1)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person found no way to set aside a card that waits for them; the decision is OrdinHost.md OH-DIS-1 to OH-DIS-4 and HLD 11 §5.15 *Set aside*: **Dismiss** on a form Ordin opened, **Not now** on a request for access or approval, **Skip** on a question. No block kind or command is added; `BlockAnswer` gains `dismissed` (OrdinHost.md OH-DIS-1). Every section above stays in force.
>
> Evidence read: `apps/desktop/ui/src/lib/blocks/kinds/Question.svelte` (choice buttons and the other answer, sent with `ordin.answer_block`, disabled once the item no longer waits), `Form.svelte` (the form's verb), `CrewPlan.svelte` and `crew-plan.svelte.ts` (**Start**, SM-WR-15.5), `Result.svelte` (the offer's **Not now**), `apps/desktop/ui/src/lib/views/tasks/TaskFormView.svelte` (the form Ordin opened, SM-TF-8; **Cancel** closes the tab and the form still waits), `apps/desktop/ui/src/lib/approval/ApprovalScreen.svelte` (**Reject** or **Not now** by `reject_style`), `apps/desktop/ui/src/lib/store/backend-state.svelte.ts` (`markAnswered`; `needs_you_changed` reloads *Needs you*); SM-FA-4.

- **SM-DIS-1 Skip, Dismiss and Not now** *(amended January 2026 (user decision; Approved by the user January 2026))*. Every waiting item Ordin shows has one button that sets it aside without answering it; a question whose own choices include Not now (the folder card of OH-ACC-2, a pack or language offer) uses that Not now and draws no Skip beside it (amended January 2026 (user decision; Approved by the user January 2026)). It is drawn only while the item waits, after the item's own buttons, and is disabled while a reply is awaited, while the backend waits for its server, or when the backend does not answer the `ordin` group:
  1. a `question` block: **Skip** (`block-skip`), after the choices and the other answer; OH-ACC-2's folder question too, so SM-FA-4's card reads **Allow**, **Not now**, **Skip** (the Ordin Host answers its Skip as its Not now, OrdinHost.md OH-DIS-3);
  2. a `form` block: **Dismiss** (`block-dismiss`), beside the form's verb;
  3. the form Ordin opened for a task (SM-TF-8): **Dismiss** after **Cancel**; once the dismissal is taken the form's tab closes. A form opened from the tree has no Dismiss: no card waits for it;
  4. a `crew_plan` block: **Not now** (`block-not-now`) beside **Start**, drawn while **Start** is.

  A press sends `ordin.answer_block { workspace_id, block_id, answer: { kind: "dismissed" } }`. An `accepted` reply marks the item answered, as an answer does, so it leaves the conversation's waiting cards at once, and the Ordin Host's `NeedsYouChanged` reloads *Needs you*. An error reply draws its sentence through `ErrorLine` and the item still waits. An offer keeps its own **Not now**, an approval card its **Reject** or **Not now**, and a colleague's access its **Decline**; they gain no second button.

### String table (keys added, `crates/ordin-localisation/catalogues/en-GB/screens-blocks.ftl`)

| Id | en-GB text |
|---|---|
| `block-skip` | Skip |
| `block-dismiss` | Dismiss |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — Skip on a question sends a dismissal (SM-DIS-1)
    Given the conversation holds a waiting question "Which supplier?" with choices "Kestrel" and "Acme"
    Then the card shows the buttons "Kestrel", "Acme" and "Skip"
    When the person presses Skip
    Then ordin.answer_block is sent with the answer { kind: dismissed }
    And the question no longer waits, Skip is gone and the choices are disabled

  Scenario: Happy path — Dismiss on a form sends a dismissal, not the form (SM-DIS-1)
    Given the conversation holds a waiting form "Q3 rates"
    When the person presses Dismiss
    Then ordin.answer_block is sent with the answer { kind: dismissed }
    And the form no longer waits

  Scenario: Edge — Dismiss on the form Ordin opened for a task closes its tab (SM-DIS-1)
    Given the form Ordin opened for Compare documents is open as a screen
    When the person presses Dismiss
    Then ordin.answer_block is sent with the answer { kind: dismissed } and runs.start_task is not sent
    And the card no longer waits and the form's tab is closed

  Scenario: Happy path — Not now on a crew plan starts no crew (SM-DIS-1)
    Given a valid crew plan waits
    When the person presses Not now
    Then ordin.answer_block is sent with the answer { kind: dismissed } and crews.start is not sent
    And Start is no longer offered

  Scenario: Edge — the folder question also carries Skip (SM-DIS-1, SM-FA-4)
    Given the folder question with "Allow" and "Not now" waits
    Then its buttons are "Allow", "Not now" and "Skip"
    When the person presses Skip
    Then ordin.answer_block is sent with the answer { kind: dismissed }

  Scenario: Error — a refused dismissal keeps the item waiting (SM-DIS-1)
    Given ordin.answer_block answers OH_BLOCK_ANSWERED
    When the person presses Skip
    Then the error line says "This was already answered." and the item still waits
```

### Gaps

None.

## Amendment January 2026 — The Facts list at scale, and topics of things (SM-SF-27; amended January 2026 (user decision; Approved by the user January 2026))

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). HLD: `docs/design/ordin-desktop/11-ux-guidelines.md` §5.10 *Facts at scale*. Commands: ServiceApi.md SVC-TOP-1 (`memory.thing_topics`, `memory.set_thing_topic`); owner MemoryService.md MEM-TOP-1 to MEM-TOP-7. SM-SF-4 (the Facts tab) and SM-SF-12 (the Things tab) stay in force with the changes below.

- **SM-SF-27 Grouping, search, filters, paging and topics** (amended January 2026 (user decision; Approved by the user January 2026)).
  1. **Group by.** Above the Facts list, a select `memory-group-by` offers Thing (default), Kind of fact, Source, Added by and Topic (`memory-group-by-thing`, `-kind`, `-source`, `-added-by`, `-topic`). The choice is remembered per workspace in the view preferences (`ordin.view.memory.group_by`, a map of workspace id to choice); an unknown stored value reads as Thing. Topic is offered only while the backend answers `memory.thing_topics`; otherwise a stored Topic reads as Thing. **Group by** and **Search facts** sit on one row, each label beside its control, both controls 2 rem high and centred on the row, so their labels and boxes line up (amended January 2026; user decision; Approved by the user January 2026). Choosing Topic sends `memory.thing_topics` again (MemoryService.md MEM-TOP-3 settles a topic an agent gave once its thing exists, at that read), and a `workspace_changed` for the workspace reads the topics again too, so the groups follow the topics known now and not those of when the screen opened (amended January 2026; user decision; Approved by the user January 2026).
  2. **Groups.** Each group is a section whose heading is a button with `aria-expanded` that collapses and expands it, reading `memory-group-title` ("{name} ({count})", for example "Manchester Digital Strategy 2021–2026 (14)"); its lines are a list labelled by the heading. Groups are in the order their first fact is met in the list; the group of facts about no thing (Thing, `memory-group-no-thing`) and the group of things with no topic (Topic, `memory-group-no-topic`) come last.
     - *Thing*: by the fact's subject (its triple's subject, else its first thing); the lines use `hideSubject`, so each reads word · value while its accessible name keeps all three parts.
     - *Kind of fact*: by `kind`, named by `KIND_MESSAGE`.
     - *Source*: by the source document's `origin_ref` (a document or an agent's citation), else by the origin in words (SM-SF-11's lines, for example "From our conversation").
     - *Added by*: `memory-added-by-you` (the person), `memory-added-by-team` (a team folder), the agent's name (an agent or its citation; `memory-added-by-an-agent` when no installed agent matches) or `memory-added-by-ordin` (any other origin).
     - *Topic*: by the topic of the fact's subject (`memory.thing_topics`). A fact whose subject has no thing id yet (the memory component had not tied it to a thing when the fact was read, so `triple.subject.id` is empty) takes the topic of the loaded thing whose name or other name matches the subject's name, case and surrounding spaces aside; the Topic chip filters by the same rule (amended January 2026; user decision; Approved by the user January 2026).
  3. **Search.** One search box `memory-facts-search` matches its text, case and accents aside, against the subject, the word and the value of each fact (an older note's statement); each match is marked with `<mark>` in the line (`FactLine`'s `highlight`). `memory.list` takes no text query, so the search and the chips filter the pages already loaded; while a further page exists and a search or chip hides facts, the line `memory-facts-search-loaded` says how many facts were searched and **Load more facts** (`memory-facts-load-more`) reads the next page with the existing cursor.
  4. **Filter chips** (`memory-filters`, each a toggle button with `aria-pressed`): Has conflicts (`conflicts_with` not empty), Out of date (`may_be_out_of_date`), Older notes (`older_note` and no triple), This workspace (a `workspace_id`) and Everywhere (no `workspace_id`) — the last two exclusive — and a Topic select (`memory-filter-topic`: All topics, each topic of the place, No topic). With no fact left, `memory-facts-none-match`.
  5. **Paging.** A group draws its first 50 lines; **Show {count} more** (`memory-group-show-more`) draws the next 50. The list draws its first 100 groups; **Show more groups** (`memory-groups-show-more`) draws the next 100. Grouping 5,000 loaded facts takes ≤ 1 s on the reference machine.
  6. **A thing's topic.** On the Things tab each row shows `memory-topic` ("Topic: {topic}") when the thing has one, and a ⋯ menu (`more-actions-for`) with **Change topic…** (`memory-topic-change`). A thing's page shows its topic, or `memory-topic-none`, and **Change topic…**. **Change topic…** opens a dialog titled `memory-topic-title` with `memory-topic-help`, radio buttons for each topic of the place, **A new topic** (`memory-topic-new`, with a text box of 1–40 characters) and **No topic** (`memory-topic-clear`); **Save** (`memory-topic-save`, not destructive) sends `memory.set_thing_topic { workspace_id, id, topic }` (`null` for No topic), takes the place's topics from the answer and announces `memory-topic-saved`. `MEM_TOPIC_INVALID`, `MEM_TOPIC_REFUSED` and `MEM_THING_NOT_FOUND` draw their sentence in the dialog, which stays open.

### Strings (added; en-GB)

| Id | Text |
|---|---|
| `memory-group-by` | Group by (amended January 2026 (user decision; Approved by the user January 2026), SM-SF-27) |
| `memory-group-by-thing` | Thing |
| `memory-group-by-kind` | Kind of fact |
| `memory-group-by-source` | Source |
| `memory-group-by-added-by` | Added by |
| `memory-group-by-topic` | Topic |
| `memory-group-title` | { $name } ({ $count }) |
| `memory-group-no-thing` | Not about one thing |
| `memory-group-no-topic` | No topic |
| `memory-group-show-more` | Show { $count } more |
| `memory-groups-show-more` | Show more groups |
| `memory-added-by-you` | You |
| `memory-added-by-ordin` | Ordin |
| `memory-added-by-team` | Your team |
| `memory-added-by-an-agent` | An agent |
| `memory-facts-search` | Search facts |
| `memory-facts-search-loaded` | Searching the { $count } facts loaded so far. Load more facts to search further. |
| `memory-facts-none-match` | No facts match. |
| `memory-facts-load-more` | Load more facts |
| `memory-filters` | Show only |
| `memory-filter-conflicts` | Has conflicts |
| `memory-filter-out-of-date` | Out of date |
| `memory-filter-older` | Older notes |
| `memory-filter-workspace` | This workspace |
| `memory-filter-everywhere` | Everywhere |
| `memory-filter-topic` | Topic |
| `memory-filter-topic-all` | All topics |
| `memory-topic` | Topic: { $topic } |
| `memory-topic-none` | No topic yet |
| `memory-topic-change` | Change topic… |
| `memory-topic-title` | Topic of { $thing } |
| `memory-topic-help` | A short plain label, such as Strategy, People, Funding or Suppliers. |
| `memory-topic-new` | A new topic |
| `memory-topic-clear` | No topic |
| `memory-topic-save` | Save |
| `memory-topic-saved` | Topic saved. |

### Error Table

No code is added; `MEM_TOPIC_INVALID`, `MEM_TOPIC_REFUSED` and `MEM_THING_NOT_FOUND` draw the owner's sentence (ErrorHandling.md).

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: The Facts list at scale, and topics of things (SM-SF-27)

  Scenario: Happy path — grouped by thing
    Given two facts about "Manchester Digital Strategy 2021–2026", one about Kestrel and an older note
    When the Facts tab is shown
    Then the groups read "Manchester Digital Strategy 2021–2026 (2)", "Kestrel (1)" and the older note's thing last, and lines under a thing read word · value

  Scenario: Happy path — Group by Kind of fact, Source, Added by and Topic
    When the person chooses each grouping
    Then the facts are grouped by kind, by source document or origin, by you, Ordin or the agent's name, and by topic with "No topic" last
    And the choice is remembered for the workspace

  Scenario: Happy path — one search over the three parts, matches marked
    When the person searches "three"
    Then only facts whose subject, word or value holds "three" are listed, the match marked

  Scenario: Happy path — the chips
    When the person turns on Has conflicts, Out of date, Older notes, This workspace or Everywhere, or picks a topic
    Then only the facts each names are listed

  Scenario: Edge — a large group in pages
    Given 120 facts about one thing and a further page
    Then the group draws 50 lines, Show 50 more draws 100, and Load more facts reads the next page with the cursor

  Scenario: Happy path — Change topic…
    When the person opens Kestrel's ⋯ menu, chooses Change topic…, types the new topic "Suppliers" and saves
    Then memory.set_thing_topic { workspace_id, id, topic: "Suppliers" } is sent and the row reads "Topic: Suppliers"

  Scenario: Error — a refused topic
    Given memory.set_thing_topic answers MEM_TOPIC_REFUSED
    Then its sentence is drawn in the dialog, which stays open

  Scenario: Edge — a backend without topics
    Given memory.thing_topics answers SVC_COMMAND_UNKNOWN
    Then Group by offers no Topic, and a stored Topic reads as Thing
```

### Gaps

None.

## Amendment January 2026 — the smoke test of the memory screen, the screen tabs, the PDF view and the Agents screen (amended January 2026; user decision; Approved by the user January 2026)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: the person's smoke test of the desktop window. The rows changed are the *Screen area and tabs* row of step 6's table, step 6's *What Ordin remembers — two tabs* and *History*, SM-TK-2, SM-SF-13, SM-SF-22, SM-SF-27, SM-PDF-1 and SM-AL-1; each carries its own amendment sentence. No command, event or error code is added or changed.

### Strings (added; en-GB)

| Id | Text |
|---|---|
| `screen-tab-menu` | Tab actions for { $title } |
| `screen-tab-menu-close` | Close |
| `screen-tab-menu-close-others` | Close others |
| `screen-tab-menu-close-right` | Close tabs to the right |
| `screen-tab-menu-close-all` | Close all |
| `memory-diagram-select-hint` | Press a box or an arrow in the diagram to see its facts here. |

Removed: `team-share-from-tree` (SM-TK-2).

```gherkin
Feature: The smoke-test fixes

  Scenario: Happy path — closing several tabs
    Given Home, Supplier list, Contract and Notes are open, Contract in front
    When the person right-presses Supplier list and chooses Close others
    Then only Supplier list stays open, in front

  Scenario: Edge — Home closes
    When the person closes every tab, Home included
    Then the area reads "Nothing is open" and Home opens again only from the side rail

  Scenario: Happy path — History in place
    When the person opens a fact's details and presses History
    Then "How this fact changed" opens inside that fact's line, and History pressed again closes it

  Scenario: Edge — a diagram with nothing selected
    Given the diagram opened from the Things tab, with no centre
    Then under the drawing only "Press a box or an arrow in the diagram to see its facts here." is drawn after the things

  Scenario: Happy path — Group by Topic follows the topics known now
    Given an agent gave Kestrel the topic "Suppliers" after the screen opened
    When the person chooses Group by Topic
    Then memory.thing_topics is read again and Kestrel's facts are under "Suppliers"
```

### Gaps

None.

## Amendment January 2026 — collapsing the side panes (SM-PC-1; amended January 2026; user decision; Approved by the user January 2026)

- **SM-PC-1 Collapse and restore the side panel and the conversation** (amended January 2026; user decision; Approved by the user January 2026). The side panel (the side rail with the tree) and the conversation panel beside the screen area each draw, at their top, a small icon button in the editor toolbar's style (1.75 rem square, `i-chev` pointing towards the pane's outer edge to collapse and away from it to restore), with `aria-expanded` (true while the pane is open) and its words as `aria-label` and `title`: `pane-rail-collapse` / `pane-rail-restore` and `pane-conversation-collapse` / `pane-conversation-restore`. Pressing it collapses the pane to a strip 2.5 rem wide that holds only the restore button: the pane's content stays mounted but hidden, and its resize edge is hidden and out of the tab order. The window's five columns (side panel, its edge, screen area, the conversation's edge, conversation) keep their places whatever is collapsed: a collapsed pane's resize edge stays in its column, neither drawn nor reached by pointer, tab or assistive technology, and is never taken out of the layout, so collapsing one pane never moves or hides the screen area, the other pane or a restore button (amended January 2026; user decision; Approved by the user January 2026). The button keeps the focus after each press, and while collapsed it is the region's focus target for F6. The state is kept in the view preferences as `ordin.view.panel.collapsed` (`{ rail, conversation }`; a value that does not parse reads as both open, as other view preferences do) and restored when the window opens. Below 1,024 CSS pixels the conversation slides over and is opened from the header as before, so its collapse button is not drawn and a stored collapse is ignored there.

### Strings (added; en-GB)

| Id | Text |
|---|---|
| `pane-rail-collapse` | Hide the side panel |
| `pane-rail-restore` | Show the side panel |
| `pane-conversation-collapse` | Hide the conversation |
| `pane-conversation-restore` | Show the conversation |
| `pane-agent-work` | Make the conversation wide for working with agents |

The button `pane-agent-work` (`id="agent-work-toggle"`, `aria-pressed`) sits beside the conversation's collapse button. Pressed, it collapses the side panel to its strip, opens the conversation, and splits the width left 60% conversation, 40% screen area; pressed again, it restores the side panel's and the conversation's collapsed state exactly as they were, and their widths, which it never changes. The layout is remembered as `ordin.view.panel.agent_work` (amended January 2026; user decision; Approved by the user January 2026).

```gherkin
Feature: Collapsing the side panes (SM-PC-1)

  Scenario: Happy path — hide and show the side panel
    When the person presses Hide the side panel
    Then the side panel becomes a slim strip with Show the side panel, and pressing it brings the panel back

  Scenario: Edge — remembered
    Given the person collapsed the conversation
    When the window opens again
    Then the conversation is a strip with Show the conversation

  Scenario: Edge — collapsing one pane moves nothing else (amended January 2026; user decision; Approved by the user January 2026)
    When the person hides the side panel, the conversation, or both
    Then the screen area and every pane still open keep their columns, and each collapsed pane's strip shows its restore button
```

### Gaps

None.

## Amendment January 2026 — Publish to…, the Documents overview, one agent's page, Keep as dashboard

> **Status:** (amended January 2026; user decision; Approved by the user January 2026). Source: four user decisions on the desktop's UX: publishing moves from a page button into the document's menu; the Documents screen becomes worth opening beyond the tree; the Agents screen becomes the page of one agent; **Keep as dashboard…** is wired, in the report's menu and as a tool of Ordin's conversation. Actions are menu items or small icon buttons with an accessible name and a tooltip, never a row of page buttons (11-ux-guidelines.md).
>
> Evidence read: `crates/ordin-service-api/src/commands/push.rs` (`PublishCopyRequest`), `crates/ordin-push/src/copies.rs` (the `PUB_*` format rules; a SharePoint destination is resolved as `graph:drives/<library>/items/<folder>`), `apps/desktop/ui/src/lib/service-api/commands.gen.ts` (`confluence.find`, `sharepoint.find`, `files.versions`, `files.list_folder` with sort `changed_desc`, `files.search`, `workspaces.links`, `agents.get`, `agents.made_of`, `runs.list`, `reports.list`, `reports.get`, `runs.get`, `dashboards.keep`, `dashboards.list`, `dashboards.refresh`), `crates/ordin-journal/src/reports.rs` (`keep_dashboard`, `add_dashboard_refresh`), `crates/ordin-host/src/crews.rs` (`dashboards_refresh`).

### Publish to… (closes the Publish a copy… half of G-CU-3)

- **SM-PUB-1 One Publish to… item.** A document's menu — its row's ⋯ and right-click menu in the side tree (SM-RL-3, after **Open in its app** and before **Move to Bin…**) and its row's ⋯ on the Documents overview (SM-DS-6) — holds one item **Publish to…** (`publish-to`). The document page shows content only and has no menu. Pressed, it opens to one item per destination, `publish-to-destination` with the destination's name: **Confluence…** when the workspace has an active link of kind `confluence_location` and the backend answers `confluence.find`; **SharePoint…** when it has an active link of kind `sharepoint_location` and the backend answers `sharepoint.find`; both also need `push.publish_copy` and `files.versions` (SM-UP-1). The destinations are read with `workspaces.links` when the menu opens. A destination the document's kind cannot go to under the 2C `PUB_*` rules is left out: a Markdown document goes to Confluence as a page (`format = page`, `confluence_page`) and to SharePoint as a Word copy (`format = word`, `sharepoint_file`); any other file goes to Confluence only, as an attachment (`format = exact_file`, `confluence_attachment`). A document in a code repository place (a `git_repository_folder` or `git_remote` link on its place) offers no destination (G-PUB-1). With no destination, **Publish to…** is not drawn. The overview's row menu holds **Open** (`doc-menu-open`), **Ask Ordin about this**, **Add to knowledge** and **Add to memory** as the tree's document menu does, then **Publish to…**.
- **SM-PUB-2 The Publish to dialog.** Choosing a destination opens a `ConfirmDialog` titled `publish-to-title`, body `publish-to-body`, verb **Publish** (`publish-to-verb`, not destructive). It holds a search field `publish-to-find` with **Find** (`publish-confluence-search`, at least 2 characters), which sends `confluence.find` or `sharepoint.find` with `limit = 10` and lists, as one radio choice each, the found places that can hold a copy: a Confluence result with a `page_id`; a SharePoint result that is a folder with an `item_id`. None → `publish-to-nothing`. **Publish** with nothing chosen draws `publish-to-choose` and sends nothing. Otherwise it sends `files.versions { place_id, path }` and then `push.publish_copy` with `original = place_file`, `original_version` = the highest `version` as text, `original_name` and `file_name` = the document's name, `page_file = null`, `followed_branch = null`, `keep_up_to_date = false`, the format of SM-PUB-1 and the destination: `{ space: space_key, parent_page_id: page_id }` for Confluence; `{ site: site_name, library: drive_id, folder: item_id }` for SharePoint. `ok` → the dialog closes and `publish-to-sent` is announced (the Push Service's approval card waits in Needs you; nothing is published from the screens). An error → its sentence in the dialog, which stays open.

### The Documents overview

- **SM-DS-4 Recently changed.** The screen `documents` with no instance draws the heading `tree-documents`, a search field labelled `documents-search-label`, the sub-heading `documents-recent-heading` and one compact row per document: it sends `workspaces.links { workspace_id }`, keeps the active links that have a place and are not Jira issues, sends `files.list_folder { place_id, folder: "", sort: "changed_desc" }` for each, keeps the files (not the folders), and draws the newest 30 across all places, newest first. Each row draws the file kind's icon, the name and one line: `documents-row-changed-by` (where it lives — the link's `display_name` —, the date of `changed_at`, and `changed_by`) or `documents-row-changed` when `changed_by` is absent. A row opens its document page (SM-NS-4). No place → `documents-no-places` (SM-DS-2); no document → `documents-recent-none`. A place that cannot be read draws its error with **Try again** and the others' rows. It is read again on each `workspace_changed` for the workspace.
- **SM-DS-5 Search.** Submitting the field with at least 2 characters sends `files.search { query, place_ids: <the places of SM-DS-4>, limit: 30 }` and replaces the rows with the hits, under the searched text as sub-heading: the name is the last segment of `path`, the line `documents-row-changed` (a hit carries no `changed_by`). No hit → `documents-search-none`. Clearing the field, or fewer than 2 characters, draws SM-DS-4's list again.
- **SM-DS-6 Actions only in the row's menu.** A row's actions are in its ⋯ menu (an icon button named `more-actions-for`), with the items of SM-PUB-1's overview menu. The overview draws no other button.

### Publishing a folder automatically

- **SM-FP-1 Publish this folder automatically… in the folder's menu.** A folder row in the side tree (SM-RL-3) whose place is the person's folder or the workspace's own folder (`Place.kind` `persons_folder` or `workspace_folder`), when the backend answers `confluence.find` and `workspaces.propose_confluence_link`, holds **Publish this folder automatically…** (`publish-folder`) in its ⋯ and right-click menu, unless `workspaces.confluence_links` (read when the menu opens) already holds a link on this place and folder. It opens a `ConfirmDialog` titled `publish-folder-title`, body `publish-folder-body`, with the search field `publish-confluence-find` and **Find** (`publish-confluence-search`, at least 2 characters, `confluence.find` with `limit = 10`), one radio choice per found page with a `page_id` (`publish-confluence-nothing` when none), and verb **Publish** (`publish-to-verb`, not destructive). **Publish** with nothing chosen draws `publish-to-choose`; otherwise it sends `workspaces.propose_confluence_link { workspace_id, place_id, folder (no trailing /), location }`; `ok` → the dialog closes, `publish-folder-sent` is announced and the approval card named by `approval_id` opens. An error → its sentence in the dialog. Nothing is published before the person approves the card (ADR-026).

### The page of one agent

- **SM-AG-6 Agents in the side tree.** The tree's **Agents** row (SM-AT-1) opens and closes the list of the workspace's agents and opens no screen of its own; its ⋯ menu keeps **Browse the agent library**. An agent row opens `agents:<agent instance id>` with the agent's `display_name` as the tab title, and shows its tasks under it as SM-AT-2 says; its ⋯ menu keeps **What it's made of** and **Remove…**. The side tree is the one list of agents: the screen `agents` with no instance draws only `agents-choose-in-tree`.
- **SM-AG-7 What the page holds.** The screen `agents:<agent instance id>` sends `agents.get`, `agents.made_of { detail: "business_user" }` and, when the backend answers it, `runs.list { workspace_id }`, again on each `workspace_changed`. It draws the agent's name as heading; `agent-page-what` with `what_it_does`; a waiting update as SM-AG-2 (with **Update** and **Not now**, the person's decision); `agent-page-tasks` with the agent's own runs (`agent.agent_instance_id` = this agent), newest by `queued_at` first, at most 5, each `agent-page-task-line` (date and SM-RS-1 state label) opening its run screen, or `agent-page-no-tasks`; `agent-page-choices` with one line per `InstalledAgent.uses` item, `agent-page-choice-line` with the kind's plain words and the label, `agent-page-choice-fixed` when it is not changeable, or `agent-page-no-choices`; then SM-FA-1's folder access and Check it (SM-WR-9). An `agents.get` refusal draws its sentence. Changing a choice is not offered here (G-AG-1).
- **SM-AG-8 Actions in the page's ⋯ menu.** **Show the installed files** (SM-WR-14, when the workspace links code and the backend answers `agents.installed_files`) and **Remove [agent]…** (SM-AG-1) are items of the ⋯ menu beside the heading; the menu is not drawn when it would be empty. After a removal the page's tab closes and the tree reads its agents again. **Browse the agent library** is not drawn on this page.

### Keep as dashboard

- **SM-DB-1 Keep as dashboard… in the report's menu.** A `report` block draws a ⋯ menu beside its title, holding **Keep as dashboard…** (`dashboard-keep`), drawn when the backend answers `reports.list`, `runs.get` and `dashboards.keep`. It opens a `ConfirmDialog` titled `dashboard-keep-title`, body `dashboard-keep-body`, a field `dashboard-keep-name` holding the report's title cut to 80 characters, and verb **Keep** (`dashboard-keep-verb`, not destructive).
- **SM-DB-2 What Keep sends.** A blank name or one over 80 characters → `dashboard-keep-name-needed`, nothing sent. Otherwise: `reports.list { workspace_id }`, the report with the block's `title` and the same `as_of` instant (none → `dashboard-keep-not-found`); `runs.get { run_id }` of that report; `made_by` from the run's origin — `task` → `{ kind: "task", task_id }`, `conversation` → `{ kind: "conversation" }` (2C `DashboardSource::Conversation`, this amendment), `crew_round` → `dashboard-keep-from-crew`, nothing sent; then `dashboards.keep { workspace_id, name, made_by, inputs: <the run's inputs_used>, run_id }`. `ok` → the dialog closes and `dashboard-kept` is announced; an error (a name already used, `REPORT_DASHBOARD_NAME_DUPLICATE`) → its sentence in the dialog.
- **SM-DB-3 Dashboards in the side tree.** The tree's **Dashboards** row opens the Dashboards screen and shows, under it, one row per dashboard of `dashboards.list { workspace_id }`, read when it opens and again on each `workspace_changed`; a row opens `dashboards:<dashboard id>` with the dashboard's name as tab title. On the Dashboards screen a row opens the same screen.
- **SM-DB-4 A dashboard's screen.** The screen `dashboards:<dashboard id>` reads `dashboards.list` and, when the backend answers them, `reports.list` and `reports.get`, again on each `workspace_changed`. It draws the name as heading with **Refresh** as a small icon button (`dashboard-refresh` as accessible name and tooltip, drawn when the backend answers `dashboards.refresh`), the line `dashboard-kept-on` and, with a report, `dashboard-as-of`; then the report of the last of `refresh_runs` that has one: each part's heading, key figures as label, value and unit, a table or a chart as its table of columns and rows. No report yet → `dashboard-none-yet`; the dashboard gone → `dashboard-gone`. **Refresh** sends `dashboards.refresh { workspace_id, dashboard_id }`; started → `dashboard-refresh-started` is announced, and the new report shows once made (OrdinHost.md OH-KD-2; JournalAndArtefactStore.md JA-DB-1).

```gherkin
  Scenario: Happy path — publishing a document to Confluence from its menu (SM-PUB-1, SM-PUB-2)
    Given the workspace is connected to Confluence
    When the person chooses Publish to… › Confluence… on notes.md, finds "Supplier pages" and presses Publish
    Then push.publish_copy is sent for notes.md at its newest version as a page under that page
    And the dialog closes saying it waits for approval

  Scenario: Edge case — no destination connected (SM-PUB-1)
    Given the workspace has only a folder on this computer
    Then the document's menu has no Publish to…

  Scenario: Error path — Publish with no place chosen (SM-PUB-2)
    When the person presses Publish before choosing a place
    Then the dialog asks for a place and nothing is sent

  Scenario: Happy path — the overview lists what changed most recently (SM-DS-4)
    Given two places with documents
    When the person opens Documents from the tree
    Then the newest documents of both places are listed with where they live, when and by whom

  Scenario: Error path — a place that cannot be read (SM-DS-4)
    Then its error is drawn with Try again and the other place's documents are listed

  Scenario: Happy path — an agent's page from the tree (SM-AG-6, SM-AG-7)
    When the person opens Agents in the side tree and chooses "Document agent"
    Then its page says what it does, lists its recent tasks and its choices

  Scenario: Happy path — keeping a report of the conversation (SM-DB-1, SM-DB-2, SM-DB-3)
    Given Ordin showed the report "Supplier prices" in the conversation
    When the person chooses Keep as dashboard… and presses Keep
    Then dashboards.keep is sent with made_by conversation and the report's run
    And "Supplier prices" appears under Dashboards in the tree

  Scenario: Edge case — a crew's report (SM-DB-2)
    Then Keep says to keep it from the crew's workflow and nothing is sent
```

### Gaps

- **G-PUB-1** *A document in a code repository place.* `push.publish_copy` needs the commit of such an original and the branch it follows; no command the screens hold answers the commit of one file, so Publish to… offers no destination for it.
- **G-DS-1** *Whether Ordin has read a document into knowledge or memory.* No command maps a place's file to its knowledge pages or memory facts (`knowledge.list_read_pages` answers team folders' pages, with no place or path), so the overview's rows do not say it.
- **G-DS-2** *Recently changed below a place's top folder.* `files.list_folder` lists one folder; no command lists a place's recent changes across its sub-folders, so SM-DS-4 reads each place's top folder only.
- **G-AG-1** *Changing an agent's choice on its page.* `agents.change_choice` takes a `PackChoice` the screens cannot build from `UsesItem` alone (the choices offered are not listed), so the page shows choices without changing them.

## Amendment January 2026 — Settings › Memory holds settings only (SM-MS-1 to SM-MS-5; amended January 2026; user decision; Approved by the user January 2026)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). It replaces SM-NS-1.9's body; every other section stays in force. HLD: `docs/design/ordin-desktop/11-ux-guidelines.md` §4 *Settings › Memory* (amended January 2026 (user decision; Approved by the user January 2026)). Owners: `components/OrdinDesktop.md` DSK-MEM-1 (saves the settings and hands them on), `components/MemoryService.md` MEM-SET-1 to MEM-SET-5 (enforces them), `components/OrdinHost.md` OH-MS-1 (Ordin's seed).
>
> Evidence read: `apps/desktop/ui/src/lib/views/Settings/Settings.svelte` (the section drew the `memory` screen in place, a second copy of the side tree's page); `apps/desktop/ui/src/lib/views/fixed/facts.ts` (`GROUP_BY`, `GROUP_PAGE` 50); `apps/desktop/ui/src/lib/views/fixed/FactLine.svelte` (`expanded`, closed by default); `apps/desktop/ui/src/lib/store/view-prefs.ts` (`ViewPrefs`, `memoryGroupBy`); `apps/desktop/src/prefs.rs` (`settings.get` / `settings.change`).

- **SM-MS-1 The section (amended January 2026; user decision; Approved by the user January 2026).** `views/Settings/MemorySettings.svelte` draws the heading `settings-section-what-ordin-remembers`, the line `settings-memory-intro`, then three groups, each an `<h4>` and a `role="group"` labelled by it, in this order: *What Ordin keeps* (SM-MS-2), *How memory is shown* (SM-MS-3), *How Ordin uses memory* (SM-MS-4). Every control has a label and a one-line explanation tied to it with `aria-describedby`. No fact, thing or held write is drawn, and no `memory.*` command is sent. The section is listed when the local backend answers `settings.get` and `settings.change` (SM-UP-1).
- **SM-MS-2 What Ordin keeps (backend; amended January 2026; user decision; Approved by the user January 2026).** Read with `settings.get { section: what_ordin_remembers }`, changed with `settings.change { key, value }` when the control changes, kept by the program (DSK-MEM-1) and enforced by the Memory Service (MEM-SET-2 to MEM-SET-4). (a) `memory.keep_mode`, two choices: `on_its_own` "Keep it on its own" (default) and `ask_first` "Ask me first". (b) *Learn from*: switches `memory.learn_from_conversations` "Conversations" and `memory.learn_from_documents` "Documents", both on by default. Email is not a source: no product path writes a fact from an email. (c) `memory.suggest_removal_after_days`, a choice of `null` "As long as Ordin thinks they're useful" (default), 30, 90, 180 or 365 days; the explanation says nothing is removed unless the person chooses **Forget**.
- **SM-MS-3 How memory is shown (this computer; amended January 2026; user decision; Approved by the user January 2026).** Kept in the view preferences under `ordin.view.memory.shown` as `{ groupBy, pageSize, detailsOpen }` (`ViewPrefs.memoryShown` / `setMemoryShown`); each field that fails to parse falls back to its default alone, with `UI_STORAGE_UNAVAILABLE` as for every view preference. (a) *Group facts by*: the Facts list's Group by choices (`GROUP_BY`: Thing, Kind of fact, Source, Added by, Topic), default Thing; the memory screen uses it where the workspace has no Group by choice of its own (SM-SF-27), and Topic falls back to Thing where the backend has no topics, as there. (b) *Facts shown before Show more*: 10, 25, 50 or 100, default 50 (`GROUP_PAGE`); a group draws that many lines and each **Show more** adds as many. (c) *Show each fact's details straight away*: off by default; on, every `FactLine` of the Facts list starts open (`expanded`). The memory screen reads these when it opens.
- **SM-MS-4 How Ordin uses memory (backend; amended January 2026; user decision; Approved by the user January 2026).** As SM-MS-2. (a) `memory.use_everywhere` "Use facts in other workspaces", on by default: on, a fact the person made **Remember everywhere** is used in every workspace (today's behaviour); off, each workspace's recall uses only its own facts and **Remember everywhere** is refused (`MEM_EVERYWHERE_TURNED_OFF`, MEM-SET-5). (b) `memory.say_which_facts` "Say which facts an answer used", off by default: on, Ordin's next conversation is asked to end an answer that relied on recalled facts by naming them (OH-MS-1).
- **SM-MS-5 Messages (en-GB, `screens-settings.ftl`; amended January 2026; user decision; Approved by the user January 2026).** `settings-memory-intro`, `settings-memory-group-keeps`, `settings-memory-keep`, `settings-memory-keep-on-its-own`, `settings-memory-keep-ask-first`, `settings-memory-keep-hint`, `settings-memory-sources`, `settings-memory-source-conversations`, `settings-memory-source-conversations-hint`, `settings-memory-source-documents`, `settings-memory-source-documents-hint`, `settings-memory-removal`, `settings-memory-removal-faded`, `settings-memory-removal-days`, `settings-memory-removal-hint`, `settings-memory-group-shown`, `settings-memory-group-by`, `settings-memory-group-by-hint`, `settings-memory-page-size`, `settings-memory-page-size-hint`, `settings-memory-details-open`, `settings-memory-details-open-hint`, `settings-memory-group-uses`, `settings-memory-use-everywhere`, `settings-memory-use-everywhere-hint`, `settings-memory-say-which`, `settings-memory-say-which-hint`. Group by reuses `memory-group-by-*`. `settings-memory-workspace` and `settings-memory-no-workspace` are removed. No message names IT or anyone else to ask; every setting is the person's own.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A Settings › Memory value of the wrong type, or a period outside 1–3,650 days (SM-MS-2; amended January 2026; user decision; Approved by the user January 2026) | `Err(permanent)` | `DSK_PREFERENCE_VALUE_INVALID` | the control returns to its stored value; the sentence by `ErrorLine` |
| A Settings › Memory value cannot be saved (SM-MS-2, SM-MS-4) | `Err(retryable)` | `DSK_PREFERENCES_WRITE_FAILED` | the control returns to its stored value; the sentence with **Try again**, which sends the same change |
| `settings.get` of `what_ordin_remembers` fails | `Err` from the core | the owner's code | only *How memory is shown* is drawn, with the sentence and **Try again** |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Settings › Memory holds settings only (SM-MS-1 to SM-MS-5)

  Scenario: Happy path — three groups of settings and no facts
    When the person opens Settings › Memory
    Then they see What Ordin keeps, How memory is shown and How Ordin uses memory, each control with a one-line explanation
    And no fact is listed and no memory command is sent

  Scenario: Happy path — a backend setting is changed
    When the person chooses "Ask me first"
    Then settings.change { key: "memory.keep_mode", value: "ask_first" } is sent and the choice stays

  Scenario: Edge — display choices shape the memory page
    Given the person chose Group by Kind of fact, 10 facts before Show more and details open
    When they open a workspace's memory page that has no Group by choice of its own
    Then facts are grouped by kind, each group shows 10 with Show more adding 10, and each fact's details are open

  Scenario: Error — a refused change
    When settings.change is refused with DSK_PREFERENCE_VALUE_INVALID
    Then the control returns to its stored value and the sentence is shown

  Scenario: Error — the settings cannot be read
    When settings.get of what_ordin_remembers fails
    Then only How memory is shown is drawn, with the sentence
```

### Gaps

None.

## Amendment January 2026 — icon buttons, the Workspaces menu, Search results and Memory tables (amended January 2026; user decision; Approved by the user January 2026)

> Every section above stays in force except where a bullet below amends it in place. Evidence read: the uncommitted `apps/desktop/ui/src/App.svelte`, `lib/shell/SideRail.svelte`, `lib/shell/ConversationPanel.svelte`, `lib/shell/screens.ts`, `lib/shell/open-item.ts`, `lib/views/SearchDialog.svelte`, `lib/views/Settings/MemorySettings.svelte`, `lib/views/fixed/Memory.svelte`, `lib/views/fixed/facts.ts`, `lib/views/fixed/Knowledge.svelte`.

- **SM-PC-1 (amended in place).** An open pane draws its collapse button in its own top row, not in a strip of its own: the side panel's at the end of the Home and Search row, the conversation's at the start of the conversation's top row, before **Summarise**, **Clear** and **Earlier conversations**, which are now small icon buttons in the same style (`i-bullets`, `i-bin`, `i-history`; their words as `aria-label` and `title`). A collapsed pane is still the 2.5 rem strip holding only the restore button. In a narrow window the conversation draws no collapse button.
- **Step 7, side rail row (amended in place).** **Home** and **Search or go to…** are small icon buttons (`i-home`, `i-search`) with `rail-home` / `rail-home-count` and `rail-search` as `aria-label` and `title`; Home's count stays drawn on its button.
- **SM-WS-1 and SM-TW-3 (amended in place).** **New workspace** and **Open a team workspace…** are no longer buttons under the Workspaces heading: an icon button beside the heading (`aria-haspopup="menu"`, `more-actions-tip` as `aria-label` and `title`) opens a menu holding them, each shown on the terms it had before. The menu takes the focus on its first item; Up and Down move between items; Escape closes it and returns the focus to the button; a press outside closes it.
- **SM-OS-2 Search results open their page.** In *Search or go to…*, a *Go to* result opens the screen the side tree opens for it: an agent → its agent page (`agents:<id>`), a workflow → `workflow:<id>`, a conversation → `earlier-conversation:<id>`, each with the result's name as its tab title; an item kind with no screen opens its workspace only. A *Found in pages* knowledge result opens the knowledge page (`knowledge:<page id>`), as a knowledge card does.
- **SM-OS-3 A removed item's tabs close (amended January 2026; user decision; Approved by the user January 2026).** When an item is removed — from the side tree's ⋯ or right-click menu, from the item's own screen, or elsewhere — every screen tab open for it closes straight away, in every workspace's row of tabs; when the front tab closes the front moves as for Close (the nearest tab left open after it, else before it). The items and their tabs: a dashboard (`dashboards:<id>`); a document (`document:<place id>:<path>`); a linked place removed from the workspace (`documents:<place id>` and every `document:<place id>:…`); a knowledge page (`knowledge:<page id>`); a workflow (`workflow:<id>`); an agent (`agents:<id>`, `installed-files:<id>` and every `task-form:<id>:…`); an earlier conversation (`earlier-conversation:<id>`, and each one it continues). When the side tree reads a complete list again (on opening and on each `workspace_changed`) — the dashboards, the agents, a folder's documents — a tab whose item is no longer in it closes, so a removal made elsewhere closes its tabs too. Closing never deletes anything else.
- **SM-MS-1 (amended in place).** Each group of Settings › Memory (What Ordin keeps, How memory is shown, How Ordin uses memory) is drawn as a card, as the other Settings sections draw their parts; the group's name is the card's only heading.
- **SM-SF-17 (amended in place).** The Kinds of fact tab is a table, one row per kind of fact; **Rename** (`memory-word-rename`, `i-edit`) and **Merge** (`memory-word-merge`, `i-fold`) are small icon buttons at the row's end, replacing the row's ⋯ menu.
- **SM-SF-27 (amended in place).** Things are a table, one row per thing; **Change topic…** (`memory-topic-change`, `i-edit`) is a small icon button at the row's end, replacing the thing's ⋯ menu. Item 2, *Kind of fact*: facts are grouped by the fact's word (its triple's predicate, named by the predicate's label, as on the Kinds of fact tab); a fact with no triple falls back to its memory class, named by `KIND_MESSAGE`.
- **Knowledge screen, step 9 item 3 (amended in place).** In **Edit**, **Done** (`edit-done`, `i-check`, the default) and **Cancel** (`edit-cancel`, `i-remove`) are small icon buttons at the top of the editor, and the icon button that shows and hides the page's details (`knowledge-details-show` / `knowledge-details-hide`) sits beside the title field.

### Strings (added; en-GB)

| Message id | Text | Spec |
|---|---|---|
| `diagnostics-support-nothing` | There is nothing to save yet. | SM-NS-3 |
| `diagnostics-col-reference` | Reference | SM-NS-3 |
| `diagnostics-col-reference-hint` | A short code for this event. Quote it to support so they can find it. | SM-NS-3 |
| `installed-files-tree` | The files of { $agent } | SM-NS-11 |
| `installed-files-choose` | Choose a file to read it. | SM-NS-11 |
| `installed-files-open-all` | Open all folders | SM-NS-11 |
| `installed-files-close-all` | Close all folders | SM-NS-11 |
| `installed-files-copy` | Copy the text | SM-NS-11 |
| `installed-files-copied` | Copied | SM-NS-11 |
| `installed-files-cut` | Only the start of this file is shown. | SM-NS-11 |
| `installed-files-details` | Details | SM-NS-11 |

Changed: `settings-section-about` and `about-title` read "Updates"; `installed-files-title` reads "What { $agent } is made of"; `diagnostics-support-note` as in SM-NS-3. Withdrawn: `diagnostics-support-pick`, `diagnostics-support-limit`, `diagnostics-support-save`.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: One-press support save, Updates and the agent's files

  Scenario: Happy path — saving logs for support in one press (SM-NS-3)
    Given 25 runs on this computer
    When the person presses Save logs for support…
    Then diagnostics.support_export is sent with the newest 20 run ids

  Scenario: Edge — nothing to save (SM-NS-3)
    Given no runs on this computer
    When the person presses Save logs for support…
    Then "There is nothing to save yet." is shown and nothing is sent

  Scenario: Happy path — the release notes are in Updates (SM-UP-2)
    When the person opens Settings › Updates
    Then this version's release notes are drawn below Check for updates, with no What's new button

  Scenario: Happy path — reading an agent's Markdown file (SM-NS-11)
    Given What the agent is made of is open
    When the person chooses skills/a.md in the tree
    Then agents.installed_file is sent and the file is drawn formatted

  Scenario: Happy path — removing a dashboard closes its tab (SM-OS-3)
    Given the dashboard "Prices" is open on its tab
    When the person removes "Prices" from the side tree
    Then the tab "Prices" closes and the neighbouring tab comes to the front

  Scenario: Edge — an item removed elsewhere (SM-OS-3)
    Given the agent "Reviewer" is open on its tab
    When the side tree reads its agents again and "Reviewer" is not among them
    Then the tab "Reviewer" closes

  Scenario: Happy path — a search result opens the agent's page (SM-OS-2)
    When the person chooses an agent in Search or go to…
    Then the agent's page opens with the agent's name as its tab title

  Scenario: Edge — a fact with no triple is grouped by its class (SM-SF-27)
    Given a fact written before facts were triples
    When the person groups by Kind of fact
    Then the fact is in the group named by its memory class
```

### Gaps

None.


## Amendment January 2026 — Settings › Agents and models (SM-AGM-1) (amended January 2026; user decision; Approved by the user January 2026)

- **SM-AGM-1 Settings › Agents and models (amended January 2026; user decision; Approved by the user January 2026).** A Settings section `agents-and-models` ("Agents and models"), listed after Using your plan, for the local backend only, when it answers `app.agent_settings` and `app.change_agent_setting` (`components/ServiceApi.md` SVC-AGM-1; SM-UP-1). It reads `app.agent_settings` when opened and sends one `app.change_agent_setting` per choice; each answer replaces the view. Drawn as Settings › Memory draws its groups, one card per group, in plain words that never name a harness, an adapter or IT:
  1. **What runs your agents.** A list **Run my agents with**: *Same as my organisation (<name>)* (the default), then each harness that is `ready` or `needs_set_up`, plus the one in use, the chosen one and the organisation's; a harness that is not `ready` carries its mark (*Needs setting up*, *Not found on this computer*, *Not available in this version yet*). Under it: *A change here starts the next time you open Ordin.* and *Your agents run with <name> now.*
  2. **Models for <name>**, per harness shown: one row per level (*Quick, everyday work* = smaller, *Most work* = standard, *The hardest work* = most capable), each a text box. The backend lists no models, so the box is empty for "use the default" and its placeholder is the level's `default_model` (else *Chosen by <name>*). A name is sent when the box changes; an empty box sends `model: null`. A name outside 1–100 letters, digits, dots, dashes and underscores is not sent and the row says so. A row with the person's own model has an icon button **Use the default** (icon `undo`, named and with a tooltip) that sends `model: null`.
  3. **Effort for <name>**, per harness shown: one list per kind of work (*Talking with you*, *Coordinating a crew*, *Routine steps*, *Working on documents*, *Working on code*), offering *Let <name> decide* (`null`) and the harness's `effort_levels` (Claude Code: Low, Medium, High, Very high, Highest). A harness with no effort control shows one line instead: *<name> doesn't offer an effort setting, so it decides for itself.*
  A refused change keeps the stored value; `DSK_PREFERENCES_WRITE_FAILED` offers **Try again**, which sends the same change. Words: `crates/ordin-localisation/catalogues/en-GB/screens-settings.ftl` (`settings-agents-*`, `settings-section-agents-and-models`).

```gherkin
Feature: Settings › Agents and models (SM-AGM-1)

  Scenario: Happy path — the three groups, with each default shown
    Given every choice is on its default
    When the person opens Settings › Agents and models
    Then they see What runs your agents, Models for Claude Code and Effort for Claude Code
    And the model boxes show haiku, sonnet and opus as placeholders

  Scenario: Happy path — a typed model is kept and Use the default clears it
    When the person types "claude-sonnet-4.5" for Most work
    Then app.change_agent_setting is sent with that model
    And Use the default sends model null

  Scenario: Edge case — a tool with no effort control
    Given Copilot is shown
    Then its effort card says it decides for itself and shows no list
```
- **SM-SW-NTF Settings ids that act (amended January 2026; user decision; Approved by the user January 2026).** **Notifications** (`settings-notify-approvals`, `settings-notify-finished-work`, `settings-notify-questions`) draws the switches `settings.get { section: notifications }` returns; the desktop program returns only the kinds it raises (OrdinDesktop.md DSK-NTF-1), so `settings-notify-draft-reviews` and `settings-notify-handovers` are not drawn while their notices are not raised. **Using your plan**: **−** (`settings-plan-agents-fewer`) is disabled while the number of agents at the same time is 1, the least it can be, as well as when the company set it. The dead-controls sweep (`apps/desktop/ui/tests/dead-controls.test.ts`) opens every Settings section on its own and clicks each control of the open section; a ticked radio button counts as the choice already made.

## Amendment January 2026 — A dashboard's menu: Refresh, Export and Remove (SM-DB-5 to SM-DB-8) (amended January 2026; user decision; Approved by the user January 2026)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). It adds to *Keep as dashboard* (SM-DB-1 to SM-DB-4); every other section stays in force. Owners: `components/OrdinDesktop.md` DSK-DBX-1 to DSK-DBX-4 (writes the files on this computer), `components/ServiceApi.md` SVC-DBX-1 (the two commands), `components/JournalAndArtefactStore.md` JA-DB-2 (the dashboards and reports read).
>
> Evidence read: `apps/desktop/ui/src/lib/shell/DashboardsTreeGroup.svelte` (a dashboard row had no menu), `apps/desktop/ui/src/lib/shell/AgentsTreeGroup.svelte` and `DocumentsTreeGroup.svelte` (the tree's ⋯ and right-click menu), `apps/desktop/ui/src/lib/views/publish/PublishToItems.svelte` (an item that opens to further items), `apps/desktop/ui/src/lib/views/fixed/Dashboards.svelte` (SM-DB-4).

- **SM-DB-5 Export (amended January 2026; user decision; Approved by the user January 2026).** A dashboard's menu holds **Export** (`dashboard-export`), drawn when the backend is this computer and answers `dashboards.export` and `dashboards.add_to_presentation`. Pressed, it opens under itself to **PDF…** (`dashboard-export-pdf`), **PowerPoint…** (`dashboard-export-pptx`), **Excel…** (`dashboard-export-xlsx`), **Word…** (`dashboard-export-docx`) and **Add to an existing PowerPoint…** (`dashboard-add-to-presentation`), as **Publish to…** opens to its destinations (SM-PUB-1). Choosing a format closes the menu and sends `dashboards.export { dashboard_id, format }` (`pdf`, `pptx`, `xlsx`, `docx`); the save dialog opens with the dashboard's name proposed. `ok` → `dashboard-exported` is announced. `DSK_DIALOG_CANCELLED` → nothing is drawn or announced. Any other error → its sentence under the menu's owner (the tree's Dashboards group, or the dashboard's heading).
- **SM-DB-6 Add to an existing PowerPoint… (amended January 2026; user decision; Approved by the user January 2026).** Sends `dashboards.add_to_presentation { dashboard_id }`; the file chooser opens. `ok` with `as_copy: false` → `dashboard-added-to-presentation` is announced; with `as_copy: true` (the presentation was open in PowerPoint or could not be replaced, so the person saved a copy) → `dashboard-added-as-copy`. Cancelling either dialog → nothing; an error → as SM-DB-5 (`DSK_PRESENTATION_UNREADABLE` for a file slides cannot be added to).
- **SM-DB-7 A dashboard row's menu in the side tree (amended January 2026; user decision; Approved by the user January 2026).** Each dashboard row under **Dashboards** (SM-DB-3) has a ⋯ icon button (`more-actions-for` as accessible name, `more-actions-tip` as tooltip) and opens the same menu on right-click, with the tree's menu markup and keys (Escape closes it, the first item takes focus). The menu holds, in this order: **Refresh** (`dashboard-refresh`, when the backend answers `dashboards.refresh`; sends `dashboards.refresh { workspace_id, dashboard_id }` and announces `dashboard-refresh-started`), **Export** (SM-DB-5) and **Remove…** (`dashboard-remove`, when the backend answers `dashboards.remove`, drawn as a removal). **Remove…** opens a `ConfirmDialog` titled `dashboard-remove-title`, body `dashboard-remove-body`, destructive verb **Remove** (`dashboard-remove-verb`); **Cancel** sends nothing; **Remove** sends `dashboards.remove { dashboard_id }`, and on `ok` the dialog closes, the row goes and `dashboard-removed` is announced; an error → its sentence in the dialog, which stays open. With none of the three answered, no ⋯ is drawn and right-click does nothing.
- **SM-DB-8 The dashboard screen's ⋯ menu (amended January 2026; user decision; Approved by the user January 2026).** On `dashboards:<dashboard id>` (SM-DB-4) a ⋯ icon button sits after **Refresh**, drawn when SM-DB-5's **Export** is; its menu holds **Export** with the same items and behaviour. **Refresh** stays a small icon button.
- **SM-DB-9 Messages (en-GB, `screens-fixed.ftl`; amended January 2026; user decision; Approved by the user January 2026).** `dashboard-export`, `dashboard-export-pdf`, `dashboard-export-pptx`, `dashboard-export-xlsx`, `dashboard-export-docx`, `dashboard-add-to-presentation`, `dashboard-exported`, `dashboard-added-to-presentation`, `dashboard-added-as-copy`, `dashboard-remove`, `dashboard-remove-title`, `dashboard-remove-body`, `dashboard-remove-verb`, `dashboard-removed`; the program's own `dashboard-export-figure`, `dashboard-export-value`, `dashboard-export-unit`, `dashboard-export-pick-presentation` and `dashboard-export-copy-name` (DSK-DBX-1, DSK-DBX-2). No message names anyone to ask; everything is done on this computer.

```gherkin
Feature: A dashboard's menu (SM-DB-5 to SM-DB-8)

  Scenario: Happy path — saving a dashboard as a PowerPoint from the tree (SM-DB-5, SM-DB-7)
    Given the dashboard "Prices" is under Dashboards
    When the person chooses ⋯ › Export › PowerPoint… and saves
    Then dashboards.export is sent with format pptx
    And "Prices was saved." is announced

  Scenario: Happy path — adding the dashboard to a presentation (SM-DB-6)
    When the person chooses Export › Add to an existing PowerPoint… and picks Board pack.pptx
    Then dashboards.add_to_presentation is sent and the new slides are at the end of Board pack.pptx

  Scenario: Edge case — the presentation is open in PowerPoint (SM-DB-6)
    When the program saves a copy instead
    Then the announcement says a copy was saved

  Scenario: Edge case — the save dialog is cancelled (SM-DB-5)
    Then nothing is announced and no error is drawn

  Scenario: Happy path — removing a dashboard asks first (SM-DB-7)
    When the person chooses ⋯ › Remove… and presses Remove
    Then dashboards.remove is sent and the row goes

  Scenario: Error path — a dashboard with no report yet (SM-DB-5)
    Then the sentence of DSK_DASHBOARD_NO_REPORT is drawn

  Scenario: Happy path — the dashboard screen's ⋯ (SM-DB-8)
    When the person chooses ⋯ › Export › Word… on the dashboard's screen
    Then dashboards.export is sent with format docx
```

### Gaps

None.

## Amendment January 2026 — Meeting minutes (SM-MIN-1 to SM-MIN-10) (amended January 2026; user decision; Approved by the user January 2026)

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Every section above stays in force. Commands: `components/ServiceApi.md` SVC-MIN-1 to SVC-MIN-4. Owners: `components/MinutesService.md`, `components/OrdinDesktop.md` DSK-MIN-2 and DSK-MIN-4.
>
> Evidence read: `apps/desktop/ui/src/lib/shell/WorkspaceTree.svelte` (`DashboardsTreeGroup` imported :21 and drawn :161; `DocumentsTreeGroup` :22, drawn :229 behind `answers(backend, 'files.list_folder')`); `apps/desktop/ui/src/lib/editor/MarkdownEditor.svelte` and `setup.ts`; this file's SM-UP-1, SM-DB-3, SM-DB-4, SM-AGM-1.

The rules of the earlier amendments hold: the sentence, never the code; every action is a visible, named button where the person is looking (a menu may repeat it, never hold it alone); text from a transcript or the minutes is drawn through Svelte text interpolation only; the words are plain, with no technical term. Every entry below passes SM-UP-1 (drawn only when the backend answers its commands), so a build without `minutes` draws none of it.

1. **SM-MIN-1 Minutes in the side tree.** Under each workspace, after **Dashboards**, a **Minutes** row (`minutes-tree`) drawn when the backend answers `minutes.list`. It opens the Minutes screen `minutes-home:<workspace id>` and shows under it one row per minutes of `minutes.list { workspace_id }` (title, newest first; read when it opens and on each `workspace_changed`); a row opens `minutes:<minutes id>` with the title as tab title. A `MinutesTreeGroup.svelte` sits beside `DashboardsTreeGroup.svelte`.
2. **SM-MIN-2 The Minutes screen.** Heading **Minutes** (`minutes-heading`) with two buttons always visible in its header: **Make minutes from a file…** (`minutes-import`) and **Record** (`minutes-record`, primary). Under them, the recorder line (SM-MIN-6), then **In progress** — one row per job of `minutes.jobs` that is not `done` or `cancelled`, with its file name, a plain stage line (`minutes-stage-<state>`: "Waiting to start", "Waiting for the speech model", "Downloading the speech model — 420 of 1,533 MB", "Reading the transcript", "Turning the recording into text — 12 of 60 minutes", "Waiting for your OK", "Ordin is writing the minutes"), a progress bar where the state has numbers, **Cancel** (`minutes-cancel`) and, when `failed`, the code's sentence with **Try again** (`minutes-retry`) — then the list of minutes as in the tree. The screen reads `minutes.status` and `minutes.jobs` when it opens and every 2 seconds while a job is in progress.
3. **SM-MIN-3 Asking before the speech model downloads.** A job in `waiting_for_model` while `model_consent_given` is false opens a `ConfirmDialog` titled `minutes-model-title` ("Download Ordin's speech model?"), body `minutes-model-body`: "To turn recordings into text on this computer, Ordin needs a speech model of { $size } MB. It is saved in { $folder } and used only on this computer. Transcript files work without it.", verbs **Download** (`minutes-model-download`) and **Not now** (`minutes-model-not-now`). **Download** sends `minutes.give_model_consent`; **Not now** sends `minutes.cancel_job` for that job. `$size` and `$folder` come from `MinutesStatus.model`.
4. **SM-MIN-4 The one-time notice.** A job in `waiting_for_notice` opens a `ConfirmDialog` titled `minutes-notice-title` ("Before Ordin writes minutes"), body `minutes-notice-body`: "Ordin sends the text of the meeting — never the recording — to { $service } to write the minutes. This is the AI service your agents already use.", verbs **Write the minutes** (`minutes-notice-accept`) → `minutes.accept_notice`, and **Not now** → `minutes.cancel_job`. Shown once; never again after it was accepted.
5. **SM-MIN-5 Record and the reminder.** **Record** is enabled only when `MinutesStatus.recorder.state` is `ready`. Each press opens a `ConfirmDialog` titled `minutes-record-title` ("Tell everyone you're recording"), body `minutes-record-body`: "Let everyone in the meeting know that you are recording before you start.", verbs **Start recording** (`minutes-record-start`) → `minutes.record { workspace_id }`, and **Cancel**. The dialog shows on every press; there is no "don't show again". With `others_heard: false` the body adds `minutes-record-mic-only`: "Only your microphone will be recorded. Update OBS to version 30 or later to record what others say."
6. **SM-MIN-6 The recorder line.** From `MinutesStatus.recorder`, one sentence and its buttons, each state distinct: `not_installed` → `error-obs-not-installed` with **Get OBS** (`minutes-get-obs`, `app.open_web_page { page: obs_download }`); `not_reachable` → `error-obs-not-reachable` with **Open OBS** (`minutes-open-obs`, `minutes.open_obs`) and **Try again**; `password_needed` or `password_refused` → its sentence with a password field **OBS password** (`minutes-obs-password`, masked) and **Save** (`minutes.save_obs_password`), then a fresh `minutes.status`; `version_unsupported` → `error-obs-version-unsupported`; `ready` → "OBS is ready to record." (`minutes-obs-ready`); `recording` → the indicator's words. **Record** stays disabled in every state but `ready`.
7. **SM-MIN-7 The recording indicator.** While `MinutesStatus.recording` is set — from the `minutes.record` answer to the `minutes.stop` answer — the app header on every screen shows a red dot, "Recording — 12:04" (`minutes-recording`, the minutes and seconds since `started_at`, updated each second, announced to screen readers once at start and once at stop) and a **Stop** button (`minutes-stop`) → `minutes.stop { recording_id }`. The shell reads `minutes.status` every 5 seconds while the indicator shows, so a recording stopped in OBS ends the indicator. **Stop** needs no other input: the job appears under **In progress** and the minutes arrive when ready, announced with `minutes-ready` ("Minutes of { $title } are ready.").
8. **SM-MIN-8 A minutes page.** `minutes:<id>` reads `minutes.get` and opens the body in `MarkdownEditor.svelte` with the title as heading and the line `minutes-kept-on` ("Made from { $source } on { $date }"). Header buttons, always visible: **Save** (`minutes-save`, enabled after a change) → `minutes.save_edit { minutes_id, body, base_revision }`, `MIN_EDIT_CONFLICT` shown with **Reopen**; **Export** (`minutes-export`) opening to **PDF…** and **Word…** → `minutes.export`; **Show transcript** (`minutes-show-transcript`) → `minutes.transcript`, drawn below as speaker and text lines with their times; **Delete recording…** (`minutes-delete-recording`, only when `recording` is set; asks first: "Delete the recording? The minutes and the transcript stay.") → `minutes.delete_recording`; **Remove…** (`minutes-remove`; asks first) → `minutes.remove`. A ⋯ menu on the tree row repeats Export and Remove.
9. **SM-MIN-9 Settings › Meeting minutes.** A Settings section `meeting-minutes` ("Meeting minutes"), local backend only, when the backend answers `minutes.status` and `minutes.change_settings`. Cards: **Speech model** — *Larger — more accurate, about 1.5 GB* (default) and *Smaller — faster, about 470 MB*, with the chosen model's state ("Downloaded" with **Remove the speech model** → `minutes.remove_model`, or "Not downloaded yet"); **Recordings** — the switch *Delete recordings after the minutes are saved* (off by default) and the line "Recordings are kept in Ordin's folder on this computer until you delete them."; **OBS** — the port (default 4455, 1024–65535, `MIN_SETTING_INVALID` shown under the field) and the password field of SM-MIN-6. Each change sends one `minutes.change_settings`; its answer replaces the view.
10. **SM-MIN-10 Messages.** en-GB in a new catalogue `screens-minutes.ftl` (ids `minutes-*` above, `settings-section-meeting-minutes`), and the error sentences of `errors-minutes.ftl`, `errors-transcription.ftl` and `errors-obs.ftl`. OBS menu names in sentences are OBS's own labels, quoted as OBS shows them.

```gherkin
Feature: Meeting minutes on the screens (SM-MIN-1 to SM-MIN-10)

  Scenario: Happy path — minutes from a transcript appear under Minutes
    Given the backend answers the minutes group and the notice was accepted
    When the person presses Make minutes from a file… and picks "meeting.vtt"
    Then a job shows "Reading the transcript", then "Ordin is writing the minutes"
    And a row "Supplier call" appears under Minutes in the tree and opens in the editor

  Scenario: Edge — the consent prompt names size and folder
    Given no speech model and no consent
    When the person picks "speech-60s.m4a"
    Then the dialog names the size in MB and the folder
    And pressing Not now cancels the job and downloads nothing

  Scenario: Happy path — record and stop
    Given OBS is ready
    When the person presses Record
    Then the reminder dialog shows
    When the person presses Start recording
    Then the header shows "Recording" with Stop on every screen until Stop is answered
    When the person presses Stop
    Then a job appears under In progress with no other question

  Scenario: Edge — the reminder on every press
    When the person presses Record three times, cancelling each time
    Then the reminder dialog shows three times

  Scenario: Error — three distinct OBS problems
    Given OBS is not installed, or not reachable, or refuses the password
    Then the recorder line shows Get OBS, or Open OBS with the menu path, or the password field respectively
    And Record is disabled

  Scenario: Edge — a build without the switch
    Given the backend does not answer the minutes group
    Then no Minutes row, no Record button, no recording indicator and no Meeting minutes settings are drawn

  Scenario: Happy path — edits persist and export
    When the person edits the minutes, presses Save, closes and reopens them
    Then the edit is there
    When the person presses Export › Word…
    Then minutes.export is sent with format docx
```

### Gaps

None.

## Amendment 2026-11-21 — Text for Ordin in Minutes › In progress (SM-MIN-11)

> **Status:** Approved by the user, 2026-11-21 ("approve as written"). Every section above stays in force. Source: MinutesService.md MIN-TR-1 to MIN-TR-5 (jobs of purpose `transcript` started by Ordin's `transcribe_recording`).

- **SM-MIN-11 A job that turns a recording into text for Ordin.** In SM-MIN-2's **In progress**, a job of purpose `transcript` shows its file name followed by " — text for Ordin" (`minutes-job-for-ordin`), with the same stage lines, progress, **Cancel** and **Try again**, so the person sees and can stop work Ordin started, and the speech-model question (SM-MIN-3) and the one-time notice (SM-MIN-4) open for it exactly as for minutes. `transcript_ready` counts as finished: the row leaves **In progress** like `done`, nothing is announced, and no minutes row appears. The 2-second reading of SM-MIN-2 treats `transcript_ready` as not moving. en-GB: `minutes-job-for-ordin` = "text for Ordin"; `minutes-stage-transcript-ready` = "Ready for Ordin" (used by Ordin's `list_minutes_jobs`, OrdinHost.md OH-MIN-7).

## Amendment 2026-11-22 — The speech model chosen by the machine (SM-MIN-12)

> **Status:** Approved by the user, 2026-11-22 ("approve as written"). Every section above stays in force except where this section replaces it. Source: MinutesService.md MIN-MACH-1 to MIN-MACH-3; 2C wire WIRE-MIN-8.

- **SM-MIN-12 The Speech model card.** Replaces the two options of SM-MIN-9's **Speech model** card with three, in this order: *Recommended for this computer* (`minutes-model-recommended`, the default) with the line "{ $model } — the one this computer handles well" (`minutes-model-recommended-line`, `$model` the localised name of `MinutesStatus.recommended_model`: `minutes-model-name-larger` "Larger, about 1.5 GB" or `minutes-model-name-smaller` "Smaller, about 470 MB"); *Larger — more accurate, about 1.5 GB*; *Smaller — faster, about 470 MB*. When the person picks *Larger* while `recommended_model` is `smaller`, the line `minutes-model-larger-slow` shows under the card: "On this computer the larger model may take more than an hour for each hour of meeting." The chosen model's state line ("Downloaded" with **Remove the speech model**, or "Not downloaded yet") describes `MinutesStatus.model`, the model in use. Each pick sends one `minutes.change_settings` with `speech_model` `recommended`, `larger` or `smaller`. SM-MIN-3's consent prompt is unchanged: `$size` comes from `MinutesStatus.model`.

```gherkin
Feature: The speech model chosen by the machine (SM-MIN-12)

  Scenario: The recommendation is the default and names the model
    Given minutes.status answers speech_model "recommended" and recommended_model "smaller"
    When Settings › Meeting minutes opens
    Then "Recommended for this computer" is selected with the line "Smaller, about 470 MB — the one this computer handles well"

  Scenario: Picking the larger model on a computer it does not suit
    Given recommended_model is "smaller"
    When the person picks "Larger — more accurate, about 1.5 GB"
    Then one minutes.change_settings is sent with speech_model "larger"
    And the line "On this computer the larger model may take more than an hour for each hour of meeting." shows

## Amendment 2026-11-22 — Minutes follow the app's menus and icon buttons (SM-MIN-13)

> **Status:** Approved by the user, 2026-11-22 ("go with A"). Every section above stays in force except where this section replaces it. Source: the Phase 12 smoke test of plan 07 — the user: "the new minutes feature does not align with the UX approach of the rest of the system because it does not make proper use of context menus and icon-buttons". The pattern followed is a dashboard's (SM-DB-5, SM-DB-7, SM-DB-8): actions on an item live in its ⋯ menu, also opened by right-click; a page header holds small icon buttons and a ⋯.
>
> Evidence read: `apps/desktop/ui/src/lib/views/minutes/MinutesPage.svelte` (five text buttons in the header), `apps/desktop/ui/src/lib/shell/MinutesTreeGroup.svelte` (a row menu of Export and Remove only), the Minutes screen of that time, since removed by SM-MIN-19 (list rows with no menu, text **Cancel** and **Try again**), `apps/desktop/ui/src/lib/views/fixed/Dashboards.svelte` and `DashboardExportItems.svelte` (the pattern).

- **SM-MIN-13 The minutes menu.** One menu serves a minutes item wherever it is drawn: the tree row (SM-MIN-1), the row of the Minutes screen's list (SM-MIN-2) and the minutes page (SM-MIN-8). It is opened by a ⋯ icon button (`more-actions-for` as accessible name, `more-actions-tip` as tooltip) and, on a row, also by right-click, with the tree's menu markup and keys (Escape closes it, the first item takes focus). It holds, in this order:
  1. **Export** (`minutes-export`, icon `save`), opening under itself to **PDF…** (`minutes-export-pdf`, icon `pdf`) and **Word…** (`minutes-export-word`, icon `word`), as a dashboard's Export opens (SM-DB-5); a choice closes the menu and sends `minutes.export` as SM-MIN-8 says.
  2. **Delete recording…** (`minutes-delete-recording`, icon `bin`), on the minutes page only and only while `Minutes.recording` is set (a row's `MinutesSummary` does not carry it), asking first as SM-MIN-8 says.
  3. **Remove…** (`minutes-remove`, icon `remove`, drawn as a removal), asking first as SM-MIN-8 says.
- **SM-MIN-14 The minutes page header.** Replaces SM-MIN-8's header buttons. Icon buttons are drawn as on the knowledge page (`Knowledge.svelte`: class `icon-btn`, small icon, 1.75 rem square). After the title: **Save** as an icon button (icon `save`, `minutes-save` as accessible name and tooltip, enabled after a change), **Show transcript** as a pressed-state icon button (icon `eye`, `minutes-show-transcript` as accessible name and tooltip, `aria-pressed` while the transcript shows), then the ⋯ of SM-MIN-13. **Reopen** after `MIN_EDIT_CONFLICT` stays a text button beside the error.
- **SM-MIN-15 The Minutes screen.** Replaces parts of SM-MIN-2. Header: **Record** stays a text button, the primary one and the screen's one main action (UX guidelines §4, *Actions in menus, content in panes*); **Make minutes from a file…** (`minutes-import`, icon `plus`) moves into the screen's ⋯ menu (`more-actions-for` with the heading as name) and into the ⋯ and right-click menu of the **Minutes** tree row, as **Import workflow…** sits on the Workflows group. From the tree row, a started import opens the Minutes screen, where its job shows. Each kept-minutes row has the ⋯ and right-click menu of SM-MIN-13. Each job row's **Cancel** becomes an icon button (icon `stop`, `minutes-cancel` as accessible name and tooltip) and **Try again** an icon button (icon `play`, `minutes-retry` as accessible name and tooltip). The recorder line's buttons (SM-MIN-6) stay text buttons: they are the one next step of a sentence.
- **SM-MIN-16 Messages.** None new; the names above are SM-MIN-10's.

```gherkin
Feature: Minutes follow the app's menus and icon buttons (SM-MIN-13 to SM-MIN-15)

  Scenario: The page header holds icon buttons and a menu
    Given a minutes page with a recording
    Then its header shows the icon buttons "Save" and "Show transcript" and a ⋯ button
    And the ⋯ menu holds Export, Delete recording… and Remove…, in this order

  Scenario: A row's menu opens by right-click
    Given the Minutes screen lists "Supplier call"
    When the person right-clicks the row
    Then the menu holds Export and Remove…, and Export opens to PDF… and Word…

  Scenario: A job is cancelled with an icon button
    Given a job is in progress
    Then its row shows the icon button "Cancel" and, when it failed, the icon button "Try again"

  Scenario: Making minutes from a file is in the menus
    Given the Minutes screen is open
    Then its header shows "Record" and a ⋯ button, and no "Make minutes from a file…" button
    And the screen's ⋯ menu and the Minutes tree row's menu each hold "Make minutes from a file…"
```

## Amendment 2026-11-22 — The agent's files in two panes (SM-IF-1 to SM-IF-4)

> **Status:** Approved by the user, 2026-11-22 ("go with A for the agent files screen"). Every section above stays in force except where this section replaces it. Source: the user — "in what (agent) is made of treeview, render selected content artefacts from treeview in separate pane". Evidence read: `apps/desktop/ui/src/lib/views/parts/InstalledFilesView.svelte` — below 40 rem of screen width, the reader of SM-NS-11 dropped under the tree, so with the conversation panel open a chosen file was drawn further down the same page. Checked against the UX guidelines: §4 *Actions in menus, content in panes* (the tree and the file are content; the two buttons below are controls of that content and stay with it, as a Markdown document's **Edit** does), and the icon buttons are drawn as the screen's existing **Open all folders** and **Close all folders** (`.md-tool`).

- **SM-IF-1 Two panes at every width.** Replaces SM-NS-11's layout: the tree of files and the reader of the chosen file sit side by side at every width of the screen area. From 40 rem the tree column is 12–18 rem wide; below it, from 8 rem up to 40 % of the width. The reader never moves under the tree.
- **SM-IF-2 Hide the files.** The tree's toolbar begins with the icon button **Hide the files** (`installed-files-hide-tree`, icon `back`, its words as accessible name and tooltip), which hides the tree so the reader has the whole width; the reader then begins with **Show the files** (`installed-files-show-tree`, icon `folder`), which brings the tree back as it was. The choice lasts while the screen is open.
- **SM-IF-3 Each pane scrolls on its own.** The tree stays in view while the reader scrolls a long file; a long tree scrolls inside its own pane.
- **SM-IF-4 Messages** (en-GB, `screens-fixed.ftl`): `installed-files-hide-tree` = "Hide the files", `installed-files-show-tree` = "Show the files".

```gherkin
Feature: The agent's files in two panes (SM-IF-1 to SM-IF-3)

  Scenario: A chosen file is read beside the tree on a narrow screen
    Given What Release planner is made of is open in a screen area 35 rem wide
    When the person chooses "rules/release.md" in the tree
    Then the file is drawn in the reader beside the tree, not below it

  Scenario: Hiding the files gives the reader the whole width
    Given a file is open in the reader
    When the person presses "Hide the files"
    Then the tree is not drawn, the file stays open, and "Show the files" brings the tree back
```

## Amendment 2026-11-22 — Minutes live in the tree; no Minutes screen (SM-MIN-17 to SM-MIN-22)

> **Status:** Approved by the user, 2026-11-22 ("go with the recommendation"). Every section above stays in force except where this section replaces it. Source: the Phase 12 smoke test of plan 07 — the user: "the central pane is meaningless, minutes already show in the treeview, why repeating in the central pane? also record button can be moved to the treeview context menu". Checked against the UX guidelines: §4 *Actions in menus, content in panes* (actions on a node live in its ⋯ and right-click menu), UX-A14 (tree rows and their menus), UX-A36 (every decision waits in *Needs you*), UX-A87 (icon buttons).

- **SM-MIN-17 No Minutes screen.** The screen `minutes-home:<workspace id>` of SM-MIN-2 is removed, with SM-MIN-15's header and list. The minutes page (SM-MIN-8, SM-MIN-14) and Settings › Meeting minutes (SM-MIN-9, SM-MIN-12) stay.
- **SM-MIN-18 The Minutes row.** Replaces SM-MIN-1's main action: choosing **Minutes** (`minutes-tree`) unfolds or folds it and opens no screen. Its ⋯ and right-click menu holds, in this order, **Record…** (`minutes-record-menu`, icon `mic`; not drawn while a recording runs, when the header's **Stop** of SM-MIN-7 is the way out) and **Make minutes from a file…** (`minutes-import`, icon `plus`; the program's file dialog, then the row unfolds to show the job). Under the row, when unfolded: the jobs of SM-MIN-19, then one row per kept minutes as SM-MIN-1 and SM-MIN-13 say.
- **SM-MIN-19 Jobs in the tree.** Replaces SM-MIN-2's **In progress**. Each job not `done`, `transcript_ready` or `cancelled` is a row under **Minutes**, before the kept minutes: icon `mic` for a recording or an audio file and `page` for a transcript file; its file name (with " — text for Ordin" for a job of purpose `transcript`, SM-MIN-11); and, after it, its stage line of SM-MIN-2 in the secondary colour (a failed job: the mark `info` and "Couldn't finish", `minutes-job-failed`, with the code's sentence as the row's tooltip). The row's ⋯ and right-click menu holds **Cancel** (`minutes-cancel`, icon `stop`) while it has not finished, and **Try again** (`minutes-retry`, icon `play`) when it failed; a failed job's menu begins with its sentence as a line that is not an item. The rows are read when the row is drawn, on each `workspace_changed`, when a recording ends, and every 2 seconds while a job moves, as SM-MIN-2 said for the screen; `minutes-ready` is announced as SM-MIN-7 says.
- **SM-MIN-20 Record… and OBS.** Replaces SM-MIN-5's **Record** button and SM-MIN-6's recorder line. **Record…** reads `minutes.status` and opens one dialog. With `recorder.state` `ready` it is SM-MIN-5's reminder, unchanged. In any other state it is titled `minutes-obs-not-ready-title` ("OBS isn't ready to record") and holds SM-MIN-6's sentence for that state with its controls — **Get OBS**, **Open OBS**, or the password field with **Save** — and the verb **Try again** (`minutes-retry`), which reads `minutes.status` again; once it reads `ready`, the same dialog becomes the reminder. **Cancel** closes it.
- **SM-MIN-21 The two questions wait in *Needs you*.** Replaces SM-MIN-3's and SM-MIN-4's opening. A job in `waiting_for_model` while `model_consent_given` is false adds a line to *Needs you*, "Download the speech model for { $file }?" (`minutes-needs-model`), and a job in `waiting_for_notice` while `writer_notice_seen` is false the line "Before Ordin writes minutes of { $file }" (`minutes-needs-notice`), each with **Open**, which opens SM-MIN-3's or SM-MIN-4's dialog, unchanged. The line goes once the dialog is answered. Lines come from the workspaces whose **Minutes** row is drawn in the tree.
- **SM-MIN-22 Messages** (en-GB, `screens-minutes.ftl`): `minutes-record-menu` = "Record…", `minutes-job-failed` = "Couldn't finish", `minutes-obs-not-ready-title` = "OBS isn't ready to record", `minutes-needs-model` = "Download the speech model for { $file }?", `minutes-needs-notice` = "Before Ordin writes minutes of { $file }". `minutes-in-progress`, `minutes-empty`, `minutes-obs-ready` and `minutes-recording-line` are no longer drawn and are removed; `minutes-heading` stays as the minutes page's tab title.

```gherkin
Feature: Minutes live in the tree (SM-MIN-17 to SM-MIN-21)

  Scenario: The Minutes row opens no screen and its menu records
    Given the workspace shows Minutes in the tree and OBS is ready
    When the person chooses Minutes
    Then it unfolds and no screen opens
    When the person right-clicks Minutes and chooses Record…
    Then the reminder "Tell everyone you're recording" opens

  Scenario: OBS not ready
    Given OBS is not reachable
    When the person chooses Record…
    Then the dialog "OBS isn't ready to record" shows the not-reachable sentence with Open OBS and Try again

  Scenario: A job in the tree
    Given a recording is being turned into text, 12 of 60 minutes done
    Then a row under Minutes shows its file name and "Turning the recording into text — 12 of 60 minutes"
    And its menu holds Cancel

  Scenario: The speech model question waits in Needs you
    Given a job waits for the speech model and no consent was given
    Then Needs you shows "Download the speech model for meeting.m4a?" with Open
    When the person chooses Open and Download
    Then minutes.give_model_consent is sent and the line goes
```


## Amendment 2026-02-02 — Copilot's own words when it is not installed (SM-CPX-1)

> **Status:** Approved by the user, 2026-02-02 ("approve, and allow Copilot without a company profile"). Source: SetupAssistant.md SET-CPX-4; HLD ADR-123. **derivedFromHld:** 0.73.0. Every section above stays in force.

- **SM-CPX-1** The words table gains `setup-harness-missing-copilot` "GitHub Copilot isn't on this computer yet. Install it, then choose Check again." and `setup-harness-untested-copilot` "This version of GitHub Copilot hasn't been checked with ordin yet. Install a checked version, then try again." `FirstStart` draws them as any failed `CheckResult` (SM-FS-5) with **Check again** only: there is no **Install GitHub Copilot** button, as SM-FS-9 already draws none for Copilot. Settings › Agents and models keeps its marks unchanged (SM-AGM-1: *Not found on this computer*, *Needs setting up*), so Copilot is offered to run agents only when it is `ready`.

```gherkin
  Scenario: Error path — GitHub Copilot is not installed (SM-CPX-1)
    Given setup.run_guided answers ready false with a failed result SETUP_HARNESS_MISSING for copilot_cli with message id setup-harness-missing-copilot
    Then FirstStart shows "GitHub Copilot isn't on this computer yet. Install it, then choose Check again."
    And the button "Check again" and no install button

  Scenario: Edge case — Copilot not found is not offered to run agents (SM-CPX-1, SM-AGM-1)
    Given app.agent_settings lists copilot_cli with state not_found
    When Settings › Agents and models is opened
    Then GitHub Copilot is listed with the mark "Not found on this computer"
```

### Gaps

None.

## Amendment 2026-02-04 — Which assistant, asked at first start (SM-FS-10 to SM-FS-12)

> **Status:** Approved by the user, 2026-02-04 ("approve as written"). Source: RFC 17 acceptance criteria 1 and 8; SetupAssistant.md SET-CHO-3; OrdinDesktop.md DSK-CHO-2; 11 §3 (button words), §4 (one main action), §5.1 *Getting ready*, UX-A14. **derivedFromHld:** 0.75.0. This amendment adds a part to `FirstStart` (`apps/desktop/ui/src/lib/views/setup/FirstStart.svelte`) and no new screen.

- **SM-FS-10 The question.** While a step `harness_choice` is in state `needs_you`, `FirstStart` keeps its frame, brand tile, heading and step rows (SM-FS-2). In place of the sign-in button of SM-FS-3 it draws:
  - the question `setup-choose-heading`, followed by `setup-choose-note` in small muted type;
  - a group of choices (a radio group, with `setup-choose-heading` as its accessible name), one row per offered attachment from the latest `setup.status` answer (offered: state `ready` or `needs_sign_in`, SET-CHO-3), with Claude Code first. Each row shows the harness's name (`harness-claude-code` or `harness-copilot`) in bold, and under it, in small muted type, `setup-choose-signed-in` when the attachment is `ready` or `setup-choose-sign-in-next` when it is `needs_sign_in`. No row is chosen at the start: ordin does not steer the person towards either assistant;
  - one primary button, the screen's one main action (11 §4). With nothing chosen it reads `setup-choose-pick-first` and is disabled. Once a row is chosen it reads **Use { $harness }** (`setup-choose-use`, 11 §3: the button says what happens).

  Pressing it disables the button and sends `setup.choose_harness { harness }`. Keyboard: the arrow keys move between choices, and Enter on a chosen row presses the button.
- **SM-FS-11 The answer.**
  - `restarting: false` → the question is removed, and the step rows go on as the guided run's `setup_progress` events arrive (SM-FS-2 to SM-FS-4).
  - `restarting: true` → the question and the button are removed, and `setup-choose-restarting` with `$harness` is drawn under the steps. Nothing else can be pressed. The window closes when the program restarts (DSK-CHO-2), and first start is drawn again by SM-FS-1.
  - An error answer → drawn by step 11 under the button, which is enabled again. `SETUP_CHOICE_NOT_SAVED` keeps the person's row chosen, so pressing the button again sends the same choice. `SETUP_HARNESS_NOT_OFFERED` and `SETUP_NO_CHOICE_PENDING` also send `setup.status {}` and redraw from its answer.
- **SM-FS-12 Words.** SM-FS-5's catalogue table gains these rows (en-GB, `screens-setup.ftl`):

| Message id | en-GB |
|---|---|
| `setup-step-harness-choice` | Choosing your assistant |
| `setup-choose-heading` | Which assistant should ordin use? |
| `setup-choose-note` | Both are on this computer. You can change this later in Settings › Agents and models. |
| `setup-choose-signed-in` | Signed in |
| `setup-choose-sign-in-next` | You'll sign in next |
| `setup-choose-pick-first` | Choose an assistant |
| `setup-choose-use` | Use { $harness } |
| `setup-choose-restarting` | Starting ordin again with { $harness }… |
| `setup-no-choice-pending` | That question has already been answered. |
| `setup-harness-not-offered` | That assistant isn't available any more. Choose again. |
| `setup-choice-not-saved` | ordin couldn't save your choice. Try again. |

Every sentence says what the person can do (SetupAssistant.md SET-R1-9); none mentions IT, accounts or passwords (SM-FS-2).

- **SM-FS-13 Which assistant the words name** *(amended 2026-02-05, found in the 0.2.1-test.3 smoke test: after choosing Copilot and the restart, first start named Claude Code and signed in to it, while the guided run waited for Copilot; approved by the user 2026-02-05)*. When `setup.status` says `needs_guided`, `FirstStart` also sends `app.agent_settings {}` once and keeps `harness_in_use` (`copilot` → `copilot_cli`). The harness SM-FS-2 and SM-FS-3 name, and the one `setup.sign_in_harness` is sent for, is the chosen row once the question is answered, else that harness in use when the `setup.status` answer holds an attachment for it (a default `claude_code` with only Copilot installed has none), else the first attachment of the `setup.status` answer as before. When the question appears during the guided run, `setup.status {}` is sent again once and its attachments give the rows of SM-FS-10, because the assistants are looked for after the first answer.

```gherkin
Feature: Which assistant, asked at first start (SM-FS-10 to SM-FS-12)

  Scenario: Happy path — the person picks Copilot (SM-FS-10, SM-FS-11)
    Given first start with step harness_choice needs_you and attachments claude_code ready and copilot_cli ready
    Then the question "Which assistant should ordin use?" offers Claude Code and GitHub Copilot, each "Signed in", none chosen
    And the main button reads "Choose an assistant" and is disabled
    When the person chooses GitHub Copilot
    Then the main button reads "Use GitHub Copilot"
    When the person presses it
    Then setup.choose_harness {"harness": "copilot_cli"} is sent
    And on the answer {"restarting": true} "Starting ordin again with GitHub Copilot…" is drawn and nothing can be pressed

  Scenario: Edge case — an assistant that needs signing in is marked (SM-FS-10)
    Given step harness_choice needs_you, claude_code ready and copilot_cli needs_sign_in
    Then GitHub Copilot's row reads "You'll sign in next"

  Scenario: Edge case — choosing the assistant already in use goes on in place (SM-FS-11)
    When the answer is {"restarting": false}
    Then the question is removed and the step rows go on

  Scenario: Edge case — no question without the step (SM-FS-10)
    Given first start with no step harness_choice
    Then no question is drawn

  Scenario: Error path — the choice could not be saved (SM-FS-11)
    Given the person chose GitHub Copilot and pressed the main button
    When the answer is the error SETUP_CHOICE_NOT_SAVED
    Then "ordin couldn't save your choice. Try again." is drawn and the button is enabled with GitHub Copilot still chosen
```

### Gaps

None.


## Amendment 2026-02-05 — A turn that fails before any reply says so (SM-RF-1)

> **Status:** Approved by the user, 2026-02-05 ("approve as written"). Source: the 0.2.1-test.5 smoke test: "who are you?" got neither a reply nor an error; Diagnostics held `RA_UNREACHABLE`. The run's state reaches the screens (`run_state_changed`), its reason does not (no error on `Run` or the event), so the sentence is general and points at Diagnostics. **derivedFromHld:** 0.75.0. Every section above stays in force except where this amendment changes it.

- **SM-RF-1 The failed turn.** The panel remembers the person's last message sent with `send` while the working line (SM-WK-1, from `ordin_turn_changed`) is shown for the workspace. When `ordin_turn_changed` then arrives with `state` `idle` and no Ordin item has been drawn in the conversation since that message, the panel's error line (step 11) draws `conv-turn-failed` with **Try again**, which sends the same message again as `send` does. An Ordin item drawn after the message, a new `send`, or opening another conversation clears it. `ordin_turn_changed` carries no run id, so the rule reads the turn, not a run's state.
- Catalogue (en-GB, `screens.ftl`): `conv-turn-failed` = "Ordin couldn't get an answer this time. Try again; if it happens again, choose Save logs for support… in Settings › Diagnostics."

```gherkin
Feature: A turn that fails before any reply says so (SM-RF-1)

  Scenario: Error path — the run fails before a reply
    Given the person sent "who are you?" and the working line is shown
    When ordin_turn_changed arrives with state idle and no Ordin item was drawn
    Then the working line is gone and "Ordin couldn't get an answer this time." is drawn with Try again
    When the person presses Try again
    Then the message is sent again

  Scenario: Edge case — a failure after a reply draws nothing
    Given Ordin's reply to the turn is drawn
    When ordin_turn_changed arrives with state idle
    Then no error line is drawn
```

### Gaps

None. The working line's source is `ordin_turn_changed` (`apps/desktop/ui/src/lib/store/backend-state.svelte.ts:329-331`), which carries `workspace_id`, `state` and `since` only.

## Amendment February 2026 — The failed turn's cause reaches Diagnostics (SM-RF-2) (amended February 2026; user decision; Approved by the user)

- **SM-RF-2 The cause in Diagnostics.** When SM-RF-1 draws `conv-turn-failed` and the `idle` event carries `failure` (ServiceApi.md SVC-WK-2), the panel records `UI_TURN_FAILED` in this window's diagnostics ring (step 12) with the `failure` word as its detail, for the backend the conversation belongs to; with no `failure` the detail is `unknown`. The conversation's sentence is unchanged and shows no code: Diagnostics stays the only view that shows codes (SM-NS-3), and it lists the ring in the order recorded, so the last failed turn's cause is its lowest `UI_TURN_FAILED` line.

```gherkin
Feature: The failed turn's cause reaches Diagnostics (SM-RF-2)

  Scenario: Error path — the turn timed out
    Given the person sent "who are you?" and the working line is shown
    When ordin_turn_changed arrives with state idle and failure "timeout"
    Then "Ordin couldn't get an answer this time." is drawn
    And the diagnostics ring holds UI_TURN_FAILED with detail "timeout"

  Scenario: Edge case — an older service sends no failure
    Given the working line is shown
    When ordin_turn_changed arrives with state idle and no failure
    Then the diagnostics ring holds UI_TURN_FAILED with detail "unknown"
```

### Gaps

None.

## Amendment 2026-02-11 — A Confluence link row offers the document menu (SM-RL-5) (user decision 2026-02-11)

Plan 09 Phase 7: a Confluence link's own row in the Documents tree offered only **Remove**, while a PDF's row offers the document menu (SM-RL-4).

- **SM-RL-5 The page's menu on the link row.** For a `confluence_location` link with a place, when `ordin.send_message` is answered, the link row's menu holds, in order: **Ask Ordin about this**, **Add to knowledge**, **Add to memory**, then **Remove from this workspace…** when `workspaces.remove_link` is answered. Each of the first three reads the working copy's root (`files.list_folder { place_id, folder: "" }`), takes the file ending `.confluence.xhtml` there (the linked page, WorkspaceService.md WS-RD-1), and sends the same message SM-RL-4 sends for a file, named by the link's display name, with that file attached as a `place_file`. When no such file is there yet, the row draws `tree-link-page-not-read` and sends nothing. **Open in its app** and **Move to Bin…** stay on the file row only: the page lives in Confluence. Other link kinds are unchanged.

```gherkin
Feature: A Confluence link row offers the document menu (SM-RL-5)

  Scenario: Happy path — Add to memory from the link row
    Given the Confluence link "Supplier pages" whose working copy holds "Supplier pages.confluence.xhtml"
    When the person chooses Add to memory on the link row
    Then Ordin is sent "Add the facts in “Supplier pages” to memory." with that file attached
```

## Amendment 2026-02-11 — A linked Confluence page is one row (SM-RL-6) (user decision 2026-02-11)

The 0.2.1-test.18 smoke test: a link to a single Confluence page opened to a second row, the page's own `.confluence.xhtml` file, under the link row. The user's decision: one page is one item, working as a PDF's row does.

- **SM-RL-6 The link row is the page.** For a `confluence_location` link with a place whose working copy's root holds a file ending `.confluence.xhtml` (the linked page, WorkspaceService.md WS-RD-1), read with `files.list_folder { place_id, folder: "" }` when the links are listed: pressing the link row opens `document:<place id>:<that file's path>` titled with the link's display name (the page rendered, OrdinDesktop.md DSK-CFP-1), not `documents:<place id>`. The folder the page's child pages and attachments sit in, the file's path without `.confluence.xhtml`, is read once with `files.list_folder`; when it has items the row also opens and closes, under it, that folder's contents as SM-RL-3 draws a folder's (the page's own file is not drawn again); when it has none the row has no `aria-expanded` and shows nothing under it. The row's menu is SM-RL-5's. A link to a whole space, or a page whose file is not there yet, keeps SM-RL-3.

```gherkin
Feature: A linked Confluence page is one row (SM-RL-6)

  Scenario: Happy path — a page with no children
    Given the Confluence link "Rates" whose working copy holds only "Rates.confluence.xhtml"
    When the person presses the link row
    Then the page "Rates" opens and no row is drawn under the link

  Scenario: A page with a child page
    Given the Confluence link "Rates" whose working copy holds "Rates.confluence.xhtml" and "Rates/2026.confluence.xhtml"
    When the person presses the link row
    Then the page "Rates" opens and "2026.confluence.xhtml" is drawn under the link
```

## Amendment 2026-02-11 — Closing says it is closing (SM-QT-1) (user decision 2026-02-11)

- **SM-QT-1 The closing line.** On `ordin://quitting { running_sessions }` (OrdinDesktop.md DSK-QT-1) the quit question, if open, closes and the window is covered by one centred line, `role="status"`, above every dialog, with nothing under it pressable: `quitting-sessions` ("Closing ordin — stopping agent sessions, please wait…") when `running_sessions` is above 0, else `quitting` ("Closing ordin…"). It stays until the window goes.

```gherkin
Feature: Closing says it is closing (SM-QT-1)

  Scenario: Happy path — agent sessions are running
    Given two agent sessions are running
    When the person quits and confirms
    Then the window shows "Closing ordin — stopping agent sessions, please wait…" until it goes
```

## Amendment 2026-02-11 — How far back the support file goes (SM-NS-3b) (user decision 2026-02-11)

- **SM-NS-3b The window.** Beside **Save logs for support…** (SM-NS-3a) Diagnostics draws a select named `diagnostics-support-window` with four choices: Last 5 minutes (the default), Last 15 minutes, Last hour, Last 3 days, kept while the window is open. Saving sends `diagnostics.support_export { run_ids, last_minutes }` with 5, 15 or 60, and no `last_minutes` for Last 3 days, so the file is as before (OrdinDesktop.md DSK-LOG-6). The save button's tooltip `diagnostics-support-note` says the file holds the logs "for the time you choose".

```gherkin
Feature: How far back the support file goes (SM-NS-3b)

  Scenario: Happy path — the default and another window
    Given Diagnostics is open
    Then the window choice shows Last 5 minutes
    When the person saves logs for support
    Then diagnostics.support_export is sent with last_minutes 5
    When the person chooses Last 3 days and saves
    Then diagnostics.support_export is sent with no last_minutes
```

## Amendment 2026-02-11 — A search that finds nothing says so (SM-AW-5) (user decision 2026-02-11)

Plan 09 Phase 2: on **Add to workspace**, a SharePoint or Confluence search that found nothing drew nothing, so the person could not tell "nothing found" from "not working".

- **SM-AW-5 Nothing found.** When `sharepoint.find` or `confluence.find` answers an empty list, the screen draws one line under the field, `add-search-none` ("Nothing found for “<query>”. Try other words.", `role="status"`, muted text), for the query that answer was for. The line is not drawn while the field holds a different query, while an error is shown, or when the field is empty.

```gherkin
Feature: A search that finds nothing says so (SM-AW-5)

  Scenario: Edge case — an empty answer
    Given the person searches Confluence for "Supplier policy"
    When confluence.find answers an empty list
    Then "Nothing found for “Supplier policy”. Try other words." is drawn under the field
```

## Amendment 2026-02-11 — A file's tree row shows its kind's icon (SM-IC-5) (user decision 2026-02-11)

The 0.2.1-test.14 smoke test: a PDF in the Documents tree showed the plain page icon, while its screen tab showed the PDF icon. `apps/desktop/ui/src/lib/shell/TreeFolder.svelte` drew `i-page` for every file.

- **SM-IC-5 A file's icon in the tree.** A file row of the Documents tree draws `fileKindIcon(name)` (`apps/desktop/ui/src/lib/icons/kinds.ts`) in the tile tint `fileKindTint` gives, the icon and tint its screen tab and chips use; a file of no known kind draws `i-page`, untinted, as before. Folder rows are unchanged (`i-folder`).

```gherkin
Feature: A file's tree row shows its kind's icon (SM-IC-5)

  Scenario: Happy path — a PDF shows the PDF icon
    Given the Documents tree lists "Quarterly plan.pdf"
    Then its row draws i-pdf in the pdf tint
```

## Amendment 2026-02-11 — A reply still on its way is not a failed turn (SM-RF-3) (user decision 2026-02-11)

The 0.2.1-test.13 smoke test: "Ordin couldn't get an answer this time." was drawn after turns whose replies the log records (`copilot.event.turn_ended has_final_text: true`), and the reply then appeared under it. `block_available` and the `idle` `ordin_turn_changed` arrive together; the panel draws an available block only after `ordin.block` answers, so SM-RF-1 saw the turn end with no new Ordin item.

- **SM-RF-3 Judge once shown blocks are drawn.** `BackendState` counts, per workspace, the available blocks whose `ordin.block` has not answered (`pendingBlocks`, never stored); the count falls when the call answers or fails. SM-RF-1 draws `conv-turn-failed` only when that count is 0; while it is above 0 the panel waits, and judges again when it falls. A block whose fetch fails is not drawn, so a turn whose only reply failed to fetch still ends in `conv-turn-failed`.

```gherkin
Feature: A reply still on its way is not a failed turn (SM-RF-3)

  Scenario: Edge case — the reply is still on its way when the turn ends
    Given the person sent "read this PDF" and the working line is shown
    And block_available arrives for Ordin's reply, whose ordin.block has not answered
    When ordin_turn_changed arrives with state idle
    Then "Ordin couldn't get an answer this time." is not drawn
    And when ordin.block answers, the reply is drawn and no failure is drawn
```

### Gaps

None.

## Amendment February 2026 — A question's short choices sit side by side, coloured by what they do (SM-QB-1) (amended February 2026; user decision; Approved by the user)

The 0.2.1-test.9 smoke test: the folder question of SM-FA-4 showed **Allow** above **Not now**, both alike, so the main answer did not stand out. This amendment changes the "one button per choice, one under another" layout of a `question` block (scenes `ambiguous`, `agentAsks`; SM-FA-4); every other rule of the block stays.

- **SM-QB-1 Choice layout and colour.** `apps/desktop/ui/src/lib/blocks/kinds/Question.svelte` draws the choices:
  1. **Row or column.** On one wrapping row when there are at most 3 choices and none is longer than 24 characters (after trimming); otherwise one under another, as before.
  2. **Way out.** A choice whose trimmed, lower-cased text is `not now`, `skip`, `cancel`, `later` or `no` is a plain button.
  3. **Danger.** A choice whose trimmed, lower-cased text starts with `reject`, `delete`, `remove`, `discard` or `stop` is a `danger` button (outlined in `--color-danger`).
  4. **Main action.** When at least one choice is a way out, the first choice that is neither a way out nor danger is the `primary` button (the filled button, UX guideline §4: one main action). With no way-out choice the question offers options of equal weight, and no choice is primary.
  5. Order, wording, the **Skip** button of SM-DIS-1 and what a press sends are unchanged.

```gherkin
Feature: A question's short choices sit side by side (SM-QB-1)

  Scenario: Happy path — the folder question
    Given the question "Document agent needs to read Docs to answer." with choices Allow and Not now
    When it is drawn
    Then Allow and Not now are on one row
    And Allow is the primary button and Not now a plain button

  Scenario: Edge case — options of equal weight
    Given a question with choices "Monthly" and "Weekly"
    When it is drawn
    Then neither choice is the primary button

  Scenario: Edge case — a long choice stacks the choices
    Given a question with a choice longer than 24 characters
    When it is drawn
    Then the choices are one under another
```

### Gaps

None.

## Amendment 2026-02-12 — SharePoint through a folder OneDrive syncs (SM-AW-6) (user decision 2026-02-12, "add the OneDrive fallback"; ADR-126)

Plan 09 Phase 4: a person whose organisation blocks ordin's Microsoft app, or whose build carries none and who has no company profile, could not use SharePoint at all. Most such people already have OneDrive signed in to their work account, and OneDrive can sync a SharePoint library to a folder on the computer. Adding that folder needs no sign-in in ordin.

- **SM-AW-6 Use a folder OneDrive syncs.** On **Add to workspace**, with the SharePoint entry chosen, on a backend that answers `app.pick_folder`:
  1. The search step and the sign-in step each draw, below their own controls, the secondary button **Use a folder OneDrive syncs** (`add-onedrive-open`), which opens the OneDrive step.
  2. The SharePoint entry goes straight to the OneDrive step, instead of `not_configured`, when `sharepoint.find` answers `WS_SOURCE_NOT_CONFIGURED`, or `identity.sign_in { kind: microsoft }` answers `CRED_PROVIDER_NOT_CONFIGURED`, since no sign-in can complete there. `CRED_ADMIN_CONSENT_REQUIRED` stays on the sign-in step with its sentence, where the button of item 1 is drawn.
  3. The OneDrive step is one panel: the heading `add-onedrive-title`; `add-onedrive-line` (OneDrive can keep a SharePoint library on your computer, and ordin reads it from there); three numbered steps `add-onedrive-step-1` (open the library in your browser), `add-onedrive-step-2` (select **Sync**, and OneDrive adds it to your computer), `add-onedrive-step-3` (choose that folder here; on a Mac it is under your OneDrive folder in Finder's sidebar, on Windows under your organisation's name in File Explorer); the one main action **Choose the synced folder…** (`add-onedrive-choose`), which sends `app.pick_folder {}` and adds the answer as a `local_folder` link exactly as **Choose a folder…** does (SM-AW-3); and **Back** (`add-back`) to *Where it lives*. A refusal is drawn under the main action with `ErrorLine`.
  4. The link is an ordinary folder on this computer: the *Linked* label, *Added*, reading progress and folder access are those of a local folder. OneDrive keeps it in step with SharePoint; ordin does not search SharePoint for it.

```gherkin
Feature: SharePoint through a folder OneDrive syncs (SM-AW-6)

  Scenario: Happy path — the person adds a synced library
    Given the SharePoint entry is chosen on Add to workspace
    When the person presses "Use a folder OneDrive syncs" and then "Choose the synced folder…"
    And the picker answers "/Users/asha/Library/CloudStorage/OneDrive-SharedLibraries-Contoso/Finance - Documents"
    Then workspaces.add_link is sent with a local_folder link of that path
    And Added is drawn

  Scenario: Edge case — no Microsoft sign-in is possible in this version
    Given no company profile names Microsoft and the build carries no Microsoft app
    When the person presses "Sign in with Microsoft"
    And identity.sign_in answers CRED_PROVIDER_NOT_CONFIGURED
    Then the OneDrive step is drawn

  Scenario: Error path — the organisation blocks ordin's app
    Given the sign-in answers CRED_ADMIN_CONSENT_REQUIRED
    Then its sentence is drawn on the sign-in step
    And "Use a folder OneDrive syncs" is drawn below it

  Scenario: Edge case — the person closes the picker
    Given the OneDrive step is drawn
    When the picker answers no folder
    Then nothing is sent and the step stays drawn
```

## Amendment 2026-02-12 — The question to move ordin (SM-FS-14) (plan 08 Phase 19; user decision; Approved by the user February 2026)

- **SM-FS-14 Move ordin to your Applications folder?** On macOS, when `app.install_place` answers `installed: false`, first start draws one panel before anything else of SM-FS (before the assistant question of SM-FS-10 and before the checks): the heading `setup-move-title` ("Move ordin to your Applications folder?"), the line `setup-move-line` ("It goes in the Applications folder in your home folder. No administrator is needed."), the one main action **Move ordin** (`setup-move-do`) and **Not now** (`setup-move-later`). **Move ordin** sends `app.move_to_applications {}` and draws `setup-move-moving` ("Moving ordin…", `role="status"`) while it waits; the program then quits by itself. A refusal is drawn with `ErrorLine` under the main action, with **Try again** when retryable, and **Not now** stays. **Not now** carries on with first start; the question is drawn again at the next start while ordin is not installed. A backend that does not answer `app.install_place`, and every other platform, draws nothing new. Words in `screens-setup.ftl` (en-GB). UX: one main action in words (11 §4).

```gherkin
Feature: The question to move ordin (SM-FS-14)

  Scenario: Happy path — the person moves ordin
    Given app.install_place answers installed false
    When first start is drawn
    Then "Move ordin to your Applications folder?" is drawn before the checks
    When the person presses Move ordin
    Then app.move_to_applications is sent and "Moving ordin…" is drawn

  Scenario: Edge case — not now
    When the person presses Not now
    Then first start carries on and nothing is sent

  Scenario: Error path — the copy fails
    Given app.move_to_applications answers DSK_MOVE_COPY_FAILED
    Then its sentence is drawn with Try again, and Not now stays
```

## Amendment 2026-02-12 — Menu items say they came from a menu (SM-RL-7) (plan 10 Phase 7; Approved by the user 2026-02-12)

> **Status:** Approved by the user, 2026-02-12 ("approve phase 7 as written, build it"). Source: the user's decisions of 2026-02-11 (a repeated **Add to memory** gives the same answer; search does not list menu actions). Amends SM-RL-4, SM-RL-5, SM-PUB-1's overview menu and SM-ASK-1; ServiceApi.md SVC-MENU-1 defines the field.

- **SM-RL-7 `from_menu` on menu messages.** Every `ordin.send_message` sent by a menu item adds `from_menu`: **Ask Ordin about this** (SM-ASK-1, wherever it is drawn) sends `"ask_about"`; **Add to memory** (SM-RL-4, SM-RL-5, the Documents overview of SM-PUB-1) sends `"add_to_memory"`; **Add to knowledge** (the same places, and a folder row's menu) sends `"add_to_knowledge"`. The text, the attachment and the conversation brought into view are unchanged. A message typed in the conversation sends no `from_menu`. When the answer is the question of OrdinHost.md OH-MENU-3 ("already in memory"), it is drawn as any waiting `QuestionBlock`; no screen change is needed.

```gherkin
Feature: Menu items say they came from a menu (SM-RL-7)

  Scenario: Happy path — Add to memory from the tree
    When the person chooses Add to memory on "Quarterly plan.pdf"
    Then ordin.send_message carries from_menu "add_to_memory" with the file attached

  Scenario: Edge case — a typed message
    When the person types a message and sends it
    Then ordin.send_message carries no from_menu
```

### Gaps

None.

## Amendment 2026-02-12 — Clear and Continue offer no key points (SM-NKP-1) (plan 10 Phase 8; Approved by the user 2026-02-12)

> **Status:** Approved by the user, 2026-02-12 ("approve phase 8 as written, build it"). Source: the user's decision of 2026-02-11 on **Keep the key points first** ("remove it"; **Summarise** keeps a conversation's substance); HLD 11 §5.12, UX-A21 and UX-A76 (revision 0.76.2). Counterparts: ServiceApi.md SVC-NKP-1, OrdinHost.md OH-NKP-1. Evidence read: `apps/desktop/ui/src/lib/shell/ConversationPanel.svelte:176-180`, `:363`; `apps/desktop/ui/src/lib/views/ContinueDialog.svelte:51`; `apps/desktop/ui/src/lib/views/earlier.svelte.ts:198-210`, `:262-268`; `crates/ordin-localisation/catalogues/en-GB/screens.ftl:155`, `screens-settings.ftl:246`.

- **SM-NKP-1 No Keep the key points first.** It replaces SM-WR-12 and the parts of SM-CC-3 and SM-CC-14 that name it.
  1. Clear's confirmation draws `confirm-clear-body`, the verb **Clear** and **Cancel**, and no third choice. **Clear** sends `ordin.start_fresh { workspace_id, kept_points: [] }` as before.
  2. Continue's confirmation (SM-CC-3) draws `confirm-continue-body`, **Continue** and **Cancel**; focus still goes to **Cancel**. **Continue** sends `ordin.continue_conversation { workspace_id, conversation_id, kept_points: [] }` as before.
  3. No screen sends `ordin.prepare_start_fresh`. The string `confirm-clear-keep-points` is removed from every catalogue.
  4. `confirm-clear-body` changes to the text below: it drops the key-points sentence and keeps the pointer to **Summarise**.

| Id | Text |
|---|---|
| `confirm-clear-body` | Ordin forgets what was said here and starts a new conversation. This one stays readable in Earlier conversations. Summarise instead keeps this conversation going. |

```gherkin
Feature: Clear and Continue offer no key points (SM-NKP-1)

  Scenario: Clear asks with two buttons
    Given the live conversation of W has messages
    When the person presses Clear
    Then the dialog "Clear this conversation?" shows Clear and Cancel only

  Scenario: Continue asks with two buttons
    Given the live conversation of W has 2 messages
    When the person presses Continue this conversation on X
    Then the dialog "Continue this conversation?" shows Continue and Cancel only
    When the person presses Continue
    Then ordin.continue_conversation is sent with W, X and no kept points
```

### Gaps

None.

## Amendment 2026-02-12 — Knowledge pages in the tree, and screens that keep up (SM-KT-1 to SM-KT-3) (plan 10 Phase 9; Approved by the user 2026-02-12)

> **Status:** Approved by the user, 2026-02-12 ("approve phase 9 as written, build it"). Source: the user's reports of 2026-02-11 (a page Ordin wrote did not appear in the tree or on the Knowledge screen; list pages in the tree under **Knowledge**, in their folders); user decisions of 2026-02-12: choosing **Knowledge** still opens the Knowledge screen and its arrow unfolds the pages (1A); one folder level — the person's and Ordin's pages at the top, each team folder's pages under the folder's name (2A). HLD 11 §4, UX-A14. Counterparts: KnowledgeService.md KS-PC-1, ServiceApi.md SVC-PC-1. Evidence read: `apps/desktop/ui/src/lib/shell/WorkspaceTree.svelte:48`, `:167`; `apps/desktop/ui/src/lib/shell/MinutesTreeGroup.svelte` (a group row that unfolds); `apps/desktop/ui/src/lib/views/fixed/Knowledge.svelte:80`; `apps/desktop/ui/src/lib/store/backend-state.svelte.ts:347-349`; `PageListing` and `PageSummary` (`crates/ordin-knowledge/src/model.rs`); the screen key `knowledge` with `<page id>` (this spec, table of screens).

- **SM-KT-1 The Knowledge row unfolds.** The row keeps its icon, its label `tree-knowledge` and its action: choosing the label opens the Knowledge screen, as before. It gains an arrow (`aria-expanded`), like any node that has children (UX-A14); the arrow, and Right/Left on the focused row, unfold and fold it. Whether it is unfolded is kept with the tree's other unfolded nodes. It is drawn only when the backend answers `knowledge.list`, as now. The row gains no ⋯ menu.
- **SM-KT-2 What it lists.** When unfolded, the row sends `knowledge.list { workspace_id }` and draws, from `this_workspace` and `from_other_teams`, every page whose `state` is not `removed`:
  1. One folder row per distinct `folder` in `from_other_teams`, ordered by name; the folder row has the `folder` icon, its name, `folder_description` as its tooltip when present, and unfolds and folds (its state not kept). Under it, its pages.
  2. Then the pages of `this_workspace`, at the top level.
  3. At each level, pages are ordered by title (case-insensitive). When a level holds more than 10 pages, it draws the 10 most recently changed (`last_changed_at`), still ordered by title, then a row `tree-show-all` ("Show all…") which draws the rest at that level in place.
  4. A page row shows its title with the `page` icon; choosing it opens the screen `knowledge:<page id>`. It has no ⋯ menu: a page's actions are on its screen.
  5. Pages from agents (`from_agents`) and files not adopted (`not_adopted`) are not listed in the tree; they stay on the Knowledge screen.
  6. No page at all → one muted line `tree-knowledge-none` ("No pages yet."). A failed read → the error line with **Try again**, as the Minutes group draws it.
- **SM-KT-3 Screens keep up.** On `knowledge_pages_changed` for a workspace (ServiceApi.md SVC-PC-1), the desktop raises that workspace's revision, as for `knowledge_agents_changed`. An unfolded Knowledge row and an open Knowledge screen for that workspace read `knowledge.list` again when the revision changes; a page that is no longer listed is drawn no more, and an open page screen is left as it is.

| Id | Text |
|---|---|
| `tree-show-all` | Show all… |
| `tree-knowledge-none` | No pages yet. |

| Condition | Error code | Behaviour |
|---|---|---|
| `knowledge.list` fails when the row unfolds or reloads | as returned | Error line under the row with **Try again**; the rows already drawn are left as they were |

```gherkin
Feature: Knowledge pages in the tree (SM-KT-1 to SM-KT-3)

  Scenario: Choosing Knowledge still opens the screen
    When the person chooses Knowledge under workspace W
    Then the Knowledge screen of W opens
    And the Knowledge row is not unfolded

  Scenario: Unfolding lists team folders, then the workspace's pages
    Given W has the pages "Budget" and "Agenda" and the team folder "Finance" with the page "Rates"
    When the person unfolds Knowledge under W
    Then the rows are "Finance", then "Agenda", then "Budget"
    And "Rates" is under "Finance"

  Scenario: A page written while the row is unfolded appears
    Given Knowledge under W is unfolded
    When the event knowledge_pages_changed {workspace_id: W} arrives
    Then knowledge.list is read again for W
    And the new page is drawn

  Scenario: An open Knowledge screen shows a new page
    Given the Knowledge screen of W is open
    When the event knowledge_pages_changed {workspace_id: W} arrives
    Then the screen reads knowledge.list again

  Scenario: Edge case — more than 10 pages at a level
    Given W has 12 pages
    When the person unfolds Knowledge under W
    Then the 10 most recently changed are drawn by title, then "Show all…"
    When the person chooses "Show all…"
    Then all 12 are drawn by title

  Scenario: Error — the list cannot be read
    Given knowledge.list fails for W
    When the person unfolds Knowledge under W
    Then an error line with Try again is drawn under the row
```

### Gaps

None.
