// Earlier conversations and Continue this conversation (ADR-108: SM-CC-2 to SM-CC-7).
import { beforeAll, describe, expect, it } from 'vitest';

import { err, ok, R1_GROUPS, settle } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { loaded, text } from './settings-support';
import ContinueDialog from '../src/lib/views/ContinueDialog.svelte';
import EarlierConversation from '../src/lib/views/EarlierConversation.svelte';
import EarlierConversations from '../src/lib/views/EarlierConversations.svelte';
import { chains, ContinueFlow, EarlierListModel, EarlierReadModel, prepareKeyPoints } from '../src/lib/views/earlier.svelte';
import type { ConversationSummary } from '../src/lib/service-api/types.gen';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const conv = (id: string, over: Partial<ConversationSummary> = {}): ConversationSummary => ({
  conversation_id: id, first_request: `First request of ${id}`, message_count: 4, started_at: `2026-01-1${Math.min(id.length, 9)}T10:00:00Z`, state: 'earlier', ...over,
});
const message = (seq: number, over: Record<string, unknown> = {}) => ({ seq, at: '2026-01-10T10:00:00Z', author: 'person', text: `message ${seq}`, ...over });

describe('the list: one entry per chain (SM-CC-6)', () => {
  it('folds a conversation that another earlier one continues into its chain', () => {
    const items = [conv('a', { started_at: '2026-01-01T10:00:00Z', message_count: 3 }), conv('b', { continues: 'a', started_at: '2026-01-02T10:00:00Z', message_count: 5 }), conv('c', { started_at: '2026-01-03T10:00:00Z' })];
    const entries = chains(items);
    expect(entries.map((e) => e.head.conversation_id)).toEqual(['b', 'c']);
    expect(entries[0]?.chain.map((c) => c.conversation_id)).toEqual(['a', 'b']);
    expect(entries[0]?.messageCount).toBe(8);
  });

  it('lists earlier conversations newest first with the first request, date and message count; the live one is not listed', async () => {
    const t = await loaded(R1_GROUPS, (tr) =>
      tr.on('ordin.list_conversations', ok({
        items: [
          conv('live', { state: 'current', continues: 'old', started_at: '2026-01-05T10:00:00Z' }),
          conv('old', { first_request: 'Q3 supplier rates', started_at: '2026-01-01T10:00:00Z', message_count: 12 }),
          conv('newer', { first_request: 'Account brief', started_at: '2026-01-03T10:00:00Z', message_count: 7 }),
        ],
        next_cursor: null,
      })),
    );
    const model = new EarlierListModel(t.handle, WS);
    await model.load();
    expect(t.transport.sentOf('ordin.list_conversations')[0]?.input).toEqual({ workspace_id: WS });
    expect(model.entries.map((e) => e.head.conversation_id)).toEqual(['newer', 'old']);
    const out = text(clean(html(EarlierConversations, { backend: t.state, workspaceId: WS, instance: null, model })));
    expect(out).toContain('Earlier conversations');
    expect(out).toContain('Q3 supplier rates');
    expect(out).toContain('12 messages');
    expect(out).toContain('Account brief');
    expect(out).not.toContain('live');
  });
});

