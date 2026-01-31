// One test per block kind, each using the kind's JSON example from 2C §7 (a kind with no example in 2C is
// built from its Rust definition, and says so). Rendering is server-side with no DOM: what a component
// reads in an effect (a command's reply) is not drawn here; those paths are tested through their helpers.
import { blockTitle, plainLine } from '../src/lib/blocks/titles';
import { beforeAll, describe, expect, it } from 'vitest';

import BlockBody from '../src/lib/blocks/BlockBody.svelte';
import BlockView from '../src/lib/blocks/BlockView.svelte';
import FactList from '../src/lib/blocks/FactList.svelte';
import KeyFigures from '../src/lib/blocks/kinds/KeyFigures.svelte';
import { KNOWN_KINDS, placementOf } from '../src/lib/blocks/types';
import type { Block } from '../src/lib/service-api/types.gen';
import type { AppContext } from '../src/lib/shell/context';
import { event } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const PLACE = '0192a0f2-2222-7333-8444-555566667777';
const RUN = '0192a1c2-3344-7556-8677-8899aabbccdd';
const text = (markup: string) => markup.replace(/<!--[\s\S]*?-->/g, '').replace(/<style[\s\S]*?<\/style>/g, '').replace(/<[^>]*>/g, ' ').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');

let app: AppContext;
beforeAll(async () => {
  ({ app } = await appWith());
});

const draw = (block: unknown, id = 'b1') => html(BlockBody, { backend: app.local, workspaceId: WS, blockId: id, block }, app);
const drawView = (block: unknown, id = 'b1') => html(BlockView, { backend: app.local, workspaceId: WS, blockId: id, block }, app);

const table = {
  kind: 'table',
  title: 'Q3 rates by supplier',
  columns: [{ name: 'Supplier', type: 'text' }, { name: 'Rate', type: 'money', currency: 'GBP' }, { name: 'Change', type: 'percentage' }],
  rows: [['Kestrel', 412.5, 0.08], ['Acme', 398.0, -0.02]],
  source: { kind: 'documents', documents: [{ item: { kind: 'place_file', place_id: PLACE, relative_path: 'Q3/rates.xlsx' }, version: '7' }] },
};
const chart = {
  kind: 'chart',
  title: 'Spend by supplier',
  chart: 'bar',
  orientation: 'vertical',
  arrangement: 'grouped',
  columns: [{ name: 'Supplier', type: 'text' }, { name: 'Q2', type: 'money', currency: 'GBP' }, { name: 'Q3', type: 'money', currency: 'GBP' }],
  rows: [['Kestrel', 382000, 412000], ['Acme', 401000, 398000]],
  x: 'Supplier',
  series: ['Q2', 'Q3'],
  y_title: 'Spend',
  source: { kind: 'worked_out', by: 'Supplier analyst', from: [{ kind: 'place_file', place_id: PLACE, relative_path: 'Q3/rates.xlsx' }] },
};
const keyFigures = {
  kind: 'key_figures',
  figures: [{ label: 'Spend', value: 412000, unit: 'GBP', change: { against: 'Q2', earlier_value: 381500 } }],
  source: { kind: 'company_system', system: 'Finance ledger', read_at: '2026-01-10T15:20:00.000Z' },
};

