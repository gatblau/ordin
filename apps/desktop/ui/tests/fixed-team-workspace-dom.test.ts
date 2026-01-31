// @vitest-environment happy-dom
// A team workspace mounted (ScreensModule.md SM-TW-1 to SM-TW-10): the share and open dialogs, the access
// card's buttons and focus, Team activity in the tree and its screen, and Leave. Each refusal the spec lists
// is drawn as its sentence through ErrorLine, never as its code.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { stripIsolates, t } from '../src/lib/i18n/fluent';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { makeKey, SCREEN_IDS } from '../src/lib/shell/screens';
import SideRail from '../src/lib/shell/SideRail.svelte';
import WorkspaceTree from '../src/lib/shell/WorkspaceTree.svelte';
import { TeamController } from '../src/lib/views/fixed/team.svelte';
import TeamActivity from '../src/lib/views/fixed/TeamActivity.svelte';
import TeamPanel from '../src/lib/views/fixed/TeamPanel.svelte';
import TeamWorkspaceShareDialog from '../src/lib/views/fixed/TeamWorkspaceShareDialog.svelte';
import type { FakeTransport } from './support/fake-backend';
import { err, hello, LOCAL, ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';
import { entry, grant, status, TEAM_GROUPS, teamWs } from './team-workspace-support';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const page = (): string => clean(document.body.textContent);
async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}
const button = (label: string): HTMLButtonElement => {
  const found = [...document.querySelectorAll('button')].filter((b) => clean(b.textContent) === label).pop();
  expect(found, `a button named ${label}`).toBeDefined();
  return found as HTMLButtonElement;
};
const press = async (label: string): Promise<void> => {
  button(label).click();
  await flush();
};
/** Opens the menu of the icon button beside the Workspaces heading, when the rail draws one. */
const openWorkspaceMenu = async (): Promise<void> => {
  document.querySelector<HTMLButtonElement>('.workspaces-head button[aria-haspopup="menu"]')?.click();
  await flush();
};
function type(input: HTMLInputElement, value: string): void {
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.dispatchEvent(new Event('change', { bubbles: true }));
}
function draw(component: unknown, props: Record<string, unknown>, app?: AppContext): HTMLElement {
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(component as never, { target, props: props as never, ...(app === undefined ? {} : { context: new Map<unknown, unknown>([[APP_KEY, app]]) }) });
  return target;
}

const OWN_LINK = uid('sl-team');
const sharePreview = {
  folder_name: 'acme/supplier-team › knowledge',
  places: [{ key: 'p1', display_name: 'Contracts', kind: 'sharepoint_location' }],
  stays_here: ['Notes'],
  agents: [{ key: 'a1', name: 'Supplier analyst', access: [{ place_name: 'Contracts', access: 'read_and_change' }] }],
  knowledge: { new_facts: 12, new_pages: 3, changed_facts: 0, changed_pages: 0, removed_facts: 0, removed_pages: 0 },
  results_choices: [
    { choice: { kind: 'team_folder' }, name: 'acme/supplier-team › knowledge › results' },
    { choice: { kind: 'link', source_link_id: uid('sl-sp') }, name: 'Contracts' },
  ],
};

async function shareDialog(setup: (t: FakeTransport) => void) {
  const b = await backendWith({
    groups: TEAM_GROUPS,
    setup: (t) => {
      t.on('team_knowledge.candidate_links', ok([{ id: OWN_LINK, display_name: 'acme/supplier-team › knowledge' }])).on('team_knowledge.workspace_share_preview', ok(sharePreview));
      setup(t);
    },
  });
  const controller = new TeamController(b.handle, WS);
  controller.status = status({ binding: null, header_line: null }) as never;
  let closed = 0;
  draw(TeamWorkspaceShareDialog, { controller, workspaceName: 'Q3 supplier review', onclose: () => closed++ });
  await flush();
  const select = document.querySelector('select') as HTMLSelectElement;
  select.value = OWN_LINK;
  select.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
  return { ...b, controller, closed: () => closed };
}

