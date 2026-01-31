// Jira pieces (ADR-106): Add to this workspace › Jira issue…, the handover draft's "Which issue?", and a
// linked issue's changed item (SM-WR-5): the items of a remote link are read with
// `workspaces.working_copy_items`, and an item with `origin_newer_at` offers **Bring in the new version**,
// which sends `workspaces.read_item`. Jira is drawn on the desktop's own backend only; a server's
// workspace never offers it. Every text read from Jira is drawn as text.
//
// spec: ordin-desktop/components/ScreensModule#ADR-106
// spec: ordin-desktop/components/ScreensModule#SM-WR-5
import { SvelteMap } from 'svelte/reactivity';

import type { DraftJiraView, JiraIssueSelection, JiraIssueSummary, ServiceError, SourceLink, WorkingCopyItem } from '../../service-api/types.gen';
import type { Handle } from './util';

export const JIRA_SEARCH_DEBOUNCE_MS = 300;
export const JIRA_SEARCH_LIMIT = 25;
/** Codes meaning Jira is not connected: the dialog then points to Settings › Jira. */
const NOT_CONNECTED = new Set(['JIRA_NOT_CONNECTED', 'WS_SOURCE_SIGN_IN_NEEDED']);

/** Jira is offered on the desktop's own backend only. */
export function jiraOffered(handle: Pick<Handle, 'kind'>): boolean {
  return handle.kind === 'local';
}

/** The address of a picked issue: `https://<site>/browse/<key>`. */
export function issueAddress(issue: JiraIssueSummary): string {
  return `https://${issue.issue.site}/browse/${issue.issue.key}`;
}

export class JiraAddController {
  pasted = $state('');
  text = $state('');
  results = $state.raw<JiraIssueSummary[]>([]);
  picked = $state<string | null>(null);
  searched = $state(false);
  notConnected = $state(false);
  error = $state.raw<ServiceError | null>(null);
  added = $state(false);
  /** The link the last successful **Add** made (*Added* of SM-AW-4 names it). */
  link = $state.raw<SourceLink | null>(null);
  readonly #handle: Handle;
  readonly workspaceId: string;
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(handle: Handle, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
  }

  /** Searches 300 ms after the last keystroke. */
  type(text: string): void {
    this.text = text;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.search(), JIRA_SEARCH_DEBOUNCE_MS);
  }

  async search(): Promise<void> {
    const text = this.text.trim();
    if (text === '') {
      this.results = [];
      this.searched = false;
      return;
    }
    const out = await this.#handle.call('jira.find_issues', { text, limit: JIRA_SEARCH_LIMIT });
    this.searched = true;
    if (out.kind === 'error') {
      this.notConnected = NOT_CONNECTED.has(out.error.code);
      this.error = this.notConnected ? null : out.error;
      this.results = [];
      return;
    }
    this.notConnected = false;
    this.error = null;
    this.results = out.result;
  }

  /** **Add** sends the pasted text, or the picked row's address. */
  async add(): Promise<boolean> {
    const picked = this.results.find((r) => r.id === this.picked);
    const address = this.pasted.trim() !== '' ? this.pasted.trim() : picked !== undefined ? issueAddress(picked) : '';
    if (address === '') return false;
    const out = await this.#handle.call('workspaces.add_link', { workspace_id: this.workspaceId, location: { kind: 'jira_issue', address } });
    if (out.kind === 'error') {
      this.notConnected = NOT_CONNECTED.has(out.error.code);
      this.error = this.notConnected ? null : out.error;
      return false;
    }
    this.error = null;
    this.added = true;
    this.link = out.result;
    return true;
  }
}

/** The handover draft's choice of issue (ADR-106 step 4). */
export class HandoverJiraController {
  jira = $state.raw<DraftJiraView | null>(null);
  line = $state('');
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly draftId: string;

  constructor(handle: Handle, draftId: string, jira: DraftJiraView | null) {
    this.#handle = handle;
    this.draftId = draftId;
    this.jira = jira;
    this.line = jira?.line ?? '';
  }

  /** A change sends `handover.update_draft` with `jira_issue`; the draft shows the line the reply carries. */
  async choose(selection: JiraIssueSelection): Promise<boolean> {
    const out = await this.#handle.call('handover.update_draft', { draft: this.draftId, jira_issue: selection });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    if (out.result.jira != null) {
      this.jira = out.result.jira;
      this.line = out.result.jira.line;
    }
    return true;
  }

  /** After a link was added through the dialog, the draft is opened again so the issue appears in `options`. */
  async reopen(workspaceId: string, results: string[]): Promise<boolean> {
    const out = await this.#handle.call('handover.open_draft', { workspace_id: workspaceId, results });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.jira = out.result.jira ?? null;
    this.line = out.result.jira?.line ?? '';
    return true;
  }
}

/** A linked issue's items (SM-WR-5): read when the link's node is expanded and on each `workspace_changed`. */
export class LinkItemsController {
  items = $state.raw<WorkingCopyItem[]>([]);
  error = $state.raw<ServiceError | null>(null);
  /** A refused **Bring in the new version**, by `origin_path`: drawn beside the item, whose notice stays. */
  readonly itemErrors = new SvelteMap<string, ServiceError>();
  readonly #handle: Handle;
  readonly sourceLinkId: string;

  constructor(handle: Handle, sourceLinkId: string) {
    this.#handle = handle;
    this.sourceLinkId = sourceLinkId;
  }

  /** `workspaces.working_copy_items { source_link_id }`. */
  async load(): Promise<void> {
    const out = await this.#handle.call('workspaces.working_copy_items', { source_link_id: this.sourceLinkId });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.items = out.result;
  }

  /** The item's notice is drawn only while `origin_newer_at` is set; nothing changes before the press. */
  static changed(item: WorkingCopyItem): boolean {
    return item.origin_newer_at !== null && item.origin_newer_at !== undefined;
  }

  /** **Bring in the new version**: `ok` redraws the item from the reply, with no notice. */
  async bringIn(item: WorkingCopyItem): Promise<boolean> {
    const out = await this.#handle.call('workspaces.read_item', { source_link_id: this.sourceLinkId, origin_path: item.origin_path });
    if (out.kind === 'error') {
      this.itemErrors.set(item.origin_path, out.error);
      return false;
    }
    this.itemErrors.delete(item.origin_path);
    this.items = this.items.map((i) => (i.origin_path === item.origin_path ? out.result : i));
    return true;
  }
}
