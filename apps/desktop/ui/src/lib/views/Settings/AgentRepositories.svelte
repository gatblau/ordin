<script lang="ts">
  // Settings › Agent repositories: one row per repository, **Show its parts** (opens in the open
  // workspace, never in Settings), **Rename…**, **Remove…** and **Add an agent repository…**.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import { openScreen } from '../../shell/open-item';
  import { makeKey } from '../../shell/screens';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from '../fixed/util';
  import { NAME_MAX, ReposModel, validName } from './repos-model.svelte';

  interface Props {
    backend: BackendState;
    model?: ReposModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new ReposModel(backend.handle);
  const app = appOrNull();
  const front = $derived(app?.shell.openWorkspace ?? null);

  onMount(() => void model.load());

  const formatId = (format: string): string => (format === 'apm' ? 'settings-repos-format-apm' : 'settings-repos-format-ordin');

  function showParts(id: string): void {
    if (app === null || front === null) return;
    openScreen(app, backend.id, front, makeKey('repository-parts', id));
  }
</script>

<section class="agent-repositories" aria-labelledby="settings-repos-heading">
  <h3 id="settings-repos-heading">{t('settings-section-agent-repositories')}</h3>
  <ul>
    {#each model.items as row (row.repository.id)}
      <li data-repository={row.repository.id}>
        <span class="name">{row.repository.display_name}</span>
        <span class="format">{t(formatId(row.repository.format))}</span>
        <span class="count-line">{t('settings-repos-agent-count', { count: row.agent_count })}</span>
        {#if row.set_by_company}<span class="tag">{t('settings-repos-locked')}</span>{/if}
        {#if row.repository.person_can_publish && front !== null}
          <button type="button" onclick={() => showParts(row.repository.id)}>{t('settings-repos-show-parts')}</button>
        {/if}
        {#if !row.set_by_company}
          <button type="button" onclick={() => model.startRename(row)}>{t('settings-repos-rename')}</button>
          <button type="button" onclick={() => (model.removing = row)}>{t('settings-repos-remove')}</button>
        {/if}
      </li>
    {/each}
  </ul>
  <p><button type="button" onclick={() => model.openAdd()}>{t('settings-repos-add')}</button></p>
  <ErrorLine error={model.error} onretry={() => void model.load()} />

  {#if model.renaming !== null}
    <form
      class="rename-dialog"
      role="group"
      aria-labelledby="settings-repos-rename-label"
      onsubmit={(e) => {
        e.preventDefault();
        void model.rename();
      }}
    >
      <label id="settings-repos-rename-label">
        {t('settings-repos-rename-field')}
        <input type="text" bind:value={model.renameText} maxlength={NAME_MAX} autocomplete="off" />
      </label>
      <button type="submit" data-default="true" disabled={!validName(model.renameText)}>{t('settings-repos-rename-verb')}</button>
      <button type="button" onclick={() => (model.renaming = null)}>{t('confirm-cancel')}</button>
    </form>
  {/if}

  {#if model.adding}
    <form
      class="add-dialog"
      role="group"
      aria-labelledby="settings-repos-add-title"
      onsubmit={(e) => {
        e.preventDefault();
        void (model.probe === null ? model.lookUp() : model.add());
      }}
    >
      <h4 id="settings-repos-add-title" tabindex="-1">{t('settings-repos-add')}</h4>
      <label>
        {t('settings-repos-add-address')}
        <input type="text" bind:value={model.address} autocomplete="off" />
      </label>
      <button type="button" class:primary={model.probe === null} disabled={!model.addressValid} onclick={() => void model.lookUp()}>{t('settings-repos-add-look')}</button>
      {#if model.probe !== null}
        <p>{t('settings-repos-add-found', { count: model.probe.agent_count, format: t(formatId(model.probe.format)) })}</p>
        <label>
          {t('settings-repos-add-name')}
          <input type="text" bind:value={model.name} maxlength={NAME_MAX} autocomplete="off" />
        </label>
        {#if !model.probe.person_can_publish}<p>{t('settings-repos-add-cannot-publish')}</p>{/if}
        <button type="submit" data-default="true" disabled={!validName(model.name)}>{t('settings-repos-add-verb')}</button>
      {/if}
      <ErrorLine error={model.dialogError} />
      <button type="button" onclick={() => model.closeAdd()}>{t('confirm-cancel')}</button>
    </form>
  {/if}
</section>

{#if model.removing !== null}
  <ConfirmDialog
    title={t('settings-repos-remove-title', { name: model.removing.repository.display_name })}
    body={t('settings-repos-remove-body')}
    verb={t('settings-repos-remove-verb')}
    initialFocus="cancel"
    onconfirm={() => void model.remove()}
    oncancel={() => (model.removing = null)}
  />
{/if}

<style>
  /* The prototype's add form (scene `repoAdd`): small labels over full-width fields, one column, the
     next action as the primary button. */
  .add-dialog {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-sm);
  }
  .add-dialog h4 {
    margin: 0;
    font-size: 15px;
  }
  .add-dialog label {
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
    font-size: 13px;
  }
  .add-dialog label input {
    font-size: 14px;
  }
  .add-dialog p {
    margin: 0;
    font-size: 14px;
  }
</style>
