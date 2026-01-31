// SM-NS-2: Settings › Sign-ins — the rows and their ways to sign in, the browser and device-code waits, the
// mail password dialog, the client certificate import, and the services this version cannot sign in to.
import { beforeAll, describe, expect, it } from 'vitest';

import { backendWith } from './fixed-support';
import { err, type FakeTransport, ok, R1_GROUPS, settle } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { text } from './settings-support';
import SignIns from '../src/lib/views/Settings/SignIns.svelte';
import { kindMessageId, MAIL_PASSWORD_KINDS, SignInsModel, SshKeyModel } from '../src/lib/views/Settings/signins-model.svelte';
import { uid } from './support/ids';
import { WebPageOpener } from '../src/lib/views/Settings/web-page.svelte';

beforeAll(loadWindowCatalogue);

const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const credential = (kind: string, state = 'signed_in', over: Record<string, unknown> = {}) => ({ id: `c-${kind}`, kind, state, account_label: `${kind}@example.com`, expires_at: null, ...over });
const GROUPS = [...R1_GROUPS, 'profile'];
/** One service's row: from its `data-service` up to the next row (a row holds lists of its own). */
const serviceRow = (out: string, service: string): string => {
  const at = out.indexOf(`data-service="${service}"`);
  const next = out.indexOf('<li class="list-row', at);
  return out.slice(at, next === -1 ? out.indexOf('</ul>', at) : next);
};

describe('Sign-ins rows (SM-NS-2.1)', () => {
  it('table: the product name of each kind is signin-kind-<kind> with _ written -', () => {
    for (const kind of ['github', 'gitlab', 'slack', 'confluence', 'microsoft', 'google', 'jira', 'oidc', 'mail_server_single_sign_on', 'mail_server_password', 'mail_server_app_password']) {
      expect(kindMessageId(kind as never)).toBe(`signin-kind-${kind.replaceAll('_', '-')}`);
    }
  });

  it('Happy path — one row per account in the order given: product, label, state, expiry and the action', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr.on('identity.list', ok({ items: [credential('github', 'signed_in', { expires_at: '2026-03-01T00:00:00Z' }), credential('google', 'needs_sign_in'), credential('slack', 'signed_out')] })),
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    for (const item of model.items) model.toggle(item.id);
    const markup = clean(html(SignIns, { backend: t.state, model }));
    const out = text(markup);
    expect(out.indexOf('GitHub')).toBeLessThan(out.indexOf('Google'));
    expect(out.indexOf('Google')).toBeLessThan(out.indexOf('Slack'));
    expect(out).toContain('github@example.com');
    expect(out).toContain('Signed in');
    expect(out).toContain('until');
    expect(out).toContain('Sign in again to keep using it');
    expect(out).toContain('Asked for the first time Ordin needs it');
    expect(out.match(/Sign out/g)).toHaveLength(1);
    expect(out.match(/Sign in(?! again)/g)?.length).toBeGreaterThanOrEqual(2);
  });

  it('Sign out sends the credential id at once', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [credential('github')] })).on('identity.sign_out', ok({})) });
    const model = new SignInsModel(t.handle);
    await model.load();
    await model.signOut(uid('c-github'));
    expect(t.transport.sentOf('identity.sign_out')[0]?.input).toEqual({ credential_id: uid('c-github') });
  });
});

