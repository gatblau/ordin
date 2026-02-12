<script lang="ts">
  // The memory diagram (ADR-120 item 2; SM-SF-21): ECharts' `graph` series through `chart-draw.ts`'s loader.
  // The canvas is hidden from assistive technology: the lists beside it are its equivalent (SM-SF-22). A
  // press on a box or an arrow is handed to the screen; the drawing itself changes nothing. When drawing
  // fails, `UI_CHART_RENDER_FAILED` is recorded and the canvas is not shown, so the lists stand alone.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-21
  // spec: ordin-desktop/components/ScreensModule#SM-SF-22
  // spec: ordin-desktop/components/ScreensModule#SM-SF-28
  import { onMount } from 'svelte';

  import type { DiagnosticSink } from '../../backend/backend';
  import { drawChart, loadEcharts, type ChartInstance, type ChartLoader } from '../../blocks/chart-draw';
  import { t } from '../../i18n/fluent';
  import type { DiagramView } from '../../service-api/types.gen';
  import { diagramColours, diagramOption, diagramPress, type DiagramPress } from './diagram-option';

  interface Props {
    view: DiagramView;
    diagnostics: DiagnosticSink | undefined;
    onpress: (press: Exclude<DiagramPress, null>) => void;
    /** The fact whose panel is open, drawn marked (SM-SF-28). */
    selected?: string | null;
    /** The chart library; ECharts unless a test gives its own. */
    load?: ChartLoader;
  }
  let { view, diagnostics, onpress, selected = null, load = loadEcharts }: Props = $props();

  let element = $state<HTMLElement | null>(null);
  let failed = $state(false);
  let instance: ChartInstance | null = null;

  const reducedMotion = (): boolean => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  // The theme's colours, read when drawn and again on a theme change (ADR-116).
  const option = (v: DiagramView): ReturnType<typeof diagramOption> => {
    const style = getComputedStyle(document.documentElement);
    return diagramOption(v, diagramColours((name) => style.getPropertyValue(name).trim()), { animate: !reducedMotion(), selected });
  };

  function redraw(): void {
    if (instance === null) return;
    try {
      instance.setOption(option(view), true);
    } catch {
      diagnostics?.record('UI_CHART_RENDER_FAILED');
      failed = true;
    }
  }

  onMount(() => {
    let disposed = false;
    const cleanups: Array<() => void> = [];
    void (async () => {
      if (element === null) return;
      const result = await drawChart(element, option(view), diagnostics, load);
      if (disposed) {
        if (result.kind === 'drawn') result.instance.dispose();
        return;
      }
      if (result.kind === 'failed') {
        failed = true;
        return;
      }
      instance = result.instance;
      instance.on?.('click', (params) => {
        const press = diagramPress(params);
        if (press !== null) onpress(press);
      });
      // Drawn again on a theme change (ADR-116), and resized with its box.
      const scheme = matchMedia('(prefers-color-scheme: dark)');
      scheme.addEventListener('change', redraw);
      cleanups.push(() => scheme.removeEventListener('change', redraw));
      const attributes = new MutationObserver(redraw);
      attributes.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'style'] });
      cleanups.push(() => attributes.disconnect());
      if (typeof ResizeObserver === 'function') {
        const size = new ResizeObserver(() => instance?.resize());
        size.observe(element);
        cleanups.push(() => size.disconnect());
      }
    })();
    return () => {
      disposed = true;
      for (const stop of cleanups) stop();
      instance?.dispose();
      instance = null;
    };
  });

  // A new `DiagramView` (a new centre) or a new selected fact redraws the same canvas.
  $effect(() => {
    void view;
    void selected;
    redraw();
  });
</script>

<div bind:this={element} class="diagram-canvas" aria-hidden="true" hidden={failed}></div>
<!-- A drawing failure is said in words, so the person knows why only the lists are there (SM-SF-21). -->
{#if failed}<p class="diagram-not-drawn" role="status">{t('memory-diagram-not-drawn')}</p>{/if}

<style>
  .diagram-canvas {
    inline-size: 100%;
    block-size: 420px;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    background: var(--color-background);
  }
  .diagram-canvas[hidden] {
    display: none;
  }
  .diagram-not-drawn {
    margin: 0;
    color: var(--color-text-muted);
  }
</style>
