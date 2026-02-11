// @vitest-environment happy-dom
// SM-QT-1: once ordin starts closing, the window shows one line until it goes.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';

import App from '../src/App.svelte';
import { startApp } from '../src/lib/app/bootstrap';
import '../src/lib/shell/load-registrations';
import { FakeTransport, settle } from './support/fake-backend';

let mounted: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (mounted !== null) unmount(mounted);
  mounted = null;
  document.body.innerHTML = '';
});

async function closing(running: number): Promise<HTMLElement | null> {
  const transport = new FakeTransport();
  const app = await startApp(transport, { storage: null });
  const target = document.createElement('div');
  document.body.appendChild(target);
  mounted = mount(App, { target, props: { app } });
  await settle(20);
  expect(target.querySelector('.quitting')).toBeNull();
  transport.desktop('quitting', { running_sessions: running });
  flushSync();
  return target.querySelector('.quitting');
}

describe('Closing says it is closing (SM-QT-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-QT-1
  it('Happy path — with agent sessions running, it says they are being stopped', async () => {
    const line = await closing(2);
    expect(line?.getAttribute('role')).toBe('status');
    expect(line?.textContent?.replace(/[⁨⁩]/g, '')).toContain('stopping agent sessions, please wait…');
  });

  it('Edge case — with none running, it says only that it is closing', async () => {
    const line = await closing(0);
    const text = line?.textContent?.replace(/[⁨⁩]/g, '') ?? '';
    expect(text).toContain('Closing');
    expect(text).not.toContain('agent sessions');
  });
});