describe('Share as a team workspace… (SM-TW-2)', () => {
  it('Happy path — the share dialog says what is shared before it is shared', async () => {
    const { transport, controller, closed, state } = await shareDialog((t) => t.on('team_knowledge.share_workspace', ok(status({ busy: 'sharing' }))));
    expect(page()).toContain('Share Q3 supplier review as a team workspace');
    expect(transport.sentOf('team_knowledge.workspace_share_preview')[0]?.input).toEqual({ ws: WS, choice: { kind: 'link', source_link_id: OWN_LINK } });
    const words = page();
    expect(words).toContain('Places everyone will link, each with their own sign-in: Contracts');
    expect(words).toContain('Stays on this computer: Notes');
    expect(words).toContain('Supplier analyst, which can read and change Contracts');
    expect(words).toContain('Results will be saved in acme/supplier-team › knowledge › results');
    expect(words).toContain('12 facts and 3 pages');
    expect(words).toContain("What you share stays in the repository's history");
    // Where results are saved: the team's folder first, then each choice by its name.
    const radios = [...document.querySelectorAll<HTMLInputElement>('input[type=radio]')];
    expect(radios.map((r) => clean(r.parentElement?.textContent))).toEqual(["In the team's folder", 'Contracts']);
    expect(radios[0]?.checked).toBe(true);
    // Cancel is first in focus.
    expect(clean(document.activeElement?.textContent)).toBe('Cancel');
    expect(transport.sentOf('team_knowledge.share_workspace')).toHaveLength(0);
    await press('Share');
    expect(transport.sentOf('team_knowledge.share_workspace')[0]?.input).toEqual({ ws: WS, choice: { kind: 'link', source_link_id: OWN_LINK }, results: { kind: 'team_folder' } });
    expect(closed()).toBe(1);
    // The panel then shows Preparing your share….
    void unmount(drawn as never);
    drawn = null;
    document.body.innerHTML = '';
    draw(TeamPanel, { backend: state, workspaceId: WS, initialStatus: controller.status });
    flushSync();
    expect(page()).toContain('Preparing your share…');
  });

  it('choosing another results place changes the line and the choice sent', async () => {
    const { transport } = await shareDialog((t) => t.on('team_knowledge.share_workspace', ok(status({ busy: 'sharing' }))));
    const radios = [...document.querySelectorAll<HTMLInputElement>('input[type=radio]')];
    radios[1]?.click();
    await flush();
    expect(page()).toContain('Results will be saved in Contracts');
    await press('Share');
    expect((transport.sentOf('team_knowledge.share_workspace')[0]?.input as { results: unknown }).results).toEqual({ kind: 'link', source_link_id: uid('sl-sp') });
  });

  // Scenario: Happy path — Change… after sharing (SM-TW-5; amended January 2026, the user's decision)
  // spec: ordin-desktop/components/ScreensModule#SM-TW-5
  it('Change… reads results_choices, not the share preview, checks the current place and saves the choice', async () => {
    const choices = {
      choices: [
        { choice: { kind: 'team_folder' }, name: 'acme/supplier-team › knowledge › results' },
        { choice: { kind: 'link', source_link_id: uid('sl-sp') }, name: 'Supplier results' },
      ],
      current: { kind: 'link', source_link_id: uid('sl-sp') },
    };
    const b = await backendWith({
      groups: TEAM_GROUPS,
      setup: (t) => t.on('team_knowledge.results_choices', ok(choices)).on('team_knowledge.set_results_place', ok(status())),
    });
    const controller = new TeamController(b.handle, WS);
    controller.status = status({ team_workspace: teamWs() }) as never;
    let closed = 0;
    draw(TeamWorkspaceShareDialog, { controller, workspaceName: 'Q3 supplier review', resultsOnly: true, onclose: () => closed++ });
    await flush();
    expect(b.transport.sentOf('team_knowledge.results_choices')[0]?.input).toEqual({ ws: WS });
    expect(b.transport.sentOf('team_knowledge.workspace_share_preview')).toHaveLength(0);
    expect(page()).toContain('Where results are saved');
    const radios = [...document.querySelectorAll<HTMLInputElement>('input[type=radio]')];
    expect(radios.map((r) => clean(r.parentElement?.textContent))).toEqual(["In the team's folder", 'Supplier results']);
    expect(radios[1]?.checked).toBe(true);
    radios[0]?.click();
    await flush();
    await press('Save');
    expect(b.transport.sentOf('team_knowledge.set_results_place')[0]?.input).toEqual({ ws: WS, results: { kind: 'team_folder' } });
    expect(closed).toBe(1);
  });

  it('Change… draws a refusal of results_choices as its sentence', async () => {
    const b = await backendWith({
      groups: TEAM_GROUPS,
      setup: (t) => t.on('team_knowledge.results_choices', err('TEAM_WS_NOT_TEAM', "This workspace isn't a team workspace.")),
    });
    const controller = new TeamController(b.handle, WS);
    controller.status = status() as never;
    draw(TeamWorkspaceShareDialog, { controller, workspaceName: 'W', resultsOnly: true, onclose: () => undefined });
    await flush();
    expect(page()).toContain("This workspace isn't a team workspace.");
    expect(page()).not.toContain('TEAM_WS_NOT_TEAM');
    expect(document.querySelectorAll('input[type=radio]')).toHaveLength(0);
    expect(button('Save').disabled).toBe(true);
  });

  it('a workspace that already shares knowledge shows its folder, fixed, and previews it at once', async () => {
    const b = await backendWith({ groups: TEAM_GROUPS, setup: (t) => t.on('team_knowledge.workspace_share_preview', ok(sharePreview)) });
    const controller = new TeamController(b.handle, WS);
    controller.status = status() as never;
    draw(TeamWorkspaceShareDialog, { controller, workspaceName: 'Q3 supplier review', onclose: () => undefined });
    await flush();
    expect(page()).toContain('Team knowledge: acme/supplier-team › knowledge');
    expect(document.querySelector('select')).toBeNull();
    expect(b.transport.sentOf('team_knowledge.workspace_share_preview')[0]?.input).toEqual({ ws: WS, choice: { kind: 'link', source_link_id: uid('sl-own') } });
  });

  it('an address that is not a folder says so and sends nothing', async () => {
    const b = await backendWith({ groups: TEAM_GROUPS, setup: (t) => t.on('team_knowledge.candidate_links', ok([])) });
    const controller = new TeamController(b.handle, WS);
    controller.status = status({ binding: null, header_line: null }) as never;
    draw(TeamWorkspaceShareDialog, { controller, workspaceName: 'W', onclose: () => undefined });
    await flush();
    type(document.querySelector('input[type=text]') as HTMLInputElement, 'not an address');
    await flush();
    expect(page()).toContain('Paste the address of a folder as GitHub or GitLab shows it.');
    expect(b.transport.sentOf('team_knowledge.workspace_share_preview')).toHaveLength(0);
  });

  it.each([
    ['TEAM_WS_ALREADY_TEAM', {}],
    ['TEAM_WS_FOLDER_TAKEN', {}],
    ['TEAM_WS_RESULTS_NOT_SUITABLE', {}],
    ['TEAM_READ_ONLY', {}],
    ['TEAM_ALREADY_ON', {}],
    ['TEAM_LINK_NOT_SUITABLE', {}],
    ['TEAM_NO_ACCESS', {}],
    ['TEAM_SIGN_IN_NEEDED', { action: 'sign_in_again' }],
    ['TEAM_REMOTE_UNREACHABLE', { action: 'try_again' }],
  ] as const)('Error — %s stays in the dialog as its sentence', async (code, extra) => {
    const { transport, closed } = await shareDialog((t) => t.on('team_knowledge.share_workspace', err(code, `The sentence of ${code.toLowerCase()}.`, extra)));
    await press('Share');
    expect(closed()).toBe(0);
    expect(page()).toContain(`The sentence of ${code.toLowerCase()}.`);
    expect(page()).not.toContain(code);
    if ('action' in extra && extra.action === 'try_again') {
      await press('Try again');
      expect(transport.sentOf('team_knowledge.share_workspace')).toHaveLength(2);
    }
    if ('action' in extra && extra.action === 'sign_in_again') expect(button('Sign in again')).toBeDefined();
  });
});

