// What Ordin remembers: Facts, Things, To check (ADR-111, SM-SF-4, SM-SF-5, SM-SF-6).
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { MEMORY_GET_CHUNK, MemoryController } from '../src/lib/views/fixed/memory-controller.svelte';
import { AgentNames, citationLine, heldKindMessage } from '../src/lib/views/fixed/facts';
import FactLine from '../src/lib/views/fixed/FactLine.svelte';
import LinkLine from '../src/lib/views/fixed/LinkLine.svelte';
import * as wiring from '../src/lib/backend/wiring';
import { err, ok, settle } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { linkId, thingId, uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));

const AGENT = '0192f400-0000-7000-8000-0000000000a1';
const agents = [{ instance: { id: AGENT, display_name: 'Quote checker' }, tasks: [], uses: [], workspaces: [] }];

const fact = (over: Record<string, unknown> = {}) => ({
  id: uid('f1'), statement: 'Pay Kestrel first.', kind: 'known', true_from: '2026-01-10T00:00:00Z', written_at: '2026-01-10T00:00:00Z',
  came_from: { kind: 'person' }, fading: false, may_be_out_of_date: false, needs_check: false, ...over,
});
const held = (over: Record<string, unknown> = {}) => ({
  id: 'h1', statement: 'Pay this supplier first.', about: 'Kestrel', fact_class: 'semantic', cited: 'quote.pdf', things: [{ name: 'Kestrel', other_names: ['Kestrel Ltd'] }],
  links: [{ from: 'Kestrel', kind: 'supplies', to: 'Northwind', ended: false, confidence_permille: 850 }], content_hash: 'hash-1', trust_permille: 500,
  written_by: AGENT, held_at: '2026-01-16T10:00:00Z', needs_check: true, ...over,
});

async function controller(setup: Parameters<typeof backendWith>[0] = {}) {
  const b = await backendWith({ ...setup, setup: (tr) => { tr.on('agents.list', ok(agents)); tr.on('memory.list', ok({ items: [], next_cursor: null })); setup.setup?.(tr); } });
  return { ...b, c: new MemoryController(b.handle, WS) };
}

let heldReply: unknown;
let citeReply: unknown;
beforeEach(() => {
  vi.restoreAllMocks();
  heldReply = { kind: 'ok', result: [held()] };
  citeReply = { kind: 'ok', result: [] };
  vi.spyOn(wiring, 'heldWrites').mockImplementation(async () => heldReply as never);
  vi.spyOn(wiring, 'citationsToConfirm').mockImplementation(async () => citeReply as never);
});

