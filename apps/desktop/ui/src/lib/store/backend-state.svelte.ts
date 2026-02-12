// The event store: one `BackendState` per backend. It applies Service API events in sequence order,
// reloads through commands on a gap or a `reset`, and keeps nothing that is not also in the journal
// (apart from view preferences). Views derive what they draw from it.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import { SvelteMap } from 'svelte/reactivity';

import type { BackendHandle, BackendId, DiagnosticSink, EventEnvelope, ServiceEvent } from '../backend/backend';
import type { HasGroups } from '../backend/groups';
import type {
  ApprovalCard,
  ApprovalState,
  Block,
  CrewRunState,
  HomeRow,
  MessageAuthor,
  ProgressCount,
  RunState,
  RunStep,
  ServerConnectionState,
  StartUpReportItem,
  StepMark,
  Workspace,
  WorkspaceTree,
  ServiceHello,
} from '../service-api/types.gen';

export const PROGRESS_LINES_KEPT = 200;
/** The run states that end a run: its elapsed time stops there (SM-PG-4). */
const RUN_ENDED: ReadonlySet<string> = new Set(['completed', 'failed', 'stopped_at_limit', 'cancelled', 'interrupted', 'abandoned', 'no_reply']);
const SUPPORTED_MAJOR = 1;

export type ProgressLine = { kind: 'line'; line: string } | { kind: 'plan_changed'; plan_version: number };

export interface RunView {
  state: RunState;
  progressLines: ProgressLine[];
  /** Lines older than the last 200, folded away. */
  folded: number;
  count: ProgressCount | null;
  plan: { plan_version: number; steps: RunStep[] } | null;
  startUp: StartUpReportItem[];
  /** When this window first saw the run, from its first event's time (SM-PG-4). */
  startedAt?: string;
  /** When the run reached a state that ends it (SM-PG-4). */
  endedAt?: string;
}
export interface CrewView {
  round: number;
  state: CrewRunState;
  startUp: StartUpReportItem[];
}
export interface DecidedView {
  state: ApprovalState;
  decided_on: string;
  at: string;
}
/** Carried items first, then by sequence number (SM-CC-7). */
const byPlace = (a: { carried?: boolean; seq: number }, b: { carried?: boolean; seq: number }): number =>
  (b.carried === true ? 1 : 0) - (a.carried === true ? 1 : 0) || a.seq - b.seq;

export interface ConversationItem {
  block_id: string;
  workspace_id: string;
  block: Block;
  seq: number;
  at: string;
  /** A question, form, offer or crew plan not yet answered. */
  waiting: boolean;
  conversation_id?: string;
  /** Who the item is from; absent means Ordin. The person's own message is drawn as theirs (SM-CV-1). */
  from?: MessageAuthor;
  /** A waiting item moved into this conversation from the one it replaced (SM-CC-7); drawn at the top. */
  carried?: boolean;
}
/** SM-WK-1: Ordin is working on a turn in a workspace, since `since`, doing `doing` when it said. */
export interface OrdinWorking {
  since: string;
  doing?: string;
}
export interface NeedsYouGroup {
  workspace_id: string | null;
  workspace_name: string | null;
  approvals: ApprovalCard[];
  rows: HomeRow[];
}

/** A block that waits for the person until answered (11 §5.15). */
export function isWaitingBlock(block: Block): boolean {
  if (block.kind === 'question' || block.kind === 'form' || block.kind === 'crew_plan') return true;
  return block.kind === 'result' && 'of' in block && block.of === 'offer';
}

/** Events the store has dealt with; subscribers (the locale reload, the quit flow, the approval opener) act on them. */
export type EventListener = (env: EventEnvelope, state: BackendState) => void;

export interface BackendStateOptions {
  handle: BackendHandle;
  diagnostics?: DiagnosticSink;
  /** Workspace ids whose tree is loaded at each load and reload (the expanded ones). */
  expandedWorkspaces?: () => string[];
  now?: () => string;
}

