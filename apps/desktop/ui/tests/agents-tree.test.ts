// @vitest-environment happy-dom
// The Agents row of the tree: agents, their tasks and their menus (SM-AT-1), mounted over the fake transport.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { APP_KEY } from '../src/lib/shell/context';
import AgentsTreeGroup from '../src/lib/shell/AgentsTreeGroup.svelte';
import { hello, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
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

const WS = uid('ws');
const AGENT = uid('agent');
const text = (el: Element | null): string => (el?.textContent ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const form = (command_id: string, title: string) => ({ command_id, title, description: '', inputs: null, argument_label: null, startable: true });
const installed = {
  instance: { id: AGENT, display_name: 'Supplier analyst' },
  tasks: ['/compare-quotes', '/summarise-rates'],
  task_forms: [form('urn:ordin:acme:commands:compare-quotes', 'Compare quotes'), form('urn:ordin:acme:commands:summarise-rates', 'Summarise rates')],
  uses: [],
  workspaces: [WS],
};

async function draw(initiallyOpen = true, groups: string[] = [...R1_GROUPS, 'runs']) {
  const { app, transport } = await appWith((t) => t.on('service.hello', ok(hello(LOCAL, groups))).on('agents.list', ok([installed])).on('agents.remove', ok({})));
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(AgentsTreeGroup as never, { target, props: { backend: app.local, workspaceId: WS, initiallyOpen } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await flush();
  return { app, target, transport };
}
async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}
const button = (target: HTMLElement, label: string): HTMLElement => {
  const found = [...target.querySelectorAll('button')].find((b) => text(b).includes(label) || (b.getAttribute('aria-label') ?? '').replace(/[⁨⁩]/g, '').includes(label));
  expect(found, `a button named ${label}`).toBeDefined();
  return found as HTMLElement;
};

describe('Agents in the tree (SM-AT-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-AT-1
  // spec: ordin-desktop/components/ScreensModule#SM-AT-2
  it('Happy path — an open Agents row lists each agent; pressing an agent shows its tasks by title; a task opens its form', async () => {
    const { app, target, transport } = await draw();
    expect(text(target)).toContain('Supplier analyst');
    expect(text(target)).not.toContain('Compare quotes');
    button(target, 'Supplier analyst').click();
    await flush();
    expect(text(target)).toContain('Compare quotes');
    transport.sent.length = 0;
    button(target, 'Compare quotes').click();
    await flush();
    const scope = ScreenAreaState.scope(LOCAL, WS);
    expect(app.screens.front(scope)).toBe(`task-form:${AGENT}:urn:ordin:acme:commands:compare-quotes`);
    expect(app.screens.tabs(scope).find((t) => t.key === app.screens.front(scope))?.title).toBe('Compare quotes');
    expect(transport.commandsSent()).toEqual([]);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-AT-2
  it('Edge — a backend without the command draws each task row as plain text', async () => {
    const { target } = await draw(true, R1_GROUPS);
    button(target, 'Supplier analyst').click();
    await flush();
    expect(text(target)).toContain('Compare quotes');
    expect([...target.querySelectorAll('button')].some((b) => text(b) === 'Compare quotes')).toBe(false);
  });

  it('Happy path — the group menu offers the library; the agent menu offers What it is made of and Remove', async () => {
    const { target } = await draw();
    button(target, 'More actions for Agents').click();
    await flush();
    expect(text(target)).toContain('Browse the agent library');
    button(target, 'More actions for Supplier analyst').click();
    await flush();
    expect(text(target)).toContain("What it's made of");
    expect(text(target)).toContain('Remove Supplier analyst…');
  });

  it('Happy path — Remove asks first, then sends agents.remove and reads the list again', async () => {
    const { target, transport } = await draw();
    button(target, 'More actions for Supplier analyst').click();
    await flush();
    button(target, 'Remove Supplier analyst…').click();
    await flush();
    expect(transport.commandsSent()).not.toContain('agents.remove');
    expect(text(document.body)).toContain('Remove Supplier analyst from this workspace?');
    button(document.body, 'Remove').click();
    await flush();
    expect(transport.sentOf('agents.remove')).toHaveLength(1);
    expect(transport.sentOf('agents.list').length).toBeGreaterThanOrEqual(2);
  });
});
