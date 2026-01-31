<script lang="ts">
  // Remembered during this run, the disclosure of the *What happened* layer (ADR-111 step 8): closed by
  // default; opening sends `memory.remembered_in_run`; it is read again on each `run_progress` while open
  // and the run is running (`refreshKey` changes then).
  //
  // spec: ordin-desktop/components/ScreensModule#ADR-111
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import FactLine from './FactLine.svelte';
  import { RememberedController } from './memory-used.svelte';

  interface Props {
    backend: BackendState;
    runId: string;
    workspaceId?: string | null;
    running?: boolean;
    /** Changes on each `run_progress` event of the run. */
    refreshKey?: number;
  }
  let { backend, runId, workspaceId = null, running = false, refreshKey = 0 }: Props = $props();

  const c = new RememberedController(backend.handle, runId, workspaceId);
  let first = true;
  $effect(() => {
    void refreshKey;
    if (first) {
      first = false;
      return;
    }
    if (c.open && running) void c.load();
  });
</script>

<div class="remembered">
  <button type="button" aria-expanded={c.open} onclick={() => void c.toggle()}>{t('memory-remembered-in-run')}</button>
  {#if c.open}
    <ErrorLine error={c.error} />
    {#if c.loaded && c.items.length === 0}
      <p>{t('memory-remembered-none')}</p>
    {:else}
      <ul>
        {#each c.items as item (item.fact.id)}
          {@const by = c.byLine(item)}
          <FactLine fact={item.fact} names={c.names} />
          <li class="secondary">{t(by.id, by.args)}</li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>
