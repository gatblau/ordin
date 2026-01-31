<script lang="ts">
  // A Markdown document as a formatted page. The text becomes a tree (markdown.ts) and the tree becomes
  // elements, so markup in the text is shown literally and a link is never a link (2C §7).
  import { parseMarkdown } from './markdown';
  import type { Inline } from './markdown';

  interface Props {
    source: string;
    /** Draw each link's target after its words, as text (an approval's content view, SM-SF-1.5). */
    showTargets?: boolean;
  }
  let { source, showTargets = false }: Props = $props();
  const blocks = $derived(parseMarkdown(source));
</script>

{#snippet inlines(list: Inline[])}
  {#each list as part, i (i)}
    {#if part.kind === 'strong'}<strong>{part.text}</strong>{:else if part.kind === 'emphasis'}<em>{part.text}</em>{:else if part.kind === 'code'}<code>{part.text}</code>{:else if part.kind === 'link'}{part.text}{#if showTargets}{' '}({part.target}){/if}{:else}{part.text}{/if}
  {/each}
{/snippet}

<div class="markdown">
  {#each blocks as block, index (index)}
    {#if block.kind === 'heading'}
      <svelte:element this={`h${Math.min(6, block.level + 2)}`}>{@render inlines(block.inlines)}</svelte:element>
    {:else if block.kind === 'paragraph'}
      <p>{@render inlines(block.inlines)}</p>
    {:else if block.kind === 'list'}
      <svelte:element this={block.ordered ? 'ol' : 'ul'}>
        {#each block.items as item, k (k)}<li>{@render inlines(item)}</li>{/each}
      </svelte:element>
    {:else if block.kind === 'quote'}
      <blockquote>{@render inlines(block.inlines)}</blockquote>
    {:else if block.kind === 'code'}
      <pre><code>{block.text}</code></pre>
    {:else}
      <hr />
    {/if}
  {/each}
</div>

<style>
  .markdown {
    max-inline-size: 70ch;
    line-height: 1.5;
  }
  blockquote {
    margin-inline: 0;
    padding-inline-start: var(--space-12);
    border-inline-start: 3px solid var(--color-line);
    color: var(--color-text-muted);
  }
  pre {
    overflow: auto;
    padding: var(--space-sm);
    background: var(--color-surface);
    border-radius: var(--radius-md);
  }
</style>
