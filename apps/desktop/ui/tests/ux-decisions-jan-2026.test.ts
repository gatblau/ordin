// @vitest-environment happy-dom
// The four user decisions of January 2026: Publish to… in a document's menu (SM-PUB-1, SM-PUB-2), the
// Documents overview (SM-DS-4 to SM-DS-6), the page of one agent (SM-AG-6 to SM-AG-8) and keeping a report
// as a dashboard (SM-DB-1 to SM-DB-4). Pure rules first, then the models over the fake backend, then the
// components mounted so their reads and sends run.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { defaultName, keepAsDashboard, matchReport, sourceOf } from '../src/lib/blocks/keep-dashboard';
import { AgentPage, tasksOf } from '../src/lib/views/fixed/agent-page.svelte';
import { DashboardPage, newestReportOf } from '../src/lib/views/fixed/dashboard-page.svelte';
import { DocumentsOverview, RECENT_LIMIT } from '../src/lib/views/fixed/documents-overview.svelte';
import { canHold, copyRequest, formatFor, isRepositoryPlace, publishDestinations, type FoundPlace } from '../src/lib/views/publish/destinations';
import DocumentMenu from '../src/lib/views/publish/DocumentMenu.svelte';
import PublishToDialog from '../src/lib/views/publish/PublishToDialog.svelte';
import AgentsTreeGroup from '../src/lib/shell/AgentsTreeGroup.svelte';
import DashboardsTreeGroup from '../src/lib/shell/DashboardsTreeGroup.svelte';
import TreeFolder from '../src/lib/shell/TreeFolder.svelte';
import { backendWith, WS } from './fixed-support';
import { err, ok, R1_GROUPS, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const AT = '2026-01-10T10:43:58.120Z';
const PLACE = uid('place');
const PLACE2 = uid('place2');
const PUBLISH_GROUPS = [...R1_GROUPS, 'push', 'confluence', 'sharepoint', 'reports', 'runs'];
const words = (el: Element): string => (el.textContent ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');
const has = (groups: string[]) => ({ groups: new Set(groups) });

const link = (kind: string, place: string | null, name: string, state = 'active') =>
  ({ added_at: AT, display_name: name, id: uid(`l-${name}`), kind, origin_location: name, place_id: place, state, workspace_id: WS }) as never;
const entry = (path: string, changed: string, by: string | null = null, folder = false) => ({
  changed_at: changed,
  changed_by: by,
  has_versions: true,
  is_folder: folder,
  kind: folder ? '' : (path.split('.').pop() ?? ''),
  name: path.replace(/\/$/, '').split('/').pop() ?? path,
  path,
  place_id: PLACE,
  size_bytes: 10,
});
const pageLocation = { space_key: 'PROC', space_name: 'Procurement', page_id: '4000' as string | null, title: 'Supplier pages', web_url: 'https://example.invalid/4000' };
const folderLocation = { drive_id: 'drive-1', drive_name: 'Documents', is_folder: true, item_id: 'item-9' as string | null, path: 'Quotes', site_id: 'site-1', site_name: 'Finance', web_url: 'https://example.invalid/q' };
const confluencePage: FoundPlace = { kind: 'confluence', location: pageLocation };
const sharepointFolder: FoundPlace = { kind: 'sharepoint', location: folderLocation };

async function mountIn(component: unknown, props: Record<string, unknown>): Promise<HTMLElement> {
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(component as never, { target, props: props as never });
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
  return target;
}

const press = async (el: Element | null | undefined): Promise<void> => {
  (el as HTMLElement | null)?.click();
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
};
const buttonNamed = (root: ParentNode, text: string): HTMLButtonElement | undefined =>
  Array.from(root.querySelectorAll('button')).find((b) => (b.textContent ?? '').replace(/[⁨⁩]/g, '').trim() === text);

// ---------------------------------------------------------------------------------------------- Publish to…

describe('Publish to… destinations (SM-PUB-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-PUB-1
  const links = [link('confluence_location', uid('pc'), 'Procurement'), link('sharepoint_location', uid('ps'), 'Finance')];

  it('Happy path — every connected destination, in a fixed order', () => {
    expect(publishDestinations(has(PUBLISH_GROUPS), links, 'notes.md', false)).toEqual(['confluence', 'sharepoint']);
  });

  it('Edge case — no connected destination, a proposed link, or a repository document offers nothing', () => {
    expect(publishDestinations(has(PUBLISH_GROUPS), [], 'notes.md', false)).toEqual([]);
    expect(publishDestinations(has(PUBLISH_GROUPS), [link('confluence_location', null, 'P', 'proposed')], 'notes.md', false)).toEqual([]);
    expect(publishDestinations(has(PUBLISH_GROUPS), links, 'notes.md', true)).toEqual([]);
  });

  it('Edge case — a file that is not Markdown goes only to Confluence, as an attachment', () => {
    expect(publishDestinations(has(PUBLISH_GROUPS), links, 'quote.xlsx', false)).toEqual(['confluence']);
    expect(formatFor('confluence', 'quote.xlsx')).toBe('exact_file');
    expect(formatFor('sharepoint', 'quote.xlsx')).toBeNull();
    expect(formatFor('sharepoint', 'Notes.MD')).toBe('word');
  });

  it('Error path — a backend that does not answer the commands offers nothing (SM-UP-1)', () => {
    expect(publishDestinations(has(R1_GROUPS), links, 'notes.md', false)).toEqual([]);
    expect(publishDestinations(has([...R1_GROUPS, 'push', 'confluence']), links, 'notes.md', false)).toEqual(['confluence']);
  });

  it('a repository place is known by its link', () => {
    expect(isRepositoryPlace([link('git_repository_folder', PLACE, 'Billing')], PLACE)).toBe(true);
    expect(isRepositoryPlace([link('local_folder', PLACE, 'Mine')], PLACE)).toBe(false);
  });
});

describe('The copy request (SM-PUB-2)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-PUB-2
  const base = { workspaceId: WS, placeId: PLACE, path: 'Pricing/notes.md', name: 'notes.md', version: 4 };

  it('Happy path — a Markdown document becomes a Confluence page under the page found', () => {
    const r = copyRequest({ ...base, found: confluencePage });
    expect(r).toMatchObject({
      original: { kind: 'place_file', place_id: PLACE, path: 'Pricing/notes.md' },
      original_version: '4',
      destination: { kind: 'confluence_page', space: 'PROC', parent_page_id: '4000' },
      format: 'page',
      keep_up_to_date: false,
      file_name: 'notes.md',
    });
  });

  it('Edge case — another file becomes an attachment; SharePoint takes a Word copy in the folder found', () => {
    expect(copyRequest({ ...base, name: 'q.xlsx', path: 'q.xlsx', found: confluencePage })?.destination.kind).toBe('confluence_attachment');
    expect(copyRequest({ ...base, found: sharepointFolder })).toMatchObject({ destination: { kind: 'sharepoint_file', site: 'Finance', library: 'drive-1', folder: 'item-9' }, format: 'word' });
  });

  it('Error path — a whole space, a library root or a file cannot hold a copy', () => {
    const space: FoundPlace = { kind: 'confluence', location: { ...pageLocation, page_id: null } };
    const root: FoundPlace = { kind: 'sharepoint', location: { ...folderLocation, item_id: null } };
    expect(canHold(space)).toBe(false);
    expect(canHold(root)).toBe(false);
    expect(copyRequest({ ...base, found: space })).toBeNull();
    expect(copyRequest({ ...base, name: 'q.xlsx', found: sharepointFolder })).toBeNull();
  });
});

