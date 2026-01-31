// @vitest-environment happy-dom
// Settings at the foot of the side rail (SM-RAIL-1). Mounted, so pressing it opens the screen.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import SideRail from '../src/lib/shell/SideRail.svelte';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { hello, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const WS = uid('ws');
function draw(app: AppContext): HTMLElement {
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(SideRail as never, { target, props: {} as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  return target;
}

const words = (el: Element | null): string => (el?.textContent ?? '').replace(/[⁨⁩]/g, '').trim();

describe('Settings in the side rail (SM-RAIL-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-RAIL-1
  it('Happy path — Settings from the side rail', async () => {
    const { app } = await appWith((t) => t.on('workspaces.list', ok([{ id: WS, name: 'Supplier review', created_at: 'x', own_folder_place_id: 'p', state: 'active' }])));
    app.shell.openWorkspaceTab(LOCAL, WS);
    const target = draw(app);
    await settle(30);
    flushSync();
    const settings = target.querySelector('button.rail-settings') as HTMLButtonElement;
    expect(words(settings)).toBe('Settings');
    expect(settings.getAttribute('aria-pressed')).toBe('false');
    // It is the last thing in the rail, after the tree and its foot.
    expect(target.querySelector('nav')?.lastElementChild?.contains(settings)).toBe(true);
    settings.click();
    flushSync();
    expect(app.screens.front(ScreenAreaState.scope(LOCAL, WS))).toBe('settings');
    expect(settings.getAttribute('aria-pressed')).toBe('true');
    // The workspace and its conversation stay open.
    expect(app.shell.openWorkspace).toBe(WS);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-RAIL-1
  it('Edge case — a backend that does not answer settings.get', async () => {
    const { app } = await appWith((t) => t.on('service.hello', ok(hello(LOCAL, R1_GROUPS.filter((g) => g !== 'settings')))));
    const target = draw(app);
    await settle(30);
    flushSync();
    expect(target.querySelector('nav')).not.toBeNull();
    expect(target.querySelector('button.rail-settings')).toBeNull();
  });
});
