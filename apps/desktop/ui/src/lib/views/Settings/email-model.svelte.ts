// Settings › Email (11 §5.22): the mailbox line, the signature, the footer switch, how often to check and
// the email links of each workspace. Each change sends `mail.set_settings` with the whole
// `MailSettingsUpdate`; the control keeps the person's choice while the command is in flight and returns to
// the stored value on a refusal.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-1
import type { BackendHandle } from '../../backend/backend';
import type { CheckSchedule, CredentialKind, CredentialView, EmailLink, MailSettings, MailSettingsUpdate, OrganisationProfileDocument, ServiceError, SignInCheck } from '../../service-api/types.gen';
import { mailMethod } from './services';
import { SignInsModel } from './signins-model.svelte';

/** The kinds of account that make a mailbox. */
export const MAILBOX_KINDS: readonly CredentialKind[] = ['microsoft', 'google', 'mail_server_single_sign_on', 'mail_server_password', 'mail_server_app_password'];
export const PASSWORD_KINDS: readonly CredentialKind[] = ['mail_server_password', 'mail_server_app_password'];
export const SIGNATURE_MAX = 2000;

export interface WorkspaceLinks {
  id: string;
  name: string;
  links: EmailLink[];
}

export class EmailModel {
  settings = $state.raw<MailSettings | null>(null);
  mailbox = $state.raw<CredentialView | null>(null);
  check = $state<SignInCheck | null>(null);
  workspaces = $state.raw<WorkspaceLinks[]>([]);
  signature = $state('');
  error = $state<ServiceError | null>(null);
  /** The mail password dialog (SM-NS-2.3) opened by **Change password**. */
  changingPassword = $state(false);
  /** The line that says what **Sign out of email** did. */
  done = $state('');
  /** The signature was saved on leaving its box. */
  signatureSaved = $state(false);
  /** The organisation profile, when it can be read: which mailbox **Sign in** signs in to. */
  document = $state.raw<OrganisationProfileDocument | null>(null);
  readonly signIns: SignInsModel;
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle, signIns: SignInsModel = new SignInsModel(handle)) {
    this.#handle = handle;
    this.signIns = signIns;
  }

  get passwordKind(): boolean {
    return this.mailbox !== null && PASSWORD_KINDS.includes(this.mailbox.kind);
  }

  async load(): Promise<void> {
    const settings = await this.#handle.call('mail.settings', {});
    if (settings.kind === 'error') {
      this.error = settings.error;
      return;
    }
    this.settings = settings.result;
    this.signature = settings.result.signature;
    this.error = null;
    await this.signIns.load();
    const profile = await this.#handle.call('profile.get_document', {});
    this.document = profile.kind === 'ok' ? profile.result.document : null;
    const mine = this.signIns.items.filter((c) => MAILBOX_KINDS.includes(c.kind));
    this.mailbox = mine.find((c) => c.state === 'signed_in') ?? mine[0] ?? null;
    this.check = null;
    if (this.mailbox !== null) {
      const verified = await this.#handle.call('mail.verify_sign_in', {});
      if (verified.kind === 'ok') this.check = verified.result;
    }
    await this.loadWorkspaces();
  }

  /** Per workspace its email links by `display_name`; a workspace with none is not listed. */
  async loadWorkspaces(): Promise<void> {
    const list = await this.#handle.call('workspaces.list', { filter: 'active' });
    if (list.kind === 'error') {
      this.error = list.error;
      return;
    }
    const out: WorkspaceLinks[] = [];
    for (const w of list.result) {
      const links = await this.#handle.call('workspaces.email_links', { workspace_id: w.id });
      if (links.kind === 'ok' && links.result.length > 0) out.push({ id: w.id, name: w.name, links: links.result });
    }
    this.workspaces = out;
  }

  /** Sends the whole update with `patch` laid over the stored values. */
  async update(patch: Partial<MailSettingsUpdate>): Promise<boolean> {
    const stored = this.settings;
    if (stored === null) return false;
    const next: MailSettingsUpdate = {
      add_signature: stored.add_signature,
      agent_foot_line: stored.agent_foot_line,
      check: stored.check,
      signature: stored.signature,
      ...patch,
    };
    // The control shows the choice while the command is in flight.
    this.settings = { ...stored, ...next };
    const out = await this.#handle.call('mail.set_settings', next);
    if (out.kind === 'ok') {
      this.settings = out.result;
      this.signature = out.result.signature;
      this.error = null;
      return true;
    }
    this.settings = stored;
    this.signature = stored.signature;
    this.error = out.error;
    return false;
  }

  setAddSignature(on: boolean): Promise<boolean> {
    return this.update({ add_signature: on });
  }
  setFootLine(on: boolean): Promise<boolean> {
    return this.update({ agent_foot_line: on });
  }
  setCheck(check: CheckSchedule): Promise<boolean> {
    return this.update({ check });
  }
  /** Saved on leaving the box, and only when it changed; then "Saved" shows under it. */
  async saveSignature(): Promise<boolean> {
    const text = [...this.signature].slice(0, SIGNATURE_MAX).join('');
    if (this.settings === null || text === this.settings.signature) return true;
    this.signatureSaved = false;
    const saved = await this.update({ signature: text });
    this.signatureSaved = saved;
    return saved;
  }

  /** **Sign out of email**: at once, then the line that says what it did (11 §5.22, as decided). */
  async signOut(doneLine = ''): Promise<void> {
    const mailbox = this.mailbox;
    if (mailbox === null) return;
    const ok = await this.signIns.signOut(mailbox.id);
    if (this.signIns.error !== null) this.error = this.signIns.error;
    await this.load();
    this.done = ok ? doneLine : '';
  }

  /**
   * **Sign in** with no mailbox: by the profile's mail system, Microsoft 365 or Google through the browser,
   * the company's mail server through the mail sign-in dialog of its first sign-in method.
   */
  async signIn(): Promise<void> {
    this.done = '';
    const system = this.document?.email?.system ?? 'company_server';
    if (system === 'microsoft365' || system === 'google_workspace') {
      const out = await this.signIns.signIn(system === 'microsoft365' ? 'microsoft' : 'google');
      if (out === 'error') this.error = this.signIns.error;
      await this.load();
    } else this.signIns.openMailFor(mailMethod(this.document));
  }

  /** The mail password dialog saved a password: the line and the check are read again. */
  async passwordSaved(): Promise<void> {
    this.changingPassword = false;
    await this.load();
  }
}