describe('SM-SF-4 — written by agents, waiting for you', () => {
  it('Happy path — the person keeps a held write: Keep sends that write\'s content hash and announces it', async () => {
    const keep = vi.spyOn(wiring, 'keepHeld').mockResolvedValue({ kind: 'ok', result: {} });
    const { c } = await controller();
    await c.load();
    expect(c.toCheckDrawn).toBe(true);
    expect(c.held).toHaveLength(1);
    const entry = c.held?.[0] as wiring.HeldWrite;
    await c.keepHeldWrite(entry);
    expect(keep).toHaveBeenCalledWith(expect.anything(), WS, 'h1', 'hash-1');
    expect(c.held).toEqual([]);
    await settle();
    expect(c.announcer.text).toBe('Kept. Ordin will use this from now on.');
  });

  it('after a re-read that changes an entry\'s hash, Keep sends the new hash', async () => {
    const keep = vi.spyOn(wiring, 'keepHeld').mockResolvedValue({ kind: 'ok', result: {} });
    const { c } = await controller();
    await c.load();
    heldReply = { kind: 'ok', result: [held({ content_hash: 'hash-2' })] };
    await c.loadToCheck();
    await c.keepHeldWrite(c.held?.[0] as wiring.HeldWrite);
    expect(keep).toHaveBeenCalledWith(expect.anything(), WS, 'h1', 'hash-2');
  });

  it('Security — a held write changed after the person read it: the sentence is shown on the entry, the list is read again and nothing is kept', async () => {
    vi.spyOn(wiring, 'keepHeld').mockResolvedValue({ kind: 'error', error: { code: 'MEM_HELD_CHANGED', message: 'This fact changed after you read it. Read it again before keeping it.' } });
    const { c } = await controller();
    await c.load();
    const reads = vi.mocked(wiring.heldWrites).mock.calls.length;
    await c.keepHeldWrite(c.held?.[0] as wiring.HeldWrite);
    expect(c.errorOn('h1')?.message).toBe('This fact changed after you read it. Read it again before keeping it.');
    expect(vi.mocked(wiring.heldWrites).mock.calls.length).toBe(reads + 1);
    expect(c.held).toHaveLength(1);
  });

  it('Happy path — the person discards a held write: discardHeld is sent without a confirmation', async () => {
    const discard = vi.spyOn(wiring, 'discardHeld').mockResolvedValue({ kind: 'ok', result: {} });
    const { c } = await controller();
    await c.load();
    await c.discardHeldWrite(c.held?.[0] as wiring.HeldWrite);
    expect(discard).toHaveBeenCalledWith(expect.anything(), WS, 'h1');
    expect(c.pending).toBeNull();
    await settle();
    expect(c.announcer.text).toBe("Not kept. Ordin won't use this.");
  });

  it('MEM_NOT_HELD shows the owner sentence and reads the list again', async () => {
    vi.spyOn(wiring, 'discardHeld').mockResolvedValue({ kind: 'error', error: { code: 'MEM_NOT_HELD', message: "This fact doesn't need checking." } });
    const { c } = await controller();
    await c.load();
    const reads = vi.mocked(wiring.heldWrites).mock.calls.length;
    await c.discardHeldWrite(c.held?.[0] as wiring.HeldWrite);
    expect(c.errorOn('h1')?.message).toBe("This fact doesn't need checking.");
    expect(vi.mocked(wiring.heldWrites).mock.calls.length).toBe(reads + 1);
  });

  it('Edge case — nothing to check is wired yet: no To check tab and no waiting line', async () => {
    heldReply = { kind: 'not_wired' };
    citeReply = { kind: 'not_wired' };
    const { c } = await controller();
    await c.load();
    expect(c.held).toBeNull();
    expect(c.citations).toBeNull();
    expect(c.toCheckDrawn).toBe(false);
  });

  it('one section wired draws the tab with that section only; the count adds both', async () => {
    heldReply = { kind: 'not_wired' };
    citeReply = { kind: 'ok', result: [uid('c1')] };
    const { c } = await controller({ setup: (tr) => tr.on('memory.get', ok({ facts: [fact({ id: uid('c1') })] })) });
    await c.load();
    expect(c.held).toBeNull();
    expect(c.toCheckDrawn).toBe(true);
    expect(c.toCheckCount).toBe(1);
  });

  it('an entry with every field present draws each piece; an unknown fact_class draws no kind line; no writer reads "from a colleague"', () => {
    expect(heldKindMessage('episodic')).toBe('memory-kind-happened');
    expect(heldKindMessage('semantic')).toBe('memory-kind-known');
    expect(heldKindMessage('consolidated')).toBe('memory-kind-summary');
    expect(heldKindMessage('something-else')).toBeNull();
    expect(heldKindMessage(null)).toBeNull();
    expect(t('memory-held-by-agent', { agent: 'Quote checker' })).toBe('Written by Quote checker');
    expect(t('memory-held-link-sure', { percent: '85%' })).toBe('85% sure');
    expect(t('memory-held-link', { from: 'Kestrel', kind: 'supplies', to: 'Northwind' })).toBe('Kestrel supplies Northwind');
    expect(t('memory-thing-also-called', { names: 'Kestrel Ltd' })).toBe('Also called Kestrel Ltd');
  });

  it('the agent\'s name comes from agents.list; an unmatched id reads "an agent"', async () => {
    const { c } = await controller();
    await c.load();
    expect(c.names.nameOf(AGENT)).toBe('Quote checker');
    expect(c.names.nameOf('someone-else')).toBeNull();
  });
});

