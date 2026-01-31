// The blocks module without a DOM: Markdown as a tree, line diffs, table arrangement (10,000 rows), the
// ECharts option, the chart failure rule, the Slack permalink rule, the folding of a long message.
import { beforeAll, describe, expect, it } from 'vitest';

import { setFormatLocale } from '../src/lib/format/intl';
import { drawChart, type ChartInstance } from '../src/lib/blocks/chart-draw';
import { CHART_TOKENS, chartOption, pointCount } from '../src/lib/blocks/chart-option';
import { diffLines, splitLines } from '../src/lib/blocks/diff';
import { parseInline, parseMarkdown } from '../src/lib/blocks/markdown';
import { arrangeRows, cellText, visibleWindow, WINDOW_MARGIN } from '../src/lib/blocks/table-model';
import { isSlackPermalink } from '../src/lib/blocks/slack-link';
import { foldMessage } from '../src/lib/approval/message-fold';
import type { ChartBlock, TableColumn } from '../src/lib/service-api/types.gen';
import { Diagnostics } from '../src/lib/store/diagnostics.svelte';

beforeAll(() => setFormatLocale('en-GB'));

describe('markdown: text never becomes markup', () => {
  it('raw HTML in the source is text in a paragraph', () => {
    const tree = parseMarkdown('<img src=x onerror=alert(1)>');
    expect(tree).toEqual([{ kind: 'paragraph', inlines: [{ kind: 'text', text: '<img src=x onerror=alert(1)>' }] }]);
  });

  it('a link keeps its words and its target as text, never as an element', () => {
    expect(parseInline('see [the brief](https://example.com/x) now')).toEqual([
      { kind: 'text', text: 'see ' },
      { kind: 'link', text: 'the brief', target: 'https://example.com/x' },
      { kind: 'text', text: ' now' },
    ]);
  });

  it('headings, lists, quotes, code fences and rules', () => {
    const tree = parseMarkdown('# Title\n\nSome **bold** and *soft* and `code`.\n\n- one\n- two\n\n1. first\n\n> quoted\n\n```\nlet a = 1;\n```\n\n---');
    expect(tree.map((b) => b.kind)).toEqual(['heading', 'paragraph', 'list', 'list', 'quote', 'code', 'rule']);
    expect(tree[2]).toMatchObject({ kind: 'list', ordered: false });
    expect(tree[3]).toMatchObject({ kind: 'list', ordered: true });
    expect(tree[5]).toEqual({ kind: 'code', text: 'let a = 1;' });
  });
});

describe('diff', () => {
  it('aligns changed lines in two columns', () => {
    const rows = diffLines(splitLines('a\nb\nc\n'), splitLines('a\nB\nc\nd\n'));
    expect(rows.map((r) => [r.left?.kind ?? null, r.right?.kind ?? null])).toEqual([
      ['same', 'same'],
      ['removed', 'added'],
      ['same', 'same'],
      [null, 'added'],
    ]);
    expect(rows[1]).toMatchObject({ left: { text: 'b' }, right: { text: 'B' } });
  });

  it('identical text has no changed row, and empty text has no rows', () => {
    expect(diffLines(['x'], ['x']).every((r) => r.left?.kind === 'same')).toBe(true);
    expect(diffLines([], [])).toEqual([]);
  });
});

