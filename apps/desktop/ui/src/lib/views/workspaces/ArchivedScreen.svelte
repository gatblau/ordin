<script lang="ts">
  // Archived workspaces (SM-WS-5): each with Restore and Remove workspace…. Read when it opens and on each
  // workspace_changed.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WS-5
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from '../fixed/util';
  import LifecycleDialog from './LifecycleDialog.svelte';
  import { lifecycleOf } from './lifecycle.svelte';

  let { backend }: ScreenProps = $props();
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const model = lifecycleOf(backend, app?.shell ?? null);

  onMount(() => {
    void model.refresh();
    const stop = backend.subscribe((env) => {
      if (env.kind === 'workspace_changed') void model.refresh();
    });
    // Closing the screen closes its confirmation.
    return () => {
      stop();
      if (model.asking?.from === 'archived') model.cancel();
    };
  });
</script>

<section class="archived" aria-labelledby="archived-heading">
  <h2 id="archived-heading" tabindex="-1" data-region-focus>{t('archived-title')}</h2>
  <p class="muted">{t('archived-lead')}</p>
  {#if model.archived.length === 0}
    <p class="muted small">{t('archived-none')}</p>
  {:else}
    <ul class="list-rows">
      {#each model.archived as workspace (workspace.id)}
        <li class="list-row">
          <span class="icon-tile"><Icon name="ws" /></span>
          <span class="grow">
            <b>{workspace.name}</b>
            <ErrorLine error={model.rowErrors.get(workspace.id) ?? null} />
          </span>
          <button type="button" class="small" onclick={() => void model.restore(workspace)}><Icon name="history" />{t('ws-restore')}</button>
          {#if model.offered.remove}
            <button type="button" class="small" onclick={() => void model.askBin(workspace, 'archived')}><Icon name="bin" />{t('ws-remove')}</button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <LifecycleDialog {model} place="archived" />
</section>

<style>
  .archived {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .grow {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
</style>
