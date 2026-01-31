// Check it (SA-CLOSE-01), Knowledge page links (ADR-104), Jira pieces (ADR-106), the Slack channel screen
// and the registration of the fixed screens (SM-UP-1).
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { answersAll } from '../src/lib/backend/groups';
import { stripIsolates, t as rawT, i18n } from '../src/lib/i18n/fluent';
import { clearScreens, getScreen, SCREEN_IDS } from '../src/lib/shell/screens';
import { CheckItController, forgetCheckRuns, orderedFindings } from '../src/lib/views/fixed/check-it.svelte';
import CheckItPanel from '../src/lib/views/fixed/CheckItPanel.svelte';
import { KnowledgeController, parseLines, parseTags } from '../src/lib/views/fixed/knowledge-controller.svelte';
import { splitWikiLinks, linkShown, tagFilter } from '../src/lib/views/fixed/wikilinks';
import { HandoverJiraController, JiraAddController, issueAddress, jiraOffered } from '../src/lib/views/fixed/jira.svelte';
import JiraAddDialog from '../src/lib/views/fixed/JiraAddDialog.svelte';
import HandoverJira from '../src/lib/views/fixed/HandoverJira.svelte';
import { isSlackPermalink, openSlackPermalink } from '../src/lib/views/fixed/slack-link';
import { SlackController } from '../src/lib/views/fixed/slack-controller.svelte';
import SlackChannel from '../src/lib/views/fixed/SlackChannel.svelte';
import { err, ok, R1_GROUPS } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));
beforeEach(() => forgetCheckRuns());

// ---- Check it -----------------------------------------------------------------------------------

const report = (findings: unknown[] = [], over: Record<string, unknown> = {}) => ({
  check_run_id: uid('run-1'), checked_at: '2026-01-16T10:00:00Z', passed: findings.length === 0, findings, agents_that_stop: [], publish_gate_met: true, tidied: [], version_checked: '1', subject: { kind: 'installed' }, ...over,
});
const finding = (severity: 'failure' | 'warning', message_id: string, over: Record<string, unknown> = {}) => ({ args: {}, code: message_id, detail: '', fixes: [], message_id, severity, ...over });