export class BackendState implements HasGroups {
  readonly id: BackendId;
  readonly kind: 'local' | 'server';
  readonly handle: BackendHandle;

  lastSeq = $state(0);
  loaded = $state(false);
  /** True while a load or reload runs; the screens keep their last state, read-only. */
  loading = $state(false);
  connection = $state<ServerConnectionState | undefined>(undefined);
  asOf = $state<string | undefined>(undefined);
  /** A server's display name (servers only) and, when its connection is `refused_for_version`, the side that must update. */
  displayName = $state('');
  olderSide = $state<'desktop' | 'server' | undefined>(undefined);
  /** `service.hello`'s `groups` (SM-UP-1). */
  groups = $state.raw<ReadonlySet<string>>(new Set());
  hello = $state.raw<ServiceHello | null>(null);

  workspaces = $state.raw<Workspace[]>([]);
  trees = new SvelteMap<string, WorkspaceTree>();
  needsYouRows = $state.raw<HomeRow[]>([]);
  runs = new SvelteMap<string, RunView>();
  crews = new SvelteMap<string, CrewView>();
  pendingApprovals = new SvelteMap<string, ApprovalCard>();
  decided = new SvelteMap<string, DecidedView>();
  /** Every approval card seen since start, kept after it is decided so the conversation can draw `card-decided`. */
  seenCards = new SvelteMap<string, ApprovalCard>();
  conversation = new SvelteMap<string, ConversationItem[]>();
  /** SM-WK-1: per workspace, whether Ordin is working (`ordin_turn_changed`); never stored. */
  ordinWorking = new SvelteMap<string, OrdinWorking>();
  /** SM-RF-2: per workspace, the `failure` word of the last `idle` (SVC-WK-2); read when a turn fails. */
  ordinTurnFailure = new Map<string, string>();
  /** SM-RF-3: per workspace, the shown blocks still being fetched; never stored. */
  pendingBlocks = new SvelteMap<string, number>();

  /** Bumped when the matching event arrives, so a view can `$effect` on it and re-read (sign-ins, setup, one workspace). */
  signInRevision = $state(0);
  setupRevision = $state(0);
  needsYouRevision = $state(0);
  workspaceRevisions = new SvelteMap<string, number>();

  #diagnostics: DiagnosticSink | undefined;
  #expanded: () => string[];
  #now: () => string;
  #buffer: EventEnvelope[] = [];
  #listeners = new Set<EventListener>();
  #reloadHooks = new Set<() => Promise<void>>();
  #queue: Promise<void> = Promise.resolve();
  #loadPromise: Promise<boolean> | null = null;

  constructor(options: BackendStateOptions) {
    this.handle = options.handle;
    this.id = options.handle.id;
    this.kind = options.handle.kind;
    this.#diagnostics = options.diagnostics;
    this.#expanded = options.expandedWorkspaces ?? (() => []);
    this.#now = options.now ?? (() => new Date().toISOString());
  }

  /** Calls `listener` after each event has been applied. Returns the unsubscribe function. */
  subscribe(listener: EventListener): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  /** Registers the commands of an open screen, run at each load and reload (step 5). Returns the unregister function. */
  onReload(hook: () => Promise<void>): () => void {
    this.#reloadHooks.add(hook);
    return () => this.#reloadHooks.delete(hook);
  }

  workspaceRevision(workspaceId: string): number {
    return this.workspaceRevisions.get(workspaceId) ?? 0;
  }

  // ---- derived -------------------------------------------------------------------------------

