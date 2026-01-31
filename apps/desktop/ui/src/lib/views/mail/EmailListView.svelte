<script lang="ts">
  // The messages of one email link (SM-NS-12.1), newest first: who it is from, **Outside the company** in
  // word and icon, the subject, the date and the attachment names; a row opens the message. Mail text is
  // other people's text, drawn as text nodes.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-12
  import { formatDate, formatList, formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { who, type EmailListController } from './mail-controller.svelte';

  interface Props {
    backend: BackendState;
    c: EmailListController;
    onopen?: (messageRef: string) => void;
  }
  let { backend, c, onopen }: Props = $props();
</script>

<section class="email-list" aria-labelledby="email-list-heading">
  <h2 id="email-list-heading" tabindex="-1" data-region-focus>{c.link?.display_name ?? t('tree-email')}</h2>
  <p class="muted small">{t('email-link-note')}</p>
  <ErrorLine error={c.error} onretry={() => void c.load()} />
  {#if c.loaded && c.messages.length === 0 && c.error === null}
    <p>{t('email-list-empty')}</p>
  {/if}
  <!-- One row per message, the whole row opening it (the prototype's `.libl` / `.librow`, scene `mailRead`). -->
  <ul class="list-rows">
    {#each c.messages as message (message.message_ref)}
      <li>
        <button type="button" class="list-row" onclick={() => onopen?.(message.message_ref)}>
          <span class="icon-tile"><Icon name="mail" /></span>
          <span class="grow">
            <b>{who(message.from)}</b>
            {#if message.from.outside_company}<span class="state-label waiting"><span aria-hidden="true">↗</span>{t('approval-outside-company')}</span>{/if}
            <small>{message.subject} · {formatDate(message.date)}, {formatTime(message.date)}{#if message.attachment_names.length > 0}{' · '}{formatList(message.attachment_names)}{/if}</small>
          </span>
        </button>
      </li>
    {/each}
  </ul>
  {#if c.nextCursor !== null}
    <button type="button" onclick={() => void c.showMore()}>{t('fixed-show-more')}</button>
  {/if}
</section>

<style>
  .email-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .email-list > :is(h2, p) {
    margin: 0;
  }
  /* The row is a button: full width, start-aligned text, and the list row's border and padding. */
  button.list-row {
    inline-size: 100%;
    min-block-size: 0;
    text-align: start;
    font-size: 16px;
  }
  button.list-row:hover {
    border-color: var(--color-line-strong);
  }
</style>
