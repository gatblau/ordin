// What Ordin used (ADR-111 step 7) and Remembered during this run (step 8); a page named by its id (SM-CU-11).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { beforeAll, describe, expect, it } from 'vitest';

import { AgentNames } from '../src/lib/views/fixed/facts';
import LinkLine from '../src/lib/views/fixed/LinkLine.svelte';
import { MemoryController } from '../src/lib/views/fixed/memory-controller.svelte';
import { RememberedController, UsedController } from '../src/lib/views/fixed/memory-used.svelte';
import MemoryUsedPanel from '../src/lib/views/fixed/MemoryUsedPanel.svelte';
import RememberedInRun from '../src/lib/views/fixed/RememberedInRun.svelte';
import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { linkId, thingId, uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));

const fact = (id: string, over: Record<string, unknown> = {}) => ({ id, statement: `Fact ${id}`, kind: 'known', true_from: '2026-01-10T00:00:00Z', written_at: '2026-01-10T00:00:00Z', came_from: { kind: 'person' }, fading: false, may_be_out_of_date: false, needs_check: false, ...over });
const link = { id: linkId('l1'), kind: 'supplies', since: '2026-01-08T00:00:00Z', from: { id: thingId('t1'), name: 'Borealis' }, to: { id: thingId('t2'), name: 'packaging' }, by: { kind: 'person' } };
const P = uid('P');
const Q = uid('Q');

describe('What Ordin used', () => {
  it('Happy path — lists the facts and links of an answer; Forget on the first fact asks first and sends memory.forget', async () => {
    const { handle, transport } = await backendWith({
      setup: (tr) =>
        tr
          .on('agents.list', ok([]))
          .on('memory.used', ok({ facts: [fact(uid('a')), fact(uid('b')), fact(uid('c'))], links: [link], gone: 2 }))
          .on('memory.forget_count', ok({ summaries: 0 }))
          .on('memory.forget', ok({})),
    });
    const c = new UsedController(handle, WS, [uid('R1')]);
    await c.load();
    expect(transport.sentOf('memory.used')[0]?.input).toEqual({ workspace_id: WS, recall_ids: [uid('R1')] });
    expect(c.facts).toHaveLength(3);
    expect(c.links).toHaveLength(1);
    expect(t('memory-used-gone', { count: 2 })).toBe('2 items were forgotten since.');
    await c.memory.askForgetFact(uid('a'));
    await c.confirmForget();
    expect(transport.sentOf('memory.forget')[0]?.input).toEqual({ id: uid('a') });
    expect(c.facts.map((f) => f.id)).toEqual([uid('b'), uid('c')]);
  });

  it('Edge case — nothing is fetched for an answer that used no memory: no link is drawn and memory.used is never sent', async () => {
    const { handle, transport, state } = await backendWith();
    const c = new UsedController(handle, WS, []);
    await c.load();
    expect(transport.sentOf('memory.used')).toHaveLength(0);
    expect(html(MemoryUsedPanel, { backend: state, workspaceId: WS, recallIds: undefined }).replace(/<!--.*?-->/g, '').trim()).toBe('');
    expect(text(html(MemoryUsedPanel, { backend: state, workspaceId: WS, recallIds: [uid('R1')] }))).toContain('What Ordin used');
    expect(transport.sentOf('memory.used')).toHaveLength(0);
  });

  it('Happy path — a thing explained in a page: a fact from the page shows "Supplier Borealis › Contract terms"', async () => {
    const { handle } = await backendWith({
      setup: (tr) =>
        tr
          .on('agents.list', ok([]))
          .on('memory.used', ok({ facts: [fact(uid('a'), { page_id: P, page_section: 'Contract terms' })], links: [], gone: 0 }))
          .on('knowledge.get', ok({ page: { id: P, title: 'Supplier Borealis' } })),
    });
    const c = new UsedController(handle, WS, [uid('R1')]);
    await c.load();
    expect(c.pageTitles[P]).toBe('Supplier Borealis');
    expect(t('memory-used-page', { page: 'Supplier Borealis', section: 'Contract terms' })).toBe('Supplier Borealis › Contract terms');
  });
});

