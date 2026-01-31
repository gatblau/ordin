// Releases and updates (ADR-114, ADR-072): the update line and the What's new card in *Needs you*
// (SM-UP-3, SM-UP-4, SM-UP-7), and the state behind Settings › About (SM-UP-2, SM-UP-8).
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-2
// spec: ordin-desktop/components/ScreensModule#SM-UP-3
// spec: ordin-desktop/components/ScreensModule#SM-UP-4
// spec: ordin-desktop/components/ScreensModule#SM-UP-6
// spec: ordin-desktop/components/ScreensModule#SM-UP-7
// spec: ordin-desktop/components/ScreensModule#SM-UP-8
import type { BackendHandle } from '../../backend/backend';
import type { ServiceError, UpdateOffer, UpdateState, WhatsNew } from '../../service-api/types.gen';

/** How often the shell asks for an offer (SM-UP-3, chosen there). */
export const UPDATE_POLL_MS = 30 * 60 * 1000;
/** How often About asks again while a download runs (SM-UP-2.1, chosen there). */
export const DOWNLOAD_POLL_MS = 5000;

/**
 * The offer's state as the generated `UpdateState` holds it (SM-WR-6); `other` only for a value a newer
 * backend sends that this version does not know.
 */
// spec: ordin-desktop/components/ScreensModule#SM-WR-6
export function offerState(offer: Pick<UpdateOffer, 'state'>): UpdateState | 'other' {
  switch (offer.state) {
    case 'downloading':
    case 'ready':
    case 'administrator_needed':
      return offer.state;
    default:
      return 'other';
  }
}

// ---- release notes are plain text (SM-UP-6) ---------------------------------------------------------

export type NoteBlock = { kind: 'text'; lines: string[] } | { kind: 'list'; items: string[] };

/**
 * Line breaks are kept; a line starting with `- ` or `* ` is a list item; nothing else is interpreted.
 */
export function parseNotes(notes: string): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  for (const line of notes.split(/\r?\n/)) {
    const item = /^[-*] (.*)$/.exec(line);
    const last = blocks[blocks.length - 1];
    if (item !== null) {
      if (last?.kind === 'list') last.items.push(item[1] ?? '');
      else blocks.push({ kind: 'list', items: [item[1] ?? ''] });
    } else if (last?.kind === 'text') last.lines.push(line);
    else blocks.push({ kind: 'text', lines: [line] });
  }
  return blocks.filter((b) => (b.kind === 'text' ? b.lines.some((l) => l.trim() !== '') : true));
}

// ---- the update line and What's new (Needs you) ----------------------------------------------------

export class UpdateStore {
  offer = $state.raw<UpdateOffer | null>(null);
  whatsNew = $state.raw<WhatsNew | null>(null);
  /** Hidden with **Not now** until the next start of the program (nothing is stored). */
  hidden = $state(false);
  restarting = $state(false);
  error = $state<ServiceError | null>(null);
  whatsNewError = $state<ServiceError | null>(null);
  readonly #handle: BackendHandle;
  #timer: ReturnType<typeof setInterval> | undefined;
  #started = false;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  /** At start, after the local backend loads: the offer, What's new, and the offer again every 30 minutes. */
  async start(pollMs: number = UPDATE_POLL_MS): Promise<void> {
    if (this.#started) return;
    this.#started = true;
    this.#timer = setInterval(() => void this.refresh(), pollMs);
    await Promise.all([this.refresh(), this.loadWhatsNew()]);
  }

  stop(): void {
    if (this.#timer !== undefined) clearInterval(this.#timer);
    this.#timer = undefined;
  }

  async refresh(): Promise<void> {
    const out = await this.#handle.call('setup.update_offer', {});
    if (out.kind === 'ok') this.offer = out.result.offer ?? null;
  }

  async loadWhatsNew(): Promise<void> {
    const out = await this.#handle.call('setup.whats_new', {});
    if (out.kind === 'ok') this.whatsNew = out.result.whats_new ?? null;
  }

  /** The line shows for a `ready` offer, and for `administrator_needed` without a restart button. */
  get line(): { version: string; kind: 'ready' | 'administrator_needed' } | null {
    if (this.hidden || this.offer === null) return null;
    const s = offerState(this.offer);
    return s === 'ready' || s === 'administrator_needed' ? { version: this.offer.version, kind: s } : null;
  }

  /** The card shows while the notes are unseen. */
  get card(): WhatsNew | null {
    return this.whatsNew !== null && !this.whatsNew.seen ? this.whatsNew : null;
  }

  notNow(): void {
    this.hidden = true;
  }

  /** **Restart to update**: sends `setup.install_update`; the quit flow is the program's (SM-UP-3.2). */
  async restart(): Promise<void> {
    if (this.restarting) return;
    this.restarting = true;
    this.error = null;
    const out = await this.#handle.call('setup.install_update', {});
    if (out.kind === 'ok') return;
    this.restarting = false;
    if (out.error.code === 'SETUP_UPDATE_NONE') {
      this.offer = null;
      return;
    }
    this.error = out.error;
  }

  /** **Got it**: marks the notes seen; a refusal keeps the card. */
  async gotIt(): Promise<void> {
    const card = this.card;
    if (card === null) return;
    const out = await this.#handle.call('setup.whats_new_seen', { version: card.version });
    if (out.kind === 'ok') {
      this.whatsNew = { ...card, seen: true };
      this.whatsNewError = null;
    } else this.whatsNewError = out.error;
  }
}

const stores = new WeakMap<BackendHandle, UpdateStore>();
/** The one store of a backend handle; created on first use. */
export function updateStoreFor(handle: BackendHandle): UpdateStore {
  let store = stores.get(handle);
  if (store === undefined) {
    store = new UpdateStore(handle);
    stores.set(handle, store);
  }
  return store;
}

// ---- Settings › About -------------------------------------------------------------------------------

export type AboutStatus =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'up_to_date' }
  | { kind: 'downloading'; version: string }
  | { kind: 'ready'; version: string }
  | { kind: 'administrator_needed'; version: string }
  | { kind: 'could_not_check' };

export class AboutModel {
  status = $state.raw<AboutStatus>({ kind: 'idle' });
  version = $state('');
  error = $state<ServiceError | null>(null);
  /** The side panel: `null` closed, otherwise the version and notes (or no notes). */
  panel = $state.raw<{ version: string; notes: string } | { none: true } | null>(null);
  readonly #handle: BackendHandle;
  readonly #store: UpdateStore | null;
  readonly #appVersion: () => Promise<string>;
  #timer: ReturnType<typeof setInterval> | undefined;

