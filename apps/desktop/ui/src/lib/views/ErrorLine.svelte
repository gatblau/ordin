<script lang="ts">
  // A command's `ServiceError` as its message and at most one action (11 §6, P7). Never the code. The
  // actions are Try again and Sign in again; there is no help request to anyone else (SM-ER-3).
  //
  // spec: ordin-desktop/components/ScreensModule#SVC_NOT_IN_THIS_VERSION
  import { t } from '../i18n/fluent';
  import type { ServiceError } from '../service-api/types.gen';

  interface Props {
    error: ServiceError | null;
    /** Name of the server when the failing command went to one (for `server-cannot-reach`). */
    serverName?: string;
    /** Re-sends the same command with a new request id (the `try_again` action). */
    onretry?: () => void;
    /** Opens Settings › Sign-ins at the account concerned (the `sign_in_again` action). */
    onsignin?: () => void;
    /** The action button's words when the screen names its own (first start's Try again, SM-FS-4). */
    actionLabel?: string;
  }

  let { error, serverName, onretry, onsignin, actionLabel }: Props = $props();

  const transport = $derived(error !== null && (error.code === 'UI_IPC_FAILED' || error.code === 'UI_COMMAND_TIMEOUT'));
  const newerVersion = $derived(error !== null && error.code === 'SVC_NOT_IN_THIS_VERSION');
  const sentence = $derived.by(() => {
    if (error === null) return '';
    if (newerVersion) return t('error-needs-newer-version');
    // SM-ER-1: on this computer the catalogue's own sentence, never a bare Try again.
    // spec: ordin-desktop/components/ScreensModule#SM-ER-1
    if (transport) return serverName !== undefined ? t('server-cannot-reach', { server: serverName }) : t(error.code === 'UI_COMMAND_TIMEOUT' ? 'error-ui-command-timeout' : 'error-ui-ipc-failed');
    return error.message;
  });
  const action = $derived.by(() => {
    if (error === null || newerVersion) return null;
    if (transport) return 'try_again';
    // SM-ER-3: an older backend may still name `send_to_it`; the screens draw no button for it.
    return error.action === 'try_again' || error.action === 'sign_in_again' ? error.action : null;
  });

  function press(): void {
    if (action === 'try_again') onretry?.();
    else if (action === 'sign_in_again') onsignin?.();
  }
</script>

{#if error !== null}
  <div class="error-line" role="alert">
    {#if sentence !== ''}<p class="error-sentence">{sentence}</p>{/if}
    {#if action !== null}
      <button type="button" onclick={press}>
        {actionLabel !== undefined ? actionLabel : action === 'try_again' ? t('error-try-again') : t('error-sign-in-again')}
      </button>
    {/if}
    {#if error.reference}
      <p class="error-reference">{t('error-reference', { reference: error.reference })}</p>
    {/if}
  </div>
{/if}
