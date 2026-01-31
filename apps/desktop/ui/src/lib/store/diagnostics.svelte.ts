// The in-memory diagnostics ring (Internal Logic step 12): the last 500 entries, codes and
// identifiers only, never the text of a block, card or message.
//
// spec: ordin-desktop/components/ScreensModule#UI_IPC_FAILED
import type { DiagnosticSink } from '../backend/backend';

export const RING_SIZE = 500;

export interface DiagnosticEntry {
  at: string;
  code: string;
  backend?: string;
  detail?: string;
}

export class Diagnostics implements DiagnosticSink {
  entries = $state<DiagnosticEntry[]>([]);
  #now: () => string;

  constructor(now: () => string = () => new Date().toISOString()) {
    this.#now = now;
  }

  record(code: string, detail?: string, backend?: string): void {
    const entry: DiagnosticEntry = { at: this.#now(), code };
    if (backend !== undefined) entry.backend = backend;
    if (detail !== undefined) entry.detail = detail;
    const next = [...this.entries, entry];
    this.entries = next.length > RING_SIZE ? next.slice(next.length - RING_SIZE) : next;
  }

  codes(): string[] {
    return this.entries.map((e) => e.code);
  }
}
