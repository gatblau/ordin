<script lang="ts">
  // Screen `email-message:<email link id>:<message ref>` (SM-NS-12.2).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-12
  import { onMount } from 'svelte';

  import type { ScreenProps } from '../../shell/screens';
  import EmailMessageView from './EmailMessageView.svelte';
  import { EmailMessageController, splitMessageInstance } from './mail-controller.svelte';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const c = new EmailMessageController(backend.handle, workspaceId ?? '', splitMessageInstance(instance ?? '').messageRef);

  onMount(() => {
    void c.load();
  });
</script>

<EmailMessageView {backend} workspaceId={workspaceId ?? ''} {c} />
