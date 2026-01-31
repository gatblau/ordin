// @vitest-environment happy-dom
// Settings › Memory (ScreensModule.md SM-NS-1.9, amended January 2026; user decision; Approved by the user
// January 2026): settings only, in three groups. What Ordin keeps and How Ordin uses memory go through
// settings.get / settings.change of `what_ordin_remembers` (enforced by the backend); How memory is shown is
// this computer's view preference, which the memory page reads.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import * as wiring from '../src/lib/backend/wiring';
import { stripIsolates } from '../src/lib/i18n/fluent';
import { APP_KEY } from '../src/lib/shell/context';
import { KEYS, MEMORY_SHOWN_DEFAULT, ViewPrefs, type StorageLike } from '../src/lib/store/view-prefs';
import { isGroupBy } from '../src/lib/views/fixed/facts';
import Memory from '../src/lib/views/fixed/Memory.svelte';
import { MemoryController } from '../src/lib/views/fixed/memory-controller.svelte';
import { MemorySettingsModel } from '../src/lib/views/Settings/memory-settings-model.svelte';
import Settings from '../src/lib/views/Settings/Settings.svelte';
import type { FactView, SettingView } from '../src/lib/service-api/types.gen';
import { backendWith, WS } from './fixed-support';
import { text } from './settings-support';
import { err, ok, settle } from './support/fake-backend';
import { thingId, uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);
const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');

const view = (key: string, value: unknown, over: Partial<SettingView> = {}): SettingView => ({ key, value, set_by: 'person', person_may_change: true, ...over });
const DEFAULTS: SettingView[] = [
  view('memory.keep_mode', 'on_its_own'),
  view('memory.learn_from_conversations', true),
  view('memory.learn_from_documents', true),
  view('memory.suggest_removal_after_days', null),
  view('memory.use_everywhere', true),
  view('memory.say_which_facts', false),
];
const answer = (settings: SettingView[] = DEFAULTS) => ok({ section: 'what_ordin_remembers', settings });
const echo = (i: unknown) => {
  const { key, value } = i as { key: string; value: unknown };
  return ok(view(key, value));
};

function store(): StorageLike & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
}

describe('Settings › Memory — How memory is shown (this computer)', () => {
  it('defaults to by thing, 50 and closed, and saves each choice', () => {
    const s = store();
    const prefs = new ViewPrefs(s);
    expect(prefs.memoryShown()).toEqual(MEMORY_SHOWN_DEFAULT);
    expect(MEMORY_SHOWN_DEFAULT).toEqual({ groupBy: 'thing', pageSize: 50, detailsOpen: false });
    const model = new MemorySettingsModel(null as never, prefs);
    model.setGroupBy('topic');
    model.setPageSize(25);
    model.setDetailsOpen(true);
    expect(JSON.parse(s.data.get(KEYS.memoryShown) ?? '{}')).toEqual({ groupBy: 'topic', pageSize: 25, detailsOpen: true });
    expect(new ViewPrefs(s).memoryShown(isGroupBy)).toEqual({ groupBy: 'topic', pageSize: 25, detailsOpen: true });
  });

  it('Error path — a choice outside the list is ignored; a damaged value falls back field by field', () => {
    const s = store();
    const prefs = new ViewPrefs(s);
    const model = new MemorySettingsModel(null as never, prefs);
    model.setGroupBy('colour');
    model.setPageSize(7);
    expect(s.data.has(KEYS.memoryShown)).toBe(false);
    s.data.set(KEYS.memoryShown, JSON.stringify({ groupBy: 'colour', pageSize: 7, detailsOpen: true }));
    expect(new ViewPrefs(s).memoryShown(isGroupBy)).toEqual({ groupBy: 'thing', pageSize: 50, detailsOpen: true });
    s.data.set(KEYS.memoryShown, '{not json');
    expect(new ViewPrefs(s).memoryShown(isGroupBy)).toEqual(MEMORY_SHOWN_DEFAULT);
  });

  it('the memory page groups by the default only where a workspace has no choice of its own', () => {
    const prefs = new ViewPrefs(store());
    prefs.setMemoryShown({ groupBy: 'kind', pageSize: 10, detailsOpen: false });
    expect(new MemoryController(null as never, WS, prefs).groupBy).toBe('kind');
    prefs.setMemoryGroupBy(WS, 'source');
    expect(new MemoryController(null as never, WS, prefs).groupBy).toBe('source');
    expect(new MemoryController(null as never, uid('w2'), prefs).groupBy).toBe('kind');
  });

  it('the memory page shows the chosen number before Show more, and Show more adds as many', async () => {
    vi.spyOn(wiring, 'heldWrites').mockResolvedValue({ kind: 'not_wired' });
    vi.spyOn(wiring, 'citationsToConfirm').mockResolvedValue({ kind: 'not_wired' });
    const subject = { id: thingId('strategy'), name: 'Strategy' };
    const many = Array.from({ length: 30 }, (_, i) =>
      ({
        id: uid(`f${i}`), statement: `S ${i}`, kind: 'known', true_from: '2026-01-10T00:00:00Z', written_at: '2026-01-10T00:00:00Z', came_from: { kind: 'person' },
        fading: false, may_be_out_of_date: false, needs_check: false, workspace_id: WS, triple: { subject, predicate: { key: `w${i}`, label: `w ${i}` }, object: { kind: 'text', value: `${i}` } },
      }) as FactView,
    );
    const b = await backendWith({ setup: (tr) => tr.on('agents.list', ok([])).on('memory.list', ok({ items: many, next_cursor: null })).on('memory.thing_topics', ok({ things: [], topics: [] })) });
    const prefs = new ViewPrefs(store());
    prefs.setMemoryShown({ groupBy: 'thing', pageSize: 10, detailsOpen: true });
    const c = new MemoryController(b.handle, WS, prefs);
    expect(c.pageSize).toBe(10);
    expect(c.detailsOpen).toBe(true);
    await c.loadFacts();
    let group = c.grouping.groups[0]!;
    expect(group.shown.length).toBe(10);
    c.showMore(group.key);
    group = c.grouping.groups[0]!;
    expect(group.shown.length).toBe(20);
    // Without the setting: today's 50 and closed details.
    const plain = new MemoryController(b.handle, WS, new ViewPrefs(store()));
    expect(plain.pageSize).toBe(50);
    expect(plain.detailsOpen).toBe(false);
  });
});

