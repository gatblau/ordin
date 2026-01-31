// Team knowledge (ADR-103, ADR-110): the address split, the panel and the dialogs' commands.
import { beforeAll, describe, expect, it } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { splitFolderAddress, teamErrorMessage, TeamController, teamOutcome } from '../src/lib/views/fixed/team.svelte';
import TeamPanel from '../src/lib/views/fixed/TeamPanel.svelte';
import { ok, err } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';
import { TEAM_GROUPS } from './team-workspace-support';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));

const status = (over: Record<string, unknown> = {}) => ({
  workspace_id: WS, folders: [], descriptions: [], folder_large: false, held_for_check: 0, problems: [], workspace_facts: 0, workspace_large: false, ...over,
});

describe('splitting a folder address', () => {
  it('GitHub: https://<host>/<owner>/<name>/tree/<branch>/<path>', () => {
    expect(splitFolderAddress('https://github.com/acme/platform-team/tree/main/knowledge')).toEqual({ url: 'https://github.com/acme/platform-team', branch: 'main', path: 'knowledge' });
  });
  it('GitLab: https://<host>/<group>/<name>/-/tree/<branch>/<path>, with a deeper path', () => {
    expect(splitFolderAddress('https://gitlab.example.com/acme/team/-/tree/trunk/docs/knowledge')).toEqual({ url: 'https://gitlab.example.com/acme/team', branch: 'trunk', path: 'docs/knowledge' });
  });
  it('the repository root has an empty path', () => {
    expect(splitFolderAddress('https://github.com/acme/team/tree/main')).toEqual({ url: 'https://github.com/acme/team', branch: 'main', path: '' });
  });
  it.each(['not a url', 'https://github.com/acme/team', 'http://github.com/acme/team/tree/main/x', 'https://user:pw@github.com/a/b/tree/main/x', ''])('"%s" is not the address of a folder', (text) => {
    expect(splitFolderAddress(text)).toBeNull();
  });
});

describe('the panel\'s commands', () => {
  // Amended January 2026 (user decision): knowledge-only sharing is no longer turned on from the screens.
  it('Edge — the controller no longer turns knowledge-only sharing on; a read status keeps its header', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.status', ok(status({ binding: { folder: 'knowledge' }, header_line: 'acme/supplier-team › knowledge' }))) });
    const c = new TeamController(handle, WS);
    expect('turnOn' in c).toBe(false);
    await c.load();
    expect(transport.commandsSent()).not.toContain('team_knowledge.turn_on');
    expect(c.status?.header_line).toBe('acme/supplier-team › knowledge');
    expect(t('team-header', { folder: c.status?.header_line ?? '' })).toBe('Team knowledge: acme/supplier-team › knowledge');
  });

  it('Happy path — reading another team\'s folder sends url, branch and path', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.add_read_folder', ok(status())) });
    const parsed = splitFolderAddress('https://github.com/acme/platform-team/tree/main/knowledge');
    await new TeamController(handle, WS).addReadFolder({ kind: 'new_folder', ...(parsed as { url: string; branch: string; path: string }) });
    expect(transport.sentOf('team_knowledge.add_read_folder')[0]?.input).toEqual({
      ws: WS, choice: { kind: 'new_folder', url: 'https://github.com/acme/platform-team', branch: 'main', path: 'knowledge' },
    });
  });

  it('Get them sends get (with the folder id for another folder) and share sends share', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.get', ok(status({ busy: 'getting' }))).on('team_knowledge.share', ok(status({ busy: 'sharing' }))) });
    const c = new TeamController(handle, WS);
    await c.get();
    await c.get(uid('f2'));
    await c.share();
    expect(transport.sentOf('team_knowledge.get').map((e) => e.input)).toEqual([{ ws: WS }, { ws: WS, folder_id: uid('f2') }]);
    expect(c.busy).toBe(true);
    expect(t('team-busy-getting')).toBe('Getting what the team has learned…');
  });

  it('a refusal is kept for the dialog and Try again re-sends the command', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.add_read_folder', err('TEAM_NO_ACCESS', 'You cannot open that folder.', { action: 'try_again' })) });
    const c = new TeamController(handle, WS);
    expect(await c.addReadFolder({ kind: 'link', source_link_id: uid('x') })).toBe(false);
    expect(c.error?.message).toBe('You cannot open that folder.');
    await c.retry?.();
    expect(transport.sentOf('team_knowledge.add_read_folder')).toHaveLength(2);
  });

  it('Stop sharing sends turn_off and reads the status again', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.turn_off', ok({})).on('team_knowledge.status', ok(status())) });
    await new TeamController(handle, WS).turnOff();
    expect(transport.commandsSent()).toEqual(['team_knowledge.turn_off', 'team_knowledge.status']);
  });

  it('the status is read when a workspace_changed arrives (the controller reads again)', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.status', ok(status())) });
    const c = new TeamController(handle, WS);
    await c.load();
    await c.load();
    expect(transport.sentOf('team_knowledge.status')).toHaveLength(2);
  });
});

