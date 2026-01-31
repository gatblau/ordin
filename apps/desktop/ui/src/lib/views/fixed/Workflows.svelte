<script lang="ts">
  import { t } from '../../i18n/fluent';
  import type { ScreenProps } from '../../shell/screens';
  import { loadWorkflows } from './lists.svelte';
  import LoadedList from './LoadedList.svelte';
  import { appOrNull } from './util';
  import { openScreen } from '../../shell/open-item';
  import { makeKey } from '../../shell/screens';

  let { backend, workspaceId }: ScreenProps = $props();
  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();

  // A workflow row opens its screen (SM-NS-5).
  function open(row: { id: string; title: string }): void {
    if (app !== null) openScreen(app, backend.handle.id, workspaceId, makeKey('workflow', row.id), { title: row.title });
  }
</script>

<LoadedList {backend} {workspaceId} heading={t('tree-workflows')} load={() => loadWorkflows(backend.handle, workspaceId ?? '')} onopen={open} icon="keptcrew" />
