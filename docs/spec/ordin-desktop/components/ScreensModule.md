# SPEC: App Shell, Block Renderer (drawing side)

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-103) · amended January 2026 (ADR-104; promoted to Approved by the user January 2026) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-108; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (the dictation type is `DictationAvailability`) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
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

## Purpose

The screens module is everything the person sees in the ordin desktop window: the **App Shell** — side rail with Home, *Search or go to…* and the workspace tree, the screen area with one tab per open screen, the conversation panel with *Needs you* as its fixed first tab, Settings, Diagnostics and engineering mode's fixed views (02:313–330; 11 §4) — and the **Block Renderer's drawing side**, which draws every block of the fixed catalogue with the application's own code (02:332–339; ADR-028, ADR-046, ADR-067, ADR-068, ADR-070). It runs in the Tauri WebView, depends only on TypeScript types generated from `ordin-service-api` (2B-1 rule 6), holds one backend handle per backend it shows — the desktop's own core and each paired ordin server — and never knows where a backend runs (ADR-096 item 2). It holds no state that is not also in the journal, apart from view preferences (02:328).

## Approach

Svelte 5 components with runes (`$state`, `$derived`) over one event store per backend: the store applies Service API events in sequence order, reloads through commands on a gap or a `reset`, and every view derives what it draws from the store and from command results; nothing is drawn from markup supplied by data. All calls leave through one file, `lib/backend/tauri.ts`, the only importer of `@tauri-apps/api` (SA-UI-01). No component library (SA-UI-02): ordin's own components, styled only from design tokens. Words come from Fluent catalogues (`@fluent/bundle`, SA-I18N-01), never literals. Error-code prefix for the module's own diagnostics: `UI_` (*chosen here*; the person never sees one). Rejected: a global store library or a client-side router (Svelte's runes and one `ScreenArea` keyed by screen identifiers cover both, with no added dependency). Rejected: drawing blocks from HTML produced in the core (ADR-028: data, never markup).

### Reconciliation assumptions

- **SA-RECON-01** The views call the owners' command names that the Service API catalogue adopted (rulings R1, R6; reconciliation rows 177, 180; X5): `home.view`, `home.document_opened`, `home.run_again_inputs`, `home.needs_you`, the `ordin.*` conversation group, `approval.list_pending`, `identity.*` sign-ins, `catalogue.*` agent repositories, `setup.check_now`, and `setup.help_preview` then `setup.help_send` in place of `help.send_to_it`.
- **SA-RECON-02** Dictation (row 178) uses `dictation.get_availability`, `dictation.start { field }`, `dictation.stop` and `dictation.cancel { session_id }`; its `DictationUpdate` values arrive on the Command Bridge's `service_streams` channel as `StreamMessage { stream: <session_id>, kind: "dictation", payload: DictationUpdate }`, never as journalled events; terminal bytes and language-server messages arrive on the same channel with `kind` `terminal` and `lsp`.
- **SA-RECON-03** *Amended January 2026 (ADR-109; SM-LG-1):* message catalogue text comes from the core with `localisation.get_catalogue`, never bundled into the screens at build time, as Localisation's spec states (row 76, N16) — the earlier wording here ("stay bundled at build time", O19) contradicted it and could not show a language the person edits, makes or imports. At start the module reads the active locale from `localisation.get_settings` and the brand's string overrides and design tokens from `brand.get`, and re-reads them on `locale_changed` and `brand_changed` (N16, N34, row 179).
- **SA-CLOSE-01** **Check it** on *What [agent] is made of* and on every part page calls the Checker's commands as the Service API restates them: `checker.check_installed { agent_instance_id }` for an installed agent, `checker.check_library_item { agent_repository_id, kind, identifier }` for a library part (`agent_repository_id` absent for the bundled catalogue), and `checker.report { check_run_id }` to show an earlier result again (closes O165).
- **SA-CLOSE-02** `sign_in_changed` carries `credential_kind` (ServiceApi.md SA-CLOSE-02); the store reads `credential_id`, `credential_kind` and `state`.
- **SA-CLOSE-03** Settings › Sign-ins offers GitLab by `identity.sign_in { kind: "gitlab" }` when the organisation profile names a GitLab OAuth application, else by a token box that sends `identity.save_gitlab_token { token }` (the box is a password field, cleared after sending); and, on the desktop when the profile requires a client certificate, **Import a certificate…**, which sends `identity.import_client_certificate {}` (the desktop program opens the file pickers). Which GitLab form to show comes from `identity.sign_in` itself: a reply `CRED_PROVIDER_NOT_CONFIGURED` for `gitlab` switches the row to the token box *(chosen here)*.
- **SA-CLOSE-04** *(chosen here)* A project's check commands are set on the project's settings screen, which sends `projects.set_check_commands { project_id, commands }` (ServiceApi.md SA-CLOSE-07, ProjectService.md SA-CLOSE-03) when the engineer presses **Save**; the screen shows the project's current `check_commands` from the `Project` the command returns (or from `projects.open`), one command per line, and, when the list is empty, `project-checks-none` = "No checks set". No other screen and no program sets them.
- **SA-RECON-04** The version sentence follows `ServerView.older_side` (`desktop` or `server`), which the Server Connector fills (N36); the error line shows `ServiceError.reference` when present (N35).

## Shared Context

### Libraries (1A; versions fixed there)

`svelte` 5.57.2, `vite` 8.3.4, `@sveltejs/vite-plugin-svelte` 7.3.1, `typescript` 7.0.2, `@tauri-apps/api` 2.12.4, `@fluent/bundle` 0.19.1, `echarts` 6.1.0 (SA-UI-03, provisional until C-40), `@xterm/xterm` 6.0.0 (SA-TERM-01), Monaco Editor (05 *Code editor*; version pinned at lock time, 05 *Build / test toolchain*). Build-time only (05 *Build / test toolchain*): `vitest` (tests), `svelte-check` (type check of `.svelte` files). TypeScript types are generated by the Service API crate's own generator (`ordin-service-api-gen`, Service API spec), so no npm generator is added.

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
  code: string; message: string; action?: 'try_again' | 'sign_in_again' | 'send_to_it';
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
export type ContentRef = { kind: 'artefact'; artefact_id: string } | { kind: 'place_file'; place_id: string; relative_path: string };
export type ApprovalContent =
  | { kind: 'message'; label: string; text: string; destination_note?: string }
  | { kind: 'command'; label: string; command: string; runs_in: string }
  | { kind: 'changes'; label: string; change_set: ContentRef }
  | { kind: 'page'; label: string; page: ContentRef }
  | { kind: 'files'; label: string; files: ContentRef[] }
  | { kind: 'email'; label: string; email: EmailApproval };
export interface ApprovalPress { card_ref: string; decision: 'approve' | 'reject' }   // command approval.decide
```

`Block` is the tagged union of 2C wire §7 with `kind` ∈ `message`, `question`, `form`, `documents`, `preview`, `document`, `changes`, `plan_progress`, `table`, `chart`, `key_figures`, `report`, `email_list`, `email`, `telegram_list`, `result`, `agent_proposal`, `agent_library`, `crew_plan`, `memory`, `knowledge`; its fields are those 2C fixes and the generated file carries them (for example `TableBlock { title, columns: { name, type, … }[], rows: Cell[][], source: PartSource }`, cells `null | number | string | ContentRef`; `ChartBlock { title, chart, …, columns, rows, x, series, x_title?, y_title?, source }`; `ReportBlock { title, as_of, parts: { heading, content }[] }`; `PartSource` = `documents` | `company_system` | `earlier_result` | `worked_out`). Limits the drawing side relies on (A-34, LW-29–LW-42): table ≤ 10,000 rows × 30 columns; chart ≤ 8 series and ≤ 5,000 points, pie ≤ 6 slices; key figures 1–8; report 1–12 parts, ≤ 2 MB; plan 3–7 steps of ≤ 80 characters; question 2–4 choices.

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
| `conv-show-closed` | Show closed items ({ $count }) | 11 §4 |
| `conv-close-finished` | Close finished items | 11 §4 |
| `card-close` / `card-show-again` / `card-closed` | Close / Show again / { $title } · closed | 11 §3, §4 |
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
| `progress-count-of` / `progress-count-so-far` | { $done } of { $total } / { $done } so far | 11 §5.4 |
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
| `server-version-server-older` | { $server } runs a different version of ordin. Ask whoever runs it to update it. | 11 §5.31 |
| `server-version-desktop-older` | Update ordin on this computer to use { $server }. | 11 §5.31 |
| `server-no-local-folders` | Folders on this computer can't be added to a workspace on { $server }. | 11 §5.31 |
| `server-open-copy` | Open a copy in Word | 11 §5.31 |
| `server-open-copy-note` | This saves a copy on this computer. Changes to the copy stay here. | 11 §5.31 |
| `server-no-code-views` | Code views aren't available for workspaces on a server yet. | 11 §5.31 |
| `server-cannot-reach` | Can't reach { $server } right now. ordin will keep trying while it's open. | 11 §6 |
| `server-ended` | This computer is no longer connected to { $server }. | 11 §6 |
| `server-connect-again` | Connect again… | 11 §6 |
| `error-try-again` / `error-sign-in-again` / `error-send-to-it` | Try again / Sign in again / Send to IT | 11 §6 |
| `error-reference` | Reference { $reference } | 11 §6 |
| `error-sent-to-it` | Sent to IT. IT will reply to you directly. | 11 §6 (OQ-53 settled) |
| `dictation-preparing` | Getting dictation ready… | DictationService (`Preparing`) |
| `dictation-failed` | Dictation stopped. You can try again. | DictationService step C6 |
| `settings-section-<name>` | Sign-ins, Agent repositories, Email, Telegram, Slack, Jira, Servers, Dictation, How Ordin talks to you, Language, Using your plan, Notifications, History, Display, Code, What Ordin remembers, Diagnostics | 11 §4; amended by ADR-102, ADR-106 and ADR-109 |
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
| `team-share-with-team` ‡ | Share knowledge with a team… | ADR-103 |
| `team-share-dialog-title` ‡ | Share this workspace's knowledge with a team | ADR-103 |
| `team-share-dialog-pick` ‡ | Choose a folder in a GitHub or GitLab repository | ADR-103 |
| `team-share-dialog-address` ‡ | Or paste the folder's address from GitHub or GitLab | ADR-103 |
| `team-share-dialog-address-invalid` ‡ | Paste the address of a folder as GitHub or GitLab shows it. | ADR-103 |
| `team-share-dialog-all` ‡ | Everything Ordin remembers in this workspace and every knowledge page are shared with the people who can open this folder. | ADR-103 (D6) |
| `team-share-dialog-history` ‡ | What you share stays in the repository's history, even after you delete it. | ADR-103 (D9) |
| `team-share-dialog-verb` ‡ | Start sharing | ADR-103 |
| `team-header` ‡ | Team knowledge: { $folder } | ADR-103 |
| `team-available` ‡ | { $facts } new facts and { $pages } pages from { $people } | ADR-103 (Fluent plurals give "1 page", "8 new facts") |
| `team-get` ‡ | Get them | ADR-103 |
| `team-share` ‡ | Share what we've learned | ADR-103 |
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
    - `conversation: Map<WorkspaceId, ConversationItem[]>` — blocks shown, in `seq` order, each with `closed: boolean` (view preference) and its one-line card state;
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
    | Screen area and tabs (`ScreenArea`, `ScreenTabs`) | centre | one row of tabs per workspace; something already open comes to the front; at most 6 open — opening a seventh closes the oldest tab neither in front nor showing running work; tabs out of view under `tabs-more`; Delete closes the focused tab; closing never deletes | — (view preference) | — |
    | Conversation panel (`ConversationPanel`) | right | *Needs you* as the fixed first tab (never closed), then one tab per opened workspace with a new mark and waiting count; switching a tab switches the whole window to that workspace; tools **Summarise**, **Clear**, **Earlier**; the long-conversation nudge; `conv-show-closed`; the message box with the dictation button only where `dictation.get_availability` is not `unavailable` (macOS 26+; step 16); **Clear** asks first, then sends `ordin.prepare_start_fresh` and, with the points the person keeps, `ordin.start_fresh` | `ordin.read_conversation`, `ordin.send_message`, `ordin.answer_block` (a choice, other text, a form or an offer as its `BlockAnswer`), `ordin.close_item`, `ordin.show_item`, `ordin.close_finished`, `ordin.summarise`, `ordin.prepare_start_fresh`, `ordin.start_fresh`, `ordin.continue_conversation` (amended January 2026, ADR-108: the conversation picker in the header and **Continue this conversation**, SM-CC-1…7), `ordin.list_conversations`, `ordin.block`, `dictation.get_availability`, `dictation.start`, `dictation.stop`, `dictation.cancel` | `block_shown`, `block_available`, `approval_*` |
    | *Needs you* (`NeedsYouTab`) | conversation panel, first tab | every waiting item grouped by workspace, the open workspace first, "On [server]" for a server's; choosing one opens its workspace and item with focus on its heading, never on a button | `home.needs_you`, `approval.list_pending`, `approval.get` | `needs_you_changed`, `approval_*` |
    | Home (`Home`) | centre, fixed view | Waiting for you (only the oldest keeps a primary **Open**), Running now (state label and **Show progress**), Since you last looked (newest first, **Open**, **Run again…** on finished work); filter **All workspaces** or one; **new** marks with a label; empty sections say so in one line; a row opens the item in its own workspace; no button approves, sends, posts or publishes *Waiting for you* header has **Open a handover link…** (opens a one-field dialog; paste → `handover.open_link { link }`; errors drawn with `ErrorLine`) (amended January 2026, ADR-102). | `home.view`, `home.opened`, `home.document_opened`, `home.run_again_inputs` | `run_state_changed`, `approval_*`, `crew_round_changed`, `needs_you_changed` |
    | Search or go to… (`SearchDialog`) | dialog over the window; Ctrl+K / ⌘K anywhere; rail control | WAI-ARIA combobox with listbox; groups Go to, Found in pages and documents, Actions; result count announced as it changes; a result in another workspace names it and, on use, switches first; footer `search-footer`; Escape returns focus where it was; queries debounced 120 ms | `search.query` | — |
    | Settings (`Settings/*`) | centre, fixed view | sections in 11 §4's order: Sign-ins, Agent repositories, Email, Telegram, Slack (amended January 2026, ADR-102), Jira (amended January 2026, ADR-106), Servers, Dictation, How Ordin talks to you, Language (amended January 2026, ADR-109; SM-LG-2), Using your plan, Notifications, History, Display, Code, What Ordin remembers, Diagnostics; "Set by your company" on locked values; Notifications and Display are `settings.get` / `settings.change` of the sections `notifications` and `display`, answered by the desktop program; History validates in place (`settings-history-invalid` under the box, tied by `aria-describedby`, focus kept, last value kept until a valid one); Servers lists each server with its state, **Connect to a server…** (address and code, or the one pasted line), **Disconnect this computer…** (confirmation), **Remove this server** | `settings.get`, `settings.change` (sections `notifications` and `display`, answered by the desktop program), `identity.list`, `identity.sign_in`, `identity.sign_in_wait`, `identity.sign_in_cancel`, `identity.sign_out`, `identity.save_mail_password`, `identity.save_gitlab_token`, `identity.import_client_certificate`, `jira.connect_token` (amended January 2026, ADR-106), `catalogue.list_repositories`, `catalogue.probe_repository` then `catalogue.add_repository`, `catalogue.rename_repository`, `catalogue.remove_repository`, `localisation.get_settings`, `localisation.set_locale`, `usage.get_settings`, `usage.update_settings`, `usage.today`, `servers.list`, `servers.connect`, `servers.disconnect`, `servers.remove`, `setup.status`, `setup.check_now`, `setup.sign_in_harness`, `dictation.get_availability`; Language (amended January 2026, ADR-109): `localisation.list_languages`, `localisation.export_language`, `localisation.import_language`, `localisation.remove_language`, `ordin.estimate_language_draft`, `ordin.draft_language`, `ordin.stop_language_draft` | `server_state_changed`, `sign_in_changed`, `setup_progress`, `locale_changed`, `language_changed` (amended January 2026, ADR-109) |
    | Diagnostics (`Diagnostics`) | Settings › Diagnostics | journal events, versions and technical names, for support; the only view showing codes; also this window's own diagnostics ring (step 12) | `diagnostics.lines`, `diagnostics.support_export`, `setup.help_preview`, `setup.help_send` | — |
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
11. **Errors** (`ErrorLine`, 11 §6, P7). A command's `ServiceError` is shown as its `message` and at most one action: `try_again` re-sends the same command with a new `request_id`; `sign_in_again` opens Settings › Sign-ins at the account concerned; `send_to_it` calls `setup.help_preview` with the error's code and reference, shows the returned preview on the approval-style help card (11 §6; not an Approval Service card), and on the person's press calls `setup.help_send { preview_id }`; then `error-sent-to-it`. Under the action, `error-reference` with `ServiceError.reference` when the error carries one; none → the line is not drawn. Background retries show as a quiet status line, never repeated pop-ups. Diagnostics is never offered as the way forward. An `invoke` that rejects (Bridge unavailable) → `UI_IPC_FAILED`, shown as `server-cannot-reach` for a server's backend, otherwise `error-try-again` with no further detail; a command that waits for no model and has no reply after 10 s → `UI_COMMAND_TIMEOUT`, shown the same way.
12. **Diagnostics ring.** Every `UI_…` and `SVC_EVENT_GAP` diagnostic is kept in memory, last 500 entries (`{ at, code, backend?, detail }` with no content — no text of a block, card or message), shown under Settings › Diagnostics as "This window" *(assumed here: the screens module writes no log file of its own)*.
13. **Notifications opened from outside.** On `ordin://open-target`, open the target's workspace and item exactly as *Needs you* does (approval card, run progress, crew progress, or the question in the conversation). `ordin://open-target` with kind `handover` also follows a handover link (OrdinDesktop step 13). Its `workspace_id` is optional (OrdinDesktop.md): when present, open that workspace and the handover screen as today; when absent (a handover not yet accepted), open the handover screen in front of the current view, as *Needs you* opens a handover it lists. (amended January 2026, ADR-102)
14. **Keyboard and focus** (11 §10). F6 / Shift+F6 move between the tree, the open screen and the conversation; screen and conversation tabs follow WAI-ARIA tabs (arrow keys move and show); the side panels' edges are separators operable with arrow keys, Home, End and Enter (restore), widths stored as view preferences; screen-reader announcements (`aria-live="polite"`) for a new approval and a finished task, using the Now sentence; visible focus on every control.
15. **Narrow windows.** Below 1,024 CSS pixels wide *(assumed here: three columns of at least 240, 480 and 300 pixels)*, the conversation slides over from the side, opened from **Conversation** in the header, which shows a mark when something new arrives. At 700 pixels or less, an editing page keeps at least 60 % of the window's height (11 §4).
16. **Dictation** (DictationService §E; ADR-043). At start and on `locale_changed`, `dictation.get_availability`; `unavailable` → no microphone button anywhere; `needs_assets` → the button shows and starting shows "Getting dictation ready…" on `preparing`. Pressing the button sends `dictation.start { field }` with the field's `FieldRef` (*chosen here*: `conversation-message-box` or `form:<block id>:<field name>`, ≤ 128 characters); the reply's `session_id` selects the `StreamMessage`s of kind `dictation` whose `stream` equals it. `partial` replaces the provisional tail after the last `final` (drawn dimmed); `final` appends to the field and is no longer replaced; `ended` removes the provisional tail and resets the button (reason `failed` shows `dictation-failed` with **Try again**). Pressing again sends `dictation.stop { session_id }`; Escape or leaving the field sends `dictation.cancel { session_id }`. Nothing is sent until the person presses the field's own send or submit control; dictated words never act on a card, and an approval card has no dictation control.

## Data Model

The module owns no table and no file. View preferences "remembered on that computer only" live in the WebView's `localStorage`, keys prefixed `ordin.view.` *(chosen here)*:

| Key | Value | Default |
|---|---|---|
| `ordin.view.panel.rail` | width in CSS pixels, 200–480 | 280 |
| `ordin.view.panel.conversation` | width in CSS pixels, 280–640 | 380 |
| `ordin.view.tree.show` | `{ "<workspace id>": { "documents": bool, "code": bool } }` (one always true) | both true |
| `ordin.view.tabs` | `{ "<backend id>:<workspace id>": ["<screen key>", …] }`, at most 6 each | empty |
| `ordin.view.closed` | `{ "<backend id>:<workspace id>": ["<block id>", …] }` — items folded with **Close** | empty |
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
    Then the person reads "Northwind server runs a different version of ordin. Ask whoever runs it to update it."

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
    Then it shows the message, the button "Send to IT" and "Reference r-81"

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

1. **Workspace header.** While `team_knowledge.status { ws }` has a `binding`, the workspace header shows `team-header` with `header_line` ("Team knowledge: acme/supplier-team › knowledge"), and in its **⋯** menu **Stop sharing with the team** (`team-stop`). Without a binding, the workspace's **⋯** menu offers **Share knowledge with a team…** (`team-share-with-team`). The status is read when the workspace opens and re-read with `team_knowledge.status` on every `workspace_changed` for that workspace, which is how the outcome of a check, get or share reaches the screen.
2. **Share knowledge with a team… (dialog).** One list of `team_knowledge.candidate_links { ws }` (each by its display name) and a second choice, one field `team-share-dialog-address` for the folder's web address as the host shows it (`TeamFolderChoice::NewFolder`). The screen splits the address, so the person never types a branch: `https://<host>/<owner>/<name>/tree/<branch>/<path>` (GitHub) or `https://<host>/<group>/<name>/-/tree/<branch>/<path>` (GitLab) → `url = https://<host>/<owner or group>/<name>`, `branch` = the segment after `tree/`, `path` = the rest (empty for the repository root); a branch name containing `/` is read as its first segment (*chosen here*; a folder on such a branch is chosen from the linked folders instead). Any other text → `team-share-dialog-address-invalid` under the field, nothing sent. The lines `team-share-dialog-all` and `team-share-dialog-history` sit above the verb **Start sharing**, which sends `team_knowledge.turn_on`. A refusal is drawn under the fields with `ErrorLine` (`TEAM_LINK_NOT_SUITABLE`, `TEAM_NO_ACCESS`, `TEAM_SIGN_IN_NEEDED` with **Sign in again**, `TEAM_REMOTE_UNREACHABLE` with **Try again**, `TEAM_FORMAT_NEWER`); focus stays in the dialog.
3. **Team knowledge panel** (under the header, one line each, shown only when it applies):
   - `busy` → one progress line in place of the panel's buttons: `team-busy-checking`, `team-busy-getting` or `team-busy-sharing`; **Get them** and **Share what we've learned** are disabled while it shows. `team_knowledge.check_now`, `team_knowledge.get` and `team_knowledge.share` answer at once with the status (`busy` set); the screen draws that status and waits for `workspace_changed`.
   - `last_error` → its owner's sentence with its action through `ErrorLine` (**Try again** re-sends the command that failed; **Sign in again** opens the sign-in), until the next status without it.
   - `available` → `team-available` with the counts and up to five names, and **Get them** (`team_knowledge.get`); when `last_get` arrives the line is replaced by the new status, which no longer carries those items.
   - **Share what we've learned** (`team-share`) → `team_knowledge.share`; when `last_share` arrives, the card it raised is an ordinary approval card (step 7, drawn unchanged), also listed in *Needs you*. When `share_unavailable` is set the button is disabled and its sentence (for `TEAM_READ_ONLY`: "You can read this team's knowledge but not add to it.") is shown beside it, tied by `aria-describedby`. `TEAM_NOTHING_TO_SHARE` in `last_error` is drawn as `team-nothing-to-share`, not as an error.
   - `waiting_for_review` → `team-waiting-review` with **Open the review** (the request's address, through the system browser as other external links are opened).
   - `held_for_check > 0` → `team-held-for-check`; choosing it opens What Ordin remembers for the workspace filtered to the facts waiting for a check, each with **Keep it** (`memory.keep_checked { id }`) and **Forget** (`memory.forget { id }`).
   - `problems` non-empty → a **Not taken in** list (`team-not-taken-title`), one row per problem with `team-not-taken-file` or `team-not-taken-fact`.
   - `folder_large` → `team-folder-large`.
4. **Stop sharing with the team** opens a confirmation (`team-stop-confirm`, verb `team-stop-verb`, **Cancel** first in focus); confirming sends `team_knowledge.turn_off`.
5. **Who shared it.** In What Ordin remembers and in the knowledge page list and page screen, an item that came from a colleague shows `team-shared-by` with the sharer's name and the date in the person's locale; a page that cites files only on the sharer's computer also shows `team-cites-their-files`.
6. A workspace on a connected server shows the same header, panel and dialog through the Remote API; nothing differs on screen.

### Error Table (rows added)

N/A — no new code; the `TEAM_*` and `MEM_*` refusals are drawn by `ErrorLine` as their owners' sentences.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — turning team knowledge on shows the header
    Given workspace "Q3 supplier review" with no binding
    When the person chooses Share knowledge with a team…, picks the linked folder "supplier-team/knowledge" and presses Start sharing
    Then team_knowledge.turn_on is sent with choice link
    And the header shows "Team knowledge: acme/supplier-team › knowledge"

  Scenario: Edge case — the dialog says what will be shared before the first share
    When the Share knowledge with a team… dialog opens
    Then it shows "What you share stays in the repository's history, even after you delete it."

  Scenario: Happy path — new items from colleagues
    Given team_knowledge.status has available 8 new facts and 1 new page from "Priya" and "Sam"
    Then the panel shows "8 new facts and 1 page from Priya and Sam" with Get them
    When Get them is pressed
    Then team_knowledge.get is sent and the panel shows "Getting what the team has learned…"
    And on workspace_changed the screen re-reads team_knowledge.status and, with busy cleared and last_get set, the line is gone

  Scenario: Edge case — read-only access
    Given share_unavailable is TEAM_READ_ONLY
    Then Share what we've learned is disabled and "You can read this team's knowledge but not add to it." is shown beside it

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
2. **Pages that link here.** Below **Where this comes from**, `knowledge-links-in-title` and one row per `links_in` page (its title; activating opens it); none → `knowledge-links-in-none`. **Links on this page** (`knowledge-links-out-title`) lists `links_out` once per target, unresolved ones with `knowledge-link-missing`; `links_truncated > 0` adds `knowledge-links-truncated`.
3. **Other names and tags.** Under the title: `aliases` as `knowledge-other-names` and `tags` as chips. In **Edit**, two fields below the title: **Other names** (one per line, `knowledge-other-names-hint`) and **Tags** (comma-separated, `knowledge-tags-hint`); **Save** sends `knowledge.save_edit` with `tags` and `aliases` only when either field changed. `unregistered_aliases` shows `knowledge-name-in-use` per name; `title_refused` shows `knowledge-title-in-use`.
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
    - **Language** picker (`language-picker-label`): a WAI-ARIA listbox of `language-follow-computer` (sends `localisation.set_locale { locale: null }`) and every `offered` language by `native_name` with its tag ("Français — fr-FR"); a person's language carries `language-made-by-you` or `language-imported`. A language with `choosable: false` is not listed. Choosing one sends `localisation.set_locale { locale }`.
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

1. **Header.** A workspace with an own folder keeps `team-header`. A workspace with no own folder but at least one folder in `folders` (company-named folders, `applies_to: code`) shows `team-no-own-folder` in the panel in place of **Share what we've learned**; **Share knowledge with a team…** stays in the **⋯** menu.
2. **Also reads** (`team-also-reads`), under the own folder's lines, one block per entry of `folders`, in the order given:
   - the folder's `name`, its `description` in secondary text when present, and the `team-set-by-company` tag when `set_by_company`;
   - `team-folder-counts` with `held.facts` and `held.pages`;
   - `available` → `team-available` and **Get them**, which sends `team_knowledge.get { ws, folder_id }`; `busy` and `last_error` draw as for the own folder;
   - `blocked` → its sentence through `ErrorLine`: `TEAM_COMPANY_FOLDER_NO_ACCESS` as `team-folder-no-access` with the action **Ask IT** (`send_to_it`, step 11's help card); `TEAM_REVIEW_NOT_REQUIRED` and `TEAM_REVIEW_CANNOT_TELL` as their messages, with no action;
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
    Then its block shows "You can't open Security standards yet." with Ask IT
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

1. **What Ordin remembers — two tabs** (the `memory` view of step 6 and the fixed view of step 9, Settings › What Ordin remembers, and the tree's *What Ordin remembers* node): **Facts** (`memory-tab-facts`, first, as before) and **Things** (`memory-tab-things`), a WAI-ARIA tab list; the chosen tab is a view preference per workspace. Agents' own pages, **Open Knowledge** and the Team knowledge lines stay under both tabs as before.
2. **Each fact line** (Facts tab, a thing's facts, **What Ordin used**, **Remembered during this run**), drawn by one component `FactLine`, adds to the existing statement, origin and actions:
   - its kind in words from `kind` (`memory-kind-happened`, `memory-kind-known`, `memory-kind-summary`) as secondary text;
   - `memory-true-since` with `true_from`, or `memory-true-between` when `true_until` is present;
   - when `replaces` is present: `memory-replaced` with the replaced fact's statement, read through `memory.get` for that id within the same workspace (a `MEM_FACT_NOT_FOUND` answer drops the line, never an error);
   - when `fading`: the notice `memory-fading` with `not_used_since` and **Keep** (`memory-keep`), which sends `memory.keep { id }`; on success the notice goes, the kind reads `memory-kind-known`, and `memory-kept` is announced (polite live region); `MEM_ALREADY_KEPT` redraws the line from the reply's fact with no message;
   - **History** (`memory-history`) in the line's **⋯** menu: opens a dialog titled `memory-history-title` listing `memory.fact_history { id }`'s `versions` oldest first as `memory-history-version` (`when` = `memory-true-since` or `memory-true-between` of that version), with the links of the reply under them as link lines (step 4, without actions);
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
    Given memory.thing returns a live link from "Borealis" kind "supplies" to "packaging" since 3 Oct
    Then its detail shows "Borealis supplies packaging · since 3 Oct" with Correct and Forget

  Scenario: Happy path — a fact corrected twice shows its kind, truth and history (criterion 10)
    Given a FactView with kind known, true_from 12 Oct, replaces F2 whose statement is "Prefers pricing in dollars"
    Then its line shows "Something known", "True since 12 Oct" and "Replaced "Prefers pricing in dollars""
    When the person chooses History
    Then memory.fact_history is sent and three versions are listed oldest first, the first two with "True from … to …"

  Scenario: Happy path — a fading fact can be kept (criterion 11)
    Given a FactView with fading true and not_used_since 20 Sep
    Then its line shows "Fading — not used since 20 Sep" with Keep
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
    Given memory.used returns a wider source from agent "Supplier knowledge", repository "git.example.org/acme/contracts@main", path "suppliers/borealis.md", commit "9f2c1e7a0b", page_title "Supplier Borealis", section "Contract terms", as_of 22 Oct 09:40, answer_id A
    When the person opens What Ordin used
    Then the line shows "From Supplier knowledge · acme/contracts › borealis.md · version 9f2c1e7", "Supplier Borealis › Contract terms", "As of 22 Oct 09:40" and "Answer reference A"
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
2. **SM-UP-2 Settings › About** (`settings-section-about`), the last section after Diagnostics, a fixed view `Settings/About`, local backend only. It draws, in order: the brand's product name and `about-version` with the application's version, read once through `lib/backend/tauri.ts` from the application-version call of `@tauri-apps/api` 2.12.4 (*chosen here*; SA-UI-UP-04); **Check for updates** (`about-check`); one status line in a polite live region; **What's new** (`about-whats-new`).
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
