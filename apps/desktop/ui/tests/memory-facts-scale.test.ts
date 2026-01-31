// @vitest-environment happy-dom
// The Facts list at scale (ScreensModule.md SM-SF-27, amended January 2026 (user decision; Approved by the
// user January 2026)): Group by (Thing, Kind of fact, Source, Added by, Topic), one search over the three
// parts with its matches marked, the filter chips, paging inside a group, and a thing's topic with
// Change topic…. The pure helpers first, then the controller over the fake transport, then the screen.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import * as wiring from '../src/lib/backend/wiring';
import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { APP_KEY } from '../src/lib/shell/context';
import { KEYS, ViewPrefs, type StorageLike } from '../src/lib/store/view-prefs';
import {
  AgentNames,
  factMatches,
  filterFacts,
  GROUP_PAGE,
  groupFacts,
  highlightParts,
  isGroupBy,
  NO_FILTERS,
  topicOf,
  topicsByName,
} from '../src/lib/views/fixed/facts';
import Memory from '../src/lib/views/fixed/Memory.svelte';
import { MemoryController } from '../src/lib/views/fixed/memory-controller.svelte';
import { backendWith, WS } from './fixed-support';
import { err, ok, settle, type FakeTransport } from './support/fake-backend';
import { thingId, uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';
import type { FactView } from '../src/lib/service-api/types.gen';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));

const STRATEGY = { id: thingId('strategy'), name: 'Manchester Digital Strategy 2021–2026' };
const KESTREL = { id: thingId('kestrel'), name: 'Kestrel' };
const LOOSE = { id: thingId('loose'), name: 'Loose' };
const AGENT = '0192f400-0000-7000-8000-0000000000a1';
const agents = [{ instance: { id: AGENT, display_name: 'Quote checker' }, tasks: [], uses: [], workspaces: [] }];
const names = new AgentNames(agents as never);

const fact = (id: string, over: Record<string, unknown> = {}): FactView =>
  ({
    id: uid(id), statement: `Statement ${id}`, kind: 'known', true_from: '2026-01-10T00:00:00Z', written_at: '2026-01-10T00:00:00Z',
    came_from: { kind: 'person' }, fading: false, may_be_out_of_date: false, needs_check: false, workspace_id: WS, ...over,
  }) as FactView;
const about = (subject: { id: string; name: string }, label: string, value: string) => ({
  subject,
  predicate: { key: label.replace(/ /g, ''), label },
  object: { kind: 'text', value },
});

const budget = fact('budget', { triple: about(STRATEGY, 'budget', 'Three million pounds'), came_from: { kind: 'document', origin_ref: 'strategy.pdf', version: '1' } });
const year = fact('year', { triple: about(STRATEGY, 'target year', '2026'), came_from: { kind: 'conversation', run_id: uid('run') } });
const staff = fact('staff', {
  triple: about(KESTREL, 'number of employees', '240 people'),
  came_from: { kind: 'agent', agent_instance_id: AGENT, run_id: uid('run') },
  kind: 'happened',
  conflicts_with: [uid('other')],
});
const older = fact('older', { older_note: true, statement: 'Kestrel pays in thirty days', may_be_out_of_date: true, workspace_id: null, things: [LOOSE] });
const ALL = [budget, year, staff, older];
const TOPICS = { [STRATEGY.id]: 'Strategy', [KESTREL.id]: 'Suppliers' };

// ---- the pure helpers -------------------------------------------------------------------------------------

