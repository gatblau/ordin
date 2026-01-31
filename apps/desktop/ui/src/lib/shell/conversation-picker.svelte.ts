// The conversation picker of the conversation panel's header (ADR-108, SM-CC-1): the live conversation's
// name, the menu of earlier conversations, and the keys of a WAI-ARIA menu button. The key handling is
// free of the DOM so it is tested without one.
//
// spec: ordin-desktop/components/ScreensModule#SM-CC-1
import { formatDate } from '../format/intl';
import type { ConversationSummary } from '../service-api/types.gen';
import { EarlierListModel, type ChainEntry } from '../views/earlier.svelte';

/** The live conversation's name is its first request cut to this many characters. */
export const NAME_LIMIT = 48;
/** The picker lists at most this many earlier conversations; more adds `conv-picker-all`. */
export const EARLIER_LIMIT = 20;

/** `first_request` cut to 48 characters at a word end with "…"; `fallback` (`conv-picker-new`) before the first message. */
export function conversationName(first: string | null | undefined, fallback: string): string {
  if (first === null || first === undefined || first.trim() === '') return fallback;
  const text = first.trim();
  if (text.length <= NAME_LIMIT) return text;
  const head = text.slice(0, NAME_LIMIT);
  // A cut that lands exactly on a word end keeps the whole 48 characters; otherwise back up to the last space.
  const wordEnd = text[NAME_LIMIT] === ' ' ? NAME_LIMIT : head.lastIndexOf(' ');
  return `${(wordEnd > 0 ? head.slice(0, wordEnd) : head).trimEnd()}…`;
}

export interface MenuEntry {
  kind: 'live' | 'earlier' | 'all' | 'none' | 'clear';
  /** The conversation an `earlier` entry opens. */
  conversationId?: string;
  /** What the entry reads, and the start of what a typed character matches. */
  label: string;
  /** Accessible name when it differs from the label. */
  accessibleName?: string;
  /** The second line: `conv-picker-current` or `conv-picker-date`. */
  detail?: string;
  disabled: boolean;
  /** A separator is drawn before this entry. */
  separatorBefore: boolean;
}

export interface MenuMove {
  /** The entry to focus, or `null` for none. */
  focus: number | null;
  close: boolean;
  /** Focus goes back to the button. */
  restoreButton: boolean;
}

const usable = (entries: readonly MenuEntry[]): number[] => entries.flatMap((e, i) => (e.disabled ? [] : [i]));

/** What a key does on the closed button: Enter, Space or Down Arrow opens on the first item, Up Arrow on the last. */
export function buttonKey(key: string, entries: readonly MenuEntry[]): { open: boolean; focus: number | null } {
  const ok = usable(entries);
  if (key === 'Enter' || key === ' ' || key === 'ArrowDown') return { open: true, focus: ok[0] ?? null };
  if (key === 'ArrowUp') return { open: true, focus: ok[ok.length - 1] ?? null };
  return { open: false, focus: null };
}

/** What a key does inside the open menu: Up and Down move and wrap, Home and End, a typed character, Escape, Tab. */
export function menuKey(key: string, current: number, entries: readonly MenuEntry[]): MenuMove | null {
  const ok = usable(entries);
  const at = ok.indexOf(current);
  if (key === 'Escape') return { focus: null, close: true, restoreButton: true };
  if (key === 'Tab') return { focus: null, close: true, restoreButton: false };
  if (ok.length === 0) return null;
  if (key === 'ArrowDown') return { focus: ok[(at + 1) % ok.length] ?? null, close: false, restoreButton: false };
  if (key === 'ArrowUp') return { focus: ok[(at <= 0 ? ok.length : at) - 1] ?? null, close: false, restoreButton: false };
  if (key === 'Home') return { focus: ok[0] ?? null, close: false, restoreButton: false };
  if (key === 'End') return { focus: ok[ok.length - 1] ?? null, close: false, restoreButton: false };
  if (key.length === 1 && key !== ' ') {
    const typed = key.toLocaleLowerCase();
    for (let step = 1; step <= ok.length; step += 1) {
      const index = ok[(Math.max(at, 0) + step) % ok.length];
      if (index !== undefined && entries[index]?.label.toLocaleLowerCase().startsWith(typed)) return { focus: index, close: false, restoreButton: false };
    }
  }
  return null;
}

type Say = (id: string, args?: Record<string, string | number>) => string;

/** The picker's data and open state for one workspace's conversation. */
export class ConversationPickerModel {
  open = $state(false);
  focused = $state(-1);
  readonly list: EarlierListModel;
  readonly #say: Say;

  constructor(list: EarlierListModel, say: Say) {
    this.list = list;
    this.#say = say;
  }

  /** The live conversation (`state: 'current'`) when the list holds one. */
  get live(): ConversationSummary | undefined {
    return this.list.all.find((c) => c.state === 'current');
  }

  /** The button's text. */
  get name(): string {
    return conversationName(this.live?.first_request, this.#say('conv-picker-new'));
  }

  get label(): string {
    return this.#say('conv-picker-label', { name: this.name });
  }

  /** Earlier entries, newest first (one per chain, as the Earlier conversations screen lists them). */
  get earlier(): ChainEntry[] {
    return this.list.entries;
  }

  get entries(): MenuEntry[] {
    const rows: MenuEntry[] = [
      { kind: 'live', label: this.name, detail: this.#say('conv-picker-current'), disabled: false, separatorBefore: false },
    ];
    const shown = this.earlier.slice(0, EARLIER_LIMIT);
    shown.forEach((entry, index) => {
      const title = entry.chain[0]?.first_request ?? entry.head.first_request ?? this.#say('conv-picker-new');
      const date = formatDate(entry.head.started_at, { dateStyle: 'medium' });
      rows.push({
        kind: 'earlier',
        conversationId: entry.head.conversation_id,
        label: title,
        accessibleName: `${title}, ${date}`,
        detail: this.#say('conv-picker-date', { date, count: entry.messageCount }),
        disabled: false,
        separatorBefore: index === 0,
      });
    });
    if (this.earlier.length > EARLIER_LIMIT) {
      rows.push({ kind: 'all', label: this.#say('conv-picker-all', { count: this.earlier.length }), disabled: false, separatorBefore: false });
    } else if (this.earlier.length === 0) {
      rows.push({ kind: 'none', label: this.#say('conv-picker-none'), disabled: true, separatorBefore: true });
    }
    rows.push({ kind: 'clear', label: this.#say('conv-clear'), disabled: false, separatorBefore: true });
    return rows;
  }

  /** Opening sends `ordin.list_conversations` (first page). */
  async show(where: 'first' | 'last'): Promise<void> {
    this.open = true;
    this.focused = where === 'first' ? 0 : this.entries.length - 1;
    await this.list.load();
    // The entries changed with the list; the last item is still the last one.
    if (where === 'last') this.focused = this.entries.length - 1;
  }

  close(): void {
    this.open = false;
    this.focused = -1;
  }
}
