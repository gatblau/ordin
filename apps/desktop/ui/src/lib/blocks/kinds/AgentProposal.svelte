<script lang="ts">
  // `agent_proposal`: an agent Ordin suggests, with the reason. Its screen is the agent library with the
  // proposed agent first, marked and chosen, where the person adds it with **Add it** (SM-AL-1, SM-AL-5).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-AL-1
  // spec: ordin-desktop/components/ScreensModule#SM-AL-5
  import Library from '../../views/fixed/Library.svelte';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'agent_proposal'> = $props();
</script>

<div class="block-agent-proposal">
  <p class="reason">{block.reason}</p>
  <Library {backend} {workspaceId} instance={block.agent} />
</div>

<style>
  .reason {
    overflow-wrap: anywhere;
  }
</style>
