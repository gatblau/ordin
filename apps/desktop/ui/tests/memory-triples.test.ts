// Facts as a thing, a word and a value; conflicts; correcting a value; the Things list; words; what Ordin
// read; a thing's summaries; and the memory diagram's data (ScreensModule.md SM-SF-13 to SM-SF-24).
// Controllers over the fake transport, lines drawn with `svelte/server`, the diagram's option as a plain
// object. The screens in a DOM are in tests/memory-diagram-screen.test.ts.
import { beforeAll, describe, expect, it, vi } from 'vitest';

import * as wiring from '../src/lib/backend/wiring';
import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { KnowledgeController } from '../src/lib/views/fixed/knowledge-controller.svelte';
import { MemoryController, namesAKindOfNote } from '../src/lib/views/fixed/memory-controller.svelte';
import { UsedController } from '../src/lib/views/fixed/memory-used.svelte';
import {
  AgentNames,
  addedByLine,
  dateText,
  draftOf,
  draftUnchanged,
  isFactDate,
  objectInputOf,
  objectText,
  tripleLine,
  tripleName,
  wordLabel,
  wordMeaning,
  worthListing,
} from '../src/lib/views/fixed/facts';
import { diagramGraph, diagramOption, diagramPress, type DiagramColours } from '../src/lib/views/fixed/diagram-option';
import FactLine from '../src/lib/views/fixed/FactLine.svelte';
import { err, ok, settle } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { thingId, uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));
const plain = (markup: string): string => stripIsolates(text(markup)).trim();

const KESTREL = { id: thingId('kestrel'), name: 'Kestrel' };
const TOM = { id: thingId('tom'), name: 'Tom Reid' };
const NORTHWIND = { id: thingId('northwind'), name: 'Northwind' };
const AGENT = '0192f400-0000-7000-8000-0000000000a1';
const agents = [{ instance: { id: AGENT, display_name: 'Quote checker' }, tasks: [], uses: [], workspaces: [] }];

const triple = (object: Record<string, unknown>, label = 'account manager', subject = KESTREL) => ({
  subject,
  predicate: { key: label.replace(/ (\w)/g, (_, c: string) => c.toUpperCase()), label },
  object,
});
const fact = (id: string, over: Record<string, unknown> = {}) => ({
  id: uid(id), statement: `Statement ${id}`, kind: 'known', true_from: '2026-01-10T00:00:00Z', written_at: '2026-01-10T00:00:00Z',
  came_from: { kind: 'person' }, fading: false, may_be_out_of_date: false, needs_check: false, ...over,
});
const money = fact('m', { triple: triple({ kind: 'money', amount: '3000000', currency: 'GBP' }) });
const manager = fact('a', { triple: triple({ kind: 'thing', thing: TOM }) });

async function controller(setup: Parameters<typeof backendWith>[0] = {}) {
  vi.spyOn(wiring, 'heldWrites').mockResolvedValue({ kind: 'not_wired' });
  vi.spyOn(wiring, 'citationsToConfirm').mockResolvedValue({ kind: 'not_wired' });
  const b = await backendWith({ ...setup, setup: (tr) => { tr.on('agents.list', ok(agents)); tr.on('memory.list', ok({ items: [], next_cursor: null })); setup.setup?.(tr); } });
  return { ...b, c: new MemoryController(b.handle, WS) };
}

// ---- SM-SF-13 -------------------------------------------------------------------------------------------

