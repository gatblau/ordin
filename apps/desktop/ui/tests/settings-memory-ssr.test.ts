// Settings › Memory (ScreensModule.md SM-NS-1.9, amended January 2026; user decision; Approved by the user
// January 2026): settings only, in three groups. What Ordin keeps and How Ordin uses memory go through
// settings.get / settings.change of `what_ordin_remembers` (enforced by the backend); How memory is shown is
// this computer's view preference, which the memory page reads.
import { beforeAll, describe, expect, it } from 'vitest';

import * as wiring from '../src/lib/backend/wiring';
import { type StorageLike } from '../src/lib/store/view-prefs';
import Memory from '../src/lib/views/fixed/Memory.svelte';
import MemorySettings from '../src/lib/views/Settings/MemorySettings.svelte';
import { MemorySettingsModel } from '../src/lib/views/Settings/memory-settings-model.svelte';
import Settings from '../src/lib/views/Settings/Settings.svelte';
import { visibleSections } from '../src/lib/views/Settings/sections';
import type { SettingView } from '../src/lib/service-api/types.gen';
import { loaded, text } from './settings-support';
import { err, hello, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';

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

describe('Settings › Memory — the section', () => {
  it('is listed when the backend answers settings.get and settings.change, not memory.list', () => {
    expect(visibleSections({ groups: new Set(['settings']), kind: 'local' }).map((s) => s.id)).toContain('what-ordin-remembers');
    expect(visibleSections({ groups: new Set(['memory']), kind: 'local' }).map((s) => s.id)).not.toContain('what-ordin-remembers');
  });

  it('Happy path — draws the three groups with a label and a one-line explanation each, and no facts', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('settings.get', answer()));
    const model = new MemorySettingsModel(t.handle);
    await model.load();
    expect(t.transport.sentOf('settings.get')[0]?.input).toEqual({ section: 'what_ordin_remembers' });
    const out = text(clean(html(MemorySettings, { backend: t.state, model })));
    for (const words of [
      'What Ordin keeps',
      'Keep it on its own',
      'Ask me first',
      'Learn from',
      'Conversations',
      'Documents',
      'Suggest removing facts not used for',
      'Nothing is removed unless you choose Forget.',
      'How memory is shown',
      'Group facts by',
      'Facts shown before Show more',
      "Show each fact's details straight away",
      'How Ordin uses memory',
      'Use facts in other workspaces',
      'Say which facts an answer used',
    ]) {
      expect(out, words).toContain(words);
    }
    expect(out).not.toMatch(/\bIT\b/);
    expect(out).not.toContain('Open a workspace');
    expect(clean(html(MemorySettings, { backend: t.state, model }))).not.toContain('fact-list');
  });

  it('Settings › Memory draws the settings, not the memory view of the workspace', async () => {
    const { app } = await appWith((tr) => {
      tr.on('service.hello', ok(hello(LOCAL, R1_GROUPS)));
      tr.on('settings.get', answer());
    });
    const out = clean(html(Settings, { backend: app.local, workspaceId: null, instance: null, section: 'what-ordin-remembers' }, app));
    expect(out).toContain('settings-memory-heading');
    expect(out).not.toContain('id="memory-heading"');
    expect(out).not.toContain('role="tablist"');
  });

  it('Edge — before the backend answers, only How memory is shown is drawn', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('settings.get', err('SVC_UNAVAILABLE', 'Not available just now.')));
    const model = new MemorySettingsModel(t.handle);
    await model.load();
    expect(model.loaded).toBe(false);
    const out = text(clean(html(MemorySettings, { backend: t.state, model })));
    expect(out).toContain('How memory is shown');
    expect(out).not.toContain('What Ordin keeps');
    expect(out).toContain('Not available just now.');
  });
});

describe('Settings › Memory — What Ordin keeps and How Ordin uses memory (backend)', () => {
  it('reads the defaults, today’s behaviour', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('settings.get', answer()));
    const model = new MemorySettingsModel(t.handle);
    await model.load();
    expect(model.keep).toBe('on_its_own');
    expect(model.isOn('memory.learn_from_conversations')).toBe(true);
    expect(model.isOn('memory.learn_from_documents')).toBe(true);
    expect(model.removal).toBeNull();
    expect(model.isOn('memory.use_everywhere')).toBe(true);
    expect(model.isOn('memory.say_which_facts')).toBe(false);
  });

  it('table: each setting is sent with settings.change and the answer replaces it', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('settings.get', answer()).on('settings.change', echo));
    const model = new MemorySettingsModel(t.handle);
    await model.load();
    const rows: [string, unknown][] = [
      ['memory.keep_mode', 'ask_first'],
      ['memory.learn_from_conversations', false],
      ['memory.learn_from_documents', false],
      ['memory.suggest_removal_after_days', 90],
      ['memory.suggest_removal_after_days', null],
      ['memory.use_everywhere', false],
      ['memory.say_which_facts', true],
    ];
    for (const [key, value] of rows) expect(await model.change(key, value), key).toBe(true);
    expect(t.transport.sentOf('settings.change').map((s) => s.input)).toEqual(rows.map(([key, value]) => ({ key, value })));
    expect(model.keep).toBe('ask_first');
    expect(model.removal).toBeNull();
    expect(model.isOn('memory.use_everywhere')).toBe(false);
    expect(model.isOn('memory.say_which_facts')).toBe(true);
  });

  it('Error path — a refused value returns the control to its stored value and shows the sentence', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('settings.get', answer()).on('settings.change', err('DSK_PREFERENCE_VALUE_INVALID', 'That value cannot be used.')));
    const model = new MemorySettingsModel(t.handle);
    await model.load();
    expect(await model.change('memory.suggest_removal_after_days', 0)).toBe(false);
    expect(model.removal).toBeNull();
    expect(model.error?.code).toBe('DSK_PREFERENCE_VALUE_INVALID');
    expect(text(clean(html(MemorySettings, { backend: t.state, model })))).toContain('That value cannot be used.');
  });

  it('Error path — a failed save offers Try again, which sends the same change', async () => {
    let fail = true;
    const t = await loaded(R1_GROUPS, (tr) =>
      tr.on('settings.get', answer()).on('settings.change', (i) => (fail ? err('DSK_PREFERENCES_WRITE_FAILED', 'Could not save.') : echo(i))),
    );
    const model = new MemorySettingsModel(t.handle);
    await model.load();
    expect(await model.change('memory.keep_mode', 'ask_first')).toBe(false);
    expect(model.keep).toBe('on_its_own');
    expect(model.error?.action).toBe('try_again');
    fail = false;
    model.retry();
    await settle();
    expect(model.keep).toBe('ask_first');
    expect(t.transport.sentOf('settings.change')).toHaveLength(2);
  });

  it('Edge — a setting the company fixed is not sent', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('settings.get', answer([view('memory.use_everywhere', true, { set_by: 'company', person_may_change: false })])));
    const model = new MemorySettingsModel(t.handle);
    await model.load();
    expect(await model.change('memory.use_everywhere', false)).toBe(false);
    expect(t.transport.sentOf('settings.change')).toEqual([]);
    expect(model.has('memory.keep_mode')).toBe(false);
  });
});

