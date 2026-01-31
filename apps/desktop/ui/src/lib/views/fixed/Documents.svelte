<script lang="ts">
  // Documents. With a place (instance = the place id), the files of that place, a folder at a time (SM-DS-3).
  // With no place, opened from the workspace's Documents row, where the workspace's documents live: one row
  // per link that has a place, which opens that place's documents (SM-DS-1, SM-DS-2).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-DS-1
  // spec: ordin-desktop/components/ScreensModule#SM-DS-2
  // spec: ordin-desktop/components/ScreensModule#SM-DS-3
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { ScreenProps } from '../../shell/screens';
  import { DocumentLinksModel } from '../add/links-model.svelte';
  import FixedList from './FixedList.svelte';
  import { loadFolder } from './lists.svelte';
  import LoadedList from './LoadedList.svelte';
  import PublishingFolder from './PublishingFolder.svelte';
  import { appOrNull } from './util';
  import { openScreen } from '../../shell/open-item';
  import { makeKey, SCREEN_IDS } from '../../shell/screens';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  let folder = $state('');
  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const links = new DocumentLinksModel(backend.handle, workspaceId ?? '');
  const places = $derived(links.links.filter((l) => l.place_id != null && l.place_id !== ''));
  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));
  let loadedOnce = $state(false);

  onMount(() => {
    if (instance === null) void links.load().then(() => (loadedOnce = true));
  });
  let first = true;
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    if (instance === null) void links.load();
  });

  // A folder row opens the folder; a file row (the only one with a date) opens its document page (SM-NS-4).
  function open(row: { id: string; title: string; detail?: string | undefined }): void {
    if (row.detail === undefined) folder = row.id;
    else if (app !== null) openScreen(app, backend.handle.id, workspaceId, makeKey(SCREEN_IDS.document, `${instance ?? ''}:${row.id}`), { title: row.title });
  }

  // A place row opens that place's documents, as its tree row does (SM-RL-1).
  function openPlace(row: { id: string; title: string }): void {
    if (app !== null) openScreen(app, backend.handle.id, workspaceId, makeKey(SCREEN_IDS.documents, row.id), { title: row.title });
  }
</script>

{#if instance === null}
  <FixedList
    {backend}
    heading={t('tree-documents')}
    rows={places.map((l) => ({ id: l.place_id ?? '', title: l.display_name }))}
    error={links.error}
    onretry={() => void links.load()}
    onopen={openPlace}
  >
    {#if loadedOnce && links.error === null && places.length === 0}<p>{t('documents-no-places')}</p>{/if}
  </FixedList>
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
