// Seeds for the agents, crews, memory, knowledge, documents and changes scenes of the side-by-side review
// (cross-cutting/VisualDesign.md, Internal Logic step 7). Replies are shaped as the Service API types; the
// sample names are the prototype's. Sample data is illustrative.
import { blocks, conversation, file, home, SCENES as BASE, type Seed } from './scenes';
import { openScreen } from '../src/lib/shell/open-item';
import { makeKey, SCREEN_IDS } from '../src/lib/shell/screens';
import type { AppContext } from '../src/lib/shell/context';
import { event, hello, LOCAL, R1_GROUPS, type FakeTransport, type ok as okFn } from '../tests/support/fake-backend';
import { BLOCK_SCREEN_ID } from '../src/lib/blocks/block-screen';
import { blockTitle } from '../src/lib/blocks/titles';
import type { Block } from '../src/lib/service-api/types.gen';

const REPO = '0192a1c5-1111-7222-8333-444455556666';
const part = (id: string, name: string, from: string, summary: string) => ({
  from,
  offered: true,
  state_label: 'available',
  summary,
  part: {
    agent_repository_id: REPO, commit: 'a1b2c3d', content_digest: '0'.repeat(64), file_path: `agents/${id}.yaml`, id, identifier: `urn:ordin:${id}`,
    kind: 'WorkspaceProfile', name, owner: from, status: 'published', supersedes: null, uses: [], version: '1.0.0', vocabulary_terms_cited: [], withdrawn: false,
  },
});
const LIBRARY = [
  ['contract-clause-checker', 'Contract clause checker', 'Company agents', 'Finds and compares clauses in contracts, and lists the ones that are missing or unusual.'],
  ['spend-analyser', 'Spend analyser', 'Company agents', 'Totals spend by supplier and quarter from spreadsheets and invoices.'],
  ['account-planner', 'Account planner', 'Company agents', 'Prepares account briefs from contracts, plans and notes.'],
  ['contract-reviewer', 'Contract reviewer', 'Company agents', 'Summarises a contract’s risks for a manager, in one page.'],
  ['expense-filer', 'Expense filer', 'Company agents', 'Files expense claims from the receipts in a folder.'],
];
const REPOS_LIBRARY = [
  ['release-planner', 'Release planner', 'Engineering agents', 'Plans releases and drafts go/no-go notes.'],
  ...LIBRARY.slice(0, 2),
  ['purchase-order-checker', 'Purchase order checker', 'Procurement team agents', 'Checks purchase orders against the approved supplier list and the budget.'],
  ...LIBRARY.slice(2),
];
const library = (rows: string[][]) => (t: FakeTransport, ok: typeof okFn) => {
  t.on('catalogue.search', ok({ items: rows.map(([id, name, from, summary]) => ({ from, part: part(id!, name!, from!, summary!) })), next_cursor: null }));
};

/** A block scene whose seed also answers the commands its views call. */
const withReplies = (name: string, list: unknown[], replies: (t: FakeTransport, ok: typeof okFn) => void): Seed => {
  const base = blocks(name, list);
  return Object.assign((t: FakeTransport, local: string, ok: typeof okFn) => {
    base(t, local, ok);
    replies(t, ok);
  }, { after: base.after });
};

// Agents installed in Q3 supplier review (`agents.list`), as the prototype's tree lists them.
const AGENT_IDS = { analyst: '0192a0f9-aaaa-7bbb-8ccc-000000000001', deck: '0192a0f9-aaaa-7bbb-8ccc-000000000002' };
const installed = (id: string, name: string) => ({
  instance: { id, display_name: name, workspace_id: WS_Q3, state: 'ready', stopped_reason: null },
  tasks: [], update: null, uses: [], workspaces: [WS_Q3],
});
const WS_Q3 = '0192f0a1-0000-7000-8000-000000000003';
const agents = (t: FakeTransport, ok: typeof okFn) => t.on('agents.list', ok([installed(AGENT_IDS.analyst, 'Supplier analyst'), installed(AGENT_IDS.deck, 'Deck builder')]));
/** Answers the groups an R1 build has, and the extra ones a scene's views need. */
const groups = (t: FakeTransport, ok: typeof okFn, extra: string[]) => t.on('service.hello', ok(hello(LOCAL, [...R1_GROUPS, ...extra])));