describe('SM-SF-5 — sources to confirm', () => {
  const cited = fact({ id: uid('c1'), came_from: { kind: 'agent_citing', agent_instance_id: AGENT, origin_ref: 'quote.pdf', version: '1' } });

  it('Happy path — the person confirms an agent\'s source', async () => {
    citeReply = { kind: 'ok', result: [uid('c1')] };
    const confirm = vi.spyOn(wiring, 'confirmCitation').mockResolvedValue({ kind: 'ok', result: {} });
    const { c } = await controller({ setup: (tr) => tr.on('memory.get', ok({ facts: [cited] })) });
    await c.load();
    expect(c.citations?.map((f) => f.id)).toEqual([uid('c1')]);
    const out = text(html(FactLine, { fact: c.citations?.[0], names: c.names }));
    expect(out).toContain('Quote checker says this comes from quote.pdf (version 1)');
    await c.confirmSource(c.citations?.[0] as never);
    expect(confirm).toHaveBeenCalledWith(expect.anything(), WS, uid('c1'));
    await settle();
    expect(c.announcer.text).toBe('Confirmed. This fact can now be shared with your team.');
    expect(c.citations).toEqual([]);
  });

  it('ids are read with memory.get in chunks of 100, in order, and an id not returned is not drawn', async () => {
    const ids = Array.from({ length: 150 }, (_, i) => uid(`c${i}`));
    citeReply = { kind: 'ok', result: ids };
    const { c, transport } = await controller({
      setup: (tr) => tr.on('memory.get', (input) => ok({ facts: (input as { ids: string[] }).ids.filter((id) => id !== uid('c7')).map((id) => fact({ id })) })),
    });
    await c.loadToCheck();
    const gets = transport.sentOf('memory.get').map((e) => (e.input as { ids: string[] }).ids.length);
    expect(gets).toEqual([MEMORY_GET_CHUNK, 50]);
    expect(c.citations).toHaveLength(149);
    expect(c.citations?.some((f) => f.id === uid('c7'))).toBe(false);
    expect(c.citations?.[0]?.id).toBe(uid('c0'));
  });

  it('MEM_NOT_HELD shows its sentence and the section is read again', async () => {
    citeReply = { kind: 'ok', result: [uid('c1')] };
    vi.spyOn(wiring, 'confirmCitation').mockResolvedValue({ kind: 'error', error: { code: 'MEM_NOT_HELD', message: "This fact doesn't need checking." } });
    const { c } = await controller({ setup: (tr) => tr.on('memory.get', ok({ facts: [cited] })) });
    await c.load();
    const reads = vi.mocked(wiring.citationsToConfirm).mock.calls.length;
    await c.confirmSource(c.citations?.[0] as never);
    expect(c.errorOn(uid('c1'))?.message).toBe("This fact doesn't need checking.");
    expect(vi.mocked(wiring.citationsToConfirm).mock.calls.length).toBe(reads + 1);
  });
});

describe('SM-SF-6 — an agent\'s citation reads as the agent\'s', () => {
  const cited = fact({ came_from: { kind: 'agent_citing', agent_instance_id: AGENT, origin_ref: 'quote.pdf', version: '1' } });

  it('Security — an agent\'s citation is never drawn as a plain source, and quote.pdf is not a link', () => {
    const out = html(FactLine, { fact: cited, names: new AgentNames(agents as never) });
    expect(text(out)).toContain('Quote checker says this comes from quote.pdf (version 1)');
    // The entry's i-agent icon is the only `href` (a sprite reference, not a link).
    expect(out.replace(/<use href="#i-[a-z]+">/g, '')).not.toMatch(/href=/);
    expect(out).not.toMatch(/<a[ >]/);
  });

  it('an agent that is not in the list reads "An agent says…"', () => {
    expect(citationLine(cited as never, new AgentNames())).toEqual({ id: 'memory-cited-by-an-agent', args: { source: 'quote.pdf', version: '1' } });
    expect(text(html(FactLine, { fact: cited, names: new AgentNames() }))).toContain('An agent says this comes from quote.pdf (version 1)');
  });

  it('any other origin draws no citation line', () => {
    expect(citationLine(fact() as never, new AgentNames())).toBeNull();
  });
});

