<script lang="ts">
  // The document browser of a task's form (SM-TF-9): a small dialog that works as the side tree's Documents
  // group does. The workspace's places and its linked code repositories at the top, folders that open one
  // level at a time with `files.list_folder`, and a search box over file names (`files.search`). Choosing a
  // file closes it; Cancel and Escape close it only, never the form.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TF-9
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { FileEntry, SearchHit, ServiceError } from '../../service-api/types.gen';
  import { focusableIn, trapIndex } from '../../shell/focus';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { documentLine, type DocumentRoot } from './document-names';

  interface Props {
    backend: BackendState;
    /** The places at the top, in order (SM-TF-9 step 1). */
    roots: DocumentRoot[];
    /** The field's label, read with the dialog's title. */
    label: string;
    onchoose: (placeId: string, path: string) => void;
    oncancel: () => void;
  }
  let { backend, roots, label, onchoose, oncancel }: Props = $props();

  /** Hits drawn for a search; only names are searched here. */
  const SEARCH_LIMIT = 50;
  const SEARCH_WAIT_MS = 250;

  type Folder = { entries: FileEntry[]; next: string | null; error: ServiceError | null; loaded: boolean };
  const id = `doc-browser-${Math.random().toString(36).slice(2, 10)}`;
  let root = $state<HTMLElement | null>(null);
  let search = $state<HTMLInputElement | null>(null);
  let heading = $state<HTMLElement | null>(null);
  let folders = $state<Record<string, Folder>>({});
  let opened = $state<string[]>([]);
  let query = $state('');
  let hits = $state.raw<SearchHit[] | null>(null);
  let searchError = $state.raw<ServiceError | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let asked = 0;
  const canSearch = $derived(answers(backend, 'files.search'));

  const key = (placeId: string, folder: string): string => `${placeId}\u0000${folder}`;

  onMount(() => {
    (search ?? heading)?.focus();
    return () => {
      if (timer !== null) clearTimeout(timer);
    };
  });

  async function read(placeId: string, folder: string, cursor: string | null): Promise<void> {
    const k = key(placeId, folder);
    const out = await backend.handle.call('files.list_folder', { place_id: placeId, folder, sort: 'name_asc', ...(cursor === null ? {} : { cursor }) });
    const was = folders[k];
    if (out.kind === 'error') {
      folders[k] = { entries: was?.entries ?? [], next: was?.next ?? null, error: out.error, loaded: true };
      return;
    }
    folders[k] = { entries: cursor === null ? out.result.items : [...(was?.entries ?? []), ...out.result.items], next: out.result.next_cursor ?? null, error: null, loaded: true };
  }

  function toggle(placeId: string, folder: string): void {
    const k = key(placeId, folder);
    if (opened.includes(k)) {
      opened = opened.filter((o) => o !== k);
      return;
    }
    opened = [...opened, k];
    if (folders[k] === undefined) void read(placeId, folder, null);
  }

  function typed(value: string): void {
    query = value;
    if (timer !== null) clearTimeout(timer);
    if (value.trim() === '') {
      hits = null;
      searchError = null;
      return;
    }
    timer = setTimeout(() => void find(value.trim()), SEARCH_WAIT_MS);
  }

  async function find(text: string): Promise<void> {
    const mine = ++asked;
    const out = await backend.handle.call('files.search', { place_ids: roots.map((r) => r.placeId), query: text, limit: SEARCH_LIMIT });
    if (mine !== asked) return;
    if (out.kind === 'error') {
      searchError = out.error;
      hits = [];
      return;
    }
    searchError = null;
    hits = out.result.hits.filter((h) => h.name_match);
  }

  const fileName = (path: string): string => path.split('/').filter((p) => p !== '').pop() ?? path;

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      oncancel();
    } else if (event.key === 'Tab' && root !== null) {
      const items = focusableIn(root);
      const next = trapIndex(items.indexOf(document.activeElement as HTMLElement), items.length, event.shiftKey);
      if (next >= 0) {
        event.preventDefault();
        items[next]?.focus();
      }
    }
  }
</script>