describe('SM-SF-13 — a fact reads as its three parts', () => {
  const names = new AgentNames();

  it('Happy path — one line that reads as a sentence: "Kestrel · account manager · £3,000,000.00"', () => {
    const out = html(FactLine, { fact: money, names, onopenthing: () => undefined });
    // Amended January 2026: subject · word · value on one line.
    const claim = out.match(/<p class="statement claim[^"]*">([\s\S]*?)<\/p>/)?.[1] ?? '';
    expect(plain(claim)).toBe('Kestrel · account manager · £3,000,000.00');
    // Collapsed by default: the evidence line waits behind Details (DOM cases in fact-line-details.test.ts).
    expect(out).not.toContain('class="secondary');
    // Details is an icon button named by its aria-label (amended January 2026).
    expect(out).toContain('aria-label="Details"');
    // The accessible name is the three parts joined by spaces, never `·`.
    expect(stripIsolates(out)).toContain('aria-label="Kestrel account manager £3,000,000.00"');
    // The subject is a link button that opens the thing.
    expect(out).toMatch(/<button type="button" class="link[^"]*"[^>]*>Kestrel<\/button>/);
  });

  it('a thing value is a link button too; text, number, dates and yes/no read as written', () => {
    expect(html(FactLine, { fact: manager, names })).toMatch(/<button type="button" class="link[^"]*"[^>]*>Tom Reid<\/button>/);
    expect(objectText({ kind: 'text', value: 'Pays in 30 days' })).toBe('Pays in 30 days');
    expect(objectText({ kind: 'number', value: '240', unit: 'staff' })).toBe('240 staff');
    expect(objectText({ kind: 'number', value: '240' })).toBe('240');
    expect(objectText({ kind: 'yes_no', value: true })).toBe('yes');
    expect(objectText({ kind: 'yes_no', value: false })).toBe('no');
  });

  it('Edge — a date is drawn by its precision: the year, the month and year, or the day', () => {
    expect(dateText('2026')).toBe('2026');
    expect(dateText('2026-04')).toBe('April 2026');
    expect(dateText('2026-04-15')).toBe('15 Apr 2026');
  });

  it('Edge — an older note is marked and cannot be corrected', () => {
    const older = fact('o', { older_note: true });
    const out = html(FactLine, { expanded: true, fact: older, names, oncorrect: () => undefined, onforget: () => undefined });
    expect(plain(out)).toContain('Older note');
    expect(stripIsolates(out)).toContain(`title="${t('memory-older-note-help')}"`);
    expect(plain(out)).toContain('Statement o');
    expect(out).toContain(`aria-label="${t('memory-link-forget')}"`);
    expect(out).not.toContain(`aria-label="${t('memory-correct')}"`);
  });

  it('Error path — money with a currency Intl refuses reads as the amount and the code, never an error', () => {
    expect(objectText({ kind: 'money', amount: '12', currency: 'ZZ' })).toBe('12 ZZ');
    expect(objectText({ kind: 'money', amount: 'lots', currency: 'GBP' })).toBe('lots GBP');
  });

  it('Edge — a fact with a triple draws no separate list of things; one without keeps it', () => {
    const named = fact('n', { things: [KESTREL] });
    expect(plain(html(FactLine, { expanded: true, fact: named, names }))).toContain('Kestrel');
    const both = { ...manager, things: [{ id: thingId('other'), name: 'Elsewhere' }] };
    expect(plain(html(FactLine, { expanded: true, fact: both, names }))).not.toContain('Elsewhere');
  });
});

// ---- SM-SF-14 -------------------------------------------------------------------------------------------

