// The email message screen (SM-NS-12.2).
import { beforeAll, describe, expect, it } from 'vitest';

import EmailMessageView from '../src/lib/views/mail/EmailMessageView.svelte';
import { EmailMessageController, splitMessageInstance } from '../src/lib/views/mail/mail-controller.svelte';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';

beforeAll(loadWindowCatalogue);

const GROUPS = ['service', 'mail', 'ordin'];
const message = (over: Record<string, unknown> = {}) => ({
  attachment_names: ['offer.pdf'],
  attachments: [{ bytes: 10, file_name: 'offer.pdf', index: 0, media_type: 'application/pdf' }],
  cc: [{ address: 'cc@acme.test', outside_company: false }],
  date: '2026-01-16T10:00:00Z',
  email_link_id: 'l1',
  from: { address: 'bo@acme.test', name: 'Bo', outside_company: true },
  hidden_text: false,
  message_ref: 'ref-1',
  subject: 'New prices',
  text: 'Hello.\n\n<b>Not markup</b>',
  to: [{ address: 'me@co.test', outside_company: false }],
  truncated: true,
  ...over,
});

describe('Email message (SM-NS-12.2)', () => {
  it('splits the instance into the link and the message reference', () => {
    expect(splitMessageInstance('l1:ref:with:colons')).toEqual({ linkId: 'l1', messageRef: 'ref:with:colons' });
  });

  it('Security — an email\'s hidden text (SM-NS-12): "This message holds hidden text, which isn\'t shown." is drawn', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('mail.open_message', ok(message({ hidden_text: true }))) });
    const c = new EmailMessageController(handle, WS, 'ref-1');
    await c.load();
    expect(text(html(EmailMessageView, { backend: state, workspaceId: WS, c }))).toContain("This message holds hidden text, which isn't shown.");
  });

  it('draws the message in order, as text, with the outside-company mark, the cut note and the buttons', async () => {
    const { handle, state, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('mail.open_message', ok(message())) });
    const c = new EmailMessageController(handle, WS, 'ref-1');
    await c.load();
    expect(transport.sentOf('mail.open_message')[0]?.input).toEqual({ message_ref: 'ref-1', workspace_id: WS });
    const markup = html(EmailMessageView, { backend: state, workspaceId: WS, c });
    const body = text(markup);
    expect(body).toContain('New prices');
    expect(body).toContain('From: Bo');
    expect(body).toContain('Outside the company');
    expect(body).toContain('Cc: cc@acme.test');
    expect(body).not.toContain("hidden text");
    expect(markup).toContain('&lt;b>Not markup&lt;/b>');
    expect(markup).not.toContain('<b>Not markup</b>');
    expect(body).toContain('Only the start of this message is shown.');
    expect(body).toContain('offer.pdf');
    expect(body).toContain('Save attachment');
    expect(body).toContain('Reply with Ordin');
    expect(body).toContain('Ask Ordin about this');
  });

  it('Save attachment sends the workspace, the message and the index, then reads "Saved."; Reply with Ordin sends the sender and the reference', async () => {
    const { handle, state, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('mail.open_message', ok(message())).on('mail.save_attachment', ok({ path: 'p', place_id: 'pl' })).on('ordin.send_message', ok({})) });
    const c = new EmailMessageController(handle, WS, 'ref-1');
    await c.load();
    await c.saveAttachment(0);
    expect(transport.sentOf('mail.save_attachment')[0]?.input).toEqual({ workspace_id: WS, message_ref: 'ref-1', index: 0 });
    expect(text(html(EmailMessageView, { backend: state, workspaceId: WS, c }))).toContain('Saved.');
    await c.replyWithOrdin(state, (sender, ref) => `Draft a reply to ${sender}'s email.\nMessage reference: ${ref}`);
    expect(transport.sentOf('ordin.send_message')[0]?.input).toEqual({ workspace_id: WS, text: "Draft a reply to Bo's email.\nMessage reference: ref-1" });
  });

  it('Error path — a refused read draws the owner\'s sentence and no message', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('mail.open_message', err('MAIL_MESSAGE_GONE', 'That message is no longer in the mailbox.')) });
    const c = new EmailMessageController(handle, WS, 'ref-1');
    await c.load();
    const body = text(html(EmailMessageView, { backend: state, workspaceId: WS, c }));
    expect(body).toContain('That message is no longer in the mailbox.');
    expect(body).not.toContain('Save attachment');
  });
});
