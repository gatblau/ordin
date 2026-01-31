// Pressing a button on an approval card: `approval.decide { card_ref, decision }` to the card's backend, and
// what the reply means for the card (ScreensModule step 7).
import type { BackendState } from '../store/backend-state.svelte';
import type { ApprovalCard, PressDecision, ServiceError } from '../service-api/types.gen';

export type DecisionResult =
  | { kind: 'decided'; state: string; decided_on: string }
  /** The card was decided elsewhere or has closed: its sentence is shown and the buttons are not offered again. */
  | { kind: 'closed'; error: ServiceError }
  /** Any other refusal: its sentence is shown and the buttons stay. */
  | { kind: 'failed'; error: ServiceError };

/** The two refusals after which the card cannot be decided any more. */
export const CLOSED_CODES: readonly string[] = ['APPR_CARD_ALREADY_DECIDED', 'APPR_CARD_CLOSED'];

export async function decide(backend: BackendState, card: ApprovalCard, decision: PressDecision): Promise<DecisionResult> {
  const out = await backend.handle.call('approval.decide', { card_ref: decision === 'approve' ? card.buttons.approve : card.buttons.reject, decision });
  if (out.kind === 'ok') return { kind: 'decided', state: out.result.state, decided_on: out.result.decided_on };
  return CLOSED_CODES.includes(out.error.code) ? { kind: 'closed', error: out.error } : { kind: 'failed', error: out.error };
}