describe('Check it', () => {
  it('Happy path — Check it on an installed agent: checker.check_installed with that agent_instance_id; a clean report reads "Every check passed."', async () => {
    const { handle, transport, state } = await backendWith({ setup: (tr) => tr.on('checker.check_installed', ok(report())) });
    const c = new CheckItController(handle, { kind: 'installed', agentInstanceId: '0192a1c9-aaaa-7bbb-8ccc-ddddeeee0002' });
    await c.check();
    expect(transport.sentOf('checker.check_installed')[0]?.input).toEqual({ agent_instance_id: '0192a1c9-aaaa-7bbb-8ccc-ddddeeee0002' });
    expect(c.report?.findings).toEqual([]);
    const panel = text(html(CheckItPanel, { backend: state, subject: { kind: 'installed', agentInstanceId: 'x' } }));
    expect(panel).toContain('Check it');
    expect(t('check-checked-by-ordin')).toBe('Checked by ordin');
    expect(t('check-all-passed')).toBe('Every check passed.');
  });

  it('Happy path — Check it on a library part: no agent_repository_id, kind and identifier; a finding is one line naming the part', async () => {
    const { handle, transport } = await backendWith({
      setup: (tr) => tr.on('checker.check_library_item', ok(report([finding('failure', 'check-trust-needs-it-approval', { part_identifier: 'urn:ordin:acme:tool-bindings:jira' })]))),
    });
    const c = new CheckItController(handle, { kind: 'library', partKind: 'ToolBinding', identifier: 'urn:ordin:acme:tool-bindings:jira' });
    await c.check();
    expect(transport.sentOf('checker.check_library_item')[0]?.input).toEqual({ kind: 'ToolBinding', identifier: 'urn:ordin:acme:tool-bindings:jira' });
    expect(c.report?.findings[0]?.message_id).toBe('check-trust-needs-it-approval');
    expect(c.report?.findings[0]?.part_identifier).toBe('urn:ordin:acme:tool-bindings:jira');
  });

  it('a library part of a repository sends its agent_repository_id', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('checker.check_library_item', ok(report())) });
    await new CheckItController(handle, { kind: 'library', agentRepositoryId: uid('repo-1'), partKind: 'Skill', identifier: 'urn:x' }).check();
    expect(transport.sentOf('checker.check_library_item')[0]?.input).toMatchObject({ agent_repository_id: uid('repo-1') });
  });

  it('Error path — Check it cannot read the repository: the error line shows the message and Try again, and no "Every check passed."', async () => {
    const { handle, state } = await backendWith({ setup: (tr) => tr.on('checker.check_library_item', err('CHK_UNREADABLE', "Can't reach the company's agent library right now", { action: 'try_again' })) });
    const subject = { kind: 'library' as const, partKind: 'Skill' as const, identifier: 'urn:x' };
    const c = new CheckItController(handle, subject);
    await c.check();
    expect(c.report).toBeNull();
    expect(c.error?.message).toBe("Can't reach the company's agent library right now");
    expect(c.error?.action).toBe('try_again');
  });

  it('a failure finding is marked before warnings', () => {
    const ordered = orderedFindings([finding('warning', 'w1') as never, finding('failure', 'f1') as never, finding('warning', 'w2') as never, finding('failure', 'f2') as never]);
    expect(ordered.map((f) => f.message_id)).toEqual(['f1', 'f2', 'w1', 'w2']);
  });

  it('the panel keeps the last check_run_id for the page and reopens it with checker.report', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('checker.check_installed', ok(report())).on('checker.report', ok(report())) });
    const subject = { kind: 'installed' as const, agentInstanceId: uid('a1') };
    await new CheckItController(handle, subject).check();
    const reopened = new CheckItController(handle, subject);
    await reopened.reopen();
    expect(transport.sentOf('checker.report')[0]?.input).toEqual({ check_run_id: uid('run-1') });
    expect(reopened.report?.check_run_id).toBe(uid('run-1'));
    const other = new CheckItController(handle, { kind: 'installed', agentInstanceId: 'a2' });
    await other.reopen();
    expect(transport.sentOf('checker.report')).toHaveLength(1);
  });

  it('is not drawn when the backend does not answer the checker group', async () => {
    const { state } = await backendWith({ groups: ['service'] });
    expect(html(CheckItPanel, { backend: state, subject: { kind: 'installed', agentInstanceId: uid('a1') } }).replace(/<!--.*?-->/g, '').trim()).toBe('');
  });
});

// ---- Knowledge page links -------------------------------------------------------------------------

const page = (over: Record<string, unknown> = {}) => ({
  page: { id: uid('p1'), title: 'Q3 review notes', body_markdown: 'See [[Supplier rates#Review cycle]] and [[Purchasing policy]].', last_changed_at: '2026-01-06T00:00:00Z' },
  aliases: [], tags: [], links_in: [], links_out: [], links_truncated: 0, suggestions: [], unregistered_aliases: [], changes: [], cites_other_computer: false, read_only: false, ...over,
});

