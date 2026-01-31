// Settings › Language (SM-LG-2), the Language screen (SM-LG-3), Make a new language… (SM-LG-4), Import…
// (SM-LG-5) and Export… (SM-LG-6). Every command goes to the `BackendHandle` the model is built with, so
// a personal server's languages (SM-LG-7) use the same code bound to that server.
//
// spec: ordin-desktop/components/ScreensModule#SM-LG-2
// spec: ordin-desktop/components/ScreensModule#SM-LG-3
// spec: ordin-desktop/components/ScreensModule#SM-LG-4
// spec: ordin-desktop/components/ScreensModule#SM-LG-5
// spec: ordin-desktop/components/ScreensModule#SM-LG-6
// spec: ordin-desktop/components/ScreensModule#SM-LG-7
import type { BackendHandle } from '../../backend/backend';
import type {
  ImportReport,
  LanguageSummary,
  MessageOnly,
  MessageRow,
  OfferedLocale,
  OrdinEstimateLanguageDraftResult,
  ServiceError,
} from '../../service-api/types.gen';

/** A language file larger than this is not read (SM-LG-5). */
export const MAX_LANGUAGE_FILE_BYTES = 2_097_152;
/** Rows per page of the Language screen. */
export const MESSAGE_PAGE = 100;
/** Ids sent per `localisation.mark_reviewed` call. */
export const MARK_REVIEWED_MAX = 200;

/** "Japanese — ja": the language's name, then its tag. */
export function languageLabel(offered: { native_name?: string | null; locale: string }): string {
  return offered.native_name ? `${offered.native_name} — ${offered.locale}` : offered.locale;
}

export interface StateLine {
  messageId: string;
  args: Record<string, number>;
  /** An action beside the line: stop drafting, or ask Ordin to draft what is still in English. */
  action: 'stop' | 'ask_ordin' | null;
}

/** The state line of a language: the first match of SM-LG-2. */
export function stateLine(s: LanguageSummary): StateLine {
  if (s.draft !== undefined && s.draft !== null) {
    return { messageId: 'language-state-drafting', args: { done: s.draft.done, total: s.draft.total }, action: 'stop' };
  }
  if (s.needs_review > 0) return { messageId: 'language-state-needs-checking', args: { count: s.needs_review }, action: null };
  if (s.state === 'needs_review') return { messageId: 'language-state-ready-to-use', args: {}, action: null };
  if (s.needs_changing > 0) return { messageId: 'language-state-needs-changing', args: { count: s.needs_changing }, action: null };
  if (s.in_english > 0) return { messageId: 'language-state-in-english', args: { count: s.in_english }, action: 'ask_ordin' };
  return { messageId: 'language-state-in-use', args: {}, action: null };
}

/** A person's language: made or imported, or a shipped one the person has edited. */
export function isYours(s: LanguageSummary): boolean {
  return s.kind === 'made' || s.kind === 'imported' || (s.kind === 'shipped' && s.edited > 0);
}

export class LanguageSettingsModel {
  active = $state('');
  following = $state(true);
  offered = $state.raw<OfferedLocale[]>([]);
  languages = $state.raw<LanguageSummary[]>([]);
  areas = $state.raw<string[]>([]);
  error = $state<ServiceError | null>(null);
  /** A polite announcement (import report). */
  announcement = $state('');
  report = $state.raw<ImportReport | null>(null);
  showLeftOut = $state(false);
  /** A file waiting for the person to choose **Replace**. */
  pendingReplace = $state.raw<{ file_name: string; ftl: string; language: string } | null>(null);
  /** **Export…** is choosing a language. */
  exporting = $state(false);
  /** A made-language draft being set up. */
  make = $state.raw<MakeLanguageModel | null>(null);
  /** The server this model is bound to is an agent-profile one: `LOC_LANGUAGES_UNAVAILABLE`. */
  unavailable = $state<ServiceError | null>(null);
  readonly handle: BackendHandle;
  readonly #saver: BackendHandle;

