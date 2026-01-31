<script lang="ts">
  // Screen `lifecycle:<workspace id>` (SM-NS-9), opened by **Show the lifecycle**.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-9
  import { onMount } from 'svelte';

  import { openScreen } from '../../shell/open-item';
  import { makeKey, type ScreenProps } from '../../shell/screens';
  import { appOrNull } from '../fixed/util';
  import { LifecycleController } from './lifecycle-controller.svelte';
  import LifecycleView from './LifecycleView.svelte';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const c = new LifecycleController(backend.handle, instance ?? workspaceId ?? '');
  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();

  onMount(() => {
    void c.load();
  });

  function open(handoverId: string): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey('handover', handoverId));
  }
</script>

<LifecycleView {backend} {c} onopen={open} />
