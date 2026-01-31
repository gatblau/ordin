// @vitest-environment happy-dom
// The centre with nothing open (amended January 2026; user decision; Approved by the user January 2026): the
// brand's logo large and faint, the version, and a small Check for updates button (SM-UP-2.1).
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';

import App from '../src/App.svelte';
import { startApp } from '../src/lib/app/bootstrap';
import '../src/lib/shell/load-registrations';
import type { StorageLike } from '../src/lib/store/view-prefs';
import { err, FakeTransport, ok, settle } from './support/fake-backend';

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

async function emptyWindow(logo = true): Promise<{ target: HTMLElement; transport: FakeTransport }> {
  Object.defineProperty(window, 'innerWidth', { value: 1400, configurable: true });
  const data = new Map<string, string>();
  const storage: StorageLike = { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
  const transport = new FakeTransport();
  transport.version = '2.3.4';
  transport
    .on('setup.update_offer', ok({ offer: null }))
    .on('setup.whats_new', ok({ whats_new: null }))
    .on('setup.check_update', ok({ offer: null }))
    .on('brand.get_asset', logo ? ok({ media_type: 'image/svg+xml', base64: btoa('<svg xmlns="http://www.w3.org/2000/svg"></svg>') }) : err('BRAND_ASSET_MISSING', 'no logo'));
  const app = await startApp(transport, { document: null, storage });
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(App, { target, props: { app } });
  const step = async (): Promise<void> => {
    for (let i = 0; i < 4; i++) {
      await settle();
      flushSync();
    }
  };
  await step();
  // Close every tab Home opened with.
  for (let close = target.querySelector<HTMLButtonElement>('.tab-close'); close !== null; close = target.querySelector<HTMLButtonElement>('.tab-close')) {
    close.click();
    flushSync();
  }
  await step();
  return { target, transport };
}

describe('the empty centre', () => {
  it('Happy path — a faint logo, the version and Check for updates; the old words are gone', async () => {
    const { target } = await emptyWindow();
    const centre = target.querySelector('.empty-centre')!;
    expect(centre).not.toBeNull();
    const logo = centre.querySelector<HTMLImageElement>('.watermark img.logo-light')!;
    expect(logo.src.startsWith('data:image/svg+xml;base64,')).toBe(true);
    expect(logo.alt).toBe('');
    expect(centre.querySelector('.watermark')!.getAttribute('aria-hidden')).toBe('true');
    expect(centre.querySelector('.version')!.textContent).toContain('2.3.4');
    expect(centre.querySelector('button.check')!.textContent).toContain('Check for updates');
    expect(target.textContent).not.toContain('Nothing is open');
  });

  it('Happy path — Check for updates asks the core and says so when there is nothing new', async () => {
    const { target, transport } = await emptyWindow();
    target.querySelector<HTMLButtonElement>('.empty-centre button.check')!.click();
    for (let i = 0; i < 4; i++) {
      await settle();
      flushSync();
    }
    expect(transport.sent.some((c) => c.command === 'setup.check_update')).toBe(true);
    expect(target.querySelector('.empty-centre .status')!.textContent).toContain('up to date');
  });

  it('Error path — a brand without a readable logo draws no image, and the rest still shows', async () => {
    const { target } = await emptyWindow(false);
    expect(target.querySelector('.empty-centre img')).toBeNull();
    expect(target.querySelector('.empty-centre .version')).not.toBeNull();
    expect(target.querySelector('.empty-centre button.check')).not.toBeNull();
  });
});
