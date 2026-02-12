<script lang="ts">
  // A button that sends one message to the conversation, such as **Ask Ordin about this**. Disabled while
  // it is in flight and while a server's connection is down; an error is shown as its one sentence.
  // An *Ask Ordin about …* label draws `chat` before its words (V8 *Actions*); any other label none.
  import { t } from '../i18n/fluent';
  import type { BackendState } from '../store/backend-state.svelte';
  import type { ContentRef, ServiceError } from '../service-api/types.gen';
  import Icon from '../icons/Icon.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { askOrdin, canAsk, isWaitingForServer } from './ask';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    /** Message id of the button's label. */
    labelId: string;
    /** The message the person sends (already in the active language). */
    text: string;
    attachments?: readonly ContentRef[];
  }
  let { backend, workspaceId, labelId, text, attachments = [] }: Props = $props();

  let busy = $state(false);
  let error = $state<ServiceError | null>(null);
  const offered = $derived(backend.groups.has('ordin'));
  const waiting = $derived(isWaitingForServer(backend));
  const asksAbout = $derived(labelId.includes('-ask-about-'));

  async function press(): Promise<void> {
    if (busy || !canAsk(backend)) return;
    busy = true;
    const out = await askOrdin(backend, workspaceId, text, attachments, asksAbout ? 'ask_about' : undefined);
    busy = false;
    error = out.kind === 'error' ? out.error : null;
  }
</script>

{#if offered}
  <button type="button" disabled={busy || waiting} title={waiting ? t('server-waiting-for', { server: backend.displayName }) : undefined} onclick={() => void press()}>
    {#if asksAbout}<Icon name="chat" />{/if}
    {t(labelId)}
  </button>
  <ErrorLine {error} onretry={() => void press()} />
{/if}
