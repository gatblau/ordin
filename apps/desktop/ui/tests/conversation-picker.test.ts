// The conversation picker, the continued fold and Continue's follow-ups (ADR-108: SM-CC-1, SM-CC-4, SM-CC-6,
// SM-CC-7), the run-state words, the screens opened at once by block_shown, and the Adopt actions.
import { beforeAll, describe, expect, it } from 'vitest';

import { startApp } from '../src/lib/app/bootstrap';
import Message from '../src/lib/blocks/kinds/Message.svelte';
import type { ConversationSummary, RunState } from '../src/lib/service-api/types.gen';
import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import ConversationPicker from '../src/lib/shell/ConversationPicker.svelte';
import { buttonKey, conversationName, ConversationPickerModel, EARLIER_LIMIT, menuKey, type MenuEntry } from '../src/lib/shell/conversation-picker.svelte';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { ShellState } from '../src/lib/shell/shell-state.svelte';
import ContinuedFold from '../src/lib/views/ContinuedFold.svelte';
import { EarlierListModel, EarlierReadModel } from '../src/lib/views/earlier.svelte';
import AdoptControls from '../src/lib/views/fixed/AdoptControls.svelte';
import { TeamController } from '../src/lib/views/fixed/team.svelte';
import { event, FakeTransport, LOCAL, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));
const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');

const conv = (id: string, over: Partial<ConversationSummary> = {}): ConversationSummary => ({
  conversation_id: id, first_request: `First request of ${id}`, message_count: 4, started_at: '2026-01-10T10:00:00Z', state: 'earlier', ...over,
});
const entry = (label: string, disabled = false): MenuEntry => ({ kind: 'earlier', label, disabled, separatorBefore: false });

describe('SM-CC-1 the name on the button', () => {
  it.each([
    ['Q3 supplier rates', 'Q3 supplier rates'],
    ['x'.repeat(48), 'x'.repeat(48)],
    ['Compare the supplier rates for the third quarter against the second quarter', 'Compare the supplier rates for the third quarter…'],
    ['a'.repeat(60), `${'a'.repeat(48)}…`],
    ['Exactly forty-eight characters then a space here ok', 'Exactly forty-eight characters then a space here…'],
  ])('cuts %j at a word end', (first, expected) => {
    expect(conversationName(first, 'fallback')).toBe(expected);
  });

  it('Edge case — before the first message the button reads conv-picker-new', () => {
    expect(conversationName(null, 'New conversation')).toBe('New conversation');
    expect(conversationName(undefined, 'New conversation')).toBe('New conversation');
  });
});

describe('SM-CC-1 the keys of a menu button', () => {
  const entries = [entry('Live'), entry('Alpha'), entry('Beta'), entry('None', true), entry('Clear')];

  it('Enter, Space or Down Arrow opens on the first item and Up Arrow on the last', () => {
    expect(buttonKey('Enter', entries)).toEqual({ open: true, focus: 0 });
    expect(buttonKey(' ', entries)).toEqual({ open: true, focus: 0 });
    expect(buttonKey('ArrowDown', entries)).toEqual({ open: true, focus: 0 });
    expect(buttonKey('ArrowUp', entries)).toEqual({ open: true, focus: 4 });
    expect(buttonKey('x', entries).open).toBe(false);
  });

  it.each([
    ['ArrowDown', 2, 4], // skips the disabled item
    ['ArrowDown', 4, 0], // wraps
    ['ArrowUp', 0, 4], // wraps
    ['ArrowUp', 1, 0],
    ['Home', 2, 0],
    ['End', 1, 4],
    ['b', 0, 2], // a typed character moves to the next item that starts with it
    ['a', 2, 1],
  ])('%s from item %i focuses item %i', (key, from, to) => {
    expect(menuKey(key, from, entries)?.focus).toBe(to);
  });

  it('Escape closes and returns focus to the button; Tab closes and lets focus move on', () => {
    expect(menuKey('Escape', 1, entries)).toEqual({ focus: null, close: true, restoreButton: true });
    expect(menuKey('Tab', 1, entries)).toEqual({ focus: null, close: true, restoreButton: false });
    expect(menuKey('z', 1, entries)).toBeNull();
  });
});

