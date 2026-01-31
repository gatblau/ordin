// Catalogue loading, t(), the brand overlay and the locale reload (SA-I18N-01, SA-RECON-03, SM-LG-1).
// Words come from the core with `localisation.get_catalogue`; nothing is bundled at build time.
//
// spec: ordin-desktop/components/ScreensModule#SA-RECON-03
import { FluentBundle, FluentResource, type FluentVariable } from '@fluent/bundle';
import { createSubscriber } from 'svelte/reactivity';

import type { BackendHandle, DiagnosticSink } from '../backend/backend';
import type { ActiveLocale, BrandGetResult, CatalogueText } from '../service-api/types.gen';

export type MessageArgs = Record<string, FluentVariable>;

/** Token names a brand pack may not override: approval-card structure, danger and waiting colours (chosen here: by prefix). */
/** Token names whose colours the application keeps fixed (ADR-041); Brand refuses them first. */
const PROTECTED_TOKEN = /^color\.(danger|waiting|run-state|approval)\./;
/** The brand tokens the screens apply (cross-cutting/VisualDesign.md P2). */
const BRAND_TOKEN = /^(?:color\.([a-z0-9-]+)\.(light|dark)|font\.body|(space)\.(sm|md)|(radius)\.(md|lg))$/;
/** The required and optional colour roles of a brand pack (Brand.md, amendment January 2026). */
const COLOUR_ROLES = new Set([
  'text', 'text-muted', 'background', 'surface', 'accent', 'on-accent', 'border-control', 'focus',
  'text-secondary', 'accent-ink', 'accent-soft', 'chip', 'line', 'line-strong', 'primary', 'on-primary',
]);

/** Fluent wraps arguments in U+2068 … U+2069 for bidirectional text; tests compare without them. */
export function stripIsolates(text: string): string {
  return text.replace(/[⁨⁩]/g, '');
}

export class I18n {
  #bundles: FluentBundle[] = [];
  #missing = new Set<string>();
  #product = 'ordin';
  #coordinator = '';
  #diagnostics: DiagnosticSink | undefined;
  #notify: (() => void) | undefined;
  #subscribe: () => void;
  locale = 'en-GB';
  direction: 'ltr' | 'rtl' = 'ltr';

  constructor(diagnostics?: DiagnosticSink) {
    this.#diagnostics = diagnostics;
    // `t()` called inside an effect or a template re-runs when the catalogue or the brand changes.
    this.#subscribe = createSubscriber((update) => {
      this.#notify = update;
      return () => {
        this.#notify = undefined;
      };
    });
  }

  /** Builds one bundle per returned resource, in `fallback_chain` order (the first is the active locale). */
  load(catalogue: CatalogueText): void {
    this.#bundles = catalogue.resources.map((r) => {
      const bundle = new FluentBundle(r.locale);
      bundle.addResource(new FluentResource(r.ftl));
      return bundle;
    });
    this.#missing.clear();
    this.locale = catalogue.locale;
    this.direction = catalogue.direction;
    this.#notify?.();
  }

  /** The brand's strings: `{ $product }` and `{ $coordinator }` in any message take these values. */
  setBrand(result: Pick<BrandGetResult, 'strings'>): void {
    this.#product = result.strings.product_name;
    this.#coordinator = result.strings.coordinator_name;
    this.#notify?.();
  }

  get product(): string {
    this.#subscribe();
    return this.#product;
  }

  t(id: string, args?: MessageArgs): string {
    this.#subscribe();
    const full: MessageArgs = { product: this.#product, coordinator: this.#coordinator, ...args };
    for (let i = 0; i < this.#bundles.length; i++) {
      const bundle = this.#bundles[i];
      const message = bundle?.getMessage(id);
      if (bundle === undefined || message?.value === undefined || message.value === null) continue;
      if (i > 0) this.#recordMissing(id);
      // With an errors list Fluent never throws: a variable the caller did not give is drawn as `{$name}` and
      // the message is recorded. Without one it throws, and an uncaught throw while drawing took the window
      // down (plan 04: a tab restored with no title drew `lifecycle-title` with no `$name`).
      const errors: Error[] = [];
      const text = bundle.formatPattern(message.value, full, errors);
      if (errors.length > 0) this.#recordMissing(id);
      return text;
    }
    this.#recordMissing(id);
    return id;
  }

  has(id: string): boolean {
    return this.#bundles.some((b) => b.hasMessage(id));
  }

  #recordMissing(id: string): void {
    if (this.#missing.has(id)) return;
    this.#missing.add(id);
    this.#diagnostics?.record('UI_MESSAGE_MISSING', id);
  }
}

/** Reads the active locale and its catalogue from the local backend (Internal Logic step 1). */
export async function loadLocale(i18n: I18n, local: BackendHandle, locale?: ActiveLocale): Promise<ActiveLocale | null> {
  let active = locale;
  if (active === undefined) {
    const settings = await local.call('localisation.get_settings', {});
    if (settings.kind !== 'ok') return null;
    active = settings.result.active;
  }
  const catalogue = await local.call('localisation.get_catalogue', { locale: active.locale });
  if (catalogue.kind !== 'ok') return null;
  i18n.load(catalogue.result);
  return active;
}

