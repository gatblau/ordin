// Getting memory ready (ScreensModule.md amendment 2026-01-28, SM-MD-1 to SM-MD-5): the local
// backend's `MemoryModelsView`, kept from `app.memory_models` at start and replaced by each
// `memory_models_changed` event, and the four commands the card sends.
//
// spec: ordin-desktop/components/ScreensModule#SM-MD-1
// spec: ordin-desktop/components/ScreensModule#SM-MD-2
// spec: ordin-desktop/components/ScreensModule#SM-MD-3
// spec: ordin-desktop/components/ScreensModule#SM-MD-4
import type { BackendHandle, EventEnvelope } from '../../backend/backend';
import type { MemoryModelsView, ServiceError } from '../../service-api/types.gen';

/** What the card draws: the question, the progress, the check, the ready line, or the failure. */
export type CardShows = 'ask' | 'downloading' | 'checking' | 'ready' | 'failed' | null;

/** How long the ready line stays (SM-MD-1, SM-MD-3). */
export const READY_SHOWN_MS = 5_000;

/** The catalogue id of a failure code: `error-<code in kebab-case>` (SM-MD-4, EH-SEN-1). */
export const failureMessageId = (code: string): string => `error-${code.toLowerCase().replaceAll('_', '-')}`;

/** `size_bytes` ÷ 1,000,000 rounded to the nearest whole number (SM-MD-2). */
export const sizeMb = (bytes: number): number => Math.round(bytes / 1_000_000);

/** `received_bytes` in whole MB, rounded down (SM-MD-3). */
export const receivedMb = (bytes: number): number => Math.floor(bytes / 1_000_000);

/** `received_bytes` × 100 ÷ `size_bytes`, rounded down (SM-MD-3). */
export const percent = (view: MemoryModelsView): number => (view.size_bytes > 0 ? Math.floor((view.received_bytes * 100) / view.size_bytes) : 0);

export interface MemoryModelsOptions {
  readyMs?: number;
  /** The timer that hides the ready line; replaceable in tests. */
  setTimer?: (run: () => void, ms: number) => unknown;
}

export class MemoryModelsModel {
  view = $state.raw<MemoryModelsView | null>(null);
  /** The person pressed **Download** or **Try again** in this run (SM-MD-1). */
  pressed = $state(false);
  /** `state` became `ready` less than 5 seconds ago (SM-MD-1). */
  justReady = $state(false);
  /** A `ServiceError` answered to one of the four commands, drawn by step 11 inside the card (SM-MD-4). */
  error = $state.raw<ServiceError | null>(null);
  readonly handle: BackendHandle;
  readonly #readyMs: number;
  readonly #setTimer: (run: () => void, ms: number) => unknown;

  /** `handle` is the local backend's: the card is never drawn for a server's conversation (SM-MD-1). */
  constructor(handle: BackendHandle, options: MemoryModelsOptions = {}) {
    this.handle = handle;
    this.#readyMs = options.readyMs ?? READY_SHOWN_MS;
    this.#setTimer = options.setTimer ?? ((run, ms) => setTimeout(run, ms));
  }

  /** What the card draws now; `null` when it is not drawn (SM-MD-1). */
  get shows(): CardShows {
    const view = this.view;
    if (view === null || view.state === 'not_in_build') return null;
    if (view.state === 'downloading') return 'downloading';
    if (view.state === 'checking') return 'checking';
    if (view.state === 'ready') return this.justReady ? 'ready' : null;
    if (view.state === 'failed' && this.pressed) return 'failed';
    if (view.ask) return 'ask';
    return null;
  }

  /** `app.memory_models {}` at start (SM-MD-1); a failure leaves the card undrawn. */
  async load(): Promise<void> {
    const out = await this.handle.call('app.memory_models', {});
    if (out.kind === 'ok') this.#take(out.result);
  }

  /** One event of the local backend's store: `memory_models_changed` replaces the view (SM-MD-1). */
  receive(env: EventEnvelope): void {
    if (env.kind === 'memory_models_changed') this.#take(env.models);
  }

  /** **Download**, **Carry on downloading** or **Try again**: `app.download_memory_models {}`. */
  async download(): Promise<void> {
    this.pressed = true;
    await this.#send('app.download_memory_models');
  }

  /** **Stop**: `app.stop_memory_models_download {}`. */
  async stop(): Promise<void> {
    await this.#send('app.stop_memory_models_download');
  }

  /** **Not now**: `app.decline_memory_models {}`; the card goes when the answer's `ask` is false. */
  async notNow(): Promise<void> {
    this.pressed = false;
    await this.#send('app.decline_memory_models');
  }

  async #send(command: 'app.download_memory_models' | 'app.stop_memory_models_download' | 'app.decline_memory_models'): Promise<void> {
    const out = await this.handle.call(command, {});
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.#take(out.result);
  }

  #take(next: MemoryModelsView): void {
    const before = this.view?.state;
    this.view = next;
    if (next.state === 'ready' && before !== undefined && before !== 'ready') {
      this.justReady = true;
      this.#setTimer(() => (this.justReady = false), this.#readyMs);
    }
    if (next.state !== 'failed' && next.state !== 'downloading' && next.state !== 'checking') this.pressed = false;
  }
}
