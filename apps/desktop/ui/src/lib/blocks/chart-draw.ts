// Drawing a chart into an element, and the failure rule: when ECharts throws, the block's data is drawn as a
// table instead and `UI_CHART_RENDER_FAILED` is recorded (ScreensModule step 8, Error Table). ECharts is
// loaded on first use so it stays out of the initial JavaScript (≤ 600 KB gzipped).
//
// spec: ordin-desktop/components/ScreensModule#SA-UI-03
import type { DiagnosticSink } from '../backend/backend';
import type { ChartOption } from './chart-option';

/** What the drawing needs from the chart library; ECharts' `init` result has these members. */
export interface ChartInstance {
  setOption(option: ChartOption, notMerge?: boolean): void;
  resize(): void;
  dispose(): void;
}
export type ChartLoader = (element: HTMLElement) => Promise<ChartInstance>;

/** Loads ECharts' canvas renderer with only the chart types and components the six kinds use. */
export const loadEcharts: ChartLoader = async (element) => {
  const [core, charts, components, renderers] = await Promise.all([
    import('echarts/core'),
    import('echarts/charts'),
    import('echarts/components'),
    import('echarts/renderers'),
  ]);
  core.use([
    charts.BarChart, charts.LineChart, charts.PieChart, charts.ScatterChart,
    components.GridComponent, components.TooltipComponent, components.LegendComponent, components.AriaComponent,
    renderers.CanvasRenderer,
  ]);
  return core.init(element, undefined, { renderer: 'canvas' }) as unknown as ChartInstance;
};

export type DrawResult = { kind: 'drawn'; instance: ChartInstance } | { kind: 'failed' };

export async function drawChart(
  element: HTMLElement,
  option: ChartOption,
  diagnostics: DiagnosticSink | undefined,
  load: ChartLoader = loadEcharts,
): Promise<DrawResult> {
  try {
    const instance = await load(element);
    try {
      instance.setOption(option, true);
    } catch (error) {
      instance.dispose();
      throw error;
    }
    return { kind: 'drawn', instance };
  } catch {
    diagnostics?.record('UI_CHART_RENDER_FAILED');
    return { kind: 'failed' };
  }
}
