// SM-SF-7: a linked folder's automatic publishing line.
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import * as wiring from '../src/lib/backend/wiring';
import type { ConfluencePublishing } from '../src/lib/service-api/types.gen';
import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { PublishingController, grantNames } from '../src/lib/views/fixed/publishing.svelte';
import { AgentNames } from '../src/lib/views/fixed/facts';
import PublishingLine from '../src/lib/views/fixed/PublishingLine.svelte';
import { err, ok } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';
import { backendWith } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));

const agent = (id: string, name: string) => ({ instance: { id, display_name: name }, tasks: [], uses: [], workspaces: [] });
const LINK = uid('link-1');
const view = (automatic: 'on' | 'off', by: ConfluencePublishing['turned_off_by'] = []) => ({ kind: 'ok' as const, result: { confluence_link_id: LINK, automatic, turned_off_by: by } });

beforeEach(() => vi.restoreAllMocks());

async function line(reply: unknown, agents = [agent('a1', 'Build helper')]) {
  vi.spyOn(wiring, 'confluencePublishing').mockResolvedValue(reply as never);
  const { handle } = await backendWith({ setup: (tr) => tr.on('agents.list', ok(agents)) });
  const c = new PublishingController(handle, LINK);
  await c.load();
  return c;
}
const sentence = (c: PublishingController) => {
  const l = c.line;
  if (l.kind === 'on') return t('confluence-auto-on');
  if (l.kind === 'off') return t('confluence-auto-off', { names: l.more > 0 ? t('confluence-auto-off-more', { names: l.names, count: l.more }) : l.names });
  return '';
};

describe('SM-SF-7 — the automatic-publishing line', () => {
  it('Happy path — a linked folder publishes automatically', async () => {
    expect(sentence(await line(view('on')))).toBe('Your own changes here are published to Confluence automatically.');
  });

  it('Error path — an agent that can run programs turns automatic publishing off; no switch is offered', async () => {
    const c = await line(view('off', [{ kind: 'agent_instance', agent_instance_id: 'a1' }]));
    expect(sentence(c)).toBe('Changes here are not published automatically. They wait in Ready to publish, because Build helper can run programs that could change these files.');
    expect(t('confluence-auto-how-to-turn-on')).toBe('To publish automatically again, remove their access to this folder.');
    const src = import.meta.glob('../src/lib/views/fixed/PublishingLine.svelte', { query: '?raw', import: 'default', eager: true });
    const markup = Object.values(src)[0] as string;
    expect(markup).not.toMatch(/<input|role="switch"|type="checkbox"/);
  });

  it('a crew reads "a crew of agents"; an agent not in agents.list reads "an agent"; repeated words are drawn once', async () => {
    const names = new AgentNames([agent('a1', 'Build helper')] as never);
    expect(grantNames([{ kind: 'crew', crew_id: 'c1' } as never], names)).toEqual({ names: 'a crew of agents', more: 0 });
    expect(grantNames([{ kind: 'agent_instance', agent_instance_id: 'zzz' }], names)).toEqual({ names: 'an agent', more: 0 });
    expect(
      grantNames([{ kind: 'agent_instance', agent_instance_id: 'zzz' }, { kind: 'agent_instance', agent_instance_id: 'yyy' }, { kind: 'agent_instance', agent_instance_id: 'a1' }], names),
    ).toEqual({ names: 'an agent and Build helper', more: 0 });
  });

  it('Edge case — many agents are named: the first three and "and 2 more"', async () => {
    const agents = ['One', 'Two', 'Three', 'Four', 'Five'].map((n, i) => agent(`a${i}`, n));
    const by = agents.map((a) => ({ kind: 'agent_instance' as const, agent_instance_id: a.instance.id }));
    const c = await line(view('off', by), agents);
    expect(c.line).toEqual({ kind: 'off', names: 'One, Two and Three', more: 2 });
    expect(sentence(c)).toContain('One, Two and Three and 2 more can run programs');
  });

  it('Edge case — the publishing state is re-read when access changes', async () => {
    const spy = vi.spyOn(wiring, 'confluencePublishing').mockResolvedValue(view('off', [{ kind: 'agent_instance', agent_instance_id: 'a1' }]) as never);
    const { handle } = await backendWith({ setup: (tr) => tr.on('agents.list', ok([agent('a1', 'Build helper')])) });
    const c = new PublishingController(handle, LINK);
    await c.load();
    expect(c.line.kind).toBe('off');
    spy.mockResolvedValue(view('on') as never);
    await c.load();
    expect(sentence(c)).toBe('Your own changes here are published to Confluence automatically.');
    expect(spy).toHaveBeenCalledTimes(2);
  });

  it('not wired draws no line', async () => {
    expect((await line({ kind: 'not_wired' })).line.kind).toBe('none');
  });

  it.each(['WS_LINK_NOT_FOUND', 'WS_FILES_FAILED', 'WS_DATASTORE_UNAVAILABLE'])('%s draws no line and the owner\'s sentence in its place', async (code) => {
    const c = await line({ kind: 'error', error: { code, message: 'Sentence.' } });
    expect(c.line.kind).toBe('none');
    expect(c.error?.message).toBe('Sentence.');
  });

  it('the component draws nothing for a state it cannot read', async () => {
    vi.spyOn(wiring, 'confluencePublishing').mockResolvedValue({ kind: 'not_wired' });
    const { state } = await backendWith();
    const { html } = await import('./support/ssr');
    expect(html(PublishingLine, { backend: state, workspaceId: 'w', linkId: LINK }).replace(/<!--.*?-->/g, '').trim()).toBe('');
  });
});

describe('SM-SF-10 — the publishing line reads workspaces.confluence_publishing', () => {
  it('Happy path — the folder shows that automatic publishing is off', async () => {
    let asked: unknown = null;
    const { handle } = await backendWith({
      setup: (tr) =>
        tr
          .on('agents.list', ok([agent('a1', 'Report writer')]))
          .on('workspaces.confluence_publishing', (input: unknown) => {
            asked = input;
            return ok({ confluence_link_id: LINK, automatic: 'off', turned_off_by: [{ kind: 'agent_instance', agent_instance_id: 'a1' }] });
          }),
    });
    const c = new PublishingController(handle, LINK);
    await c.load();
    expect(asked).toEqual({ confluence_link_id: LINK });
    expect(c.line).toEqual({ kind: 'off', names: 'Report writer', more: 0 });
    expect(sentence(c)).toContain('Report writer can run programs');
  });

  it('Error path — the link is gone', async () => {
    const { handle } = await backendWith({ setup: (tr) => tr.on('workspaces.confluence_publishing', err('WS_LINK_NOT_FOUND', 'That link no longer exists.')) });
    const c = new PublishingController(handle, LINK);
    await c.load();
    expect(c.line.kind).toBe('none');
    expect(c.error?.code).toBe('WS_LINK_NOT_FOUND');
    expect(c.error?.message).toBe('That link no longer exists.');
  });
});
