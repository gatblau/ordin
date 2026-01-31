// Changes waiting: the coding tasks of a project and the entries of their change sets, accepted or
// rejected one by one; then the engineer's commit and push (no approval card: the engineer is the
// approver, ADR-021). Editor and review traffic is never journalled.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-2
import type { DecisionOutcome, EntryDecision, ReviewTask, ServiceError, StatusEntry } from '../../service-api/types.gen';
import { Announcer, type Handle } from '../fixed/util';

export class ChangesController {
  tasks = $state.raw<ReviewTask[]>([]);
  own = $state.raw<StatusEntry[]>([]);
  loaded = $state(false);
  error = $state.raw<ServiceError | null>(null);
  busy = $state(false);
  /** The refusal or conflict of the last decision, by entry id. */
  conflicts = $state.raw<Record<string, { agents: string; yours: string }>>({});
  readonly announcer = new Announcer();
  readonly #handle: Handle;
  readonly projectId: string;

  constructor(handle: Handle, projectId: string) {
    this.#handle = handle;
    this.projectId = projectId;
  }

  async load(): Promise<void> {
    const review = await this.#handle.call('projects.review', { project_id: this.projectId });
    if (review.kind === 'error') {
      this.error = review.error;
      return;
    }
    this.error = null;
    this.tasks = review.result;
    const status = await this.#handle.call('projects.status', { project_id: this.projectId });
    if (status.kind === 'ok') this.own = status.result;
    this.loaded = true;
  }

  async decide(entry: string, decision: EntryDecision): Promise<DecisionOutcome | null> {
    if (this.busy) return null;
    this.busy = true;
    const out = await this.#handle.call('projects.decide_entry', { decision, entry });
    this.busy = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return null;
    }
    this.error = null;
    if (out.result.kind === 'conflict') {
      this.conflicts = { ...this.conflicts, [entry]: { agents: out.result.agents, yours: out.result.yours } };
    } else {
      const { [entry]: _gone, ...rest } = this.conflicts;
      this.conflicts = rest;
      await this.load();
    }
    return out.result;
  }

  /** Keep one of several attempts at a request; the others are set aside by the Project Service. */
  async keepAttempt(task: string): Promise<boolean> {
    const out = await this.#handle.call('projects.keep_attempt', { task });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    await this.load();
    return true;
  }

  /** The engineer's commit, then push. A failed commit sends no push. */
  async commitAndPush(message: string): Promise<boolean> {
    this.busy = true;
    const commit = await this.#handle.call('projects.commit', { message, project_id: this.projectId });
    if (commit.kind === 'error') {
      this.busy = false;
      this.error = commit.error;
      return false;
    }
    const push = await this.#handle.call('projects.push', { project_id: this.projectId });
    this.busy = false;
    if (push.kind === 'error') {
      this.error = push.error;
      return false;
    }
    this.error = null;
    await this.load();
    return true;
  }
}
