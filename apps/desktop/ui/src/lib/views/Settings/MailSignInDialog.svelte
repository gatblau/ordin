<script lang="ts">
  // The mail sign-in screen of 11 §5.22 in a modal dialog (SM-NS-2.3): the address, the app password or password
  // with **Show** / **Hide**, and **Sign in**. The password field is cleared after sending; an empty password
  // is not sent and the dialog says so. For the company mail server's single sign-on the dialog asks for the
  // address only, and **Sign in** goes on in the browser. Focus starts in the address, Tab stays in the
  // dialog, Escape closes it and focus returns to the button that opened it (amended January 2026 (user
  // decision)).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2
  import { t } from '../../i18n/fluent';
  import type { BackendHandle } from '../../backend/backend';
  import { focusableIn, trapIndex } from '../../shell/focus';
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
  let root = $state<HTMLElement | null>(null);
  const app = $derived(model.mailDialog !== null && MAIL_PASSWORD_KINDS[model.mailDialog.kind] === 'app_password');
  const sso = $derived(model.mailSingleSignOn);
  let opener: Element | null = null;

  /** Moves focus into the dialog when it opens and back to its opener when it closes. */
  function focusOnOpen(node: HTMLElement): { destroy: () => void } {
    opener = document.activeElement;
    node.querySelector<HTMLElement>('input')?.focus();
    return {
      destroy: () => {
        if (opener instanceof HTMLElement) opener.focus();
      },
    };
  }

  function close(): void {
    shown = false;
    model.mailDialog = null;
  }

  function onkeydown(event: KeyboardEvent): void {
    if (model.mailDialog === null) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      close();
    } else if (event.key === 'Tab' && root !== null) {
      const items = focusableIn(root);
      const next = trapIndex(items.indexOf(document.activeElement as HTMLElement), items.length, event.shiftKey);
      if (next >= 0) {
        event.preventDefault();
        items[next]?.focus();
      }
    }
  }
</script>

<svelte:window {onkeydown} />

{#if model.mailDialog !== null}
  <div class="dialog-backdrop">
    <div bind:this={root} class="dialog" role="dialog" aria-modal="true" aria-labelledby="signin-mail-title" use:focusOnOpen>
      <form
        class="mail-signin field-stack"
        onsubmit={(e) => {
          e.preventDefault();
          void model.saveMailPassword().then((saved) => {
            if (saved) onsaved?.();
          });
        }}
      >
        <h2 id="signin-mail-title" tabindex="-1">{t('signin-mail-title')}</h2>
        <p>{sso ? t('signin-mail-sso-note') : t('signin-mail-password-note')}</p>
        <label>
          {t('signin-mail-address')}
          <input type="email" bind:value={model.mailAddress} autocomplete="off" aria-describedby="signin-mail-address-hint" />
        </label>
        <p id="signin-mail-address-hint" class="secondary">{t('signin-mail-address-hint')}</p>
        {#if !sso}
          <label>
            {app ? t('signin-mail-app-password') : t('signin-mail-password')}
            <input
              type={shown ? 'text' : 'password'}
              bind:value={model.mailPassword}
              autocomplete="off"
              aria-invalid={model.mailPasswordEmpty ? 'true' : undefined}
              aria-describedby={model.mailPasswordEmpty ? 'signin-mail-password-empty' : undefined}
            />
          </label>
          {#if model.mailPasswordEmpty}<p id="signin-mail-password-empty" class="invalid">{t('signin-mail-password-empty')}</p>{/if}
          <button type="button" class="small" aria-pressed={shown} onclick={() => (shown = !shown)}>{shown ? t('signin-hide') : t('signin-show')}</button>
          <p>{t('signin-mail-keychain-note')}</p>
        {/if}
        <ErrorLine error={model.error} />
        <div class="dialog-buttons">
          <button type="submit" data-default="true">{t('signin-sign-in')}</button>
          <button type="button" onclick={close}>{t('confirm-cancel')}</button>
        </div>
      </form>
    </div>
  </div>
{/if}
