// A team workspace (ScreensModule.md SM-TW-1 to SM-TW-12, ADR-121): the sentences of the access card and of
// Team activity, the panel drawn from a status, a connected server, and the no-Git-words rule. The dialogs,
// the card's buttons, the tree and the side rail are mounted in fixed-team-workspace-dom.test.ts.
import { beforeAll, describe, expect, it } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { makeKey, screenKeyFor, SCREEN_IDS } from '../src/lib/shell/screens';
import TeamActivityTreeRow from '../src/lib/shell/TeamActivityTreeRow.svelte';
import { teamErrorMessage, TeamController } from '../src/lib/views/fixed/team.svelte';
import {
  accessPhrase,
  activityLine,
  grantLine,
  nameAllowed,
  notSharedYet,
  offersTeamWorkspace,
  TeamActivityList,
  TeamGrants,
  teamWorkspaceProblem,
} from '../src/lib/views/fixed/team-workspace.svelte';
import TeamPanel from '../src/lib/views/fixed/TeamPanel.svelte';
import { err, ok } from './support/fake-backend';
import { uid } from './support/ids';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { entry, grant, status, TEAM_GROUPS, teamWs } from './team-workspace-support';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));
const plain = (s: string): string => stripIsolates(s);

const ftl = import.meta.glob('../../../../crates/ordin-localisation/catalogues/en-GB/*.ftl', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('the access card\'s sentence (SM-TW-6)', () => {
  it('Security — the card asks before anything is granted: "priya added Supplier analyst, which can read and change Contracts — Allow?"', () => {
    expect(plain(grantLine(grant()))).toBe('priya added Supplier analyst, which can read and change Contracts — Allow?');
  });

  it('Security — an unconfirmed name on the access card says so', () => {
    const line = plain(grantLine(grant({ agent_name: 'Contract checker', added_by_confirmed: false })));
    expect(line).toBe('Someone added Contract checker, which can read and change Contracts — Allow? (says it was priya)');
  });

  it('more access to an agent already here reads team-ws-grant-more-access', () => {
    expect(plain(grantLine(grant({ new_agent: false })))).toBe('priya gave Supplier analyst more access: it can read and change Contracts — Allow?');
  });

  it.each([
    [[{ place_name: 'Contracts', access: 'read_and_change' }], 'read and change Contracts'],
    [[{ place_name: 'Notes', access: 'read' }], 'read Notes'],
    [[{ place_name: 'Contracts', access: 'read_and_change' }, { place_name: 'Quotes', access: 'read_and_change' }], 'read and change Contracts and Quotes'],
    [[{ place_name: 'Contracts', access: 'read_and_change' }, { place_name: 'Notes', access: 'read' }], 'read and change Contracts, and read Notes'],
  ])('table: the access phrase groups entries by access (%j)', (lines, phrase) => {
    expect(plain(accessPhrase(lines as never))).toBe(phrase);
  });
});

describe('Team activity lines (SM-TW-8)', () => {
  it('Edge — an entry with no title: "sam ran a task with Supplier analyst"', () => {
    expect(plain(activityLine(entry({ task: null })))).toBe('sam ran a task with Supplier analyst');
  });

  it('Security — a name the history does not record says so', () => {
    expect(plain(activityLine(entry({ by: 'priya', by_confirmed: false })))).toBe('Someone ran Compare documents with Supplier analyst (says it was priya)');
  });

  it('this computer\'s entry not yet shared reads "You" and carries Not shared yet', () => {
    const mine = entry({ by: null, mine: true });
    expect(plain(activityLine(mine))).toBe('You ran Compare documents with Supplier analyst');
    expect(notSharedYet(mine)).toBe(true);
    expect(notSharedYet(entry())).toBe(false);
  });

  it('a replaced change names both people, each unconfirmed name followed by its own note', () => {
    const replaced = entry({ kind: 'replaced', by: 'sam', by_confirmed: false, replaced_by: 'priya', replaced_by_confirmed: false, subject: 'Contracts', task: null, outcome: null });
    expect(plain(activityLine(replaced))).toBe("Someone's change to Contracts was replaced by Someone's later share (says it was sam) (says it was priya)");
    expect(plain(activityLine(entry({ kind: 'replaced', by: 'sam', replaced_by: 'priya', replaced_by_confirmed: true, subject: 'Contracts' })))).toBe("sam's change to Contracts was replaced by priya's later share");
  });

  it('Show earlier reads the next page with the cursor and adds it after the first', async () => {
    const pages = [
      { items: [entry({ id: uid('a1') })], next_cursor: 'c2' },
      { items: [entry({ id: uid('a2') })], next_cursor: null },
    ];
    let n = 0;
    const { handle, transport } = await backendWith({ groups: TEAM_GROUPS, setup: (tr) => tr.on('team_knowledge.activity', () => ok(pages[n++])) });
    const list = new TeamActivityList(handle, WS);
    await list.load();
    expect(list.next).toBe('c2');
    await list.more();
    expect(list.entries.map((e) => e.id)).toEqual([uid('a1'), uid('a2')]);
    expect(list.next).toBeNull();
    expect(transport.sentOf('team_knowledge.activity').map((e) => e.input)).toEqual([{ ws: WS }, { ws: WS, cursor: 'c2' }]);
  });
});

describe('the waiting grants and their refusals (SM-TW-6)', () => {
  it('Allow sends allow_grant with the grant\'s id, closes the card and announces it', async () => {
    const { handle, transport } = await backendWith({ groups: TEAM_GROUPS, setup: (tr) => tr.on('team_knowledge.pending_grants', ok([grant()])).on('team_knowledge.allow_grant', ok(status())) });
    const grants = new TeamGrants(handle, WS);
    await grants.load();
    expect(await grants.decide(grants.list[0] as never, true)).toBe(true);
    expect(transport.sentOf('team_knowledge.allow_grant')[0]?.input).toEqual({ ws: WS, grant_id: uid('g1') });
    expect(grants.list).toEqual([]);
    await Promise.resolve();
    expect(grants.announcer.text).toBe('Allowed. Supplier analyst can use what your team gave it.');
  });

  it('Don\'t allow sends decline_grant and announces that nothing changed', async () => {
    const { handle, transport } = await backendWith({ groups: TEAM_GROUPS, setup: (tr) => tr.on('team_knowledge.decline_grant', ok(status())) });
    const grants = new TeamGrants(handle, WS);
    grants.list = [grant()];
    await grants.decide(grant(), false);
    expect(transport.sentOf('team_knowledge.decline_grant')[0]?.input).toEqual({ ws: WS, grant_id: uid('g1') });
    await Promise.resolve();
    expect(grants.announcer.text).toBe('Not allowed. Nothing changed on this computer.');
  });

  it.each(['TEAM_WS_AGENT_NOT_ADDED', 'TEAM_WS_ALLOW_FAILED'])('Error — %s stays in the card and the card stays open', async (code) => {
    const { handle } = await backendWith({ groups: TEAM_GROUPS, setup: (tr) => tr.on('team_knowledge.allow_grant', err(code, 'The sentence.', code === 'TEAM_WS_ALLOW_FAILED' ? { action: 'try_again' } : {})) });
    const grants = new TeamGrants(handle, WS);
    grants.list = [grant()];
    expect(await grants.decide(grant(), true)).toBe(false);
    expect(grants.errors[uid('g1')]?.code).toBe(code);
    expect(grants.list).toHaveLength(1);
  });

  it('Error — TEAM_WS_GRANT_NOT_FOUND reads the list again', async () => {
    const { handle, transport } = await backendWith({
      groups: TEAM_GROUPS,
      setup: (tr) => tr.on('team_knowledge.allow_grant', err('TEAM_WS_GRANT_NOT_FOUND', 'This was already answered.')).on('team_knowledge.pending_grants', ok([])),
    });
    const grants = new TeamGrants(handle, WS);
    grants.list = [grant()];
    await grants.decide(grant(), true);
    expect(transport.commandsSent()).toEqual(['team_knowledge.allow_grant', 'team_knowledge.pending_grants']);
    expect(grants.list).toEqual([]);
    expect(grants.goneError?.message).toBe('This was already answered.');
  });
});

describe('the panel of a team workspace (SM-TW-4, SM-TW-5, SM-TW-11)', () => {
  const draw = async (st: Record<string, unknown>, kind: 'local' | 'server' = 'local') => {
    const { state } = await backendWith({ groups: TEAM_GROUPS, kind });
    return html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status(st) as never });
  };

  it('the header says Team workspace, and Waiting for your approval while proposed', async () => {
    const out = text(await draw({ team_workspace: teamWs({ state: 'proposed' }) }));
    expect(out).toContain('Team workspace: acme/supplier-team › knowledge');
    expect(out).toContain('Waiting for your approval to share');
    expect(out).not.toContain('Team knowledge: acme');
  });

  it('one line per place, agent and count that applies, results with Change…, and Leave in place of Stop sharing', async () => {
    const out = text(
      await draw({
        team_workspace: teamWs({
          places: [
            { key: 'p1', display_name: 'Contracts', state: 'no_access' },
            { key: 'p2', display_name: 'Quotes', state: 'removal_waits' },
            { key: 'p3', display_name: 'Rates', state: 'waiting' },
            { key: 'p4', display_name: 'Linked', state: 'linked' },
          ],
          agents_not_here: [{ key: 'a1', name: 'Contract checker', access: [] }],
          results_here_only: true,
          unsent_results: 2,
          pending_grants: 1,
        }),
      }),
    );
    expect(out).toContain("Contracts: you don't have access");
    expect(out).toContain('Quotes was removed by your team; it stays until your changes in it are sent.');
    expect(out).toContain("Rates can't be reached just now; ordin tries again later.");
    expect(out).not.toContain('Linked:');
    expect(out).toContain("Contract checker isn't on this computer.");
    expect(out).toContain('Results are saved in acme/supplier-team › knowledge › results');
    expect(out).toContain('Change…');
    expect(out).toContain('Your results stay on this computer');
    expect(out).toContain('2 results wait to be shared');
    expect(out).toContain('1 change by your team waits for you');
    expect(out).toContain('Review');
    expect(out).toContain('Leave the team workspace');
    expect(out).not.toContain('Stop sharing with the team');
    expect(out).not.toContain('Share as a team workspace…');
  });

  // SM-TK-2 and SM-TW-14 (amended January 2026, user decision): knowledge-only sharing is not offered on its own.
  // SM-TK-2 (amended January 2026; user decision; Approved by the user January 2026): the pointer line is gone.
  it('SM-TK-2 — a workspace with nothing shared draws nothing, and no share control', async () => {
    const out = text(await draw({ binding: null, header_line: null }));
    expect(out.trim()).toBe('');
    expect(out).not.toContain('Share knowledge with a team…');
    expect(out).not.toContain('Share as a team workspace…');
  });

  it('SM-TW-14 — a workspace that shares its knowledge only keeps working and is offered Share the whole workspace', async () => {
    const out = text(await draw({}));
    expect(out).toContain('Team knowledge: acme/supplier-team › knowledge');
    expect(out).toContain("Only this workspace's knowledge is shared with your team.");
    expect(out).toContain('Share the whole workspace');
    expect(out).toContain("Share Ordin's memory and knowledge pages with your team");
    expect(out).not.toContain('Share as a team workspace…');
    expect(text(await draw({ team_workspace: teamWs() }))).not.toContain('Share the whole workspace');
  });

  it('opening draws Opening the team workspace… until the status comes without busy', async () => {
    expect(text(await draw({ busy: 'opening', team_workspace: teamWs() }))).toContain('Opening the team workspace…');
  });

  it('SM-TW-11 — last_error TEAM_WS_FOLDER_TAKEN draws its sentence, never the code', async () => {
    expect(teamErrorMessage('TEAM_WS_FOLDER_TAKEN')).toEqual({ id: 'team-ws-error-folder-taken', action: null });
    const out = await draw({ last_error: 'TEAM_WS_FOLDER_TAKEN' });
    expect(text(out)).toContain('This folder already holds a team workspace. Open it with Open a team workspace… instead.');
    expect(out).not.toContain('TEAM_WS_FOLDER_TAKEN');
  });

  it.each([
    ['TEAM_WS_RESULTS_NO_ACCESS', '', "You can't open the place where the team saves results, so your results stay on this computer."],
    ['TEAM_WS_RESULT_TOO_LARGE', 'big.pdf', 'big.pdf is too large to share; it stays on this computer.'],
    ['TEAM_WS_RESULT_KEPT', 'quote (2).md', 'A colleague saved a result with the same name, so both are kept; yours is now quote (2).md.'],
    ['TEAM_WS_RESULT_NOT_TAKEN', 'quote.md', "A colleague's quote.md wasn't brought here because yours has the same name. Rename yours, then get your team's memory and knowledge pages again."],
    ['TEAM_WS_DESCRIPTION_UNREADABLE', '', "This team workspace's settings can't be read, so your team's latest changes to it weren't taken in."],
    ['TEAM_WS_FORMAT_NEWER', '', "This team workspace needs a newer ordin to take in your team's latest changes."],
    ['TEAM_WS_NAME_IN_USE', '', 'Your team renamed this workspace, but another workspace here already has that name.'],
  ])('SM-TW-11 — the problem %s is drawn in Not taken in as its sentence', async (code, path, sentence) => {
    expect(plain(teamWorkspaceProblem({ code, path }) ?? '')).toBe(sentence);
    const out = text(await draw({ problems: [{ code, path, first_seen_at: '2026-01-20T09:30:02.114Z' }] }));
    expect(out).toContain('Not taken in');
    expect(out).toContain(sentence);
  });

  it('any other problem code keeps the Team knowledge rule', () => {
    expect(teamWorkspaceProblem({ code: 'TEAM_FILE_UNREADABLE', path: 'x' })).toBeNull();
  });
});

