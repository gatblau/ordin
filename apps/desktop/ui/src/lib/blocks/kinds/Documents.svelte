<script lang="ts">
  // `documents`: the referenced items as a list grouped by folder, 25 at a time with **Show more**.
  // Actions: **Ask Ordin about these**, and **Open** on each item.
  import { t } from '../../i18n/fluent';
  import AskButton from '../AskButton.svelte';
  import ContentRefLine from '../ContentRefLine.svelte';
  import { refKey } from '../refs';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'documents'> = $props();
  const PAGE = 25;
  let shown = $state(PAGE);

  const folderOf = (path: string): string => {
    const at = path.lastIndexOf('/');
    return at < 0 ? '' : path.slice(0, at);
  };
  const visible = $derived(block.items.slice(0, shown));
  const groups = $derived.by(() => {
    const byFolder = new Map<string, typeof visible>();
    for (const item of visible) {
      const folder = item.kind === 'place_file' ? folderOf(item.relative_path) : '';
      byFolder.set(folder, [...(byFolder.get(folder) ?? []), item]);
    }
    return [...byFolder];
  });
</script>

<div class="block-documents">
  {#each groups as [folder, items] (folder)}
    <section>
      {#if folder !== ''}<h4>{folder}</h4>{/if}
      <ul>
        {#each items as reference (refKey(reference))}
          <li><ContentRefLine {backend} workspaceId={block.workspace_id} {reference} /></li>
        {/each}
      </ul>
    </section>
  {/each}
  {#if shown < block.items.length}
    <button type="button" onclick={() => (shown += PAGE)}>{t('block-show-more')}</button>
  {/if}
  <AskButton {backend} {workspaceId} labelId="block-ask-about-these" text={t('block-ask-text-these')} attachments={block.items} />
</div>

<style>
  /* One row per document with a divider under it (the prototype's `table.files`, scenes `docs`, `find`). */
  .block-documents {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    align-items: flex-start;
  }
  .block-documents > section {
    align-self: stretch;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 14px;
  }
  li {
    padding: 5px var(--space-06);
    border-block-end: 1px solid var(--color-line);
  }
  h4 {
    margin: 0;
    font-size: 15px;
    overflow-wrap: anywhere;
  }
</style>