describe('Sign-in waits (SM-NS-2.2)', () => {
  it('Happy path — a device-code sign-in (SM-NS-2)', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('identity.list', ok({ items: [credential('github', 'signed_out')] })),
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    model.challenge = { kind: 'device_code', challenge_id: uid('ch1'), expires_at: '2026-01-10T11:00:00Z', user_code: 'WDJB-MJHT', verification_uri: 'https://github.com/login/device' };
    model.challengeKind = 'github';
    const markup = clean(html(SignIns, { backend: t.state, model }));
    const out = text(markup);
    expect(out).toContain('Open https://github.com/login/device and enter this code:');
    expect(out).toContain('WDJB-MJHT');
    expect(out).toContain('Copy the code');
    // The address is text, never a link (an icon's `<use href>` is not one); the code is monospaced.
    expect(markup).not.toMatch(/<a\b[^>]*href=/);
    expect(markup).toMatch(/<code[^>]*>\s*WDJB-MJHT/);
  });

  it('a device-code challenge waits with identity.sign_in_wait and then reads the list again', async () => {
    let release: () => void = () => undefined;
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr
          .on('identity.list', ok({ items: [] }))
          .on('identity.sign_in', ok({ kind: 'device_code', challenge_id: uid('ch1'), expires_at: 'x', user_code: 'AB-CD', verification_uri: 'https://example.com/device' }))
          .on('identity.sign_in_wait', () => new Promise((resolve) => (release = () => resolve(ok(credential('github')))))),
    });
    const model = new SignInsModel(t.handle);
    const pending = model.signIn('github');
    await settle();
    expect(model.challenge?.kind).toBe('device_code');
    release();
    expect(await pending).toBe('ok');
    expect(model.challenge).toBeNull();
    expect(t.transport.commandsSent()).toEqual(['identity.sign_in', 'identity.sign_in_wait', 'identity.list']);
  });

  it('browser_opened and relayed_browser both say to finish in the browser, with Cancel sending identity.sign_in_cancel', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [credential('github', 'signed_out')] })).on('identity.sign_in_cancel', ok({})) });
    const model = new SignInsModel(t.handle);
    await model.load();
    for (const challenge of [
      { kind: 'browser_opened', challenge_id: uid('ch1'), expires_at: 'x' },
      { kind: 'relayed_browser', challenge_id: uid('ch1'), expires_at: 'x', authorisation_url: 'https://example.com/auth' },
    ] as const) {
      model.challenge = challenge;
      model.challengeKind = 'github';
      const markup = clean(html(SignIns, { backend: t.state, model }));
      expect(text(markup)).toContain('Finish signing in in your browser.');
      expect(text(markup)).toContain('Cancel');
      expect(markup).not.toMatch(/<a\b[^>]*href=/);
    }
    await model.cancelSignIn();
    expect(t.transport.sentOf('identity.sign_in_cancel')[0]?.input).toEqual({ challenge_id: uid('ch1') });
  });

  it('already_signed_in redraws the row from identity.list and does not wait', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('identity.list', ok({ items: [credential('github')] })).on('identity.sign_in', ok({ kind: 'already_signed_in', credential: credential('github') })),
    });
    const model = new SignInsModel(t.handle);
    expect(await model.signIn('github')).toBe('ok');
    expect(t.transport.commandsSent()).toEqual(['identity.sign_in', 'identity.list']);
  });

  it('Error path — CRED_PROVIDER_NOT_CONFIGURED for gitlab draws no sentence; the open row offers the token', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [credential('gitlab', 'signed_out')] })).on('identity.sign_in', err('CRED_PROVIDER_NOT_CONFIGURED', 'GitLab sign-in is not set up.')) });
    const model = new SignInsModel(t.handle);
    await model.load();
    expect(await model.signIn('gitlab')).toBe('not_configured');
    expect(model.error).toBeNull();
    model.toggle('c-gitlab');
    const out = text(html(SignIns, { backend: t.state, model }));
    expect(out).toContain('Paste a GitLab access token');
    expect(out).not.toContain('GitLab sign-in is not set up.');
  });

  // Amended January 2026 (user decision): GitHub's Sign in did nothing when the build carried no GitHub
  // sign-in, because CRED_PROVIDER_NOT_CONFIGURED was swallowed for every kind, not only GitLab.
  it('Happy path — Sign in on the GitHub row with no account shows the device code', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr
          .on('identity.list', ok({ items: [] }))
          .on('identity.sign_in', ok({ kind: 'device_code', challenge_id: uid('gh1'), expires_at: 'x', user_code: 'GH-12', verification_uri: 'https://github.com/login/device' }))
          .on('identity.sign_in_wait', () => new Promise(() => {})),
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    void model.startNew('github');
    await settle();
    expect(t.transport.sentOf('identity.sign_in')[0]?.input).toEqual({ kind: 'github' });
    const out = text(clean(html(SignIns, { backend: t.state, model })));
    expect(out).toContain('GH-12');
    expect(out).toContain('https://github.com/login/device');
  });

  it('Error path — CRED_PROVIDER_NOT_CONFIGURED for slack is drawn as its sentence, never silently', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [] })).on('identity.sign_in', err('CRED_PROVIDER_NOT_CONFIGURED', "Signing in to that service isn't set up in this version of ordin.")) });
    const model = new SignInsModel(t.handle);
    await model.load();
    expect(await model.signIn('slack')).toBe('error');
    expect(model.error?.code).toBe('CRED_PROVIDER_NOT_CONFIGURED');
    expect(text(clean(html(SignIns, { backend: t.state, model })))).toContain("Signing in to that service isn't set up in this version of ordin.");
  });

  it('Error path — CRED_SIGN_IN_CANCELLED draws nothing', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('identity.list', ok({ items: [] })).on('identity.sign_in', ok({ kind: 'browser_opened', challenge_id: uid('ch1'), expires_at: 'x' })).on('identity.sign_in_wait', err('CRED_SIGN_IN_CANCELLED', 'You cancelled the sign-in.')),
    });
    const model = new SignInsModel(t.handle);
    expect(await model.signIn('github')).toBe('error');
    expect(model.error).toBeNull();
    expect(text(html(SignIns, { backend: t.state, model }))).not.toContain('You cancelled the sign-in.');
  });

  it('Error path — CRED_SIGN_IN_TIMEOUT draws its sentence with Try again', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('identity.list', ok({ items: [] })).on('identity.sign_in', ok({ kind: 'browser_opened', challenge_id: uid('ch1'), expires_at: 'x' })).on('identity.sign_in_wait', err('CRED_SIGN_IN_TIMEOUT', 'The sign-in took too long.')),
    });
    const model = new SignInsModel(t.handle);
    await model.signIn('github');
    const out = text(clean(html(SignIns, { backend: t.state, model })));
    expect(out).toContain('The sign-in took too long.');
    expect(out).toContain('Try again');
  });
});

