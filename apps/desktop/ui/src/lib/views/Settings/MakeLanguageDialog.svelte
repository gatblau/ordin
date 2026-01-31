<script lang="ts">
  // Make a new language… (SM-LG-4): name and code, **Next** shows the estimate, **Draft it** (never the
  // default button) or Cancel. Nothing is drafted before Draft it.
  import { onMount } from 'svelte';

  import type { BackendHandle } from '../../backend/backend';
  import { t } from '../../i18n/fluent';
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

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />

<div class="dialog-backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="make-language-heading">
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
    <div class="dialog-buttons">
      <button type="button" data-default="true" onclick={onclose}>{t('confirm-cancel')}</button>
      {#if make.estimate === null}
        <button type="button" disabled={!make.validFields || make.busy} onclick={() => void make.next()}>{t('language-make-next')}</button>
      {:else}
        <button type="button" disabled={make.busy} onclick={() => void draft()}>{t('language-offer-draft')}</button>
      {/if}
    </div>
  </div>
</div>
