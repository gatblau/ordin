<script lang="ts">
  // `table`: title, typed columns, rows, sort by any column, a search box and the source line.
  import { t } from '../../i18n/fluent';
  import AskButton from '../AskButton.svelte';
  import TableView from '../TableView.svelte';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'table'> = $props();
</script>

<div class="block-table">
  <TableView {backend} title={block.title} columns={block.columns} rows={block.rows} source={block.source} />
  <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: block.title })} />
</div>