describe('a workspace on a connected server (SM-TW-12)', () => {
  it('Edge — its menu has no "Share as a team workspace…" and its tree has no "Team activity" row', async () => {
    const { state } = await backendWith({ groups: TEAM_GROUPS, kind: 'server' });
    expect(offersTeamWorkspace(state, 'team_knowledge.share_workspace')).toBe(false);
    expect(offersTeamWorkspace(state, 'team_knowledge.activity')).toBe(false);
    const panel = text(html(TeamPanel, { backend: state, workspaceId: WS, initialStatus: status({ binding: null, header_line: null, team_workspace: teamWs({ pending_grants: 1 }) }) as never }));
    expect(panel).not.toContain('Share knowledge with a team…');
    expect(panel).not.toContain('Share the whole workspace');
    expect(panel).not.toContain('Share this workspace from the side tree');
    expect(panel).not.toContain('Team workspace:');
    expect(panel).not.toContain('Leave the team workspace');
    const { app } = await appWith();
    const row = html(TeamActivityTreeRow, { backend: state, workspaceId: WS }, app);
    expect(text(row).trim()).toBe('');
    // The local backend offers both when it answers the group.
    const local = await backendWith({ groups: TEAM_GROUPS });
    expect(offersTeamWorkspace(local.state, 'team_knowledge.share_workspace')).toBe(true);
  });
});

