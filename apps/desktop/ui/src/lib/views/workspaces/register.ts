// Registers the Archived and Bin screens (ScreensModule.md SM-WS-5, SM-WS-7; picked up by
// lib/shell/load-registrations.ts). Each is offered only when the backend answers its commands (SM-UP-1).
//
// spec: ordin-desktop/components/ScreensModule#SM-WS-5
// spec: ordin-desktop/components/ScreensModule#SM-WS-7
import { registerScreen } from '../../shell/screens';
import ArchivedScreen from './ArchivedScreen.svelte';
import BinScreen from './BinScreen.svelte';
import { ARCHIVED_SCREEN, BIN_SCREEN } from './lifecycle.svelte';

registerScreen({ id: ARCHIVED_SCREEN, titleId: 'archived-title', component: ArchivedScreen, commands: ['workspaces.list', 'workspaces.restore'] });
registerScreen({
  id: BIN_SCREEN,
  titleId: 'bin-title',
  component: BinScreen,
  commands: ['workspaces.list', 'workspaces.restore_from_bin', 'workspaces.delete_from_bin', 'workspaces.empty_bin', 'workspaces.bin_impact'],
});