describe('a read-only conversation', () => {
  async function read(extra: Parameters<typeof loaded>[1] = () => undefined) {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('ordin.list_conversations', ok({ items: [conv(uid('a'), { message_count: 2 }), conv(uid('b'), { continues: uid('a'), message_count: 2 })], next_cursor: null }));
      tr.on('ordin.read_conversation', (input) => ok({ items: [message(1, { text: `from ${(input as { conversation_id: string }).conversation_id}` })], next_cursor: null }));
      extra(tr);
    });
    const model = new EarlierReadModel(t.handle, WS, uid('b'));
    await model.load();
    return { ...t, model };
  }

  it('draws the whole chain, oldest first, with where the next one continued', async () => {
    const t = await read();
    const out = text(clean(html(EarlierConversation, { backend: t.state, workspaceId: WS, instance: uid('b'), model: t.model })));
    expect(out.indexOf(`from ${uid('a')}`)).toBeGreaterThan(-1);
    expect(out.indexOf(`from ${uid('a')}`)).toBeLessThan(out.indexOf(`from ${uid('b')}`));
    expect(out).toContain('Continued');
  });

  it('Continue is offered on an earlier conversation only', async () => {
    const t = await read();
    const out = text(html(EarlierConversation, { backend: t.state, workspaceId: WS, instance: uid('b'), model: t.model }));
    expect(out).toContain('Continue this conversation');
    expect(out).toContain('Ask Ordin about it');
    // The live conversation offers it nowhere: the list does not carry it either.
    const list = await loaded(R1_GROUPS, (tr) => tr.on('ordin.list_conversations', ok({ items: [conv(uid('live'), { state: 'current' })], next_cursor: null })));
    const model = new EarlierListModel(list.handle, WS);
    await model.load();
    expect(text(html(EarlierConversations, { backend: list.state, workspaceId: WS, instance: null, model }))).not.toContain('Continue');
  });

  it('A moved item shows once', async () => {
    const t = await read((tr) => {
      tr.on('ordin.read_conversation', ok({ items: [message(1, { block_id: uid('q1'), text: 'Which supplier?' })], next_cursor: null }));
      tr.on('ordin.block', ok({ conversation_id: uid('live'), block: { kind: 'message', text: 'x', attachments: [] }, block_id: uid('q1'), waiting: true, from: 'ordin', at: '2026-01-10T10:00:00Z' }));
    });
    const out = text(clean(html(EarlierConversation, { backend: t.state, workspaceId: WS, instance: uid('b'), model: t.model })));
    expect(out).toContain('Moved to the current conversation');
    expect(out).not.toContain('Which supplier?');
  });

  it('Show more reads the next 100 of the first conversation that has more', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('ordin.list_conversations', ok({ items: [conv(uid('a'))], next_cursor: null }));
      tr.on('ordin.read_conversation', (input) => ((input as { cursor?: string }).cursor === undefined ? ok({ items: [message(1)], next_cursor: 'c2' }) : ok({ items: [message(2)], next_cursor: null })));
    });
    const model = new EarlierReadModel(t.handle, WS, uid('a'));
    await model.load();
    expect(model.hasMore).toBe(true);
    await model.showMore();
    expect(model.parts[0]?.messages.map((m) => m.seq)).toEqual([1, 2]);
    expect(model.hasMore).toBe(false);
  });

  it('Ask Ordin about it sends the question', async () => {
    const t = await read((tr) => tr.on('ordin.ask_about_conversation', ok({ accepted: true })));
    t.model.question = 'What did we decide?';
    expect(await t.model.ask()).toBe(true);
    expect(t.transport.sentOf('ordin.ask_about_conversation')[0]?.input).toEqual({ workspace_id: WS, conversation_id: uid('b'), question: 'What did we decide?' });
  });
});