describe('SM-SF-14 — conflicts with, and Keep this one', () => {
  const A = fact('A', { triple: triple({ kind: 'money', amount: '100', currency: 'GBP' }, 'price'), conflicts_with: [uid('B')] });
  const B = fact('B', { triple: triple({ kind: 'money', amount: '120', currency: 'GBP' }, 'price'), conflicts_with: [uid('A')] });

  it('Happy path — keep one of two conflicting facts: keep_conflict_side { keep: A, other: B } is sent and the list is read again', async () => {
    const keep = vi.fn(async () => ok(A));
    const { c, transport } = await controller({ setup: (tr) => { tr.on('memory.list', ok({ items: [A, B], next_cursor: null })); tr.on('memory.keep_conflict_side', keep as never); } });
    await c.loadFacts();
    expect(transport.sentOf('memory.get')).toHaveLength(0); // both are on screen
    c.askKeepSide(c.facts[0]!);
    expect(c.keepPending?.others.map((f) => f.id)).toEqual([uid('B')]);
    const lists = transport.sentOf('memory.list').length;
    await c.confirmKeepSide();
    expect(transport.sentOf('memory.keep_conflict_side').map((e) => e.input)).toEqual([{ keep: uid('A'), other: uid('B') }]);
    expect(transport.sentOf('memory.list').length).toBe(lists + 1);
    await settle();
    expect(c.announcer.text).toBe(t('memory-keep-this-done'));
    expect(c.keepPending).toBeNull();
  });

  it('the line carries the icon info and the words, each side offers Keep this one, and a fact in the list is a link', () => {
    const out = html(FactLine, { expanded: true, fact: A, names: new AgentNames(), conflicting: [{ fact: B, here: true }], onkeepside: () => undefined });
    expect(out).toMatch(/<p class="conflict[^"]*">[\s\S]{0,60}?<svg class="icon icon-small" aria-hidden="true" focusable="false"><use href="#i-info"/);
    expect(plain(out)).toContain('Conflicts with: Kestrel · price · £120.00');
    expect(out).toMatch(/<button type="button" class="link[^"]*"[^>]*>Kestrel · price · £120.00<\/button>/);
    expect(plain(out)).toContain(t('memory-keep-this'));
  });

  it('Edge — a conflicting fact not on screen is read once with memory.get and drawn as text', async () => {
    const lone = fact('L', { triple: triple({ kind: 'text', value: 'Sam' }), conflicts_with: [uid('X'), uid('Y')] });
    const other = fact('M', { conflicts_with: [uid('X')] });
    const { c, transport } = await controller({
      setup: (tr) => {
        tr.on('memory.list', ok({ items: [lone, other], next_cursor: null }));
        tr.on('memory.get', ok({ facts: [fact('X', { triple: triple({ kind: 'text', value: 'Ann' }) })] }));
      },
    });
    await c.loadFacts();
    expect(transport.sentOf('memory.get').map((e) => e.input)).toEqual([{ ids: [uid('X'), uid('Y')], workspace_id: WS }]);
    // Y was not returned (forgotten meanwhile): it is not drawn.
    expect(c.conflictsOf(c.facts[0]!).map((f) => f.id)).toEqual([uid('X')]);
    const out = html(FactLine, { expanded: true, fact: lone, names: new AgentNames(), conflicting: [{ fact: c.conflictsOf(c.facts[0]!)[0]!, here: false }] });
    expect(plain(out)).toContain('Conflicts with: Kestrel · account manager · Ann');
    expect(out).not.toMatch(/<button[^>]*>Kestrel · account manager · Ann/);
    await c.loadConflicts(c.facts);
    expect(transport.sentOf('memory.get')).toHaveLength(2); // Y again only, X is kept
    expect(transport.sentOf('memory.get')[1]?.input).toEqual({ ids: [uid('Y')], workspace_id: WS });
  });

  it('Edge — three facts in conflict: the person chooses the one to keep, and one call goes per fact it conflicts with', async () => {
    const A3 = { ...A, conflicts_with: [uid('B'), uid('C')] };
    const C = fact('C', { triple: triple({ kind: 'money', amount: '90', currency: 'GBP' }, 'price'), conflicts_with: [uid('A')] });
    const B3 = { ...B, conflicts_with: [uid('A')] };
    const { c, transport } = await controller({ setup: (tr) => { tr.on('memory.list', ok({ items: [A3, B3, C], next_cursor: null })); tr.on('memory.keep_conflict_side', ok(A3)); } });
    await c.loadFacts();
    c.askKeepSide(c.facts[0]!);
    await c.confirmKeepSide();
    expect(transport.sentOf('memory.keep_conflict_side').map((e) => e.input)).toEqual([
      { keep: uid('A'), other: uid('B') },
      { keep: uid('A'), other: uid('C') },
    ]);
    transport.sent.length = 0;
    c.askKeepSide(c.facts[0]!);
    c.chooseKeep(uid('B'));
    await c.confirmKeepSide();
    expect(transport.sentOf('memory.keep_conflict_side').map((e) => e.input)).toEqual([{ keep: uid('B'), other: uid('A') }]);
  });

  it('Error path — MEM_CONFLICT_NOT_FOUND draws its sentence on the line and reads the list again; later calls are not sent', async () => {
    const A3 = { ...A, conflicts_with: [uid('B'), uid('C')] };
    const C = fact('C', { conflicts_with: [uid('A')] });
    const { c, transport } = await controller({
      setup: (tr) => {
        tr.on('memory.list', ok({ items: [A3, B, C], next_cursor: null }));
        tr.on('memory.keep_conflict_side', err('MEM_CONFLICT_NOT_FOUND', 'These facts no longer disagree.'));
      },
    });
    await c.loadFacts();
    c.askKeepSide(c.facts[0]!);
    const lists = transport.sentOf('memory.list').length;
    await c.confirmKeepSide();
    expect(transport.sentOf('memory.keep_conflict_side')).toHaveLength(1);
    expect(c.errorOn(uid('A'))?.message).toBe('These facts no longer disagree.');
    expect(transport.sentOf('memory.list').length).toBe(lists + 1);
  });

  it('Error path — another refusal stays on the line and the list is not read again', async () => {
    const { c, transport } = await controller({
      setup: (tr) => {
        tr.on('memory.list', ok({ items: [A, B], next_cursor: null }));
        tr.on('memory.keep_conflict_side', err('MEM_FACT_NOT_FOUND', 'That fact is no longer here.'));
      },
    });
    await c.loadFacts();
    c.askKeepSide(c.facts[0]!);
    const lists = transport.sentOf('memory.list').length;
    await c.confirmKeepSide();
    expect(c.errorOn(uid('A'))?.message).toBe('That fact is no longer here.');
    expect(transport.sentOf('memory.list').length).toBe(lists);
  });
});

// ---- SM-SF-15 -------------------------------------------------------------------------------------------

