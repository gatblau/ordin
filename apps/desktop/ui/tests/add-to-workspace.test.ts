// Add to this workspace (SM-AW-1 to SM-AW-4) and removing a link (SM-RL-1, SM-RL-2): one test per Gherkin
// scenario of the amendment and per added error row, over the fake transport.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import DocumentsTreeGroup from '../src/lib/shell/DocumentsTreeGroup.svelte';
import SideRail from '../src/lib/shell/SideRail.svelte';
import { getScreen } from '../src/lib/shell/screens';
import { ADD_SEARCH_LIMIT, ADD_TO_WORKSPACE_SCREEN, AddToWorkspaceModel, placeName } from '../src/lib/views/add/add-model.svelte';
import AddToWorkspace from '../src/lib/views/add/AddToWorkspace.svelte';
import { DocumentLinksModel, orderedLinks } from '../src/lib/views/add/links-model.svelte';
import '../src/lib/views/add/register';
import { err, hello, LOCAL, ok, R1_GROUPS } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
afterEach(() => void vi.useRealTimers());
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));

const GROUPS = [...R1_GROUPS, 'sharepoint', 'confluence', 'jira'];
const SUPPLIERS = { site_id: 's1', site_name: 'Procurement', drive_id: 'd1', drive_name: 'Documents', item_id: 'i1', path: 'Suppliers', is_folder: true, web_url: 'https://acme.sharepoint.com/sites/Procurement/Suppliers' };
const link = (id: string, kind: string, display_name: string, over: Record<string, unknown> = {}) =>
  ({ id, kind, display_name, state: 'active', workspace_id: WS, added_at: '2026-01-06T09:00:00.000Z', origin_location: display_name, place_id: `p-${id}`, ...over }) as never;
const JIRA = link(uid('l-jira'), 'jira_issue', 'ABC-12', { place_id: null });
const render = (state: unknown, model: AddToWorkspaceModel) => text(html(AddToWorkspace, { backend: state, workspaceId: WS, instance: null, model }));

