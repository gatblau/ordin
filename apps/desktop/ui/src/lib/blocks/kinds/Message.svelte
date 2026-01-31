<script lang="ts">
  // `message` from Ordin or the system: the text as formatted Markdown (SM-MSG-1), attachments as one-line
  // cards. MarkdownView turns the text into a tree and draws every piece through Svelte text interpolation,
  // so markup in the text is shown literally and a link is drawn as its words, never as a link. The
  // person's own messages are drawn by the conversation panel as typed (SM-CV-1), never here. An answer
  // that carries `memory_used` shows the quiet link **What Ordin used** under it (ADR-111 step 7).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MSG-1
  // spec: ordin-desktop/components/ScreensModule#ADR-111
  import { answers } from '../../backend/groups';
  import MemoryUsedPanel from '../../views/fixed/MemoryUsedPanel.svelte';
  import ContentRefLine from '../ContentRefLine.svelte';
  import MarkdownView from '../MarkdownView.svelte';
  import { refKey } from '../refs';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'message'> = $props();
</script>

<div class="block-message">
  <MarkdownView source={block.text} />
  {#if block.attachments.length > 0}
    <ul class="attachments">
      {#each block.attachments as reference (refKey(reference))}
        <li><ContentRefLine {backend} {workspaceId} {reference} /></li>
      {/each}
    </ul>
  {/if}
  {#if (block.memory_used ?? []).length > 0 && answers(backend, 'memory.used')}
    <MemoryUsedPanel {backend} {workspaceId} recallIds={block.memory_used} />
  {/if}
</div>

<style>
  .block-message {
    overflow-wrap: anywhere;
  }
  .block-message :global(.markdown p) {
    margin-block: var(--space-sm);
  }
  .attachments {
    list-style: none;
    padding: 0;
    display: grid;
    gap: var(--space-04);
  }
</style>
