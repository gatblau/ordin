// Icons on Home, *Search or go to…*, the Needs you tab and the earlier conversations (ScreensModule.md
// amendment *Icons*, SM-IC-3; cross-cutting/VisualDesign.md V8 *Where icons are drawn*, *Actions*).
// Rendering is server-side with no DOM.
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { clearActionHandlers, registerActionHandler, SEARCH_ACTION_NAMES } from '../src/lib/search/actions';
import ConversationPicker from '../src/lib/shell/ConversationPicker.svelte';
import { ConversationPickerModel } from '../src/lib/shell/conversation-picker.svelte';
import NeedsYouTab from '../src/lib/shell/NeedsYouTab.svelte';
import EarlierConversation from '../src/lib/views/EarlierConversation.svelte';
import EarlierConversations from '../src/lib/views/EarlierConversations.svelte';
import { EarlierListModel, EarlierReadModel } from '../src/lib/views/earlier.svelte';
import Home from '../src/lib/views/Home.svelte';
import { HomeModel } from '../src/lib/views/home-model.svelte';
import { SearchModel } from '../src/lib/views/search.svelte';
import SearchDialog from '../src/lib/views/SearchDialog.svelte';
import type { ConversationSummary, FoundResult, GoToResult, HomeRow, OpenTarget, SearchAnswer } from '../src/lib/service-api/types.gen';
import { approvalCard, event, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { loaded } from './settings-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
afterEach(clearActionHandlers);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));
const plain = (markup: string) => markup.replace(/<!--[\s\S]*?-->/g, '').replace(/[⁨⁩]/g, '');

/**
 * The icon drawn last before the first occurrence of `words`, provided no `stop` lies between them (the
 * icon belongs to the same row or button); `null` otherwise.
 */
function iconBefore(markup: string, words: string, stop = '</li>'): string | null {
  const at = markup.indexOf(words);
  if (at < 0) throw new Error(`not drawn: ${words}`);
  const before = markup.slice(0, at);
  const uses = [...before.matchAll(/<use href="#i-([a-z]+)"/g)];
  const last = uses[uses.length - 1];
  if (last === undefined) return null;
  return before.slice(last.index!).includes(stop) ? null : last[1]!;
}
/** The tile around the icon drawn last before `words`. */
function tileBefore(markup: string, words: string): string {
  const before = markup.slice(0, markup.indexOf(words));
  return before.slice(before.lastIndexOf('<span class="icon-tile')).match(/^<span class="icon-tile[^"]*"[^>]*>/)?.[0] ?? '';
}

describe('SM-IC-3 — Home', () => {
  const row = (line: string, opens: OpenTarget, over: Partial<HomeRow> = {}): HomeRow => ({
    at: '2026-01-10T10:00:00Z', is_new: false, opens, run_again: false, workspace_id: WS, workspace_name: 'Q3 supplier review', line, ...over,
  });
  async function draw(view: { waiting?: HomeRow[]; running?: HomeRow[]; since_last_looked?: HomeRow[] }): Promise<string> {
    const reply = { new_count: 0, waiting: [], running: [], since_last_looked: [], ...view };
    const s = await loaded(R1_GROUPS, (tr) => tr.on('home.view', ok(reply)));
    const model = new HomeModel(s.handle, { openRow: () => undefined, openHandover: () => undefined });
    await model.load();
    return plain(html(Home, { backend: s.state as never, workspaceId: null, instance: null, model }));
  }

  it.each([
    ['a finished run', { kind: 'run', run_id: 'r1', workspace_id: WS }, 'task'],
    ['a crew run', { kind: 'crew_run', crew_run_id: 'c1', workspace_id: WS }, 'crew'],
    ['a changed document', { kind: 'document', path: 'Contracts/Kestrel.docx', place_id: 'p', workspace_id: WS }, 'docs'],
    ['an out-of-date knowledge page', { kind: 'knowledge_page', page_id: 'k1', workspace_id: WS }, 'know'],
    ['anything else (a handover)', { kind: 'handover', handover_id: 'h1', workspace_id: WS }, 'task'],
    ['anything else (a workspace)', { kind: 'workspace', workspace_id: WS }, 'task'],
  ] as Array<[string, OpenTarget, string]>)('a Since you last looked row for %s draws its feed icon', async (line, opens, icon) => {
    const out = await draw({ since_last_looked: [row(line, opens)] });
    expect(iconBefore(out, line)).toBe(icon);
    // Untinted, as the prototype's feed draws it.
    expect(tileBefore(out, line)).toMatch(/^<span class="icon-tile[^"]*">$/);
  });

  it('a Waiting for you row draws i-info before its words', async () => {
    const out = await draw({ waiting: [row('Publish the revised account brief?', { kind: 'approval', approval_id: 'a1', workspace_id: WS })] });
    expect(iconBefore(out, 'Publish the revised account brief?')).toBe('info');
  });

  it('a Running now row draws i-run in the running tint, a crew run i-crew in the crew tint; Show progress draws i-eye', async () => {
    const out = await draw({
      running: [
        row('Comparing the Q3 rates', { kind: 'run', run_id: 'r1', workspace_id: WS }),
        row('Supplier crew', { kind: 'crew_run', crew_run_id: 'c1', workspace_id: WS }),
      ],
    });
    expect(iconBefore(out, 'Comparing the Q3 rates')).toBe('run');
    expect(tileBefore(out, 'Comparing the Q3 rates')).toContain('data-tint="running"');
    expect(iconBefore(out, 'Supplier crew')).toBe('crew');
    expect(tileBefore(out, 'Supplier crew')).toContain('data-tint="crew"');
    expect(out).toMatch(/<button[^>]*><svg class="icon icon-default"[^>]*><use href="#i-eye"><\/use><\/svg>Show progress<\/button>/);
  });
});