describe('SM-SF-15 — correct a value', () => {
  it('Happy path — Correct sends memory.correct { id, object }, announces it and reads the list again', async () => {
    const correct = vi.fn(async () => ok(money));
    const { c, transport } = await controller({ setup: (tr) => { tr.on('memory.correct', correct as never); } });
    await c.correct(money as never, { money: { amount: '3200000', currency: 'GBP' } });
    expect(transport.sentOf('memory.correct')[0]?.input).toEqual({ id: uid('m'), object: { money: { amount: '3200000', currency: 'GBP' } } });
    await settle();
    expect(c.announcer.text).toBe(t('memory-corrected'));
    expect(transport.sentOf('memory.list')).toHaveLength(1);
  });

  it.each([
    ['a thing by its name', { kind: 'thing', a: 'Ann Lee', b: '', yes: false }, { thing: { name: 'Ann Lee' } }],
    ['a text', { kind: 'text', a: ' Thirty days ', b: '', yes: false }, { text: 'Thirty days' }],
    ['a number with a unit', { kind: 'number', a: '12.5', b: 'kg', yes: false }, { number: { value: '12.5', unit: 'kg' } }],
    ['a number with no unit', { kind: 'number', a: '7', b: '', yes: false }, { number: { value: '7', unit: null } }],
    ['money', { kind: 'money', a: '10.00', b: 'EUR', yes: false }, { money: { amount: '10.00', currency: 'EUR' } }],
    ['a year', { kind: 'date', a: '2026', b: '', yes: false }, { date: '2026' }],
    ['a month', { kind: 'date', a: '2026-04', b: '', yes: false }, { date: '2026-04' }],
    ['a day', { kind: 'date', a: '2026-04-15', b: '', yes: false }, { date: '2026-04-15' }],
    ['yes or no', { kind: 'yes_no', a: '', b: '', yes: true }, { yes_no: true }],
  ])('a value of its form is sent: %s', (_, draft, input) => {
    expect(objectInputOf(draft as never)).toEqual(input);
  });

  it.each([
    ['an empty name', { kind: 'thing', a: '  ', b: '' }],
    ['a name over 200', { kind: 'thing', a: 'x'.repeat(201), b: '' }],
    ['a text over 1,000', { kind: 'text', a: 'x'.repeat(1001), b: '' }],
    ['a number that is not a decimal', { kind: 'number', a: '12,5', b: '' }],
    ['a unit over 32', { kind: 'number', a: '1', b: 'u'.repeat(33) }],
    ['a currency of two letters', { kind: 'money', a: '1', b: 'GB' }],
    ['a currency in lower case', { kind: 'money', a: '1', b: 'gbp' }],
    ['a date of no form', { kind: 'date', a: '15/04/2026', b: '' }],
    ['a day that does not exist', { kind: 'date', a: '2026-02-30', b: '' }],
    ['a thirteenth month', { kind: 'date', a: '2026-13', b: '' }],
    ['a box emptied to nothing', { kind: 'text', a: null, b: null }],
  ])('Save stays off for %s', (_, draft) => {
    expect(objectInputOf({ yes: false, ...draft } as never)).toBeNull();
  });

  it('Edge — Save stays off while the value is unchanged', () => {
    const object = { kind: 'money', amount: '3000000', currency: 'GBP' } as const;
    expect(draftUnchanged(draftOf(object), object)).toBe(true);
    expect(draftUnchanged({ ...draftOf(object), a: '3100000' }, object)).toBe(false);
    expect(isFactDate('2024-02-29')).toBe(true);
  });

  it('Error path — a refused correction draws the owner sentence on the line; the list is not read again', async () => {
    const { c, transport } = await controller({ setup: (tr) => { tr.on('memory.correct', err('MEM_TRIPLE_INVALID', "That value can't be saved like that.")); } });
    await c.correct(money as never, { text: 'x' });
    expect(c.errorOn(uid('m'))?.message).toBe("That value can't be saved like that.");
    expect(transport.sentOf('memory.list')).toHaveLength(0);
  });

  it('Error path — MEM_OLDER_NOTE from the backend draws its sentence on the line', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.correct', err('MEM_OLDER_NOTE', "An older note can't be corrected. Forget it and tell Ordin the fact again.")); } });
    await c.correct(fact('o') as never, { text: 'x' });
    expect(c.errorOn(uid('o'))?.message).toContain("An older note can't be corrected.");
  });
});

// ---- SM-SF-16 -------------------------------------------------------------------------------------------

describe('SM-SF-16 — the Things list', () => {
  const thing = (label: string, fact_count: number, link_count: number) => ({ id: thingId(label), name: label, other_names: [], fact_count, link_count });

  it('Happy path — the things memory.things returns are drawn', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.things', ok({ items: [thing('kestrel', 3, 1), thing('tom', 1, 0)], next_cursor: null })); } });
    await c.loadThings();
    expect(c.things.map((x) => x.name)).toEqual(['kestrel', 'tom']);
  });

  it('Edge — a thing with links but no live fact is never drawn, whatever its link count', () => {
    expect(worthListing({ fact_count: 0 })).toBe(false);
    expect(worthListing({ fact_count: 1 })).toBe(true);
  });

  it('Error path — a refused read draws its sentence for the screen', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.things', err('MEM_STORE_UNAVAILABLE', "ordin can't reach its memory right now. Try again in a moment.")); } });
    await c.loadThings();
    expect(c.errorOn('')?.code).toBe('MEM_STORE_UNAVAILABLE');
  });
});

// ---- SM-SF-17 -------------------------------------------------------------------------------------------

const term = (key: string, label: string, over: Record<string, unknown> = {}) => ({
  key, label, meaning: `Meaning of ${label}`, object: 'thing', single_valued: false, seed_unrenamed: false, added_by: { kind: 'person' }, fact_count: 1, ...over,
});

