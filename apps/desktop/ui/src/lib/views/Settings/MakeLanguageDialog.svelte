<script lang="ts">
  // Make a new language… (SM-LG-4): name and code, **Next** shows the estimate, **Draft it** (never the
  // default button) or Cancel. Nothing is drafted before Draft it. Tab stays inside the dialog and the verb
  // comes before Cancel, as in the confirmation pattern.
  import { onMount } from 'svelte';

  import type { BackendHandle } from '../../backend/backend';
  import { t } from '../../i18n/fluent';
  import { focusableIn, trapIndex } from '../../shell/focus';
  import ErrorLine from '../ErrorLine.svelte';
  import type { MakeLanguageModel } from './language-model.svelte';

  interface Props {
    make: MakeLanguageModel;
    backend: BackendHandle;
    onclose: () => void;
    ondrafted?: () => void;
  }
  let { make, backend, onclose, ondrafted }: Props = $props();
  let nameBox = $state<HTMLInputElement | null>(null);
  let tagBox = $state<HTMLInputElement | null>(null);
  let root = $state<HTMLElement | null>(null);

  /** Escape closes; Tab stays inside the dialog (ConfirmDialog's handling; amended January 2026 (user decision)). */
  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      onclose();
    } else if (event.key === 'Tab' && root !== null) {
      const items = focusableIn(root);
      const next = trapIndex(items.indexOf(document.activeElement as HTMLElement), items.length, event.shiftKey);
      if (next >= 0) {
        event.preventDefault();
        items[next]?.focus();
      }
    }
  }

  onMount(() => nameBox?.focus());
  // On an error, focus goes to the field it concerns.
  $effect(() => {
    if (make.error !== null) (make.errorField === 'name' ? nameBox : tagBox)?.focus();
  });

  async function draft(): Promise<void> {
    if (await make.draft()) {
      ondrafted?.();
      onclose();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="dialog-backdrop">
  <div bind:this={root} class="dialog field-stack" role="dialog" aria-modal="true" aria-labelledby="make-language-heading">
    <h2 id="make-language-heading" tabindex="-1" data-region-focus>{t('language-make')}</h2>
    <label>{t('language-make-name')} <input bind:this={nameBox} type="text" maxlength="60" bind:value={make.name} disabled={make.estimate !== null} /></label>
    <label>{t('language-make-tag')} <input bind:this={tagBox} type="text" maxlength="35" bind:value={make.tag} disabled={make.estimate !== null} /></label>
    <p class="hint">{t('language-make-tag-hint')}</p>
    {#if make.estimate !== null}
      <p>
        {#if make.estimate.estimate}
          {t('language-make-estimate', {
            band: t(`language-band-${make.estimate.estimate.band}`),
            min: String(make.estimate.estimate.min_turns),
            max: String(make.estimate.estimate.max_turns),
            count: String(make.estimate.messages),
          })}
        {:else}
          {t('language-make-no-estimate', { count: String(make.estimate.messages) })}
        {/if}
      </p>
      <p>{t('language-make-review-line')}</p>
    {/if}
    <ErrorLine error={make.error} />
    <!-- The verb first, then Cancel, as the confirmation pattern; Draft it is never the default button. -->
    <div class="dialog-buttons">
      {#if make.estimate === null}
        <button type="button" class="go" disabled={!make.validFields || make.busy} onclick={() => void make.next()}>{t('language-make-next')}</button>
      {:else}
        <button type="button" class="go" disabled={make.busy} onclick={() => void draft()}>{t('language-offer-draft')}</button>
      {/if}
      <button type="button" data-default="true" onclick={onclose}>{t('confirm-cancel')}</button>
    </div>
  </div>
</div>