describe('Open a team workspace… (SM-TW-3)', () => {
  const NEW_WS = uid('ws-new');
  const openPreview = {
    name: 'Q3 supplier review',
    name_in_use: false,
    folder_name: 'acme/supplier-team › knowledge',
    places: [
      { key: 'p1', display_name: 'Contracts', kind: 'sharepoint_location', reach: 'no_access' },
      { key: 'p2', display_name: 'Quotes', kind: 'sharepoint_location', reach: 'will_link' },
    ],
    agents: [
      { key: 'a1', name: 'Supplier analyst', access: [{ place_name: 'Contracts', access: 'read_and_change' }], found: 'already_here' },
      { key: 'a2', name: 'Contract checker', access: [{ place_name: 'Quotes', access: 'read' }], found: 'not_in_your_repositories' },
    ],
    knowledge: { new_facts: 4, new_pages: 1, changed_facts: 0, changed_pages: 0, removed_facts: 0, removed_pages: 0 },
    results: 'acme/supplier-team › knowledge › results',
  };

  async function rail(setup: (t: FakeTransport) => void = () => undefined) {
    let listed = 0;
    const { app, transport } = await appWith((t) => {
      t.on('service.hello', ok(hello(LOCAL, TEAM_GROUPS)))
        .on('workspaces.list', () => ok(listed++ === 0 ? [] : [{ id: NEW_WS, name: 'Q3 supplier review', created_at: '2026-01-20T09:30:02.114Z', own_folder_place_id: uid('p'), state: 'active' }]))
        .on('team_knowledge.workspace_open_preview', ok(openPreview))
        .on('team_knowledge.open_workspace', ok({ workspace_id: NEW_WS, status: status({ busy: 'opening', team_workspace: teamWs() }) }));
      setup(t);
    });
    draw(SideRail, {}, app);
    await settle(30);
    flushSync();
    // The rail offers it in the menu beside the Workspaces heading.
    await openWorkspaceMenu();
    await press('Open a team workspace…');
    type(document.querySelector('.team-ws-dialog input[type=text]') as HTMLInputElement, 'https://github.com/acme/supplier-team/tree/main/knowledge');
    await flush();
    return { app, transport };
  }

  it('Happy path — opening a team workspace', async () => {
    const { app, transport } = await rail();
    expect(clean(document.activeElement?.textContent)).toBe('Cancel');
    await press('Continue');
    expect(transport.sentOf('team_knowledge.workspace_open_preview')[0]?.input).toEqual({ url: 'https://github.com/acme/supplier-team', branch: 'main', path: 'knowledge' });
    const words = page();
    expect(words).toContain("Contracts (You don't have access)");
    expect(words).not.toContain('Quotes (');
    expect(words).toContain('Supplier analyst, which can read and change Contracts · Already on this computer');
    expect(words).toContain("Contract checker, which can read Quotes · Not in your agent library, so it won't be added");
    expect(words).toContain('Results will be saved in acme/supplier-team › knowledge › results');
    expect(words).toContain('Opening links these places with your own sign-ins');
    expect((document.querySelector('.team-ws-dialog input[type=text]') as HTMLInputElement).value).toBe('Q3 supplier review');
    await press('Open');
    expect(transport.sentOf('team_knowledge.open_workspace')[0]?.input).toEqual({ url: 'https://github.com/acme/supplier-team', branch: 'main', path: 'knowledge', name: 'Q3 supplier review' });
    await flush();
    expect(document.querySelector('.team-ws-dialog')).toBeNull();
    expect(app.shell.openWorkspace).toBe(NEW_WS);
    expect(app.local.workspaces.map((w) => w.id)).toEqual([NEW_WS]);
  });

  it('a name in use keeps Open off until the name changes', async () => {
    await rail((t) => t.on('team_knowledge.workspace_open_preview', ok({ ...openPreview, name_in_use: true })));
    await press('Continue');
    expect(page()).toContain('Another workspace already has this name. Choose another.');
    expect(button('Open').disabled).toBe(true);
    type(document.querySelector('.team-ws-dialog input[type=text]') as HTMLInputElement, 'Q3 supplier review (team)');
    await flush();
    expect(button('Open').disabled).toBe(false);
  });

  it.each([
    'TEAM_WS_NOT_A_TEAM_WORKSPACE', 'TEAM_WS_FORMAT_NEWER', 'TEAM_WS_DESCRIPTION_UNREADABLE', 'TEAM_FORMAT_NEWER',
    'TEAM_LINK_NOT_SUITABLE', 'TEAM_NO_ACCESS', 'TEAM_SIGN_IN_NEEDED', 'TEAM_REMOTE_UNREACHABLE',
  ])('Error — %s from Continue stays in the dialog as its sentence', async (code) => {
    await rail((t) => t.on('team_knowledge.workspace_open_preview', err(code, `The sentence of ${code.toLowerCase()}.`)));
    await press('Continue');
    expect(page()).toContain(`The sentence of ${code.toLowerCase()}.`);
    expect(page()).not.toContain(code);
    expect(document.querySelector('.team-ws-dialog')).not.toBeNull();
  });

  it.each(['TEAM_WS_ALREADY_OPEN', 'TEAM_WS_NAME_IN_USE', 'TEAM_WS_OPEN_FAILED'])('Error — %s from Open stays in the dialog as its sentence', async (code) => {
    const { app } = await rail((t) => t.on('team_knowledge.open_workspace', err(code, `The sentence of ${code.toLowerCase()}.`)));
    await press('Continue');
    await press('Open');
    expect(page()).toContain(`The sentence of ${code.toLowerCase()}.`);
    expect(page()).not.toContain(code);
    expect(app.shell.openWorkspace).not.toBe(NEW_WS);
  });

  it('an address that is not a folder says so and sends nothing', async () => {
    const { transport } = await rail();
    type(document.querySelector('.team-ws-dialog input[type=text]') as HTMLInputElement, 'https://github.com/acme/supplier-team');
    await press('Continue');
    expect(page()).toContain('Paste the address of a folder as GitHub or GitLab shows it.');
    expect(transport.sentOf('team_knowledge.workspace_open_preview')).toHaveLength(0);
  });

  it('SM-TW-12 — the rail offers it only when the local backend answers the team knowledge group', async () => {
    const { app } = await appWith();
    draw(SideRail, {}, app);
    await settle(30);
    flushSync();
    await openWorkspaceMenu();
    expect(page()).not.toContain('Open a team workspace…');
  });
});

