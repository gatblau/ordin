<script lang="ts">
  // `email_list`: sender (with **Outside the company**, a word and an icon), subject, date and attachments,
  // from the Mail Service's data. The block carries message references only; nothing about a message comes
  // from the block.
  import type { CommandOutcome } from '../../backend/backend';
  import { formatDate, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { MessageSummary, ServiceError } from '../../service-api/types.gen';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import AskButton from '../AskButton.svelte';
  import type { KindProps } from '../types';
  import { getApp } from '../../shell/context';
  import { openScreen } from '../../shell/open-item';
  import { SCREEN_IDS } from '../../shell/screens';

  let { backend, workspaceId, block }: KindProps<'email_list'> = $props();
  const app = getApp();

  let items = $state<MessageSummary[]>([]);
  let loaded = $state(false);
  let error = $state<ServiceError | null>(null);

  $effect(() => {
    const wanted = new Set(block.messages);
    loaded = false;
    if (!backend.groups.has('mail')) {
      loaded = true;
      return;
    }
    let current = true;
    void (async () => {
      let cursor: string | null = null;
      const found: MessageSummary[] = [];
      type Page = { items: MessageSummary[]; next_cursor?: string | null };
      // The referenced messages are read from the link's list a page at a time until all are found.
      for (let page = 0; page < 20 && found.length < wanted.size; page++) {
        const out: CommandOutcome<Page> = await backend.handle.call('mail.list_messages', { workspace_id: block.workspace_id, email_link_id: block.email_link_id, ...(cursor !== null ? { cursor } : {}) });
        if (!current) return;
        if (out.kind !== 'ok') {
          error = out.error;
          break;
        }
        found.push(...out.result.items.filter((m) => wanted.has(m.message_ref)));
        cursor = out.result.next_cursor ?? null;
        if (cursor === null) break;
      }
      if (current) {
        items = block.messages.flatMap((ref) => found.filter((m) => m.message_ref === ref));
        loaded = true;
      }
    })();
    return () => {
      current = false;
    };
  });

  const senderName = (m: MessageSummary): string => m.from.name ?? m.from.address;
</script>

<div class="block-email-list">
  {#if loaded && items.length === 0 && error === null}<p>{t('block-cannot-show')}</p>{/if}
  <ul>
    {#each items as message (message.message_ref)}
      <li>
        <p class="sender">
          <strong>{senderName(message)}</strong>
          {#if message.from.outside_company}<span class="outside"><span aria-hidden="true">⚠</span> {t('approval-outside-company')}</span>{/if}
        </p>
        <p class="subject">{message.subject}</p>
        <p class="meta">{formatDate(message.date)}, {formatTime(message.date)}</p>
        {#if message.attachment_names.length > 0}
          <p class="meta">{t('block-attachments')}: {message.attachment_names.join(', ')}</p>
        {/if}
      </li>
    {/each}
  </ul>
  <div class="actions">
    <button type="button" onclick={() => openScreen(app, backend.id, workspaceId, SCREEN_IDS.email)}>{t('card-open')}</button>
    <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: t('tree-email') })} />
  </div>
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
  .outside {
    color: var(--color-waiting-text);
    font-weight: 600;
  }
  .subject {
    overflow-wrap: anywhere;
  }
  .meta {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .actions {
    display: flex;
    gap: var(--space-sm);
  }
</style>
