<script lang="ts">
  // The import screen (SM-NS-10), in the order the spec gives: heading, what Ordin reads, what is brought
  // in, which agent repository, the folder, **Import**.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-10
  import { t } from '../../i18n/fluent';
  import type { PartKind } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import type { ImportController } from './import.svelte';
  import { OFFERED_PART_KINDS, PART_KIND_MESSAGE } from './part-kinds';

  interface Props {
    backend: BackendState;
    c: ImportController;
    onimport?: () => void;
  }
  let { backend, c, onimport }: Props = $props();
</script>

<section class="import" aria-labelledby="import-heading">
  <h2 id="import-heading" tabindex="-1" data-region-focus>{t('import-title')}</h2>
  <p class="quiet">{t('import-what-ordin-reads')}</p>
  <fieldset class="what">
    <legend>{t('import-what')}</legend>
    <label><input type="radio" name="import-what" checked={c.what === 'agent'} onchange={() => (c.what = 'agent')} /> {t('import-agent')}</label>
    <label><input type="radio" name="import-what" checked={c.what === 'part'} onchange={() => (c.what = 'part')} /> {t('import-part')}</label>
  </fieldset>
  {#if c.what === 'part'}
    <label class="field">
      {t('import-part-kind')}
      <select value={c.partKind ?? ''} onchange={(e) => (c.partKind = (e.currentTarget.value || null) as PartKind | null)}>
        <option value="" disabled>{''}</option>
        {#each OFFERED_PART_KINDS as kind (kind)}<option value={kind}>{t(PART_KIND_MESSAGE[kind])}</option>{/each}
      </select>
    </label>
  {/if}
  <label class="field">
    {t('import-which-repository')}
    <select value={c.repositoryId ?? ''} onchange={(e) => (c.repositoryId = e.currentTarget.value || null)}>
      <option value="" disabled>{''}</option>
      {#each c.choices as r (r.repository.id)}<option value={r.repository.id}>{r.repository.display_name}</option>{/each}
    </select>
  </label>
  <p>
    <button type="button" onclick={() => void c.chooseFolder()}>{t('import-choose-folder')}</button>
    {#if c.folderName !== null}<span>{c.folderName}</span>{/if}
  </p>
  <button type="button" class="primary import-verb" disabled={!c.ready || c.busy} onclick={() => onimport?.()}>{t('import-verb')}</button>
  <ErrorLine error={c.error} />
</section>

<style>
  /* The prototype's import screen (scene `importStart`, `R.imp`). */
  .import {
    gap: var(--space-sm);
  }
  .import > h2 {
    font-size: 19px;
  }
  .quiet {
    color: var(--color-text-muted);
  }
  .what {
    border: 0;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
  .what > legend {
    font-weight: 700;
    padding: 0;
    margin-block-end: var(--space-06);
  }
  .what label {
    display: flex;
    gap: var(--space-06);
    align-items: center;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    font-size: 13px;
    font-weight: 700;
    max-inline-size: 360px;
  }
  .field select {
    font-size: 16px;
    font-weight: 400;
  }
  .import-verb {
    align-self: flex-start;
  }
</style>
