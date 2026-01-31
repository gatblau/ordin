// The Minutes screen's state (ScreensModule.md SM-MIN-2 to SM-MIN-6): `minutes.status`, `minutes.jobs` and
// `minutes.list` read when it opens and every 2 seconds while a job is moving; the questions a job waits on
// (the speech model, the one-time notice); Record and its reminder; the OBS password. The component draws
// it; the tests drive it.
//
// spec: ordin-desktop/components/ScreensModule#SM-MIN-2
import type { BackendHandle, ServiceError } from '../../backend/backend';
import type { MinutesJob, MinutesStatus, MinutesSummary } from '../../service-api/minutes.gen';
import { JOBS_POLL_MS, moving } from './minutes-model';
import { recording } from './recording.svelte';

type Handle = Pick<BackendHandle, 'call'>;

/** The person closed the operating system's file dialog: no job, nothing to say. */
const DIALOG_CANCELLED = 'DSK_DIALOG_CANCELLED';

export class MinutesScreenModel {
  status = $state.raw<MinutesStatus | null>(null);
  jobs = $state.raw<MinutesJob[]>([]);
  items = $state.raw<MinutesSummary[]>([]);
  /** The error of the last read. */
  error = $state<ServiceError | null>(null);
  /** The error of the last button. */
  actionError = $state<ServiceError | null>(null);
  /** The reminder before recording is open (SM-MIN-5). */
  askingToRecord = $state(false);
  /** Jobs whose question was answered here, so it is not asked again before the next read. */
  answered = $state.raw<ReadonlySet<string>>(new Set());
  busy = $state(false);
  readonly #handle: Handle;
  readonly #workspaceId: string;
  #timer: ReturnType<typeof setInterval> | null = null;
  /** The jobs seen done, so each is announced once. */
  #done = new Set<string>();
  #first = true;
  /** Called with the title of minutes that became ready (SM-MIN-7, `minutes-ready`). */
  onready: (title: string) => void = () => undefined;

  constructor(handle: Handle, workspaceId: string) {
    this.#handle = handle;
    this.#workspaceId = workspaceId;
  }

  /** The job waiting for the speech model while no consent was given (SM-MIN-3). */
  get consentJob(): MinutesJob | null {
    if (this.status === null || this.status.model_consent_given) return null;
    return this.jobs.find((j) => j.state.state === 'waiting_for_model' && !this.answered.has(j.id)) ?? null;
  }

  /** The job waiting for the one-time notice (SM-MIN-4). */
  get noticeJob(): MinutesJob | null {
    if (this.status === null || this.status.writer_notice_seen) return null;
    return this.jobs.find((j) => j.state.state === 'waiting_for_notice' && !this.answered.has(j.id)) ?? null;
  }

  async load(): Promise<void> {
    await Promise.all([this.readStatus(), this.readJobs(), this.readList()]);
  }

  async readStatus(): Promise<void> {
    const out = await this.#handle.call('minutes.status', { workspace_id: this.#workspaceId });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.status = out.result;
  }

  async readList(): Promise<void> {
    const out = await this.#handle.call('minutes.list', { workspace_id: this.#workspaceId });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.items = out.result.items;
  }

  async readJobs(): Promise<void> {
    const out = await this.#handle.call('minutes.jobs', { workspace_id: this.#workspaceId });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.jobs = out.result;
    const ready: string[] = [];
    for (const job of out.result) {
      if (job.state.state !== 'done' || this.#done.has(job.id)) continue;
      this.#done.add(job.id);
      if (!this.#first) ready.push(job.state.minutes_id);
    }
    this.#first = false;
    if (ready.length > 0) {
      await this.readList();
      for (const id of ready) {
        const title = this.items.find((m) => m.id === id)?.title;
        if (title !== undefined) this.onready(title);
      }
    }
    this.#schedule();
  }

  /** Reads every 2 seconds while a job is moving, and stops when none is. */
  #schedule(): void {
    const keepReading = this.jobs.some(moving);
    if (keepReading && this.#timer === null) {
      this.#timer = setInterval(() => void this.#tick(), JOBS_POLL_MS);
    } else if (!keepReading && this.#timer !== null) {
      clearInterval(this.#timer);
      this.#timer = null;
    }
  }

  async #tick(): Promise<void> {
    await Promise.all([this.readStatus(), this.readJobs()]);
  }

  dispose(): void {
    if (this.#timer !== null) clearInterval(this.#timer);
    this.#timer = null;
  }

  async #act<T>(send: () => Promise<{ kind: 'ok'; result: T } | { kind: 'error'; error: ServiceError }>): Promise<T | null> {
    this.busy = true;
    const out = await send();
    this.busy = false;
    if (out.kind === 'error') {
      if (out.error.code !== DIALOG_CANCELLED) this.actionError = out.error;
      return null;
    }
    this.actionError = null;
    return out.result;
  }

  /** **Make minutes from a file…** (`minutes.import`; the program opens the file dialog). */
  async importFile(): Promise<void> {
    const job = await this.#act(() => this.#handle.call('minutes.import', { workspace_id: this.#workspaceId }));
    if (job !== null) await this.readJobs();
  }

  /** **Record**: opens the reminder on every press (SM-MIN-5). */
  askRecord(): void {
    this.askingToRecord = true;
  }

  /** **Start recording** in the reminder: `minutes.record`; the indicator shows from its answer (SM-MIN-7). */
  async startRecording(): Promise<void> {
    this.askingToRecord = false;
    const view = await this.#act(() => this.#handle.call('minutes.record', { workspace_id: this.#workspaceId }));
    if (view !== null) recording.started(this.#handle, view);
    await this.readStatus();
  }

  async cancelJob(job: MinutesJob): Promise<void> {
    this.answered = new Set([...this.answered, job.id]);
    await this.#act(() => this.#handle.call('minutes.cancel_job', { job_id: job.id }));
    await this.readJobs();
  }

  async retry(job: MinutesJob): Promise<void> {
    await this.#act(() => this.#handle.call('minutes.retry', { job_id: job.id }));
    await this.readJobs();
  }

  /** **Download** in the speech model question (SM-MIN-3). */
  async giveModelConsent(job: MinutesJob): Promise<void> {
    this.answered = new Set([...this.answered, job.id]);
    await this.#act(() => this.#handle.call('minutes.give_model_consent', {}));
    await Promise.all([this.readStatus(), this.readJobs()]);
  }

  /** **Write the minutes** in the one-time notice (SM-MIN-4). */
  async acceptNotice(job: MinutesJob): Promise<void> {
    this.answered = new Set([...this.answered, job.id]);
    await this.#act(() => this.#handle.call('minutes.accept_notice', {}));
    await Promise.all([this.readStatus(), this.readJobs()]);
  }

  /** **Open OBS** (SM-MIN-6), then a fresh status. */
  async openObs(): Promise<void> {
    await this.#act(() => this.#handle.call('minutes.open_obs', {}));
    await this.readStatus();
  }

  /** **Get OBS**: OBS's download page in the browser (SM-MIN-6). */
  async getObs(): Promise<void> {
    await this.#act(() => this.#handle.call('app.open_web_page', { page: 'obs_download' }));
  }

  /** **Save** beside the OBS password field, then a fresh status (SM-MIN-6). The password is not kept here. */
  async savePassword(password: string): Promise<boolean> {
    const saved = await this.#act(() => this.#handle.call('minutes.save_obs_password', { password }));
    await this.readStatus();
    return saved !== null;
  }
}
