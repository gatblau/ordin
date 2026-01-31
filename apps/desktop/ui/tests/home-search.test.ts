// Search or go to… (11 §5.17) and its Actions group (SM-SF-9, SM-UP-1).
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { ACTION_LABEL_IDS, actionScreen, clearActionHandlers, registerActionHandler, SEARCH_ACTION_NAMES } from '../src/lib/search/actions';
import { SEARCH_DEBOUNCE_MS, SearchModel } from '../src/lib/views/search.svelte';
import SearchDialog from '../src/lib/views/SearchDialog.svelte';
import { hello, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { loaded, text } from './settings-support';
import type { GoToResult, SearchAnswer } from '../src/lib/service-api/types.gen';

beforeAll(loadWindowCatalogue);
afterEach(clearActionHandlers);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const OTHER = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c99';
const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const screenEntry = (name: string, label = 'Raw label from the core'): GoToResult => ({ kind: 'workspace', label, opens: { kind: 'screen', name } });
const answer = (over: Partial<SearchAnswer> = {}): SearchAnswer => ({ actions: [], complete: true, found: [], go_to: [], query: 'x', ...over });

async function search(reply: SearchAnswer, options: { inFront?: string | null; groups?: string[] } = {}) {
  const t = await loaded(options.groups ?? R1_GROUPS, (tr) => tr.on('search.query', ok(reply)));
  const front = options.inFront === undefined ? WS : options.inFront;
  const model = new SearchModel({ handle: t.handle, groups: t.state, inFront: () => front, diagnostics: t.diagnostics });
  model.query = 'x';
  await model.run();
  return { ...t, model };
}
const registerAll = () => {
  for (const name of SEARCH_ACTION_NAMES) registerActionHandler(name, () => undefined);
};

describe('The Actions group (SM-SF-9)', () => {
  it('table-driven over the seven names: the label comes from the catalogue, the entry’s own label is ignored', async () => {
    registerAll();
    const labels: Record<string, string> = {
      home: 'Home', new_workspace: 'New workspace', add_documents: 'Add documents', summarise: 'Summarise', clear: 'Clear', earlier_conversations: 'Earlier conversations', settings: 'Settings',
    };
    for (const name of SEARCH_ACTION_NAMES) {
      const t = await search(answer({ actions: [screenEntry(name)] }));
      const entries = t.model.actions;
      expect(entries, name).toHaveLength(1);
      expect(entries[0]?.labelId).toBe(ACTION_LABEL_IDS[name]);
      expect(entries[0]?.label).toBeNull();
      // Drawn through the catalogue.
      const app = (await appWith()).app;
      app.shell.searchOpen = true;
      const out = text(clean(html(SearchDialog, { model: t.model }, app)));
      expect(out, name).toContain(labels[name]);
      expect(out, name).not.toContain('Raw label from the core');
    }
  });

  it('Error path — an action the screens do not know', async () => {
    const t = await search(answer({ actions: [screenEntry('reports'), screenEntry('home')] }));
    expect(t.model.actions.map((e) => e.action)).toEqual(['home']);
    expect(t.diagnostics.codes()).toContain('UI_SEARCH_ACTION_UNKNOWN');
  });

  it('UI_SEARCH_ACTION_UNKNOWN — an opens kind other than screen is not drawn and records the code', async () => {
    const t = await search(answer({ actions: [{ kind: 'workspace', label: 'x', opens: { kind: 'workspace', workspace_id: WS } }] }));
    expect(t.model.actions).toEqual([]);
    expect(t.diagnostics.codes()).toEqual(['UI_SEARCH_ACTION_UNKNOWN']);
    expect(actionScreen(screenEntry('settings'))).toBe('settings');
    expect(actionScreen({ kind: 'workspace', label: 'x', opens: { kind: 'workspace', workspace_id: WS } })).toBeNull();
  });

  it('the four workspace actions are absent with no workspace in front', async () => {
    registerAll();
    const entries = SEARCH_ACTION_NAMES.map((n) => screenEntry(n));
    const none = await search(answer({ actions: entries }), { inFront: null });
    expect(none.model.actions.map((e) => e.action)).toEqual(['home', 'new_workspace', 'settings']);
    const some = await search(answer({ actions: entries }));
    expect(some.model.actions).toHaveLength(7);
  });

  it('every entry passes SM-UP-1: an action whose command group the backend does not answer is not drawn', async () => {
    registerAll();
    const entries = SEARCH_ACTION_NAMES.map((n) => screenEntry(n));
    const t = await search(answer({ actions: entries }), { groups: R1_GROUPS.filter((g) => g !== 'ordin') });
    expect(t.model.actions.map((e) => e.action)).toEqual(['home', 'new_workspace', 'add_documents', 'settings']);
  });

  it('an action that needs another module’s surface is drawn only once that module has registered its handler', async () => {
    const entries = ['new_workspace', 'summarise', 'clear', 'add_documents', 'earlier_conversations'].map((n) => screenEntry(n));
    const before = await search(answer({ actions: entries }));
    expect(before.model.actions.map((e) => e.action)).toEqual(['earlier_conversations']);
    registerActionHandler('summarise', () => undefined);
    const after = await search(answer({ actions: entries }));
    expect(after.model.actions.map((e) => e.action)).toEqual(['summarise', 'earlier_conversations']);
  });

  it('Behavioural check: given a service.hello without mail or slack, no mail or Slack entry is drawn in Search', async () => {
    registerAll();
    const t = await search(answer({ actions: SEARCH_ACTION_NAMES.map((n) => screenEntry(n)) }), { groups: R1_GROUPS });
    const labels = t.model.entries.map((e) => e.labelId ?? e.label ?? '');
    expect(labels.join(' ')).not.toMatch(/mail|slack|telegram/i);
  });
});

describe('Search or go to…', () => {
  const full = answer({
    go_to: [
      { kind: 'workspace', label: 'Q4 forecast', workspace_id: OTHER, opens: { kind: 'workspace', workspace_id: OTHER } },
      { kind: 'agent', label: 'Contract clause checker', workspace_id: OTHER, opens: { kind: 'item', id: 'a1', item_kind: 'agent', workspace_id: OTHER } },
    ],
    found: [{ label: 'Kestrel quote', snippet: 'Quote from Kestrel', workspace_id: WS, opens: { kind: 'document', path: 'Q3/Kestrel.md', place_id: 'p1', workspace_id: WS } }],
  });

  it('Edge case — search across workspaces', async () => {
    const t = await search(full);
    const app = (await appWith()).app;
    app.shell.searchOpen = true;
    const markup = clean(html(SearchDialog, { model: t.model }, app));
    const out = text(markup);
    expect(out).toContain('Go to');
    expect(out).toContain('Found in pages and documents');
    expect(out).toContain('Kestrel quote');
    // The number of results is announced (SM-CU-6).
    // Fluent wraps the number in direction isolates (U+2068, U+2069); the words read "3 results".
    expect(markup.replace(/[\u2068\u2069]/g, '')).toMatch(/role="status" aria-live="polite">3 results</);
    // A result from another workspace names it, and an agent there says how to use it here.
    expect(t.model.goTo[0]?.elsewhere).toBe(true);
    expect(t.model.found[0]?.elsewhere).toBe(false);
    expect(out).toContain('To use this here, add it to this workspace.');
  });

  it('is a combobox with a listbox, with the footer line', async () => {
    const t = await search(full);
    const app = (await appWith()).app;
    app.shell.searchOpen = true;
    const markup = clean(html(SearchDialog, { model: t.model }, app));
    expect(markup).toMatch(/role="combobox"/);
    expect(markup).toMatch(/aria-expanded="true"/);
    expect(markup).toMatch(/role="listbox"/);
    expect(markup).toMatch(/role="option"/);
    expect(text(markup)).toContain('↑↓ to move · Enter to open · Esc to close');
  });

  it('is drawn only while open', async () => {
    const t = await search(full);
    const app = (await appWith()).app;
    expect(html(SearchDialog, { model: t.model }, app)).not.toContain('role="dialog"');
  });

  it('queries are debounced 120 ms and the last one wins', async () => {
    expect(SEARCH_DEBOUNCE_MS).toBe(120);
    const t = await loaded(R1_GROUPS, (tr) =>
      tr.on('search.query', (input) => {
        const q = (input as { query: string }).query;
        return ok(answer({ query: q, go_to: [{ kind: 'workspace', label: q, opens: { kind: 'workspace', workspace_id: WS } }] }));
      }),
    );
    const model = new SearchModel({ handle: t.handle, groups: t.state, inFront: () => WS });
    model.query = 'Kes';
    const first = model.run();
    model.query = 'Kestrel';
    const second = model.run();
    await Promise.all([first, second]);
    expect(model.answer?.query).toBe('Kestrel');
    model.query = '   ';
    await model.run();
    expect(model.answer).toBeNull();
    void settle;
  });

  it('sends the workspace in front as current_workspace_id, and the arrow keys wrap', async () => {
    const t = await search(full);
    expect(t.transport.sentOf('search.query')[0]?.input).toEqual({ query: 'x', current_workspace_id: WS });
    expect(t.model.entries).toHaveLength(3);
    t.model.move(-1);
    expect(t.model.active).toBe(2);
    t.model.move(1);
    expect(t.model.active).toBe(0);
  });
});
describe('SM-CU-6 — the search result count', () => {
  const live = (markup: string): string => /role="status" aria-live="polite">([^<]*)</.exec(clean(markup))?.[1]?.replace(/[⁨⁩]/g, '') ?? '';
  const three = answer({
    go_to: [{ kind: 'workspace', label: 'Kestrel', workspace_id: WS, opens: { kind: 'workspace', workspace_id: WS } }],
    found: [
      { label: 'Kestrel quote', workspace_id: WS, opens: { kind: 'document', path: 'Q3/Kestrel.md', place_id: 'p1', workspace_id: WS } },
      { label: 'Kestrel rates', workspace_id: WS, opens: { kind: 'document', path: 'Q3/Rates.md', place_id: 'p1', workspace_id: WS } },
    ],
  });
  async function drawn(reply: SearchAnswer, query = 'Kestrel'): Promise<{ markup: string; model: SearchModel }> {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('search.query', ok(reply)));
    const model = new SearchModel({ handle: t.handle, groups: t.state, inFront: () => WS, diagnostics: t.diagnostics });
    model.query = query;
    await model.run();
    const app = (await appWith()).app;
    app.shell.searchOpen = true;
    return { markup: html(SearchDialog, { model }, app), model };
  }

  it('Happy path — the search box announces its results (SM-CU-6)', async () => {
    const { markup, model } = await drawn(three);
    expect(model.announcement).toEqual({ id: 'search-result-count', count: 3 });
    expect(live(markup)).toBe('3 results');
  });

  it('one result is read in the singular', async () => {
    const { markup } = await drawn(answer({ go_to: [{ kind: 'workspace', label: 'Kestrel', workspace_id: WS, opens: { kind: 'workspace', workspace_id: WS } }] }));
    expect(live(markup)).toBe('1 result');
  });

  it('Edge case — an answer with nothing drawn reads No results', async () => {
    const { markup, model } = await drawn(answer());
    expect(model.announcement).toEqual({ id: 'search-no-results' });
    expect(live(markup)).toBe('No results');
  });

  it('the count is of the entries drawn after the Actions filtering, not of the reply', async () => {
    // An action the screens do not know is not drawn (SM-SF-9), so it is not counted.
    const { markup } = await drawn(answer({ actions: [screenEntry('reports')], go_to: three.go_to }));
    expect(live(markup)).toBe('1 result');
  });

  it('nothing is announced while the box is empty', async () => {
    const { model } = await drawn(three);
    model.query = '   ';
    expect(model.announcement).toBeNull();
    await model.run();
    expect(model.answer).toBeNull();
    expect(model.announcement).toBeNull();
  });

  it('nothing is announced before the first answer', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('search.query', ok(three)));
    const model = new SearchModel({ handle: t.handle, groups: t.state, inFront: () => WS });
    model.query = 'Kestrel';
    expect(model.announcement).toBeNull();
  });
});

void hello;
void LOCAL;
