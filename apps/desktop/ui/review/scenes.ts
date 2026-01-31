// The data each reviewed scene needs, shaped as the Service API replies (types.gen.ts), with the
// prototype's sample names. Sample data is illustrative (12 §2).
import type { AppContext } from '../src/lib/shell/context';
import { BLOCK_SCREEN_ID } from '../src/lib/blocks/block-screen';
import { blockTitle } from '../src/lib/blocks/titles';
import type { Block } from '../src/lib/service-api/types.gen';
import { openScreen } from '../src/lib/shell/open-item';
import { makeKey } from '../src/lib/shell/screens';
import { approvalCard, event, type FakeTransport, type ok as okFn } from '../tests/support/fake-backend';

export type Seed = ((t: FakeTransport, local: string, ok: typeof okFn) => void) & { after?: (app: AppContext, t: FakeTransport, local: string) => void };

export const WS = {
  onboarding: '0192f0a1-0000-7000-8000-000000000001',
  northwind: '0192f0a1-0000-7000-8000-000000000002',
  q3: '0192f0a1-0000-7000-8000-000000000003',
};
export const ws = (id: string, name: string) => ({ id, name, created_at: '2026-01-05T09:00:00.000Z', own_folder_place_id: `${id}-p`, state: 'active' });
export const row = (line: string, workspace: string, name: string, opens: unknown, over: Record<string, unknown> = {}) => ({
  at: '2026-01-10T10:00:00.000Z', is_new: false, line, opens, run_again: false, workspace_id: workspace, workspace_name: name, ...over,
});

export function workspaces(t: FakeTransport, ok: typeof okFn): void {
  t.on('workspaces.list', ok([ws(WS.onboarding, 'Onboarding guide refresh'), ws(WS.northwind, 'Northwind account brief'), ws(WS.q3, 'Q3 supplier review')]));
  t.on('workspaces.tree', (input) => {
    const id = (input as { workspace_id: string }).workspace_id;
    const name = id === WS.q3 ? 'Q3 supplier review' : id === WS.northwind ? 'Northwind account brief' : 'Onboarding guide refresh';
    return ok({
      workspace: ws(id, name), places: [], agents: [], workflows: ['Quarter close', 'Supplier onboarding', 'Rate check'], dashboards: ['Q3 rates'],
      knowledge_pages: 4, memory_facts: 12, running: [], waiting_for_person: id === WS.onboarding ? 1 : 0,
    });
  });
}

export const home: Seed = (t, _local, ok) => {
  workspaces(t, ok);
  const waiting = [row('Send 1 change back?', WS.onboarding, 'Onboarding guide refresh', { kind: 'approval', approval_id: 'a1', workspace_id: WS.onboarding })];
  const running = [row('Supplier rates crew', WS.q3, 'Q3 supplier review', { kind: 'crew_run', crew_run_id: 'c1', workspace_id: WS.q3 })];
  const since = [
    row('Acme contract 2026.docx was changed in SharePoint by J. Patel', WS.q3, 'Q3 supplier review', { kind: 'workspace', workspace_id: WS.q3 }, { is_new: true }),
    row('Northwind contract 2026.pdf was changed in SharePoint by L. Byrne', WS.northwind, 'Northwind account brief', { kind: 'workspace', workspace_id: WS.northwind }, { is_new: true }),
    row('Onboarding writer refreshed First week checklist.docx', WS.onboarding, 'Onboarding guide refresh', { kind: 'run', run_id: 'r2', workspace_id: WS.onboarding }, { is_new: true, run_again: true }),
  ];
  t.on('home.view', ok({ new_count: 6, waiting, running, since_last_looked: since }));
  t.on('home.needs_you', ok(waiting));
};

export const conversation: Seed = Object.assign(
  (t: FakeTransport, local: string, ok: typeof okFn) => {
    home(t, local, ok);
    try {
      localStorage.setItem('ordin.view.tree.expanded', JSON.stringify([`${local}:${WS.q3}`]));
    } catch {
      // A private window: the tree stays folded.
    }
  },
  {
    after: (app: AppContext, t: FakeTransport, local: string) => {
      app.shell.openWorkspaceTab(local, WS.q3);
      const message = (id: string, text: string) => ({ kind: 'block_shown' as const, workspace_id: WS.q3, block_id: id, block: { kind: 'message', text, attachments: [] } as never });
      t.emit(
        event(local as never, 1, message('m1', 'I’ve opened the crew’s progress in the centre so you can watch it. Stop crew is there and in the tree on the left.')),
        event(local as never, 2, message('m2', 'The supplier rates are updated in the spreadsheet; three suppliers changed their day rate.')),
      );
    },
  },
);

