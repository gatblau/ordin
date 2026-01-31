<script lang="ts">
  // The Language screen (SM-LG-3), one tab per language: header with the state line, Use this language,
  // Restore all and Remove this language; Find; one row per message with Edit, Restore and Mark as checked.
  // Text is drawn as text; the language's text and edit box carry its own `dir` and `lang`.
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import { getApp } from '../../shell/context';
  import { ScreenAreaState } from '../../shell/screen-area.svelte';
  import type { ScreenProps } from '../../shell/screens';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { languageLabel, LanguageScreenModel, stateLine } from './language-model.svelte';
  import type { MessageOnly } from '../../service-api/types.gen';

  interface Props extends ScreenProps {
    model?: LanguageScreenModel;
  }
  let { backend, instance, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const app = given === undefined ? getApp() : null;

  /** `[<backend id>@]<locale>[?only=<filter>]`. */
  function parse(raw: string | null): { backend: string | null; locale: string; only: MessageOnly | null } {
    const [head = '', query = ''] = (raw ?? '').split('?');
    const at = head.indexOf('@');
    const only = new URLSearchParams(query).get('only');
    const filter = only === 'needs_review' || only === 'in_english' || only === 'edited' || only === 'needs_changing' ? only : null;
    return { backend: at < 0 ? null : head.slice(0, at), locale: at < 0 ? head : head.slice(at + 1), only: filter };
  }
  // svelte-ignore state_referenced_locally
  const parsed = parse(instance);
  // svelte-ignore state_referenced_locally
  const handle = parsed.backend === null ? backend.handle : (app?.stateFor(parsed.backend)?.handle ?? backend.handle);
  // svelte-ignore state_referenced_locally
  const model = given ?? new LanguageScreenModel(handle, parsed.locale, parsed.only);
  const summary = $derived(model.summary);
  const name = $derived(summary === null ? model.locale : languageLabel(summary));
  const line = $derived(summary === null ? null : stateLine(summary));
  let heading = $state<HTMLElement | null>(null);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let editButtons = $state<Record<string, HTMLElement | null>>({});

  const ONLY: Array<[MessageOnly, string]> = [
    ['needs_review', 'language-only-needs-checking'],
    ['in_english', 'language-only-in-english'],
    ['edited', 'language-only-edited'],
    ['needs_changing', 'language-only-needs-changing'],
  ];

  onMount(() => {
    heading?.focus();
    void model.load();
    const stop = backend.subscribe((env) => {
      if (env.kind === 'language_changed' && env.locale === model.locale) void model.load();
    });
    return () => {
      stop();
      clearTimeout(searchTimer);
    };
  });

  // The tab closes when the language is removed.
  $effect(() => {
    if (model.removed && app !== null) {
      app.screens.close(ScreenAreaState.scope(app.local.id, null), `language:${instance ?? model.locale}`);
    }
  });

  function onSearch(value: string): void {
    model.text = value;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => void model.refind(), 200);
  }

  async function save(): Promise<void> {
    const id = model.editing?.id;
    if (await model.save(model.editing?.text ?? '')) {
      if (id !== undefined) editButtons[id]?.focus();
    }
  }
  function cancelEdit(): void {
    const id = model.editing?.id;
    model.cancelEdit();
    if (id !== undefined) queueMicrotask(() => editButtons[id]?.focus());
  }
</script>

