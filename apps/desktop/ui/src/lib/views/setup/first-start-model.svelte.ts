// First start (ScreensModule.md, amendment January 2026, SM-FS-1 to SM-FS-5): whether the window draws
// its three regions or only `FirstStart` (`setup.status`), the guided run (`setup.run_guided`) and the
// progress it publishes (`setup_progress`), and signing in to the harness (`setup.sign_in_harness`).
//
// spec: ordin-desktop/components/ScreensModule#SM-FS-1
// spec: ordin-desktop/components/ScreensModule#SM-FS-2
// spec: ordin-desktop/components/ScreensModule#SM-FS-3
// spec: ordin-desktop/components/ScreensModule#SM-FS-4
// spec: ordin-desktop/components/ScreensModule#SM-FS-5
// spec: ordin-desktop/components/ScreensModule#SM-FS-9
import type { BackendHandle, EventEnvelope } from '../../backend/backend';
import { t } from '../../i18n/fluent';
import { WebPageOpener } from '../Settings/web-page.svelte';
import type {
  CheckKind,
  CheckResult,
  HarnessAttachment,
  HarnessKind,
  PersonAction,
  ServiceError,
  SetupProgress,
  SetupReport,
  StepState,
} from '../../service-api/types.gen';

/** `checking`: `setup.status` has not answered yet; `first_start`: only `FirstStart` is drawn; `regions`: the three regions. */
export type StartPhase = 'checking' | 'first_start' | 'regions';

/** The run-state dot of a step's state (V6 *Run-state dot*): `waiting` → not yet, `running` → now, `done` → done, `needs_you` → waiting, `failed` → failed. */
export type DotTone = 'empty' | 'now' | 'done' | 'waiting' | 'failed';

export const DOT_OF_STATE: Readonly<Record<StepState, DotTone>> = {
  waiting: 'empty',
  running: 'now',
  done: 'done',
  needs_you: 'waiting',
  failed: 'failed',
};

/** The glyph each dot carries (V6): none, •, ✓, ! and ×. Text, not an icon (V8 *Status glyphs are not icons*). */
export const DOT_GLYPH: Readonly<Record<DotTone, string>> = { empty: '', now: '•', done: '✓', waiting: '!', failed: '×' };

/** The message of a step's kind: `setup-step-<kind with _ as ->` (SM-FS-5). */
export const stepMessageId = (kind: CheckKind): string => `setup-step-${kind.replaceAll('_', '-')}`;

/**
 * The screen-reader word of a step's state, `setup-step-state-<state>` (SM-FS-2) with `_` written `-`: the
 * catalogue build accepts kebab-case ids only, so `needs_you` is `setup-step-state-needs-you`.
 */
export const stateMessageId = (state: StepState): string => `setup-step-state-${state.replaceAll('_', '-')}`;

/** The message of a Setup Assistant result code: `<code lower-cased, _ as ->` (SM-FS-5). */
export const resultMessageId = (code: string): string => code.toLowerCase().replaceAll('_', '-');

/** The id of the harness's name: `harness-claude-code` or `harness-copilot` (SM-FS-2). */
export const harnessNameId = (harness: HarnessKind): string => (harness === 'copilot_cli' ? 'harness-copilot' : 'harness-claude-code');

/** The button a failed result's action draws (SM-FS-4); `null` → the sentence only. */
export type ResultButton =
  | { kind: 'try_again' }
  | { kind: 'sign_in' }
  | null;

export function resultButton(action: PersonAction | null | undefined): ResultButton {
  switch (action) {
    case 'try_again':
      return { kind: 'try_again' };
    // SM-FS-4: no other action draws a button; the sentence says what to do.
    case 'sign_in':
      return { kind: 'sign_in' };
    default:
      return null;
  }
}

const NO_PROGRESS: SetupProgress = { done: 0, total: 0, running: false, steps: [] };

