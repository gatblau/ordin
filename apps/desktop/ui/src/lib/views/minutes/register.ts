// Registers the minutes page and the *Needs you* lines of the minutes jobs, each offered only when the
// backend answers every command it sends (SM-UP-1), so a build without the `minutes` switch offers neither.
// There is no Minutes screen: minutes, their jobs, Record… and Make minutes from a file… live in the tree
// (SM-MIN-17).
//
// spec: ordin-desktop/components/ScreensModule#SM-MIN-1
// spec: ordin-desktop/components/ScreensModule#SM-MIN-17
// spec: ordin-desktop/components/ScreensModule#SM-MIN-21
import { registerNeedsYouSlot, registerScreen } from '../../shell/screens';
import MinutesNeedsYou from './MinutesNeedsYou.svelte';
import MinutesPage from './MinutesPage.svelte';
import { MINUTES_PAGE_SCREEN } from './register-ids';

registerScreen({
  id: MINUTES_PAGE_SCREEN,
  titleId: 'minutes-heading',
  component: MinutesPage,
  commands: ['minutes.get', 'minutes.save_edit', 'minutes.export', 'minutes.transcript', 'minutes.remove'],
});
registerNeedsYouSlot({ id: 'minutes-questions', component: MinutesNeedsYou });
