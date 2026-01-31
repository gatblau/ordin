// The application context every component reaches with `getApp()`; built once by `startApp`.
import { getContext, setContext } from 'svelte';

import type { Backends, BackendId, DesktopEventName, StreamMessage, Transport } from '../backend/backend';
import type { I18n } from '../i18n/fluent';
import type { BackendState, Stores } from '../store/backend-state.svelte';
import type { Diagnostics } from '../store/diagnostics.svelte';
import type { ViewPrefs } from '../store/view-prefs';
import type { FirstStartModel } from '../views/setup/first-start-model.svelte';
import type { MemoryModelsModel } from '../views/memory/memory-models-model.svelte';
import type { ScreenAreaState } from './screen-area.svelte';
import type { ShellState } from './shell-state.svelte';

/** Fan-out of the unjournalled `service_streams` messages, matched by `stream` (the session or stream id a command returned). */
export class StreamHub {
  #byStream = new Map<string, Set<(m: StreamMessage) => void>>();
  subscribe(stream: string, listener: (m: StreamMessage) => void): () => void {
    let set = this.#byStream.get(stream);
    if (set === undefined) {
      set = new Set();
      this.#byStream.set(stream, set);
    }
    set.add(listener);
    return () => set.delete(listener);
  }
  dispatch(message: StreamMessage): void {
    for (const l of this.#byStream.get(message.stream) ?? []) l(message);
  }
}

export interface AppContext {
  transport: Transport;
  backends: Backends;
  stores: Stores;
  /** The local backend's store. */
  local: BackendState;
  diagnostics: Diagnostics;
  i18n: I18n;
  prefs: ViewPrefs;
  screens: ScreenAreaState;
  shell: ShellState;
  streams: StreamHub;
  /** First start (ScreensModule.md SM-FS-1 to SM-FS-4): whether the window draws `FirstStart` or its three regions. */
  firstStart: FirstStartModel;
  /** Getting memory ready (ScreensModule.md SM-MD-1): the local backend's models, drawn as a card in its conversations. */
  memoryModels: MemoryModelsModel;
  /** The store of a backend, or `undefined`. */
  stateFor(id: BackendId): BackendState | undefined;
  /** Subscribes to `ordin://quit-requested`, `open-target` or `appearance`; the quit and appearance ones are also handled by the shell. */
  onDesktop(name: DesktopEventName, listener: (payload: unknown) => void): () => void;
  /** Stops every listener `startApp` opened. */
  dispose(): void;
}

export const APP_KEY = Symbol('ordin.app');

export function setApp(app: AppContext): void {
  setContext(APP_KEY, app);
}

export function getApp(): AppContext {
  const app = getContext<AppContext | undefined>(APP_KEY);
  if (app === undefined) throw new Error('UI_NO_APP_CONTEXT');
  return app;
}