describe('The mail password dialog (SM-NS-2.3)', () => {
  const setup = (tr: FakeTransport) =>
    tr.on('identity.list', ok({ items: [credential('mail_server_app_password', 'needs_sign_in', { account_label: 'you@company.com' })] })).on('identity.save_mail_password', ok(credential('mail_server_app_password')));

  it('a mail password kind opens the dialog preset to the account label instead of a browser sign-in', async () => {
    const t = await backendWith({ groups: GROUPS, setup });
    const model = new SignInsModel(t.handle);
    await model.load();
    await model.startSignIn(model.items[0]!);
    expect(t.transport.sentOf('identity.sign_in')).toEqual([]);
    expect(model.mailAddress).toBe('you@company.com');
    const out = text(clean(html(SignIns, { backend: t.state, model })));
    for (const sentence of ["Sign in to your company's email", 'Email address', 'App password', 'Show', 'Filled in from your company account.']) expect(out).toContain(sentence);
    expect(MAIL_PASSWORD_KINDS.mail_server_password).toBe('mail_password');
  });

  it('Sign in sends the address, the kind and the password, and clears the password field', async () => {
    const t = await backendWith({ groups: GROUPS, setup });
    const model = new SignInsModel(t.handle);
    await model.load();
    await model.startSignIn(model.items[0]!);
    model.mailPassword = 'app-secret';
    expect(await model.saveMailPassword()).toBe(true);
    expect(t.transport.sentOf('identity.save_mail_password')[0]?.input).toEqual({ account_label: 'you@company.com', kind: 'app_password', password: 'app-secret' });
    expect(model.mailPassword).toBe('');
    expect(model.mailDialog).toBeNull();
  });

  it('Error path — a refusal keeps the dialog and the address, and the password field is empty', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => { setup(tr); tr.on('identity.save_mail_password', err('CRED_REFUSED', 'The mail server did not accept it.')); } });
    const model = new SignInsModel(t.handle);
    await model.load();
    await model.startSignIn(model.items[0]!);
    model.mailPassword = 'wrong';
    expect(await model.saveMailPassword()).toBe(false);
    expect(model.mailDialog).not.toBeNull();
    expect(model.mailAddress).toBe('you@company.com');
    expect(model.mailPassword).toBe('');
    expect(text(html(SignIns, { backend: t.state, model }))).toContain('The mail server did not accept it.');
  });
});