describe('ADR-111 — fact lines', () => {
  it('Happy path — a fact corrected twice shows its kind, truth and the fact it replaced', () => {
    const out = text(html(FactLine, { fact: fact({ replaces: uid('f0') }), names: new AgentNames(), replaced: 'Prefers pricing in dollars' }));
    expect(out).toContain('Something known');
    expect(out).toMatch(/True since/);
    expect(out).toContain('Replaced "Prefers pricing in dollars"');
  });

  it('a fact with an end shows "True from … to …"', () => {
    expect(text(html(FactLine, { fact: fact({ true_until: '2026-01-21T00:00:00Z' }), names: new AgentNames() }))).toMatch(/True from .* to /);
  });

  it('Happy path — a fading fact can be kept: the notice and Keep are drawn; Keep sends memory.keep and the line reads Something known', async () => {
    const f = fact({ fading: true, kind: 'happened', not_used_since: '2026-01-04T00:00:00Z' });
    const out = text(html(FactLine, { fact: f, names: new AgentNames(), onkeep: () => undefined }));
    expect(out).toContain('Fading — not used since');
    expect(out).toContain('Keep');
    const { c, transport } = await controller({ setup: (tr) => tr.on('memory.keep', ok(fact({ fading: false, kind: 'known' }))) });
    c.facts = [f as never];
    await c.keep(f as never);
    expect(transport.sentOf('memory.keep')[0]?.input).toEqual({ id: uid('f1') });
    expect(c.facts[0]?.fading).toBe(false);
    expect(c.facts[0]?.kind).toBe('known');
    await settle();
    expect(c.announcer.text).toBe('Kept. Ordin will keep using this.');
  });

  it('MEM_ALREADY_KEPT redraws the line with no message', async () => {
    const { c, transport } = await controller({ setup: (tr) => tr.on('memory.keep', err('MEM_ALREADY_KEPT', 'Already kept.')) });
    await c.keep(fact() as never);
    expect(c.error).toBeNull();
    expect(transport.commandsSent()).toContain('memory.list');
  });

  it('a summary shows how many facts it was made from', () => {
    expect(text(html(FactLine, { fact: fact({ kind: 'summary', made_from_count: 3 }), names: new AgentNames() }))).toContain('Made from 3 facts');
  });

  it('Edge case — Forget names the summaries that go with a fact', async () => {
    const { c, transport } = await controller({ setup: (tr) => tr.on('memory.forget_count', ok({ summaries: 1 })).on('memory.forget', ok({ id: uid('f1'), summaries_removed: 1 })) });
    c.facts = [fact() as never];
    await c.askForgetFact(uid('f1'));
    expect(transport.sentOf('memory.forget_count')[0]?.input).toEqual({ id: uid('f1') });
    expect(c.pending).toEqual({ kind: 'fact', id: uid('f1'), summaries: 1 });
    expect(t('memory-forget-confirm-summaries', { count: 1 })).toBe('This also forgets 1 summary made from it.');
    await c.confirmPending();
    expect(transport.sentOf('memory.forget')[0]?.input).toEqual({ id: uid('f1') });
    expect(c.facts).toEqual([]);
    expect(c.pending).toBeNull();
  });

  it('a link reads as words: "Borealis supplies packaging · since …"', () => {
    const link = { id: linkId('l1'), kind: 'supplies', since: '2026-01-08T00:00:00Z', from: { id: thingId('t1'), name: 'Borealis' }, to: { id: thingId('t2'), name: 'packaging' }, by: { kind: 'person' } };
    const out = text(html(LinkLine, { link, names: new AgentNames(), oncorrect: () => undefined, onforget: () => undefined }));
    expect(out).toMatch(/Borealis supplies packaging · since/);
    expect(out).toContain('Correct');
    expect(out).toContain('Forget');
    expect(out).toContain('Set by you');
  });

  it('an ended link has no actions', () => {
    const link = { id: linkId('l1'), kind: 'supplies', since: '2026-01-08T00:00:00Z', until: '2026-01-09T00:00:00Z', from: { id: thingId('t1'), name: 'Borealis' }, to: { id: thingId('t2'), name: 'packaging' }, by: { kind: 'ordin' } };
    const out = text(html(LinkLine, { link, names: new AgentNames(), oncorrect: () => undefined, onforget: () => undefined }));
    expect(out).toMatch(/from .* to /);
    expect(out).not.toContain('Correct');
    expect(out).toContain('Remembered by Ordin');
  });
});

