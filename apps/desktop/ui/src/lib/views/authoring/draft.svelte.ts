// The agent draft (SM-NS-6; screen `agent-draft:<agent draft id>`) and the part draft (SM-NS-7.3; screen
// `part-draft:<id>`). Talk to the agent maker, the files shown to people who code, a rehearsal's added
// situations (G-CU-8) and a part's own form (G-CU-2) are not drawn in R1.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-6
// spec: ordin-desktop/components/ScreensModule#SM-NS-7
import { t } from '../../i18n/fluent';
import type {
  AgentDraftView,
  Finding,
  FixChoice,
  PartDraftView,
  Rehearsal,
  RepositorySummary,
  ReviewDraftState,
  ServiceError,
} from '../../service-api/types.gen';
import { orderedFindings } from '../fixed/check-it.svelte';
import { Announcer, type Handle } from '../fixed/util';

/** The message id of the band by a draft's state (SM-NS-6.1); `null` for a discarded draft. */
export function bandMessage(state: ReviewDraftState): string | null {
  switch (state) {
    case 'draft':
      return 'draft-band-draft';
    case 'waiting_for_review':
      return 'draft-band-waiting';
    case 'changes_asked_for':
      return 'draft-band-changes-asked';
    case 'approved_not_signed_yet':
      return 'draft-band-approved-not-signed';
    case 'published':
      return 'draft-band-published';
    case 'discarded':
      return null;
  }
}

/** The failures first, then the warnings, as the Check it panel orders them. */
export const orderedChecks = (checks: readonly Finding[]): Finding[] => orderedFindings(checks);
export const failureCount = (checks: readonly Finding[]): number => checks.filter((f) => f.severity === 'failure').length;

/** The text of a finding, from its message id and arguments. */
export const findingText = (f: Finding): string => t(f.message_id, { ...f.args, ...(f.part_identifier ? { part: f.part_identifier } : {}) });

/** The fixes of a finding that this screen offers, with their label ids (SM-NS-6.1). */
export function offeredFixes(f: Finding): Array<{ choice: FixChoice; label: string }> {
  const out: Array<{ choice: FixChoice; label: string }> = [];
  for (const fix of f.fixes) {
    if (fix.fix === 'fix_it') out.push({ choice: 'fix_it', label: 'check-fix-it' });
    else if (fix.fix === 'ask_ordin') out.push({ choice: 'ask_ordin', label: 'check-ask-ordin-to-fix' });
  }
  return out;
}

export type NextAction = 'fix' | 'rehearse' | 'try' | 'publish' | null;

export class AgentDraftController {
  view = $state.raw<AgentDraftView | null>(null);
  repositories = $state.raw<RepositorySummary[]>([]);
  rehearsal = $state.raw<Rehearsal | null>(null);
  error = $state.raw<ServiceError | null>(null);
  busy = $state(false);
  readonly announcer = new Announcer();
  readonly #handle: Handle;
  readonly #onclosed: () => void;

  constructor(handle: Handle, readonly draftId: string, readonly workspaceId: string | null, onclosed: () => void = () => undefined) {
    this.#handle = handle;
    this.#onclosed = onclosed;
  }

  /** The one next action of the **Next** bar (SM-NS-6.1). */
  get next(): NextAction {
    const v = this.view;
    if (v === null) return null;
    if (failureCount(v.checks) > 0) return 'fix';
    if (v.draft.rehearsal_id === undefined || v.draft.rehearsal_id === null) return 'rehearse';
    if (v.can_try && !v.draft.trial) return 'try';
    if (v.can_publish) return 'publish';
    return null;
  }

  /** The name of a repository the draft may be published to, from `catalogue.list_repositories`. */
  repositoryName(id: string): string {
    return this.repositories.find((r) => r.repository.id === id)?.repository.display_name ?? id;
  }

  async load(): Promise<void> {
    const [draft, repos] = await Promise.all([
      this.#handle.call('authoring.agent_draft', this.draftId),
      this.#handle.call('catalogue.list_repositories', {}),
    ]);
    if (repos.kind === 'ok') this.repositories = repos.result;
    if (draft.kind === 'error') return void (this.error = draft.error);
    this.error = null;
    this.view = draft.result;
  }