describe('Import a certificate… (SM-NS-2.4)', () => {
  const required = (value: boolean) => (tr: FakeTransport) =>
    tr.on('identity.list', ok({ items: [] })).on('profile.get_document', ok({ document: { server: { client_certificate_required: value } }, source: 'verified' }));

  it('Edge case — no certificate import unless the company requires one (SM-NS-2)', async () => {
    const t = await backendWith({ groups: GROUPS, setup: required(false) });
    const model = new SignInsModel(t.handle);
    await model.loadCertificateRequirement();
    expect(model.certificateRequired).toBe(false);
    expect(text(html(SignIns, { backend: t.state, model }))).not.toContain('Import a certificate…');
  });

  it('Happy path — a required certificate is imported and drawn with its name and date', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) => { required(true)(tr); tr.on('identity.import_client_certificate', ok({ certificate: { name: 'Work laptop', subject: 'CN=sam', not_after: '2027-03-01T00:00:00Z' } })); },
    });
    const model = new SignInsModel(t.handle);
    await model.loadCertificateRequirement();
    expect(text(html(SignIns, { backend: t.state, model }))).toContain('Import a certificate…');
    await model.importCertificate();
    expect(t.transport.sentOf('identity.import_client_certificate')[0]?.input).toEqual({});
    const out = text(html(SignIns, { backend: t.state, model }));
    expect(out).toContain('Certificate Work laptop imported.');
    expect(out).toContain("It's valid until");
  });

  it('a cancelled import (no certificate) draws nothing', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => { required(true)(tr); tr.on('identity.import_client_certificate', ok({})); } });
    const model = new SignInsModel(t.handle);
    await model.importCertificate();
    expect(model.certificate).toBeNull();
    expect(model.certificateError).toBeNull();
  });

  it('Error path — CRED_CERTIFICATE_EXPIRED and CRED_CERTIFICATE_INVALID draw their sentences under the button', async () => {
    for (const [code, sentence] of [['CRED_CERTIFICATE_EXPIRED', 'This certificate has expired.'], ['CRED_CERTIFICATE_INVALID', 'This file is not a certificate ordin can read.']] as const) {
      const t = await backendWith({ groups: GROUPS, setup: (tr) => { required(true)(tr); tr.on('identity.import_client_certificate', err(code, sentence)); } });
      const model = new SignInsModel(t.handle);
      await model.loadCertificateRequirement();
      await model.importCertificate();
      expect(text(html(SignIns, { backend: t.state, model })), code).toContain(sentence);
    }
  });

  it('the local backend only: a server backend never offers the import', async () => {
    const t = await backendWith({ groups: GROUPS, kind: 'server', setup: required(true) });
    const model = new SignInsModel(t.handle);
    model.certificateRequired = true;
    expect(text(html(SignIns, { backend: t.state, model }))).not.toContain('Import a certificate…');
  });
});

