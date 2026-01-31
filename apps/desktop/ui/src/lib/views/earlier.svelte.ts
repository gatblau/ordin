// Earlier conversations (ADR-108): the list (one entry per chain), a read-only conversation drawn as a
// chain, and Continue this conversation (SM-CC-2 to SM-CC-5, SM-CC-7).
//
// spec: ordin-desktop/components/ScreensModule#SM-CC-2
// spec: ordin-desktop/components/ScreensModule#SM-CC-3
// spec: ordin-desktop/components/ScreensModule#SM-CC-4
// spec: ordin-desktop/components/ScreensModule#SM-CC-5
// spec: ordin-desktop/components/ScreensModule#SM-CC-6
// spec: ordin-desktop/components/ScreensModule#SM-CC-7
import type { BackendHandle } from '../backend/backend';
import type { ConversationMessage, ConversationSummary, ServiceError } from '../service-api/types.gen';

/** Messages drawn per step of a read-only conversation. */
export const READ_PAGE = 100;

/** An entry of the list: the newest conversation of a chain and the whole chain's message count. */
export interface ChainEntry {
  /** The newest conversation of the chain: the one a read-only tab opens. */
  head: ConversationSummary;
  /** Oldest first. */
  chain: ConversationSummary[];
  messageCount: number;
}

/** One entry per chain: a conversation another one continues is folded into the chain of its continuation. */
export function chains(items: readonly ConversationSummary[]): ChainEntry[] {
  const byId = new Map(items.map((c) => [c.conversation_id, c]));
  const continued = new Set(items.map((c) => c.continues).filter((x): x is string => x !== null && x !== undefined));
  const heads = items.filter((c) => !continued.has(c.conversation_id));
  return heads.map((head) => {
    const chain: ConversationSummary[] = [];
    const seen = new Set<string>();
    for (let cur: ConversationSummary | undefined = head; cur !== undefined && !seen.has(cur.conversation_id); cur = cur.continues ? byId.get(cur.continues) : undefined) {
      seen.add(cur.conversation_id);
      chain.unshift(cur);
    }
    return { head, chain, messageCount: chain.reduce((n, c) => n + c.message_count, 0) };
  });
}

/**
 * **Delete…** after the confirmation (SM-WR-16, SM-WR-17): `ordin.delete_conversation { conversation_id }`.
 * Returns the refusal, or `null` when the conversation and the ones it continues are gone.
 */
// spec: ordin-desktop/components/ScreensModule#SM-WR-16
// spec: ordin-desktop/components/ScreensModule#SM-WR-17
export async function deleteConversation(handle: BackendHandle, conversationId: string): Promise<ServiceError | null> {
  const out = await handle.call('ordin.delete_conversation', { conversation_id: conversationId });
  return out.kind === 'ok' ? null : out.error;
}

/** The list: `ordin.list_conversations`, earlier conversations only, newest first. */
export class EarlierListModel {
  entries = $state.raw<ChainEntry[]>([]);
  all = $state.raw<ConversationSummary[]>([]);
  error = $state<ServiceError | null>(null);
  next = $state<string | null>(null);
  readonly #handle: BackendHandle;
  readonly workspaceId: string;

  constructor(handle: BackendHandle, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('ordin.list_conversations', { workspace_id: this.workspaceId });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.next = out.result.next_cursor ?? null;
    this.#set(out.result.items);
  }

  async more(): Promise<void> {
    if (this.next === null) return;
    const out = await this.#handle.call('ordin.list_conversations', { workspace_id: this.workspaceId, cursor: this.next });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.next = out.result.next_cursor ?? null;
    this.#set([...this.all, ...out.result.items]);
  }

  #set(items: ConversationSummary[]): void {
    this.all = items;
    // The live conversation is not an earlier one; a chain is made of earlier conversations only.
    this.entries = chains(items.filter((c) => c.state === 'earlier')).sort((a, b) => (a.head.started_at < b.head.started_at ? 1 : -1));
  }
}

