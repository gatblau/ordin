// Settings › Sign-ins and Settings › Jira: the person's accounts (`identity.list`), signing in through the
// browser (`identity.sign_in` then `identity.sign_in_wait`), signing out, the rows' ways to sign in shown
// and hidden by one chevron (SM-NS-2.7), the GitHub and GitLab tokens (SM-NS-2.6, SA-CLOSE-03) and Jira's
// API-token connection (ADR-106).
//
// spec: ordin-desktop/components/ScreensModule#SA-CLOSE-02
// spec: ordin-desktop/components/ScreensModule#SA-CLOSE-03
// spec: ordin-desktop/components/ScreensModule#SM-NS-2
import type { BackendHandle } from '../../backend/backend';
import type { BackendState } from '../../store/backend-state.svelte';
import type { ClientCertificateView, CredentialKind, CredentialView, MailPasswordKind, MailSignInMethod, OrganisationProfileDocument, ServiceError, SignInChallenge, SshCheckOutcome, SshKey } from '../../service-api/types.gen';

/** Re-reads `identity.list` on every `sign_in_changed` (SA-CLOSE-02). Returns the unsubscribe function. */
export function reloadOnSignIn(backend: BackendState, reload: () => Promise<void>): () => void {
  return backend.subscribe((env) => {
    if (env.kind === 'sign_in_changed') void reload();
  });
}

/** The mail kinds whose sign-in is a password in a dialog, not the browser (SM-NS-2.3). */
export const MAIL_PASSWORD_KINDS: Readonly<Partial<Record<CredentialKind, MailPasswordKind>>> = {
  mail_server_password: 'mail_password',
  mail_server_app_password: 'app_password',
};

/** The id of the account's product name: `signin-kind-<kind>` with `_` written `-`. */
export const kindMessageId = (kind: CredentialKind): string => `signin-kind-${kind.replaceAll('_', '-')}`;

export class SignInsModel {
  items = $state.raw<CredentialView[]>([]);
  error = $state<ServiceError | null>(null);
  /** GitLab's token field. */
  token = $state('');
  /**
   * The rows whose ways to sign in are shown, by row key (SM-NS-2.7; amended January 2026; user decision;
   * Approved by the user January 2026).
   */
  expanded = $state.raw<ReadonlySet<string>>(new Set());
  /** `identity.list`'s `github_one_click`: this build carries a GitHub App, so one-click sign-in is offered. */
  githubOneClick = $state(false);
  /** The sign-in now waiting for the person, and the kind it is for (SM-NS-2.2). */
  challenge = $state.raw<SignInChallenge | null>(null);
  challengeKind = $state<CredentialKind | null>(null);
  /** The mail password dialog: the kind being signed in, or `null`. */
  mailDialog = $state.raw<CredentialView | { kind: CredentialKind; account_label: string } | null>(null);
  mailAddress = $state('');
  mailPassword = $state('');
  /** A field of a dialog or box left empty, so nothing was sent (amended January 2026 (user decision)). */
  mailPasswordEmpty = $state(false);
  tokenEmpty = $state(false);
  /** GitHub's token box (SM-NS-2.6), kept apart from GitLab's. */
  githubToken = $state('');
  githubTokenEmpty = $state(false);
  /** GitHub did not accept the pasted token (`CRED_PROVIDER_REFUSED`): the box says how to make a new one. */
  githubTokenRefused = $state(false);
  /** The organisation profile, when read (`profile.get_document`); `null` lists every service as used. */
  document = $state.raw<OrganisationProfileDocument | null>(null);
  /** The one line saying what the last action did (a sign-out), read out politely. */
  done = $state('');
  /** True when the profile requires a client certificate (`document.server.client_certificate_required`). */
  certificateRequired = $state(false);
  certificate = $state.raw<ClientCertificateView | null>(null);
  certificateError = $state<ServiceError | null>(null);
  readonly #handle: BackendHandle;
  loads = 0;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  async load(): Promise<void> {
    this.loads++;
    const out = await this.#handle.call('identity.list', {});
    if (out.kind === 'ok') {
      this.items = out.result.items;
      this.githubOneClick = out.result.github_one_click === true;
      this.error = null;
    } else this.error = out.error;
  }

  /** Reads whether the profile requires a client certificate; the local backend only (SM-NS-2.4). */
  async loadCertificateRequirement(): Promise<void> {
    const out = await this.#handle.call('profile.get_document', {});
    this.document = out.kind === 'ok' ? out.result.document : null;
    this.certificateRequired = out.kind === 'ok' && out.result.document.server?.client_certificate_required === true;
  }

