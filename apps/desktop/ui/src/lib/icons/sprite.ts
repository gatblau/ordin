// The icon sprite (ScreensModule.md amendment *Icons*, SM-IC-1; cross-cutting/VisualDesign.md V8 and
// Internal Logic step 8). The window holds one hidden `<svg>` of 50 `<symbol>`s, ordin's own drawings,
// each replaced by the active brand pack's symbol of the same name when the pack's icon set for the
// effective theme is SVG and carries it. Every element of the sprite is created with
// `createElementNS` and given only the attributes of the allowlist below; no markup string from a pack
// is ever handed to the document as markup.
//
// spec: ordin-desktop/components/ScreensModule#SM-IC-1
import spriteText from '../../assets/icons/ordin-icons.svg?raw';

import type { BackendHandle, DiagnosticSink } from '../backend/backend';
import type { BrandGetResult } from '../service-api/types.gen';

/** The 50 icon names of V8, without their `i-` prefix, in V8's order. */
export const ICON_NAMES = [
  'chev', 'ws', 'docs', 'sp', 'cf', 'gl', 'gh', 'lc', 'folder', 'word',
  'sheet', 'slides', 'pdf', 'page', 'md', 'image', 'agent', 'remote', 'task', 'crew',
  'keptcrew', 'run', 'mail', 'chart', 'memory', 'know', 'mic', 'more', 'plus', 'chat',
  'open', 'send', 'publish', 'remove', 'bin', 'stop', 'play', 'edit', 'info', 'key',
  'search', 'home', 'eye', 'project', 'code', 'history', 'copy', 'term', 'fold', 'unfold',
] as const;

export type IconName = (typeof ICON_NAMES)[number];

export type IconTheme = 'light' | 'dark';

const SVG_NS = 'http://www.w3.org/2000/svg';
const SYMBOL_IDS: ReadonlySet<string> = new Set(ICON_NAMES.map((name) => `i-${name}`));

/** The elements a symbol may hold, and the attributes each keeps (Internal Logic step 8). */
const ALLOWED: Readonly<Record<string, readonly string[]>> = {
  symbol: ['id', 'viewBox'],
  path: ['d', 'stroke-width'],
  circle: ['cx', 'cy', 'r'],
  ellipse: ['cx', 'cy', 'rx', 'ry'],
  rect: ['x', 'y', 'width', 'height', 'rx', 'ry'],
  line: ['x1', 'y1', 'x2', 'y2'],
  polyline: ['points'],
  polygon: ['points'],
  g: [],
};

/** Marks the window's sprite, so a second `installSprite` finds it. */
const SPRITE_MARK = 'data-ordin-sprite';

/** A symbol held as a detached, already-sanitised element of the sprite's own document. */
type SymbolMap = Map<string, Element>;

interface SpriteState {
  sprite: Element;
  own: SymbolMap;
  brand: Record<IconTheme, SymbolMap>;
  drawnTheme: IconTheme | null;
  /** Raised by each `applyBrandIcons`, so an older call that answers late does not overwrite a newer one. */
  generation: number;
  stopWatching: () => void;
}

const states = new WeakMap<Document, SpriteState>();

/**
 * Copies `source` into `doc` when its element is on the allowlist, with the allowed attributes only;
 * an element off the list is dropped with its children (Internal Logic step 8).
 */
function copyAllowed(source: Element, doc: Document): Element | null {
  const keep = ALLOWED[source.localName];
  if (keep === undefined || source.namespaceURI !== SVG_NS) return null;
  const out = doc.createElementNS(SVG_NS, source.localName);
  for (const name of keep) {
    const value = source.getAttribute(name);
    if (value !== null) out.setAttribute(name, value);
  }
  for (const child of Array.from(source.children)) {
    const copy = copyAllowed(child, doc);
    if (copy !== null) out.appendChild(copy);
  }
  return out;
}

/** The icon symbols of a parsed SVG document, sanitised into `doc`; the first symbol of a name wins. */
function symbolsOf(parsed: Document, doc: Document): SymbolMap {
  const out: SymbolMap = new Map();
  for (const symbol of Array.from(parsed.getElementsByTagNameNS(SVG_NS, 'symbol'))) {
    const id = symbol.getAttribute('id');
    if (id === null || !SYMBOL_IDS.has(id) || out.has(id)) continue;
    const copy = copyAllowed(symbol, doc);
    if (copy !== null) out.set(id, copy);
  }
  return out;
}

/** Parses SVG text; `null` when the parser reports an error or the root is not `<svg>`. */
function parseSvg(text: string, doc: Document): Document | null {
  const view = doc.defaultView as (Window & typeof globalThis) | null;
  const Parser = view?.DOMParser ?? globalThis.DOMParser;
  const parsed = new Parser().parseFromString(text, 'image/svg+xml');
  if (parsed.getElementsByTagName('parsererror').length > 0) return null;
  const root = parsed.documentElement;
  if (root === null || root.localName !== 'svg' || root.namespaceURI !== SVG_NS) return null;
  return parsed;
}

/** The effective theme: `data-theme` when the person chose one, else the operating system's. */
export function effectiveTheme(doc: Document = document): IconTheme {
  const chosen = doc.documentElement.dataset['theme'];
  if (chosen === 'light' || chosen === 'dark') return chosen;
  const view = doc.defaultView;
  return view?.matchMedia?.('(prefers-color-scheme: dark)').matches === true ? 'dark' : 'light';
}

