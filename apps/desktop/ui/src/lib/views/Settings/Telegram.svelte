<script lang="ts">
  // Settings › Telegram: the fixed sentence, then by `telegram.status` the reason it is not offered, the
  // connected bot with **Remove Telegram** (at once, then a line says what it did), or **Set up Telegram**
  // as three steps with the key field (amended January 2026 (user decision)).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-1
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { NOT_OFFERED_MESSAGE, TelegramModel } from './telegram-model.svelte';
  import { WebPageOpener } from './web-page.svelte';

  interface Props {
    backend: BackendState;
    model?: TelegramModel;
    pages?: WebPageOpener;
  }
  let { backend, model: given, pages: givenPages }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new TelegramModel(backend.handle);
  // SM-SW-5: **Open BotFather in Telegram** (`app.open_web_page`, local backend; amended January 2026 (user decision)).
  // svelte-ignore state_referenced_locally
  const pages = givenPages ?? new WebPageOpener(backend.handle);
  const status = $derived(model.status);

  onMount(() => void model.load());
</script>

<section class="telegram settings-section" aria-labelledby="settings-telegram-heading">
  <h3 id="settings-telegram-heading">{t('settings-section-telegram')}</h3>
  <p class="fixed"><b>{t('settings-telegram-fixed')}</b></p>
  {#if status !== null}
    {#if !status.offered}
      {#if status.reason_not_offered}<p>{t(NOT_OFFERED_MESSAGE[status.reason_not_offered])}</p>{/if}
    {:else if status.connected && status.bot}
      <div class="setting-row">
        <span class="grow">{t('settings-telegram-bot', { name: status.bot.display_name, username: status.bot.username })}</span>
        <button type="button" class="small" onclick={() => void model.remove(t('settings-telegram-removed'))}>{t('settings-telegram-remove')}</button>
      </div>
      {#if !status.bot.can_read_all_group_messages}<p class="intro">{t('settings-telegram-admin-note')}</p>{/if}
    {:else}
      <h4>{t('settings-telegram-set-up')}</h4>
      <p class="intro">{t('settings-telegram-set-up-line')}</p>
      <form
        class="field-stack"
        onsubmit={(e) => {
          e.preventDefault();
          void model.connect();
        }}
      >
        <ol class="steps">
          <li>
            <b>{t('settings-telegram-step-make')}</b>
            <p class="hint">{t('settings-telegram-step-make-line')}</p>
            <button type="button" class="small" onclick={() => void pages.open('telegram_botfather')}>{t('settings-telegram-open-botfather')}</button>
            <ErrorLine error={pages.error} onretry={() => void pages.retry()} />
          </li>
          <li>
            <b>{t('settings-telegram-step-paste')}</b>
            <label>
              {t('settings-telegram-key')}
              <input
                type="password"
                bind:value={model.key}
                autocomplete="off"
                aria-invalid={model.keyEmpty ? 'true' : undefined}
                aria-describedby="settings-telegram-key-hint"
              />
            </label>
            <p id="settings-telegram-key-hint" class={model.keyEmpty ? 'invalid' : 'hint'}>{model.keyEmpty ? t('settings-telegram-key-empty') : t('settings-telegram-key-hint')}</p>
            <button type="submit" data-default="true">{t('settings-telegram-connect')}</button>
          </li>
          <li>
            <b>{t('settings-telegram-step-group')}</b>
            <p class="hint">{t('settings-telegram-step-group-line')}</p>
          </li>
        </ol>
      </form>
    {/if}
  {/if}
  <p class="done-line" role="status" aria-live="polite">{model.done}</p>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
</section>

<style>
  .telegram {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .steps {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .steps > li {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-04);
  }
</style>
