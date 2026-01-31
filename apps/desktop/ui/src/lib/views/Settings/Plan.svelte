<script lang="ts">
  // Settings › Using your plan: Today, How Ordin uses your plan, the four switches, Summarise long
  // conversations, Ask before work estimated as, Agents working at the same time and the folded More
  // choices.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { formatPercent } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { ASK_BEFORE, CHOICES, PlanModel, SUMMARISE, SWITCHES, THINKING } from './plan-model.svelte';

  interface Props {
    backend: BackendState;
    model?: PlanModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new PlanModel(backend.handle);
  const settings = $derived(model.settings);
  const today = $derived(model.today);
  const permille = (value: number): string => formatPercent(value / 1000);

  onMount(() => void model.load());
</script>

<section class="plan" aria-labelledby="settings-plan-heading">
  <h3 id="settings-plan-heading">{t('settings-section-using-your-plan')}</h3>
  {#if today !== null}
    <div class="today">
      <h4>{t('settings-plan-today')}</h4>
      <p>{t('settings-plan-today-line', { percent: permille(today.typical_day_permille) })}</p>
      <details>
        <summary>{t('settings-plan-details')}</summary>
        <p>{t('settings-plan-today-details', { turns: today.model_turns, percent: permille(today.smaller_share_permille), answered: today.answered_without_model })}</p>
      </details>
    </div>
  {/if}
  {#if settings !== null}
    <fieldset>
      <legend>{t('settings-plan-choice')}</legend>
      {#each CHOICES as choice (choice.value)}
        <label>
          <input
            type="radio"
            name="plan-choice"
            value={choice.value}
            checked={settings.choice.value === choice.value}
            disabled={settings.choice.set_by === 'company'}
            onchange={() => void model.update({ choice: choice.value })}
          />
          {t(`settings-plan-choice-${choice.id}`)}
          <span class="secondary">{t(`settings-plan-choice-${choice.id}-line`)}</span>
        </label>
      {/each}
      <label>
        <input type="radio" name="plan-choice" value="custom" checked={settings.choice.value === 'custom'} disabled />
        {t('settings-plan-choice-custom')}
      </label>
      {#if settings.choice.set_by === 'company'}<span class="tag">{t('settings-set-by-company')}</span>{/if}
    </fieldset>
    {#each SWITCHES as sw (sw.field)}
      <p>
        <label>
          <input
            type="checkbox"
            role="switch"
            checked={settings[sw.field].value}
            disabled={settings[sw.field].set_by === 'company'}
            onchange={(e) => void model.update({ [sw.field]: e.currentTarget.checked })}
          />
          {t(sw.id)}
        </label>
        {#if settings[sw.field].set_by === 'company'}<span class="tag">{t('settings-set-by-company')}</span>{/if}
      </p>
    {/each}
    <p>
      <label>
        {t('settings-plan-summarise')}
        <select value={settings.summarise_long_conversations.value} disabled={settings.summarise_long_conversations.set_by === 'company'} onchange={(e) => void model.update({ summarise_long_conversations: e.currentTarget.value as never })}>
          {#each SUMMARISE as option (option.value)}<option value={option.value}>{t(option.id)}</option>{/each}
        </select>
      </label>
      {#if settings.summarise_long_conversations.set_by === 'company'}<span class="tag">{t('settings-set-by-company')}</span>{/if}
    </p>
    <p>
      <label>
        {t('settings-plan-ask-before')}
        <select value={settings.ask_before_estimated_requests.value} disabled={settings.ask_before_estimated_requests.set_by === 'company'} onchange={(e) => void model.update({ ask_before_estimated_requests: e.currentTarget.value as never })}>
          {#each ASK_BEFORE as option (option.value)}<option value={option.value}>{t(option.id)}</option>{/each}
        </select>
      </label>
      {#if settings.ask_before_estimated_requests.set_by === 'company'}<span class="tag">{t('settings-set-by-company')}</span>{/if}
    </p>
    <p class="agents-at-once" role="group" aria-label={t('settings-plan-agents-at-once')}>
      <span>{t('settings-plan-agents-at-once')}</span>
      <button type="button" aria-label="−" disabled={settings.agents_at_same_time.set_by === 'company'} onclick={() => void model.stepAgents(-1)}>−</button>
      <output>{settings.agents_at_same_time.value}</output>
      <button type="button" aria-label="+" disabled={settings.agents_at_same_time.set_by === 'company'} onclick={() => void model.stepAgents(1)}>+</button>
      {#if settings.agents_at_same_time.set_by === 'company'}<span class="tag">{t('settings-set-by-company')}</span>{/if}
    </p>
    <details class="more">
      <summary>{t('settings-plan-more')}</summary>
      <p>
        <label>
          <input
            type="checkbox"
            role="switch"
            checked={settings.show_usage_on_each_run.value}
            disabled={settings.show_usage_on_each_run.set_by === 'company'}
            onchange={(e) => void model.update({ show_usage_on_each_run: e.currentTarget.checked })}
          />
          {t('settings-plan-show-usage')}
        </label>
      </p>
      <p>
        <label>
          {t('settings-plan-thinking')}
          <select value={settings.thinking.value} disabled={settings.thinking.set_by === 'company'} onchange={(e) => void model.update({ thinking: e.currentTarget.value as never })}>
            {#each THINKING as option (option.value)}<option value={option.value}>{t(option.id)}</option>{/each}
          </select>
        </label>
      </p>
    </details>
  {/if}
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>

<style>
  /* Settings › Using your plan (scene `useSettings`, the prototype's `drawSetUse`): Today on the ground, the
     choices as bordered cards (the chosen one outlined in accent), then one row per setting, a divider above
     each, its words at the start and its control at the end, all in Small text. */
  .plan {
    font-size: 13px;
  }
  .today {
    padding: var(--space-sm) var(--space-10);
    border-radius: var(--radius-md);
    background: var(--color-background);
  }
  .today > h4 {
    font-size: 13px;
  }
  .today > h4,
  .today > p,
  .today > details {
    margin: 0;
  }
  fieldset {
    border: 0;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-06);
  }
  legend {
    font-size: 13px;
    font-weight: 600;
    margin-block-end: var(--space-06);
    padding: 0;
  }
  fieldset > label {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: var(--space-sm);
    align-items: start;
    padding: var(--space-sm) var(--space-10);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    font-weight: 600;
    cursor: pointer;
  }
  fieldset > label:has(input:checked) {
    border-color: var(--color-accent);
    box-shadow: inset 0 0 0 1px var(--color-accent);
  }
  fieldset > label > input {
    grid-row: 1 / span 2;
  }
  .secondary {
    font-size: 12px;
    font-weight: 400;
    color: var(--color-text-muted);
  }
  .plan > p,
  .more > p {
    margin: 0;
    padding: var(--space-06) 0;
    border-block-start: 1px solid var(--color-line);
    display: flex;
    gap: var(--space-06);
    align-items: center;
    flex-wrap: wrap;
  }
  .plan > p > label,
  .more > p > label {
    flex: 1;
    display: flex;
    gap: var(--space-06);
    align-items: center;
    justify-content: space-between;
    font-weight: 600;
  }
  .plan > p > label:has(input) {
    justify-content: flex-start;
  }
  select {
    flex: none;
    max-inline-size: 48%;
    padding: 3px var(--space-06);
    font-size: 13px;
    font-weight: 400;
  }
  .agents-at-once > span {
    flex: 1;
    font-weight: 600;
  }
  .agents-at-once > button {
    padding: 3px var(--space-sm);
    min-height: 28px;
    font-size: 13px;
  }
  .tag {
    font-size: 12px;
    font-weight: 700;
    color: var(--color-text-muted);
  }
  .more > summary {
    cursor: pointer;
    padding: var(--space-06) 0;
    font-size: 16px;
    font-weight: 700;
  }
</style>
