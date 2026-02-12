<script lang="ts">
  // The question to move ordin to the person's Applications folder (ScreensModule.md SM-FS-14): drawn
  // alone, before anything else of first start, while ordin runs from outside an Applications folder.
  // One main action, **Move ordin**; **Not now** carries on.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-FS-14
  import { i18n, t } from '../../i18n/fluent';
  import ErrorLine from '../ErrorLine.svelte';
  import type { FirstStartModel } from './first-start-model.svelte';

  let { model }: { model: FirstStartModel } = $props();
</script>

<div class="move" lang={i18n.locale}>
  <section class="card" aria-labelledby="move-heading">
    <div class="brand">
      <span class="logo" aria-hidden="true"></span>
      <span class="word">{i18n.product}</span>
    </div>
    <h1 id="move-heading">{t('setup-move-title')}</h1>
    <p>{t('setup-move-line')}</p>
    <div class="actions">
      <button type="button" class="primary" disabled={model.moving} onclick={() => void model.moveToApplications()}>{t('setup-move-do')}</button>
      <button type="button" disabled={model.moving} onclick={() => void model.notNow()}>{t('setup-move-later')}</button>
    </div>
    {#if model.moving}<p class="muted" role="status">{t('setup-move-moving')}</p>{/if}
    <ErrorLine error={model.moveError} onretry={() => void model.moveToApplications()} />
  </section>
</div>

<style>
  /* As first start's card (FirstStart.svelte; cross-cutting/VisualDesign.md V6 *Brand*). */
  .move {
    min-block-size: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 100px var(--space-md) var(--space-md);
    background: var(--color-background);
  }
  .card {
    inline-size: 100%;
    max-inline-size: 440px;
    display: flex;
    flex-direction: column;
    gap: var(--space-14);
    padding: 28px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-card);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-10);
    font-weight: 700;
    font-size: 18px;
    color: var(--color-text);
  }
  .logo {
    inline-size: 28px;
    block-size: 28px;
    flex: none;
    border-radius: var(--radius-md);
    background: var(--color-primary);
  }
  h1 {
    font-size: 22px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
</style>