describe('SM-SF-27 — grouping', () => {
  it('Happy path — Group by Thing: one group per thing with its count, facts about no thing last', () => {
    const groups = groupFacts(ALL, 'thing', names, TOPICS);
    expect(groups.map((g) => [g.name, g.facts.length])).toEqual([
      [STRATEGY.name, 2],
      ['Kestrel', 1],
      ['Loose', 1],
    ]);
    expect(t('memory-group-title', { name: STRATEGY.name, count: 14 })).toBe('Manchester Digital Strategy 2021–2026 (14)');
  });

  it('Group by Kind of fact, Source and Added by', () => {
    expect(groupFacts(ALL, 'kind', names, {}).map((g) => g.name)).toEqual(['budget', 'target year', 'number of employees', t('memory-kind-known')]);
    const sources = groupFacts(ALL, 'source', names, {}).map((g) => stripIsolates(g.name));
    expect(sources).toEqual(['strategy.pdf', t('memory-from-conversation'), t('memory-from-agent', { agent: 'Quote checker' }), t('memory-from-you')]);
    expect(groupFacts(ALL, 'added_by', names, {}).map((g) => [stripIsolates(g.name), g.facts.length])).toEqual([
      [t('memory-added-by-ordin'), 2],
      ['Quote checker', 1],
      [t('memory-added-by-you'), 1],
    ]);
  });

  // Kind of fact is the fact's word (its predicate), not its memory class: facts that all "happened"
  // but are of several kinds land in one group per kind, not all under "Something that happened".
  it('Group by Kind of fact — facts of several kinds, all of one memory class, get one group per kind', () => {
    const happened = (id: string, subject: { id: string; name: string }, label: string) => fact(id, { kind: 'happened', triple: about(subject, label, id) });
    const facts = [
      happened('a', KESTREL, 'supplies'),
      happened('b', STRATEGY, 'budget'),
      happened('c', STRATEGY, 'supplies'),
      happened('d', KESTREL, 'price'),
      fact('e', { kind: 'happened' }),
    ];
    const groups = groupFacts(facts, 'kind', names, {});
    expect(groups.map((g) => [g.name, g.facts.length])).toEqual([
      ['supplies', 2],
      ['budget', 1],
      ['price', 1],
      [t('memory-kind-happened'), 1],
    ]);
    expect(new Set(groups.map((g) => g.key)).size).toBe(4);
  });

  // SM-SF-27 (amended January 2026; user decision; Approved by the user January 2026): a fact whose subject has
  // no id yet (the memory component had not tied it to a thing) finds its topic by the thing's name.
  it('Edge — a subject with no id finds its topic by name, case and spacing aside, or by another name', () => {
    const byName = topicsByName([{ id: STRATEGY.id, name: STRATEGY.name, other_names: ['MDS'] }], TOPICS);
    const nameless = fact('n', { triple: about({ id: '', name: '  manchester digital strategy 2021–2026 ' }, 'budget', 'x') });
    const alias = fact('m', { triple: about({ id: '', name: 'mds' }, 'budget', 'y') });
    const stranger = fact('s', { triple: about({ id: '', name: 'Someone else' }, 'budget', 'z') });
    expect(topicOf(nameless, TOPICS, byName)).toBe('Strategy');
    expect(topicOf(alias, TOPICS, byName)).toBe('Strategy');
    expect(topicOf(stranger, TOPICS, byName)).toBeNull();
    expect(topicOf(nameless, TOPICS)).toBeNull();
    expect(groupFacts([budget, nameless], 'topic', names, TOPICS, byName).map((g) => [g.name, g.facts.length])).toEqual([['Strategy', 2]]);
  });

  it('Happy path — Group by Topic: things with no topic under "No topic", last', () => {
    const groups = groupFacts(ALL, 'topic', names, TOPICS);
    expect(groups.map((g) => [g.name, g.facts.length])).toEqual([
      ['Strategy', 2],
      ['Suppliers', 1],
      [t('memory-group-no-topic'), 1],
    ]);
  });

  it('Edge — an unknown stored choice is not a grouping', () => {
    expect(isGroupBy('topic')).toBe(true);
    expect(isGroupBy('colour')).toBe(false);
  });
});

describe('SM-SF-27 — search and filters', () => {
  it('Happy path — one search over subject, word and value, case and accents aside', () => {
    expect(factMatches(budget, 'manchester')).toBe(true);
    expect(factMatches(budget, 'BUDGET')).toBe(true);
    expect(factMatches(budget, 'three million')).toBe(true);
    expect(factMatches(budget, 'kestrel')).toBe(false);
    expect(factMatches(older, 'thirty')).toBe(true);
    expect(factMatches(fact('café', { triple: about(KESTREL, 'meets at', 'Café Rouge') }), 'cafe')).toBe(true);
  });

  it('matches are marked in the line', () => {
    expect(highlightParts('Manchester Digital', 'digi')).toEqual([
      { text: 'Manchester ', hit: false },
      { text: 'Digi', hit: true },
      { text: 'tal', hit: false },
    ]);
    expect(highlightParts('Café Rouge', 'cafe')).toEqual([{ text: 'Café', hit: true }, { text: ' Rouge', hit: false }]);
  });

  it('the chips: Has conflicts, Out of date, Older notes, This workspace, Everywhere and a topic', () => {
    const ids = (f: Partial<typeof NO_FILTERS>, q = '') => filterFacts(ALL, { ...NO_FILTERS, ...f }, q, TOPICS).map((x) => x.id);
    expect(ids({ conflicts: true })).toEqual([staff.id]);
    expect(ids({ outOfDate: true })).toEqual([older.id]);
    expect(ids({ older: true })).toEqual([older.id]);
    expect(ids({ scope: 'workspace' })).toEqual([budget.id, year.id, staff.id]);
    expect(ids({ scope: 'everywhere' })).toEqual([older.id]);
    expect(ids({ topic: 'Strategy' })).toEqual([budget.id, year.id]);
    expect(ids({ topic: '' })).toEqual([older.id]);
    expect(ids({ scope: 'workspace' }, 'kestrel')).toEqual([staff.id]);
  });
});

