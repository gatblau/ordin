// SM-SF-2: the problems line of a coding task never reads "No new problems" for a task that was not checked.
import { beforeAll, describe, expect, it } from 'vitest';

import { problemsLine } from '../src/lib/views/engineering/problems';
import ProblemsLine from '../src/lib/views/engineering/ProblemsLine.svelte';
import { html, loadWindowCatalogue } from './support/ssr';
import { text } from './fixed-support';

beforeAll(loadWindowCatalogue);

const problem = (n: number) => ({ file: `src/billing${n}.rs`, line: n, message: `Unused value ${n}`, severity: 'warning' });
const review = (count: number, unchecked: boolean | undefined, problems = 0) =>
  ({
    task: { new_problems: count, ...(unchecked === undefined ? {} : { problems_unchecked: unchecked }) },
    new_problems: Array.from({ length: problems }, (_, i) => problem(i + 1)),
  }) as never;

describe('SM-SF-2 — the four rows of the problems line', () => {
  it.each([
    { unchecked: false, count: 0, row: 'none', icon: 'tick', ids: ['changes-problems-none'] },
    { unchecked: false, count: 2, row: 'count', icon: 'warning', ids: ['changes-problems-count'] },
    { unchecked: true, count: 0, row: 'unchecked', icon: 'question', ids: ['changes-problems-unchecked'] },
    { unchecked: true, count: 2, row: 'count_and_unchecked', icon: 'warning', ids: ['changes-problems-count', 'changes-problems-unchecked-also'] },
  ])('problems_unchecked $unchecked with $count new problems reads $row', ({ unchecked, count, row, icon, ids }) => {
    const line = problemsLine(review(count, unchecked, count));
    expect(line.row).toBe(row);
    expect(line.icon).toBe(icon);
    expect(line.messageIds).toEqual(ids);
  });

  it('problems_unchecked absent is read as false', () => {
    expect(problemsLine(review(0, undefined)).row).toBe('none');
    expect(problemsLine(review(1, undefined, 1)).row).toBe('count');
  });

  it('the words "No new problems" belong to the first row only', () => {
    for (const [count, unchecked] of [[0, true], [3, false], [3, true]] as const) {
      expect(text(html(ProblemsLine, { review: review(count, unchecked, count) }))).not.toContain('No new problems');
    }
    expect(text(html(ProblemsLine, { review: review(0, false) }))).toContain('No new problems');
  });
});

describe('SM-SF-2 — drawn', () => {
  it('Security — an unchecked task never reads as clean', () => {
    const out = text(html(ProblemsLine, { review: review(0, true) }));
    expect(out).toContain('Not checked for new problems: some changed files had no code checking.');
    expect(out).not.toContain('No new problems');
  });

  it('Happy path — a checked task with no problems', () => {
    expect(text(html(ProblemsLine, { review: review(0, false) }))).toContain('No new problems');
  });

  it('Edge case — problems found and some files unchecked', () => {
    const out = text(html(ProblemsLine, { review: review(2, true, 2) }));
    expect(out).toContain('2 new problems');
    expect(out).toContain('src/billing1.rs:1 Unused value 1');
    expect(out).toContain('src/billing2.rs:2 Unused value 2');
    expect(out).toContain('Some changed files had no code checking, so there may be more.');
    expect(out.indexOf('Unused value 2')).toBeLessThan(out.indexOf('so there may be more'));
  });

  it('one problem reads in the singular and every icon carries a text label', () => {
    const out = html(ProblemsLine, { review: review(1, false, 1) });
    expect(text(out)).toContain('1 new problem');
    expect(text(out)).not.toContain('1 new problems');
    expect(out).toMatch(/aria-hidden="true">⚠</);
  });

  it('a problem message holding markup is drawn literally', () => {
    const r = { task: { new_problems: 1 }, new_problems: [{ file: 'a.rs', line: 1, message: '<img src=x onerror=alert(1)>', severity: 'error' }] } as never;
    const out = html(ProblemsLine, { review: r });
    expect(out).not.toContain('<img');
    expect(out).toContain('&lt;img src=x onerror=alert(1)>');
  });
});
