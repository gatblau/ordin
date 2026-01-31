<script lang="ts">
  // A table from a `table` block, a chart's **Show as table** or a report part: title, typed columns, rows,
  // sort by any column, a search box and the source line. Only the visible rows (± 50) are in the document
  // (A-34: up to 10,000 rows by 30 columns). A string cell is always text; a document link opens an inline
  // preview read through its reference.
  import { t } from '../i18n/fluent';
  import type { Cell, ContentRef, ReportPartSource, TableColumn } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import RefPreview from './RefPreview.svelte';
  import { refName } from './refs';
  import SourceLine from './SourceLine.svelte';
  import { arrangeRows, cellText, visibleWindow, type SortDirection } from './table-model';

  interface Props {
    backend: BackendState;
    title: string;
    columns: TableColumn[];
    rows: Cell[][];
    source: ReportPartSource;
    /** The level of the title's heading. */
    level?: 3 | 4;
  }
  let { backend, title, columns, rows, source, level = 3 }: Props = $props();

  const ROW_HEIGHT = 28;
  const VIEWPORT = 420;

  let query = $state('');
  let sort = $state<{ column: number; direction: SortDirection } | null>(null);
  let scrollTop = $state(0);
  let opened = $state<ContentRef | null>(null);

  const order = $derived(arrangeRows(columns, rows, query, sort));
  const span = $derived(visibleWindow(order.length, scrollTop, VIEWPORT, ROW_HEIGHT));
  const shown = $derived(order.slice(span.start, span.end));

  function toggleSort(column: number): void {
    if (sort === null || sort.column !== column) sort = { column, direction: 'ascending' };
    else if (sort.direction === 'ascending') sort = { column, direction: 'descending' };
    else sort = null;
  }
  const ariaSort = (column: number): 'ascending' | 'descending' | 'none' => (sort?.column === column ? sort.direction : 'none');
</script>

<div class="table-view">
  <svelte:element this={`h${level}`}>{title}</svelte:element>
  <label class="search">
    <span class="visually-hidden">{t('block-table-search')}</span>
    <input type="search" placeholder={t('block-table-search')} bind:value={query} oninput={() => (scrollTop = 0)} />
  </label>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="scroller" style:max-block-size={`${VIEWPORT}px`} onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)} role="region" aria-label={title} tabindex="0">
    <table>
      <thead>
        <tr>
          {#each columns as column, index (index)}
            <th scope="col" aria-sort={ariaSort(index)}>
              <button type="button" onclick={() => toggleSort(index)}>
                {column.name}
                <span aria-hidden="true">{sort?.column === index ? (sort.direction === 'ascending' ? '▲' : '▼') : ''}</span>
              </button>
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#if span.start > 0}<tr aria-hidden="true" style:block-size={`${span.start * ROW_HEIGHT}px`}></tr>{/if}
        {#each shown as rowIndex (rowIndex)}
          <tr class="data">
            {#each columns as column, c (c)}
              {@const cell = rows[rowIndex]?.[c] ?? null}
              <td class:numeric={column.type === 'number' || column.type === 'money' || column.type === 'percentage'}>
                {#if cell !== null && typeof cell === 'object'}
                  <button type="button" onclick={() => (opened = cell)}>{refName(cell) ?? t('block-file-unnamed')}</button>
                {:else}{cellText(column, cell)}{/if}
              </td>
            {/each}
          </tr>
        {/each}
        {#if span.end < order.length}<tr aria-hidden="true" style:block-size={`${(order.length - span.end) * ROW_HEIGHT}px`}></tr>{/if}
      </tbody>
    </table>
  </div>
  {#if order.length === 0}<p>{t('block-table-no-rows')}</p>{/if}
  <SourceLine {source} />
  {#if opened !== null}
    <div class="opened">
      <RefPreview {backend} reference={opened} />
      <button type="button" onclick={() => (opened = null)}>{t('block-close-preview')}</button>
    </div>
  {/if}
</div>

<style>
  .scroller {
    overflow: auto;
  }
  .table-view {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .table-view > :is(h3, h4) {
    margin: 0;
    font-size: 15px;
  }
  table {
    inline-size: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    position: sticky;
    inset-block-start: 0;
    background: var(--color-surface);
    text-align: start;
  }
  th button {
    all: unset;
    cursor: pointer;
    display: block;
    padding: 5px var(--space-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
  }
  th button:focus-visible {
    outline: 3px solid var(--color-focus);
  }
  tr.data {
    block-size: 28px;
  }
  th {
    border-block-end: 1px solid var(--color-line);
  }
  td {
    padding: 0 var(--space-sm);
    border-block-end: 1px solid var(--color-line);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-inline-size: 28ch;
  }
  td.numeric {
    text-align: end;
    font-variant-numeric: tabular-nums;
  }
  .search {
    display: block;
    max-inline-size: 260px;
  }
  .search input {
    font: inherit;
    font-size: 13px;
    color: inherit;
    inline-size: 100%;
    box-sizing: border-box;
    background: var(--color-background);
    border: 1px solid var(--color-border-control);
    border-radius: var(--radius-md);
    padding: 5px var(--space-sm);
  }
</style>
