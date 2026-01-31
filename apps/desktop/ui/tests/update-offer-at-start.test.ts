// @vitest-environment happy-dom
// SM-UP-3: at start, after the local backend loads, the shell asks for the update offer once, then every
// 30 minutes. Plan 04, FU-092: the walk's log showed setup.update_offer seven times "at start"; they were
// the previous session's half-hourly asks, kept in the same log file. The window started as main.ts starts
// it, with the update line and What's new both drawn in Needs you, asks once.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import App from '../src/App.svelte';
import { startApp } from '../src/lib/app/bootstrap';
import '../src/lib/shell/load-registrations';
import { UPDATE_POLL_MS } from '../src/lib/views/Settings/updates.svelte';
import { FakeTransport, ok, settle } from './support/fake-backend';

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
  vi.useRealTimers();
});

describe('SM-UP-3 the update offer at start', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-UP-3
  it('FU-092 — one setup.update_offer per start, and one more each 30 minutes', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
    const transport = new FakeTransport();
    transport.on('setup.update_offer', ok({ offer: null })).on('setup.whats_new', ok({ whats_new: null }));
    const app = await startApp(transport, { document: null, storage: null });
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(App, { target, props: { app } });
    for (let i = 0; i < 6; i++) {
      await settle();
      flushSync();
    }
    expect(transport.sentOf('setup.update_offer')).toHaveLength(1);
    expect(transport.sentOf('setup.whats_new')).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(UPDATE_POLL_MS);
    await settle();
    expect(transport.sentOf('setup.update_offer')).toHaveLength(2);
  });
});