describe('Knowledge — page links', () => {
  it('wiki links are drawn by their shown text', () => {
    expect(linkShown('target|shown')).toBe('shown');
    expect(linkShown('target#section')).toBe('target › section');
    expect(linkShown('target')).toBe('target');
    const segs = splitWikiLinks('a ![[Image page]] b [[X|y]]', [{ name: 'Image page', target: { page_id: 'p9', title: 'Image page' } } as never, { name: 'X' } as never]);
    expect(segs.map((s) => (s.kind === 'text' ? s.text : `<${s.shown}>`))).toEqual(['a ', '<Image page>', ' b ', '<y>']);
    expect(segs[3]).toMatchObject({ kind: 'link', link: { name: 'X' } });
  });

  it('Happy path — a link opens the page it names, scrolled to its heading', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('knowledge.get', ok(page())) });
    const c = new KnowledgeController(handle, WS);
    await c.openLink({ name: 'Supplier rates', section: 'Review cycle', target: { page_id: uid('p2'), title: 'Supplier rates' } });
    expect(transport.sentOf('knowledge.get')[0]?.input).toEqual({ page_id: uid('p2') });
    expect(c.scrollTo).toBe('Review cycle');
  });

  it('Edge case — a link to no page is drawn muted with the tooltip and opens nothing', async () => {
    const { handle, transport } = await backendWith();
    await new KnowledgeController(handle, WS).openLink({ name: 'Purchasing policy' } as never);
    expect(transport.sentOf('knowledge.get')).toHaveLength(0);
    expect(t('knowledge-link-missing', { name: 'Purchasing policy' })).toBe('No page called Purchasing policy');
  });

  it('Happy path — accepting a suggestion sends the suggestion id and the page is read again', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('knowledge.get', ok(page())).on('knowledge.accept_suggestion', ok({})) });
    const c = new KnowledgeController(handle, WS);
    await c.open(uid('p1'));
    transport.sent.length = 0;
    await c.acceptSuggestion({ id: uid('s1'), kind: 'link', other: { page_id: uid('p2'), title: 'Supplier rates' }, matched_text: 'supplier rates' } as never);
    expect(transport.commandsSent()).toEqual(['knowledge.accept_suggestion', 'knowledge.get']);
    expect(transport.sentOf('knowledge.accept_suggestion')[0]?.input).toEqual({ page_id: uid('p1'), suggestion_id: uid('s1') });
  });

  it('KNOW_SUGGESTION_STALE is drawn as its sentence and the page is read again', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('knowledge.get', ok(page())).on('knowledge.dismiss_suggestion', err('KNOW_SUGGESTION_STALE', 'That suggestion is out of date.')) });
    const c = new KnowledgeController(handle, WS);
    await c.open(uid('p1'));
    await c.dismissSuggestion({ id: uid('s1'), kind: 'duplicate', other: { page_id: uid('p2'), title: 'x' } } as never);
    expect(c.error?.message).toBe('That suggestion is out of date.');
    expect(transport.sentOf('knowledge.get')).toHaveLength(2);
  });

  it('Error path — a tag with a space: the owner\'s sentence is shown beside Tags, with the edit kept', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('knowledge.get', ok(page())).on('knowledge.save_edit', err('KNOW_TAG_INVALID', 'A tag cannot contain a space.')) });
    const c = new KnowledgeController(handle, WS);
    await c.open(uid('p1'));
    expect(await c.save({ title: 'Q3 review notes', body_markdown: 'x', tags: ['supplier rates'], aliases: [] })).toBe(false);
    expect(c.editError?.message).toBe('A tag cannot contain a space.');
    expect(transport.sentOf('knowledge.save_edit')[0]?.input).toMatchObject({ tags: ['supplier rates'], aliases: [] });
  });

  it('tags and other names are sent only when either field changed', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('knowledge.get', ok(page({ tags: ['a'], aliases: ['b'] }))).on('knowledge.save_edit', ok({})) });
    const c = new KnowledgeController(handle, WS);
    await c.open(uid('p1'));
    await c.save({ title: 'Q3 review notes', body_markdown: 'new', tags: ['a'], aliases: ['b'] });
    expect(Object.keys(transport.sentOf('knowledge.save_edit')[0]?.input as object)).not.toContain('tags');
    await c.save({ title: 'Q3 review notes', body_markdown: 'new', tags: ['a', 'c'], aliases: ['b'] });
    expect(transport.sentOf('knowledge.save_edit')[1]?.input).toMatchObject({ tags: ['a', 'c'], aliases: ['b'] });
  });

  it('a rename leaves the notice with the number of pages that linked here', async () => {
    const { handle } = await backendWith({ setup: (tr) => tr.on('knowledge.get', ok(page({ links_in: [{ page_id: 'p3', title: 'A' }, { page_id: 'p4', title: 'B' }] }))).on('knowledge.save_edit', ok({})) });
    const c = new KnowledgeController(handle, WS);
    await c.open(uid('p1'));
    await c.save({ title: 'New title', body_markdown: 'x', tags: [], aliases: [] });
    expect(c.renamed).toHaveLength(2);
    expect(t('knowledge-renamed-links', { count: 2 })).toBe('Links on 2 other pages were updated to the new title.');
  });

  it('the filter box sends its text, and a tag chip puts tag:<tag> in it', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('knowledge.list', ok({ this_workspace: [], from_agents: [], from_other_teams: [], not_adopted: [] })) });
    const c = new KnowledgeController(handle, WS);
    await c.load('state:out-of-date');
    await c.filterByTag('suppliers');
    expect(transport.sentOf('knowledge.list').map((e) => e.input)).toEqual([{ workspace_id: WS, filter: 'state:out-of-date' }, { workspace_id: WS, filter: 'tag:suppliers' }]);
    expect(c.filter).toBe(tagFilter('suppliers'));
    expect(parseLines('a\n\n b ')).toEqual(['a', 'b']);
    expect(parseTags('a, b ,,c')).toEqual(['a', 'b', 'c']);
  });
});