  constructor(handle: BackendHandle, appVersion: () => Promise<string>, store: UpdateStore | null = null) {
    this.#handle = handle;
    this.#appVersion = appVersion;
    this.#store = store;
  }

  /** On opening: the application version and `setup.update_offer` once (SM-UP-2.2). */
  async open(): Promise<void> {
    this.version = await this.#appVersion().catch(() => '');
    const out = await this.#handle.call('setup.update_offer', {});
    if (out.kind === 'ok') this.#draw(out.result.offer ?? null, false);
    void this.#store?.refresh();
  }

  close(): void {
    this.#stopPolling();
  }

  get busy(): boolean {
    return this.status.kind === 'checking';
  }

  #draw(offer: UpdateOffer | null, fromCheck: boolean): void {
    if (offer === null) {
      this.status = fromCheck ? { kind: 'up_to_date' } : { kind: 'idle' };
      this.#stopPolling();
      return;
    }
    const s = offerState(offer);
    if (s === 'downloading') {
      this.status = { kind: 'downloading', version: offer.version };
      this.#startPolling();
    } else if (s === 'ready') {
      this.status = { kind: 'ready', version: offer.version };
      this.#stopPolling();
    } else if (s === 'administrator_needed') {
      this.status = { kind: 'administrator_needed', version: offer.version };
      this.#stopPolling();
    }
  }

  #startPolling(): void {
    if (this.#timer !== undefined) return;
    this.#timer = setInterval(() => void this.#poll(), DOWNLOAD_POLL_MS);
  }
  #stopPolling(): void {
    if (this.#timer !== undefined) clearInterval(this.#timer);
    this.#timer = undefined;
  }
  async #poll(): Promise<void> {
    const out = await this.#handle.call('setup.update_offer', {});
    if (out.kind === 'ok') this.#draw(out.result.offer ?? null, false);
  }

  /** **Check for updates** (SM-UP-2.1). */
  async check(): Promise<void> {
    if (this.busy) return;
    this.status = { kind: 'checking' };
    this.error = null;
    const out = await this.#handle.call('setup.check_update', {});
    if (out.kind === 'ok') {
      this.#draw(out.result.offer ?? null, true);
      void this.#store?.refresh();
      return;
    }
    this.status = { kind: 'idle' };
    if (out.error.code === 'SETUP_UPDATE_UNREACHABLE' || out.error.code === 'SETUP_UPDATE_SIGNATURE_INVALID') {
      this.status = { kind: 'could_not_check' };
    } else this.error = out.error;
  }

  /** **What's new**: reads the notes; never marks them seen. */
  async openWhatsNew(): Promise<void> {
    const out = await this.#handle.call('setup.whats_new', {});
    if (out.kind !== 'ok') {
      this.error = out.error;
      return;
    }
    const w = out.result.whats_new;
    this.panel = w === null || w === undefined ? { none: true } : { version: w.version, notes: w.notes };
  }

  /** The same side panel for a personal server's update (SM-UP-5.1). */
  showNotes(version: string, notes: string): void {
    this.panel = { version, notes };
  }

  closePanel(): void {
    this.panel = null;
  }

  /** **Restart to update** beside the ready line acts as SM-UP-3.2. */
  async restart(): Promise<void> {
    if (this.#store === null) return;
    await this.#store.restart();
    this.error = this.#store.error;
  }
}
