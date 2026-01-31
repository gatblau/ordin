// The running recording, shared by the Minutes screen and the indicator in the app header (ScreensModule.md
// SM-MIN-7): set from the `minutes.record` answer, cleared by the `minutes.stop` answer or when a
// `minutes.status` read every 5 seconds finds no recording (stopped in OBS). `ended` counts the recordings
// that ended, so the Minutes screen reads its jobs again when one does.
//
// spec: ordin-desktop/components/ScreensModule#SM-MIN-7
import type { BackendHandle, ServiceError } from '../../backend/backend';
import type { RecordingView } from '../../service-api/minutes.gen';
import { RECORDING_POLL_MS } from './minutes-model';

type Handle = Pick<BackendHandle, 'call'>;

export class RecordingState {
  view = $state.raw<RecordingView | null>(null);
  /** Incremented each time a recording ends. */
  ended = $state(0);
  stopping = $state(false);
  error = $state<ServiceError | null>(null);
  #handle: Handle | null = null;
  #timer: ReturnType<typeof setInterval> | null = null;

  /** Called with the `minutes.record` answer. */
  started(handle: Handle, view: RecordingView): void {
    this.#handle = handle;
    this.view = view;
    this.error = null;
    this.#poll();
  }

  /** **Stop**: `minutes.stop { recording_id }`; the indicator stays until the answer. */
  async stop(): Promise<boolean> {
    const view = this.view;
    if (view === null || this.#handle === null || this.stopping) return false;
    this.stopping = true;
    const out = await this.#handle.call('minutes.stop', { recording_id: view.id });
    this.stopping = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.#end();
    return true;
  }

  /** One `minutes.status` read: no recording any more (stopped in OBS) ends the indicator. */
  async check(): Promise<void> {
    const view = this.view;
    if (view === null || this.#handle === null) return;
    const out = await this.#handle.call('minutes.status', { workspace_id: view.workspace_id });
    if (out.kind === 'ok' && (out.result.recording == null || out.result.recording.id !== view.id) && this.view?.id === view.id) this.#end();
  }

  /** Forgets everything (tests). */
  reset(): void {
    this.#stopPoll();
    this.view = null;
    this.error = null;
    this.stopping = false;
    this.#handle = null;
  }

  #end(): void {
    this.#stopPoll();
    this.view = null;
    this.error = null;
    this.ended += 1;
  }

  #poll(): void {
    this.#stopPoll();
    this.#timer = setInterval(() => void this.check(), RECORDING_POLL_MS);
  }

  #stopPoll(): void {
    if (this.#timer !== null) clearInterval(this.#timer);
    this.#timer = null;
  }
}

/** The window's one recording. */
export const recording = new RecordingState();
