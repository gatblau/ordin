<script lang="ts">
  // The *Getting memory ready* card (ScreensModule.md amendment 2026-01-28, SM-MD-1 to SM-MD-5): at the
  // top of the local backend's conversation region, above the messages. Not a dialog: it takes no
  // focus, and the conversation, the tree and the screens stay usable. Its words name memory, the size
  // and the host only (SM-MD-5).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MD-1
  // spec: ordin-desktop/components/ScreensModule#SM-MD-2
  // spec: ordin-desktop/components/ScreensModule#SM-MD-3
  // spec: ordin-desktop/components/ScreensModule#SM-MD-4
  // spec: ordin-desktop/components/ScreensModule#SM-MD-5
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { failureMessageId, MemoryModelsModel, percent, receivedMb, sizeMb } from './memory-models-model.svelte';

  let { model }: { model: MemoryModelsModel } = $props();
  const shows = $derived(model.shows);
  const view = $derived(model.view);
</script>

{#if shows !== null && view !== null}
  <section class="memory-models" aria-labelledby="memory-models-heading">
    <div class="heading" aria-live="polite">
      <Icon name="memory" />
      <h2 id="memory-models-heading">{t('memory-models-title')}</h2>
    </div>
    {#if shows === 'ask'}
      <p>{t('memory-models-ask', { size: sizeMb(view.size_bytes), host: view.host })}</p>
      <div class="actions">
        <button type="button" class="primary" onclick={() => void model.download()}>{t(view.received_bytes > 0 ? 'memory-models-resume' : 'memory-models-download')}</button>
        <button type="button" onclick={() => void model.notNow()}>{t('memory-models-not-now')}</button>
      </div>
    {:else if shows === 'downloading'}
      <p>{t('memory-models-progress', { received: receivedMb(view.received_bytes), size: sizeMb(view.size_bytes) })}</p>
      <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={percent(view)} aria-labelledby="memory-models-heading">
        <div class="fill" style:inline-size={`${percent(view)}%`}></div>
      </div>
      <div class="actions">
        <button type="button" class="secondary" onclick={() => void model.stop()}><Icon name="stop" />{t('memory-models-stop')}</button>
      </div>
    {:else if shows === 'checking'}
      <p>{t('memory-models-checking')}</p>
    {:else if shows === 'ready'}
      <p>{t('memory-models-ready')}</p>
    {:else if shows === 'failed'}
      <p>{t(failureMessageId(view.failure ?? ''))}</p>
      <div class="actions">
        <button type="button" class="primary" onclick={() => void model.download()}>{t('memory-models-try-again')}</button>
        <button type="button" onclick={() => void model.notNow()}>{t('memory-models-not-now')}</button>
      </div>
    {/if}
    <ErrorLine error={model.error} />
  </section>
{/if}

<style>
  /* A card of the conversation region (cross-cutting/VisualDesign.md V6 "Card"; icons per V8). */
  .memory-models {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    margin: var(--space-sm) var(--space-md) 0;
    padding: var(--space-sm);
    border: 1px solid var(--color-line);
    border-radius: 8px;
    background: var(--color-surface);
  }
  .heading {
    display: flex;
    align-items: center;
    gap: var(--space-06);
  }
  .heading h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .memory-models p {
    margin: 0;
  }
  .actions {
    display: flex;
    gap: var(--space-06);
    flex-wrap: wrap;
  }
  .actions button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-04);
  }
  .bar {
    block-size: 6px;
    border-radius: 3px;
    background: var(--color-chip);
    overflow: hidden;
  }
  .fill {
    block-size: 100%;
    background: var(--color-accent);
  }
</style>
