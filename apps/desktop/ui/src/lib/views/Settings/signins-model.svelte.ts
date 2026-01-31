// Settings › Sign-ins and Settings › Jira: the person's accounts (`identity.list`), signing in through the
// browser (`identity.sign_in` then `identity.sign_in_wait`), signing out, the GitLab token box
// (SA-CLOSE-03) and Jira's API-token connection (ADR-106).
//
// spec: ordin-desktop/components/ScreensModule#SA-CLOSE-02
// spec: ordin-desktop/components/ScreensModule#SA-CLOSE-03
// spec: ordin-desktop/components/ScreensModule#SM-NS-2
import type { BackendHandle } from '../../backend/backend';
import type { BackendState } from '../../store/backend-state.svelte';
import type { ClientCertificateView, CredentialKind, CredentialView, MailPasswordKind, ServiceError, SignInChallenge } from '../../service-api/types.gen';

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
  /** The kinds whose row shows the token box (a GitLab sign-in the organisation has not configured). */
  tokenBox = $state.raw<ReadonlySet<CredentialKind>>(new Set());
  token = $state('');
  /** The sign-in now waiting for the person, and the kind it is for (SM-NS-2.2). */
  challenge = $state.raw<SignInChallenge | null>(null);
  challengeKind = $state<CredentialKind | null>(null);
  /** The mail password dialog: the kind being signed in, or `null`. */
  mailDialog = $state.raw<CredentialView | { kind: CredentialKind; account_label: string } | null>(null);
  mailAddress = $state('');
  mailPassword = $state('');
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
      this.error = null;
    } else this.error = out.error;
  }

  /** Reads whether the profile requires a client certificate; the local backend only (SM-NS-2.4). */
  async loadCertificateRequirement(): Promise<void> {
    const out = await this.#handle.call('profile.get_document', {});
    this.certificateRequired = out.kind === 'ok' && out.result.document.server?.client_certificate_required === true;
  }

  /**
   * Browser or device-code sign-in; `CRED_PROVIDER_NOT_CONFIGURED` for GitLab switches the row to the token
   * box (SA-CLOSE-03). `CRED_SIGN_IN_CANCELLED` draws nothing; `CRED_SIGN_IN_TIMEOUT` is drawn with **Try again**.
   */
  async signIn(kind: CredentialKind, site?: string): Promise<'ok' | 'not_configured' | 'error'> {
    this.error = null;
    const out = await this.#handle.call('identity.sign_in', { kind, ...(site === undefined ? {} : { site }) });
    if (out.kind === 'error') {
      if (out.error.code === 'CRED_PROVIDER_NOT_CONFIGURED') {
        if (kind === 'gitlab') this.tokenBox = new Set([...this.tokenBox, kind]);
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

  /** **Cancel** while a challenge waits. */
  async cancelSignIn(): Promise<void> {
    const challenge = this.challenge;
    if (challenge === null || !('challenge_id' in challenge)) return;
    await this.#handle.call('identity.sign_in_cancel', { challenge_id: challenge.challenge_id });
  }

  /** **Sign in** of a row: a mail password kind opens its dialog, any other kind starts the sign-in. */
  async startSignIn(item: CredentialView): Promise<void> {
    if (item.kind in MAIL_PASSWORD_KINDS) this.openMailDialog(item);
    else await this.signIn(item.kind);
  }

  openMailDialog(item: { kind: CredentialKind; account_label: string }): void {
    this.mailDialog = item;
    this.mailAddress = item.account_label;
    this.mailPassword = '';
    this.error = null;
  }

  /** **Sign in** of the mail dialog; the password field is cleared after sending. */
  async saveMailPassword(): Promise<boolean> {
    const dialog = this.mailDialog;
    const kind = dialog === null ? undefined : MAIL_PASSWORD_KINDS[dialog.kind];
    if (dialog === null || kind === undefined) return false;
    const password = this.mailPassword;
    this.mailPassword = '';
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

  async signOut(credentialId: string): Promise<void> {
    const out = await this.#handle.call('identity.sign_out', { credential_id: credentialId });
    if (out.kind === 'error') this.error = out.error;
    await this.load();
  }

  /** The box is a password field, cleared after sending. */
  async saveGitlabToken(): Promise<void> {
    const token = this.token;
    this.token = '';
    if (token.trim() === '') return;
    const out = await this.#handle.call('identity.save_gitlab_token', { token });
    if (out.kind === 'error') this.error = out.error;
    else {
      this.tokenBox = new Set([...this.tokenBox].filter((k) => k !== 'gitlab'));
      await this.load();
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
  /** SM-JS-2: the site field holds no site, so nothing was sent. */
  siteNeeded = $state(false);
  connection = $state.raw<CredentialView | null>(null);
  /** The site and name `jira.connect_token` returned, kept for the session. */
  last = $state.raw<{ site: string; name: string } | null>(null);
  confirmDisconnect = $state(false);
  readonly #handle: BackendHandle;
  readonly #signIns: SignInsModel;
  /** The last input sent, held in memory so **Try again** re-sends the same command (the field itself is cleared). */
  #lastInput: { site: string; email: string; token: string } | null = null;

  constructor(handle: BackendHandle, signIns: SignInsModel = new SignInsModel(handle)) {
    this.#handle = handle;
    this.#signIns = signIns;
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

  async disconnect(): Promise<void> {
    this.confirmDisconnect = false;
    const connection = this.connection;
    if (connection === null) return;
    await this.#signIns.signOut(connection.id);
    await this.load();
  }
}
