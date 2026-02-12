// What the Knowledge row lists when unfolded (ScreensModule.md SM-KT-2): one row per team folder, by name,
// with its pages under it, then the workspace's own pages at the top level. Removed pages, pages from
// agents and files not adopted are not listed. At a level of more than 10 pages, the 10 most recently
// changed are drawn first, still by title, and **Show all…** draws the rest.
//
// spec: ordin-desktop/components/ScreensModule#SM-KT-2
import type { PageListing, PageSummary } from '../service-api/types.gen';

/** Pages shown at a level before **Show all…** (SM-KT-2.3). */
export const FIRST_SHOWN = 10;

export interface KnowledgeFolder {
  name: string;
  description: string | null;
  pages: PageSummary[];
}

export interface KnowledgeTree {
  folders: KnowledgeFolder[];
  pages: PageSummary[];
}

const byName = (a: string, b: string): number => {
  const x = a.toLocaleLowerCase();
  const y = b.toLocaleLowerCase();
  return x < y ? -1 : x > y ? 1 : 0;
};

const byTitle = (a: PageSummary, b: PageSummary): number => byName(a.title, b.title);

const listed = (p: PageSummary): boolean => p.state !== 'removed';

/** The folders and top-level pages of a listing, each level ordered by name (SM-KT-2.1 to 2.3). */
export function knowledgeTree(listing: PageListing): KnowledgeTree {
  const folders = new Map<string, KnowledgeFolder>();
  for (const page of listing.from_other_teams.filter(listed)) {
    const name = page.folder ?? '';
    let folder = folders.get(name);
    if (folder === undefined) {
      folder = { name, description: page.folder_description ?? null, pages: [] };
      folders.set(name, folder);
    }
    folder.pages.push(page);
  }
  for (const folder of folders.values()) folder.pages.sort(byTitle);
  return {
    folders: [...folders.values()].sort((a, b) => byName(a.name, b.name)),
    pages: listing.this_workspace.filter(listed).sort(byTitle),
  };
}

/**
 * The pages drawn at one level: all of them when `all` is set or there are at most 10; otherwise the 10
 * most recently changed, by title (SM-KT-2.3). `more` says whether **Show all…** is drawn.
 */
export function shownAtLevel(pages: PageSummary[], all: boolean): { shown: PageSummary[]; more: boolean } {
  if (all || pages.length <= FIRST_SHOWN) return { shown: pages, more: false };
  const newest = [...pages].sort((a, b) => (a.last_changed_at < b.last_changed_at ? 1 : a.last_changed_at > b.last_changed_at ? -1 : 0)).slice(0, FIRST_SHOWN);
  return { shown: newest.sort(byTitle), more: true };
}
