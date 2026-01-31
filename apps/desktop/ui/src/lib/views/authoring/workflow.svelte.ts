// The workflow screen and its editor (SM-NS-5; screen `workflow:<workflow id>`). Control blocks, Show as diagram,
// shared workflows and Share… are not drawn in R1 (G-CU-4); the editor keeps a control block it does not draw
// exactly where it was, so a save never loses one.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-5
// spec: ordin-desktop/components/ScreensModule#WFF_NAME_DUPLICATE
import { stripIsolates, t } from '../../i18n/fluent';
import type {
  CheckProblem,
  ServiceError,
  Workspace,
  WorkflowFile,
  WorkflowFileStep,
  WorkflowView,
} from '../../service-api/types.gen';
import { Announcer, type Handle } from '../fixed/util';

export type Panel = 'rename' | 'duplicate' | 'export' | 'remove' | null;
export type AgentStep = Extract<WorkflowFileStep, { kind: 'agent' }>;

const DIALOG_CANCELLED = 'DSK_DIALOG_CANCELLED';

export function cloneFile(file: WorkflowFile): WorkflowFile {
  return JSON.parse(JSON.stringify(file)) as WorkflowFile;
}

/** The version a screen without an explicit one acts on: the one the view holds, else the newest line. */
export function currentVersion(view: WorkflowView): number {
  return view.version ?? view.versions.reduce((n, v) => Math.max(n, v.version), 0);
}

export class WorkflowController {
  view = $state.raw<WorkflowView | null>(null);
  error = $state.raw<ServiceError | null>(null);
  panelError = $state.raw<ServiceError | null>(null);
  panel = $state<Panel>(null);
  workspaces = $state.raw<Workspace[]>([]);
  editing = $state(false);
  draft = $state.raw<WorkflowFile | null>(null);
  problems = $state.raw<CheckProblem[]>([]);
  notices = $state.raw<CheckProblem[]>([]);
  changeNote = $state('');
  askingDiscard = $state(false);
  renameName = $state('');
  renameEmpty = $state(false);
  duplicateName = $state('');
  duplicateTo = $state('');
  savedAs = $state<number | null>(null);
  renamedFrom = $state<string | null>(null);
  ranCharterId = $state<string | null>(null);
  readonly announcer = new Announcer();
  readonly #handle: Handle;
  readonly #onclosed: () => void;
  #original = '';

  constructor(handle: Handle, readonly workflowId: string, readonly workspaceId: string | null, onclosed: () => void = () => undefined) {
    this.#handle = handle;
    this.#onclosed = onclosed;
  }

  get missingAgent(): boolean {
    return this.view?.agents.some((a) => a.presence === 'missing') ?? false;
  }
  get imported(): boolean {
    return this.view?.workflow.state === 'not_saved_yet';
  }
  get dirty(): boolean {
    return this.editing && this.draft !== null && JSON.stringify(this.draft) !== this.#original;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('workflows.get', { workflow_id: this.workflowId });
    if (out.kind === 'error') return void (this.error = out.error);
    this.error = null;
    this.view = out.result;
  }

