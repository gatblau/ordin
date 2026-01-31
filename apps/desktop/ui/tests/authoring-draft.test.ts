// SM-NS-6: the agent draft. SM-NS-7.3: the part draft.
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import AgentDraft from '../src/lib/views/authoring/AgentDraft.svelte';
import DraftChecks from '../src/lib/views/authoring/DraftChecks.svelte';
import { AgentDraftController, PartDraftController, bandMessage, failureCount, offeredFixes, orderedChecks } from '../src/lib/views/authoring/draft.svelte';
import { err, ok, R1_GROUPS } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));
const GROUPS = [...R1_GROUPS, 'authoring'];

const DRAFT = uid('draft-1');
const finding = (severity: 'failure' | 'warning', code: string, fixes: unknown[] = []) => ({ args: {}, code, detail: '', fixes, message_id: `check-${code}`, severity }) as never;
const agentView = (over: Record<string, unknown> = {}, draft: Record<string, unknown> = {}) => ({
  can_publish: false,
  can_try: false,
  checks: [],
  draft: { id: DRAFT, name: 'Rates helper', purpose: 'Keeps rates fresh', state: 'draft', agent_repository_id: uid('repo-1'), rehearsal_id: null, trial: false, will: ['Read rates'], wont: ['Send mail'], needs: ['A rates file'], tasks: ['Refresh'], ...draft },
  publish_to: [uid('repo-1'), uid('repo-2')],
  reviewer_words: ['Looks careful.'],
  tidied: [],
  ...over,
});
const repo = (id: string, name: string) => ({ agent_count: 0, set_by_company: false, repository: { id, display_name: name, format: 'ordin', person_can_publish: true } });

async function agent(view: unknown, extra: (tr: import('./support/fake-backend').FakeTransport) => void = () => undefined) {
  const closed = vi.fn();
  const made = await backendWith({
    groups: GROUPS,
    setup: (tr) => {
      tr.on('authoring.agent_draft', ok(view)).on('catalogue.list_repositories', ok([repo(uid('repo-1'), 'Company agents'), repo(uid('repo-2'), 'Team agents')]));
      extra(tr);
    },
  });
  const c = new AgentDraftController(made.handle, DRAFT, WS, closed);
  await c.load();
  return { ...made, c, closed };
}

