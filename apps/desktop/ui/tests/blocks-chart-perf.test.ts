// C-40: a chart of 5,000 points drawn in at most 300 ms.
//
// Not measurable here as the spec states it: the production renderer is ECharts' canvas renderer inside the
// WebView, and these tests run in Node with no DOM and no canvas. What can be measured without a DOM is the
// whole ECharts pipeline up to its output (option merge, layout, series processing, and drawing into
// ECharts' string renderer), which is the same work minus the pixel fill. That figure is checked here as a
// lower bound; the canvas figure stays UNMEASURED until it is taken in the running window.
import * as echarts from 'echarts/core';
import { BarChart, LineChart, PieChart, ScatterChart } from 'echarts/charts';
import { AriaComponent, GridComponent, LegendComponent, TooltipComponent } from 'echarts/components';
import { SVGRenderer } from 'echarts/renderers';
import { beforeAll, describe, expect, it } from 'vitest';

import { setFormatLocale } from '../src/lib/format/intl';
import { CHART_TOKENS, chartOption, pointCount } from '../src/lib/blocks/chart-option';
import type { ChartBlock } from '../src/lib/service-api/types.gen';

beforeAll(() => {
  setFormatLocale('en-GB');
  echarts.use([BarChart, LineChart, PieChart, ScatterChart, GridComponent, TooltipComponent, LegendComponent, AriaComponent, SVGRenderer]);
});

const colors = CHART_TOKENS.map((_, i) => `#1f5fb${i}`);

function block(chart: string, rows: number, series: number): ChartBlock {
  const names = Array.from({ length: series }, (_, i) => `S${i + 1}`);
  return {
    title: 'Load',
    chart,
    ...(chart === 'bar' ? { orientation: 'vertical', arrangement: 'grouped' } : {}),
    columns: [{ name: 'X', type: chart === 'scatter' ? 'number' : 'text' }, ...names.map((name) => ({ name, type: 'number' }))],
    rows: Array.from({ length: rows }, (_, r) => [chart === 'scatter' ? r : `p${r}`, ...names.map((_n, s) => ((r * 7 + s * 13) % 997) + 0.5)]),
    x: 'X',
    series: names,
    source: { kind: 'earlier_result', run_id: 'r' },
  } as unknown as ChartBlock;
}

function render(b: ChartBlock): { ms: number; svg: string } {
  const started = performance.now();
  const chart = echarts.init(null, null, { renderer: 'svg', ssr: true, width: 900, height: 400 });
  chart.setOption(chartOption(b, colors, { animate: false }));
  const svg = (chart as unknown as { renderToSVGString(): string }).renderToSVGString();
  chart.dispose();
  return { ms: performance.now() - started, svg };
}

describe('C-40 (lower bound, string renderer in Node; canvas figure unmeasured)', () => {
  // 5,000 points = rows x series, within A-34 (at most 8 series).
  const cases: Array<[string, ChartBlock]> = [
    ['bar', block('bar', 625, 8)],
    ['line', block('line', 5000, 1)],
    ['area', block('area', 1250, 4)],
    ['scatter', block('scatter', 2500, 2)],
  ];
  for (const [name, b] of cases) {
    it(`${name}: ${pointCount(b)} points`, () => {
      expect(pointCount(b)).toBeLessThanOrEqual(5000);
      render(b); // warm-up: module initialisation is not drawing
      const best = Math.min(...[1, 2, 3, 4, 5].map(() => render(b).ms));
      expect(render(b).svg.length).toBeGreaterThan(1000);
      expect(best, `${name} best of 5`).toBeLessThan(300);
    });
  }

  it('pie: at most 6 slices', () => {
    const b = block('pie', 6, 1);
    expect(render(b).svg).toContain('<svg');
  });
});