  /** **Fix it** and **Ask Ordin to fix it** (`authoring.apply_fix`), then the draft is read again. */
  async applyFix(finding: Finding, fix: FixChoice): Promise<void> {
    const out = await this.#handle.call('authoring.apply_fix', { draft: { kind: 'agent', id: this.draftId }, finding_code: finding.code, fix });
    if (out.kind === 'error') return void (this.error = out.error);
    await this.load();
  }

  /** **Rehearse** runs the situations the Authoring Service adds from the draft's own expectations (SA-UI-CU-04). */
  async rehearse(): Promise<void> {
    if (this.workspaceId === null || this.busy) return;
    this.busy = true;
    const out = await this.#handle.call('authoring.rehearse', { id: this.draftId, workspace_id: this.workspaceId, situations: [] });
    this.busy = false;
    if (out.kind === 'error') return void (this.error = out.error);
    this.rehearsal = out.result;
    await this.load();
  }

  async tryIt(): Promise<void> {
    if (this.workspaceId === null || this.busy) return;
    this.busy = true;
    const out = await this.#handle.call('authoring.try_it', { id: this.draftId, workspace_id: this.workspaceId });
    this.busy = false;
    if (out.kind === 'error') return void (this.error = out.error);
    await this.load();
  }

  /** **Publish…** raises the Approval Service's card (step 7); nothing else is drawn here. */
  async publish(): Promise<void> {
    const out = await this.#handle.call('authoring.publish', { kind: 'agent', id: this.draftId });
    if (out.kind === 'error') return void (this.error = out.error);
    await this.load();
  }

  async sendChange(): Promise<void> {
    const out = await this.#handle.call('authoring.send_change', { kind: 'agent', id: this.draftId });
    if (out.kind === 'error') return void (this.error = out.error);
    await this.load();
  }

  async setPublishTo(repository: string): Promise<void> {
    const out = await this.#handle.call('authoring.set_publish_to', { id: this.draftId, repository });
    if (out.kind === 'error') return void (this.error = out.error);
    this.view = out.result;
  }

  /** **Discard** after the confirmation; the tab closes on `ok`. */
  async discard(): Promise<void> {
    const out = await this.#handle.call('authoring.discard', { kind: 'agent', id: this.draftId });
    if (out.kind === 'error') return void (this.error = out.error);
    this.#onclosed();
  }
}

export class PartDraftController {
  view = $state.raw<PartDraftView | null>(null);
  /** The part's name, read with `catalogue.part` when the draft changes an existing part (`draft.part_id`). */
  partName = $state('');
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly #onclosed: () => void;

  constructor(handle: Handle, readonly draftId: string, onclosed: () => void = () => undefined) {
    this.#handle = handle;
    this.#onclosed = onclosed;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('authoring.part_draft', this.draftId);
    if (out.kind === 'error') return void (this.error = out.error);
    this.error = null;
    this.view = out.result;
    const partId = out.result.draft.part_id;
    if (partId !== undefined && partId !== null && this.partName === '') {
      const part = await this.#handle.call('catalogue.part', { id: partId });
      if (part.kind === 'ok') this.partName = part.result.part.name;
    }
  }

  /** The question `draft-part-changes-what`: both answers send the draft's own `fields` unchanged. */
  async answerChangesWhat(changes: boolean): Promise<void> {
    const view = this.view;
    if (view === null) return;
    const out = await this.#handle.call('authoring.update_part_draft', { id: this.draftId, changes_what_part_does: changes, fields: view.draft.fields });
    if (out.kind === 'error') return void (this.error = out.error);
    this.view = out.result;
  }

  /** A fix chosen under the draft's checks (SM-CHK-3), as `AgentDraftController.applyFix` does for an agent. */
  async applyFix(finding: Finding, fix: FixChoice): Promise<void> {
    const out = await this.#handle.call('authoring.apply_fix', { draft: { kind: 'part', id: this.draftId }, finding_code: finding.code, fix });
    if (out.kind === 'error') return void (this.error = out.error);
    await this.load();
  }

  async publish(): Promise<void> {
    const out = await this.#handle.call('authoring.publish', { kind: 'part', id: this.draftId });
    if (out.kind === 'error') return void (this.error = out.error);
    await this.load();
  }

  async discard(): Promise<void> {
    const out = await this.#handle.call('authoring.discard', { kind: 'part', id: this.draftId });
    if (out.kind === 'error') return void (this.error = out.error);
    this.#onclosed();
  }
}
