// SM-NS-2: Settings › Sign-ins — the rows, the browser and device-code waits, the mail password dialog and
// the client certificate import.
import { beforeAll, describe, expect, it } from 'vitest';

import { backendWith } from './fixed-support';
import { err, type FakeTransport, ok, R1_GROUPS, settle } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { text } from './settings-support';
import SignIns from '../src/lib/views/Settings/SignIns.svelte';
import { kindMessageId, MAIL_PASSWORD_KINDS, SignInsModel } from '../src/lib/views/Settings/signins-model.svelte';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const credential = (kind: string, state = 'signed_in', over: Record<string, unknown> = {}) => ({ id: `c-${kind}`, kind, state, account_label: `${kind}@example.com`, expires_at: null, ...over });
const GROUPS = [...R1_GROUPS, 'profile'];

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
    // The address is text, never a link; the code is monospaced.
    expect(markup).not.toMatch(/href=/);
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
      expect(markup).not.toMatch(/href=/);
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

  it('Error path — CRED_PROVIDER_NOT_CONFIGURED for gitlab switches the row to the token box with no sentence', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [credential('gitlab', 'signed_out')] })).on('identity.sign_in', err('CRED_PROVIDER_NOT_CONFIGURED', 'GitLab sign-in is not set up.')) });
    const model = new SignInsModel(t.handle);
    await model.load();
    expect(await model.signIn('gitlab')).toBe('not_configured');
    expect(model.error).toBeNull();
    const out = text(html(SignIns, { backend: t.state, model }));
    expect(out).toContain('Paste a GitLab access token');
    expect(out).not.toContain('GitLab sign-in is not set up.');
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