describe('SM-IC-3 — Search or go to…', () => {
  const go = (kind: GoToResult['kind'], label: string, opens: OpenTarget = { kind: 'workspace', workspace_id: WS }): GoToResult => ({ kind, label, opens, workspace_id: WS });
  const found = (label: string, opens: OpenTarget): FoundResult => ({ label, opens, workspace_id: WS });
  async function draw(answer: Partial<SearchAnswer>): Promise<string> {
    const reply: SearchAnswer = { actions: [], complete: true, found: [], go_to: [], query: 'x', ...answer };
    const s = await loaded(R1_GROUPS, (tr) => tr.on('search.query', ok(reply)));
    const model = new SearchModel({ handle: s.handle, groups: s.state, inFront: () => WS, diagnostics: s.diagnostics });
    model.query = 'x';
    await model.run();
    const { app } = await appWith();
    app.shell.searchOpen = true;
    return plain(html(SearchDialog, { model }, app));
  }
  const doc = (path: string): OpenTarget => ({ kind: 'document', path, place_id: 'p', workspace_id: WS });

  it.each([
    ['workspace', 'Supplier review', 'ws'],
    ['agent', 'Rates checker', 'agent'],
    ['workflow', 'Monthly close', 'keptcrew'],
    ['dashboard', 'Spend by supplier', 'chart'],
    ['knowledge_page', 'Supplier rates Q3', 'know'],
    ['conversation', 'Compare the rates', 'history'],
    ['document', 'Kestrel quote.pdf', 'pdf'],
    ['document', 'Rates.xlsx', 'sheet'],
    ['document', 'Notes without a kind', 'word'],
  ] as Array<[GoToResult['kind'], string, string]>)('a Go to result of kind %s (%s) draws i-%s before its words', async (kind, label, icon) => {
    const out = await draw({ go_to: [go(kind, label, kind === 'document' ? doc(label) : undefined)] });
    expect(iconBefore(out, label, 'role="option"')).toBe(icon);
  });

  it.each([
    ['Supplier rates Q3', { kind: 'knowledge_page', page_id: 'k1', workspace_id: WS }, 'know'],
    ['Kestrel.pdf', doc('Quotes/Kestrel.pdf'), 'pdf'],
    ['Rates memo', doc('Quotes/Rates memo'), 'word'],
    ['A run', { kind: 'run', run_id: 'r1', workspace_id: WS }, null],
  ] as Array<[string, OpenTarget, string | null]>)('a Found result %s draws %s', async (label, opens, icon) => {
    const out = await draw({ found: [found(label, opens)] });
    expect(iconBefore(out, label, 'role="option"')).toBe(icon);
  });

  it('table-driven over the seven actions: each draws its V8 icon before its words', async () => {
    for (const name of SEARCH_ACTION_NAMES) registerActionHandler(name, () => undefined);
    const expected: Record<string, [string, string]> = {
      home: ['Home', 'home'],
      new_workspace: ['New workspace', 'plus'],
      settings: ['Settings', 'key'],
      add_documents: ['Add documents', 'plus'],
      summarise: ['Summarise', 'fold'],
      clear: ['Clear', 'chat'],
      earlier_conversations: ['Earlier conversations', 'history'],
    };
    for (const name of SEARCH_ACTION_NAMES) {
      const out = await draw({ actions: [{ kind: 'workspace', label: 'raw', opens: { kind: 'screen', name } }] });
      const [words, icon] = expected[name]!;
      expect(iconBefore(out, `<span class="label`, 'role="option"'), name).toBe(icon);
      expect(out, name).toContain(words);
    }
  });
});