describe('SM-CC-1 the menu', () => {
  async function model(items: ConversationSummary[]) {
    const tb = await backendWith({ setup: (tr) => tr.on('ordin.list_conversations', ok({ items, next_cursor: null })) });
    const list = new EarlierListModel(tb.handle, WS);
    return { ...tb, picker: new ConversationPickerModel(list, t), list };
  }

  it('Happy path — the live conversation, a separator, earlier entries newest first, a separator and Clear; opening sends list_conversations', async () => {
    const m = await model([
      conv('live', { state: 'current', first_request: 'Compare the supplier rates for the third quarter against the second' }),
      conv('old', { first_request: 'Q3 rates', started_at: '2026-01-01T10:00:00Z', message_count: 12 }),
      conv('new', { first_request: 'Account brief', started_at: '2026-01-03T10:00:00Z', message_count: 7 }),
    ]);
    await m.picker.show('first');
    expect(m.transport.sentOf('ordin.list_conversations')).toHaveLength(1);
    const rows = m.picker.entries;
    expect(rows.map((r) => r.kind)).toEqual(['live', 'earlier', 'earlier', 'clear']);
    expect(rows[0]?.label).toBe('Compare the supplier rates for the third quarter…');
    expect(rows[0]?.detail).toBe('Current');
    expect(rows.slice(1, 3).map((r) => r.conversationId)).toEqual(['new', 'old']);
    expect(rows[1]?.accessibleName).toMatch(/^Account brief, /);
    expect(rows[1]?.separatorBefore).toBe(true);
    expect(rows[3]?.separatorBefore).toBe(true);
    expect(m.picker.label).toBe(t('conv-picker-label', { name: 'Compare the supplier rates for the third quarter…' }));
  });

  it('more than 20 earlier conversations end with All earlier conversations', async () => {
    const items = Array.from({ length: 25 }, (_, i) => conv(`c${i}`, { started_at: `2026-01-${String(i + 1).padStart(2, '0')}T10:00:00Z` }));
    const m = await model(items);
    await m.picker.show('first');
    const rows = m.picker.entries;
    expect(rows.filter((r) => r.kind === 'earlier')).toHaveLength(EARLIER_LIMIT);
    expect(rows.find((r) => r.kind === 'all')?.label).toBe('All earlier conversations (25)');
  });

  it('Edge case — no earlier conversation: a disabled No earlier conversations item', async () => {
    const m = await model([conv('live', { state: 'current' })]);
    await m.picker.show('last');
    const none = m.picker.entries.find((r) => r.kind === 'none');
    expect(none).toMatchObject({ label: 'No earlier conversations', disabled: true });
    expect(m.picker.focused).toBe(m.picker.entries.length - 1);
  });

  it('draws a menu button with aria-haspopup, aria-expanded and radio items', async () => {
    const m = await model([conv('live', { state: 'current', first_request: 'Live' }), conv('old', { first_request: 'Older' })]);
    const closed = clean(html(ConversationPicker, { model: m.picker, onopen: () => undefined, onall: () => undefined, onclear: () => undefined }));
    expect(closed).toContain('aria-haspopup="menu"');
    expect(closed).toContain('aria-expanded="false"');
    expect(closed).not.toContain('role="menu"');
    await m.picker.show('first');
    const open = clean(html(ConversationPicker, { model: m.picker, onopen: () => undefined, onall: () => undefined, onclear: () => undefined }));
    expect(open).toContain('aria-expanded="true"');
    expect(open).toContain('role="menu"');
    expect(open).toContain('role="menuitemradio" aria-checked="true"');
    expect(open).toContain('aria-checked="false"');
    expect(open).toContain('role="separator"');
  });
});