describe('Add to this workspace — SharePoint and Confluence (SM-AW-1, SM-AW-2, SM-AW-4)', () => {
  it('Happy path — a SharePoint library is added: find with query and limit 25, add_link of kind sharepoint, then Added', async () => {
    vi.useFakeTimers();
    const { handle, transport, state } = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr
          .on('sharepoint.find', ok([SUPPLIERS]))
          .on('workspaces.add_link', ok(link(uid('l1'), 'sharepoint_location', 'Suppliers')))
          .on('workspaces.link_progress', ok({ source_link_id: uid('l1'), done: false, listed: 120, read: 40, not_read: 0 })),
    });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('sharepoint');
    model.type('Suppliers');
    expect(transport.sentOf('sharepoint.find')).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(300);
    expect(transport.sentOf('sharepoint.find')[0]?.input).toEqual({ query: 'Suppliers', limit: ADD_SEARCH_LIMIT });
    expect(placeName(model.results[0]!)).toBe('Procurement › Suppliers');
    expect(render(state, model)).toContain('Procurement › Suppliers');
    await model.addPlace(model.results[0]!);
    expect(transport.sentOf('workspaces.add_link')[0]?.input).toEqual({ workspace_id: WS, location: { kind: 'sharepoint', location: SUPPLIERS } });
    expect(model.step).toBe('added');
    expect(await model.pollProgress()).toBe(true);
    const out = render(state, model);
    expect(out).toContain('Added');
    expect(out).toContain('Suppliers is linked to this workspace.');
    expect(out).toContain('Reading 40 of 120 documents…');
    expect(out).toContain('Show documents');
    expect(out).toContain('Add something else');
  });

  it('the search waits for 2 characters and clears the list when emptied', async () => {
    const { handle, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('confluence.find', ok([])) });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('confluence');
    await model.search('a');
    expect(transport.sentOf('confluence.find')).toHaveLength(0);
    await model.search('ab');
    expect(transport.sentOf('confluence.find')[0]?.input).toEqual({ query: 'ab', limit: 25 });
    model.type('');
    expect(model.results).toEqual([]);
  });

  it('Error path — SharePoint needs a sign-in first: the sign-in step replaces the list, and a successful wait searches again', async () => {
    let signedIn = false;
    const { handle, transport, state } = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr
          .on('sharepoint.find', () => (signedIn ? ok([SUPPLIERS]) : err('SP_SIGN_IN_NEEDED', 'Sign in to SharePoint so Ordin can read this location.', { action: 'sign_in_again' })))
          .on('identity.sign_in', ok({ kind: 'browser_opened', challenge_id: uid('c1'), expires_at: '2026-01-28T10:00:00Z' }))
          .on('identity.sign_in_wait', () => {
            signedIn = true;
            return ok({ kind: 'microsoft' });
          }),
    });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('sharepoint');
    await model.search('Contracts');
    expect(model.step).toBe('sign_in');
    const out = render(state, model);
    expect(out).toContain('Sign in with Microsoft');
    expect(out).toContain(t('add-sign-in-line'));
    await model.signIn(t('add-sign-in-reason'));
    expect(transport.sentOf('identity.sign_in')[0]?.input).toEqual({ kind: 'microsoft', reason: 'To list the places you can add to this workspace' });
    expect(transport.sentOf('identity.sign_in_wait')[0]?.input).toEqual({ challenge_id: uid('c1') });
    expect(transport.sentOf('sharepoint.find').map((e) => e.input)).toEqual([
      { query: 'Contracts', limit: 25 },
      { query: 'Contracts', limit: 25 },
    ]);
    expect(model.step).toBe('search');
    expect(model.results).toHaveLength(1);
  });

  it.each([
    ['CONF_SIGN_IN_NEEDED', 'confluence', 'Sign in to Confluence'],
    ['WS_SOURCE_SIGN_IN_NEEDED', 'sharepoint', 'Sign in with Microsoft'],
  ] as const)('Error row — %s from a search draws the sign-in step for %s', async (code, entry, words) => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on(`${entry}.find`, err(code, 'Sign in.')) });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose(entry);
    await model.search('Supplier');
    expect(model.step).toBe('sign_in');
    expect(model.signInKind).toBe(entry === 'confluence' ? 'confluence' : 'microsoft');
    expect(render(state, model)).toContain(words);
  });

  it.each(['CONF_NOT_CONFIGURED', 'WS_SOURCE_NOT_CONFIGURED'])('Error row — %s: Confluence is not set up, with Open Settings', async (code) => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('confluence.find', err(code, 'Not set up.')) });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('confluence');
    await model.search('Supplier policy');
    expect(model.step).toBe('not_configured');
    const out = render(state, model);
    expect(out).toContain("Confluence isn't set up on this computer yet.");
    expect(out).toContain('Open Settings');
  });

  it('Error row — the same location added twice: SRC_DUPLICATE under Add, nothing added', async () => {
    const { handle, state } = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('sharepoint.find', ok([SUPPLIERS])).on('workspaces.add_link', err('SRC_DUPLICATE', 'This place is already linked to the workspace.')),
    });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('sharepoint');
    await model.search('Suppliers');
    await model.addPlace(model.results[0]!);
    expect(model.step).toBe('search');
    expect(model.errorAt).toBe('add');
    const out = render(state, model);
    expect(out.indexOf('This place is already linked to the workspace.')).toBeGreaterThan(out.indexOf('Add'));
  });

  it('any other search error is drawn under the field, and Back returns to Where it lives', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('sharepoint.find', err('WS_SOURCE_UNAVAILABLE', 'SharePoint can’t be reached just now.', { action: 'try_again' })) });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('sharepoint');
    await model.search('Suppliers');
    expect(model.errorAt).toBe('field');
    expect(render(state, model)).toContain('SharePoint can’t be reached just now.');
    model.back();
    expect(model.step).toBe('where');
    expect(render(state, model)).toContain(t('add-intro'));
  });
});

