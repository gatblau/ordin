// The email draft screen (SM-NS-12.3).
import { beforeAll, describe, expect, it } from 'vitest';

import EmailDraftView from '../src/lib/views/mail/EmailDraftView.svelte';
import { EmailDraftController } from '../src/lib/views/mail/mail-controller.svelte';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const GROUPS = ['service', 'mail'];
const draft = (over: Record<string, unknown> = {}) => ({
  attachments: [], bcc: [], body: 'Dear Bo,\n\nThanks.', cc: [], created_at: '2026-01-16T10:00:00Z', drafted_by: { kind: 'agent_instance', agent_instance_id: 'a1' }, edited_by_person: false, from: 'me@co.test',
  id: uid('d1'), kind: 'new', state: 'draft', subject: 'Prices', to: ['bo@acme.test'], workspace_id: 'ws', ...over,
});

describe('Email draft (SM-NS-12.3)', () => {
  it('draws who drafted it, To, the body and the four buttons while it may change', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('mail.get_draft', ok(draft())) });
    const c = new EmailDraftController(handle, uid('d1'));
    await c.load();
    const body = text(html(EmailDraftView, { backend: state, c }));
    expect(body).toContain('Drafted by an agent');
    expect(html(EmailDraftView, { backend: state, c, agentName: 'Clause checker' })).toContain('Drafted by');
    expect(text(html(EmailDraftView, { backend: state, c, agentName: 'Clause checker' }))).toContain('Drafted by Clause checker');
    expect(body).toContain('To: bo@acme.test');
    expect(body).toContain('Dear Bo,');
    for (const label of ['Send…', 'Edit', 'Save to mail drafts', 'Discard']) expect(body).toContain(label);
  });

  it('Send… sends mail.send with the id and reads the draft again; Save to mail drafts says so', async () => {
    const { handle, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('mail.get_draft', ok(draft())).on('mail.send', ok('approval-1')).on('mail.save_to_mail_drafts', ok({})) });
    const c = new EmailDraftController(handle, uid('d1'));
    await c.load();
    expect(await c.send()).toBe(true);
    expect(transport.sentOf('mail.send')[0]?.input).toEqual({ id: uid('d1') });
    expect(await c.saveToMailDrafts()).toBe(true);
    expect(c.notice).toBe('saved_to_drafts');
    expect(transport.sentOf('mail.get_draft')).toHaveLength(3);
  });

  it('Save in the editor sends mail.edit_draft with To, Subject and body', async () => {
    const { handle, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('mail.get_draft', ok(draft())).on('mail.edit_draft', (input) => ok(draft({ subject: (input as { subject: string }).subject }))) });
    const c = new EmailDraftController(handle, uid('d1'));
    await c.load();
    expect(await c.edit({ to: ['bo@acme.test', 'cy@acme.test'], subject: 'New prices', body: 'Hello' })).toBe(true);
    expect(transport.sentOf('mail.edit_draft')[0]?.input).toEqual({ draft_id: uid('d1'), to: ['bo@acme.test', 'cy@acme.test'], cc: [], bcc: [], subject: 'New prices', body: 'Hello', attachments: [] });
    expect(c.draft?.subject).toBe('New prices');
  });

  it('by state: sent, not sure it was sent, and not sent with the reason; none of them offers the buttons', async () => {
    const read = async (over: Record<string, unknown>) => {
      const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('mail.get_draft', ok(draft(over))) });
      const c = new EmailDraftController(handle, uid('d1'));
      await c.load();
      return text(html(EmailDraftView, { backend: state, c }));
    };
    const sent = await read({ state: 'sent' });
    expect(sent).toContain("Sent. It's in your Sent folder.");
    expect(sent).not.toContain('Send…');
    expect(await read({ state: 'not_sure_it_was_sent' })).toContain("Not sure this was sent.");
    expect(await read({ state: 'not_sent', failure_reason: 'The server refused bo@acme.test.' })).toContain('The server refused bo@acme.test.');
  });

  it('Error path — MAIL_DRAFT_STATE_INVALID draws its sentence and the draft is read again with mail.get_draft', async () => {
    const { handle, state, transport } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('mail.get_draft', ok(draft({ state: 'sent' }))).on('mail.send', err('MAIL_DRAFT_STATE_INVALID', 'This draft can\'t be sent any more.')) });
    const c = new EmailDraftController(handle, uid('d1'));
    await c.load();
    transport.sent.length = 0;
    expect(await c.send()).toBe(false);
    expect(transport.commandsSent()).toEqual(['mail.send', 'mail.get_draft']);
    expect(text(html(EmailDraftView, { backend: state, c }))).toContain("This draft can't be sent any more.");
  });
});