// Crew plan (scenes `crew` and `useEstimate`): the prototype's crew of Supplier analyst and Deck builder.
const CHARTER = '0192a1c6-0000-7000-8000-000000000001';
const crew = (t: FakeTransport, ok: typeof okFn) => {
  groups(t, ok, ['crews']);
  agents(t, ok);
  t.on('crews.charter', ok({
    charter: {
      allowed_handoffs: [], charter_version: 1, coordinator: 'ordin', id: CHARTER, input_fields: [], name: 'Crew plan — supplier rates',
      purpose: 'Bring the Supplier summary, rates.md and the review deck in line with “Supplier rates Q3.xlsx”.',
      members: [
        { member: { kind: 'agent_instance', agent_instance_id: AGENT_IDS.analyst }, role: 'Reads the rates, changes the summary and rates.md' },
        { member: { kind: 'agent_instance', agent_instance_id: AGENT_IDS.deck }, role: 'Updates the slides' },
      ],
      guardrails: { approval_points: [], failure_repeat_limit: 2, max_members_running: 2, max_rounds: 6, members_allowed_external_actions: [], request_budget: 40, wall_clock_secs: 1200 },
      state: 'valid', steps: [], workflow_id: null, workflow_version: null, workspace_id: WS_Q3, written_at: '2026-01-14T13:00:00.000Z',
    },
  }));
};
const estimate = { ask_first: false, band: 'medium', based_on: [{ kind: 'limits' }], min_turns: 12, max_turns: 30, typical_day_permille: 80 };

// What Ordin remembers (scene `memory`): the prototype's five facts.
const RUN1 = '0192a1c2-3344-7556-8677-000000000001';
const fact = (id: string, statement: string, came_from: unknown, written_at: string, over: Record<string, unknown> = {}) => ({
  id, statement, came_from, kind: 'known', fading: false, may_be_out_of_date: false, needs_check: false, true_from: written_at, written_at, workspace_id: WS_Q3, ...over,
});
const FACTS = [
  fact('f1', 'You prefer “to be confirmed” over showing an old rate.', { kind: 'conversation', run_id: RUN1 }, '2026-01-14T09:00:00.000Z'),
  fact('f2', 'The Supplier summary page is owned by Priya Shah in Procurement.', { kind: 'document', origin_ref: 'Confluence — Supplier summary', version: '13' }, '2026-01-08T09:00:00.000Z'),
  fact('f3', 'Q3 rates come from “Supplier rates Q3.xlsx” in SharePoint.', { kind: 'agent_citing', agent_instance_id: AGENT_IDS.analyst, origin_ref: 'Supplier rates Q3.xlsx', version: '6', run_id: RUN1 }, '2026-01-14T10:00:00.000Z'),
  fact('f4', 'Slides use the company chart style, not the default.', { kind: 'conversation', run_id: RUN1 }, '2026-01-03T09:00:00.000Z', { workspace_id: null }),
  fact('f5', 'Kestrel’s account manager is Tom Reid.', { kind: 'document', origin_ref: 'An email attached to a contract', version: '1' }, '2026-03-12T09:00:00.000Z', { may_be_out_of_date: true }),
];
const memory = (t: FakeTransport, ok: typeof okFn) => {
  agents(t, ok);
  t.on('memory.get', ok({ facts: FACTS }));
  t.on('memory.list', ok({ items: FACTS, next_cursor: null }));
  t.on('memory.things', ok({ items: [], next_cursor: null }));
  t.on('memory.held_writes', ok([]));
  t.on('memory.citations_to_confirm', ok([]));
};

