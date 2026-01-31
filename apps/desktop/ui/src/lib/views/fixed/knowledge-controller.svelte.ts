// The Knowledge screen's controller: the page list with its filter, the page with its links, other names
// and tags, and the suggestions (ADR-104). Nothing changes until a button is pressed.
//
// spec: ordin-desktop/components/ScreensModule#ADR-104
import type { PageLinkView, PageListing, PageView, ServiceError, SuggestionView } from '../../service-api/types.gen';
import { Announcer, type Handle } from './util';

export interface PageEdit {
  title: string;
  body_markdown: string;
  tags: string[];
  aliases: string[];
}

/** Other names, one per line; and tags, comma-separated (ADR-104 step 3). */
export const parseLines = (text: string): string[] => text.split('\n').map((s) => s.trim()).filter((s) => s !== '');
export const parseTags = (text: string): string[] => text.split(',').map((s) => s.trim()).filter((s) => s !== '');

export class KnowledgeController {
  listing = $state.raw<PageListing | null>(null);
  filter = $state('');
  page = $state.raw<PageView | null>(null);
  /** The heading to scroll to after a link into a section opens a page. */
  scrollTo = $state<string | null>(null);
  error = $state.raw<ServiceError | null>(null);
  /** The edit-time refusals drawn beside Tags / Other names / Title, with the edit kept. */
  editError = $state.raw<ServiceError | null>(null);
  /** Pages whose links were updated by a rename, to be listed on **Show them**. */
  renamed = $state.raw<PageView['links_in'] | null>(null);
  readonly announcer = new Announcer();
  readonly #handle: Handle;
  readonly workspaceId: string;

  constructor(handle: Handle, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
  }

  async load(filter = this.filter): Promise<void> {
    this.filter = filter;
    const trimmed = filter.trim();
    const out = await this.#handle.call('knowledge.list', { workspace_id: this.workspaceId, ...(trimmed === '' ? {} : { filter: trimmed }) });
    if (out.kind === 'ok') {
      this.listing = out.result;
      this.error = null;
    } else {
      this.error = out.error;
    }
  }

  /** Activating a tag chip puts `tag:<tag>` in the filter box. */
  async filterByTag(tag: string): Promise<void> {
    await this.load(`tag:${tag}`);
  }

  async open(pageId: string, section: string | null = null): Promise<void> {
    const out = await this.#handle.call('knowledge.get', { page_id: pageId });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.page = out.result;
    // The section is scrolled to when a heading of that text exists, else the top (decided by the view).
    this.scrollTo = section;
  }

  close(): void {
    this.page = null;
    this.scrollTo = null;
  }

  /** A link with no target opens nothing. */
  async openLink(link: PageLinkView): Promise<void> {
    if (link.target == null) return;
    await this.open(link.target.page_id, link.section ?? null);
  }

  async acceptSuggestion(s: SuggestionView): Promise<void> {
    await this.#decide(s, 'knowledge.accept_suggestion');
  }
  async dismissSuggestion(s: SuggestionView): Promise<void> {
    await this.#decide(s, 'knowledge.dismiss_suggestion');
  }
  async #decide(s: SuggestionView, command: 'knowledge.accept_suggestion' | 'knowledge.dismiss_suggestion'): Promise<void> {
    if (this.page === null) return;
    const pageId = this.page.page.id;
    const out = await this.#handle.call(command, { page_id: pageId, suggestion_id: s.id });
    // KNOW_SUGGESTION_STALE is drawn by ErrorLine and the page is read again either way.
    await this.open(pageId);
    if (out.kind === 'error') this.error = out.error;
  }

  /**
   * Saves a page. `tags` and `aliases` are sent only when either changed. A refusal (`KNOW_TAG_INVALID`,
   * `KNOW_ALIAS_INVALID`) stays beside the field with the edit kept. A save that changed the title of a page
   * other pages link to leaves the rename notice (the number of `links_in` before the save).
   */
  async save(edit: PageEdit): Promise<boolean> {
    const before = this.page;
    if (before === null) return false;
    const changedExtra = edit.tags.join('\n') !== before.tags.join('\n') || edit.aliases.join('\n') !== before.aliases.join('\n');
    const out = await this.#handle.call('knowledge.save_edit', {
      page_id: before.page.id,
      title: edit.title,
      body_markdown: edit.body_markdown,
      ...(changedExtra ? { tags: edit.tags, aliases: edit.aliases } : {}),
    });
    if (out.kind === 'error') {
      this.editError = out.error;
      return false;
    }
    this.editError = null;
    const titleChanged = edit.title !== before.page.title;
    await this.open(before.page.id);
    this.renamed = titleChanged && before.links_in.length > 0 ? before.links_in : null;
    return true;
  }
}