  /** Pending approvals plus the waiting items *Needs you* returns, grouped by workspace, `openWorkspaceId` first. */
  needsYouGroups(openWorkspaceId?: string | null): NeedsYouGroup[] {
    const groups = new Map<string | null, NeedsYouGroup>();
    const names = new Map(this.workspaces.map((w) => [w.id, w.name]));
    const group = (id: string | null, name?: string | null): NeedsYouGroup => {
      let g = groups.get(id);
      if (g === undefined) {
        g = { workspace_id: id, workspace_name: name ?? (id === null ? null : (names.get(id) ?? null)), approvals: [], rows: [] };
        groups.set(id, g);
      }
      return g;
    };
    for (const card of this.pendingApprovals.values()) group(card.workspace_id).approvals.push(card);
    for (const row of this.needsYouRows) {
      if (row.opens.kind === 'approval' && this.pendingApprovals.has(row.opens.approval_id)) continue;
      group(row.workspace_id ?? null, row.workspace_name).rows.push(row);
    }
    return [...groups.values()].sort((a, b) => {
      const rank = (g: NeedsYouGroup): number => (g.workspace_id !== null && g.workspace_id === openWorkspaceId ? 0 : 1);
      return rank(a) - rank(b);
    });
  }

  get needsYouCount(): number {
    return this.needsYouGroups().reduce((n, g) => n + g.approvals.length + g.rows.length, 0);
  }

  needsYouCountFor(workspaceId: string): number {
    const g = this.needsYouGroups().find((x) => x.workspace_id === workspaceId);
    return g === undefined ? 0 : g.approvals.length + g.rows.length;
  }

  // ---- receiving events ----------------------------------------------------------------------

  /** Entry point for the event stream: events are applied one at a time, in arrival order. */
  receive(env: EventEnvelope): Promise<void> {
    if (this.loading || !this.loaded) {
      this.#buffer.push(env);
      return Promise.resolve();
    }
    this.#queue = this.#queue.then(() => this.apply(env));
    return this.#queue;
  }

  /** Applies one event (Internal Logic step 4). Resolves when it, and any reload it caused, is done. */
  async apply(env: EventEnvelope, skipGapCheck = false): Promise<void> {
    if (env.api_version.major !== SUPPORTED_MAJOR) {
      this.#diagnostics?.record('UI_EVENT_VERSION', env.kind, this.id);
      return;
    }
    if (env.seq <= this.lastSeq) return;
    if (!skipGapCheck && env.seq > this.lastSeq + 1 && this.loaded) {
      this.#diagnostics?.record('SVC_EVENT_GAP', `${this.lastSeq}->${env.seq}`, this.id);
      await this.reload();
      if (env.seq <= this.lastSeq) return;
    }
    const known = this.#byKind(env);
    if (!known) {
      this.#diagnostics?.record('UI_EVENT_KIND_UNKNOWN', String((env as { kind?: unknown }).kind), this.id);
      this.lastSeq = env.seq;
      return;
    }
    if (env.kind === 'reset') {
      this.#clear();
      await this.reload();
      this.#notify(env);
      return;
    }
    this.lastSeq = env.seq;
    this.#notify(env);
  }

  #notify(env: EventEnvelope): void {
    for (const listener of this.#listeners) listener(env, this);
  }

