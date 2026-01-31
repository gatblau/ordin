// ECharts under the window's frozen `Object.prototype` (OrdinDesktop `tauri.conf.json`: `freezePrototype`;
// ScreensModule.md SM-SF-21). The window freezes the prototype before any script runs, and ECharts then threw
// while loading, so the memory diagram (and every chart) was never drawn. Each case runs ECharts in its own
// Node process, the prototype frozen first as the window does, and draws the diagram of one thing and four
// values with no joins: once as ECharts ships, once through the build's rewrite.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { describe, expect, it } from 'vitest';

import { echartsFrozenPrototype, isScheduler, patchScheduler } from '../echarts-frozen-prototype';

const UI = fileURLToPath(new URL('..', import.meta.url));
const PATCH = pathToFileURL(fileURLToPath(new URL('../echarts-frozen-prototype.ts', import.meta.url))).href;

/** A Node process that freezes `Object.prototype`, loads ECharts (rewritten when `patched`) and draws. */
function draw(patched: boolean): string {
  const script = `
    import { registerHooks } from 'node:module';
    import { fileURLToPath } from 'node:url';
    if (${patched}) {
      const { isScheduler, patchScheduler } = await import(${JSON.stringify(PATCH)});
      registerHooks({
        load(url, context, next) {
          const out = next(url, context);
          if (url.startsWith('file:') && isScheduler(fileURLToPath(url))) return { ...out, source: patchScheduler(String(out.source)) };
          return out;
        },
      });
    }
    Object.freeze(Object.prototype);
    try {
      const [core, charts, renderers] = await Promise.all([import('echarts/core'), import('echarts/charts'), import('echarts/renderers')]);
      core.use([charts.GraphChart, renderers.SVGRenderer]);
      const chart = core.init(null, null, { renderer: 'svg', ssr: true, width: 600, height: 420 });
      const box = (id, name, w, h) => ({ id, name, symbol: 'roundRect', symbolSize: [w, h], label: { show: true } });
      const values = [0, 1, 2, 3].map((i) => box('value:f' + i, String(60 + i), 100, 26));
      chart.setOption({
        animation: false,
        aria: { enabled: false },
        series: [{
          type: 'graph', layout: 'force', roam: true, draggable: false, force: { repulsion: 320, edgeLength: [90, 160] },
          edgeSymbol: ['none', 'arrow'], data: [box('thing:a', 'Manchester Digital Strategy', 150, 44), ...values],
          links: values.map((v) => ({ source: 'thing:a', target: v.id, label: { show: true, formatter: 'partners consulted' } })),
        }],
      }, true);
      const svg = chart.renderToSVGString();
      console.log(svg.includes('Manchester Digital Strategy') ? 'drawn' : 'empty');
      chart.dispose();
    } catch (error) {
      console.log('failed: ' + error.message);
    }
    // ECharts' animation loop keeps the event loop alive; the answer is printed, so end here.
    process.exit(0);
  `;
  return execFileSync(process.execPath, ['--input-type=module', '-e', script], { cwd: UI, encoding: 'utf8', timeout: 20_000, killSignal: 'SIGKILL' }).trim();
}

describe('SM-SF-21 — ECharts under the frozen prototype', () => {
  it('Error path — as ECharts ships, loading it under the frozen prototype throws, so nothing is drawn', () => {
    expect(draw(false)).toMatch(/^failed: .*read only property 'constructor'/);
  });

  it('Happy path — through the build\'s rewrite, the diagram of one thing and four values is drawn', () => {
    expect(draw(true)).toBe('drawn');
  });

  it('the rewrite finds its one assignment in the installed ECharts, and the build plugin applies only to Scheduler.js', () => {
    const file = createRequire(import.meta.url).resolve('echarts/lib/core/Scheduler.js', { paths: [UI] });
    expect(isScheduler(file)).toBe(true);
    const patched = patchScheduler(readFileSync(file, 'utf8'));
    expect(patched).toContain('Object.defineProperty(target, name_1,');
    expect(patched).not.toContain('target[name_1] = noop;');
    expect(isScheduler(`${UI}node_modules/echarts/lib/core/echarts.js`)).toBe(false);
    expect(() => patchScheduler('export {};')).toThrow(/not found/);
    expect(echartsFrozenPrototype().name).toBe('ordin-echarts-frozen-prototype');
  });
});
