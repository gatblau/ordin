// The screen area's tabs: one row per workspace, something already open comes to the front, at most
// 6 open (opening a seventh closes the oldest tab neither in front nor showing running work), closing
// never deletes. Only the screen keys are remembered (Data Model, `ordin.view.tabs`).
import { SvelteMap } from 'svelte/reactivity';

import { MAX_TABS, type ViewPrefs } from '../store/view-prefs';
import { parseKey, SCREEN_IDS } from './screens';

export interface Tab {
  key: string;
  /** Text title from data (a document name); drawn as text. When absent the title is the screen's catalogue title. */
  title?: string;
  /** Opened by Ordin and not yet looked at (11 §4 *new* mark). */
  isNew?: boolean;
}

/**
 * The keys a tab menu's entry closes, for the tab at `index` (amended January 2026; user decision; Approved by
 * the user January 2026): Close others — every tab but that one; Close tabs to the right — those after it;
 * Close all — every tab.
 */
export function tabsToClose(all: readonly Tab[], index: number, which: 'others' | 'right' | 'all'): string[] {
  if (which === 'all') return all.map((tab) => tab.key);
  if (index < 0 || index >= all.length) return [];
  if (which === 'right') return all.slice(index + 1).map((tab) => tab.key);
  return all.filter((_, i) => i !== index).map((tab) => tab.key);
}

/** The kinds of item the side tree can remove; each one's tabs close when it goes (SM-OS-3). */
export type RemovableKind = 'dashboard' | 'document' | 'document_place' | 'knowledge_page' | 'workflow' | 'agent' | 'conversation' | 'minutes';

/**
 * The item a tab shows, for a kind of item: the id that tab's key carries, or `null` when the tab shows no
 * item of that kind. An agent's tabs are its page, *What it's made of* and its task forms
 * (`task-form:<agent id>:<command id>`); a linked place's tabs are its documents screen and its documents.
 */
export function itemOfTab(kind: RemovableKind, key: string): string | null {
  const { id, instance } = parseKey(key);
  if (instance === null) return null;
  const head = (): string => {
    const at = instance.indexOf(':');
    return at < 0 ? instance : instance.slice(0, at);
  };
  switch (kind) {
    case 'dashboard':
      return id === SCREEN_IDS.dashboards ? instance : null;
    case 'document':
      return id === SCREEN_IDS.document ? instance : null;
    case 'document_place':
      if (id === SCREEN_IDS.documents) return instance;
      return id === SCREEN_IDS.document ? head() : null;
    case 'knowledge_page':
      return id === SCREEN_IDS.knowledge ? instance : null;
    case 'workflow':
      return id === 'workflow' ? instance : null;
    case 'agent':
      if (id === SCREEN_IDS.agents || id === 'installed-files') return instance;
      return id === 'task-form' ? head() : null;
    case 'conversation':
      return id === 'earlier-conversation' ? instance : null;
    // spec: ordin-desktop/components/ScreensModule#SM-MIN-1
    case 'minutes':
      return id === 'minutes' ? instance : null;
  }
}

export class ScreenAreaState {
  #tabs = new SvelteMap<string, Tab[]>();
  #front = new SvelteMap<string, string | null>();
  /** The tabs first read from the saved preferences, per scope. Plain, not reactive: reading must never write reactive state (Svelte refuses it while a screen is drawn). */
  #saved = new Map<string, Tab[]>();
  #prefs: ViewPrefs | undefined;
  #showsRunningWork: (scope: string, key: string) => boolean;

  constructor(prefs?: ViewPrefs, showsRunningWork: (scope: string, key: string) => boolean = () => false) {
    this.#prefs = prefs;
    this.#showsRunningWork = showsRunningWork;
  }

  /** The scope of a workspace's tab row: `<backend id>:<workspace id>`; `<backend id>:` for none. */
  static scope(backend: string, workspaceId: string | null): string {
    return `${backend}:${workspaceId ?? ''}`;
  }