// ---- Jira ----------------------------------------------------------------------------------------

const issue = (key: string, summary = 'Rates') => ({ id: key, issue: { key, site: 'acme.atlassian.net' }, summary, labels: [], status_category: 'to_do', updated: '2026-01-06T00:00:00Z' });

describe('Jira pieces (ADR-106)', () => {
  it('Happy path — linking an issue from search: the picked row\'s address is sent', async () => {
    vi.useFakeTimers();
    try {
      const { handle, transport } = await backendWith({ setup: (tr) => tr.on('jira.find_issues', ok([issue('ABC-12')])).on('workspaces.add_link', ok({})) });
      const c = new JiraAddController(handle, WS);
      c.type('rates');
      await vi.advanceTimersByTimeAsync(299);
      expect(transport.sentOf('jira.find_issues')).toHaveLength(0);
      await vi.advanceTimersByTimeAsync(2);
      expect(transport.sentOf('jira.find_issues')[0]?.input).toEqual({ text: 'rates', limit: 25 });
      c.picked = 'ABC-12';
      expect(await c.add()).toBe(true);
      expect(transport.sentOf('workspaces.add_link')[0]?.input).toEqual({ workspace_id: WS, location: { kind: 'jira_issue', address: 'https://acme.atlassian.net/browse/ABC-12' } });
      expect(issueAddress(issue('ABC-12') as never)).toBe('https://acme.atlassian.net/browse/ABC-12');
    } finally {
      vi.useRealTimers();
    }
  });

  it('pasted text is sent as it was pasted', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('workspaces.add_link', ok({})) });
    const c = new JiraAddController(handle, WS);
    c.pasted = ' https://acme.atlassian.net/browse/ABC-1 ';
    await c.add();
    expect(transport.sentOf('workspaces.add_link')[0]?.input).toMatchObject({ location: { address: 'https://acme.atlassian.net/browse/ABC-1' } });
  });

  it('no match says so; not connected points to Settings; other refusals are drawn under the field', async () => {
    const { handle } = await backendWith({ setup: (tr) => tr.on('jira.find_issues', ok([])).on('workspaces.add_link', err('WS_SOURCE_SIGN_IN_NEEDED', 'Sign in.')) });
    const c = new JiraAddController(handle, WS);
    c.text = 'zzz';
    await c.search();
    expect(c.searched && c.results.length === 0).toBe(true);
    c.pasted = 'https://acme.atlassian.net/browse/ABC-1';
    await c.add();
    expect(c.notConnected).toBe(true);
    expect(c.error).toBeNull();
    expect(t('jira-add-not-connected')).toBe('Connect Jira in Settings to link an issue.');
    const dup = await backendWith({ setup: (tr) => tr.on('workspaces.add_link', err('SRC_DUPLICATE', 'Already linked.')) });
    const d = new JiraAddController(dup.handle, WS);
    d.pasted = 'x';
    await d.add();
    expect(d.error?.message).toBe('Already linked.');
  });

  it('Edge case — no Jira on a server\'s workspace: "Jira issue…" is not offered', async () => {
    const { state } = await backendWith({ kind: 'server' });
    expect(jiraOffered(state.handle)).toBe(false);
    expect(html(JiraAddDialog, { backend: state, workspaceId: WS, onclose: () => undefined }).replace(/<!--.*?-->/g, '').trim()).toBe('');
    const local = await backendWith();
    expect(text(html(JiraAddDialog, { backend: local.state, workspaceId: WS, onclose: () => undefined }))).toContain('Jira issue…');
  });

  const jira = (over: Record<string, unknown> = {}) => ({ line: 'Sam is assigned ABC-12.', options: [{ key: 'ABC-12', summary: 'Rates' }, { key: 'ABC-20', summary: 'Terms' }], project: 'Supplier onboarding', selection: { kind: 'linked', key: 'ABC-12' }, ...over });

  it('Happy path — choosing the issue a handover rides on: update_draft with create_new; the draft shows the line the reply carries', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('handover.update_draft', ok({ jira: jira({ selection: { kind: 'create_new' }, line: 'ordin creates a new issue in Supplier onboarding.' }) })) });
    const c = new HandoverJiraController(handle, uid('draft-1'), jira() as never);
    await c.choose({ kind: 'create_new' });
    expect(transport.sentOf('handover.update_draft')[0]?.input).toEqual({ draft: uid('draft-1'), jira_issue: { kind: 'create_new' } });
    expect(c.line).toBe('ordin creates a new issue in Supplier onboarding.');
    await c.choose({ kind: 'linked', key: 'ABC-20' });
    expect(transport.sentOf('handover.update_draft')[1]?.input).toEqual({ draft: uid('draft-1'), jira_issue: { kind: 'linked', key: 'ABC-20' } });
  });

  it('Edge case — no linked issue in the project: Use an existing issue… is offered, and handover.open_draft is sent again after a link', async () => {
    const { handle, transport, state } = await backendWith({ setup: (tr) => tr.on('handover.open_draft', ok({ jira: jira() })) });
    const empty = jira({ options: [], selection: { kind: 'create_new' } });
    const out = text(html(HandoverJira, { backend: state, draftId: 'd', jira: empty }));
    expect(out).toContain('Use an existing issue…');
    expect(out).toContain('A new issue will be created in Supplier onboarding');
    const c = new HandoverJiraController(handle, 'd', empty as never);
    await c.reopen(WS, ['r1']);
    expect(transport.sentOf('handover.open_draft')[0]?.input).toEqual({ workspace_id: WS, results: ['r1'] });
    expect(c.jira?.options).toHaveLength(2);
  });

  it('the issue list is a single choice, each option "<key> <summary>", and the line is drawn as text', async () => {
    const { state } = await backendWith();
    const out = html(HandoverJira, { backend: state, draftId: 'd', jira: jira({ line: '<b>Hi</b>' }) as never });
    expect(text(out)).toContain('Which issue?');
    expect(text(out)).toContain('ABC-12 Rates');
    expect(text(out)).toContain('ABC-20 Terms');
    expect(out).not.toContain('<b>Hi</b>');
  });
});