// SM-NS-2.6 (amended January 2026; user decision): a build with no GitHub App offers a personal access token.
describe('GitHub personal access token (SM-NS-2.6)', () => {
  const notConfigured = (tr: FakeTransport) => tr.on('identity.list', ok({ items: [] })).on('identity.sign_in', err('CRED_PROVIDER_NOT_CONFIGURED', "Signing in to that service isn't set up in this version of ordin."));
  const row = (out: string) => serviceRow(out, 'github');

  it('Edge case — a build with no GitHub App: the open row offers the token with its steps, and no one-click sign-in', async () => {
    const t = await backendWith({ groups: GROUPS, setup: notConfigured });
    const model = new SignInsModel(t.handle);
    await model.load();
    expect(model.githubOneClick).toBe(false);
    model.toggle('github');
    const out = text(clean(row(html(SignIns, { backend: t.state, model }))));
    expect(out).toContain('With a token');
    expect(out).toContain('Paste a GitHub personal access token');
    expect(out).toContain('Contents to Read and write');
    expect(out).toContain("Open GitHub's token page");
    expect(out).not.toContain('With one click');
    expect(out).not.toContain('Sign in');
  });

  it('Happy path — a build with a GitHub App: the open row offers one-click sign-in instead of the token', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr
          .on('identity.list', ok({ items: [], github_one_click: true }))
          .on('identity.sign_in', ok({ kind: 'device_code', challenge_id: uid('gh1'), expires_at: 'x', user_code: 'GH-12', verification_uri: 'https://github.com/login/device' }))
          .on('identity.sign_in_wait', () => new Promise(() => {})),
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    expect(model.githubOneClick).toBe(true);
    model.toggle('github');
    const out = text(clean(row(html(SignIns, { backend: t.state, model }))));
    expect(out).toContain('With one click');
    expect(out).toContain('Sign in');
    expect(out).not.toContain('Paste a GitHub personal access token');
    void model.startNew('github');
    await settle();
    expect(t.transport.sentOf('identity.sign_in')[0]?.input).toEqual({ kind: 'github' });
  });

  it('a not-configured answer for GitHub draws no sentence and turns the one-click way into the token', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [], github_one_click: true })).on('identity.sign_in', err('CRED_PROVIDER_NOT_CONFIGURED', 'Not set up.')) });
    const model = new SignInsModel(t.handle);
    await model.load();
    await model.startNew('github');
    expect(model.error).toBeNull();
    expect(model.githubOneClick).toBe(false);
  });

  it('Happy path — Open GitHub\'s token page sends app.open_web_page with github_tokens', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => notConfigured(tr).on('app.open_web_page', ok({})) });
    const pages = new WebPageOpener(t.handle);
    expect(await pages.open('github_tokens')).toBe(true);
    expect(t.transport.sentOf('app.open_web_page')[0]?.input).toEqual({ page: 'github_tokens' });
  });

  it('Happy path — a pasted token is sent once and the list is read again; the open row then offers Sign out', async () => {
    let saved = false;
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr
          .on('identity.list', () => ok({ items: saved ? [credential('github', 'signed_in', { id: uid('gh-cred') })] : [] }))
          .on('identity.save_github_token', () => ((saved = true), ok({ ...credential('github'), id: uid('gh-cred') }))),
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    model.githubToken = '  github_pat_secret  ';
    expect(await model.saveGithubToken()).toBe(true);
    expect(t.transport.sentOf('identity.save_github_token')[0]?.input).toEqual({ token: 'github_pat_secret' });
    expect(model.githubToken).toBe('');
    expect(model.items.map((c) => c.kind)).toEqual(['github']);
    model.toggle(uid('gh-cred'));
    const out = text(clean(row(html(SignIns, { backend: t.state, model }))));
    expect(out).toContain('github@example.com');
    expect(out).toContain('Sign out');
  });

  it('Edge case — an empty box sends nothing and says so', async () => {
    const t = await backendWith({ groups: GROUPS, setup: notConfigured });
    const model = new SignInsModel(t.handle);
    model.toggle('github');
    model.githubToken = '   ';
    expect(await model.saveGithubToken()).toBe(false);
    expect(t.transport.sentOf('identity.save_github_token')).toEqual([]);
    expect(text(clean(html(SignIns, { backend: t.state, model })))).toContain('Paste a token first.');
  });

  it('Error path — a token GitHub refuses says how to make a new one, and the field is cleared', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => notConfigured(tr).on('identity.save_github_token', err('CRED_PROVIDER_REFUSED', 'The service refused the sign-in.')) });
    const model = new SignInsModel(t.handle);
    model.toggle('github');
    model.githubToken = 'github_pat_bad';
    expect(await model.saveGithubToken()).toBe(false);
    expect(model.githubToken).toBe('');
    const out = text(clean(html(SignIns, { backend: t.state, model })));
    expect(out).toContain("GitHub didn't accept that token.");
  });

  it('Error path — GitHub unreachable is drawn as its sentence', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => notConfigured(tr).on('identity.save_github_token', err('CRED_PROVIDER_UNREACHABLE', "ordin couldn't reach that service. Check your internet connection and try again.")) });
    const model = new SignInsModel(t.handle);
    model.githubToken = 'github_pat_ok';
    await model.saveGithubToken();
    expect(model.error?.code).toBe('CRED_PROVIDER_UNREACHABLE');
    expect(text(clean(html(SignIns, { backend: t.state, model })))).toContain("ordin couldn't reach that service.");
  });
});

