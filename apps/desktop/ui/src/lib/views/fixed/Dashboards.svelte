<script lang="ts">
  // Dashboards. With no instance, the workspace's kept dashboards; a row opens the dashboard (SM-DB-3).
  // With a dashboard (instance = its id), its newest report under its name, with **Refresh** as a small
  // icon button (SM-DB-4), and a ⋯ menu beside it holding **Export** (SM-DB-8). A chart part is drawn as its
  // table here, the rows it is drawn from.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-DB-3
  // spec: ordin-desktop/components/ScreensModule#SM-DB-4
  // spec: ordin-desktop/components/ScreensModule#SM-DB-8
  import { answers, answersAll } from '../../backend/groups';
  import { formatDate, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import { openScreen } from '../../shell/open-item';
  import { makeKey, SCREEN_IDS, type ScreenProps } from '../../shell/screens';
  import type { ServiceError } from '../../service-api/types.gen';
  import ErrorLine from '../ErrorLine.svelte';
  import { canExport } from './dashboard-export';
  import DashboardExportItems from './DashboardExportItems.svelte';
  import { DashboardPage } from './dashboard-page.svelte';
  import { loadDashboards } from './lists.svelte';
  import LoadedList from './LoadedList.svelte';
  import { appOrNull } from './util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const page = instance === null || workspaceId === null ? null : new DashboardPage(backend.handle, workspaceId, instance, answersAll(backend, ['reports.list', 'reports.get']));
  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));
  const exportable = $derived(canExport(backend));
  let menuOpen = $state(false);
  let exportError = $state<ServiceError | null>(null);

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  $effect(() => {
    void revision;
    if (page !== null) void page.load();
  });

  function openDashboard(row: { id: string; title: string }): void {
    if (app !== null) openScreen(app, backend.handle.id, workspaceId, makeKey(SCREEN_IDS.dashboards, row.id), { title: row.title });
  }

  async function refresh(): Promise<void> {
    if (page === null || page.dashboard === null) return;
    const name = page.dashboard.name;
    if (await page.refresh()) app?.shell.announce(t('dashboard-refresh-started', { name }));
  }

  const cell = (value: string | null | undefined): string => value ?? '';
</script>

{#if page === null}
  <LoadedList {backend} {workspaceId} heading={t('tree-dashboards')} load={() => loadDashboards(backend.handle, workspaceId ?? '')} icon="chart" tint="sheet" onopen={openDashboard} />
{:else}
  <section class="dashboard" aria-labelledby="dashboard-heading">
    {#if page.dashboard !== null}
      <div class="heading tree-line">
        <span class="icon-tile" data-tint="sheet"><Icon name="chart" size="heading" /></span>
        <h2 id="dashboard-heading" tabindex="-1" data-region-focus>{page.dashboard.name}</h2>
        {#if answers(backend, 'dashboards.refresh')}<button type="button" class="icon-button" aria-label={t('dashboard-refresh')} title={t('dashboard-refresh')} disabled={page.refreshing} onclick={() => void refresh()}><Icon name="play" /></button>{/if}
        {#if exportable}
          {@const name = page.dashboard.name}
          <span class="tree-actions shown">
            <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuOpen} aria-label={t('more-actions-for', { name })} title={t('more-actions-tip')} onclick={() => (menuOpen = !menuOpen)}><Icon name="more" /></button>
          </span>
          {#if menuOpen}
            <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name })} use:focusFirst onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); menuOpen = false; } }}>
              <DashboardExportItems {backend} dashboardId={page.dashboardId} {name} onchoose={() => (menuOpen = false)} onerror={(e) => (exportError = e)} />
            </div>
          {/if}
        {/if}
      </div>
      <ErrorLine error={exportError} />
      <p class="secondary">
        {t('dashboard-kept-on', { date: formatDate(page.dashboard.kept_at) })}{#if page.report !== null} · {t('dashboard-as-of', { time: `${formatDate(page.report.as_of)}, ${formatTime(page.report.as_of)}` })}{/if}
      </p>
    {:else if page.loaded && page.error === null}
      <p>{t('dashboard-gone')}</p>
    {/if}
    <ErrorLine error={page.error} onretry={() => void page.load()} />
    {#if page.dashboard !== null && page.loaded && page.report === null && page.error === null}
      <p class="secondary">{t('dashboard-none-yet')}</p>
    {/if}
    {#if page.report !== null}
      {#each page.report.parts as part, index (index)}
        <section class="part">
          <h3>{part.heading}</h3>
          {#if part.data.kind === 'key_figures'}
            <ul class="figures">
              {#each part.data.figures as figure (figure.label)}
                <li><span class="label">{figure.label}</span> <strong>{figure.value}</strong> {figure.unit}</li>
              {/each}
            </ul>
          {:else}
            <div class="table-wrap">
              <table>
                <thead><tr>{#each part.data.columns as column (column.name)}<th scope="col">{column.name}</th>{/each}</tr></thead>
                <tbody>
                  {#each part.data.rows as row, r (r)}
                    <tr>{#each row as value, c (c)}<td>{cell(value)}</td>{/each}</tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        </section>
      {/each}
    {/if}
  </section>
{/if}

<style>
  .dashboard {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .heading {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-12);
  }
  .heading h2 {
    flex: 1;
    margin: 0;
  }
  .icon-button {
    min-height: 0;
    padding: var(--space-04);
    border: 0;
    background: none;
    border-radius: var(--radius-sm);
  }
  .icon-button:hover {
    background: var(--color-chip);
  }
  .part h3 {
    margin: 0 0 var(--space-04);
    font-size: 15px;
  }
  .figures {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-md);
  }
  .table-wrap {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
  }
  th,
  td {
    text-align: start;
    padding: var(--space-04) var(--space-sm);
    border-block-end: 1px solid var(--color-line);
  }
  .secondary,
  .label {
    color: var(--color-text-muted);
  }
</style>