// Blocks, as the prototype's block scenes show them (ScreensModule step 6 scene table). Each scene puts its
// blocks in Q3 supplier review's conversation and opens the first card's screen in the centre.
export const PLACE = `${WS.q3}-p`;
export const file = (relative_path: string) => ({ kind: 'place_file', place_id: PLACE, relative_path });
export const RUN = '0192a1c2-3344-7556-8677-8899aabbccdd';
const rates = {
  kind: 'table',
  title: 'Day rate by supplier',
  columns: [{ name: 'Supplier', type: 'text' }, { name: 'Q2', type: 'money', currency: 'GBP' }, { name: 'Q3', type: 'money', currency: 'GBP' }, { name: 'Change', type: 'percentage' }],
  rows: [['Acme', 415, 428, 0.031], ['Borealis', 432, 440, 0.019], ['Kestrel', 398, 405, 0.018], ['Northwind', 402, null, null]],
  source: { kind: 'documents', documents: [{ item: file('Procurement/Supplier rates Q3.xlsx'), version: '6' }] },
};
const chart = {
  kind: 'chart', title: 'Day rate by supplier, Q2 and Q3', chart: 'bar', orientation: 'vertical', arrangement: 'grouped',
  columns: rates.columns.slice(0, 3), rows: rates.rows.map((r) => r.slice(0, 3)), x: 'Supplier', series: ['Q2', 'Q3'], y_title: 'Day rate',
  source: { kind: 'worked_out', by: 'Supplier analyst', from: [file('Procurement/Supplier rates Q3.xlsx')] },
};
const figures = {
  kind: 'key_figures',
  figures: [
    { label: 'Average Q3 day rate', value: 424, unit: 'GBP', change: { against: 'Q2', earlier_value: 412 } },
    { label: 'Q3 rates received', value: 3, unit: null, change: null },
    { label: 'Shortest notice', value: 30, unit: 'days', change: null },
  ],
  source: { kind: 'worked_out', by: 'Supplier analyst', from: [file('Procurement/Supplier rates Q3.xlsx')] },
};
const step = (position: number, mark: string, line: string) => ({ id: `s${position}`, position, line, mark, plan_version: 1, run_id: RUN, events_seen: [], marks_set: [], came_from: 'agent' });
const BLOCKS: Record<string, unknown[]> = {
  free: [
    { kind: 'message', text: 'Because Supplier rates Q3.xlsx has no Q3 rate for Borealis, and you asked me to mark missing rates “to be confirmed” rather than show an old one.', attachments: [] },
    { kind: 'result', of: 'produced', run_id: RUN, summary: 'Supplier rates Q3.xlsx updated', items: [file('Procurement/Supplier rates Q3.xlsx')] },
  ],
  ambiguous: [{ kind: 'question', question: 'Which Q3 report do you mean?', choices: ['Q3 supplier rates (SharePoint)', 'Q3 spend summary (Confluence)'], allow_other: true }],
  form: [{
    kind: 'form', title: 'Compare quotes', verb: 'Compare quotes', target: { kind: 'agent_instance', agent_instance_id: '0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff' }, command: 'compare-quotes',
    fields: [
      { name: 'quotes', label: 'Quotes to compare', required: true, input: 'text', value: 'Kestrel, Acme and Northwind Q3', placeholder: null, filled_by_ordin: true },
      { name: 'notes', label: 'Anything else', required: false, input: 'text', value: '', placeholder: 'Optional', filled_by_ordin: false },
    ],
  }],
  docs: [{ kind: 'documents', workspace_id: WS.q3, items: ['Acme contract 2026.docx', 'Borealis contract 2026.pdf', 'Kestrel quote Q3.pdf', 'Supplier rates Q3.xlsx'].map((n) => file(`Procurement/${n}`)) }],
  progress: [{ kind: 'plan_progress', subject: { kind: 'run', run_id: RUN } }],
  reportTable: [rates],
  report: [{ kind: 'report', title: 'Q3 supplier rates', as_of: '2026-01-14T13:05:00.000Z', parts: [{ heading: 'At a glance', content: figures }, { heading: 'Day rate by supplier, Q2 and Q3', content: chart }, { heading: 'Rates', content: rates }] }],
  chartKinds: [chart],
  flowSave: [{ kind: 'result', of: 'offer', offer: 'save_as_workflow', run_id: RUN }],
  checklist: [{ kind: 'result', of: 'checklist', title: 'Ready to publish', items: [file('Procurement/Supplier rates Q3.xlsx'), file('Procurement/Q3 summary.docx')] }],
  addAgent: [{ kind: 'agent_proposal', agent_repository_id: '0192a1c5-1111-7222-8333-444455556666', agent: 'urn:ordin:supplier-analyst', reason: 'It compares supplier quotes and keeps the rates spreadsheet up to date.' }],
};

