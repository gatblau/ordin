// Home: Waiting for you, Running now, Since you last looked; the filter; Open a handover link… .
// No button on Home approves, sends, posts or publishes.
//
// spec: ordin-desktop/components/ScreensModule#HAND_LINK_INVALID
import type { BackendHandle } from '../backend/backend';
import { stateMessageId } from '../i18n/run-state';
import type { BackendState } from '../store/backend-state.svelte';
import type { HomeRow, HomeView, RunAgainSubject, ServiceError } from '../service-api/types.gen';

/** What opening a row or a handover needs from the shell; the component supplies the real one. */
export interface HomeOpener {
  /** Opens the row's item in its own workspace. */
  openRow(row: HomeRow): void;
  /** Opens the handover screen with no workspace (step 13). */
  openHandover(handoverId: string): void;
}

/** The events that make Home read again. */
export const HOME_REFRESH_EVENTS: ReadonlySet<string> = new Set(['run_state_changed', 'approval_requested', 'approval_decided', 'crew_round_changed', 'needs_you_changed']);

export function rowKey(row: HomeRow): string {
  return `${row.opens.kind}:${row.at}:${row.line}`;
}

/** One group of a Home section: a workspace's rows under its name, or rows with no workspace under a heading id. */
export interface HomeGroup {
  key: string;
  /** The workspace's name, drawn as it is; `null` for the two groups headed by `headingId`. */
  name: string | null;
  headingId: 'home-group-no-workspace' | 'home-group-hosted-agents' | null;
  rows: HomeRow[];
}

/**
 * A section's rows grouped by `workspace_name`, the groups in the order the reply gives them; then the
 * rows with no `workspace_id` under `home-group-no-workspace`, and last those that open a
 * `hosted_approval` under `home-group-hosted-agents`. Rows keep the reply's order inside a group.
 */
// spec: ordin-desktop/components/ScreensModule#SM-CU-5
export function groupRows(rows: readonly HomeRow[]): HomeGroup[] {
  const byName = new Map<string, HomeGroup>();
  const none: HomeRow[] = [];
  const hosted: HomeRow[] = [];
  for (const row of rows) {
    if (row.workspace_id === null || row.workspace_id === undefined) {
      (row.opens.kind === 'hosted_approval' ? hosted : none).push(row);
      continue;
    }
    const name = row.workspace_name ?? '';
    const group = byName.get(name);
    if (group === undefined) byName.set(name, { key: `ws:${name}`, name, headingId: null, rows: [row] });
    else group.rows.push(row);
  }
  const groups = [...byName.values()];
  if (none.length > 0) groups.push({ key: 'no-workspace', name: null, headingId: 'home-group-no-workspace', rows: none });
  if (hosted.length > 0) groups.push({ key: 'hosted-agents', name: null, headingId: 'home-group-hosted-agents', rows: hosted });
  return groups;
}

/** The subject of Run again…, for a row that opens a run or a crew run. */
export function runAgainSubject(row: HomeRow): RunAgainSubject | null {
  if (row.opens.kind === 'run') return { kind: 'run', run_id: row.opens.run_id };
  if (row.opens.kind === 'crew_run') return { kind: 'crew_run', crew_run_id: row.opens.crew_run_id };
  return null;
}

export class HomeModel {
  view = $state.raw<HomeView | null>(null);
  filter = $state<string | null>(null);
  error = $state<ServiceError | null>(null);

  linkOpen = $state(false);
  linkText = $state('');
  linkBusy = $state(false);
  linkError = $state<ServiceError | null>(null);
  /** The place Home now follows after a link whose handover is not readable yet. */
  following = $state<string | null>(null);

  readonly #handle: BackendHandle;
  readonly #opener: HomeOpener;

  constructor(handle: BackendHandle, opener: HomeOpener) {
    this.#handle = handle;
    this.#opener = opener;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('home.view', this.filter === null ? {} : { workspace_id: this.filter });
    if (out.kind === 'ok') {
      this.view = out.result;
      this.error = null;
    } else this.error = out.error;
  }

  /** Opening Home marks it opened (the new marks clear on the next visit). */
  async markOpened(): Promise<void> {
    await this.#handle.call('home.opened', {});
  }

  async setFilter(workspaceId: string | null): Promise<void> {
    this.filter = workspaceId;
    await this.load();
  }

  /** Only the oldest waiting row keeps the primary **Open**. */
  get oldestWaiting(): HomeRow | null {
    const rows = this.view?.waiting ?? [];
    let oldest: HomeRow | null = null;
    for (const row of rows) if (oldest === null || row.at < oldest.at) oldest = row;
    return oldest;
  }

  /** Opens the row's item in its own workspace; a document also tells Home it was opened. */
  async open(row: HomeRow): Promise<void> {
    this.#opener.openRow(row);
    if (row.opens.kind === 'document') {
      await this.#handle.call('home.document_opened', {
        item: { kind: 'place_file', place_id: row.opens.place_id, relative_path: row.opens.path },
        workspace_id: row.opens.workspace_id,
      });
    }
  }

  /** **Run again…**: reads what the earlier work used (`home.run_again_inputs`), then opens it. */
  async runAgain(row: HomeRow): Promise<void> {
    const subject = runAgainSubject(row);
    if (subject !== null) {
      const out = await this.#handle.call('home.run_again_inputs', { subject });
      if (out.kind === 'error') {
        this.error = out.error;
        return;
      }
    }
    this.#opener.openRow(row);
  }

  // ---- Open a handover link… ------------------------------------------------------------------------

  openLink(): void {
    this.linkOpen = true;
    this.linkText = '';
    this.linkError = null;
  }
  closeLink(): void {
    this.linkOpen = false;
    this.linkError = null;
  }

  /** `opened` → the handover screen; `not_found_yet` → the dialog closes and Home says it follows the place; an error stays on the dialog. */
  async submitLink(): Promise<void> {
    const link = this.linkText.trim();
    if (link === '' || this.linkBusy) return;
    this.linkBusy = true;
    const out = await this.#handle.call('handover.open_link', { link });
    this.linkBusy = false;
    if (out.kind === 'error') {
      this.linkError = out.error;
      return;
    }
    this.linkOpen = false;
    this.linkError = null;
    if (out.result.kind === 'opened') this.#opener.openHandover(out.result.handover_id);
    else this.following = out.result.place;
  }
}

/** The label id of a run state in the *Running now* section, when the store knows the run. */
export function runStateLabelId(backend: BackendState, row: HomeRow): string | null {
  if (row.opens.kind === 'run') {
    const state = backend.runs.get(row.opens.run_id)?.state;
    return state === undefined ? null : stateMessageId(state);
  }
  return null;
}
