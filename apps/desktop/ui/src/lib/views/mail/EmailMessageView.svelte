<script lang="ts">
  // One email (SM-NS-12.2), in the order the spec gives: subject, From (with **Outside the company**), To,
  // Cc, date, the hidden-text and cut notes, the text as paragraphs, the attachments with **Save
  // attachment**, then **Reply with Ordin** and **Ask Ordin about this**. The text is never markup.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-12
  import AskButton from '../../blocks/AskButton.svelte';
  import { canAsk } from '../../blocks/ask';
  import { formatDate, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import { fileKindIcon } from '../../icons/kinds';
  import type { IconName } from '../../icons/sprite';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { paragraphs, who, type EmailMessageController } from './mail-controller.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    c: EmailMessageController;
  }
  let { backend, workspaceId, c }: Props = $props();

  // An attachment's icon (V8 *Chips*): a spreadsheet or a PDF its own, anything else `word`.
  function attachmentIcon(name: string): IconName {
    const kind = fileKindIcon(name);
    return kind === 'sheet' || kind === 'pdf' ? kind : 'word';
  }
</script>

<section class="email-message" aria-labelledby="email-message-heading">
  {#if c.message !== null}
    {@const m = c.message}
    <h2 id="email-message-heading" tabindex="-1" data-region-focus>{m.subject}</h2>
    <!-- From, To, Cc and the date as one two-column list (the prototype's `.anat.mailhd`, scenes `mailDraft`, `mailSteer`). -->
    <dl class="header">
      <dt>{t('block-email-from')}:</dt>
      <dd>
        {who(m.from)}
        {#if m.from.outside_company}<span class="state-label waiting"><span aria-hidden="true">↗</span>{t('approval-outside-company')}</span>{/if}
      </dd>
      <dt>{t('block-email-to')}:</dt>
      <dd>{m.to.map(who).join(', ')}</dd>
      {#if m.cc.length > 0}
        <dt>{t('email-cc')}:</dt>
        <dd>{m.cc.map(who).join(', ')}</dd>
      {/if}
      <dd class="date">{formatDate(m.date)}, {formatTime(m.date)}</dd>
    </dl>
    {#if m.hidden_text}<p role="note" class="note"><span aria-hidden="true">⚠</span> {t('email-hidden-text')}</p>{/if}
    <!-- The text on a surface of its own (the prototype's `.mdv.mailbody`). -->
    <article class="body">
      {#each paragraphs(m.text) as paragraph, i (i)}<p class="text">{paragraph}</p>{/each}
    </article>
    {#if m.truncated}<p class="muted small">{t('email-cut')}</p>{/if}
    {#each m.attachments as attachment (attachment.index)}
      <p class="attachment small">
        <span class="chip"><Icon name={attachmentIcon(attachment.file_name)} size="small" /> {attachment.file_name}</span>
        <button type="button" class="small" onclick={() => void c.saveAttachment(attachment.index)}>{t('block-save-attachment')}</button>
      </p>
    {/each}
    {#if c.saved}<p role="status">{t('block-file-saved')}</p>{/if}
    <div class="actions">
      <button type="button" class="small primary" disabled={!canAsk(backend)} onclick={() => void c.replyWithOrdin(backend, (sender, ref) => `${t('block-reply-email-text', { sender })}\n${t('block-reply-reference', { id: ref })}`)}>
        {t('block-reply-with-ordin')}
      </button>
      <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: m.subject })} />
    </div>
  {/if}
  <ErrorLine error={c.error} onretry={() => void c.load()} />
</section>

<style>
  .email-message {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    max-inline-size: 1100px;
  }
  .email-message > :is(h2, p) {
    margin: 0;
  }
  .header {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 2px var(--space-10);
    margin: 0;
    font-size: 14px;
  }
  .header dt {
    font-weight: 700;
  }
  .header dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .header .date {
    grid-column: 2;
  }
  .header .state-label {
    margin-inline-start: var(--space-04);
  }
  .body {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    padding: var(--space-14) 18px;
    line-height: 1.55;
    display: flex;
    flex-direction: column;
    gap: var(--space-10);
  }
  .text {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .note {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-10);
    background: var(--color-background);
  }
  .attachment {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }
  .attachment .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-04);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
  /* **Ask Ordin about this** is drawn by the shared ask button; here it takes the small size beside **Reply with Ordin**. */
  .actions :global(button) {
    padding: 3px var(--space-sm);
    min-block-size: 28px;
    font-size: 13px;
  }
</style>
