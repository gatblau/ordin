// Where `openScreen` puts a screen (SM-OS-1): a screen with no workspace opens in the screen area the person
// sees, so Settings chosen with a workspace open is in front of that workspace, not out of sight.
import { describe, expect, it } from 'vitest';

import type { AppContext } from '../src/lib/shell/context';
import { openScreen } from '../src/lib/shell/open-item';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { SCREEN_IDS } from '../src/lib/shell/screens';
import { ShellState } from '../src/lib/shell/shell-state.svelte';
import { LOCAL, SERVER } from './support/fake-backend';
import { uid } from './support/ids';

const WS = uid('ws');

function app(): AppContext {
  return { shell: new ShellState(), screens: new ScreenAreaState() } as unknown as AppContext;
}

describe('openScreen', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-OS-1
  it('Happy path — Settings with a workspace open is in front of that workspace, which stays open (SM-OS-1)', () => {
    const a = app();
    a.shell.openWorkspaceTab(LOCAL, WS);
    openScreen(a, LOCAL, null, SCREEN_IDS.settings);
    expect(a.shell.openWorkspace).toBe(WS);
    expect(a.screens.front(ScreenAreaState.scope(LOCAL, WS))).toBe(SCREEN_IDS.settings);
  });

  it('with no workspace open, the screen opens in the no-workspace area', () => {
    const a = app();
    a.shell.openBackend = LOCAL;
    openScreen(a, LOCAL, null, SCREEN_IDS.settings);
    expect(a.screens.front(ScreenAreaState.scope(LOCAL, null))).toBe(SCREEN_IDS.settings);
  });

  it("another backend's screen switches the window to that backend with no workspace", () => {
    const a = app();
    a.shell.openWorkspaceTab(LOCAL, WS);
    openScreen(a, SERVER, null, 'server-languages');
    expect(a.shell.openBackend).toBe(SERVER);
    expect(a.shell.openWorkspace).toBeNull();
    expect(a.screens.front(ScreenAreaState.scope(SERVER, null))).toBe('server-languages');
  });

  it('a screen of a workspace opens that workspace', () => {
    const a = app();
    openScreen(a, LOCAL, WS, SCREEN_IDS.documents);
    expect(a.shell.openWorkspace).toBe(WS);
    expect(a.screens.front(ScreenAreaState.scope(LOCAL, WS))).toBe(SCREEN_IDS.documents);
  });
});