describe('A document’s ⋯ menu and the Publish to dialog (SM-PUB-1, SM-PUB-2)', () => {
  it('Happy path — the menu lists Publish to… and, opened, each connected destination', async () => {
    const b = await backendWith({ groups: PUBLISH_GROUPS, setup: (t) => t.on('workspaces.links', ok([link('confluence_location', uid('pc'), 'Procurement')])) });
    const target = await mountIn(DocumentMenu, { backend: b.state, workspaceId: WS, placeId: PLACE, path: 'notes.md', name: 'notes.md', onopen: () => {} });
    await press(target.querySelector('button[aria-haspopup="menu"]'));
    expect(words(target)).toContain('Open');
    await press(buttonNamed(target, 'Publish to…'));
    expect(buttonNamed(target, 'Confluence…')).toBeDefined();
    expect(buttonNamed(target, 'SharePoint…')).toBeUndefined();
  });

  it('Edge case — with no destination connected the menu has no Publish to…', async () => {
    const b = await backendWith({ groups: PUBLISH_GROUPS, setup: (t) => t.on('workspaces.links', ok([link('local_folder', PLACE, 'Mine')])) });
    const target = await mountIn(DocumentMenu, { backend: b.state, workspaceId: WS, placeId: PLACE, path: 'notes.md', name: 'notes.md', onopen: () => {} });
    await press(target.querySelector('button[aria-haspopup="menu"]'));
    expect(words(target)).toContain('Open');
    expect(words(target)).not.toContain('Publish to…');
  });

  it('Happy path — find a page, choose it, Publish sends push.publish_copy at the newest version', async () => {
    let closed: boolean | null = null;
    const b = await backendWith({
      groups: PUBLISH_GROUPS,
      setup: (t) =>
        t
          .on('confluence.find', ok([pageLocation]))
          .on('files.versions', ok([{ version: 2 }, { version: 5 }, { version: 3 }]))
          .on('push.publish_copy', ok(uid('copy'))),
    });
    const target = await mountIn(PublishToDialog, { backend: b.state, workspaceId: WS, placeId: PLACE, path: 'notes.md', name: 'notes.md', system: 'confluence', onclose: (sent: boolean) => (closed = sent) });
    const input = target.querySelector('input[type="search"]') as HTMLInputElement;
    input.value = 'supplier';
    input.dispatchEvent(new Event('input'));
    flushSync();
    target.querySelector('form')?.dispatchEvent(new Event('submit', { cancelable: true }));
    for (let i = 0; i < 4; i++) {
      await settle();
      flushSync();
    }
    expect(words(target)).toContain('Procurement');
    await press(target.querySelector('input[type="radio"]'));
    await press(buttonNamed(target, 'Publish'));
    const sent = b.transport.sentOf('push.publish_copy').map((e) => e.input) as Record<string, unknown>[];
    expect(sent).toHaveLength(1);
    expect(sent[0]).toMatchObject({ original_version: '5', format: 'page', destination: { kind: 'confluence_page', parent_page_id: '4000' } });
    expect(closed).toBe(true);
  });

  it('Error path — Publish with nothing chosen asks for a place; a refusal stays in the dialog', async () => {
    let closed: boolean | null = null;
    const b = await backendWith({
      groups: PUBLISH_GROUPS,
      setup: (t) =>
        t
          .on('confluence.find', ok([pageLocation]))
          .on('files.versions', ok([{ version: 1 }]))
          .on('push.publish_copy', err('PUSH_DESTINATION_REFUSED', 'Confluence refused the copy.')),
    });
    const target = await mountIn(PublishToDialog, { backend: b.state, workspaceId: WS, placeId: PLACE, path: 'notes.md', name: 'notes.md', system: 'confluence', onclose: (sent: boolean) => (closed = sent) });
    await press(buttonNamed(target, 'Publish'));
    expect(words(target)).toContain('Choose a place in Confluence first.');
    expect(b.transport.sentOf('push.publish_copy')).toHaveLength(0);
    const input = target.querySelector('input[type="search"]') as HTMLInputElement;
    input.value = 'supplier';
    input.dispatchEvent(new Event('input'));
    flushSync();
    target.querySelector('form')?.dispatchEvent(new Event('submit', { cancelable: true }));
    for (let i = 0; i < 4; i++) {
      await settle();
      flushSync();
    }
    await press(target.querySelector('input[type="radio"]'));
    await press(buttonNamed(target, 'Publish'));
    expect(b.transport.sentOf('push.publish_copy')).toHaveLength(1);
    expect(closed).toBeNull();
    expect(target.querySelector('[role="dialog"]')).not.toBeNull();
  });
});

