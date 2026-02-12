<script lang="ts">
  // The confirmation pattern (11 §5.5, §10): the title is the question; one or two lines; the verb and
  // Cancel. A verb that removes, discards or stops is never the default button. Focus goes to the heading
  // (or to Cancel), Tab is trapped, Escape cancels, focus returns to the opener.
  import { onMount, type Snippet } from 'svelte';

  import { t } from '../i18n/fluent';
  import { focusableIn, trapIndex } from '../shell/focus';

  interface Props {
    title: string;
    body?: string;
    verb: string;
    /** Removes, discards or stops something: the verb is then never the default button. Default true. */
    destructive?: boolean;
    initialFocus?: 'heading' | 'cancel';
    onconfirm: () => void;
    oncancel: () => void;
    /** The words of the button that goes back (default Cancel; **Keep editing** when leaving an editor). */
    cancelLabel?: string;
    /** Drawn under the body: a further line, or the error of a failed verb (the dialog then stays open). */
    children?: Snippet;
  }

  let { title, body, verb, destructive = true, initialFocus = 'heading', onconfirm, oncancel, cancelLabel, children }: Props = $props();

  const headingId = `confirm-${Math.random().toString(36).slice(2, 10)}`;
  let root = $state<HTMLElement | null>(null);
  let heading = $state<HTMLElement | null>(null);
  let cancelButton = $state<HTMLElement | null>(null);
  let opener: Element | null = null;

  onMount(() => {
    opener = document.activeElement;
    (initialFocus === 'cancel' ? cancelButton : heading)?.focus();
    return () => {
      if (opener instanceof HTMLElement) opener.focus();
    };
  });

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      oncancel();
    } else if (event.key === 'Tab' && root !== null) {
      const items = [heading, ...focusableIn(root)].filter((x): x is HTMLElement => x !== null);
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
  <div bind:this={root} class="dialog" role="dialog" aria-modal="true" aria-labelledby={headingId}>
    <h2 id={headingId} bind:this={heading} tabindex="-1">{title}</h2>
    {#if body}<p class="dialog-body">{body}</p>{/if}
    {@render children?.()}
    <!-- The verb first, then Cancel (scene `removeWs`); the default button keeps its plain look. -->
    <div class="dialog-buttons">
      <button type="button" class:danger={destructive} class:go={!destructive} data-default={destructive ? undefined : 'true'} onclick={onconfirm}>
        {verb}
      </button>
      <button type="button" bind:this={cancelButton} data-default={destructive ? 'true' : undefined} onclick={oncancel}>
        {cancelLabel ?? t('confirm-cancel')}
      </button>
    </div>
  </div>
</div>
