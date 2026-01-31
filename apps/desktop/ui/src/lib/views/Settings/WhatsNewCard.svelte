<script lang="ts">
  // The What's new card at the top of *Needs you*, above the update line (SM-UP-4). It is not counted in the
  // waiting count and is not repeated as a notification.
  import { answers } from '../../backend/groups';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import Notes from './Notes.svelte';
  import { updateStoreFor, type UpdateStore } from './updates.svelte';

  interface Props {
    backend: BackendState;
    store?: UpdateStore;
  }
  let { backend, store: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const store = given ?? updateStoreFor(backend.handle);
  const offered = $derived(backend.kind === 'local' && answers(backend, 'setup.whats_new'));
  const card = $derived(store.card);

  $effect(() => {
    if (offered && backend.loaded) void store.start();
  });
</script>

{#if offered && card !== null}
  <section class="whats-new" aria-labelledby="whats-new-heading">
    <h3 id="whats-new-heading">{t('whats-new-title', { version: card.version })}</h3>
    <Notes notes={card.notes} />
    <button type="button" onclick={() => void store.gotIt()}>{t('whats-new-got-it')}</button>
    <ErrorLine error={store.whatsNewError} />
  </section>
{/if}

<style>
  .whats-new {
    padding: var(--space-sm);
    border-block-end: 1px solid var(--color-line);
  }
</style>