describe('SM-CC-6 a continued conversation', () => {
  it('the fold reads conv-continued-line with Show and the chain count less the live conversation’s own messages', async () => {
    const live = conv(uid('live'), { state: 'current', continues: uid('a'), message_count: 9, started_at: '2026-01-05T10:00:00Z' });
    const known = [conv(uid('a'), { message_count: 6 }), live];
    const tb = await backendWith();
    const out = text(clean(html(ContinuedFold, { backend: tb.state, workspaceId: WS, live, known, model: new EarlierReadModel(tb.handle, WS, uid('a')) })));
    expect(out).toContain('earlier messages (6)');
    expect(out).toContain('Show');
    expect(tb.transport.sentOf('ordin.read_conversation')).toHaveLength(0);
  });

  it('showing reads the conversation continued and then each earlier link, oldest first', async () => {
    const tb = await backendWith({
      setup: (tr) => {
        tr.on('ordin.list_conversations', ok({ items: [conv(uid('a'), { message_count: 2 }), conv(uid('b'), { continues: uid('a'), message_count: 5 }), conv(uid('live'), { state: 'current', continues: uid('b'), message_count: 8 })], next_cursor: null }));
        tr.on('ordin.read_conversation', ok({ items: [{ seq: 1, at: '2026-01-10T10:00:00Z', author: 'person', text: 'hello' }], next_cursor: null }));
      },
    });
    const model = new EarlierReadModel(tb.handle, WS, uid('b'));
    await model.load();
    expect(model.parts.map((p) => p.conversation.conversation_id)).toEqual([uid('a'), uid('b')]);
    expect(tb.transport.sentOf('ordin.read_conversation').map((s) => (s.input as { conversation_id: string }).conversation_id)).toEqual([uid('a'), uid('b')]);
  });
});

describe('SM-CC-4 and SM-CC-7 after Continue', () => {
  it('Continue bumps the epoch the panel watches to read the new conversation and focus the message box', () => {
    const shell = new ShellState();
    const before = shell.continuedEpoch;
    shell.afterContinue();
    expect(shell.continuedEpoch).toBe(before + 1);
  });

  it('the workspace conversation reads the live conversation; carried items come first', async () => {
    const waiting = (id: string) => ok({ at: '2026-01-10T10:00:00Z', block: { kind: 'message', text: id, attachments: [] }, block_id: id, conversation_id: uid('live'), from: 'ordin', waiting: id === uid('carried') });
    const tb = await backendWith({
      setup: (tr) => {
        tr.on('ordin.list_conversations', ok({ items: [conv(uid('old')), conv(uid('live'), { state: 'current', continues: uid('old') })], next_cursor: null }));
        tr.on('ordin.read_conversation', ok({ items: [
          { seq: 5, at: '2026-01-10T10:00:00Z', author: 'ordin', text: 'later', block_id: uid('later') },
          { seq: 9, at: '2026-01-10T10:00:00Z', author: 'ordin', text: 'moved', block_id: uid('carried'), carried: true },
          { seq: 6, at: '2026-01-10T10:00:00Z', author: 'person', text: 'no block' },
        ], next_cursor: null }));
        tr.on('ordin.block', (input) => waiting((input as { block_id: string }).block_id));
      },
    });
    await tb.state.reloadConversation(WS);
    expect(tb.transport.sentOf('ordin.read_conversation')[0]?.input).toEqual({ conversation_id: uid('live') });
    const items = tb.state.conversation.get(WS) ?? [];
    expect(items.map((i) => i.block_id)).toEqual([uid('carried'), uid('later')]);
    expect(items[0]?.waiting).toBe(true);
  });

  it('a message whose block belongs to another conversation is drawn as conv-item-moved', async () => {
    const tb = await backendWith({
      setup: (tr) => {
        tr.on('ordin.list_conversations', ok({ items: [conv(uid('a'))], next_cursor: null }));
        tr.on('ordin.read_conversation', ok({ items: [{ seq: 1, at: '2026-01-10T10:00:00Z', author: 'ordin', text: 'x', block_id: uid('b1') }], next_cursor: null }));
        tr.on('ordin.block', ok({ conversation_id: uid('live') }));
      },
    });
    const model = new EarlierReadModel(tb.handle, WS, uid('a'));
    await model.load();
    expect(model.moved.has(uid('b1'))).toBe(true);
    expect(t('conv-item-moved')).toBe('Moved to the current conversation');
  });
});

