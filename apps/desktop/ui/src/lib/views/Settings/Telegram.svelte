<script lang="ts">
  // Settings › Telegram: the fixed sentence, then by `telegram.status` the reason it is not offered, the
  // connected bot with **Remove Telegram**, or **Set up Telegram** with the key field.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { NOT_OFFERED_MESSAGE, TelegramModel } from './telegram-model.svelte';

  interface Props {
    backend: BackendState;
    model?: TelegramModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new TelegramModel(backend.handle);
  const status = $derived(model.status);

  onMount(() => void model.load());
</script>

<section class="telegram" aria-labelledby="settings-telegram-heading">
  <h3 id="settings-telegram-heading">{t('settings-section-telegram')}</h3>
  <p class="fixed">{t('settings-telegram-fixed')}</p>
  {#if status !== null}
    {#if !status.offered}
      {#if status.reason_not_offered}<p>{t(NOT_OFFERED_MESSAGE[status.reason_not_offered])}</p>{/if}
    {:else if status.connected && status.bot}
      <p>{t('settings-telegram-bot', { name: status.bot.display_name, username: status.bot.username })}</p>
      {#if !status.bot.can_read_all_group_messages}<p>{t('settings-telegram-admin-note')}</p>{/if}
      <button type="button" onclick={() => void model.remove()}>{t('settings-telegram-remove')}</button>
    {:else}
      <h4>{t('settings-telegram-set-up')}</h4>
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void model.connect();
        }}
      >
        <label>
          {t('settings-telegram-key')}
          <input type="password" bind:value={model.key} autocomplete="off" />
        </label>
        <button type="submit" data-default="true">{t('settings-telegram-connect')}</button>
      </form>
    {/if}
  {/if}
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>
