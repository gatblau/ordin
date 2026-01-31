// *Add to this workspace* (SM-AW-1 to SM-AW-4): the steps of the screen and the commands each sends.
// *Where it lives* lists the entries; SharePoint and Confluence search and add a location (SM-AW-2), with the
// sign-in step of Settings › Sign-ins when the source needs one; a folder on this computer is chosen with the
// system picker (SM-AW-3); a successful `workspaces.add_link` draws *Added* and, while the first read runs,
// how far it has got (SM-AW-4).
//
// spec: ordin-desktop/components/ScreensModule#SM-AW-1
// spec: ordin-desktop/components/ScreensModule#SM-AW-2
// spec: ordin-desktop/components/ScreensModule#SM-AW-3
// spec: ordin-desktop/components/ScreensModule#SM-AW-4
import type { BackendHandle } from '../../backend/backend';
import type { ConfluenceLocation, CredentialKind, LinkLocation, LinkProgress, ServiceError, SharePointLocation, SourceLink, SourceLinkKind } from '../../service-api/types.gen';

/** The screen's identifier: one per workspace (SM-AW-1). */
export const ADD_TO_WORKSPACE_SCREEN = 'add-to-workspace';

/** The search waits this long after the last keystroke (SM-AW-2). */
export const ADD_SEARCH_DEBOUNCE_MS = 300;
export const ADD_SEARCH_LIMIT = 25;
export const ADD_SEARCH_MIN = 2;
export const ADD_SEARCH_MAX = 200;
/** `workspaces.link_progress` is asked again this often while the screen is in front (SM-AW-4). */
export const LINK_PROGRESS_EVERY_MS = 2000;

/** The entries of *Where it lives* this screen opens itself (Jira opens its own dialog). */
export type AddEntry = 'sharepoint' | 'confluence' | 'local' | 'code';
export type AddStep = 'where' | 'search' | 'sign_in' | 'not_configured' | 'folder' | 'jira' | 'added';

/** The link kind each entry adds, for the *Linked* label. */
export const ENTRY_LINK_KIND: Readonly<Record<AddEntry | 'jira', SourceLinkKind>> = {
  sharepoint: 'sharepoint_location',
  confluence: 'confluence_location',
  local: 'local_folder',
  code: 'git_repository_folder',
  jira: 'jira_issue',
};

/** Codes that replace the list with the sign-in step (SM-AW-2). */
const SIGN_IN_NEEDED = new Set(['SP_SIGN_IN_NEEDED', 'CONF_SIGN_IN_NEEDED', 'WS_SOURCE_SIGN_IN_NEEDED']);
/** Codes that draw `add-confluence-not-configured` with **Open Settings** (SM-AW-2). */
const NOT_CONFIGURED = new Set(['CONF_NOT_CONFIGURED', 'WS_SOURCE_NOT_CONFIGURED']);

/** One row of a SharePoint or Confluence answer. */
export type FoundPlace = { kind: 'sharepoint'; location: SharePointLocation } | { kind: 'confluence'; location: ConfluenceLocation };

/** A row's name in bold: `site_name › path` or `space_name › title` (the separator is drawn as text). */
export function placeName(place: FoundPlace): string {
  if (place.kind === 'sharepoint') {
    const path = place.location.path.trim();
    return path === '' ? `${place.location.site_name} › ${place.location.drive_name}` : `${place.location.site_name} › ${path}`;
  }
  return `${place.location.space_name} › ${place.location.title}`;
}

export function placeAddress(place: FoundPlace): string {
  return place.location.web_url;
}

export class AddToWorkspaceModel {
  step = $state<AddStep>('where');
  entry = $state<AddEntry | null>(null);
  /** The workspace's links, for *Linked* (`workspaces.links`). */
  links = $state.raw<SourceLink[]>([]);
  query = $state('');
  results = $state.raw<FoundPlace[]>([]);
  /** An error drawn by step 11: under the field, under **Add** or under **Choose a folder…**. */
  error = $state.raw<ServiceError | null>(null);
  /** Where `error` is drawn: under the search field, or under **Add** (`SRC_DUPLICATE`). */
  errorAt = $state<'field' | 'add'>('field');
  signingIn = $state(false);
  added = $state.raw<SourceLink | null>(null);
  progress = $state.raw<LinkProgress | null>(null);
  readonly workspaceId: string;
  readonly #handle: Pick<BackendHandle, 'call'>;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #lastQuery = '';

  constructor(handle: Pick<BackendHandle, 'call'>, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
  }

  /** `workspaces.links { workspace_id }`; a failure leaves the labels as they were. */
  async loadLinks(): Promise<void> {
    const out = await this.#handle.call('workspaces.links', { workspace_id: this.workspaceId });
    if (out.kind === 'ok') this.links = out.result;
  }

  /** True when the workspace already holds an active link of the entry's kind (`add-linked`). */
  linked(entry: AddEntry | 'jira'): boolean {
    return this.links.some((l) => l.state === 'active' && l.kind === ENTRY_LINK_KIND[entry]);
  }

  /** A row of *Where it lives*. */
  choose(entry: AddEntry): void {
    this.entry = entry;
    this.error = null;
    this.query = '';
    this.results = [];
    this.#lastQuery = '';
    this.step = entry === 'sharepoint' || entry === 'confluence' ? 'search' : 'folder';
  }

  openJira(): void {
    this.entry = null;
    this.error = null;
    this.step = 'jira';
  }

