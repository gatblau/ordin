// @vitest-environment happy-dom
// Icons (ScreensModule.md amendment *Icons*, SM-IC-1, SM-IC-2, SM-IC-4; cross-cutting/VisualDesign.md V8
// and Internal Logic step 8): the sprite, its replacement by a brand pack's icons, the icon part and
// the `.icon` rules.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { BackendHandle } from '../src/lib/backend/backend';
import Icon from '../src/lib/icons/Icon.svelte';
import { applyBrandIcons, ICON_NAMES, installSprite, removeSprite } from '../src/lib/icons/sprite';

const ROOT = join(__dirname, '..');
const OWN = readFileSync(join(ROOT, 'src/assets/icons/ordin-icons.svg'));
const BUILD_PACK = readFileSync(join(ROOT, '../../../crates/ordin-brand/build-brand/icons-light.svg'));
const CSS = readFileSync(join(ROOT, 'src/styles/tokens.css'), 'utf8');

const SVG_NS = 'http://www.w3.org/2000/svg';
const COMPANY = { source: 'organisation_profile' } as const;
const b64 = (text: string): string => Buffer.from(text, 'utf8').toString('base64');
const svg = (body: string): string => `<svg xmlns="${SVG_NS}">${body}</svg>`;

/** The sprite's symbols, by id, in document order. */
function sprite(): Element[] {
  const root = document.body.firstElementChild;
  expect(root?.localName).toBe('svg');
  return Array.from(root!.children);
}
function symbol(id: string): Element {
  const found = sprite().find((s) => s.getAttribute('id') === id);
  if (found === undefined) throw new Error(`no symbol ${id}`);
  return found;
}
/** The drawing of ordin's own symbol `id`, from the module's file. */
function ownDrawing(id: string): string {
  const text = OWN.toString('utf8');
  const at = text.indexOf(`<symbol id="${id}"`);
  return text.slice(text.indexOf('>', at) + 1, text.indexOf('</symbol>', at));
}
/** A symbol's children as markup, for comparison with the file. */
function drawing(el: Element): string {
  return Array.from(el.children)
    .map((c) => `<${c.localName}${Array.from(c.attributes).map((a) => ` ${a.name}="${a.value}"`).join('')}/>`)
    .join('');
}

interface Recorded {
  code: string;
  detail?: string;
}

/**
 * A local backend handle answering `brand.get` and `brand.get_asset` with the wire shapes of
 * `src/lib/service-api/commands.gen.ts` (`BrandGetResult`, `AssetBytes`). The shared FakeTransport is not
 * used here: its catalogue import is outside the root that the DOM environment may read.
 */
function packWith(light: { media_type: string; base64: string } | 'error', dark?: { media_type: string; base64: string } | 'error') {
  const calls: string[] = [];
  let brandGet: unknown = { kind: 'error', error: { code: 'BRAND_UNAVAILABLE', message: 'unavailable' } };
  const handle = {
    id: '0192f100-0000-7000-8000-00000000000a',
    kind: 'local',
    call: async (command: string, input: unknown) => {
      calls.push(command);
      if (command === 'brand.get') return brandGet;
      const kind = (input as { asset: { kind: string } }).asset.kind;
      const answer = kind === 'icon_set_light' ? light : (dark ?? { media_type: 'image/svg+xml', base64: b64(svg('')) });
      return answer === 'error' ? { kind: 'error', error: { code: 'BRAND_ASSET_UNAVAILABLE', message: 'unavailable' } } : { kind: 'ok', result: answer };
    },
  } as unknown as BackendHandle;
  const recorded: Recorded[] = [];
  const diagnostics = { record: (code: string, detail?: string) => void recorded.push({ code, ...(detail === undefined ? {} : { detail }) }) };
  const sent = (command: string): number => calls.filter((c) => c === command).length;
  const answerBrandGet = (result: unknown): void => {
    brandGet = { kind: 'ok', result };
  };
  return { handle, recorded, diagnostics, sent, answerBrandGet };
}

