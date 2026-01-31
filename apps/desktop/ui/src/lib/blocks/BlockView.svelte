<script lang="ts">
  // How the conversation draws a block (registered with `registerBlockView`). A kind that appears "in full"
  // is drawn by BlockBody; a kind that appears on a screen is a one-line card. An unknown kind is not drawn:
  // the diagnostic `UI_BLOCK_KIND_UNKNOWN` is recorded (no content, only the kind) and nothing broken shows
  // (11 §5.15).
  //
  // spec: ordin-desktop/components/ScreensModule#UI_BLOCK_KIND_UNKNOWN
  import { getApp } from '../shell/context';
  import type { BlockViewProps } from '../shell/screens';
  import BlockBody from './BlockBody.svelte';
  import OneLineCard from './OneLineCard.svelte';
  import { isKnownKind, placementOf } from './types';

  let props: BlockViewProps = $props();
  const app = getApp();

  const known = $derived(isKnownKind(props.block.kind));
  // Recorded once, when the view is made (the kind of a shown block does not change).
  // svelte-ignore state_referenced_locally
  if (!isKnownKind(props.block.kind)) app.diagnostics.record('UI_BLOCK_KIND_UNKNOWN', String(props.block.kind).slice(0, 40), props.backend.id);
</script>

{#if known}
  {#if placementOf(props.block) === 'full'}
    <BlockBody {...props} />
  {:else}
    <OneLineCard {...props} />
  {/if}
{/if}
