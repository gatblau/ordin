<script lang="ts">
  // `slack_list`: sender, time, text and thread-reply count of the linked channel's messages;
  // `slack-message-instruction-note` on a message that tries to instruct the agent. **Open in Slack** opens
  // the message's permalink through `app.open_slack_link`: only a link from the core, never a link inside
  // message text, and only `https://<name>.slack.com/archives/…` (ADR-102).
  //
  // spec: ordin-desktop/components/ScreensModule#UI_LINK_REFUSED
  import { formatDate, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { ServiceError, SlackMessageOut } from '../../service-api/types.gen';
  import { getApp } from '../../shell/context';
  import Icon from '../../icons/Icon.svelte';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import { askOrdin, canAsk } from '../ask';
  import { isSlackPermalink, openInSlack } from '../slack-link';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'slack_list'> = $props();
  const app = getApp();

  let messages = $state<SlackMessageOut[]>([]);
  let cursor = $state<string | null>(null);
  let loaded = $state(false);
  let error = $state<ServiceError | null>(null);

  async function load(from: string | null): Promise<void> {
    if (!backend.groups.has('slack')) {
      loaded = true;
      return;
    }
    const out = await backend.handle.call('slack.messages', { link_id: block.slack_link_id, ws: workspaceId, ...(from !== null ? { cursor: from } : {}) });
    loaded = true;
    if (out.kind === 'ok') {
      messages = from === null ? out.result.items : [...messages, ...out.result.items];
      cursor = out.result.next_cursor ?? null;
      error = null;
    } else {
      error = out.error;
    }
  }
  $effect(() => {
    void load(null);
  });

  // A thread reply carries the `ts` of its thread's first message in `thread_ts`.
  const repliesTo = (message: SlackMessageOut): number => messages.filter((m) => m.thread_ts === message.ts && m.ts !== message.ts).length;
  const topLevel = $derived(messages.filter((m) => m.thread_ts === undefined || m.thread_ts === null || m.thread_ts === m.ts));

  async function reply(message: SlackMessageOut): Promise<void> {
    if (!canAsk(backend)) return;
    const text = `${t('block-reply-text', { sender: message.sender_name, channel: message.channel_name })}\n${t('block-reply-reference', { id: message.ts })}`;
    const out = await askOrdin(backend, workspaceId, text);
    error = out.kind === 'error' ? out.error : null;
  }

  async function open(message: SlackMessageOut): Promise<void> {
    error = await openInSlack(app, backend.id, message.permalink);
  }
</script>

<div class="block-slack">
  {#if loaded && messages.length === 0 && error === null}<p>{t('block-no-messages')}</p>{/if}
  <ul>
    {#each topLevel as message (message.ts)}
      <li>
        <p class="meta"><strong>{message.sender_name}</strong> · {formatDate(message.sent_at)}, {formatTime(message.sent_at)}</p>
        <p class="text">{message.text}</p>
        {#if repliesTo(message) > 0}<p class="meta">{t('block-thread-replies', { count: repliesTo(message) })}</p>{/if}
        {#if message.asks_agent_to_act}<p class="note"><span aria-hidden="true">ⓘ</span> {t('slack-message-instruction-note')}</p>{/if}
        <button type="button" disabled={!canAsk(backend)} onclick={() => void reply(message)}>{t('block-reply-with-ordin')}</button>
        {#if isSlackPermalink(message.permalink)}
          <button type="button" onclick={() => void open(message)}><Icon name="open" />{t('block-open-in-slack')}</button>
        {/if}
      </li>
    {/each}
  </ul>
  {#if cursor !== null}<button type="button" onclick={() => void load(cursor)}>{t('block-show-more')}</button>{/if}
  <ErrorLine {error} />
</div>

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  /* One row per item (the prototype's `.librow`; scenes `mailRead`, `tgRead`, `skChannel`, `library`, `know`). */
  ul > li {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    padding: 10px var(--space-12);
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .meta,
  .note {
    color: var(--color-text-muted);
    font-size: 12px;
  }
</style>