/** A scene that opens one of the fixed screens of Q3 supplier review in the centre. */
const fixedScreen = (id: string, replies: (t: FakeTransport, ok: typeof okFn) => void): Seed =>
  Object.assign((t: FakeTransport, local: string, ok: typeof okFn) => {
    conversation(t, local, ok);
    replies(t, ok);
  }, {
    after: (app: AppContext, t: FakeTransport, local: string) => {
      conversation.after?.(app, t, local);
      setTimeout(() => openScreen(app, local as never, WS_Q3, id), 50);
    },
  });

// Knowledge (scenes `know`, `knLinks`): the prototype's pages of Q3 supplier review.
const summary = (id: string, title: string, last_changed_at: string, over: Record<string, unknown> = {}) => ({
  id, title, last_changed_at, cites_other_computer: false, past_stale_after: false, read_only: false, state: 'current', tags: [], trust: 'machine_confirmed',
  workspace_id: WS_Q3, written_by: 'ordin', ...over,
});
const PAGES = [
  summary('k1', 'How supplier rates are set', '2026-01-14T09:00:00.000Z', { tags: ['rates'] }),
  summary('k2', 'Acme Ltd', '2026-01-09T09:00:00.000Z', { state: 'may_be_out_of_date' }),
  summary('k3', 'Kestrel', '2026-01-07T09:00:00.000Z'),
];
const BODIES: Record<string, string> = {
  k1: '# How supplier rates are set\n\nEach supplier sends a **day rate** for the quarter. Procurement records it in *Supplier rates Q3.xlsx* and publishes it on the Supplier summary page.\n\n- A rise above **5%** on the previous quarter needs Priya Shah’s sign-off.\n- A supplier without a new rate is shown as “to be confirmed”, never at the old rate.\n\nEach supplier’s terms are on its own page: [[Acme Ltd]], [[Kestrel#Notice period]] and [[Borealis]]. Earlier quotes are in [[Q2 quote comparison]].\n',
  k2: '# Acme Ltd\n\nPreferred supplier for contract staff since 2023.\n\n- Day rate: **£428** from Q3 (3.9% rise; within the 5% cap).\n- Notice period: **30 days**, in writing.\n- Payment terms: **30 days**.\n- Account manager: Sara Lund.\n',
  k3: '# Kestrel\n\nQ2 day rate £395. Its Q3 quote is £405 a day, not yet confirmed in writing.\n',
};
const LINKS_OUT: Record<string, unknown[]> = {
  k1: [
    { name: 'Acme Ltd', section: null, target: { page_id: 'k2', title: 'Acme Ltd' } },
    { name: 'Kestrel', section: 'Notice period', target: { page_id: 'k3', title: 'Kestrel' } },
    { name: 'Borealis', section: null, target: null },
    { name: 'Q2 quote comparison', section: null, target: null },
  ],
};
const knowledge = (t: FakeTransport, ok: typeof okFn) => {
  t.on('knowledge.list', ok({ this_workspace: PAGES, from_agents: [{ agent_instance_id: AGENT_IDS.analyst, agent_name: 'Supplier analyst', relative_path: 'knowledge/procurement-rate-rules.md', title: 'Procurement rate rules' }], from_other_teams: [], not_adopted: [] }));
  t.on('knowledge.get', (input) => {
    const id = (input as { page_id: string }).page_id;
    const s = PAGES.find((p) => p.id === id) ?? PAGES[0]!;
    return ok({
      aliases: [], changes: id === 'k2' ? [{ changed_at: '2026-01-14T13:40:00.000Z', changed_by: 'J. Patel', location: 'SharePoint — Procurement › Suppliers', newer_version: '6', origin: 'Acme contract 2026.docx', version_read: '5' }] : [],
      cites_other_computer: false, links_in: id === 'k2' ? [{ page_id: 'k1', title: 'How supplier rates are set' }] : [], links_out: LINKS_OUT[id] ?? [], links_truncated: 0,
      page: { body_markdown: BODIES[id] ?? '', edited_by_person: false, id, last_changed_at: s.last_changed_at, sources: [], state: s.state, title: s.title, workspace_id: WS_Q3, written_by: 'ordin' },
      past_stale_after: false, pending_update: false, read_only: false, relative_path: `knowledge/${id}.md`, suggestions: [], tags: s.tags, trust: 'machine_confirmed', unregistered_aliases: [], verified: [],
    });
  });
};

