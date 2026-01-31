// @vitest-environment happy-dom
// A fact line, short first (ScreensModule.md SM-SF-13 and SM-SF-14, amended January 2026): collapsed, the
// word and the value, then the subject; **Details** expands the evidence line, the conflict line and the
// actions; a conflict keeps a small mark while collapsed.
import { createRawSnippet, flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import FactLine from '../src/lib/views/fixed/FactLine.svelte';
import { AgentNames } from '../src/lib/views/fixed/facts';
import { thingId, uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const STRATEGY = { id: thingId('strategy'), name: 'Manchester Digital Strategy 2021–2026' };
const fact = (id: string, over: Record<string, unknown> = {}) => ({
  id: uid(id), statement: `Statement ${id}`, kind: 'known', true_from: '2026-11-18T00:00:00Z', written_at: '2026-11-18T00:00:00Z',
  came_from: { kind: 'person' }, fading: false, may_be_out_of_date: false, needs_check: false,
  triple: { subject: STRATEGY, predicate: { key: 'partnersConsulted', label: 'partners consulted' }, object: { kind: 'number', value: '63' } },
  ...over,
});
const words = (node: Element | null | undefined): string => (node?.textContent ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const button = (root: Element, name: string): HTMLButtonElement | undefined =>
  // The accessible name: an icon button's aria-label (amended January 2026), else its words.
  Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find((b) => (b.getAttribute('aria-label') ?? words(b)) === name);

let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
afterEach(() => {
  if (drawn) {
    unmount(drawn.part);
    drawn.target.remove();
    drawn = null;
  }
});

function line(props: Record<string, unknown>): HTMLElement {
  const target = document.createElement('ul');
  document.body.appendChild(target);
  const part = mount(FactLine, { target, props: { names: new AgentNames(), ...props } as never });
  drawn = { target, part };
  flushSync();
  return target.querySelector<HTMLElement>('.fact-line')!;
}

describe('SM-SF-13 — a fact line, short first', () => {
  it('Happy path — collapsed: "<subject> · partners consulted · 63" on one line, the subject a link, and nothing else but Details', () => {
    const opened = vi.fn();
    const li = line({ fact: fact('a'), onopenthing: opened, oncorrect: () => undefined, onforget: () => undefined, onhistory: () => undefined });
    expect(words(li.querySelector('.claim'))).toBe(`${STRATEGY.name} · partners consulted · 63`);
    const subject = li.querySelector('.subject button.link');
    expect(words(subject)).toBe(STRATEGY.name);
    expect(li.querySelector('.secondary')).toBeNull();
    expect(words(li)).not.toContain('True since');
    for (const name of ['Correct', 'History', 'Forget']) expect(button(li, name)).toBeUndefined();
    expect(button(li, 'Details')?.getAttribute('aria-expanded')).toBe('false');
    // The accessible name is the three parts in reading order, subject first.
    expect(li.getAttribute('aria-label')?.replace(/[⁨⁩]/g, '')).toBe(`${STRATEGY.name} partners consulted 63`);
    (subject as HTMLButtonElement).click();
    expect(opened).toHaveBeenCalledWith(STRATEGY.id);
  });

  it('Happy path — Details expands "True since 18 Nov 2026", the source, kind and scope, and Correct, History and Forget; Hide details closes it', () => {
    const li = line({ fact: fact('a'), oncorrect: () => undefined, onforget: () => undefined, onhistory: () => undefined });
    button(li, 'Details')!.click();
    flushSync();
    const toggle = button(li, 'Hide details')!;
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    const evidence = li.querySelector('.secondary')!;
    expect(toggle.getAttribute('aria-controls')).toBe(evidence.id);
    expect(words(evidence)).toMatch(/^True since 18 Nov 2026 · /);
    expect(words(evidence)).toContain('Something known');
    for (const name of ['Correct', 'History', 'Forget']) expect(button(li, name)).toBeDefined();
    toggle.click();
    flushSync();
    expect(li.querySelector('.secondary')).toBeNull();
    expect(button(li, 'Details')?.getAttribute('aria-expanded')).toBe('false');
  });

  it('Keyboard — Details is a real button reached with Tab; Enter expands, and a fading fact then offers Keep', () => {
    const li = line({ fact: fact('f', { kind: 'happened', fading: true, not_used_since: '2026-06-01T00:00:00Z' }), onkeep: () => undefined });
    const toggle = button(li, 'Details')!;
    expect(toggle.tagName).toBe('BUTTON');
    expect(toggle.tabIndex).toBe(0);
    expect(button(li, 'Keep')).toBeUndefined();
    toggle.focus();
    // A native button turns Enter and Space into a click.
    toggle.click();
    flushSync();
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(button(li, 'Keep')).toBeDefined();
  });

  it('Edge — collapsing closes an open correction unsaved', () => {
    const correct = vi.fn();
    const li = line({ fact: fact('a'), oncorrect: correct });
    button(li, 'Details')!.click();
    flushSync();
    button(li, 'Correct')!.click();
    flushSync();
    expect(li.querySelector('fieldset.edit')).not.toBeNull();
    button(li, 'Hide details')!.click();
    flushSync();
    expect(li.querySelector('fieldset.edit')).toBeNull();
    expect(correct).not.toHaveBeenCalled();
  });
});

describe('SM-SF-14 — a conflict stays visible when collapsed', () => {
  const other = fact('b', { triple: { subject: STRATEGY, predicate: { key: 'partnersConsulted', label: 'partners consulted' }, object: { kind: 'number', value: '58' } } });

  it('Edge — collapsed, a fact in conflict keeps a small mark; expanded, the full conflict line and Keep this one', () => {
    const li = line({ fact: fact('a'), conflicting: [{ fact: other, here: true }], onkeepside: () => undefined });
    expect(words(li.querySelector('.conflict-mark'))).toBe('Conflicts with another fact');
    expect(li.querySelector('.conflict')).toBeNull();
    button(li, 'Details')!.click();
    flushSync();
    expect(li.querySelector('.conflict-mark')).toBeNull();
    expect(words(li.querySelector('.conflict'))).toContain('58');
    expect(button(li, 'Keep this one')).toBeDefined();
  });

  it('a fact with no conflict has no mark', () => {
    expect(line({ fact: fact('a') }).querySelector('.conflict-mark')).toBeNull();
  });
});

// SM-SF-13 (amended January 2026; user decision; Approved by the user January 2026): the line's secondary
// actions are small icon buttons with tooltips, and History opens inside the fact's own line.
describe('SM-SF-13 — icon actions and History in place', () => {
  const all = { oncorrect: () => undefined, onforget: () => undefined, onhistory: () => undefined, oneverywhere: () => undefined, onkeepchecked: () => undefined };

  it('Happy path — Details, Correct, History, Keep it, Remember everywhere and Forget are icon buttons, each named with a matching tooltip', () => {
    const li = line({ fact: fact('a', { workspace_id: uid('ws'), needs_check: true }), ...all });
    const details = button(li, 'Details')!;
    expect(details.title).toBe('Details');
    expect(words(details)).toBe('');
    expect(details.querySelector('svg.icon')).not.toBeNull();
    details.click();
    flushSync();
    expect(button(li, 'Hide details')!.title).toBe('Hide details');
    for (const name of ['Correct', 'History', 'Keep it', 'Remember everywhere', 'Forget']) {
      const b = button(li, name);
      expect(b, name).toBeDefined();
      expect(b!.title).toBe(name);
      expect(words(b)).toBe('');
      expect(b!.querySelector('svg.icon')).not.toBeNull();
    }
  });

  it('Happy path — History pressed calls back, and the history drawn while open sits inside the line, after the actions', () => {
    const history = vi.fn();
    const snippet = createRawSnippet(() => ({ render: () => '<p class="the-history">How this fact changed</p>' }));
    const li = line({ fact: fact('a'), onhistory: history, history: snippet, historyOpen: true });
    // Collapsed, the history is not drawn even while open.
    expect(li.querySelector('.the-history')).toBeNull();
    button(li, 'Details')!.click();
    flushSync();
    const drawnHistory = li.querySelector('.fact-history .the-history');
    expect(drawnHistory).not.toBeNull();
    expect(li.contains(drawnHistory)).toBe(true);
    expect(button(li, 'History')!.getAttribute('aria-expanded')).toBe('true');
    button(li, 'History')!.click();
    expect(history).toHaveBeenCalledTimes(1);
  });

  it('Edge — while closed, History is not expanded and nothing of it is drawn', () => {
    const snippet = createRawSnippet(() => ({ render: () => '<p class="the-history">x</p>' }));
    const li = line({ fact: fact('a'), onhistory: () => undefined, history: snippet, historyOpen: false, expanded: true });
    expect(button(li, 'History')!.getAttribute('aria-expanded')).toBe('false');
    expect(li.querySelector('.fact-history')).toBeNull();
  });

  it('Happy path — while correcting, Save and Cancel are icon buttons named and titled by their words', () => {
    const li = line({ fact: fact('a'), oncorrect: () => undefined, expanded: true });
    button(li, 'Correct')!.click();
    flushSync();
    for (const [name, icon] of [['Save', 'check'], ['Cancel', 'remove']] as const) {
      const b = button(li, name)!;
      expect(b).toBeDefined();
      expect(b.getAttribute('aria-label')).toBe(name);
      expect(b.title).toBe(name);
      expect(words(b)).toBe('');
      expect(b.classList.contains('icon-btn')).toBe(true);
      expect(b.querySelector('use')?.getAttribute('href')).toBe(`#i-${icon}`);
    }
  });

  it('Edge — Save is off until the value changes; Cancel closes the correction unsaved', () => {
    const saved: unknown[] = [];
    const li = line({ fact: fact('a'), oncorrect: (_f: unknown, o: unknown) => void saved.push(o), expanded: true });
    button(li, 'Correct')!.click();
    flushSync();
    expect(button(li, 'Save')!.disabled).toBe(true);
    button(li, 'Cancel')!.click();
    flushSync();
    expect(button(li, 'Save')).toBeUndefined();
    expect(button(li, 'Correct')).toBeDefined();
    expect(saved).toEqual([]);
  });
});
