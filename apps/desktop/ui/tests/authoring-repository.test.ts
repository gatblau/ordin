// SM-NS-7: Parts of [display name], New part…, Withdraw [part].
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { RepositoryPartsController, WithdrawController, healthGroups, matchesShow } from '../src/lib/views/authoring/repository.svelte';
import { NEW_PART_KINDS, PART_KIND_MESSAGE } from '../src/lib/views/authoring/support';
import RepositoryParts from '../src/lib/views/authoring/RepositoryParts.svelte';
import { err, ok, R1_GROUPS } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));
const GROUPS = [...R1_GROUPS, 'authoring'];

const repo = (id: string, name: string, over: Record<string, unknown> = {}) => ({ agent_count: 3, set_by_company: false, repository: { id, display_name: name, format: 'ordin', person_can_publish: true, ...over } });
const partView = (id: string, name: string, label = 'current', kind = 'Skill') => ({ from: 'Company', offered: true, state_label: label, summary: `About ${name}`, part: { id, name, kind, agent_repository_id: uid('repo-1'), status: 'active', withdrawn: false } });
const entry = (id: string, name: string, label = 'current') => ({ from: 'Company', part: partView(id, name, label) });

async function parts(extra: (tr: import('./support/fake-backend').FakeTransport) => void = () => undefined, repos = [repo(uid('repo-1'), 'Company agents'), repo(uid('repo-2'), 'Team agents', { person_can_publish: false })]) {
  const opened: string[] = [];
  const made = await backendWith({
    groups: GROUPS,
    setup: (tr) => {
      tr.on('catalogue.list_repositories', ok(repos)).on('catalogue.search', ok({ items: [entry(uid('p1'), 'Reading notice periods'), entry(uid('p2'), 'Old rule', 'withdrawn'), entry(uid('p3'), 'Careful tone', 'not_for_new_agents')], next_cursor: null }));
      extra(tr);
    },
  });
  const c = new RepositoryPartsController(made.handle, uid('repo-1'), (k) => opened.push(k));
  await c.load();
  return { ...made, c, opened };
}

describe('SM-NS-7.2 — Parts of [display name]', () => {
  it('On opening it reads the repositories and searches that one; only repositories the person can publish to are switchable', async () => {
    const { c, transport } = await parts();
    expect(transport.sentOf('catalogue.search')[0]?.input).toEqual({ repositories: [uid('repo-1')], kinds: [], include_not_for_new: false });
    expect(c.publishable.map((r) => r.repository.id)).toEqual([uid('repo-1')]);
    expect(t('repo-parts-title', { name: 'Company agents' })).toBe('Parts of Company agents');
  });

  it('a repository the person cannot publish to draws no tabs and no parts', async () => {
    const made = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('catalogue.list_repositories', ok([repo(uid('repo-2'), 'Team agents', { person_can_publish: false })])) });
    const c = new RepositoryPartsController(made.handle, uid('repo-2'));
    await c.load();
    expect(c.current).toBeNull();
    expect(made.transport.sentOf('catalogue.search')).toHaveLength(0);
    expect(text(html(RepositoryParts, { backend: made.state, workspaceId: WS, instance: uid('repo-2') }))).not.toContain(t('repo-tab-words'));
  });

  it('Show: In use · Not for new agents · Withdrawn keeps the rows of that state', async () => {
    const { c } = await parts();
    expect(c.shown.map((e) => e.part.part.name)).toEqual(['Reading notice periods']);
    c.show = 'not_for_new';
    expect(c.shown.map((e) => e.part.part.name)).toEqual(['Careful tone']);
    c.show = 'withdrawn';
    expect(c.shown.map((e) => e.part.part.name)).toEqual(['Old rule']);
    expect(matchesShow('in_use', 'not_offered')).toBe(true);
  });

  it('the search box sends its text; a choice other than In use asks for parts not for new agents', async () => {
    const { c, transport } = await parts();
    c.query = ' notice ';
    c.show = 'withdrawn';
    await c.search();
    expect(transport.sentOf('catalogue.search')[1]?.input).toEqual({ repositories: [uid('repo-1')], kinds: [], include_not_for_new: true, text: 'notice' });
  });

  it('choosing a row reads Used by and History only when they are opened; Read it and Withdraw… open their screens', async () => {
    const { c, transport, opened } = await parts((tr) => tr.on('catalogue.used_by', ok({ part_id: uid('p1'), agents: [partView(uid('a1'), 'Rates helper')], parts: [] })).on('catalogue.part_history', ok({ replaced_by: [], replaces: [], versions: [] })));
    c.choose(c.entries[0]!);
    expect(transport.sentOf('catalogue.used_by')).toHaveLength(0);
    await c.loadUsedBy();
    await c.loadHistory();
    expect(transport.sentOf('catalogue.used_by')[0]?.input).toEqual({ id: uid('p1') });
    expect(transport.sentOf('catalogue.part_history')[0]?.input).toEqual({ id: uid('p1') });
    c.readIt();
    c.withdraw();
    expect(opened).toEqual([`part-page:library:${uid('p1')}`, `withdraw-part:${uid('p1')}`]);
  });

  it('an APM-format repository draws Parts only, with its note', async () => {
    const { c } = await parts(() => undefined, [repo(uid('repo-1'), 'Registry', { format: 'apm' })]);
    expect(c.isApm).toBe(true);
    expect(t('repo-apm-note', { name: 'Registry' })).toContain('APM format');
  });

  it('Words, Layout and Health each read their own command with the repository id', async () => {
    const { c, transport } = await parts((tr) =>
      tr.on('catalogue.vocabulary', ok([{ id: 'v', label: 'Notice', description: 'A warning period' }]))
        .on('catalogue.layout', ok({ kinds: [{ kind: 'Skill', directory: 'skills', count: 4 }] }))
        .on('catalogue.health', ok({ agent_repository_id: uid('repo-1'), commit: 'c', computed_at: '2026-01-06T10:00:00Z', lines: [{ kind: 'unused_part', part_id: uid('p1') }, { kind: 'unused_part', part_id: uid('p2') }, { kind: 'look_alike', part_id: uid('p3'), similar_to: uid('p1') }] })));
    await c.openTab('words');
    await c.openTab('layout');
    await c.openTab('health');
    for (const command of ['catalogue.vocabulary', 'catalogue.layout', 'catalogue.health'] as const) expect(transport.sentOf(command)[0]?.input).toEqual({ id: uid('repo-1') });
    expect(c.vocabulary[0]?.label).toBe('Notice');
    expect(c.layout[0]?.count).toBe(4);
    expect(healthGroups(c.health!)).toEqual([{ kind: 'unused_part', count: 2 }, { kind: 'look_alike', count: 1 }]);
  });

  it('every part kind has a plain-words message id, and New part… offers only offered kinds', () => {
    expect(PART_KIND_MESSAGE.Skill).toBe('part-kind-ability');
    expect(NEW_PART_KINDS).not.toContain('AgentIdentity');
    expect(NEW_PART_KINDS).not.toContain('Journey');
  });

  it('New part… sends authoring.new_part and opens the part draft', async () => {
    const { c, transport, opened } = await parts((tr) => tr.on('authoring.new_part', ok({ draft: { id: 'pd-9' } })));
    c.newKind = 'Rule';
    c.newName = ' Notice periods ';
    await c.newPart(false);
    expect(transport.sentOf('authoring.new_part')[0]?.input).toEqual({ agent_repository_id: uid('repo-1'), kind: 'Rule', name: 'Notice periods', workspace_links_code: false });
    expect(opened).toEqual(['part-draft:pd-9']);
  });

  it('Error path — a refused search draws the sentence', async () => {
    const { c } = await parts((tr) => tr.on('catalogue.search', err('CAT_UNREADABLE', 'The library cannot be read right now.', { action: 'try_again' })));
    await c.search();
    expect(c.error?.message).toBe('The library cannot be read right now.');
  });
});

