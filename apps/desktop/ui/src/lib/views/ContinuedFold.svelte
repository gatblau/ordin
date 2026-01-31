<script lang="ts">
  // A continued conversation (ADR-108, SM-CC-6): when the live conversation continues an earlier one, a fold
  // above its own messages reads `conv-continued-line` with **Show** / **Hide**. Showing it reads the
  // conversation it continues, then each earlier link, read-only, oldest first, 100 at a time with
  // **Show more**, with `conv-continued-from` between two conversations. Text is drawn as text.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-CC-6
  import { formatDate } from '../format/intl';
  import { t } from '../i18n/fluent';
  import type { ConversationSummary } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import ErrorLine from './ErrorLine.svelte';
  import { EarlierReadModel } from './earlier.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    /** The live conversation, with `continues` set. */
    live: ConversationSummary;
    /** The summaries the list holds, to count the messages of the conversations continued. */
    known: readonly ConversationSummary[];
    model?: EarlierReadModel;
  }
  let { backend, workspaceId, live, known, model: given }: Props = $props();

  // svelte-ignore state_referenced_locally
  const model = given ?? new EarlierReadModel(backend.handle, workspaceId, live.continues ?? '');
  let shown = $state(false);

  // The chain's `message_count` less the live conversation's own messages: the message count of the
  // conversation the live one continues is exactly that (its own count includes its chain).
  const count = $derived(known.find((c) => c.conversation_id === live.continues)?.message_count ?? 0);

  async function toggle(): Promise<void> {
    shown = !shown;
    if (shown && model.parts.length === 0) await model.load();
  }
</script>

<div class="continued-fold">
  <p>
    {t('conv-continued-line', { time: formatDate(live.started_at, { dateStyle: 'medium', timeStyle: 'short' }), count })}
    <button type="button" aria-expanded={shown} onclick={() => void toggle()}>{shown ? t('conv-continued-hide') : t('conv-continued-show')}</button>
  </p>
  {#if shown}
    <ErrorLine error={model.error} />
    {#each model.parts as part, i (part.conversation.conversation_id)}
      {#if i > 0}<p class="chain-from">{t('conv-continued-from', { time: formatDate(part.conversation.started_at, { dateStyle: 'medium', timeStyle: 'short' }) })}</p>{/if}
      <ol class="messages">
        {#each part.messages as m (m.seq)}
          <li data-author={m.author}>
            {#if m.block_id && model.moved.has(m.block_id)}
              <span class="moved">{t('conv-item-moved')}</span>
            {:else}
              <span class="text">{m.text}</span>
            {/if}
          </li>
        {/each}
      </ol>
    {/each}
    {#if model.hasMore}<button type="button" onclick={() => void model.showMore()}>{t('settings-show-more')}</button>{/if}
  {/if}
</div>