// Markdown documents (scenes `md`, `mdNew`): the `document` block of rates.md and of a new document.
const MD: Record<string, string> = {
  'supplier-portal/docs/rates.md': '# Supplier day rates\n\nDay rates agreed with each supplier, reviewed every quarter. The **Rate review procedure** in Confluence says how.\n\n| Supplier | Day rate (£) |\n|---|---|\n| Acme | 412 |\n| Kestrel | 395 |\n\n## Notes\n\n- Rates exclude VAT.\n- Borealis is *to be confirmed*.\n',
  'Procurement/Supplier summary (version 13).txt': 'Supplier summary\nAcme Ltd — £412 per day\nBorealis — £440 per day\n',
  'Procurement/Supplier summary.txt': 'Supplier summary\nAcme Ltd — £428 per day (Q3)\nBorealis — to be confirmed\n',
  'Supplier quotes/Q3 rate changes.md': '# Q3 rate changes\n\nWhat changed in supplier day rates this quarter, for the procurement team.\n\n## Changes\n\n| Supplier | Q2 (£) | Q3 (£) |\n|---|---|---|\n| Acme | 412 | 428 |\n| Kestrel | 395 | 395 |\n\n## Still open\n\n- Borealis is *to be confirmed*.\n- Kestrel’s account manager is to be checked.\n',
};
const markdown = (t: FakeTransport, ok: typeof okFn) => {
  t.on('files.preview', (input) => {
    const path = (input as { path: string }).path;
    return ok(MD[path] !== undefined ? { kind: 'text', markdown: true, text: MD[path], truncated: false } : { kind: 'none' });
  });
};

// What's running (scene `runs`): one plan_progress block per run state, each run with its one step.
const RUN_STATES: [string, string][] = [
  ['queued', 'Draft go/no-go note — Release planner'], ['running', 'Compare supplier quotes — Supplier analyst'], ['paused', 'Contract review (workflow)'],
  ['stopped_at_limit', 'Supplier rates refresh'], ['failed', 'Summarise Borealis framework'], ['interrupted', 'Check clauses in 4 contracts'],
  ['no_reply', 'Ask the support agent about Kestrel'], ['abandoned', 'Post in #procurement'], ['cancelled', 'Pricing agent’s step'], ['completed', 'Update rates.md'],
];
const runId = (i: number) => `0192a1c2-3344-7556-8677-0000000001${String(i).padStart(2, '0')}`;
const runsAt = (front: number): Seed => Object.assign((t: FakeTransport, local: string, ok: typeof okFn) => {
  conversation(t, local, ok);
  groups(t, ok, ['runs']);
}, {
  after: (app: AppContext, t: FakeTransport, local: string) => {
    app.shell.openWorkspaceTab(local, WS_Q3);
    let seq = 1;
    RUN_STATES.forEach(([state, line], i) => {
      const run = runId(i);
      const mark = state === 'running' ? 'now' : state === 'completed' ? 'done' : 'next';
      t.emit(event(local as never, seq++, { kind: 'plan_changed', run_id: run, plan_version: 1, steps: [{ id: `r${i}s1`, position: 1, line, mark, plan_version: 1, run_id: run, events_seen: [], marks_set: [], came_from: 'agent' }] } as never));
      t.emit(event(local as never, seq++, { kind: 'run_state_changed', run_id: run, state } as never));
      t.emit(event(local as never, seq++, { kind: 'block_shown', workspace_id: WS_Q3, block_id: `runs${i}`, block: { kind: 'plan_progress', subject: { kind: 'run', run_id: run } } as never }));
    });
    setTimeout(() => openScreen(app, local as never, WS_Q3, makeKey(BLOCK_SCREEN_ID, `runs${front}`), { title: blockTitle({ kind: 'plan_progress', subject: { kind: 'run', run_id: runId(front) } } as Block) }), 50);
  },
});

