// @vitest-environment happy-dom
// SM-SF-29: confirm several sources at once, one test per Gherkin scenario.
import { flushSync, mount, unmount } from 'svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { MemoryController } from '../src/lib/views/fixed/memory-controller.svelte';
import Memory from '../src/lib/views/fixed/Memory.svelte';
import { APP_KEY } from '../src/lib/shell/context';
import * as wiring from '../src/lib/backend/wiring';
import { ok, settle } from './support/fake-backend';
import { appWith, loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const fact = (n: number) => ({
  id: uid(`s${n}`), statement: `Fact ${n}.`, kind: 'known', true_from: '2026-02-10T00:00:00Z', written_at: '2026-02-10T00:00:00Z',
  came_from: { kind: 'agent_citing', origin_ref: 'quote.pdf', version: '1' }, fading: false, may_be_out_of_date: false, needs_check: false,
});
const IDS = [1, 2, 3].map((n) => uid(`s${n}`));
let citeIds: string[];

async function controller() {
  const b = await backendWith({
    setup: (tr) => {
      tr.on('agents.list', ok([]));
      tr.on('memory.list', ok({ items: [], next_cursor: null }));
      tr.on('memory.get', (input) => ok({ facts: (input as { ids: string[] }).ids.map((id) => fact(Number(IDS.indexOf(id)) + 1)) }));
    },
  });
  const c = new MemoryController(b.handle, WS);
  await c.loadToCheck();
  return { ...b, c };
}

beforeEach(() => {
  vi.restoreAllMocks();
  citeIds = [...IDS];
  vi.spyOn(wiring, 'heldWrites').mockResolvedValue({ kind: 'ok', result: [] } as never);
  vi.spyOn(wiring, 'citationsToConfirm').mockImplementation(async () => ({ kind: 'ok', result: citeIds }) as never);
});

describe('Confirm several sources at once (SM-SF-29)', () => {
  it('Happy path — ticking two and pressing Confirm selected confirms them in order with one announcement', async () => {
    const confirm = vi.spyOn(wiring, 'confirmCitation').mockResolvedValue({ kind: 'ok', result: {} } as never);
    const { c } = await controller();
    c.toggleTick(IDS[2] as string);
    c.toggleTick(IDS[0] as string);
    expect(c.ticked.size).toBe(2);
    await c.confirmSelected();
    expect(confirm.mock.calls.map((a) => a[2])).toEqual([IDS[0], IDS[2]]);
    expect(c.citations?.map((f) => f.id)).toEqual([IDS[1]]);
    expect(c.ticked.size).toBe(0);
    await settle();
    expect(c.announcer.text).toBe('2 sources confirmed. These facts can now be shared with your team.');
    expect(c.citationFocus).toBe(`memory-citation-${IDS[1]}`);
  });

  it('Select all ticks every source, and Select none clears them', async () => {
    const { c } = await controller();
    c.toggleAll();
    expect(c.allTicked).toBe(true);
    expect(c.ticked.size).toBe(3);
    c.toggleAll();
    expect(c.ticked.size).toBe(0);
  });

  it('Error path — a refusal mid-way stops the run, draws its sentence there and keeps the rest ticked', async () => {
    const confirm = vi.spyOn(wiring, 'confirmCitation')
      .mockResolvedValueOnce({ kind: 'ok', result: {} } as never)
      .mockResolvedValueOnce({ kind: 'error', error: { code: 'MEM_STORE_UNAVAILABLE', message: 'Memory is not available.' } } as never);
    const { c } = await controller();
    c.toggleAll();
    await c.confirmSelected();
    expect(confirm).toHaveBeenCalledTimes(2);
    expect(c.errorOn(IDS[1] as string)?.code).toBe('MEM_STORE_UNAVAILABLE');
    expect(c.citations?.map((f) => f.id)).toEqual([IDS[1], IDS[2]]);
    expect([...c.ticked]).toEqual([IDS[1], IDS[2]]);
    expect(c.confirmingSelected).toBe(false);
    await settle();
    expect(c.announcer.text).toBe('1 sources confirmed. These facts can now be shared with your team.');
  });

  it('Edge case — reading the section again keeps only the ticks it still lists; one source draws no tick box', async () => {
    const { c } = await controller();
    c.toggleAll();
    citeIds = [IDS[0] as string];
    await c.loadToCheck();
    expect([...c.ticked]).toEqual([IDS[0]]);
  });

  it('Keyboard — Select all and Confirm selected come first in Tab order, each tick box named by its fact', async () => {
    vi.restoreAllMocks();
    const { app } = await appWith((tr) => {
      tr.on('agents.list', ok([]));
      tr.on('memory.list', ok({ items: [], next_cursor: null }));
      tr.on('memory.held_writes', ok([]));
      tr.on('memory.citations_to_confirm', ok(IDS));
      tr.on('memory.get', (input) => ok({ facts: (input as { ids: string[] }).ids.map((id) => fact(IDS.indexOf(id) + 1)) }));
    });
    const target = document.createElement('div');
    document.body.appendChild(target);
    const part = mount(Memory, { target, props: { backend: app.local, workspaceId: WS, instance: null }, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    try {
      for (let i = 0; i < 6; i++) { await settle(); flushSync(); }
      const tab = Array.from(target.querySelectorAll<HTMLElement>('[role="tab"]')).find((b) => (b.textContent ?? '').includes('To check'));
      tab?.click();
      for (let i = 0; i < 6; i++) { await settle(); flushSync(); }
      const section = target.querySelector('section[aria-labelledby="memory-citations-title"]') as HTMLElement;
      const controls = Array.from(section.querySelectorAll<HTMLElement>('button, input')).map((e) => (e.getAttribute('aria-label') ?? (e.textContent ?? '')).replace(/[\u2068\u2069]/g, '').trim());
      expect(controls.slice(0, 2)).toEqual(['Select all', 'Confirm selected (0)']);
      const boxes = section.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
      expect(boxes).toHaveLength(3);
      expect(boxes[0]?.getAttribute('aria-label')?.replace(/[\u2068\u2069]/g, '')).toBe('Select “Fact 1.”');
      expect(controls.indexOf('Confirm source')).toBeGreaterThan(controls.findIndex((n) => n.startsWith('Select “')));
      boxes[0]?.click();
      flushSync();
      expect((section.querySelectorAll('button')[1]?.textContent ?? '').replace(/[\u2068\u2069]/g, '').trim()).toBe('Confirm selected (1)');
    } finally {
      void unmount(part);
      target.remove();
    }
  });
});