describe('SM-IC-3 — the Needs you tab', () => {
  it('an approval and a waiting row each draw i-info before their words', async () => {
    const { app, transport } = await appWith((tr) => tr.on('workspaces.list', ok([{ id: WS, name: 'Q4 forecast', created_at: 'x', own_folder_place_id: 'p', state: 'active' }])));
    transport.emit(event(LOCAL, 1, { kind: 'approval_requested', card: approvalCard() }));
    await settle();
    app.local.needsYouRows = [
      { at: '2026-01-10T10:00:00Z', is_new: false, line: 'Choose a supplier', opens: { kind: 'block', block_id: 'b1', workspace_id: WS }, run_again: false, workspace_id: WS },
    ];
    const out = plain(html(NeedsYouTab, { backends: app.stores.all() }, app));
    expect(iconBefore(out, 'Publish the revised account brief?')).toBe('info');
    expect(iconBefore(out, 'Choose a supplier')).toBe('info');
  });
});

describe('SM-IC-3 — earlier conversations', () => {
  const conv = (id: string, over: Partial<ConversationSummary> = {}): ConversationSummary => ({
    conversation_id: id, first_request: `First request of ${id}`, message_count: 4, started_at: '2026-01-10T10:00:00Z', state: 'earlier', ...over,
  });

  it('the Earlier conversations list draws i-chat before each entry', async () => {
    const s = await loaded(R1_GROUPS, (tr) => tr.on('ordin.list_conversations', ok({ items: [conv('old', { first_request: 'Q3 supplier rates' })], next_cursor: null })));
    const model = new EarlierListModel(s.handle, WS);
    await model.load();
    const out = plain(html(EarlierConversations, { backend: s.state, workspaceId: WS, instance: null, model }));
    expect(iconBefore(out, 'Q3 supplier rates')).toBe('chat');
  });

  it('a read-only conversation’s Ask … about it draws i-chat; Continue draws none', async () => {
    const s = await loaded(R1_GROUPS, (tr) => {
      tr.on('ordin.list_conversations', ok({ items: [conv(uid('b'))], next_cursor: null }));
      tr.on('ordin.read_conversation', ok({ items: [], next_cursor: null }));
    });
    const model = new EarlierReadModel(s.handle, WS, uid('b'));
    await model.load();
    const out = plain(html(EarlierConversation, { backend: s.state, workspaceId: WS, instance: uid('b'), model }));
    const ask = t('conv-ask-about');
    expect(iconBefore(out, `${ask}</button>`, '</button>')).toBe('chat');
    expect(iconBefore(out, `${t('conv-continue')}</button>`, '</button>')).toBeNull();
  });

  it('the conversation picker: an earlier conversation draws i-chat, All earlier conversations i-history, the live one and Clear none', async () => {
    const items = [conv('live', { state: 'current', first_request: 'Live talk' }), ...Array.from({ length: 25 }, (_, i) => conv(`c${i}`, { first_request: `Earlier ${i}`, started_at: `2026-01-${String(i + 1).padStart(2, '0')}T10:00:00Z` }))];
    const s = await loaded(R1_GROUPS, (tr) => tr.on('ordin.list_conversations', ok({ items, next_cursor: null })));
    const picker = new ConversationPickerModel(new EarlierListModel(s.handle, WS), t);
    await picker.show('first');
    const out = plain(html(ConversationPicker, { model: picker, onopen: () => undefined, onall: () => undefined, onclear: () => undefined }));
    const menu = out.slice(out.indexOf('role="menu"'));
    expect(iconBefore(menu, '>Earlier 24<')).toBe('chat');
    expect(iconBefore(menu, '>All earlier conversations (25)<')).toBe('history');
    expect(iconBefore(menu, '>Live talk<')).toBeNull();
    expect(iconBefore(menu, `>${t('conv-clear')}<`)).toBeNull();
  });
});
