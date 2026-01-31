<script lang="ts">
  // Settings › Agent repositories: the intro, one row per repository (its name, then where it is, its
  // format only in a workspace that links code, and its number of agents), **Show its parts** (opens in the
  // open workspace, never in Settings; a line says why it is missing with no workspace open), **Rename…** (a
  // dialog), **Remove…** and **Add an agent repository…**, with a line when there is none and one when one
  // is added (amended January 2026 (user decision)).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { focusableIn, trapIndex } from '../../shell/focus';
  import { readLinksCode } from '../fixed/links-code';

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

  let codeLinked = $state(false);
  let renameBox = $state<HTMLElement | null>(null);
  let addBox = $state<HTMLElement | null>(null);
  const anyCanPublish = $derived(model.items.some((r) => r.repository.person_can_publish));

  onMount(() => {
    void model.load();
    void readLinksCode(backend.handle, front).then((linked) => (codeLinked = linked));
  });

  /** "On GitHub" from the repository's host. */
  function hostWords(host: string | undefined): string {
    if (host === undefined || host === '') return '';
    const name = host.toLowerCase() === 'github.com' ? 'GitHub' : host;
    return `${t('settings-repos-on', { host: name })} · `;
  }

  /** A dialog's keys: Escape closes it, Tab stays inside it. */
  function dialogKeys(event: KeyboardEvent, root: HTMLElement | null, close: () => void): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      close();
    } else if (event.key === 'Tab' && root !== null) {
      const items = focusableIn(root);
      const next = trapIndex(items.indexOf(document.activeElement as HTMLElement), items.length, event.shiftKey);
      if (next >= 0) {
        event.preventDefault();
        items[next]?.focus();
      }
    }
  }

  /** Focus goes to the dialog's first field when it opens. */
  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('input')?.focus();
  }

  const formatId = (format: string): string => (format === 'apm' ? 'settings-repos-format-apm' : 'settings-repos-format-ordin');

  const addedLine = (name: string, count: number): string => t('settings-repos-added', { name, count });

  function showParts(id: string): void {
    if (app === null || front === null) return;
    openScreen(app, backend.id, front, makeKey('repository-parts', id));
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (model.renaming !== null) dialogKeys(e, renameBox, () => (model.renaming = null));
    else if (model.adding) dialogKeys(e, addBox, () => model.closeAdd());
  }}
/>

<section class="agent-repositories settings-section" aria-labelledby="settings-repos-heading">
  <h3 id="settings-repos-heading">{t('settings-section-agent-repositories')}</h3>
  <p class="intro">{t('settings-repos-intro')}</p>
  {#if model.items.length === 0}
    <p>{t('settings-repos-none')}</p>
  {:else}
    <ul class="list-rows">
      {#each model.items as row (row.repository.id)}
        <li class="list-row" data-repository={row.repository.id}>
          <span class="grow">
            <b class="name">{row.repository.display_name}</b>
            <small>
              {hostWords(row.repository.host)}{#if codeLinked}<span class="format">{t(formatId(row.repository.format))}</span> · {/if}<span class="count-line">{t('settings-repos-agent-count', { count: row.agent_count })}</span>{#if row.set_by_company} · <span class="tag">{t('settings-repos-locked')}</span>{/if}
            </small>
          </span>
          <span class="row-actions">
            {#if row.repository.person_can_publish && front !== null}
              <button type="button" class="small" onclick={() => showParts(row.repository.id)}>{t('settings-repos-show-parts')}</button>
            {/if}
            {#if !row.set_by_company}
              <button type="button" class="small" onclick={() => model.startRename(row)}>{t('settings-repos-rename')}</button>
              <button type="button" class="small" onclick={() => (model.removing = row)}>{t('settings-repos-remove')}</button>
            {/if}
          </span>
        </li>
      {/each}
    </ul>
    {#if anyCanPublish && front === null}<p class="hint">{t('settings-repos-show-parts-needs-workspace')}</p>{/if}
  {/if}
  <p><button type="button" onclick={() => model.openAdd()}>{t('settings-repos-add')}</button></p>
  <p class="done-line" role="status" aria-live="polite">{model.done}</p>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>

{#if model.renaming !== null}
  {@const renaming = model.renaming}
  <div class="dialog-backdrop">
    <div bind:this={renameBox} class="dialog" role="dialog" aria-modal="true" aria-labelledby="settings-repos-rename-title" use:focusFirst>
      <form
        class="field-stack"
        onsubmit={(e) => {
          e.preventDefault();
          void model.rename();
        }}
      >
        <h2 id="settings-repos-rename-title" tabindex="-1">{t('settings-repos-rename-title', { name: renaming.repository.display_name })}</h2>
        <label>
          {t('settings-repos-rename-field')}
          <input type="text" bind:value={model.renameText} maxlength={NAME_MAX} autocomplete="off" />
        </label>
        <div class="dialog-buttons">
          <button type="submit" class="go" disabled={!validName(model.renameText)}>{t('settings-repos-rename-verb')}</button>
          <button type="button" data-default="true" onclick={() => (model.renaming = null)}>{t('confirm-cancel')}</button>
        </div>
      </form>
    </div>
  </div>
{/if}

{#if model.adding}
  <div class="dialog-backdrop">
    <div bind:this={addBox} class="dialog" role="dialog" aria-modal="true" aria-labelledby="settings-repos-add-title" use:focusFirst>
      <form
        class="add-dialog field-stack"
        onsubmit={(e) => {
          e.preventDefault();
          void (model.probe === null ? model.lookUp() : model.add(addedLine));
        }}
      >
        <h2 id="settings-repos-add-title" tabindex="-1">{t('settings-repos-add')}</h2>
        <label>
          {t('settings-repos-add-address')}
          <input type="text" bind:value={model.address} autocomplete="off" aria-describedby="settings-repos-add-hint" />
        </label>
        <p id="settings-repos-add-hint" class="hint">{t('settings-repos-add-address-hint')}</p>
        <button type="button" class:primary={model.probe === null} disabled={!model.addressValid} onclick={() => void model.lookUp()}>{t('settings-repos-add-look')}</button>
        {#if model.probe !== null}
          <p>{t('settings-repos-add-found', { count: model.probe.agent_count, format: t(formatId(model.probe.format)) })}</p>
          <label>
            {t('settings-repos-add-name')}
            <input type="text" bind:value={model.name} maxlength={NAME_MAX} autocomplete="off" />
          </label>
          {#if !model.probe.person_can_publish}<p>{t('settings-repos-add-cannot-publish')}</p>{/if}
        {/if}
        <ErrorLine error={model.dialogError} />
        <div class="dialog-buttons">
          {#if model.probe !== null}
            <button type="submit" data-default="true" disabled={!validName(model.name)}>{t('settings-repos-add-verb')}</button>
          {/if}
          <button type="button" onclick={() => model.closeAdd()}>{t('confirm-cancel')}</button>
        </div>
      </form>
    </div>
  </div>
{/if}

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
  /* The add dialog (scene `repoAdd`): small labels over full-width fields, one column, the next action as
     the primary button. */
  .add-dialog p {
    margin: 0;
    font-size: 14px;
  }
</style>
