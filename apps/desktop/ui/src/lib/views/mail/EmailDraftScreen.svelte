<script lang="ts">
  // Screen `email-draft:<draft id>` (SM-NS-12.3). The agent that drafted it is named from `agents.list`
  // when the backend answers it; otherwise the general words are drawn.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-12
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { ScreenAreaState } from '../../shell/screen-area.svelte';
  import { makeKey, type ScreenProps } from '../../shell/screens';
  import { appOrNull } from '../fixed/util';
  import EmailDraftView from './EmailDraftView.svelte';
  import { EmailDraftController } from './mail-controller.svelte';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const id = instance ?? '';
  const c = new EmailDraftController(backend.handle, id);
  let agentName = $state<string | null>(null);

  onMount(() => {
    void c.load().then(async () => {
      const by = c.draft?.drafted_by;
      if (by === undefined || by.kind !== 'agent_instance' || workspaceId === null || !answers(backend, 'agents.list')) return;
      const out = await backend.handle.call('agents.list', { workspace_id: workspaceId });
      if (out.kind === 'ok') agentName = out.result.find((a) => a.instance.id === by.agent_instance_id)?.instance.display_name ?? null;
    });
  });

  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();

  function closeTab(): void {
    app?.screens.close(ScreenAreaState.scope(backend.id, workspaceId), makeKey('email-draft', id));
  }
</script>

<EmailDraftView {backend} {c} {agentName} ondiscarded={closeTab} />