  /**
   * Browser or device-code sign-in. `CRED_PROVIDER_NOT_CONFIGURED` for GitLab and GitHub draws no sentence:
   * their rows already offer the token (SA-CLOSE-03, SM-NS-2.6), and for GitHub the one-click way is no
   * longer drawn; for Jira it is returned to the caller, and for any other kind is drawn as its sentence.
   * `CRED_SIGN_IN_CANCELLED` draws nothing; `CRED_SIGN_IN_TIMEOUT` is drawn with **Try again**.
   */
  async signIn(kind: CredentialKind, site?: string, mailAccount?: string): Promise<'ok' | 'not_configured' | 'error'> {
    this.error = null;
    this.done = '';
    const out = await this.#handle.call('identity.sign_in', {
      kind,
      ...(site === undefined ? {} : { site }),
      ...(mailAccount === undefined ? {} : { mail_account: mailAccount }),
    });
    if (out.kind === 'error') {
      // GitLab and GitHub answer `CRED_PROVIDER_NOT_CONFIGURED` with their token way (SA-CLOSE-03, SM-NS-2.6)
      // and Jira with Settings › Jira's own line; for any other kind the sentence is drawn, so **Sign in**
      // never does nothing (amended January 2026; user decision).
      if (out.error.code === 'CRED_PROVIDER_NOT_CONFIGURED' && (kind === 'gitlab' || kind === 'github' || kind === 'jira')) {
        if (kind === 'github') this.githubOneClick = false;
        return 'not_configured';
      }
      this.#refuse(out.error);
      return 'error';
    }
    if (out.result.kind !== 'already_signed_in') {
      this.challenge = out.result;
      this.challengeKind = kind;
      const wait = await this.#handle.call('identity.sign_in_wait', { challenge_id: out.result.challenge_id });
      this.challenge = null;
      this.challengeKind = null;
      if (wait.kind === 'error') {
        this.#refuse(wait.error);
        return 'error';
      }
    }
    await this.load();
    return 'ok';
  }

  #refuse(error: ServiceError): void {
    if (error.code === 'CRED_SIGN_IN_CANCELLED') this.error = null;
    else if (error.code === 'CRED_SIGN_IN_TIMEOUT') this.error = { ...error, action: 'try_again' };
    else this.error = error;
  }

  /** Whether the row's ways to sign in are shown. */
  isOpen(key: string): boolean {
    return this.expanded.has(key);
  }

  /** The row's chevron: shows the ways to sign in when they are hidden, and hides them when shown. */
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2.7
  toggle(key: string): boolean {
    const open = !this.expanded.has(key);
    this.expanded = open ? new Set([...this.expanded, key]) : new Set([...this.expanded].filter((k) => k !== key));
    return open;
  }

  /** **Cancel** while a challenge waits. */
  async cancelSignIn(): Promise<void> {
    const challenge = this.challenge;
    if (challenge === null || !('challenge_id' in challenge)) return;
    await this.#handle.call('identity.sign_in_cancel', { challenge_id: challenge.challenge_id });
  }

  /**
   * **Sign in** of a row: a mail password kind opens its dialog; the company mail server's single sign-on
   * opens the dialog for the address when none is saved, and signs in with the saved one otherwise; any other
   * kind starts the sign-in.
   */
  async startSignIn(item: CredentialView): Promise<void> {
    if (item.kind in MAIL_PASSWORD_KINDS) this.openMailDialog(item);
    else if (item.kind === 'mail_server_single_sign_on') {
      if (item.account_label.trim() === '') this.openMailDialog(item);
      else await this.signIn(item.kind, undefined, item.account_label);
    } else await this.signIn(item.kind);
  }

  /** **Sign in** of a service with no saved account (SM-NS-2.5): a kind with nothing saved goes through `identity.sign_in` as any other. */
  async startNew(kind: CredentialKind): Promise<void> {
    await this.startSignIn({ id: '', kind, account_label: '', state: 'signed_out' });
  }

  /** The company mail server with no saved account: the dialog for the profile's first sign-in method. */
  openMailFor(method: MailSignInMethod, address = ''): void {
    const kind: CredentialKind = method === 'single_sign_on' ? 'mail_server_single_sign_on' : method === 'app_password' ? 'mail_server_app_password' : 'mail_server_password';
    this.openMailDialog({ kind, account_label: address });
  }

  openMailDialog(item: { kind: CredentialKind; account_label: string }): void {
    this.mailDialog = item;
    this.mailAddress = item.account_label;
    this.mailPassword = '';
    this.mailPasswordEmpty = false;
    this.error = null;
  }

  /** The dialog asks for the address only (single sign-on): its **Sign in** goes on in the browser. */
  get mailSingleSignOn(): boolean {
    return this.mailDialog?.kind === 'mail_server_single_sign_on';
  }

  /** **Sign in** of the mail dialog; the password field is cleared after sending. */
  async saveMailPassword(): Promise<boolean> {
    const dialog = this.mailDialog;
    if (dialog !== null && dialog.kind === 'mail_server_single_sign_on') {
      const address = this.mailAddress.trim();
      if (address === '') return false;
      this.mailDialog = null;
      return (await this.signIn('mail_server_single_sign_on', undefined, address)) === 'ok';
    }
    const kind = dialog === null ? undefined : MAIL_PASSWORD_KINDS[dialog.kind];
    if (dialog === null || kind === undefined) return false;
    const password = this.mailPassword;
    this.mailPassword = '';
    // An empty password is not sent, and the dialog says so (amended January 2026 (user decision)).
    this.mailPasswordEmpty = password === '';
    if (this.mailAddress.trim() === '' || password === '') return false;
    const out = await this.#handle.call('identity.save_mail_password', { account_label: this.mailAddress.trim(), kind, password });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    this.mailDialog = null;
    await this.load();
    return true;
  }

  /** **Import a certificate…**; a result without a certificate (cancelled) draws nothing. */
  async importCertificate(): Promise<void> {
    this.certificateError = null;
    const out = await this.#handle.call('identity.import_client_certificate', {});
    if (out.kind === 'error') {
      this.certificateError = out.error;
      return;
    }
    if (out.result.certificate != null) this.certificate = out.result.certificate;
  }

  /** **Sign out** at once; `done` then holds the line that says so, given by the caller. */
  async signOut(credentialId: string, doneLine = ''): Promise<boolean> {
    const out = await this.#handle.call('identity.sign_out', { credential_id: credentialId });
    if (out.kind === 'error') this.error = out.error;
    else this.done = doneLine;
    await this.load();
    return out.kind === 'ok';
  }

  /**
   * GitHub's token box (SM-NS-2.6): a password field, cleared after sending; an empty box sends nothing and
   * says so. GitHub checks the token before it is kept; a refusal is drawn with its sentence.
   */
  // spec: ordin-desktop/components/ScreensModule#SM-NS-2.6
  async saveGithubToken(): Promise<boolean> {
    const token = this.githubToken;
    this.githubToken = '';
    this.githubTokenEmpty = token.trim() === '';
    this.githubTokenRefused = false;
    if (this.githubTokenEmpty) return false;
    this.error = null;
    const out = await this.#handle.call('identity.save_github_token', { token: token.trim() });
    if (out.kind === 'error') {
      if (out.error.code === 'CRED_PROVIDER_REFUSED') this.githubTokenRefused = true;
      else this.error = out.error;
      return false;
    }
    await this.load();
    return true;
  }

  /** The box is a password field, cleared after sending; an empty box sends nothing and says so. */
  async saveGitlabToken(): Promise<void> {
    const token = this.token;
    this.token = '';
    this.tokenEmpty = token.trim() === '';
    if (this.tokenEmpty) return;
    const out = await this.#handle.call('identity.save_gitlab_token', { token });
    if (out.kind === 'error') this.error = out.error;
    else await this.load();
  }
}