export class FirstStartModel {
  phase = $state<StartPhase>('checking');
  /** A failed `setup.status`, drawn in the frame with Try again (SM-FS-1). */
  statusError = $state.raw<ServiceError | null>(null);
  progress = $state.raw<SetupProgress>(NO_PROGRESS);
  attachments = $state.raw<HarnessAttachment[]>([]);
  /** The last `setup.run_guided` answer; `null` while one is running. */
  report = $state.raw<SetupReport | null>(null);
  /** A `ServiceError` answer of `setup.run_guided` (SM-FS-4). */
  runError = $state.raw<ServiceError | null>(null);
  signingIn = $state(false);
  signInError = $state.raw<ServiceError | null>(null);
  readonly handle: BackendHandle;
  /** **Install Claude Code** opens the official install page with `app.open_web_page` (SM-FS-9). */
  readonly pages: WebPageOpener;
  #guidedSent = false;

  /** `handle` is the local backend's: first start asks the local backend only (SM-FS-1). */
  constructor(handle: BackendHandle) {
    this.handle = handle;
    this.pages = new WebPageOpener(handle);
  }

  /** One event of the local backend's store; `setup_progress` is taken from the local backend only (SM-FS-2). */
  receive(env: EventEnvelope): void {
    if (env.kind === 'setup_progress' && this.phase === 'first_start') this.progress = env.progress;
  }

  /** `setup.status {}` (SM-FS-1). Sent at start-up, and again by Try again after a failure. */
  async checkStatus(): Promise<void> {
    const out = await this.handle.call('setup.status', {});
    if (out.kind === 'error') {
      this.statusError = { ...out.error, action: 'try_again' };
      this.phase = 'first_start';
      return;
    }
    this.statusError = null;
    this.attachments = out.result.attachments;
    this.progress = out.result.progress;
    this.phase = out.result.needs_guided ? 'first_start' : 'regions';
  }

  /** Sends `setup.run_guided {}` the first time only (SM-FS-2); `FirstStart` calls it once it is drawn. */
  begin(): void {
    if (this.#guidedSent || this.phase !== 'first_start' || this.statusError !== null) return;
    this.#guidedSent = true;
    void this.runGuided();
  }

  /** `setup.run_guided {}`; `ready = true` draws the regions (SM-FS-4). Also Try again's. */
  async runGuided(): Promise<void> {
    this.#guidedSent = true;
    this.report = null;
    this.runError = null;
    const out = await this.handle.call('setup.run_guided', {});
    if (out.kind === 'error') {
      this.runError = out.error;
      return;
    }
    this.report = out.result;
    if (out.result.ready) this.phase = 'regions';
  }

  /** The harness of the answer's first attachment; `claude_code` when there is none (SM-FS-2, SM-FS-3). */
  get harness(): HarnessKind {
    return this.attachments[0]?.harness ?? 'claude_code';
  }

  /** The harness's name for the words (`harness-claude-code` or `harness-copilot`). */
  get harnessName(): string {
    return t(harnessNameId(this.harness));
  }

  /** The failed results of a run that ended with no ready harness, in order (SM-FS-4). */
  get failedResults(): CheckResult[] {
    const report = this.report;
    if (report === null || report.ready) return [];
    return report.results.filter((r) => r.outcome === 'failed');
  }

  /** Claude Code is not on this computer: **Install Claude Code** and **Check again** are drawn (SM-FS-9). */
  get showInstall(): boolean {
    return this.harness === 'claude_code' && this.failedResults.some((r) => r.code === 'SETUP_HARNESS_MISSING');
  }

  /** **Install Claude Code**: `app.open_web_page { page: "claude_code_install" }` (SM-FS-9). */
  async installClaudeCode(): Promise<void> {
    await this.pages.open('claude_code_install');
  }

  /** The sign-in button is drawn while `harness_sign_in` needs the person, or a failed result asks for a sign-in (SM-FS-3, SM-FS-4). */
  get showSignIn(): boolean {
    if (this.progress.steps.some((s) => s.check === 'harness_sign_in' && s.state === 'needs_you')) return true;
    return this.failedResults.some((r) => r.action === 'sign_in');
  }

  /** `setup.sign_in_harness { harness }` (SM-FS-3). The guided run resumes by itself; nothing more is sent. */
  async signIn(): Promise<void> {
    if (this.signingIn) return;
    this.signingIn = true;
    this.signInError = null;
    const out = await this.handle.call('setup.sign_in_harness', { harness: this.harness });
    this.signingIn = false;
    if (out.kind === 'error') this.signInError = out.error;
  }
}