describe('SM-NS-6 — the agent draft', () => {
  it('On opening it sends authoring.agent_draft with the draft id and reads the repositories for names', async () => {
    const { c, transport } = await agent(agentView());
    expect(transport.sentOf('authoring.agent_draft')[0]?.input).toBe(DRAFT);
    expect(t('draft-agent-title', { name: c.view!.draft.name })).toBe('Agent draft: Rates helper');
    expect(c.repositoryName(uid('repo-2'))).toBe('Team agents');
  });

  it('the band is chosen by state', () => {
    expect(bandMessage('draft')).toBe('draft-band-draft');
    expect(bandMessage('waiting_for_review')).toBe('draft-band-waiting');
    expect(bandMessage('changes_asked_for')).toBe('draft-band-changes-asked');
    expect(bandMessage('approved_not_signed_yet')).toBe('draft-band-approved-not-signed');
    expect(bandMessage('published')).toBe('draft-band-published');
    expect(bandMessage('discarded')).toBeNull();
  });

  it('Happy path — the agent draft\'s next step (SM-NS-6)', async () => {
    const { c } = await agent(agentView({ checks: [finding('failure', 'a'), finding('failure', 'b'), finding('warning', 'w')], can_publish: true }, { rehearsal_id: 'r' }));
    expect(failureCount(c.view!.checks)).toBe(2);
    expect(c.next).toBe('fix');
    expect(t('draft-next-fix', { count: failureCount(c.view!.checks) })).toBe('Fix 2 checks');
    expect(c.next).not.toBe('publish');
  });

  it('the one next action runs Fix, Rehearse, Try it, Publish in that order', async () => {
    expect((await agent(agentView())).c.next).toBe('rehearse');
    expect((await agent(agentView({ can_try: true }, { rehearsal_id: 'r' }))).c.next).toBe('try');
    expect((await agent(agentView({ can_try: true, can_publish: true }, { rehearsal_id: 'r', trial: true }))).c.next).toBe('publish');
    expect((await agent(agentView({}, { rehearsal_id: 'r' }))).c.next).toBeNull();
  });

  it('failed checks are drawn before warnings; only Fix it and Ask Ordin to fix it are offered as buttons', () => {
    const list = orderedChecks([finding('warning', 'w'), finding('failure', 'f') ]);
    expect(list.map((f) => f.code)).toEqual(['f', 'w']);
    const fixes = offeredFixes(finding('failure', 'f', [{ fix: 'fix_it', action: { action: 'drop_unknown_field', field: 'x' } }, { fix: 'ask_ordin' }, { fix: 'change_it_yourself', field: 'x' }]) as never);
    expect(fixes).toEqual([{ choice: 'fix_it', label: 'check-fix-it' }, { choice: 'ask_ordin', label: 'check-ask-ordin-to-fix' }]);
  });

  it('Fix it sends authoring.apply_fix for that finding and reads the draft again', async () => {
    const { c, transport } = await agent(agentView({ checks: [finding('failure', 'a', [{ fix: 'ask_ordin' }])] }), (tr) => tr.on('authoring.apply_fix', ok({ kind: 'agent', id: DRAFT })));
    await c.applyFix(c.view!.checks[0]!, 'ask_ordin');
    expect(transport.sentOf('authoring.apply_fix')[0]?.input).toEqual({ draft: { kind: 'agent', id: DRAFT }, finding_code: 'a', fix: 'ask_ordin' });
    expect(transport.sentOf('authoring.agent_draft')).toHaveLength(2);
  });

  it('Rehearse sends no situations of its own and keeps the result: passed of total', async () => {
    const rehearsal = { agent_draft_id: DRAFT, draft_version: '1', id: 'r', rehearsed_at: '', run_ids: [], situations: [
      { description: 'A rates file arrives', documents_used: [], reviewer_opinion: 'Fine.', expectations: [{ text: 'Reads it', outcome: 'met' }] },
      { description: 'No file arrives', documents_used: [], reviewer_opinion: '', expectations: [{ text: 'Asks', outcome: 'not_met' }] },
    ] };
    const { c, transport } = await agent(agentView(), (tr) => tr.on('authoring.rehearse', ok(rehearsal)));
    await c.rehearse();
    expect(transport.sentOf('authoring.rehearse')[0]?.input).toEqual({ id: DRAFT, workspace_id: WS, situations: [] });
    expect(c.rehearsal?.situations).toHaveLength(2);
    expect(t('draft-rehearsal-result', { passed: 1, total: 2 })).toBe('1 of 2 situations passed');
  });

  it('Try it, Publish… and Send the change… send their commands with the draft id', async () => {
    const { c, transport } = await agent(agentView(), (tr) => tr.on('authoring.try_it', ok('s')).on('authoring.publish', ok({})).on('authoring.send_change', ok({})));
    await c.tryIt();
    await c.publish();
    await c.sendChange();
    expect(transport.sentOf('authoring.try_it')[0]?.input).toEqual({ id: DRAFT, workspace_id: WS });
    expect(transport.sentOf('authoring.publish')[0]?.input).toEqual({ kind: 'agent', id: DRAFT });
    expect(transport.sentOf('authoring.send_change')[0]?.input).toEqual({ kind: 'agent', id: DRAFT });
  });

  it('Publish to lists the repositories by display name; the choice sends authoring.set_publish_to', async () => {
    const { c, transport } = await agent(agentView(), (tr) => tr.on('authoring.set_publish_to', ok(agentView({}, { agent_repository_id: uid('repo-2') }))));
    await c.setPublishTo(uid('repo-2'));
    expect(transport.sentOf('authoring.set_publish_to')[0]?.input).toEqual({ id: DRAFT, repository: uid('repo-2') });
    expect(c.view?.draft.agent_repository_id).toBe(uid('repo-2'));
  });

  it('Discard sends authoring.discard and closes the tab; a refusal keeps it and draws the sentence', async () => {
    const good = await agent(agentView(), (tr) => tr.on('authoring.discard', ok({})));
    await good.c.discard();
    expect(good.transport.sentOf('authoring.discard')[0]?.input).toEqual({ kind: 'agent', id: DRAFT });
    expect(good.closed).toHaveBeenCalledOnce();
    const bad = await agent(agentView(), (tr) => tr.on('authoring.discard', err('AUTH_DRAFT_BUSY', 'The draft is being published.')));
    await bad.c.discard();
    expect(bad.c.error?.message).toBe('The draft is being published.');
    expect(bad.closed).not.toHaveBeenCalled();
  });

  it('Error path — authoring.agent_draft refused: the sentence is drawn and no draft', async () => {
    const { c } = await agent(null, (tr) => tr.on('authoring.agent_draft', err('AUTH_DRAFT_UNKNOWN', 'That draft is gone.')));
    expect(c.view).toBeNull();
    expect(c.error?.message).toBe('That draft is gone.');
  });

  it('the draft draws the band, the reviewer\'s words as text (never markup) and the tidied count', async () => {
    const raw = html(DraftChecks, { state: 'waiting_for_review', reviewerWords: ['<b>Looks</b> careful'], checks: [finding('failure', 'a')], tidied: [{ field: 'f', before: 'colour', after: 'color' }] });
    expect(raw).not.toContain('<b>Looks');
    expect(raw).toContain('&lt;b>Looks');
    const markup = text(raw);
    expect(markup).toContain(t('draft-band-waiting'));
    expect(markup).toContain(t('check-checked-by-ordin'));
    expect(markup).toContain(t('draft-tidied', { count: 1 }));
  });

  it('SM-UP-1 — the agent draft screen draws nothing for the draft until it is read', async () => {
    const { state } = await backendWith({ groups: GROUPS });
    expect(text(html(AgentDraft, { backend: state, workspaceId: WS, instance: DRAFT }))).not.toContain(t('draft-discard'));
  });
});

