<script lang="ts">
  // The **Export** item of a dashboard's menu (SM-DB-5, SM-DB-6), in the side tree and on the dashboard's
  // screen: pressed, it opens to **PDF…**, **PowerPoint…**, **Excel…**, **Word…** and **Add to an existing
  // PowerPoint…**, as **Publish to…** opens to its destinations. Choosing one closes the menu (`onchoose`)
  // and saves on this computer; the outcome is announced, a cancelled dialog shows nothing and an error is
  // handed to `onerror`.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-DB-5
  // spec: ordin-desktop/components/ScreensModule#SM-DB-6
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ServiceError } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import { addToPresentation, announcement, EXPORT_FORMATS, exportDashboard, type ExportOutcome } from './dashboard-export';
  import { appOrNull } from './util';

  interface Props {
    backend: BackendState;
    dashboardId: string;
    name: string;
    /** Called when an item is chosen, so the menu closes before the dialog opens. */
    onchoose: () => void;
    onerror: (error: ServiceError | null) => void;
  }
  let { backend, dashboardId, name, onchoose, onerror }: Props = $props();
  const app = appOrNull();
  let open = $state(false);

  async function run(work: Promise<ExportOutcome>): Promise<void> {
    onchoose();
    onerror(null);
    const outcome = await work;
    if (outcome.kind === 'error') {
      onerror(outcome.error);
      return;
    }
    const line = announcement(outcome);
    if (line !== null) app?.shell.announce(t(line, { name }));
  }
</script>

<button type="button" role="menuitem" aria-haspopup="menu" aria-expanded={open} onclick={() => (open = !open)}><Icon name="save" />{t('dashboard-export')}</button>
{#if open}
  <div role="menu" class="export-list" aria-label={t('dashboard-export')}>
    {#each EXPORT_FORMATS as item (item.format)}
      <button type="button" role="menuitem" onclick={() => void run(exportDashboard(backend.handle, dashboardId, item.format))}><Icon name={item.icon} />{t(item.label)}</button>
    {/each}
    <button type="button" role="menuitem" onclick={() => void run(addToPresentation(backend.handle, dashboardId))}><Icon name="slides" />{t('dashboard-add-to-presentation')}</button>
  </div>
{/if}

<style>
  .export-list {
    display: flex;
    flex-direction: column;
    padding-inline-start: var(--space-md);
  }
</style>