describe('opening, Change… and Leave through the controller', () => {
  it('SM-TW-5 — Change… sends set_results_place with the chosen place', async () => {
    const { handle, transport } = await backendWith({ groups: TEAM_GROUPS, setup: (tr) => tr.on('team_knowledge.set_results_place', ok(status())) });
    await new TeamController(handle, WS).setResultsPlace({ kind: 'link', source_link_id: uid('sl2') });
    expect(transport.sentOf('team_knowledge.set_results_place')[0]?.input).toEqual({ ws: WS, results: { kind: 'link', source_link_id: uid('sl2') } });
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TW-5
  it('SM-TW-5 — Change… reads its choices from results_choices (amended January 2026, the user\'s decision)', async () => {
    const answer = { choices: [{ choice: { kind: 'team_folder' }, name: 'acme/supplier-team › knowledge › results' }], current: { kind: 'team_folder' } };
    const { handle, transport } = await backendWith({ groups: TEAM_GROUPS, setup: (tr) => tr.on('team_knowledge.results_choices', ok(answer)) });
    const c = new TeamController(handle, WS);
    expect(await c.resultsChoices()).toEqual(answer);
    expect(transport.sentOf('team_knowledge.results_choices')[0]?.input).toEqual({ ws: WS });
    expect(transport.sentOf('team_knowledge.workspace_share_preview')).toHaveLength(0);
  });

  it('SM-TW-10 — TEAM_SHARE_PENDING is kept for the confirmation through ErrorLine', async () => {
    const { handle } = await backendWith({ groups: TEAM_GROUPS, setup: (tr) => tr.on('team_knowledge.leave_workspace', err('TEAM_SHARE_PENDING', 'Your last share is still waiting.')) });
    const c = new TeamController(handle, WS);
    expect(await c.leaveWorkspace()).toBe(false);
    expect(c.error?.code).toBe('TEAM_SHARE_PENDING');
  });

  it('the open preview\'s name is 1 to 80 characters after trimming', () => {
    expect(nameAllowed('Q3 supplier review')).toBe(true);
    expect(nameAllowed('   ')).toBe(false);
    expect(nameAllowed('x'.repeat(80))).toBe(true);
    expect(nameAllowed('x'.repeat(81))).toBe(false);
  });
});

describe('Needs you opens the access card (HomeAndSearch.md HS-TW-2)', () => {
  it('OpenTarget team_grant opens Team activity with that grant', () => {
    expect(screenKeyFor({ kind: 'team_grant', workspace_id: WS, grant_id: uid('g1') })).toBe(makeKey(SCREEN_IDS.teamActivity, uid('g1')));
    expect(SCREEN_IDS.teamActivity).toBe('team-activity');
  });
});

describe('the words of a team workspace', () => {
  it('Edge — no Git words: no team-ws-* message names branch, commit, push, pull, merge, rebase, conflict or clone', () => {
    const banned = /\b(branch|commit|push|pull|merge|rebase|conflict|clone)\b/i;
    const all = Object.values(ftl).join('\n');
    const ids = [...all.matchAll(/^(team-ws-[a-z0-9-]+)\s*=/gm)].map((m) => m[1] as string);
    expect(ids.length).toBeGreaterThanOrEqual(75);
    const rendered = ids.map((id) => [id, t(id, { workspace: 'W', folder: 'F', places: 'P', names: 'N', agent: 'A', access: 'X', facts: 2, pages: 2, place: 'P', count: 2, person: 'Q', first: 'F', second: 'S', task: 'T', subject: 'S', other: 'O', file: 'F' })] as const);
    expect(rendered.filter(([, text]) => banned.test(text)).map(([id]) => id)).toEqual([]);
  });

  it('the spec\'s quoted sentences render as written', () => {
    expect(t('team-ws-preview-places', { places: 'Contracts' })).toBe('Places everyone will link, each with their own sign-in: Contracts');
    expect(t('team-ws-preview-knowledge', { facts: 1, pages: 3 })).toBe('1 fact and 3 pages');
    expect(t('team-ws-left')).toBe('You left the team workspace. Everything is still here.');
    expect(t('tree-team-activity')).toBe('Team activity');
    expect(t('screen-team-activity')).toBe('Team activity');
    expect(t('rail-open-team-workspace')).toBe('Open a team workspace…');
  });
});
