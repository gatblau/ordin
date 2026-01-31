<script lang="ts">
  // Settings › Sign-ins: every service ordin can use, always listed, each with what it is for and one row
  // per saved account (account, state). Each row has one chevron that shows and hides its ways to sign in:
  // GitHub with one click (only when the build carries a GitHub App), with a token and with an SSH key;
  // GitLab with one click (only when the profile names a GitLab app) and with a token; the others signing
  // in in the browser; and, when signed in, **Sign out**. A service this version cannot sign in to is
  // greyed out with one line and no control. Confluence is connected with an API token (site, email and
  // token, checked before it is kept), or with one click when the profile names an Atlassian app, or with
  // the Jira API token. Jira is connected in Settings › Jira. The mail sign-in dialog
  // and, on this computer when the profile requires one, **Import a certificate…** stay as before.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2
  // spec: ordin-desktop/components/ScreensModule#SA-CLOSE-03
  // spec: ordin-desktop/components/ScreensModule#SM-SW-3
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2.6
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2.7
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2.8
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2.9
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2.11
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import Icon from '../../icons/Icon.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import MailSignInDialog from './MailSignInDialog.svelte';
  import { confluenceOneClick, gitlabOneClick, mailMethod, serviceRows, type ServiceRow } from './services';
  import { ConfluenceTokenModel, reloadOnSignIn, SignInsModel, SshKeyModel } from './signins-model.svelte';
  import { WebPageOpener } from './web-page.svelte';

  interface Props {
    backend: BackendState;
    model?: SignInsModel;
    /** Opens GitHub's token page in the browser (`app.open_web_page`, SM-NS-2.6). */
    pages?: WebPageOpener;
    /** GitHub's SSH key options (SM-NS-2.8); drawn only when the backend answers `app.ssh_keys`. */
    ssh?: SshKeyModel;
    /** Confluence's API-token sub-section (SM-NS-2.11). */
    confluence?: ConfluenceTokenModel;
  }
  let { backend, model: given, pages: givenPages, ssh: givenSsh, confluence: givenConfluence }: Props = $props();
  // svelte-ignore state_referenced_locally
  const ssh = givenSsh ?? new SshKeyModel(backend.handle);
  const sshOffered = $derived(answers(backend, 'app.ssh_keys'));
  // svelte-ignore state_referenced_locally
  const model = given ?? new SignInsModel(backend.handle);
  // svelte-ignore state_referenced_locally
  const pages = givenPages ?? new WebPageOpener(backend.handle);
  // svelte-ignore state_referenced_locally
  const confluence = givenConfluence ?? new ConfluenceTokenModel(backend.handle, model);
  const confluenceTokenOffered = $derived(answers(backend, 'confluence.connect_token'));
  /** Jira signed in: **Use my Jira sign-in** is offered under Confluence (its site is the account label). */
  const jiraSignedIn = $derived(model.items.find((c) => c.kind === 'jira' && c.state === 'signed_in') ?? null);
  const challenge = $derived(model.challenge);
  const rows = $derived(serviceRows(model.items, model.document, (command) => answers(backend, command)));
  /** The last sign-in started, so **Try again** repeats it. */
  let retry = $state<(() => void) | null>(null);

  onMount(() => {
    void model.load();
    if (answers(backend, 'profile.get_document')) void model.loadCertificateRequirement();
    return reloadOnSignIn(backend, () => model.load());
  });

  async function copy(code: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(code);
    } catch {
      // The clipboard is not available: the code stays on screen to type.
    }
  }

  /** The chevron; opening GitHub's row reads the SSH keys again. */
  function toggle(row: ServiceRow): void {
    if (model.toggle(row.key) && row.service.id === 'github' && sshOffered) void ssh.load();
  }

  /** **Sign in** in the browser: a saved account signs in again, a service with none starts afresh. */
  function signIn(row: ServiceRow): void {
    const item = row.credential;
    const run =
      row.service.id === 'mail' && item === null
        ? () => model.openMailFor(mailMethod(model.document))
        : item !== null
          ? () => void model.startSignIn(item)
          : () => void model.startNew(row.service.kinds[0]!);
    retry = run;
    run();
  }
</script>

