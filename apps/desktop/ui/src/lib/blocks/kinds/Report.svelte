<script lang="ts">
  // `report`: title, "as of", 1 to 12 parts each a table, chart or key figures under its heading. A part the
  // drawing side does not know shows `block-cannot-show` and **Ask Ordin why**.
  import { formatDate, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import AskButton from '../AskButton.svelte';
  import type { KindProps } from '../types';
  import Chart from './Chart.svelte';
  import KeyFigures from './KeyFigures.svelte';
  import Table from './Table.svelte';

  let { backend, workspaceId, blockId, block }: KindProps<'report'> = $props();
  const asOf = $derived(`${formatDate(block.as_of)}, ${formatTime(block.as_of)}`);
</script>

<article class="block-report" aria-labelledby={`report-${blockId}`}>
  <h3 id={`report-${blockId}`}>{block.title}</h3>
  <p class="block-meta">{t('report-as-of', { time: asOf })}</p>
  {#each block.parts as part, index (index)}
    <section class="part" class:same-title={"title" in part.content && part.content.title === part.heading}>
      <h4>{part.heading}</h4>
      {#if part.content.kind === 'table'}
        <Table {backend} {workspaceId} {blockId} block={part.content} />
      {:else if part.content.kind === 'chart'}
        <Chart {backend} {workspaceId} {blockId} block={part.content} />
      {:else if part.content.kind === 'key_figures'}
        <KeyFigures {backend} {workspaceId} {blockId} block={part.content} />
      {:else}
        <p>{t('block-cannot-show')}</p>
        <AskButton {backend} {workspaceId} labelId="block-ask-why" text={t('block-ask-text-why', { title: part.heading })} />
      {/if}
    </section>
  {/each}
  <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: block.title })} />
</article>

<style>
  .block-report {
    display: flex;
    flex-direction: column;
    gap: var(--space-12);
  }
  .block-report > h3 {
    margin: 0;
  }
  .part > h4 {
    margin: 0;
    font-size: 15px;
  }
  /* The part's heading already names a chart or table titled the same. */
  .same-title :global(:is(.block-chart, .table-view) > h3:first-child),
  .same-title :global(.block-table > .table-view > :is(h3, h4):first-child) {
    display: none;
  }
</style>