  /** **Save to this workspace** on an imported workflow. */
  async saveImported(): Promise<void> {
    const view = this.view;
    if (view === null) return;
    const out = await this.#handle.call('workflows.save', {
      workflow_id: this.workflowId,
      base_version: view.version ?? null,
      change_note: stripIsolates(t('workflow-imported-note-text')),
      file: view.file,
    });
    if (out.kind === 'error') return void (this.error = out.error);
    this.savedAs = out.result.version;
    await this.load();
  }

  /** **Discard** of an imported workflow that was not saved. */
  async discardImported(): Promise<void> {
    const out = await this.#handle.call('workflows.discard_draft', { workflow_id: this.workflowId });
    if (out.kind === 'error') return void (this.error = out.error);
    this.#onclosed();
  }

  async restore(version: number): Promise<void> {
    const out = await this.#handle.call('workflows.restore', { workflow_id: this.workflowId, version });
    if (out.kind === 'error') return void (this.error = out.error);
    this.error = null;
    await this.load();
  }

  /** **Run…** sends `workflows.run`; unavailable while an agent is missing. The `crew_charter_id` it returns is kept. */
  async run(): Promise<void> {
    if (this.missingAgent) return;
    const out = await this.#handle.call('workflows.run', { workflow_id: this.workflowId });
    if (out.kind === 'error') return void (this.error = out.error);
    this.error = null;
    this.ranCharterId = out.result.crew_charter_id;
  }

  openPanel(panel: Exclude<Panel, null>): void {
    this.panelError = null;
    this.panel = panel;
    const view = this.view;
    if (view === null) return;
    if (panel === 'rename') {
      this.renameName = view.file.name;
      this.renameEmpty = false;
    } else if (panel === 'duplicate') {
      // A name that is sent to the core carries no bidirectional isolate marks.
      this.duplicateName = stripIsolates(t('workflow-copy-of', { name: view.file.name }));
      this.duplicateTo = view.workflow.workspace_id;
      void this.#loadWorkspaces();
    }
  }
  closePanel(): void {
    this.panel = null;
    this.panelError = null;
  }

  async #loadWorkspaces(): Promise<void> {
    const out = await this.#handle.call('workspaces.list', { filter: 'active' });
    if (out.kind === 'ok') this.workspaces = out.result;
  }

  /** The workspace choice: this one first, then the others of `workspaces.list`. */
  get workspaceChoices(): Workspace[] {
    const here = this.view?.workflow.workspace_id;
    const all = this.workspaces;
    return [...all.filter((w) => w.id === here), ...all.filter((w) => w.id !== here)];
  }

  /** **Rename**: an empty name is refused in place and not sent; `WFF_NAME_DUPLICATE` keeps the panel open. */
  async rename(): Promise<void> {
    const view = this.view;
    if (view === null) return;
    const name = this.renameName.trim();
    if (name === '') {
      this.renameEmpty = true;
      return;
    }
    this.renameEmpty = false;
    const old = view.file.name;
    const out = await this.#handle.call('workflows.rename', { workflow_id: this.workflowId, name });
    if (out.kind === 'error') return void (this.panelError = out.error);
    this.renamedFrom = old;
    this.closePanel();
    this.announcer.say(t('workflow-renamed-from', { name: old }));
    await this.load();
  }

  async duplicate(): Promise<void> {
    const view = this.view;
    if (view === null) return;
    const out = await this.#handle.call('workflows.duplicate', {
      workflow_id: this.workflowId,
      version: currentVersion(view),
      name: this.duplicateName.trim(),
      to_workspace_id: this.duplicateTo,
    });
    if (out.kind === 'error') return void (this.panelError = out.error);
    this.closePanel();
  }

  /** **Export…**: `workflows.export`, then the save dialog through `files.save_export`; a cancelled dialog draws nothing. */
  async export(): Promise<void> {
    const view = this.view;
    if (view === null) return;
    const out = await this.#handle.call('workflows.export', { workflow_id: this.workflowId, version: currentVersion(view) });
    if (out.kind === 'error') return void (this.panelError = out.error);
    const saved = await this.#handle.call('files.save_export', { file_name: out.result.file_name, text: out.result.yaml });
    if (saved.kind === 'error' && saved.error.code !== DIALOG_CANCELLED) return void (this.panelError = saved.error);
    this.closePanel();
  }

  /** **Remove…** after the confirmation (D11); the tab closes on `ok`. */
  async remove(): Promise<void> {
    const out = await this.#handle.call('workflows.remove', { workflow_id: this.workflowId, confirmed: true });
    this.closePanel();
    if (out.kind === 'error') return void (this.error = out.error);
    this.#onclosed();
  }

  // ---- the editor (SM-NS-5.3) ---------------------------------------------------------------------

  startEdit(): void {
    const view = this.view;
    if (view === null) return;
    this.draft = cloneFile(view.file);
    this.#original = JSON.stringify(this.draft);
    this.problems = [];
    this.notices = [];
    this.changeNote = '';
    this.askingDiscard = false;
    this.savedAs = null;
    this.editing = true;
  }

  /** The indexes, among the draft's steps, of the steps the editor draws (agent steps). */
  get stepIndexes(): number[] {
    return (this.draft?.steps ?? []).flatMap((s, i) => (s.kind === 'agent' ? [i] : []));
  }

  setStepLine(index: number, line: string): void {
    this.#update((file) => {
      const step = file.steps[index];
      if (step?.kind === 'agent') step.line = line;
    });
  }
  setInputLabel(index: number, label: string): void {
    this.#update((file) => {
      const input = file.inputs[index];
      if (input !== undefined) input.label = label;
    });
  }
  removeInput(index: number): void {
    this.#update((file) => void file.inputs.splice(index, 1));
  }
  removeStep(index: number): void {
    this.#update((file) => void file.steps.splice(index, 1));
  }
  /** Swaps an agent step with the previous (`-1`) or next (`+1`) agent step; a control block between them stays put. */
  moveStep(index: number, direction: -1 | 1): void {
    const indexes = this.stepIndexes;
    const at = indexes.indexOf(index);
    const other = indexes[at + direction];
    if (at < 0 || other === undefined) return;
    this.#update((file) => {
      const a = file.steps[index] as WorkflowFileStep;
      file.steps[index] = file.steps[other] as WorkflowFileStep;
      file.steps[other] = a;
    });
  }
  #update(change: (file: WorkflowFile) => void): void {
    if (this.draft === null) return;
    const next = cloneFile(this.draft);
    change(next);
    this.draft = next;
  }

  /** The problems that name a step by its position, and the ones that name nothing drawn (`at` unmatched). */
  problemsAt(position: string): CheckProblem[] {
    return this.problems.filter((p) => p.at === position);
  }
  get unplacedProblems(): CheckProblem[] {
    const positions = new Set((this.draft?.steps ?? []).map((s) => s.position));
    return this.problems.filter((p) => !positions.has(p.at));
  }

  /** **Save**: `workflows.check`; any problem is drawn and nothing is saved; else `workflows.save` with the change note. */
  async save(): Promise<boolean> {
    const view = this.view;
    const file = this.draft;
    if (view === null || file === null || this.changeNote.trim() === '') return false;
    const checked = await this.#handle.call('workflows.check', { workspace_id: view.workflow.workspace_id, file });
    if (checked.kind === 'error') {
      this.error = checked.error;
      return false;
    }
    this.error = null;
    this.problems = checked.result.problems;
    this.notices = checked.result.notices;
    if (checked.result.problems.length > 0) return false;
    const out = await this.#handle.call('workflows.save', {
      workflow_id: this.workflowId,
      base_version: view.version ?? null,
      change_note: this.changeNote.trim(),
      file,
    });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.savedAs = out.result.version;
    this.editing = false;
    this.draft = null;
    this.announcer.say(t('workflow-saved-as', { version: out.result.version }));
    await this.load();
    return true;
  }

  cancel(): void {
    if (this.dirty) this.askingDiscard = true;
    else this.#leave();
  }
  discardChanges(): void {
    this.#leave();
  }
  keepChanging(): void {
    this.askingDiscard = false;
  }
  #leave(): void {
    this.editing = false;
    this.draft = null;
    this.askingDiscard = false;
    this.problems = [];
    this.notices = [];
  }
}