  /** `saver` is the local backend, which opens the save dialog (`files.save_export`). */
  constructor(handle: BackendHandle, saver: BackendHandle = handle) {
    this.handle = handle;
    this.#saver = saver;
  }

  async load(): Promise<void> {
    const [settings, list] = await Promise.all([this.handle.call('localisation.get_settings', {}), this.handle.call('localisation.list_languages', {})]);
    if (settings.kind === 'ok') {
      this.active = settings.result.active.locale;
      this.following = settings.result.following_operating_system;
      this.offered = settings.result.offered;
      this.error = null;
    } else this.#fail(settings.error);
    if (list.kind === 'ok') {
      this.languages = list.result.languages;
      this.areas = list.result.areas;
    } else this.#fail(list.error);
  }

  #fail(error: ServiceError): void {
    if (error.code === 'LOC_LANGUAGES_UNAVAILABLE') this.unavailable = error;
    else this.error = error;
  }

  summaryOf(locale: string): LanguageSummary | undefined {
    return this.languages.find((l) => l.locale === locale);
  }

  /** The picker's languages: every offered language except one marked not choosable. */
  get choices(): Array<OfferedLocale & { kind: LanguageSummary['kind'] | null }> {
    return this.offered
      .filter((o) => this.summaryOf(o.locale)?.choosable !== false)
      .map((o) => ({ ...o, kind: this.summaryOf(o.locale)?.kind ?? null }));
  }

  get yours(): LanguageSummary[] {
    return this.languages.filter(isYours);
  }

  /** **Follow this computer** sends `locale: null`. */
  async choose(locale: string | null): Promise<void> {
    const out = await this.handle.call('localisation.set_locale', { locale });
    if (out.kind === 'ok') {
      this.active = out.result.locale;
      this.following = locale === null;
      this.error = null;
    } else this.#fail(out.error);
  }

  async stopDrafting(locale: string): Promise<void> {
    const out = await this.handle.call('ordin.stop_language_draft', { locale });
    if (out.kind === 'error') this.error = out.error;
    await this.load();
  }

  openMake(prefill?: { locale: string; name: string }): void {
    this.make = new MakeLanguageModel(this.handle, prefill);
  }
  closeMake(): void {
    this.make = null;
  }

  // ---- Import… -------------------------------------------------------------------------------------

  /** Sends the file's text, or draws the too-large sentence and sends nothing (SM-LG-5). */
  async importFile(file: { name: string; size: number; text: () => Promise<string> }, replace = false): Promise<ImportReport | null> {
    if (file.size > MAX_LANGUAGE_FILE_BYTES) {
      this.fileTooLarge = true;
      return null;
    }
    this.fileTooLarge = false;
    const ftl = await file.text();
    return this.#import(file.name, ftl, replace);
  }
  fileTooLarge = $state(false);

  async #import(file_name: string, ftl: string, replace: boolean): Promise<ImportReport | null> {
    const out = await this.handle.call('localisation.import_language', { file_name, ftl, replace });
    if (out.kind === 'ok') {
      this.report = out.result;
      this.pendingReplace = null;
      this.error = null;
      await this.load();
      return out.result;
    }
    if (out.error.code === 'LOC_LANGUAGE_EXISTS' && !replace) {
      const known = this.languages.find((l) => file_name.includes(l.locale));
      this.pendingReplace = { file_name, ftl, language: known?.native_name ?? known?.locale ?? file_name };
      return null;
    }
    this.error = out.error;
    return null;
  }

  async confirmReplace(): Promise<ImportReport | null> {
    const pending = this.pendingReplace;
    if (pending === null) return null;
    return this.#import(pending.file_name, pending.ftl, true);
  }
  cancelReplace(): void {
    this.pendingReplace = null;
  }

  // ---- Export… -------------------------------------------------------------------------------------

  /** The languages that can be exported: every one but English (UK). */
  get exportable(): OfferedLocale[] {
    return this.offered.filter((o) => o.locale !== 'en-GB');
  }

  /** Reads the language file, then `files.save_export` on the local backend; cancelling shows nothing. */
  async exportLanguage(locale: string): Promise<void> {
    this.exporting = false;
    const out = await this.handle.call('localisation.export_language', { locale });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    const saved = await this.#saver.call('files.save_export', { file_name: out.result.file_name, text: out.result.ftl });
    if (saved.kind === 'error' && saved.error.code !== 'DSK_DIALOG_CANCELLED') this.error = saved.error;
  }
}