describe('SM-SF-17 — the Words tab', () => {
  const provides = term('provides', 'provides', { fact_count: 3, added_by: { kind: 'ordin' } });
  const supplies = term('supplies', 'supplies');
  const price = term('price', 'price', { object: 'money', single_valued: true, seed_unrenamed: true, added_by: { kind: 'seed' } });

  it('Happy path — rename and merge a word: merge_term is sent and "Merged. 3 facts are now of the other kind." is announced', async () => {
    const { c, transport } = await controller({
      setup: (tr) => {
        tr.on('memory.vocabulary', ok({ terms: [price, provides, supplies] }));
        tr.on('memory.merge_term', ok({ kept: supplies, rewritten: 3 }));
        tr.on('memory.rename_term', ok({ ...supplies, label: 'delivers' }));
      },
    });
    await c.loadWords();
    // "price" is not offered as a target, its object being money.
    expect(c.mergeTargets(c.words![1]! as never).map((w) => w.key)).toEqual(['supplies']);
    expect(await c.mergeWord(provides as never, 'supplies')).toBeNull();
    expect(transport.sentOf('memory.merge_term')[0]?.input).toEqual({ workspace_id: WS, key: 'provides', into: 'supplies' });
    await settle();
    expect(c.announcer.text).toBe('Merged. 3 facts are now of the other kind.');
    expect(await c.renameWord(supplies as never, 'delivers')).toBe(true);
    expect(transport.sentOf('memory.rename_term')[0]?.input).toEqual({ workspace_id: WS, key: 'supplies', label: 'delivers' });
    await settle();
    expect(c.announcer.text).toBe(t('memory-word-renamed'));
    expect(transport.sentOf('memory.vocabulary')).toHaveLength(3);
  });

  it('a word reads: its label, meaning, what it links to, one or several, who added it and how many facts use it', () => {
    const names = new AgentNames(agents as never);
    // A seed word never renamed reads from the catalogue; a renamed one as stored.
    expect(wordLabel(term('worksFor', 'works for', { seed_unrenamed: true }) as never)).toBe(t('memory-term-works-for'));
    expect(wordMeaning(term('worksFor', 'x', { seed_unrenamed: true }) as never)).toBe('An organisation the subject works for.');
    expect(wordLabel(term('worksFor', 'employed by', { seed_unrenamed: false }) as never)).toBe('employed by');
    expect(wordLabel(term('accountManager', 'account manager', { seed_unrenamed: true }) as never)).toBe('account manager');
    const by = (origin: Record<string, unknown>) => { const m = addedByLine(origin as never, names); return t(m.id, m.args); };
    expect(by({ kind: 'seed' })).toBe('Built in');
    expect(by({ kind: 'ordin' })).toBe('Added by Ordin');
    expect(by({ kind: 'agent', agent_instance_id: AGENT })).toBe('Added by Quote checker');
    expect(by({ kind: 'agent', agent_instance_id: uid('gone') })).toBe('Added by an agent');
    expect(by({ kind: 'hosted_agent' })).toBe("Added by your organisation's agent");
    expect(by({ kind: 'team' })).toBe('From your team');
    expect(by({ kind: 'person' })).toBe('Added by you');
    expect(t('memory-word-count', { count: 1 })).toBe('Used by 1 fact');
  });

  it('Edge — a word with no other word of its kind offers no merge target', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.vocabulary', ok({ terms: [price, supplies] })); } });
    await c.loadWords();
    expect(c.mergeTargets(price as never)).toEqual([]);
  });

  it('Error path — MEM_TERM_LABEL_TAKEN draws its sentence under the box and nothing is announced', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.rename_term', err('MEM_TERM_LABEL_TAKEN', 'Another word already reads like that. Merge them instead.')); } });
    expect(await c.renameWord(supplies as never, 'provides')).toBe(false);
    expect(c.errorOn('word:supplies')?.message).toBe('Another word already reads like that. Merge them instead.');
    await settle();
    expect(c.announcer.text).toBe('');
  });

  it('Error path — a refused merge is returned for the dialog to draw', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.merge_term', err('MEM_TERM_KIND_DIFFERS', "These words link to different kinds of things, so they can't be merged.")); } });
    expect((await c.mergeWord(provides as never, 'price'))?.code).toBe('MEM_TERM_KIND_DIFFERS');
  });

  it('Error path — a refused read of the words draws its sentence on the tab', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.vocabulary', err('MEM_STORE_TIMEOUT', "ordin's memory took too long to answer. Try again in a moment.")); } });
    await c.loadWords();
    expect(c.words).toBeNull();
    expect(c.errorOn('words')?.code).toBe('MEM_STORE_TIMEOUT');
  });
});

// ---- SM-SF-26 -------------------------------------------------------------------------------------------

describe('SM-SF-26 — a kind of fact named by a kind of note', () => {
  it('reads labels as MEM-TR-13 (c) reads a proposed one', () => {
    for (const label of ['key fact', 'Key facts', 'KEY_FACT', 'known-issues', 'Note', 'issues', 'Summaries', 'detail', 'point.', 'infos']) {
      expect(namesAKindOfNote(label), label).toBe(true);
    }
    for (const label of ['target year', 'budget', 'supplies', 'footnote', 'price point', 'notes app']) {
      expect(namesAKindOfNote(label), label).toBe(false);
    }
  });

  it('Happy path — "Key facts" shows the line and "target year" does not', async () => {
    const keyFacts = term('keyFacts', 'Key facts', { object: 'text' });
    const target = term('targetYear', 'target year', { object: 'text' });
    const { c } = await controller({ setup: (tr) => { tr.on('memory.vocabulary', ok({ terms: [keyFacts, target] })); } });
    await c.loadWords();
    expect(c.notAKind(keyFacts as never)).toBe(true);
    expect(c.notAKind(target as never)).toBe(false);
    expect(t('memory-word-not-a-kind')).toBe('Not a kind of fact. Use Merge into… to move these facts to a better kind.');
  });

  it('Edge case — a word with nothing to merge into shows no line', async () => {
    const note = term('note', 'note', { object: 'text' });
    const supplies = term('supplies', 'supplies');
    const { c } = await controller({ setup: (tr) => { tr.on('memory.vocabulary', ok({ terms: [note, supplies] })); } });
    await c.loadWords();
    expect(c.notAKind(note as never)).toBe(false);
  });
});

