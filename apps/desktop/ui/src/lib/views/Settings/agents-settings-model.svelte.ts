// Settings › Agents and models (SM-AGM-1; amended January 2026; user decision; Approved by the user
// January 2026): the person's own choice of what runs their agents, the model for each level and the
// effort for each kind of work. Read with `app.agent_settings`, changed one choice at a time with
// `app.change_agent_setting`; the desktop program keeps them in its preferences and every run uses them
// (OrdinDesktop DSK-AGM-1). A choice left on its default follows the organisation's profile, then the
// built-in default. A refused change returns the control to its stored value;
// `DSK_PREFERENCES_WRITE_FAILED` offers **Try again**.
//
// spec: ordin-desktop/components/ScreensModule#SM-AGM-1
import type { BackendHandle } from '../../backend/backend';
import type {
  AgentHarnessView,
  AgentSettingsView,
  ChangeAgentSetting,
  HarnessName,
  HarnessReadiness,
  ModelLevel,
  ServiceError,
  WorkKind,
} from '../../service-api/types.gen';

export const HARNESS_NAME: Readonly<Record<HarnessName, string>> = {
  claude_code: 'settings-agents-name-claude-code',
  copilot: 'settings-agents-name-copilot',
};

/** The mark beside a harness that cannot run agents yet; `ready` has none. */
export const READINESS_MESSAGE: Readonly<Record<Exclude<HarnessReadiness, 'ready'>, string>> = {
  needs_set_up: 'settings-agents-state-needs-set-up',
  not_found: 'settings-agents-state-not-found',
  not_in_this_version: 'settings-agents-state-not-in-this-version',
};

export const LEVEL_MESSAGE: Readonly<Partial<Record<ModelLevel, string>>> = {
  smaller: 'settings-agents-level-smaller',
  standard: 'settings-agents-level-standard',
  most_capable: 'settings-agents-level-most-capable',
};

export const KIND_MESSAGE: Readonly<Record<WorkKind, string>> = {
  talking: 'settings-agents-kind-talking',
  coordinating: 'settings-agents-kind-coordinating',
  routine: 'settings-agents-kind-routine',
  documents: 'settings-agents-kind-documents',
  code: 'settings-agents-kind-code',
};

/** The words of each effort value; a value with no words here is shown as written. */
export const EFFORT_MESSAGE: Readonly<Record<string, string>> = {
  low: 'settings-agents-effort-low',
  medium: 'settings-agents-effort-medium',
  high: 'settings-agents-effort-high',
  xhigh: 'settings-agents-effort-xhigh',
  max: 'settings-agents-effort-max',
};

/** The rule the backend checks: 1–100 letters, digits, `.`, `_` and `-`. */
export function isModelName(name: string): boolean {
  return /^[A-Za-z0-9._-]{1,100}$/.test(name);
}

/** Drawn when it can run agents or is set up somewhere; the one in use and the chosen one always are. */
export function shownHarnesses(view: AgentSettingsView): AgentHarnessView[] {
  return view.harnesses.filter(
    (h) =>
      h.readiness === 'ready' ||
      h.readiness === 'needs_set_up' ||
      h.harness === view.harness_in_use ||
      h.harness === view.chosen_harness ||
      h.harness === view.organisation_harness,
  );
}

export class AgentsSettingsModel {
  view = $state.raw<AgentSettingsView | null>(null);
  error = $state<ServiceError | null>(null);
  /** A model typed that the backend would refuse, by `<harness>.<level>`. */
  invalid = $state.raw<ReadonlySet<string>>(new Set());
  busy = $state(false);
  #retry: (() => Promise<boolean>) | null = null;
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('app.agent_settings', {});
    if (out.kind === 'ok') {
      this.view = out.result;
      this.error = null;
    } else this.error = out.error;
  }

  get harnesses(): AgentHarnessView[] {
    return this.view === null ? [] : shownHarnesses(this.view);
  }

  /** The value of the "Run my agents with" control: `''` is "same as my organisation". */
  get harnessChoice(): HarnessName | '' {
    return this.view?.chosen_harness ?? '';
  }

  async change(input: ChangeAgentSetting): Promise<boolean> {
    this.#retry = () => this.change(input);
    this.busy = true;
    const out = await this.#handle.call('app.change_agent_setting', input);
    this.busy = false;
    if (out.kind === 'ok') {
      this.view = out.result;
      this.error = null;
      return true;
    }
    this.error = out.error.code === 'DSK_PREFERENCES_WRITE_FAILED' ? { ...out.error, action: 'try_again' } : out.error;
    return false;
  }

  setHarness(value: string): Promise<boolean> {
    const harness = value === 'claude_code' || value === 'copilot' ? value : null;
    return this.change({ change: 'harness', harness });
  }

  /** A typed model: an empty box goes back to the default; a name the backend would refuse is held back. */
  setModel(harness: HarnessName, level: ModelLevel, typed: string): Promise<boolean> {
    const key = `${harness}.${level}`;
    const name = typed.trim();
    const invalid = new Set(this.invalid);
    if (name !== '' && !isModelName(name)) {
      invalid.add(key);
      this.invalid = invalid;
      return Promise.resolve(false);
    }
    invalid.delete(key);
    this.invalid = invalid;
    const current = this.view?.harnesses.find((h) => h.harness === harness)?.models.find((m) => m.level === level)?.chosen ?? '';
    if (name === current) return Promise.resolve(true);
    return this.change({ change: 'model', harness, level, model: name === '' ? null : name });
  }

  isInvalid(harness: HarnessName, level: ModelLevel): boolean {
    return this.invalid.has(`${harness}.${level}`);
  }

  setEffort(harness: HarnessName, kind: WorkKind, value: string): Promise<boolean> {
    return this.change({ change: 'effort', harness, kind, effort: value === '' ? null : value });
  }

  retry(): void {
    void this.#retry?.();
  }
}