<section class="sign-ins settings-section" aria-labelledby="settings-sign-ins-heading">
  <h3 id="settings-sign-ins-heading">{t('settings-section-sign-ins')}</h3>
  <p class="intro">{t('signin-intro')}</p>
  <ul class="list-rows">
    {#each rows as row (row.key)}
      {@const item = row.credential}
      {@const name = t(row.service.nameId)}
      {#if !row.available}
        <!-- SM-NS-2.9: this version cannot sign in to it; greyed out, one line, no control. -->
        <li class="list-row unavailable" aria-disabled="true" data-kind={row.service.kinds[0]} data-service={row.service.id}>
          <span class="grow">
            <b class="product">{t('signin-service', { name, for: t(row.service.forId) })}</b>
            <small class="state-line">{t('signin-not-available')}</small>
          </span>
        </li>
      {:else}
        {@const open = model.isOpen(row.key)}
        {@const label = t(open ? 'signin-row-hide' : 'signin-row-show', { service: name })}
        {@const waysId = `signin-ways-${row.key}`}
        <li class="list-row" data-kind={item?.kind ?? row.service.kinds[0]} data-service={row.service.id}>
          <span class="grow">
            <b class="product">{t('signin-service', { name, for: t(row.service.forId) })}</b>
            <small class="state-line">
              {#if item === null}
                {t('signin-state-signed-out')}
              {:else if item.state === 'signed_in'}
                {item.account_label} · {item.expires_at ? t('signin-signed-in-until', { date: formatDate(item.expires_at) }) : t('signin-state-signed-in')}
              {:else if item.state === 'needs_sign_in'}
                {item.account_label} · <span class="state-label waiting"><span aria-hidden="true">!</span>{t('signin-state-needs-sign-in')}</span>
              {:else}
                {item.account_label} · {t('signin-state-signed-out')}
              {/if}
            </small>
          </span>
          <span class="row-actions">
            <!-- SM-NS-2.7: one chevron per row shows and hides its ways to sign in. -->
            <button type="button" class="icon-btn disclose" aria-expanded={open} aria-controls={open ? waysId : undefined} aria-label={label} title={label} onclick={() => toggle(row)}><Icon name="chev" size="small" /></button>
          </span>
          {#if model.challenge !== null && row.service.kinds.includes(model.challengeKind ?? 'oidc') && (item === null || item.kind === model.challengeKind)}
            <div class="wait">
              {#if challenge !== null && challenge.kind === 'device_code'}
                <p class="device-code">
                  {t('signin-device-code', { address: challenge.verification_uri })}
                  <code class="user-code">{challenge.user_code}</code>
                  <button type="button" class="small" onclick={() => void copy(challenge.user_code)}>{t('signin-copy-code')}</button>
                </p>
              {:else}
                <p>{t('signin-browser-opened')}</p>
              {/if}
              <button type="button" class="small" onclick={() => void model.cancelSignIn()}>{t('confirm-cancel')}</button>
            </div>
          {/if}
          {#if open}
            <div id={waysId} class="ways">
              {#if item !== null && item.state === 'signed_in'}
                <div class="way signed-in">
                  {#if row.service.id === 'confluence' && confluence.last !== null}
                    <span class="grow">{t('signin-confluence-connected', { site: confluence.last.site, name: confluence.last.name })}</span>
                  {:else}
                    <span class="grow">{item.account_label} · {item.expires_at ? t('signin-signed-in-until', { date: formatDate(item.expires_at) }) : t('signin-state-signed-in')}</span>
                  {/if}
                  {#if row.service.id === 'confluence'}
                    <button type="button" class="small" onclick={() => void model.signOut(item.id, t('signin-confluence-disconnected'))}>{t('signin-confluence-disconnect')}</button>
                  {:else}
                    <button type="button" class="small" onclick={() => void model.signOut(item.id, t('signin-signed-out-of', { service: name }))}>{t('signin-sign-out')}</button>
                  {/if}
                </div>
              {/if}
              {#if row.service.id === 'github'}
                {#if model.githubOneClick}
                  <div class="way">
                    <h4>{t('signin-way-one-click')}</h4>
                    <p><button type="button" class="small" disabled={model.challenge !== null} onclick={() => signIn(row)}>{t('signin-sign-in')}</button></p>
                  </div>
                {:else}
                  <!-- SM-NS-2.6: a build with no GitHub App; the person makes a token on GitHub and pastes it. -->
                  <form
                    class="way field-stack"
                    onsubmit={(e) => {
                      e.preventDefault();
                      void model.saveGithubToken();
                    }}
                  >
                    <h4>{t('signin-way-token')}</h4>
                    <p>{t('signin-github-token-why')}</p>
                    <ol class="guide">
                      <li>{t('signin-github-token-step-1')}</li>
                      <li>{t('signin-github-token-step-2')}</li>
                      <li>{t('signin-github-token-step-3')}</li>
                    </ol>
                    <p>
                      <button type="button" class="small" onclick={() => void pages.open('github_tokens')}>{t('signin-github-open-token-page')}</button>
                    </p>
                    <ErrorLine error={pages.error} onretry={() => void pages.retry()} />
                    <label>
                      {t('signin-github-paste-token')}
                      <input type="password" bind:value={model.githubToken} autocomplete="off" aria-describedby="signin-github-hint" aria-invalid={model.githubTokenEmpty || model.githubTokenRefused ? 'true' : undefined} />
                    </label>
                    {#if model.githubTokenRefused}
                      <p id="signin-github-hint" class="invalid" role="alert">{t('signin-github-token-refused')}</p>
                    {:else}
                      <p id="signin-github-hint" class="hint">{model.githubTokenEmpty ? t('signin-github-token-empty') : t('signin-github-token-hint')}</p>
                    {/if}
                    <button type="submit" class="small" data-default="true">{t('signin-github-save')}</button>
                  </form>
                {/if}
                {#if sshOffered}
                  <!-- SM-NS-2.8: git's own traffic with GitHub over SSH; the GitHub API still needs the token. -->
                  <div id="signin-ssh-github" class="way ssh-box field-stack">
                    <h4>{t('signin-way-ssh')}</h4>
                    <p>{t('signin-github-ssh-why')}</p>
                    {#if ssh.keys.length === 0}
                      <p class="hint">{t('signin-github-ssh-no-keys')}</p>
                    {:else}
                      <label>
                        {t('signin-github-ssh-choose')}
                        <select value={ssh.chosen ?? ''} onchange={(e) => void ssh.choose(e.currentTarget.value === '' ? null : e.currentTarget.value)}>
                          <option value="">{t('signin-github-ssh-none')}</option>
                          {#each ssh.keys as key (key.path)}
                            <option value={key.path}>{key.name}</option>
                          {/each}
                        </select>
                      </label>
                    {/if}
                    <p>
                      <button type="button" class="small" disabled={ssh.busy} onclick={() => void ssh.create()}>{t('signin-github-ssh-create')}</button>
                    </p>
                    {#if ssh.chosenKey !== null}
                      {@const key = ssh.chosenKey}
                      <p class="public-key-line">
                        <span class="grow">{t('signin-github-ssh-public')}</span>
                        <button type="button" class="icon-btn" aria-label={t('signin-github-ssh-copy')} title={t('signin-github-ssh-copy')} onclick={() => void ssh.copy()}><Icon name="copy" size="small" /></button>
                      </p>
                      <code class="public-key">{key.public_key}</code>
                      {#if ssh.copied}<p class="hint" role="status">{t('signin-github-ssh-copied')}</p>{/if}
                      <p>{t('signin-github-ssh-add')}</p>
                      <p>
                        <button type="button" class="small" onclick={() => void pages.open('github_ssh_keys')}>{t('signin-github-ssh-open-page')}</button>
                        <button type="button" class="small" disabled={ssh.busy} onclick={() => void ssh.runCheck()}>{t('signin-github-ssh-check')}</button>
                      </p>
                      <p class="check-line" role="status" aria-live="polite">
                        {#if ssh.busy}{t('signin-github-ssh-checking')}{:else if ssh.check === 'works'}{t('signin-github-ssh-works')}{:else if ssh.check === 'refused'}{t('signin-github-ssh-refused')}{:else if ssh.check === 'unreachable'}{t('signin-github-ssh-unreachable')}{/if}
                      </p>
                    {/if}
                    <ErrorLine error={ssh.error} onretry={() => void ssh.load()} />
                  </div>
                {/if}
              {:else if row.service.id === 'gitlab'}
                {#if gitlabOneClick(model.document)}
                  <div class="way">
                    <h4>{t('signin-way-one-click')}</h4>
                    <p><button type="button" class="small" disabled={model.challenge !== null} onclick={() => signIn(row)}>{t('signin-sign-in')}</button></p>
                  </div>
                {/if}
                <form
                  class="way field-stack"
                  onsubmit={(e) => {
                    e.preventDefault();
                    void model.saveGitlabToken();
                  }}
                >
                  <h4>{t('signin-way-token')}</h4>
                  <label>
                    {t('signin-gitlab-paste-token')}
                    <input type="password" bind:value={model.token} autocomplete="off" aria-describedby="signin-gitlab-hint" aria-invalid={model.tokenEmpty ? 'true' : undefined} />
                  </label>
                  <p id="signin-gitlab-hint" class="hint">{model.tokenEmpty ? t('signin-gitlab-token-empty') : t('signin-gitlab-token-hint')}</p>
                  <button type="submit" class="small" data-default="true">{t('signin-gitlab-save')}</button>
                </form>
              {:else if row.service.id === 'confluence'}
                {#if item === null || item.state !== 'signed_in'}
                  {#if confluenceOneClick(model.document) || !confluenceTokenOffered}
                    <div class="way">
                      <h4>{t('signin-way-one-click')}</h4>
                      <p><button type="button" class="small" disabled={model.challenge !== null} onclick={() => signIn(row)}>{t('signin-sign-in')}</button></p>
                    </div>
                  {/if}
                  {#if confluenceTokenOffered}
                    <!-- SM-NS-2.11: the person makes an Atlassian API token and pastes it; checked before it is kept. -->
                    <form
                      id="signin-confluence-token"
                      class="way field-stack"
                      onsubmit={(e) => {
                        e.preventDefault();
                        void confluence.connect();
                      }}
                    >
                      <h4>{t('signin-way-api-token')}</h4>
                      <p>{t('signin-confluence-token-why')}</p>
                      <label>
                        {t('signin-confluence-site')}
                        <input type="text" bind:value={confluence.site} autocomplete="off" placeholder={t('signin-confluence-site-placeholder')} aria-invalid={confluence.siteNeeded ? 'true' : undefined} />
                      </label>
                      {#if confluence.siteNeeded}<p class="invalid" role="alert">{t('signin-confluence-site-needed')}</p>{/if}
                      <label>
                        {t('signin-confluence-email')}
                        <input type="email" bind:value={confluence.email} autocomplete="off" />
                      </label>
                      <p>{t('signin-confluence-token-how')}</p>
                      <p>
                        <button type="button" class="small" onclick={() => void pages.open('atlassian_api_tokens')}>{t('signin-confluence-open-token-page')}</button>
                      </p>
                      <ErrorLine error={pages.error} onretry={() => void pages.retry()} />
                      <label>
                        {t('signin-confluence-token')}
                        <input type="password" bind:value={confluence.token} autocomplete="off" aria-describedby="signin-confluence-hint" aria-invalid={confluence.tokenEmpty ? 'true' : undefined} />
                      </label>
                      <p id="signin-confluence-hint" class="hint">{confluence.tokenEmpty ? t('signin-confluence-token-empty') : t('signin-confluence-token-hint')}</p>
                      <p>
                        <button type="submit" class="small" data-default="true" disabled={confluence.busy}>{t('signin-confluence-connect')}</button>
                        {#if jiraSignedIn !== null}
                          <button type="button" class="small" disabled={confluence.busy} onclick={() => void confluence.useJira(jiraSignedIn.account_label)}>{t('signin-confluence-use-jira', { site: jiraSignedIn.account_label })}</button>
                        {/if}
                      </p>
                      <ErrorLine error={confluence.error} onretry={() => void confluence.retry()} />
                    </form>
                  {/if}
                {/if}
              {:else if item === null || item.state !== 'signed_in'}
                <p class="way"><button type="button" class="small" disabled={model.challenge !== null} onclick={() => signIn(row)}>{t('signin-sign-in')}</button></p>
              {/if}
            </div>
          {/if}
        </li>
      {/if}
    {/each}
  </ul>
  <p class="done-line" role="status" aria-live="polite">{model.done}</p>
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
  <ErrorLine error={model.mailDialog === null ? model.error : null} onretry={() => (retry !== null ? retry() : void model.load())} />
</section>

<style>
  /* One row per service and account (scene `settings`, the prototype's `.lrows` / `.lrow`): the product and
     what it is for in Control text, the account and its state under it in Label text, muted, and the row's
     chevron at the end. A sign-in that waits, and the ways to sign in, take the row's full width. */
  .sign-ins .list-row {
    flex-wrap: wrap;
    align-items: center;
  }
  .sign-ins .list-row > :is(.wait, .ways) {
    flex-basis: 100%;
  }
  /* A service this version cannot sign in to: greyed out, no control (SM-NS-2.9). */
  .sign-ins .list-row.unavailable {
    opacity: 0.55;
  }
  /* The ways to sign in, one sub-section each, inside the row. */
  .ways {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    padding-block-start: var(--space-sm);
  }
  .way h4 {
    margin: 0;
    font-size: 13px;
  }
  .way.signed-in {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    font-size: 13px;
  }
  .wait {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    align-items: center;
    font-size: 13px;
  }
  /* The row's chevron (Diagnostics.svelte `.icon-btn.disclose`): points right when folded, down when open. */
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    min-height: 0;
    padding: 0;
  }
  .disclose :global(.icon) {
    transition: transform 120ms ease;
  }
  .disclose[aria-expanded='true'] :global(.icon) {
    transform: rotate(90deg);
  }
  .public-key-line {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }
  .public-key {
    display: block;
    font-family: var(--font-mono);
    font-size: 12px;
    overflow-wrap: anywhere;
    user-select: all;
  }
  .user-code {
    font-family: var(--font-mono);
    font-size: 20px;
    letter-spacing: 0.1em;
  }
</style>
