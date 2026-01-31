// @vitest-environment happy-dom
// Settings › Using your plan, its layout (ScreensModule.md SM-SW-6, amended January 2026; user decision;
// Approved by the user January 2026): a usage card, then the cards How Ordin uses your plan, Ways to use
// less, Limits and More choices; every row holds its title and its control in the one row element, and every
// control is named by its title.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import Plan from '../src/lib/views/Settings/Plan.svelte';
import { PlanModel } from '../src/lib/views/Settings/plan-model.svelte';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const sv = <T>(value: T, set_by = 'person') => ({ set_by, value });
const usage = (over: Record<string, unknown> = {}) => ({
  agents_at_same_time: sv(1), answer_simple_requests_without_model: sv(true), ask_before_estimated_requests: sv('heavy'), choice: sv('balanced'),
  continue_from_summary_after_break: sv(false), daily_pause_requests: { set_by: 'person' }, give_agents_only_needed_parts: sv(true), model_level_per_kind: sv({}),
  reuse_answers: sv(true), show_usage_on_each_run: sv(false), summarise_long_conversations: sv('early'), thinking: sv('automatic'), updated_at: '2026-01-10T10:00:00Z', ...over,
});
const today = { typical_day_permille: 30, model_turns: 4, smaller_share_permille: 500, answered_without_model: 1 };
let mounted: Record<string, unknown> | null = null;
afterEach(() => {
  if (mounted !== null) unmount(mounted);
  mounted = null;
  document.body.innerHTML = '';
});
/** Mounts the section in the page, as the window does, so what is checked is what is drawn. */
function draw(over: Record<string, unknown> = {}): Document {
  const settings = usage(over);
  const handle = { call: async (command: string) => ({ kind: 'ok', result: command === 'usage.today' ? today : settings }) };
  const model = Object.assign(new PlanModel(handle as never), { settings, today });
  const target = document.createElement('div');
  document.body.append(target);
  mounted = mount(Plan, { target, props: { backend: null as never, model } });
  flushSync();
  return document;
}
const words = (el: Element | null | undefined): string => (el?.textContent ?? '').replace(/[\u2068\u2069]/g, '').replace(/\s+/g, ' ').trim();
/** The words of the elements an `aria-labelledby` names. */
const nameOf = (doc: Document, el: Element): string =>
  (el.getAttribute('aria-labelledby') ?? '').split(' ').filter(Boolean).map((id) => words(doc.getElementById(id))).join(' ');

describe('Settings › Using your plan, layout (SM-SW-6)', () => {
  it('draws the usage card first, then the four cards in order', () => {
    const doc = draw();
    const groups = [...doc.querySelectorAll('section.plan > .group')];
    expect(groups).toHaveLength(5);
    expect(groups[0]?.classList.contains('today')).toBe(true);
    expect(words(groups[0])).toContain("Today you've used a small part of a typical day's plan.");
    expect(groups.slice(1).map((g) => words(g.querySelector('h4')))).toEqual(['How Ordin uses your plan', 'Ways to use less', 'Limits', 'More choices']);
  });

  it('Details and More choices are disclosures, closed at first, whose content sits inside their card', () => {
    const doc = draw();
    for (const controls of ['settings-plan-today-details', 'settings-plan-more-rows']) {
      const button = doc.querySelector(`button[aria-controls="${controls}"]`);
      expect(button, controls).not.toBeNull();
      expect(button?.getAttribute('aria-expanded')).toBe('false');
      expect(button?.querySelector('svg.icon')).not.toBeNull();
      const content = doc.getElementById(controls);
      expect((content as HTMLElement | null)?.hidden).toBe(true);
      expect(content?.closest('.group')).toBe(button?.closest('.group'));
      (button as HTMLButtonElement).click();
      flushSync();
      expect(button?.getAttribute('aria-expanded')).toBe('true');
      expect((content as HTMLElement | null)?.hidden).toBe(false);
    }
    expect(nameOf(doc, doc.querySelector('button[aria-controls="settings-plan-more-rows"]')!)).toBe('More choices');
    expect(doc.querySelector('button[aria-controls="settings-plan-today-details"]')?.getAttribute('aria-label')).toBe('Details');
  });

  it('the three choices are one radio group, each a radio, a bold title and a hint, the chosen one marked', () => {
    const doc = draw();
    const radios = [...doc.querySelectorAll('fieldset.choices input[type="radio"]')];
    expect(radios.map((r) => r.getAttribute('value'))).toEqual(['save_my_plan', 'balanced', 'best_results']);
    expect(new Set(radios.map((r) => r.getAttribute('name')))).toEqual(new Set(['plan-choice']));
    for (const radio of radios) {
      const choice = radio.closest('label.choice');
      expect(choice?.firstElementChild).toBe(radio);
      expect(choice?.querySelector('b')).not.toBeNull();
      expect(choice?.querySelector('small')).not.toBeNull();
    }
    expect(nameOf(doc, radios[1]!)).toBe('Balanced (default)');
    expect((doc.querySelector('label.choice[data-choice="balanced"] input') as HTMLInputElement | null)?.checked).toBe(true);
  });

  it('table: each row holds its title and its control in the same row element, the control after the words', () => {
    const doc = draw();
    const cases: Array<[string, string, string]> = [
      ['Ways to use less', 'Answer simple requests without the model', 'input[role="switch"]'],
      ['Ways to use less', 'Reuse an answer when nothing has changed', 'input[role="switch"]'],
      ['Ways to use less', 'Give agents only the parts they need', 'input[role="switch"]'],
      ['Ways to use less', 'After a break of more than an hour, continue from a summary', 'input[role="switch"]'],
      ['Ways to use less', 'Summarise long conversations', 'select'],
      ['Limits', 'Ask before work estimated as', 'select'],
      ['Limits', 'Agents working at the same time', '.stepper'],
      ['More choices', 'Show usage on each run', 'input[role="switch"]'],
      ['More choices', 'How much Ordin thinks first', 'select'],
    ];
    const rows = [...doc.querySelectorAll('.row')];
    expect(rows).toHaveLength(cases.length);
    for (const [card, title, control] of cases) {
      const row = rows.find((r) => words(r.querySelector('.words > b')) === title);
      expect(row, title).toBeDefined();
      expect(words(row?.closest('.group')?.querySelector('h4')), title).toBe(card);
      const el = row?.querySelector(control);
      expect(el, title).not.toBeNull();
      expect(el?.closest('.row'), title).toBe(row);
      expect(row?.lastElementChild, title).toBe(el);
      if (control !== '.stepper') {
        expect(nameOf(doc, el!), title).toBe(title);
        expect(doc.getElementById(el!.getAttribute('aria-describedby') ?? '')?.tagName, title).toBe('SMALL');
      }
    }
  });

  it('every control in the section has a name', () => {
    const doc = draw();
    for (const el of doc.querySelectorAll('section.plan :is(input, select, button, output)')) {
      const name = el.getAttribute('aria-label') ?? nameOf(doc, el);
      expect(name.trim(), el.outerHTML).not.toBe('');
    }
  });

  it('Fewer agents is disabled at 1; More agents is not', () => {
    const doc = draw();
    expect(doc.querySelector('button[aria-label="Fewer agents at the same time"]')?.matches(':disabled')).toBe(true);
    expect(doc.querySelector('button[aria-label="More agents at the same time"]')?.matches(':disabled')).toBe(false);
    expect(words(doc.querySelector('.stepper output'))).toBe('1');
  });
});
