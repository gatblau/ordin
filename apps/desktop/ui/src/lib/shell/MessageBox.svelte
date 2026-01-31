<script lang="ts">
  // The conversation's message box. Nothing is sent until the person presses Send (or Ctrl/⌘+Enter).
  import { t } from '../i18n/fluent';

  interface Props {
    /** Sends the text; resolves true when it was accepted (the box is then cleared). */
    onsend: (text: string) => Promise<boolean>;
    /** Why sending is off (a server that is not connected): the sentence shown, and Send disabled. */
    disabledBecause?: string;
  }
  let { onsend, disabledBecause }: Props = $props();

  let text = $state('');
  let busy = $state(false);
  let box = $state<HTMLTextAreaElement | null>(null);

  export function focus(): void {
    box?.focus();
  }

  async function send(): Promise<void> {
    const value = text.trim();
    if (value === '' || busy || disabledBecause !== undefined) return;
    busy = true;
    const accepted = await onsend(value);
    busy = false;
    if (accepted) text = '';
  }
</script>

<form
  class="message-box"
  onsubmit={(e) => {
    e.preventDefault();
    void send();
  }}
>
  <label class="visually-hidden" for="conversation-message-box">{t('conv-header-conversation')}</label>
  <textarea
    id="conversation-message-box"
    bind:this={box}
    bind:value={text}
    maxlength="16384"
    rows="1"
    onkeydown={(e) => {
      if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        void send();
      }
    }}
  ></textarea>
  {#if disabledBecause !== undefined}<p class="message-box-note">{disabledBecause}</p>{/if}
  <button type="submit" data-default="true" disabled={busy || disabledBecause !== undefined || text.trim() === ''}>{t('conv-send')}</button>
</form>

<style>
  /* The composer (cross-cutting/VisualDesign.md V5 "Composer"; scene `long`). */
  .message-box {
    display: flex;
    gap: var(--space-sm);
    align-items: flex-end;
    flex-wrap: wrap;
    border-block-start: 1px solid var(--color-line);
    background: var(--color-surface);
    padding: var(--space-10) var(--space-12) var(--space-06);
    margin-block-start: auto;
  }
  textarea {
    flex: 1;
    min-width: 0;
    resize: none;
    border-radius: var(--radius-lg);
    padding: var(--space-10) var(--space-12);
    min-height: 44px;
  }
  .message-box-note {
    flex-basis: 100%;
    font-size: 13px;
    color: var(--color-text-muted);
  }
</style>