describe('the panel\'s sentences', () => {
  it('Happy path — new items from colleagues read with plurals: "8 new facts and 1 page from Priya and Sam"', () => {
    expect(t('team-available', { facts: 8, pages: 1, people: 'Priya and Sam' })).toBe('8 new facts and 1 page from Priya and Sam');
  });
  it('Edge case — facts waiting for a check', () => {
    expect(t('team-held-for-check', { count: 2 })).toBe('2 facts wait for you to check them');
  });
  it('Edge case — read-only access: the sentence beside the disabled Share button', () => {
    expect(t(teamErrorMessage('TEAM_READ_ONLY').id)).toBe("You can read this team's knowledge but not add to it.");
  });
  it('Error path — the repository cannot be reached: the sentence with Try again', () => {
    const m = teamErrorMessage('TEAM_REMOTE_UNREACHABLE');
    expect(t(m.id)).toBe("Your team's knowledge can't be reached just now.");
    expect(m.action).toBe('try_again');
  });
  it('TEAM_NOTHING_TO_SHARE is drawn as its message, not as an error', () => {
    expect(teamErrorMessage('TEAM_NOTHING_TO_SHARE')).toEqual({ id: 'team-nothing-to-share', action: null });
  });
  it('Edge case — the dialog says what will be shared before the first share', () => {
    expect(t('team-share-dialog-history')).toBe("What you share stays in the repository's history, even after you delete it.");
  });
});

describe('the panel drawn from a status', () => {
  const folder = (over: Record<string, unknown> = {}) => ({
    folder_id: uid('f1'), name: 'Security standards', set_by_company: true, may_stop: false, held: { facts: 3, pages: 1 }, problems: [], folder_large: false, access: 'read_only', bundle_kind: 'knowledge', ...over,
  });
  const draw = async (st: Record<string, unknown>) => {
    const { state } = await backendWith();
    return html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status(st) as never });
  };

  it('is not drawn when the backend does not answer the team knowledge group (SM-UP-1)', async () => {
    const { state } = await backendWith({ groups: ['service', 'memory'] });
    expect(html(TeamPanel, { backend: state, workspaceId: WS }).replace(/<!--.*?-->/g, '').trim()).toBe('');
  });

  it('Edge case — a folder set by the company cannot be stopped', async () => {
    const out = text(await draw({ binding: { folder: 'k' }, folders: [folder()] }));
    expect(out).toContain('Set by your company');
    expect(out).toContain('3 facts and 1 page');
    expect(out).not.toContain('Stop reading');
    expect(text(await draw({ binding: { folder: 'k' }, folders: [folder({ set_by_company: false, may_stop: true })] }))).toContain('Stop reading');
  });

  it('Edge case — no access to a folder set by the company: its sentence with no help button, and every other folder is drawn as before', async () => {
    const out = text(await draw({ binding: { folder: 'k' }, folders: [folder({ blocked: 'TEAM_COMPANY_FOLDER_NO_ACCESS' }), folder({ folder_id: uid('f2'), name: 'Platform notes', set_by_company: false })] }));
    expect(out).toContain("You can't open Security standards yet.");
    expect(out).not.toContain('Send to IT');
    expect(out).toContain('Platform notes');
  });

  it('Edge case — no adopt without an own folder: the panel says the workspace shares to no team folder', async () => {
    const out = text(await draw({ folders: [folder()] }));
    expect(out).toContain("This workspace reads knowledge your company names but doesn't share to a team folder.");
    expect(out).not.toContain('Share knowledge with a team…');
  });

  it('busy draws one progress line and disables Get them and Share', async () => {
    const out = await draw({ binding: { folder: 'k' }, busy: 'getting', available: { counts: { new_facts: 8, new_pages: 1 }, from: ['Priya', 'Sam'] } });
    expect(text(out)).toContain('Getting what the team has learned…');
    expect(text(out)).not.toContain('8 new facts');
    expect(out).toMatch(/<button[^>]*disabled[^>]*>(?:<svg[^>]*><use[^>]*><\/use><\/svg><!---->)?\s*Share Ordin/);
  });

  it('available items read with Get them; read-only access disables Share and ties its sentence to the button', async () => {
    const out = await draw({ binding: { folder: 'k' }, available: { counts: { new_facts: 8, new_pages: 1 }, from: ['Priya', 'Sam'] }, share_unavailable: 'TEAM_READ_ONLY' });
    expect(text(out)).toContain('8 new facts and 1 page from Priya and Sam');
    expect(text(out)).toContain("Get your team's memory and knowledge pages");
    expect(text(out)).toContain("You can read this team's knowledge but not add to it.");
    expect(out).toContain('aria-describedby="team-share-unavailable"');
    expect(out).toMatch(/<button[^>]*disabled/);
  });

  it('last_error TEAM_REMOTE_UNREACHABLE draws the sentence with Try again; TEAM_NOTHING_TO_SHARE is not an error', async () => {
    const bad = text(await draw({ binding: { folder: 'k' }, last_error: 'TEAM_REMOTE_UNREACHABLE' }));
    expect(bad).toContain("Your team's knowledge can't be reached just now.");
    expect(bad).toContain('Try again');
    const none = await draw({ binding: { folder: 'k' }, last_error: 'TEAM_NOTHING_TO_SHARE' });
    expect(text(none)).toContain('Nothing new to share.');
    expect(none).not.toContain('role="alert"');
  });

  it('problems list "Not taken in" and a large folder says so', async () => {
    const out = text(await draw({ binding: { folder: 'k' }, folder_large: true, problems: [{ code: 'TEAM_FILE_UNREADABLE', path: 'notes.md', first_seen_at: 'x' }, { code: 'TEAM_FACT_NOT_TAKEN', path: 'f.md', first_seen_at: 'x' }] }));
    expect(out).toContain('Not taken in');
    expect(out).toContain('notes.md could not be read.');
    expect(out).toContain('A shared fact was not taken in');
    expect(out).toContain("This team's folder is very large");
  });
});

