<script lang="ts">
  // Settings › Jira (ADR-106): not connected → the intro, site, email, API token, the guide, **Connect** and
  // **Sign in with Atlassian…**; connected → the site and name, and **Disconnect Jira…**.
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { JiraModel, reloadOnSignIn } from './signins-model.svelte';

  interface Props {
    backend: BackendState;
    model?: JiraModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new JiraModel(backend.handle);
  let tokenField = $state<HTMLInputElement | null>(null);
  let siteField = $state<HTMLInputElement | null>(null);

  onMount(() => {
    void model.load();
    // A `sign_in_changed` with `credential_kind` `jira` re-reads `identity.list`.
    return reloadOnSignIn(backend, () => model.load());
  });

  async function connect(retry = false): Promise<void> {
    await model.connect(retry);
    // SM-JS-2: with no site, nothing was sent and the site is what to fill in.
    if (model.siteNeeded) siteField?.focus();
    else tokenField?.focus();
  }
</script>

<section class="jira" aria-labelledby="settings-jira-heading">
  <h3 id="settings-jira-heading">{t('settings-section-jira')}</h3>
  {#if model.connection !== null}
    <p>
      {#if model.last !== null}
        {t('settings-jira-connected', { site: model.last.site, name: model.last.name })}
      {:else}
        {model.connection.account_label}
      {/if}
    </p>
    <button type="button" onclick={() => (model.confirmDisconnect = true)}>{t('settings-jira-disconnect')}</button>
  {:else}
    <p>{t('settings-jira-intro')}</p>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void connect();
      }}
    >
      <label>{t('settings-jira-site')} <input bind:this={siteField} type="text" bind:value={model.site} autocomplete="off" /></label>
      <label>{t('settings-jira-email')} <input type="email" bind:value={model.email} autocomplete="off" /></label>
      <label>{t('settings-jira-token')} <input bind:this={tokenField} type="password" bind:value={model.token} autocomplete="off" /></label>
      <p class="guide">{t('settings-jira-token-guide')}</p>
      <button type="submit" data-default="true" disabled={model.busy}>{t('settings-jira-connect')}</button>
      {#if model.oauthUnavailable}
        <p>{t('settings-jira-oauth-unavailable')}</p>
      {:else}
        <button type="button" onclick={async () => { await model.signInWithAtlassian(); if (model.siteNeeded) siteField?.focus(); }}>{t('settings-jira-oauth')}</button>
      {/if}
    </form>
    {#if model.siteNeeded}<p class="error-line" role="alert">{t('settings-jira-site-needed')}</p>{/if}
    {#if model.otherSite}<p class="error-line" role="alert">{t('settings-jira-other-site')}</p>{/if}
    <ErrorLine error={model.error} onretry={() => void connect(true)} />
  {/if}
</section>

{#if model.confirmDisconnect}
  <ConfirmDialog
    title={t('settings-jira-disconnect')}
    verb={t('settings-jira-disconnect')}
    initialFocus="cancel"
    onconfirm={() => void model.disconnect()}
    oncancel={() => (model.confirmDisconnect = false)}
  />
{/if}