describe('Continue this conversation (SM-CC-3 to SM-CC-5)', () => {
  function flowFor(handle: Parameters<typeof ContinueFlow.prototype.send>[0] extends never ? never : ConstructorParameters<typeof ContinueFlow>[0], live: number, calls: string[] = []) {
    return new ContinueFlow(handle, {
      workspaceId: WS,
      liveMessages: () => live,
      nameOf: () => 'Q3 supplier rates',
      announceText: (n) => `Continuing “${n}”`,
      onContinued: (id, text) => calls.push(`${id}|${text}`),
    });
  }

  it('Asking first, and Cancel changes nothing', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('ordin.continue_conversation', ok({ conversation_id: 'new' })));
    const flow = flowFor(t.handle, 2);
    await flow.request(uid('x'));
    expect(flow.asking).toBe(uid('x'));
    const dialog = text(html(ContinueDialog, { name: 'Q3 supplier rates', onkeep: () => undefined, oncontinue: () => undefined, oncancel: () => undefined }));
    expect(dialog).toContain('Continue this conversation?');
    expect(dialog).toContain('Keep the key points first');
    expect(dialog).toContain('Continue');
    expect(dialog).toContain('Cancel');
    expect(dialog).toContain('Ordin stops working from the current conversation and works from “Q3 supplier rates” instead.');
    flow.cancel();
    expect(t.transport.sent).toEqual([]);
  });

  it('Continue is not the default button', () => {
    const markup = html(ContinueDialog, { name: 'x', onkeep: () => undefined, oncontinue: () => undefined, oncancel: () => undefined });
    expect(markup).toMatch(/<button[^>]*data-default="true"[^>]*>Cancel/);
    expect(markup).not.toMatch(/data-default="true"[^>]*>Continue/);
  });

  it('Nothing to ask when the live conversation is empty', async () => {
    const calls: string[] = [];
    const t = await loaded(R1_GROUPS, (tr) => tr.on('ordin.continue_conversation', ok({ conversation_id: 'new' })));
    const flow = flowFor(t.handle, 0, calls);
    await flow.request(uid('x'));
    expect(flow.asking).toBeNull();
    expect(t.transport.sentOf('ordin.continue_conversation')[0]?.input).toEqual({ workspace_id: WS, conversation_id: uid('x'), kept_points: [] });
    // SM-CC-4: the tab closes, the new conversation shows, a polite announcement says what is continued.
    expect(calls).toEqual(['new|Continuing “Q3 supplier rates”']);
  });

  it('Continue in the dialog sends with no kept points', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('ordin.continue_conversation', ok({ conversation_id: 'new' })));
    const flow = flowFor(t.handle, 3);
    await flow.request(uid('x'));
    await flow.confirm();
    expect(t.transport.sentOf('ordin.continue_conversation')[0]?.input).toEqual({ workspace_id: WS, conversation_id: uid('x'), kept_points: [] });
  });

  it('Keeping key points before continuing', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('ordin.prepare_start_fresh', ok({ accepted: true }));
      tr.on('ordin.continue_conversation', ok({ conversation_id: 'new' }));
    });
    const flow = flowFor(t.handle, 2);
    await flow.request(uid('x'));
    await flow.keepPointsFirst();
    // Scenario: Happy path — Keep the key points first while continuing (SM-WR-12)
    expect(t.transport.commandsSent()).toEqual(['ordin.prepare_start_fresh']);
    expect(t.transport.sentOf('ordin.prepare_start_fresh')[0]?.input).toEqual({ workspace_id: WS, conversation_id: uid('x') });
    expect(flow.keeping).toBe(uid('x'));
  });

  it('Keep the key points first from Clear names no conversation (SM-WR-12)', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('ordin.prepare_start_fresh', ok({ accepted: true })));
    expect(await prepareKeyPoints(t.handle, WS)).toBeNull();
    expect(t.transport.sentOf('ordin.prepare_start_fresh')[0]?.input).toEqual({ workspace_id: WS });
  });

  it('a crew running refuses the key points like Continue (SM-WR-12, SM-CC-5)', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('ordin.prepare_start_fresh', err('OH_CREW_RUNNING', 'A crew is running.')));
    const flow = flowFor(t.handle, 2);
    await flow.request(uid('x'));
    await flow.keepPointsFirst();
    expect(flow.waitsForCrew).toBe(true);
    expect(flow.keeping).toBeNull();
  });

  it('A crew is running', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('ordin.continue_conversation', err('OH_CREW_RUNNING', 'A crew is running.')));
    const flow = flowFor(t.handle, 0);
    await flow.request(uid('x'));
    expect(flow.waitsForCrew).toBe(true);
    expect(flow.error).toBeNull();
    // Unavailable until the next run_state_changed, and a second press sends nothing.
    t.transport.sent.length = 0;
    await flow.request(uid('x'));
    expect(t.transport.sent).toEqual([]);
    flow.onRunStateChanged();
    expect(flow.waitsForCrew).toBe(false);
  });

  it('Continue this conversation is unavailable with "Waits until the crew finishes"', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('ordin.list_conversations', ok({ items: [conv(uid('a'))], next_cursor: null }));
      tr.on('ordin.read_conversation', ok({ items: [], next_cursor: null }));
    });
    const model = new EarlierReadModel(t.handle, WS, uid('a'));
    await model.load();
    const flow = flowFor(t.handle, 0);
    flow.waitsForCrew = true;
    const markup = html(EarlierConversation, { backend: t.state, workspaceId: WS, instance: uid('a'), model, flow });
    expect(text(markup)).toContain('Waits until the crew finishes');
    expect(markup).toMatch(/<button[^>]*disabled[^>]*>Continue this conversation/);
  });

  it('other errors are drawn by the error line as their owner’s sentence', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('ordin.list_conversations', ok({ items: [conv(uid('a'))], next_cursor: null }));
      tr.on('ordin.read_conversation', ok({ items: [], next_cursor: null }));
      tr.on('ordin.continue_conversation', err('OH_CONVERSATION_NOT_CONTINUABLE', 'That conversation can’t be continued.'));
    });
    const model = new EarlierReadModel(t.handle, WS, uid('a'));
    await model.load();
    const flow = flowFor(t.handle, 0);
    await flow.request(uid('a'));
    expect(flow.waitsForCrew).toBe(false);
    expect(text(html(EarlierConversation, { backend: t.state, workspaceId: WS, instance: uid('a'), model, flow }))).toContain('That conversation can’t be continued.');
    void settle;
  });
});