// SM-NS-2.7 (amended January 2026; user decision): one chevron per service row shows and hides its ways to sign in.
describe('Showing and hiding the ways to sign in (SM-NS-2.7)', () => {
  const rowOf = serviceRow;

  it.each([
    ['github', 'GitHub', 'With a token'],
    ['gitlab', 'GitLab', 'Paste a GitLab access token'],
    ['microsoft', 'Microsoft 365', 'Sign in'],
  ] as const)('table: the %s row has one chevron and nothing else until it is opened', async (service, product, inside) => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [] })) });
    const model = new SignInsModel(t.handle);
    await model.load();
    const name = `\u2068${product}\u2069`;
    const folded = rowOf(html(SignIns, { backend: t.state, model }), service);
    expect(folded.match(/<button\b/g)).toHaveLength(1);
    expect(folded).toContain('aria-expanded="false"');
    expect(folded).toContain(`title="Show the ways to sign in to ${name}"`);
    expect(folded).not.toContain(`id="signin-ways-${service}"`);
    expect(text(clean(folded))).not.toContain(inside === 'Sign in' ? 'Sign in' : inside);

    expect(model.toggle(service)).toBe(true);
    const opened = rowOf(html(SignIns, { backend: t.state, model }), service);
    expect(opened).toContain('aria-expanded="true"');
    expect(opened).toContain(`aria-controls="signin-ways-${service}"`);
    expect(opened).toContain(`title="Hide the ways to sign in to ${name}"`);
    expect(opened).toContain(`id="signin-ways-${service}"`);
    expect(text(clean(opened))).toContain(inside);

    expect(model.toggle(service)).toBe(false);
    expect(model.isOpen(service)).toBe(false);
  });

  it('GitLab offers one-click sign-in only when the profile names a GitLab app', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [] })) });
    const model = new SignInsModel(t.handle);
    model.toggle('gitlab');
    expect(text(clean(rowOf(html(SignIns, { backend: t.state, model }), 'gitlab')))).not.toContain('With one click');
    model.document = { format_version: 1, profile_version: '1.0.0', gitlab: { client_id: 'gl-app', host: 'gitlab.com' } } as never;
    const out = text(clean(rowOf(html(SignIns, { backend: t.state, model }), 'gitlab')));
    expect(out).toContain('With one click');
    expect(out).toContain('With a token');
  });

  it('a signed-in row shows the account and Sign out inside it, and no Sign in', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [credential('microsoft')] })) });
    const model = new SignInsModel(t.handle);
    await model.load();
    model.toggle('c-microsoft');
    const out = rowOf(clean(html(SignIns, { backend: t.state, model })), 'microsoft');
    expect(out).toMatch(/id="signin-ways-c-microsoft"[\s\S]*microsoft@example\.com[\s\S]*Sign out/);
    expect(text(out)).not.toMatch(/Sign in(?! again)/);
  });
});

