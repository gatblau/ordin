// The shell's scenes for the side-by-side review (cross-cutting/VisualDesign.md, Internal Logic step 7):
// the tree, the conversation panel, Search or go to…, Settings, the engineering screens and Earlier
// conversations, with the prototype's sample names. Sample data is illustrative (12 §2).
import type { AppContext } from '../src/lib/shell/context';
import { openScreen } from '../src/lib/shell/open-item';
import { makeKey, SCREEN_IDS } from '../src/lib/shell/screens';
import { event, hello, LOCAL, R1_GROUPS, SERVER, type FakeTransport, type ok as okFn } from '../tests/support/fake-backend';
import { blocks, conversation, home, WS, ws, workspaces, type Seed } from './scenes';

type Ok = typeof okFn;
const after = (seed: Seed, then: (app: AppContext, t: FakeTransport, local: string) => void): Seed =>
  Object.assign((t: FakeTransport, local: string, ok: Ok) => seed(t, local, ok), {
    after: (app: AppContext, t: FakeTransport, local: string) => {
      seed.after?.(app, t, local);
      then(app, t, local);
    },
  });
const later = (fn: () => void, ms = 80): void => void setTimeout(fn, ms);

// The groups of a build that also answers email, so the tree shows Email as the prototype's does.
const GROUPS = [...R1_GROUPS, 'mail'];

// Messages in Q3 supplier review's conversation, as the prototype's scenes say them.
const say = (t: FakeTransport, local: string, from: number, lines: string[]): void =>
  t.emit(...lines.map((text, i) => event(local as never, from + i, { kind: 'block_shown', workspace_id: WS.q3, block_id: `say${from + i}`, block: { kind: 'message', text, attachments: [] } as never })));

const EARLIER = [
  { conversation_id: '0192f2c0-0000-7000-8000-000000000001', first_request: 'Compare the supplier quotes for me', message_count: 14, started_at: '2026-01-11T09:12:00.000Z', ended_at: '2026-01-11T11:40:00.000Z', state: 'earlier' },
  { conversation_id: '0192f2c0-0000-7000-8000-000000000002', first_request: 'Update the supplier summary with the new Q3 rates', message_count: 23, started_at: '2026-01-09T14:03:00.000Z', ended_at: '2026-01-09T16:20:00.000Z', state: 'earlier' },
  { conversation_id: '0192f2c0-0000-7000-8000-000000000003', first_request: 'Which suppliers have not sent a Q3 rate yet?', message_count: 6, started_at: '2026-01-05T10:30:00.000Z', ended_at: '2026-01-05T10:52:00.000Z', state: 'earlier' },
  { conversation_id: '0192f2c0-0000-7000-8000-000000000009', first_request: 'Send the changes back', message_count: 31, started_at: '2026-01-14T13:00:00.000Z', state: 'live' },
];
const withEarlier = (t: FakeTransport, ok: Ok): void => {
  t.on('service.hello', ok(hello(LOCAL, GROUPS)));
  t.on('ordin.list_conversations', ok({ items: EARLIER, next_cursor: null }));
};

/** `tree`: Q3 supplier review open in the tree, the crew's progress in the centre. */
const tree: Seed = Object.assign(
  (t: FakeTransport, local: string, ok: Ok) => {
    blocks('progress')(t, local, ok);
    withEarlier(t, ok);
  },
  { after: blocks('progress').after! },
);

/** `long`: a long conversation in Q3 supplier review. */
const long: Seed = after(
  Object.assign((t: FakeTransport, local: string, ok: Ok) => {
    conversation(t, local, ok);
    withEarlier(t, ok);
  }, { after: conversation.after! }),
  (_app, t, local) =>
    say(t, local, 20, [
      'Here’s a draft: “Could you send your Q3 day rate by Thursday?”',
      'Make it a bit friendlier',
      '“Hello — could you send your Q3 day rate by Thursday? Thank you.”',
      'Send the changes back',
    ]),
);

/** `switch`: Q3 supplier review's documents in the centre, the other workspaces in the tree. */
const switchScene: Seed = blocks('docs');