  /** **Back** and **Add something else**: *Where it lives* again. */
  back(): void {
    clearTimeout(this.#timer);
    this.step = 'where';
    this.entry = null;
    this.error = null;
    this.results = [];
    this.query = '';
    this.added = null;
    this.progress = null;
  }

  /** The search sends 300 ms after the last keystroke; empty clears the list. */
  type(query: string): void {
    this.query = query;
    clearTimeout(this.#timer);
    if (query.trim() === '') {
      this.results = [];
      this.error = null;
      return;
    }
    this.#timer = setTimeout(() => void this.search(), ADD_SEARCH_DEBOUNCE_MS);
  }

  /** `sharepoint.find` or `confluence.find` `{ query, limit: 25 }` for 2 to 200 characters. */
  async search(query: string = this.query): Promise<void> {
    const text = query.trim();
    if (text === '') {
      this.results = [];
      return;
    }
    if (text.length < ADD_SEARCH_MIN || text.length > ADD_SEARCH_MAX) return;
    this.#lastQuery = text;
    if (this.entry === 'sharepoint') {
      const out = await this.#handle.call('sharepoint.find', { query: text, limit: ADD_SEARCH_LIMIT });
      if (out.kind === 'error') return this.#refuse(out.error, 'field');
      this.error = null;
      this.results = out.result.map((location) => ({ kind: 'sharepoint', location }));
    } else if (this.entry === 'confluence') {
      const out = await this.#handle.call('confluence.find', { query: text, limit: ADD_SEARCH_LIMIT });
      if (out.kind === 'error') return this.#refuse(out.error, 'field');
      this.error = null;
      this.results = out.result.map((location) => ({ kind: 'confluence', location }));
    }
  }

  /** The account the sign-in step signs in to. */
  get signInKind(): CredentialKind {
    return this.entry === 'confluence' ? 'confluence' : 'microsoft';
  }

  /**
   * **Sign in with Microsoft** / **Sign in to Confluence**: `identity.sign_in` then `identity.sign_in_wait`, as
   * Settings › Sign-ins does; a successful wait sends the last search again.
   */
  async signIn(reason: string): Promise<void> {
    if (this.signingIn) return;
    this.signingIn = true;
    this.error = null;
    try {
      const out = await this.#handle.call('identity.sign_in', { kind: this.signInKind, reason });
      if (out.kind === 'error') {
        this.error = out.error;
        return;
      }
      if (out.result.kind !== 'already_signed_in') {
        const wait = await this.#handle.call('identity.sign_in_wait', { challenge_id: out.result.challenge_id });
        if (wait.kind === 'error') {
          this.error = wait.error.code === 'CRED_SIGN_IN_CANCELLED' ? null : wait.error;
          return;
        }
      }
      this.step = 'search';
      this.query = this.#lastQuery;
      await this.search(this.#lastQuery);
    } finally {
      this.signingIn = false;
    }
  }

  /** **Add** on a found row: `workspaces.add_link` with the location. */
  async addPlace(place: FoundPlace): Promise<void> {
    const location: LinkLocation = place.kind === 'sharepoint' ? { kind: 'sharepoint', location: place.location } : { kind: 'confluence', location: place.location };
    await this.#add(location);
  }

  /** **Choose a folder…**: `app.pick_folder {}`; no folder → nothing changes; a folder → `workspaces.add_link`. */
  async chooseFolder(): Promise<void> {
    const kind = this.entry === 'code' ? 'git_repository_folder' : 'local_folder';
    const picked = await this.#handle.call('app.pick_folder', {});
    if (picked.kind === 'error') {
      this.error = picked.error;
      return;
    }
    const folder = picked.result.folder;
    if (folder === undefined || folder === null || folder === '') return;
    await this.#add({ kind, path: folder });
  }

  /** After the Jira dialog added an issue: *Added* with its link. */
  jiraAdded(link: SourceLink | undefined): void {
    if (link === undefined) {
      this.back();
      void this.loadLinks();
      return;
    }
    this.#addedLink(link);
  }

  async #add(location: LinkLocation): Promise<void> {
    const out = await this.#handle.call('workspaces.add_link', { workspace_id: this.workspaceId, location });
    if (out.kind === 'error') {
      if (location.kind === 'sharepoint' || location.kind === 'confluence') this.#refuse(out.error, 'add');
      else this.error = out.error;
      return;
    }
    this.#addedLink(out.result);
  }

  #addedLink(link: SourceLink): void {
    this.error = null;
    this.added = link;
    this.progress = null;
    this.step = 'added';
    this.links = [...this.links.filter((l) => l.id !== link.id), link];
  }

  /** `workspaces.link_progress { source_link_id }`; returns true while the first read is still running. */
  async pollProgress(): Promise<boolean> {
    const link = this.added;
    if (link === null) return false;
    const out = await this.#handle.call('workspaces.link_progress', { source_link_id: link.id });
    if (out.kind === 'error' || this.added?.id !== link.id) return false;
    this.progress = out.result;
    return !out.result.done;
  }

  #refuse(error: ServiceError, at: 'field' | 'add'): void {
    this.errorAt = at;
    if (SIGN_IN_NEEDED.has(error.code)) {
      this.error = null;
      this.results = [];
      this.step = 'sign_in';
    } else if (NOT_CONFIGURED.has(error.code)) {
      this.error = null;
      this.results = [];
      this.step = 'not_configured';
    } else this.error = error;
  }
}