// ---- Slack channel screen ------------------------------------------------------------------------

describe('Slack channel screen', () => {
  it('only an https://<name>.slack.com/archives/… link opens', () => {
    expect(isSlackPermalink('https://acme.slack.com/archives/C123/p1')).toBe(true);
    for (const bad of ['http://acme.slack.com/archives/C1', 'https://evil.com/archives/C1', 'https://acme.slack.com.evil.com/archives/C1', 'https://acme.slack.com/messages/C1', 'javascript:alert(1)', 'https://u:p@acme.slack.com/archives/C1', '']) {
      expect(isSlackPermalink(bad), bad).toBe(false);
    }
  });

  it('A Slack permalink from the core is not https://*.slack.com/archives/…: not opened, UI_LINK_REFUSED recorded', async () => {
    const { handle, transport, diagnostics } = await backendWith({ setup: (tr) => tr.on('app.open_slack_link', ok({})) });
    expect(await openSlackPermalink(handle, 'https://evil.example/archives/C1', diagnostics)).toBe(false);
    expect(transport.sentOf('app.open_slack_link')).toHaveLength(0);
    expect(diagnostics.codes()).toEqual(['UI_LINK_REFUSED']);
    expect(await openSlackPermalink(handle, 'https://acme.slack.com/archives/C1/p1', diagnostics)).toBe(true);
    expect(transport.sentOf('app.open_slack_link')[0]?.input).toEqual({ url: 'https://acme.slack.com/archives/C1/p1' });
  });

  const link = (over: Record<string, unknown> = {}) => ({ id: uid('l1'), channel_id: 'C1', name: 'suppliers', ordin_reads: false, private: false, state: 'linked', team_id: 'T', workspace_id: WS, linked_at: 'x', ...over });
  const summary = (over: Record<string, unknown> = {}) => ({ kept_from: '2026-01-06T00:00:00Z', link: link(over), new_messages: 3, older_skipped: false, reactions_paused_today: false });

  it('links, messages, the Ordin-reads switch and Remove from this workspace send their commands', async () => {
    const { handle, transport } = await backendWith({
      setup: (tr) =>
        tr
          .on('slack.links', ok([summary()]))
          .on('slack.messages', ok({ items: [{ ts: '1', text: 'Please <b>act</b>', sender_name: 'Sam', sent_at: '2026-01-06T10:00:00Z', asks_agent_to_act: true, permalink: 'https://acme.slack.com/archives/C1/p1', channel_name: 'suppliers', from_person: false, link_id: uid('l1') }], next_cursor: null }))
          .on('slack.set_ordin_reads', ok(link({ ordin_reads: true })))
          .on('slack.unlink', ok({})),
    });
    const c = new SlackController(handle, WS);
    await c.loadLinks();
    await c.loadMessages(uid('l1'));
    expect(transport.sentOf('slack.messages')[0]?.input).toEqual({ ws: WS, link_id: uid('l1') });
    await c.setOrdinReads(uid('l1'), true);
    expect(transport.sentOf('slack.set_ordin_reads')[0]?.input).toEqual({ ws: WS, link_id: uid('l1'), on: true });
    expect(c.links[0]?.link.ordin_reads).toBe(true);
    expect(await c.unlink(uid('l1'))).toBe(true);
    expect(c.links).toEqual([]);
  });

  it('a message is other people\'s text: markup is literal and the instruction note is drawn', async () => {
    const { state } = await backendWith();
    // The screen reads in onMount; check the catalogue sentence and that the screen draws no raw markup path.
    expect(t('slack-message-instruction-note')).toBe("This message asks ordin to do something. It's shown as information only.");
    expect(html(SlackChannel, { backend: state, workspaceId: WS, instance: uid('l1') })).not.toContain('{@html');
    expect(t('slack-link-no-access', { channel: 'suppliers' })).toBe("You're no longer in #suppliers, so ordin can't read it.");
  });
});