// -------------------------------------------------------------------------------------- Documents overview

describe('The Documents overview (SM-DS-4, SM-DS-5)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-DS-4
  // spec: ordin-desktop/components/ScreensModule#SM-DS-5
  const links = [link('local_folder', PLACE, 'My notes'), link('sharepoint_location', PLACE2, 'Finance'), link('jira_issue', null, 'PROJ-1')];

  it('Happy path — the newest documents across places, with where, when and by whom', async () => {
    const b = await backendWith({
      setup: (t) =>
        t.on('workspaces.links', ok(links)).on('files.list_folder', (input) => {
          const place = (input as { place_id: string }).place_id;
          return place === PLACE
            ? ok({ items: [entry('old.md', '2026-01-01T09:00:00Z', 'you'), entry('Sub/', '2026-01-09T09:00:00Z', null, true)], next_cursor: null })
            : ok({ items: [{ ...entry('quote.xlsx', '2026-01-08T09:00:00Z', 'Ordin'), place_id: PLACE2 }], next_cursor: null });
        }),
    });
    const o = new DocumentsOverview(b.handle, WS);
    await o.load();
    expect(o.rows.map((r) => [r.name, r.place, r.changedBy])).toEqual([
      ['quote.xlsx', 'Finance', 'Ordin'],
      ['old.md', 'My notes', 'you'],
    ]);
    expect(b.transport.sentOf('files.list_folder').map((e) => (e.input as { sort: string }).sort)).toEqual(['changed_desc', 'changed_desc']);
  });

  it('Edge case — at most RECENT_LIMIT rows; no place linked leaves no rows', async () => {
    const many = Array.from({ length: RECENT_LIMIT + 5 }, (_, i) => entry(`d${i}.md`, `2026-01-${String((i % 28) + 1).padStart(2, '0')}T09:00:00Z`));
    const b = await backendWith({ setup: (t) => t.on('workspaces.links', ok([links[0]])).on('files.list_folder', ok({ items: many, next_cursor: null })) });
    const o = new DocumentsOverview(b.handle, WS);
    await o.load();
    expect(o.rows).toHaveLength(RECENT_LIMIT);
    const none = await backendWith({ setup: (t) => t.on('workspaces.links', ok([])) });
    const empty = new DocumentsOverview(none.handle, WS);
    await empty.load();
    expect(empty.places).toEqual([]);
    expect(empty.rows).toEqual([]);
  });

  it('Happy path — a search replaces the list with hits over the same places; a short text goes back', async () => {
    const b = await backendWith({
      setup: (t) =>
        t
          .on('workspaces.links', ok(links))
          .on('files.list_folder', ok({ items: [], next_cursor: null }))
          .on('files.search', ok({ coverage: { indexed: 2, total: 2 }, hits: [{ changed_at: AT, name_match: true, path: 'Q/quote.xlsx', place_id: PLACE2 }] })),
    });
    const o = new DocumentsOverview(b.handle, WS);
    await o.load();
    await o.search('quote');
    expect(o.searched).toBe('quote');
    expect(o.rows).toEqual([{ placeId: PLACE2, path: 'Q/quote.xlsx', name: 'quote.xlsx', place: 'Finance', changedAt: AT, changedBy: null }]);
    expect((b.transport.sentOf('files.search')[0]?.input as { place_ids: string[] }).place_ids).toEqual([PLACE, PLACE2]);
    await o.search('q');
    expect(o.searched).toBe('');
  });

  it('Error path — a place that cannot be read shows its error and leaves the others', async () => {
    const b = await backendWith({
      setup: (t) =>
        t.on('workspaces.links', ok(links)).on('files.list_folder', (input) =>
          (input as { place_id: string }).place_id === PLACE ? err('FILES_PLACE_UNAVAILABLE', 'That folder can’t be read now.') : ok({ items: [{ ...entry('quote.xlsx', AT), place_id: PLACE2 }], next_cursor: null }),
        ),
    });
    const o = new DocumentsOverview(b.handle, WS);
    await o.load();
    expect(o.error?.code).toBe('FILES_PLACE_UNAVAILABLE');
    expect(o.rows.map((r) => r.name)).toEqual(['quote.xlsx']);
  });
});

