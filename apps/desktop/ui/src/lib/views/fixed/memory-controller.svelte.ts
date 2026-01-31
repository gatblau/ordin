// What Ordin remembers for one workspace: the Facts, Things and To check tabs (ADR-111, SM-SF-4,
// SM-SF-5). Everything changes only when the person presses a button; nothing here is offered to Ordin
// or to an agent. The owner's sentence is drawn by `ErrorLine`, never the code.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-4
// spec: ordin-desktop/components/ScreensModule#SM-SF-5
// spec: ordin-desktop/components/ScreensModule#SM-CU-11
import {
  citationsToConfirm,
  confirmCitation,
  discardHeld,
  heldWrites,
  keepHeld,
  type HeldWrite,
} from '../../backend/wiring';
import { t } from '../../i18n/fluent';
import type {
  FactHistoryView,
  FactView,
  LinkView,
  OriginsView,
  ServiceError,
  ThingCount,
  ThingDetail,
  ThingView,
} from '../../service-api/types.gen';
import { AgentNames } from './facts';
import { pageIdsOf, PageTitles } from './page-titles.svelte';
import { Announcer, settle, type Handle } from './util';

export type MemoryTab = 'facts' | 'things' | 'to_check';
export const MEMORY_GET_CHUNK = 100;
const SEARCH_DEBOUNCE_MS = 150;

/** A confirmation waiting for the person: forgetting a fact, a link or everything about a thing. */
export type PendingForget =
  | { kind: 'fact'; id: string; summaries: number }
  | { kind: 'link'; link: LinkView }
  | { kind: 'thing'; id: string; name: string; counts: ThingCount; changed: boolean };

export class MemoryController {
  tab = $state<MemoryTab>('facts');
  facts = $state.raw<FactView[]>([]);
  factsCursor = $state<string | null>(null);
  replaced = $state.raw<Record<string, string>>({});
  names = $state.raw(new AgentNames());
  things = $state.raw<ThingView[]>([]);
  thingsCursor = $state<string | null>(null);
  thingSearch = $state('');
  detail = $state.raw<ThingDetail | null>(null);
  history = $state.raw<{ fact: FactView; view: FactHistoryView } | null>(null);
  origins = $state.raw<Record<string, OriginsView>>({});
  /** `null`: not wired, the section is not drawn (SM-SF-4.1). */
  held = $state.raw<HeldWrite[] | null>(null);
  citations = $state.raw<FactView[] | null>(null);
  pending = $state.raw<PendingForget | null>(null);
  /** The error of the last action, and the entry it belongs on (`''` for the screen). */
  error = $state.raw<{ error: ServiceError; on: string } | null>(null);
  readonly announcer = new Announcer();
  /** The id of the entry focus should move to after an outcome (SM-SF-4 accessibility). */
  focusAfter = $state<string | null>(null);
  /** The pages that set the links drawn here, each read once while the view is open (SM-CU-11). */
  readonly pages: PageTitles;
  readonly #handle: Handle;
  readonly workspaceId: string;
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(handle: Handle, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
    this.pages = new PageTitles(handle);
  }

  // ---- derived -----------------------------------------------------------------------------

  /** Both sections not wired: no tab and no Facts line (SM-SF-4.1). */
  get toCheckDrawn(): boolean {
    return this.held !== null || this.citations !== null;
  }
  get toCheckCount(): number {
    return (this.held?.length ?? 0) + (this.citations?.length ?? 0);
  }

  // ---- loading -----------------------------------------------------------------------------

  async load(): Promise<void> {
    await Promise.all([this.loadAgents(), this.loadFacts(), this.loadToCheck()]);
  }

  async loadAgents(): Promise<void> {
    const out = await this.#handle.call('agents.list', { workspace_id: this.workspaceId });
    if (out.kind === 'ok') this.names = new AgentNames(out.result);
  }

  async loadFacts(): Promise<void> {
    const out = await this.#handle.call('memory.list', { workspace_id: this.workspaceId });
    if (out.kind === 'error') {
      this.error = { error: out.error, on: '' };
      return;
    }
    this.facts = out.result.items;
    this.factsCursor = out.result.next_cursor ?? null;
    await this.#loadReplaced(out.result.items);
  }

  async moreFacts(): Promise<void> {
    if (this.factsCursor === null) return;
    const out = await this.#handle.call('memory.list', { workspace_id: this.workspaceId, cursor: this.factsCursor });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: '' });
    this.facts = [...this.facts, ...out.result.items];
    this.factsCursor = out.result.next_cursor ?? null;
    await this.#loadReplaced(out.result.items);
  }

