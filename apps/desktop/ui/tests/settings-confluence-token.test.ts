// SM-NS-2.11 (amended January 2026; user decision; Approved by the user January 2026): Settings › Sign-ins ›
// Confluence with an API token — the row is active with the default profile, its With an API token
// sub-section (site, email, token, Atlassian's token page, Connect), Use my Jira sign-in, and Disconnect.
import { beforeAll, describe, expect, it } from 'vitest';

import { backendWith } from './fixed-support';
import { err, type FakeTransport, ok, R1_GROUPS } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { text } from './settings-support';
import { uid } from './support/ids';
import SignIns from '../src/lib/views/Settings/SignIns.svelte';
import { serviceAvailable } from '../src/lib/views/Settings/services';
import { ConfluenceTokenModel, SignInsModel } from '../src/lib/views/Settings/signins-model.svelte';
import { WebPageOpener } from '../src/lib/views/Settings/web-page.svelte';

beforeAll(loadWindowCatalogue);

const GROUPS = [...R1_GROUPS, 'profile', 'confluence'];
const DEFAULT_PROFILE = ok({ document: { format_version: 1, profile_version: '1.0.0' }, source: 'defaults' });
const SECRET = 'ATATT3xFfGF0-confluence-secret';
const credential = (kind: string, state = 'signed_in', over: Record<string, unknown> = {}) => ({ id: `c-${kind}`, kind, state, account_label: 'acme.atlassian.net', expires_at: null, ...over });
const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const rowOf = (markup: string) => {
  const at = markup.indexOf('data-service="confluence"');
  const next = markup.indexOf('<li class="list-row', at);
  return markup.slice(markup.lastIndexOf('<li', at), next === -1 ? markup.indexOf('</ul>', at) : next);
};

async function opened(setup: (tr: FakeTransport) => FakeTransport, groups = GROUPS) {
  const t = await backendWith({ groups, setup });
  const model = new SignInsModel(t.handle);
  await model.load();
  await model.loadCertificateRequirement();
  const key = model.items.find((c) => c.kind === 'confluence')?.id ?? 'confluence';
  model.toggle(key);
  const confluence = new ConfluenceTokenModel(t.handle, model);
  const pages = new WebPageOpener(t.handle);
  const draw = () => rowOf(clean(html(SignIns, { backend: t.state, model, confluence, pages })));
  return { t, model, confluence, pages, draw };
}

