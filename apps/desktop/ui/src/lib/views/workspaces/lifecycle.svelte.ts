// Renaming, archiving and the Bin for the local backend's workspaces (ScreensModule.md, amendment January
// 2026, SM-WS-2 to SM-WS-8). One model per backend, shared by the tree's ⋯ menu, the rail's foot and the
// Archived and Bin screens, so a count drawn in the rail follows a change made on a screen.
//
// spec: ordin-desktop/components/ScreensModule#SM-WS-2
import { SvelteMap } from 'svelte/reactivity';

import type { ServiceError } from '../../backend/backend';
import { answers, answersAll } from '../../backend/groups';
import { t } from '../../i18n/fluent';
import { formatList } from '../../format/intl';
import type { BinImpact, Workspace } from '../../service-api/types.gen';
import { NEEDS_YOU_TAB, type ShellState } from '../../shell/shell-state.svelte';
import type { BackendState } from '../../store/backend-state.svelte';

export const ARCHIVED_SCREEN = 'archived';
export const BIN_SCREEN = 'bin';
/** WorkspaceService.md: a name is 1–80 characters after trimming. */
export const WORKSPACE_NAME_MAX = 80;

/** Where a lifecycle action was chosen: its dialog and its error line are drawn there. */
export type LifecyclePlace = 'rail' | 'archived' | 'bin';

export type Asking =
  | { kind: 'bin'; from: LifecyclePlace; workspace: Workspace; impact: BinImpact }
  | { kind: 'delete'; from: LifecyclePlace; workspace: Workspace; impact: BinImpact }
  | { kind: 'empty'; from: LifecyclePlace; names: string[] };

export class WorkspaceLifecycle {
  readonly #backend: BackendState;
  #shell: ShellState | null;

  archived = $state.raw<Workspace[]>([]);
  binned = $state.raw<Workspace[]>([]);
  /** The workspace whose Rename… form is open (SM-WS-3). */
  renaming = $state<string | null>(null);
  renameError = $state<ServiceError | null>(null);
  /** The error line under the tree (SM-WS-4, SM-WS-6 chosen from the tree). */
  railError = $state<ServiceError | null>(null);
  /** Each row's error line on the Archived and Bin screens, by workspace id. */
  rowErrors = new SvelteMap<string, ServiceError>();
  /** The confirmation open, and the error line inside it after a refused verb. */
  asking = $state<Asking | null>(null);
  askError = $state<ServiceError | null>(null);
  busy = $state(false);
  /** SM-WS-6: the line under the tree after Move to Bin, with Undo. */
  binnedLine = $state<{ workspace: Workspace } | null>(null);

  constructor(backend: BackendState, shell: ShellState | null) {
    this.#backend = backend;
    this.#shell = shell;
  }

  /** The shell, once a component with the app context has made this model. */
  useShell(shell: ShellState | null): void {
    if (shell !== null) this.#shell = shell;
  }

  get offered(): { rename: boolean; archive: boolean; remove: boolean } {
    const b = this.#backend;
    const local = b.kind === 'local';
    return {
      rename: local && answers(b, 'workspaces.rename'),
      archive: local && answers(b, 'workspaces.archive'),
      remove: local && answersAll(b, ['workspaces.bin_impact', 'workspaces.move_to_bin']),
    };
  }