describe('SM-CU-11 — a page named by its id', () => {
  const byPage = (pageId: string, id = linkId('l2')) => ({ ...link, id, by: { kind: 'page', page_id: pageId } });

  it('reads each distinct page id once, from the facts and the links, and holds the titles', async () => {
    const { handle, transport } = await backendWith({
      setup: (tr) =>
        tr
          .on('agents.list', ok([]))
          .on('memory.used', ok({ facts: [fact(uid('a'), { page_id: P }), fact(uid('b'), { page_id: P })], links: [byPage(P), byPage(Q, linkId('l3'))], gone: 0 }))
          .on('knowledge.get', (input) => ok({ page: { id: (input as { page_id: string }).page_id, title: `Page ${(input as { page_id: string }).page_id}` } })),
    });
    const c = new UsedController(handle, WS, [uid('R1')]);
    await c.load();
    expect(transport.sentOf('knowledge.get').map((e) => e.input)).toEqual([{ page_id: P }, { page_id: Q }]);
    expect(c.pageTitles).toEqual({ [P]: `Page ${P}`, [Q]: `Page ${Q}` });
    // Read once while the view is open: a second load sends no knowledge.get again.
    await c.load();
    expect(transport.sentOf('knowledge.get')).toHaveLength(2);
  });

  it('Error path — a refusal (the page was removed) is held as gone', async () => {
    const { handle } = await backendWith({
      setup: (tr) =>
        tr
          .on('agents.list', ok([]))
          .on('memory.used', ok({ facts: [fact(uid('a'), { page_id: P })], links: [], gone: 0 }))
          .on('knowledge.get', err('KNOW_PAGE_NOT_FOUND', 'That page is no longer here.')),
    });
    const c = new UsedController(handle, WS, [uid('R1')]);
    await c.load();
    expect(c.pageTitles).toEqual({ [P]: null });
  });

  it('a link set by a page names the page: "From the page Supplier Borealis", with Open the page', () => {
    const out = text(html(LinkLine, { link: byPage(P), names: new AgentNames(), pageTitles: { [P]: 'Supplier Borealis' }, onopenpage: () => undefined }));
    expect(out).toContain(t('memory-link-by-page', { page: 'Supplier Borealis' }));
    expect(out).toContain('Open the page');
  });

  it('Error path — a link set by a page that is gone reads memory-page-gone, with no Open the page', () => {
    const out = text(html(LinkLine, { link: byPage(P), names: new AgentNames(), pageTitles: { [P]: null }, onopenpage: () => undefined }));
    expect(out).toContain('From a page that is no longer here');
    expect(out).not.toContain('Open the page');
  });

  it('nothing is kept in localStorage', () => {
    expect(readFileSync(join(__dirname, '../src/lib/views/fixed/page-titles.svelte.ts'), 'utf8')).not.toMatch(/localStorage|ViewPrefs/);
  });
});

describe('SM-CU-11 — the Things tab names a link\'s page', () => {
  it('opening a thing reads the pages of its links once each', async () => {
    const { handle, transport } = await backendWith({
      setup: (tr) =>
        tr
          .on('memory.thing', ok({ thing: { id: thingId('t1'), name: 'Borealis', other_names: [], fact_count: 0, link_count: 2 }, facts: [], links: [{ ...link, id: linkId('l2'), by: { kind: 'page', page_id: P } }, { ...link, id: linkId('l3'), by: { kind: 'page', page_id: P } }] }))
          .on('knowledge.get', err('KNOW_PAGE_NOT_FOUND', 'That page is no longer here.')),
    });
    const c = new MemoryController(handle, WS);
    await c.openThing(thingId('t1'));
    await c.openThing(thingId('t1'));
    expect(transport.sentOf('knowledge.get')).toHaveLength(1);
    expect(c.pages.titles).toEqual({ [P]: null });
  });
});

describe('Remembered during this run', () => {
  it('Happy path — three lines: "Remembered by Supplier analyst" twice and "Remembered by Ordin" once', async () => {
    const agent = '0192f400-0000-7000-8000-0000000000a9';
    const { handle, transport } = await backendWith({
      setup: (tr) =>
        tr
          .on('agents.list', ok([{ instance: { id: agent, display_name: 'Supplier analyst' }, tasks: [], uses: [], workspaces: [] }]))
          .on('memory.remembered_in_run', ok({ items: [
            { by: { kind: 'crew_member', agent_instance_id: agent }, fact: fact(uid('a')) },
            { by: { kind: 'agent', agent_instance_id: agent }, fact: fact(uid('b')) },
            { by: { kind: 'ordin' }, fact: fact(uid('c')) },
          ] })),
    });
    const c = new RememberedController(handle, uid('run-1'), WS);
    expect(c.open).toBe(false);
    expect(transport.sentOf('memory.remembered_in_run')).toHaveLength(0);
    await c.toggle();
    expect(transport.sentOf('memory.remembered_in_run')[0]?.input).toEqual({ run_id: uid('run-1') });
    const lines = c.items.map((i) => { const l = c.byLine(i); return t(l.id, l.args); });
    expect(lines).toEqual(['Remembered by Supplier analyst', 'Remembered by Supplier analyst', 'Remembered by Ordin']);
  });

  it('empty reads "Nothing was remembered during this run."; closed by default draws no items', async () => {
    const { handle, state } = await backendWith({ setup: (tr) => tr.on('memory.remembered_in_run', ok({ items: [] })) });
    const c = new RememberedController(handle, uid('run-1'), null);
    await c.toggle();
    expect(c.loaded && c.items.length === 0).toBe(true);
    expect(t('memory-remembered-none')).toBe('Nothing was remembered during this run.');
    const out = html(RememberedInRun, { backend: state, runId: uid('run-1') });
    expect(text(out)).toContain('Remembered during this run');
    expect(out).toContain('aria-expanded="false"');
  });
});
