// The state of the three email screens (SM-NS-12): a link's message list, one message and one draft.
// Mail text is other people's text: it is kept as plain strings and drawn as text nodes.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-12
import { askOrdin } from '../../blocks/ask';
import type { EmailDraft, EmailLink, EmailRecipient, MessageSummary, MessageView, ServiceError } from '../../service-api/types.gen';
import type { BackendState } from '../../store/backend-state.svelte';
import type { Handle } from '../fixed/util';

/** The name a recipient is shown by: the name, else the address. */
export const who = (r: EmailRecipient): string => (r.name != null && r.name !== '' ? r.name : r.address);

/** `email-message:<email link id>:<message ref>` → its two parts; the link id has no colon. */
export function splitMessageInstance(instance: string): { linkId: string; messageRef: string } {
  const at = instance.indexOf(':');
  return at < 0 ? { linkId: instance, messageRef: '' } : { linkId: instance.slice(0, at), messageRef: instance.slice(at + 1) };
}

/** Paragraphs of a text, split on blank lines. */
export const paragraphs = (text: string): string[] => text.split(/\n\s*\n/).filter((p) => p.trim() !== '');

export class EmailListController {
  link = $state.raw<EmailLink | null>(null);
  messages = $state.raw<MessageSummary[]>([]);
  nextCursor = $state<string | null>(null);
  loaded = $state(false);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly #workspaceId: string;
  readonly #linkId: string;

  constructor(handle: Handle, workspaceId: string, linkId: string) {
    this.#handle = handle;
    this.#workspaceId = workspaceId;
    this.#linkId = linkId;
  }

  /** Marks the link looked at, names it, and reads the first page (50 a page, newest first). */
  async load(): Promise<void> {
    await this.#handle.call('mail.mark_looked', { link: this.#linkId });
    const links = await this.#handle.call('workspaces.email_links', { workspace_id: this.#workspaceId });
    if (links.kind === 'ok') this.link = links.result.find((l) => l.id === this.#linkId) ?? null;
    else this.error = links.error;
    this.messages = [];
    this.nextCursor = null;
    await this.#page(null);
    this.loaded = true;
  }

  async showMore(): Promise<void> {
    if (this.nextCursor !== null) await this.#page(this.nextCursor);
  }

  async #page(cursor: string | null): Promise<void> {
    const out = await this.#handle.call('mail.list_messages', {
      workspace_id: this.#workspaceId,
      email_link_id: this.#linkId,
      ...(cursor === null ? {} : { cursor }),
    });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.messages = [...this.messages, ...out.result.items];
    this.nextCursor = out.result.next_cursor ?? null;
  }
}

export class EmailMessageController {
  message = $state.raw<MessageView | null>(null);
  saved = $state(false);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly #workspaceId: string;
  readonly #messageRef: string;

  constructor(handle: Handle, workspaceId: string, messageRef: string) {
    this.#handle = handle;
    this.#workspaceId = workspaceId;
    this.#messageRef = messageRef;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('mail.open_message', { message_ref: this.#messageRef, workspace_id: this.#workspaceId });
    if (out.kind === 'ok') {
      this.message = out.result;
      this.error = null;
    } else {
      this.error = out.error;
    }
  }

  async saveAttachment(index: number): Promise<void> {
    const out = await this.#handle.call('mail.save_attachment', { workspace_id: this.#workspaceId, message_ref: this.#messageRef, index });
    this.saved = out.kind === 'ok';
    this.error = out.kind === 'error' ? out.error : null;
  }

  /** **Reply with Ordin**: the conversation message names the sender and carries the message reference. */
  async replyWithOrdin(backend: BackendState, text: (sender: string, ref: string) => string): Promise<void> {
    if (this.message === null) return;
    const out = await askOrdin(backend, this.#workspaceId, text(who(this.message.from), this.message.message_ref));
    this.error = out.kind === 'error' ? out.error : null;
  }
}

export class EmailDraftController {
  draft = $state.raw<EmailDraft | null>(null);
  error = $state.raw<ServiceError | null>(null);
  notice = $state<'saved_to_drafts' | null>(null);
  busy = $state(false);
  readonly #handle: Handle;
  readonly #id: string;

  constructor(handle: Handle, id: string) {
    this.#handle = handle;
    this.#id = id;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('mail.get_draft', { id: this.#id });
    if (out.kind === 'ok') this.draft = out.result;
    else this.error = out.error;
  }

  /** Sends nothing itself: `mail.send` raises the approval card. */
  async send(): Promise<boolean> {
    return this.#run('mail.send');
  }

  async saveToMailDrafts(): Promise<boolean> {
    const ok = await this.#run('mail.save_to_mail_drafts');
    this.notice = ok ? 'saved_to_drafts' : null;
    return ok;
  }

  async discard(): Promise<boolean> {
    return this.#run('mail.discard_draft');
  }

  /** **Save** in the editor: To and Subject above the body; the rest of the draft is sent as it is. */
  async edit(fields: { to: string[]; subject: string; body: string }): Promise<boolean> {
    const d = this.draft;
    if (d === null || this.busy) return false;
    this.busy = true;
    const out = await this.#handle.call('mail.edit_draft', {
      draft_id: d.id,
      to: fields.to,
      cc: d.cc,
      bcc: d.bcc,
      subject: fields.subject,
      body: fields.body,
      attachments: d.attachments,
    });
    this.busy = false;
    if (out.kind === 'error') {
      await this.#refused(out.error);
      return false;
    }
    this.draft = out.result;
    this.error = null;
    return true;
  }

  async #run(command: 'mail.send' | 'mail.save_to_mail_drafts' | 'mail.discard_draft'): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    this.notice = null;
    const out = await this.#handle.call(command, { id: this.#id });
    this.busy = false;
    if (out.kind === 'error') {
      await this.#refused(out.error);
      return false;
    }
    this.error = null;
    if (command !== 'mail.discard_draft') await this.load();
    return true;
  }

  /** `MAIL_DRAFT_STATE_INVALID` draws its sentence and the screen re-reads the draft. */
  async #refused(error: ServiceError): Promise<void> {
    if (error.code === 'MAIL_DRAFT_STATE_INVALID') await this.load();
    this.error = error;
  }
}

/** A draft may be edited or sent while it is `draft` or `waiting_for_you`. */
export const draftIsEditable = (d: EmailDraft): boolean => d.state === 'draft' || d.state === 'waiting_for_you';
