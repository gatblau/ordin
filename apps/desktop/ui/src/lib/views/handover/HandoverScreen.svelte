<script lang="ts">
  // Screen `handover:<handover id>` (SM-NS-8). The instance is the handover id; the screen reads it on
  // opening. Accepting opens the workspace the backend returns.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-8
  import { onMount } from 'svelte';

  import { makeKey, type ScreenProps } from '../../shell/screens';
  import { ScreenAreaState } from '../../shell/screen-area.svelte';
  import { appOrNull } from '../fixed/util';
  import { HandoverController } from './handover-controller.svelte';
  import HandoverView from './HandoverView.svelte';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const id = instance ?? '';
  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();
  const c = new HandoverController(backend.handle, id, (opened) => app?.shell.openWorkspaceTab(backend.id, opened));

  onMount(() => {
    void c.load();
  });

  function close(): void {
    app?.screens.close(ScreenAreaState.scope(backend.id, workspaceId), makeKey('handover', id));
  }
</script>

<HandoverView {backend} {workspaceId} {c} onclose={close} />