// SM-TK-1, SM-TK-2, SM-TK-3 (amended January 2026, user decision): the panel names both memory and
// knowledge pages, and a share or get says what it carried. SM-TK-2 (amended again January 2026, user
// decision): a workspace with nothing shared shows one line pointing at the side tree, on Memory and Knowledge alike.
describe('memory and knowledge pages travel together', () => {
  const counts = (over: Record<string, number> = {}) => ({ new_facts: 0, changed_facts: 0, removed_facts: 0, new_pages: 0, changed_pages: 0, removed_pages: 0, ...over });
  const draw = async (st: Record<string, unknown>, props: Record<string, unknown> = {}) => {
    const { state } = await backendWith({ groups: TEAM_GROUPS });
    return text(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status(st) as never, ...props }));
  };

  it('SM-TK-1 — the share and get verbs name memory and knowledge pages, and the panel says what stays here', async () => {
    const out = await draw({ binding: { folder: 'k', access: 'read_write' }, available: { counts: counts({ new_facts: 2, new_pages: 1 }), from: ['Priya'] } });
    expect(out).toContain("Share Ordin's memory and knowledge pages with your team");
    expect(out).toContain("Get your team's memory and knowledge pages");
    expect(out).toContain('Facts, kinds of fact, the links between them, where each fact came from, and knowledge pages. Your conversations and your documents themselves stay on this computer.');
    expect(out).not.toContain("Share what we've learned");
    expect(out).not.toContain('Get them');
  });

  it('SM-TK-1 — a team workspace leaves the what-stays line to its own share dialog', async () => {
    const out = await draw({ binding: { folder: 'k', access: 'read_write' }, team_workspace: { state: 'shared', places: [], agents_not_here: [], results: 'k', results_here_only: false, unsent_results: 0, pending_grants: 0 } });
    expect(out).not.toContain('Facts, kinds of fact');
  });

  it.each([
    ['not shared', {}, true],
    ['reading only company folders', { folders: [{ folder_id: uid('f1'), name: 'Security', set_by_company: true, may_stop: false, held: { facts: 1, pages: 0 }, problems: [], folder_large: false }] }, false],
    ['sharing its knowledge', { binding: { folder: 'k', access: 'read_write' } }, false],
  ])('SM-TK-2 (amended January 2026) — no pointer to the side tree, %s', async (_name, st, empty) => {
    const out = await draw(st);
    expect(out).not.toContain('Share this workspace from the side tree');
    if (empty) expect(out.replace(/<!--.*?-->/g, '').trim()).toBe('');
    expect(out).not.toContain('Share knowledge with a team…');
    expect(out).not.toContain("Your team can use this workspace's knowledge pages");
  });

  it('SM-TK-2 — no pointer when the backend does not answer the team knowledge group', async () => {
    const { state } = await backendWith({ groups: ['service', 'memory'] });
    expect(html(TeamPanel, { backend: state, workspaceId: WS }).replace(/<!--.*?-->/g, '').trim()).toBe('');
  });

  it('SM-TK-3 — the count lines read with plurals', () => {
    expect(t('team-share-outcome', { facts: 42, pages: 6 })).toBe('Your share of 42 facts and 6 pages is waiting for your approval in Needs you.');
    expect(t('team-share-outcome', { facts: 1, pages: 1 })).toBe('Your share of 1 fact and 1 page is waiting for your approval in Needs you.');
    expect(t('team-get-outcome', { facts: 12, pages: 2 })).toBe('Got 12 facts and 2 pages from your team.');
  });

  it('SM-TK-3 — teamOutcome counts new and changed items of the report the command left', () => {
    const before = JSON.stringify(null);
    const proposed = { share_id: uid('s1'), approval_id: uid('a1'), held_for_check: 0, counts: counts({ new_facts: 40, changed_facts: 2, removed_facts: 5, new_pages: 6 }) };
    expect(teamOutcome('share', before, status({ last_share: proposed }) as never)).toEqual({ kind: 'share', facts: 42, pages: 6 });
    expect(teamOutcome('share', before, status({ last_share: proposed, busy: 'sharing' }) as never)).toBeNull();
    expect(teamOutcome('share', before, status({ last_share: proposed, last_error: 'TEAM_REMOTE_UNREACHABLE' }) as never)).toBeNull();
    expect(teamOutcome('share', JSON.stringify(proposed), status({ last_share: proposed }) as never)).toBeNull();
    expect(teamOutcome('get', before, status({ last_get: { commit: 'c', not_taken: 0, skipped_files: 0, counts: counts({ removed_facts: 3 }) } }) as never)).toBeNull();
  });

  it('SM-TK-3 — Happy path: a share says what it carried once its status arrives without busy', async () => {
    const proposed = { share_id: uid('s1'), approval_id: uid('a1'), held_for_check: 0, counts: counts({ new_facts: 42, new_pages: 6 }) };
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.share', ok(status({ binding: { folder: 'k' }, busy: 'sharing' }))) });
    const c = new TeamController(handle, WS);
    await c.share();
    expect(c.outcome).toBeNull();
    transport.on('team_knowledge.status', ok(status({ binding: { folder: 'k' }, last_share: proposed })));
    await c.load();
    expect(c.outcome).toEqual({ kind: 'share', facts: 42, pages: 6 });
    expect(stripIsolates(c.announcer.text)).toBe('Your share of 42 facts and 6 pages is waiting for your approval in Needs you.');
  });

  it('SM-TK-3 — a get of the own folder says what came; a get of another folder and a failed get say nothing', async () => {
    const got = { commit: 'c2', not_taken: 0, skipped_files: 0, counts: counts({ new_facts: 10, changed_facts: 2, new_pages: 2 }) };
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.get', ok(status({ binding: { folder: 'k' }, busy: 'getting' }))) });
    const c = new TeamController(handle, WS);
    await c.get();
    transport.on('team_knowledge.status', ok(status({ binding: { folder: 'k' }, last_get: got })));
    await c.load();
    expect(c.outcome).toEqual({ kind: 'get', facts: 12, pages: 2 });

    await c.get(uid('f2'));
    expect(c.outcome).toEqual({ kind: 'get', facts: 12, pages: 2 });
    await c.get();
    expect(c.outcome).toBeNull();
    transport.on('team_knowledge.status', ok(status({ binding: { folder: 'k' }, last_get: got, last_error: 'TEAM_REMOTE_UNREACHABLE' })));
    await c.load();
    expect(c.outcome).toBeNull();
  });
});
