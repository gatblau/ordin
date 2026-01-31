<script lang="ts">
  // Settings › Jira (ADR-106), the one place Jira is connected (SM-NS-2.10): not connected → the intro, site, email, the three numbered lines on making an
  // API token with Atlassian's token page, the token, **Connect** and **Sign in with Atlassian…**, fields
  // stacked and the site's message tied to its field; connected → "Connected to [site]", and **Disconnect
  // Jira…**, whose question says what is deleted and what stays (amended January 2026 (user decision)).
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { JiraModel, reloadOnSignIn } from './signins-model.svelte';
  import { WebPageOpener } from './web-page.svelte';

  interface Props {
    backend: BackendState;
    model?: JiraModel;
    pages?: WebPageOpener;
  }
  let { backend, model: given, pages: givenPages }: Props = $props();
  // svelte-ignore state_referenced_locally
  const model = given ?? new JiraModel(backend.handle);
  // SM-SW-11: **Open Atlassian's token page** (`app.open_web_page`, local backend), with **Copy the address**
  // kept as the fallback (amended January 2026 (user decision)).
  // svelte-ignore state_referenced_locally
  const pages = givenPages ?? new WebPageOpener(backend.handle);
  let tokenField = $state<HTMLInputElement | null>(null);
  let siteField = $state<HTMLInputElement | null>(null);
  let copied = $state(false);
  /** Atlassian's own page for making an API token, shown and copied when the browser does not open. */
  const TOKEN_PAGE = 'https://id.atlassian.com/manage-profile/security/api-tokens';

  async function copyTokenPage(): Promise<void> {
    try {
      await navigator.clipboard.writeText(TOKEN_PAGE);
      copied = true;
    } catch {
      // The clipboard is not available: the address stays on screen to select.
    }
  }

  onMount(() => {
    void model.load();
    // SM-NS-2.10: with no Atlassian app named by the profile, only the API token is offered.
    if (answers(backend, 'profile.get_document')) void model.loadProfile();
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

<section class="jira settings-section" aria-labelledby="settings-jira-heading">
  <h3 id="settings-jira-heading">{t('settings-section-jira')}</h3>
  {#if model.connection !== null}
    <div class="setting-row">
      <span class="grow">
        {#if model.last !== null}
          {t('settings-jira-connected', { site: model.last.site, name: model.last.name })}
        {:else}
          {t('settings-jira-connected-site', { site: model.connection.account_label })}
        {/if}
      </span>
      <button type="button" class="small" onclick={() => (model.confirmDisconnect = true)}>{t('settings-jira-disconnect')}</button>
    </div>
  {:else}
    <p class="intro">{t('settings-jira-intro')}</p>
    <form
      class="field-stack"
      onsubmit={(e) => {
        e.preventDefault();
        void connect();
      }}
    >
      <label>
        {t('settings-jira-site')}
        <input
          bind:this={siteField}
          type="text"
          bind:value={model.site}
          autocomplete="off"
          aria-invalid={model.siteNeeded ? 'true' : undefined}
          aria-describedby={model.siteNeeded ? 'settings-jira-site-needed' : undefined}
        />
      </label>
      {#if model.siteNeeded}<p id="settings-jira-site-needed" class="invalid" role="alert">{t('settings-jira-site-needed')}</p>{/if}
      <label>{t('settings-jira-email')} <input type="email" bind:value={model.email} autocomplete="off" /></label>
      <ol class="guide">
        <li>{t('settings-jira-token-step-1')}</li>
        <li>{t('settings-jira-token-step-2')}</li>
        <li>{t('settings-jira-token-step-3')}</li>
      </ol>
      <p class="token-page">
        <code>{TOKEN_PAGE}</code>
        <button type="button" class="small" onclick={() => void pages.open('atlassian_api_tokens')}>{t('settings-jira-open-token-page')}</button>
        <button type="button" class="small" onclick={() => void copyTokenPage()}>{t('settings-jira-copy-token-page')}</button>
        <span class="hint" role="status" aria-live="polite">{copied ? t('settings-jira-token-page-copied') : ''}</span>
      </p>
      <ErrorLine error={pages.error} onretry={() => void pages.retry()} />
      <label>{t('settings-jira-token')} <input bind:this={tokenField} type="password" bind:value={model.token} autocomplete="off" aria-describedby="settings-jira-token-note" /></label>
      <p id="settings-jira-token-note" class="hint">{t('settings-jira-token-note')}</p>
      <div class="row-actions">
        <button type="submit" data-default="true" disabled={model.busy}>{t('settings-jira-connect')}</button>
        {#if !model.oauthUnavailable && !model.oauthNotOffered}
          <button type="button" onclick={async () => { await model.signInWithAtlassian(); if (model.siteNeeded) siteField?.focus(); }}>{t('settings-jira-oauth')}</button>
        {/if}
      </div>
      {#if model.oauthUnavailable}<p>{t('settings-jira-oauth-unavailable')}</p>{/if}
    </form>
    {#if model.otherSite}<p class="error-line" role="alert">{t('settings-jira-other-site')}</p>{/if}
    <ErrorLine error={model.error} onretry={() => void connect(true)} />
  {/if}
  <p class="done-line" role="status" aria-live="polite">{model.done}</p>
</section>

{#if model.confirmDisconnect}
  <ConfirmDialog
    title={t('settings-jira-disconnect-title')}
    body={t('settings-jira-disconnect-body')}
    verb={t('settings-jira-disconnect-verb')}
    initialFocus="cancel"
    onconfirm={() => void model.disconnect(t('settings-jira-disconnected'))}
    oncancel={() => (model.confirmDisconnect = false)}
  />
{/if}

<style>
  .jira {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .guide {
    font-size: 13px;
  }
  .token-page {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    align-items: center;
    font-size: 13px;
  }
  .token-page code {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }
</style>
