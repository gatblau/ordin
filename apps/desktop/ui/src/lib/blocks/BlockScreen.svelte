<script lang="ts">
  // The screen of one block (`block:<block id>`): the block's full content under its title. The item is the
  // conversation's own; one that is not in the store (an earlier conversation's) is read with `ordin.block`.
  import { t } from '../i18n/fluent';
  import type { Block, ServiceError } from '../service-api/types.gen';
  import type { ScreenProps } from '../shell/screens';
  import ErrorLine from '../views/ErrorLine.svelte';
  import BlockBody from './BlockBody.svelte';
  import { isKnownKind } from './types';
  import { blockTitle } from './titles';

  let { backend, workspaceId, instance }: ScreenProps = $props();

  let fetched = $state<Block | null>(null);
  let error = $state<ServiceError | null>(null);
  const fromStore = $derived(workspaceId === null || instance === null ? undefined : backend.conversation.get(workspaceId)?.find((i) => i.block_id === instance)?.block);
  const block = $derived(fromStore ?? fetched);

  $effect(() => {
    if (fromStore !== undefined || workspaceId === null || instance === null || !backend.groups.has('ordin')) return;
    let current = true;
    void backend.handle.call('ordin.block', { block_id: instance, workspace_id: workspaceId }).then((out) => {
      if (!current) return;
      if (out.kind === 'ok') fetched = out.result.block;
      else error = out.error;
    });
    return () => {
      current = false;
    };
  });
</script>

<section class="block-screen">
  {#if block !== null && workspaceId !== null && instance !== null && isKnownKind(block.kind)}
    <h2 tabindex="-1">{blockTitle(block)}</h2>
    <BlockBody {backend} {workspaceId} blockId={instance} {block} />
  {:else if error === null}
    <p>{t('block-cannot-show')}</p>
  {/if}
  <ErrorLine {error} />
</section>