/** `needsTab`: the Needs you tab chosen, three things waiting in two workspaces. */
const needsTab: Seed = Object.assign(
  (t: FakeTransport, local: string, ok: Ok) => {
    conversation(t, local, ok);
    const row = (line: string, workspace: string, name: string, opens: unknown) => ({ at: '2026-01-10T10:00:00.000Z', is_new: false, line, opens, run_again: false, workspace_id: workspace, workspace_name: name });
    t.on('home.needs_you', ok([
      row('Update the documents, or compare Q3 with Q2?', WS.q3, 'Q3 supplier review', { kind: 'workspace', workspace_id: WS.q3 }),
      row('Fill in: Compare supplier quotes', WS.q3, 'Q3 supplier review', { kind: 'workspace', workspace_id: WS.q3 }),
      row('Send 1 change back?', WS.onboarding, 'Onboarding guide refresh', { kind: 'approval', approval_id: 'a1', workspace_id: WS.onboarding }),
    ]));
  },
  {
    after: (app: AppContext, t: FakeTransport, local: string) => {
      conversation.after!(app, t, local);
      later(() => {
        app.shell.activeConversation = 'needs-you';
        (document.getElementById('conv-tab-needs-you') as HTMLButtonElement | null)?.click();
      });
    },
  },
);

/** The Earlier conversations screen: one row per conversation (no prototype scene; drawn as `docs`'s list). */
const earlierList: Seed = after(
  Object.assign((t: FakeTransport, local: string, ok: Ok) => {
    conversation(t, local, ok);
    withEarlier(t, ok);
  }, { after: conversation.after! }),
  (app, _t, local) => later(() => openScreen(app, local as never, WS.q3, SCREEN_IDS.earlierConversations)),
);

// ---- engineering ---------------------------------------------------------------------------------

const PROJECT = '0192f3a0-0000-7000-8000-0000000000c1';
const task = (id: string, request: string, branch: string, over: Record<string, unknown> = {}) => ({
  agent_instance_id: '0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff', base_branch: 'q3-rates', base_commit: '4f2a9c1', branch, id, new_problems: 0,
  origin: { kind: 'conversation' }, project_id: PROJECT, request, state: 'ready_for_review', test_outcome: 'passed', worktree_path: `/work/${id}`, ...over,
});
const changeSet = (taskId: string, entries: { file: string; hunk: string }[]) => ({
  coding_task_id: taskId, created_at: '2026-01-14T13:00:00.000Z', id: `cs-${taskId}`, project_id: PROJECT, run_id: '0192a1c2-3344-7556-8677-8899aabbccdd', state: 'in_review',
  entries: entries.map((e, i) => ({ ...e, id: `${taskId}-e${i}`, state: 'pending' })),
});
const HUNK_RATES = '@@ -12,7 +12,7 @@ func Rates(s Supplier) []Rate {\n-\treturn s.Q2\n+\treturn s.Q3\n }';
const HUNK_TEST = '@@ -4,3 +4,6 @@\n+func TestRatesQ3(t *testing.T) {\n+\tif len(Rates(acme)) != 4 { t.Fatal("want 4") }\n+}';
const HUNK_A = '@@ -6,1 +6,1 @@\n-func dayRate(r float64) float64 { return math.Ceil(r) }\n+func dayRate(r float64) float64 { return math.Round(r) }';
const HUNK_B = '@@ -6,1 +6,1 @@\n-func dayRate(r float64) float64 { return math.Ceil(r) }\n+func dayRate(r float64) float64 { return r }';

const engineering = (reviews: unknown[]): Seed =>
  after(
    Object.assign((t: FakeTransport, local: string, ok: Ok) => {
      conversation(t, local, ok);
      t.on('projects.review', ok(reviews));
      t.on('projects.status', ok([{ path: 'internal/invoice.go', status: 'modified' }]));
    }, { after: conversation.after! }),
    (app, _t, local) => later(() => openScreen(app, local as never, WS.q3, makeKey('changes-waiting', PROJECT), { title: 'Changes waiting' })),
  );

/** `eng`: Supplier analyst's change in supplier-portal, waiting for review. */
const eng = engineering([
  { task: task('t1', 'Use the Q3 rates in the portal', 'ordin/q3-rates'), new_problems: [], change_set: changeSet('t1', [{ file: 'internal/rates.go', hunk: HUNK_RATES }, { file: 'internal/rates_test.go', hunk: HUNK_TEST }]) },
]);