  #announce(text: string): void {
    this.#shell?.announce(text);
  }

  #clearLines(): void {
    this.railError = null;
    this.binnedLine = null;
    this.rowErrors.clear();
  }

  /** Reads the active, archived and binned workspaces again. */
  async refresh(): Promise<void> {
    const h = this.#backend.handle;
    const [active, archived, binned] = await Promise.all([
      h.call('workspaces.list', { filter: 'active' }),
      h.call('workspaces.list', { filter: 'archived' }),
      h.call('workspaces.list', { filter: 'in_bin' }),
    ]);
    if (active.kind === 'ok') this.#backend.workspaces = active.result;
    if (archived.kind === 'ok') this.archived = archived.result;
    if (binned.kind === 'ok') this.binned = binned.result;
  }

  /** A workspace that left the active list closes its conversation tab; the window shows no workspace when it was open. */
  #leave(workspaceId: string): void {
    const shell = this.#shell;
    if (shell === null) return;
    const backend = this.#backend.id;
    shell.conversationTabs = shell.conversationTabs.filter((tab) => !(tab.backend === backend && tab.workspaceId === workspaceId));
    if (shell.activeConversation === `${backend}:${workspaceId}`) shell.activeConversation = NEEDS_YOU_TAB;
    if (shell.openBackend === backend && shell.openWorkspace === workspaceId) shell.openWorkspace = null;
  }

  // ---- SM-WS-3 Rename… ------------------------------------------------------------------------

  // spec: ordin-desktop/components/ScreensModule#SM-WS-3
  startRename(workspaceId: string): void {
    this.#clearLines();
    this.renameError = null;
    this.renaming = workspaceId;
  }

  cancelRename(): void {
    this.renaming = null;
    this.renameError = null;
  }

  /** True when `name` may be sent for `current`: not empty after trimming, and not the same name. */
  static canRename(current: string, name: string): boolean {
    const trimmed = name.trim();
    return trimmed !== '' && trimmed !== current;
  }

  async rename(workspaceId: string, name: string): Promise<boolean> {
    const trimmed = name.trim();
    if (trimmed === '' || this.busy) return false;
    this.busy = true;
    try {
      const out = await this.#backend.handle.call('workspaces.rename', { workspace_id: workspaceId, name: trimmed });
      if (out.kind !== 'ok') {
        this.renameError = out.error;
        return false;
      }
      this.renaming = null;
      this.renameError = null;
      await this.refresh();
      this.#announce(t('ws-renamed', { name: out.result.name }));
      return true;
    } finally {
      this.busy = false;
    }
  }

  // ---- SM-WS-4 Archive, SM-WS-5 Restore ---------------------------------------------------------

  // spec: ordin-desktop/components/ScreensModule#SM-WS-4
  async archive(workspace: Workspace): Promise<boolean> {
    this.#clearLines();
    const out = await this.#backend.handle.call('workspaces.archive', { workspace_id: workspace.id });
    if (out.kind !== 'ok') {
      this.railError = out.error;
      return false;
    }
    this.#leave(workspace.id);
    await this.refresh();
    this.#announce(t('ws-archived', { name: workspace.name }));
    return true;
  }

  // spec: ordin-desktop/components/ScreensModule#SM-WS-5
  async restore(workspace: Workspace): Promise<boolean> {
    this.#clearLines();
    const out = await this.#backend.handle.call('workspaces.restore', { workspace_id: workspace.id });
    if (out.kind !== 'ok') {
      this.rowErrors.set(workspace.id, out.error);
      return false;
    }
    await this.refresh();
    this.#announce(t('ws-restored', { name: workspace.name }));
    return true;
  }

  // ---- SM-WS-6 Remove workspace… ------------------------------------------------------------------

  // spec: ordin-desktop/components/ScreensModule#SM-WS-6
  async askBin(workspace: Workspace, from: LifecyclePlace): Promise<boolean> {
    this.#clearLines();
    const impact = await this.#backend.handle.call('workspaces.bin_impact', { workspace_id: workspace.id });
    if (impact.kind !== 'ok') {
      if (from === 'rail') this.railError = impact.error;
      else this.rowErrors.set(workspace.id, impact.error);
      return false;
    }
    this.askError = null;
    this.asking = { kind: 'bin', from, workspace, impact: impact.result };
    return true;
  }

  // ---- SM-WS-7 The Bin, SM-WS-8 Deleting from the Bin ----------------------------------------------

  /** The rail's counts: the archived and binned workspaces (SM-WS-5, SM-WS-7). */
  async loadCounts(): Promise<void> {
    const h = this.#backend.handle;
    const [archived, binned] = await Promise.all([h.call('workspaces.list', { filter: 'archived' }), h.call('workspaces.list', { filter: 'in_bin' })]);
    if (archived.kind === 'ok') this.archived = archived.result;
    if (binned.kind === 'ok') this.binned = binned.result;
  }

  // spec: ordin-desktop/components/ScreensModule#SM-WS-7
  async loadBin(): Promise<void> {
    const out = await this.#backend.handle.call('workspaces.list', { filter: 'in_bin' });
    if (out.kind === 'ok') this.binned = out.result;
  }

  async restoreFromBin(workspace: Workspace, place: LifecyclePlace = 'bin'): Promise<boolean> {
    this.#clearLines();
    const out = await this.#backend.handle.call('workspaces.restore_from_bin', { workspace_id: workspace.id });
    if (out.kind !== 'ok') {
      if (place === 'rail') this.railError = out.error;
      else this.rowErrors.set(workspace.id, out.error);
      return false;
    }
    await this.refresh();
    this.#announce(t('bin-restored', { name: workspace.name }));
    return true;
  }

  // spec: ordin-desktop/components/ScreensModule#SM-WS-8
  async askDelete(workspace: Workspace): Promise<boolean> {
    this.#clearLines();
    const impact = await this.#backend.handle.call('workspaces.bin_impact', { workspace_id: workspace.id });
    if (impact.kind !== 'ok') {
      this.rowErrors.set(workspace.id, impact.error);
      return false;
    }
    this.askError = null;
    this.asking = { kind: 'delete', from: 'bin', workspace, impact: impact.result };
    return true;
  }

  askEmpty(): void {
    this.#clearLines();
    this.askError = null;
    this.asking = { kind: 'empty', from: 'bin', names: this.binned.map((w) => w.name) };
  }

  cancel(): void {
    this.asking = null;
    this.askError = null;
  }

  /** The confirmation's verb: Move to Bin, Delete or Empty Bin. A refusal keeps the dialog open with its error line. */
  async confirm(): Promise<boolean> {
    const asking = this.asking;
    if (asking === null || this.busy) return false;
    this.busy = true;
    try {
      const h = this.#backend.handle;
      if (asking.kind === 'bin') {
        const out = await h.call('workspaces.move_to_bin', { workspace_id: asking.workspace.id });
        if (out.kind !== 'ok') return this.#refused(out.error);
        this.asking = null;
        this.#leave(asking.workspace.id);
        await this.refresh();
        this.binnedLine = { workspace: asking.workspace };
        this.#announce(t('ws-binned', { name: asking.workspace.name }));
      } else if (asking.kind === 'delete') {
        const out = await h.call('workspaces.delete_from_bin', { workspace_id: asking.workspace.id });
        if (out.kind !== 'ok') return this.#refused(out.error);
        this.asking = null;
        await this.loadBin();
        this.#announce(t('bin-deleted', { name: asking.workspace.name }));
      } else {
        const out = await h.call('workspaces.empty_bin', {});
        if (out.kind !== 'ok') return this.#refused(out.error);
        this.asking = null;
        await this.loadBin();
        this.#announce(t('bin-emptied', { count: out.result.deleted }));
      }
      this.askError = null;
      return true;
    } finally {
      this.busy = false;
    }
  }

  #refused(error: ServiceError): false {
    this.askError = error;
    return false;
  }

  /** Undo on the line after Move to Bin (SM-WS-6): the workspace comes back from the Bin. */
  async undoBin(): Promise<boolean> {
    const line = this.binnedLine;
    if (line === null) return false;
    return this.restoreFromBin(line.workspace, 'rail');
  }
}