// SM-NS-2.9 (amended January 2026; user decision): a service this version cannot sign in to.
describe('Services not available in this version (SM-NS-2.9)', () => {
  it('Edge case — the default profile: GitHub and GitLab work; the others are greyed out with one line and no control', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('identity.list', ok({ items: [] })).on('profile.get_document', ok({ document: { format_version: 1, profile_version: '1.0.0' }, source: 'defaults' })),
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    await model.loadCertificateRequirement();
    const markup = clean(html(SignIns, { backend: t.state, model }));
    const rowOf = (service: string) => {
      const at = markup.indexOf(`data-service="${service}"`);
      return markup.slice(markup.lastIndexOf('<li', at), markup.indexOf('</li>', at));
    };
    for (const service of ['github', 'gitlab']) {
      expect(rowOf(service), service).not.toContain('aria-disabled');
      expect(rowOf(service), service).toMatch(/<button\b/);
    }
    for (const service of ['microsoft', 'google', 'confluence', 'slack', 'mail']) {
      expect(rowOf(service), service).toContain('aria-disabled="true"');
      expect(text(rowOf(service)), service).toContain('Not available in this version yet');
      expect(rowOf(service), service).not.toMatch(/<(button|input|select)\b/);
    }
    expect(markup).not.toContain('data-service="oidc"');
    expect(markup).not.toContain('data-service="jira"');
  });

  it('a saved account of a service is never greyed out, so it can be signed out', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('identity.list', ok({ items: [credential('slack')] })).on('profile.get_document', ok({ document: { format_version: 1, profile_version: '1.0.0' }, source: 'defaults' })),
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    await model.loadCertificateRequirement();
    model.toggle('c-slack');
    expect(text(clean(html(SignIns, { backend: t.state, model })))).toContain('Sign out');
  });
});