describe('the access card (SM-TW-6) and Team activity (SM-TW-8)', () => {
  async function screen(options: { grants?: unknown[]; entries?: unknown[]; instance?: string | null; team?: boolean; setup?: (t: FakeTransport) => void } = {}) {
    const { app, transport } = await appWith((t) => {
      t.on('service.hello', ok(hello(LOCAL, TEAM_GROUPS)))
        .on('workspaces.list', ok([{ id: WS, name: 'Q3 supplier review', created_at: '2026-01-20T09:30:02.114Z', own_folder_place_id: uid('p'), state: 'active' }]))
        .on('team_knowledge.pending_grants', ok(options.grants ?? []))
        .on('team_knowledge.allow_grant', ok(status()))
        .on('team_knowledge.decline_grant', ok(status()))
        .on('team_knowledge.activity', ok({ items: options.entries ?? [], next_cursor: null }))
        .on('team_knowledge.status', ok(status(options.team === false ? {} : { team_workspace: teamWs({ pending_grants: (options.grants ?? []).length }) })));
      options.setup?.(t);
    });
    app.shell.openWorkspaceTab(LOCAL, WS);
    const target = draw(TeamActivity, { backend: app.local, workspaceId: WS, instance: options.instance ?? null }, app);
    await flush();
    return { app, transport, target };
  }

  it('Security — the access card asks before anything is granted; Don\'t allow has focus and is not the default', async () => {
    const { transport } = await screen({ grants: [grant()] });
    expect(page()).toContain('priya added Supplier analyst, which can read and change Contracts — Allow?');
    const decline = button("Don't allow");
    expect(document.activeElement).toBe(decline);
    expect(decline.hasAttribute('data-default')).toBe(false);
    expect(button('Allow').hasAttribute('data-default')).toBe(false);
    // Don't allow comes before Allow.
    const order = [...document.querySelectorAll('.grant-card button')].map((b) => clean(b.textContent));
    expect(order).toEqual(["Don't allow", 'Allow']);
    expect(transport.sentOf('team_knowledge.allow_grant')).toHaveLength(0);
    await press('Allow');
    expect(transport.sentOf('team_knowledge.allow_grant')[0]?.input).toEqual({ ws: WS, grant_id: uid('g1') });
    expect(document.querySelector('.grant-card')).toBeNull();
    expect(clean(document.querySelector('[aria-live=polite]')?.textContent)).toBe('Allowed. Supplier analyst can use what your team gave it.');
  });

  it('left-out places are named under the line', async () => {
    await screen({ grants: [grant({ left_out: ['Rates', 'Quotes'] })] });
    expect(page()).toContain("Left out because you can't open them: Rates and Quotes");
  });

  it.each([
    ['TEAM_WS_AGENT_NOT_ADDED', {}],
    ['TEAM_WS_ALLOW_FAILED', { action: 'try_again' }],
  ] as const)('Error — %s is drawn in the card through ErrorLine', async (code, extra) => {
    const { transport } = await screen({ grants: [grant()], setup: (t) => t.on('team_knowledge.allow_grant', err(code, `The sentence of ${code.toLowerCase()}.`, extra)) });
    await press('Allow');
    const card = document.querySelector('.grant-card');
    expect(clean(card?.textContent)).toContain(`The sentence of ${code.toLowerCase()}.`);
    expect(page()).not.toContain(code);
    if ('action' in extra) {
      await press('Try again');
      expect(transport.sentOf('team_knowledge.allow_grant')).toHaveLength(2);
    }
  });

  it('Error — TEAM_WS_GRANT_NOT_FOUND is drawn and the list is read again', async () => {
    let reads = 0;
    const { transport } = await screen({
      grants: [grant()],
      setup: (t) =>
        t.on('team_knowledge.decline_grant', err('TEAM_WS_GRANT_NOT_FOUND', 'This was already answered.')).on('team_knowledge.pending_grants', () => ok(reads++ === 0 ? [grant()] : [])),
    });
    await press("Don't allow");
    expect(transport.sentOf('team_knowledge.pending_grants')).toHaveLength(2);
    expect(document.querySelector('.grant-card')).toBeNull();
    expect(page()).toContain('This was already answered.');
  });

  it('Happy path — Team activity: the access card comes first and the entries follow, newest first, each result a link that opens the file', async () => {
    const place = uid('place');
    const entries = [
      entry({ id: uid('a2'), at: '2026-01-21T10:00:00.000Z', results: [{ name: 'Kestrel quote — comparison.md', file: { kind: 'place_file', place_id: place, relative_path: 'results/Kestrel quote — comparison.md' } }] }),
      entry({ id: uid('a1'), by: 'priya', task: 'Summarise rates', outcome: 'failed', results: [{ name: 'rates.md' }] }),
    ];
    const { app, target } = await screen({ grants: [grant()], entries });
    const html = target.innerHTML;
    expect(html.indexOf('grant-card')).toBeLessThan(html.indexOf('entries'));
    const lines = [...target.querySelectorAll('.entry .line')].map((l) => clean(l.textContent));
    expect(lines).toEqual(['sam ran Compare documents with Supplier analyst', 'priya ran Summarise rates with Supplier analyst']);
    expect(page()).toContain('Done');
    expect(page()).toContain("Didn't finish");
    // A result not on this computer is plain text with its tooltip.
    const notHere = target.querySelector('.muted-link') as HTMLElement;
    expect(clean(notHere.textContent)).toBe('rates.md');
    expect(notHere.getAttribute('title')).toBe("Not on this computer yet. Get your team's memory and knowledge pages to bring it here.");
    await press('Kestrel quote — comparison.md');
    expect(app.screens.front(ScreenAreaState.scope(LOCAL, WS))).toBe(makeKey(SCREEN_IDS.document, `${place}:results/Kestrel quote — comparison.md`));
  });

  it('none says so; an entry not yet shared carries Not shared yet', async () => {
    await screen();
    expect(page()).toContain('Nothing done here yet. Tasks your team runs in this workspace appear here.');
    document.body.innerHTML = '';
    await screen({ entries: [entry({ by: null, mine: true })] });
    expect(page()).toContain('You ran Compare documents with Supplier analyst');
    expect(page()).toContain('Not shared yet');
  });

  it('Show earlier sends the next page with the cursor', async () => {
    const { transport } = await screen({ setup: (t) => t.on('team_knowledge.activity', (input) => ok({ items: [entry({ id: (input as { cursor?: string }).cursor ? uid('a9') : uid('a1') })], next_cursor: (input as { cursor?: string }).cursor ? null : 'c2' })) });
    await press('Show earlier');
    expect(transport.sentOf('team_knowledge.activity').map((e) => e.input)).toEqual([{ ws: WS }, { ws: WS, cursor: 'c2' }]);
    expect(document.querySelectorAll('.entry')).toHaveLength(2);
  });

  it('HS-TW-2 — opened from Needs you, the grant\'s card takes the focus; an answered one says so', async () => {
    await screen({ grants: [grant(), grant({ grant_id: uid('g2'), agent_name: 'Contract checker' })], instance: uid('g2') });
    const card = (document.activeElement as HTMLElement).closest('.grant-card');
    expect(card?.getAttribute('data-grant')).toBe(uid('g2'));
    document.body.innerHTML = '';
    await screen({ grants: [], instance: uid('g3') });
    // The sentence is the Home and Search amendment's (`needs-you-team-grant-gone`), read back through `t`.
    const gone = 'needs-you-team-grant-gone';
    expect(clean(document.querySelector('p[role=status]')?.textContent)).toBe(stripIsolates(t(gone)));
    expect(document.querySelector('.grant-card')).toBeNull();
  });

  it('a workspace that stops being a team workspace closes the screen', async () => {
    const { app } = await screen({ team: false });
    expect(app.screens.front(ScreenAreaState.scope(LOCAL, WS))).not.toBe(SCREEN_IDS.teamActivity);
  });
});