{#snippet folder(placeId: string, path: string, depth: number)}
  {@const f = folders[key(placeId, path)]}
  {#if f !== undefined}
    <ErrorLine error={f.error} onretry={() => void read(placeId, path, null)} />
    <ul role="group" class="rows">
      {#each f.entries as entry (entry.path)}
        <li role="treeitem" aria-selected="false" aria-expanded={entry.is_folder ? opened.includes(key(placeId, entry.path)) : undefined}>
          {#if entry.is_folder}
            <button type="button" class="row" style={`padding-inline-start: ${8 + depth * 16}px`} onclick={() => toggle(placeId, entry.path)}><Icon name="folder" />{entry.name}</button>
            {#if opened.includes(key(placeId, entry.path))}{@render folder(placeId, entry.path, depth + 1)}{/if}
          {:else}
            <button type="button" class="row file" data-path={entry.path} style={`padding-inline-start: ${8 + depth * 16}px`} onclick={() => onchoose(placeId, entry.path)}><Icon name="page" />{entry.name}</button>
          {/if}
        </li>
      {/each}
      {#if f.loaded && f.error === null && f.entries.length === 0}
        <li class="empty" style={`padding-inline-start: ${8 + depth * 16}px`}>{t('task-form-browser-empty')}</li>
      {/if}
      {#if f.next !== null}
        <li><button type="button" class="row" style={`padding-inline-start: ${8 + depth * 16}px`} onclick={() => void read(placeId, path, f.next)}>{t('settings-show-more')}</button></li>
      {/if}
    </ul>
  {/if}
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="dialog-backdrop">
  <div bind:this={root} class="dialog browser" role="dialog" aria-modal="true" aria-labelledby={`${id}-title`} aria-describedby={`${id}-for`} tabindex="-1" {onkeydown}>
    <h2 id={`${id}-title`} bind:this={heading} tabindex="-1">{t('task-form-browser-title')}</h2>
    <p id={`${id}-for`} class="for">{label}</p>
    {#if canSearch}
      <label class="search-label" for={`${id}-search`}>{t('task-form-browser-search')}</label>
      <input id={`${id}-search`} bind:this={search} type="search" class="search" value={query} oninput={(e) => typed(e.currentTarget.value)} />
    {/if}
    <div class="list">
      {#if hits !== null}
        <ErrorLine error={searchError} onretry={() => void find(query.trim())} />
        <ul class="rows" aria-label={t('task-form-browser-search')}>
          {#each hits as hit (`${hit.place_id}:${hit.path}`)}
            <li>
              <button type="button" class="row file hit" data-path={hit.path} onclick={() => onchoose(hit.place_id, hit.path)}>
                <span class="hit-name"><Icon name="page" />{fileName(hit.path)}</span>
                <span class="hit-where">{documentLine(roots, hit.place_id, hit.path)}</span>
              </button>
            </li>
          {/each}
          {#if searchError === null && hits.length === 0}<li class="empty">{t('task-form-browser-none')}</li>{/if}
        </ul>
      {:else}
        <ul role="tree" class="rows" aria-labelledby={`${id}-title`}>
          {#each roots as place (place.placeId)}
            <li role="treeitem" aria-selected="false" aria-expanded={opened.includes(key(place.placeId, ''))}>
              <button type="button" class="row place" data-place={place.placeId} onclick={() => toggle(place.placeId, '')}><Icon name={place.code ? 'code' : 'folder'} />{place.name}</button>
              {#if opened.includes(key(place.placeId, ''))}{@render folder(place.placeId, '', 1)}{/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    <div class="dialog-buttons">
      <button type="button" onclick={oncancel}>{t('task-form-cancel')}</button>
    </div>
  </div>
</div>

<style>
  .browser {
    width: min(32rem, calc(100vw - 32px));
  }
  .for,
  .hit-where,
  .empty {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .for {
    margin: 0;
  }
  .search-label {
    font-size: 14px;
  }
  .search {
    width: 100%;
    box-sizing: border-box;
  }
  .list {
    max-height: 20rem;
    overflow: auto;
    margin-block: var(--space-sm);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-height: 28px;
    text-align: start;
    background: none;
    border: none;
  }
  .hit {
    flex-direction: column;
    align-items: flex-start;
  }
  .hit-name {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .empty {
    list-style: none;
    padding-block: 4px;
  }
</style>
