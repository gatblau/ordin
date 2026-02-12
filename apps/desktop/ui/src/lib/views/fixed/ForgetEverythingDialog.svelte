<script lang="ts">
  // The confirmation of Forget everything (SM-FE-2): the count, what stays, a field where the person types
  // the workspace's name (or "every workspace"), and the danger button enabled only once it matches.
  // Cancel has the focus when it opens; Escape cancels; Tab stays inside; focus returns to the opener.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-FE-2
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import { focusableIn, trapIndex } from '../../shell/focus';
  import ErrorLine from '../ErrorLine.svelte';
  import type { ForgetEverything } from './forget-everything.svelte';

  interface Props {
    model: ForgetEverything;
    /** The workspace's name; absent for every workspace. */
    name?: string;
    onforgotten: (facts: number) => void;
    oncancel: () => void;
  }

  let { model, name, onforgotten, oncancel }: Props = $props();

  const headingId = `forget-everything-${Math.random().toString(36).slice(2, 10)}`;
  const fieldId = `${headingId}-field`;
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

  function cancel(): void {
    if (!model.running) oncancel();
  }

  async function forget(): Promise<void> {
    const facts = await model.forget();
    if (facts !== null) onforgotten(facts);
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      cancel();
    } else if (event.key === 'Tab' && root !== null) {
      const items = focusableIn(root);
      const next = trapIndex(items.indexOf(document.activeElement as HTMLElement), items.length, event.shiftKey);
      if (next >= 0) {
        event.preventDefault();
        items[next]?.focus();
      }
    }
  }

  const title = $derived(name !== undefined ? t('forget-everything-title-workspace', { name }) : t('forget-everything-title-all'));
  const label = $derived(
    name !== undefined ? t('forget-everything-type-name', { name }) : t('forget-everything-type-all', { phrase: t('forget-everything-all-phrase') }),
  );
</script>

<svelte:window {onkeydown} />

<div class="dialog-backdrop">
  <div bind:this={root} class="dialog" role="dialog" aria-modal="true" aria-labelledby={headingId}>
    <h2 id={headingId} tabindex="-1">{title}</h2>
    {#if model.nothing}
      <p class="dialog-body">{t('forget-everything-nothing')}</p>
      <div class="dialog-buttons">
        <button type="button" bind:this={cancelButton} data-default="true" onclick={cancel}>{t('card-close')}</button>
      </div>
    {:else if model.count !== null}
      {@const count = model.count}
      <p class="dialog-body">{t('forget-everything-body', { facts: count.facts })}</p>
      {#if count.to_check > 0}<p class="dialog-body">{t('forget-everything-to-check', { count: count.to_check })}</p>{/if}
      {#if count.everywhere > 0}<p class="dialog-body">{t('forget-everything-everywhere', { count: count.everywhere })}</p>{/if}
      <p class="dialog-body">{t('forget-everything-keeps')}</p>
      <label for={fieldId}>{label}</label>
      <input id={fieldId} type="text" autocomplete="off" spellcheck="false" bind:value={model.typed} disabled={model.running} />
      <ErrorLine error={model.error} onretry={() => void forget()} />
      <div class="dialog-buttons">
        <button type="button" class="danger" disabled={!model.confirmed || model.running} onclick={() => void forget()}>
          {t('forget-everything-confirm')}
        </button>
        <button type="button" bind:this={cancelButton} data-default="true" disabled={model.running} onclick={cancel}>
          {t('confirm-cancel')}
        </button>
      </div>
    {/if}
  </div>
</div>
