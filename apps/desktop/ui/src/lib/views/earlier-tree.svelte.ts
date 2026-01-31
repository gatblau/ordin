// The group *Earlier conversations* under a workspace in the tree (SM-WR-7): one node per chain (SM-CC-6),
// at most 20, newest first, each named by its first request cut as SM-CC-1 cuts it; more than 20 adds a last
// node `conv-picker-all`. Each node's main action is **Read it**; its ⋯ menu holds **Read it**, **Ask Ordin
// about it** and **Continue this conversation**. Ask and Continue open the read-only tab, which carries them
// out as SM-CC-2 to SM-CC-5 state (the intent is handed to the tab once).
//
// spec: ordin-desktop/components/ScreensModule#SM-WR-7
import type { BackendHandle } from '../backend/backend';
import { conversationName, EARLIER_LIMIT } from '../shell/conversation-picker.svelte';
import { EarlierListModel } from './earlier.svelte';

export interface EarlierTreeNode {
  kind: 'conversation' | 'all';
  /** The newest conversation of the chain: the one **Read it** opens. */
  conversationId?: string;
  name: string;
}

export type EarlierIntent = 'ask' | 'continue';

const intents = new Map<string, EarlierIntent>();
const intentKey = (backend: string, conversationId: string): string => `${backend}:${conversationId}`;

/** Hands **Ask Ordin about it** or **Continue this conversation** to the read-only tab about to open. */
export function intendOnOpen(backend: string, conversationId: string, intent: EarlierIntent): void {
  intents.set(intentKey(backend, conversationId), intent);
}

/** The intent handed to this tab, read once. */
export function takeIntent(backend: string, conversationId: string): EarlierIntent | null {
  const key = intentKey(backend, conversationId);
  const intent = intents.get(key) ?? null;
  intents.delete(key);
  return intent;
}

export class EarlierTreeModel {
  readonly list: EarlierListModel;
  readonly #say: (id: string, args?: Record<string, string | number>) => string;

  constructor(handle: BackendHandle, workspaceId: string, say: (id: string, args?: Record<string, string | number>) => string) {
    this.list = new EarlierListModel(handle, workspaceId);
    this.#say = say;
  }

  /** `ordin.list_conversations { workspace_id }` (first page). */
  async load(): Promise<void> {
    await this.list.load();
  }

  get nodes(): EarlierTreeNode[] {
    const entries = this.list.entries;
    const nodes: EarlierTreeNode[] = entries.slice(0, EARLIER_LIMIT).map((entry) => ({
      kind: 'conversation',
      conversationId: entry.head.conversation_id,
      name: conversationName(entry.chain[0]?.first_request ?? entry.head.first_request, this.#say('conv-picker-new')),
    }));
    if (entries.length > EARLIER_LIMIT) nodes.push({ kind: 'all', name: this.#say('conv-picker-all', { count: entries.length }) });
    return nodes;
  }
}
