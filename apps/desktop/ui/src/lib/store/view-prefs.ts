// View preferences remembered on this computer only (Data Model): localStorage keys prefixed
// `ordin.view.`. A value that fails to parse is replaced by its default and `UI_STORAGE_UNAVAILABLE`
// is recorded once. Nothing else is stored: no block content, card, message text or path.
//
// spec: ordin-desktop/components/ScreensModule#UI_STORAGE_UNAVAILABLE
import type { DiagnosticSink } from '../backend/backend';

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const KEYS = {
  rail: 'ordin.view.panel.rail',
  conversation: 'ordin.view.panel.conversation',
  treeShow: 'ordin.view.tree.show',
  tabs: 'ordin.view.tabs',
  expanded: 'ordin.view.tree.expanded',
} as const;

/** Default widths are the prototype's (290 and 420; ScreensModule amendment January 2026, item 3). */
export const RAIL = { min: 200, max: 480, default: 290 } as const;
export const CONVERSATION = { min: 280, max: 640, default: 420 } as const;
export const MAX_TABS = 6;

export interface TreeShow {
  documents: boolean;
  code: boolean;
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}
function isStringList(v: unknown): v is string[] {
  return Array.isArray(v) && v.every((s) => typeof s === 'string');
}

export class ViewPrefs {
  #storage: StorageLike | null;
  #diagnostics: DiagnosticSink | undefined;
  #reported = false;

  constructor(storage: StorageLike | null | undefined, diagnostics?: DiagnosticSink) {
    this.#storage = storage ?? null;
    this.#diagnostics = diagnostics;
  }

  #unavailable(): void {
    if (this.#reported) return;
    this.#reported = true;
    this.#diagnostics?.record('UI_STORAGE_UNAVAILABLE');
  }

  #read<T>(key: string, fallback: T, valid: (v: unknown) => v is T): T {
    try {
      if (this.#storage === null) throw new Error('no storage');
      const raw = this.#storage.getItem(key);
      if (raw === null) return fallback;
      const parsed: unknown = JSON.parse(raw);
      if (valid(parsed)) return parsed;
      throw new Error('unparsable');
    } catch {
      this.#unavailable();
      return fallback;
    }
  }

  #write(key: string, value: unknown): void {
    try {
      if (this.#storage === null) throw new Error('no storage');
      this.#storage.setItem(key, JSON.stringify(value));
    } catch {
      this.#unavailable();
    }
  }

  #width(key: string, range: { min: number; max: number; default: number }): number {
    const v = this.#read<number>(key, range.default, (x): x is number => typeof x === 'number' && Number.isFinite(x));
    return Math.min(range.max, Math.max(range.min, Math.round(v)));
  }

  railWidth(): number {
    return this.#width(KEYS.rail, RAIL);
  }
  setRailWidth(px: number): number {
    const v = Math.min(RAIL.max, Math.max(RAIL.min, Math.round(px)));
    this.#write(KEYS.rail, v);
    return v;
  }
  conversationWidth(): number {
    return this.#width(KEYS.conversation, CONVERSATION);
  }
  setConversationWidth(px: number): number {
    const v = Math.min(CONVERSATION.max, Math.max(CONVERSATION.min, Math.round(px)));
    this.#write(KEYS.conversation, v);
    return v;
  }

  treeShow(workspaceId: string): TreeShow {
    const all = this.#read<Record<string, unknown>>(KEYS.treeShow, {}, isRecord);
    const one = all[workspaceId];
    if (isRecord(one) && typeof one['documents'] === 'boolean' && typeof one['code'] === 'boolean') {
      // One is always true.
      return one['documents'] || one['code'] ? { documents: one['documents'], code: one['code'] } : { documents: true, code: true };
    }
    return { documents: true, code: true };
  }
  setTreeShow(workspaceId: string, show: TreeShow): void {
    const all = this.#read<Record<string, unknown>>(KEYS.treeShow, {}, isRecord);
    all[workspaceId] = show.documents || show.code ? show : { documents: true, code: true };
    this.#write(KEYS.treeShow, all);
  }

  #lists(key: string): Record<string, string[]> {
    const raw = this.#read<Record<string, unknown>>(key, {}, isRecord);
    const out: Record<string, string[]> = {};
    for (const [k, v] of Object.entries(raw)) if (isStringList(v)) out[k] = v;
    return out;
  }

  /** The screen keys open in `<backend id>:<workspace id>`, at most 6. */
  tabs(scope: string): string[] {
    return (this.#lists(KEYS.tabs)[scope] ?? []).slice(0, MAX_TABS);
  }
  setTabs(scope: string, screenKeys: string[]): void {
    const all = this.#lists(KEYS.tabs);
    all[scope] = screenKeys.slice(0, MAX_TABS);
    this.#write(KEYS.tabs, all);
  }

  /** Expanded tree node keys; `fallback` (the open workspace) when nothing is stored. */
  expanded(fallback: string[] = []): string[] {
    return this.#read<string[]>(KEYS.expanded, fallback, isStringList);
  }
  setExpanded(nodeKeys: string[]): void {
    this.#write(KEYS.expanded, nodeKeys);
  }
}

/** The WebView's localStorage when it is there and usable; `null` otherwise. */
export function browserStorage(): StorageLike | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}
