// Run-state message ids (ScreensModule.md, amendments *Run-state message ids*, SM-RS-1 to SM-RS-4, and *The
// labels of a crew run's states*, SM-RS-6 and SM-RS-7; SM-WR-10):
// the mapping rule, the twelve ids in the en-GB bundle, and the paused label at both places a run state
// is drawn (the plan_progress block and Home's Running now).
import { beforeAll, describe, expect, it } from 'vitest';

import BlockBody from '../src/lib/blocks/BlockBody.svelte';
import { I18n, stripIsolates, t } from '../src/lib/i18n/fluent';
import { stateMessageId } from '../src/lib/i18n/run-state';
import type { CrewRunState, HomeRow, RunState } from '../src/lib/service-api/types.gen';
import { Diagnostics } from '../src/lib/store/diagnostics.svelte';
import { runStateLabelId } from '../src/lib/views/home-model.svelte';
import { backendWith, WS } from './fixed-support';
import { EN_GB_FTL } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const RUN = '0192a1c2-3344-7556-8677-8899aabbccdd';
const CREW = '0192a1c2-3344-7556-8677-8899aabbccee';
const text = (markup: string) => markup.replace(/<!--[\s\S]*?-->/g, '').replace(/<style[\s\S]*?<\/style>/g, '').replace(/<[^>]*>/g, ' ').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');
const runView = (state: string) => ({ state, progressLines: [], folded: 0, count: null, plan: null, startUp: [] }) as never;
const runningRow: HomeRow = { at: '2026-01-29T10:00:00Z', is_new: false, line: 'Comparing the Q3 rates', opens: { kind: 'run', run_id: RUN, workspace_id: WS }, run_again: false, workspace_id: WS, workspace_name: 'Q3 supplier review' };

// SM-RS-2: the nine stored states with one label.
const ONE_LABEL: Array<[RunState, string, string]> = [
  ['queued', 'state-queued', 'Waiting to start'],
  ['running', 'state-running', 'Working'],
  ['stopped_at_limit', 'state-stopped-at-limit', 'Stopped — reached its limit'],
  ['failed', 'state-failed', "Couldn't finish"],
  ['interrupted', 'state-interrupted', 'Interrupted'],
  ['no_reply', 'state-no-reply', 'No answer yet'],
  ['abandoned', 'state-abandoned', 'Not used (stopped crew)'],
  ['completed', 'state-completed', 'Done'],
  ['cancelled', 'state-cancelled', 'Stopped'],
];
// SM-RS-3: the three paused ids.
const PAUSED: Array<[string, string]> = [
  ['state-paused-approval', 'Waiting for you'],
  ['state-paused-slack', 'Waiting for you in Slack'],
  ['state-paused-closed', 'Paused while ordin was closed'],
];

// SM-RS-6: the four states only a crew run has.
const CREW_ONLY: Array<[CrewRunState, string, string]> = [
  ['starting', 'state-starting', 'Starting'],
  ['finished', 'state-finished', 'Done'],
  ['escalated', 'state-escalated', 'Handed back to you'],
  ['killed', 'state-killed', 'Stopped'],
];
// SM-RS-7: every value of the generated enums. A `Record` over each union, so a value added to either enum
// without a row here fails the type check, and a row with no message fails the test.
const EVERY_RUN_STATE: Record<RunState, true> = {
  queued: true, running: true, paused: true, completed: true, failed: true, stopped_at_limit: true, cancelled: true, interrupted: true, abandoned: true, no_reply: true,
};
const EVERY_CREW_STATE: Record<CrewRunState, true> = {
  starting: true, running: true, paused: true, finished: true, escalated: true, stopped_at_limit: true, killed: true, failed: true,
};

/** A catalogue of its own, so a missing message is recorded where the test can see it. */
function bundle(): { i18n: I18n; diagnostics: Diagnostics } {
  const diagnostics = new Diagnostics();
  const i18n = new I18n(diagnostics);
  i18n.load({ locale: 'en-GB', direction: 'ltr', resources: [{ locale: 'en-GB', ftl: EN_GB_FTL }] });
  return { i18n, diagnostics };
}

