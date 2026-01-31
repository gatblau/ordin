// Project settings (SA-CLOSE-04, SM-SF-3.3): Code checking, then Check commands. The commands are
// set here and nowhere else; Save sends the list and the reply's `Project.check_commands` is redrawn.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-3
// spec: ordin-desktop/components/ScreensModule#SA-CLOSE-04
import type { ServiceError } from '../../service-api/types.gen';
import { Announcer, type Handle } from '../fixed/util';
import { ensureProject, projectOf, rememberProject } from './projects';

export const MAX_CHECK_COMMANDS = 20;
export const MAX_CHECK_COMMAND_LENGTH = 1000;

/** `true`, `false` or `null` (never answered), as `projects.language_servers_answer` returns it. */
export type CodeCheckingAnswer = boolean | null;

/** The sentence and the button of the Code checking section for each answer (SM-SF-3.3). */
export function answerLine(answer: CodeCheckingAnswer): { message: string; button: string } {
  if (answer === true) return { message: 'code-checking-on', button: 'code-checking-turn-off' };
  if (answer === false) return { message: 'code-checking-off', button: 'code-checking-yes' };
  return { message: 'code-checking-unanswered', button: 'code-checking-yes' };
}

export class ProjectSettingsController {
  /** One entry per text box. */
  commands = $state<string[]>([]);
  answer = $state<CodeCheckingAnswer | undefined>(undefined);
  displayName = $state('');
  error = $state.raw<ServiceError | null>(null);
  /** `projects.get` refused (`PROJ_NOT_FOUND`): drawn in place of the project's name (SM-WR-4). */
  projectError = $state.raw<ServiceError | null>(null);
  answerError = $state.raw<ServiceError | null>(null);
  saving = $state(false);
  answering = $state(false);
  readonly announcer = new Announcer();
  readonly #handle: Handle;
  readonly projectId: string;

  constructor(handle: Handle, projectId: string) {
    this.#handle = handle;
    this.projectId = projectId;
    const known = projectOf(handle.id, projectId);
    if (known !== undefined) {
      this.commands = [...known.check_commands];
      this.displayName = known.display_name;
    }
  }

  /** SM-WR-4: reads the project when this backend has not seen it. */
  async loadProject(): Promise<void> {
    const read = await ensureProject(this.#handle, this.projectId);
    if (!read.ok) {
      this.projectError = read.error;
      return;
    }
    this.projectError = null;
    this.commands = [...read.project.check_commands];
    this.displayName = read.project.display_name;
  }

  /** Reads the code-checking answer when the screen opens. */
  async loadAnswer(): Promise<void> {
    const out = await this.#handle.call('projects.language_servers_answer', { project_id: this.projectId });
    if (out.kind === 'ok') {
      this.answer = out.result;
      this.answerError = null;
    } else {
      this.answerError = out.error;
    }
  }

  /** Sends the answer and re-reads it after `ok` (SM-SF-3.3). */
  async setAnswer(allowed: boolean): Promise<void> {
    if (this.answering) return;
    this.answering = true;
    const out = await this.#handle.call('projects.answer_language_servers', { allowed, project_id: this.projectId });
    this.answering = false;
    if (out.kind === 'error') {
      this.answerError = out.error;
      return;
    }
    await this.loadAnswer();
  }

  get canAdd(): boolean {
    return this.commands.length < MAX_CHECK_COMMANDS;
  }

  add(): void {
    if (this.canAdd) this.commands = [...this.commands, ''];
  }

  remove(index: number): void {
    this.commands = this.commands.filter((_, i) => i !== index);
  }

  edit(index: number, text: string): void {
    this.commands = this.commands.map((c, i) => (i === index ? text : c));
  }

  /**
   * Sends the list (blank boxes are left out). A refusal (`PROJ_CHECKS_INVALID`, `PROJ_NOT_FOUND`) is the
   * command's error line and the boxes keep what was typed; the core checks the limits again.
   */
  async save(): Promise<boolean> {
    if (this.saving) return false;
    this.saving = true;
    const list = this.commands.map((c) => c.trim()).filter((c) => c !== '');
    const out = await this.#handle.call('projects.set_check_commands', { commands: list, project_id: this.projectId });
    this.saving = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    rememberProject(this.#handle.id, out.result);
    this.commands = [...out.result.check_commands];
    this.displayName = out.result.display_name;
    return true;
  }
}
