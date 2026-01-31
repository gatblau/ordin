// @vitest-environment happy-dom
// The run screen (SM-RS-1), mounted so its reading runs.
import { mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import RunScreen from '../src/lib/views/RunScreen.svelte';
import { WS } from './fixed-support';
import { err, hello, LOCAL, ok, R1_GROUPS, settle, type FakeTransport } from './support/fake-backend';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const RUN = uid('run');
const AGENT = uid('agent');
const text = (el: Element): string => (el.textContent ?? '').replace(/[⁨⁩]/g, '');

function runView(state: string, endReason: string | null) {
  return ok({
    workspace_id: WS,
    run: {
      id: RUN,
      state,
      end_reason: endReason,
      agent: { kind: 'agent_instance', agent_instance_id: AGENT },
      adapter: 'claude_local',
      origin: { kind: 'task' },
      inputs_used: [],
      limits: { wall_clock_secs: 3600, idle_timeout_secs: 600 },
      queued_at: '2026-01-10T10:43:58.120Z',
      usage: {},
    },
    task: { id: uid('task'), workspace_id: WS, free_text: 'Summarise the meeting notes', arguments: [], attached_artefacts: [], requested_at: '2026-01-10T10:43:58.120Z', target: { kind: 'agent_instance', agent_instance_id: AGENT } },
  });
}

/** The run screen for RUN in an app whose backend answers `setup`; the app context is given, as the shell gives it. */
async function draw(setup: (t: FakeTransport) => void): Promise<{ target: HTMLElement; transport: FakeTransport; app: AppContext }> {
  // The desktop answers `runs` and `journal` (ALWAYS_ANSWERED_GROUPS); the shared test list leaves them out.
  const { app, transport } = await appWith((tr) => {
    tr.on('service.hello', ok(hello(LOCAL, [...R1_GROUPS, 'runs', 'journal'])));
    setup(tr);
  });
  transport.sent.length = 0;
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(RunScreen as never, { target, props: { backend: app.local, workspaceId: WS, instance: RUN } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await settle(30);
  return { target, transport, app };
}

describe('The run screen (SM-RS-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-RS-1
  it('Happy path — a finished run shows the agent, what it was asked and what it produced', async () => {
    const artefact = uid('artefact');
    const { target, transport } = await draw((t) =>
        t
          .on('runs.get', runView('completed', 'Kestrel is cheapest.'))
          .on('agents.get', ok({ instance: { display_name: 'Document agent' }, tasks: [], uses: [], workspaces: [WS] }))
          .on('journal.list_entries', ok({ items: [{ id: uid('entry'), kind: 'run_result', component: 'session_supervisor', recorded_at: '2026-01-10T10:44:00.000Z', summary: 'run result', run_id: RUN, data: { reason: 'completed', final_message: 'Kestrel is cheapest at 1,240 GBP per container.', artefacts: [artefact] } }], next_cursor: null })),
    );
    expect(transport.sentOf('runs.get').map((e) => e.input)).toEqual([{ run_id: RUN }]);
    expect(transport.sentOf('journal.list_entries').map((e) => e.input)).toEqual([{ filter: { run_id: RUN, kind: 'run_result' } }]);
    expect(target.querySelector('#run-screen-heading')?.textContent).toBe('Document agent');
    expect(text(target)).toContain('Asked: Summarise the meeting notes');
    expect(text(target)).toContain('Kestrel is cheapest at 1,240 GBP per container.');
  });

  it('Edge case — with no journal entry the run keeps its first words', async () => {
    const { target } = await draw((t) => t.on('runs.get', runView('completed', 'Kestrel is cheapest.')).on('agents.get', ok({ instance: { display_name: 'Document agent' }, tasks: [], uses: [], workspaces: [WS] })).on('journal.list_entries', ok({ items: [], next_cursor: null })),
    );
    expect(text(target)).toContain('Kestrel is cheapest.');
  });

  it('Edge case — a running run shows its progress and no result', async () => {
    const { target, transport } = await draw((t) => t.on('runs.get', runView('running', null)).on('agents.get', ok({ instance: { display_name: 'Document agent' }, tasks: [], uses: [], workspaces: [WS] })),
    );
    expect(transport.sentOf('journal.list_entries')).toEqual([]);
    expect(text(target)).not.toContain('It left no message.');
  });
});

const agentReply = ok({ instance: { display_name: 'Document agent' }, tasks: [], uses: [], workspaces: [WS] });
const noJournal = ok({ items: [], next_cursor: null });
const tryAgainButton = (target: HTMLElement): HTMLButtonElement | undefined =>
  [...target.querySelectorAll('button')].find((b) => b.textContent === 'Try again');

describe('Try again on a run that stopped (SM-RS-8)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-RS-8
  it('Happy path — an interrupted run is tried again and the new run opens', async () => {
    const next = uid('run');
    const { target, transport, app } = await draw((t) =>
      t
        .on('runs.get', runView('interrupted', null))
        .on('agents.get', agentReply)
        .on('journal.list_entries', noJournal)
        .on('runs.try_again', ok({ kind: 'started', run_id: next })),
    );
    const button = tryAgainButton(target);
    expect(button).toBeDefined();
    button?.click();
    await settle(20);
    expect(transport.sentOf('runs.try_again').map((e) => e.input)).toEqual([{ workspace_id: WS, run_id: RUN }]);
    const tabs = app.screens.tabs(ScreenAreaState.scope(app.local.id, WS));
    expect(tabs.some((tab) => tab.key === `run:${next}`)).toBe(true);
  });

  it('Edge case — when the conversation asks first, the screen says so', async () => {
    const { target } = await draw((t) =>
      t
        .on('runs.get', runView('failed', null))
        .on('agents.get', agentReply)
        .on('journal.list_entries', noJournal)
        .on('runs.try_again', ok({ kind: 'waiting', text: 'Waiting for the person to start it.' })),
    );
    tryAgainButton(target)?.click();
    await settle(20);
    expect(text(target)).toContain('Nothing has started yet: the conversation needs you first.');
    // The host's sentence is written for Ordin, not the person (SM-RS-8).
    expect(text(target)).not.toContain('Waiting for the person to start it.');
  });

  it('Edge case — a completed run offers no Try again', async () => {
    const { target } = await draw((t) => t.on('runs.get', runView('completed', 'Done.')).on('agents.get', agentReply).on('journal.list_entries', noJournal));
    expect(tryAgainButton(target)).toBeUndefined();
  });

  it('Error path — a refusal is the error line', async () => {
    const { target } = await draw((t) =>
      t
        .on('runs.get', runView('interrupted', null))
        .on('agents.get', agentReply)
        .on('journal.list_entries', noJournal)
        .on('runs.try_again', err('OH_RUN_NOT_REPEATABLE', "This work can't be started again from here. Ask ordin in the conversation instead.")),
    );
    tryAgainButton(target)?.click();
    await settle(20);
    expect(text(target)).toContain("This work can't be started again from here.");
  });
});
