<script lang="ts">
  // Settings › Language (SM-LG-2): the picker as a radio group with the language in use checked, Edit
  // wording, Export… (with a line saying what to choose), Import…, Make a new language…, and Your languages
  // with one state line each, or a line when there are none (amended January 2026 (user decision)). Bound to
  // the backend it is given, so a personal server's languages (SM-LG-7) are this same view.
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

<section class="language settings-section" aria-labelledby="settings-language-heading">
  <h3 id="settings-language-heading">{t('settings-section-language')}</h3>
  <ErrorLine error={model.unavailable} />
  {#if model.unavailable === null}
    <fieldset class="choice-group picker">
      <legend id="language-picker-label">{t('language-picker-label')}</legend>
      <label>
        <input type="radio" name="language-choice" value="" checked={model.following} onchange={() => void model.choose(null)} />
        {t('language-follow-computer')}
      </label>
      {#each model.choices as choice (choice.locale)}
        <label>
          <input type="radio" name="language-choice" value={choice.locale} checked={!model.following && model.active === choice.locale} onchange={() => void model.choose(choice.locale)} />
          {languageLabel(choice)}{#if kindLabel(choice.kind) !== null} <span class="secondary">· {kindLabel(choice.kind)}</span>{/if}
        </label>
      {/each}
    </fieldset>

    <div class="row-actions">
      <button type="button" onclick={() => openLanguage(model.active)}>{t('language-edit')}</button>
      <button type="button" onclick={() => (model.exporting = !model.exporting)}>{t('language-export')}</button>
      <button type="button" onclick={() => fileInput?.click()}>{t('language-import')}</button>
      <input bind:this={fileInput} type="file" accept=".ftl,text/plain" class="visually-hidden" tabindex="-1" aria-hidden="true" onchange={(e) => void picked(e)} />
      <button type="button" onclick={() => model.openMake()}>{t('language-make')}</button>
    </div>

    {#if model.exporting}
      <div class="export" role="group" aria-labelledby="language-export-line">
        <p id="language-export-line" class="intro">{t('language-export-pick')}</p>
        <div class="row-actions">
          {#each model.exportable as option (option.locale)}
            <button type="button" class="small" onclick={() => void model.exportLanguage(option.locale)}>{languageLabel(option)}</button>
          {/each}
        </div>
      </div>
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
    {#if model.yours.length === 0}
      <p class="intro">{t('language-yours-none')}</p>
    {/if}
    <ul class="yours list-rows" aria-live="polite">
      {#each model.yours as summary (summary.locale)}
        {@const line = stateLine(summary)}
        <li class="list-row">
          <span class="grow">
            <b class="name">{languageLabel(summary)}</b>
            <small class="state">{t(line.messageId, line.args)}</small>
          </span>
          <span class="row-actions">
            {#if line.action === 'stop'}
              <button type="button" class="small" onclick={() => void model.stopDrafting(summary.locale)}>{t('language-stop-drafting')}</button>
            {:else if line.action === 'ask_ordin'}
              <button type="button" class="small" onclick={() => model.openMake({ locale: summary.locale, name: summary.native_name ?? summary.locale })}>{t('language-ask-ordin-draft')}</button>
            {/if}
            <button type="button" class="small" onclick={() => openLanguage(summary.locale)}>{t('language-open')}</button>
          </span>
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

<style>
  .language {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
</style>