/** A scene whose blocks are shown in Q3 supplier review's conversation, the first card's screen in front. */
export const blocks = (name: string, given?: unknown[]): Seed =>
  Object.assign((t: FakeTransport, local: string, ok: typeof okFn) => conversation(t, local, ok), {
    after: (app: AppContext, t: FakeTransport, local: string) => {
      app.shell.openWorkspaceTab(local, WS.q3);
      if (name === 'progress') {
        t.emit(event(local as never, 1, { kind: 'plan_changed', run_id: RUN, plan_version: 1, steps: [step(1, 'done', 'Read the supplier rates'), step(2, 'now', 'Comparing the Q3 rates with Q2'), step(3, 'next', 'Mark missing rates to be confirmed'), step(4, 'next', 'Write the summary')] } as never));
        t.emit(event(local as never, 2, { kind: 'run_progress', run_id: RUN, progress: { line: 'Read 3 of 4 suppliers', count: { done: 3, total: 4 } } } as never));
      }
      const list = given ?? BLOCKS[name] ?? [];
      list.forEach((block, index) => t.emit(event(local as never, 10 + index, { kind: 'block_shown', workspace_id: WS.q3, block_id: `${name}${index}`, block: block as never })));
      const first = list.findIndex((b) => !['message', 'question'].includes((b as { kind: string }).kind) && !((b as { of?: string }).of === 'offer'));
      if (first >= 0) setTimeout(() => openScreen(app, local as never, WS.q3, makeKey(BLOCK_SCREEN_ID, `${name}${first}`), { title: blockTitle(list[first] as Block) }), 50);
    },
  });

// An approval card (scene `reject`): the card is pending and its screen is in front.
const approval: Seed = Object.assign((t: FakeTransport, local: string, ok: typeof okFn) => conversation(t, local, ok), {
  after: (app: AppContext, t: FakeTransport, local: string) => {
    const card = approvalCard({
      approval_id: '0192a1c4-0000-7000-8000-0000000000a1', workspace_id: WS.q3, question: 'Send 3 changes back?', verb: 'Send back',
      where_line: 'Confluence — Procurement space (1); GitHub — supplier-portal (1); SharePoint — Procurement › Suppliers (1)',
      changes: 'Q3 rates in all three; Borealis and Kestrel marked “to be confirmed”; rates.md is saved to the shared copy for everyone working on it',
      affects: [{ kind: 'file', label: 'Supplier summary', version: '13' }, { kind: 'file', label: 'rates.md', version: null }, { kind: 'file', label: 'Supplier review deck.pptx', version: '8' }],
      content: { kind: 'changes', label: 'See the changes', change_set: { kind: 'artefact', artefact_id: '0192a1c4-0000-7000-8000-000000000001' } },
    });
    app.local.pendingApprovals.set((card as { approval_id: string }).approval_id, card);
    setTimeout(() => openScreen(app, local as never, WS.q3, makeKey('approval', (card as { approval_id: string }).approval_id), { title: 'Send 3 changes back?' }), 50);
    void t;
  },
});

export const SCENES: Record<string, Seed> = { reject: approval, home, conversation, ...Object.fromEntries(Object.keys(BLOCKS).map((k) => [k, blocks(k)])) };
