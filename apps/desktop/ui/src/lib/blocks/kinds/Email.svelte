<script lang="ts">
  // `email`: one message, or one draft with who drafted it. A message offers **Reply with Ordin**, **Save
  // attachment** and **Ask Ordin about this**; a draft offers **Send…** (which raises an approval card; this
  // block never sends), **Edit**, **Save to mail drafts** and **Discard**. Mail text is drawn as text. An
  // attachment draws `sheet`, `pdf` or else `word` by its name (V8 *Chips*).
  import { formatDate, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import { fileKindIcon, fileKindTint } from '../../icons/kinds';
  import type { IconName } from '../../icons/sprite';
  import type { EmailDraft, EmailRecipient, MessageView, ServiceError } from '../../service-api/types.gen';
  import { getApp } from '../../shell/context';
  import { openScreen } from '../../shell/open-item';
  import { SCREEN_IDS } from '../../shell/screens';
  import ConfirmDialog from '../../views/ConfirmDialog.svelte';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import AskButton from '../AskButton.svelte';
  import { askOrdin, canAsk, isWaitingForServer } from '../ask';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'email'> = $props();
  const app = getApp();
  const attachmentIcon = (name: string): IconName => {
    const kind = fileKindIcon(name);
    return kind === 'sheet' || kind === 'pdf' ? kind : 'word';
  };

  let message = $state<MessageView | null>(null);
  let draft = $state<EmailDraft | null>(null);
  let error = $state<ServiceError | null>(null);
  let notice = $state<string | null>(null);
  let confirmDiscard = $state(false);
  let busy = $state(false);

  $effect(() => {
    const target = block;
    message = null;
    draft = null;
    if (!backend.groups.has('mail')) return;
    let current = true;
    void (async () => {
      if (target.of === 'message') {
        const out = await backend.handle.call('mail.open_message', { message_ref: target.message_id, workspace_id: target.workspace_id });
        if (!current) return;
        if (out.kind === 'ok') message = out.result;
        else error = out.error;
      } else {
        const out = await backend.handle.call('mail.get_draft', { id: target.draft_id });
        if (!current) return;
        if (out.kind === 'ok') draft = out.result;
        else error = out.error;
      }
    })();
    return () => {
      current = false;
    };
  });

  const who = (r: EmailRecipient): string => r.name ?? r.address;
  let agentName = $state<string | null>(null);
  $effect(() => {
    const by = draft?.drafted_by;
    agentName = null;
    if (by === undefined || by.kind !== 'agent_instance' || !backend.groups.has('agents')) return;
    const id = by.agent_instance_id;
    let current = true;
    void backend.handle.call('agents.list', { workspace_id: workspaceId }).then((out) => {
      if (current && out.kind === 'ok') agentName = out.result.find((a) => a.instance.id === id)?.instance.display_name ?? null;
    });
    return () => {
      current = false;
    };
  });
  const draftedBy = $derived.by(() => {
    if (draft === null || draft.drafted_by.kind === 'person') return null;
    if (draft.drafted_by.kind === 'agent_instance' && agentName !== null) return t('block-email-drafted-by', { agent: agentName });
    return t('block-email-drafted-by-an-agent');
  });
  const editable = $derived(draft !== null && (draft.state === 'draft' || draft.state === 'waiting_for_you'));

  async function run(command: 'mail.send' | 'mail.save_to_mail_drafts' | 'mail.discard_draft', done: string | null): Promise<void> {
    if (draft === null || busy || isWaitingForServer(backend)) return;
    busy = true;
    const out = await backend.handle.call(command, { id: draft.id });
    busy = false;
    error = out.kind === 'error' ? out.error : null;
    notice = out.kind === 'ok' ? done : null;
  }

  async function saveAttachment(index: number): Promise<void> {
    if (message === null) return;
    const out = await backend.handle.call('mail.save_attachment', { index, message_ref: message.message_ref, workspace_id: workspaceId });
    error = out.kind === 'error' ? out.error : null;
    notice = out.kind === 'ok' ? t('block-file-saved') : null;
  }

  async function reply(): Promise<void> {
    if (message === null || !canAsk(backend)) return;
    const sender = who(message.from);
    const out = await askOrdin(backend, workspaceId, `${t('block-reply-email-text', { sender })}\n${t('block-reply-reference', { id: message.message_ref })}`);
    error = out.kind === 'error' ? out.error : null;
  }
</script>

<div class="block-email">
  {#if message !== null}
    <article>
      <h4>{message.subject}</h4>
      <p class="meta">
        <strong>{who(message.from)}</strong>
        {#if message.from.outside_company}<span class="outside"><span aria-hidden="true">⚠</span> {t('approval-outside-company')}</span>{/if}
        · {formatDate(message.date)}, {formatTime(message.date)}
      </p>
      <p class="text">{message.text}</p>
      {#if message.truncated}<p class="meta">{t('approval-bound-version-cut')}</p>{/if}
      {#each message.attachments as attachment (attachment.index)}
        <p><span class="icon-tile" data-tint={fileKindTint(attachmentIcon(attachment.file_name))}><Icon name={attachmentIcon(attachment.file_name)} size="small" /></span> {attachment.file_name} <button type="button" onclick={() => void saveAttachment(attachment.index)}>{t('block-save-attachment')}</button></p>
      {/each}
    </article>
    <div class="actions">
      <button type="button" disabled={!canAsk(backend)} onclick={() => void reply()}>{t('block-reply-with-ordin')}</button>
      <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: message.subject })} />
    </div>
  {:else if draft !== null}
    <article>
      <h4>{draft.subject}</h4>
      {#if draftedBy !== null}<p class="meta">{draftedBy}</p>{/if}
      <p class="meta">{t('block-email-to')}: {draft.to.join(', ')}</p>
      <p class="text">{draft.body}</p>
    </article>
    {#if editable}
      <div class="actions">
        <button type="button" disabled={busy || isWaitingForServer(backend)} onclick={() => void run('mail.send', null)}>{t('block-email-send')}</button>
        <button type="button" onclick={() => openScreen(app, backend.id, workspaceId, SCREEN_IDS.email)}>{t('block-email-edit')}</button>
        <button type="button" disabled={busy} onclick={() => void run('mail.save_to_mail_drafts', t('block-email-saved-to-drafts'))}>{t('block-email-save-to-drafts')}</button>
        <button type="button" class="danger" disabled={busy} onclick={() => (confirmDiscard = true)}>{t('block-email-discard')}</button>
      </div>
    {/if}
  {:else if error === null}
    <p role="status" aria-busy="true"></p>
  {/if}
  {#if notice !== null}<p role="status">{notice}</p>{/if}
  <ErrorLine {error} />
</div>

{#if confirmDiscard}
  <ConfirmDialog
    title={t('block-email-discard-title')}
    verb={t('block-email-discard')}
    onconfirm={() => {
      confirmDiscard = false;
      void run('mail.discard_draft', null);
    }}
    oncancel={() => (confirmDiscard = false)}
  />
{/if}

<style>
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .meta {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .outside {
    color: var(--color-waiting-text);
    font-weight: 600;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
</style>
