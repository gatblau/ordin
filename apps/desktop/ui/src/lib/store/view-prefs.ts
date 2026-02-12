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
  /** What the Facts list groups by, per workspace (ScreensModule.md SM-SF-27). */
  memoryGroupBy: 'ordin.view.memory.group_by',
  /** Which side panes are collapsed to a strip (amended January 2026; user decision; Approved by the user January 2026). */
  collapsed: 'ordin.view.panel.collapsed',
  /** Settings › Memory "How memory is shown" (SM-NS-1.9; amended January 2026; user decision; Approved by the user January 2026). */
  memoryShown: 'ordin.view.memory.shown',
  /** The agent-work layout and the panes' collapsed state to restore when it is left (amended January 2026; user decision; Approved by the user January 2026). */
  agentWork: 'ordin.view.panel.agent_work',
  /** The workspaces whose Knowledge row is unfolded, as `<backend id>:<workspace id>` (ScreensModule.md SM-KT-1). */
  knowledgeUnfolded: 'ordin.view.tree.knowledge_unfolded',
} as const;

/** The choices of "Facts shown before Show more" (SM-NS-1.9). */
export const MEMORY_PAGE_SIZES = [10, 25, 50, 100] as const;
export type MemoryPageSize = (typeof MEMORY_PAGE_SIZES)[number];

/**
 * Settings › Memory "How memory is shown" (SM-NS-1.9): what the Facts list groups by when a workspace has
 * no choice of its own, how many facts a group shows before **Show more**, and whether a fact's details
 * start open. The defaults are today's: by thing, 50, closed.
 */
export interface MemoryShown {
  groupBy: string;
  pageSize: MemoryPageSize;
  detailsOpen: boolean;
}
export const MEMORY_SHOWN_DEFAULT: MemoryShown = { groupBy: 'thing', pageSize: 50, detailsOpen: false };

/** The side panes that collapse: the side tree (`rail`) and the conversation beside the screen area. */
export interface Collapsed {
  rail: boolean;
  conversation: boolean;
}

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
  collapsed(): Collapsed {
    const v = this.#read<Record<string, unknown>>(KEYS.collapsed, {}, isRecord);
    return { rail: v.rail === true, conversation: v.conversation === true };
  }
  setCollapsed(value: Collapsed): void {
    this.#write(KEYS.collapsed, { rail: value.rail, conversation: value.conversation });
  }
  /** The agent-work layout: `null` when it is off, else the collapsed state to restore when it is left. */
  agentWork(): { restore: Collapsed } | null {
    const v = this.#read<Record<string, unknown>>(KEYS.agentWork, {}, isRecord);
    if (v.on !== true || !isRecord(v.restore)) return null;
    return { restore: { rail: v.restore.rail === true, conversation: v.restore.conversation === true } };
  }
  setAgentWork(value: { restore: Collapsed } | null): void {
    this.#write(KEYS.agentWork, value === null ? { on: false } : { on: true, restore: { rail: value.restore.rail, conversation: value.restore.conversation } });
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

  /** SM-SF-27: what the Facts list of `workspaceId` groups by; `fallback` when nothing valid is stored. */
  memoryGroupBy<T extends string>(workspaceId: string, valid: (v: unknown) => v is T, fallback: T): T {
    const all = this.#read<Record<string, unknown>>(KEYS.memoryGroupBy, {}, isRecord);
    const one = all[workspaceId];
    return valid(one) ? one : fallback;
  }
  setMemoryGroupBy(workspaceId: string, groupBy: string): void {
    const all = this.#read<Record<string, unknown>>(KEYS.memoryGroupBy, {}, isRecord);
    all[workspaceId] = groupBy;
    this.#write(KEYS.memoryGroupBy, all);
  }

  /** SM-NS-1.9: Settings › Memory "How memory is shown"; each field falls back to its default on its own. */
  memoryShown(validGroupBy: (v: unknown) => boolean = (v) => typeof v === 'string'): MemoryShown {
    const v = this.#read<Record<string, unknown>>(KEYS.memoryShown, {}, isRecord);
    return {
      groupBy: typeof v.groupBy === 'string' && validGroupBy(v.groupBy) ? v.groupBy : MEMORY_SHOWN_DEFAULT.groupBy,
      pageSize: (MEMORY_PAGE_SIZES as readonly unknown[]).includes(v.pageSize) ? (v.pageSize as MemoryPageSize) : MEMORY_SHOWN_DEFAULT.pageSize,
      detailsOpen: typeof v.detailsOpen === 'boolean' ? v.detailsOpen : MEMORY_SHOWN_DEFAULT.detailsOpen,
    };
  }
  setMemoryShown(value: MemoryShown): void {
    this.#write(KEYS.memoryShown, { groupBy: value.groupBy, pageSize: value.pageSize, detailsOpen: value.detailsOpen });
  }

  /** Expanded tree node keys; `fallback` (the open workspace) when nothing is stored. */
  expanded(fallback: string[] = []): string[] {
    return this.#read<string[]>(KEYS.expanded, fallback, isStringList);
  }
  setExpanded(nodeKeys: string[]): void {
    this.#write(KEYS.expanded, nodeKeys);
  }

  /** The Knowledge rows unfolded in the tree (SM-KT-1), kept apart from `expanded`, which holds workspaces only. */
  knowledgeUnfolded(): string[] {
    return this.#read<string[]>(KEYS.knowledgeUnfolded, [], isStringList);
  }
  setKnowledgeUnfolded(nodeKeys: string[]): void {
    this.#write(KEYS.knowledgeUnfolded, nodeKeys);
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
