<script lang="ts">
  // Rename… under a workspace's row (SM-WS-3): one field holding the current name, selected, with Rename and
  // Cancel. Enter renames, Escape cancels; a refusal is the error line and the field keeps what was typed.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WS-3
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { Workspace } from '../../service-api/types.gen';
  import ErrorLine from '../ErrorLine.svelte';
  import { WORKSPACE_NAME_MAX, WorkspaceLifecycle } from './lifecycle.svelte';

  interface Props {
    model: WorkspaceLifecycle;
    workspace: Workspace;
  }
  let { model, workspace }: Props = $props();

  // svelte-ignore state_referenced_locally
  let name = $state(workspace.name);
  let field = $state<HTMLInputElement | null>(null);
  const can = $derived(WorkspaceLifecycle.canRename(workspace.name, name) && !model.busy);

  onMount(() => {
    field?.focus();
    field?.select();
  });

  function onkeydown(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    model.cancelRename();
  }

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    if (can) void model.rename(workspace.id, name);
  }
</script>

<div role="group" class="rename-workspace" aria-label={t('ws-rename-field')}>
  <form onsubmit={submit}>
    <label>
      {t('ws-rename-field')}
      <input bind:this={field} bind:value={name} maxlength={WORKSPACE_NAME_MAX} {onkeydown} />
    </label>
    <div class="buttons">
      <button type="submit" class="small" data-default="true" disabled={!can}>{t('ws-rename-verb')}</button>
      <button type="button" class="small" onclick={() => model.cancelRename()}>{t('confirm-cancel')}</button>
    </div>
    <ErrorLine error={model.renameError} onretry={() => void model.rename(workspace.id, name)} />
  </form>
</div>

<style>
  .rename-workspace form {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    padding: var(--space-04) 0 var(--space-06) calc(2px + 18px + 5px);
    font-size: 14px;
  }
  .rename-workspace label {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  .buttons {
    display: flex;
    gap: var(--space-06);
  }
</style>
