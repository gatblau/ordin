// A page named by its id (SM-CU-11). `FactView` and `LinkBy` of kind `page` carry a page id only, so a
// view that draws them reads each distinct page id once with `knowledge.get { page_id }` while it is open.
// A refusal means the page was removed: the id maps to `null`, and the view draws `memory-page-gone` in
// place of the line, with no **Open the page**. The titles live as long as the view; nothing is stored.
//
// spec: ordin-desktop/components/ScreensModule#SM-CU-11
import type { FactView, LinkView } from '../../service-api/types.gen';
import type { Handle } from './util';

/** A page's title, `null` when `knowledge.get` refused it (the page is gone); absent while unread. */
export type PageTitleMap = Readonly<Record<string, string | null>>;

export class PageTitles {
  titles = $state.raw<PageTitleMap>({});
  readonly #handle: Handle;
  readonly #asked = new Set<string>();

  constructor(handle: Handle) {
    this.#handle = handle;
  }

  /** Reads every id not read before by this view, once each. */
  async read(ids: Iterable<string>): Promise<void> {
    const fresh = [...new Set(ids)].filter((id) => !this.#asked.has(id));
    if (fresh.length === 0) return;
    for (const id of fresh) this.#asked.add(id);
    const found: Record<string, string | null> = {};
    for (const id of fresh) {
      const out = await this.#handle.call('knowledge.get', { page_id: id });
      found[id] = out.kind === 'ok' ? out.result.page.title : null;
    }
    this.titles = { ...this.titles, ...found };
  }
}

/** The page ids of facts drawn from a page and of links set by a page. */
export function pageIdsOf(facts: readonly FactView[], links: readonly LinkView[]): string[] {
  const ids: string[] = [];
  for (const f of facts) if (typeof f.page_id === 'string' && f.page_id !== '') ids.push(f.page_id);
  for (const l of links) if (l.by.kind === 'page') ids.push(l.by.page_id);
  return ids;
}
