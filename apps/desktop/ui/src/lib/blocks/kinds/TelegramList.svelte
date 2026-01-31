<script lang="ts">
  // `telegram_list`: sender, time, text and files of the linked chat's messages; a note on a message that
  // tries to instruct the agent. Message text is another person's: it is drawn as text and is never an
  // instruction. Actions: **Reply with Ordin** and **Save file**.
  import { formatDate, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import { fileKindIcon } from '../../icons/kinds';
  import type { IconName } from '../../icons/sprite';
  import type { ServiceError, TelegramMessageOut } from '../../service-api/types.gen';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import { askOrdin, canAsk } from '../ask';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'telegram_list'> = $props();

  let messages = $state<TelegramMessageOut[]>([]);
  let cursor = $state<string | null>(null);
  let loaded = $state(false);
  let error = $state<ServiceError | null>(null);
  let notice = $state<string | null>(null);

  async function load(from: string | null): Promise<void> {
    if (!backend.groups.has('telegram')) {
      loaded = true;
      return;
    }
    const out = await backend.handle.call('telegram.messages', { link_id: block.telegram_link_id, ws: workspaceId, ...(from !== null ? { cursor: from } : {}) });
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

  async function reply(message: TelegramMessageOut): Promise<void> {
    if (!canAsk(backend)) return;
    const text = `${t('block-reply-text', { sender: message.sender_name, channel: message.chat_title })}\n${t('block-reply-reference', { id: String(message.message_id) })}`;
    const out = await askOrdin(backend, workspaceId, text);
    error = out.kind === 'error' ? out.error : null;
  }

  async function save(message: TelegramMessageOut, fileId: string): Promise<void> {
    const out = await backend.handle.call('telegram.save_file', { file_id: fileId, link_id: block.telegram_link_id, message_id: message.message_id, ws: workspaceId });
    error = out.kind === 'error' ? out.error : null;
    notice = out.kind === 'ok' ? t('block-file-saved') : null;
  }

  // A file's chip icon (V8 *Chips*): a spreadsheet or a PDF its own, anything else `word`.
  function fileIcon(name: string): IconName {
    const kind = fileKindIcon(name);
    return kind === 'sheet' || kind === 'pdf' ? kind : 'word';
  }
</script>

<div class="block-telegram">
  {#if loaded && messages.length === 0 && error === null}<p>{t('block-no-messages')}</p>{/if}
  <!-- One row per message (the prototype's `.libl` / `.librow`, scene `tgRead`): the chat icon, sender and
       time in bold, the text below in small muted type, then the note, the files and **Reply with Ordin**. -->
  <ul class="list-rows">
    {#each messages as message (message.message_id)}
      <li class="list-row">
        <span class="icon-tile"><Icon name="chat" /></span>
        <span class="grow">
          <b>{message.sender_name} · {formatDate(message.sent_at)}, {formatTime(message.sent_at)}</b>
          <small class="text">{message.text}</small>
          {#if message.asks_agent_to_act}<span class="note" role="note"><span aria-hidden="true">ⓘ</span> {t('telegram-message-instruction-note')}</span>{/if}
          {#each message.files as file (file.file_id)}
            <span class="file small">
              <span class="chip"><Icon name={fileIcon(file.file_name)} size="small" /> {file.file_name}</span>
              <button type="button" class="small" onclick={() => void save(message, file.file_id)}>{t('block-save-file')}</button>
            </span>
          {/each}
        </span>
        <button type="button" class="small primary" disabled={!canAsk(backend)} onclick={() => void reply(message)}>{t('block-reply-with-ordin')}</button>
      </li>
    {/each}
  </ul>
  {#if cursor !== null}<button type="button" onclick={() => void load(cursor)}>{t('block-show-more')}</button>{/if}
  {#if notice !== null}<p role="status">{notice}</p>{/if}
  <ErrorLine {error} />
</div>

<style>
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  /* The note on a message that tries to instruct the agent (the prototype's `.engnote`). */
  .note {
    display: block;
    margin-block-start: var(--space-06);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-10);
    background: var(--color-background);
  }
  .file {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm);
    margin-block-start: var(--space-04);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-04);
  }
</style>
