// What Ordin remembers for one workspace: the Facts, Things and To check tabs (ADR-111, SM-SF-4,
// SM-SF-5). Everything changes only when the person presses a button; nothing here is offered to Ordin
// or to an agent. The owner's sentence is drawn by `ErrorLine`, never the code.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-4
// spec: ordin-desktop/components/ScreensModule#SM-SF-5
// spec: ordin-desktop/components/ScreensModule#SM-CU-11
// spec: ordin-desktop/components/ScreensModule#SM-SF-14
// spec: ordin-desktop/components/ScreensModule#SM-SF-15
// spec: ordin-desktop/components/ScreensModule#SM-SF-17
// spec: ordin-desktop/components/ScreensModule#SM-SF-19
// spec: ordin-desktop/components/ScreensModule#SM-SF-20
// spec: ordin-desktop/components/ScreensModule#SM-SF-22
// spec: ordin-desktop/components/ScreensModule#SM-SF-23
// spec: ordin-desktop/components/ScreensModule#SM-SF-27
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
  DiagramView,
  FactHistoryView,
  FactView,
  LinkView,
  ObjectInput,
  OriginsView,
  ServiceError,
  TermView,
  ThingCount,
  ThingDetail,
  ThingRef,
  ThingView,
} from '../../service-api/types.gen';
import {
  AgentNames,
  filterFacts,
  GROUP_PAGE,
  GROUPS_PAGE,
  groupFacts,
  isGroupBy,
  NO_FILTERS,
  notPassage,
  topicsByName,
  worthListing,
  type FactFilters,
  type FactGroup,
  type GroupBy,
  type TopicMap,
} from './facts';
import { pageIdsOf, PageTitles } from './page-titles.svelte';
import { Announcer, settle, type Handle } from './util';

export type MemoryTab = 'facts' | 'things' | 'words' | 'to_check';
export const MEMORY_GET_CHUNK = 100;
const SEARCH_DEBOUNCE_MS = 150;

/** A confirmation waiting for the person: forgetting a fact, a link or everything about a thing. */
export type PendingForget =
  | { kind: 'fact'; id: string; summaries: number }
  | { kind: 'link'; link: LinkView }
  | { kind: 'thing'; id: string; name: string; counts: ThingCount; changed: boolean };

/** **Keep this one** waiting for the person (SM-SF-14): the fact pressed and the facts it conflicts with. */
export interface PendingKeep {
  fact: FactView;
  /** The facts in conflict with `fact`, as read; the dialog names them. */
  others: FactView[];
  /** With two or more conflicts the person chooses, among `fact` and `others`, the one to keep. */
  choice: string;
}

/** The diagram screen (SM-SF-20): where it was opened from, so **Back to the list** returns there. */
export interface DiagramPlace {
  from: 'things' | 'thing';
}

/** Where the Group by choice is remembered (the view preferences, `ViewPrefs`). */
export interface GroupByPrefs {
  memoryGroupBy<T extends string>(workspaceId: string, valid: (v: unknown) => v is T, fallback: T): T;
  setMemoryGroupBy(workspaceId: string, groupBy: string): void;
  /** Settings › Memory "How memory is shown" (SM-NS-1.9; amended January 2026; user decision; Approved by the user January 2026). */
  memoryShown?(validGroupBy: (v: unknown) => boolean): { groupBy: string; pageSize: number; detailsOpen: boolean };
}

/** One group as the list draws it: its facts so far, and how many more **Show more** reveals. */
export interface ShownGroup extends FactGroup {
  shown: FactView[];
  more: number;
  open: boolean;
}

