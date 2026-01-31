<script lang="ts">
  // Settings › Memory (SM-NS-1.9; amended January 2026; user decision; Approved by the user January 2026):
  // settings only, in three groups — What Ordin keeps, How memory is shown, How Ordin uses memory. The
  // facts themselves are seen and changed on the memory page in the side tree.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import { MEMORY_PAGE_SIZES } from '../../store/view-prefs';
  import ErrorLine from '../ErrorLine.svelte';
  import {
    GROUP_BY,
    GROUP_BY_MESSAGE,
    KEEP_CHOICES,
    KEEP_KEY,
    KEEP_MESSAGE,
    MemorySettingsModel,
    REMOVAL_CHOICES,
    REMOVAL_KEY,
    SOURCE_SWITCHES,
    USE_SWITCHES,
    type ShownPrefs,
  } from './memory-settings-model.svelte';

  interface Props {
    backend: BackendState;
    prefs?: ShownPrefs;
    model?: MemorySettingsModel;
  }
  let { backend, prefs, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new MemorySettingsModel(backend.handle, prefs);

  onMount(() => void model.load());
</script>

<section class="memory-settings settings-section" aria-labelledby="settings-memory-heading">
  <h3 id="settings-memory-heading">{t('settings-section-what-ordin-remembers')}</h3>
  <p class="intro">{t('settings-memory-intro')}</p>

  {#if model.loaded}
    <div class="group" role="group" aria-labelledby="settings-memory-keeps">
      <h4 id="settings-memory-keeps">{t('settings-memory-group-keeps')}</h4>
      {#if model.has(KEEP_KEY)}
        <fieldset class="choice-group keep" disabled={model.locked(KEEP_KEY)}>
          <legend>{t('settings-memory-keep')}</legend>
          {#each KEEP_CHOICES as value (value)}
            <label>
              <input type="radio" name="memory-keep" {value} checked={model.keep === value} onchange={() => void model.change(KEEP_KEY, value)} />
              {t(KEEP_MESSAGE[value])}
            </label>
          {/each}
          <p class="hint">{t('settings-memory-keep-hint')}</p>
        </fieldset>
      {/if}
      <fieldset class="switches">
        <legend>{t('settings-memory-sources')}</legend>
        {#each SOURCE_SWITCHES as row (row.key)}
          {#if model.has(row.key)}
            <label class="switch-line" data-key={row.key}>
              <input type="checkbox" role="switch" checked={model.isOn(row.key)} disabled={model.locked(row.key)} aria-describedby={`${row.id}-hint`} onchange={(e) => void model.change(row.key, e.currentTarget.checked)} />
              {t(row.id)}
            </label>
            <p class="hint" id={`${row.id}-hint`}>{t(row.hint)}</p>
          {/if}
        {/each}
      </fieldset>
      {#if model.has(REMOVAL_KEY)}
        <label class="select-line">
          {t('settings-memory-removal')}
          <select aria-describedby="settings-memory-removal-hint" disabled={model.locked(REMOVAL_KEY)} value={model.removal === null ? '' : String(model.removal)} onchange={(e) => { const v = e.currentTarget.value; void model.change(REMOVAL_KEY, v === '' ? null : Number(v)); }}>
            {#each REMOVAL_CHOICES as days (days)}
              <option value={days === null ? '' : String(days)}>{days === null ? t('settings-memory-removal-faded') : t('settings-memory-removal-days', { days })}</option>
            {/each}
          </select>
        </label>
        <p class="hint" id="settings-memory-removal-hint">{t('settings-memory-removal-hint')}</p>
      {/if}
    </div>
  {/if}

  <div class="group" role="group" aria-labelledby="settings-memory-shown">
    <h4 id="settings-memory-shown">{t('settings-memory-group-shown')}</h4>
    <label class="select-line">
      {t('settings-memory-group-by')}
      <select aria-describedby="settings-memory-group-by-hint" value={model.display.groupBy} onchange={(e) => model.setGroupBy(e.currentTarget.value)}>
        {#each GROUP_BY as by (by)}<option value={by}>{t(GROUP_BY_MESSAGE[by])}</option>{/each}
      </select>
    </label>
    <p class="hint" id="settings-memory-group-by-hint">{t('settings-memory-group-by-hint')}</p>
    <label class="select-line">
      {t('settings-memory-page-size')}
      <select aria-describedby="settings-memory-page-size-hint" value={String(model.display.pageSize)} onchange={(e) => model.setPageSize(Number(e.currentTarget.value))}>
        {#each MEMORY_PAGE_SIZES as size (size)}<option value={String(size)}>{size}</option>{/each}
      </select>
    </label>
    <p class="hint" id="settings-memory-page-size-hint">{t('settings-memory-page-size-hint')}</p>
    <label class="switch-line">
      <input type="checkbox" role="switch" checked={model.display.detailsOpen} aria-describedby="settings-memory-details-hint" onchange={(e) => model.setDetailsOpen(e.currentTarget.checked)} />
      {t('settings-memory-details-open')}
    </label>
    <p class="hint" id="settings-memory-details-hint">{t('settings-memory-details-open-hint')}</p>
  </div>

  {#if model.loaded}
    <div class="group" role="group" aria-labelledby="settings-memory-uses">
      <h4 id="settings-memory-uses">{t('settings-memory-group-uses')}</h4>
      {#each USE_SWITCHES as row (row.key)}
        {#if model.has(row.key)}
          <label class="switch-line" data-key={row.key}>
            <input type="checkbox" role="switch" checked={model.isOn(row.key)} disabled={model.locked(row.key)} aria-describedby={`${row.id}-hint`} onchange={(e) => void model.change(row.key, e.currentTarget.checked)} />
            {t(row.id)}
          </label>
          <p class="hint" id={`${row.id}-hint`}>{t(row.hint)}</p>
        {/if}
      {/each}
    </div>
  {/if}
  <ErrorLine error={model.error} onretry={() => (model.error?.code === 'DSK_PREFERENCES_WRITE_FAILED' ? model.retry() : void model.load())} />
</section>

<style>
  /* One card per group, as the other Settings sections draw their parts; the group's name is its only
     heading, so the first question inside it reads as a question, not as a second heading. */
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
  .keep > legend {
    font-weight: 400;
    margin-block-end: var(--space-04);
  }
  fieldset {
    border: 0;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  legend {
    font-size: 13px;
    padding: 0;
  }
  .select-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm);
    font-size: 13px;
  }
  .hint {
    margin: 0 0 var(--space-04);
    font-size: 12px;
    color: var(--color-text-muted);
  }
</style>
