// Diagnostics (Settings › Diagnostics): the one view that shows technical names — journal diagnostic
// lines, and this window's own diagnostics ring (step 12).
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
// spec: ordin-desktop/components/ScreensModule#SM-NS-3
import type { BackendHandle } from '../backend/backend';
import type { DiagnosticLine, Run, ServiceError } from '../service-api/types.gen';

/** A support export names at most 20 runs (SM-NS-3). */
export const MAX_RUNS = 20;

export class DiagnosticsModel {
  lines = $state.raw<DiagnosticLine[]>([]);
  next = $state<string | null>(null);
  error = $state<ServiceError | null>(null);
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
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

  /** The panel of **Save logs for support…**. */
  panelOpen = $state(false);
  runs = $state.raw<Run[]>([]);
  runsNext = $state<string | null>(null);
  ticked = $state.raw<ReadonlySet<string>>(new Set());
  /** True after a 21st tick was refused. */
  limitShown = $state(false);
  savedPath = $state<string | null>(null);
  exportError = $state<ServiceError | null>(null);

  /** Newest first by `queued_at`. */
  static newestFirst(runs: Run[]): Run[] {
    return [...runs].sort((a, b) => Date.parse(b.queued_at) - Date.parse(a.queued_at));
  }

  async openPanel(): Promise<void> {
    this.panelOpen = true;
    this.savedPath = null;
    this.limitShown = false;
    this.ticked = new Set();
    this.exportError = null;
    const out = await this.#handle.call('runs.list', {});
    if (out.kind === 'ok') {
      this.runs = DiagnosticsModel.newestFirst(out.result.items);
      this.runsNext = out.result.next_cursor ?? null;
    } else this.exportError = out.error;
  }

  async moreRuns(): Promise<void> {
    if (this.runsNext === null) return;
    const out = await this.#handle.call('runs.list', { cursor: this.runsNext });
    if (out.kind === 'ok') {
      this.runs = DiagnosticsModel.newestFirst([...this.runs, ...out.result.items]);
      this.runsNext = out.result.next_cursor ?? null;
    } else this.exportError = out.error;
  }

  /** Unticking always works; the 21st tick stays unticked and shows the limit line. */
  toggle(id: string): void {
    const next = new Set(this.ticked);
    if (next.has(id)) {
      next.delete(id);
      this.limitShown = false;
    } else if (next.size >= MAX_RUNS) {
      this.limitShown = true;
      return;
    } else next.add(id);
    this.ticked = next;
  }

  /** **Save…**: nothing is sent with no tick; `DSK_DIALOG_CANCELLED` draws nothing. */
  async exportSupport(): Promise<boolean> {
    if (this.ticked.size === 0 || this.ticked.size > MAX_RUNS) return false;
    this.exportError = null;
    this.savedPath = null;
    const out = await this.#handle.call('diagnostics.support_export', { run_ids: [...this.ticked] });
    if (out.kind === 'ok') {
      this.savedPath = out.result.saved_path;
      return true;
    }
    if (out.error.code !== 'DSK_DIALOG_CANCELLED') this.exportError = out.error;
    return false;
  }
}
