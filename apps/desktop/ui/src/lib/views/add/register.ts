// Registers *Add to this workspace* (SM-AW-1), one per workspace, offered only when the backend answers the
// commands it always sends (SM-UP-1); the commands of each entry gate that entry.
//
// spec: ordin-desktop/components/ScreensModule#SM-AW-1
import { registerScreen } from '../../shell/screens';
import { ADD_TO_WORKSPACE_SCREEN } from './add-model.svelte';
import AddToWorkspace from './AddToWorkspace.svelte';

registerScreen({
  id: ADD_TO_WORKSPACE_SCREEN,
  titleId: 'add-title',
  component: AddToWorkspace,
  commands: ['workspaces.links', 'workspaces.add_link', 'workspaces.link_progress'],
});
