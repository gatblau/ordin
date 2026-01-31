<script lang="ts">
  // `preview`: one document, page or image, read from the core for the reference.
  import { t } from '../../i18n/fluent';
  import AskButton from '../AskButton.svelte';
  import RefPreview from '../RefPreview.svelte';
  import { refName } from '../refs';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'preview'> = $props();
</script>

<!-- The document on a surface of its own, then its actions (the prototype's preview block, scene `cfPreview`). -->
<div class="block-preview">
  <div class="pane">
    <RefPreview {backend} reference={block.item} />
  </div>
  <div class="actions">
    <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: refName(block.item) ?? t('block-file-unnamed') })} attachments={[block.item]} />
  </div>
</div>

<style>
  .block-preview {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .pane {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    padding: var(--space-12) var(--space-14);
    font-size: 14px;
  }
  /* **Ask Ordin about this** takes the small size of the prototype's action row. */
  .actions :global(button) {
    padding: 3px var(--space-sm);
    min-block-size: 28px;
    font-size: 13px;
  }
</style>