// ---- Registration (SM-UP-1) ------------------------------------------------------------------------

describe('SM-UP-1 — the fixed screens are offered only for the groups the backend answers', () => {
  it('registers the fixed screens, each gated by its commands; slack and telegram are hidden on an R1 build', async () => {
    clearScreens();
    await import('../src/lib/views/fixed/register');
    const { SLACK_CHANNEL_SCREEN } = await import('../src/lib/views/fixed/register');
    const ids = [SCREEN_IDS.documents, SCREEN_IDS.agents, SCREEN_IDS.workflows, SCREEN_IDS.dashboards, SCREEN_IDS.telegram, SCREEN_IDS.slack, SLACK_CHANNEL_SCREEN, SCREEN_IDS.knowledge, SCREEN_IDS.memory];
    const r1 = { groups: new Set(R1_GROUPS) };
    for (const id of ids) expect(getScreen(id)?.commands?.length, id).toBeGreaterThan(0);
    const offered = ids.filter((id) => answersAll(r1, getScreen(id)?.commands ?? []));
    expect(offered).not.toContain(SCREEN_IDS.slack);
    expect(offered).not.toContain(SCREEN_IDS.telegram);
    expect(offered).not.toContain(SLACK_CHANNEL_SCREEN);
    expect(offered).toEqual(expect.arrayContaining([SCREEN_IDS.documents, SCREEN_IDS.agents, SCREEN_IDS.workflows, SCREEN_IDS.dashboards, SCREEN_IDS.knowledge, SCREEN_IDS.memory]));
    const all = { groups: new Set([...R1_GROUPS, 'slack', 'telegram', 'app']) };
    expect(ids.every((id) => answersAll(all, getScreen(id)?.commands ?? []))).toBe(true);
    void i18n;
  });
});