// ---- SM-SF-18 -------------------------------------------------------------------------------------------

describe('SM-SF-18 — no What Ordin read on a knowledge page (amended January 2026)', () => {
  it('Edge case — a knowledge page draws no What Ordin read and asks for no passages', async () => {
    const { readFileSync } = await import('node:fs');
    const source = readFileSync(new URL('../src/lib/views/fixed/Knowledge.svelte', import.meta.url), 'utf8');
    expect(source).not.toContain("'memory.passages'");
    expect(source).not.toContain('memory-read-title');
  });
});

// ---- SM-SF-19 -------------------------------------------------------------------------------------------

describe("SM-SF-19 — a thing's summaries", () => {
  const summary = fact('sum', { kind: 'summary', made_from_count: 4 });
  const detail = (summaries: unknown[]) => ({ thing: { ...KESTREL, other_names: [], fact_count: 1, link_count: 0 }, facts: [manager], links: [], summaries });

  it('Happy path — the summaries come with the thing; Forget asks first and the page is read again', async () => {
    const { c, transport } = await controller({
      setup: (tr) => {
        tr.on('memory.thing', ok(detail([summary])));
        tr.on('memory.forget_count', ok({ summaries: 0 }));
        tr.on('memory.forget', ok({}));
      },
    });
    await c.openThing(KESTREL.id);
    expect(c.detail?.summaries?.map((s) => s.id)).toEqual([uid('sum')]);
    await c.askForgetFact(uid('sum'));
    expect(c.pending).toEqual({ kind: 'fact', id: uid('sum'), summaries: 0 });
    await c.confirmPending();
    expect(transport.sentOf('memory.forget')[0]?.input).toEqual({ id: uid('sum') });
    expect(transport.sentOf('memory.thing')).toHaveLength(2);
  });

  it('Edge — a thing with no summaries has none to draw', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.thing', ok(detail([]))); } });
    await c.openThing(KESTREL.id);
    expect(c.detail?.summaries).toEqual([]);
  });

  it('Error path — a refused Forget draws its sentence on the summary', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.thing', ok(detail([summary]))); tr.on('memory.forget_count', err('MEM_FACT_NOT_FOUND', 'That fact is no longer here.')); } });
    await c.openThing(KESTREL.id);
    await c.askForgetFact(uid('sum'));
    expect(c.errorOn(uid('sum'))?.message).toBe('That fact is no longer here.');
  });
});

// ---- SM-SF-20 to SM-SF-24: the diagram's data -----------------------------------------------------------

const join = (from: { id: string }, to: { id: string }, label: string, id: string, conflict = false) => ({ from: from.id, to: to.id, word: { key: label, label }, fact_id: uid(id), conflict });
const view = (over: Record<string, unknown> = {}) => ({
  centre: KESTREL,
  things: [{ thing: KESTREL, step: 0, fact_count: 3 }, { thing: TOM, step: 1, fact_count: 1 }, { thing: NORTHWIND, step: 2, fact_count: 1 }],
  joins: [join(KESTREL, TOM, 'account manager', 'j1'), join(TOM, NORTHWIND, 'works for', 'j2')],
  values: [{ word: { key: 'price', label: 'price' }, object: { kind: 'money', amount: '120', currency: 'GBP' }, fact_id: uid('v1'), conflict: false }],
  conflicts: [],
  left_out: 0,
  total_things: 3,
  ...over,
});
const colours: DiagramColours = { accent: 'A', onAccent: 'OA', surface: 'S', text: 'T', muted: 'M', line: 'L' };

