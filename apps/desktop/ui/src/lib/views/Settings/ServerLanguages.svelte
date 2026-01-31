<script lang="ts">
  // SM-LG-7: a personal server's languages, SM-LG-2's content bound to that server's backend. The instance
  // is the server's backend id; the tab's title names the server.
  import { getApp } from '../../shell/context';
  import type { ScreenProps } from '../../shell/screens';
  import Language from './Language.svelte';

  let { backend, instance }: ScreenProps = $props();
  const app = getApp();
  const bound = $derived((instance === null ? undefined : app.stateFor(instance)) ?? backend);
</script>

<Language backend={bound} />
