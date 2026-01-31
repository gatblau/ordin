// Registers the handover screen and the lifecycle screen (SM-NS-8, SM-NS-9), each offered only when the
// backend answers every command it sends (SM-UP-1).
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-8
// spec: ordin-desktop/components/ScreensModule#SM-NS-9
import { registerScreen } from '../../shell/screens';
import HandoverScreen from './HandoverScreen.svelte';
import LifecycleScreen from './LifecycleScreen.svelte';

export const HANDOVER_SCREEN = 'handover';
export const LIFECYCLE_SCREEN = 'lifecycle';

registerScreen({
  id: HANDOVER_SCREEN,
  titleId: 'screen-handover',
  component: HandoverScreen,
  commands: ['handover.open', 'handover.accept_and_start', 'handover.return_options', 'handover.return_to', 'handover.take_back', 'handover.remind', 'workspaces.list'],
});
registerScreen({ id: LIFECYCLE_SCREEN, titleId: 'lifecycle-title', component: LifecycleScreen, commands: ['handover.stage_of', 'handover.lifecycle_view'] });
