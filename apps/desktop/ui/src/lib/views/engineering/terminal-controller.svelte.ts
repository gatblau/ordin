// The engineer's terminal: bytes in by `projects.terminal_input`, bytes out on `service_streams` as
// `StreamMessage` of kind `terminal` with payload `{ bytes_b64 }`, matched by the `StreamId` the open
// command returned. Nothing travels as a journalled event and no agent can reach the terminal.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-3
import type { StreamMessage } from '../../backend/backend';
import type { ServiceError } from '../../service-api/types.gen';
import type { Handle } from '../fixed/util';

/** Decodes the base64 of one stream payload; `null` when the payload is not `{ bytes_b64 }`. */
export function decodeBytes(payload: unknown): Uint8Array | null {
  if (typeof payload !== 'object' || payload === null) return null;
  const b64 = (payload as { bytes_b64?: unknown }).bytes_b64;
  if (typeof b64 !== 'string') return null;
  try {
    const raw = atob(b64);
    const out = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
    return out;
  } catch {
    return null;
  }
}

/** What the screen hands the controller to hear the stream (the app's `StreamHub`). */
export interface StreamSource {
  subscribe(stream: string, listener: (m: StreamMessage) => void): () => void;
}

export class TerminalController {
  stream = $state<string | null>(null);
  error = $state.raw<ServiceError | null>(null);
  closed = $state(false);
  readonly #handle: Handle;
  readonly #streams: StreamSource;
  readonly #write: (bytes: Uint8Array) => void;
  #unsubscribe: (() => void) | null = null;

  constructor(handle: Handle, streams: StreamSource, write: (bytes: Uint8Array) => void) {
    this.#handle = handle;
    this.#streams = streams;
    this.#write = write;
  }

  async open(projectId: string, cols: number, rows: number): Promise<boolean> {
    const out = await this.#handle.call('projects.terminal_open', { project_id: projectId, cols, rows });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.stream = out.result;
    this.#unsubscribe = this.#streams.subscribe(out.result, (m) => {
      if (m.kind !== 'terminal') return;
      const bytes = decodeBytes(m.payload);
      if (bytes !== null) this.#write(bytes);
    });
    return true;
  }

  async input(text: string): Promise<void> {
    if (this.stream === null || this.closed) return;
    await this.#handle.call('projects.terminal_input', { terminal: this.stream, bytes: [...new TextEncoder().encode(text)] });
  }

  async resize(cols: number, rows: number): Promise<void> {
    if (this.stream === null || this.closed) return;
    await this.#handle.call('projects.terminal_resize', { terminal: this.stream, cols, rows });
  }

  async close(): Promise<void> {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    if (this.stream === null || this.closed) return;
    this.closed = true;
    await this.#handle.call('projects.terminal_close', { terminal: this.stream });
  }
}
