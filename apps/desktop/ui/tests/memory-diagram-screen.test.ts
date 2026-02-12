// @vitest-environment happy-dom
// What Ordin remembers in a DOM (ScreensModule.md SM-SF-13 to SM-SF-24): Keep this one, Correct a value,
// the Words tab, a thing's summaries, What Ordin read on a knowledge page, and the diagram screen with its
// lists, its keyboard path and its errors. Drawn in a DOM because each step follows a press and a reply.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import type { ChartInstance } from '../src/lib/blocks/chart-draw';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import Knowledge from '../src/lib/views/fixed/Knowledge.svelte';
import Memory from '../src/lib/views/fixed/Memory.svelte';
import MemoryDiagram from '../src/lib/views/fixed/MemoryDiagram.svelte';
import { err, ok, settle, type FakeTransport } from './support/fake-backend';
import { appWith, loadWindowCatalogue } from './support/ssr';
import { thingId, uid } from './support/ids';

beforeAll(loadWindowCatalogue);

/** A test may stand in for ECharts on the diagram screen to press an arrow; otherwise the real loader runs. */
const chart = vi.hoisted(() => ({ fake: null as null | (() => Promise<unknown>) }));
vi.mock('../src/lib/blocks/chart-draw', async (original) => {
  const real = await original<typeof import('../src/lib/blocks/chart-draw')>();
  return { ...real, loadEcharts: (...args: unknown[]) => (chart.fake ? chart.fake() : (real.loadEcharts as (...a: unknown[]) => unknown)(...args)) };
});

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const KESTREL = { id: thingId('kestrel'), name: 'Kestrel' };
const TOM = { id: thingId('tom'), name: 'Tom Reid' };
const NORTHWIND = { id: thingId('northwind'), name: 'Northwind' };
const THINGS = [KESTREL, TOM, NORTHWIND];

const fact = (id: string, over: Record<string, unknown> = {}) => ({
  id: uid(id), statement: `Statement ${id}`, kind: 'known', true_from: '2026-01-10T00:00:00Z', written_at: '2026-01-10T00:00:00Z',
  came_from: { kind: 'person' }, fading: false, may_be_out_of_date: false, needs_check: false, ...over,
});
const triple = (object: Record<string, unknown>, label = 'account manager', subject = KESTREL) => ({ subject, predicate: { key: label.replace(/ /g, ''), label }, object });
const manager = fact('a', { triple: triple({ kind: 'thing', thing: TOM }) });
const thingView = (ref: { id: string; name: string }) => ({ ...ref, other_names: [], fact_count: 2, link_count: 1 });
const detail = (ref: { id: string; name: string }, over: Record<string, unknown> = {}) => ({ thing: thingView(ref), facts: [manager], links: [], summaries: [], ...over });
const view = (centre: { id: string; name: string } | null, over: Record<string, unknown> = {}) => ({
  centre,
  things: [
    { thing: KESTREL, fact_count: 3, ...(centre ? { step: 0 } : {}) },
    { thing: TOM, fact_count: 1, ...(centre ? { step: 1 } : {}) },
    { thing: NORTHWIND, fact_count: 1, ...(centre ? { step: 2 } : {}) },
  ],
  joins: [
    { from: KESTREL.id, to: TOM.id, word: { key: 'accountManager', label: 'account manager' }, fact_id: uid('a'), conflict: false },
    { from: TOM.id, to: NORTHWIND.id, word: { key: 'worksFor', label: 'works for' }, fact_id: uid('w'), conflict: false },
  ],
  values: [],
  conflicts: [],
  left_out: 0,
  total_things: 3,
  ...over,
});
/** `memory.diagram` around the thing named by `centre`, or without one. */
const diagramReply = (over: Record<string, unknown> = {}) => (input: unknown) => {
  const centre = (input as { centre?: string }).centre;
  return ok(view(centre === undefined ? null : (THINGS.find((x) => x.id === centre) ?? null), over));
};

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
const buttonNamed = (root: ParentNode, name: string): HTMLButtonElement | undefined =>
  // A button's accessible name: its aria-label (an icon button, SM-SF-13 amended January 2026), else its words.
  Array.from(root.querySelectorAll('button')).find((b) => (b.getAttribute('aria-label') ?? words(b)) === name);
/** A fact line's claim as read on screen, without its thing menus' hidden names. */
const claimOf = (node: Element | null | undefined): string => {
  const copy = node?.cloneNode(true) as Element | undefined;
  copy?.querySelectorAll('.thing-menu, .thing-menu-list').forEach((m) => m.remove());
  return words(copy);
};
const live = (root: HTMLElement): string => words(root.querySelector('[aria-live="polite"]'));

