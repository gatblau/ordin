import { describe, expect, it } from 'vitest';

import { createHandle, type EventEnvelope } from '../src/lib/backend/backend';
import { BackendState, PROGRESS_LINES_KEPT, Stores } from '../src/lib/store/backend-state.svelte';
import { Diagnostics } from '../src/lib/store/diagnostics.svelte';
import { approvalCard, event, FakeTransport, hello, LOCAL, ok, settle } from './support/fake-backend';
import { uid } from './support/ids';

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';

async function loadedState(options: { lastSeq?: number; groups?: string[] } = {}) {
  const transport = new FakeTransport();
  transport.on('service.hello', ok(hello(LOCAL, options.groups ?? ['service', 'home'], options.lastSeq ?? 0)));
  const diagnostics = new Diagnostics();
  const handle = createHandle({ id: LOCAL, kind: 'local', transport, diagnostics });
  const state = new BackendState({ handle, diagnostics });
  await state.load();
  return { transport, diagnostics, handle, state };
}
const run = (seq: number, state: 'running' | 'completed' = 'running', id = 'r1') =>
  event(LOCAL, seq, { kind: 'run_state_changed', run_id: id, state });

describe('load and reload (step 5)', () => {
  it('uses service.hello for the baseline and the groups', async () => {
    const { state, transport } = await loadedState({ lastSeq: 40, groups: ['service', 'mail'] });
    expect(state.lastSeq).toBe(40);
    expect(state.loaded).toBe(true);
    expect([...state.groups]).toEqual(['mail', 'service']);
    expect(transport.commandsSent().slice(0, 2)).toEqual(['service.hello', 'workspaces.list']);
    expect(transport.commandsSent()).toEqual(expect.arrayContaining(['home.needs_you', 'approval.list_pending']));
  });

  it('SM-UP-1: a reload keeps the new set of groups', async () => {
    const { state, transport } = await loadedState({ groups: ['service', 'mail'] });
    transport.on('service.hello', ok(hello(LOCAL, ['service'], 3)));
    await state.reload();
    expect([...state.groups]).toEqual(['service']);
  });

  it('events that arrive before the first load completes are applied after it, those at or below the baseline dropped', async () => {
    const transport = new FakeTransport();
    transport.on('service.hello', ok(hello(LOCAL, ['service'], 10)));
    const handle = createHandle({ id: LOCAL, kind: 'local', transport });
    const state = new BackendState({ handle });
    void state.receive(run(9));
    void state.receive(run(11, 'completed'));
    await state.load();
    await settle();
    expect(state.lastSeq).toBe(11);
    expect(state.runs.get('r1')?.state).toBe('completed');
  });
});

