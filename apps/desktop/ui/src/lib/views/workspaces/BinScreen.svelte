<script lang="ts">
  // The Bin (SM-WS-7, SM-WS-8; 11 §5.13, UX-A68; scene `binWs`): the workspaces the person removed, each with
  // Restore and Delete now…, and Empty Bin… after them. Read when it opens and on each workspace_changed.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WS-7
  // spec: ordin-desktop/components/ScreensModule#SM-WS-8
  import { onMount } from 'svelte';

  import { formatDate } from '../../format/intl';
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
    void model.loadBin();
    const stop = backend.subscribe((env) => {
      if (env.kind === 'workspace_changed') void model.loadBin();
    });
    // Closing the screen closes its confirmation.
    return () => {
      stop();
      if (model.asking?.from === 'bin') model.cancel();
    };
  });
</script>

<section class="bin" aria-labelledby="bin-heading">
  <h2 id="bin-heading" tabindex="-1" data-region-focus>{t('bin-title')}</h2>
  <p class="muted">{t('bin-lead')}</p>
  {#if model.binned.length === 0}
    <p class="muted small">{t('bin-none')}</p>
  {:else}
    <ul class="list-rows">
      {#each model.binned as workspace (workspace.id)}
        <li class="list-row">
          <span class="icon-tile"><Icon name="ws" /></span>
          <span class="grow">
            <b>{workspace.name}</b>
            {#if workspace.moved_to_bin_at}<small>{t('bin-removed-on', { date: formatDate(workspace.moved_to_bin_at, { dateStyle: 'short' }) })}</small>{/if}
            <ErrorLine error={model.rowErrors.get(workspace.id) ?? null} />
          </span>
          <button type="button" class="small" onclick={() => void model.restoreFromBin(workspace)}><Icon name="history" />{t('bin-restore')}</button>
          <button type="button" class="small" onclick={() => void model.askDelete(workspace)}><Icon name="remove" />{t('bin-delete')}</button>
        </li>
      {/each}
    </ul>
    <div class="foot">
      <button type="button" class="danger" onclick={() => model.askEmpty()}><Icon name="bin" />{t('bin-empty')}</button>
    </div>
  {/if}
  <LifecycleDialog {model} place="bin" />
</section>

<style>
  .bin {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .grow {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
  .foot {
    display: flex;
  }
</style>
