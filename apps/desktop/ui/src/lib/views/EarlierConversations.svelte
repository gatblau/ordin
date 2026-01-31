<script lang="ts">
  // Earlier conversations (ADR-108): each earlier conversation's first request, date and number of
  // messages, one entry per chain. One opens read-only on its own tab (EarlierConversation). Each entry
  // draws `chat` before its words (V8 *Lists*, an earlier conversation).
  import { onMount } from 'svelte';

  import { formatDate } from '../format/intl';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import { getApp } from '../shell/context';
  import { openScreen } from '../shell/open-item';
  import { makeKey } from '../shell/screens';
  import type { ScreenProps } from '../shell/screens';
  import ErrorLine from './ErrorLine.svelte';
  import { EarlierListModel } from './earlier.svelte';

  interface Props extends ScreenProps {
    model?: EarlierListModel;
  }
  let { backend, workspaceId, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const app = given === undefined ? getApp() : null;
  // svelte-ignore state_referenced_locally
  const model = given ?? new EarlierListModel(backend.handle, workspaceId ?? '');
  const name = (first: string | null | undefined): string => first ?? t('conv-picker-new');

  onMount(() => void model.load());

  function read(id: string, title: string): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey('earlier-conversation', id), { title });
  }
</script>

<section class="earlier" aria-labelledby="earlier-heading">
  <h2 id="earlier-heading" tabindex="-1" data-region-focus>{t('conv-earlier-name')}</h2>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
  <ul>
    {#each model.entries as entry (entry.head.conversation_id)}
      <li>
        <button type="button" onclick={() => read(entry.head.conversation_id, name(entry.head.first_request))}>
          <span class="icon-tile"><Icon name="chat" /></span>
          <span class="first">{name(entry.chain[0]?.first_request ?? entry.head.first_request)}</span>
          <span class="meta">{t('conv-picker-date', { date: formatDate(entry.head.started_at, { dateStyle: 'medium' }), count: entry.messageCount })}</span>
        </button>
      </li>
    {/each}
  </ul>
  {#if model.next !== null}<button type="button" onclick={() => void model.more()}>{t('settings-show-more')}</button>{/if}
</section>

<style>
  /* One row per conversation, drawn as the documents list's rows (scene `docs`, `table.files`):
     Small text, a divider under each row and under the list's top edge, the row's own words in body colour. */
  .earlier ul {
    list-style: none;
    margin: 0;
    padding: 0;
    border-block-start: 1px solid var(--color-line);
    font-size: 13px;
  }
  .earlier li > button {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    inline-size: 100%;
    min-height: 0;
    padding: 5px var(--space-06);
    border: 0;
    border-block-end: 1px solid var(--color-line);
    border-radius: 0;
    background: none;
    color: var(--color-text);
    font-size: 13px;
    text-align: start;
  }
  .earlier .first {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .earlier .meta {
    flex: none;
  }
</style>
