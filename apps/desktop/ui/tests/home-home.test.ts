// Home (11 §5.17) and Open a handover link… .
import { beforeAll, describe, expect, it } from 'vitest';

import { err, event, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { loaded, text } from './settings-support';
import Home from '../src/lib/views/Home.svelte';
import { groupRows, HOME_REFRESH_EVENTS, HomeModel, runAgainSubject } from '../src/lib/views/home-model.svelte';
import type { HomeRow } from '../src/lib/service-api/types.gen';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const row = (over: Partial<HomeRow> & { line: string }): HomeRow => ({
  at: '2026-01-10T10:00:00Z', is_new: false, opens: { kind: 'workspace', workspace_id: WS }, run_again: false, workspace_id: WS, workspace_name: 'Q3 supplier review', ...over,
});
const view = (over: Record<string, HomeRow[]> = {}) => ({ new_count: 0, waiting: [], running: [], since_last_looked: [], ...over });

async function home(reply: unknown, opened: HomeRow[] = [], handovers: string[] = [], setup: Parameters<typeof loaded>[1] = () => undefined) {
  const t = await loaded(R1_GROUPS, (tr) => {
    tr.on('home.view', ok(reply));
    setup(tr);
  });
  const model = new HomeModel(t.handle, { openRow: (r) => opened.push(r), openHandover: (id) => handovers.push(id) });
  await model.load();
  return { ...t, model };
}
const draw = (t: { state: never; model: HomeModel }) => html(Home, { backend: t.state, workspaceId: null, instance: null, model: t.model });

describe('Home', () => {
  const waiting = [
    row({ line: 'Publish the revised account brief?', at: '2026-01-10T09:00:00Z', opens: { kind: 'approval', approval_id: 'a1', workspace_id: WS } }),
    row({ line: 'Choose a supplier', at: '2026-01-10T11:00:00Z', opens: { kind: 'block', block_id: 'b1', workspace_id: WS } }),
  ];

  it('draws Waiting for you, Running now and Since you last looked, each row opening in its own workspace', async () => {
    const t = await home(view({
      waiting,
      running: [row({ line: 'Comparing the Q3 rates', opens: { kind: 'run', run_id: uid('r1'), workspace_id: WS } })],
      since_last_looked: [row({ line: 'Quote checked', is_new: true, run_again: true, opens: { kind: 'run', run_id: uid('r2'), workspace_id: WS } })],
    }));
    const out = text(clean(draw(t as never)));
    for (const heading of ['Waiting for you', 'Running now', 'Since you last looked']) expect(out).toContain(heading);
    expect(out).toContain('Publish the revised account brief?');
    expect(out).toContain('Q3 supplier review');
    expect(out).toContain('Show progress');
    expect(out).toContain('Run again…');
    expect(out).toContain('new');
  });

  it('only the oldest waiting row keeps a primary Open', async () => {
    const t = await home(view({ waiting }));
    const markup = clean(draw(t as never));
    expect(t.model.oldestWaiting?.line).toBe('Publish the revised account brief?');
    expect((markup.match(/data-default="true"/g) ?? []).length).toBe(1);
    expect(markup.indexOf('data-default="true"')).toBeLessThan(markup.indexOf('Choose a supplier') + 200);
    expect(markup).toMatch(/Publish the revised account brief\?[\s\S]*?<button[^>]*data-default="true"[^>]*>Open/);
  });

  it('an empty Running now says so in one line', async () => {
    const t = await home(view());
    expect(text(clean(draw(t as never)))).toContain('Nothing running now');
  });

  it('no button on Home approves, sends, posts or publishes', async () => {
    const t = await home(view({ waiting }));
    const buttons = [...clean(draw(t as never)).matchAll(/<button[^>]*>([^<]*)<\/button>/g)].map((m) => m[1]);
    expect(buttons.join(' ')).not.toMatch(/Approve|Send|Post|Publish/);
  });

  it('the filter All workspaces or one sends workspace_id', async () => {
    const t = await home(view());
    t.transport.sent.length = 0;
    await t.model.setFilter(WS);
    expect(t.transport.sentOf('home.view')[0]?.input).toEqual({ workspace_id: WS });
    await t.model.setFilter(null);
    expect(t.transport.sentOf('home.view')[1]?.input).toEqual({});
    expect(text(clean(draw(t as never)))).toContain('All workspaces');
  });

  it('opening a row calls the opener; a document also sends home.document_opened', async () => {
    const opened: HomeRow[] = [];
    const doc = row({ line: 'Quote — Kestrel.md changed', opens: { kind: 'document', path: 'Q3/Quote.md', place_id: uid('p1'), workspace_id: WS } });
    const t = await home(view({ since_last_looked: [doc] }), opened, [], (tr) => tr.on('home.document_opened', ok({})));
    await t.model.open(doc);
    expect(opened).toEqual([doc]);
    expect(t.transport.sentOf('home.document_opened')[0]?.input).toEqual({ item: { kind: 'place_file', place_id: uid('p1'), relative_path: 'Q3/Quote.md' }, workspace_id: WS });
  });

  it('Run again… reads what the earlier work used, then opens it', async () => {
    const opened: HomeRow[] = [];
    const finished = row({ line: 'Done', run_again: true, opens: { kind: 'run', run_id: uid('r2'), workspace_id: WS } });
    const t = await home(view({ since_last_looked: [finished] }), opened, [], (tr) => tr.on('home.run_again_inputs', ok([])));
    await t.model.runAgain(finished);
    expect(t.transport.sentOf('home.run_again_inputs')[0]?.input).toEqual({ subject: { kind: 'run', run_id: uid('r2') } });
    expect(opened).toHaveLength(1);
    expect(runAgainSubject(row({ line: 'x', opens: { kind: 'crew_run', crew_run_id: 'c1', workspace_id: WS } }))).toEqual({ kind: 'crew_run', crew_run_id: 'c1' });
    expect(runAgainSubject(row({ line: 'x' }))).toBeNull();
  });

  it('Running now shows the run’s state label from the store', async () => {
    const t = await home(view({ running: [row({ line: 'Comparing', opens: { kind: 'run', run_id: uid('r1'), workspace_id: WS } })] }));
    await t.state.apply(event(LOCAL, t.state.lastSeq + 1, { kind: 'run_state_changed', run_id: uid('r1'), state: 'running' }));
    expect(clean(draw(t as never))).toMatch(/class="state[ "]/);
  });

  it('reads again on the events that change it', () => {
    expect([...HOME_REFRESH_EVENTS].sort()).toEqual(['approval_decided', 'approval_requested', 'crew_round_changed', 'needs_you_changed', 'run_state_changed']);
  });

  it('a failed read is drawn by the error line', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('home.view', err('HOME_SOURCE_UNAVAILABLE', 'Home can’t be read right now.', { action: 'try_again' })));
    const model = new HomeModel(t.handle, { openRow: () => undefined, openHandover: () => undefined });
    await model.load();
    const out = text(html(Home, { backend: t.state, workspaceId: null, instance: null, model }));
    expect(out).toContain('Home can’t be read right now.');
    expect(out).toContain('Try again');
  });

  it('opening Home sends home.opened', async () => {
    const t = await home(view(), [], [], (tr) => tr.on('home.opened', ok({})));
    await t.model.markOpened();
    expect(t.transport.commandsSent()).toContain('home.opened');
  });
});

