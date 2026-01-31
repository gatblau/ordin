<script lang="ts">
  // Settings › Dictation (ADR-078): what `dictation.get_availability` says, and no control: dictation is
  // started from the message box and form fields only, which a line says, with where the speech stays
  // (amended January 2026 (user decision)).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { formatLocale } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { DictationAvailability, ServiceError } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';

  interface Props {
    backend: BackendState;
    /** An answer already read (the tests); it is replaced by the first read. */
    initial?: DictationAvailability | null;
  }
  let { backend, initial = null }: Props = $props();
  // svelte-ignore state_referenced_locally
  let availability = $state.raw<DictationAvailability | null>(initial);
  let error = $state<ServiceError | null>(null);

  /** The locale's name in the active locale (`Intl.DisplayNames`). */
  function languageName(locale: string): string {
    try {
      return new Intl.DisplayNames([formatLocale()], { type: 'language' }).of(locale) ?? locale;
    } catch {
      return locale;
    }
  }

  async function load(): Promise<void> {
    const out = await backend.handle.call('dictation.get_availability', {});
    if (out.kind === 'ok') {
      availability = out.result;
      error = null;
    } else error = out.error;
  }

  onMount(() => void load());
</script>

<section class="dictation settings-section" aria-labelledby="settings-dictation-heading">
  <h3 id="settings-dictation-heading">{t('settings-section-dictation')}</h3>
  {#if availability !== null}
    {#if availability.state === 'available'}
      <p>{t('settings-dictation-available', { language: languageName(availability.locale) })}</p>
      <p class="intro">{t('settings-dictation-where')}</p>
    {:else if availability.state === 'needs_assets'}
      <p>{t('settings-dictation-needs-assets')}</p>
      <p class="intro">{t('settings-dictation-where')}</p>
    {:else}
      <p>{availability.message}</p>
    {/if}
  {/if}
  <ErrorLine {error} onretry={() => void load()} />
</section>
