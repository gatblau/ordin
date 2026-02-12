// SM-NS-2.12: Microsoft 365 without a company profile, through the build's own Microsoft app (ADR-126).
import { beforeAll, describe, expect, it } from 'vitest';

import { backendWith } from './fixed-support';
import { err, ok, R1_GROUPS, settle } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { text } from './settings-support';
import { uid } from './support/ids';
import SignIns from '../src/lib/views/Settings/SignIns.svelte';
import { SignInsModel } from '../src/lib/views/Settings/signins-model.svelte';
import { serviceAvailable, serviceRows } from '../src/lib/views/Settings/services';

beforeAll(loadWindowCatalogue);

const GROUPS = [...R1_GROUPS, 'profile'];
const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const noMicrosoft = { format_version: 1, profile_version: '1.0.0' } as never;
const all = () => true;

describe('Microsoft 365 without a company profile (SM-NS-2.12)', () => {
  it('table: the Microsoft row is available with the build\'s app, or with a profile that names one', () => {
    const company = { format_version: 1, profile_version: '1.0.0', microsoft: { client_id: 'a', tenant_id: 'b' } } as never;
    const cases: Array<[unknown, boolean, boolean]> = [
      [noMicrosoft, false, false],
      [noMicrosoft, true, true],
      [company, false, true],
      [company, true, true],
    ];
    for (const [document, builtIn, available] of cases) {
      expect(serviceAvailable('microsoft', document as never, all, builtIn), `${builtIn}`).toBe(available);
    }
    const row = serviceRows([], noMicrosoft, all, true).find((r) => r.service.id === 'microsoft');
    expect(row?.available).toBe(true);
  });

  it('Happy path — identity.list\'s microsoft_built_in is read; a missing field counts as false', async () => {
    for (const [answer, expected] of [[{ items: [], microsoft_built_in: true }, true], [{ items: [] }, false]] as const) {
      const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok(answer)) });
      const model = new SignInsModel(t.handle);
      await model.load();
      expect(model.microsoftBuiltIn).toBe(expected);
    }
  });

  it('Happy path — waiting on the build\'s app says to use a work or school account; a company app does not', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [], microsoft_built_in: true })) });
    const model = new SignInsModel(t.handle);
    await model.load();
    model.challenge = { kind: 'browser_opened', challenge_id: uid('ms1'), expires_at: 'x' };
    model.challengeKind = 'microsoft';
    model.document = noMicrosoft;
    expect(text(clean(html(SignIns, { backend: t.state, model })))).toContain('Sign in with your work or school account.');
    model.document = { format_version: 1, profile_version: '1.0.0', microsoft: { client_id: 'a', tenant_id: 'b' } } as never;
    expect(text(clean(html(SignIns, { backend: t.state, model })))).not.toContain('Sign in with your work or school account.');
  });

  it('Error path — an organisation that must approve the app is drawn as its sentence, with no Try again', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr
          .on('identity.list', ok({ items: [], microsoft_built_in: true }))
          .on('identity.sign_in', ok({ kind: 'browser_opened', challenge_id: uid('ms2'), expires_at: 'x' }))
          .on('identity.sign_in_wait', err('CRED_ADMIN_CONSENT_REQUIRED', 'Your organisation has to approve ordin.')),
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    expect(await model.signIn('microsoft')).toBe('error');
    await settle();
    expect(model.error?.code).toBe('CRED_ADMIN_CONSENT_REQUIRED');
    expect(model.error?.action ?? null).toBeNull();
  });
});