describe('Settings › Memory — the memory page opens details when asked', () => {
  let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
  afterEach(() => {
    if (drawn !== null) void unmount(drawn.part);
    drawn?.target.remove();
    drawn = null;
    document.body.innerHTML = '';
  });

  async function draw(detailsOpen: boolean) {
    const subject = { id: thingId('kestrel'), name: 'Kestrel' };
    const fact = {
      id: uid('one'), statement: 'Kestrel pays in thirty days', kind: 'known', true_from: '2026-01-10T00:00:00Z', written_at: '2026-01-10T00:00:00Z', came_from: { kind: 'person' },
      fading: false, may_be_out_of_date: false, needs_check: false, workspace_id: WS, triple: { subject, predicate: { key: 'terms', label: 'payment terms' }, object: { kind: 'text', value: 'thirty days' } },
    };
    const { app } = await appWith((tr) => {
      tr.on('agents.list', ok([]));
      tr.on('memory.list', ok({ items: [fact], next_cursor: null }));
      tr.on('memory.held_writes', err('SVC_COMMAND_UNKNOWN', 'memory.held_writes'));
      tr.on('memory.citations_to_confirm', err('SVC_COMMAND_UNKNOWN', 'memory.citations_to_confirm'));
      tr.on('memory.things', ok({ items: [], next_cursor: null }));
      tr.on('memory.thing_topics', ok({ things: [], topics: [] }));
    });
    app.prefs.setMemoryShown({ ...MEMORY_SHOWN_DEFAULT, detailsOpen });
    const target = document.createElement('div');
    document.body.appendChild(target);
    const part = mount(Memory, { target, props: { backend: app.local, workspaceId: WS, instance: null }, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    drawn = { target, part };
    for (let i = 0; i < 6; i++) {
      await settle();
      flushSync();
    }
    return target;
  }

  it('closed by default', async () => {
    const target = await draw(false);
    expect(target.querySelector(`#fact-details-${uid('one')}`)).toBeNull();
  });

  it('open when the setting is on', async () => {
    const target = await draw(true);
    expect(target.querySelector(`#fact-details-${uid('one')}`)).not.toBeNull();
    expect(stripIsolates(target.textContent ?? '')).toContain('Kestrel');
  });
});
