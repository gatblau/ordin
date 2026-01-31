// cross-cutting/VisualDesign.md (ADR-116): the values of `tokens.css`, the brand-token mapping P2, the
// fonts P4, the contrast table V7, and the lint rule on component styles. Computed values come from a
// resolver over `tokens.css` (VisualDesign.md, Acceptance Criteria: happy-dom does not resolve nested
// `var()` fallbacks dependably).
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { createHandle } from '../src/lib/backend/backend';
import { CHART_TOKENS, chartOption, themeChartOption } from '../src/lib/blocks/chart-option';
import { applyBrandFonts, applyBrandToDocument, brandTokens } from '../src/lib/i18n/fluent';
import { CONVERSATION, RAIL, ViewPrefs } from '../src/lib/store/view-prefs';
import { NARROW_AT_MOST_PX } from '../src/lib/shell/shell-state.svelte';
import type { ChartBlock } from '../src/lib/service-api/types.gen';
import { FakeTransport, LOCAL, ok } from './support/fake-backend';

const ROOT = join(__dirname, '..');
const CSS = readFileSync(join(ROOT, 'src/styles/tokens.css'), 'utf8');

/** The declarations of the first block whose selector text equals `selector`. */
function block(selector: string): Map<string, string> {
  const at = CSS.indexOf(`${selector} {`);
  if (at < 0) throw new Error(`no block ${selector}`);
  const body = CSS.slice(CSS.indexOf('{', at) + 1, CSS.indexOf('}', at));
  const out = new Map<string, string>();
  for (const line of body.split(';')) {
    const colon = line.indexOf(':');
    if (colon > 0) out.set(line.slice(0, colon).trim(), line.slice(colon + 1).trim());
  }
  return out;
}

const LIGHT = block(':root');
const DARK = block(":root[data-theme='dark']");
const DARK_OS = block(":root:not([data-theme='light'])");

/** Resolves a property in a theme, with `brand` standing for the `--brand-*` properties set on :root. */
function resolve(name: string, theme: 'light' | 'dark', brand: Record<string, string> = {}): string {
  const lookup = (property: string): string | undefined => {
    if (property in brand) return brand[property];
    if (theme === 'dark' && DARK.has(property)) return DARK.get(property);
    return LIGHT.get(property);
  };
  const expand = (value: string): string => {
    let out = value;
    for (let guard = 0; guard < 20 && out.includes('var('); guard += 1) {
      out = out.replace(/var\((--[a-z0-9-]+)(?:,\s*((?:[^()]|\([^()]*\))*))?\)/i, (_, property: string, fallback?: string) => {
        const found = lookup(property);
        return found !== undefined ? found : (fallback ?? '');
      });
    }
    return out.trim();
  };
  const raw = lookup(name);
  if (raw === undefined) throw new Error(`no property ${name}`);
  return expand(raw);
}

function luminance(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0]! + 0.7152 * c[1]! + 0.0722 * c[2]!;
}
function ratio(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)].sort((m, n) => n - m) as [number, number];
  return (x + 0.05) / (y + 0.05);
}