describe('applying events (step 4), table-driven', () => {
  it.each([
    ['in order', [41, 42, 43], 43, []],
    ['a repeat after reconnection is dropped', [41, 41, 42], 42, []],
    ['an older sequence number is dropped', [41, 42, 40], 42, []],
  ])('%s', async (_name, seqs, expectedLast, codes) => {
    const { state, diagnostics } = await loadedState({ lastSeq: 40 });
    for (const seq of seqs) await state.receive(run(seq));
    expect(state.lastSeq).toBe(expectedLast);
    expect(diagnostics.codes()).toEqual(codes);
  });

  it('Edge case — a gap in the sequence: SVC_EVENT_GAP, reload through commands, then the event is applied', async () => {
    const { state, diagnostics, transport } = await loadedState({ lastSeq: 120 });
    const before = transport.sentOf('service.hello').length;
    transport.on('service.hello', ok(hello(LOCAL, ['service', 'home'], 122)));
    await state.receive(run(124, 'completed'));
    expect(diagnostics.codes()).toEqual(['SVC_EVENT_GAP']);
    expect(transport.sentOf('service.hello').length).toBe(before + 1);
    expect(state.lastSeq).toBe(124);
    expect(state.runs.get('r1')?.state).toBe('completed');
  });

  it('a gap whose reload already covers the event drops it', async () => {
    const { state, transport } = await loadedState({ lastSeq: 120 });
    transport.on('service.hello', ok(hello(LOCAL, ['service'], 130)));
    await state.receive(run(124));
    expect(state.lastSeq).toBe(130);
    expect(state.runs.has('r1')).toBe(false);
  });

  it('Edge case — reset: the backend state is cleared and reloaded, with nothing shown', async () => {
    const { state, diagnostics, transport } = await loadedState({ lastSeq: 40 });
    await state.receive(run(41));
    await state.receive(event(LOCAL, 42, { kind: 'approval_requested', card: approvalCard() }));
    expect(state.pendingApprovals.size).toBe(1);
    transport.on('service.hello', ok(hello(LOCAL, ['service'], 50)));
    await state.receive(event(LOCAL, 43, { kind: 'reset' }));
    expect(state.runs.size).toBe(0);
    expect(state.pendingApprovals.size).toBe(0);
    expect(state.lastSeq).toBe(50);
    expect(diagnostics.codes()).toEqual([]);
  });

  it('Error path — an event of an unknown kind is ignored and UI_EVENT_KIND_UNKNOWN recorded', async () => {
    const { state, diagnostics } = await loadedState({ lastSeq: 40 });
    await state.receive({ api_version: { major: 1, minor: 3 }, backend: LOCAL, seq: 41, at: 'x', kind: 'timeline_changed' } as unknown as EventEnvelope);
    expect(diagnostics.codes()).toEqual(['UI_EVENT_KIND_UNKNOWN']);
    expect(state.lastSeq).toBe(41);
  });

  it('Error path — an event of another major version is dropped (UI_EVENT_VERSION)', async () => {
    const { state, diagnostics } = await loadedState({ lastSeq: 40 });
    await state.receive({ ...run(41), api_version: { major: 2, minor: 0 } });
    expect(diagnostics.codes()).toEqual(['UI_EVENT_VERSION']);
    expect(state.lastSeq).toBe(40);
  });

  it('Error path — an event for an unknown backend is dropped (UI_BACKEND_UNKNOWN)', async () => {
    const { state, diagnostics } = await loadedState();
    const stores = new Stores(diagnostics);
    stores.add(state);
    await stores.receive(event('0192f100-0000-7000-8000-0000000000ff', 1, { kind: 'reset' }));
    expect(diagnostics.codes()).toEqual(['UI_BACKEND_UNKNOWN']);
  });

  it('Stores routes an event to the store of its backend and reports the after-map', async () => {
    const { state } = await loadedState({ lastSeq: 5 });
    const stores = new Stores();
    stores.add(state);
    await stores.receive(run(6));
    expect(stores.lastSeqs()).toEqual({ [LOCAL]: 6 });
  });
});

