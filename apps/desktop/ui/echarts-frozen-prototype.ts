// ECharts under the window's frozen `Object.prototype` (OrdinDesktop `tauri.conf.json`: `freezePrototype`).
//
// When `echarts/core` is first evaluated, `Scheduler.js`'s `mockMethods` copies every enumerable name of a
// class's prototype onto a plain object with `target[name] = noop`. One of those names is `constructor`,
// and a plain object inherits `constructor` from `Object.prototype`; once that is frozen the inherited
// property is read-only, so in strict code the assignment throws ("Cannot assign to read only property
// 'constructor'") and ECharts never loads: every chart, and the memory diagram, fails to draw.
//
// The build rewrites that one assignment as `Object.defineProperty`, which defines an own property and so is
// not stopped by the read-only property it shadows. The prototype stays frozen. When ECharts changes and the
// assignment is no longer found, the build stops rather than shipping charts that cannot load.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-21
import type { Plugin } from 'vite';

const SCHEDULER = /[\\/]node_modules[\\/]echarts[\\/]lib[\\/]core[\\/]Scheduler\.js$/;
const ASSIGNMENT = 'target[name_1] = noop;';
const DEFINITION = 'Object.defineProperty(target, name_1, { value: noop, writable: true, enumerable: true, configurable: true });';

/** `Scheduler.js` with its one assignment rewritten; throws when the assignment is not there. */
export function patchScheduler(code: string): string {
  if (!code.includes(ASSIGNMENT)) {
    throw new Error('echarts-frozen-prototype: `target[name_1] = noop;` not found in echarts/lib/core/Scheduler.js');
  }
  return code.replace(ASSIGNMENT, DEFINITION);
}

/** Whether a module id is ECharts' `Scheduler.js`. */
export function isScheduler(id: string): boolean {
  return SCHEDULER.test(id.split('?')[0] ?? id);
}

function transform(code: string, id: string): { code: string; map: null } | null {
  if (!isScheduler(id)) return null;
  return { code: patchScheduler(code), map: null };
}

/** The Vite plugin that applies `patchScheduler` to ECharts' `Scheduler.js` (the build). */
export function echartsFrozenPrototype(): Plugin {
  return { name: 'ordin-echarts-frozen-prototype', enforce: 'pre', transform };
}

/** The same for the development server, which pre-bundles ECharts without Vite's own plugins (`tauri dev`). */
export function echartsFrozenPrototypeDeps(): { name: string; transform: typeof transform } {
  return { name: 'ordin-echarts-frozen-prototype-deps', transform };
}