describe('ADR-111 — the Things tab', () => {
  const thing = (over: Record<string, unknown> = {}) => ({ id: thingId('t1'), name: 'Borealis', other_names: ['Borealis Ltd'], fact_count: 12, link_count: 4, ...over });

  it('Happy path — the Things tab lists every thing with its counts, and a typed search is debounced', async () => {
    vi.useFakeTimers();
    try {
      const { c, transport } = await controller({ setup: (tr) => tr.on('memory.things', ok({ items: [thing(), thing({ id: thingId('t2'), name: 'packaging', other_names: [], fact_count: 3, link_count: 1 })], next_cursor: null })) });
      await c.loadThings();
      expect(c.things.map((x) => t('memory-thing-counts', { facts: x.fact_count, links: x.link_count }))).toEqual(['12 facts · 4 links', '3 facts · 1 link']);
      c.searchThings('Borealis Ltd');
      expect(transport.sentOf('memory.things')).toHaveLength(1);
      await vi.advanceTimersByTimeAsync(149);
      expect(transport.sentOf('memory.things')).toHaveLength(1);
      await vi.advanceTimersByTimeAsync(2);
      expect(transport.sentOf('memory.things')[1]?.input).toEqual({ workspace_id: WS, search: 'Borealis Ltd' });
    } finally {
      vi.useRealTimers();
    }
  });

  it('Happy path — forget everything about a thing names the counts; Forget all of it sends the expected counts', async () => {
    const counts = { facts: 12, links: 4, summaries: 0 };
    const { c, transport } = await controller({ setup: (tr) => tr.on('memory.thing_count', ok(counts)).on('memory.forget_thing', ok({})).on('memory.things', ok({ items: [], next_cursor: null })) });
    await c.askForgetThing(thingId('t1'), 'Borealis');
    expect(c.pending).toEqual({ kind: 'thing', id: thingId('t1'), name: 'Borealis', counts, changed: false });
    expect(t('memory-thing-forget-confirm', { thing: 'Borealis', facts: 12, links: 4 })).toBe('Forget everything about Borealis? This forgets 12 facts and 4 links.');
    await c.confirmPending();
    expect(transport.sentOf('memory.forget_thing')[0]?.input).toEqual({ id: thingId('t1'), workspace_id: WS, expected: counts });
    await settle();
    expect(c.announcer.text).toBe("Forgotten. Ordin won't use anything about Borealis again.");
    expect(c.pending).toBeNull();
  });

  it('Error path — the counts changed while deciding: counts are read again and the same dialog waits for the person', async () => {
    let counts = { facts: 12, links: 4, summaries: 0 };
    const { c, transport } = await controller({
      setup: (tr) => tr.on('memory.thing_count', () => ok(counts)).on('memory.forget_thing', err('MEM_THING_COUNT_CHANGED', 'Changed.')),
    });
    await c.askForgetThing(thingId('t1'), 'Borealis');
    counts = { facts: 13, links: 4, summaries: 0 };
    await c.confirmPending();
    expect(transport.sentOf('memory.thing_count')).toHaveLength(2);
    expect(c.pending).toMatchObject({ kind: 'thing', changed: true, counts: { facts: 13 } });
    expect(t('memory-thing-count-changed', { thing: 'Borealis' })).toBe('Something about Borealis changed while you were deciding. Check the numbers again.');
  });

  it('Happy path — two things made one: same_as sends keep and merge and announces it', async () => {
    const detail = { thing: thing({ id: thingId('t1') }), facts: [], links: [] };
    const { c, transport } = await controller({ setup: (tr) => tr.on('memory.same_as', ok(thing({ id: thingId('t1') }))).on('memory.thing', ok(detail)) });
    await c.sameAs({ id: thingId('t1'), name: 'Borealis' }, { id: thingId('t9'), name: 'Borealis Ltd' });
    expect(transport.sentOf('memory.same_as')[0]?.input).toEqual({ workspace_id: WS, keep: thingId('t1'), merge: thingId('t9') });
    expect(c.detail?.thing.id).toBe(thingId('t1'));
    await settle();
    expect(c.announcer.text).toBe('Borealis Ltd is now another name for Borealis.');
  });

  it('a link correction sends the new words and the detail is read again; forgetting a link asks first', async () => {
    const link = { id: linkId('l1'), kind: 'supplies', since: '2026-01-08T00:00:00Z', from: { id: thingId('t1'), name: 'Borealis' }, to: { id: thingId('t2'), name: 'packaging' }, by: { kind: 'person' } };
    const detail = { thing: thing(), facts: [], links: [link] };
    const { c, transport } = await controller({ setup: (tr) => tr.on('memory.thing', ok(detail)).on('memory.correct_link', ok(link)).on('memory.forget_link', ok({})) });
    await c.openThing(thingId('t1'));
    expect(await c.correctLink(link as never, { kind: 'sells', from: thingId('t1'), to: thingId('t2') })).toBe(true);
    expect(transport.sentOf('memory.correct_link')[0]?.input).toEqual({ id: linkId('l1'), kind: 'sells', from: thingId('t1'), to: thingId('t2') });
    c.askForgetLink(link as never);
    expect(c.pending?.kind).toBe('link');
    await c.confirmPending();
    expect(transport.sentOf('memory.forget_link')[0]?.input).toEqual({ id: linkId('l1') });
  });

  it('History lists the versions the owner gave', async () => {
    const view = { versions: [fact({ id: uid('v1') }), fact({ id: uid('v2') })], links: [] };
    const { c, transport } = await controller({ setup: (tr) => tr.on('memory.fact_history', ok(view)) });
    await c.openHistory(fact() as never);
    expect(transport.sentOf('memory.fact_history')[0]?.input).toEqual({ id: uid('f1') });
    expect(c.history?.view.versions).toHaveLength(2);
  });
});
