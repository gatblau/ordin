<script lang="ts">
  // Documents. With a place (instance = the place id), the files of that place, a folder at a time (SM-DS-3).
  // With no place, opened from the workspace's Documents row: the overview of recently changed documents
  // across every place, with a search (SM-DS-4, SM-DS-5). Each row says where the document lives, when it
  // changed and by whom when known; its actions are in its ⋯ menu, the same as the tree's (SM-DS-6).
  // No place linked yet → `documents-no-places` (SM-DS-2).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-DS-2
  // spec: ordin-desktop/components/ScreensModule#SM-DS-3
  // spec: ordin-desktop/components/ScreensModule#SM-DS-4
  // spec: ordin-desktop/components/ScreensModule#SM-DS-6
  import { onMount } from 'svelte';

  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import { fileKindIcon, fileKindTint } from '../../icons/kinds';
  import type { ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import DocumentMenu from '../publish/DocumentMenu.svelte';
  import { DocumentsOverview, type DocumentRow } from './documents-overview.svelte';
  import { loadFolder } from './lists.svelte';
  import LoadedList from './LoadedList.svelte';
  import PublishingFolder from './PublishingFolder.svelte';
  import { appOrNull } from './util';
  import { openScreen } from '../../shell/open-item';
  import { makeKey, SCREEN_IDS } from '../../shell/screens';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  let folder = $state('');
  let query = $state('');
  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const overview = new DocumentsOverview(backend.handle, workspaceId ?? '');
  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));

  onMount(() => {
    if (instance === null) void overview.load();
  });
  let first = true;
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    if (instance === null) void (overview.searched === '' ? overview.load() : overview.search(overview.searched));
  });

  // A folder row opens the folder; a file row (the only one with a date) opens its document page (SM-NS-4).
  function open(row: { id: string; title: string; detail?: string | undefined }): void {
    if (row.detail === undefined) folder = row.id;
    else if (app !== null) openScreen(app, backend.handle.id, workspaceId, makeKey(SCREEN_IDS.document, `${instance ?? ''}:${row.id}`), { title: row.title });
  }

  function openDocument(row: DocumentRow): void {
    if (app !== null) openScreen(app, backend.handle.id, workspaceId, makeKey(SCREEN_IDS.document, `${row.placeId}:${row.path}`), { title: row.name });
  }

  const detail = (row: DocumentRow): string =>
    row.changedBy === null
      ? t('documents-row-changed', { place: row.place, date: formatDate(row.changedAt) })
      : t('documents-row-changed-by', { place: row.place, date: formatDate(row.changedAt), who: row.changedBy });
</script>

{#if instance === null}
  <section class="documents-overview" aria-labelledby="documents-overview-heading">
    <h2 id="documents-overview-heading" tabindex="-1" data-region-focus>{t('tree-documents')}</h2>
    {#if overview.loaded && overview.error === null && overview.places.length === 0}
      <p>{t('documents-no-places')}</p>
    {:else}
      <form class="search" role="search" onsubmit={(e) => { e.preventDefault(); void overview.search(query); }}>
        <input type="search" bind:value={query} aria-label={t('documents-search-label')} placeholder={t('documents-search-label')} oninput={() => { if (query.trim() === '' && overview.searched !== '') void overview.load(); }} />
      </form>
      <h3>{overview.searched === '' ? t('documents-recent-heading') : overview.searched}</h3>
      <ErrorLine error={overview.error} onretry={() => void overview.load()} />
      {#if overview.loaded && overview.rows.length === 0 && overview.error === null}
        <p class="secondary">{overview.searched === '' ? t('documents-recent-none') : t('documents-search-none')}</p>
      {/if}
      <ul class="rows">
        {#each overview.rows as row (`${row.placeId}:${row.path}`)}
          {@const icon = fileKindIcon(row.name) ?? 'page'}
          <li class="tree-line row">
            <button type="button" class="open" onclick={() => openDocument(row)}>
              <span class="icon-tile" data-tint={fileKindTint(icon)}><Icon name={icon} /></span>
              <span class="text"><span class="name">{row.name}</span><span class="detail">{detail(row)}</span></span>
            </button>
            {#if workspaceId !== null}
              <DocumentMenu {backend} {workspaceId} placeId={row.placeId} path={row.path} name={row.name} onopen={() => openDocument(row)} />
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{:else}
  <!-- A folder row changes `folder`; the list is drawn again for it, since it loads only when it opens. -->
  {#key folder}
    <LoadedList
      {backend}
      {workspaceId}
      heading={t('tree-documents')}
      load={() => loadFolder(backend.handle, instance, folder)}
      onopen={open}
    />
  {/key}
  <PublishingFolder {backend} workspaceId={workspaceId ?? ''} placeId={instance} {folder} />
{/if}

<style>
  .documents-overview {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .documents-overview h3 {
    margin: 0;
    font-size: 14px;
    color: var(--color-text-muted);
  }
  .search input {
    inline-size: 100%;
    max-inline-size: 480px;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    position: relative;
    border-radius: var(--radius-row);
  }
  .row:hover {
    background: var(--color-chip);
  }
  .open {
    flex: 1;
    min-inline-size: 0;
    display: flex;
    gap: var(--space-sm);
    align-items: center;
    border: 0;
    background: none;
    text-align: start;
    padding: var(--space-04) var(--space-sm);
    min-height: 0;
  }
  .text {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }
  .name,
  .detail {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .detail,
  .secondary {
    font-size: 12px;
    color: var(--color-text-muted);
  }
</style>
