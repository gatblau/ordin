<script lang="ts">
  // Settings › Sign-ins: one row per account (product name, label, state), **Sign in** and **Sign out**,
  // the browser or device-code wait with **Cancel**, the GitLab token box when the organisation names no
  // GitLab sign-in (SA-CLOSE-03), the mail password dialog and, on this computer when the profile requires
  // one, **Import a certificate…**.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2
  // spec: ordin-desktop/components/ScreensModule#SA-CLOSE-03
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import MailSignInDialog from './MailSignInDialog.svelte';
  import { kindMessageId, reloadOnSignIn, SignInsModel } from './signins-model.svelte';

  interface Props {
    backend: BackendState;
    model?: SignInsModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new SignInsModel(backend.handle);
  const challenge = $derived(model.challenge);
  let retryKind = $state<string | null>(null);

  onMount(() => {
    void model.load();
    if (backend.kind === 'local' && answers(backend, 'profile.get_document')) void model.loadCertificateRequirement();
    return reloadOnSignIn(backend, () => model.load());
  });

  async function copy(code: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(code);
    } catch {
      // The clipboard is not available: the code stays on screen to type.
    }
  }
</script>

<section class="sign-ins" aria-labelledby="settings-sign-ins-heading">
  <h3 id="settings-sign-ins-heading">{t('settings-section-sign-ins')}</h3>
  <ul>
    {#each model.items as item (item.id)}
      <li data-kind={item.kind}>
        <span class="product">{t(kindMessageId(item.kind))}</span>
        <span class="account secondary">{item.account_label}</span>
        <span class="state">{t(item.state === 'signed_in' ? 'signin-state-signed-in' : item.state === 'needs_sign_in' ? 'signin-state-needs-sign-in' : 'signin-state-signed-out')}</span>
        {#if item.state === 'signed_in' && item.expires_at}
          <span class="expires">{t('signin-expires', { date: formatDate(item.expires_at) })}</span>
        {/if}
        {#if item.state === 'signed_in'}
          <button type="button" onclick={() => void model.signOut(item.id)}>{t('signin-sign-out')}</button>
        {:else}
          <button type="button" disabled={model.challenge !== null} onclick={() => ((retryKind = item.id), void model.startSignIn(item))}>{t('signin-sign-in')}</button>
        {/if}
        {#if model.challenge !== null && model.challengeKind === item.kind}
          {#if challenge !== null && challenge.kind === 'device_code'}
            <p class="device-code">
              {t('signin-device-code', { address: challenge.verification_uri })}
              <code class="user-code">{challenge.user_code}</code>
              <button type="button" onclick={() => void copy(challenge.user_code)}>{t('signin-copy-code')}</button>
            </p>
          {:else}
            <p>{t('signin-browser-opened')}</p>
          {/if}
          <button type="button" onclick={() => void model.cancelSignIn()}>{t('confirm-cancel')}</button>
        {/if}
        {#if item.kind === 'gitlab' && model.tokenBox.has('gitlab')}
          <form
            onsubmit={(e) => {
              e.preventDefault();
              void model.saveGitlabToken();
            }}
          >
            <label>{t('signin-gitlab-paste-token')} <input type="password" bind:value={model.token} autocomplete="off" /></label>
          </form>
        {/if}
      </li>
    {/each}
  </ul>
  <MailSignInDialog {model} backend={backend.handle} />
  {#if backend.kind === 'local' && model.certificateRequired}
    <p>
      <button type="button" onclick={() => void model.importCertificate()}>{t('signin-import-certificate')}</button>
    </p>
    {#if model.certificate !== null}
      <p>{t('signin-certificate-imported', { name: model.certificate.name, date: formatDate(model.certificate.not_after) })}</p>
    {/if}
    <ErrorLine error={model.certificateError} />
  {/if}
  <ErrorLine
    error={model.mailDialog === null ? model.error : null}
    onretry={() => {
      const item = model.items.find((i) => i.id === retryKind);
      if (item !== undefined) void model.startSignIn(item);
      else void model.load();
    }}
  />
</section>

<style>
  /* One row per account (scene `settings`, the prototype's `.lrows` / `.lrow`): the product in Control text,
     the account and its state under it in Label text, muted, and the row's button at the end. */
  .sign-ins > ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .sign-ins > ul > li {
    display: grid;
    grid-template-columns: auto auto 1fr auto;
    align-items: center;
    column-gap: var(--space-06);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-06) var(--space-10);
    font-size: 14px;
  }
  .product {
    grid-column: 1 / 4;
    grid-row: 1;
  }
  .account,
  .state,
  .expires {
    grid-row: 2;
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .sign-ins > ul > li > button {
    grid-column: 4;
    grid-row: 1 / span 2;
    padding: 3px var(--space-sm);
    min-height: 28px;
    font-size: 13px;
  }
  .sign-ins > ul > li > button ~ button,
  .sign-ins > ul > li > p,
  .sign-ins > ul > li > form {
    grid-column: 1 / -1;
    grid-row: auto;
    justify-self: start;
    font-size: 13px;
  }
</style>