describe('Confluence with an API token (SM-NS-2.11)', () => {
  it('Edge case — the default profile names no Atlassian app, yet the Confluence row is active', async () => {
    const { draw } = await opened((tr) => tr.on('identity.list', ok({ items: [] })).on('profile.get_document', DEFAULT_PROFILE));
    const row = draw();
    expect(row).not.toContain('aria-disabled');
    expect(text(row)).not.toContain('Not available in this version yet');
    expect(serviceAvailable('confluence', { format_version: 1, profile_version: '1.0.0' } as never, () => true)).toBe(true);
  });

  it('Happy path — the open row has a With an API token sub-section: site, email, the token page, the token and Connect', async () => {
    const { draw } = await opened((tr) => tr.on('identity.list', ok({ items: [] })).on('profile.get_document', DEFAULT_PROFILE));
    const row = draw();
    const out = text(row);
    expect(row).toContain('id="signin-confluence-token"');
    for (const words of ['With an API token', 'Your Confluence site', 'The email address you sign in to Atlassian with', "Open Atlassian's token page", 'Create API token', 'API token', 'Connect']) {
      expect(out, words).toContain(words);
    }
    expect(row).toMatch(/<input[^>]*type="password"/);
    // No one-click way: the profile names no Atlassian app.
    expect(out).not.toContain('With one click');
    expect(out).not.toContain('Use my Jira sign-in');
  });

  it('the one-click way stays when the profile names an Atlassian app', async () => {
    const profile = ok({ document: { format_version: 1, profile_version: '1.0.0', confluence: { oauth_client_id: 'app-1', site: 'https://acme.atlassian.net' } }, source: 'file' });
    const { draw } = await opened((tr) => tr.on('identity.list', ok({ items: [] })).on('profile.get_document', profile));
    const out = text(draw());
    expect(out).toContain('With one click');
    expect(out).toContain('With an API token');
  });

  it("Open Atlassian's token page sends app.open_web_page with atlassian_api_tokens", async () => {
    const { t, pages } = await opened((tr) => tr.on('identity.list', ok({ items: [] })).on('app.open_web_page', ok({})));
    expect(await pages.open('atlassian_api_tokens')).toBe(true);
    expect(t.transport.sentOf('app.open_web_page')[0]?.input).toEqual({ page: 'atlassian_api_tokens' });
  });

  it('Happy path — Connect sends the host, the email and the token once, clears the token and reads the list again', async () => {
    let items: unknown[] = [];
    const { t, model, confluence, draw } = await opened((tr) =>
      tr
        .on('identity.list', () => ok({ items }))
        .on('profile.get_document', DEFAULT_PROFILE)
        .on('confluence.connect_token', () => ((items = [credential('confluence')]), ok({ account_id: '5b10a2844c20165700ede21g', display_name: 'Sam Lee' }))),
    );
    confluence.site = ' https://Acme.atlassian.net/wiki/home ';
    confluence.email = ' sam@acme.com ';
    confluence.token = SECRET;
    expect(await confluence.connect()).toBe(true);
    expect(t.transport.sentOf('confluence.connect_token')[0]?.input).toEqual({ site: 'acme.atlassian.net', email: 'sam@acme.com', token: SECRET });
    expect(confluence.token).toBe('');
    expect(confluence.last).toEqual({ site: 'acme.atlassian.net', name: 'Sam Lee' });
    // The saved account is a row of its own; its chevron shows the connected state.
    model.toggle('c-confluence');
    const row = draw();
    expect(text(row)).toContain('Connected to acme.atlassian.net as Sam Lee');
    expect(text(row)).toContain('Disconnect');
    expect(row).not.toContain('id="signin-confluence-token"');
    expect(row).not.toContain(SECRET);
  });

  it('Error path — an empty site or token sends nothing and says so', async () => {
    const { t, confluence, draw } = await opened((tr) => tr.on('identity.list', ok({ items: [] })).on('profile.get_document', DEFAULT_PROFILE));
    confluence.email = 'sam@acme.com';
    confluence.token = SECRET;
    expect(await confluence.connect()).toBe(false);
    expect(confluence.siteNeeded).toBe(true);
    expect(text(draw())).toContain('Type your Confluence site');
    confluence.site = 'acme.atlassian.net';
    expect(await confluence.connect()).toBe(false);
    expect(confluence.tokenEmpty).toBe(true);
    expect(text(draw())).toContain('Paste a token first.');
    expect(t.transport.sentOf('confluence.connect_token')).toEqual([]);
  });

  it('Error path — a refused token is drawn with its sentence; the token is never on screen', async () => {
    const { confluence, draw } = await opened((tr) =>
      tr
        .on('identity.list', ok({ items: [] }))
        .on('profile.get_document', DEFAULT_PROFILE)
        .on('confluence.connect_token', err('CONF_TOKEN_REFUSED', "Confluence didn't accept those sign-in details. Check them and try again.")),
    );
    confluence.site = 'acme.atlassian.net';
    confluence.email = 'sam@acme.com';
    confluence.token = SECRET;
    expect(await confluence.connect()).toBe(false);
    const row = draw();
    expect(text(row)).toContain("Confluence didn't accept those sign-in details.");
    expect(row).not.toContain(SECRET);
  });

  it('Use my Jira sign-in is offered when Jira is connected, and sends confluence.connect_with_jira', async () => {
    let items: unknown[] = [credential('jira')];
    const { t, confluence, draw } = await opened((tr) =>
      tr
        .on('identity.list', () => ok({ items }))
        .on('profile.get_document', DEFAULT_PROFILE)
        .on('confluence.connect_with_jira', () => ((items = [credential('jira'), credential('confluence')]), ok({ account_id: '5b10a2844c20165700ede21g', display_name: 'Sam Lee' }))),
    );
    expect(text(draw())).toContain('Use my Jira sign-in for acme.atlassian.net');
    expect(await confluence.useJira('acme.atlassian.net')).toBe(true);
    expect(t.transport.sentOf('confluence.connect_with_jira')[0]?.input).toEqual({});
    expect(confluence.last).toEqual({ site: 'acme.atlassian.net', name: 'Sam Lee' });
  });

  it('Disconnect signs the Confluence account out at once', async () => {
    const { t, model, draw } = await opened((tr) => tr.on('identity.list', ok({ items: [credential('confluence')] })).on('profile.get_document', DEFAULT_PROFILE).on('identity.sign_out', ok({})));
    expect(text(draw())).toContain('Disconnect');
    expect(await model.signOut(uid('c-confluence'))).toBe(true);
    expect(t.transport.sentOf('identity.sign_out')[0]?.input).toEqual({ credential_id: uid('c-confluence') });
  });

  it('a backend that does not answer confluence.connect_token keeps the default profile greyed out (SM-NS-2.9)', async () => {
    const { draw } = await opened((tr) => tr.on('identity.list', ok({ items: [] })).on('profile.get_document', DEFAULT_PROFILE), [...R1_GROUPS, 'profile']);
    expect(text(draw())).toContain('Not available in this version yet');
  });
});