/** Puts, for each of the 50 names, the brand's symbol for the effective theme or ordin's own into the sprite. */
function draw(doc: Document, state: SpriteState, force = false): void {
  const theme = effectiveTheme(doc);
  if (!force && state.drawnTheme === theme) return;
  const brand = state.brand[theme];
  const symbols: Element[] = [];
  for (const name of ICON_NAMES) {
    const id = `i-${name}`;
    const chosen = brand.get(id) ?? state.own.get(id);
    if (chosen !== undefined) symbols.push(chosen.cloneNode(true) as Element);
  }
  state.sprite.replaceChildren(...symbols);
  state.drawnTheme = theme;
}

/** Redraws the sprite whenever the effective theme changes: `data-theme`, or the operating system's. */
function watchTheme(doc: Document, onChange: () => void): () => void {
  const view = doc.defaultView;
  const scheme = view?.matchMedia?.('(prefers-color-scheme: dark)');
  scheme?.addEventListener?.('change', onChange);
  const Observer = (view as (Window & typeof globalThis) | null)?.MutationObserver ?? globalThis.MutationObserver;
  const attributes = Observer === undefined ? null : new Observer(onChange);
  attributes?.observe(doc.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  return () => {
    scheme?.removeEventListener?.('change', onChange);
    attributes?.disconnect();
  };
}

/**
 * Adds the hidden sprite as the first child of `<body>`, holding ordin's 50 symbols, before the first
 * screen is drawn (Internal Logic step 8). A second call keeps the sprite already there.
 */
export function installSprite(doc: Document = document): void {
  if (states.has(doc)) return;
  const sprite = doc.createElementNS(SVG_NS, 'svg');
  sprite.setAttribute(SPRITE_MARK, '');
  sprite.setAttribute('aria-hidden', 'true');
  sprite.setAttribute('focusable', 'false');
  sprite.setAttribute('style', 'position:absolute;width:0;height:0');
  const parsed = parseSvg(spriteText, doc);
  // The module's own file is checked by its tests; an unreadable copy draws no icon rather than stopping the window.
  const own = parsed === null ? new Map<string, Element>() : symbolsOf(parsed, doc);
  const state: SpriteState = {
    sprite,
    own,
    brand: { light: new Map(), dark: new Map() },
    drawnTheme: null,
    generation: 0,
    stopWatching: () => {},
  };
  states.set(doc, state);
  doc.body.insertBefore(sprite, doc.body.firstChild);
  draw(doc, state, true);
  state.stopWatching = watchTheme(doc, () => draw(doc, state));
}

/** Removes the sprite and its theme watch (tests, and a window being torn down). */
export function removeSprite(doc: Document = document): void {
  const state = states.get(doc);
  if (state === undefined) return;
  state.stopWatching();
  state.sprite.remove();
  states.delete(doc);
}

/** Decodes standard base64 into UTF-8 text. */
function decodeBase64(base64: string): string {
  const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
  return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
}

/** One theme's map from the pack's icon set, or the `UI_BRAND_ICONS_IGNORED` detail when it is not used. */
async function brandMap(backend: BackendHandle, theme: IconTheme, doc: Document): Promise<SymbolMap | string> {
  let asset;
  try {
    asset = await backend.call('brand.get_asset', { asset: { kind: theme === 'light' ? 'icon_set_light' : 'icon_set_dark' } });
  } catch {
    return 'unavailable';
  }
  if (asset.kind !== 'ok') return 'unavailable';
  if (asset.result.media_type === 'image/png') return 'png';
  if (asset.result.media_type !== 'image/svg+xml') return 'unreadable';
  let parsed: Document | null;
  try {
    parsed = parseSvg(decodeBase64(asset.result.base64), doc);
  } catch {
    parsed = null;
  }
  if (parsed === null) return 'unreadable';
  const map = symbolsOf(parsed, doc);
  return map.size === 0 ? 'no named icons' : map;
}

export interface ApplyBrandIconsOptions {
  /** `brand.get`'s answer when the caller already has it; otherwise `brand.get` is called. */
  brand?: Pick<BrandGetResult, 'source'>;
  diagnostics?: DiagnosticSink;
  doc?: Document;
}

/**
 * Builds, for each theme, the map of icon name → the active pack's symbol, and redraws the sprite
 * (Internal Logic step 8). A build pack empties both maps and makes no `brand.get_asset` call; a PNG,
 * unreadable, unnamed or unavailable icon set leaves its theme's map empty and records
 * `UI_BRAND_ICONS_IGNORED` with detail `icon set <theme>: png | unreadable | no named icons | unavailable`.
 */
// spec: ordin-desktop/cross-cutting/VisualDesign#UI_BRAND_ICONS_IGNORED
export async function applyBrandIcons(backend: BackendHandle, options: ApplyBrandIconsOptions = {}): Promise<void> {
  const doc = options.doc ?? (typeof document === 'undefined' ? undefined : document);
  if (doc === undefined) return;
  const state = states.get(doc);
  if (state === undefined) return;
  const generation = ++state.generation;
  let brand = options.brand;
  if (brand === undefined) {
    const answer = await backend.call('brand.get', {});
    if (answer.kind !== 'ok') return;
    brand = answer.result;
  }
  const maps: Record<IconTheme, SymbolMap> = { light: new Map(), dark: new Map() };
  if (brand.source !== 'build') {
    for (const theme of ['light', 'dark'] as const) {
      const map = await brandMap(backend, theme, doc);
      if (typeof map === 'string') options.diagnostics?.record('UI_BRAND_ICONS_IGNORED', `icon set ${theme}: ${map}`, backend.id);
      else maps[theme] = map;
    }
  }
  if (generation !== state.generation) return;
  state.brand = maps;
  draw(doc, state, true);
}
