<script lang="ts">
  // Screen `email:<email link id>` (SM-NS-12.1). Opening it marks the link looked at.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-12
  import { onMount } from 'svelte';

  import { openScreen } from '../../shell/open-item';
  import { makeKey, type ScreenProps } from '../../shell/screens';
  import { appOrNull } from '../fixed/util';
  import EmailListView from './EmailListView.svelte';
  import { EmailListController } from './mail-controller.svelte';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const linkId = instance ?? '';
  const c = new EmailListController(backend.handle, workspaceId ?? '', linkId);

  onMount(() => {
    void c.load();
  });

  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();

  function open(messageRef: string): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey('email-message', `${linkId}:${messageRef}`), { title: c.messages.find((m) => m.message_ref === messageRef)?.subject ?? '' });
  }
</script>

<EmailListView {backend} {c} onopen={open} />
