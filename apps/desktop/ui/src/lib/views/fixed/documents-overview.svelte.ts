// The Documents overview (SM-DS-4, SM-DS-5): the workspace's recently changed documents across every place
// it links, newest first, with what the tree does not say — where each lives, when it last changed and by
// whom when the File Service knows. A search replaces the list with `files.search` hits over the same
// places. Only existing commands: `workspaces.links`, `files.list_folder` (`changed_desc`, each place's top
// folder) and `files.search`.
//
// spec: ordin-desktop/components/ScreensModule#SM-DS-4
// spec: ordin-desktop/components/ScreensModule#SM-DS-5
import type { BackendHandle } from '../../backend/backend';
import type { ServiceError, SourceLink } from '../../service-api/types.gen';

/** The most rows the overview draws. */
export const RECENT_LIMIT = 30;

export interface DocumentRow {
  placeId: string;
  path: string;
  name: string;
  /** The link's display name: where the document lives. */
  place: string;
  changedAt: string;
  /** "Ordin", an agent's name, "you" or the origin's author; `null` when unknown (a search hit). */
  changedBy: string | null;
}

const nameOf = (path: string): string => path.replace(/\/$/, '').split('/').pop() ?? path;
const newestFirst = (a: DocumentRow, b: DocumentRow): number => Date.parse(b.changedAt) - Date.parse(a.changedAt);

export class DocumentsOverview {
  rows = $state.raw<DocumentRow[]>([]);
  places = $state.raw<SourceLink[]>([]);
  error = $state.raw<ServiceError | null>(null);
  loaded = $state(false);
  /** The text searched for; empty → the recently changed list. */
  searched = $state('');
  readonly #handle: Pick<BackendHandle, 'call'>;
  readonly #workspaceId: string;

  constructor(handle: Pick<BackendHandle, 'call'>, workspaceId: string) {
    this.#handle = handle;
    this.#workspaceId = workspaceId;
  }

  #placeName(placeId: string): string {
    return this.places.find((l) => l.place_id === placeId)?.display_name ?? '';
  }

  async #readPlaces(): Promise<boolean> {
    const out = await this.#handle.call('workspaces.links', { workspace_id: this.#workspaceId });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.places = out.result.filter((l) => l.state === 'active' && l.kind !== 'jira_issue' && l.place_id != null && l.place_id !== '');
    return true;
  }

  /** The recently changed list; a place that cannot be read leaves its documents out and shows its error. */
  async load(): Promise<void> {
    this.searched = '';
    if (!(await this.#readPlaces())) {
      this.loaded = true;
      return;
    }
    let failed: ServiceError | null = null;
    const lists = await Promise.all(
      this.places.map(async (link) => {
        const placeId = link.place_id ?? '';
        const out = await this.#handle.call('files.list_folder', { place_id: placeId, folder: '', sort: 'changed_desc' });
        if (out.kind === 'error') {
          failed ??= out.error;
          return [];
        }
        return out.result.items
          .filter((e) => !e.is_folder)
          .map((e): DocumentRow => ({ placeId, path: e.path, name: e.name, place: link.display_name, changedAt: e.changed_at, changedBy: e.changed_by ?? null }));
      }),
    );
    this.error = failed;
    this.rows = lists.flat().sort(newestFirst).slice(0, RECENT_LIMIT);
    this.loaded = true;
  }

  /** `files.search` over the workspace's places; text shorter than two characters goes back to the list. */
  async search(text: string): Promise<void> {
    const query = text.trim();
    if (query.length < 2) {
      await this.load();
      return;
    }
    if (this.places.length === 0 && !(await this.#readPlaces())) return;
    const out = await this.#handle.call('files.search', { query, place_ids: this.places.map((l) => l.place_id ?? ''), limit: RECENT_LIMIT });
    this.searched = query;
    if (out.kind === 'error') {
      this.error = out.error;
      this.rows = [];
      return;
    }
    this.error = null;
    this.rows = out.result.hits.map((h) => ({ placeId: h.place_id, path: h.path, name: nameOf(h.path), place: this.#placeName(h.place_id), changedAt: h.changed_at, changedBy: null }));
  }
}