describe('FU-134 run-state words (the table of run states)', () => {
  it.each<[RunState, string]>([
    ['queued', 'Waiting to start'],
    ['running', 'Working'],
    ['stopped_at_limit', 'Stopped — reached its limit'],
    ['failed', 'Couldn’t finish'],
    ['interrupted', 'Interrupted'],
    ['no_reply', 'No answer yet'],
    ['abandoned', 'Not used (stopped crew)'],
    ['completed', 'Done'],
    ['cancelled', 'Stopped'],
  ])('state-%s reads %s', (state, label) => {
    expect(t(`state-${state.replace(/_/g, '-')}`).replace(/'/g, '’')).toBe(label);
  });
});

describe('FU-134 a form and a crew plan appear on a screen at once', () => {
  const block = (kind: string) => ({ kind, title: 'T', text: 'hello', attachments: [] });
  async function started() {
    const transport = new FakeTransport();
    const app = await startApp(transport, { document: null, storage: null });
    return { app, transport };
  }
  it.each(['form', 'crew_plan'])('a %s block opens block:<id> as a new tab without moving the person’s place', async (kind) => {
    const { app, transport } = await started();
    transport.emit(event(LOCAL, 1, { kind: 'block_shown', workspace_id: WS, block_id: 'b1', block: block(kind) as never }));
    await new Promise((r) => setTimeout(r, 0));
    const tabs = app.screens.tabs(ScreenAreaState.scope(LOCAL, WS));
    expect(tabs.map((x) => x.key)).toEqual(['block:b1']);
    expect(tabs[0]?.isNew).toBe(true);
  });
  it('a message block opens no screen', async () => {
    const { app, transport } = await started();
    transport.emit(event(LOCAL, 1, { kind: 'block_shown', workspace_id: WS, block_id: 'b2', block: block('message') as never }));
    await new Promise((r) => setTimeout(r, 0));
    expect(app.screens.tabs(ScreenAreaState.scope(LOCAL, WS))).toEqual([]);
  });
});

describe('ADR-111 step 7 What Ordin used under an answer', () => {
  it('an answer with memory_used shows the quiet link; one without shows none', async () => {
    const tb = await backendWith();
    const base = { kind: 'message', text: 'Answer', attachments: [] };
    const props = (block: unknown) => ({ backend: tb.state, workspaceId: WS, blockId: 'b', block });
    expect(text(clean(html(Message, props({ ...base, memory_used: ['R1'] }))))).toContain('What Ordin used');
    expect(text(clean(html(Message, props(base))))).not.toContain('What Ordin used');
  });
});

describe('ADR-110 Adopt, Adopt again and Keep as it is are reachable', () => {
  async function controls(over: { access?: 'read_write' | 'read_only'; folder?: string; mark?: 'changed_at_source' | 'removed_at_source' }) {
    const tb = await backendWith();
    const team = new TeamController(tb.handle, WS);
    team.status = { binding: { folder: 'knowledge', access: over.access ?? 'read_write' }, header_line: 'acme/supplier-team › knowledge' } as never;
    const props = { backend: tb.state, team, item: { kind: 'fact', id: uid('f1') }, ...(over.folder !== undefined ? { folder: over.folder } : {}), ...(over.mark !== undefined ? { mark: over.mark } : {}), statement: 'S' };
    return { tb, team, out: text(clean(html(AdoptControls, props))) };
  }

  it('Adopt into [own folder]… on a fact from a read folder when the own folder is read-write', async () => {
    expect((await controls({ folder: 'acme/platform' })).out).toContain('Adopt into acme/supplier-team › knowledge…');
  });
  it('Edge case — nothing is offered without an own folder with read-write access', async () => {
    expect((await controls({ folder: 'acme/platform', access: 'read_only' })).out.trim()).toBe('');
  });
  it('a fact changed at its source offers Adopt again and Keep as it is; one removed offers only Keep as it is', async () => {
    const changed = (await controls({ mark: 'changed_at_source' })).out;
    expect(changed).toContain('Adopt again');
    expect(changed).toContain('Keep as it is');
    const removed = (await controls({ mark: 'removed_at_source' })).out;
    expect(removed).not.toContain('Adopt again');
    expect(removed).toContain('Keep as it is');
  });
  it('Keep as it is sends team_knowledge.keep_as_is', async () => {
    const tb = await backendWith({ setup: (tr) => tr.on('team_knowledge.keep_as_is', ok({})) });
    expect(await new TeamController(tb.handle, WS).keepAsIs({ kind: 'fact', id: uid('f1') })).toBe(true);
    expect(tb.transport.sentOf('team_knowledge.keep_as_is')[0]?.input).toEqual({ ws: WS, item: { kind: 'fact', id: uid('f1') } });
  });
});
