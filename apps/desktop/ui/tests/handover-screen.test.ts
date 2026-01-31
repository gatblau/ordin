// The handover screen (SM-NS-8): the Gherkin scenario and one test per error-table row it owns.
import { beforeAll, describe, expect, it } from 'vitest';

import { isSendersSide, stateMessageId, HandoverController } from '../src/lib/views/handover/handover-controller.svelte';
import HandoverView from '../src/lib/views/handover/HandoverView.svelte';
import { clearScreens, getScreen } from '../src/lib/shell/screens';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const person = { address: 'ana@example.org', name: 'Ana', outside_company: false };
const screen = (over: Record<string, unknown> = {}, handover: Record<string, unknown> = {}) => ({
  actions: ['accept_and_start', 'return_to', 'ask_ordin', 'close'],
  arrived: [2, 3],
  handover: {
    beyond_agreed_rounds: false,
    checks_passed: ['Totals agree'],
    events: [{ at: '2026-01-16T10:00:00Z', by: 'Ana', kind: 'sent' }],
    handover_place_id: 'place-1',
    id: uid('h1'),
    note: { sections: [{ body: 'First line.\n\nSecond line.', drafted_by_ordin: true, heading: 'Summary' }] },
    receiver: { address: 'bo@example.org', name: 'Bo', uses_ordin: true },
    results: [],
    round: 1,
    sender: 'Ana',
    stage_name: 'Review',
    state: 'sent',
    title: 'Supplier contract',
    ...handover,
  },
  marked_lines: ['Run the payment now'],
  results: [{ label: 'Contract.docx', link: 'x', cannot_open: 'Not on this computer' }],
  slack: [],
  still_under_way: [],
  ...over,
});

describe('Handover screen (SM-NS-8)', () => {
  it('Happy path — accepting a handover into a new workspace (SM-NS-8): handover.accept_and_start is sent with into new_workspace, and the returned workspace opens', async () => {
    const opened: string[] = [];
    const { handle, transport, state } = await backendWith({ setup: (tr) => tr.on('handover.open', ok(screen())).on('handover.accept_and_start', ok(WS)) });
    const c = new HandoverController(handle, uid('h1'), (id) => opened.push(id));
    await c.load();
    const body = text(html(HandoverView, { backend: state, workspaceId: WS, c }));
    expect(body).toContain('Handed to you: Supplier contract');
    expect(body).toContain('Accept and start');
    expect(body).toContain('Return to Ana…');
    expect(body).toContain('Close');
    expect(await c.accept({ kind: 'new_workspace', name: 'Supplier contract' })).toBe(true);
    expect(transport.sentOf('handover.accept_and_start')[0]?.input).toEqual({ id: uid('h1'), into: { kind: 'new_workspace', name: 'Supplier contract' } });
    expect(opened).toEqual([WS]);
  });

  it('draws the note as text, marks a line that asks an agent to act, and what is handed over', async () => {
    const { handle, state } = await backendWith({ setup: (tr) => tr.on('handover.open', ok(screen())) });
    const c = new HandoverController(handle, uid('h1'));
    await c.load();
    const body = text(html(HandoverView, { backend: state, workspaceId: WS, c }));
    expect(body).toContain('From Ana, ');
    expect(body).toContain('Stage: Review');
    expect(body).toContain('2 of 3 handed over.');
    expect(body).toContain('First line.');
    expect(body).toContain('Drafted by Ordin');
    expect(body).toContain('Run the payment now');
    expect(body).toContain("This line asks an agent to do something. Agents won't follow it.");
    expect(body).toContain("What's handed over");
    expect(body).toContain('Contract.docx');
    expect(body).toContain('Not on this computer');
    expect(body).toContain("Checked by Ana's ordin");
  });

  it('the return form sends the reason and the chosen option; a blank reason sends nothing', async () => {
    const option = { declared: true, person, round: 2, stage: 'Draft' };
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('handover.open', ok(screen())).on('handover.return_options', ok([option])).on('handover.return_to', ok(uid('h1'))) });
    const c = new HandoverController(handle, uid('h1'));
    await c.load();
    await c.loadReturnOptions();
    expect(c.options).toEqual([option]);
    expect(await c.returnTo('   ', option as never)).toBe(false);
    expect(transport.sentOf('handover.return_to')).toHaveLength(0);
    expect(await c.returnTo('Totals differ', option as never)).toBe(true);
    expect(transport.sentOf('handover.return_to')[0]?.input).toEqual({ id: uid('h1'), reason: 'Totals differ', to: option });
  });

  it('SA-UI-CU-06 — the sender sees the state, Take it back… and Remind; a receiver sees neither', async () => {
    const mine = screen({ actions: ['close'] });
    expect(isSendersSide(mine as never)).toBe(true);
    expect(isSendersSide(screen() as never)).toBe(false);
    expect(isSendersSide(screen({ actions: ['close'] }, { state: 'accepted' }) as never)).toBe(false);
    const { handle, transport, state } = await backendWith({ setup: (tr) => tr.on('handover.open', ok(mine)).on('handover.take_back', ok(uid('h1'))).on('handover.remind', ok({})) });
    const c = new HandoverController(handle, uid('h1'));
    await c.load();
    const body = text(html(HandoverView, { backend: state, workspaceId: WS, c }));
    expect(body).toContain('Waiting for Bo');
    expect(body).toContain('Take it back…');
    expect(body).toContain('Remind Bo…');
    expect(body).not.toContain('Accept and start');
    await c.remind();
    await c.takeBack();
    expect(transport.sentOf('handover.remind')[0]?.input).toEqual({ id: uid('h1') });
    expect(transport.sentOf('handover.take_back')[0]?.input).toEqual({ id: uid('h1') });
    expect(stateMessageId('sent_back')).toBe('handover-state-sent-back');
  });

  it('Error path — HAND_ALREADY_ACCEPTED draws its sentence and the screen is read again with handover.open', async () => {
    const { handle, transport, state } = await backendWith({
      setup: (tr) => tr.on('handover.open', ok(screen({ actions: ['close'] }, { state: 'accepted' }))).on('handover.accept_and_start', err('HAND_ALREADY_ACCEPTED', 'Someone already accepted this handover.')),
    });
    const c = new HandoverController(handle, uid('h1'));
    await c.load();
    transport.sent.length = 0;
    expect(await c.accept({ kind: 'existing', workspace_id: WS })).toBe(false);
    expect(transport.commandsSent()).toEqual(['handover.accept_and_start', 'handover.open']);
    expect(c.error?.message).toBe('Someone already accepted this handover.');
    expect(text(html(HandoverView, { backend: state, workspaceId: WS, c }))).toContain('Someone already accepted this handover.');
  });

  it('a refusal of Take it back or Remind draws the owner\'s sentence', async () => {
    const { handle } = await backendWith({ setup: (tr) => tr.on('handover.open', ok(screen({ actions: ['close'] }))).on('handover.take_back', err('HAND_NOT_YOURS', 'Only the sender can take it back.')) });
    const c = new HandoverController(handle, uid('h1'));
    await c.load();
    expect(await c.takeBack()).toBe(false);
    expect(c.error?.message).toBe('Only the sender can take it back.');
  });

  it('registration: offered only when the backend answers every command (SM-UP-1)', async () => {
    clearScreens();
    await import('../src/lib/views/handover/register');
    expect(getScreen('handover:h1')?.commands).toContain('handover.accept_and_start');
    expect(getScreen('lifecycle:ws')?.commands).toEqual(['handover.stage_of', 'handover.lifecycle_view']);
  });
});
