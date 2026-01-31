// Settings › Memory (SM-NS-1.9; amended January 2026; user decision; Approved by the user January 2026):
// settings only, in three groups. Viewing and changing facts stays on the memory page.
//
// "What Ordin keeps" and "How Ordin uses memory" are read with `settings.get` of the section
// `what_ordin_remembers` and changed with `settings.change { key, value }`; the desktop program saves them
// and the Memory Service and Ordin Host enforce them (OrdinDesktop DSK-MEM-1). "How memory is shown" is
// this computer's view preference (`ViewPrefs.memoryShown`). A refused change returns the control to its
// stored value; `DSK_PREFERENCES_WRITE_FAILED` offers **Try again**.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-1
import type { BackendHandle } from '../../backend/backend';
import type { ServiceError, SettingView } from '../../service-api/types.gen';
import { GROUP_BY, GROUP_BY_MESSAGE, isGroupBy } from '../fixed/facts';
import { MEMORY_PAGE_SIZES, MEMORY_SHOWN_DEFAULT, type MemoryPageSize, type MemoryShown } from '../../store/view-prefs';

export const KEEP_KEY = 'memory.keep_mode';
export const REMOVAL_KEY = 'memory.suggest_removal_after_days';
export const KEEP_CHOICES = ['on_its_own', 'ask_first'] as const;
export type KeepChoice = (typeof KEEP_CHOICES)[number];
/** `null` is "When Ordin judges they have faded" — today's behaviour. */
export const REMOVAL_CHOICES = [null, 30, 90, 180, 365] as const;
export type RemovalChoice = (typeof REMOVAL_CHOICES)[number];

/** The on/off settings, with their label and one-line explanation, by group. */
export const SOURCE_SWITCHES: readonly { key: string; id: string; hint: string }[] = [
  { key: 'memory.learn_from_conversations', id: 'settings-memory-source-conversations', hint: 'settings-memory-source-conversations-hint' },
  { key: 'memory.learn_from_documents', id: 'settings-memory-source-documents', hint: 'settings-memory-source-documents-hint' },
];
export const USE_SWITCHES: readonly { key: string; id: string; hint: string }[] = [
  { key: 'memory.use_everywhere', id: 'settings-memory-use-everywhere', hint: 'settings-memory-use-everywhere-hint' },
  { key: 'memory.say_which_facts', id: 'settings-memory-say-which', hint: 'settings-memory-say-which-hint' },
];

export const KEEP_MESSAGE: Readonly<Record<KeepChoice, string>> = {
  on_its_own: 'settings-memory-keep-on-its-own',
  ask_first: 'settings-memory-keep-ask-first',
};
export { GROUP_BY, GROUP_BY_MESSAGE };

/** Where "How memory is shown" is kept (the view preferences, `ViewPrefs`). */
export interface ShownPrefs {
  memoryShown(validGroupBy: (v: unknown) => boolean): MemoryShown;
  setMemoryShown(value: MemoryShown): void;
}

export class MemorySettingsModel {
  views = $state.raw<ReadonlyMap<string, SettingView>>(new Map());
  /** What each control shows while its change is in flight. */
  shown = $state.raw<ReadonlyMap<string, unknown>>(new Map());
  error = $state<ServiceError | null>(null);
  display = $state<MemoryShown>({ ...MEMORY_SHOWN_DEFAULT });
  #retry: (() => Promise<boolean>) | null = null;
  readonly #handle: BackendHandle;
  readonly #prefs: ShownPrefs | undefined;

  constructor(handle: BackendHandle, prefs?: ShownPrefs) {
    this.#handle = handle;
    this.#prefs = prefs;
    if (prefs !== undefined) this.display = prefs.memoryShown(isGroupBy);
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('settings.get', { section: 'what_ordin_remembers' });
    if (out.kind === 'ok') {
      this.views = new Map(out.result.settings.map((s) => [s.key, s]));
      this.shown = new Map();
      this.error = null;
    } else this.error = out.error;
  }

  /** Whether the backend answered any setting: the two backend groups are drawn only then. */
  get loaded(): boolean {
    return this.views.size > 0;
  }

  value(key: string): unknown {
    return this.shown.has(key) ? this.shown.get(key) : this.views.get(key)?.value;
  }

  get keep(): KeepChoice {
    const v = this.value(KEEP_KEY);
    return (KEEP_CHOICES as readonly unknown[]).includes(v) ? (v as KeepChoice) : 'on_its_own';
  }

  get removal(): RemovalChoice {
    const v = this.value(REMOVAL_KEY);
    return (REMOVAL_CHOICES as readonly unknown[]).includes(v) ? (v as RemovalChoice) : null;
  }

  isOn(key: string): boolean {
    return this.value(key) === true;
  }

  has(key: string): boolean {
    return this.views.has(key);
  }

  locked(key: string): boolean {
    const view = this.views.get(key);
    return view === undefined || view.set_by === 'company' || !view.person_may_change;
  }

  async change(key: string, value: unknown): Promise<boolean> {
    if (this.locked(key)) return false;
    this.#retry = () => this.change(key, value);
    this.shown = new Map(this.shown).set(key, value);
    const out = await this.#handle.call('settings.change', { key, value });
    const shown = new Map(this.shown);
    shown.delete(key);
    this.shown = shown;
    if (out.kind === 'ok') {
      this.views = new Map(this.views).set(key, out.result);
      this.error = null;
      return true;
    }
    this.error = out.error.code === 'DSK_PREFERENCES_WRITE_FAILED' ? { ...out.error, action: 'try_again' } : out.error;
    return false;
  }

  retry(): void {
    void this.#retry?.();
  }

  // ---- How memory is shown (this computer) --------------------------------------------------------
  #save(change: Partial<MemoryShown>): void {
    this.display = { ...this.display, ...change };
    this.#prefs?.setMemoryShown(this.display);
  }
  setGroupBy(by: string): void {
    if (isGroupBy(by)) this.#save({ groupBy: by });
  }
  setPageSize(size: number): void {
    if ((MEMORY_PAGE_SIZES as readonly number[]).includes(size)) this.#save({ pageSize: size as MemoryPageSize });
  }
  setDetailsOpen(open: boolean): void {
    this.#save({ detailsOpen: open });
  }
}
