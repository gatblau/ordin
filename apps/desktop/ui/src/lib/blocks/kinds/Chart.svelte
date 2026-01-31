<script lang="ts">
  // `chart`: bar (vertical or horizontal, grouped or stacked), line, area, scatter or pie, drawn by ECharts
  // (SA-UI-03) on a canvas; **Show as table** draws exactly `columns` and `rows`. If the library throws, the
  // table is drawn in its place and `UI_CHART_RENDER_FAILED` is recorded.
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import { getApp } from '../../shell/context';
  import AskButton from '../AskButton.svelte';
  import SourceLine from '../SourceLine.svelte';
  import { drawChart, type ChartInstance } from '../chart-draw';
  import { CHART_TOKENS, chartOption, themeChartOption } from '../chart-option';
  import TableView from '../TableView.svelte';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'chart'> = $props();
  const app = getApp();

  let element = $state<HTMLElement | null>(null);
  let asTable = $state(false);
  let failed = $state(false);
  let instance: ChartInstance | null = null;

  const palette = (): string[] => {
    const style = getComputedStyle(document.documentElement);
    return CHART_TOKENS.map((token) => style.getPropertyValue(token).trim());
  };
  const reducedMotion = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;
  const option = (): ReturnType<typeof chartOption> => {
    const style = getComputedStyle(document.documentElement);
    return themeChartOption(chartOption(block, palette(), { animate: !reducedMotion() }), (name) => style.getPropertyValue(name).trim());
  };

  onMount(() => {
    let disposed = false;
    const cleanups: Array<() => void> = [];
    void (async () => {
      if (element === null) return;
      const result = await drawChart(element, option(), app.diagnostics);
      if (disposed) {
        if (result.kind === 'drawn') result.instance.dispose();
        return;
      }
      if (result.kind === 'failed') {
        failed = true;
        return;
      }
      instance = result.instance;
      // Re-render on a theme change, and resize with the container.
      const redraw = (): void => {
        try {
          instance?.setOption(option(), true);
        } catch {
          app.diagnostics.record('UI_CHART_RENDER_FAILED');
          failed = true;
        }
      };
      const scheme = matchMedia('(prefers-color-scheme: dark)');
      scheme.addEventListener('change', redraw);
      const attributes = new MutationObserver(redraw);
      attributes.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'style'] });
      const size = new ResizeObserver(() => instance?.resize());
      size.observe(element);
      cleanups.push(() => scheme.removeEventListener('change', redraw), () => attributes.disconnect(), () => size.disconnect());
    })();
    return () => {
      disposed = true;
      for (const stop of cleanups) stop();
      instance?.dispose();
      instance = null;
    };
  });
</script>

<div class="block-chart">
  <h3>{block.title}</h3>
  <!-- Show as table sits under the title, above the chart (scenes `report`, `chartKinds`). -->
  {#if !failed}
    <div class="toggle"><button type="button" class="small" aria-pressed={asTable} onclick={() => (asTable = !asTable)}>{t('report-show-as-table')}</button></div>
  {/if}
  <!-- The canvas stays in the document while the table is shown, so toggling back keeps the same chart. -->
  <div bind:this={element} class="canvas" role="img" aria-label={block.title} hidden={failed || asTable}></div>
  {#if !failed && !asTable}<SourceLine source={block.source} />{/if}
  {#if failed || asTable}
    <TableView {backend} title={block.title} level={4} columns={block.columns} rows={block.rows} source={block.source} />
  {/if}
  <div class="actions">
    <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: block.title })} />
  </div>
</div>

<style>
  .block-chart {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .block-chart > h3 {
    margin: 0;
    font-size: 15px;
  }
  .canvas {
    inline-size: 100%;
    block-size: 320px;
  }
  .canvas[hidden] {
    display: none;
  }
  .actions {
    display: flex;
    gap: var(--space-sm);
  }
</style>