<section class="language-screen" aria-labelledby="language-screen-heading">
  <h2 id="language-screen-heading" bind:this={heading} tabindex="-1" data-region-focus>{t('language-screen-title', { language: name })}</h2>
  {#if line !== null}<p class="state" aria-live="polite">{t(line.messageId, line.args)}</p>{/if}
  <div class="header-actions">
    {#if summary !== null && summary.state === 'needs_review'}
      <button type="button" disabled={!model.canUse} aria-describedby={model.canUse ? undefined : 'language-use-hint'} onclick={() => void model.use()}>{t('language-use')}</button>
      {#if !model.canUse}<span id="language-use-hint">{t('language-use-check-first')}</span>{/if}
    {/if}
    {#if summary !== null && summary.kind === 'shipped' && summary.edited > 0}
      <button type="button" onclick={() => (model.confirm = 'restore_all')}>{t('language-restore-all')}</button>
    {/if}
    {#if summary !== null && summary.kind !== 'shipped'}
      <button type="button" class="danger" onclick={() => (model.confirm = 'remove')}>{t('language-remove')}</button>
    {/if}
  </div>

  <div class="find" role="search">
    <label>{t('language-search-label')} <input type="search" value={model.text} oninput={(e) => onSearch(e.currentTarget.value)} /></label>
    <label>
      {t('language-area-label')}
      <select value={model.area} onchange={(e) => { model.area = e.currentTarget.value; void model.refind(); }}>
        <option value="">{t('language-area-all')}</option>
        {#each model.areas as area (area)}<option value={area}>{area}</option>{/each}
      </select>
    </label>
    {#each ONLY as [key, messageId] (key)}
      <button type="button" aria-pressed={model.only === key} onclick={() => void model.toggleOnly(key)}>{t(messageId)}</button>
    {/each}
    {#if model.only === 'needs_review' && model.shownUnreviewedSafety.length > 0}
      <button type="button" onclick={() => void model.markChecked(model.shownUnreviewedSafety)}>{t('language-mark-all-checked')}</button>
    {/if}
    <p class="count-line" role="status" aria-live="polite">{t('language-results', { count: model.rows.length })}</p>
  </div>
  <ErrorLine error={model.error} />

  <ul class="rows">
    {#each model.rows as row (row.message_id)}
      <li role="group" aria-label={row.english}>
        <p class="english" dir="ltr" lang="en-GB">{row.english}</p>
        {#if model.editing !== null && model.editing.id === row.message_id}
          <label>
            {t('language-edit-label', { language: name })}
            <textarea
              rows="3"
              dir={summary?.direction ?? 'ltr'}
              lang={model.locale}
              value={model.editing.text}
              aria-describedby={model.editing.error !== null ? `edit-error-${row.message_id}` : undefined}
              oninput={(e) => model.editing && (model.editing = { ...model.editing, text: e.currentTarget.value })}
              onkeydown={(e) => e.key === 'Escape' && (e.stopPropagation(), cancelEdit())}
            ></textarea>
          </label>
          <div id={`edit-error-${row.message_id}`}><ErrorLine error={model.editing.error} /></div>
          <button type="button" data-default="true" onclick={() => void save()}>{t('language-save')}</button>
          <button type="button" onclick={cancelEdit}>{t('confirm-cancel')}</button>
        {:else}
          <p class="text" dir={summary?.direction ?? 'ltr'} lang={model.locale}>{row.text ?? t('language-shows-english')}</p>
          <span class="source">{t(`language-source-${row.source}`)}</span>
          {#if row.safety}<span class="tag">{t('language-approval-wording')}</span>{/if}
          <button type="button" bind:this={editButtons[row.message_id]} onclick={() => model.startEdit(row)}>{t('language-edit-message')}</button>
          {#if row.shipped !== undefined && row.shipped !== null && row.source !== 'shipped'}
            <button type="button" onclick={() => void model.restore(row)}>{t('language-restore')}</button>
          {/if}
          {#if row.safety && !row.reviewed}
            <button type="button" onclick={() => void model.markChecked([row.message_id])}>{t('language-mark-checked')}</button>
          {/if}
        {/if}
      </li>
    {/each}
  </ul>
  {#if model.next !== null}<button type="button" onclick={() => void model.showMore()}>{t('settings-show-more')}</button>{/if}
</section>

{#if model.confirm === 'restore_all' && summary !== null}
  <ConfirmDialog
    title={t('confirm-restore-all-title', { language: name })}
    body={t('confirm-restore-all-body', { count: summary.edited })}
    verb={t('language-restore-all-verb')}
    initialFocus="cancel"
    onconfirm={() => void model.restoreAll()}
    oncancel={() => (model.confirm = null)}
  />
{:else if model.confirm === 'remove'}
  <ConfirmDialog
    title={t('confirm-remove-language-title', { language: name })}
    body={t('confirm-remove-language-body')}
    verb={t('language-remove-verb')}
    initialFocus="cancel"
    onconfirm={() => void model.removeLanguage()}
    oncancel={() => (model.confirm = null)}
  />
{/if}
