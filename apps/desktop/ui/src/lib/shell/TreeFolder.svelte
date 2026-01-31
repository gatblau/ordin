<script lang="ts">
  // The inside of a linked location in the tree: its sub-folders and documents, read with `files.list_folder`
  // when the row is opened, a folder one level at a time (SM-RL-3). A folder row opens or closes its own
  // contents; a document row opens the document page.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-RL-3
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { FileEntry, ServiceError } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { appOrNull } from '../views/fixed/util';
  import TreeFolder from './TreeFolder.svelte';
  import { openScreen } from './open-item';
  import { makeKey, SCREEN_IDS } from './screens';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    placeId: string;
    /** The folder's path inside the place, `/`-separated, empty for the place itself. */
    folder?: string;
    /** How deep the rows sit, for their indent. */
    depth?: number;
  }
  let { backend, workspaceId, placeId, folder = '', depth = 1 }: Props = $props();

  const app = appOrNull();
  let entries = $state<FileEntry[]>([]);
  let error = $state<ServiceError | null>(null);
  let loaded = $state(false);
  let next = $state<string | null>(null);
  let opened = $state<string[]>([]);

  async function read(cursor: string | null): Promise<void> {
    const out = await backend.handle.call('files.list_folder', {
      place_id: placeId,
      folder,
      sort: 'name_asc',
      ...(cursor === null ? {} : { cursor }),
    });
    loaded = true;
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    error = null;
    entries = cursor === null ? out.result.items : [...entries, ...out.result.items];
    next = out.result.next_cursor ?? null;
  }

  $effect(() => {
    void backend.workspaceRevision(workspaceId);
    void read(null);
  });

  const indent = $derived(`padding-inline-start: calc(2px + 28px + ${23 + depth * 14}px)`);

  function openFile(entry: FileEntry): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.document, `${placeId}:${entry.path}`), { title: entry.name });
  }

  function toggle(entry: FileEntry): void {
    opened = opened.includes(entry.path) ? opened.filter((p) => p !== entry.path) : [...opened, entry.path];
  }
</script>

<ErrorLine {error} onretry={() => void read(null)} />
<div role="group" class="tree-group">
  {#each entries as entry (entry.path)}
    {#if entry.is_folder}
      <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf" aria-expanded={opened.includes(entry.path)}>
        <div class="tree-line">
          <button type="button" class="tree-row leaf" style={indent} onclick={() => toggle(entry)}><span class="icon-tile"><Icon name="folder" /></span><span class="tree-name">{entry.name}</span></button>
        </div>
        {#if opened.includes(entry.path)}
          <TreeFolder {backend} {workspaceId} {placeId} folder={entry.path} depth={depth + 1} />
        {/if}
      </div>
    {:else}
      <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf">
        <div class="tree-line">
          <button type="button" class="tree-row leaf" style={indent} onclick={() => openFile(entry)}><span class="icon-tile"><Icon name="page" /></span><span class="tree-name">{entry.name}</span></button>
        </div>
      </div>
    {/if}
  {/each}
  {#if loaded && error === null && entries.length === 0}
    <p class="tree-empty" style={indent}>{t('tree-folder-empty')}</p>
  {/if}
  {#if next !== null}
    <button type="button" class="tree-row leaf" style={indent} onclick={() => void read(next)}>{t('settings-show-more')}</button>
  {/if}
</div>

<style>
  .tree-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tree-empty {
    margin: 0;
    color: var(--color-text-muted);
    font-size: 12px;
  }
</style>
