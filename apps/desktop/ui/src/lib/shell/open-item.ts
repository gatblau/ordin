// Opening a thing the way *Needs you*, Home and notifications do: switch the window to its workspace,
// then bring its screen to the front. Opening never deletes and never moves the person's place unasked
// (`inFront: false` for a screen Ordin opened).
import type { BackendId } from '../backend/backend';
import type { OpenTarget } from '../service-api/types.gen';
import type { AppContext } from './context';
import { ScreenAreaState } from './screen-area.svelte';
import { screenKeyFor } from './screens';

export function openScreen(app: AppContext, backend: BackendId, workspaceId: string | null, key: string, options: { title?: string; inFront?: boolean } = {}): void {
  if (workspaceId !== null) {
    app.shell.openWorkspaceTab(backend, workspaceId);
    app.screens.open(ScreenAreaState.scope(backend, workspaceId), key, options);
    return;
  }
  // SM-OS-1: a screen with no workspace (Settings, a language, Home) opens where the person is, in the
  // screen area they see. It went to the no-workspace area while a workspace was open, so it opened out of
  // sight and the click seemed to do nothing. Another backend's screen switches the window to that backend.
  // spec: ordin-desktop/components/ScreensModule#SM-OS-1
  if (app.shell.openBackend !== backend) {
    app.shell.openBackend = backend;
    app.shell.openWorkspace = null;
  }
  app.screens.open(ScreenAreaState.scope(backend, app.shell.openWorkspace), key, options);
}

/** Opens an `OpenTarget`: its workspace and, when it has a screen, that screen. Returns the key opened, or `null`. */
export function openTarget(app: AppContext, backend: BackendId, workspaceId: string | null, target: OpenTarget): string | null {
  const key = target.kind === 'screen' ? target.name : screenKeyFor(target);
  const workspace = workspaceId ?? ('workspace_id' in target ? target.workspace_id : null) ?? null;
  if (key === null) {
    if (workspace !== null) app.shell.openWorkspaceTab(backend, workspace);
    return null;
  }
  openScreen(app, backend, workspace, key);
  return key;
}