// ---- GitHub with an SSH key ---------------------------------------------------------------------------

/**
 * GitHub's SSH key options (SM-NS-2.8; amended January 2026; user decision; Approved by the user January
 * 2026): the keys in the person's SSH folder (`app.ssh_keys`), a new key (`app.create_ssh_key`), the
 * choice (`app.choose_ssh_key`) and **Check it works** (`app.check_ssh_key`). Only on this computer.
 */
// spec: ordin-desktop/components/ScreensModule#SM-NS-2.8
export class SshKeyModel {
  keys = $state.raw<SshKey[]>([]);
  chosen = $state<string | null>(null);
  busy = $state(false);
  check = $state<SshCheckOutcome | null>(null);
  copied = $state(false);
  error = $state<ServiceError | null>(null);
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  /** The chosen key as listed, or `null` when none is chosen or it is no longer in the folder. */
  get chosenKey(): SshKey | null {
    return this.keys.find((k) => k.path === this.chosen) ?? null;
  }

  #take(view: { keys: SshKey[]; chosen?: string | null }): void {
    this.keys = view.keys;
    this.chosen = view.chosen ?? null;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('app.ssh_keys', {});
    if (out.kind === 'ok') {
      this.#take(out.result);
      this.error = null;
    } else this.error = out.error;
  }

  /** **Make a new key**: made, then chosen; the result of an earlier check no longer applies. */
  async create(): Promise<void> {
    this.busy = true;
    this.error = null;
    const out = await this.#handle.call('app.create_ssh_key', {});
    this.busy = false;
    if (out.kind === 'ok') {
      this.#take(out.result);
      this.check = null;
    } else this.error = out.error;
  }

  /** The key list's choice; `null` goes back to the token alone. */
  async choose(path: string | null): Promise<void> {
    this.error = null;
    const out = await this.#handle.call('app.choose_ssh_key', { path });
    if (out.kind === 'ok') {
      this.#take(out.result);
      this.check = null;
    } else this.error = out.error;
  }

  /** **Check it works**: the outcome is drawn as one plain line. */
  async runCheck(): Promise<void> {
    this.busy = true;
    this.check = null;
    this.error = null;
    const out = await this.#handle.call('app.check_ssh_key', {});
    this.busy = false;
    if (out.kind === 'ok') this.check = out.result.outcome;
    else this.error = out.error;
  }

  /** **Copy the public key**; the clipboard may be unavailable, and the key stays on screen to select. */
  async copy(): Promise<void> {
    const key = this.chosenKey;
    if (key === null) return;
    try {
      await navigator.clipboard.writeText(key.public_key);
      this.copied = true;
    } catch {
      this.copied = false;
    }
  }
}