describe('Add to this workspace — a folder on this computer (SM-AW-3)', () => {
  it('Error path — a folder that is not a repository: its message under Choose a folder…, nothing added', async () => {
    const { handle, transport, state } = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('app.pick_folder', ok({ folder: '/Users/sam/notes' })).on('workspaces.add_link', err('WS_NOT_A_REPOSITORY', 'This folder doesn’t hold a Git repository.')),
    });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('code');
    await model.chooseFolder();
    expect(transport.sentOf('workspaces.add_link')[0]?.input).toEqual({ workspace_id: WS, location: { kind: 'git_repository_folder', path: '/Users/sam/notes' } });
    expect(model.step).toBe('folder');
    expect(model.added).toBeNull();
    const out = render(state, model);
    expect(out.indexOf('This folder doesn’t hold a Git repository.')).toBeGreaterThan(out.indexOf('Choose a folder…'));
  });

  it('Error row — a protected folder: WS_PLACE_REFUSED under the button', async () => {
    const { handle, transport, state } = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('app.pick_folder', ok({ folder: '/System' })).on('workspaces.add_link', err('WS_PLACE_REFUSED', 'ordin can’t use this folder.')),
    });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('local');
    await model.chooseFolder();
    expect(transport.sentOf('workspaces.add_link')[0]?.input).toMatchObject({ location: { kind: 'local_folder', path: '/System' } });
    expect(render(state, model)).toContain('ordin can’t use this folder.');
  });

  it('no folder chosen: nothing changes and nothing is added', async () => {
    const { handle, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('app.pick_folder', ok({ folder: null })) });
    const model = new AddToWorkspaceModel(handle, WS);
    model.choose('local');
    await model.chooseFolder();
    expect(transport.sentOf('workspaces.add_link')).toHaveLength(0);
    expect(model.step).toBe('folder');
    expect(model.error).toBeNull();
  });
});

describe('Add to this workspace — Where it lives (SM-AW-1)', () => {
  it('lists the entries the backend answers, marks a kind already linked, and draws no GitHub, GitLab, Email or Slack', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('workspaces.links', ok([link(uid('l1'), 'sharepoint_location', 'Contracts 2026')])) });
    const model = new AddToWorkspaceModel(handle, WS);
    await model.loadLinks();
    const out = render(state, model);
    for (const words of ['Add to this workspace', t('add-intro'), 'SharePoint', 'Confluence', 'This computer', 'A code repository on this computer', 'Jira issue…']) expect(out).toContain(words);
    expect(out.match(/Linked/g)).toHaveLength(1);
    for (const absent of ['GitHub', 'GitLab', 'Email…', 'Slack']) expect(out).not.toContain(absent);
    expect(model.linked('sharepoint')).toBe(true);
    expect(model.linked('confluence')).toBe(false);
  });

  it('a server’s workspace offers no folder on this computer, no Confluence and no Jira', async () => {
    const { handle, state } = await backendWith({ kind: 'server', groups: GROUPS });
    const out = render(state, new AddToWorkspaceModel(handle, WS));
    expect(out).toContain('SharePoint');
    for (const absent of ['This computer', 'A code repository', 'Confluence', 'Jira issue…']) expect(out).not.toContain(absent);
  });

  it('is registered as the screen add-to-workspace, titled add-title', () => {
    const def = getScreen(ADD_TO_WORKSPACE_SCREEN);
    expect(def?.titleId).toBe('add-title');
    expect(def?.commands).toEqual(['workspaces.links', 'workspaces.add_link', 'workspaces.link_progress']);
  });

  it('the tree offers it: the workspace row’s More actions and the Documents row’s Add documents…', async () => {
    const { app } = await appWith((tr) => {
      tr.on('workspaces.list', ok([{ id: WS, name: 'Q3 supplier review', created_at: 'x', own_folder_place_id: 'p', state: 'active' }]));
      tr.on('service.hello', ok(hello(LOCAL, GROUPS)));
    });
    app.prefs.setExpanded([`${LOCAL}:${WS}`]);
    const out = html(SideRail, {}, app).replace(/[\u2068\u2069]/g, '');
    expect(out).toContain('aria-label="More actions for Q3 supplier review"');
    expect(out).toContain('aria-label="Add documents…"');
    expect(out).toContain('title="Add documents…"');
  });
});