describe('table arrangement (A-34: 10,000 rows by 30 columns)', () => {
  const columns: TableColumn[] = [
    { name: 'Supplier', type: 'text' },
    { name: 'Rate', type: 'money', currency: 'GBP' },
    { name: 'Change', type: 'percentage' },
    { name: 'Day', type: 'date' },
  ];

  it('typed cells: money, percentage, date, and a string starting with = is text', () => {
    expect(cellText(columns[1] as TableColumn, 412.5)).toBe('£412.50');
    expect(cellText(columns[2] as TableColumn, 0.08)).toBe('8%');
    expect(cellText(columns[3] as TableColumn, '2026-01-10')).toMatch(/10/);
    expect(cellText(columns[0] as TableColumn, '=SUM(A1:A9)')).toBe('=SUM(A1:A9)');
    expect(cellText(columns[1] as TableColumn, null)).toBe('');
  });

  it('sorts by a column, empty cells last, and searches every word across cells', () => {
    const rows = [
      ['Kestrel', 412.5, 0.08, '2026-01-10'],
      ['Acme', 398, -0.02, '2026-01-09'],
      ['Northwind', null, 0.01, '2026-01-11'],
    ];
    expect(arrangeRows(columns, rows, '', { column: 1, direction: 'ascending' })).toEqual([1, 0, 2]);
    expect(arrangeRows(columns, rows, '', { column: 1, direction: 'descending' })).toEqual([0, 1, 2]);
    expect(arrangeRows(columns, rows, 'acme zzz', null)).toEqual([]);
    expect(arrangeRows(columns, rows, 'north', null)).toEqual([2]);
  });

  it('sorting or searching 10,000 rows takes at most 200 ms, and only the visible rows (± 50) are in the document', () => {
    const wide: TableColumn[] = Array.from({ length: 30 }, (_, i) => (i === 0 ? { name: 'c0', type: 'text' } : { name: `c${i}`, type: 'number' }));
    const rows = Array.from({ length: 10_000 }, (_, r) => wide.map((c, i) => (c.type === 'text' ? `row ${r}` : (r * 31 + i) % 977)));
    // Wall-clock time depends on what else the machine is doing (other test files run beside this one), so
    // the figure is the process's own CPU time, the fastest of five runs (each builds its own table).
    const proc = (globalThis as unknown as { process: { cpuUsage(previous?: { user: number; system: number }): { user: number; system: number } } }).process;
    const runs: number[] = [];
    let sorted: number[] = [];
    let found: number[] = [];
    for (let run = 0; run < 5; run++) {
      const fresh = rows.map((r) => [...r]);
      const before = proc.cpuUsage();
      sorted = arrangeRows(wide, fresh, '', { column: 5, direction: 'descending' });
      found = arrangeRows(wide, fresh, 'row 99', null);
      const used = proc.cpuUsage(before);
      runs.push((used.user + used.system) / 1000);
    }
    expect(sorted).toHaveLength(10_000);
    expect(found.length).toBeGreaterThan(0);
    expect(Math.min(...runs), `CPU ms per run: ${runs.map((r) => r.toFixed(0)).join(', ')}`).toBeLessThan(200);
    const span = visibleWindow(10_000, 5_000 * 28, 420, 28);
    expect(span.end - span.start).toBeLessThanOrEqual(15 + 2 * WINDOW_MARGIN);
    expect(span.start).toBe(5_000 - WINDOW_MARGIN);
  });
});

