// Registers the task form screen (SM-TF-1): offered only when the backend answers both commands it sends
// (SM-UP-1). Picked up by `lib/shell/load-registrations.ts`.
//
// spec: ordin-desktop/components/ScreensModule#SM-TF-1
import { registerScreen } from '../../shell/screens';
import { TASK_FORM_SCREEN } from './ids';
import TaskFormScreen from './TaskFormScreen.svelte';

registerScreen({ id: TASK_FORM_SCREEN, titleId: 'screen-task-form', component: TaskFormScreen, commands: ['agents.list', 'runs.start_task'] });