// ---- the controller -----------------------------------------------------------------------------------------

function memoryStore(): StorageLike & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
}

async function controller(setup: (tr: FakeTransport) => void, prefs?: ViewPrefs) {
  vi.spyOn(wiring, 'heldWrites').mockResolvedValue({ kind: 'not_wired' });
  vi.spyOn(wiring, 'citationsToConfirm').mockResolvedValue({ kind: 'not_wired' });
  const b = await backendWith({
    setup: (tr) => {
      tr.on('agents.list', ok(agents));
      tr.on('memory.list', ok({ items: ALL, next_cursor: null }));
      tr.on('memory.thing_topics', ok({ things: [], topics: [] }));
      setup(tr);
    },
  });
  return { ...b, c: new MemoryController(b.handle, WS, prefs) };
}

describe('SM-SF-27 — the controller', () => {
  it('Group by is remembered per workspace in the view preferences', async () => {
    const store = memoryStore();
    const prefs = new ViewPrefs(store);
    const { c } = await controller(() => undefined, prefs);
    expect(c.groupBy).toBe('thing');
    c.setGroupBy('source');
    expect(JSON.parse(store.data.get(KEYS.memoryGroupBy) ?? '{}')).toEqual({ [WS]: 'source' });
    expect(c.workspaceId).toBe(WS);
    expect(new MemoryController(null as never, WS, prefs).groupBy).toBe('source');
    expect(new MemoryController(null as never, 'another', prefs).groupBy).toBe('thing');
  });

  it('Paging — a large group draws 50 lines, then Show more draws the next 50; the cursor loads the next page', async () => {
    const many = Array.from({ length: 120 }, (_, i) => fact(`f${i}`, { triple: about(STRATEGY, `word ${i}`, `value ${i}`) }));
    const { c, transport } = await controller((tr) =>
      tr.on('memory.list', (input) =>
        (input as { cursor?: string }).cursor === 'next' ? ok({ items: [fact('late', { triple: about(KESTREL, 'budget', '1') })], next_cursor: null }) : ok({ items: many, next_cursor: 'next' }),
      ),
    );
    await c.loadFacts();
    let group = c.grouping.groups[0]!;
    expect(group.facts.length).toBe(120);
    expect(group.shown.length).toBe(GROUP_PAGE);
    expect(group.more).toBe(70);
    c.showMore(group.key);
    group = c.grouping.groups[0]!;
    expect(group.shown.length).toBe(100);
    await c.moreFacts();
    expect(transport.sentOf('memory.list').at(-1)?.input).toEqual({ workspace_id: WS, cursor: 'next' });
    expect(c.grouping.groups.map((g) => g.name)).toEqual([STRATEGY.name, 'Kestrel']);
    expect(c.factsCursor).toBeNull();
  });

  it('Edge — thousands of facts group quickly', async () => {
    const big = Array.from({ length: 5000 }, (_, i) => fact(`b${i}`, { triple: about({ id: thingId(`t${i % 400}`), name: `Thing ${i % 400}` }, 'budget', `${i}`) }));
    const { c } = await controller((tr) => tr.on('memory.list', ok({ items: big, next_cursor: null })));
    await c.loadFacts();
    const started = performance.now();
    const view = c.grouping;
    expect(view.total).toBe(400);
    expect(view.groups.length).toBe(100);
    expect(performance.now() - started).toBeLessThan(1000);
  });

  it('Happy path — Change topic: memory.set_thing_topic is sent, the topics are taken from the answer and it is announced', async () => {
    const set = vi.fn(async () => ok({ things: [{ thing_id: KESTREL.id, topic: 'Funding', set_by: 'person', set_at: '2026-01-10T00:00:00Z' }], topics: ['Funding'] }));
    const { c, transport } = await controller((tr) => tr.on('memory.set_thing_topic', set as never));
    await c.load();
    expect(transport.sentOf('memory.thing_topics')[0]?.input).toEqual({ workspace_id: WS });
    expect(await c.setTopic(KESTREL.id, 'Funding')).toBe(true);
    expect(transport.sentOf('memory.set_thing_topic')[0]?.input).toEqual({ workspace_id: WS, id: KESTREL.id, topic: 'Funding' });
    expect(c.topicOfThing(KESTREL.id)).toBe('Funding');
    expect(c.topicNames).toEqual(['Funding']);
    await settle();
    expect(c.announcer.text).toBe(t('memory-topic-saved'));
  });

  it('Error path — a refused topic keeps its sentence on the dialog', async () => {
    const { c } = await controller((tr) => tr.on('memory.set_thing_topic', err('MEM_TOPIC_REFUSED', 'no')));
    expect(await c.setTopic(KESTREL.id, 'ana@example.com')).toBe(false);
    expect(c.errorOn(`topic:${KESTREL.id}`)?.code).toBe('MEM_TOPIC_REFUSED');
  });

  // SM-SF-27 (amended January 2026; user decision; Approved by the user January 2026): choosing Topic reads the
  // topics again, so a topic an agent gave after the screen opened (settled on that read) groups its facts.
  it('Happy path — choosing Topic reads the topics again and groups under the topics now known', async () => {
    let answer = { things: [] as unknown[], topics: [] as string[] };
    const { c, transport } = await controller((tr) => tr.on('memory.thing_topics', (async () => ok(answer)) as never));
    await c.load();
    expect(transport.sentOf('memory.thing_topics')).toHaveLength(1);
    answer = { things: [{ thing_id: STRATEGY.id, topic: 'Strategy', set_by: 'agent', set_at: '2026-01-10T00:00:00Z' }], topics: ['Strategy'] };
    c.setGroupBy('topic');
    await settle();
    expect(transport.sentOf('memory.thing_topics')).toHaveLength(2);
    expect(c.grouping.groups.map((g) => [g.name, g.facts.length])).toEqual([
      ['Strategy', 2],
      [t('memory-group-no-topic'), 2],
    ]);
  });

  it('Edge — choosing another grouping does not read the topics again', async () => {
    const { c, transport } = await controller(() => undefined);
    await c.load();
    c.setGroupBy('kind');
    await settle();
    expect(transport.sentOf('memory.thing_topics')).toHaveLength(1);
  });

  it('Edge — a backend without topics: Topic is not offered and Group by Topic reads as Thing', async () => {
    const { c } = await controller((tr) => tr.on('memory.thing_topics', err('SVC_COMMAND_UNKNOWN', 'memory.thing_topics')));
    await c.load();
    expect(c.topicsWired).toBe(false);
    c.setGroupBy('topic');
    expect(c.effectiveGroupBy).toBe('thing');
  });
});

