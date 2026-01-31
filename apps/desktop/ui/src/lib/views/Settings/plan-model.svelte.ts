// Settings › Using your plan (11 §5.18, UX-A32): today's use, the plan choice, the four switches, when to
// summarise, when to ask, how many agents work at once, and the folded "More choices". Each change sends
// `usage.update_settings` with the one field changed; the control keeps the person's choice while the
// command is in flight and returns to the stored value on a refusal. The model level per kind and the daily
// pause are not drawn in R1 (G-CU-6).
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-1
import type { BackendHandle } from '../../backend/backend';
import type { AskBefore, ServiceError, SummariseWhen, ThinkingLevel, Today, UsageChoice, UsageSettings, UsageSettingsChange } from '../../service-api/types.gen';

/** The choosable plans, with the id suffix of their message (`custom` is drawn but never chosen). */
export const CHOICES: readonly { value: Exclude<UsageChoice, 'custom'>; id: string }[] = [
  { value: 'save_my_plan', id: 'save' },
  { value: 'balanced', id: 'balanced' },
  { value: 'best_results', id: 'best' },
];
export const SWITCHES = [
  { field: 'answer_simple_requests_without_model', id: 'settings-plan-without-model' },
  { field: 'reuse_answers', id: 'settings-plan-reuse' },
  { field: 'give_agents_only_needed_parts', id: 'settings-plan-only-needed' },
  { field: 'continue_from_summary_after_break', id: 'settings-plan-continue-after-break' },
] as const;
export const SUMMARISE: readonly { value: SummariseWhen; id: string }[] = [
  { value: 'early', id: 'settings-plan-summarise-early' },
  { value: 'when_nearly_full', id: 'settings-plan-summarise-nearly-full' },
  { value: 'only_when_asked', id: 'settings-plan-summarise-only-asked' },
];
export const ASK_BEFORE: readonly { value: AskBefore; id: string }[] = [
  { value: 'heavy', id: 'settings-plan-ask-heavy' },
  { value: 'medium_or_heavy', id: 'settings-plan-ask-medium-or-heavy' },
  { value: 'never', id: 'settings-plan-ask-never' },
];
export const THINKING: readonly { value: ThinkingLevel; id: string }[] = [
  { value: 'low', id: 'settings-plan-thinking-low' },
  { value: 'automatic', id: 'settings-plan-thinking-automatic' },
  { value: 'high', id: 'settings-plan-thinking-high' },
];

/** The least number of agents that can work at once (a count of agents cannot go below one). */
export const AGENTS_MIN = 1;

/** Lays one changed field over the stored settings, keeping each field's `set_by`. */
export function applyChange(stored: UsageSettings, change: UsageSettingsChange): UsageSettings {
  const next = { ...stored };
  for (const key of ['agents_at_same_time', 'answer_simple_requests_without_model', 'ask_before_estimated_requests', 'choice', 'continue_from_summary_after_break', 'give_agents_only_needed_parts', 'reuse_answers', 'show_usage_on_each_run', 'summarise_long_conversations', 'thinking'] as const) {
    const value = change[key];
    if (value != null) (next as Record<string, unknown>)[key] = { ...stored[key], value };
  }
  return next;
}

export class PlanModel {
  today = $state.raw<Today | null>(null);
  settings = $state.raw<UsageSettings | null>(null);
  error = $state<ServiceError | null>(null);
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  async load(): Promise<void> {
    const [today, settings] = await Promise.all([this.#handle.call('usage.today', {}), this.#handle.call('usage.get_settings', {})]);
    if (today.kind === 'ok') this.today = today.result;
    if (settings.kind === 'ok') {
      this.settings = settings.result;
      this.error = null;
    } else this.error = settings.error;
  }

  async update(change: UsageSettingsChange): Promise<boolean> {
    const stored = this.settings;
    if (stored === null) return false;
    this.settings = applyChange(stored, change);
    const out = await this.#handle.call('usage.update_settings', change);
    if (out.kind === 'ok') {
      this.settings = out.result;
      this.error = null;
      return true;
    }
    this.settings = stored;
    this.error = out.error;
    return false;
  }

  /** **−** and **+** of the agents count. */
  stepAgents(by: 1 | -1): Promise<boolean> {
    const now = this.settings?.agents_at_same_time.value ?? AGENTS_MIN;
    const next = Math.max(AGENTS_MIN, now + by);
    return next === now ? Promise.resolve(true) : this.update({ agents_at_same_time: next });
  }
}