  /** Updates the state for a known kind; returns false for a kind this version does not know. */
  #byKind(env: EventEnvelope): boolean {
    const e: ServiceEvent = env;
    switch (e.kind) {
      case 'run_state_changed':
        this.#patchRun(e.run_id, (r) => ({ ...r, state: e.state, endedAt: RUN_ENDED.has(e.state) ? (r.endedAt ?? env.at) : undefined }), env.at);
        return true;
      case 'run_progress':
        this.#patchRun(e.run_id, (r) => {
          const lines: ProgressLine[] = [...r.progressLines, { kind: 'line', line: e.progress.line }];
          const over = Math.max(0, lines.length - PROGRESS_LINES_KEPT);
          return { ...r, progressLines: lines.slice(over), folded: r.folded + over, count: e.progress.count ?? null };
        }, env.at);
        return true;
      case 'plan_changed':
        this.#patchRun(e.run_id, (r) => ({
          ...r,
          plan: { plan_version: e.plan_version, steps: e.steps },
          progressLines: [...r.progressLines, { kind: 'plan_changed', plan_version: e.plan_version }],
        }), env.at);
        return true;
      case 'step_marked':
        this.#patchRun(e.run_id, (r) =>
          r.plan !== null && r.plan.plan_version === e.plan_version
            ? {
                ...r,
                plan: {
                  ...r.plan,
                  steps: r.plan.steps.map((s) => (s.position === e.position ? { ...s, mark: e.mark as StepMark } : s)),
                },
              }
            : r,
          env.at,
        );
        return true;
      case 'start_up_report_ready':
        if (e.subject.kind === 'run') this.#patchRun(e.subject.run_id, (r) => ({ ...r, startUp: e.items }));
        else if (e.subject.kind === 'crew_run') this.#patchCrew(e.subject.crew_run_id, (c) => ({ ...c, startUp: e.items }));
        return true;
      case 'approval_requested':
        this.pendingApprovals.set(e.card.approval_id, e.card);
        this.seenCards.set(e.card.approval_id, e.card);
        this.needsYouRevision++;
        return true;
      case 'approval_decided':
        this.pendingApprovals.delete(e.approval_id);
        this.decided.set(e.approval_id, { state: e.state, decided_on: e.decided_on, at: env.at });
        this.needsYouRevision++;
        return true;
      case 'crew_round_changed':
        this.#patchCrew(e.crew_run_id, (c) => ({ ...c, round: e.round, state: e.state }));
        return true;
      case 'block_shown':
        // SM-WK-1: Ordin's reply ends the working line.
        if (e.author == null || e.author === 'ordin') this.ordinWorking.delete(e.workspace_id);
        this.#addItem({
          block_id: e.block_id,
          workspace_id: e.workspace_id,
          block: e.block,
          seq: env.seq,
          at: env.at,
          ...(e.author != null ? { from: e.author } : {}),
        });
        return true;
      case 'block_available':
        void this.#fetchBlock(e.workspace_id, e.block_id, env.seq, env.at);
        return true;
      case 'workspace_changed':
        this.workspaceRevisions.set(e.workspace_id, this.workspaceRevision(e.workspace_id) + 1);
        if (this.trees.has(e.workspace_id)) void this.loadTree(e.workspace_id);
        return true;
      case 'needs_you_changed':
        this.needsYouRevision++;
        void this.loadNeedsYou();
        return true;
      case 'sign_in_changed':
        this.signInRevision++;
        return true;
      case 'setup_progress':
        this.setupRevision++;
        return true;
      // spec: ordin-desktop/components/ScreensModule#SM-WK-1
      case 'ordin_turn_changed':
        // spec: ordin-desktop/components/ScreensModule#SM-RF-2
        if (e.state === 'idle' && e.failure != null) this.ordinTurnFailure.set(e.workspace_id, e.failure);
        else this.ordinTurnFailure.delete(e.workspace_id);
        if (e.state === 'working') this.ordinWorking.set(e.workspace_id, { since: e.since, ...(e.doing != null ? { doing: e.doing } : {}) });
        else this.ordinWorking.delete(e.workspace_id);
        return true;
      case 'knowledge_pages_changed': // ScreensModule.md SM-KT-3
      case 'knowledge_agents_changed':
        this.workspaceRevisions.set(e.workspace_id, this.workspaceRevision(e.workspace_id) + 1);
        return true;
      case 'reset':
      case 'server_state_changed':
      case 'locale_changed':
      case 'language_changed':
      case 'brand_changed':
      case 'more_found':
      case 'memory_models_changed':
        // Acted on by the subscribers (the app reloads catalogues and brand, maps servers).
        return true;
      default:
        return false;
    }
  }

  // Views are replaced, never mutated: a `SvelteMap` tracks `set`, not changes inside its values.
  #patchRun(id: string, change: (run: RunView) => RunView, at?: string): void {
    const current = this.runs.get(id) ?? { state: 'queued', progressLines: [], folded: 0, count: null, plan: null, startUp: [] };
    const next = change(current);
    this.runs.set(id, at !== undefined && next.startedAt === undefined ? { ...next, startedAt: at } : next);
  }
  #patchCrew(id: string, change: (crew: CrewView) => CrewView): void {
    this.crews.set(id, change(this.crews.get(id) ?? { round: 0, state: 'starting', startUp: [] }));
  }

  #addItem(base: Omit<ConversationItem, 'waiting'> & Partial<Pick<ConversationItem, 'waiting'>>): void {
    const item: ConversationItem = {
      ...base,
      waiting: base.waiting ?? isWaitingBlock(base.block),
    };
    const list = this.conversation.get(base.workspace_id) ?? [];
    if (list.some((i) => i.block_id === item.block_id)) return;
    const next = [...list, item].sort(byPlace);
    this.conversation.set(base.workspace_id, next);
  }

  /**
   * SM-CC-4: after Continue, the workspace's conversation reads `ordin.read_conversation` of the live
   * conversation and draws its blocks; entries with `carried: true` come first (SM-CC-7).
   */
  async reloadConversation(workspaceId: string): Promise<void> {
    const list = await this.handle.call('ordin.list_conversations', { workspace_id: workspaceId });
    if (list.kind !== 'ok') return;
    const live = list.result.items.find((c) => c.state === 'current');
    if (live === undefined) {
      this.conversation.set(workspaceId, []);
      return;
    }
    const read = await this.handle.call('ordin.read_conversation', { conversation_id: live.conversation_id });
    if (read.kind !== 'ok') return;
    const items: ConversationItem[] = [];
    for (const m of read.result.items) {
      if (m.block_id === undefined || m.block_id === null) continue;
      const out = await this.handle.call('ordin.block', { block_id: m.block_id, workspace_id: workspaceId });
      if (out.kind !== 'ok') continue;
      items.push({
        block_id: m.block_id,
        workspace_id: workspaceId,
        block: out.result.block,
        seq: m.seq,
        at: out.result.at ?? m.at,
        waiting: out.result.waiting,
        conversation_id: out.result.conversation_id,
        from: out.result.from,
        ...(m.carried === true ? { carried: true } : {}),
      });
    }
    this.conversation.set(workspaceId, items.sort(byPlace));
  }

  // spec: ordin-desktop/components/ScreensModule#SM-RF-3
  async #fetchBlock(workspaceId: string, blockId: string, seq: number, at: string): Promise<void> {
    this.pendingBlocks.set(workspaceId, (this.pendingBlocks.get(workspaceId) ?? 0) + 1);
    try {
      await this.#fetchBlockNow(workspaceId, blockId, seq, at);
    } finally {
      const left = (this.pendingBlocks.get(workspaceId) ?? 1) - 1;
      if (left > 0) this.pendingBlocks.set(workspaceId, left);
      else this.pendingBlocks.delete(workspaceId);
    }
  }

  async #fetchBlockNow(workspaceId: string, blockId: string, seq: number, at: string): Promise<void> {
    const out = await this.handle.call('ordin.block', { block_id: blockId, workspace_id: workspaceId });
    if (out.kind !== 'ok') return;
    this.#addItem({
      block_id: blockId,
      workspace_id: workspaceId,
      block: out.result.block,
      seq,
      at: out.result.at ?? at,
      waiting: out.result.waiting,
      conversation_id: out.result.conversation_id,
      from: out.result.from,
    });
  }

  /** The person answered a waiting item; it no longer waits. */
  markAnswered(workspaceId: string, blockId: string): void {
    const list = this.conversation.get(workspaceId);
    if (list === undefined) return;
    this.conversation.set(
      workspaceId,
      list.map((i) => (i.block_id === blockId ? { ...i, waiting: false } : i)),
    );
  }

  setConnection(state: ServerConnectionState, asOf?: string | null, extra?: { displayName?: string; olderSide?: 'desktop' | 'server' | null }): void {
    this.connection = state;
    this.asOf = asOf ?? undefined;
    if (extra?.displayName !== undefined) this.displayName = extra.displayName;
    if (extra !== undefined) this.olderSide = extra.olderSide ?? undefined;
  }

  #clear(): void {
    this.runs.clear();
    this.crews.clear();
    this.pendingApprovals.clear();
    this.decided.clear();
    this.seenCards.clear();
    this.conversation.clear();
    this.ordinWorking.clear();
    this.ordinTurnFailure.clear();
    this.trees.clear();
    this.workspaces = [];
    this.needsYouRows = [];
    this.lastSeq = 0;
  }

  // ---- load and reload (step 5) -------------------------------------------------------------

  /** Loads (or reloads) from commands. Concurrent calls share one load. */
  reload(): Promise<boolean> {
    if (this.#loadPromise !== null) return this.#loadPromise;
    this.#loadPromise = this.#load().finally(() => {
      this.#loadPromise = null;
    });
    return this.#loadPromise;
  }

  async load(): Promise<boolean> {
    return this.reload();
  }

  async #load(): Promise<boolean> {
    this.loading = true;
    // SM-WK-1: the working line is cleared when the backend reloads; a turn still running says so again.
    this.ordinWorking.clear();
    this.ordinTurnFailure.clear();
    try {
      const hello = await this.handle.call('service.hello', {});
      if (hello.kind !== 'ok') return false;
      this.hello = hello.result;
      this.groups = new Set(hello.result.groups);
      const baseline = hello.result.last_seq;

      const list = await this.handle.call('workspaces.list', { filter: 'active' });
      if (list.kind === 'ok') this.workspaces = list.result;
      await Promise.all(this.#expanded().map((id) => this.loadTree(id)));
      await this.loadNeedsYou();
      await this.#loadPendingApprovals();
      await Promise.all([...this.#reloadHooks].map((hook) => hook().catch(() => undefined)));

      this.lastSeq = baseline;
      this.loaded = true;
      return true;
    } finally {
      this.loading = false;
      const buffered = this.#buffer.filter((e) => e.seq > this.lastSeq).sort((a, b) => a.seq - b.seq);
      this.#buffer = [];
      for (const env of buffered) this.#queue = this.#queue.then(() => this.apply(env));
    }
  }

  async loadTree(workspaceId: string): Promise<void> {
    const out = await this.handle.call('workspaces.tree', { workspace_id: workspaceId });
    if (out.kind === 'ok') this.trees.set(workspaceId, out.result);
  }

  async loadNeedsYou(): Promise<void> {
    const out = await this.handle.call('home.needs_you', {});
    if (out.kind === 'ok') this.needsYouRows = out.result;
  }

  async #loadPendingApprovals(): Promise<void> {
    const out = await this.handle.call('approval.list_pending', {});
    if (out.kind !== 'ok') return;
    const next = new Map<string, ApprovalCard>();
    for (const record of out.result.items) if (record.state === 'pending') next.set(record.card.approval_id, record.card);
    for (const id of [...this.pendingApprovals.keys()]) if (!next.has(id)) this.pendingApprovals.delete(id);
    for (const [id, card] of next) {
      this.pendingApprovals.set(id, card);
      this.seenCards.set(id, card);
    }
  }

  /** `now` for views that stamp local changes. */
  now(): string {
    return this.#now();
  }
}

/** All the stores of the window, one per backend; routes each event to its store. */
export class Stores {
  #byId = new SvelteMap<BackendId, BackendState>();
  #diagnostics: DiagnosticSink | undefined;

  constructor(diagnostics?: DiagnosticSink) {
    this.#diagnostics = diagnostics;
  }

  add(state: BackendState): void {
    this.#byId.set(state.id, state);
  }
  remove(id: BackendId): void {
    this.#byId.delete(id);
  }
  get(id: BackendId): BackendState | undefined {
    return this.#byId.get(id);
  }
  all(): BackendState[] {
    return [...this.#byId.values()];
  }

  receive(env: EventEnvelope): Promise<void> {
    const state = this.#byId.get(env.backend);
    if (state === undefined) {
      this.#diagnostics?.record('UI_BACKEND_UNKNOWN', env.kind, env.backend);
      return Promise.resolve();
    }
    return state.receive(env);
  }

  /** The `after` map for `service_events`: the last seq drawn per backend. */
  lastSeqs(): Record<BackendId, number> {
    return Object.fromEntries(this.all().map((s) => [s.id, s.lastSeq]));
  }
}
