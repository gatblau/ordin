<script lang="ts">
  // Settings › How Ordin talks to you: Tone, Formality, Length, Call me and Notes for Ordin. A choice with
  // `set_by = 'company'` reads "Your company's default" beside it and stays changeable. The preview is not
  // drawn in R1 (G-CU-5).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { LENGTHS, NOTES_MAX, REGISTERS, TONES, VoiceModel } from './voice-model.svelte';

  interface Props {
    backend: BackendState;
    model?: VoiceModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new VoiceModel(backend.handle);
  const settings = $derived(model.settings);

  onMount(() => void model.load());
</script>

<section class="voice" aria-labelledby="settings-voice-heading">
  <h3 id="settings-voice-heading">{t('settings-section-how-ordin-talks')}</h3>
  <p class="fixed">{t('settings-voice-fixed')}</p>
  {#if settings !== null}
    <fieldset>
      <legend>{t('settings-voice-tone')}</legend>
      {#each TONES as tone (tone)}
        <label>
          <input type="radio" name="voice-tone" value={tone} checked={settings.tone.value === tone} onchange={() => void model.update({ tone })} />
          {t(`settings-voice-tone-${tone}`)}
        </label>
      {/each}
      {#if settings.tone.set_by === 'company'}<span class="tag">{t('settings-voice-company-default')}</span>{/if}
    </fieldset>
    <fieldset>
      <legend>{t('settings-voice-formality')}</legend>
      {#each REGISTERS as register (register)}
        <label>
          <input type="radio" name="voice-register" value={register} checked={settings.register.value === register} onchange={() => void model.update({ register })} />
          {t(`settings-voice-formality-${register}`)}
        </label>
      {/each}
      {#if settings.register.set_by === 'company'}<span class="tag">{t('settings-voice-company-default')}</span>{/if}
    </fieldset>
    <fieldset>
      <legend>{t('settings-voice-length')}</legend>
      {#each LENGTHS as length (length)}
        <label>
          <input type="radio" name="voice-length" value={length} checked={settings.length.value === length} onchange={() => void model.update({ length })} />
          {t(`settings-voice-length-${length}`)}
        </label>
      {/each}
      {#if settings.length.set_by === 'company'}<span class="tag">{t('settings-voice-company-default')}</span>{/if}
    </fieldset>
    <p>
      <label>
        {t('settings-voice-call-me')}
        <input type="text" bind:value={model.nameToUse} onblur={() => void model.saveName()} autocomplete="off" />
      </label>
      {#if settings.name_to_use.set_by === 'company'}<span class="tag">{t('settings-voice-company-default')}</span>{/if}
    </p>
    <p>
      <label>
        {t('settings-voice-notes')}
        <textarea rows="3" maxlength={NOTES_MAX} placeholder={t('settings-voice-notes-example')} bind:value={model.notes} onblur={() => void model.saveNotes()}></textarea>
      </label>
      {#if settings.notes.set_by === 'company'}<span class="tag">{t('settings-voice-company-default')}</span>{/if}
    </p>
    <p class="secondary">{t('settings-voice-notes-limit')}</p>
  {/if}
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>

<style>
  /* Settings › How Ordin talks to you (scene `voiceSettings`, the prototype's `drawSetVoice`): a muted line,
     then one row per setting, a divider above each, its name at the start in Small text weight 600 and its
     control at the end. */
  .voice {
    font-size: 13px;
  }
  .fixed,
  p.secondary {
    color: var(--color-text-muted);
  }
  p.secondary {
    font-size: 12px;
  }
  fieldset {
    margin: 0;
    padding: var(--space-06) 0;
    border: 0;
    border-block-start: 1px solid var(--color-line);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-06) var(--space-12);
  }
  legend {
    float: inline-start;
    flex: 1;
    padding: 0;
    font-weight: 600;
  }
  fieldset > label {
    display: inline-flex;
    gap: var(--space-04);
    align-items: center;
  }
  .voice > p:not(.fixed, .secondary) {
    margin: 0;
    padding: var(--space-06) 0;
    border-block-start: 1px solid var(--color-line);
  }
  .voice > p > label {
    display: flex;
    gap: var(--space-06);
    align-items: center;
    justify-content: space-between;
    font-weight: 600;
  }
  .voice > p > label:has(textarea) {
    flex-direction: column;
    align-items: stretch;
  }
  .voice input[type='text'] {
    max-inline-size: 48%;
    font-weight: 400;
  }
  textarea {
    inline-size: 100%;
    box-sizing: border-box;
    font-weight: 400;
  }
  .tag {
    font-size: 12px;
    color: var(--color-text-muted);
  }
</style>
