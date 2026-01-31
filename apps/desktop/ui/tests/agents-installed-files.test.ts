// @vitest-environment happy-dom
// Show the installed files on What [agent] is made of (ScreensModule.md SM-WR-9, with SM-WR-14 and
// SM-NS-11): drawn only in a workspace that links code, and opening `installed-files:<agent instance id>`
// with the agent's name as its tab title. Drawn in a DOM, because the button appears after the person
// chooses an agent and the replies arrive.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import type { SourceLink } from '../src/lib/service-api/types.gen';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import Agents from '../src/lib/views/fixed/Agents.svelte';
import { err, ok, settle, type FakeTransport } from './support/fake-backend';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const AGENT = '0192f400-0000-7000-8000-0000000000a1';
const SHOW = 'Show the installed files';
const link = (kind: SourceLink['kind'], state: SourceLink['state']): SourceLink =>
  ({ id: 'l', workspace_id: WS, kind, state, origin_location: 'x', display_name: 'x', added_at: '2026-01-26T10:00:00.000Z' }) as SourceLink;

let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn.part);
  drawn?.target.remove();
  drawn = null;
});

async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

const buttonNamed = (root: HTMLElement, words: string): HTMLButtonElement | undefined =>
  Array.from(root.querySelectorAll('button')).find((b) => (b.textContent ?? '').replace(/[⁨⁩]/g, '').trim() === words);

/** Agents in workspace W, with the agent chosen so What [agent] is made of is open. */
async function madeOfOpen(setup: (t: FakeTransport) => void): Promise<{ app: AppContext; target: HTMLElement; transport: FakeTransport }> {
  const { app, transport } = await appWith((tr) => {
    tr.on('agents.list', ok([{ instance: { id: AGENT, display_name: 'Quote checker' }, tasks: [], uses: [], workspaces: [] }]));
    tr.on('agents.made_of', ok({ display_name: 'Quote checker', what_it_does: 'Checks supplier quotes.' }));
    setup(tr);
  });
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(Agents, { target, props: { backend: app.local, workspaceId: WS, instance: null }, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  drawn = { target, part };
  await flush();
  const agent = buttonNamed(target, 'Quote checker');
  expect(agent, 'the agent row').toBeDefined();
  agent?.click();
  await flush();
  return { app, target, transport };
}

describe('SM-WR-9 — Show the installed files', () => {
  it('Happy path — a workspace with a linked repository folder links code (SM-WR-14)', async () => {
    const { target, transport } = await madeOfOpen((tr) => tr.on('workspaces.links', ok([link('git_repository_folder', 'active')])));
    expect(transport.sentOf('workspaces.links')[0]?.input).toEqual({ workspace_id: WS });
    expect(buttonNamed(target, SHOW)).toBeDefined();
  });

  it('pressing it opens installed-files:<agent instance id> titled with the agent\'s name (SM-NS-11)', async () => {
    const { app, target } = await madeOfOpen((tr) => tr.on('workspaces.links', ok([link('git_remote', 'active')])));
    buttonNamed(target, SHOW)?.click();
    flushSync();
    const tab = app.screens.tabs(ScreenAreaState.scope(app.local.id, WS)).find((t) => t.key === `installed-files:${AGENT}`);
    expect(tab?.title).toBe('Quote checker');
  });

  it.each([
    ['only a proposed repository link', ok([link('git_remote', 'proposed')])],
    ['only links that hold no code', ok([link('local_folder', 'active'), link('jira_issue', 'active')])],
    ['no link', ok([])],
    ['an error reply to workspaces.links', err('WS_NOT_FOUND', 'That workspace wasn\'t found.')],
  ])('Edge case — %s: the button is not drawn', async (_case, reply) => {
    const { target } = await madeOfOpen((tr) => tr.on('workspaces.links', reply));
    expect(buttonNamed(target, SHOW)).toBeUndefined();
    // What [agent] is made of is open all the same.
    expect(target.textContent).toContain('Checks supplier quotes.');
  });
});
