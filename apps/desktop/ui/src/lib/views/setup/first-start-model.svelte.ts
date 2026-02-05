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
//
// Which assistant, asked at first start (amendment 2026-02-04): the question while `harness_choice` needs
// the person, and `setup.choose_harness`.
// spec: ordin-desktop/components/ScreensModule#SM-FS-10
// spec: ordin-desktop/components/ScreensModule#SM-FS-11
// spec: ordin-desktop/components/ScreensModule#SM-FS-13
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

/** The sentence id the Setup Assistant gives a missing Copilot CLI (SetupAssistant.md SET-CPX-4). */
const COPILOT_MISSING = 'setup-harness-missing-copilot';

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
  /** The row the person chose in the question; none at the start (SM-FS-10). */
  chosen = $state<HarnessKind | null>(null);
  /** `setup.choose_harness` has been sent and not answered (SM-FS-10). */
  choosing = $state(false);
  /** An error answer of `setup.choose_harness`, drawn under the button (SM-FS-11). */
  chooseError = $state.raw<ServiceError | null>(null);
  /** The answer was `restarting: false`: the question is removed and the steps go on (SM-FS-11). */
  choiceAnswered = $state(false);
  /** The answer was `restarting: true`: the harness ordin starts again with (SM-FS-11). */
  restartingWith = $state<HarnessKind | null>(null);
  /** The harness ordin runs with now (`app.agent_settings` `harness_in_use`), read at first start (SM-FS-13). */
  inUse = $state<HarnessKind | null>(null);
  readonly handle: BackendHandle;
  /** **Install Claude Code** opens the official install page with `app.open_web_page` (SM-FS-9). */
  readonly pages: WebPageOpener;
  #guidedSent = false;
  /** `setup.status` has been asked again for the question's rows (SM-FS-10). */
  #choiceStatusSent = false;

  /** `handle` is the local backend's: first start asks the local backend only (SM-FS-1). */
  constructor(handle: BackendHandle) {
    this.handle = handle;
    this.pages = new WebPageOpener(handle);
  }

  /** One event of the local backend's store; `setup_progress` is taken from the local backend only (SM-FS-2). */
  receive(env: EventEnvelope): void {
    if (env.kind !== 'setup_progress' || this.phase !== 'first_start') return;
    this.progress = env.progress;
    // SM-FS-10: the rows come from the latest `setup.status` answer. The assistants are found during the
    // guided run, after the first answer, so the answer is asked for again when the question appears.
    if (this.showChoice && !this.#choiceStatusSent) {
      this.#choiceStatusSent = true;
      void this.refreshOffered();
    }
  }

  /** `setup.status {}` for the attachments only, when the question appears (SM-FS-10). */
  async refreshOffered(): Promise<void> {
    const out = await this.handle.call('setup.status', {});
    if (out.kind === 'ok') this.attachments = out.result.attachments;
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
    this.choiceAnswered = false;
    this.attachments = out.result.attachments;
    this.progress = out.result.progress;
    this.phase = out.result.needs_guided ? 'first_start' : 'regions';
    if (this.phase === 'first_start' && this.inUse === null) await this.readInUse();
  }

  /** SM-FS-13: the harness the guided run walks is the one in use, which a stored choice decides. */
  async readInUse(): Promise<void> {
    const out = await this.handle.call('app.agent_settings', {});
    if (out.kind === 'ok') this.inUse = out.result.harness_in_use === 'copilot' ? 'copilot_cli' : 'claude_code';
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

  /**
   * The chosen harness once the question is answered (SM-FS-11), else the harness in use when it has an
   * attachment (SM-FS-13), else the answer's first attachment; `claude_code` when there is none (SM-FS-2, SM-FS-3).
   */
  get harness(): HarnessKind {
    const inUse = this.attachments.some((a) => a.harness === this.inUse) ? this.inUse : null;
    return this.chosen ?? inUse ?? this.attachments[0]?.harness ?? 'claude_code';
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
    return this.harness === 'claude_code' && this.failedResults.some((r) => r.code === 'SETUP_HARNESS_MISSING' && r.message_id !== COPILOT_MISSING);
  }

  /** GitHub Copilot is not on this computer: **Check again** only, no install button (SM-CPX-1, ADR-123). */
  // spec: ordin-desktop/components/ScreensModule#SM-CPX-1
  get showCheckAgain(): boolean {
    return !this.showInstall && this.failedResults.some((r) => r.message_id === COPILOT_MISSING);
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

  /** The question is drawn while `harness_choice` needs the person and has not been answered (SM-FS-10, SM-FS-11). */
  get showChoice(): boolean {
    if (this.choiceAnswered || this.restartingWith !== null) return false;
    return this.progress.steps.some((s) => s.check === 'harness_choice' && s.state === 'needs_you');
  }

  /** The offered attachments (state `ready` or `needs_sign_in`), Claude Code first (SM-FS-10). */
  get offered(): HarnessAttachment[] {
    const offered = this.attachments.filter((a) => a.state === 'ready' || a.state === 'needs_sign_in');
    return [...offered.filter((a) => a.harness === 'claude_code'), ...offered.filter((a) => a.harness !== 'claude_code')];
  }

  /** A row of the question chosen; nothing is sent until the button is pressed (SM-FS-10). */
  pick(harness: HarnessKind): void {
    if (this.choosing) return;
    this.chosen = harness;
  }

  /** The main button: `setup.choose_harness { harness }` (SM-FS-10), and its three answers (SM-FS-11). */
  async choose(): Promise<void> {
    const harness = this.chosen;
    if (harness === null || this.choosing || this.restartingWith !== null) return;
    this.choosing = true;
    this.chooseError = null;
    const out = await this.handle.call('setup.choose_harness', { harness });
    this.choosing = false;
    if (out.kind === 'error') {
      // The button is the one action: the error line draws the sentence only (11 §4).
      this.chooseError = { code: out.error.code, message: out.error.message, reference: out.error.reference };
      // SETUP_CHOICE_NOT_SAVED keeps the row chosen; the other two redraw from `setup.status`.
      if (out.error.code === 'SETUP_HARNESS_NOT_OFFERED' || out.error.code === 'SETUP_NO_CHOICE_PENDING') {
        await this.checkStatus();
        if (!this.offered.some((a) => a.harness === this.chosen)) this.chosen = null;
      }
      return;
    }
    if (out.result.restarting) this.restartingWith = harness;
    else this.choiceAnswered = true;
  }
}