describe('SM-NS-7.3 — the part draft', () => {
  const partView = (over: Record<string, unknown> = {}, draft: Record<string, unknown> = {}) => ({
    agents_that_stop: [], can_publish: true, checks: [], form: {}, reviewer_words: [], tidied: [], used_by: ['Rates helper', 'Mail helper'], writing_guide: 'Write plainly.',
    draft: { id: uid('pd-1'), part_id: uid('part-1'), state: 'draft', fields: { a: 1 }, changes_what_part_does: null, ...draft },
    ...over,
  });
  async function part(view: unknown = partView(), extra: (tr: import('./support/fake-backend').FakeTransport) => void = () => undefined) {
    const closed = vi.fn();
    const made = await backendWith({ groups: GROUPS, setup: (tr) => { tr.on('authoring.part_draft', ok(view)).on('catalogue.part', ok({ part: { name: 'Reading notice periods' } })); extra(tr); } });
    const c = new PartDraftController(made.handle, uid('pd-1'), closed);
    await c.load();
    return { ...made, c, closed };
  }

  it('On opening it sends authoring.part_draft and names the part from catalogue.part', async () => {
    const { c, transport } = await part();
    expect(transport.sentOf('authoring.part_draft')[0]?.input).toBe(uid('pd-1'));
    expect(c.partName).toBe('Reading notice periods');
    expect(t('draft-part-used-by', { count: 2, names: 'Rates helper, Mail helper' })).toContain('2 agents');
  });

  it('both answers of "Does this change what the part does?" send the draft\'s own fields unchanged', async () => {
    const { c, transport } = await part(partView(), (tr) => tr.on('authoring.update_part_draft', ok(partView({}, { changes_what_part_does: true }))));
    await c.answerChangesWhat(true);
    expect(transport.sentOf('authoring.update_part_draft')[0]?.input).toEqual({ id: uid('pd-1'), changes_what_part_does: true, fields: { a: 1 } });
    await c.answerChangesWhat(false);
    expect(transport.sentOf('authoring.update_part_draft')[1]?.input).toEqual({ id: uid('pd-1'), changes_what_part_does: false, fields: { a: 1 } });
  });

  it('Publish… sends kind part; Discard closes the tab', async () => {
    const { c, transport, closed } = await part(partView(), (tr) => tr.on('authoring.publish', ok({})).on('authoring.discard', ok({})));
    await c.publish();
    expect(transport.sentOf('authoring.publish')[0]?.input).toEqual({ kind: 'part', id: uid('pd-1') });
    await c.discard();
    expect(transport.sentOf('authoring.discard')[0]?.input).toEqual({ kind: 'part', id: uid('pd-1') });
    expect(closed).toHaveBeenCalledOnce();
  });
});