describe('SM-CU-5 — Home\'s empty sections and groups', () => {
  const OTHER = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c99';
  const hosted = row({ line: 'Reply in #suppliers?', workspace_id: null, workspace_name: null, opens: { kind: 'hosted_approval', approval_id: 'h1', remote_agent_id: 'ra1' } });
  const loose = row({ line: 'A handover for you', workspace_id: null, workspace_name: null, opens: { kind: 'handover', handover_id: 'x1' } });
  const q3 = (line: string) => row({ line });
  const q4 = (line: string) => row({ line, workspace_id: OTHER, workspace_name: 'Q4 forecast' });

  it('Edge case — Home with nothing waiting (SM-CU-5)', async () => {
    const t = await home(view());
    const out = text(clean(draw(t as never)));
    expect(out).toContain('Nothing is waiting for you');
  });

  it.each([
    ['Waiting for you', 'waiting', 'Nothing is waiting for you'],
    ['Running now', 'running', 'Nothing running now'],
    ['Since you last looked', 'since_last_looked', 'Nothing new since you last looked'],
  ])('%s with no row draws its one line, and a row removes it', async (_heading, section, line) => {
    const empty = text(clean(draw((await home(view())) as never)));
    expect(empty).toContain(line);
    const some = text(clean(draw((await home(view({ [section]: [q3('One row')] }))) as never)));
    expect(some).not.toContain(line);
    expect(some).toContain('One row');
  });

  it('rows are grouped by workspace name in the reply\'s order; no workspace, then Hosted agents, last', () => {
    const groups = groupRows([q4('a'), hosted, q3('b'), loose, q4('c')]);
    expect(groups.map((g) => g.name ?? g.headingId)).toEqual(['Q4 forecast', 'Q3 supplier review', 'home-group-no-workspace', 'home-group-hosted-agents']);
    expect(groups.map((g) => g.rows.map((r) => r.line))).toEqual([['a', 'c'], ['b'], ['A handover for you'], ['Reply in #suppliers?']]);
  });

  it('a section with no row outside a workspace draws neither of the two group headings', () => {
    expect(groupRows([q3('a'), q4('b')]).every((g) => g.headingId === null)).toBe(true);
    expect(groupRows([])).toEqual([]);
  });

  it('the group headings are drawn: the workspace\'s name, Not in a workspace and Hosted agents, in that order', async () => {
    const t = await home(view({ waiting: [hosted, q4('In Q4'), loose] }));
    const markup = clean(draw(t as never));
    const headings = [...markup.matchAll(/<h4 class="group-heading[^"]*">([^<]*)<\/h4>/g)].map((m) => m[1]);
    expect(headings).toEqual(['Q4 forecast', 'Not in a workspace', 'Hosted agents']);
    const out = text(markup);
    expect(out.indexOf('Not in a workspace')).toBeLessThan(out.indexOf('A handover for you'));
    expect(out.indexOf('Hosted agents')).toBeLessThan(out.indexOf('Reply in #suppliers?'));
  });

  it('the oldest waiting row keeps the only primary Open across groups', async () => {
    const t = await home(view({ waiting: [q4('Later'), row({ line: 'Oldest', at: '2026-01-09T08:00:00Z' })] }));
    const markup = clean(draw(t as never));
    expect((markup.match(/data-default="true"/g) ?? []).length).toBe(1);
    expect(markup).toMatch(/Oldest[\s\S]*?<button[^>]*data-default="true"[^>]*>Open/);
  });
});

describe('Open a handover link…', () => {
  it('the dialog has a one-field form with its hint', async () => {
    const t = await home(view());
    t.model.openLink();
    const out = text(draw(t as never));
    expect(out).toContain('Open a handover link…');
    expect(out).toContain('Paste the link from the Slack message or email.');
  });

  it('a link whose handover is readable opens the handover screen with no workspace', async () => {
    const handovers: string[] = [];
    const t = await home(view(), [], handovers, (tr) => tr.on('handover.open_link', ok({ kind: 'opened', handover_id: 'h1' })));
    t.model.openLink();
    t.model.linkText = 'ordin://handover/h1';
    await t.model.submitLink();
    expect(t.transport.sentOf('handover.open_link')[0]?.input).toEqual({ link: 'ordin://handover/h1' });
    expect(handovers).toEqual(['h1']);
    expect(t.model.linkOpen).toBe(false);
  });

  it('Edge case — a pasted link whose handover is not readable yet', async () => {
    const t = await home(view(), [], [], (tr) => tr.on('handover.open_link', ok({ kind: 'not_found_yet', place: 'Supplier onboarding handovers' })));
    t.model.openLink();
    t.model.linkText = 'https://example.org/h';
    await t.model.submitLink();
    expect(t.model.linkOpen).toBe(false);
    expect(text(draw(t as never))).toContain("ordin now follows Supplier onboarding handovers; the handover will appear when it's readable.");
  });

  it('HAND_LINK_INVALID — Error path — a pasted link that is not a handover link', async () => {
    const handovers: string[] = [];
    const t = await home(view(), [], handovers, (tr) => tr.on('handover.open_link', err('HAND_LINK_INVALID', 'That is not a handover link.')));
    t.model.openLink();
    t.model.linkText = 'https://example.com/x';
    await t.model.submitLink();
    // The dialog stays open with the sentence, and nothing opens.
    expect(t.model.linkOpen).toBe(true);
    expect(handovers).toEqual([]);
    const out = text(draw(t as never));
    expect(out).toContain('That is not a handover link.');
    expect(out).not.toContain('HAND_LINK_INVALID');
  });

  it('an empty field sends nothing', async () => {
    const t = await home(view());
    t.model.openLink();
    await t.model.submitLink();
    expect(t.transport.sentOf('handover.open_link')).toEqual([]);
    void settle;
  });
});