/** Make a new language… (SM-LG-4): name and tag, then the estimate, then **Draft it**. */
export class MakeLanguageModel {
  name = $state('');
  tag = $state('');
  estimate = $state.raw<OrdinEstimateLanguageDraftResult | null>(null);
  error = $state<ServiceError | null>(null);
  /** The field the error concerns, for focus. */
  errorField = $state<'name' | 'tag'>('tag');
  busy = $state(false);
  done = $state(false);
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle, prefill?: { locale: string; name: string }) {
    this.#handle = handle;
    if (prefill !== undefined) {
      this.tag = prefill.locale;
      this.name = prefill.name;
    }
  }

  get validFields(): boolean {
    const n = this.name.trim().length;
    const t = this.tag.trim().length;
    return n >= 1 && n <= 60 && t >= 2 && t <= 35;
  }

  /** **Next** sends the estimate request; nothing is drafted before **Draft it**. */
  async next(): Promise<boolean> {
    if (!this.validFields) return false;
    this.busy = true;
    const out = await this.#handle.call('ordin.estimate_language_draft', { locale: this.tag.trim() });
    this.busy = false;
    if (out.kind === 'ok') {
      this.estimate = out.result;
      this.error = null;
      return true;
    }
    this.#fail(out.error);
    return false;
  }

  async draft(): Promise<boolean> {
    if (this.estimate === null) return false;
    this.busy = true;
    const out = await this.#handle.call('ordin.draft_language', { locale: this.tag.trim(), name: this.name.trim() });
    this.busy = false;
    if (out.kind === 'ok') {
      this.done = true;
      return true;
    }
    this.#fail(out.error);
    return false;
  }

  #fail(error: ServiceError): void {
    this.error = error;
    this.errorField = error.code === 'LOC_TAG_INVALID' || error.code === 'LOC_LANGUAGE_EXISTS' ? 'tag' : 'name';
  }
}

// ---- the Language screen ---------------------------------------------------------------------------

export interface Editing {
  id: string;
  text: string;
  error: ServiceError | null;
}

export class LanguageScreenModel {
  summary = $state.raw<LanguageSummary | null>(null);
  areas = $state.raw<string[]>([]);
  rows = $state.raw<MessageRow[]>([]);
  next = $state<string | null>(null);
  text = $state('');
  area = $state('');
  only = $state<MessageOnly | null>(null);
  editing = $state.raw<Editing | null>(null);
  error = $state<ServiceError | null>(null);
  confirm = $state<'restore_all' | 'remove' | null>(null);
  /** Set when the language has been removed: the tab closes. */
  removed = $state(false);
  announcement = $state('');
  readonly locale: string;
  readonly handle: BackendHandle;

  constructor(handle: BackendHandle, locale: string, only: MessageOnly | null = null) {
    this.handle = handle;
    this.locale = locale;
    this.only = only;
  }

