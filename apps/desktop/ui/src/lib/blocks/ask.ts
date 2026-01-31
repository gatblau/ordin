// "Ask Ordin about this": a message sent to the conversation (`ordin.send_message`), with the references the
// block holds attached. Nothing here approves, sends or publishes anything outside ordin (11 §5.15).
import { answers } from '../backend/groups';
import type { CommandOutcome } from '../backend/backend';
import type { BackendState } from '../store/backend-state.svelte';
import type { ContentRef, OrdinSendMessageResult } from '../service-api/types.gen';

export const MAX_ATTACHMENTS = 20;

export function canAsk(backend: BackendState): boolean {
  return answers(backend, 'ordin.send_message') && !isWaitingForServer(backend);
}

/** A server's backend whose connection is not `connected` shows `server-waiting-for` and disables what would send. */
export function isWaitingForServer(backend: BackendState): boolean {
  return backend.kind === 'server' && backend.connection !== 'connected';
}

export function askOrdin(
  backend: BackendState,
  workspaceId: string,
  text: string,
  attachments: readonly ContentRef[] = [],
): Promise<CommandOutcome<OrdinSendMessageResult>> {
  const kept = attachments.slice(0, MAX_ATTACHMENTS);
  return backend.handle.call('ordin.send_message', {
    workspace_id: workspaceId,
    text,
    ...(kept.length > 0 ? { attachments: [...kept] } : {}),
  });
}
