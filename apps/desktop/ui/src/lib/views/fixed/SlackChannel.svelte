<script lang="ts">
  // A Slack channel linked to the workspace (instance = the link id): its messages as other people's text
  // (instruction note where a message asks the agent to act), the **Let Ordin look at new messages**
  // switch and **Remove from this workspace** after a confirmation.
  //
  // spec: ordin-desktop/components/ScreensModule#UI_LINK_REFUSED
  import { onMount } from 'svelte';

  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ScreenProps } from '../../shell/screens';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { openSlackPermalink } from './slack-link';
  import { SlackController } from './slack-controller.svelte';
  import { appOrNull } from './util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();
  const c = new SlackController(backend.handle, workspaceId ?? '');
  const linkId = instance ?? '';
  let confirming = $state(false);

  onMount(() => {
    void c.loadLinks();
    void c.loadMessages(linkId);
  });
  const summary = $derived(c.links.find((l) => l.link.id === linkId));
</script>

<section class="slack-channel" aria-labelledby="slack-channel-heading">
  <h2 id="slack-channel-heading" tabindex="-1" data-region-focus>#{summary?.link.name ?? ''}</h2>
  <ErrorLine error={c.error} />
  {#if summary}
    {#if summary.link.state === 'no_access'}<p>{t('slack-link-no-access', { channel: summary.link.name })}</p>{/if}
    <p class="muted small">{t('slack-link-kept-from', { date: formatDate(summary.kept_from) })}</p>
    {#if summary.older_skipped}<p class="muted small">{t('slack-link-older-not-read')}</p>{/if}
    <!-- The switch and its note in one bordered row (the prototype's `.lrow` with `.toggle`, scene `skChannel`). -->
    <div class="switch-row">
      <label>
        <input type="checkbox" checked={summary.link.ordin_reads} onchange={(e) => void c.setOrdinReads(linkId, e.currentTarget.checked)} />
        <b>{t('slack-ordin-reads')}</b>
      </label>
      <small>{t('slack-ordin-reads-note')}</small>
    </div>
  {/if}
  <!-- One row per message (the prototype's `.libl` / `.librow`). -->
  <ul class="list-rows">
    {#each c.messages as message (message.ts)}
      <li class="list-row">
        <span class="icon-tile"><Icon name="chat" /></span>
        <span class="grow">
          <b>{message.sender_name} · {formatDate(message.sent_at)}</b>
          <small class="message-text">{message.text}</small>
          {#if message.asks_agent_to_act}<span class="note" role="note">{t('slack-message-instruction-note')}</span>{/if}
        </span>
        <button type="button" class="small" onclick={() => void openSlackPermalink(backend.handle, message.permalink, app?.diagnostics)}>
          <Icon name="open" />
          {t('slack-open-in-slack')}
        </button>
      </li>
    {/each}
  </ul>
  <div><button type="button" class="danger small" onclick={() => (confirming = true)}><Icon name="remove" />{t('slack-remove-from-workspace')}</button></div>
  {#if confirming}
    <ConfirmDialog
      title={t('slack-remove-confirm')}
      verb={t('slack-remove-from-workspace')}
      onconfirm={() => void c.unlink(linkId).then(() => (confirming = false))}
      oncancel={() => (confirming = false)}
    />
  {/if}
</section>

<style>
  .slack-channel {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .slack-channel > :is(h2, p) {
    margin: 0;
  }
  .switch-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-06) var(--space-10);
    font-size: 14px;
  }
  .switch-row > * {
    flex: 1;
    min-inline-size: 160px;
  }
  .switch-row label {
    display: flex;
    align-items: center;
    gap: var(--space-06);
    font-size: 13px;
  }
  .switch-row small {
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .message-text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  /* The note on a message that reads like an instruction (the prototype's `.engnote`). */
  .note {
    display: block;
    margin-block-start: var(--space-06);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-10);
    background: var(--color-background);
  }
</style>