export async function loadBrand(i18n: I18n, local: BackendHandle): Promise<BrandGetResult | null> {
  const brand = await local.call('brand.get', {});
  if (brand.kind !== 'ok') return null;
  i18n.setBrand(brand.result);
  return brand.result;
}

/**
 * The brand tokens the screens apply, as the `--brand-*` custom properties `tokens.css` reads
 * (cross-cutting/VisualDesign.md P2). A token the screens do not map, or one naming a fixed colour,
 * is not applied and is recorded as `UI_BRAND_TOKEN_IGNORED` with its name only.
 */
// spec: ordin-desktop/cross-cutting/VisualDesign#UI_BRAND_TOKEN_IGNORED
export function brandTokens(tokens: Record<string, string>, diagnostics?: DiagnosticSink): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const [name, value] of Object.entries(tokens)) {
    const match = PROTECTED_TOKEN.test(name) ? null : BRAND_TOKEN.exec(name);
    const role = match?.[1];
    if (match === null || (role !== undefined && !COLOUR_ROLES.has(role))) {
      diagnostics?.record('UI_BRAND_TOKEN_IGNORED', name);
      continue;
    }
    if (role !== undefined) out.push([`--brand-color-${role}-${match[2]}`, value]);
    else if (name === 'font.body') out.push(['--brand-font-body', value === 'system-ui' ? value : `"${value}"`]);
    else out.push([`--brand-${match[3] ?? match[5]}-${match[4] ?? match[6]}`, value]);
  }
  return out;
}

export interface DocumentLike {
  documentElement: {
    lang: string;
    dir: string;
    style: { setProperty(name: string, value: string): void; removeProperty?(name: string): string };
  };
}

/** Sets `<html lang>` and `dir` from the active locale. */
export function applyLocaleToDocument(active: Pick<ActiveLocale, 'locale' | 'direction'>, doc: DocumentLike | undefined = globalDocument()): void {
  if (doc === undefined) return;
  doc.documentElement.lang = active.locale;
  doc.documentElement.dir = active.direction;
}

/** The `--brand-*` properties set on each document, removed before the next pack's are set. */
const appliedTokens = new WeakMap<object, string[]>();

/**
 * Sets the active pack's tokens on `:root` through CSSOM, after removing the previous pack's, so a role
 * the new pack omits returns to its fallback (cross-cutting/VisualDesign.md P2, Internal Logic step 2).
 */
export function applyBrandToDocument(
  tokens: Record<string, string>,
  doc: DocumentLike | undefined = globalDocument(),
  diagnostics?: DiagnosticSink,
): void {
  if (doc === undefined) return;
  const style = doc.documentElement.style;
  for (const name of appliedTokens.get(doc) ?? []) style.removeProperty?.(name);
  const pairs = brandTokens(tokens, diagnostics);
  for (const [name, value] of pairs) style.setProperty(name, value);
  appliedTokens.set(doc, pairs.map(([name]) => name));
}

/** The font faces added for the active pack, removed before the next pack's are added. */
interface FontSetLike {
  add(face: FontFace): unknown;
  delete(face: FontFace): unknown;
}
const addedFaces = new WeakMap<object, FontFace[]>();

/**
 * Adds the active pack's font files under the family its `font.body` names, with the weight range
 * 100–900 (cross-cutting/VisualDesign.md P4). A file that fails to load or decode is skipped and
 * recorded as `UI_BRAND_FONT_FAILED`; text keeps the next family of `--font-body`.
 */
// spec: ordin-desktop/cross-cutting/VisualDesign#UI_BRAND_FONT_FAILED
export async function applyBrandFonts(
  brand: Pick<BrandGetResult, 'design_tokens' | 'font_count'>,
  local: BackendHandle,
  diagnostics?: DiagnosticSink,
  fonts: FontSetLike | undefined = typeof document === 'undefined' ? undefined : document.fonts,
): Promise<void> {
  if (fonts === undefined) return;
  for (const face of addedFaces.get(fonts) ?? []) fonts.delete(face);
  addedFaces.set(fonts, []);
  const family = brand.design_tokens['font.body'];
  if (family === undefined || family === 'system-ui') return;
  const added: FontFace[] = [];
  for (let index = 0; index < brand.font_count; index += 1) {
    try {
      const asset = await local.call('brand.get_asset', { asset: { kind: 'font', index } });
      if (asset.kind !== 'ok') throw new Error('brand.get_asset');
      const bytes = Uint8Array.from(atob(asset.result.base64), (c) => c.charCodeAt(0));
      const face = new FontFace(family, bytes.buffer, { weight: '100 900', style: 'normal' });
      await face.load();
      fonts.add(face);
      added.push(face);
    } catch {
      diagnostics?.record('UI_BRAND_FONT_FAILED', `font ${index}`);
    }
  }
  addedFaces.set(fonts, added);
}

function globalDocument(): DocumentLike | undefined {
  return typeof document === 'undefined' ? undefined : (document as unknown as DocumentLike);
}

/** The window's own instance; views import `t`. Tests make their own `I18n`. */
export const i18n = new I18n();
export const t = (id: string, args?: MessageArgs): string => i18n.t(id, args);
