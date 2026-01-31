<script lang="ts">
  // The foot of the local workspace tree: Archived and the Bin with their counts, the error line of an action
  // chosen in the tree, the line after Move to Bin with Undo, and the tree's confirmation (SM-WS-4 to SM-WS-7).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WS-5
  // spec: ordin-desktop/components/ScreensModule#SM-WS-7
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import { openScreen } from '../../shell/open-item';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from '../fixed/util';
  import LifecycleDialog from './LifecycleDialog.svelte';
  import { ARCHIVED_SCREEN, BIN_SCREEN, lifecycleOf } from './lifecycle.svelte';

  interface Props {
    backend: BackendState;
  }
  let { backend }: Props = $props();
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const model = lifecycleOf(backend, app?.shell ?? null);
  const listed = $derived(answers(backend, 'workspaces.list'));

  onMount(() => {
    if (answers(backend, 'workspaces.list')) void model.loadCounts();
    return backend.subscribe((env) => {
      if (env.kind === 'workspace_changed' && answers(backend, 'workspaces.list')) void model.loadCounts();
    });
  });

  function open(key: string): void {
    if (app !== null) openScreen(app, backend.id, null, key);
  }
</script>

{#if listed}
  <div class="lifecycle-foot">
    <ErrorLine error={model.railError} />
    {#if model.binnedLine !== null}
      <p class="binned-line" role="status">
        {t('ws-binned', { name: model.binnedLine.workspace.name })}
        <button type="button" class="small" onclick={() => void model.undoBin()}>{t('ws-undo')}</button>
      </p>
    {/if}
    {#if model.archived.length > 0}
      <button type="button" class="foot-entry" aria-label={t('rail-archived-count', { count: model.archived.length })} onclick={() => open(ARCHIVED_SCREEN)}>
        <span class="icon-tile"><Icon name="fold" /></span>{t('rail-archived')}<span class="count">{model.archived.length}</span>
      </button>
    {/if}
    <button type="button" class="foot-entry" aria-label={model.binned.length > 0 ? t('rail-bin-count', { count: model.binned.length }) : t('rail-bin')} onclick={() => open(BIN_SCREEN)}>
      <span class="icon-tile"><Icon name="bin" /></span>{t('rail-bin')}{#if model.binned.length > 0}<span class="count">{model.binned.length}</span>{/if}
    </button>
    <LifecycleDialog {model} place="rail" />
  </div>
{/if}

<style>
  .lifecycle-foot {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
    font-size: 14px;
  }
  .foot-entry {
    display: flex;
    align-items: center;
    gap: 5px;
    min-height: 30px;
    padding: 2px 6px 2px calc(2px + 18px + 5px);
    border: 1px solid transparent;
    border-radius: var(--radius-row);
    background: none;
    text-align: start;
    font-size: 14px;
    font-weight: 400;
  }
  .foot-entry:hover {
    background: var(--color-chip);
  }
  .foot-entry .count {
    margin-inline-start: auto;
  }
  .binned-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-06);
    font-size: 13px;
    color: var(--color-text-secondary);
  }
</style>