describe('one test per block kind (2C §7 JSON examples)', () => {
  it('message — {"kind":"message","text":"Supplier analyst will compare the three quotes.","attachments":[]}', () => {
    const out = draw({ kind: 'message', text: 'Supplier analyst will compare the three quotes.', attachments: [] });
    expect(text(out)).toContain('Supplier analyst will compare the three quotes.');
  });

  // SM-MSG-1: Ordin writes Markdown; its message is drawn formatted, never with the marks showing.
  it('message — Ordin\'s Markdown is drawn formatted: bold, a list and a heading', () => {
    const out = draw({ kind: 'message', text: '## Where things stand\n\n**Done.** Two points:\n\n- the quotes\n- the terms', attachments: [] });
    expect(out).toMatch(/<strong[^>]*>Done\.<\/strong>/);
    // Server-side rendering puts comment markers inside elements; they are skipped here.
    const inside = (tag: string, words: string) => new RegExp(`<${tag}[^>]*>(?:<!--[^>]*-->)*${words}(?:<!--[^>]*-->)*</${tag}>`);
    expect(out).toMatch(inside('li', 'the quotes'));
    expect(out).toMatch(inside('li', 'the terms'));
    expect(out).toMatch(inside('h[3-6]', 'Where things stand'));
    expect(text(out)).not.toContain('**');
    expect(text(out)).not.toContain('##');
  });

  // SM-RES-1: what an agent said at the end of its task is its own Markdown, drawn formatted.
  it('result — a produced result draws the agent\'s final message formatted, with no marks and no link', () => {
    const out = draw({ kind: 'result', of: 'produced', run_id: RUN, summary: '# Summary\n\n**Done.** Two points:\n\n- the quotes\n- the terms\n\nSee [rates](https://example.com/rates).', items: [] });
    expect(out).toMatch(/<strong[^>]*>Done\.<\/strong>/);
    expect(out).toMatch(/<li[^>]*>(?:<!--[^>]*-->)*the quotes(?:<!--[^>]*-->)*<\/li>/);
    expect(text(out)).not.toContain('**');
    expect(text(out)).not.toContain('# Summary');
    expect(out).not.toMatch(/<a\b/);
  });

  it('message — a link in Ordin\'s text is drawn as its words, never as a link', () => {
    const out = draw({ kind: 'message', text: 'See [the rates page](https://example.com/rates).', attachments: [] });
    expect(text(out)).toContain('the rates page');
    expect(out).not.toMatch(/<a\b/);
    expect(out).not.toContain('https://example.com/rates');
  });

  it('question — choices as buttons and a box for an answer in words when allow_other', () => {
    const out = draw({ kind: 'question', question: 'Which quarter should I compare?', choices: ['Q2', 'Q3'], allow_other: true });
    expect(text(out)).toContain('Which quarter should I compare?');
    expect(out).toMatch(/<button[^>]*>Q2<\/button>/);
    expect(out).toMatch(/<button[^>]*>Q3<\/button>/);
    expect(text(out)).toContain('Something else');
    expect(out).toMatch(/<input[^>]*type="text"/);
  });

  it('form — labelled fields with a mark on those Ordin filled, and the form\'s own verb', () => {
    const out = draw({
      kind: 'form',
      title: 'Compare quotes',
      verb: 'Compare quotes',
      target: { kind: 'agent_instance', agent_instance_id: '0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff' },
      command: 'compare-quotes',
      fields: [{ name: 'arguments', label: 'Quotes to compare', required: true, input: 'text', value: 'Kestrel, Acme and Northwind Q3', placeholder: null, filled_by_ordin: true }],
    });
    expect(text(out)).toContain('Quotes to compare');
    expect(text(out)).toContain('Filled in by Ordin');
    expect(out).toContain('value="Kestrel, Acme and Northwind Q3"');
    expect(out).toMatch(/<button[^>]*type="submit"[^>]*>Compare quotes<\/button>/);
  });

  it('documents — the referenced items, 25 at a time with Show more, and Ask Ordin about these', () => {
    const items = Array.from({ length: 30 }, (_, i) => ({ kind: 'place_file', place_id: PLACE, relative_path: `Q3/quote ${String(i).padStart(2, '0')}.pdf` }));
    const out = draw({ kind: 'documents', workspace_id: WS, items });
    expect(text(out)).toContain('quote 00.pdf');
    expect(text(out)).toContain('quote 24.pdf');
    expect(text(out)).not.toContain('quote 25.pdf');
    expect(text(out)).toContain('Show more');
    expect(text(out)).toContain('Ask Ordin about these');
  });

  it('preview — {"kind":"preview","item":{"kind":"place_file",…}} offers Ask Ordin about this', () => {
    const out = draw({ kind: 'preview', item: { kind: 'place_file', place_id: PLACE, relative_path: 'Q3/Kestrel quote.pdf' } });
    expect(text(out)).toContain('Ask Ordin about this');
  });

  it('document — a formatted page with Open and Ask Ordin about this (2C has no example: DocumentBlock { item })', () => {
    const out = draw({ kind: 'document', item: { kind: 'place_file', place_id: PLACE, relative_path: 'Notes/Brief.md' } });
    expect(text(out)).toContain('Open');
    expect(text(out)).toContain('Ask Ordin about this');
  });

  it('changes — Ask for more changes (2C has no example: ChangesBlock { before, after, view })', () => {
    const ref = (p: string) => ({ kind: 'place_file', place_id: PLACE, relative_path: p });
    for (const view of ['text_differences', 'before_and_after_pages', 'before_and_after_images']) {
      const out = draw({ kind: 'changes', before: ref('a.md'), after: ref('b.md'), view });
      expect(text(out), view).toContain('Ask for more changes');
    }
  });

  it('plan_progress — Now, the steps with a symbol and a word each, and What happened', () => {
    app.local.runs.set(RUN, {
      state: 'running',
      progressLines: [{ kind: 'line', line: 'Read 12 of 40 quotes' }],
      folded: 0,
      count: { done: 12, total: 40 },
      plan: {
        plan_version: 1,
        steps: [
          { id: 's1', position: 1, line: 'Read the supplier quotes', mark: 'done', plan_version: 1, run_id: RUN, events_seen: [], marks_set: [], came_from: 'agent' },
          { id: 's2', position: 2, line: 'Compare the Q3 rates', mark: 'now', plan_version: 1, run_id: RUN, events_seen: [], marks_set: [], came_from: 'agent' },
          { id: 's3', position: 3, line: 'Write the summary', mark: 'next', plan_version: 1, run_id: RUN, events_seen: [], marks_set: [], came_from: 'agent' },
        ],
      },
      startUp: [],
    } as never);
    const out = draw({ kind: 'plan_progress', subject: { kind: 'run', run_id: RUN } });
    expect(text(out)).toContain('Compare the Q3 rates');
    expect(text(out)).toMatch(/step 2 of 3/);
    expect(out).toContain('✓');
    expect(out).toContain('▶');
    expect(text(out)).toContain('Show what happened');
    expect(text(out)).toMatch(/12 of 40/);
  });

  it('table — title, typed columns, rows, a search box and the source line', () => {
    const out = draw(table);
    const t = text(out);
    expect(t).toContain('Q3 rates by supplier');
    expect(t).toContain('£412.50');
    expect(t).toContain('8%');
    expect(t).toContain('-2%');
    expect(out).toMatch(/aria-sort="none"/);
    expect(out).toMatch(/type="search"/);
    expect(t).toContain('Where these numbers come from');
    expect(t).toContain('rates.xlsx (version 7)');
  });

  it('chart — drawn on a canvas under its title, labelled, with Show as table', () => {
    const out = draw(chart);
    expect(out).toMatch(/role="img"[^>]*aria-label="Spend by supplier"/);
    expect(text(out)).toContain('Show as table');
  });

  it('key_figures — label, value with its currency, and the change against the earlier value', () => {
    const out = draw(keyFigures);
    expect(text(out)).toContain('Spend');
    expect(text(out)).toContain('£412,000.00');
    expect(text(out)).toContain('Up 8% on Q2');
    expect(text(out)).toContain('Finance ledger');
  });

  it('report — title, "as of", each part under its heading (the 2C example with its table filled in)', () => {
    const out = draw({ kind: 'report', title: 'Q3 supplier rates', as_of: '2026-01-10T15:20:00.000Z', parts: [{ heading: 'Rates', content: table }] });
    expect(text(out)).toContain('Q3 supplier rates');
    expect(text(out)).toMatch(/as of 10 Jan 2026/);
    expect(text(out)).toContain('Rates');
    expect(text(out)).toContain('Q3 rates by supplier');
    expect(text(out)).toContain('Where these numbers come from');
  });

  it('email_list — Open and Ask Ordin about this (the sender and subject come from the Mail Service)', () => {
    const out = draw({ kind: 'email_list', workspace_id: WS, email_link_id: '0192a1c5-1111-7222-8333-444455556666', messages: ['imap:INBOX:1:42'] });
    expect(text(out)).toContain('Open');
    expect(text(out)).toContain('Ask Ordin about this');
  });

  it('email — {"kind":"email","of":"draft","draft_id":"0192a1c5-7777-7888-8999-aaaabbbbcccc"} draws without the draft\'s text until it is read', () => {
    const out = draw({ kind: 'email', of: 'draft', draft_id: '0192a1c5-7777-7888-8999-aaaabbbbcccc' });
    expect(out).toContain('block-email');
  });

  it('telegram_list — a list from the link (2C has no example: TelegramListBlock { telegram_link_id })', () => {
    const out = draw({ kind: 'telegram_list', telegram_link_id: '0192a1c5-aaaa-7bbb-8ccc-ddddeeeeffff' });
    expect(out).toContain('block-telegram');
  });

  it('slack_list — a list from the link (SlackListBlock { slack_link_id })', () => {
    const out = draw({ kind: 'slack_list', slack_link_id: '0192a1c5-aaaa-7bbb-8ccc-ddddeeeeffff' });
    expect(out).toContain('block-slack');
  });

  it('result — produced shows what was made, an offer waits for an answer, a checklist is a list to tick', () => {
    const items = [{ kind: 'place_file', place_id: PLACE, relative_path: 'Q3/summary.docx' }];
    const produced = draw({ kind: 'result', of: 'produced', run_id: RUN, summary: 'Summary written', items });
    expect(text(produced)).toContain('Summary written');
    expect(text(produced)).toContain('summary.docx');
    const offer = draw({ kind: 'result', of: 'offer', offer: 'save_as_workflow', run_id: RUN });
    expect(text(offer)).toContain('Save as workflow…');
    expect(text(offer)).toContain('Not now');
    const checklist = draw({ kind: 'result', of: 'checklist', title: 'Ready to publish', items });
    expect(text(checklist)).toContain('Ready to publish');
    expect(checklist).toMatch(/type="checkbox"/);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-AL-5
  it('agent_proposal — the reason, then the agent library (SM-AL-5; the chosen agent and Ask Ordin in tests/library.test.ts)', () => {
    const out = draw({ kind: 'agent_proposal', agent_repository_id: '0192a1c5-1111-7222-8333-444455556666', agent: 'urn:ordin:supplier-analyst', reason: 'It compares quotes.' });
    expect(text(out)).toContain('It compares quotes.');
    expect(out).toMatch(/type="search"/);
  });

  it('agent_library — a search box over the library', () => {
    const out = draw({ kind: 'agent_library', search: 'quotes' });
    expect(out).toMatch(/type="search"[^>]*value="quotes"|value="quotes"[^>]*type="search"/);
  });

  it('crew_plan — the estimated use is drawn at once; the charter and Start follow the read (SM-WR-15, tests/crew-plan.test.ts)', () => {
    const out = draw({
      kind: 'crew_plan',
      crew_charter_id: '0192a1c5-1111-7222-8333-444455556666',
      estimate: { band: 'medium', min_turns: 20, max_turns: 40, typical_day_permille: 120, based_on: [], ask_first: true },
    });
    expect(text(out)).toContain('Medium use');
    expect(text(out)).toContain('about 20 to 40 turns');
    expect(text(out)).toContain('12%');
    expect(out).not.toMatch(/Start/);
  });

  it('memory — facts with their sources; an agent\'s citation reads as the agent\'s claim (SM-SF-6)', () => {
    const fact = (over: object) => ({ id: 'f1', statement: 'Kestrel quotes in pounds.', kind: 'known', came_from: { kind: 'conversation', run_id: RUN }, fading: false, may_be_out_of_date: false, needs_check: false, true_from: '2026-01-01T00:00:00Z', written_at: '2026-01-01T00:00:00Z', ...over });
    const agents = [{ id: 'a1', display_name: 'Supplier analyst' }];
    const cited = fact({ came_from: { kind: 'agent_citing', agent_instance_id: 'a1', origin_ref: 'Q3/rates.xlsx', version: '7' } });
    const out = html(FactList, { facts: [cited], agents }, app);
    expect(text(out)).toContain('Supplier analyst says this comes from Q3/rates.xlsx (version 7)');
    expect(out).not.toMatch(/href=/);
    const unknown = html(FactList, { facts: [cited], agents: [] }, app);
    expect(text(unknown)).toContain('An agent says this comes from Q3/rates.xlsx (version 7)');
    const plain = html(FactList, { facts: [fact({})], agents }, app);
    expect(text(plain)).not.toContain('says this comes from');
    expect(draw({ kind: 'memory', workspace_id: WS, facts: ['f1'] })).toContain('block-memory');
  });

  it('knowledge — the workspace\'s pages, opened on the knowledge screen', () => {
    const out = draw({ kind: 'knowledge', workspace_id: WS });
    expect(text(out)).toContain('Open');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-FA-3
  it('access — the sentence and a tick box per folder (SM-FA-3; the ticking in tests/folder-access.test.ts)', () => {
    const out = draw({ kind: 'access', agent_instance_id: '0192f400-0000-7000-8000-0000000000a1', agent_name: 'Document agent', text: "Document agent can't use this workspace's folders yet.", places: [{ place_id: '0192f400-0000-7000-8000-0000000000b1', name: 'Sample documents', read_only: false }] } as never);
    expect(text(out)).toContain("Document agent can't use this workspace's folders yet.");
    expect(text(out)).toContain('Document agent can read and change Sample documents');
    expect(out).toMatch(/type="checkbox"/);
  });

  it('every kind of 2C §7 has a component test above, and the drawing side knows exactly those kinds', () => {
    expect([...KNOWN_KINDS].sort()).toEqual(
      ['access', 'agent_library', 'agent_proposal', 'changes', 'chart', 'crew_plan', 'document', 'documents', 'email', 'email_list', 'form', 'key_figures', 'knowledge', 'memory', 'message', 'plan_progress', 'preview', 'question', 'report', 'result', 'slack_list', 'table', 'telegram_list'].sort(),
    );
  });
});

describe('where each kind appears (step 6)', () => {
  const cases: Array<[Block['kind'] | 'result-offer', 'full' | 'card']> = [
    ['message', 'full'], ['question', 'full'], ['result-offer', 'full'], ['access', 'full'],
    ['form', 'card'], ['documents', 'card'], ['table', 'card'], ['chart', 'card'], ['report', 'card'], ['plan_progress', 'card'], ['memory', 'card'], ['knowledge', 'card'], ['crew_plan', 'card'],
  ];
  it.each(cases)('%s is drawn %s in the conversation', (kind, where) => {
    const block = kind === 'result-offer' ? ({ kind: 'result', of: 'offer', offer: 'save_as_workflow' } as unknown as Block) : ({ kind } as unknown as Block);
    expect(placementOf(block)).toBe(where);
  });

  it('a screen-and-card kind is one line in the conversation: its title and Open, and Waiting for you while it waits', () => {
    const out = drawView(table);
    expect(text(out)).toContain('Q3 rates by supplier');
    expect(text(out)).toContain('Open');
    expect(out).not.toContain('£412.50');
    app.local.conversation.set(WS, [{ block_id: 'f1', workspace_id: WS, block: { kind: 'form' } as never, seq: 1, at: '', waiting: true }]);
    const form = html(BlockView, { backend: app.local, workspaceId: WS, blockId: 'f1', block: { kind: 'form', title: 'Compare quotes', verb: 'Go', target: { kind: 'agent_instance', agent_instance_id: 'x' }, fields: [] } }, app);
    expect(text(form)).toContain('Waiting for you');
    app.local.conversation.delete(WS);
  });
});

describe('Error path — unknown block kind', () => {
  it('a block_shown event carrying kind "timeline" draws nothing and records UI_BLOCK_KIND_UNKNOWN', () => {
    const before = app.diagnostics.codes().length;
    const out = drawView({ kind: 'timeline', events: [] });
    expect(text(out).trim()).toBe('');
    expect(app.diagnostics.codes().slice(before)).toEqual(['UI_BLOCK_KIND_UNKNOWN']);
  });
});

describe('Error path — markup in data is shown as text', () => {
  const markup = '<img src=x onerror=alert(1)>';
  it('a message block whose text is markup shows those characters literally and creates no element', () => {
    const out = draw({ kind: 'message', text: markup, attachments: [] });
    expect(out).toContain('&lt;img src=x onerror=alert(1)>');
    expect(out).not.toMatch(/<img/);
  });

  it('every text a block draws from its data stays text: question, form, table cells, key figures, plan steps', () => {
    for (const block of [
      { kind: 'question', question: markup, choices: [markup, 'b'], allow_other: false },
      { kind: 'form', title: markup, verb: 'Go', target: { kind: 'agent_instance', agent_instance_id: 'x' }, fields: [{ name: 'a', label: markup, required: false, input: 'text', value: markup, placeholder: null, filled_by_ordin: false }] },
      { ...table, title: markup, rows: [[markup, 1, 0.1]] },
      { ...keyFigures, figures: [{ label: markup, value: 1 }] },
      { kind: 'agent_proposal', agent_repository_id: 'r', agent: 'urn:ordin:x', reason: markup },
    ]) {
      const out = draw(block);
      expect(out, block.kind).not.toMatch(/<img/);
      expect(out, block.kind).toContain('&lt;img');
    }
  });

  it('a table cell starting with = is text, not a formula', () => {
    const out = draw({ ...table, rows: [['=SUM(A1:A9)', 1, 0.1]] });
    expect(text(out)).toContain('=SUM(A1:A9)');
  });
});

describe('Edge case — a question with choices stays in the conversation', () => {
  it('a question block shown by an event waits (no Close) and is drawn in full with its 3 choices', async () => {
    const { app: a, transport } = await appWith();
    void transport;
    const body = { kind: 'question', question: 'Which supplier?', choices: ['Kestrel', 'Acme', 'Northwind'], allow_other: false };
    await a.local.apply(event(a.local.id, a.local.lastSeq + 1, { kind: 'block_shown', workspace_id: WS, block_id: 'q1', block: body } as never));
    const item = a.local.conversation.get(WS)?.[0];
    expect(item?.waiting).toBe(true);
    const out = html(BlockView, { backend: a.local, workspaceId: WS, blockId: 'q1', block: body }, a);
    for (const choice of ['Kestrel', 'Acme', 'Northwind']) expect(out).toMatch(new RegExp(`<button[^>]*>${choice}</button>`));
  });
});

describe('Happy path — a report with a chart shown as a table', () => {
  it('the table the chart falls back to shows exactly the chart\'s columns and rows, and each part shows where its numbers come from', () => {
    const out = draw({ kind: 'report', title: 'Q3', as_of: '2026-01-10T15:20:00.000Z', parts: [{ heading: 'Spend', content: chart }] });
    // The chart is drawn on a canvas; its data table is the TableView that "Show as table" draws.
    const asTable = html(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (BlockBody as any),
      { backend: app.local, workspaceId: WS, blockId: 'c', block: { ...table, title: chart.title, columns: chart.columns, rows: chart.rows, source: chart.source } },
      app,
    );
    for (const cell of ['Supplier', 'Q2', 'Q3', 'Kestrel', 'Acme', '£382,000.00', '£412,000.00', '£401,000.00', '£398,000.00']) expect(text(asTable)).toContain(cell);
    expect(text(out)).toContain('Where these numbers come from');
    expect(text(out)).toContain('Worked out by Supplier analyst from rates.xlsx');
  });
});

describe('KeyFigures change words', () => {
  const draws = (figure: object) => text(html(KeyFigures, { backend: app.local, workspaceId: WS, blockId: 'k', block: { kind: 'key_figures', figures: [figure], source: keyFigures.source } }, app));
  it('up, down, unchanged, and a change from zero', () => {
    expect(draws({ label: 'A', value: 110, change: { against: 'Q2', earlier_value: 100 } })).toContain('Up 10% on Q2');
    expect(draws({ label: 'A', value: 90, change: { against: 'Q2', earlier_value: 100 } })).toContain('Down 10% on Q2');
    expect(draws({ label: 'A', value: 100, change: { against: 'Q2', earlier_value: 100 } })).toContain('Unchanged on Q2');
    expect(draws({ label: 'A', value: 5, change: { against: 'Q2', earlier_value: 0 } })).toContain('Q2: 0');
    expect(draws({ label: 'A', value: 5 })).not.toContain('Q2');
  });
});

describe('Happy path — progress in three layers', () => {
  it('a plan of 4 steps with step 2 marked now: the Now sentence, each step as a symbol and a word, and the line kept under Show what happened', async () => {
    const { app: a } = await appWith();
    const run = '0192a1c2-0000-7000-8000-0000000000cc';
    const step = (position: number, mark: string, line: string) => ({ id: `s${position}`, position, line, mark, plan_version: 1, run_id: run, events_seen: [], marks_set: [], came_from: 'agent' });
    await a.local.apply(event(a.local.id, a.local.lastSeq + 1, { kind: 'plan_changed', run_id: run, plan_version: 1, steps: [step(1, 'done', 'Read the quotes'), step(2, 'now', 'comparing the Q3 rates'), step(3, 'next', 'Check the totals'), step(4, 'next', 'Write the summary')] } as never));
    await a.local.apply(event(a.local.id, a.local.lastSeq + 1, { kind: 'run_progress', run_id: run, progress: { line: 'Read 12 of 40 quotes', count: { done: 12, total: 40 } } } as never));
    const out = html(BlockBody, { backend: a.local, workspaceId: WS, blockId: 'p1', block: { kind: 'plan_progress', subject: { kind: 'run', run_id: run } } }, a);
    expect(text(out)).toContain('Now: comparing the Q3 rates — step 2 of 4');
    expect(text(out)).toMatch(/✓ done Read the quotes/);
    expect(text(out)).toMatch(/▶ now comparing the Q3 rates/);
    expect(text(out)).toMatch(/○ next Check the totals/);
    expect(text(out)).toMatch(/○ next Write the summary/);
    expect(text(out)).toContain('12 of 40');
    expect(out).toMatch(/aria-expanded="false"[^>]*>Show what happened/);
    expect(a.local.runs.get(run)?.progressLines).toEqual([{ kind: 'plan_changed', plan_version: 1 }, { kind: 'line', line: 'Read 12 of 40 quotes' }]);
  });
});

describe('Error Table — a Slack permalink from the core is not https://*.slack.com/archives/…', () => {
  it('UI_LINK_REFUSED: nothing opens and only a diagnostic is recorded; a good link is sent to the desktop program', async () => {
    const { openInSlack } = await import('../src/lib/blocks/slack-link');
    const { app: a, transport } = await appWith();
    transport.on('app.open_slack_link', { kind: 'ok', result: {} });
    const before = a.diagnostics.codes().length;
    expect(await openInSlack(a, a.local.id, 'https://evil.example.com/archives/C1')).toBeNull();
    expect(transport.sentOf('app.open_slack_link')).toEqual([]);
    expect(a.diagnostics.codes().slice(before)).toEqual(['UI_LINK_REFUSED']);
    expect(await openInSlack(a, a.local.id, 'https://northwind.slack.com/archives/C1/p170')).toBeNull();
    expect(transport.sentOf('app.open_slack_link')[0]?.input).toEqual({ url: 'https://northwind.slack.com/archives/C1/p170' });
  });
});

describe('Ask Ordin about this', () => {
  it('sends the message to the conversation with the references the block holds, and nothing else', async () => {
    const { askOrdin } = await import('../src/lib/blocks/ask');
    const { app: a, transport } = await appWith();
    transport.on('ordin.send_message', { kind: 'ok', result: { accepted: true, position_in_queue: 0 } });
    const reference = { kind: 'place_file' as const, place_id: PLACE, relative_path: 'Q3/rates.xlsx' };
    await askOrdin(a.local, WS, 'Tell me more about “Q3 rates”.', [reference]);
    expect(transport.sentOf('ordin.send_message')[0]?.input).toEqual({ workspace_id: WS, text: 'Tell me more about “Q3 rates”.', attachments: [reference] });
    await askOrdin(a.local, WS, 'Hello');
    expect(transport.sentOf('ordin.send_message')[1]?.input).toEqual({ workspace_id: WS, text: 'Hello' });
  });
});

// SM-MSG-1: a title cut from the first line of a Markdown text shows its words, not its marks.
describe('block titles from Markdown text (SM-MSG-1, SM-RES-1)', () => {
  it('Happy path — the marks of a heading, bold, italic, code, a list and a link are not in the title', () => {
    expect(plainLine('## Where things stand')).toBe('Where things stand');
    expect(plainLine('**Done.** I wrote Meeting notes - summary.md:')).toBe('Done. I wrote Meeting notes - summary.md:');
    expect(plainLine('- the *quotes* and `the terms`')).toBe('the quotes and the terms');
    expect(plainLine('See [the rates page](https://example.com/rates).')).toBe('See the rates page.');
  });

  it('Edge case — snake_case words and lone asterisks are left as written', () => {
    expect(plainLine('Read release_notes_0_1_1 now')).toBe('Read release_notes_0_1_1 now');
    expect(plainLine('Cost: 3 * 4 = 12')).toBe('Cost: 3 * 4 = 12');
  });

  it('Happy path — a message, a produced result and a question carry plain titles', () => {
    expect(blockTitle({ kind: 'message', text: '**Done.** All of it.\n\n- one', attachments: [] } as never)).toBe('Done. All of it.');
    expect(blockTitle({ kind: 'result', of: 'produced', run_id: 'r', summary: '# Summary\n\nText', items: [] } as never)).toBe('Summary');
  });
});
