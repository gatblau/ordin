<script lang="ts">
  // Settings › Display: Appearance (Same as this computer / Light / Dark), a group of choices with no box;
  // when the company set it, the choices are disabled with "Set by your company" (amended January 2026
  // (user decision)).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import type { BackendState } from '../../store/backend-state.svelte';
  import { t } from '../../i18n/fluent';
  import ErrorLine from '../ErrorLine.svelte';
  import { APPEARANCES, DisplayModel, splitAppearance } from './display-model.svelte';

  interface Props {
    backend: BackendState;
    model?: DisplayModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new DisplayModel(backend.handle);
  const text = $derived(splitAppearance(t('settings-appearance')));

  onMount(() => void model.load());
</script>

<section class="display settings-section" aria-labelledby="settings-display-heading">
  <h3 id="settings-display-heading">{t('settings-section-display')}</h3>
  <fieldset class="choice-group" disabled={!model.mayChange}>
    <legend>{text.label}{#if !model.mayChange} <span class="tag">{t('settings-set-by-company')}</span>{/if}</legend>
    {#each APPEARANCES as value, i (value)}
      <label>
        <input type="radio" name="appearance" {value} checked={model.appearance === value} onchange={() => void model.choose(value)} />
        {text.options[i]}
      </label>
    {/each}
  </fieldset>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>
