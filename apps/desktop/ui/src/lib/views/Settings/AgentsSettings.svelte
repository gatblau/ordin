<script lang="ts">
  // Settings › Agents and models (SM-AGM-1; amended January 2026; user decision; Approved by the user
  // January 2026): what runs the person's agents, the model for each level and the effort for each kind
  // of work, one card per group as Settings › Memory draws them. Each choice the person leaves on its
  // default follows the organisation's profile, then the built-in default.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-AGM-1
  import { onMount } from 'svelte';

  import Icon from '../../icons/Icon.svelte';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import {
    AgentsSettingsModel,
    EFFORT_MESSAGE,
    HARNESS_NAME,
    KIND_MESSAGE,
    LEVEL_MESSAGE,
    READINESS_MESSAGE,
  } from './agents-settings-model.svelte';

  interface Props {
    backend: BackendState;
    model?: AgentsSettingsModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new AgentsSettingsModel(backend.handle);

  onMount(() => void model.load());

  const nameOf = (harness: keyof typeof HARNESS_NAME) => t(HARNESS_NAME[harness]);
  const effortWords = (value: string) => (EFFORT_MESSAGE[value] === undefined ? value : t(EFFORT_MESSAGE[value]));
</script>

<section class="agents-settings settings-section" aria-labelledby="settings-agents-heading">
  <h3 id="settings-agents-heading">{t('settings-section-agents-and-models')}</h3>
  <p class="intro">{t('settings-agents-intro')}</p>

  {#if model.view !== null}
    {@const view = model.view}
    <div class="group" role="group" aria-labelledby="settings-agents-runs">
      <h4 id="settings-agents-runs">{t('settings-agents-group-runs')}</h4>
      <label class="select-line">
        {t('settings-agents-runs-with')}
        <select data-control="harness" aria-describedby="settings-agents-runs-hint" disabled={model.busy} value={model.harnessChoice} onchange={(e) => void model.setHarness(e.currentTarget.value)}>
          <option value="">{t('settings-agents-runs-default', { name: nameOf(view.organisation_harness) })}</option>
          {#each model.harnesses as h (h.harness)}
            <option value={h.harness}>{h.readiness === 'ready' ? nameOf(h.harness) : `${nameOf(h.harness)} — ${t(READINESS_MESSAGE[h.readiness])}`}</option>
          {/each}
        </select>
      </label>
      <p class="hint" id="settings-agents-runs-hint">{t('settings-agents-runs-hint')}</p>
      <p class="hint" data-line="in-use">{t('settings-agents-runs-now', { name: nameOf(view.harness_in_use) })}</p>
    </div>

    {#each model.harnesses as h (h.harness)}
      <div class="group" role="group" data-harness={h.harness} aria-labelledby={`settings-agents-models-${h.harness}`}>
        <h4 id={`settings-agents-models-${h.harness}`}>{t('settings-agents-group-models', { name: nameOf(h.harness) })}</h4>
        <p class="hint">{t('settings-agents-models-hint')}</p>
        {#each h.models.filter((m) => LEVEL_MESSAGE[m.level] !== undefined) as m (m.level)}
          {@const id = `settings-agents-model-${h.harness}-${m.level}`}
          <div class="model-line">
            <label for={id}>{t(LEVEL_MESSAGE[m.level] ?? 'settings-agents-level-standard')}</label>
            <input
              {id}
              type="text"
              data-level={m.level}
              autocomplete="off"
              spellcheck="false"
              maxlength="100"
              value={m.chosen ?? ''}
              placeholder={m.default_model ?? t('settings-agents-model-placeholder-none', { name: nameOf(h.harness) })}
              aria-invalid={model.isInvalid(h.harness, m.level)}
              aria-describedby={model.isInvalid(h.harness, m.level) ? `${id}-invalid` : undefined}
              onchange={(e) => void model.setModel(h.harness, m.level, e.currentTarget.value)}
            />
            {#if m.chosen !== undefined && m.chosen !== null}
              <button type="button" class="icon-button" data-action="use-default" aria-label={t('settings-agents-use-default')} title={t('settings-agents-use-default')} disabled={model.busy} onclick={() => void model.setModel(h.harness, m.level, '')}><Icon name="undo" /></button>
            {/if}
          </div>
          {#if model.isInvalid(h.harness, m.level)}<p class="hint invalid" id={`${id}-invalid`}>{t('settings-agents-model-invalid')}</p>{/if}
        {/each}
      </div>

      <div class="group" role="group" data-effort={h.harness} aria-labelledby={`settings-agents-effort-${h.harness}`}>
        <h4 id={`settings-agents-effort-${h.harness}`}>{t('settings-agents-group-effort', { name: nameOf(h.harness) })}</h4>
        {#if h.effort_levels.length === 0}
          <p class="hint" data-line="no-effort">{t('settings-agents-effort-none', { name: nameOf(h.harness) })}</p>
        {:else}
          <p class="hint">{t('settings-agents-effort-hint')}</p>
          {#each h.effort as row (row.kind)}
            <label class="select-line">
              {t(KIND_MESSAGE[row.kind])}
              <select data-kind={row.kind} disabled={model.busy} value={row.chosen ?? ''} onchange={(e) => void model.setEffort(h.harness, row.kind, e.currentTarget.value)}>
                <option value="">{t('settings-agents-effort-default', { name: nameOf(h.harness) })}</option>
                {#each h.effort_levels as value (value)}<option {value}>{effortWords(value)}</option>{/each}
              </select>
            </label>
          {/each}
        {/if}
      </div>
    {/each}
  {/if}
  <ErrorLine error={model.error} onretry={() => (model.error?.code === 'DSK_PREFERENCES_WRITE_FAILED' ? model.retry() : void model.load())} />
</section>

<style>
  /* One card per group, as Settings › Memory draws its parts. */
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
    flex-shrink: 0;
    padding: var(--space-sm) var(--space-10);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
  }
  .group > h4 {
    font-size: 13px;
    margin: 0 0 var(--space-04);
    padding-block-end: var(--space-04);
    border-block-end: 1px solid var(--color-line);
  }
  .select-line,
  .model-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm);
    font-size: 13px;
  }
  .model-line > label {
    min-inline-size: 12em;
  }
  .model-line > input {
    flex: 1;
    min-inline-size: 10em;
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
  .hint {
    margin: 0 0 var(--space-04);
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .hint.invalid {
    color: var(--color-danger, var(--color-text-muted));
  }
</style>
