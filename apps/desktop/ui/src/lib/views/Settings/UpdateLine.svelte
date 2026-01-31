<script lang="ts">
  // The update line at the top of *Needs you* (SM-UP-3, SM-UP-7): not a pop-up, dialog or notification.
  // Registered as a Needs-you slot; drawn for the local backend only.
  import { answers } from '../../backend/groups';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { updateStoreFor, type UpdateStore } from './updates.svelte';

  interface Props {
    backend: BackendState;
    store?: UpdateStore;
  }
  let { backend, store: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const store = given ?? updateStoreFor(backend.handle);
  const offered = $derived(backend.kind === 'local' && answers(backend, 'setup.update_offer'));
  const line = $derived(store.line);

  // After the local backend loads (SM-UP-3).
  $effect(() => {
    if (offered && backend.loaded) void store.start();
  });
</script>

{#if offered && line !== null}
  <div class="update-line" role="region" aria-label={t(line.kind === 'ready' ? 'update-ready-line' : 'update-ask-administrator', { version: line.version })}>
    <p>
      {#if store.restarting}{t('update-restarting')}{:else if line.kind === 'ready'}{t('update-ready-line', { version: line.version })}{:else}{t('update-ask-administrator', { version: line.version })}{/if}
    </p>
    {#if line.kind === 'ready'}
      <button type="button" disabled={store.restarting} onclick={() => void store.restart()}>{t('update-restart')}</button>
    {/if}
    <button type="button" disabled={store.restarting} onclick={() => store.notNow()}>{t('update-later')}</button>
    <ErrorLine error={store.error} onretry={() => void store.restart()} />
  </div>
{/if}

<style>
  .update-line {
    padding: var(--space-sm);
    border-block-end: 1px solid var(--color-line);
  }
</style>
