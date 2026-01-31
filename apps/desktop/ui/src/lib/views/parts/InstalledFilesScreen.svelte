<script lang="ts">
  // Screen `installed-files:<agent instance id>` (SM-NS-11).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-11
  import { onMount } from 'svelte';

  import { ScreenAreaState } from '../../shell/screen-area.svelte';
  import { makeKey, type ScreenProps } from '../../shell/screens';
  import { appOrNull } from '../fixed/util';
  import { InstalledFilesController } from './installed-files.svelte';
  import InstalledFilesView from './InstalledFilesView.svelte';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const c = new InstalledFilesController(backend.handle, instance ?? '');
  const agent = appOrNull()?.screens.tabs(ScreenAreaState.scope(backend.id, workspaceId)).find((tab) => tab.key === makeKey('installed-files', instance))?.title ?? '';

  onMount(() => {
    void c.load();
  });
</script>

<InstalledFilesView {backend} {c} {agent} />
