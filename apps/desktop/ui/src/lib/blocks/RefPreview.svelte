<script lang="ts">
  // Reads a `ContentRef` through its command when the view opens and draws it (ResolvedView). A reference is
  // never opened as a URL.
  import type { BackendState } from '../store/backend-state.svelte';
  import type { ContentRef } from '../service-api/types.gen';
  import { resolveRef, type Resolved } from './refs';
  import ResolvedView from './ResolvedView.svelte';

  interface Props {
    backend: BackendState;
    reference: ContentRef;
    bound?: boolean;
    showTargets?: boolean;
    heading?: HTMLElement | null;
  }
  let { backend, reference, bound = false, showTargets = false, heading = $bindable(null) }: Props = $props();

  let resolved = $state<Resolved | null>(null);
  $effect(() => {
    const target = reference;
    resolved = null;
    let current = true;
    void resolveRef(backend, target).then((r) => {
      if (current) resolved = r;
    });
    return () => {
      current = false;
    };
  });
</script>

<ResolvedView {resolved} {bound} {showTargets} backend={backend.handle} bind:heading />