describe('Team activity in the tree (SM-TW-7)', () => {
  async function tree(kind: 'team' | 'plain') {
    const { app, transport } = await appWith((t) =>
      t
        .on('service.hello', ok(hello(LOCAL, TEAM_GROUPS)))
        .on('workspaces.list', ok([{ id: WS, name: 'Q3 supplier review', created_at: '2026-01-20T09:30:02.114Z', own_folder_place_id: uid('p'), state: 'active' }]))
        .on('team_knowledge.status', ok(status(kind === 'team' ? { team_workspace: teamWs({ pending_grants: 1 }) } : {})))
        .on('team_knowledge.candidate_links', ok([]))
        .on('team_knowledge.workspace_share_preview', ok(sharePreview)),
    );
    const target = draw(WorkspaceTree, { backends: [app.local] }, app);
    await flush();
    (target.querySelector('.tree-toggle') as HTMLButtonElement).click();
    await flush();
    return { app, transport, target };
  }

  it('Happy path — the tree shows "Team activity" with the count 1, after Running now and before Knowledge', async () => {
    const { app, target } = await tree('team');
    const rows = [...target.querySelectorAll('.tree-group .tree-row')].map((r) => clean(r.textContent));
    const at = rows.indexOf('Team activity 1');
    expect(at).toBeGreaterThan(rows.indexOf('Running now'));
    expect(at).toBeLessThan(rows.indexOf('Knowledge'));
    expect(target.querySelector('.tree-group .count')?.getAttribute('aria-label')).toBe('1 change by your team waits for you');
    await press('Team activity 1');
    expect(app.screens.front(ScreenAreaState.scope(LOCAL, WS))).toBe(SCREEN_IDS.teamActivity);
  });

  // SM-TW-13 (amended January 2026, user decision; Approved by the user January 2026): sharing lives in the
  // workspace's ⋯ menu (and its right-click menu); the row keeps only a non-interactive team icon.
  const menuItems = (): string[] => [...document.querySelectorAll('[role=menuitem]')].map((b) => clean(b.textContent));
  const pickItem = async (label: string): Promise<void> => {
    ([...document.querySelectorAll<HTMLButtonElement>('[role=menuitem]')].find((b) => clean(b.textContent) === label) as HTMLButtonElement).click();
    await flush();
  };
  // spec: ordin-desktop/components/ScreensModule#SM-TW-13
  it('SM-TW-13 — an unshared workspace: no share control on the row; its menu offers Share with your team…, which opens the share dialog', async () => {
    const { target, app } = await tree('plain');
    const row = target.querySelector('.tree-node') as HTMLElement;
    expect([...row.querySelectorAll('button')].map((b) => clean(b.textContent))).not.toContain('Share');
    expect(row.querySelector('[role=img]')).toBeNull();
    (row.querySelector('.tree-action') as HTMLButtonElement).click();
    await flush();
    expect(menuItems()).toContain('Share with your team…');
    expect(menuItems()).not.toContain('Shared with your team');
    const before = app.shell.openWorkspace;
    await pickItem('Share with your team…');
    expect(document.querySelector('[role=dialog]')).not.toBeNull();
    expect(page()).toContain('Share Q3 supplier review as a team workspace');
    expect(app.shell.openWorkspace).toBe(before);
  });

  it('SM-TW-13 — a right-click on the row opens the same menu', async () => {
    const { target } = await tree('plain');
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    (target.querySelector('.tree-node') as HTMLElement).dispatchEvent(event);
    await flush();
    expect(event.defaultPrevented).toBe(true);
    expect(menuItems()).toContain('Share with your team…');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TW-13
  it('SM-TW-13 — a team workspace: a team icon on the row, and Shared with your team and Leave in its menu', async () => {
    const { target } = await tree('team');
    const row = target.querySelector('.tree-node') as HTMLElement;
    const icon = row.querySelector('[role=img]');
    expect(icon?.getAttribute('aria-label')).toBe('Shared with your team');
    expect(icon?.closest('button')).toBeNull();
    expect(icon?.querySelector('use')?.getAttribute('href')).toBe('#i-crew');
    (row.querySelector('.tree-action') as HTMLButtonElement).click();
    await flush();
    expect(menuItems()).toEqual(expect.arrayContaining(['Shared with your team', 'Leave the team workspace']));
    expect(menuItems()).not.toContain('Share with your team…');
    await pickItem('Shared with your team');
    expect(page()).toContain('Team workspace: acme/supplier-team › knowledge');
    expect(page()).toContain('Shared with everyone who can open acme/supplier-team › knowledge.');
    (row.querySelector('.tree-action') as HTMLButtonElement).click();
    await flush();
    await pickItem('Leave the team workspace');
    expect(page()).toContain('Leave Q3 supplier review?');
  });
});

describe('Leave the team workspace (SM-TW-10)', () => {
  async function panel(setup: (t: FakeTransport) => void) {
    const b = await backendWith({ groups: TEAM_GROUPS, setup: (t) => {
      t.on('workspaces.list', ok([{ id: WS, name: 'Q3 supplier review', created_at: '2026-01-20T09:30:02.114Z', own_folder_place_id: uid('p'), state: 'active' }]));
      t.on('team_knowledge.status', ok(status({ team_workspace: teamWs() })));
      setup(t);
    } });
    b.state.workspaces = [{ id: WS, name: 'Q3 supplier review' } as never];
    draw(TeamPanel, { backend: b.state, workspaceId: WS });
    await flush();
    return b;
  }

  it('Happy path — leaving asks, then leave_workspace is sent and the leaving is announced', async () => {
    const { transport } = await panel((t) => t.on('team_knowledge.leave_workspace', ok({})));
    await press('Leave the team workspace');
    expect(page()).toContain('Leave Q3 supplier review?');
    expect(page()).toContain('It stays on this computer as your own workspace');
    expect(clean(document.activeElement?.textContent)).toBe('Cancel');
    expect(transport.sentOf('team_knowledge.leave_workspace')).toHaveLength(0);
    await press('Leave');
    expect(transport.sentOf('team_knowledge.leave_workspace')[0]?.input).toEqual({ ws: WS });
    expect(clean(document.querySelector('.team-panel [aria-live=polite]')?.textContent)).toBe('You left the team workspace. Everything is still here.');
  });

  it('Error — TEAM_SHARE_PENDING is drawn in the confirmation through ErrorLine', async () => {
    await panel((t) => t.on('team_knowledge.leave_workspace', err('TEAM_SHARE_PENDING', 'Your last share is still on its way. Try again when it has gone.')));
    await press('Leave the team workspace');
    await press('Leave');
    expect(page()).toContain('Your last share is still on its way.');
    expect(page()).not.toContain('TEAM_SHARE_PENDING');
    expect(page()).toContain('Leave Q3 supplier review?');
  });
});
