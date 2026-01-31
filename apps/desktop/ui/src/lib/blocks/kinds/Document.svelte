<script lang="ts">
  // `document`: a Markdown document as a formatted page (the read view of 11 §4). Editing happens on the
  // document's own screen, opened with **Open** (a Markdown document's Open draws `md`, V8 *Actions*).
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import { getApp } from '../../shell/context';
  import { openTarget } from '../../shell/open-item';
  import AskButton from '../AskButton.svelte';
  import RefPreview from '../RefPreview.svelte';
  import { refName } from '../refs';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'document'> = $props();
  const app = getApp();

  function open(): void {
    if (block.item.kind !== 'place_file') return;
    openTarget(app, backend.id, workspaceId, { kind: 'document', place_id: block.item.place_id, path: block.item.relative_path, workspace_id: workspaceId });
  }
</script>

<div class="block-document">
  <div class="page"><RefPreview {backend} reference={block.item} /></div>
  <div class="actions">
    {#if block.item.kind === 'place_file'}<button type="button" onclick={open}><Icon name="md" />{t('card-open')}</button>{/if}
    <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: refName(block.item) ?? t('block-file-unnamed') })} attachments={[block.item]} />
  </div>
</div>

<style>
  /* The prototype reads a Markdown document on a card (`.mdv.mdread`; scenes `md`, `mdNew`). */
  .page {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-lg);
    padding: 18px;
  }
  .page :global(h1) {
    margin-block: 0 var(--space-12);
  }
  .actions {
    display: flex;
    gap: var(--space-sm);
    margin-block-start: var(--space-sm);
  }
</style>