/** `engTwo`: two attempts at one change, side by side. */
const engTwo = engineering([
  { task: task('ta', 'Round day rates to the nearest pound', 'ordin/rounding-a', { attempt_group_id: 'g1' }), new_problems: [], change_set: changeSet('ta', [{ file: 'internal/invoice.go', hunk: HUNK_A }]) },
  {
    task: task('tb', 'Keep the quote exact; round only the invoice total', 'ordin/rounding-b', { attempt_group_id: 'g1', new_problems: 1 }),
    new_problems: [{ file: 'internal/invoice.go', line: 3, message: '"math" imported and not used', severity: 'error' }],
    change_set: changeSet('tb', [{ file: 'internal/invoice.go', hunk: HUNK_B }]),
  },
]);


const INVOICE_GO = 'package internal\n\nimport "math"\n\n// dayRate rounds a day rate to whole pounds\nfunc dayRate(r float64) float64 { return math.Ceil(r) }\n';
/** `engEdit`: internal/invoice.go open in the code editor, main.rs on the tab before it. */
const engEdit: Seed = after(
  Object.assign((t: FakeTransport, local: string, ok: Ok) => {
    conversation(t, local, ok);
    t.on('projects.get', ok({ id: PROJECT, display_name: 'supplier-portal', place_id: `${WS.q3}-code`, check_commands: ['go test ./...'], current_branch: 'q3-rates', is_git_repository: true, language_servers_in_use: ['gopls'], opened_at: '2026-01-14T13:00:00.000Z', state: 'open' }));
    t.on('projects.editor_open', (input) => ok({ asks_to_start_servers: false, server_line: (input as { path: string }).path.endsWith('.go') ? 'Go language server: gopls v0.20.0' : 'Rust language server: rust-analyzer', stream: `s-${(input as { path: string }).path}`, language: null }));
    t.on('projects.editor_close', ok({}));
    t.on('files.preview', (input) => ok({ kind: 'text', text: (input as { path: string }).path.endsWith('.go') ? INVOICE_GO : 'fn main() {\n    println!("rates");\n}\n', truncated: false }));
  }, { after: conversation.after! }),
  (app, _t, local) =>
    later(() => {
      for (const path of ['tools/ratecheck/src/main.rs', 'internal/invoice.go']) openScreen(app, local as never, WS.q3, makeKey('code-editor', `${PROJECT}:${path}`), { title: path.split('/').pop()! });
    }),
);

// ---- Settings ------------------------------------------------------------------------------------

const set = <T>(value: T, set_by: 'person' | 'company' = 'person') => ({ value, set_by });
const SETTINGS_GROUPS = [...GROUPS, 'voice', 'history', 'diagnostics'];
const settingsAt = (section: string | null, extra: (t: FakeTransport, ok: Ok) => void = () => {}): Seed =>
  Object.assign(
    (t: FakeTransport, local: string, ok: Ok) => {
      home(t, local, ok);
      t.on('service.hello', ok(hello(LOCAL, SETTINGS_GROUPS)));
      t.on('identity.list', ok({ items: [
        { id: 'c1', kind: 'github', account_label: 'priya-shah', state: 'signed_in' },
        { id: 'c2', kind: 'microsoft', account_label: 'priya.shah@northwind-group.com', state: 'needs_sign_in' },
        { id: 'c3', kind: 'confluence', account_label: 'northwind.atlassian.net', state: 'signed_out' },
        { id: 'c4', kind: 'slack', account_label: 'Northwind', state: 'signed_out' },
      ] }));
      t.on('usage.today', ok({ typical_day_permille: 200, model_turns: 34, smaller_share_permille: 820, answered_without_model: 11 }));
      t.on('usage.get_settings', ok({
        agents_at_same_time: set(2), answer_simple_requests_without_model: set(true), ask_before_estimated_requests: set('heavy'), choice: set('balanced'),
        continue_from_summary_after_break: set(true), daily_pause_requests: set(null), give_agents_only_needed_parts: set(true), model_level_per_kind: set({}),
        reuse_answers: set(true), show_usage_on_each_run: set(false), summarise_long_conversations: set('when_nearly_full'), thinking: set('automatic'), updated_at: '2026-01-14T13:00:00.000Z',
      }));
      t.on('voice.get', ok({ tone: set('friendly', 'company'), register: set('everyday'), length: set('brief'), name_to_use: set('Priya'), notes: set(''), updated_at: '2026-01-14T13:00:00.000Z' }));
      t.on('history.get', ok({ days: 90, company_max_days: 365 }));
      t.on('diagnostics.lines', ok({ items: [], next_cursor: null }));
      extra(t, ok);
    },
    { after: (app: AppContext, _t: FakeTransport, local: string) => later(() => openScreen(app, local as never, null, makeKey(SCREEN_IDS.settings, section))) },
  );

