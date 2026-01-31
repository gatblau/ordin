<script lang="ts">
  // What a resolved `ContentRef` looks like. In an approval card (`bound`) it is the exact version the card
  // covers: heading, note, the preview, and the cut line when the text was truncated (SM-SF-1.1); when the
  // version cannot be read the card says so and the person is told to reject if they need to see it (SM-SF-1.2).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-1
  import { t } from '../i18n/fluent';
  import ErrorLine from '../views/ErrorLine.svelte';
  import type { BackendHandle } from '../backend/backend';
  import PreviewPane from './PreviewPane.svelte';
  import type { Resolved } from './refs';

  interface Props {
    /** `null` while the read is in flight. */
    resolved: Resolved | null;
    /** True for the content view opened by an approval card. */
    bound?: boolean;
    /** Show link targets even when the reference is not a bound version (SM-SF-1.5). */
    showTargets?: boolean;
    /** The backend the failing read went to, for the error line's actions. */
    backend?: BackendHandle;
    /** Bound to the heading so the card can move focus to it when the view opens. */
    heading?: HTMLElement | null;
  }
  let { resolved, bound = false, showTargets = false, backend, heading = $bindable(null) }: Props = $props();
</script>

{#if resolved === null}
  <p class="busy" role="status" aria-busy="true"></p>
{:else if resolved.kind === 'ok'}
  {#if bound}
    <h2 bind:this={heading} tabindex="-1">{t('approval-bound-version-heading', { name: resolved.name })}</h2>
    <p class="note">{t('approval-bound-version-note')}</p>
  {/if}
  <PreviewPane preview={resolved.preview} label={resolved.name} showTargets={bound || showTargets} />
  {#if bound && resolved.preview.kind === 'text' && resolved.preview.truncated}
    <p class="note">{t('approval-bound-version-cut')}</p>
  {/if}
{:else if bound}
  <p bind:this={heading} tabindex="-1">{t('approval-bound-version-unavailable')}</p>
  {#if resolved.kind === 'error'}<ErrorLine error={resolved.error} {...backend === undefined ? {} : { backend }} />{/if}
{:else}
  <p>{t('block-cannot-show')}</p>
  {#if resolved.kind === 'error'}<ErrorLine error={resolved.error} {...backend === undefined ? {} : { backend }} />{/if}
{/if}

<style>
  .note {
    color: var(--color-text-muted);
  }
</style>
