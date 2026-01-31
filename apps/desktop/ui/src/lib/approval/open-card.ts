// Opening an approval card when `approval_requested` arrives (ScreensModule step 7): as a screen in the
// card's own workspace, in front only when it answers what the person just did, otherwise as a new tab marked
// *new* ("Ordin does not move the person's place", 11 §4). A card from another workspace opens there without
// switching the window.
import type { EventEnvelope } from '../backend/backend';
import type { AppContext } from '../shell/context';
import { ScreenAreaState } from '../shell/screen-area.svelte';
import { makeKey, SCREEN_IDS } from '../shell/screens';
import type { BackendState } from '../store/backend-state.svelte';

/**
 * Handles one applied event. `answersWhatPersonJustDid` is true when the card answers the person's own last
 * action (they just asked to publish); the shell knows that, the event does not say.
 */
export function openApprovalCard(app: AppContext, state: BackendState, env: EventEnvelope, answersWhatPersonJustDid = false): string | null {
  if (env.kind !== 'approval_requested') return null;
  const card = env.card;
  const key = makeKey(SCREEN_IDS.approval, card.approval_id);
  // Opened in its own workspace's screen area; the window is not switched.
  app.screens.open(ScreenAreaState.scope(state.id, card.workspace_id), key, { title: card.question, inFront: answersWhatPersonJustDid });
  return key;
}