const settings = settingsAt(null);
const useSettings = settingsAt('using-your-plan');
const voiceSettings = settingsAt('how-ordin-talks');
const historySetting = after(settingsAt('history', (t, ok) => t.on('history.set_days', ok({ days: 90 }))), () =>
  later(() => {
    // The prototype's state: 0 typed, refused in place.
    const box = document.getElementById('history-days') as HTMLInputElement | null;
    if (box === null) return;
    box.value = '0';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    box.dispatchEvent(new Event('change', { bubbles: true }));
  }, 400),
);

// ---- Search or go to… ----------------------------------------------------------------------------

const doc = (workspace: string, path: string) => ({ kind: 'document', workspace_id: workspace, place_id: `${workspace}-p`, path });
const page = (workspace: string, id: string) => ({ kind: 'knowledge_page', workspace_id: workspace, page_id: id });
const find = (query: string, answer: Record<string, unknown>): Seed =>
  after(
    Object.assign((t: FakeTransport, local: string, ok: Ok) => {
      conversation(t, local, ok);
      t.on('search.query', ok({ query, complete: true, go_to: [], found: [], actions: [], ...answer }));
    }, { after: conversation.after! }),
    (app) =>
      later(() => {
        app.shell.searchOpen = true;
        later(() => {
          const box = document.querySelector<HTMLInputElement>('.dialog.search input');
          if (box === null) return;
          box.value = query;
          box.dispatchEvent(new Event('input', { bubbles: true }));
        }, 60);
      }),
  );

/** `findAll`: “notice” found in pages and documents of the workspace in front. */
const findAll = find('notice', {
  found: [
    { label: 'Acme Ltd', snippet: '…om Q3 (3.9% rise; within the 5% cap). · Notice period: 30 days, in writing. · Payment terms: 30 …', workspace_id: WS.q3, opens: page(WS.q3, 'k1') },
    { label: 'Kestrel', snippet: '…lier summary shows “to be confirmed”. · Notice period: 60 days, in writing, under the framework …', workspace_id: WS.q3, opens: page(WS.q3, 'k2') },
    { label: 'Acme contract 2026.docx', snippet: 'Notice period: 30 days, in writing. Payment terms: 45 da…', workspace_id: WS.q3, opens: doc(WS.q3, 'Contracts 2026/Acme contract 2026.docx') },
    { label: 'Borealis framework.docx', snippet: '…r 12 months; either side gives 90 days’ notice.…', workspace_id: WS.q3, opens: doc(WS.q3, 'Contracts 2026/Borealis framework.docx') },
    { label: 'Supplier onboarding policy', snippet: 'A new supplier gives a notice period of at least 30 days.…', workspace_id: WS.q3, opens: page(WS.q3, 'k3') },
    { label: 'Quote — Kestrel.pdf', snippet: 'Day rate £405. Notice period of 60 days from the start of the contract.…', workspace_id: WS.q3, opens: doc(WS.q3, 'Supplier quotes/Quote — Kestrel.pdf') },
  ],
});

