// SM-DIS-1: **Skip** on a question, **Dismiss** on a form and **Not now** on a crew plan set a waiting
// item aside without answering it. The press sends `ordin.answer_block` with `{ kind: 'dismissed' }`
// (OrdinHost.md OH-DIS-1); once the backend takes it the item stops waiting, so it leaves *Needs you*
// and the conversation's waiting cards. Nothing is granted or started.
//
// spec: ordin-desktop/components/ScreensModule#SM-DIS-1
import type { ServiceError } from '../service-api/types.gen';
import type { BackendState } from '../store/backend-state.svelte';

/** Dismisses the block; `null` when the backend took it, else the error to draw under the buttons. */
export async function dismissBlock(backend: BackendState, workspaceId: string, blockId: string): Promise<ServiceError | null> {
  const out = await backend.handle.call('ordin.answer_block', { block_id: blockId, workspace_id: workspaceId, answer: { kind: 'dismissed' } });
  if (out.kind === 'ok') {
    if (out.result.accepted) backend.markAnswered(workspaceId, blockId);
    return null;
  }
  return out.error;
}
