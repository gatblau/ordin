<script lang="ts">
  // A checklist card that names its linked folder (SM-WR-13): the folder's Ready to publish list, drawn as
  // SM-SF-8 states, with **Publish checked items…**; a refusal draws its sentence in place of the list.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WR-13
  import { onMount } from 'svelte';

  import { openScreen } from '../shell/open-item';
  import { makeKey, SCREEN_IDS } from '../shell/screens';
  import type { BackendState } from '../store/backend-state.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { appOrNull } from '../views/fixed/util';
  import { ReadyListModel } from './ready-list.svelte';
  import ReadyChecklist from './ReadyChecklist.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    confluenceLinkId: string;
    model?: ReadyListModel;
  }
  let { backend, workspaceId, confluenceLinkId, model: given }: Props = $props();
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const model = given ?? new ReadyListModel(backend.handle, confluenceLinkId);

  onMount(() => {
    if (given === undefined) void model.load();
  });

  function published(approvalId: string): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.approval, approvalId));
  }
</script>

<ErrorLine error={model.error} onretry={() => void model.load()} />
{#if model.items !== null}
  <ReadyChecklist {backend} {confluenceLinkId} items={model.items} onpublished={published} />
{/if}