/** `findCount`: “a” matches in Go to across workspaces, then pages and documents, then Actions. */
const findCount = find('a', {
  go_to: [
    { kind: 'workspace', label: 'Onboarding guide refresh', detail: 'Workspace', workspace_id: WS.onboarding, opens: { kind: 'workspace', workspace_id: WS.onboarding } },
    { kind: 'agent', label: 'Onboarding writer', detail: 'Agent', workspace_id: WS.onboarding, opens: { kind: 'workspace', workspace_id: WS.onboarding } },
    { kind: 'knowledge_page', label: 'Laptop set-up on day 2', detail: 'Knowledge page', workspace_id: WS.onboarding, opens: page(WS.onboarding, 'k9') },
    { kind: 'conversation', label: 'Draft the welcome pack for October starters', detail: 'Earlier conversation · 5 Jan, 09:30', workspace_id: WS.onboarding, opens: { kind: 'workspace', workspace_id: WS.onboarding } },
    { kind: 'document', label: 'Welcome pack 2026.docx', detail: 'SharePoint › New starters', workspace_id: WS.onboarding, opens: doc(WS.onboarding, 'New starters/Welcome pack 2026.docx') },
    { kind: 'document', label: 'Office map.png', detail: 'SharePoint › New starters', workspace_id: WS.onboarding, opens: doc(WS.onboarding, 'New starters/Office map.png') },
    { kind: 'document', label: 'Starter survey results.xlsx', detail: 'SharePoint › New starters', workspace_id: WS.onboarding, opens: doc(WS.onboarding, 'New starters/Starter survey results.xlsx') },
  ],
  found: [{ label: 'Kestrel', snippet: 'Q2 day rate £395. Its Q3 quote is £405 a day, not yet c…', workspace_id: WS.q3, opens: page(WS.q3, 'k2') }],
  actions: ['home', 'new_workspace', 'summarise', 'earlier_conversations', 'settings'].map((name) => ({ kind: 'workspace', label: name, opens: { kind: 'screen', name } })),
});

// ---- a connected server --------------------------------------------------------------------------

const SRV = { invoices: '0192f0a1-0000-7000-8000-0000000000e1', payments: '0192f0a1-0000-7000-8000-0000000000e2' };
/** `srvTree`: Northwind server below this computer's workspaces, its Invoice checks open with its card waiting. */
const srvTree: Seed = Object.assign(
  (t: FakeTransport, local: string, ok: Ok) => {
    home(t, local, ok);
    const mine = (env: { backend: string }) => env.backend === SERVER;
    t.on('service.hello', (_i, env) => ok(mine(env) ? hello(SERVER, GROUPS, 0, 'personal_server') : hello(LOCAL, GROUPS)));
    t.on('servers.list', ok([{ server_connection_id: 'sc1', display_name: 'Northwind server', state: 'connected', backend: SERVER }]));
    t.on('workspaces.list', (_i, env) =>
      ok(mine(env) ? [ws(SRV.invoices, 'Invoice checks'), ws(SRV.payments, 'Supplier payments')] : [ws(WS.onboarding, 'Onboarding guide refresh'), ws(WS.northwind, 'Northwind account brief'), ws(WS.q3, 'Q3 supplier review')]),
    );
    const homeRows = t.replies.get('home.needs_you');
    t.on('home.needs_you', (i, env) =>
      mine(env)
        ? ok([{ at: '2026-01-10T10:00:00.000Z', is_new: false, line: 'Send the payment query to Northwind Ltd?', opens: { kind: 'approval', approval_id: 'sa1', workspace_id: SRV.invoices }, run_again: false, workspace_id: SRV.invoices, workspace_name: 'Invoice checks' }])
        : typeof homeRows === 'function' ? homeRows(i, env) : (homeRows as never),
    );
    try {
      localStorage.setItem('ordin.view.tree.expanded', JSON.stringify([`${SERVER}:${SRV.invoices}`]));
    } catch {
      // A private window: the tree stays folded.
    }
  },
  {
    after: (app: AppContext, t: FakeTransport) =>
      later(() => {
        app.shell.openWorkspaceTab(SERVER as never, SRV.invoices);
        t.emit(event(SERVER as never, 1, { kind: 'block_shown', workspace_id: SRV.invoices, block_id: 'sm1', block: { kind: 'message', text: 'This is your ordin on Northwind server. I keep working here when your computer is closed.', attachments: [] } as never }));
      }, 200),
  },
);

export const SCENES: Record<string, Seed> = { tree, long, switch: switchScene, needsTab, earlierList, eng, engTwo, settings, useSettings, voiceSettings, historySetting, findAll, findCount, srvTree, engEdit };
void home;
void ws;
void workspaces;
