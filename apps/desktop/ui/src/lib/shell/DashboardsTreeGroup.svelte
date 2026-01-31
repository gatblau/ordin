<script lang="ts">
  // The Dashboards row under a workspace (SM-DB-3): its main action opens the Dashboards screen and shows,
  // under it, one row per kept dashboard (`dashboards.list`), read when it opens and again on each
  // `workspace_changed`; a dashboard row opens that dashboard (`dashboards:<dashboard id>`). A dashboard row's
  // ⋯ and right-click menu holds **Refresh**, **Export** (SM-DB-5) and **Remove…**, which asks first (SM-DB-7).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-DB-3
  // spec: ordin-desktop/components/ScreensModule#SM-DB-7
  import { answers } from '../backend/groups';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { Dashboard, ServiceError } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import ConfirmDialog from '../views/ConfirmDialog.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { canExport } from '../views/fixed/dashboard-export';
  import DashboardExportItems from '../views/fixed/DashboardExportItems.svelte';
  import { appOrNull } from '../views/fixed/util';
  import { openScreen } from './open-item';
  import { ScreenAreaState } from './screen-area.svelte';
  import { makeKey, SCREEN_IDS } from './screens';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    initiallyOpen?: boolean;
    /** The dashboard whose ⋯ menu is open when drawn (a preview or a test). */
    initialMenu?: string | null;
  }
  let { backend, workspaceId, initiallyOpen = false, initialMenu = null }: Props = $props();

  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  let open = $state(initiallyOpen);
  let dashboards = $state.raw<Dashboard[]>([]);
  let error = $state<ServiceError | null>(null);
  // svelte-ignore state_referenced_locally
  let menuFor = $state<string | null>(initialMenu);
  let removing = $state.raw<Dashboard | null>(null);
  let removeError = $state<ServiceError | null>(null);
  /** The error of the last Refresh or Export chosen in a row's menu. */
  let actionError = $state<ServiceError | null>(null);
  let busy = $state(false);
  const canRefresh = $derived(answers(backend, 'dashboards.refresh'));
  const canRemove = $derived(answers(backend, 'dashboards.remove'));
  const exportable = $derived(canExport(backend));
  const hasMenu = $derived(canRefresh || canRemove || exportable);
  const revision = $derived(backend.workspaceRevision(workspaceId));

  async function load(): Promise<void> {
    const out = await backend.handle.call('dashboards.list', { workspace_id: workspaceId });
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    error = null;
    dashboards = out.result.items;
    // SM-OS-3: a dashboard removed elsewhere closes its tab once the complete list no longer has it.
    if (out.result.next_cursor == null) app?.screens.closeMissing(ScreenAreaState.scope(backend.id, workspaceId), 'dashboard', dashboards.map((x) => x.id));
  }

  $effect(() => {
    void revision;
    if (open) void load();
  });

  function openGroup(): void {
    open = !open;
    if (app !== null) openScreen(app, backend.id, workspaceId, SCREEN_IDS.dashboards);
  }

  function openDashboard(d: Dashboard): void {
    menuFor = null;
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.dashboards, d.id), { title: d.name });
  }

  async function refresh(d: Dashboard): Promise<void> {
    menuFor = null;
    const out = await backend.handle.call('dashboards.refresh', { workspace_id: workspaceId, dashboard_id: d.id });
    if (out.kind === 'error') {
      actionError = out.error;
      return;
    }
    actionError = null;
    app?.shell.announce(t('dashboard-refresh-started', { name: d.name }));
  }

  function askRemove(d: Dashboard): void {
    menuFor = null;
    removeError = null;
    removing = d;
  }

  async function confirmRemove(): Promise<void> {
    const d = removing;
    if (d === null || busy) return;
    busy = true;
    const out = await backend.handle.call('dashboards.remove', { dashboard_id: d.id });
    busy = false;
    if (out.kind === 'error') {
      removeError = out.error;
      return;
    }
    removing = null;
    dashboards = dashboards.filter((x) => x.id !== d.id);
    app?.screens.closeScreensFor('dashboard', d.id);
    app?.shell.announce(t('dashboard-removed', { name: d.name }));
  }

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  function menuKey(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    menuFor = null;
  }
</script>

<div role="treeitem" aria-selected="false" aria-expanded={open} tabindex="-1" class="tree-leaf">
  <div class="tree-line">
    <button type="button" class="tree-row" onclick={openGroup}><span class="icon-tile"><Icon name="chart" /></span>{t('tree-dashboards')}</button>
  </div>
  {#if open}
    <ErrorLine {error} onretry={() => void load()} />
    <ErrorLine error={actionError} />
    <div role="group" class="tree-group">
      {#each dashboards as d (d.id)}
        <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf">
          <div class="tree-line">
            <button type="button" class="tree-row leaf tree-dashboard" onclick={() => openDashboard(d)} oncontextmenu={(e) => { if (!hasMenu) return; e.preventDefault(); menuFor = d.id; }}><span class="icon-tile" data-tint="sheet"><Icon name="chart" /></span><span class="tree-dashboard-name">{d.name}</span></button>
            {#if hasMenu}
              <span class="tree-actions" class:shown={menuFor === d.id}>
                <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuFor === d.id} aria-label={t('more-actions-for', { name: d.name })} title={t('more-actions-tip')} onclick={() => (menuFor = menuFor === d.id ? null : d.id)}><Icon name="more" /></button>
              </span>
              {#if menuFor === d.id}
                <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: d.name })} onkeydown={menuKey} use:focusFirst>
                  {#if canRefresh}<button type="button" role="menuitem" onclick={() => void refresh(d)}><Icon name="play" />{t('dashboard-refresh')}</button>{/if}
                  {#if exportable}<DashboardExportItems {backend} dashboardId={d.id} name={d.name} onchoose={() => (menuFor = null)} onerror={(e) => (actionError = e)} />{/if}
                  {#if canRemove}<button type="button" role="menuitem" class="danger" onclick={() => askRemove(d)}><Icon name="remove" />{t('dashboard-remove')}</button>{/if}
                </div>
              {/if}
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if removing !== null}
  {@const d = removing}
  <ConfirmDialog title={t('dashboard-remove-title', { name: d.name })} body={t('dashboard-remove-body')} verb={t('dashboard-remove-verb')} onconfirm={() => void confirmRemove()} oncancel={() => (removing = null)}>
    <ErrorLine error={removeError} />
  </ConfirmDialog>
{/if}

<style>
  .tree-dashboard {
    padding-inline-start: calc(2px + 28px + 23px);
  }
  .tree-dashboard-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