beforeEach(() => {
  document.documentElement.dataset['theme'] = 'light';
  installSprite(document);
});
afterEach(() => {
  removeSprite(document);
  delete document.documentElement.dataset['theme'];
});

describe('SM-IC-1 — the sprite', () => {
  it("Edge — the build pack carries V8's 50 drawings word for word; the editor's 9 are ordin's own", () => {
    const own = OWN.toString('utf8');
    const firstEditor = own.indexOf('  <symbol id="i-heading"');
    expect(firstEditor).toBeGreaterThan(0);
    expect(own.slice(0, firstEditor) + '</svg>\n').toBe(BUILD_PACK.toString('utf8'));
  });

  it("Happy path — the document holds one hidden sprite with exactly the 62 icon names (V8's 50, the editor's 9 and the three action icons)", () => {
    expect(ICON_NAMES).toHaveLength(62);
    const root = document.body.firstElementChild!;
    expect(root.getAttribute('aria-hidden')).toBe('true');
    expect(root.getAttribute('focusable')).toBe('false');
    expect(root.getAttribute('style')).toBe('position:absolute;width:0;height:0');
    expect(sprite().map((s) => s.getAttribute('id'))).toEqual(ICON_NAMES.map((n) => `i-${n}`));
    expect(sprite().every((s) => s.getAttribute('viewBox') === '0 0 16 16')).toBe(true);
    // A second install keeps the one sprite.
    installSprite(document);
    expect(document.querySelectorAll('svg[data-ordin-sprite]')).toHaveLength(1);
  });

  it('ordin\'s own drawings are copied whole, i-more keeping its stroke width', () => {
    for (const name of ICON_NAMES) expect(drawing(symbol(`i-${name}`)), name).toBe(ownDrawing(`i-${name}`).replace(/"\/>/g, '"/>'));
    expect(symbol('i-more').firstElementChild?.getAttribute('stroke-width')).toBe('2.6');
  });

  it('Happy path — a build pack makes no brand.get_asset call and records nothing', async () => {
    const { handle, recorded, diagnostics, sent } = packWith('error');
    await applyBrandIcons(handle, { brand: { source: 'build' }, diagnostics, doc: document });
    expect(sent('brand.get_asset')).toBe(0);
    expect(recorded).toEqual([]);
    expect(drawing(symbol('i-home'))).toBe(ownDrawing('i-home'));
  });

  it('calls brand.get itself when the caller has no answer', async () => {
    const { handle, sent, answerBrandGet } = packWith('error');
    answerBrandGet({ source: 'build' });
    await applyBrandIcons(handle, { doc: document });
    expect(sent('brand.get')).toBe(1);
    expect(sent('brand.get_asset')).toBe(0);
  });
});

describe('Internal Logic step 8 — a company pack\'s icons', () => {
  it('Edge — a company pack replaces one icon; the other 49 stay ordin\'s', async () => {
    const { handle, recorded, diagnostics } = packWith({ media_type: 'image/svg+xml', base64: b64(svg('<symbol id="i-home" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5"/></symbol>')) });
    await applyBrandIcons(handle, { brand: COMPANY, diagnostics, doc: document });
    expect(drawing(symbol('i-home'))).toBe('<circle cx="8" cy="8" r="5"/>');
    for (const name of ICON_NAMES.filter((n) => n !== 'home')) expect(drawing(symbol(`i-${name}`)), name).toBe(ownDrawing(`i-${name}`));
    // The dark set of this pack names no icon.
    expect(recorded).toEqual([{ code: 'UI_BRAND_ICONS_IGNORED', detail: 'icon set dark: no named icons' }]);
  });

  it('the V8 Example — class, fill and image are dropped, acme-logo is ignored', async () => {
    const example = '<svg xmlns="http://www.w3.org/2000/svg"><symbol id="i-home" viewBox="0 0 16 16" class="x"><path d="M2 8l6-5 6 5" fill="red"/><image href="#a"/></symbol><symbol id="acme-logo"/></svg>';
    const { handle, diagnostics } = packWith({ media_type: 'image/svg+xml', base64: b64(example) });
    await applyBrandIcons(handle, { brand: COMPANY, diagnostics, doc: document });
    const home = symbol('i-home');
    expect(Array.from(home.attributes).map((a) => a.name).sort()).toEqual(['id', 'viewBox']);
    expect(drawing(home)).toBe('<path d="M2 8l6-5 6 5"/>');
    expect(home.getElementsByTagName('image')).toHaveLength(0);
    expect(sprite().some((s) => s.getAttribute('id') === 'acme-logo')).toBe(false);
    expect(sprite()).toHaveLength(62);
  });

  it('drops an element off the list with its children, and every attribute off the list', async () => {
    const hostile = svg(
      '<symbol id="i-home" viewBox="0 0 16 16" onload="x()" style="a:b"><g transform="t" onclick="y()"><path d="M1 1" href="#x" style="s"/></g>'
      + '<foreignObject><path d="M9 9"/></foreignObject><script>z()</script><use href="#i-bin"/><a href="javascript:x"><path d="M3 3"/></a></symbol>',
    );
    const { handle, diagnostics } = packWith({ media_type: 'image/svg+xml', base64: b64(hostile) });
    await applyBrandIcons(handle, { brand: COMPANY, diagnostics, doc: document });
    const home = symbol('i-home');
    expect(home.outerHTML.replace(/ xmlns="[^"]*"/g, '')).toBe('<symbol id="i-home" viewBox="0 0 16 16"><g><path d="M1 1"></path></g></symbol>');
  });

  it.each([
    ['png', { media_type: 'image/png', base64: 'iVBORw0KGgo=' }],
    ['unreadable', { media_type: 'image/svg+xml', base64: b64('this is not xml <<<') }],
    ['no named icons', { media_type: 'image/svg+xml', base64: b64(svg('<symbol id="acme-logo" viewBox="0 0 16 16"><path d="M1 1"/></symbol>')) }],
    ['unavailable', 'error' as const],
  ])('Error — an icon set that is %s draws ordin\'s icons and records UI_BRAND_ICONS_IGNORED', async (detail, light) => {
    const { handle, recorded, diagnostics } = packWith(light, light);
    await applyBrandIcons(handle, { brand: COMPANY, diagnostics, doc: document });
    for (const name of ICON_NAMES) expect(drawing(symbol(`i-${name}`)), name).toBe(ownDrawing(`i-${name}`));
    expect(recorded).toEqual([
      { code: 'UI_BRAND_ICONS_IGNORED', detail: `icon set light: ${detail}` },
      { code: 'UI_BRAND_ICONS_IGNORED', detail: `icon set dark: ${detail}` },
    ]);
  });

  it('Edge — switching light and dark swaps the theme\'s map; the stroke stays currentColor', async () => {
    const { handle, diagnostics } = packWith(
      { media_type: 'image/svg+xml', base64: b64(svg('<symbol id="i-home" viewBox="0 0 16 16"><circle cx="8" cy="8" r="1"/></symbol>')) },
      { media_type: 'image/svg+xml', base64: b64(svg('<symbol id="i-home" viewBox="0 0 16 16"><circle cx="8" cy="8" r="2"/></symbol>')) },
    );
    await applyBrandIcons(handle, { brand: COMPANY, diagnostics, doc: document });
    expect(drawing(symbol('i-home'))).toBe('<circle cx="8" cy="8" r="1"/>');
    document.documentElement.dataset['theme'] = 'dark';
    await new Promise((r) => setTimeout(r, 0));
    expect(drawing(symbol('i-home'))).toBe('<circle cx="8" cy="8" r="2"/>');
    // No colour reaches the sprite: the icon takes the text colour through `.icon`.
    expect(sprite().some((s) => /fill=|stroke=|style=/.test(s.outerHTML))).toBe(false);
    expect(CSS).toMatch(/\.icon \{[^}]*stroke: currentColor;[^}]*\}/);
    expect(CSS).toMatch(/\.icon \{[^}]*fill: none;[^}]*\}/);
  });

  it('brand_changed to a build pack returns every icon to ordin\'s', async () => {
    const { handle, diagnostics } = packWith({ media_type: 'image/svg+xml', base64: b64(svg('<symbol id="i-home" viewBox="0 0 16 16"><circle cx="8" cy="8" r="1"/></symbol>')) });
    await applyBrandIcons(handle, { brand: COMPANY, diagnostics, doc: document });
    await applyBrandIcons(handle, { brand: { source: 'build' }, diagnostics, doc: document });
    expect(drawing(symbol('i-home'))).toBe(ownDrawing('i-home'));
  });
});

