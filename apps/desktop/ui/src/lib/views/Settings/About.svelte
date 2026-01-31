<script lang="ts">
  // Settings › About (SM-UP-2, SM-UP-8): the product name and version, Check for updates, one status line
  // in a polite live region, and What's new. Local backend only.
  import { onMount } from 'svelte';

  import { i18n, t } from '../../i18n/fluent';
  import { getApp } from '../../shell/context';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import Notes from './Notes.svelte';
  import { AboutModel, updateStoreFor } from './updates.svelte';

  interface Props {
    backend: BackendState;
    model?: AboutModel;
  }
  let { backend, model: given }: Props = $props();
  // The app context is read while the screen is set up; Svelte's getContext answers nothing later, when the version is asked for.
  // svelte-ignore state_referenced_locally
  const transport = given === undefined ? getApp().transport : null;
  // svelte-ignore state_referenced_locally
  const model = given ?? new AboutModel(backend.handle, () => transport!.appVersion(), updateStoreFor(backend.handle));
  const status = $derived(model.status);

  onMount(() => {
    void model.open();
    return () => model.close();
  });
</script>

<section class="about" aria-labelledby="settings-about-heading">
  <h3 id="settings-about-heading">{t('settings-section-about')}</h3>
  <p class="product">{i18n.product}</p>
  <p class="version">{t('about-version', { version: model.version })}</p>
  <p>
    <button type="button" disabled={model.busy} onclick={() => void model.check()}>{t('about-check')}</button>
  </p>
  <p class="status" role="status" aria-live="polite">
    {#if status.kind === 'checking'}{t('about-checking')}
    {:else if status.kind === 'up_to_date'}{t('about-up-to-date')}
    {:else if status.kind === 'downloading'}{t('about-downloading', { version: status.version })}
    {:else if status.kind === 'ready'}{t('about-ready', { version: status.version })}
    {:else if status.kind === 'administrator_needed'}{t('update-ask-administrator', { version: status.version })}
    {:else if status.kind === 'could_not_check'}{t('about-could-not-check')}{/if}
    {#if status.kind === 'ready'}
      <button type="button" onclick={() => void model.restart()}>{t('update-restart')}</button>
    {/if}
  </p>
  <p>
    <button type="button" onclick={() => void model.openWhatsNew()}>{t('about-whats-new')}</button>
  </p>
  <ErrorLine error={model.error} onretry={() => void model.check()} />

  {#if model.panel !== null}
    <aside class="side-panel" aria-labelledby="about-notes-heading">
      {#if 'none' in model.panel}
        <h4 id="about-notes-heading">{t('about-whats-new')}</h4>
        <p>{t('about-whats-new-none')}</p>
      {:else}
        <h4 id="about-notes-heading">{t('whats-new-title', { version: model.panel.version })}</h4>
        <Notes notes={model.panel.notes} />
      {/if}
      <button type="button" onclick={() => model.closePanel()}>{t('card-close')}</button>
    </aside>
  {/if}
</section>

<style>
  .side-panel {
    margin-block-start: var(--space-12);
    padding: var(--space-12);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
  }
</style>