export interface ReadPart {
  conversation: ConversationSummary;
  messages: ConversationMessage[];
  next: string | null;
  loaded: boolean;
}

/** A read-only conversation: the whole chain, oldest first. */
export class EarlierReadModel {
  parts = $state.raw<ReadPart[]>([]);
  error = $state<ServiceError | null>(null);
  /** Message ids (by block) whose block now belongs to another conversation (SM-CC-7). */
  moved = $state.raw<ReadonlySet<string>>(new Set());
  askOpen = $state(false);
  question = $state('');
  asked = $state(false);
  readonly #handle: BackendHandle;
  readonly workspaceId: string;
  readonly conversationId: string;

  constructor(handle: BackendHandle, workspaceId: string, conversationId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
    this.conversationId = conversationId;
  }

  /** Reads the list for the chain, then the first page of each conversation, oldest first. */
  async load(): Promise<void> {
    const list = await this.#handle.call('ordin.list_conversations', { workspace_id: this.workspaceId });
    if (list.kind === 'error') {
      this.error = list.error;
      return;
    }
    const entry = chains(list.result.items).find((e) => e.chain.some((c) => c.conversation_id === this.conversationId));
    const chain = entry === undefined ? list.result.items.filter((c) => c.conversation_id === this.conversationId) : entry.chain;
    // Only the conversations up to the one opened belong to this tab.
    const upTo = chain.findIndex((c) => c.conversation_id === this.conversationId);
    const wanted = upTo < 0 ? chain : chain.slice(0, upTo + 1);
    this.parts = wanted.map((conversation) => ({ conversation, messages: [], next: null, loaded: false }));
    await this.#fill();
  }

  async #fill(): Promise<void> {
    let budget = READ_PAGE;
    const parts = [...this.parts];
    for (let i = 0; i < parts.length && budget > 0; i++) {
      const part = parts[i];
      if (part === undefined || (part.loaded && part.next === null)) continue;
      const out = await this.#handle.call('ordin.read_conversation', {
        conversation_id: part.conversation.conversation_id,
        ...(part.loaded && part.next !== null ? { cursor: part.next } : {}),
      });
      if (out.kind === 'error') {
        this.error = out.error;
        break;
      }
      parts[i] = { ...part, messages: [...part.messages, ...out.result.items], next: out.result.next_cursor ?? null, loaded: true };
      budget -= out.result.items.length;
      if ((out.result.next_cursor ?? null) !== null) break;
    }
    this.parts = parts;
    await this.#markMoved();
  }

  /** **Show more**: the next 100. */
  async showMore(): Promise<void> {
    await this.#fill();
  }

  get hasMore(): boolean {
    return this.parts.some((p) => !p.loaded || p.next !== null);
  }

  /** A message whose block's `conversation_id` is not the conversation drawn is drawn as one line (SM-CC-7). */
  async #markMoved(): Promise<void> {
    const moved = new Set(this.moved);
    for (const part of this.parts) {
      for (const m of part.messages) {
        if (m.block_id === undefined || m.block_id === null || moved.has(m.block_id)) continue;
        const out = await this.#handle.call('ordin.block', { workspace_id: this.workspaceId, block_id: m.block_id });
        if (out.kind === 'ok' && out.result.conversation_id !== part.conversation.conversation_id) moved.add(m.block_id);
      }
    }
    this.moved = moved;
  }

  /** **Ask Ordin about it**: sends the person's question about this conversation. */
  async ask(): Promise<boolean> {
    const question = this.question.trim();
    if (question === '') return false;
    const out = await this.#handle.call('ordin.ask_about_conversation', { workspace_id: this.workspaceId, conversation_id: this.conversationId, question });
    if (out.kind === 'ok') {
      this.asked = true;
      this.askOpen = false;
      this.question = '';
      return true;
    }
    this.error = out.error;
    return false;
  }
}

