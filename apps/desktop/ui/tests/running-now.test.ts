// @vitest-environment happy-dom
// The workspace tree's Running now (SM-RN-1) and the empty Agents and Knowledge screens (SM-EM-1), mounted so
// their loading runs.
import { mount, unmount, type Component } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import Agents from '../src/lib/views/fixed/Agents.svelte';
import Knowledge from '../src/lib/views/fixed/Knowledge.svelte';
import RunningNow from '../src/lib/views/RunningNow.svelte';
import { backendWith, WS } from './fixed-support';
import { ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

async function draw(component: Component<never>, props: Record<string, unknown>): Promise<HTMLElement> {
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(component, { target, props: props as never });
  await settle(20);
  return target;
}

const home = (running: unknown[]) => ok({ waiting: [], running, since_last_looked: [], new_count: 0 });

describe('Running now in a workspace (SM-RN-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-RN-1
  it('Happy path — the workspace\'s running work, with Show progress, from home.view filtered to it', async () => {
    const run = uid('run');
    const { state, transport } = await backendWith({
      setup: (t) => t.on('home.view', home([{ line: 'Quote checker', at: '2026-01-10T10:43:58.120Z', is_new: false, run_again: false, workspace_id: WS, workspace_name: 'Supplier review', opens: { kind: 'run', run_id: run, workspace_id: WS } }])),
    });
    const target = await draw(RunningNow as never, { backend: state, workspaceId: WS, instance: null });
    expect(transport.sentOf('home.view').map((e) => e.input)).toEqual([{ workspace_id: WS }]);
    expect(transport.commandsSent()).not.toContain('home.opened');
    expect(target.textContent).toContain('Quote checker');
    expect(target.textContent).toContain('Show progress');
  });

  it('Edge case — nothing running says so', async () => {
    const { state } = await backendWith({ setup: (t) => t.on('home.view', home([])) });
    const target = await draw(RunningNow as never, { backend: state, workspaceId: WS, instance: null });
    expect(target.textContent).toContain('Nothing running now');
  });
});

describe('Empty screens say what to do (SM-EM-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-EM-1
  // SM-AG-6 (January 2026): the side tree is the one list of agents; the screen with no agent points to it.
  it('the Agents screen with no agent chosen points to the side tree', async () => {
    const { state } = await backendWith({ setup: (t) => t.on('agents.list', ok([])) });
    const target = await draw(Agents as never, { backend: state, workspaceId: WS, instance: null });
    expect(target.textContent).toContain('Choose an agent under Agents in the side tree to see what it does.');
  });

  it('an empty Knowledge screen says to ask Ordin', async () => {
    const { state } = await backendWith({ setup: (t) => t.on('knowledge.list', ok({ this_workspace: [], from_other_teams: [], not_adopted: [], from_agents: [] })) });
    const target = await draw(Knowledge as never, { backend: state, workspaceId: WS, instance: null });
    expect(target.textContent).toContain('No pages yet. Ask Ordin to write one, or keep what it learns.');
  });
});
