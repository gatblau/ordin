// The screen area's tabs: one row per workspace, something already open comes to the front, at most
// 6 open (opening a seventh closes the oldest tab neither in front nor showing running work), closing
// never deletes. Only the screen keys are remembered (Data Model, `ordin.view.tabs`).
import { SvelteMap } from 'svelte/reactivity';

import { MAX_TABS, type ViewPrefs } from '../store/view-prefs';

export interface Tab {
  key: string;
  /** Text title from data (a document name); drawn as text. When absent the title is the screen's catalogue title. */
  title?: string;
  /** Opened by Ordin and not yet looked at (11 §4 *new* mark). */
  isNew?: boolean;
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
}