/**
 * **Keep the key points first** (SM-WR-12): `ordin.prepare_start_fresh { workspace_id }` from Clear's
 * confirmation, with `conversation_id` from Continue's. Nothing more is sent for the flow: Ordin's form is
 * answered as any form, and the Ordin Host then starts fresh or continues (OrdinHost.md OH-WR-1).
 */
// spec: ordin-desktop/components/ScreensModule#SM-WR-12
export async function prepareKeyPoints(handle: BackendHandle, workspaceId: string, conversationId?: string): Promise<ServiceError | null> {
  const out = await handle.call('ordin.prepare_start_fresh', {
    workspace_id: workspaceId,
    ...(conversationId === undefined ? {} : { conversation_id: conversationId }),
  });
  return out.kind === 'ok' ? null : out.error;
}

/** How many messages the live conversation holds, and what to do when Continue succeeds. */
export interface ContinueContext {
  workspaceId: string;
  /** Messages in the live conversation (0 when there is none). */
  liveMessages: () => number;
  /** The name drawn in the announcement: the first request of the conversation continued. */
  nameOf: (conversationId: string) => string;
  /** After the reply: close the read-only tab, show the workspace's conversation, move focus to the message box, announce. */
  onContinued: (conversationId: string, announcement: string) => void;
  announceText: (name: string) => string;
}

/** Continue this conversation (SM-CC-2 to SM-CC-5): asks first when the live conversation has messages. */
export class ContinueFlow {
  /** The earlier conversation the dialog is about. */
  asking = $state<string | null>(null);
  /** `ordin.prepare_start_fresh` was sent for this conversation; the Ordin Host continues it after the form. */
  keeping = $state<string | null>(null);
  /** `OH_CREW_RUNNING`: unavailable until the next `run_state_changed`. */
  waitsForCrew = $state(false);
  error = $state<ServiceError | null>(null);
  busy = $state(false);
  readonly #handle: BackendHandle;
  readonly #ctx: ContinueContext;

  constructor(handle: BackendHandle, ctx: ContinueContext) {
    this.#handle = handle;
    this.#ctx = ctx;
  }

  /** Press on **Continue this conversation**: with no live messages the command goes at once. */
  async request(conversationId: string): Promise<void> {
    if (this.waitsForCrew || this.busy) return;
    if (this.#ctx.liveMessages() > 0) {
      this.asking = conversationId;
      return;
    }
    await this.send(conversationId, []);
  }

  cancel(): void {
    this.asking = null;
  }

  /** **Continue** in the dialog: no key points. */
  async confirm(): Promise<void> {
    const id = this.asking;
    this.asking = null;
    if (id !== null) await this.send(id, []);
  }

  /** **Keep the key points first** (SM-WR-12): `ordin.prepare_start_fresh` naming the conversation to continue. */
  async keepPointsFirst(): Promise<void> {
    const id = this.asking;
    this.asking = null;
    if (id === null) return;
    const refused = await prepareKeyPoints(this.#handle, this.#ctx.workspaceId, id);
    if (refused === null) {
      this.keeping = id;
      this.error = null;
    } else if (refused.code === 'OH_CREW_RUNNING') this.waitsForCrew = true;
    else this.error = refused;
  }

  async send(conversationId: string, keptPoints: string[]): Promise<void> {
    this.busy = true;
    const out = await this.#handle.call('ordin.continue_conversation', {
      workspace_id: this.#ctx.workspaceId,
      conversation_id: conversationId,
      kept_points: keptPoints,
    });
    this.busy = false;
    if (out.kind === 'ok') {
      this.error = null;
      this.#ctx.onContinued(out.result.conversation_id, this.#ctx.announceText(this.#ctx.nameOf(conversationId)));
      return;
    }
    if (out.error.code === 'OH_CREW_RUNNING') this.waitsForCrew = true;
    else this.error = out.error;
  }

  /** The next `run_state_changed` makes Continue available again. */
  onRunStateChanged(): void {
    this.waitsForCrew = false;
  }
}
