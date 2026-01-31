// The Knowledge screen's controller: the page list with its filter, the page with its links, other names
// and tags, and the suggestions (ADR-104). Nothing changes until a button is pressed.
//
// spec: ordin-desktop/components/ScreensModule#ADR-104
// spec: ordin-desktop/components/ScreensModule#SM-SF-18
import type { PageLinkView, PageListing, PageView, PassageView, ServiceError, SuggestionView } from '../../service-api/types.gen';
import { Announcer, type Handle } from './util';

export interface PageEdit {
  title: string;
  body_markdown: string;
  tags: string[];
  aliases: string[];
  /** The page's kind (Open Knowledge Format `type`); absent: unchanged. */
  type?: string;
  /** The one-sentence summary (`description`); absent: unchanged. */
  description?: string;
}

/** The kinds of page a person can choose (Open Knowledge Format 0.2, `type`), each with its message. */
export const PAGE_TYPES: ReadonlyArray<{ value: string; label: string }> = [
  { value: 'Explanation', label: 'knowledge-type-explanation' },
  { value: 'How-to', label: 'knowledge-type-how-to' },
  { value: 'Decision', label: 'knowledge-type-decision' },
  { value: 'Reference', label: 'knowledge-type-reference' },
  { value: 'Glossary', label: 'knowledge-type-glossary' },
];

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
  /** The page just removed, kept so **Undo** can bring it back (`knowledge.undo_remove`). */
  removed = $state.raw<{ id: string; title: string } | null>(null);
  /** Pages whose links were updated by a rename, to be listed on **Show them**. */
  renamed = $state.raw<PageView['links_in'] | null>(null);
  /** **What Ordin read** (SM-SF-18): collapsed until opened; the page's sections, read on opening. */
  readOpen = $state(false);
  passages = $state.raw<PassageView[] | null>(null);
  passagesError = $state.raw<ServiceError | null>(null);
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
    this.readOpen = false;
    this.passages = null;
    this.passagesError = null;
    // The section is scrolled to when a heading of that text exists, else the top (decided by the view).
    this.scrollTo = section;
  }

  close(): void {
    this.page = null;
    this.scrollTo = null;
  }

  /** Opening **What Ordin read** sends `memory.passages { workspace_id, page_id }` (SM-SF-18). */
  async toggleRead(): Promise<void> {
    this.readOpen = !this.readOpen;
    if (!this.readOpen || this.page === null) return;
    const out = await this.#handle.call('memory.passages', { workspace_id: this.workspaceId, page_id: this.page.page.id });
    if (out.kind === 'error') {
      this.passagesError = out.error;
      return;
    }
    this.passagesError = null;
    this.passages = out.result.items;
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

  /** **Remove** after the confirmation: the page goes to this computer's Bin and the list is read again. */
  async remove(): Promise<boolean> {
    const view = this.page;
    if (view === null) return false;
    const out = await this.#handle.call('knowledge.remove', { page_id: view.page.id });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    this.removed = { id: view.page.id, title: view.page.title };
    this.close();
    await this.load();
    return true;
  }

  /** **Undo** the last removal; the page returns with its title (`KNOW_TITLE_DUPLICATE` is drawn if it is taken). */
  async undoRemove(): Promise<void> {
    const gone = this.removed;
    if (gone === null) return;
    const out = await this.#handle.call('knowledge.undo_remove', { page_id: gone.id });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.removed = null;
    await this.load();
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
    // The kind and the summary are sent only when the person changed them, so a kind another tool wrote
    // (not one of the five) is kept as written; an emptied summary leaves the page's own.
    const type = edit.type !== undefined && edit.type !== (before.type ?? '') && edit.type !== '' ? edit.type : undefined;
    const description = edit.description?.trim();
    const changedDescription = description !== undefined && description !== '' && description !== (before.description ?? '');
    const out = await this.#handle.call('knowledge.save_edit', {
      page_id: before.page.id,
      title: edit.title,
      body_markdown: edit.body_markdown,
      ...(changedExtra ? { tags: edit.tags, aliases: edit.aliases } : {}),
      ...(type !== undefined ? { type } : {}),
      ...(changedDescription ? { description } : {}),
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