// ---- the screen ---------------------------------------------------------------------------------------------

let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn.part);
  drawn?.target.remove();
  drawn = null;
  document.body.innerHTML = '';
});
async function flush(): Promise<void> {
  for (let i = 0; i < 6; i++) {
    await settle();
    flushSync();
  }
}
const words = (node: Element | null | undefined): string => (node?.textContent ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const buttonNamed = (root: ParentNode, name: string): HTMLButtonElement | undefined => Array.from(root.querySelectorAll('button')).find((b) => words(b) === name);
async function press(root: ParentNode, name: string): Promise<void> {
  const b = buttonNamed(root, name);
  expect(b, `the button "${name}"`).toBeDefined();
  b?.click();
  await flush();
}

async function screen(setup: (tr: FakeTransport) => void = () => undefined) {
  const { app, transport } = await appWith((tr) => {
    tr.on('agents.list', ok(agents));
    tr.on('memory.list', ok({ items: ALL, next_cursor: null }));
    tr.on('memory.held_writes', err('SVC_COMMAND_UNKNOWN', 'memory.held_writes'));
    tr.on('memory.citations_to_confirm', err('SVC_COMMAND_UNKNOWN', 'memory.citations_to_confirm'));
    tr.on('memory.things', ok({ items: [STRATEGY, KESTREL].map((x) => ({ ...x, other_names: [], fact_count: 1, link_count: 0 })), next_cursor: null }));
    tr.on('memory.thing', (input) => ok({ thing: { ...KESTREL, id: (input as { id: string }).id, other_names: [], fact_count: 1, link_count: 0 }, facts: [], links: [], summaries: [] }));
    tr.on('memory.thing_topics', ok({ things: [{ thing_id: STRATEGY.id, topic: 'Strategy', set_by: 'ordin', set_at: '2026-01-10T00:00:00Z' }], topics: ['Strategy'] }));
    setup(tr);
  });
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(Memory, { target, props: { backend: app.local, workspaceId: WS, instance: null }, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  drawn = { target, part };
  await flush();
  return { target, transport };
}

describe('SM-SF-27 — the Facts list on screen', () => {
  it('Happy path — groups by thing with their counts; lines under a thing hide the subject; a group collapses', async () => {
    const { target } = await screen();
    const heads = Array.from(target.querySelectorAll('.group-toggle')).map(words);
    expect(heads).toEqual([`${STRATEGY.name} (2)`, 'Kestrel (1)', 'Loose (1)']);
    const first = target.querySelector('.fact-group .fact-list');
    expect(words(first)).not.toContain(STRATEGY.name);
    expect(words(first)).toContain('budget');
    const toggle = target.querySelector<HTMLButtonElement>('.group-toggle')!;
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    toggle.click();
    await flush();
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(target.querySelectorAll('.fact-group')[0]?.querySelector('.fact-list')).toBeNull();
  });

  it('Group by Topic, the search with its marks, and a chip', async () => {
    const { target } = await screen();
    const select = Array.from(target.querySelectorAll('select')).find((s) => words(s.closest('label')).startsWith(t('memory-group-by')))!;
    select.value = 'topic';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await flush();
    expect(Array.from(target.querySelectorAll('.group-toggle')).map(words)).toEqual(['Strategy (2)', `${t('memory-group-no-topic')} (2)`]);
    const search = target.querySelector<HTMLInputElement>('input[type="search"]')!;
    search.value = 'three';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();
    expect(Array.from(target.querySelectorAll('.fact-group')).length).toBe(1);
    expect(words(target.querySelector('mark'))).toBe('Three');
    search.value = '';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    await press(target, t('memory-filter-conflicts'));
    expect(Array.from(target.querySelectorAll('.group-toggle')).map(words)).toEqual([`${t('memory-group-no-topic')} (1)`]);
  });

  it('Happy path — Change topic… from the icon button on the thing’s row offers the existing topics and a new one', async () => {
    const set = vi.fn(async () => ok({ things: [{ thing_id: KESTREL.id, topic: 'Suppliers', set_by: 'person', set_at: '2026-01-10T00:00:00Z' }], topics: ['Strategy', 'Suppliers'] }));
    const { target, transport } = await screen((tr) => tr.on('memory.set_thing_topic', set as never));
    const tab = Array.from(target.querySelectorAll('[role="tab"]')).find((b) => words(b) === t('memory-tab-things')) as HTMLElement;
    tab.click();
    await flush();
    expect(words(target.querySelector('.thing-list'))).toContain(t('memory-topic', { topic: 'Strategy' }));
    // One table row per thing, its icon buttons on the right (amended November 2026).
    const rows = target.querySelectorAll<HTMLTableRowElement>('.thing-list tr');
    const change = rows[1]!.lastElementChild!.querySelector<HTMLButtonElement>(`button[aria-label="${t('memory-topic-change')}"]`)!;
    expect(change.title).toBe(t('memory-topic-change'));
    change.click();
    await flush();
    const dialog = target.querySelector('[role="dialog"], .confirm') ?? target;
    expect(words(dialog)).toContain(t('memory-topic-title', { thing: 'Kestrel' }));
    expect(words(dialog)).toContain('Strategy');
    const radios = Array.from(dialog.querySelectorAll<HTMLInputElement>('input[type="radio"][name="memory-topic"]'));
    radios.find((r) => words(r.closest('label')) === t('memory-topic-new'))!.click();
    await flush();
    const box = dialog.querySelector<HTMLInputElement>(`input[aria-label="${t('memory-topic-new')}"]`)!;
    box.value = 'Suppliers';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();
    await press(target, t('memory-topic-save'));
    expect(transport.sentOf('memory.set_thing_topic')[0]?.input).toEqual({ workspace_id: WS, id: KESTREL.id, topic: 'Suppliers' });
    expect(words(target.querySelector('.thing-list'))).toContain(t('memory-topic', { topic: 'Suppliers' }));
  });

  it('the thing’s page shows its topic and Change topic…', async () => {
    const { target } = await screen();
    const tab = Array.from(target.querySelectorAll('[role="tab"]')).find((b) => words(b) === t('memory-tab-things')) as HTMLElement;
    tab.click();
    await flush();
    await press(target, STRATEGY.name);
    expect(words(target.querySelector('.thing-topic-line'))).toContain(t('memory-topic', { topic: 'Strategy' }));
    expect(buttonNamed(target, t('memory-topic-change'))).toBeDefined();
  });
});
