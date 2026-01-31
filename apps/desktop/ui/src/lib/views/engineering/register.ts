// Registers the engineering screens (code editor, Changes waiting, terminal, project settings) under the
// identifiers below, each offered only when the backend answers every command it sends (SM-UP-1).
//
// The four screen identifiers below are SM-CU-2's. A screen key is `<id>:<instance>`: the editor's
// instance is `<project id>:<path>`, the others' is the project id.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
// spec: ordin-desktop/components/ScreensModule#SM-CU-2
import { registerScreen } from '../../shell/screens';
import ChangesWaiting from './ChangesWaiting.svelte';
import CodeEditor from './CodeEditor.svelte';
import ProjectSettings from './ProjectSettings.svelte';
import Terminal from './Terminal.svelte';

export const ENGINEERING_SCREENS = {
  codeEditor: 'code-editor',
  changesWaiting: 'changes-waiting',
  terminal: 'terminal',
  projectSettings: 'project-settings',
} as const;

registerScreen({
  id: ENGINEERING_SCREENS.codeEditor,
  titleId: 'engineering-editor-title',
  component: CodeEditor,
  commands: ['projects.editor_open', 'projects.editor_close', 'projects.lsp_send', 'projects.save_file', 'files.preview'],
});
registerScreen({
  id: ENGINEERING_SCREENS.changesWaiting,
  titleId: 'engineering-changes-title',
  component: ChangesWaiting,
  commands: ['projects.review', 'projects.decide_entry', 'projects.commit', 'projects.push'],
});
registerScreen({
  id: ENGINEERING_SCREENS.terminal,
  titleId: 'engineering-terminal-title',
  component: Terminal,
  commands: ['projects.terminal_open', 'projects.terminal_input', 'projects.terminal_resize', 'projects.terminal_close'],
});
registerScreen({
  id: ENGINEERING_SCREENS.projectSettings,
  titleId: 'engineering-settings-title',
  component: ProjectSettings,
  commands: ['projects.set_check_commands', 'projects.language_servers_answer', 'projects.answer_language_servers'],
});
