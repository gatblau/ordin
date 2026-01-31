<script lang="ts">
  // Settings › Language (SM-LG-2): the picker, Edit wording, Export…, Import…, Make a new language…, and
  // Your languages with one state line each. Bound to the backend it is given, so a personal server's
  // languages (SM-LG-7) are this same view.
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import { getApp } from '../../shell/context';
  import { openScreen } from '../../shell/open-item';
  import { makeKey } from '../../shell/screens';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { languageLabel, LanguageSettingsModel, stateLine } from './language-model.svelte';
  import MakeLanguageDialog from './MakeLanguageDialog.svelte';

  interface Props {
    backend: BackendState;
    model?: LanguageSettingsModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const app = given === undefined ? getApp() : null;
  // svelte-ignore state_referenced_locally
  const model = given ?? new LanguageSettingsModel(backend.handle, app?.local.handle ?? backend.handle);
  let fileInput = $state<HTMLInputElement | null>(null);
  const kindLabel = (kind: string | null): string | null => (kind === 'made' ? t('language-made-by-you') : kind === 'imported' ? t('language-imported') : null);

  function openLanguage(locale: string, only?: 'needs_review'): void {
    if (app === null) return;
    const instance = `${backend.id === app.local.id ? '' : `${backend.id}@`}${locale}${only === undefined ? '' : `?only=${only}`}`;
    openScreen(app, app.local.id, null, makeKey('language', instance), {
      title: t('language-screen-title', { language: languageLabel(model.summaryOf(locale) ?? { locale }) }),
    });
  }

  async function picked(event: Event): Promise<void> {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (file === undefined) return;
    const report = await model.importFile(file);
    if (report !== null) {
      app?.shell.announce(t('language-import-report', { stored: String(report.stored), unknown: String(report.dropped_unknown), english: String(report.in_english) }));
      openLanguage(report.locale, report.summary.needs_review > 0 ? 'needs_review' : undefined);
    }
  }

  onMount(() => {
    void model.load();
    return backend.subscribe((env) => {
      if (env.kind === 'locale_changed' || env.kind === 'language_changed') void model.load();
    });
  });
</script>

<section class="language" aria-labelledby="settings-language-heading">
  <h3 id="settings-language-heading">{t('settings-section-language')}</h3>
  <ErrorLine error={model.unavailable} />
  {#if model.unavailable === null}
    <div role="group" aria-labelledby="language-picker-label">
      <p id="language-picker-label">{t('language-picker-label')}</p>
      <ul role="listbox" aria-labelledby="language-picker-label">
        <li role="option" aria-selected={model.following}>
          <button type="button" onclick={() => void model.choose(null)}>{t('language-follow-computer')}</button>
        </li>
        {#each model.choices as choice (choice.locale)}
          <li role="option" aria-selected={!model.following && model.active === choice.locale}>
            <button type="button" onclick={() => void model.choose(choice.locale)}>
              {languageLabel(choice)}{#if kindLabel(choice.kind) !== null} · {kindLabel(choice.kind)}{/if}
            </button>
          </li>
        {/each}
      </ul>
    </div>

    <div class="actions">
      <button type="button" onclick={() => openLanguage(model.active)}>{t('language-edit')}</button>
      <button type="button" onclick={() => (model.exporting = !model.exporting)}>{t('language-export')}</button>
      <button type="button" onclick={() => fileInput?.click()}>{t('language-import')}</button>
      <input bind:this={fileInput} type="file" accept=".ftl,text/plain" class="visually-hidden" tabindex="-1" aria-hidden="true" onchange={(e) => void picked(e)} />
      <button type="button" onclick={() => model.openMake()}>{t('language-make')}</button>
    </div>

    {#if model.exporting}
      <ul class="export" aria-label={t('language-export')}>
        {#each model.exportable as option (option.locale)}
          <li><button type="button" onclick={() => void model.exportLanguage(option.locale)}>{languageLabel(option)}</button></li>
        {/each}
      </ul>
    {/if}
    {#if model.fileTooLarge}<p class="invalid" role="alert">{t('error-loc-file-too-large')}</p>{/if}
    {#if model.report !== null}
      <p role="status" aria-live="polite">
        {t('language-import-report', { stored: String(model.report.stored), unknown: String(model.report.dropped_unknown), english: String(model.report.in_english) })}
        {#if model.report.left_out > 0}
          {t('language-import-left-out', { count: model.report.left_out })}
          <button type="button" aria-expanded={model.showLeftOut} onclick={() => (model.showLeftOut = !model.showLeftOut)}>{t('language-show-which')}</button>
        {/if}
      </p>
      {#if model.showLeftOut}<ul>{#each model.report.left_out_ids as id (id)}<li>{id}</li>{/each}</ul>{/if}
    {/if}
    <ErrorLine error={model.error} onretry={() => void model.load()} />

    <h4>{t('language-yours')}</h4>
    <ul class="yours" aria-live="polite">
      {#each model.yours as summary (summary.locale)}
        {@const line = stateLine(summary)}
        <li>
          <span class="name">{languageLabel(summary)}</span>
          <span class="state">{t(line.messageId, line.args)}</span>
          {#if line.action === 'stop'}
            <button type="button" onclick={() => void model.stopDrafting(summary.locale)}>{t('language-stop-drafting')}</button>
          {:else if line.action === 'ask_ordin'}
            <button type="button" onclick={() => model.openMake({ locale: summary.locale, name: summary.native_name ?? summary.locale })}>{t('language-ask-ordin-draft')}</button>
          {/if}
          <button type="button" onclick={() => openLanguage(summary.locale)}>{t('language-open')}</button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if model.make !== null}
  <MakeLanguageDialog make={model.make} backend={backend.handle} onclose={() => (model.make = null)} ondrafted={() => void model.load()} />
{/if}
{#if model.pendingReplace !== null}
  <ConfirmDialog
    title={t('confirm-replace-language-title', { language: model.pendingReplace.language })}
    body={t('confirm-replace-language-body')}
    verb={t('language-replace-verb')}
    initialFocus="cancel"
    onconfirm={async () => {
      const report = await model.confirmReplace();
      if (report !== null) openLanguage(report.locale, report.summary.needs_review > 0 ? 'needs_review' : undefined);
    }}
    oncancel={() => model.cancelReplace()}
  />
{/if}