describe('VisualDesign — values (V1 to V5)', () => {
  it('Happy path — the default values in the light theme', () => {
    expect(resolve('--color-background', 'light')).toBe('#F2F4F7');
    expect(resolve('--color-surface', 'light')).toBe('#FFFFFF');
    expect(resolve('--color-text', 'light')).toBe('#17212B');
    const body = block('body');
    expect(body.get('font-size')).toBe('16px');
    expect(body.get('line-height')).toBe('1.45');
    expect(body.get('font-family')).toBe('var(--font-body)');
  });

  it('Happy path — the default values in the dark theme, chosen or from the operating system', () => {
    for (const scope of [DARK, DARK_OS]) {
      expect(scope.get('--color-background')).toContain('#11161C');
      expect(scope.get('--color-surface')).toContain('#1A2129');
      expect(scope.get('--color-text')).toContain('#E8EDF2');
    }
    expect(resolve('--color-background', 'dark')).toBe('#11161C');
  });

  it('Happy path — the body is set in Jost, which the module ships', () => {
    expect(resolve('--font-body', 'light').split(',')[0]?.trim()).toBe("'Jost'");
    expect(CSS).toMatch(/@font-face\s*{\s*font-family: 'Jost';\s*src: url\('\.\.\/assets\/fonts\/Jost-wght\.ttf'\)/);
    expect(statSync(join(ROOT, 'src/assets/fonts/Jost-wght.ttf')).size).toBeGreaterThan(100_000);
    expect(readFileSync(join(ROOT, 'src/assets/fonts/OFL.txt'), 'utf8')).toContain('SIL OPEN FONT LICENSE');
  });

  it('Happy path — a brand token reaches the screens', () => {
    expect(resolve('--color-accent', 'light', { '--brand-color-accent-light': '#0B5FFF' })).toBe('#0B5FFF');
  });

  it('Edge — an optional role falls back to the pack\'s required role', () => {
    expect(resolve('--color-line', 'light', { '--brand-color-border-control-light': '#767676' })).toBe('#767676');
    expect(resolve('--color-line', 'light')).toBe('#DDE2E8');
  });

  it('Edge — the count badge and the control border pass contrast', () => {
    expect(resolve('--color-on-status', 'dark')).toBe('#11161C');
    expect(resolve('--color-waiting', 'dark')).toBe('#F0A35E');
    expect(resolve('--color-border-control', 'light')).toBe('#7D8A98');
  });

  it('Error — brand.get fails at start: every property keeps the prototype\'s value', () => {
    expect(resolve('--color-background', 'light', {})).toBe('#F2F4F7');
  });

  it('Edge — panel widths and the narrow window', () => {
    expect(RAIL.default).toBe(290);
    expect(CONVERSATION.default).toBe(420);
    expect(NARROW_AT_MOST_PX).toBe(1100);
    const stored = new Map<string, string>([['ordin.view.panel.rail', '260']]);
    const prefs = new ViewPrefs({ getItem: (k) => stored.get(k) ?? null, setItem: () => {} });
    expect(prefs.railWidth()).toBe(260);
  });
});

describe('VisualDesign — fallback chains', () => {
  const OPTIONAL: Record<string, string> = {
    'text-secondary': 'text',
    'accent-ink': 'text',
    'accent-soft': 'surface',
    chip: 'surface',
    line: 'border-control',
    'line-strong': 'border-control',
    primary: 'text',
    'on-primary': 'background',
  };
  const ROLES = ['text', 'text-muted', 'background', 'surface', 'accent', 'on-accent', 'border-control', 'focus', ...Object.keys(OPTIONAL)];

  it.each(ROLES)('--color-%s reads its brand token in each theme and ends in a literal', (role) => {
    for (const [theme, scope] of [['light', LIGHT], ['dark', DARK], ['dark', DARK_OS]] as const) {
      const value = scope.get(`--color-${role}`) ?? '';
      expect(value.startsWith(`var(--brand-color-${role}-${theme},`)).toBe(true);
      const fallback = OPTIONAL[role];
      if (fallback !== undefined) expect(value).toContain(`var(--brand-color-${fallback}-${theme},`);
      expect(resolve(`--color-${role}`, theme)).toMatch(/^#[0-9A-F]{6}$/);
    }
  });
});

describe('VisualDesign — contrast of the default values (V7)', () => {
  const PAIRS: Array<[string, string, number]> = [
    ['--color-text', '--color-background', 4.5], ['--color-text', '--color-surface', 4.5],
    ['--color-text-muted', '--color-background', 4.5], ['--color-text-muted', '--color-surface', 4.5],
    ['--color-text-secondary', '--color-background', 4.5], ['--color-text-secondary', '--color-surface', 4.5],
    ['--color-on-accent', '--color-accent', 4.5], ['--color-accent', '--color-background', 3],
    ['--color-accent-ink', '--color-background', 4.5], ['--color-accent-ink', '--color-surface', 4.5], ['--color-accent-ink', '--color-accent-soft', 4.5],
    ['--color-text', '--color-accent-soft', 4.5], ['--color-text', '--color-chip', 4.5],
    ['--color-border-control', '--color-background', 3], ['--color-border-control', '--color-surface', 3],
    ['--color-focus', '--color-background', 3], ['--color-on-primary', '--color-primary', 4.5],
    ['--color-waiting-text', '--color-waiting-soft', 4.5], ['--color-on-status', '--color-waiting', 4.5], ['--color-waiting', '--color-background', 3],
    ['--color-done-text', '--color-done-soft', 4.5], ['--color-done', '--color-background', 3],
    ['--color-danger', '--color-danger-soft', 4.5], ['--color-danger', '--color-surface', 4.5],
    ['--color-text', '--color-approval-background', 4.5], ['--color-approval-line', '--color-approval-background', 3],
    ['--color-accent-ink', '--color-approval-background', 4.5],
    ['--color-on-status', '--color-accent', 4.5], ['--color-on-status', '--color-done', 4.5], ['--color-on-status', '--color-danger', 4.5],
    ...[1, 2, 3, 4, 5, 6, 7, 8].map((n): [string, string, number] => [`--chart-${n}`, '--color-surface', 3]),
  ];
  it.each(PAIRS)('%s on %s passes %d:1 in both themes', (fg, bg, threshold) => {
    for (const theme of ['light', 'dark'] as const) {
      expect(ratio(resolve(fg, theme), resolve(bg, theme))).toBeGreaterThanOrEqual(threshold);
    }
  });
});

describe('VisualDesign — the brand-token mapping (P2)', () => {
  it.each([
    ['color.accent.light', '#0B5FFF', [['--brand-color-accent-light', '#0B5FFF']]],
    ['color.line.dark', '#2C3642', [['--brand-color-line-dark', '#2C3642']]],
    ['font.body', 'Jost', [['--brand-font-body', '"Jost"']]],
    ['font.body', 'system-ui', [['--brand-font-body', 'system-ui']]],
    ['space.sm', '8px', [['--brand-space-sm', '8px']]],
    ['space.md', '16px', [['--brand-space-md', '16px']]],
    ['radius.md', '8px', [['--brand-radius-md', '8px']]],
    ['radius.lg', '12px', [['--brand-radius-lg', '12px']]],
    ['color.danger.light', '#FF0000', []],
    ['color.run-state.dark', '#FF0000', []],
    ['color.unknown.light', '#FF0000', []],
    ['font.heading', 'Acme', []],
    ['space.lg', '24px', []],
  ] as const)('%s = %s', (name, value, expected) => {
    const recorded: string[] = [];
    expect(brandTokens({ [name]: value }, { record: (code, detail) => void recorded.push(`${code} ${detail}`) })).toEqual(expected);
    expect(recorded).toEqual(expected.length === 0 ? [`UI_BRAND_TOKEN_IGNORED ${name}`] : []);
  });

  it('Error — a fixed colour in the tokens is ignored and the fixed value stays', () => {
    const recorded: string[] = [];
    const set = brandTokens({ 'color.danger.light': '#FF0000' }, { record: (code, detail) => void recorded.push(`${code} ${detail}`) });
    expect(resolve('--color-danger', 'light', Object.fromEntries(set))).toBe('#B42318');
    expect(recorded).toEqual(['UI_BRAND_TOKEN_IGNORED color.danger.light']);
  });

  it('Edge — a new pack removes the previous pack\'s tokens', () => {
    const style = new Map<string, string>();
    const doc = {
      documentElement: {
        lang: '',
        dir: '',
        style: { setProperty: (n: string, v: string) => void style.set(n, v), removeProperty: (n: string) => (style.delete(n), '') },
      },
    };
    applyBrandToDocument({ 'color.chip.light': '#ABCDEF' }, doc);
    applyBrandToDocument({ 'color.surface.light': '#FAFAFA' }, doc);
    expect(resolve('--color-chip', 'light', Object.fromEntries(style))).toBe('#FAFAFA');
  });
});

describe('VisualDesign — brand font files (P4)', () => {
  const realFontFace = (globalThis as { FontFace?: unknown }).FontFace;
  afterEach(() => {
    (globalThis as { FontFace?: unknown }).FontFace = realFontFace;
  });

  function fontsSet(): { faces: Set<unknown>; add(f: unknown): void; delete(f: unknown): void } {
    const faces = new Set<unknown>();
    return { faces, add: (f) => void faces.add(f), delete: (f) => void faces.delete(f) };
  }
  function stubFontFace(loads: boolean): void {
    (globalThis as { FontFace?: unknown }).FontFace = class {
      constructor(public family: string, public data: ArrayBuffer, public descriptors: object) {}
      load(): Promise<unknown> {
        return loads ? Promise.resolve(this) : Promise.reject(new Error('not a font'));
      }
    };
  }

  it('Happy path — a pack\'s font file is added under its font.body family; the next pack\'s replace it', async () => {
    stubFontFace(true);
    const transport = new FakeTransport();
    transport.on('brand.get_asset', ok({ media_type: 'font/woff2', base64: 'AAEC' }));
    const handle = createHandle({ id: LOCAL, kind: 'local', transport });
    const fonts = fontsSet();
    await applyBrandFonts({ design_tokens: { 'font.body': 'Acme Sans' }, font_count: 1 }, handle, undefined, fonts as never);
    expect([...fonts.faces].map((f) => (f as { family: string; descriptors: object }).family)).toEqual(['Acme Sans']);
    expect((([...fonts.faces][0]) as { descriptors: object }).descriptors).toEqual({ weight: '100 900', style: 'normal' });
    await applyBrandFonts({ design_tokens: { 'font.body': 'Jost' }, font_count: 0 }, handle, undefined, fonts as never);
    expect(fonts.faces.size).toBe(0);
  });

  it('Error — a brand font file cannot be decoded', async () => {
    stubFontFace(false);
    const transport = new FakeTransport();
    transport.on('brand.get_asset', ok({ media_type: 'font/ttf', base64: 'AAEC' }));
    const handle = createHandle({ id: LOCAL, kind: 'local', transport });
    const fonts = fontsSet();
    const recorded: string[] = [];
    await applyBrandFonts({ design_tokens: { 'font.body': 'Acme Sans' }, font_count: 1 }, handle, { record: (c, d) => void recorded.push(`${c} ${d}`) }, fonts as never);
    expect(recorded).toEqual(['UI_BRAND_FONT_FAILED font 0']);
    expect(fonts.faces.size).toBe(0);
  });
});

describe('VisualDesign — no literal colour or font in a component', () => {
  function svelteFiles(dir: string): string[] {
    return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
      e.isDirectory() ? svelteFiles(join(dir, e.name)) : e.name.endsWith('.svelte') ? [join(dir, e.name)] : [],
    );
  }
  it('every <style> block uses the properties only', () => {
    const offenders: string[] = [];
    for (const file of svelteFiles(join(ROOT, 'src'))) {
      const text = readFileSync(file, 'utf8');
      for (const [, style] of text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)) {
        const body = (style ?? '').replace(/\/\*[\s\S]*?\*\//g, '');
        if (/#[0-9a-fA-F]{3,8}\b|\brgba?\(|\bhsla?\(/.test(body)) offenders.push(`${file}: colour literal`);
        for (const [, family] of body.matchAll(/font-family:\s*([^;]+);/g)) {
          if (!/^var\(--font-(body|mono)\)$/.test((family ?? '').trim())) offenders.push(`${file}: font-family ${family}`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });
});

// ScreensModule amendment item 4 and Internal Logic step 4: the canvas does not read custom properties after
// drawing, so a theme change draws the chart again from the theme now in force.
describe('Charts follow the theme (ScreensModule step 8, amendment item 4)', () => {
  const sample = {
    kind: 'chart', title: 'Day rate by supplier', chart: 'bar', orientation: 'vertical', arrangement: 'grouped',
    columns: [{ name: 'Supplier', type: 'text' }, { name: 'Q2', type: 'number' }, { name: 'Q3', type: 'number' }],
    rows: [['Acme', 415, 428], ['Kestrel', 398, 405]], x: 'Supplier', series: ['Q2', 'Q3'], y_title: 'Day rate',
    source: { kind: 'earlier_result', run_id: 'r' },
  } as unknown as ChartBlock;
  const drawIn = (theme: 'light' | 'dark') =>
    themeChartOption(chartOption(sample, CHART_TOKENS.map((token) => resolve(token, theme)), { animate: false }), (name) => resolve(name, theme));

  it('drawn again after a theme change, the series, text and lines take the new theme’s values', () => {
    const light = drawIn('light');
    const dark = drawIn('dark');
    expect(light['color']).toEqual(CHART_TOKENS.map((token) => resolve(token, 'light')));
    expect(dark['color']).toEqual(CHART_TOKENS.map((token) => resolve(token, 'dark')));
    expect(dark['color']).not.toEqual(light['color']);
    for (const [option, theme] of [[light, 'light'], [dark, 'dark']] as const) {
      expect((option['legend'] as { textStyle: { color: string } }).textStyle.color).toBe(resolve('--color-text-secondary', theme));
      expect((option['textStyle'] as { color: string }).color).toBe(resolve('--color-text-muted', theme));
      const y = option['yAxis'] as { axisLabel: { color: string; formatter?: unknown }; splitLine: { lineStyle: { color: string } } };
      expect(y.axisLabel.color).toBe(resolve('--color-text-secondary', theme));
      expect(typeof y.axisLabel.formatter).toBe('function');
      expect(y.splitLine.lineStyle.color).toBe(resolve('--color-line', theme));
    }
  });
});

describe('The window never scrolls (V5-SCR-1)', () => {
  // spec: ordin-desktop/cross-cutting/VisualDesign#V5-SCR-1
  it('Edge case — a screen taller than the window: the shell is held to the window, each region scrolls', () => {
    const shell = block('.shell');
    expect(shell.get('grid-template-rows')).toBe('minmax(0, 1fr)');
    expect(shell.get('height')).toBe('100%');
    expect(block('.region').get('overflow')).toBe('auto');
    const root = block('html,\nbody,\n#app');
    expect(root.get('height')).toBe('100%');
  });

  // spec: ordin-desktop/cross-cutting/VisualDesign#V5-SCR-1
  it('Edge case — one pixel of rounding on a scaled display does not make the window scroll', () => {
    expect(block('html').get('overflow')).toBe('hidden');
    expect(block('body').get('overflow')).toBe('hidden');
  });
});

// spec: ordin-desktop/components/ScreensModule#SM-PG-1
describe('A working run shows movement (SM-PG-1)', () => {
  it('Happy path — the ring turns, and it stands still for reduced motion', () => {
    expect(CSS).toMatch(/\.activity \{[^}]*animation: ordin-spin 0\.9s linear infinite;/);
    expect(CSS).toMatch(/@keyframes ordin-spin/);
    expect(CSS).toMatch(/@media \(prefers-reduced-motion: reduce\) \{\s*\.activity \{\s*animation: none;/);
  });
});