describe('SM-SF-21 — what is drawn', () => {
  it('Happy path — a rounded box per thing, the centre larger and in the accent; an arrow per join labelled by its word', () => {
    const { nodes, edges } = diagramGraph(view() as never, colours);
    expect(nodes.map((n) => n.name)).toEqual(['Kestrel', 'Tom Reid', 'Northwind', '£120.00']);
    expect(nodes.every((n) => n.symbol === 'roundRect')).toBe(true);
    expect(nodes[0]?.itemStyle.color).toBe('A');
    expect(nodes[0]!.symbolSize[0]).toBeGreaterThan(nodes[1]!.symbolSize[0]);
    expect(edges.map((e) => e.label.formatter)).toEqual(['account manager', 'works for', 'price']);
    const option = diagramOption(view() as never, colours, { animate: false }) as { aria: unknown; series: Array<Record<string, unknown>> };
    expect(option.aria).toEqual({ enabled: false });
    expect(option.series[0]).toMatchObject({ type: 'graph', layout: 'force', roam: true, draggable: false, edgeSymbol: ['none', 'arrow'] });
  });

  it('Edge — conflicts are dashed and labelled: both edges end with "(Conflicts)"', () => {
    const v = view({ joins: [join(KESTREL, TOM, 'account manager', 'j1', true), join(KESTREL, NORTHWIND, 'account manager', 'j3', true)] });
    const { edges } = diagramGraph(v as never, colours);
    const conflicted = edges.filter((e) => e.factId !== uid('v1'));
    expect(conflicted.map((e) => e.lineStyle.type)).toEqual(['dashed', 'dashed']);
    expect(conflicted.map((e) => stripIsolates(e.label.formatter))).toEqual(['account manager (Conflicts)', 'account manager (Conflicts)']);
  });

  it('Edge — without a centre no value is drawn, and a join to a thing not drawn is left out', () => {
    const { nodes, edges } = diagramGraph(view({ centre: null, joins: [join(KESTREL, { id: thingId('elsewhere') }, 'owns', 'j9')] }) as never, colours);
    expect(nodes).toHaveLength(3);
    expect(edges).toEqual([]);
  });

  it('Error path — a press on nothing the drawing gave means nothing', () => {
    expect(diagramPress(null)).toBeNull();
    expect(diagramPress({ dataType: 'node', data: { id: 'other' } })).toBeNull();
    expect(diagramPress({ dataType: 'edge', data: {} })).toBeNull();
    expect(diagramPress({ dataType: 'node', data: { id: `thing:${TOM.id}` } })).toEqual({ kind: 'thing', id: TOM.id });
    expect(diagramPress({ dataType: 'node', data: { id: `value:${uid('v1')}` } })).toEqual({ kind: 'fact', id: uid('v1') });
    expect(diagramPress({ dataType: 'edge', data: { factId: uid('j1') } })).toEqual({ kind: 'fact', id: uid('j1') });
  });
});

describe('SM-SF-24 — bounds shown', () => {
  const many = (n: number, prefix: string) => Array.from({ length: n }, (_, i) => ({ thing: { id: thingId(`${prefix}${i}`), name: `${prefix}${i}` }, fact_count: 1 }));

  it('Happy path — 40 things, 200 joins and 40 values draw at most 80 boxes and 240 arrows', () => {
    const things = many(40, 't');
    const centre = things[0]!.thing;
    const joins = Array.from({ length: 200 }, (_, i) => join(things[i % 40]!.thing, things[(i + 1) % 40]!.thing, 'owns', `j${i}`));
    const values = Array.from({ length: 40 }, (_, i) => ({ word: { key: 'price', label: 'price' }, object: { kind: 'number', value: String(i) }, fact_id: uid(`v${i}`), conflict: false }));
    const { nodes, edges } = diagramGraph({ centre, things, joins, values, conflicts: [], left_out: 172, total_things: 212 } as never, colours);
    expect(nodes).toHaveLength(80);
    expect(edges).toHaveLength(240);
  });

  it('Edge — the left-out line reads "Showing 40 of 212 things — search to centre on one"', () => {
    expect(t('memory-diagram-left-out', { shown: 40, total: 212 })).toBe('Showing 40 of 212 things — search to centre on one');
  });

  it('Edge — an empty memory draws nothing', () => {
    expect(diagramGraph({ centre: null, things: [], joins: [], values: [], conflicts: [], left_out: 0, total_things: 0 } as never, colours)).toEqual({ nodes: [], edges: [] });
  });
});

