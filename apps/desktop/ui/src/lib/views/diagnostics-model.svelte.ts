// Diagnostics (Settings › Diagnostics): the one view that shows technical names — journal diagnostic
// lines, and this window's own diagnostics ring (step 12).
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
// spec: ordin-desktop/components/ScreensModule#SM-NS-3
// spec: ordin-desktop/components/ScreensModule#SM-NS-3a
import type { BackendHandle } from '../backend/backend';
import type { DiagnosticLine, Run, ServiceError } from '../service-api/types.gen';

/** A support export names at most 20 runs (SM-NS-3). */
export const MAX_RUNS = 20;

export class DiagnosticsModel {
  lines = $state.raw<DiagnosticLine[]>([]);
  next = $state<string | null>(null);
  error = $state<ServiceError | null>(null);
  /**
   * When **Clear logs** last succeeded in this window (ISO 8601), or `null`. The journal's list is cut by the
   * backend (JournalAndArtefactStore.md JA-DIAG-CLEAR-1); this window's own lines live only here, so those
   * from before the clear are not drawn (SM-NS-3a, amended January 2026; user decision; Approved by the user
   * January 2026).
   */
  clearedAt = $state<string | null>(null);
  readonly #handle: BackendHandle;
  readonly #now: () => string;

  constructor(handle: BackendHandle, now: () => string = () => new Date().toISOString()) {
    this.#handle = handle;
    this.#now = now;
  }

  /** True when this window's line at `at` is after the last clear (or there was none). */
  shownAfterClear(at: string): boolean {
    if (this.clearedAt === null) return true;
    const when = Date.parse(at);
    // A time that does not parse is kept: hiding it would lose a line the person did not clear.
    return Number.isNaN(when) || when > Date.parse(this.clearedAt);
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('diagnostics.lines', {});
    if (out.kind === 'ok') {
      this.lines = out.result.items;
      this.next = out.result.next_cursor ?? null;
      this.error = null;
    } else this.error = out.error;
  }

  async more(): Promise<void> {
    if (this.next === null) return;
    const out = await this.#handle.call('diagnostics.lines', { cursor: this.next });
    if (out.kind === 'ok') {
      this.lines = [...this.lines, ...out.result.items];
      this.next = out.result.next_cursor ?? null;
    } else this.error = out.error;
  }

  // ---- Save logs for support… ----------------------------------------------------------------------

  /** True while **Save logs for support…** is reading the runs and writing the file. */
  saving = $state(false);
  /** True when there was no run to include, so nothing was saved. */
  nothingToSave = $state(false);
  savedPath = $state<string | null>(null);
  exportError = $state<ServiceError | null>(null);

  /** Newest first by `queued_at`. */
  static newestFirst(runs: Run[]): Run[] {
    return [...runs].sort((a, b) => Date.parse(b.queued_at) - Date.parse(a.queued_at));
  }

  /**
   * **Save logs for support…** in one action (amended November 2026; user decision): the newest runs, up to
   * 20, and the last 3 days of logs go to one file, with nothing to tick first. `DSK_DIALOG_CANCELLED`
   * draws nothing.
   */
  async saveForSupport(): Promise<boolean> {
    if (this.saving) return false;
    this.saving = true;
    this.exportError = null;
    this.savedPath = null;
    this.nothingToSave = false;
    try {
      const listed = await this.#handle.call('runs.list', {});
      if (listed.kind !== 'ok') {
        this.exportError = listed.error;
        return false;
      }
      const runIds = DiagnosticsModel.newestFirst(listed.result.items)
        .slice(0, MAX_RUNS)
        .map((r) => r.id);
      if (runIds.length === 0) {
        this.nothingToSave = true;
        return false;
      }
      const out = await this.#handle.call('diagnostics.support_export', { run_ids: runIds });
      if (out.kind === 'ok') {
        this.savedPath = out.result.saved_path;
        return true;
      }
      if (out.error.code !== 'DSK_DIALOG_CANCELLED') this.exportError = out.error;
      return false;
    } finally {
      this.saving = false;
    }
  }

  // ---- Clear logs… (SM-NS-3a) -----------------------------------------------------------------------

  /** The question of **Clear logs…** is open. */
  clearAsked = $state(false);
  /** True once the logs were cleared, for `diagnostics-clear-done`. */
  cleared = $state(false);
  clearError = $state<ServiceError | null>(null);

  askClear(): void {
    this.clearAsked = true;
    this.cleared = false;
    this.clearError = null;
  }

  cancelClear(): void {
    this.clearAsked = false;
    this.clearError = null;
  }

  /** **Clear logs**: a refusal stays in the open dialog under its sentence. */
  async clearLogs(): Promise<boolean> {
    this.clearError = null;
    const out = await this.#handle.call('diagnostics.clear_logs', {});
    if (out.kind === 'ok') {
      this.clearAsked = false;
      this.cleared = true;
      // The backend has emptied the event list from now on (JA-DIAG-CLEAR-1): what was on screen goes, and
      // the list is read again for anything recorded since.
      this.clearedAt = this.#now();
      this.lines = [];
      this.next = null;
      await this.load();
      return true;
    }
    this.clearError = out.error;
    return false;
  }
}
