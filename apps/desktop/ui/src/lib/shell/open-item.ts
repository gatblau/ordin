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
export function openTarget(app: AppContext, backend: BackendId, workspaceId: string | null, target: OpenTarget, title?: string): string | null {
  const key = target.kind === 'screen' ? target.name : screenKeyFor(target);
  const workspace = workspaceId ?? ('workspace_id' in target ? target.workspace_id : null) ?? null;
  if (key === null) {
    if (workspace !== null) app.shell.openWorkspaceTab(backend, workspace);
    return null;
  }
  openScreen(app, backend, workspace, key, title === undefined ? {} : { title });
  return key;
}

/**
 * `ordin://open-target` (OrdinDesktop.md DSK-NTF-1): the window gained focus after a notification, so the
 * item it was about is opened: an approval on its card, a run or a crew on its screen, a question in its
 * workspace's conversation. Anything else is ignored. Returns the screen key opened, the workspace for a
 * question, or `null`.
 *
 * spec: ordin-desktop/components/OrdinDesktop#DSK-NTF-1
 */
export function openNotificationTarget(app: AppContext, payload: unknown): string | null {
  if (typeof payload !== 'object' || payload === null) return null;
  const p = payload as Record<string, unknown>;
  const str = (k: string): string | null => (typeof p[k] === 'string' && p[k] !== '' ? (p[k] as string) : null);
  const backend = str('backend') as BackendId | null;
  if (backend === null) return null;
  const workspace = str('workspace_id');
  switch (p.kind) {
    case 'approval': {
      const id = str('approval_id');
      if (id === null || workspace === null) return null;
      return openTarget(app, backend, workspace, { kind: 'approval', approval_id: id, workspace_id: workspace });
    }
    case 'run': {
      const id = str('run_id');
      if (id === null) return null;
      const key = screenKeyFor({ kind: 'run', run_id: id, workspace_id: '' });
      if (key !== null) openScreen(app, backend, null, key);
      return key;
    }
    case 'crew_run': {
      const id = str('crew_run_id');
      if (id === null) return null;
      const key = screenKeyFor({ kind: 'crew_run', crew_run_id: id, workspace_id: '' });
      if (key !== null) openScreen(app, backend, null, key);
      return key;
    }
    case 'question':
      if (workspace === null) return null;
      app.shell.openWorkspaceTab(backend, workspace);
      return workspace;
    default:
      return null;
  }
}
