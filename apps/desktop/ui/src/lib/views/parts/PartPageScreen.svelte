<script lang="ts">
  // Screen `part-page:<agent instance id>:<part id>` or `part-page:library:<part id>` (SM-NS-15).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-15
  import { onMount } from 'svelte';

  import type { ScreenProps } from '../../shell/screens';
  import InstalledPartPageView from './InstalledPartPageView.svelte';
  import LibraryPartPageView from './LibraryPartPageView.svelte';
  import { readLinksCode } from '../fixed/links-code';
  import { detailFor, InstalledPartPageController, LibraryPartPageController, parsePartPageInstance } from './part-page.svelte';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const target = parsePartPageInstance(instance ?? '');
  const installed = target.kind === 'installed' ? new InstalledPartPageController(backend.handle, target.agentInstanceId, target.partId) : null;
  const library = target.kind === 'library' ? new LibraryPartPageController(backend.handle, target.partId) : null;

  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));

  // SM-WR-14: the detail follows whether the workspace links code, read again on `workspace_changed`.
  async function loadInstalled(c: InstalledPartPageController): Promise<void> {
    await c.load(detailFor(await readLinksCode(backend.handle, workspaceId)));
  }

  onMount(() => {
    if (library !== null) void library.load();
  });
  $effect(() => {
    void revision;
    if (installed !== null) void loadInstalled(installed);
  });
</script>

{#if installed !== null && target.kind === 'installed'}
  <InstalledPartPageView {backend} {workspaceId} agentInstanceId={target.agentInstanceId} c={installed} />
{:else if library !== null}
  <LibraryPartPageView {backend} {workspaceId} c={library} />
{/if}
