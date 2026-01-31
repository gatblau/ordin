// Search or go to… (11 §5.17): the query, the three groups (Go to, Found in pages and documents, Actions)
// and the Actions group's rules (SM-SF-9, SM-UP-1).
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-9
import type { DiagnosticSink } from '../backend/backend';
import type { BackendHandle } from '../backend/backend';
import { answersAll, type HasGroups } from '../backend/groups';
import {
  ACTION_COMMANDS,
  ACTION_LABEL_IDS,
  actionHandler,
  actionScreen,
  WORKSPACE_ACTIONS,
  type SearchActionName,
} from '../search/actions';
import type { FoundResult, GoToResult, SearchAnswer, ServiceError } from '../service-api/types.gen';

/** Queries are debounced this long (ms). */
export const SEARCH_DEBOUNCE_MS = 120;

export type SearchGroup = 'go_to' | 'found' | 'actions';

export interface SearchEntry {
  group: SearchGroup;
  id: string;
  /** The text drawn: a result's label, or the catalogue's word for an action. */
  label: string | null;
  labelId: string | null;
  detail: string | null;
  workspaceId: string | null;
  /** The result belongs to another workspace than the one in front. */
  elsewhere: boolean;
  action: SearchActionName | null;
  go: GoToResult | null;
  found: FoundResult | null;
}

export interface SearchDeps {
  handle: BackendHandle;
  /** The backend, for SM-UP-1. */
  groups: HasGroups;
  /** The workspace in front, or `null`. */
  inFront: () => string | null;
  diagnostics?: DiagnosticSink;
}

export class SearchModel {
  query = $state('');
  answer = $state.raw<SearchAnswer | null>(null);
  error = $state<ServiceError | null>(null);
  active = $state(0);
  readonly #deps: SearchDeps;
  #seq = 0;

  constructor(deps: SearchDeps) {
    this.#deps = deps;
  }

  reset(): void {
    this.query = '';
    this.answer = null;
    this.error = null;
    this.active = 0;
  }

  async run(): Promise<void> {
    const query = this.query;
    const mine = ++this.#seq;
    if (query.trim() === '') {
      this.answer = null;
      return;
    }
    const front = this.#deps.inFront();
    const out = await this.#deps.handle.call('search.query', { query, ...(front === null ? {} : { current_workspace_id: front }) });
    if (mine !== this.#seq) return; // a newer query has been sent
    if (out.kind === 'ok') {
      this.answer = out.result;
      this.error = null;
      this.active = 0;
    } else this.error = out.error;
  }

  /** The Actions entries that are drawn: known names, workspace actions only with a workspace in front, backend answers, someone can act. */
  get actions(): SearchEntry[] {
    const out: SearchEntry[] = [];
    const front = this.#deps.inFront();
    for (const [i, entry] of (this.answer?.actions ?? []).entries()) {
      const name = actionScreen(entry);
      if (name === null) {
        this.#deps.diagnostics?.record('UI_SEARCH_ACTION_UNKNOWN', entry.opens.kind === 'screen' ? entry.opens.name : entry.opens.kind);
        continue;
      }
      if (WORKSPACE_ACTIONS.has(name) && front === null) continue;
      if (!answersAll(this.#deps.groups, ACTION_COMMANDS[name])) continue;
      if (!this.#isActionable(name)) continue;
      out.push({
        group: 'actions',
        id: `action-${i}`,
        label: null,
        labelId: ACTION_LABEL_IDS[name],
        detail: null,
        workspaceId: front,
        elsewhere: false,
        action: name,
        go: entry,
        found: null,
      });
    }
    return out;
  }

  /** The three actions that only open a screen need no handler; the others need the owner of the surface to have registered one. */
  #isActionable(name: SearchActionName): boolean {
    return name === 'home' || name === 'settings' || name === 'earlier_conversations' || actionHandler(name) !== undefined;
  }

  get goTo(): SearchEntry[] {
    const front = this.#deps.inFront();
    return (this.answer?.go_to ?? []).map((g, i) => ({
      group: 'go_to' as const,
      id: `go-${i}`,
      label: g.label,
      labelId: null,
      detail: g.detail ?? null,
      workspaceId: g.workspace_id ?? null,
      elsewhere: g.workspace_id != null && front !== null && g.workspace_id !== front,
      action: null,
      go: g,
      found: null,
    }));
  }

  get found(): SearchEntry[] {
    const front = this.#deps.inFront();
    return (this.answer?.found ?? []).map((f, i) => ({
      group: 'found' as const,
      id: `found-${i}`,
      label: f.label,
      labelId: null,
      detail: f.snippet ?? null,
      workspaceId: f.workspace_id,
      elsewhere: front !== null && f.workspace_id !== front,
      action: null,
      go: null,
      found: f,
    }));
  }

  /** All entries in the order drawn; the keyboard moves through this list. */
  get entries(): SearchEntry[] {
    return [...this.goTo, ...this.found, ...this.actions];
  }

  /**
   * What the polite live region reads: `search-result-count` with the number of entries drawn in the
   * three groups (after SM-SF-9's filtering), `search-no-results` when that is 0, and nothing while the
   * box is empty or before the first answer. It changes only when an answer of `search.query` arrives.
   */
  // spec: ordin-desktop/components/ScreensModule#SM-CU-6
  get announcement(): { id: 'search-result-count'; count: number } | { id: 'search-no-results' } | null {
    if (this.query.trim() === '' || this.answer === null) return null;
    const count = this.entries.length;
    return count === 0 ? { id: 'search-no-results' } : { id: 'search-result-count', count };
  }

  /** Arrow keys wrap. */
  move(delta: 1 | -1): void {
    const n = this.entries.length;
    if (n === 0) return;
    this.active = (this.active + delta + n) % n;
  }
}