describe('SM-SF-20, SM-SF-22, SM-SF-23 — the diagram controller', () => {
  const thingDetail = (ref: { id: string; name: string }, facts: unknown[]) => ({ thing: { ...ref, other_names: [], fact_count: facts.length, link_count: 0 }, facts, links: [] });
  const centred = (input: unknown) => {
    const centre = (input as { centre?: string }).centre;
    if (centre === undefined) return ok(view({ centre: null }));
    const ref = [KESTREL, TOM, NORTHWIND].find((x) => x.id === centre)!;
    return ok(view({ centre: ref }));
  };

  it('Happy path — the diagram around a thing: memory.diagram { centre } is sent, the centre\'s facts are read, and pressing a node centres it', async () => {
    const { c, transport } = await controller({
      setup: (tr) => {
        tr.on('memory.diagram', centred);
        tr.on('memory.thing', (input) => ok(thingDetail((input as { id: string }).id === TOM.id ? TOM : KESTREL, [manager])));
      },
    });
    await c.openThing(KESTREL.id).catch(() => undefined);
    await c.openDiagram(KESTREL);
    expect(c.diagramPlace).toEqual({ from: 'thing' });
    expect(transport.sentOf('memory.diagram')[0]?.input).toEqual({ workspace_id: WS, centre: KESTREL.id });
    expect(c.diagramDetail?.thing.name).toBe('Kestrel');
    await settle();
    expect(c.announcer.text).toBe('Centred on Kestrel');
    await c.centre(TOM.id);
    expect(transport.sentOf('memory.diagram')[1]?.input).toEqual({ workspace_id: WS, centre: TOM.id });
    await settle();
    expect(c.announcer.text).toBe('Centred on Tom Reid');
  });

  it('Edge — from the Things tab there is no centre and no list of facts; Back returns to the Things tab', async () => {
    const { c, transport } = await controller({ setup: (tr) => { tr.on('memory.diagram', centred); } });
    await c.openDiagram(null);
    expect(transport.sentOf('memory.diagram')[0]?.input).toEqual({ workspace_id: WS });
    expect(c.diagramDetail).toBeNull();
    expect(transport.sentOf('memory.thing')).toHaveLength(0);
    c.closeDiagram();
    expect(c.diagramPlace).toBeNull();
    expect(c.tab).toBe('things');
    expect(c.detail).toBeNull();
  });

  it('Happy path — an arrow opens its fact\'s line under the diagram; a change on it reads the diagram again', async () => {
    const { c, transport } = await controller({
      setup: (tr) => {
        tr.on('memory.diagram', centred);
        tr.on('memory.thing', ok(thingDetail(KESTREL, [manager])));
        tr.on('memory.get', ok({ facts: [manager] }));
        tr.on('memory.correct', ok(manager));
      },
    });
    await c.openDiagram(KESTREL);
    await c.openDiagramFact(uid('a'));
    expect(transport.sentOf('memory.get')[0]?.input).toEqual({ ids: [uid('a')], workspace_id: WS });
    expect(c.diagramFact?.id).toBe(uid('a'));
    const before = transport.sentOf('memory.diagram').length;
    await c.correct(manager as never, { thing: { name: 'Ann Lee' } });
    expect(transport.sentOf('memory.diagram').length).toBe(before + 1);
    expect(transport.sentOf('memory.diagram').at(-1)?.input).toEqual({ workspace_id: WS, centre: KESTREL.id });
  });

  it('Edge — the search box finds a thing to centre on', async () => {
    const { c, transport } = await controller({ setup: (tr) => { tr.on('memory.things', ok({ items: [{ ...NORTHWIND, other_names: [], fact_count: 2, link_count: 0 }], next_cursor: null })); } });
    await c.searchDiagram('north');
    expect(transport.sentOf('memory.things')[0]?.input).toEqual({ workspace_id: WS, search: 'north' });
    expect(c.diagramMatches.map((m) => m.name)).toEqual(['Northwind']);
    await c.searchDiagram('  ');
    expect(c.diagramMatches).toEqual([]);
  });

  it('Error — the centred thing was forgotten: "Ordin no longer remembers that." is shown and the diagram is read again without a centre', async () => {
    const { c, transport } = await controller({
      setup: (tr) =>
        tr.on('memory.diagram', (input) =>
          (input as { centre?: string }).centre ? err('MEM_THING_NOT_FOUND', 'Ordin no longer remembers that.') : ok(view({ centre: null })),
        ),
    });
    await c.openDiagram(KESTREL);
    expect(transport.sentOf('memory.diagram').map((e) => e.input)).toEqual([{ workspace_id: WS, centre: KESTREL.id }, { workspace_id: WS }]);
    expect(c.diagramError?.message).toBe('Ordin no longer remembers that.');
    expect(c.diagram?.centre).toBeNull();
    expect(c.diagramDetail).toBeNull();
  });

  it.each(['MEM_STORE_UNAVAILABLE', 'MEM_STORE_TIMEOUT'])('Error — %s draws its sentence with Try again, which reads the same centre again', async (code) => {
    let fail = true;
    const { c, transport } = await controller({
      setup: (tr) => {
        tr.on('memory.diagram', (input) => (fail ? err(code, 'ordin can\'t reach its memory right now. Try again in a moment.') : centred(input)));
        tr.on('memory.thing', ok(thingDetail(KESTREL, [])));
      },
    });
    await c.openDiagram(KESTREL);
    expect(c.diagramError?.code).toBe(code);
    expect(c.diagramError?.action).toBe('try_again');
    fail = false;
    await c.reloadDiagram();
    expect(transport.sentOf('memory.diagram').at(-1)?.input).toEqual({ workspace_id: WS, centre: KESTREL.id });
    expect(c.diagramError).toBeNull();
  });

  it('Error — a backend without memory.diagram hides Show as diagram and closes the screen', async () => {
    const { c } = await controller({ setup: (tr) => { tr.on('memory.diagram', err('SVC_COMMAND_UNKNOWN', 'memory.diagram')); } });
    await c.openDiagram(null);
    expect(c.diagramWired).toBe(false);
    expect(c.diagramPlace).toBeNull();
  });
});

describe('helpers', () => {
  it('a fact with no triple reads as its statement; with one, as its parts', () => {
    expect(tripleLine(fact('x') as never)).toBe('Statement x');
    expect(tripleName((money as unknown as { triple: unknown }).triple as never)).toBe('Kestrel account manager £3,000,000.00');
  });
});