// SM-NS-2.8 (amended January 2026; user decision): GitHub with an SSH key, on this computer.
describe('GitHub with an SSH key (SM-NS-2.8)', () => {
  const KEY = { path: '/home/sam/.ssh/id_ed25519', name: 'id_ed25519', public_key: 'ssh-ed25519 AAAAC3Nz sam@laptop' };
  const withKeys = (chosen: string | null) => (tr: FakeTransport) => tr.on('identity.list', ok({ items: [] })).on('app.ssh_keys', ok({ keys: [KEY], chosen }));
  const row = (out: string) => serviceRow(out, 'github');

  const opened = (t: Awaited<ReturnType<typeof backendWith>>, ssh: SshKeyModel) => {
    const model = new SignInsModel(t.handle);
    model.toggle('github');
    return row(html(SignIns, { backend: t.state, model, ssh }));
  };

  it('the open GitHub row has a With an SSH key sub-section; the row has one chevron only', async () => {
    const t = await backendWith({ groups: GROUPS, setup: withKeys(null) });
    const ssh = new SshKeyModel(t.handle);
    const folded = row(html(SignIns, { backend: t.state, model: new SignInsModel(t.handle), ssh }));
    expect(folded.match(/<button\b/g)).toHaveLength(1);
    expect(folded).not.toContain('id="signin-ssh-github"');
    await ssh.load();
    const shown = opened(t, ssh);
    expect(shown).toContain('id="signin-ssh-github"');
    const words = text(clean(shown));
    expect(words).toContain('With an SSH key');
    expect(words).toContain('Some features, such as pull requests, still need the token.');
    expect(words).toContain('id_ed25519');
    expect(words).not.toMatch(/\bIT\b/);
  });

  it('a chosen key shows its public key with a Copy icon button, the GitHub line and the two actions', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => withKeys(KEY.path)(tr).on('app.open_web_page', ok({})) });
    const ssh = new SshKeyModel(t.handle);
    await ssh.load();
    const out = opened(t, ssh);
    expect(out).toContain('ssh-ed25519 AAAAC3Nz sam@laptop');
    expect(out).toMatch(/<button[^>]*aria-label="Copy the public key"[^>]*title="Copy the public key"[^>]*><svg class="icon icon-small"[^>]*><use href="#i-copy">/);
    const words = text(clean(out));
    expect(words).toContain('Add this public key to your GitHub settings under SSH keys');
    expect(words).toContain("Open GitHub's SSH key page");
    expect(words).toContain('Check it works');
    const pages = new WebPageOpener(t.handle);
    await pages.open('github_ssh_keys');
    expect(t.transport.sentOf('app.open_web_page')[0]?.input).toEqual({ page: 'github_ssh_keys' });
  });

  it('Make a new key sends app.create_ssh_key and chooses the new key', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => withKeys(null)(tr).on('app.create_ssh_key', ok({ keys: [KEY], chosen: KEY.path })) });
    const ssh = new SshKeyModel(t.handle);
    await ssh.create();
    expect(t.transport.sentOf('app.create_ssh_key')[0]?.input).toEqual({});
    expect(ssh.chosenKey).toEqual(KEY);
  });

  it('Error path — no ssh-keygen is drawn as its sentence', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => withKeys(null)(tr).on('app.create_ssh_key', err('DSK_SSH_KEYGEN_MISSING', 'This computer has no tool for making SSH keys. Choose a key you already have instead.')) });
    const ssh = new SshKeyModel(t.handle);
    await ssh.load();
    await ssh.create();
    expect(ssh.error?.code).toBe('DSK_SSH_KEYGEN_MISSING');
    expect(text(clean(opened(t, ssh)))).toContain('This computer has no tool for making SSH keys.');
  });

  it('choosing sends the path, and choosing none sends null', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => withKeys(null)(tr).on('app.choose_ssh_key', (input) => ok({ keys: [KEY], chosen: (input as { path: string | null }).path })) });
    const ssh = new SshKeyModel(t.handle);
    await ssh.choose(KEY.path);
    expect(ssh.chosen).toBe(KEY.path);
    await ssh.choose(null);
    expect(t.transport.sentOf('app.choose_ssh_key').map((s) => s.input)).toEqual([{ path: KEY.path }, { path: null }]);
    expect(ssh.chosen).toBeNull();
  });

  it.each([
    ['works', 'It works.'],
    ['refused', "GitHub didn't accept this key."],
    ['unreachable', "ordin couldn't reach GitHub."],
  ] as const)('table: Check it works answering %s says so in plain words', async (outcome, line) => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => withKeys(KEY.path)(tr).on('app.check_ssh_key', ok({ outcome })) });
    const ssh = new SshKeyModel(t.handle);
    await ssh.load();
    await ssh.runCheck();
    expect(ssh.check).toBe(outcome);
    expect(text(clean(opened(t, ssh)))).toContain(line);
  });

  it('a backend that does not answer app.ssh_keys draws no SSH options', async () => {
    const t = await backendWith({ groups: GROUPS.filter((g) => g !== 'app'), setup: withKeys(null) });
    const model = new SignInsModel(t.handle);
    model.toggle('github');
    const out = html(SignIns, { backend: t.state, model });
    expect(out).not.toContain('With an SSH key');
  });
});