  async load(): Promise<void> {
    const [list, messages] = await Promise.all([this.handle.call('localisation.list_languages', {}), this.#page(null)]);
    if (list.kind === 'ok') {
      this.summary = list.result.languages.find((l) => l.locale === this.locale) ?? null;
      this.areas = list.result.areas;
    } else this.error = list.error;
    if (messages !== null) {
      this.rows = messages.items;
      this.next = messages.next_cursor ?? null;
      this.announcement = String(messages.items.length);
    }
  }

  async #page(cursor: string | null) {
    const out = await this.handle.call('localisation.list_messages', {
      locale: this.locale,
      ...(this.text.trim() === '' ? {} : { text: this.text.trim().slice(0, 200) }),
      ...(this.area === '' ? {} : { area: this.area }),
      ...(this.only === null ? {} : { only: this.only }),
      ...(cursor === null ? {} : { cursor }),
    });
    if (out.kind === 'ok') return out.result;
    this.error = out.error;
    return null;
  }

  /** Search text, area and filter changes reload the first page. */
  async refind(): Promise<void> {
    const page = await this.#page(null);
    if (page === null) return;
    this.rows = page.items;
    this.next = page.next_cursor ?? null;
    this.announcement = String(page.items.length);
  }

  async showMore(): Promise<void> {
    if (this.next === null) return;
    const page = await this.#page(this.next);
    if (page === null) return;
    this.rows = [...this.rows, ...page.items];
    this.next = page.next_cursor ?? null;
  }

  /** One filter at a time (`aria-pressed`); pressing the active one clears it. */
  async toggleOnly(only: MessageOnly): Promise<void> {
    this.only = this.only === only ? null : only;
    await this.refind();
  }

  startEdit(row: MessageRow): void {
    this.editing = { id: row.message_id, text: row.text ?? row.english, error: null };
  }
  cancelEdit(): void {
    this.editing = null;
  }

  /** **Save**: a refusal keeps the person's text and the box; the saved text is unchanged. */
  async save(text: string): Promise<boolean> {
    const editing = this.editing;
    if (editing === null) return false;
    const out = await this.handle.call('localisation.edit_message', { locale: this.locale, message_id: editing.id, ftl: text });
    if (out.kind === 'ok') {
      this.rows = this.rows.map((r) => (r.message_id === editing.id ? out.result : r));
      this.editing = null;
      return true;
    }
    this.editing = { ...editing, text, error: out.error };
    return false;
  }

  async restore(row: MessageRow): Promise<void> {
    const out = await this.handle.call('localisation.restore_message', { locale: this.locale, message_id: row.message_id });
    if (out.kind === 'ok') this.rows = this.rows.map((r) => (r.message_id === row.message_id ? out.result : r));
    else this.error = out.error;
  }

  async markChecked(ids: string[]): Promise<void> {
    for (let i = 0; i < ids.length; i += MARK_REVIEWED_MAX) {
      const out = await this.handle.call('localisation.mark_reviewed', { locale: this.locale, message_ids: ids.slice(i, i + MARK_REVIEWED_MAX) });
      if (out.kind === 'ok') this.summary = out.result;
      else {
        this.error = out.error;
        return;
      }
    }
    await this.refind();
  }

  get shownUnreviewedSafety(): string[] {
    return this.rows.filter((r) => r.safety && !r.reviewed).map((r) => r.message_id);
  }

  /** **Use this language** is unavailable while approval wording still needs checking. */
  get canUse(): boolean {
    return this.summary !== null && this.summary.state === 'needs_review' && this.summary.needs_review === 0;
  }

  async use(): Promise<void> {
    if (!this.canUse) return;
    const out = await this.handle.call('localisation.confirm_language', { locale: this.locale });
    if (out.kind === 'ok') this.summary = out.result;
    else this.error = out.error;
  }

  async restoreAll(): Promise<void> {
    this.confirm = null;
    const out = await this.handle.call('localisation.restore_language', { locale: this.locale });
    if (out.kind === 'ok') {
      this.summary = out.result;
      await this.refind();
    } else this.error = out.error;
  }

  async removeLanguage(): Promise<void> {
    this.confirm = null;
    const out = await this.handle.call('localisation.remove_language', { locale: this.locale });
    if (out.kind === 'ok') this.removed = true;
    else this.error = out.error;
  }
}
