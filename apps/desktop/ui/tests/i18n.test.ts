import { describe, expect, it } from 'vitest';

import { createHandle } from '../src/lib/backend/backend';
import { applyBrandToDocument, applyLocaleToDocument, brandTokens, I18n, loadBrand, loadLocale, stripIsolates } from '../src/lib/i18n/fluent';
import { Diagnostics } from '../src/lib/store/diagnostics.svelte';
import { EN_GB_FTL, FakeTransport, LOCAL, ok } from './support/fake-backend';

const en = { locale: 'en-GB', direction: 'ltr', resources: [{ locale: 'en-GB', ftl: EN_GB_FTL }] } as const;

function loaded(chain: Array<{ locale: string; ftl: string }> = [{ locale: 'en-GB', ftl: EN_GB_FTL }]) {
  const diagnostics = new Diagnostics();
  const i18n = new I18n(diagnostics);
  i18n.load({ locale: chain[0]?.locale ?? 'en-GB', direction: 'ltr', resources: chain });
  return { i18n, diagnostics };
}
const t = (i18n: I18n, id: string, args?: Record<string, string | number>) => stripIsolates(i18n.t(id, args));

describe('t()', () => {
  it('draws a message with arguments and the brand product', () => {
    const { i18n } = loaded();
    expect(t(i18n, 'rail-server-on', { server: 'Northwind server' })).toBe('On Northwind server');
    expect(t(i18n, 'quit-title')).toBe('Quit ordin?');
    i18n.setBrand({ strings: { product_name: 'Acme Copilot', coordinator_name: 'Aria', about_text: '', legal_text: '', locale: 'en-GB' } });
    expect(t(i18n, 'quit-title')).toBe('Quit Acme Copilot?');
  });

  it('a message missing in the active locale draws the en-GB text and records UI_MESSAGE_MISSING once', () => {
    const { i18n, diagnostics } = loaded([
      { locale: 'fr-FR', ftl: 'rail-home = Accueil\n' },
      { locale: 'en-GB', ftl: EN_GB_FTL },
    ]);
    expect(t(i18n, 'rail-home')).toBe('Accueil');
    expect(t(i18n, 'conv-needs-you')).toBe('Needs you');
    expect(t(i18n, 'conv-needs-you')).toBe('Needs you');
    expect(diagnostics.entries.filter((e) => e.code === 'UI_MESSAGE_MISSING').map((e) => e.detail)).toEqual(['conv-needs-you']);
  });

  // Plan 04: Fluent throws on a variable not given unless it has an errors list, and a throw while drawing took
  // the window down (a tab restored with no title drew `lifecycle-title` with no `$name`).
  it('a message drawn without one of its variables does not throw: it draws the rest and is recorded', () => {
    const { i18n, diagnostics } = loaded();
    expect(() => i18n.t('lifecycle-title')).not.toThrow();
    expect(t(i18n, 'lifecycle-title')).toContain('Lifecycle:');
    expect(diagnostics.entries.filter((e) => e.code === 'UI_MESSAGE_MISSING').map((e) => e.detail)).toEqual(['lifecycle-title']);
  });

  it('a message in no catalogue draws its id, never an empty item', () => {
    const { i18n, diagnostics } = loaded();
    expect(i18n.t('no-such-message')).toBe('no-such-message');
    expect(diagnostics.codes()).toEqual(['UI_MESSAGE_MISSING']);
  });

  it('Fluent plurals follow the active locale', () => {
    const { i18n } = loaded([{ locale: 'en-GB', ftl: 'n = { $count -> \n [one] { $count } item\n *[other] { $count } items\n}\n' }]);
    expect(t(i18n, 'n', { count: 1 })).toBe('1 item');
    expect(t(i18n, 'n', { count: 3 })).toBe('3 items');
  });

  it('has() reflects the catalogue', () => {
    const { i18n } = loaded();
    expect(i18n.has('rail-home')).toBe(true);
    expect(i18n.has('zzz')).toBe(false);
  });
});

describe('locale and brand loading', () => {
  it('Happy path — a locale change redraws the window: lang and dir follow, strings come from the new catalogue', async () => {
    const transport = new FakeTransport();
    transport.on('localisation.get_catalogue', (input) => {
      const locale = (input as { locale: string }).locale;
      return ok({ locale, direction: 'ltr', resources: locale === 'fr-FR' ? [{ locale, ftl: 'rail-home = Accueil\n' }, { locale: 'en-GB', ftl: EN_GB_FTL }] : [{ locale, ftl: EN_GB_FTL }] });
    });
    const handle = createHandle({ id: LOCAL, kind: 'local', transport });
    const i18n = new I18n();
    await loadLocale(i18n, handle);
    expect(i18n.t('rail-home')).toBe('Home');
    const doc = { documentElement: { lang: 'en-GB', dir: 'ltr', style: { setProperty() {} } } };
    const active = await loadLocale(i18n, handle, { locale: 'fr-FR', source: 'person', direction: 'ltr', fallback_chain: ['fr-FR', 'en-GB'] });
    expect(active?.locale).toBe('fr-FR');
    if (active) applyLocaleToDocument(active, doc);
    expect(doc.documentElement.lang).toBe('fr-FR');
    expect(doc.documentElement.dir).toBe('ltr');
    expect(i18n.t('rail-home')).toBe('Accueil');
    // The request went to the core for the new locale, not to a bundled file.
    expect(transport.sentOf('localisation.get_catalogue').map((e) => (e.input as { locale: string }).locale)).toEqual(['en-GB', 'fr-FR']);
  });

  it('a right-to-left locale sets dir', () => {
    const doc = { documentElement: { lang: '', dir: '', style: { setProperty() {} } } };
    applyLocaleToDocument({ locale: 'ar-SA', direction: 'rtl' }, doc);
    expect(doc.documentElement.dir).toBe('rtl');
  });

  it('the brand sets the product name and its design tokens, except the protected ones', async () => {
    const transport = new FakeTransport();
    transport.on('brand.get', ok({
      brand_identifier: 'acme', version: '2', source: 'organisation_profile', font_count: 0, links: { help_url: '', privacy_url: '' }, refusal: null,
      design_tokens: { 'color.accent.light': '#123456', 'color.approval.light': '#000000', 'color.danger.dark': '#FFFFFF', 'font.heading': 'Acme Serif' },
      strings: { product_name: 'Acme', coordinator_name: 'Aria', about_text: '', legal_text: '', locale: 'en-GB' },
    }));
    const handle = createHandle({ id: LOCAL, kind: 'local', transport });
    const i18n = new I18n();
    i18n.load(en as never);
    const brand = await loadBrand(i18n, handle);
    expect(i18n.product).toBe('Acme');
    const set: Record<string, string> = {};
    const doc = { documentElement: { lang: '', dir: '', style: { setProperty: (n: string, v: string) => void (set[n] = v) } } };
    const recorded: string[] = [];
    applyBrandToDocument(brand?.design_tokens ?? {}, doc, { record: (code, detail) => void recorded.push(`${code} ${detail ?? ''}`) });
    expect(set).toEqual({ '--brand-color-accent-light': '#123456' });
    expect(recorded.sort()).toEqual([
      'UI_BRAND_TOKEN_IGNORED color.approval.light',
      'UI_BRAND_TOKEN_IGNORED color.danger.dark',
      'UI_BRAND_TOKEN_IGNORED font.heading',
    ]);
    expect(brandTokens({ a: '1' })).toEqual([]);
  });
});