// ---- Jira -----------------------------------------------------------------------------------------

/**
 * The site field read as a host (SM-JS-1): spaces at the ends, a leading `http://` or `https://` and
 * everything from the first `/`, `?` or `#` are removed, then a trailing `.`, and the rest is put in lower
 * case, so an address pasted from the browser is sent as the host `jira.connect_token` checks.
 */
// spec: ordin-desktop/components/ScreensModule#SM-JS-1
export function jiraHost(text: string): string {
  const host = text.trim().replace(/^https?:\/\//i, '').split(/[/?#]/)[0] ?? '';
  return host.replace(/\.$/, '').toLowerCase();
}


export class JiraModel {
  site = $state('');
  email = $state('');
  token = $state('');
  busy = $state(false);
  error = $state<ServiceError | null>(null);
  /** `CRED_DUPLICATE`: already connected to another Jira site. */
  otherSite = $state(false);
  oauthUnavailable = $state(false);
  /**
   * The organisation profile names no Atlassian app (`confluence` absent), so **Sign in with Atlassian…**
   * could never complete and is not drawn (SM-NS-2.10).
   */
  oauthNotOffered = $state(false);
  /** SM-JS-2: the site field holds no site, so nothing was sent. */
  siteNeeded = $state(false);
  connection = $state.raw<CredentialView | null>(null);
  /** The site and name `jira.connect_token` returned, kept for the session. */
  last = $state.raw<{ site: string; name: string } | null>(null);
  confirmDisconnect = $state(false);
  /** The line that says what **Disconnect** did. */
  done = $state('');
  readonly #handle: BackendHandle;
  readonly #signIns: SignInsModel;
  /** The last input sent, held in memory so **Try again** re-sends the same command (the field itself is cleared). */
  #lastInput: { site: string; email: string; token: string } | null = null;

  constructor(handle: BackendHandle, signIns: SignInsModel = new SignInsModel(handle)) {
    this.#handle = handle;
    this.#signIns = signIns;
  }

  /** Reads the organisation profile: with no Atlassian app named, only the API token is offered. */
  async loadProfile(): Promise<void> {
    const out = await this.#handle.call('profile.get_document', {});
    this.oauthNotOffered = out.kind === 'ok' && out.result.document.confluence == null;
  }

  async load(): Promise<void> {
    await this.#signIns.load();
    this.connection = this.#signIns.items.find((c) => c.kind === 'jira' && c.state === 'signed_in') ?? null;
    if (this.connection === null) this.last = null;
  }

  /** **Connect**: the token field is cleared whatever the reply; focus stays in the form. */
  async connect(retry = false): Promise<boolean> {
    const input = retry && this.#lastInput !== null ? this.#lastInput : { site: jiraHost(this.site), email: this.email.trim(), token: this.token };
    // spec: ordin-desktop/components/ScreensModule#SM-JS-2
    this.siteNeeded = input.site === '';
    if (this.siteNeeded) return false;
    this.#lastInput = input;
    this.token = '';
    this.error = null;
    this.otherSite = false;
    this.busy = true;
    const out = await this.#handle.call('jira.connect_token', input);
    this.busy = false;
    if (out.kind === 'ok') {
      this.last = { site: input.site, name: out.result.display_name };
      this.#lastInput = null;
      await this.load();
      return true;
    }
    if (out.error.code === 'CRED_DUPLICATE') this.otherSite = true;
    else this.error = out.error;
    return false;
  }

  /** **Sign in with Atlassian…**: `CRED_PROVIDER_NOT_CONFIGURED` replaces the button for the rest of the session. */
  async signInWithAtlassian(): Promise<void> {
    const site = jiraHost(this.site);
    this.siteNeeded = site === '';
    if (this.siteNeeded) return;
    const out = await this.#signIns.signIn('jira', `https://${site}`);
    if (out === 'not_configured') this.oauthUnavailable = true;
    else if (out === 'ok') await this.load();
    else this.error = this.#signIns.error;
  }

  async disconnect(doneLine = ''): Promise<void> {
    this.confirmDisconnect = false;
    const connection = this.connection;
    if (connection === null) return;
    const ok = await this.#signIns.signOut(connection.id);
    await this.load();
    this.done = ok ? doneLine : '';
    if (!ok) this.error = this.#signIns.error;
  }
}

// ---- Confluence with an API token -----------------------------------------------------------------

/**
 * Settings › Sign-ins › Confluence, **With an API token** (SM-NS-2.11; amended January 2026; user decision;
 * Approved by the user January 2026): the site, the email and the API token, **Connect**
 * (`confluence.connect_token`, which checks them with Confluence before keeping the token in the keychain)
 * and, when Jira is connected with an API token, **Use my Jira sign-in** (`confluence.connect_with_jira`).
 * The token field is a password field, cleared whatever the reply; an empty site or token sends nothing and
 * says so. **Disconnect** is the row's sign-out.
 */
// spec: ordin-desktop/components/ScreensModule#SM-NS-2.11
export class ConfluenceTokenModel {
  site = $state('');
  email = $state('');
  token = $state('');
  busy = $state(false);
  error = $state<ServiceError | null>(null);
  /** The site field holds no site, so nothing was sent. */
  siteNeeded = $state(false);
  /** The token field was empty, so nothing was sent. */
  tokenEmpty = $state(false);
  /** The site and name the last connection returned, kept for the session. */
  last = $state.raw<{ site: string; name: string } | null>(null);
  readonly #handle: BackendHandle;
  readonly #signIns: SignInsModel;
  /** The last input sent, held in memory so **Try again** re-sends it (the field itself is cleared). */
  #lastInput: { site: string; email: string; token: string } | null = null;

  constructor(handle: BackendHandle, signIns: SignInsModel) {
    this.#handle = handle;
    this.#signIns = signIns;
  }

  /** **Connect**: the site is read as a host the way Settings › Jira reads it. */
  async connect(retry = false): Promise<boolean> {
    const input = retry && this.#lastInput !== null ? this.#lastInput : { site: jiraHost(this.site), email: this.email.trim(), token: this.token };
    this.token = '';
    this.siteNeeded = input.site === '';
    this.tokenEmpty = input.token.trim() === '';
    if (this.siteNeeded || this.tokenEmpty) return false;
    this.#lastInput = input;
    this.error = null;
    this.busy = true;
    const out = await this.#handle.call('confluence.connect_token', input);
    this.busy = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.#lastInput = null;
    this.last = { site: input.site, name: out.result.display_name };
    await this.#signIns.load();
    return true;
  }

  /** **Use my Jira sign-in**: the Jira API token's site, email and token, checked with Confluence. */
  async useJira(site: string): Promise<boolean> {
    this.error = null;
    this.siteNeeded = false;
    this.tokenEmpty = false;
    this.busy = true;
    const out = await this.#handle.call('confluence.connect_with_jira', {});
    this.busy = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.last = { site, name: out.result.display_name };
    await this.#signIns.load();
    return true;
  }

  /** **Try again** after a refusal re-sends the same input. */
  async retry(): Promise<void> {
    await this.connect(true);
  }
}
