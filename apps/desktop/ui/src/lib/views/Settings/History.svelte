<script lang="ts">
  // Settings › History (11 §5.12): "Keep conversations and history for [n] days", validated in place.
  // The message is formatted around a placeholder so the box sits where the number goes in every language.
  import { onMount } from 'svelte';

  import type { BackendState } from '../../store/backend-state.svelte';
  import { t } from '../../i18n/fluent';
  import ErrorLine from '../ErrorLine.svelte';
  import { HistoryModel } from './history-model.svelte';

  interface Props {
    backend: BackendState;
    model?: HistoryModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new HistoryModel(backend.handle);
  const MARK = '\u0001';
  const parts = $derived(t('settings-history-line', { days: MARK }).split(MARK));
  const boxId = 'history-days';
  const errorId = 'history-days-error';

  onMount(() => void model.load());
</script>

<section class="history" aria-labelledby="settings-history-heading">
  <h3 id="settings-history-heading">{t('settings-section-history')}</h3>
  <form
    onsubmit={(e) => {
      e.preventDefault();
      void model.submit(model.text);
    }}
  >
    <label for={boxId}>
      {parts[0]}<input
        id={boxId}
        type="text"
        inputmode="numeric"
        size="4"
        value={model.text}
        aria-invalid={model.invalid ? 'true' : undefined}
        aria-describedby={model.invalid ? errorId : undefined}
        oninput={(e) => (model.text = e.currentTarget.value)}
        onchange={(e) => void model.submit(e.currentTarget.value)}
      />{parts[1] ?? ''}
    </label>
    {#if model.invalid}<p id={errorId} class="invalid">{t('settings-history-invalid')}</p>{/if}
  </form>
  <p class="note">{t('settings-history-only-way')}</p>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>

<style>
  /* Settings › History (scene `historySetting`): one line in Small text with the box where the number goes,
     the refusal under it in the danger colour, and the muted note. */
  label {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    font-size: 13px;
  }
  .invalid {
    color: var(--color-danger);
    font-size: 13px;
  }
  .note {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  input {
    inline-size: 5.5em;
    margin-inline: var(--space-06);
    padding: 3px var(--space-06);
  }
  input[aria-invalid='true'] {
    border: 2px solid var(--color-danger);
  }
</style>
