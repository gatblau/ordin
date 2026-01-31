<script lang="ts">
  // Settings › Using your plan, drawn as Settings › Memory draws its groups (SM-SW-6; amended January 2026;
  // user decision; Approved by the user January 2026): a usage card with Details behind a disclosure, then
  // one card each for How Ordin uses your plan (a radio group), Ways to use less, Limits and More choices.
  // Every row is one element holding its title, its hint and its control; the control is named by the
  // title and described by the hint, and "Set by your company" is drawn in the hint when the company set it.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  // spec: ordin-desktop/components/ScreensModule#SM-SW-6
  import { onMount } from 'svelte';

  import { formatPercent } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { AGENTS_MIN, ASK_BEFORE, CHOICES, PlanModel, shareId, SUMMARISE, SWITCHES, THINKING } from './plan-model.svelte';

  type SwitchField = (typeof SWITCHES)[number]['field'] | 'show_usage_on_each_run';
  type SelectField = 'summarise_long_conversations' | 'ask_before_estimated_requests' | 'thinking';

  interface Props {
    backend: BackendState;
    model?: PlanModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new PlanModel(backend.handle);
  const settings = $derived(model.settings);
  const today = $derived(model.today);
  let detailsOpen = $state(false);
  let moreOpen = $state(false);
  const permille = (value: number): string => formatPercent(value / 1000);
  /** Today's use said as a share of a typical day in words, never as a percentage (11 §5.18). */
  const todayLine = (value: number): string => {
    const id = shareId(value);
    return id === null ? t('settings-plan-today-over') : t('settings-plan-today-line', { share: t(id) });
  };
  /** The id stem of a row: its title is `<stem>-title`, its hint `<stem>-hint`. */
  const stem = (field: string): string => `settings-plan-${field.replaceAll('_', '-')}`;

  onMount(() => void model.load());
</script>

<section class="plan settings-section" aria-labelledby="settings-plan-heading">
  <h3 id="settings-plan-heading">{t('settings-section-using-your-plan')}</h3>
  {#if today !== null}
    <div class="group today" role="group" aria-label={t('settings-plan-today')}>
      <div class="today-line">
        <p>{todayLine(today.typical_day_permille)}</p>
        <button
          type="button"
          class="icon-button"
          aria-label={t('settings-plan-details')}
          title={t('settings-plan-details')}
          aria-expanded={detailsOpen}
          aria-controls="settings-plan-today-details"
          onclick={() => (detailsOpen = !detailsOpen)}><Icon name="chev" size="twist" /></button
        >
      </div>
      <p class="hint" id="settings-plan-today-details" hidden={!detailsOpen}>{t('settings-plan-today-details', { turns: today.model_turns, percent: permille(today.smaller_share_permille), answered: today.answered_without_model })}</p>
    </div>
  {/if}
  {#if settings !== null}
    {@const choiceLocked = settings.choice.set_by === 'company'}
    {@const agents = settings.agents_at_same_time}
    {@const agentsLocked = agents.set_by === 'company'}
    <div class="group">
      <fieldset class="choices" disabled={choiceLocked} aria-describedby={choiceLocked ? 'settings-plan-choice-company' : undefined}>
        <legend><h4>{t('settings-plan-choice')}</h4></legend>
        {#each CHOICES as choice (choice.value)}
          {@render radio(choice.value, choice.id, settings.choice.value === choice.value)}
        {/each}
        <!-- Custom is drawn only while it is the current choice (amended January 2026 (user decision)). -->
        {#if settings.choice.value === 'custom'}
          {@render radio('custom', 'custom', true, true)}
        {/if}
        {#if choiceLocked}<p class="hint" id="settings-plan-choice-company"><b>{t('settings-set-by-company')}</b></p>{/if}
      </fieldset>
    </div>

    <div class="group" role="group" aria-labelledby="settings-plan-less">
      <h4 id="settings-plan-less">{t('settings-plan-group-less')}</h4>
      <div class="rows">
        {#each SWITCHES as sw (sw.field)}
          {@render switchRow(sw.field, t(sw.id), t(`${sw.id}-line`))}
        {/each}
        {@render selectRow('summarise_long_conversations', t('settings-plan-summarise'), t('settings-plan-summarise-line'), SUMMARISE)}
      </div>
    </div>

    <div class="group" role="group" aria-labelledby="settings-plan-limits">
      <h4 id="settings-plan-limits">{t('settings-plan-group-limits')}</h4>
      <div class="rows">
        {@render selectRow('ask_before_estimated_requests', t('settings-plan-ask-before'), t('settings-plan-ask-before-line'), ASK_BEFORE)}
        <div class="row agents-at-once" role="group" aria-labelledby="settings-plan-agents-label" aria-describedby="settings-plan-agents-hint">
          <span class="words">
            <b id="settings-plan-agents-label">{t('settings-plan-agents-at-once')}</b>
            <small id="settings-plan-agents-hint">{t('settings-plan-agents-at-once-line')}{#if agentsLocked}{' '}<b>{t('settings-set-by-company')}</b>{/if}</small>
          </span>
          <span class="control stepper">
            <button type="button" class="small" aria-label={t('settings-plan-agents-fewer')} disabled={agentsLocked || agents.value <= AGENTS_MIN} onclick={() => void model.stepAgents(-1)}>−</button>
            <output aria-live="polite" aria-labelledby="settings-plan-agents-label">{agents.value}</output>
            <button type="button" class="small" aria-label={t('settings-plan-agents-more')} disabled={agentsLocked} onclick={() => void model.stepAgents(1)}>+</button>
          </span>
        </div>
      </div>
    </div>

    <div class="group more" role="group" aria-labelledby="settings-plan-more">
      <div class="more-head">
        <h4 id="settings-plan-more">{t('settings-plan-more')}</h4>
        <button
          type="button"
          class="icon-button"
          aria-labelledby="settings-plan-more"
          aria-expanded={moreOpen}
          aria-controls="settings-plan-more-rows"
          onclick={() => (moreOpen = !moreOpen)}><Icon name="chev" size="twist" /></button
        >
      </div>
      <div class="rows" id="settings-plan-more-rows" hidden={!moreOpen}>
        {@render switchRow('show_usage_on_each_run', t('settings-plan-show-usage'), t('settings-plan-show-usage-line'))}
        {@render selectRow('thinking', t('settings-plan-thinking'), t('settings-plan-thinking-line'), THINKING)}
      </div>
    </div>
  {/if}
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>

{#snippet radio(value: string, id: string, checked: boolean, fixed = false)}
  <label class="choice" data-choice={value}>
    <input
      type="radio"
      name="plan-choice"
      {value}
      {checked}
      disabled={fixed}
      aria-labelledby={`settings-plan-choice-${id}-title`}
      aria-describedby={`settings-plan-choice-${id}-hint`}
      onchange={() => void model.update({ choice: value as (typeof CHOICES)[number]['value'] })}
    />
    <span class="words">
      <b id={`settings-plan-choice-${id}-title`}>{t(`settings-plan-choice-${id}`)}{#if value === 'balanced'}{' '}{t('settings-default')}{/if}</b>
      <small id={`settings-plan-choice-${id}-hint`}>{t(`settings-plan-choice-${id}-line`)}</small>
    </span>
  </label>
{/snippet}

{#snippet switchRow(field: SwitchField, label: string, line: string)}
  {@const value = settings?.[field]}
  {#if value !== undefined}
    <div class="row" data-field={field}>
      <span class="words">
        <b id={`${stem(field)}-title`}>{label}</b>
        <small id={`${stem(field)}-hint`}>{line}{#if value.set_by === 'company'}{' '}<b>{t('settings-set-by-company')}</b>{/if}</small>
      </span>
      <input
        class="control switch"
        type="checkbox"
        role="switch"
        checked={value.value}
        disabled={value.set_by === 'company'}
        aria-labelledby={`${stem(field)}-title`}
        aria-describedby={`${stem(field)}-hint`}
        onchange={(e) => void model.update({ [field]: e.currentTarget.checked })}
      />
    </div>
  {/if}
{/snippet}

{#snippet selectRow(field: SelectField, label: string, line: string, options: readonly { value: string; id: string }[])}
  {@const value = settings?.[field]}
  {#if value !== undefined}
    <div class="row" data-field={field}>
      <span class="words">
        <b id={`${stem(field)}-title`}>{label}</b>
        <small id={`${stem(field)}-hint`}>{line}{#if value.set_by === 'company'}{' '}<b>{t('settings-set-by-company')}</b>{/if}</small>
      </span>
      <select
        class="control"
        value={value.value}
        disabled={value.set_by === 'company'}
        aria-labelledby={`${stem(field)}-title`}
        aria-describedby={`${stem(field)}-hint`}
        onchange={(e) => void model.update({ [field]: e.currentTarget.value as never })}
      >
        {#each options as option (option.value)}<option value={option.value}>{t(option.id)}</option>{/each}
      </select>
    </div>
  {/if}
{/snippet}

<style>
  /* One card per group, as Settings › Memory and Agents and models draw theirs (SM-SW-6, amended January
     2026). Each row is one flex line that wraps: its words grow at the start, its control sits at the end
     and drops below the words when the card is too narrow for both. */
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
    flex-shrink: 0;
    padding: var(--space-sm) var(--space-10);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
  }
  .group h4 {
    font-size: 13px;
    font-weight: 600;
    margin: 0;
  }
  .group > h4,
  .more-head {
    padding-block-end: var(--space-04);
    border-block-end: 1px solid var(--color-line);
  }
  .today {
    background: var(--color-background);
  }
  .today-line,
  .more-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-sm);
  }
  .today-line > p {
    margin: 0;
    font-size: 13px;
  }
  .today .hint {
    margin: 0;
  }
  .more:has([aria-expanded='false']) .more-head {
    padding-block-end: 0;
    border-block-end: 0;
  }
  .icon-button {
    min-height: 0;
    padding: var(--space-04);
    border: 0;
    background: none;
    border-radius: var(--radius-sm);
  }
  .icon-button:hover {
    background: var(--color-chip);
  }
  [hidden] {
    display: none !important;
  }
  .choices {
    border: 0;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .choices > legend {
    padding: 0;
    margin-block-end: var(--space-06);
  }
  .choice {
    display: flex;
    gap: var(--space-sm);
    align-items: flex-start;
    padding: var(--space-sm) var(--space-10);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    cursor: pointer;
  }
  .choice > input {
    margin: 2px 0 0;
    flex-shrink: 0;
    accent-color: var(--color-accent);
  }
  .choice:has(input:checked) {
    border-color: var(--color-accent);
    background: var(--color-accent-soft);
    box-shadow: inset 0 0 0 1px var(--color-accent);
  }
  .choice:has(input:disabled) {
    cursor: default;
  }
  .words {
    flex: 1 1 16rem;
    min-inline-size: 0;
    overflow-wrap: anywhere;
    font-size: 13px;
  }
  .words > b {
    display: block;
    font-weight: 600;
  }
  small {
    display: block;
    color: var(--color-text-muted);
    font-size: 12px;
    font-weight: 400;
  }
  small > b {
    font-weight: 600;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 0;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-06) var(--space-md);
    padding: var(--space-sm) 0;
  }
  .row + .row {
    border-block-start: 1px solid var(--color-line);
  }
  .control {
    flex: 0 0 auto;
    margin-inline-start: auto;
    max-inline-size: 100%;
  }
  select.control {
    min-inline-size: 0;
    padding: 3px var(--space-06);
    font-size: 13px;
  }
  .stepper {
    display: flex;
    gap: var(--space-06);
    align-items: center;
  }
  .stepper > output {
    min-inline-size: 2ch;
    text-align: center;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .stepper > button {
    min-height: 28px;
    min-inline-size: 28px;
    padding: 0 var(--space-sm);
  }
  /* The switch: a track with a knob, from the native checkbox so it keeps its keyboard and its state. */
  .switch {
    appearance: none;
    position: relative;
    inline-size: 34px;
    block-size: 20px;
    margin: 0;
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-pill);
    background: var(--color-background);
    cursor: pointer;
    transition: background-color 0.12s;
  }
  .switch::before {
    content: '';
    position: absolute;
    inset-block-start: 2px;
    inset-inline-start: 2px;
    inline-size: 14px;
    block-size: 14px;
    border-radius: 50%;
    background: var(--color-text-muted);
    transition: transform 0.12s;
  }
  .switch:checked {
    background: var(--color-accent);
    border-color: var(--color-accent);
  }
  .switch:checked::before {
    background: var(--color-surface);
    transform: translateX(14px);
  }
  :global([dir='rtl']) .switch:checked::before {
    transform: translateX(-14px);
  }
  .switch:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .switch:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
