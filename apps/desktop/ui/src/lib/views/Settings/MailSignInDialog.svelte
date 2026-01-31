<script lang="ts">
  // The mail sign-in screen of 11 §5.22 in a dialog (SM-NS-2.3): the address, the app password or password
  // with **Show**, and **Sign in**. The password field is cleared after sending.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2
  import { t } from '../../i18n/fluent';
  import type { BackendHandle } from '../../backend/backend';
  import ErrorLine from '../ErrorLine.svelte';
  import { MAIL_PASSWORD_KINDS, type SignInsModel } from './signins-model.svelte';

  interface Props {
    model: SignInsModel;
    backend: BackendHandle;
    /** Called after the password was saved. */
    onsaved?: () => void;
  }
  let { model, backend, onsaved }: Props = $props();
  let shown = $state(false);
  const app = $derived(model.mailDialog !== null && MAIL_PASSWORD_KINDS[model.mailDialog.kind] === 'app_password');
</script>

{#if model.mailDialog !== null}
  <form
    class="mail-signin"
    role="group"
    aria-labelledby="signin-mail-title"
    onsubmit={(e) => {
      e.preventDefault();
      void model.saveMailPassword().then((saved) => {
        if (saved) onsaved?.();
      });
    }}
  >
    <h4 id="signin-mail-title" tabindex="-1">{t('signin-mail-title')}</h4>
    <p>{t('signin-mail-password-note')}</p>
    <label>
      {t('signin-mail-address')}
      <input type="email" bind:value={model.mailAddress} autocomplete="off" aria-describedby="signin-mail-address-hint" />
    </label>
    <p id="signin-mail-address-hint" class="secondary">{t('signin-mail-address-hint')}</p>
    <label>
      {app ? t('signin-mail-app-password') : t('signin-mail-password')}
      <input type={shown ? 'text' : 'password'} bind:value={model.mailPassword} autocomplete="off" />
    </label>
    <button type="button" aria-pressed={shown} onclick={() => (shown = !shown)}>{t('signin-show')}</button>
    <p>{t('signin-mail-keychain-note')}</p>
    <ErrorLine error={model.error} />
    <button type="submit" data-default="true">{t('signin-sign-in')}</button>
    <button type="button" onclick={() => (model.mailDialog = null)}>{t('confirm-cancel')}</button>
  </form>
{/if}
