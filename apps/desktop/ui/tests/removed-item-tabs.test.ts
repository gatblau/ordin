// @vitest-environment happy-dom
// A removed item's tabs close (ScreensModule.md SM-OS-3, amended January 2026; user decision; Approved by the
// user January 2026): whichever way an item is removed — the side tree's menu, its own screen, or a tree
// refresh that no longer lists it — every centre-pane tab open for it closes straight away. The store's rule
// first, then each kind removed through the real component over the fake backend.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import AgentsTreeGroup from '../src/lib/shell/AgentsTreeGroup.svelte';
import DashboardsTreeGroup from '../src/lib/shell/DashboardsTreeGroup.svelte';
import { itemOfTab, ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import TreeFolder from '../src/lib/shell/TreeFolder.svelte';
import WorkflowScreen from '../src/lib/views/authoring/WorkflowScreen.svelte';
import EarlierConversation from '../src/lib/views/EarlierConversation.svelte';
import EarlierConversations from '../src/lib/views/EarlierConversations.svelte';
import Knowledge from '../src/lib/views/fixed/Knowledge.svelte';
import { FakeTransport, hello, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const nameOf = (b: Element): string => clean(b.getAttribute('aria-label') ?? b.textContent);

async function flush(): Promise<void> {
  for (let i = 0; i < 5; i++) {
    await settle();
    flushSync();
  }
}

/** Presses the last button whose accessible name or words are `label`. */
async function press(label: string): Promise<void> {
  const button = [...document.querySelectorAll('button')].filter((b) => nameOf(b) === label || clean(b.textContent) === label).pop();
  expect(button, `a button named ${label}`).toBeDefined();
  button?.click();
  await flush();
}

async function drawWith(component: unknown, props: Record<string, unknown>, setup: (t: FakeTransport) => void, groups: string[] = R1_GROUPS) {
  const { app, transport } = await appWith((t) => {
    t.on('service.hello', ok(hello(LOCAL, groups)));
    setup(t);
  });
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(component as never, { target, props: { backend: app.local, workspaceId: WS, ...props } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await flush();
  return { app, transport, target };
}

/** What a `workspace_changed` event does to the store: the workspace's revision moves on and the tree reads again. */
async function workspaceChanged(app: AppContext): Promise<void> {
  app.local.workspaceRevisions.set(WS, app.local.workspaceRevision(WS) + 1);
  await flush();
}

const scope = (app: AppContext): string => ScreenAreaState.scope(app.local.id, WS);
const keys = (app: AppContext): string[] => app.screens.tabs(scope(app)).map((t) => t.key);
/** Opens Home, then the item's tabs, then a neighbour after them; the item's last tab is in front. */
function openTabs(app: AppContext, itemKeys: string[]): void {
  app.screens.open(scope(app), 'home');
  for (const key of itemKeys) app.screens.open(scope(app), key);
  app.screens.open(scope(app), 'memory', { inFront: false });
}

describe('The tabs store: closeScreensFor and closeMissing (SM-OS-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-OS-3
  it.each([
    ['dashboard', 'd1', ['dashboards:d1'], ['dashboards', 'dashboards:d2']],
    ['document', 'p1:a/b.md', ['document:p1:a/b.md'], ['document:p1:a/c.md', 'documents:p1']],
    ['document_place', 'p1', ['documents:p1', 'document:p1:a/b.md'], ['documents:p2', 'document:p2:a.md']],
    ['knowledge_page', 'k1', ['knowledge:k1'], ['knowledge', 'knowledge:k2']],
    ['workflow', 'w1', ['workflow:w1'], ['workflows', 'workflow:w2']],
    ['agent', 'a1', ['agents:a1', 'installed-files:a1', 'task-form:a1:urn:x'], ['agents:a2', 'task-form:a2:urn:x']],
    ['conversation', 'c1', ['earlier-conversation:c1'], ['earlier-conversations', 'earlier-conversation:c2']],
  ] as const)('Happy path — removing a %s closes its tabs and no other', (kind, id, gone, kept) => {
    const screens = new ScreenAreaState();
    const s = ScreenAreaState.scope('local', WS);
    for (const key of [...kept, ...gone]) screens.open(s, key);
    screens.closeScreensFor(kind, id);
    expect(screens.tabs(s).map((t) => t.key)).toEqual([...kept]);
    for (const key of gone) expect(itemOfTab(kind, key)).toBe(id);
  });

  it('Happy path — the front moves to the neighbouring tab, as Close moves it', () => {
    const screens = new ScreenAreaState();
    const s = ScreenAreaState.scope('local', WS);
    for (const key of ['home', 'dashboards:d1', 'memory']) screens.open(s, key);
    screens.show(s, 'dashboards:d1');
    screens.closeScreensFor('dashboard', 'd1');
    expect(screens.front(s)).toBe('memory');
  });

  it('Edge — the tab closes in every workspace row it is open in', () => {
    const screens = new ScreenAreaState();
    const a = ScreenAreaState.scope('local', WS);
    const b = ScreenAreaState.scope('local', 'other');
    screens.open(a, 'workflow:w1');
    screens.open(b, 'workflow:w1');
    screens.closeScreensFor('workflow', 'w1');
    expect(screens.tabs(a)).toEqual([]);
    expect(screens.tabs(b)).toEqual([]);
  });

  it('Edge — closeMissing closes only the tabs of that kind whose item is not listed, in that row only', () => {
    const screens = new ScreenAreaState();
    const a = ScreenAreaState.scope('local', WS);
    const b = ScreenAreaState.scope('local', 'other');
    for (const key of ['dashboards', 'dashboards:d1', 'dashboards:d2', 'agents:d2']) screens.open(a, key);
    screens.open(b, 'dashboards:d2');
    screens.closeMissing(a, 'dashboard', ['d1']);
    expect(screens.tabs(a).map((t) => t.key)).toEqual(['dashboards', 'dashboards:d1', 'agents:d2']);
    expect(screens.tabs(b).map((t) => t.key)).toEqual(['dashboards:d2']);
  });
});

describe('Each removable kind, removed through its component, closes its tab (SM-OS-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-OS-3
  const D = uid('dash');
  const dashboard = (id: string, name: string) => ({ id, workspace_id: WS, name, made_by: { kind: 'conversation' }, inputs: [], refresh_runs: [], kept_at: '2026-01-10T10:43:58.120Z' });

  it('Dashboard — Remove… in the side tree closes the dashboard tab; the neighbour comes to the front', async () => {
    const { app } = await drawWith(DashboardsTreeGroup, { initiallyOpen: true }, (t) =>
      t.on('dashboards.list', ok({ items: [dashboard(D, 'Prices')], next_cursor: null })).on('dashboards.remove', ok({})),
    [...R1_GROUPS, 'reports', 'runs']);
    openTabs(app, [`dashboards:${D}`]);
    await press('More actions for Prices');
    await press('Remove…');
    await press('Remove');
    expect(keys(app)).toEqual(['home', 'memory']);
    expect(app.screens.front(scope(app))).toBe('memory');
  });

  it('Dashboard — a tree refresh that no longer lists it closes its tab (removed elsewhere)', async () => {
    let listed = 0;
    const { app } = await drawWith(DashboardsTreeGroup, {}, (t) =>
      t.on('dashboards.list', (() => (listed++ === 0 ? ok({ items: [dashboard(D, 'Prices')], next_cursor: null }) : ok({ items: [], next_cursor: null }))) as never),
    [...R1_GROUPS, 'reports', 'runs']);
    await press('Dashboards');
    openTabs(app, [`dashboards:${D}`]);
    await workspaceChanged(app);
    expect(keys(app)).not.toContain(`dashboards:${D}`);
  });

  const AGENT = uid('agent');
  const installed = {
    instance: { id: AGENT, display_name: 'Supplier analyst' },
    tasks: [], task_forms: [], uses: [], workspaces: [WS],
  };

  it('Agent — Remove… in the side tree closes its page, What it’s made of and its task forms', async () => {
    let listed = 0;
    const { app } = await drawWith(AgentsTreeGroup, { initiallyOpen: true }, (t) =>
      t.on('agents.list', (() => (listed++ === 0 ? ok([installed]) : ok([]))) as never).on('agents.remove', ok({})),
    [...R1_GROUPS, 'runs']);
    openTabs(app, [`agents:${AGENT}`, `installed-files:${AGENT}`, `task-form:${AGENT}:urn:x`]);
    await press('More actions for Supplier analyst');
    await press('Remove Supplier analyst…');
    await press('Remove');
    expect(keys(app)).toEqual(['home', 'memory']);
  });

  it('Agent — a tree refresh that no longer lists it closes its tabs (removed elsewhere)', async () => {
    let listed = 0;
    const { app } = await drawWith(AgentsTreeGroup, {}, (t) => t.on('agents.list', (() => (listed++ === 0 ? ok([installed]) : ok([]))) as never), [...R1_GROUPS, 'runs']);
    await press('Agents');
    openTabs(app, [`agents:${AGENT}`]);
    await workspaceChanged(app);
    expect(keys(app)).toEqual(['home', 'memory']);
  });

  const PLACE = uid('place');
  const entry = (name: string, path: string) => ({ name, path, is_folder: false, kind: 'pdf', place_id: PLACE, size_bytes: 1, changed_at: '2026-01-10T10:00:00Z', has_versions: false });
  const DOC = entry('Plan.pdf', 'Plan.pdf');

  it('Document — Move to Bin… in the side tree closes the document tab', async () => {
    const { app } = await drawWith(TreeFolder, { placeId: PLACE }, (t) =>
      t.on('files.list_folder', ok({ items: [DOC], next_cursor: null })).on('files.move_to_bin', ok({ ticket: 't1' })),
    );
    openTabs(app, [`document:${PLACE}:Plan.pdf`]);
    await press('More actions for Plan.pdf');
    await press('Move to Bin…');
    await press('Move to Bin');
    expect(keys(app)).toEqual(['home', 'memory']);
  });

  it('Document — a folder read again without it closes its tab; a document of another folder stays', async () => {
    let listed = 0;
    const { app } = await drawWith(TreeFolder, { placeId: PLACE }, (t) =>
      t.on('files.list_folder', (() => (listed++ === 0 ? ok({ items: [DOC], next_cursor: null }) : ok({ items: [], next_cursor: null }))) as never),
    );
    openTabs(app, [`document:${PLACE}:Plan.pdf`, `document:${PLACE}:Q3/Other.pdf`]);
    await workspaceChanged(app);
    expect(listed).toBe(2);
    expect(keys(app)).toEqual(['home', `document:${PLACE}:Q3/Other.pdf`, 'memory']);
  });

  const PAGE = uid('page');
  const view = {
    page: { id: PAGE, title: 'Manchester overview', body_markdown: 'A short page.', last_changed_at: '2026-01-12T10:00:00Z' },
    relative_path: 'knowledge/Manchester overview.md',
    aliases: [], tags: [], links_in: [], links_out: [], links_truncated: 0, suggestions: [], unregistered_aliases: [], changes: [],
    cites_other_computer: false, read_only: false, past_stale_after: false, pending_update: false,
  };
  const listing = (titles: string[]) => ({
    this_workspace: titles.map((title) => ({ id: PAGE, title, tags: [], last_changed_at: '2026-01-12T10:00:00Z' })),
    from_other_teams: [], from_agents: [], not_adopted: [],
  });

  it('Knowledge page — Remove… on the page closes its tab', async () => {
    let listed = 0;
    const { app } = await drawWith(Knowledge, { instance: PAGE }, (t) =>
      t
        .on('knowledge.list', (() => (listed++ === 0 ? ok(listing(['Manchester overview'])) : ok(listing([])))) as never)
        .on('knowledge.get', ok(view))
        .on('knowledge.remove', ok({})),
    );
    openTabs(app, [`knowledge:${PAGE}`]);
    await press('Remove…');
    await press('Remove');
    expect(keys(app)).toEqual(['home', 'memory']);
  });

  const WF = '0192f400-0000-7000-8000-000000000001';
  const wfView = {
    agents: [],
    file: {
      agents: [], format: 1, inputs: [],
      limits: { agents_at_once: 2, external_action_agents: [], failure_repeat_limit: 2, max_rounds: 5, wall_clock_minutes: 30 },
      name: 'Supplier rates refresh', origin: 'Written in this workspace', purpose: 'Refresh the rates', steps: [],
    },
    version: 1,
    versions: [{ version: 1, saved_at: '2026-01-08T10:00:00Z', change_note: 'First' }],
    workflow: { id: WF, name: 'Supplier rates refresh', origin: 'own', state: 'saved', workspace_id: WS, created_at: '2026-01-06T10:00:00Z' },
  };

  it('Workflow — Remove… on its screen closes the workflow tab', async () => {
    const { app, transport } = await drawWith(WorkflowScreen, { instance: WF }, (t) => t.on('workflows.get', ok(wfView)).on('workflows.remove', ok({})));
    openTabs(app, [`workflow:${WF}`]);
    await press('More actions');
    await press('Remove…');
    await press('Remove…');
    expect(transport.sentOf('workflows.remove')).toHaveLength(1);
    expect(keys(app)).toEqual(['home', 'memory']);
  });

  const A = '0192a1c2-3344-7556-8677-8899aabbccd1';
  const B = '0192a1c2-3344-7556-8677-8899aabbccd2';
  const conv = (id: string, first: string) => ({ conversation_id: id, first_request: first, message_count: 2, started_at: '2026-01-10T10:00:00Z', state: 'earlier' });
  const convSetup = (t: FakeTransport) => {
    let listed = 0;
    t.on('ordin.list_conversations', (() => (listed++ === 0 ? ok({ items: [conv(A, 'Q3 supplier rates'), conv(B, 'Account brief')], next_cursor: null }) : ok({ items: [conv(B, 'Account brief')], next_cursor: null }))) as never)
      .on('ordin.read_conversation', ok({ items: [], next_cursor: null }))
      .on('ordin.delete_conversation', ok({}));
  };

  it('Conversation — Delete… on the Earlier conversations list closes its read-only tab', async () => {
    const { app } = await drawWith(EarlierConversations, { instance: null }, convSetup);
    openTabs(app, [`earlier-conversation:${A}`]);
    await press('More actions for Q3 supplier rates');
    await press('Delete…');
    await press('Delete');
    expect(keys(app)).toEqual(['home', 'memory']);
  });

  it('Conversation — Delete… on its own screen closes its tab', async () => {
    const { app } = await drawWith(EarlierConversation, { instance: A }, convSetup);
    openTabs(app, [`earlier-conversation:${A}`]);
    await press('Delete…');
    await press('Delete');
    expect(keys(app)).toEqual(['home', 'memory']);
  });
});