describe('Removing a link (SM-RL-1, SM-RL-2)', () => {
  const tree = (state: unknown, model: DocumentLinksModel) => html(DocumentsTreeGroup, { backend: state, workspaceId: WS, model, initiallyOpen: true });

  it('Documents by where they live: grouped and ordered by kind, proposed links not drawn', () => {
    const links = [
      link('a', 'jira_issue', 'ABC-12'),
      link('b', 'local_folder', 'Supplier quotes'),
      link('c', 'sharepoint_location', 'Contracts 2026'),
      link('d', 'confluence_location', 'Supplier policy', { state: 'proposed' }),
      link('e', 'git_repository_folder', 'billing-service'),
      link('f', 'confluence_location', 'Supplier pages'),
    ];
    expect(orderedLinks(links).map((l) => l.id)).toEqual(['c', 'f', 'b', 'e', 'a']);
  });

  it('Happy path — removing a Jira link says nothing in Jira changes, and confirming sends remove_link with its id', async () => {
    const { handle, transport, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('workspaces.links', ok([JIRA])).on('workspaces.remove_link', ok({})) });
    const model = new DocumentLinksModel(handle, WS);
    await model.load();
    expect(text(tree(state, model))).toContain('ABC-12');
    model.askRemove(model.links[0]!);
    const out = text(tree(state, model));
    expect(out).toContain('Remove “ABC-12” from this workspace?');
    expect(out).toContain('Your documents stay where they are. Agents in this workspace can no longer use them.');
    expect(out).toContain('Nothing in Jira changes.');
    expect(await model.confirmRemove()).toBe('ABC-12');
    expect(transport.sentOf('workspaces.remove_link')[0]?.input).toEqual({ source_link_id: uid('l-jira') });
    expect(model.removing).toBeNull();
    expect(t('remove-link-done', { name: 'ABC-12' })).toBe('Removed the link to “ABC-12”. The documents are untouched where they live.');
  });

  it('a link that is not a Jira issue has no Jira line', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS });
    const model = new DocumentLinksModel(handle, WS);
    model.askRemove(link('l2', 'sharepoint_location', 'Contracts 2026'));
    expect(text(tree(state, model))).not.toContain('Nothing in Jira changes.');
  });

  it('Edge case — Cancel removes nothing', async () => {
    const { handle, transport } = await backendWith({ groups: GROUPS });
    const model = new DocumentLinksModel(handle, WS);
    model.askRemove(JIRA);
    model.cancelRemove();
    expect(model.removing).toBeNull();
    expect(transport.sentOf('workspaces.remove_link')).toHaveLength(0);
  });

  it('Error row — removing a link fails: its message in the dialog, which stays open', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('workspaces.remove_link', err('WS_LINK_NOT_FOUND', 'This link was already removed.')) });
    const model = new DocumentLinksModel(handle, WS);
    model.askRemove(JIRA);
    expect(await model.confirmRemove()).toBeNull();
    expect(model.removing).not.toBeNull();
    const out = text(tree(state, model));
    expect(out).toContain('Remove “ABC-12” from this workspace?');
    expect(out).toContain('This link was already removed.');
  });

  it('each link row has More actions, an icon-only button with its name and tooltip', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('workspaces.links', ok([JIRA])) });
    const model = new DocumentLinksModel(handle, WS);
    await model.load();
    const out = tree(state, model).replace(/[\u2068\u2069]/g, '');
    expect(out).toContain('aria-label="More actions for ABC-12"');
    expect(out).toContain('title="More actions"');
  });
});