/** What Ordin remembers for W, drawn in the window's context. */
async function memoryScreen(setup: (t: FakeTransport) => void): Promise<{ app: AppContext; target: HTMLElement; transport: FakeTransport }> {
  const { app, transport } = await appWith((tr) => {
    tr.on('agents.list', ok([]));
    tr.on('memory.list', ok({ items: [], next_cursor: null }));
    tr.on('memory.held_writes', err('SVC_COMMAND_UNKNOWN', 'memory.held_writes'));
    tr.on('memory.citations_to_confirm', err('SVC_COMMAND_UNKNOWN', 'memory.citations_to_confirm'));
    tr.on('memory.things', ok({ items: THINGS.map(thingView), next_cursor: null }));
    tr.on('memory.thing', (input) => ok(detail(THINGS.find((x) => x.id === (input as { id: string }).id) ?? KESTREL)));
    setup(tr);
  });
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(Memory, { target, props: { backend: app.local, workspaceId: WS, instance: null }, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  drawn = { target, part };
  await flush();
  transport.sent.length = 0;
  return { app, target, transport };
}

async function press(root: ParentNode, name: string): Promise<void> {
  const button = buttonNamed(root, name);
  expect(button, `the button "${name}"`).toBeDefined();
  button?.click();
  await flush();
}

/** SM-SF-13: a fact line is collapsed; Details opens it. */
async function details(line: ParentNode): Promise<void> {
  await press(line, 'Details');
}

async function openTab(target: HTMLElement, name: string): Promise<void> {
  const tab = Array.from(target.querySelectorAll('[role="tab"]')).find((b) => words(b) === name) as HTMLElement | undefined;
  expect(tab, `the tab "${name}"`).toBeDefined();
  tab?.click();
  await flush();
}

const diagramThings = (target: HTMLElement): HTMLButtonElement[] => Array.from(target.querySelectorAll<HTMLButtonElement>('.diagram-things button'));

describe('SM-SF-20 — Show as diagram', () => {
  it('Happy path — the diagram around a thing: centred on Kestrel, the intro, Facts about Kestrel; pressing Tom Reid centres it and says so', async () => {
    const { target, transport } = await memoryScreen((tr) => tr.on('memory.diagram', diagramReply()));
    await openTab(target, 'Things');
    await press(target, 'Kestrel');
    await press(target, 'Show as diagram');
    expect(transport.sentOf('memory.diagram')[0]?.input).toEqual({ workspace_id: WS, centre: KESTREL.id });
    expect(words(target.querySelector('#memory-diagram-heading'))).toBe('Diagram of what Ordin remembers');
    expect(words(target)).toContain('Each box is something Ordin knows about.');
    expect(words(target)).toContain('Facts about Kestrel');
    expect(live(target)).toBe('Centred on Kestrel');
    // SM-SF-22: the canvas is hidden from assistive technology; the lists are its equivalent.
    expect(target.querySelector('.diagram-canvas')?.getAttribute('aria-hidden')).toBe('true');
    // SM-SF-24: no left-out line when nothing is left out.
    expect(words(target)).not.toContain('Showing');
    const tom = diagramThings(target).find((b) => words(b) === 'Tom Reid');
    tom?.click();
    await flush();
    expect(transport.sentOf('memory.diagram').at(-1)?.input).toEqual({ workspace_id: WS, centre: TOM.id });
    expect(live(target)).toBe('Centred on Tom Reid');
  });

  it('Edge — from the Things tab the diagram has no centre and Things in the diagram is the only list; Back returns to the Things tab', async () => {
    const { target, transport } = await memoryScreen((tr) => tr.on('memory.diagram', diagramReply()));
    await openTab(target, 'Things');
    await press(target, 'Show as diagram');
    expect(transport.sentOf('memory.diagram')[0]?.input).toEqual({ workspace_id: WS });
    expect(words(target)).not.toContain('Facts about');
    expect(words(target)).toContain('Things in the diagram');
    // SM-SF-22 (amended January 2026; user decision; Approved by the user January 2026): nothing selected, so
    // under the drawing only a short hint; the drawing comes first and spans the pane, the lists under it.
    expect(words(target.querySelector('.diagram-hint'))).toBe('Press a box or an arrow in the diagram to see its facts here.');
    const body = target.querySelector('.diagram-body')!;
    expect(body.firstElementChild?.classList.contains('diagram-canvas')).toBe(true);
    expect(body.querySelector('.diagram-lists .diagram-things')).toBeNull();
    // Selecting a thing replaces the hint with its facts.
    diagramThings(target).find((b) => words(b) === 'Kestrel')?.click();
    await flush();
    expect(target.querySelector('.diagram-hint')).toBeNull();
    expect(words(target.querySelector('.diagram-lists'))).toContain('Facts about Kestrel');
    await press(target, 'Back to the list');
    expect(target.querySelector('#memory-diagram-heading')).toBeNull();
    expect(target.querySelector('#memory-tab-things')?.getAttribute('aria-selected')).toBe('true');
    expect(buttonNamed(target, 'Show as diagram')).toBeDefined();
  });

  it('Edge — opened from a thing\'s page, Escape returns to that page', async () => {
    const { target } = await memoryScreen((tr) => tr.on('memory.diagram', diagramReply()));
    await openTab(target, 'Things');
    await press(target, 'Kestrel');
    await press(target, 'Show as diagram');
    buttonNamed(target, 'Back to the list')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await flush();
    expect(target.querySelector('#memory-diagram-heading')).toBeNull();
    expect(words(target.querySelector('h3'))).toBe('Kestrel');
  });

  it('Edge — the left-out line: "Showing 40 of 212 things — search to centre on one", and a found thing centres the diagram', async () => {
    const { target, transport } = await memoryScreen((tr) => {
      tr.on('memory.diagram', diagramReply({ left_out: 172, total_things: 212, things: Array.from({ length: 40 }, (_, i) => ({ thing: { id: thingId(`t${i}`), name: `Thing ${i}` }, fact_count: 1 })) }));
    });
    await openTab(target, 'Things');
    await press(target, 'Show as diagram');
    expect(words(target)).toContain('Showing 40 of 212 things — search to centre on one');
    transport.on('memory.things', ok({ items: [thingView(NORTHWIND)], next_cursor: null }));
    const search = target.querySelector<HTMLInputElement>('.diagram input[type="search"]');
    expect(search).not.toBeNull();
    search!.value = 'north';
    search!.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();
    expect(transport.sentOf('memory.things').at(-1)?.input).toEqual({ workspace_id: WS, search: 'north' });
    await press(target.querySelector('.diagram-matches')!, 'Northwind');
    expect(transport.sentOf('memory.diagram').at(-1)?.input).toEqual({ workspace_id: WS, centre: NORTHWIND.id });
  });
});

describe('SM-SF-22 — using the diagram', () => {
  it('Edge — keyboard users centre through the list: every drawn thing is a button in the order of things', async () => {
    const { target, transport } = await memoryScreen((tr) => tr.on('memory.diagram', diagramReply()));
    await openTab(target, 'Things');
    await press(target, 'Kestrel');
    await press(target, 'Show as diagram');
    // A thing two steps away (Northwind) is reachable from the keyboard as a button in the list's order.
    const list = diagramThings(target);
    expect(list.map(words)).toEqual(['Kestrel', 'Tom Reid', 'Northwind']);
    expect(list.every((b) => b.tabIndex !== -1 && b.type === 'button')).toBe(true);
    list[2]?.focus();
    expect(document.activeElement).toBe(list[2]);
    list[2]?.click(); // Enter on a button presses it
    await flush();
    expect(transport.sentOf('memory.diagram').at(-1)?.input).toEqual({ workspace_id: WS, centre: NORTHWIND.id });
  });

  it('Edge — a thing in a fact line beside the diagram centres it and the thing\'s page is not opened', async () => {
    const { target, transport } = await memoryScreen((tr) => tr.on('memory.diagram', diagramReply()));
    await openTab(target, 'Things');
    await press(target, 'Kestrel');
    await press(target, 'Show as diagram');
    const line = target.querySelector('.diagram-lists .fact-line');
    expect(line?.getAttribute('aria-label')).toBe('Kestrel account manager Tom Reid');
    await press(line!, 'Tom Reid');
    expect(transport.sentOf('memory.diagram').at(-1)?.input).toEqual({ workspace_id: WS, centre: TOM.id });
    expect(target.querySelector('#memory-diagram-heading')).not.toBeNull();
  });

  it('Happy path — the line\'s thing menu offers Open Tom Reid, which opens the thing\'s page', async () => {
    const { target, transport } = await memoryScreen((tr) => tr.on('memory.diagram', diagramReply()));
    await openTab(target, 'Things');
    await press(target, 'Kestrel');
    await press(target, 'Show as diagram');
    const line = target.querySelector('.diagram-lists .fact-line')!;
    await details(line);
    const menus = Array.from(line.querySelectorAll<HTMLButtonElement>('button[aria-haspopup="menu"]'));
    // SM-SF-22 (amended January 2026): under Facts about Kestrel the subject is not repeated, so only the value has a menu.
    expect(menus.map(words)).toEqual(['More actions for Tom Reid']);
    menus.find((m) => words(m) === 'More actions for Tom Reid')?.click();
    await flush();
    transport.sent.length = 0;
    await press(line, 'Open Tom Reid');
    expect(transport.sentOf('memory.thing')[0]?.input).toEqual({ id: TOM.id, workspace_id: WS });
    expect(target.querySelector('#memory-diagram-heading')).toBeNull();
    expect(words(target.querySelector('h3'))).toBe('Tom Reid');
  });
});

describe('SM-SF-22 (amended January 2026) — the lists beside the diagram', () => {
  const goal = fact('g', { triple: triple({ kind: 'text', value: '2038' }, 'target year') });
  const second = fact('b', { triple: triple({ kind: 'thing', thing: NORTHWIND }) });
  const other = fact('o', { triple: triple({ kind: 'thing', thing: KESTREL }, 'works with', TOM) });
  const onKestrel = (tr: FakeTransport) => {
    tr.on('memory.diagram', diagramReply());
    tr.on('memory.thing', (input) => ok(detail(THINGS.find((x) => x.id === (input as { id: string }).id) ?? KESTREL, { facts: [manager, other, goal, second] })));
  };
  async function openOnKestrel(target: HTMLElement): Promise<void> {
    await openTab(target, 'Things');
    await press(target, 'Kestrel');
    await press(target, 'Show as diagram');
  }

  it('Happy path — a line about the centre reads word · value; the accessible name keeps the three parts', async () => {
    const { target } = await memoryScreen(onKestrel);
    await openOnKestrel(target);
    const line = target.querySelector(`#fact-line-${uid('g')}`)!;
    expect(claimOf(line.querySelector('.claim'))).toBe('target year · 2038');
    expect(line.getAttribute('aria-label')).toBe('Kestrel target year 2038');
    // A fact whose subject is another thing keeps its subject.
    expect(claimOf(target.querySelector(`#fact-line-${uid('o')} .claim`))).toBe('Tom Reid · works with · Kestrel');
  });

  it('Happy path — grouped by word: written values first, then links to things, then facts about other things', async () => {
    const { target } = await memoryScreen(onKestrel);
    await openOnKestrel(target);
    const groups = Array.from(target.querySelectorAll('.diagram-lists .fact-groups > li > ul'));
    expect(groups.map((g) => g.getAttribute('aria-label'))).toEqual(['target year', 'account manager', 'works with']);
    expect(Array.from(groups[1]!.querySelectorAll('.fact-line')).map((l) => l.id)).toEqual([`fact-line-${uid('a')}`, `fact-line-${uid('b')}`]);
  });

  it('Happy path — an arrow opens its fact at the top of the lists, titled and focused; Close closes it only', async () => {
    const { target } = await memoryScreen((tr) => {
      onKestrel(tr);
      tr.on('memory.get', ok({ facts: [manager] }));
    });
    let clicked: ((params: unknown) => void) | undefined;
    const instance: ChartInstance = { setOption: () => undefined, resize: () => undefined, dispose: () => undefined, on: (_e, h) => void (clicked = h) };
    chart.fake = async () => instance;
    try {
      await openOnKestrel(target);
      const lists = target.querySelector('.diagram-lists')!;
      expect(lists.querySelector('.diagram-panel')).toBeNull();
      clicked?.({ dataType: 'edge', data: { factId: uid('a') } });
      await flush();
      const panel = lists.querySelector('.diagram-panel');
      // At the top of the lists column, before Facts about Kestrel.
      expect(lists.firstElementChild).toBe(panel);
      expect(words(panel?.querySelector('h4'))).toBe('The fact you pressed');
      expect(document.activeElement).toBe(panel?.querySelector('h4'));
      // Opened expanded, with all three parts: the subject is not dropped in the panel.
      expect(claimOf(panel?.querySelector('.claim'))).toBe('Kestrel · account manager · Tom Reid');
      await press(panel!, 'Close');
      expect(lists.querySelector('.diagram-panel')).toBeNull();
      expect(target.querySelector('#memory-diagram-heading')).not.toBeNull();
    } finally {
      chart.fake = null;
    }
  });
});

describe('SM-SF-21 — the drawing', () => {
  const colourless = { thing: KESTREL, fact_count: 1 };

  it('Happy path — the graph is drawn with ECharts\' aria off, and a press on a box or an arrow is handed on', async () => {
    let clicked: ((params: unknown) => void) | undefined;
    const options: unknown[] = [];
    const instance: ChartInstance = { setOption: (o) => void options.push(o), resize: () => undefined, dispose: () => undefined, on: (_e, h) => void (clicked = h) };
    const pressed: unknown[] = [];
    const target = document.createElement('div');
    document.body.appendChild(target);
    const part = mount(MemoryDiagram, { target, props: { view: view(KESTREL), diagnostics: undefined, onpress: (p: unknown) => pressed.push(p), load: async () => instance } });
    drawn = { target, part };
    await flush();
    expect((options[0] as { aria: unknown }).aria).toEqual({ enabled: false });
    clicked?.({ dataType: 'node', data: { id: `thing:${TOM.id}` } });
    clicked?.({ dataType: 'edge', data: { factId: uid('a') } });
    expect(pressed).toEqual([{ kind: 'thing', id: TOM.id }, { kind: 'fact', id: uid('a') }]);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-SF-28
  it('SM-SF-28 — the diagram drawn with a selected fact draws that fact\'s arrow thicker, and no other', async () => {
    const options: unknown[] = [];
    const instance: ChartInstance = { setOption: (o) => void options.push(o), resize: () => undefined, dispose: () => undefined, on: () => undefined };
    const target = document.createElement('div');
    document.body.appendChild(target);
    const part = mount(MemoryDiagram, { target, props: { view: view(KESTREL), diagnostics: undefined, selected: uid('w'), onpress: () => undefined, load: async () => instance } });
    drawn = { target, part };
    await flush();
    const links = (options.at(-1) as { series: Array<{ links: Array<{ factId: string; lineStyle: { width: number } }> }> }).series[0]!.links;
    expect(links.filter((l) => l.lineStyle.width === 3).map((l) => l.factId)).toEqual([uid('w')]);
  });

  it('Error path — a drawing failure records UI_CHART_RENDER_FAILED and hides the canvas, so the list stands alone', async () => {
    const record = vi.fn();
    const target = document.createElement('div');
    document.body.appendChild(target);
    const part = mount(MemoryDiagram, {
      target,
      props: { view: view(null, { things: [colourless] }), diagnostics: { record }, onpress: () => undefined, load: async () => { throw new Error('no canvas'); } },
    });
    drawn = { target, part };
    await flush();
    expect(record).toHaveBeenCalledWith('UI_CHART_RENDER_FAILED');
    expect(target.querySelector<HTMLElement>('.diagram-canvas')?.hidden).toBe(true);
    // The person is told in words why only the lists are there.
    expect(target.querySelector('.diagram-not-drawn')?.textContent).toBe('The diagram could not be drawn. The lists show the same facts.');
  });

  it('Happy path — a drawn diagram says nothing about failing', async () => {
    const instance: ChartInstance = { setOption: () => undefined, resize: () => undefined, dispose: () => undefined };
    const target = document.createElement('div');
    document.body.appendChild(target);
    const part = mount(MemoryDiagram, { target, props: { view: view(KESTREL), diagnostics: undefined, onpress: () => undefined, load: async () => instance } });
    drawn = { target, part };
    await flush();
    expect(target.querySelector('.diagram-not-drawn')).toBeNull();
    expect(target.querySelector<HTMLElement>('.diagram-canvas')?.hidden).toBe(false);
  });
});

describe('SM-SF-23 — errors', () => {
  it('Error — the centred thing was forgotten: "Ordin no longer remembers that." and the diagram is read again without a centre', async () => {
    const { target, transport } = await memoryScreen((tr) =>
      tr.on('memory.diagram', (input) => ((input as { centre?: string }).centre ? err('MEM_THING_NOT_FOUND', 'Ordin no longer remembers that.') : ok(view(null)))),
    );
    await openTab(target, 'Things');
    await press(target, 'Kestrel');
    await press(target, 'Show as diagram');
    expect(words(target.querySelector('[role="alert"]'))).toBe('Ordin no longer remembers that.');
    expect(transport.sentOf('memory.diagram').map((e) => e.input)).toEqual([{ workspace_id: WS, centre: KESTREL.id }, { workspace_id: WS }]);
    expect(diagramThings(target).map(words)).toEqual(['Kestrel', 'Tom Reid', 'Northwind']);
  });

  it.each(['MEM_STORE_UNAVAILABLE', 'MEM_STORE_TIMEOUT'])('Error — %s draws its sentence and Try again, which reads the diagram again', async (code) => {
    let fail = true;
    const { target, transport } = await memoryScreen((tr) => tr.on('memory.diagram', (input) => (fail ? err(code, "ordin can't reach its memory right now. Try again in a moment.") : diagramReply()(input))));
    await openTab(target, 'Things');
    await press(target, 'Show as diagram');
    expect(words(target.querySelector('[role="alert"]'))).toContain("ordin can't reach its memory right now.");
    fail = false;
    await press(target.querySelector('[role="alert"]')!, 'Try again');
    expect(transport.sentOf('memory.diagram')).toHaveLength(2);
    expect(target.querySelector('[role="alert"]')).toBeNull();
    expect(diagramThings(target)).toHaveLength(3);
  });

  it('Error — a backend without memory.diagram hides Show as diagram', async () => {
    const { target } = await memoryScreen((tr) => tr.on('memory.diagram', err('SVC_COMMAND_UNKNOWN', 'memory.diagram')));
    await openTab(target, 'Things');
    await press(target, 'Show as diagram');
    expect(target.querySelector('#memory-diagram-heading')).toBeNull();
    expect(buttonNamed(target, 'Show as diagram')).toBeUndefined();
  });
});

describe('SM-SF-14 — Keep this one in the screen', () => {
  const A = fact('A', { triple: triple({ kind: 'money', amount: '100', currency: 'GBP' }, 'price'), conflicts_with: [uid('B')] });
  const B = fact('B', { triple: triple({ kind: 'money', amount: '120', currency: 'GBP' }, 'price'), conflicts_with: [uid('A')] });

  it('Happy path — the dialog names the other fact; confirming sends keep_conflict_side and announces it', async () => {
    const { target, transport } = await memoryScreen((tr) => {
      tr.on('memory.list', ok({ items: [A, B], next_cursor: null }));
      tr.on('memory.keep_conflict_side', ok(A));
    });
    const lineA = target.querySelector(`#fact-line-${uid('A')}`)!;
    await details(lineA);
    expect(words(lineA.querySelector('.conflict'))).toContain('Conflicts with: Kestrel · price · £120.00');
    expect(lineA.querySelector('.conflict svg use')?.getAttribute('href')).toBe('#i-info');
    await press(lineA, 'Keep this one');
    const dialog = target.querySelector('[role="dialog"], [role="alertdialog"]');
    expect(words(dialog)).toContain('Keep this fact?');
    expect(words(dialog)).toContain('Ordin will stop using “Kestrel · price · £120.00”.');
    await press(dialog!, 'Keep this one');
    expect(transport.sentOf('memory.keep_conflict_side')[0]?.input).toEqual({ keep: uid('A'), other: uid('B') });
    expect(live(target)).toBe('Kept. Ordin no longer uses the other fact.');
  });

  it('Edge — the link to a fact in the same list moves focus to its line', async () => {
    const { target } = await memoryScreen((tr) => tr.on('memory.list', ok({ items: [A, B], next_cursor: null })));
    const lineA = target.querySelector(`#fact-line-${uid('A')}`)!;
    await details(lineA);
    buttonNamed(lineA, 'Kestrel · price · £120.00')?.click();
    flushSync();
    expect(document.activeElement?.id).toBe(`fact-line-${uid('B')}`);
  });

  it('Error path — MEM_CONFLICT_NOT_FOUND is drawn as its sentence and the list is read again', async () => {
    const { target, transport } = await memoryScreen((tr) => {
      tr.on('memory.list', ok({ items: [A, B], next_cursor: null }));
      tr.on('memory.keep_conflict_side', err('MEM_CONFLICT_NOT_FOUND', 'These facts no longer disagree.'));
    });
    const lineA = target.querySelector(`#fact-line-${uid('A')}`)!;
    await details(lineA);
    await press(lineA, 'Keep this one');
    await press(target.querySelector('[role="dialog"], [role="alertdialog"]')!, 'Keep this one');
    expect(words(target)).toContain('These facts no longer disagree.');
    expect(transport.sentOf('memory.list')).toHaveLength(1);
  });
});

describe('SM-SF-15 — Correct a value in the screen', () => {
  const money = fact('m', { triple: triple({ kind: 'money', amount: '3000000', currency: 'GBP' }) });

  it('Happy path — one field per part of the value; Save is off while unchanged; the currency is upper-cased as typed', async () => {
    const { target, transport } = await memoryScreen((tr) => {
      tr.on('memory.list', ok({ items: [money], next_cursor: null }));
      tr.on('memory.correct', ok(money));
    });
    const line = target.querySelector(`#fact-line-${uid('m')}`)!;
    await details(line);
    await press(line, 'Correct');
    expect(words(line.querySelector('legend'))).toBe('New value for “account manager”');
    const save = buttonNamed(line, 'Save')!;
    expect(save.disabled).toBe(true);
    const [amount, currency] = Array.from(line.querySelectorAll<HTMLInputElement>('fieldset input'));
    currency!.value = 'eur';
    currency!.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    expect(currency!.value).toBe('EUR');
    amount!.value = '3100000';
    amount!.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    expect(save.disabled).toBe(false);
    save.click();
    await flush();
    expect(transport.sentOf('memory.correct')[0]?.input).toEqual({ id: uid('m'), object: { money: { amount: '3100000', currency: 'EUR' } } });
  });

  it('Edge — a date takes three forms and shows its hint; a date of no form keeps Save off', async () => {
    const started = fact('d', { triple: triple({ kind: 'date', value: '2026-04' }, 'starts on') });
    const { target } = await memoryScreen((tr) => tr.on('memory.list', ok({ items: [started], next_cursor: null })));
    const line = target.querySelector(`#fact-line-${uid('d')}`)!;
    // SM-SF-27: grouped by thing (the default), the line under the thing's heading reads word · value.
    expect(words(line.querySelector('.claim'))).toBe('starts on · April 2026');
    await details(line);
    await press(line, 'Correct');
    expect(words(line)).toContain('A year, a month (2026-04) or a day (2026-04-15)');
    const box = line.querySelector<HTMLInputElement>('fieldset input')!;
    box.value = '04/2026';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    expect(buttonNamed(line, 'Save')?.disabled).toBe(true);
  });

  it('Error path — a refusal is drawn under the line', async () => {
    const { target } = await memoryScreen((tr) => {
      tr.on('memory.list', ok({ items: [money], next_cursor: null }));
      tr.on('memory.correct', err('MEM_TRIPLE_INVALID', 'That is what Ordin already keeps.'));
    });
    const line = target.querySelector(`#fact-line-${uid('m')}`)!;
    await details(line);
    await press(line, 'Correct');
    const [amount] = Array.from(line.querySelectorAll<HTMLInputElement>('fieldset input'));
    amount!.value = '1';
    amount!.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    await press(line, 'Save');
    expect(words(target)).toContain('That is what Ordin already keeps.');
  });
});

describe('SM-SF-17 — the Words tab in the screen', () => {
  const term = (key: string, label: string, over: Record<string, unknown> = {}) => ({
    key, label, meaning: `Meaning of ${label}`, object: 'thing', single_valued: false, seed_unrenamed: false, added_by: { kind: 'person' }, fact_count: 1, ...over,
  });
  const provides = term('provides', 'provides', { fact_count: 3 });
  const supplies = term('supplies', 'supplies');
  const price = term('price', 'price', { object: 'money', single_valued: true, seed_unrenamed: true, added_by: { kind: 'seed' } });

  async function wordsTab(setup: (t: FakeTransport) => void = () => undefined) {
    const s = await memoryScreen((tr) => {
      tr.on('memory.vocabulary', ok({ terms: [price, provides, supplies] }));
      setup(tr);
    });
    await openTab(s.target, 'Kinds of fact');
    return s;
  }
  const row = (target: HTMLElement, label: string): HTMLElement =>
    Array.from(target.querySelectorAll<HTMLElement>('.word')).find((li) => words(li.querySelector('strong')) === label)!;

  it('Happy path — merge "provides" into "supplies": "price" is not offered, merge_term is sent and announced', async () => {
    const { target, transport } = await wordsTab((tr) => tr.on('memory.merge_term', ok({ kept: supplies, rewritten: 3 })));
    expect(transport.sentOf('memory.vocabulary')).toHaveLength(1);
    expect(words(target)).toContain('Each one is a kind of fact Ordin can keep, for example price, supplies or works for');
    expect(words(row(target, 'provides'))).toContain('links to a thing · can have several · Added by you · Used by 3 facts');
    expect(words(row(target, 'price'))).toContain('links to an amount of money · one at a time · Built in');
    await press(row(target, 'provides'), 'Merge into…');
    const dialog = target.querySelector('[role="dialog"], [role="alertdialog"]')!;
    expect(words(dialog)).toContain('The 3 facts of this kind will become the kind you choose.');
    const options = Array.from(dialog.querySelectorAll('label')).map(words);
    expect(options).toEqual(['supplies']);
    await press(dialog, 'Merge');
    expect(transport.sentOf('memory.merge_term')[0]?.input).toEqual({ workspace_id: WS, key: 'provides', into: 'supplies' });
    expect(live(target)).toBe('Merged. 3 facts are now of the other kind.');
    expect(transport.sentOf('memory.vocabulary')).toHaveLength(2);
  });

  // SM-SF-17 (amended January 2026, user decision): the screen is called Memory and the tab Kinds of fact.
  it('Edge — the screen reads "Memory" and its tab "Kinds of fact"', async () => {
    const { target } = await wordsTab();
    expect(words(target.querySelector('#memory-heading'))).toBe('Memory');
    expect(target.querySelector('[role="tablist"]')?.getAttribute('aria-label')).toBe('Memory');
    expect(Array.from(target.querySelectorAll('[role="tab"]')).map(words)).toContain('Kinds of fact');
    expect(words(target)).not.toContain('What Ordin remembers');
  });

  it('Happy path — Rename in place sends rename_term and announces it', async () => {
    const { target, transport } = await wordsTab((tr) => tr.on('memory.rename_term', ok({ ...supplies, label: 'delivers' })));
    const li = row(target, 'supplies');
    await press(li, 'Rename');
    const box = li.querySelector<HTMLInputElement>('input[type="text"]')!;
    expect(buttonNamed(li, 'Save')?.disabled).toBe(true);
    box.value = 'delivers';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    await press(li, 'Save');
    expect(transport.sentOf('memory.rename_term')[0]?.input).toEqual({ workspace_id: WS, key: 'supplies', label: 'delivers' });
    expect(live(target)).toBe('Renamed. Facts of this kind read the new name.');
  });

  it('Edge — a word with no other word of its kind has no Merge into…', async () => {
    const { target } = await wordsTab();
    expect(buttonNamed(row(target, 'price'), 'Rename')).toBeDefined();
    expect(buttonNamed(row(target, 'price'), 'Merge into…')).toBeUndefined();
  });

  it('Happy path — each kind is a table row; Rename and Merge into… are buttons at its right end, each an icon-only button with a tooltip', async () => {
    const { target } = await wordsTab();
    const tr = row(target, 'provides');
    expect(tr.tagName).toBe('TR');
    expect(tr.closest('table')).not.toBeNull();
    const last = tr.lastElementChild!;
    for (const [label, icon] of [['Rename', 'edit'], ['Merge into…', 'fold']] as const) {
      const b = buttonNamed(tr, label)!;
      expect(b, label).toBeDefined();
      expect(last.contains(b)).toBe(true);
      expect(b.title).toBe(label);
      expect(words(b)).toBe('');
      expect(b.querySelector('use')?.getAttribute('href')).toBe(`#i-${icon}`);
    }
    expect(tr.querySelector('[aria-haspopup="menu"]')).toBeNull();
  });

  it('Error path — MEM_TERM_LABEL_TAKEN is drawn under the box', async () => {
    const { target } = await wordsTab((tr) => tr.on('memory.rename_term', err('MEM_TERM_LABEL_TAKEN', 'Another word already reads like that. Merge them instead.')));
    const li = row(target, 'supplies');
    await press(li, 'Rename');
    const box = li.querySelector<HTMLInputElement>('input[type="text"]')!;
    box.value = 'provides';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    await press(li, 'Save');
    expect(words(li)).toContain('Another word already reads like that. Merge them instead.');
  });

  it('Error path — a refused merge is drawn in the dialog, which stays open', async () => {
    const { target } = await wordsTab((tr) => tr.on('memory.merge_term', err('MEM_TERM_TOO_MANY_FACTS', 'This word is used by more than 500 facts, too many to merge at once.')));
    await press(row(target, 'provides'), 'Merge into…');
    await press(target.querySelector('[role="dialog"], [role="alertdialog"]')!, 'Merge');
    expect(words(target.querySelector('[role="dialog"], [role="alertdialog"]'))).toContain('too many to merge at once');
  });
});

describe("SM-SF-19 — a thing's summaries in the screen", () => {
  it('Happy path — What Ordin summed up lists each summary with its date, what it was made from and Forget', async () => {
    const summary = fact('s', { kind: 'summary', statement: 'Kestrel is the main supplier.', made_from_count: 4 });
    const { target } = await memoryScreen((tr) => tr.on('memory.thing', ok(detail(KESTREL, { summaries: [summary] }))));
    await openTab(target, 'Things');
    await press(target, 'Kestrel');
    const section = target.querySelector('[aria-labelledby="memory-summaries-heading"]');
    expect(words(section)).toContain('What Ordin summed up');
    expect(words(section)).toContain('Kestrel is the main supplier.');
    expect(words(section)).toContain('Made from 4 facts');
    expect(buttonNamed(section!, 'Forget')).toBeDefined();
  });

  it('Edge — a thing with no summaries draws no heading', async () => {
    const { target } = await memoryScreen(() => undefined);
    await openTab(target, 'Things');
    await press(target, 'Kestrel');
    expect(words(target)).not.toContain('What Ordin summed up');
  });
});


// The memory screen's tabs (amended January 2026; user decision; Approved by the user January 2026): a WAI-ARIA
// tab list moved with the arrows, Home and End, the focus following the tab shown.
describe('What Ordin remembers — Facts, Things and Kinds of fact are tabs', () => {
  const key = async (el: Element, k: string): Promise<void> => {
    el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
    await flush();
  };
  const selected = (target: HTMLElement): string => words(target.querySelector('[role="tab"][aria-selected="true"]'));

  it('Happy path — the arrows move and show, wrapping at the ends; the shown tab alone is in the tab order', async () => {
    const { target } = await memoryScreen(() => undefined);
    const list = target.querySelector('[role="tablist"]')!;
    expect(list.getAttribute('aria-orientation')).toBe('horizontal');
    expect(Array.from(list.querySelectorAll('[role="tab"]')).map(words)).toEqual(['Facts', 'Things', 'Kinds of fact']);
    await key(target.querySelector('#memory-tab-facts')!, 'ArrowRight');
    expect(selected(target)).toBe('Things');
    expect(document.activeElement?.id).toBe('memory-tab-things');
    expect(target.querySelector('#memory-tab-things')?.getAttribute('tabindex')).toBe('0');
    expect(target.querySelector('#memory-tab-facts')?.getAttribute('tabindex')).toBe('-1');
    await key(target.querySelector('#memory-tab-things')!, 'ArrowLeft');
    await key(target.querySelector('#memory-tab-facts')!, 'ArrowLeft');
    expect(selected(target)).toBe('Kinds of fact');
    expect(target.querySelector('#memory-panel-words')).not.toBeNull();
  });

  it('Keyboard — Home and End go to the first and the last tab; another key changes nothing', async () => {
    const { target } = await memoryScreen(() => undefined);
    await key(target.querySelector('#memory-tab-facts')!, 'End');
    expect(selected(target)).toBe('Kinds of fact');
    await key(target.querySelector('#memory-tab-words')!, 'Home');
    expect(selected(target)).toBe('Facts');
    await key(target.querySelector('#memory-tab-facts')!, 'a');
    expect(selected(target)).toBe('Facts');
  });

  it('Edge — the screen no longer says to share the workspace from the side tree', async () => {
    const { target } = await memoryScreen(() => undefined);
    expect(words(target)).not.toContain('Share this workspace from the side tree');
  });
});
