<script lang="ts">
  // SM-CC-3: asking before Continue when the live conversation has messages. The title is the question; the
  // buttons are Keep the key points first, Continue (never the default) and Cancel; focus to Cancel, Tab
  // trapped, Escape cancels, focus returns to the opener.
  import { onMount } from 'svelte';

  import { t } from '../i18n/fluent';
  import { focusableIn, trapIndex } from '../shell/focus';

  interface Props {
    name: string;
    onkeep: () => void;
    oncontinue: () => void;
    oncancel: () => void;
  }
  let { name, onkeep, oncontinue, oncancel }: Props = $props();
  let root = $state<HTMLElement | null>(null);
  let cancelButton = $state<HTMLElement | null>(null);
  let opener: Element | null = null;

  onMount(() => {
    opener = document.activeElement;
    cancelButton?.focus();
    return () => {
      if (opener instanceof HTMLElement) opener.focus();
    };
  });

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      oncancel();
    } else if (event.key === 'Tab' && root !== null) {
      const items = focusableIn(root);
      const next = trapIndex(items.indexOf(document.activeElement as HTMLElement), items.length, event.shiftKey);
      if (next >= 0) {
        event.preventDefault();
        items[next]?.focus();
      }
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="dialog-backdrop">
  <div bind:this={root} class="dialog" role="dialog" aria-modal="true" aria-labelledby="continue-heading">
    <h2 id="continue-heading" tabindex="-1">{t('confirm-continue-title')}</h2>
    <p class="dialog-body">{t('confirm-continue-body', { name })}</p>
    <div class="dialog-buttons">
      <button type="button" onclick={onkeep}>{t('confirm-clear-keep-points')}</button>
      <button type="button" bind:this={cancelButton} data-default="true" onclick={oncancel}>{t('confirm-cancel')}</button>
      <button type="button" onclick={oncontinue}>{t('confirm-continue-verb')}</button>
    </div>
  </div>
</div>
