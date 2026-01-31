// Maps a `chart` block to an ECharts option (SA-UI-03; ScreensModule step 8). A plain object, no DOM and no
// ECharts import, so the mapping is tested in Node. Colours come only from the theme's chart palette
// (`--chart-1` … `--chart-8`); values are formatted by `lib/format/intl.ts`; there is no toolbox, no data
// zoom and no rich-text formatter.
import { formatNumber } from '../format/intl';
import type { Cell, ChartBlock, TableColumn } from '../service-api/types.gen';
import { cellText } from './table-model';

export type ChartOption = Record<string, unknown>;

/** The palette tokens, in order. */
export const CHART_TOKENS = ['--chart-1', '--chart-2', '--chart-3', '--chart-4', '--chart-5', '--chart-6', '--chart-7', '--chart-8'] as const;

/** A-34: points drawn = rows × series. */
export function pointCount(block: Pick<ChartBlock, 'rows' | 'series'>): number {
  return block.rows.length * block.series.length;
}

const num = (cell: Cell | undefined): number | null => (typeof cell === 'number' && Number.isFinite(cell) ? cell : null);
const text = (column: TableColumn | undefined, cell: Cell | undefined): string => (column === undefined ? '' : cellText(column, cell ?? null));

export function chartOption(block: ChartBlock, colors: readonly string[], options: { animate?: boolean } = {}): ChartOption {
  const columnIndex = (name: string): number => block.columns.findIndex((c) => c.name === name);
  const xi = columnIndex(block.x);
  const xColumn = block.columns[xi];
  const series = block.series.map((name) => ({ name, index: columnIndex(name) })).filter((s) => s.index >= 0);
  const firstSeriesColumn = block.columns[series[0]?.index ?? -1];
  const valueLabel = (value: unknown): string => {
    if (typeof value !== 'number') return String(value ?? '');
    return firstSeriesColumn === undefined ? formatNumber(value) : cellText(firstSeriesColumn, value);
  };
  const base: ChartOption = {
    color: [...colors],
    animation: options.animate ?? true,
    aria: { enabled: true, label: { description: block.title } },
    legend: series.length > 1 || block.chart === 'pie' ? { type: 'scroll' } : { show: false },
    tooltip: { trigger: block.chart === 'pie' || block.chart === 'scatter' ? 'item' : 'axis', confine: true, valueFormatter: valueLabel },
  };

  if (block.chart === 'pie') {
    const first = series[0];
    return {
      ...base,
      series: [
        {
          type: 'pie',
          name: first?.name ?? '',
          radius: '65%',
          data: block.rows.map((row) => ({ name: text(xColumn, row[xi]), value: first === undefined ? null : num(row[first.index]) })),
        },
      ],
    };
  }

  const category = { type: 'category', name: block.x_title ?? undefined, data: block.rows.map((row) => text(xColumn, row[xi])) };
  const value = { type: 'value', name: block.y_title ?? undefined, axisLabel: { formatter: (v: number) => valueLabel(v) } };

  if (block.chart === 'scatter') {
    return {
      ...base,
      xAxis: { type: 'value', name: block.x_title ?? undefined, axisLabel: { formatter: (v: number) => formatNumber(v) } },
      yAxis: value,
      series: series.map((s) => ({ type: 'scatter', name: s.name, data: block.rows.map((row) => [num(row[xi]), num(row[s.index])]) })),
    };
  }

  const horizontal = block.chart === 'bar' && block.orientation === 'horizontal';
  const stacked = block.chart === 'bar' && block.arrangement === 'stacked';
  return {
    ...base,
    xAxis: horizontal ? value : category,
    yAxis: horizontal ? category : value,
    series: series.map((s) => ({
      type: block.chart === 'bar' ? 'bar' : 'line',
      name: s.name,
      ...(stacked ? { stack: 'total' } : {}),
      ...(block.chart === 'area' ? { areaStyle: {} } : {}),
      data: block.rows.map((row) => num(row[s.index])),
    })),
  };
}

// spec: ordin-desktop/components/ScreensModule#SA-UI-03
/**
 * The chart's text and lines in the theme's colours (cross-cutting/VisualDesign.md; the prototype's
 * `svg.chart`): labels muted, axis labels and the legend in the secondary text colour, grid lines in the
 * divider colour and the axis in the strong divider colour. `read` returns a custom property's value in the
 * theme in force, so a chart drawn again after a theme change takes the new theme's colours.
 */
export function themeChartOption(option: ChartOption, read: (property: string) => string): ChartOption {
  const muted = read('--color-text-muted');
  const secondary = read('--color-text-secondary');
  const axis = (value: unknown): unknown =>
    value === undefined || value === null || typeof value !== 'object'
      ? value
      : {
          ...(value as Record<string, unknown>),
          axisLabel: { ...((value as { axisLabel?: object }).axisLabel ?? {}), color: secondary },
          nameTextStyle: { color: muted },
          axisLine: { lineStyle: { color: read('--color-line-strong') } },
          splitLine: { lineStyle: { color: read('--color-line') } },
        };
  const o = option as Record<string, unknown>;
  return {
    ...o,
    textStyle: { color: muted, fontFamily: read('--font-body') },
    legend: { ...((o['legend'] as object | undefined) ?? {}), textStyle: { color: secondary } },
    xAxis: axis(o['xAxis']),
    yAxis: axis(o['yAxis']),
  } as ChartOption;
}
