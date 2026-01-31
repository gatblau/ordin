// The email list screen (SM-NS-12.1).
import { beforeAll, describe, expect, it } from 'vitest';

import EmailListView from '../src/lib/views/mail/EmailListView.svelte';
import { EmailListController } from '../src/lib/views/mail/mail-controller.svelte';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const GROUPS = ['service', 'mail', 'workspaces', 'ordin'];
const link = { id: uid('l1'), display_name: 'Inbox › Suppliers', workspace_id: WS };
const msg = (ref: string, subject: string, over: Record<string, unknown> = {}) => ({
  attachment_names: [], cc: [], date: '2026-01-16T10:00:00Z', email_link_id: uid('l1'), from: { address: 'bo@acme.test', name: 'Bo', outside_company: false }, message_ref: ref, subject, to: [], ...over,
});

describe('Email list (SM-NS-12.1)', () => {
  it('Happy path — an email list names its link (SM-NS-12): the heading reads "Inbox › Suppliers" and 2 rows are drawn newest first; opening marks the link looked at', async () => {
    const { handle, state, transport } = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('workspaces.email_links', ok([link])).on('mail.mark_looked', ok({})).on('mail.list_messages', ok({ items: [msg('m2', 'Newer', { from: { address: 'x@other.test', outside_company: true } }), msg('m1', 'Older', { attachment_names: ['a.pdf', 'b.pdf'] })] })),
    });
    const c = new EmailListController(handle, WS, uid('l1'));
    await c.load();
    expect(transport.commandsSent().slice(0, 1)).toEqual(['mail.mark_looked']);
    expect(transport.sentOf('mail.mark_looked')[0]?.input).toEqual({ link: uid('l1') });
    expect(transport.sentOf('mail.list_messages')[0]?.input).toEqual({ workspace_id: WS, email_link_id: uid('l1') });
    const markup = html(EmailListView, { backend: state, c });
    const body = text(markup);
    expect(body).toContain('Inbox › Suppliers');
    expect(markup.match(/<li/g)).toHaveLength(2);
    expect(body.indexOf('Newer')).toBeLessThan(body.indexOf('Older'));
    expect(body).toContain('x@other.test');
    expect(body).toContain('Outside the company');
    expect(body).toContain('a.pdf and b.pdf');
    expect(body).not.toContain('Show more');
  });

  it('Show more reads the next page with next_cursor while one is present', async () => {
    let calls = 0;
    const { handle, state, transport } = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr.on('workspaces.email_links', ok([link])).on('mail.mark_looked', ok({})).on('mail.list_messages', () => (calls++ === 0 ? ok({ items: [msg('m1', 'First')], next_cursor: 'c2' }) : ok({ items: [msg('m0', 'Second')] }))),
    });
    const c = new EmailListController(handle, WS, uid('l1'));
    await c.load();
    expect(text(html(EmailListView, { backend: state, c }))).toContain('Show more');
    await c.showMore();
    expect(transport.sentOf('mail.list_messages')[1]?.input).toEqual({ workspace_id: WS, email_link_id: uid('l1'), cursor: 'c2' });
    expect(c.messages.map((m) => m.subject)).toEqual(['First', 'Second']);
    expect(c.nextCursor).toBeNull();
  });

  it('an empty link reads "No messages here yet."', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('workspaces.email_links', ok([link])).on('mail.mark_looked', ok({})).on('mail.list_messages', ok({ items: [] })) });
    const c = new EmailListController(handle, WS, uid('l1'));
    await c.load();
    expect(text(html(EmailListView, { backend: state, c }))).toContain('No messages here yet.');
  });

  it('Error path — a refused list draws the owner\'s sentence and no empty-list line', async () => {
    const { handle, state } = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('workspaces.email_links', ok([link])).on('mail.mark_looked', ok({})).on('mail.list_messages', err('MAIL_UNREACHABLE', 'The mail server can\'t be reached.', { action: 'try_again' })) });
    const c = new EmailListController(handle, WS, uid('l1'));
    await c.load();
    const body = text(html(EmailListView, { backend: state, c }));
    expect(body).toContain("The mail server can't be reached.");
    expect(body).not.toContain('No messages here yet.');
  });
});
