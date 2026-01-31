// The message id of a run's (or a crew run's) stored state (ScreensModule.md, amendments *Run-state
// message ids* and *The labels of a crew run's states*). `paused` has three labels by condition, so no
// message `state-paused` exists and none is asked for: until the screens can tell the conditions apart
// (G-RS-1), every paused run or crew run draws `state-paused-approval`, whatever its `pause_reason`, the
// label that asks the person to look. A crew run's own states (`starting`, `finished`, `escalated`,
// `killed`) follow the same rule to the ids of SM-RS-6.
//
// spec: ordin-desktop/components/ScreensModule#SM-RS-1
// spec: ordin-desktop/components/ScreensModule#SM-RS-3
// spec: ordin-desktop/components/ScreensModule#SM-RS-4
// spec: ordin-desktop/components/ScreensModule#SM-RS-6
import type { CrewRunState, RunState } from '../service-api/types.gen';

/** `state-` and the stored state with every `_` as `-` (SM-RS-1); `paused` → `state-paused-approval` (SM-RS-4). */
export function stateMessageId(state: RunState | CrewRunState): string {
  if (state === 'paused') return 'state-paused-approval';
  return `state-${state.replace(/_/g, '-')}`;
}