  tabs(scope: string): Tab[] {
    const set = this.#tabs.get(scope);
    if (set !== undefined) return set;
    let saved = this.#saved.get(scope);
    if (saved === undefined) {
      saved = (this.#prefs?.tabs(scope) ?? []).map((key) => ({ key }));
      this.#saved.set(scope, saved);
    }
    return saved;
  }

  front(scope: string): string | null {
    const set = this.#front.get(scope);
    if (set !== undefined) return set;
    return this.tabs(scope)[0]?.key ?? null;
  }

  /** Opens `key`, or brings it to the front when it is open. `inFront: false` leaves the person's place (Ordin opened it). */
  open(scope: string, key: string, options: { title?: string; inFront?: boolean } = {}): void {
    const inFront = options.inFront ?? true;
    let tabs = [...this.tabs(scope)];
    const existing = tabs.find((t) => t.key === key);
    if (existing === undefined) {
      const added: Tab = { key };
      if (options.title !== undefined) added.title = options.title;
      if (!inFront) added.isNew = true;
      tabs.push(added);
      if (tabs.length > MAX_TABS) {
        const front = inFront ? key : this.front(scope);
        const victim = tabs.find((t) => t.key !== front && t.key !== key && !this.#showsRunningWork(scope, t.key));
        if (victim !== undefined) tabs = tabs.filter((t) => t !== victim);
        else tabs = tabs.slice(tabs.length - MAX_TABS);
      }
    } else if (inFront && existing.isNew === true) {
      tabs = tabs.map((t) => (t.key === key ? { key: t.key, ...(t.title !== undefined ? { title: t.title } : {}) } : t));
    }
    this.#tabs.set(scope, tabs);
    if (inFront || this.front(scope) == null) this.#front.set(scope, key);
    this.#prefs?.setTabs(scope, tabs.map((t) => t.key));
  }

  show(scope: string, key: string): void {
    if (this.tabs(scope).some((t) => t.key === key)) this.open(scope, key);
  }

  /**
   * Closes several tabs at once (the tab menu). When the front tab is among them the front moves to the
   * nearest tab left open after it, else before it; with none left the area is empty. Nothing is deleted.
   */
  closeMany(scope: string, keys: readonly string[]): void {
    const gone = new Set(keys);
    const tabs = this.tabs(scope);
    const next = tabs.filter((t) => !gone.has(t.key));
    if (next.length === tabs.length) return;
    this.#tabs.set(scope, next);
    const front = this.front(scope);
    if (front !== null && gone.has(front)) {
      const at = tabs.findIndex((t) => t.key === front);
      const after = tabs.slice(at + 1).find((t) => !gone.has(t.key));
      const before = tabs.slice(0, at).reverse().find((t) => !gone.has(t.key));
      this.#front.set(scope, (after ?? before)?.key ?? null);
    }
    this.#prefs?.setTabs(scope, next.map((t) => t.key));
  }

  /** Closes a tab; the front moves to its neighbour. Nothing is deleted. */
  close(scope: string, key: string): void {
    const tabs = this.tabs(scope);
    const at = tabs.findIndex((t) => t.key === key);
    if (at < 0) return;
    const next = tabs.filter((t) => t.key !== key);
    this.#tabs.set(scope, next);
    if (this.front(scope) === key) this.#front.set(scope, (next[at] ?? next[at - 1])?.key ?? null);
    this.#prefs?.setTabs(scope, next.map((t) => t.key));
  }

  /**
   * Closes every tab for which `match` holds, in `scope` or, with none given, in every workspace's row. The
   * front moves as `closeMany` moves it. Nothing is deleted.
   */
  closeWhere(match: (key: string) => boolean, scope?: string): void {
    const scopes = scope === undefined ? new Set([...this.#tabs.keys(), ...this.#saved.keys()]) : [scope];
    for (const s of scopes) {
      const keys = this.tabs(s).map((tab) => tab.key).filter(match);
      if (keys.length > 0) this.closeMany(s, keys);
    }
  }

  /**
   * An item was removed (from the side tree, its own screen, or anywhere else): every tab open for it closes
   * straight away, in every workspace's row (SM-OS-3).
   */
  // spec: ordin-desktop/components/ScreensModule#SM-OS-3
  closeScreensFor(kind: RemovableKind, id: string): void {
    this.closeWhere((key) => itemOfTab(kind, key) === id);
  }

  /**
   * A complete list of a kind's items was read again for `scope` (a tree refresh): a tab whose item is not in
   * it was removed elsewhere, so it closes (SM-OS-3).
   */
  closeMissing(scope: string, kind: RemovableKind, live: Iterable<string>): void {
    const kept = new Set(live);
    this.closeWhere((key) => {
      const item = itemOfTab(kind, key);
      return item !== null && !kept.has(item);
    }, scope);
  }
}
