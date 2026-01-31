// Shared types of the blocks module.
import type { BackendState } from '../store/backend-state.svelte';
import type { Block } from '../service-api/types.gen';

export type BlockKind = Block['kind'];

/** The props every kind component takes: the block narrowed to its kind. */
export interface KindProps<K extends BlockKind> {
  backend: BackendState;
  workspaceId: string;
  blockId: string;
  block: Extract<Block, { kind: K }>;
}

/** Every kind the drawing side knows (2C §7: 22 variants of `Block`, and `access`, OrdinHost.md OH-ACC-1). */
export const KNOWN_KINDS: readonly BlockKind[] = [
  'message', 'question', 'form', 'documents', 'preview', 'document', 'changes', 'plan_progress', 'table', 'chart',
  'key_figures', 'report', 'email_list', 'email', 'telegram_list', 'slack_list', 'result', 'agent_proposal',
  'agent_library', 'crew_plan', 'memory', 'knowledge', 'access',
];

export function isKnownKind(kind: string): kind is BlockKind {
  return (KNOWN_KINDS as readonly string[]).includes(kind);
}

/** Where a block is drawn in the conversation: in full, or as a one-line card with the content on a screen (11 §5.15). */
export type Placement = 'full' | 'card';

export function placementOf(block: Block): Placement {
  switch (block.kind) {
    case 'message':
    case 'question':
    // SM-FA-3: the tick boxes are in the conversation, in full.
    case 'access':
      return 'full';
    case 'result':
      return block.of === 'offer' ? 'full' : 'card';
    default:
      return 'card';
  }
}