describe('SM-IC-2 — the icon part', () => {
  it('draws exactly the svg and its use, and nothing else', () => {
    /** The drawn markup with each element's attributes in name order (Svelte sets `class` last). */
    const drawn = (props: Record<string, unknown>): string => {
      const target = document.createElement('div');
      const part = mount(Icon, { target, props: props as never });
      flushSync();
      const show = (el: Element): string =>
        `<${el.localName}${Array.from(el.attributes).map((a) => ` ${a.name}="${a.value}"`).sort().join('')}>${Array.from(el.children).map(show).join('')}</${el.localName}>`;
      const out = Array.from(target.children).map(show).join('');
      void unmount(part);
      return out;
    };
    expect(drawn({ name: 'home' })).toBe('<svg aria-hidden="true" class="icon icon-default" focusable="false"><use href="#i-home"></use></svg>');
    expect(drawn({ name: 'remove', size: 'close' })).toBe('<svg aria-hidden="true" class="icon icon-close" focusable="false"><use href="#i-remove"></use></svg>');
  });

  it.each([
    ['icon', '16px'],
    ['icon-small', '13px'],
    ['icon-twist', '12px'],
    ['icon-mark', '12px'],
    ['icon-needs-tab', '14px'],
    ['icon-close', '11px'],
    ['icon-heading', '44px'],
  ])('the size of .%s is %s', (cls, px) => {
    expect(CSS).toMatch(new RegExp(`\\.${cls} \\{[^}]*width: ${px};[^}]*height: ${px};`));
  });

  it('a default icon never shrinks, draws round strokes 1.6 wide, and the heading tile is 44 px with a 24 px drawing', () => {
    expect(CSS).toMatch(/\.icon \{[^}]*flex-shrink: 0;[^}]*stroke-width: 1\.6;[^}]*stroke-linecap: round;[^}]*stroke-linejoin: round;/);
    expect(CSS).toMatch(/\.icon-heading \{[^}]*padding: 10px;[^}]*background: var\(--color-chip\);/);
  });
});

describe('SM-IC-4 — direction', () => {
  it('Edge — direction-bearing icons mirror under rtl; an open twist turns and does not mirror', () => {
    for (const name of ['chev', 'send', 'open', 'history']) {
      expect(CSS).toContain(`[dir='rtl'] .icon:has(> use[href='#i-${name}'])`);
    }
    expect(CSS).toMatch(/use\[href='#i-history'\]\) \{\s*transform: scaleX\(-1\);/);
    expect(CSS).toMatch(/\[dir='rtl'\] \[aria-expanded='true'\] > \.icon\.icon-twist,\s*\[dir='rtl'\] \.open > \.icon\.icon-twist \{\s*transform: rotate\(90deg\);/);
    expect(CSS).toMatch(/\n\[aria-expanded='true'\] > \.icon\.icon-twist,\s*\.open > \.icon\.icon-twist \{\s*transform: rotate\(90deg\);/);
    // No other icon mirrors.
    expect(CSS.match(/\[dir='rtl'\] \.icon:has/g)).toHaveLength(4);
  });
});
