<script lang="ts">
  // Release notes are plain text (SM-UP-6): text nodes only, line breaks kept, `- ` and `* ` lines as list
  // items, no Markdown, no links, no images, no HTML.
  import { parseNotes } from './updates.svelte';

  interface Props {
    notes: string;
  }
  let { notes }: Props = $props();
  const blocks = $derived(parseNotes(notes));
</script>

<div class="notes">
  {#each blocks as block, i (i)}
    {#if block.kind === 'list'}
      <ul>
        {#each block.items as item, j (j)}<li>{item}</li>{/each}
      </ul>
    {:else}
      <p class="plain">{block.lines.join('\n')}</p>
    {/if}
  {/each}
</div>

<style>
  .plain {
    white-space: pre-line;
  }
</style>