/** The backend's answer when its catalogue has no such command (ServiceApi.md SVC-RL-3 step 5). */
const COMMAND_UNKNOWN = 'SVC_COMMAND_UNKNOWN';
/** The diagram's errors that offer **Try again** (SM-SF-23). */
const DIAGRAM_RETRY = new Set(['MEM_STORE_UNAVAILABLE', 'MEM_STORE_TIMEOUT']);

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
  /** Facts named by a `conflicts_with` and not in the list on screen, read with `memory.get` (SM-SF-14). */
  conflictFacts = $state.raw<Record<string, FactView>>({});
  keepPending = $state.raw<PendingKeep | null>(null);
  /** The words of the place (SM-SF-17); `null` until the Words tab is first shown. */
  words = $state.raw<TermView[] | null>(null);
  /** The diagram screen, when open (SM-SF-20 to SM-SF-23). */
  diagramPlace = $state.raw<DiagramPlace | null>(null);
  diagram = $state.raw<DiagramView | null>(null);
  /** The centre's facts beside the diagram (SM-SF-22). */
  diagramDetail = $state.raw<ThingDetail | null>(null);
  /** The fact opened from an arrow or a value box, in the panel under the diagram. */
  diagramFact = $state.raw<FactView | null>(null);
  diagramError = $state.raw<ServiceError | null>(null);
  /** `false` once the backend answered that it has no `memory.diagram`: **Show as diagram** is hidden. */
  diagramWired = $state(true);
  /** The things the diagram's search box found (SM-SF-20). */
  diagramMatches = $state.raw<ThingView[]>([]);
  #lastCentre: string | null = null;
  /** The id of the entry focus should move to after an outcome (SM-SF-4 accessibility). */
  focusAfter = $state<string | null>(null);
  /** The pages that set the links drawn here, each read once while the view is open (SM-CU-11). */
  readonly pages: PageTitles;
  readonly #handle: Handle;
  readonly workspaceId: string;
  #timer: ReturnType<typeof setTimeout> | undefined;
  readonly #prefs: GroupByPrefs | undefined;

  // ---- grouping, search and filters (SM-SF-27) ---------------------------------------------------
  groupBy = $state<GroupBy>('thing');
  factSearch = $state('');
  filters = $state.raw<FactFilters>(NO_FILTERS);
  /** Lines drawn per group key beyond the first page; reset when the grouping changes. */
  groupLimits = $state.raw<Record<string, number>>({});
  groupsShown = $state(GROUPS_PAGE);
  /** Group keys the person collapsed. */
  collapsed = $state.raw<Record<string, boolean>>({});
  /** Thing id → topic, and the place's topics by name (`memory.thing_topics`). */
  topics = $state.raw<TopicMap>({});
  topicNames = $state.raw<string[]>([]);
  /** `false` once the backend answered that it has no `memory.thing_topics`: Topic is not offered. */
  topicsWired = $state(true);

  constructor(handle: Handle, workspaceId: string, prefs?: GroupByPrefs) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
    this.pages = new PageTitles(handle);
    this.#prefs = prefs;
    // SM-NS-1.9: Settings › Memory gives the default grouping, the page size and how details start.
    const shown = prefs?.memoryShown?.(isGroupBy);
    const fallback: GroupBy = shown !== undefined && isGroupBy(shown.groupBy) ? shown.groupBy : 'thing';
    this.pageSize = shown?.pageSize ?? GROUP_PAGE;
    this.detailsOpen = shown?.detailsOpen ?? false;
    this.groupBy = prefs?.memoryGroupBy(workspaceId, isGroupBy, fallback) ?? fallback;
  }

  /** Facts a group shows before **Show more**, and each **Show more** adds (SM-NS-1.9). */
  readonly pageSize: number;
  /** Whether a fact's details start open (SM-NS-1.9). */
  readonly detailsOpen: boolean;

  /** The facts the chips and the search keep. */
  get visibleFacts(): FactView[] {
    return filterFacts(this.facts, this.filters, this.factSearch, this.topics, this.#topicsByName);
  }

  /** The topics by thing name, for facts whose subject has no id yet (SM-SF-27, amended January 2026). */
  get #topicsByName(): TopicMap {
    return topicsByName(this.things, this.topics);
  }

  /** What Group by draws: Topic falls back to Thing while the backend has no topics. */
  get effectiveGroupBy(): GroupBy {
    return this.groupBy === 'topic' && !this.topicsWired ? 'thing' : this.groupBy;
  }

  /** The groups drawn, each with its first page of lines, and how many groups there are (SM-SF-27). */
  get grouping(): { groups: ShownGroup[]; total: number; facts: number } {
    const visible = this.visibleFacts;
    const all = groupFacts(visible, this.effectiveGroupBy, this.names, this.topics, this.#topicsByName);
    const groups = all.slice(0, this.groupsShown).map((g) => {
      const limit = this.groupLimits[g.key] ?? this.pageSize;
      return { ...g, shown: g.facts.slice(0, limit), more: Math.max(0, g.facts.length - limit), open: this.collapsed[g.key] !== true };
    });
    return { groups, total: all.length, facts: visible.length };
  }

  setGroupBy(by: GroupBy): void {
    this.groupBy = by;
    this.groupLimits = {};
    this.groupsShown = GROUPS_PAGE;
    this.collapsed = {};
    this.#prefs?.setMemoryGroupBy(this.workspaceId, by);
    // A topic an agent gave waits until its thing exists; reading the topics again settles it (MemoryService.md
    // MEM-TOP-3), so choosing Topic groups by the topics known now, not those of when the screen opened
    // (amended January 2026; user decision; Approved by the user January 2026).
    if (by === 'topic' && this.topicsWired) void this.loadTopics();
  }

  setFilter(change: Partial<FactFilters>): void {
    this.filters = { ...this.filters, ...change };
    this.groupLimits = {};
  }

  toggleGroup(key: string): void {
    this.collapsed = { ...this.collapsed, [key]: this.collapsed[key] !== true };
  }

  /** **Show more** in one group: the next page of its lines. */
  showMore(key: string): void {
    this.groupLimits = { ...this.groupLimits, [key]: (this.groupLimits[key] ?? this.pageSize) + this.pageSize };
  }

  showMoreGroups(): void {
    this.groupsShown += GROUPS_PAGE;
  }

  // ---- topics (SM-SF-27) ------------------------------------------------------------------------

  /** `memory.thing_topics`; a backend without it leaves Topic out of Group by and the chips. */
  async loadTopics(): Promise<void> {
    const out = await this.#handle.call('memory.thing_topics', { workspace_id: this.workspaceId });
    if (out.kind === 'error') {
      if (out.error.code === COMMAND_UNKNOWN) this.topicsWired = false;
      return;
    }
    this.#takeTopics(out.result);
  }

  #takeTopics(view: { things: { thing_id: string; topic: string }[]; topics: string[] }): void {
    const map: Record<string, string> = {};
    for (const t of view.things) map[t.thing_id] = t.topic;
    this.topics = map;
    this.topicNames = view.topics;
  }

  /** The topic of a thing, or `null`. */
  topicOfThing(id: string): string | null {
    return this.topics[id] ?? null;
  }

  /**
   * **Change topic…** (SM-SF-27): `memory.set_thing_topic { workspace_id, id, topic }`; `null` clears it. The
   * owner's sentence is drawn under the dialog's box (`on` = `topic:<id>`).
   */
  async setTopic(id: string, topic: string | null): Promise<boolean> {
    const out = await this.#handle.call('memory.set_thing_topic', { workspace_id: this.workspaceId, id, topic });
    if (out.kind === 'error') {
      this.error = { error: out.error, on: `topic:${id}` };
      return false;
    }
    this.error = null;
    this.#takeTopics(out.result);
    this.announcer.say(t('memory-topic-saved'));
    return true;
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
    await Promise.all([this.loadAgents(), this.loadFacts(), this.loadToCheck(), this.loadTopics()]);
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
    this.facts = out.result.items.filter(notPassage);
    this.factsCursor = out.result.next_cursor ?? null;
    await this.#loadReplaced(out.result.items);
    await this.loadConflicts(this.facts);
  }

  async moreFacts(): Promise<void> {
    if (this.factsCursor === null) return;
    const out = await this.#handle.call('memory.list', { workspace_id: this.workspaceId, cursor: this.factsCursor });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: '' });
    this.facts = [...this.facts, ...out.result.items.filter(notPassage)];
    this.factsCursor = out.result.next_cursor ?? null;
    await this.#loadReplaced(out.result.items);
    await this.loadConflicts(this.facts);
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

  /** Correct: the new object replaces the fact's (`memory.correct { id, object }`, SM-SF-15, ServiceApi.md
   * SVC-TR-2); the list is read again, and the diagram when it is open. */
  async correct(fact: FactView, object: ObjectInput): Promise<void> {
    const out = await this.#handle.call('memory.correct', { id: fact.id, object });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: fact.id });
    this.announcer.say(t('memory-corrected'));
    await this.#afterChange();
  }

  /** Every list on screen read again after a change on a fact line. */
  async #afterChange(): Promise<void> {
    await this.loadFacts();
    if (this.detail !== null) await this.#reloadDetail();
    if (this.diagramPlace !== null) await this.reloadDiagram();
  }

  // ---- conflicts (SM-SF-14) ------------------------------------------------------------------------

  /** The facts named by `conflicts_with` that are not in `shown` and not yet read, with one `memory.get`. */
  async loadConflicts(shown: readonly FactView[]): Promise<void> {
    const here = new Set(shown.map((f) => f.id));
    const ids = [...new Set(shown.flatMap((f) => f.conflicts_with ?? []))].filter((id) => !here.has(id) && !(id in this.conflictFacts));
    if (ids.length === 0) return;
    const next = { ...this.conflictFacts };
    for (let i = 0; i < ids.length; i += MEMORY_GET_CHUNK) {
      const got = await this.#handle.call('memory.get', { ids: ids.slice(i, i + MEMORY_GET_CHUNK), workspace_id: this.workspaceId });
      if (got.kind === 'ok') for (const f of got.result.facts) next[f.id] = f;
    }
    this.conflictFacts = next;
  }

  /** A fact by id, from a list on screen or the conflicts read for them. */
  factById(id: string): FactView | null {
    return (
      this.facts.find((f) => f.id === id) ??
      this.detail?.facts.find((f) => f.id === id) ??
      this.diagramDetail?.facts.find((f) => f.id === id) ??
      this.conflictFacts[id] ??
      null
    );
  }

  /** The facts `fact` conflicts with that could be read; a fact no longer found is not drawn. */
  conflictsOf(fact: FactView): FactView[] {
    return (fact.conflicts_with ?? []).map((id) => this.factById(id)).filter((f): f is FactView => f !== null);
  }

  /** **Keep this one**: asks first (SM-SF-14). */
  askKeepSide(fact: FactView): void {
    const others = this.conflictsOf(fact);
    if (others.length === 0) return;
    this.keepPending = { fact, others, choice: fact.id };
  }

  chooseKeep(id: string): void {
    if (this.keepPending !== null) this.keepPending = { ...this.keepPending, choice: id };
  }

  /**
   * Confirmed: `memory.keep_conflict_side { keep, other }` once per fact the kept one conflicts with, stopping
   * at the first error; then the list is read again. `MEM_CONFLICT_NOT_FOUND` draws its sentence and reads
   * the list again.
   */
  async confirmKeepSide(): Promise<void> {
    const p = this.keepPending;
    if (p === null) return;
    const group = [p.fact, ...p.others];
    const kept = group.find((f) => f.id === p.choice) ?? p.fact;
    // SPEC-GAP-2: SM-SF-14 lists two or more conflicts "as radio buttons" and sends "one call per other
    // fact" without saying what the radio buttons choose. Read here as: they choose which fact to keep
    // (the pressed one first and chosen), and one call is sent per fact the kept one conflicts with.
    const others = kept.id === p.fact.id ? p.others.map((f) => f.id) : (kept.conflicts_with ?? []).filter((id) => group.some((f) => f.id === id));
    for (const other of others) {
      const out = await this.#handle.call('memory.keep_conflict_side', { keep: kept.id, other });
      if (out.kind === 'error') {
        this.keepPending = null;
        this.error = { error: out.error, on: p.fact.id };
        if (out.error.code === 'MEM_CONFLICT_NOT_FOUND') await this.#afterChange();
        return;
      }
    }
    this.keepPending = null;
    this.announcer.say(t('memory-keep-this-done'));
    await this.#afterChange();
  }

  cancelKeepSide(): void {
    this.keepPending = null;
  }

  // ---- words (SM-SF-17) ----------------------------------------------------------------------------

  async loadWords(): Promise<void> {
    const out = await this.#handle.call('memory.vocabulary', { workspace_id: this.workspaceId });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: 'words' });
    this.words = out.result.terms;
  }

  /** Rename: `memory.rename_term`; the sentence of a refusal is drawn under the box. */
  async renameWord(term: TermView, label: string): Promise<boolean> {
    const out = await this.#handle.call('memory.rename_term', { workspace_id: this.workspaceId, key: term.key, label });
    if (out.kind === 'error') {
      this.error = { error: out.error, on: `word:${term.key}` };
      return false;
    }
    this.error = null;
    this.announcer.say(t('memory-word-renamed'));
    await this.loadWords();
    return true;
  }

  /** Merge into…: `memory.merge_term`; a refusal is returned for the dialog to draw. */
  async mergeWord(term: TermView, into: string): Promise<ServiceError | null> {
    const out = await this.#handle.call('memory.merge_term', { workspace_id: this.workspaceId, key: term.key, into });
    if (out.kind === 'error') return out.error;
    this.announcer.say(t('memory-word-merged', { count: out.result.rewritten }));
    await this.loadWords();
    return null;
  }

  /** The words a word may be merged into: the others of the same object kind. */
  mergeTargets(term: TermView): TermView[] {
    return (this.words ?? []).filter((w) => w.key !== term.key && w.object === term.object);
  }

  /** SM-SF-26: the word's label names a kind of note and Merge into… is offered for it. */
  // spec: ordin-desktop/components/ScreensModule#SM-SF-26
  notAKind(term: TermView): boolean {
    return namesAKindOfNote(term.label) && this.mergeTargets(term).length > 0;
  }

  // ---- the diagram (SM-SF-20 to SM-SF-24) -----------------------------------------------------------

  /** From the Things tab without a centre; from a thing's page centred on it. */
  async openDiagram(centre: ThingRef | null): Promise<void> {
    this.diagramPlace = { from: centre === null ? 'things' : 'thing' };
    this.diagram = null;
    this.diagramDetail = null;
    this.diagramFact = null;
    this.diagramError = null;
    this.diagramMatches = [];
    await this.centre(centre?.id ?? null);
  }

  /**
   * `memory.diagram { workspace_id, centre? }`, then the centre's facts with `memory.thing`; announces the
   * centre. `MEM_THING_NOT_FOUND` keeps its sentence and reads the diagram again without a centre; a backend
   * without the command hides **Show as diagram** and closes the screen.
   */
  async centre(id: string | null): Promise<void> {
    const out = await this.#handle.call('memory.diagram', { workspace_id: this.workspaceId, ...(id === null ? {} : { centre: id }) });
    if (out.kind === 'error') {
      if (out.error.code === COMMAND_UNKNOWN) {
        this.diagramWired = false;
        this.closeDiagram();
        return;
      }
      this.diagramError = DIAGRAM_RETRY.has(out.error.code) ? { ...out.error, action: 'try_again' } : out.error;
      this.#lastCentre = id;
      if (out.error.code === 'MEM_THING_NOT_FOUND' && id !== null) {
        const again = await this.#handle.call('memory.diagram', { workspace_id: this.workspaceId });
        if (again.kind === 'ok') {
          this.#lastCentre = null;
          this.diagram = again.result;
          this.diagramDetail = null;
        }
      }
      return;
    }
    this.diagramError = null;
    this.#lastCentre = id;
    this.diagram = out.result;
    this.diagramFact = null;
    const centre = out.result.centre ?? null;
    if (centre === null) {
      this.diagramDetail = null;
      return;
    }
    const detail = await this.#handle.call('memory.thing', { id: centre.id, workspace_id: this.workspaceId });
    this.diagramDetail = detail.kind === 'ok' ? { ...detail.result, facts: detail.result.facts.filter(notPassage) } : null;
    if (this.diagramDetail !== null) await this.loadConflicts(this.diagramDetail.facts);
    this.announcer.say(t('memory-diagram-centred', { thing: centre.name }));
  }

  /** **Try again** and every change on a line read the diagram again around the same centre. */
  async reloadDiagram(): Promise<void> {
    const fact = this.diagramFact;
    await this.centre(this.#lastCentre);
    if (fact !== null) await this.openDiagramFact(fact.id);
  }

  /** A press on an arrow or a value box opens that fact's line under the diagram (`memory.get`). */
  async openDiagramFact(id: string): Promise<void> {
    const got = await this.#handle.call('memory.get', { ids: [id], workspace_id: this.workspaceId });
    if (got.kind === 'error') return void (this.diagramError = got.error);
    this.diagramFact = got.result.facts.find((f) => f.id === id) ?? null;
    if (this.diagramFact !== null) await this.loadConflicts([this.diagramFact]);
  }

  /** The search box shown when things were left out (SM-SF-20). */
  async searchDiagram(text: string): Promise<void> {
    const search = text.trim();
    if (search === '') return void (this.diagramMatches = []);
    const out = await this.#handle.call('memory.things', { workspace_id: this.workspaceId, search });
    this.diagramMatches = out.kind === 'ok' ? out.result.items.filter(worthListing) : [];
  }

  /** **Back to the list** (and Escape): to the Things tab, or to the thing's page it was opened from. */
  closeDiagram(): void {
    const from = this.diagramPlace?.from ?? 'things';
    this.diagramPlace = null;
    this.diagram = null;
    this.diagramDetail = null;
    this.diagramFact = null;
    this.diagramError = null;
    this.diagramMatches = [];
    this.tab = 'things';
    if (from === 'things') this.detail = null;
  }

  /** Remember everywhere (`memory.remember_everywhere`, SM-SF-11); the list is read again. */
  async rememberEverywhere(fact: FactView): Promise<void> {
    const out = await this.#handle.call('memory.remember_everywhere', { id: fact.id });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: fact.id });
    this.announcer.say(t('memory-everywhere-done'));
    await this.loadFacts();
  }

  /** Forget asks first (`memory.forget_count`), then confirms (`memory.forget`). */
  async askForgetFact(id: string): Promise<void> {
    const out = await this.#handle.call('memory.forget_count', { id });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: id });
    this.pending = { kind: 'fact', id, summaries: out.result.summaries };
  }

  /** History (SM-SF-13, amended January 2026): opens inside the fact's line; pressed again, it closes. */
  async openHistory(fact: FactView): Promise<void> {
    if (this.history?.fact.id === fact.id) {
      this.history = null;
      return;
    }
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
    this.things = out.result.items.filter(worthListing);
    this.thingsCursor = out.result.next_cursor ?? null;
  }

  async moreThings(): Promise<void> {
    if (this.thingsCursor === null) return;
    const search = this.thingSearch.trim();
    const out = await this.#handle.call('memory.things', { workspace_id: this.workspaceId, cursor: this.thingsCursor, ...(search === '' ? {} : { search }) });
    if (out.kind === 'error') return void (this.error = { error: out.error, on: '' });
    this.things = [...this.things, ...out.result.items.filter(worthListing)];
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
      this.detail = { ...out.result, facts: out.result.facts.filter(notPassage) };
      this.tab = 'things';
      await this.loadConflicts(this.detail.facts);
      await this.pages.read(pageIdsOf([], out.result.links));
    } else {
      this.error = { error: out.error, on: id };
    }
  }

  closeThing(): void {
    this.detail = null;
  }

  /** The thing's page read again in place, after a change on one of its lines. */
  async #reloadDetail(): Promise<void> {
    if (this.detail === null) return;
    const out = await this.#handle.call('memory.thing', { id: this.detail.thing.id, workspace_id: this.workspaceId });
    if (out.kind === 'ok') this.detail = { ...out.result, facts: out.result.facts.filter(notPassage) };
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
      if (this.diagramFact?.id === p.id) this.diagramFact = null;
      if (this.detail !== null) await this.#reloadDetail();
      if (this.diagramPlace !== null) await this.reloadDiagram();
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

/** MemoryService.md MEM-TR-13 (c): the words that name a kind of note, not how a thing relates to its value. */
const NOTE_WORDS = ['key fact', 'known issue', 'note', 'issue', 'fact', 'summary', 'detail', 'point', 'info'];

/**
 * SM-SF-26: a label read as MEM-TR-13 (c) reads a proposed one — `_` and `-` as spaces, lower case,
 * spaces collapsed, ASCII punctuation trimmed at both ends — is a kind of note, singular or plural.
 */
// spec: ordin-desktop/components/ScreensModule#SM-SF-26
export function namesAKindOfNote(label: string): boolean {
  const norm = label
    .replace(/[_-]/g, ' ')
    .replace(/\s+/g, ' ')
    .toLowerCase()
    .replace(/^[!-/:-@[-`{-~]+|[!-/:-@[-`{-~]+$/g, '')
    .trim();
  return NOTE_WORDS.some(
    (w) => norm === w || norm === `${w}s` || (w.endsWith('y') && norm === `${w.slice(0, -1)}ies`),
  );
}