describe('SM-NS-7.4 — Withdraw [part]', () => {
  async function withdraw(extra: (tr: import('./support/fake-backend').FakeTransport) => void = () => undefined) {
    const opened: string[] = [];
    const made = await backendWith({
      groups: GROUPS,
      setup: (tr) => {
        tr.on('catalogue.part', ok(partView(uid('p1'), 'Reading notice periods')))
          .on('catalogue.used_by', ok({ part_id: uid('p1'), agents: [partView(uid('a1'), 'Rates helper')], parts: [] }))
          .on('catalogue.search', ok({ items: [entry(uid('p1'), 'Reading notice periods'), entry(uid('p5'), 'Reading terms')], next_cursor: null }));
        extra(tr);
      },
    });
    const c = new WithdrawController(made.handle, uid('p1'), (k) => opened.push(k));
    await c.load();
    return { ...made, c, opened };
  }

  it('On opening it reads the part, who uses it and the parts of the same kind, without the part itself', async () => {
    const { c, transport } = await withdraw();
    expect(t('withdraw-title', { name: c.part!.part.name })).toBe('Withdraw Reading notice periods');
    expect(c.usedBy?.agents.map((a) => a.part.name)).toEqual(['Rates helper']);
    expect(transport.sentOf('catalogue.search')[0]?.input).toEqual({ repositories: [uid('repo-1')], kinds: ['Skill'], include_not_for_new: false });
    expect(c.replacements.map((e) => e.part.part.id)).toEqual([uid('p5')]);
  });

  it('the reason is required and at most 1,000 characters; nothing is sent without one', async () => {
    const { c, transport } = await withdraw((tr) => tr.on('authoring.withdraw', ok({ draft: { id: 'pd-2' } })));
    await c.withdraw();
    expect(transport.sentOf('authoring.withdraw')).toHaveLength(0);
    c.reason = 'x'.repeat(1001);
    expect(c.reasonValid).toBe(false);
    c.reason = 'x'.repeat(1000);
    expect(c.reasonValid).toBe(true);
  });

  it('Happy path — a part withdrawn with a reason (SM-NS-7)', async () => {
    const { c, transport, opened } = await withdraw((tr) => tr.on('authoring.withdraw', ok({ draft: { id: 'pd-2' } })));
    c.reason = 'Notice periods changed in law.';
    c.replacement = uid('p5');
    await c.withdraw();
    expect(transport.sentOf('authoring.withdraw')[0]?.input).toEqual({ part: uid('p1'), reason: 'Notice periods changed in law.', replacement: uid('p5') });
    expect(opened).toEqual(['part-draft:pd-2']);
  });

  it('no replacement leaves the field out of the command', async () => {
    const { c, transport } = await withdraw((tr) => tr.on('authoring.withdraw', ok({ draft: { id: 'pd-2' } })));
    c.reason = 'Gone.';
    await c.withdraw();
    expect(transport.sentOf('authoring.withdraw')[0]?.input).toEqual({ part: uid('p1'), reason: 'Gone.' });
  });

  it('Error path — a refused withdrawal draws the sentence and opens nothing', async () => {
    const { c, opened } = await withdraw((tr) => tr.on('authoring.withdraw', err('AUTH_PART_NOT_FOUND', 'That part is not in the library any more.')));
    c.reason = 'Gone.';
    await c.withdraw();
    expect(c.error?.message).toBe('That part is not in the library any more.');
    expect(opened).toEqual([]);
  });
});
