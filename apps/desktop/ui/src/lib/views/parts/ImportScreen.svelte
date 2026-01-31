<script lang="ts">
  // Screen `import` (SM-NS-10). The folder is chosen on this computer; the draft the import returns opens.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-10
  import { onMount } from 'svelte';

  import { openScreen } from '../../shell/open-item';
  import type { ScreenProps } from '../../shell/screens';
  import { appOrNull } from '../fixed/util';
  import { draftScreenKey, ImportController } from './import.svelte';
  import ImportView from './ImportView.svelte';

  let { backend, workspaceId }: ScreenProps = $props();
  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const c = new ImportController(backend.handle, app?.local.handle ?? backend.handle, workspaceId ?? '');

  onMount(() => {
    void c.load();
  });

  async function run(): Promise<void> {
    const ref = await c.run();
    if (ref !== null && app !== null) openScreen(app, backend.id, workspaceId, draftScreenKey(ref));
  }
</script>

<ImportView {backend} {c} onimport={() => void run()} />