describe('SM-RS-1 and SM-RS-2 — the id of a stored state', () => {
  it.each(ONE_LABEL)('the id built from %s is %s', (state, id) => {
    expect(stateMessageId(state)).toBe(id);
  });

  it('every id of SM-RS-2 and SM-RS-3 resolves in the en-GB bundle to the text in its row', () => {
    const { i18n, diagnostics } = bundle();
    for (const [id, words] of [...ONE_LABEL.map(([, i, w]) => [i, w] as [string, string]), ...PAUSED]) {
      expect(i18n.has(id), id).toBe(true);
      expect(stripIsolates(i18n.t(id)), id).toBe(words);
    }
    expect(diagnostics.codes()).toEqual([]);
  });

  it('Happy path — a stored state with an underscore finds its message (SM-RS-1)', () => {
    const id = stateMessageId('stopped_at_limit');
    expect(id).toBe('state-stopped-at-limit');
    expect(stripIsolates(t(id))).toBe('Stopped — reached its limit');
  });
});

describe('SM-RS-3 and SM-RS-4 — a paused run', () => {
  it('Happy path — a run paused on an approval (SM-RS-4)', () => {
    // The store's run carries no pause_reason; a paused run draws state-paused-approval whatever it is.
    const id = stateMessageId('paused');
    expect(id).toBe('state-paused-approval');
    expect(stripIsolates(t(id))).toBe('Waiting for you');
  });

  it('Edge case — no message is looked up as state-paused (SM-RS-3)', () => {
    const { i18n, diagnostics } = bundle();
    expect(i18n.has('state-paused')).toBe(false);
    const id = stateMessageId('paused');
    expect(id).not.toBe('state-paused');
    i18n.t(id);
    expect(diagnostics.codes()).not.toContain('UI_MESSAGE_MISSING');
  });

  it('Home — Running now labels a paused run Waiting for you (SM-WR-10)', async () => {
    const { state } = await backendWith();
    state.runs.set(RUN, runView('paused'));
    expect(runStateLabelId(state, runningRow)).toBe('state-paused-approval');
    state.runs.set(RUN, runView('stopped_at_limit'));
    expect(runStateLabelId(state, runningRow)).toBe('state-stopped-at-limit');
  });

  it('plan_progress — a paused run draws Waiting for you, never the id state-paused (SM-WR-10)', async () => {
    const { app } = await appWith();
    app.local.runs.set(RUN, runView('paused'));
    const out = text(html(BlockBody, { backend: app.local, workspaceId: WS, blockId: 'b1', block: { kind: 'plan_progress', subject: { kind: 'run', run_id: RUN } } }, app));
    expect(out).toContain('Waiting for you');
    expect(out).not.toContain('state-paused');
  });
});

describe('SM-RS-6 and SM-RS-7 — a crew run\'s states', () => {
  it.each(CREW_ONLY)('the id built from the crew state %s is %s, reading its row\'s words', (state, id, words) => {
    expect(stateMessageId(state)).toBe(id);
    expect(stripIsolates(t(id))).toBe(words);
  });

  it('Happy path — a crew run handed back to the person (SM-RS-6)', () => {
    const id = stateMessageId('escalated');
    expect(id).toBe('state-escalated');
    expect(stripIsolates(t(id))).toBe('Handed back to you');
  });

  it('Edge case — every state the screens receive has a label (SM-RS-7)', () => {
    const { i18n, diagnostics } = bundle();
    const states = [...Object.keys(EVERY_RUN_STATE), ...Object.keys(EVERY_CREW_STATE)] as Array<RunState | CrewRunState>;
    for (const state of states) {
      const id = stateMessageId(state);
      expect(i18n.has(id), `${state} → ${id}`).toBe(true);
      i18n.t(id);
    }
    expect(diagnostics.codes()).toEqual([]);
  });

  it('plan_progress — a crew run handed back draws its words, never its stored state', async () => {
    const { app } = await appWith();
    app.local.crews.set(CREW, { round: 3, state: 'escalated', startUp: [] } as never);
    const out = text(html(BlockBody, { backend: app.local, workspaceId: WS, blockId: 'b2', block: { kind: 'plan_progress', subject: { kind: 'crew_run', crew_run_id: CREW } } }, app));
    expect(out).toContain('Handed back to you');
    expect(out).not.toContain('state-escalated');
  });
});
