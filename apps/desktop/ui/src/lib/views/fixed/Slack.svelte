<script lang="ts">
  // Slack under a workspace: one row per linked channel, "#name" with its new-message count; a channel in
  // `no_access` shows `slack-link-no-access` (ADR-102).
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { ScreenProps } from '../../shell/screens';
  import FixedList from './FixedList.svelte';
  import { SlackController } from './slack-controller.svelte';

  let { backend, workspaceId }: ScreenProps = $props();
  const c = new SlackController(backend.handle, workspaceId ?? '');
  onMount(() => {
    void c.loadLinks();
  });
  const rows = $derived(
    c.links.map((l) => ({
      id: l.link.id,
      title: `#${l.link.name}`,
      detail: l.link.state === 'no_access' ? t('slack-link-no-access', { channel: l.link.name }) : String(l.new_messages),
    })),
  );
</script>

<FixedList {backend} heading={t('tree-slack')} {rows} error={c.error} onretry={() => void c.loadLinks()} icon="chat" />
