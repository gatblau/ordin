// @vitest-environment happy-dom
// Remove an agent, accept or decline its update, and no Withdraw… that cannot open (ScreensModule.md
// SM-AG-1 to SM-AG-4). Drawn in a DOM: the section appears after the person chooses an agent.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { startApp } from '../src/lib/app/bootstrap';
import { i18n } from '../src/lib/i18n/fluent';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import Agents from '../src/lib/views/fixed/Agents.svelte';
import RepositoryParts from '../src/lib/views/authoring/RepositoryParts.svelte';
import { err, FakeTransport, ok, R1_GROUPS, settle } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const AGENT = '0192f400-0000-7000-8000-0000000000a1';
const REPO = uid('repo-1');

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

const words = (s: string | null): string => (s ?? '').replace(/[⁨⁩]/g, '').trim();
const buttonNamed = (root: ParentNode, name: string): HTMLButtonElement | undefined =>
  Array.from(root.querySelectorAll('button')).find((b) => words(b.textContent) === name);

async function appOn(groups: string[], setup: (tr: FakeTransport) => void): Promise<{ app: AppContext; transport: FakeTransport }> {
  const transport = new FakeTransport({ localGroups: groups });
  setup(transport);
  const data = new Map<string, string>();
  const storage = { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
  const app = await startApp(transport, { document: null, storage, i18n });
  return { app, transport };
}

const installed = (update?: unknown) => ({
  instance: { id: AGENT, display_name: 'Quote checker' },
  tasks: [],
  uses: [],
  workspaces: [],
  ...(update === undefined ? {} : { update }),
});

async function agentChosen(options: { groups?: string[]; agents?: () => unknown[]; setup?: (tr: FakeTransport) => void }): Promise<{ app: AppContext; target: HTMLElement; transport: FakeTransport }> {
  const { app, transport } = await appOn(options.groups ?? R1_GROUPS, (tr) => {
    tr.on('agents.list', () => ok((options.agents ?? (() => [installed()]))()));
    tr.on('agents.made_of', ok({ display_name: 'Quote checker', what_it_does: 'Checks supplier quotes.' }));
    options.setup?.(tr);
  });
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(Agents, { target, props: { backend: app.local, workspaceId: WS, instance: null }, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  drawn = { target, part };
  await flush();
  buttonNamed(target, 'Quote checker')?.click();
  await flush();
  return { app, target, transport };
}

const inDialog = (): HTMLElement => document.querySelector('[role="dialog"]') as HTMLElement;

describe('SM-AG-1 — Remove an agent from the workspace', () => {
  it('Happy path — confirmed: agents.remove is sent with the agent and the workspace, the list is read again and the agent is gone', async () => {
    let agents = [installed()];
    const { target, transport } = await agentChosen({ agents: () => agents, setup: (tr) => tr.on('agents.remove', () => { agents = []; return ok({}); }) });
    buttonNamed(target, 'Remove Quote checker…')?.click();
    await flush();
    expect(words(inDialog().textContent)).toContain('Remove Quote checker from this workspace?');
    buttonNamed(inDialog(), 'Remove')?.click();
    await flush();
    expect(transport.sentOf('agents.remove')[0]?.input).toEqual({ agent_instance_id: AGENT, workspace_id: WS });
    expect(buttonNamed(target, 'Quote checker')).toBeUndefined();
    expect(target.textContent).not.toContain('What Quote checker is made of');
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it('Edge case — cancelled: nothing is sent and the agent stays', async () => {
    const { target, transport } = await agentChosen({});
    buttonNamed(target, 'Remove Quote checker…')?.click();
    await flush();
    buttonNamed(inDialog(), 'Cancel')?.click();
    await flush();
    expect(transport.sentOf('agents.remove')).toHaveLength(0);
    expect(buttonNamed(target, 'Quote checker')).toBeDefined();
  });

  it('Edge case — Remove is not the default button', async () => {
    const { target } = await agentChosen({});
    buttonNamed(target, 'Remove Quote checker…')?.click();
    await flush();
    expect(buttonNamed(inDialog(), 'Cancel')?.getAttribute('data-default')).toBe('true');
    expect(buttonNamed(inDialog(), 'Remove')?.getAttribute('data-default')).toBeNull();
  });

  it('Error path — a refusal is drawn inside the dialog, which stays open, and the agent stays', async () => {
    const { target } = await agentChosen({ setup: (tr) => tr.on('agents.remove', err('PKG_BUNDLED_AGENT_FIXED', 'This agent comes with ordin and can\'t be changed or removed.')) });
    buttonNamed(target, 'Remove Quote checker…')?.click();
    await flush();
    buttonNamed(inDialog(), 'Remove')?.click();
    await flush();
    expect(inDialog()).not.toBeNull();
    expect(words(inDialog().textContent)).toContain('This agent comes with ordin and can\'t be changed or removed.');
    expect(buttonNamed(target, 'Quote checker')).toBeDefined();
  });
});

describe('SM-AG-2, SM-AG-3 — An update the person can accept or decline', () => {
  const pending = { to_commit: 'abc', may_do_changes: [{ kind: 'tool_added', message_id: 'may-do-tool-added', name: 'web_search' }, { kind: 'policy_changed', message_id: 'may-do-nonsense', name: '' }] };

  it('the change is drawn in words, and an id the file lacks reads as the general sentence', async () => {
    const { target } = await agentChosen({ agents: () => [installed(pending)] });
    const text = words(target.textContent);
    expect(text).toContain('A new version of Quote checker is ready.');
    expect(text).toContain('It can now use web_search.');
    expect(text).toContain('Something about what it may do has changed.');
    expect(text).not.toContain('may-do-nonsense');
  });

  it('Happy path — Update sends agents.accept_update and the update block is gone', async () => {
    const { target, transport } = await agentChosen({ agents: () => [installed(pending)], setup: (tr) => tr.on('agents.accept_update', ok(installed())) });
    buttonNamed(target, 'Update')?.click();
    await flush();
    expect(transport.sentOf('agents.accept_update')[0]?.input).toEqual({ agent_instance_id: AGENT });
    expect(buttonNamed(target, 'Update')).toBeUndefined();
    expect(target.textContent).not.toContain('A new version');
  });

  it('Happy path — Not now sends agents.decline_update and the update block is gone', async () => {
    const { target, transport } = await agentChosen({ agents: () => [installed(pending)], setup: (tr) => tr.on('agents.decline_update', ok(installed())) });
    buttonNamed(target, 'Not now')?.click();
    await flush();
    expect(transport.sentOf('agents.decline_update')[0]?.input).toEqual({ agent_instance_id: AGENT });
    expect(buttonNamed(target, 'Not now')).toBeUndefined();
  });

  it('Error path — an update that is no longer there: the error is drawn and the block stays', async () => {
    const { target } = await agentChosen({ agents: () => [installed(pending)], setup: (tr) => tr.on('agents.accept_update', err('PKG_NO_UPDATE', 'There is no new version to accept.')) });
    buttonNamed(target, 'Update')?.click();
    await flush();
    expect(words(target.textContent)).toContain('There is no new version to accept.');
    expect(buttonNamed(target, 'Update')).toBeDefined();
  });

  it('Edge case — an agent with no update draws no update block', async () => {
    const { target } = await agentChosen({});
    expect(buttonNamed(target, 'Update')).toBeUndefined();
    expect(buttonNamed(target, 'Not now')).toBeUndefined();
  });
});

describe('SM-AG-4 — No Withdraw… the backend cannot open', () => {
  const part = { id: uid('p1'), name: 'Reading notice periods', kind: 'Skill', agent_repository_id: REPO, status: 'active', withdrawn: false };
  const view = { from: 'Company', offered: true, state_label: 'current', summary: 'About it', part };
  const repo = { agent_count: 3, set_by_company: false, repository: { id: REPO, display_name: 'Company agents', format: 'ordin', person_can_publish: true } };

  async function partsOpen(groups: string[]): Promise<HTMLElement> {
    const { app } = await appOn(groups, (tr) => {
      tr.on('catalogue.list_repositories', ok([repo])).on('catalogue.search', ok({ items: [{ from: 'Company', part: view }], next_cursor: null }));
    });
    const target = document.createElement('div');
    document.body.appendChild(target);
    const handle = mount(RepositoryParts, { target, props: { backend: app.local, workspaceId: WS, instance: REPO }, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    drawn = { target, part: handle };
    await flush();
    buttonNamed(target, 'Reading notice periods')?.click();
    await flush();
    return target;
  }

  it('Edge case — groups without authoring: neither Withdraw… nor More is drawn', async () => {
    const target = await partsOpen(R1_GROUPS);
    expect(buttonNamed(target, 'Reading notice periods')).toBeDefined();
    expect(buttonNamed(target, 'Withdraw…')).toBeUndefined();
    expect(target.textContent).not.toContain('More');
  });

  it('Happy path — a backend that answers authoring draws Withdraw… as before', async () => {
    const target = await partsOpen([...R1_GROUPS, 'authoring']);
    expect(buttonNamed(target, 'Withdraw…')).toBeDefined();
  });
});
