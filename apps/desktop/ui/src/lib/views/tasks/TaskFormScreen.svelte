<script lang="ts">
  // The task form screen `task-form:<agent instance id>:<command id>` (SM-TF-1). An agent instance id holds
  // no `:`; the command id, which does, is the rest of the instance.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TF-1
  import { t } from '../../i18n/fluent';
  import { makeKey, type ScreenProps } from '../../shell/screens';
  import TaskFormView from './TaskFormView.svelte';
  import { TASK_FORM_SCREEN } from './ids';

  let { backend, workspaceId, instance }: ScreenProps = $props();

  const at = $derived(instance === null ? -1 : instance.indexOf(':'));
  const agentId = $derived(instance === null || at < 0 ? null : instance.slice(0, at));
  const commandId = $derived(instance === null || at < 0 ? null : instance.slice(at + 1));
</script>

{#if workspaceId !== null && agentId !== null && commandId !== null}
  <TaskFormView {backend} {workspaceId} {agentId} {commandId} tabKey={makeKey(TASK_FORM_SCREEN, instance)} />
{:else}
  <p>{t('task-form-gone')}</p>
{/if}