/** The words of an open confirmation (SM-WS-6, SM-WS-8). */
export function askingWords(asking: Asking): { title: string; body: string; extra: string | null; verb: string } {
  if (asking.kind === 'bin') {
    return {
      title: t('ws-bin-title'),
      body: t('ws-bin-body'),
      extra: asking.impact.unsent_changes > 0 ? t('ws-bin-unsent', { count: asking.impact.unsent_changes, documents: formatList(asking.impact.documents) }) : null,
      verb: t('ws-bin-verb'),
    };
  }
  if (asking.kind === 'delete') {
    return {
      title: t('bin-delete-title', { name: asking.workspace.name }),
      body: t('bin-delete-body'),
      extra: asking.impact.unsent_changes > 0 ? t('bin-delete-unsent', { count: asking.impact.unsent_changes, documents: formatList(asking.impact.documents) }) : null,
      verb: t('bin-delete-verb'),
    };
  }
  return {
    title: t('bin-empty-title'),
    body: t('bin-empty-body', { count: asking.names.length, names: formatList(asking.names) }),
    extra: null,
    verb: t('bin-empty-verb'),
  };
}

const models = new WeakMap<BackendState, WorkspaceLifecycle>();

/** The one model of a backend; `shell` is taken from the first caller that has the app context. */
export function lifecycleOf(backend: BackendState, shell: ShellState | null): WorkspaceLifecycle {
  let model = models.get(backend);
  if (model === undefined) {
    model = new WorkspaceLifecycle(backend, shell);
    models.set(backend, model);
  } else model.useShell(shell);
  return model;
}