// Agent repositories (`catalogue.list_repositories`): the company's, and Engineering agents.
const repository = (id: string, display_name: string, repo: string, agent_count: number, set_by_company: boolean) => ({
  agent_count, set_by_company, last_fetch: null,
  repository: {
    added_at: '2026-01-05T09:00:00.000Z', clone_path: `/repos/${repo}`, display_name, format: 'ordin', format_version: '1', host: 'github.com', id, last_fetched_commit: 'a1b2c3d',
    person_can_curate: false, person_can_publish: true, protected_branch: 'main', repository: `acme/${repo}`, signed_merges_required: false, source: set_by_company ? 'profile' : 'person', state: 'ready',
  },
});
const REPOSITORIES = [repository(REPO, 'Company agents', 'company-agents', 11, true), repository('0192a1c5-1111-7222-8333-444455556667', 'Engineering agents', 'engineering-agents', 2, false)];
const repositories = (t: FakeTransport, ok: typeof okFn) => t.on('catalogue.list_repositories', ok(REPOSITORIES));

// The agent draft (scene `makeDraft`): Supplier contract checker, one check failed.
const DRAFT = '0192a1c7-0000-7000-8000-000000000001';
const draft = (t: FakeTransport, ok: typeof okFn) => {
  groups(t, ok, ['authoring']);
  repositories(t, ok);
  t.on('authoring.agent_draft', ok({
    can_publish: false, can_try: false, files: null, job_tasks: {}, publish_to: [REPO], reviewer_words: [], tidied: [{ before: 'utilise', after: 'use' }, { before: 'in order to', after: 'to' }, { before: 'prior to', after: 'before' }],
    checks: [
      { args: {}, code: 'capability_not_allowed', detail: '', fixes: [{ fix: 'fix_it' }, { fix: 'ask_ordin' }], message_id: 'check-capability-not-allowed', severity: 'failure' },
    ],
    draft: {
      agent_repository_id: REPO, branch: 'draft/supplier-contract-checker', created_at: '2026-01-14T09:00:00.000Z', field_authors: [], id: DRAFT, jobs: [], name: 'Supplier contract checker',
      needs: ['Read the purchasing policy in Confluence', 'Read supplier quotes and contracts in SharePoint'], origin: 'made', pull_request: null,
      purpose: 'Checks supplier quotes and contracts against the purchasing policy, and lists what is missing, unusual or due for renewal.', rehearsal_id: null, reused_parts: [],
      state: 'draft', tasks: ['Check a quote against the policy', 'List contracts that renew by themselves'], trial: false, updated_at: '2026-01-14T09:00:00.000Z',
      will: ['Use only the folders you allow in this workspace', 'Run on this computer'], wont: ['Post in Slack'], wording_changes: [],
    },
  }));
};

// Add an agent repository (scene `repoAdd`): Settings › Agent repositories with the add form open and
// the address pasted; `probe` also presses Look it up, as the prototype's Check does.
const press = (text: string) => [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === text)?.click();
const type = (label: string, value: string) => {
  const input = [...document.querySelectorAll('label')].find((l) => l.textContent?.includes(label))?.querySelector('input');
  if (!input) return;
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
};
const repoAdd = (probe: boolean): Seed => Object.assign((t: FakeTransport, local: string, ok: typeof okFn) => {
  home(t, local, ok);
  repositories(t, ok);
  t.on('catalogue.probe_repository', ok({ agent_count: 12, expires_at: '2099-01-01T00:00:00.000Z', format: 'ordin', host: 'github.com', person_can_publish: false, probe_id: 'p1', repository: 'acme/finance-agents', suggested_name: 'Finance agents' }));
}, {
  after: (app: AppContext, t: FakeTransport, local: string) => {
    void t;
    setTimeout(() => openScreen(app, local as never, null, makeKey(SCREEN_IDS.settings, 'agent-repositories')), 50);
    setTimeout(() => press('Add an agent repository…'), 400);
    setTimeout(() => type('web address', 'https://github.com/acme/finance-agents'), 600);
    if (probe) setTimeout(() => press('Look it up'), 800);
  },
});

