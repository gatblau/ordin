import { render } from 'svelte/server';
import type { Component } from 'svelte';

import { startApp } from '../../src/lib/app/bootstrap';
import { i18n } from '../../src/lib/i18n/fluent';
import { APP_KEY, type AppContext } from '../../src/lib/shell/context';
import { EN_GB_FTL, FakeTransport } from './fake-backend';

/** Loads the real en-GB catalogue into the window's own `i18n`, as `startApp` does through the core. */
export function loadWindowCatalogue(): void {
  i18n.load({ locale: 'en-GB', direction: 'ltr', resources: [{ locale: 'en-GB', ftl: EN_GB_FTL }] });
  i18n.setBrand({ strings: { product_name: 'ordin', coordinator_name: 'Ordin', about_text: '', legal_text: '', locale: 'en-GB' } });
}

/** Renders a component to HTML with the app context, with no DOM. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function html(component: Component<any>, props: Record<string, unknown> = {}, app?: AppContext): string {
  const context = app === undefined ? undefined : new Map<unknown, unknown>([[APP_KEY, app]]);
  return render(component, { props, ...(context === undefined ? {} : { context }) }).body;
}

export async function appWith(setup: (t: FakeTransport) => void = () => undefined): Promise<{ app: AppContext; transport: FakeTransport }> {
  const transport = new FakeTransport();
  setup(transport);
  const data = new Map<string, string>();
  const storage = { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
  const app = await startApp(transport, { document: null, storage, i18n });
  return { app, transport };
}