// ------------------------------------------------------------------------------------------ One agent's page

const AGENT = uid('agent');
const run = (id: string, agent: string, queued: string, state = 'completed') => ({
  id,
  agent: { kind: 'agent_instance', agent_instance_id: agent },
  queued_at: queued,
  state,
  origin: { kind: 'task', task_id: uid('t') },
  inputs_used: [],
});

describe('The page of one agent (SM-AG-6, SM-AG-7)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-AG-7
  it('Happy path — its own recent tasks, newest first, at most five', () => {
    const runs = [run('a', AGENT, '2026-01-01T00:00:00Z'), run('b', uid('other'), '2026-01-05T00:00:00Z'), ...[2, 3, 4, 5, 6, 7].map((d) => run(`r${d}`, AGENT, `2026-01-0${d}T00:00:00Z`))];
    expect(tasksOf(runs as never, AGENT).map((r) => r.id)).toEqual(['r7', 'r6', 'r5', 'r4', 'r3']);
  });

  it('Happy path — the page reads the agent, what it does and its tasks', async () => {
    const installed = { instance: { id: AGENT, display_name: 'Document agent' }, tasks: [], uses: [{ changeable: false, chosen_by: 'maker', identifier: 'x', kind: 'Skill', label: 'Summaries' }], workspaces: [WS] };
    const b = await backendWith({
      setup: (t) =>
        t
          .on('agents.get', ok(installed))
          .on('agents.made_of', ok({ display_name: 'Document agent', groups: [], part_count: 1, tasks: [], what_it_does: 'Writes and checks documents.' }))
          .on('runs.list', ok({ items: [run('r1', AGENT, AT)], next_cursor: null })),
    });
    const page = new AgentPage(b.handle, WS, AGENT);
    await page.load();
    expect(page.agent?.instance.display_name).toBe('Document agent');
    expect(page.madeOf?.what_it_does).toBe('Writes and checks documents.');
    expect(page.tasks.map((r) => r.id)).toEqual(['r1']);
    expect(page.error).toBeNull();
  });

  it('Edge case — a backend without runs lists no tasks and sends no runs.list', async () => {
    const b = await backendWith({ setup: (t) => t.on('agents.get', ok({ instance: { id: AGENT, display_name: 'A' }, tasks: [], uses: [], workspaces: [] })).on('agents.made_of', ok({ display_name: 'A', groups: [], part_count: 0, tasks: [], what_it_does: '' })) });
    const page = new AgentPage(b.handle, WS, AGENT, false);
    await page.load();
    expect(page.tasks).toEqual([]);
    expect(b.transport.sentOf('runs.list')).toHaveLength(0);
  });

  it('Error path — an agent no longer in the workspace shows its error', async () => {
    const b = await backendWith({ setup: (t) => t.on('agents.get', err('PKG_AGENT_UNKNOWN', 'That agent isn’t here any more.')).on('agents.made_of', ok({})).on('runs.list', ok({ items: [], next_cursor: null })) });
    const page = new AgentPage(b.handle, WS, AGENT);
    await page.load();
    expect(page.agent).toBeNull();
    expect(page.error?.code).toBe('PKG_AGENT_UNKNOWN');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-AG-6
  it('Happy path — the tree’s Agents row lists the agents and opens no screen of its own', async () => {
    const b = await backendWith({ setup: (t) => t.on('agents.list', ok([{ instance: { id: AGENT, display_name: 'Document agent' }, tasks: [], uses: [], workspaces: [] }])) });
    const target = await mountIn(AgentsTreeGroup, { backend: b.state, workspaceId: WS });
    expect(words(target)).not.toContain('Document agent');
    await press(target.querySelector('button.tree-row'));
    expect(words(target)).toContain('Document agent');
  });
});

// ------------------------------------------------------------------------------------------------ Dashboards

const RUN = uid('run');
const REP1 = uid('rep1');
const report = (id: string, runId: string, title = 'Supplier prices', asOf = AT) => ({ id, run_id: runId, title, as_of: asOf });

describe('Keep as dashboard… (SM-DB-1, SM-DB-2)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-DB-1
  // spec: ordin-desktop/components/ScreensModule#SM-DB-2
  it('pure rules — the matching report, the source of each origin, the default name', () => {
    expect(matchReport([report('x', RUN, 'Other'), report('y', RUN)], 'Supplier prices', '2026-01-10T10:43:58.12Z')?.id).toBe('y');
    expect(matchReport([report('y', RUN)], 'Supplier prices', '2026-01-11T00:00:00Z')).toBeNull();
    expect(sourceOf({ kind: 'conversation', workspace_id: WS })).toEqual({ kind: 'conversation' });
    expect(sourceOf({ kind: 'task', task_id: 'T' })).toEqual({ kind: 'task', task_id: 'T' });
    expect(sourceOf({ kind: 'crew_round', crew_run_id: 'C', round_number: 1 })).toBeNull();
    expect([...defaultName('x'.repeat(100))]).toHaveLength(80);
  });

  it('Happy path — a report of Ordin’s conversation is kept with the run’s inputs', async () => {
    const inputs = [{ name: 'Q3 quotes.xlsx', location: 'Docs', version_read: '4' }];
    const b = await backendWith({
      groups: PUBLISH_GROUPS,
      setup: (t) =>
        t
          .on('reports.list', ok({ items: [report(REP1, RUN)], next_cursor: null }))
          .on('runs.get', ok({ run: { ...run(RUN, AGENT, AT), origin: { kind: 'conversation', workspace_id: WS }, inputs_used: inputs }, workspace_id: WS }))
          .on('dashboards.keep', (input) => ok({ id: uid('d'), ...(input as object), refresh_runs: [RUN], kept_at: AT })),
    });
    const out = await keepAsDashboard(b.handle, WS, { title: 'Supplier prices', as_of: AT }, '  Prices ');
    expect(out.kind).toBe('kept');
    expect(b.transport.sentOf('dashboards.keep').map((e) => e.input)).toEqual([{ workspace_id: WS, name: 'Prices', made_by: { kind: 'conversation' }, inputs, run_id: RUN }]);
  });

  it('Edge case — no matching report, a crew’s report, a blank name', async () => {
    const b = await backendWith({
      groups: PUBLISH_GROUPS,
      setup: (t) =>
        t.on('reports.list', ok({ items: [report(REP1, RUN)], next_cursor: null })).on('runs.get', ok({ run: { ...run(RUN, AGENT, AT), origin: { kind: 'crew_round', crew_run_id: uid('c'), round_number: 1 } }, workspace_id: WS })),
    });
    expect((await keepAsDashboard(b.handle, WS, { title: 'Nothing', as_of: AT }, 'N')).kind).toBe('not_found');
    expect((await keepAsDashboard(b.handle, WS, { title: 'Supplier prices', as_of: AT }, 'N')).kind).toBe('from_crew');
    expect((await keepAsDashboard(b.handle, WS, { title: 'Supplier prices', as_of: AT }, '   ')).kind).toBe('name_needed');
    expect(b.transport.sentOf('dashboards.keep')).toHaveLength(0);
  });

  it('Error path — a name already used is the Journal’s refusal', async () => {
    const b = await backendWith({
      groups: PUBLISH_GROUPS,
      setup: (t) =>
        t
          .on('reports.list', ok({ items: [report(REP1, RUN)], next_cursor: null }))
          .on('runs.get', ok({ run: { ...run(RUN, AGENT, AT) }, workspace_id: WS }))
          .on('dashboards.keep', err('REPORT_DASHBOARD_NAME_DUPLICATE', 'A dashboard already has that name.')),
    });
    const out = await keepAsDashboard(b.handle, WS, { title: 'Supplier prices', as_of: AT }, 'Prices');
    expect(out.kind === 'error' && out.error.code).toBe('REPORT_DASHBOARD_NAME_DUPLICATE');
  });
});

