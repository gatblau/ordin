<script lang="ts">
  // `report`: title, "as of", 1 to 12 parts each a table, chart or key figures under its heading. A part the
  // drawing side does not know shows `block-cannot-show` and **Ask Ordin why**. The title's ⋯ menu holds
  // **Keep as dashboard…** (SM-DB-1), which asks for a name and keeps it under the workspace's Dashboards.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-DB-1
  import { answersAll } from '../../backend/groups';
  import { formatDate, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ServiceError } from '../../service-api/types.gen';
  import ConfirmDialog from '../../views/ConfirmDialog.svelte';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import { appOrNull } from '../../views/fixed/util';
  import AskButton from '../AskButton.svelte';
  import { defaultName, DASHBOARD_NAME_MAX, keepAsDashboard } from '../keep-dashboard';
  import type { KindProps } from '../types';
  import Chart from './Chart.svelte';
  import KeyFigures from './KeyFigures.svelte';
  import Table from './Table.svelte';

  let { backend, workspaceId, blockId, block }: KindProps<'report'> = $props();
  const app = appOrNull();
  const asOf = $derived(`${formatDate(block.as_of)}, ${formatTime(block.as_of)}`);
  const canKeep = $derived(answersAll(backend, ['reports.list', 'runs.get', 'dashboards.keep']));

  let menuOpen = $state(false);
  let keeping = $state(false);
  let name = $state('');
  let busy = $state(false);
  let problem = $state<string | null>(null);
  let error = $state<ServiceError | null>(null);

  function startKeep(): void {
    menuOpen = false;
    name = defaultName(block.title);
    problem = null;
    error = null;
    keeping = true;
  }

  async function keep(): Promise<void> {
    if (busy) return;
    busy = true;
    const out = await keepAsDashboard(backend.handle, workspaceId, block, name);
    busy = false;
    problem = null;
    error = null;
    switch (out.kind) {
      case 'kept':
        keeping = false;
        app?.shell.announce(t('dashboard-kept', { name: out.dashboard.name }));
        return;
      case 'name_needed':
        problem = t('dashboard-keep-name-needed');
        return;
      case 'not_found':
        problem = t('dashboard-keep-not-found');
        return;
      case 'from_crew':
        problem = t('dashboard-keep-from-crew');
        return;
      case 'error':
        error = out.error;
    }
  }

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }
</script>

<article class="block-report" aria-labelledby={`report-${blockId}`}>
  <div class="title tree-line">
    <h3 id={`report-${blockId}`}>{block.title}</h3>
    {#if canKeep}
      <span class="tree-actions shown">
        <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuOpen} aria-label={t('more-actions-for', { name: block.title })} title={t('more-actions-tip')} onclick={() => (menuOpen = !menuOpen)}><Icon name="more" /></button>
      </span>
      {#if menuOpen}
        <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: block.title })} use:focusFirst onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); menuOpen = false; } }}>
          <button type="button" role="menuitem" onclick={startKeep}><Icon name="chart" />{t('dashboard-keep')}</button>
        </div>
      {/if}
    {/if}
  </div>
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

{#if keeping}
  <ConfirmDialog
    title={t('dashboard-keep-title', { title: block.title })}
    body={t('dashboard-keep-body')}
    verb={t('dashboard-keep-verb')}
    destructive={false}
    onconfirm={() => void keep()}
    oncancel={() => (keeping = false)}
  >
    <label class="keep-name">
      {t('dashboard-keep-name')}
      <input type="text" bind:value={name} maxlength={DASHBOARD_NAME_MAX} disabled={busy} />
    </label>
    {#if problem !== null}<p role="status">{problem}</p>{/if}
    <ErrorLine {error} />
  </ConfirmDialog>
{/if}

<style>
  .block-report {
    display: flex;
    flex-direction: column;
    gap: var(--space-12);
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    position: relative;
  }
  .title > h3 {
    flex: 1;
    margin: 0;
  }
  .part > h4 {
    margin: 0;
    font-size: 15px;
  }
  .keep-name {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  /* The part's heading already names a chart or table titled the same. */
  .same-title :global(:is(.block-chart, .table-view) > h3:first-child),
  .same-title :global(.block-table > .table-view > :is(h3, h4):first-child) {
    display: none;
  }
</style>
