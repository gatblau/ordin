<script lang="ts">
  // Project settings: Code checking (SM-SF-3.3) above Check commands (SA-CLOSE-04).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-3
  // spec: ordin-desktop/components/ScreensModule#SA-CLOSE-04
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import { ProjectSettingsController, answerLine } from './project-settings.svelte';

  let { backend, instance }: ScreenProps = $props();

  const c = new ProjectSettingsController(backend.handle, instance ?? '');

  onMount(() => {
    void c.loadProject();
    void c.loadAnswer();
  });
</script>

<section class="project-settings" aria-labelledby="project-settings-heading">
  <h2 id="project-settings-heading" tabindex="-1" data-region-focus>{c.displayName === '' ? t('engineering-settings-title') : c.displayName}</h2>
  <ErrorLine error={c.projectError} />

  <section aria-labelledby="code-checking-heading">
    <h3 id="code-checking-heading">{t('code-checking-label')}</h3>
    {#if c.answer !== undefined}
      {@const line = answerLine(c.answer)}
      <p>{t(line.message)}</p>
      <button type="button" disabled={c.answering} onclick={() => void c.setAnswer(c.answer !== true)}>{t(line.button)}</button>
    {/if}
    <ErrorLine error={c.answerError} />
  </section>

  <section aria-labelledby="project-checks-heading">
    <h3 id="project-checks-heading">{t('project-checks-title')}</h3>
    {#if c.commands.length === 0}
      <p>{t('project-checks-none')}</p>
    {/if}
    {#each c.commands as command, i (i)}
      <div class="check-row">
        <input type="text" aria-label={t('project-checks-title')} maxlength="1000" value={command} oninput={(e) => c.edit(i, e.currentTarget.value)} />
        <button type="button" onclick={() => c.remove(i)}>{t('engineering-check-remove')}</button>
      </div>
    {/each}
    <button type="button" disabled={!c.canAdd} onclick={() => c.add()}>{t('project-checks-add')}</button>
    <button type="button" disabled={c.saving} onclick={() => void c.save()}>{t('engineering-save')}</button>
    <ErrorLine error={c.error} onretry={() => void c.save()} />
  </section>
  <div class="visually-hidden" aria-live="polite" role="status">{c.announcer.text}</div>
</section>

<style>
  .check-row {
    display: flex;
    gap: var(--space-sm);
    margin-block-end: var(--space-04);
  }
  .check-row input {
    flex: 1;
  }
</style>
