// Part pages for Check it (SM-NS-15) and the way Check it opens them.
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { forgetCheckRuns, partPageKeyFor } from '../src/lib/views/fixed/check-it.svelte';
import { clearScreens, getScreen } from '../src/lib/shell/screens';
import InstalledPartPageView from '../src/lib/views/parts/InstalledPartPageView.svelte';
import LibraryPartPageView from '../src/lib/views/parts/LibraryPartPageView.svelte';
import { InstalledPartPageController, LibraryPartPageController, parsePartPageInstance } from '../src/lib/views/parts/part-page.svelte';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
beforeEach(() => forgetCheckRuns());

const GROUPS = ['service', 'agents', 'catalogue', 'checker', 'ordin'];
const URN = 'urn:ordin:acme:rules:payments';
const pagePayload = (over: Record<string, unknown> = {}, line: Record<string, unknown> = {}) => ({
  file: 'rules/payments.md',
  line: { name: 'Payments rule', part: { id: URN, kind: 'Rule', commit: 'c', contentHash: 'h', name: 'Payments rule', repository: 'r', version: '1' }, state: 'in_use', what_it_does: 'Stops payments over the limit.', ...line },
  plain_kind_message_id: 'part-kind-rule',
  text: 'Never pay over 10,000.\n\nAsk first.',
  ...over,
});
const view = (over: Record<string, unknown> = {}) => ({
  from: 'Acme',
  offered: true,
  part: { id: uid('p1'), identifier: URN, kind: 'Rule', name: 'Payments rule', agent_repository_id: uid('repo-1'), ...over },
  state_label: 'current',
  summary: 'Limits payments.',
});

describe('Part pages (SM-NS-15)', () => {
  it('parses the instance: installed agent and part, or library and part', () => {
    expect(parsePartPageInstance('a1:urn:ordin:acme:rules:payments')).toEqual({ kind: 'installed', agentInstanceId: 'a1', partId: URN });
    expect(parsePartPageInstance('library:p1')).toEqual({ kind: 'library', partId: 'p1' });
  });

  it('Happy path — an installed agent\'s part page (SM-NS-15): the page shows the part\'s name, its kind in plain words, "From: ordin" and its text, and Check it is offered', async () => {
    const { handle, state, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('agents.part_page', ok(pagePayload())) });
    const c = new InstalledPartPageController(handle, uid('a1'), URN);
    await c.load();
    expect(transport.sentOf('agents.part_page')[0]?.input).toEqual({ agent_instance_id: uid('a1'), part_id: URN, detail: 'business_user' });
    const body = text(html(InstalledPartPageView, { backend: state, workspaceId: WS, agentInstanceId: uid('a1'), c }));
    expect(body).toContain('Payments rule');
    expect(body).toContain('From: ordin');
    expect(body).toContain('Stops payments over the limit.');
    expect(body).toContain('Never pay over 10,000.');
    expect(body).toContain('Ask Ordin about this part');
    expect(body).toContain('Check it');
    expect(body).not.toContain('Details');
  });

  it('a part with a source is "From: <name>"', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('agents.part_page', ok(pagePayload({}, { from: 'Acme' }))) });
    const c = new InstalledPartPageController(handle, uid('a1'), URN);
    await c.load();
    expect(text(html(InstalledPartPageView, { backend: state, workspaceId: WS, agentInstanceId: uid('a1'), c }))).toContain('From: Acme');
  });

  it('Happy path — a library part\'s page: name, kind, source, summary, Used by and Check it', async () => {
    const { handle, state, transport } = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('catalogue.part', ok(view())).on('catalogue.used_by', ok({ agents: [view({ id: uid('p9'), name: 'Clause checker' })], part_id: uid('p1'), parts: [] })),
    });
    const c = new LibraryPartPageController(handle, uid('p1'));
    await c.load();
    expect(transport.sentOf('catalogue.part')[0]?.input).toEqual({ id: uid('p1') });
    expect(transport.sentOf('catalogue.used_by')[0]?.input).toEqual({ id: uid('p1') });
    const body = text(html(LibraryPartPageView, { backend: state, workspaceId: WS, c }));
    expect(body).toContain('Payments rule');
    expect(body).toContain('From: Acme');
    expect(body).toContain('Limits payments.');
    expect(body).toContain('Used by');
    expect(body).toContain('Clause checker');
    expect(body).toContain('Check it');
  });

  it('Error path — a refused read draws the owner\'s sentence and no page, and no Check it', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('agents.part_page', err('AGENT_NOT_INSTALLED', 'That agent is no longer installed.')) });
    const c = new InstalledPartPageController(handle, uid('a1'), URN);
    await c.load();
    const body = text(html(InstalledPartPageView, { backend: state, workspaceId: WS, agentInstanceId: uid('a1'), c }));
    expect(body).toContain('That agent is no longer installed.');
    expect(body).not.toContain('Check it');
    const lib = new LibraryPartPageController((await backendWith({ groups: GROUPS, setup: (tr) => tr.on('catalogue.part', err('CAT_PART_NOT_FOUND', 'That part is not in the library.')) })).handle, uid('p1'));
    await lib.load();
    expect(lib.error?.message).toBe('That part is not in the library.');
  });

  it('Check it opens the part page of a finding that names another part of an installed agent, and nothing otherwise', () => {
    const finding = (part?: string) => ({ args: {}, code: 'c', detail: '', fixes: [], message_id: 'check-x', severity: 'failure', ...(part ? { part_identifier: part } : {}) }) as never;
    const installed = { kind: 'installed' as const, agentInstanceId: 'a1' };
    expect(partPageKeyFor(installed, finding(URN))).toBe(`part-page:a1:${URN}`);
    expect(partPageKeyFor(installed, finding(URN), URN)).toBeNull();
    expect(partPageKeyFor(installed, finding())).toBeNull();
    expect(partPageKeyFor({ kind: 'library', partKind: 'Rule', identifier: URN }, finding(URN))).toBeNull();
  });

  it('registration: the part page is offered only when the backend answers its commands (SM-UP-1)', async () => {
    clearScreens();
    await import('../src/lib/views/parts/register');
    expect(getScreen('part-page:library:p1')?.commands).toEqual(['agents.part_page', 'catalogue.part', 'catalogue.used_by']);
    expect(getScreen('installed-files:a1')?.commands).toEqual(['agents.installed_files']);
    expect(getScreen('import')?.commands).toContain('app.pick_folder');
  });
});