  /** The statement of the fact each line replaces; a fact no longer found drops its line (never an error). */
  async #loadReplaced(facts: FactView[]): Promise<void> {
    const ids = [...new Set(facts.map((f) => f.replaces).filter((x): x is string => typeof x === 'string'))];
    if (ids.length === 0) return;
    const got = await this.#handle.call('memory.get', { ids, workspace_id: this.workspaceId });
    if (got.kind !== 'ok') return;
    const next = { ...this.replaced };
    for (const f of got.result.facts) next[f.id] = f.statement;
    this.replaced = next;
  }

  /** SM-SF-4.1 and SM-SF-5.1: both sections are read on opening and on each `workspace_changed`. */
  async loadToCheck(): Promise<void> {
    const [held, cites] = await Promise.all([heldWrites(this.#handle, this.workspaceId), citationsToConfirm(this.#handle, this.workspaceId)]);
    const h = settle(held);
    this.held = h.ok ? h.value : null;
    if (!h.ok && 'error' in h) this.error = { error: h.error, on: 'held' };
    const c = settle(cites);
    if (!c.ok) {
      this.citations = null;
      if ('error' in c) this.error = { error: c.error, on: 'citations' };
      return;
    }
    const facts: FactView[] = [];
    for (let i = 0; i < c.value.length; i += MEMORY_GET_CHUNK) {
      const chunk = c.value.slice(i, i + MEMORY_GET_CHUNK);
      const got = await this.#handle.call('memory.get', { ids: chunk, workspace_id: this.workspaceId });
      if (got.kind === 'ok') {
        // The order the owner gave; an id `memory.get` did not return (forgotten meanwhile) is not drawn.
        const byId = new Map(got.result.facts.map((f) => [f.id, f]));
        for (const id of chunk) {
          const f = byId.get(id);
          if (f !== undefined) facts.push(f);
        }
      }
    }
    this.citations = facts;
  }

  // ---- facts -------------------------------------------------------------------------------

  async keep(fact: FactView): Promise<void> {
    const out = await this.#handle.call('memory.keep', { id: fact.id });
    if (out.kind === 'ok') {
      this.facts = this.facts.map((f) => (f.id === fact.id ? out.result : f));
      this.announcer.say(t('memory-kept'));
      return;
    }
    if (out.error.code === 'MEM_ALREADY_KEPT') {
      await this.loadFacts();
      return;
    }
    this.error = { error: out.error, on: fact.id };
  }

  async keepChecked(fact: FactView): Promise<void> {
    const out = await this.#handle.call('memory.keep_checked', { id: fact.id });
    if (out.kind === 'ok') this.facts = this.facts.map((f) => (f.id === fact.id ? out.result : f));
    else this.error = { error: out.error, on: fact.id };
  }

  /** Forget asks first (`memory.forget_count`), then confirms (`memory.forget`). */
  async askForgetFact(id: string): Promise<void> {
    const out = await this.#handle.call('memory.forget_count', { id });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: id });
    this.pending = { kind: 'fact', id, summaries: out.result.summaries };
  }

  async openHistory(fact: FactView): Promise<void> {
    const out = await this.#handle.call('memory.fact_history', { id: fact.id });
    if (out.kind === 'ok') {
      this.history = { fact, view: out.result };
      await this.pages.read(pageIdsOf([], out.result.links));
    } else this.error = { error: out.error, on: fact.id };
  }

  async openOrigins(id: string): Promise<void> {
    const out = await this.#handle.call('memory.fact_origins', { id });
    if (out.kind === 'ok') this.origins = { ...this.origins, [id]: out.result };
    else this.error = { error: out.error, on: id };
  }

  // ---- things ------------------------------------------------------------------------------

  async loadThings(): Promise<void> {
    const search = this.thingSearch.trim();
    const out = await this.#handle.call('memory.things', { workspace_id: this.workspaceId, ...(search === '' ? {} : { search }) });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: '' });
    this.things = out.result.items;
    this.thingsCursor = out.result.next_cursor ?? null;
  }

  async moreThings(): Promise<void> {
    if (this.thingsCursor === null) return;
    const search = this.thingSearch.trim();
    const out = await this.#handle.call('memory.things', { workspace_id: this.workspaceId, cursor: this.thingsCursor, ...(search === '' ? {} : { search }) });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: '' });
    this.things = [...this.things, ...out.result.items];
    this.thingsCursor = out.result.next_cursor ?? null;
  }

  /** Debounced 150 ms (ADR-111 step 3). */
  searchThings(text: string): void {
    this.thingSearch = text;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.loadThings(), SEARCH_DEBOUNCE_MS);
  }

  async openThing(id: string): Promise<void> {
    const out = await this.#handle.call('memory.thing', { id, workspace_id: this.workspaceId });
    if (out.kind === 'ok') {
      this.detail = out.result;
      this.tab = 'things';
      await this.pages.read(pageIdsOf([], out.result.links));
    } else {
      this.error = { error: out.error, on: id };
    }
  }

  closeThing(): void {
    this.detail = null;
  }

  async askForgetThing(id: string, name: string): Promise<void> {
    const out = await this.#handle.call('memory.thing_count', { id, workspace_id: this.workspaceId });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: id });
    this.pending = { kind: 'thing', id, name, counts: out.result, changed: false };
  }

  askForgetLink(link: LinkView): void {
    this.pending = { kind: 'link', link };
  }

  cancelPending(): void {
    this.pending = null;
  }

  /** Confirming whatever `pending` holds. */
  async confirmPending(): Promise<void> {
    const p = this.pending;
    if (p === null) return;
    if (p.kind === 'fact') {
      const out = await this.#handle.call('memory.forget', { id: p.id });
      if (out.kind === 'error') return void (this.error = { error: out.error, on: p.id });
      this.facts = this.facts.filter((f) => f.id !== p.id);
      this.pending = null;
      this.announcer.say(t('memory-forgotten'));
    } else if (p.kind === 'link') {
      const out = await this.#handle.call('memory.forget_link', { id: p.link.id });
      if (out.kind === 'error') return void (this.error = { error: out.error, on: p.link.id });
      this.pending = null;
      this.announcer.say(t('memory-forgotten'));
      if (this.detail !== null) await this.openThing(this.detail.thing.id);
    } else {
      const out = await this.#handle.call('memory.forget_thing', { id: p.id, workspace_id: this.workspaceId, expected: p.counts });
      if (out.kind === 'error') {
        if (out.error.code === 'MEM_THING_COUNT_CHANGED') {
          // Re-read the counts and show the same dialog again, still waiting for the person.
          const again = await this.#handle.call('memory.thing_count', { id: p.id, workspace_id: this.workspaceId });
          if (again.kind === 'ok') this.pending = { ...p, counts: again.result, changed: true };
          return;
        }
        return void (this.error = { error: out.error, on: p.id });
      }
      this.pending = null;
      this.detail = null;
      this.announcer.say(t('memory-thing-forgotten', { thing: p.name }));
      await this.loadThings();
    }
  }

  async correctLink(link: LinkView, change: { kind: string; from: string; to: string }): Promise<boolean> {
    const out = await this.#handle.call('memory.correct_link', { id: link.id, kind: change.kind, from: change.from, to: change.to });
    if (out.kind === 'error') {
      this.error = { error: out.error, on: link.id };
      return false;
    }
    if (this.detail !== null) await this.openThing(this.detail.thing.id);
    return true;
  }

  async sameAs(keep: { id: string; name: string }, merge: { id: string; name: string }): Promise<boolean> {
    const out = await this.#handle.call('memory.same_as', { workspace_id: this.workspaceId, keep: keep.id, merge: merge.id });
    if (out.kind === 'error') {
      this.error = { error: out.error, on: merge.id };
      return false;
    }
    await this.openThing(out.result.id);
    this.announcer.say(t('memory-same-as-done', { merged: merge.name, kept: keep.name }));
    return true;
  }

  // ---- To check (SM-SF-4, SM-SF-5) ------------------------------------------------------------

  /** **Keep** sends the `content_hash` of the entry exactly as drawn. */
  async keepHeldWrite(entry: HeldWrite): Promise<void> {
    const out = settle(await keepHeld(this.#handle, this.workspaceId, entry.id, entry.content_hash));
    if (out.ok) {
      this.#removeHeld(entry.id);
      this.announcer.say(t('memory-held-kept'));
      return;
    }
    if (!('error' in out)) return;
    this.error = { error: out.error, on: entry.id };
    // MEM_HELD_CHANGED and MEM_NOT_HELD: the owner's sentence stays on the entry, the list is read again.
    if (out.error.code === 'MEM_HELD_CHANGED' || out.error.code === 'MEM_NOT_HELD') await this.#reloadHeld();
  }

  async discardHeldWrite(entry: HeldWrite): Promise<void> {
    const out = settle(await discardHeld(this.#handle, this.workspaceId, entry.id));
    if (out.ok) {
      this.#removeHeld(entry.id);
      this.announcer.say(t('memory-held-discarded'));
      return;
    }
    if (!('error' in out)) return;
    this.error = { error: out.error, on: entry.id };
    if (out.error.code === 'MEM_NOT_HELD') await this.#reloadHeld();
  }

  async confirmSource(fact: FactView): Promise<void> {
    const out = settle(await confirmCitation(this.#handle, this.workspaceId, fact.id));
    if (out.ok) {
      this.citations = (this.citations ?? []).filter((f) => f.id !== fact.id);
      this.announcer.say(t('memory-citation-confirmed'));
      return;
    }
    if (!('error' in out)) return;
    this.error = { error: out.error, on: fact.id };
    if (out.error.code === 'MEM_NOT_HELD') await this.loadToCheck();
  }

  #removeHeld(id: string): void {
    const rest = (this.held ?? []).filter((h) => h.id !== id);
    this.held = rest;
    this.focusAfter = rest[0]?.id ?? 'memory-held-title';
  }

  async #reloadHeld(): Promise<void> {
    const held = settle(await heldWrites(this.#handle, this.workspaceId));
    if (held.ok) this.held = held.value;
  }

  errorOn(id: string): ServiceError | null {
    return this.error !== null && this.error.on === id ? this.error.error : null;
  }
}