describe('A kept dashboard (SM-DB-3, SM-DB-4)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-DB-4
  const D = uid('dash');
  const dashboard = (runs: string[]) => ({ id: D, workspace_id: WS, name: 'Prices', made_by: { kind: 'conversation' }, inputs: [], refresh_runs: runs, kept_at: AT });

  it('pure rule — the newest refresh run that has a report', () => {
    const older = uid('r-old');
    const newer = uid('r-new');
    expect(newestReportOf({ refresh_runs: [older, newer] }, [report('a', older)])?.id).toBe('a');
    expect(newestReportOf({ refresh_runs: [older, newer] }, [report('a', older), report('b', newer)])?.id).toBe('b');
    expect(newestReportOf({ refresh_runs: [uid('none')] }, [report('a', older)])).toBeNull();
  });

  it('Happy path — the page shows the report and Refresh starts a refresh', async () => {
    const parts = [{ heading: 'Prices', sources: [], data: { kind: 'table', columns: [{ name: 'Supplier', column_type: 'text' }], rows: [['Kestrel']] } }];
    const b = await backendWith({
      groups: PUBLISH_GROUPS,
      setup: (t) =>
        t
          .on('dashboards.list', ok({ items: [dashboard([RUN])], next_cursor: null }))
          .on('reports.list', ok({ items: [report(REP1, RUN)], next_cursor: null }))
          .on('reports.get', ok({ ...report(REP1, RUN), workspace_id: WS, parts }))
          .on('dashboards.refresh', ok({ kind: 'run', run_id: uid('r2') })),
    });
    const page = new DashboardPage(b.handle, WS, D);
    await page.load();
    expect(page.report?.parts[0]?.heading).toBe('Prices');
    expect(await page.refresh()).toBe(true);
    expect(b.transport.sentOf('dashboards.refresh').map((e) => e.input)).toEqual([{ workspace_id: WS, dashboard_id: D }]);
  });

  it('Edge case — a dashboard whose refresh has no report yet, and one that is gone', async () => {
    const b = await backendWith({ groups: PUBLISH_GROUPS, setup: (t) => t.on('dashboards.list', ok({ items: [dashboard([RUN])], next_cursor: null })).on('reports.list', ok({ items: [], next_cursor: null })) });
    const page = new DashboardPage(b.handle, WS, D);
    await page.load();
    expect(page.dashboard?.name).toBe('Prices');
    expect(page.report).toBeNull();
    const gone = new DashboardPage(b.handle, WS, uid('other'));
    await gone.load();
    expect(gone.dashboard).toBeNull();
  });

  it('Error path — a refused refresh shows its error', async () => {
    const b = await backendWith({ groups: PUBLISH_GROUPS, setup: (t) => t.on('dashboards.refresh', err('OH_DASHBOARD_UNKNOWN', 'That dashboard is gone.')) });
    const page = new DashboardPage(b.handle, WS, D);
    expect(await page.refresh()).toBe(false);
    expect(page.error?.code).toBe('OH_DASHBOARD_UNKNOWN');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-DB-3
  it('Happy path — a kept dashboard appears under Dashboards in the tree', async () => {
    const b = await backendWith({ setup: (t) => t.on('dashboards.list', ok({ items: [dashboard([RUN])], next_cursor: null })) });
    const target = await mountIn(DashboardsTreeGroup, { backend: b.state, workspaceId: WS, initiallyOpen: true });
    expect(words(target)).toContain('Prices');
  });
});

// ------------------------------------------------------------------------- The tree's menus (SM-PUB-1, SM-FP-1)

describe('The side tree’s document and folder menus (SM-PUB-1, SM-FP-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-FP-1
  const own = { id: PLACE, friendly_name: 'Sample documents', path: '/home/sam/Sample', kind: 'persons_folder', added_at: AT };
  const treeOf = (kind: string) => ({ workspace: { id: WS, name: 'W' }, places: [{ ...own, kind }], agents: [], running: [], dashboards: [], workflows: [], knowledge_pages: 0, memory_facts: 0, waiting_for_person: 0 });
  async function tree(kind: string, links: unknown[], confluenceLinks: unknown[] = []) {
    const b = await backendWith({
      groups: PUBLISH_GROUPS,
      setup: (t) =>
        t
          .on('workspaces.tree', ok(treeOf(kind)))
          .on('workspaces.links', ok(links))
          .on('workspaces.confluence_links', ok(confluenceLinks))
          .on('files.list_folder', ok({ items: [entry('notes.md', AT), entry('Pricing/', AT, null, true)], next_cursor: null })),
    });
    await b.state.loadTree(WS);
    const target = await mountIn(TreeFolder, { backend: b.state, workspaceId: WS, placeId: PLACE });
    return { ...b, target };
  }
  const menuOf = async (target: HTMLElement, name: string): Promise<void> =>
    press(Array.from(target.querySelectorAll('button[aria-haspopup="menu"]')).find((b) => (b.getAttribute('aria-label') ?? '').replace(/[⁨⁩]/g, '') === `More actions for ${name}`));

  it('Happy path — a document’s menu adds Publish to… beside its other items', async () => {
    const { target } = await tree('persons_folder', [link('confluence_location', uid('pc'), 'Procurement')]);
    await menuOf(target, 'notes.md');
    await press(buttonNamed(target, 'Publish to…'));
    expect(buttonNamed(target, 'Confluence…')).toBeDefined();
  });

  it('Edge case — nothing connected: no Publish to… in the document’s menu', async () => {
    const { target } = await tree('persons_folder', []);
    await menuOf(target, 'notes.md');
    expect(buttonNamed(target, 'Publish to…')).toBeUndefined();
  });

  it('Happy path — a folder of the person’s own folder offers Publish this folder automatically…', async () => {
    const { target, transport } = await tree('persons_folder', []);
    await menuOf(target, 'Pricing');
    expect(words(target), 'the folder menu').toContain('Publish this folder automatically…');
    await press(buttonNamed(target, 'Publish this folder automatically…'));
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(transport.sentOf('workspaces.propose_confluence_link')).toHaveLength(0);
  });

  it('Edge case — a linked copy, or a folder already linked, does not offer it', async () => {
    const copy = await tree('working_copy', []);
    await menuOf(copy.target, 'Pricing');
    expect(buttonNamed(copy.target, 'Publish this folder automatically…')).toBeUndefined();
    if (drawn !== null) void unmount(drawn);
    drawn = null;
    document.body.innerHTML = '';
    const linked = await tree('persons_folder', [], [{ id: uid('cl'), workspace_id: WS, place_id: PLACE, folder: 'Pricing', confluence_space: 'PROC', parent_page_id: '4000', audience: 'Team', state: 'active' }]);
    await menuOf(linked.target, 'Pricing');
    expect(buttonNamed(linked.target, 'Publish this folder automatically…')).toBeUndefined();
  });
});