// Check it (scene `checkAgent`): Agents, Supplier analyst chosen, Check it pressed; two findings fail.
const CHECK_RUN = '0192a1c8-0000-7000-8000-000000000001';
const checkAgent: Seed = Object.assign((t: FakeTransport, local: string, ok: typeof okFn) => {
  conversation(t, local, ok);
  agents(t, ok);
  t.on('workspaces.links', ok([]));
  t.on('agents.made_of', ok({ display_name: 'Supplier analyst', groups: [], part_count: 19, pinned_by: null, tasks: ['Compare quotes', 'Review supplier responses'], what_it_does: 'Reads and compares supplier documents.' }));
  const report = {
    agents_that_stop: [], check_run_id: CHECK_RUN, checked_at: '2026-01-14T14:07:00.000Z', passed: false, publish_gate_met: true, tidied: [], version_checked: '2.1.0',
    subject: { kind: 'installed', agent_instance_id: AGENT_IDS.analyst },
    findings: [
      { args: {}, code: 'ability_withdrawn', detail: '', fixes: [{ fix: 'fix_it' }, { fix: 'change_it_yourself' }], message_id: 'check-ability-withdrawn', part_identifier: 'reading-notice-periods', severity: 'failure' },
      { args: {}, code: 'task_no_output', detail: '', fixes: [{ fix: 'ask_ordin' }, { fix: 'change_it_yourself' }], message_id: 'check-task-no-output', part_identifier: null, severity: 'failure' },
    ],
  };
  t.on('checker.check_installed', ok(report));
  t.on('checker.report', ok(report));
}, {
  after: (app: AppContext, t: FakeTransport, local: string) => {
    conversation.after?.(app, t, local);
    setTimeout(() => openScreen(app, local as never, WS_Q3, SCREEN_IDS.agents), 50);
    setTimeout(() => press('Supplier analyst'), 500);
    setTimeout(() => press('Check it'), 900);
  },
});

export const SCENES: Record<string, Seed> = {
  library: withReplies('library', [{ kind: 'agent_library', search: null }], library(LIBRARY)),
  libraryRepos: withReplies('libraryRepos', [{ kind: 'agent_library', search: null }], library(REPOS_LIBRARY)),
  crew: withReplies('crew', [{ kind: 'crew_plan', crew_charter_id: CHARTER, estimate }], crew),
  useEstimate: withReplies('useEstimate', [{ kind: 'crew_plan', crew_charter_id: CHARTER, estimate }], crew),
  memory: withReplies('memory', [{ kind: 'memory', facts: FACTS.map((f) => f.id), workspace_id: WS_Q3 }], memory),
  memoryView: fixedScreen(SCREEN_IDS.memory, memory),
  know: withReplies('know', [{ kind: 'knowledge', page: null, workspace_id: WS_Q3 }], knowledge),
  knowView: fixedScreen(makeKey(SCREEN_IDS.knowledge, 'k2'), knowledge),
  knLinks: fixedScreen(makeKey(SCREEN_IDS.knowledge, 'k1'), knowledge),
  md: withReplies('md', [{ kind: 'document', item: file('supplier-portal/docs/rates.md') }], markdown),
  mdNew: withReplies('mdNew', [{ kind: 'document', item: file('Supplier quotes/Q3 rate changes.md') }], markdown),
  changes: withReplies('changes', [{ kind: 'changes', view: 'text_differences', before: file('Procurement/Supplier summary (version 13).txt'), after: file('Procurement/Supplier summary.txt') }], markdown),
  // Send back is the approval card of the base scene `reject` (Send 3 changes back?).
  sendBack: BASE['reject']!,
  changesPages: withReplies('changesPages', [{ kind: 'changes', view: 'before_and_after_pages', before: file('Procurement/Supplier summary (version 13).txt'), after: file('Procurement/Supplier summary.txt') }], markdown),
  runs: runsAt(1),
  runsStopped: runsAt(3),
  makeDraft: fixedScreen(makeKey('agent-draft', DRAFT), draft),
  repoAdd: repoAdd(false),
  repoAddFound: repoAdd(true),
  checkAgent,
};