describe('chart option (SA-UI-03)', () => {
  const base = {
    title: 'Spend by supplier',
    columns: [
      { name: 'Supplier', type: 'text' },
      { name: 'Q2', type: 'money', currency: 'GBP' },
      { name: 'Q3', type: 'money', currency: 'GBP' },
    ],
    rows: [
      ['Kestrel', 382000, 412000],
      ['Acme', 401000, 398000],
    ],
    x: 'Supplier',
    series: ['Q2', 'Q3'],
    y_title: 'Spend',
    source: { kind: 'worked_out', by: 'Supplier analyst', from: [] },
  } as const;
  const colors = CHART_TOKENS.map((_, i) => `#00000${i}`);
  const make = (extra: object): ChartBlock => ({ ...base, ...extra }) as unknown as ChartBlock;
  const seriesOf = (o: Record<string, unknown>) => o.series as Array<Record<string, unknown>>;

  it('bar: vertical grouped has no stack; stacked stacks; horizontal swaps the axes', () => {
    const grouped = chartOption(make({ chart: 'bar', orientation: 'vertical', arrangement: 'grouped' }), colors);
    expect(seriesOf(grouped).map((s) => s.type)).toEqual(['bar', 'bar']);
    expect(seriesOf(grouped)[0]).not.toHaveProperty('stack');
    const stacked = chartOption(make({ chart: 'bar', orientation: 'vertical', arrangement: 'stacked' }), colors);
    expect(seriesOf(stacked)[0]).toHaveProperty('stack', 'total');
    const horizontal = chartOption(make({ chart: 'bar', orientation: 'horizontal', arrangement: 'grouped' }), colors);
    expect((horizontal.yAxis as { type: string }).type).toBe('category');
    expect((horizontal.xAxis as { type: string }).type).toBe('value');
  });

  it('line, area (line with an area style), scatter (numeric x) and pie (one series)', () => {
    expect(seriesOf(chartOption(make({ chart: 'line' }), colors))[0]).toMatchObject({ type: 'line' });
    expect(seriesOf(chartOption(make({ chart: 'area' }), colors))[0]).toMatchObject({ type: 'line', areaStyle: {} });
    const scatter = chartOption(make({ chart: 'scatter', columns: [{ name: 'Supplier', type: 'number' }, base.columns[1], base.columns[2]], rows: [[1, 2, 3]] }), colors);
    expect(seriesOf(scatter)[0]).toMatchObject({ type: 'scatter', data: [[1, 2]] });
    const pie = chartOption(make({ chart: 'pie', series: ['Q3'] }), colors);
    expect(seriesOf(pie)).toHaveLength(1);
    expect(seriesOf(pie)[0]).toMatchObject({ type: 'pie', data: [{ name: 'Kestrel', value: 412000 }, { name: 'Acme', value: 398000 }] });
  });

  it('colours only from the chart palette, aria on with the title, no toolbox, no data zoom, no rich-text formatter', () => {
    const option = chartOption(make({ chart: 'line' }), colors);
    expect(option.color).toEqual(colors);
    expect(option.aria).toEqual({ enabled: true, label: { description: 'Spend by supplier' } });
    for (const key of ['toolbox', 'dataZoom', 'graphic', 'title']) expect(option).not.toHaveProperty(key);
    expect(JSON.stringify(option, (_k, v) => (typeof v === 'function' ? 'fn' : v))).not.toMatch(/rich|\{@/);
    expect(CHART_TOKENS).toEqual(['--chart-1', '--chart-2', '--chart-3', '--chart-4', '--chart-5', '--chart-6', '--chart-7', '--chart-8']);
  });

  it('axis values use the active locale and the series column\'s type', () => {
    const option = chartOption(make({ chart: 'line' }), colors);
    const format = (option.yAxis as { axisLabel: { formatter: (v: number) => string } }).axisLabel.formatter;
    expect(format(412000)).toBe('£412,000.00');
    expect(pointCount({ rows: [[], []], series: ['Q2', 'Q3'] })).toBe(4);
  });
});

describe('Error path — chart fails to render', () => {
  const element = {} as HTMLElement;
  it('the library throwing while drawing records UI_CHART_RENDER_FAILED and reports failed so the table is drawn', async () => {
    const diagnostics = new Diagnostics();
    const throwing: ChartInstance = { setOption: () => { throw new Error('boom'); }, resize: () => undefined, dispose: () => undefined };
    expect(await drawChart(element, {}, diagnostics, async () => throwing)).toEqual({ kind: 'failed' });
    expect(diagnostics.codes()).toEqual(['UI_CHART_RENDER_FAILED']);
  });

  it('the library failing to load fails the same way, and a clean draw records nothing', async () => {
    const diagnostics = new Diagnostics();
    expect(await drawChart(element, {}, diagnostics, async () => { throw new Error('no library'); })).toEqual({ kind: 'failed' });
    const fine: ChartInstance = { setOption: () => undefined, resize: () => undefined, dispose: () => undefined };
    expect((await drawChart(element, {}, diagnostics, async () => fine)).kind).toBe('drawn');
    expect(diagnostics.codes()).toEqual(['UI_CHART_RENDER_FAILED']);
  });
});

describe('Error Table — a Slack permalink from the core that is not https://*.slack.com/archives/…', () => {
  it('only an https slack.com archives link is a permalink (UI_LINK_REFUSED otherwise)', () => {
    expect(isSlackPermalink('https://northwind.slack.com/archives/C123/p1700000000')).toBe(true);
    for (const bad of ['http://northwind.slack.com/archives/C1/p1', 'https://evil.example.com/archives/C1', 'https://northwind.slack.com.evil.example/archives/C1', 'https://northwind.slack.com/team/U1', 'javascript:alert(1)', '']) {
      expect(isSlackPermalink(bad), bad).toBe(false);
    }
  });
});

describe('a long message on an approval card', () => {
  const lines = (n: number) => Array.from({ length: n }, (_, i) => `line ${i + 1}`).join('\n');
  it('12 lines or fewer are shown whole; more are folded to the first 6 until opened', () => {
    expect(foldMessage(lines(12), false)).toEqual({ text: lines(12), folded: false });
    expect(foldMessage(lines(13), false)).toEqual({ text: lines(6), folded: true });
    expect(foldMessage(lines(13), true)).toEqual({ text: lines(13), folded: false });
  });
});