describe('what the events change', () => {
  it('Happy path — an approval is raised and decided (the spec example)', async () => {
    const { state } = await loadedState({ lastSeq: 40 });
    await state.receive(event(LOCAL, 41, { kind: 'approval_requested', card: approvalCard() }));
    expect(state.lastSeq).toBe(41);
    expect([...state.pendingApprovals.keys()]).toEqual(['0192f1a0-5b6c-7d8e-9f00-112233445566']);
    expect(state.needsYouCount).toBe(1);
    expect(state.needsYouGroups()[0]?.workspace_id).toBe(WS);

    await state.receive(
      event(LOCAL, 42, { kind: 'approval_decided', approval_id: '0192f1a0-5b6c-7d8e-9f00-112233445566', state: 'approved', decided_on: "Asha's laptop" }, '2026-01-10T10:44:03.877Z'),
    );
    expect(state.pendingApprovals.size).toBe(0);
    expect(state.decided.get('0192f1a0-5b6c-7d8e-9f00-112233445566')).toEqual({
      state: 'approved',
      decided_on: "Asha's laptop",
      at: '2026-01-10T10:44:03.877Z',
    });
    expect(state.needsYouCount).toBe(0);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-CV-1
  it('SM-CV-1 — the person\'s own message keeps its author; a block with no author is Ordin\'s', async () => {
    const { state } = await loadedState({ lastSeq: 0 });
    const message = (text: string) => ({ kind: 'message', text, attachments: [] }) as never;
    await state.receive(event(LOCAL, 1, { kind: 'block_shown', workspace_id: WS, block_id: uid('mine'), block: message('What do the notes say?'), author: 'person' } as never));
    await state.receive(event(LOCAL, 2, { kind: 'block_shown', workspace_id: WS, block_id: uid('reply'), block: message('Rates go up in Q3.') } as never));
    const items = state.conversation.get(WS) ?? [];
    expect(items.map((i) => i.from ?? 'ordin')).toEqual(['person', 'ordin']);
  });

  it('Happy path — progress: plan, mark and a line reach the run', async () => {
    const { state } = await loadedState({ lastSeq: 0 });
    const steps = [1, 2, 3, 4].map((position) => ({ position, line: `s${position}`, mark: position === 1 ? 'done' : 'next', plan_version: 1, run_id: 'r1' })) as never;
    await state.receive(event(LOCAL, 1, { kind: 'plan_changed', run_id: 'r1', plan_version: 1, steps }));
    await state.receive(event(LOCAL, 2, { kind: 'step_marked', run_id: 'r1', plan_version: 1, position: 2, mark: 'now' }));
    await state.receive(event(LOCAL, 3, { kind: 'run_progress', run_id: 'r1', progress: { line: 'Read 12 of 40 quotes', count: { done: 12, total: 40 } } }));
    const view = state.runs.get('r1');
    expect(view?.plan?.steps.map((s) => s.mark)).toEqual(['done', 'now', 'next', 'next']);
    expect(view?.progressLines).toEqual([{ kind: 'plan_changed', plan_version: 1 }, { kind: 'line', line: 'Read 12 of 40 quotes' }]);
    expect(view?.count).toEqual({ done: 12, total: 40 });
  });

  it('keeps the last 200 progress lines and counts the folded ones', async () => {
    const { state } = await loadedState({ lastSeq: 0 });
    for (let i = 1; i <= PROGRESS_LINES_KEPT + 25; i++) {
      await state.receive(event(LOCAL, i, { kind: 'run_progress', run_id: 'r1', progress: { line: `line ${i}` } }));
    }
    const view = state.runs.get('r1');
    expect(view?.progressLines.length).toBe(PROGRESS_LINES_KEPT);
    expect(view?.folded).toBe(25);
    expect(view?.progressLines[0]).toEqual({ kind: 'line', line: 'line 26' });
  });

  it('a burst of 500 events applies in well under 100 ms', async () => {
    const { state } = await loadedState({ lastSeq: 0 });
    const started = performance.now();
    for (let i = 1; i <= 500; i++) await state.receive(run(i, i % 2 === 0 ? 'running' : 'completed', `r${i % 20}`));
    expect(performance.now() - started).toBeLessThan(100);
    expect(state.lastSeq).toBe(500);
  });

  it('Edge case — a question stays in the conversation and waits until it is answered', async () => {
    const { state } = await loadedState({ lastSeq: 0 });
    const block = { kind: 'question', question: 'Which?', choices: ['a', 'b', 'c'], allow_other: false } as never;
    await state.receive(event(LOCAL, 1, { kind: 'block_shown', workspace_id: WS, block_id: 'b1', block }));
    expect(state.conversation.get(WS)?.[0]?.waiting).toBe(true);
    state.markAnswered(WS, 'b1');
    expect(state.conversation.get(WS)?.[0]?.waiting).toBe(false);
  });

  it('a block_shown repeat does not add the item twice; items stay in seq order', async () => {
    const { state } = await loadedState({ lastSeq: 0 });
    const message = (text: string) => ({ kind: 'message', text, attachments: [] }) as never;
    await state.receive(event(LOCAL, 1, { kind: 'block_shown', workspace_id: WS, block_id: 'b1', block: message('one') }));
    await state.receive(event(LOCAL, 2, { kind: 'block_shown', workspace_id: WS, block_id: 'b1', block: message('one') }));
    await state.receive(event(LOCAL, 3, { kind: 'block_shown', workspace_id: WS, block_id: 'b2', block: message('two') }));
    expect(state.conversation.get(WS)?.map((i) => i.block_id)).toEqual(['b1', 'b2']);
  });

  it('block_available fetches the item with ordin.block', async () => {
    const { state, transport } = await loadedState({ lastSeq: 0 });
    transport.on('ordin.block', ok({
      at: '2026-01-10T10:00:00Z', block: { kind: 'message', text: 'hi', attachments: [] }, block_id: uid('b9'),
      conversation_id: '0192f000-0000-7000-8000-000000000001', from: 'ordin', waiting: false,
    }));
    await state.receive(event(LOCAL, 1, { kind: 'block_available', workspace_id: WS, block_id: uid('b9') }));
    await settle();
    expect(state.conversation.get(WS)?.[0]?.conversation_id).toBe('0192f000-0000-7000-8000-000000000001');
  });

  it('workspace_changed, needs_you_changed, sign_in_changed and setup_progress bump their revisions and re-read', async () => {
    const { state, transport } = await loadedState({ lastSeq: 0 });
    await state.receive(event(LOCAL, 1, { kind: 'workspace_changed', workspace_id: WS }));
    expect(state.workspaceRevision(WS)).toBe(1);
    const asked = transport.sentOf('home.needs_you').length;
    await state.receive(event(LOCAL, 2, { kind: 'needs_you_changed', workspace_id: WS }));
    await settle();
    expect(transport.sentOf('home.needs_you').length).toBe(asked + 1);
    await state.receive(event(LOCAL, 3, { kind: 'sign_in_changed', credential_id: 'c', credential_kind: 'gitlab', state: 'needs_sign_in' } as never));
    expect(state.signInRevision).toBe(1);
    await state.receive(event(LOCAL, 4, { kind: 'setup_progress', progress: {} as never }));
    expect(state.setupRevision).toBe(1);
  });

  it('listeners see each applied event, not the dropped ones', async () => {
    const { state } = await loadedState({ lastSeq: 0 });
    const seen: number[] = [];
    const stop = state.subscribe((env) => seen.push(env.seq));
    await state.receive(run(1));
    await state.receive(run(1));
    await state.receive(run(2));
    stop();
    await state.receive(run(3));
    expect(seen).toEqual([1, 2]);
  });

  it('pending approvals come from approval.list_pending at load, and only the pending ones', async () => {
    const transport = new FakeTransport();
    transport.on('approval.list_pending', ok({
      items: [
        { card: approvalCard(), state: 'pending', proposal: {} },
        { card: approvalCard({ approval_id: 'other' }), state: 'approved', proposal: {} },
      ],
    }));
    const handle = createHandle({ id: LOCAL, kind: 'local', transport });
    const state = new BackendState({ handle });
    await state.load();
    expect([...state.pendingApprovals.keys()]).toEqual(['0192f1a0-5b6c-7d8e-9f00-112233445566']);
  });

  it('Needs you puts the open workspace first and counts a row once when its approval is pending', async () => {
    const transport = new FakeTransport();
    transport.on('home.needs_you', ok([
      { at: 'x', is_new: false, line: 'Answer the question', opens: { kind: 'block', workspace_id: 'wB', block_id: 'b' }, run_again: false, workspace_id: 'wB', workspace_name: 'B' },
      { at: 'x', is_new: false, line: 'dup', opens: { kind: 'approval', workspace_id: WS, approval_id: '0192f1a0-5b6c-7d8e-9f00-112233445566' }, run_again: false, workspace_id: WS },
    ]));
    transport.on('approval.list_pending', ok({ items: [{ card: approvalCard(), state: 'pending', proposal: {} }] }));
    const state = new BackendState({ handle: createHandle({ id: LOCAL, kind: 'local', transport }) });
    await state.load();
    expect(state.needsYouCount).toBe(2);
    expect(state.needsYouGroups('wB')[0]?.workspace_id).toBe('wB');
    expect(state.needsYouGroups(WS)[0]?.workspace_id).toBe(WS);
    expect(state.needsYouCountFor(WS)).toBe(1);
  });
});
