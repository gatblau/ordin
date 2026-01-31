// The Settings walk-through (ScreensModule.md, Settings walk-through amendment; amended January 2026 (user
// decision)): the services list of Sign-ins, the empty states, the questions before a lowering or a removal,
// the lines that say what an action did, the labels read out, and Open in [IDE].
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { beforeAll, describe, expect, it } from 'vitest';

import { backendWith, PROJECT } from './fixed-support';
import { err, ok, R1_GROUPS } from './support/fake-backend';
import { uid } from './support/ids';
import { html, loadWindowCatalogue } from './support/ssr';
import { loaded, text } from './settings-support';
import About from '../src/lib/views/Settings/About.svelte';
import AgentRepositories from '../src/lib/views/Settings/AgentRepositories.svelte';
import CodeSection from '../src/lib/views/Settings/CodeSection.svelte';
import Display from '../src/lib/views/Settings/Display.svelte';
import Dictation from '../src/lib/views/Settings/Dictation.svelte';
import Email from '../src/lib/views/Settings/Email.svelte';
import History from '../src/lib/views/Settings/History.svelte';
import Jira from '../src/lib/views/Settings/Jira.svelte';
import KnowledgeAgents from '../src/lib/views/Settings/KnowledgeAgents.svelte';
import Language from '../src/lib/views/Settings/Language.svelte';
import Notifications from '../src/lib/views/Settings/Notifications.svelte';
import Plan from '../src/lib/views/Settings/Plan.svelte';
import Servers from '../src/lib/views/Settings/Servers.svelte';
import SignIns from '../src/lib/views/Settings/SignIns.svelte';
import Slack from '../src/lib/views/Settings/Slack.svelte';
import Telegram from '../src/lib/views/Settings/Telegram.svelte';
import Voice from '../src/lib/views/Settings/Voice.svelte';
import OpenInIde from '../src/lib/views/engineering/OpenInIde.svelte';
import FormBlock from '../src/lib/blocks/kinds/Form.svelte';
import Memory from '../src/lib/views/fixed/Memory.svelte';
import { CodeModel } from '../src/lib/views/Settings/code-model.svelte';
import { DisplayModel } from '../src/lib/views/Settings/display-model.svelte';
import { EmailModel } from '../src/lib/views/Settings/email-model.svelte';
import { HistoryModel } from '../src/lib/views/Settings/history-model.svelte';
import { KnowledgeAgentsModel } from '../src/lib/views/Settings/knowledge-agents.svelte';
import { LanguageSettingsModel } from '../src/lib/views/Settings/language-model.svelte';
import { NotificationsModel } from '../src/lib/views/Settings/notifications-model.svelte';
import { PlanModel, shareId } from '../src/lib/views/Settings/plan-model.svelte';
import { ReposModel } from '../src/lib/views/Settings/repos-model.svelte';
import { ServersModel } from '../src/lib/views/Settings/servers-model.svelte';
import { mailMethod, serviceAvailable, serviceRows } from '../src/lib/views/Settings/services';
import { JiraModel, SignInsModel } from '../src/lib/views/Settings/signins-model.svelte';
import { SlackModel } from '../src/lib/views/Settings/slack-model.svelte';
import { TelegramModel } from '../src/lib/views/Settings/telegram-model.svelte';
import { VoiceModel } from '../src/lib/views/Settings/voice-model.svelte';
import { OpenInIdeModel } from '../src/lib/views/engineering/open-in-ide.svelte';
import { AboutModel } from '../src/lib/views/Settings/updates.svelte';

beforeAll(loadWindowCatalogue);

const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const GROUPS = [...R1_GROUPS, 'profile', 'mail', 'telegram', 'voice', 'projects', 'usage'];
const credential = (kind: string, state = 'signed_in', over: Record<string, unknown> = {}) => ({ id: uid(`c-${kind}`), kind, state, account_label: `${kind}@example.com`, expires_at: null, ...over });
const profile = (over: Record<string, unknown> = {}) => ({ document: { format_version: 1, profile_version: '1.0.0', ...over }, source: 'verified', profile_version: '1.0.0' });

describe('Sign-ins lists every service (finding 1, 16, 17)', () => {
  // SM-NS-2.9 (amended January 2026; user decision): a service this version cannot sign in to is greyed out.
  it('table: which services this version can sign in to, by profile and build switch', () => {
    const doc = profile({ microsoft: { client_id: 'a', tenant_id: 'b' }, email: { system: 'company_server', enabled: true }, slack: { workspace: 'T1', client_id: 'c' } }).document as never;
    const all = () => true;
    const cases: Array<[string, boolean]> = [
      ['github', true], ['gitlab', true], ['microsoft', true], ['google', false], ['confluence', true], ['slack', true], ['mail', true], ['oidc', false],
    ];
    for (const [id, available] of cases) expect(serviceAvailable(id as never, doc, all), id).toBe(available);
    // Confluence needs no Atlassian app: an API token the person makes (SM-NS-2.11; amended January 2026;
    // user decision; Approved by the user January 2026); without confluence.connect_token it needs the app.
    expect(serviceAvailable('confluence', doc, () => false)).toBe(false);
    // Slack and the company's mail server also need their build switch.
    const none = () => false;
    expect(serviceAvailable('slack', doc, none)).toBe(false);
    expect(serviceAvailable('mail', doc, none)).toBe(false);
    // A Slack section without the company's app cannot sign in.
    expect(serviceAvailable('slack', profile({ slack: { tools_enabled: true } }).document as never, all)).toBe(false);
    // With no profile read, its part counts as present; the company account still needs one.
    expect(serviceAvailable('google', null, all)).toBe(true);
    expect(serviceAvailable('oidc', null, all)).toBe(false);
  });

  it('Happy path — every service is listed with what it is for; the ones this version cannot sign in to are greyed out with no control', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [] })).on('profile.get_document', ok(profile({ microsoft: { client_id: 'a', tenant_id: 'b' } }))) });
    const model = new SignInsModel(t.handle);
    await model.load();
    await model.loadCertificateRequirement();
    const markup = clean(html(SignIns, { backend: t.state, model }));
    const out = text(markup);
    expect(out).toContain("Ordin uses these to read and publish your documents, code and messages. Your passwords stay in this computer's keychain.");
    for (const line of ['GitHub, for your agent library and GitHub documents', 'GitLab, for GitLab repositories and documents', 'Microsoft 365, for SharePoint and email']) expect(out).toContain(line);
    // Jira is connected in Settings › Jira only (SM-NS-2.10).
    expect(out).not.toContain('Jira');
    for (const service of ['google', 'confluence', 'slack']) {
      const at = markup.indexOf(`data-service="${service}"`);
      const row = markup.slice(markup.lastIndexOf('<li', at), markup.indexOf('</li>', at));
      expect(row, service).toContain('aria-disabled="true"');
      expect(text(row), service).toContain('Not available in this version yet');
      expect(row, service).not.toMatch(/<(button|input|select)\b/);
    }
    expect(out).not.toMatch(/organisation|\bIT\b/);
  });

  it('Sign in of a service with nothing saved goes through identity.sign_in with that kind', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [] })).on('identity.sign_in', ok({ kind: 'already_signed_in', credential: credential('microsoft') })) });
    const model = new SignInsModel(t.handle);
    await model.startNew('microsoft');
    expect(t.transport.sentOf('identity.sign_in')[0]?.input).toEqual({ kind: 'microsoft' });
  });

  it('table: the mail server opens the dialog of the profile’s first method', () => {
    expect(mailMethod(profile({ email: { system: 'company_server', sign_in_methods: ['app_password'] } }).document as never)).toBe('app_password');
    expect(mailMethod(null)).toBe('mail_password');
  });

  it('the company mail server by single sign-on asks for the address, then signs in with it', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [] })).on('identity.sign_in', ok({ kind: 'already_signed_in', credential: credential('mail_server_single_sign_on') })) });
    const model = new SignInsModel(t.handle);
    model.openMailFor('single_sign_on');
    expect(model.mailSingleSignOn).toBe(true);
    const out = text(clean(html(SignIns, { backend: t.state, model })));
    expect(out).toContain("Your company's mail server uses your company account.");
    expect(out).not.toContain('App password');
    model.mailAddress = 'asha@example.com';
    expect(await model.saveMailPassword()).toBe(true);
    expect(t.transport.sentOf('identity.sign_in')[0]?.input).toEqual({ kind: 'mail_server_single_sign_on', mail_account: 'asha@example.com' });
  });

  it('a saved account reads "account · Signed in until [date]"; one that needs signing in is in the waiting colour', async () => {
    const t = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('identity.list', ok({ items: [credential('github', 'signed_in', { expires_at: '2026-12-03T00:00:00Z' }), credential('google', 'needs_sign_in')] })),
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    const markup = clean(html(SignIns, { backend: t.state, model }));
    expect(text(markup)).toMatch(/github@example\.com · Signed in until \S/);
    expect(markup).toMatch(/class="state-label waiting"[^>]*>[\s\S]*Sign in again to keep using it/);
    const rows = serviceRows(model.items, null);
    expect(rows.filter((r) => r.credential !== null).map((r) => r.service.id)).toEqual(['github', 'google']);
  });

  it('Sign out says what it did (finding 46)', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [credential('github')] })).on('identity.sign_out', ok({})) });
    const model = new SignInsModel(t.handle);
    await model.load();
    await model.signOut(uid('c-github'), 'Signed out of GitHub.');
    expect(model.done).toBe('Signed out of GitHub.');
    expect(clean(html(SignIns, { backend: t.state, model }))).toMatch(/role="status"[^>]*>Signed out of GitHub\./);
  });

  it('Error path — an empty GitLab token is not sent and says so; the box has its own Save token (finding 18)', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [] })) });
    const model = new SignInsModel(t.handle);
    await model.load();
    model.toggle('gitlab');
    model.token = '   ';
    await model.saveGitlabToken();
    expect(t.transport.sentOf('identity.save_gitlab_token')).toEqual([]);
    const out = text(clean(html(SignIns, { backend: t.state, model })));
    expect(out).toContain('Paste a token first.');
    expect(out).toContain('Save token');
  });

  it('Error path — an empty mail password is not sent and says so; Show becomes Hide (finding 46)', async () => {
    const t = await backendWith({ groups: GROUPS, setup: (tr) => tr.on('identity.list', ok({ items: [] })) });
    const model = new SignInsModel(t.handle);
    model.openMailFor('mail_password', 'asha@example.com');
    expect(await model.saveMailPassword()).toBe(false);
    expect(t.transport.sentOf('identity.save_mail_password')).toEqual([]);
    const markup = clean(html(SignIns, { backend: t.state, model }));
    expect(text(markup)).toContain('Type your password first.');
    // A real modal dialog (finding 19).
    expect(markup).toMatch(/class="dialog"[^>]*role="dialog"[^>]*aria-modal="true"/);
  });
});

describe('Settings › Email (finding 2, 24, 25, 47)', () => {
  const settings = { add_signature: false, agent_foot_line: false, agent_foot_line_locked: false, check: 'every15_minutes', signature: 'Asha' };

  it('Edge case — no mailbox signed in: the mailbox row says so with Sign in; no workspace links email says so', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('mail.settings', ok(settings)).on('identity.list', ok({ items: [] })));
    const model = new EmailModel(t.handle);
    await model.load();
    const markup = clean(html(Email, { backend: t.state, model }));
    const out = text(markup);
    expect(out).toContain('Your mailbox Not signed in — asked the first time a workspace links email');
    expect(out).toContain('Sign in');
    expect(out).toContain('No workspace links email yet.');
    // The signature box is off while Add my signature is off.
    expect(markup).toMatch(/<textarea[^>]*disabled/);
  });

  it('Sign in with no mailbox opens the mail dialog of the company server, or signs in to Microsoft 365', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('mail.settings', ok(settings)).on('identity.list', ok({ items: [] })).on('profile.get_document', ok(profile({ email: { system: 'microsoft365' } }))).on('identity.sign_in', ok({ kind: 'already_signed_in', credential: credential('microsoft') })),
    );
    const model = new EmailModel(t.handle);
    await model.load();
    await model.signIn();
    expect(t.transport.sentOf('identity.sign_in')[0]?.input).toEqual({ kind: 'microsoft' });

    const u = await loaded(GROUPS, (tr) => tr.on('mail.settings', ok(settings)).on('identity.list', ok({ items: [] })));
    const other = new EmailModel(u.handle);
    await other.load();
    await other.signIn();
    expect(other.signIns.mailDialog?.kind).toBe('mail_server_password');
  });

  it('Sign out of email acts at once and says what it did', async () => {
    const t = await loaded(GROUPS, (tr) => {
      let items = [credential('mail_server_password', 'signed_in', { account_label: 'asha@example.com' })];
      tr.on('mail.settings', ok(settings)).on('identity.list', () => ok({ items })).on('mail.verify_sign_in', ok('signed_in'));
      tr.on('identity.sign_out', () => ((items = []), ok({})));
    });
    const model = new EmailModel(t.handle);
    await model.load();
    await model.signOut("Signed out of email, and your password is removed from this computer's keychain.");
    expect(t.transport.sentOf('identity.sign_out')).toHaveLength(1);
    expect(text(clean(html(Email, { backend: t.state, model })))).toContain("Signed out of email, and your password is removed from this computer's keychain.");
  });

  it('the signature says Saved after leaving the box', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('mail.settings', ok({ ...settings, add_signature: true })).on('identity.list', ok({ items: [] })).on('mail.set_settings', (i) => ok({ ...settings, ...(i as object), agent_foot_line_locked: false })));
    const model = new EmailModel(t.handle);
    await model.load();
    model.signature = 'Asha Patel';
    expect(await model.saveSignature()).toBe(true);
    expect(text(clean(html(Email, { backend: t.state, model })))).toContain('Saved');
  });
});

describe('Settings › History asks before lowering (finding 4, 54)', () => {
  const setting = (days: number, max: number | null = null) => ok({ days, company_max_days: max, last_pruned_at: null });

  it('Happy path — a lower number asks first; confirming sends it and says it was saved', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('history.get', setting(90, 120)).on('history.set_days', (i) => setting((i as { days: number }).days, 120)));
    const model = new HistoryModel(t.handle);
    await model.load();
    expect(await model.submit('9')).toBe(false);
    expect(t.transport.sentOf('history.set_days')).toEqual([]);
    expect(model.pendingLower).toBe(9);
    const markup = clean(html(History, { backend: t.state, model }));
    const out = text(markup);
    expect(out).toContain('Remove conversations older than 9 days?');
    expect(out).toContain('They are removed from this computer at the next clean-up, within a day. An approval still waiting and work still running are never removed.');
    // The removing verb is never the default button.
    expect(markup).toMatch(/<button[^>]*class="danger"[^>]*>\s*Remove older conversations/);
    expect(out).toContain('Your company keeps history for at most 120 days.');
    expect(await model.confirmLower()).toBe(true);
    expect(t.transport.sentOf('history.set_days')[0]?.input).toEqual({ days: 9 });
    expect(text(clean(html(History, { backend: t.state, model })))).toContain('Saved. Conversations and history are kept for 9 days.');
  });

  it('Cancel sends nothing and the box shows the number kept; a higher number is sent at once', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('history.get', setting(90)).on('history.set_days', (i) => setting((i as { days: number }).days)));
    const model = new HistoryModel(t.handle);
    await model.load();
    await model.submit('30');
    model.cancelLower();
    expect(model.text).toBe('90');
    expect(t.transport.sentOf('history.set_days')).toEqual([]);
    expect(await model.submit('120')).toBe(true);
    expect(t.transport.sentOf('history.set_days')[0]?.input).toEqual({ days: 120 });
  });
});

describe('Settings › Telegram (finding 5, 26)', () => {
  it('Set up Telegram is three steps; an empty key is not sent and says so', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('telegram.status', ok({ offered: true, connected: false })));
    const model = new TelegramModel(t.handle);
    await model.load();
    expect(await model.connect()).toBe(false);
    expect(t.transport.sentOf('telegram.connect_bot')).toEqual([]);
    const out = text(clean(html(Telegram, { backend: t.state, model })));
    for (const step of ['Make your bot', 'Paste the key BotFather sent you', 'Add your bot to a group', '@BotFather', 'Paste the key first.']) expect(out).toContain(step);
  });

  it('Remove Telegram acts at once and says what it did', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('telegram.status', ok({ offered: true, connected: false })).on('telegram.remove', ok({})));
    const model = new TelegramModel(t.handle);
    await model.remove('Removed Telegram from ordin on this computer.');
    expect(t.transport.sentOf('telegram.remove')).toHaveLength(1);
    expect(text(clean(html(Telegram, { backend: t.state, model })))).toContain('Removed Telegram from ordin on this computer.');
  });
});

describe('Settings › Using your plan (finding 6, 7, 9, 12, 13, 44)', () => {
  const sv = <T>(value: T, set_by = 'person') => ({ set_by, value });
  const usage = (over: Record<string, unknown> = {}) => ({
    agents_at_same_time: sv(2), answer_simple_requests_without_model: sv(true), ask_before_estimated_requests: sv('heavy'), choice: sv('balanced'),
    continue_from_summary_after_break: sv(false), daily_pause_requests: { set_by: 'person' }, give_agents_only_needed_parts: sv(true), model_level_per_kind: sv({}),
    reuse_answers: sv(true), show_usage_on_each_run: sv(false, 'company'), summarise_long_conversations: sv('early'), thinking: sv('automatic', 'company'), updated_at: '2026-01-10T10:00:00Z', ...over,
  });
  const draw = (settings: unknown) => clean(html(Plan, { backend: null, model: Object.assign(new PlanModel(null as never), { settings, today: { typical_day_permille: 200, model_turns: 4, smaller_share_permille: 500, answered_without_model: 1 } }) }));

  it('each row has its line; Custom is not drawn unless it is the choice; Balanced is the default; Early saves most', () => {
    const out = text(draw(usage()));
    for (const line of ['use none of your plan', 'within 7 days', 'an agent can still read the rest', 'a summary costs less', 'early pays off only', 'show their estimated use first', 'uses your plan faster', 'on its progress']) expect(out).toContain(line);
    expect(out).not.toContain('Custom');
    expect(out).toContain('Balanced (default)');
    expect(out).toContain('Early (saves most)');
    expect(out).toContain("Today you've used about a fifth of a typical day's plan.");
    expect(text(draw(usage({ choice: sv('custom') })))).toContain('Custom You chose the model for each kind of work under More choices.');
  });

  it('Set by your company is in the row’s line, also for Show usage on each run and Thinking', () => {
    const markup = draw(usage());
    expect(markup.match(/<small[^>]*>[^<]*<b[^>]*>Set by your company<\/b>/g)?.length ?? 0).toBeGreaterThanOrEqual(1);
    const more = markup.slice(markup.indexOf('class="group more'));
    expect(text(more).match(/Set by your company/g)).toHaveLength(2);
  });

  it('− and + are named, and the number is read out when it changes', () => {
    const markup = draw(usage());
    expect(markup).toContain('aria-label="Fewer agents at the same time"');
    expect(markup).toContain('aria-label="More agents at the same time"');
    expect(markup).toMatch(/<output[^>]*aria-live="polite"/);
  });

  it('table: today’s use is said in words', () => {
    const cases: Array<[number, string | null]> = [[0, 'settings-plan-share-small'], [100, 'settings-plan-share-tenth'], [200, 'settings-plan-share-fifth'], [300, 'settings-plan-share-third'], [500, 'settings-plan-share-half'], [800, 'settings-plan-share-most'], [1000, null]];
    for (const [permille, id] of cases) expect(shareId(permille), String(permille)).toBe(id);
  });
});

describe('Settings › Slack (finding 27, 28, 29)', () => {
  const status = (over: Record<string, unknown> = {}) => ({
    direct_messages_allowed: true, names_allowed: true, offered: true, own_app: false, person_name: 'Asha', posting_allowed: true, profile_app: true,
    rules_allowed: true, scope_needed: false, signed_in: true, workspace_name: 'Northwind', manifest: null, reason_not_offered: null, ...over,
  });
  const rule = (state: string) => ({ rule: { id: uid(`r-${state}`), channel_id: 'C1', channel_name: 'suppliers', created_at: '2026-01-10T10:00:00Z', daily_cap: 20, kind: 'handover_notices', scope: { kind: 'link', slack_link_id: uid('l1') }, state }, sent_today: 1 });

  it('a rule names its kind and state; one turned off is listed after, with no Turn off', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('slack.status', ok(status())).on('slack.rules', ok([rule('off'), rule('on')])));
    const model = new SlackModel(t.handle);
    await model.load();
    const out = text(clean(html(Slack, { backend: t.state, model })));
    expect(out).toContain('Handover notices in #suppliers · up to 20 a day · on');
    expect(out).toContain('Handover notices in #suppliers · up to 20 a day · off');
    expect(out.indexOf('· on')).toBeLessThan(out.indexOf('· off'));
    expect(out.match(/Turn off/g)).toHaveLength(1);
  });

  it('table: one sentence per reason Slack is not offered', async () => {
    const cases: Array<[string, string]> = [
      ['tools_turned_off', "Slack isn't available here."],
      ['no_slack_app', "Your company hasn't set up a Slack app for ordin."],
      ['sign_in_not_offered_here', "Signing in to Slack isn't offered on this computer."],
      ['app_not_approved', 'Handovers still go by email.'],
    ];
    for (const [reason, sentence] of cases) {
      const t = await loaded(R1_GROUPS, (tr) => tr.on('slack.status', ok(status({ signed_in: false, offered: false, rules_allowed: false, reason_not_offered: reason }))));
      const model = new SlackModel(t.handle);
      await model.load();
      expect(text(html(Slack, { backend: t.state, model })), reason).toContain(sentence);
    }
  });

  it('Connect your own Slack app labels both boxes and its verb is Connect', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('slack.status', ok(status({ signed_in: false, profile_app: false, manifest: 'display_information: {}' }))).on('slack.rules', ok([])));
    const model = new SlackModel(t.handle);
    await model.load();
    model.ownAppOpen = true;
    const out = text(clean(html(Slack, { backend: t.state, model })));
    expect(out).toContain("Client ID (on your app's Basic Information page)");
    expect(out).toContain('App description to copy');
    expect(out).toContain('Copy');
    expect(out).toMatch(/Connect Cancel/);
  });
});

describe('Settings › Jira (finding 30, 31, 32, 49)', () => {
  it('Disconnect asks "Disconnect Jira?" with what is deleted and what stays, verb Disconnect', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('identity.list', ok({ items: [credential('jira', 'signed_in', { account_label: 'acme.atlassian.net' })] })));
    const model = new JiraModel(t.handle);
    await model.load();
    model.confirmDisconnect = true;
    const out = text(clean(html(Jira, { backend: t.state, model })));
    expect(out).toContain('Connected to acme.atlassian.net');
    expect(out).toContain('Disconnect Jira?');
    expect(out).toContain("The token is deleted from this computer's keychain. Linked issues stay in their workspaces; nothing is read until you connect again.");
    expect(out).toMatch(/Disconnect Cancel/);
  });

  it('the site message is tied to its field; the token guide is three numbered lines with Atlassian’s page', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('identity.list', ok({ items: [] })));
    const model = new JiraModel(t.handle);
    await model.load();
    await model.connect();
    const markup = clean(html(Jira, { backend: t.state, model }));
    expect(markup).toMatch(/aria-invalid="true"[^>]*aria-describedby="settings-jira-site-needed"|aria-describedby="settings-jira-site-needed"[^>]*aria-invalid="true"/);
    expect(markup.match(/<li>/g)?.length).toBe(3);
    expect(markup).toContain('https://id.atlassian.com/manage-profile/security/api-tokens');
  });
});

describe('Settings › Servers (finding 33, 34, 35, 36, 51)', () => {
  const server = (over: Record<string, unknown> = {}) => ({ server_connection_id: uid('s1'), display_name: 'Northwind', state: 'connected', backend: null, as_of: null, older_side: null, ...over });

  it('Edge case — no server: one line says how to connect one', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('servers.list', ok([])));
    const model = new ServersModel(t.handle, () => undefined);
    await model.load();
    expect(text(clean(html(Servers, { backend: t.state, model })))).toContain('No servers yet. Connect to one with the code from whoever runs it.');
  });

  it('Remove this server… asks first and sends servers.remove only on Remove; a connected server says Connected', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('servers.list', ok([server()])).on('servers.remove', ok({})));
    const model = new ServersModel(t.handle, () => undefined);
    await model.load();
    model.confirmRemove = uid('s1');
    const out = text(clean(html(Servers, { backend: t.state, model })));
    expect(out).toContain('Connected');
    expect(out).toContain('Remove Northwind from this computer?');
    expect(out).toContain('To connect again you need a new code from whoever runs it.');
    expect(t.transport.sentOf('servers.remove')).toEqual([]);
    await model.remove(uid('s1'), 'Removed Northwind from this computer.');
    expect(t.transport.sentOf('servers.remove')[0]?.input).toEqual({ server_connection_id: uid('s1') });
    expect(model.done).toBe('Removed Northwind from this computer.');
  });

  it('a server that cannot update itself says so; Connect again… opens the form under its row with the pasted-line hint', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('servers.list', ok([server({ state: 'disconnected' })])));
    const model = new ServersModel(t.handle, () => undefined);
    await model.load();
    model.lines = { [uid('s1')]: { kind: 'available', update: { version: '2.0.0', notes: '', self_update: false }, selfUpdate: false } };
    model.openConnect(uid('s1'));
    const out = text(clean(html(Servers, { backend: t.state, model })));
    expect(out).toContain("This server can't update itself. Whoever runs it installs new versions.");
    expect(out).toContain('Connect again to Northwind');
    expect(out).toContain('Or paste the one line you were given here and leave the code empty.');
  });
});

describe('Settings › Language (finding 37, 52)', () => {
  it('the picker is a radio group with the language in use checked; no language of your own says so', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('localisation.list_languages', ok({ areas: [], languages: [] }));
    });
    const model = new LanguageSettingsModel(t.handle);
    await model.load();
    const markup = clean(html(Language, { backend: t.state, model }));
    expect(markup).not.toContain('role="listbox"');
    expect(markup).toMatch(/<input[^>]*type="radio"[^>]*checked/);
    expect(text(markup)).toContain('Same as this computer');
    expect(text(markup)).toContain("You haven't made or imported a language yet.");
  });
});

describe('Other sections (finding 38, 40, 41, 45, 50, 53, 55, 56)', () => {
  it('Knowledge agents with no workspace say to open or make one, and draw no picker', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('knowledge_agents.list', ok({ connections: [] })));
    const model = new KnowledgeAgentsModel(t.handle, '');
    const markup = clean(html(KnowledgeAgents, { backend: t.state, model }));
    expect(text(markup)).toContain('Open or make a workspace to connect a knowledge agent to it.');
    expect(markup).not.toContain('<select');
  });

  it('Display set by the company says so', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('settings.get', ok({ section: 'display', settings: [{ key: 'display.appearance', value: 'dark', set_by: 'company', person_may_change: false }] })));
    const model = new DisplayModel(t.handle);
    await model.load();
    expect(text(clean(html(Display, { backend: t.state, model })))).toContain('Set by your company');
  });

  it('How Ordin talks to you marks the defaults, explains Call me and counts the notes', async () => {
    const sv = <T>(value: T) => ({ set_by: 'person', value });
    const model = Object.assign(new VoiceModel(null as never), { settings: { tone: sv('neutral'), register: sv('formal'), length: sv('shortest'), name_to_use: sv(''), notes: sv('') }, notes: 'Hello' });
    const markup = clean(html(Voice, { backend: null, model }));
    const out = text(markup);
    expect(out).toContain('Friendly (default)');
    expect(out).toContain('Everyday (default)');
    expect(out).toContain('Brief (default)');
    expect(out).toContain('Used in greetings only. Leave empty for no name.');
    expect(out).toContain('5 of 300');
    expect(markup).toMatch(/<textarea[^>]*aria-describedby="settings-voice-notes-count/);
  });

  it('Code: the line on what ticking does, each control named with its language, Restart says it restarted, no languages says so', async () => {
    const plugin = { id: uid('lp1'), language: 'Rust', default_server: 'rust-analyzer', chosen_server: null, approved_servers: ['rust-analyzer', 'other'], enabled: true, pinned_version: null, server_settings: {}, source: 'organisation_profile' };
    const t = await loaded(GROUPS, (tr) => tr.on('projects.ides', ok([{ id: 'vscode', name: 'VS Code', enabled: true }])).on('projects.language_plugins', ok([plugin])).on('projects.restart_language_server', ok({})));
    const model = new CodeModel(t.handle);
    await model.load();
    await model.restart(model.plugins[0]!, 'Restarted the language server for Rust.');
    const markup = clean(html(CodeSection, { backend: t.state, model })).replace(/[⁨⁩]/g, '');
    expect(text(markup)).toContain('Each one ticked appears as "Open in …" on the Code group, its repository and each file.');
    expect(markup).toContain('aria-label="Code checking for Rust"');
    expect(markup).toContain('aria-label="Language server for Rust"');
    expect(text(markup)).toContain('Restarted the language server for Rust.');
    const none = Object.assign(new CodeModel(t.handle), { ides: [], plugins: [] });
    expect(text(clean(html(CodeSection, { backend: t.state, model: none })))).toContain('No language has code checking yet.');
  });

  it('Updates is headed Updates and holds the version and Check for updates, with no separate What\'s new button', async () => {
    const t = await loaded(R1_GROUPS);
    const model = new AboutModel(t.handle, async () => '1.2.3', null);
    const out = text(clean(html(About, { backend: t.state, model })));
    expect(out).toContain('Updates');
    expect(out).toContain('Check for updates');
    expect(out).not.toContain('About ordin');
    expect(out).not.toMatch(/>\s*What's new\s*</);
  });

  it('Dictation says where to start it and that speech stays on the computer; Notifications’ intro is plain', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('settings.get', ok({ section: 'notifications', settings: [] })));
    expect(text(clean(html(Dictation, { backend: t.state, initial: { state: 'available', locale: 'en-GB' } })))).toContain('Your speech stays on this computer');
    const model = new NotificationsModel(t.handle);
    expect(text(clean(html(Notifications, { backend: t.state, model })))).toContain("ordin shows a notification on your computer only for what's switched on here.");
  });
});

describe('Settings › Memory has one heading (finding 42)', () => {
  it('embedded in Settings, the memory view draws no heading of its own', async () => {
    const t = await loaded([...R1_GROUPS, 'memory']);
    const alone = clean(html(Memory, { backend: t.state, workspaceId: uid('w1'), instance: null }));
    const inside = clean(html(Memory, { backend: t.state, workspaceId: uid('w1'), instance: null, embedded: true }));
    expect(alone).toContain('id="memory-heading"');
    expect(inside).not.toContain('id="memory-heading"');
    expect(inside).not.toMatch(/<h2/);
  });
});

describe('Settings › Agent repositories (finding 20, 21, 22, 23)', () => {
  const repo = (over: Record<string, unknown> = {}) => ({
    agent_count: 12, set_by_company: false, last_fetch: null,
    repository: { id: uid('ar1'), display_name: 'Finance agents', host: 'github.com', format: 'ordin', format_version: '1', added_at: '2026-01-10T10:00:00Z', clone_path: '/x', person_can_curate: true, person_can_publish: false, protected_branch: 'main', repository: 'northwind/finance-agents', ...over },
  });

  it('Edge case — no repository: the intro and one line saying how to add one', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('catalogue.list_repositories', ok([])));
    const model = new ReposModel(t.handle);
    await model.load();
    const out = text(clean(html(AgentRepositories, { backend: t.state, model })));
    expect(out).toContain('Where the agents in your agent library come from.');
    expect(out).toContain('No agent repositories yet. Add one to bring its agents into your library.');
  });

  it('a row is the name over "On GitHub · 12 agents"; Rename… is a dialog named after the repository', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('catalogue.list_repositories', ok([repo()])));
    const model = new ReposModel(t.handle);
    await model.load();
    model.startRename(model.items[0]!);
    const markup = clean(html(AgentRepositories, { backend: t.state, model }));
    expect(text(markup)).toContain('Finance agents On GitHub · 12 agents');
    expect(text(markup)).not.toContain('ordin format');
    expect(markup).toMatch(/role="dialog"[^>]*aria-labelledby="settings-repos-rename-title"/);
    expect(text(markup)).toContain('Rename Finance agents');
  });

  it('Add shows the address example and says what it added', async () => {
    const t = await loaded(R1_GROUPS, (tr) =>
      tr.on('catalogue.list_repositories', ok([])).on('catalogue.add_repository', ok(repo().repository)).on('catalogue.probe_repository', ok({ probe_id: uid('pr1'), agent_count: 12, format: 'ordin', person_can_publish: false, suggested_name: 'Finance agents', expires_at: '2999-01-01T00:00:00Z' })),
    );
    const model = new ReposModel(t.handle);
    await model.load();
    model.openAdd();
    expect(text(clean(html(AgentRepositories, { backend: t.state, model })))).toContain('For example https://github.com/northwind/finance-agents');
    model.address = 'https://github.com/northwind/finance-agents';
    await model.lookUp();
    expect(await model.add((name, count) => `Added ${name}. Its ${count} agents are in your agent library.`)).toBe(true);
    expect(model.done).toBe('Added Finance agents. Its 12 agents are in your agent library.');
  });
});

describe('Open in [IDE] (finding 3)', () => {
  const groups = [...R1_GROUPS, 'projects'];

  it('Happy path — one IDE ticked is one button; opening sends projects.open_in_ide and a notice confirms it', async () => {
    const t = await backendWith({ groups, setup: (tr) => tr.on('projects.ides', ok([{ id: 'idea', name: 'IntelliJ IDEA', enabled: true }, { id: 'vscode', name: 'VS Code', enabled: false }])).on('projects.open_in_ide', ok({})) });
    const model = new OpenInIdeModel(t.handle);
    await model.load();
    expect(model.ides.map((i) => i.id)).toEqual(['idea']);
    const markup = clean(html(OpenInIde, { backend: t.state, projectId: PROJECT, path: 'src/main.rs', name: 'main.rs', model }));
    expect(text(markup)).toContain('Open in IntelliJ IDEA');
    expect(markup).not.toContain('aria-haspopup="menu"');
    expect(await model.open(model.ides[0]!, PROJECT, 'src/main.rs', 'Opened main.rs in IntelliJ IDEA.')).toBe(true);
    expect(t.transport.sentOf('projects.open_in_ide')[0]?.input).toEqual({ project_id: PROJECT, ide_id: 'idea', path: 'src/main.rs' });
    expect(text(clean(html(OpenInIde, { backend: t.state, projectId: PROJECT, path: 'src/main.rs', name: 'main.rs', model })))).toContain('Opened main.rs in IntelliJ IDEA.');
  });

  it('two or more IDEs are items of the ⋯ menu; none ticked draws nothing', async () => {
    const t = await backendWith({ groups, setup: (tr) => tr.on('projects.ides', ok([{ id: 'idea', name: 'IntelliJ IDEA', enabled: true }, { id: 'vscode', name: 'VS Code', enabled: true }])) });
    const model = new OpenInIdeModel(t.handle);
    await model.load();
    const markup = clean(html(OpenInIde, { backend: t.state, projectId: PROJECT, name: 'Billing', model }));
    expect(markup).toContain('aria-haspopup="menu"');
    expect(markup).toContain('aria-label="Open in an IDE"');
    const none = Object.assign(new OpenInIdeModel(t.handle), { ides: [] });
    expect(text(clean(html(OpenInIde, { backend: t.state, projectId: PROJECT, name: 'Billing', model: none }))).trim()).toBe('');
  });

  it('Error path — the IDE does not start: its sentence, and no notice', async () => {
    const t = await backendWith({ groups, setup: (tr) => tr.on('projects.open_in_ide', err('PROJ_IDE_START_FAILED', "ordin couldn't open the IDE.")) });
    const model = Object.assign(new OpenInIdeModel(t.handle), { ides: [{ id: 'idea', name: 'IntelliJ IDEA', enabled: true }] });
    expect(await model.open(model.ides[0]!, PROJECT, null, 'Opened Billing in IntelliJ IDEA.')).toBe(false);
    expect(model.notice).toBe('');
    expect(text(clean(html(OpenInIde, { backend: t.state, projectId: PROJECT, name: 'Billing', model })))).toContain("ordin couldn't open the IDE.");
  });
});

describe('The form block’s document field opens the document browser (SM-TF-9)', () => {
  it('a document field is one Choose a document… button, not two drop-down lists', async () => {
    const t = await loaded(R1_GROUPS);
    const block = { kind: 'form', title: 'Apply a plan phase', verb: 'Apply', target: { kind: 'agent_instance', agent_instance_id: uid('a1') }, fields: [{ name: 'plan', label: 'Plan', required: true, input: 'document', value: null, filled_by_ordin: false }] };
    const markup = clean(html(FormBlock, { backend: t.state, workspaceId: uid('w1'), blockId: 'f1', block }));
    expect(text(markup)).toContain('Choose a document…');
    expect(markup).not.toContain('<select');
    expect(markup).toContain('aria-haspopup="dialog"');
  });
});

describe('Layout: the shared classes and the Settings frame (finding 8, 10, 11, 14, 15, 43)', () => {
  const ROOT = join(__dirname, '..');
  const CSS = readFileSync(join(ROOT, 'src/styles/tokens.css'), 'utf8');
  const PLAN = readFileSync(join(ROOT, 'src/lib/views/Settings/Plan.svelte'), 'utf8');
  const SETTINGS = readFileSync(join(ROOT, 'src/lib/views/Settings/Settings.svelte'), 'utf8');
  /** The declarations of the first block whose selector text equals `selector`. */
  function block(css: string, selector: string): Map<string, string> {
    const at = css.indexOf(`${selector} {`);
    if (at < 0) throw new Error(`no block ${selector}`);
    const body = css.slice(css.indexOf('{', at) + 1, css.indexOf('}', at));
    const out = new Map<string, string>();
    for (const line of body.split(';')) {
      const colon = line.indexOf(':');
      if (colon > 0) out.set(line.slice(0, colon).trim(), line.slice(colon + 1).trim());
    }
    return out;
  }

  it('table: the classes several sections share are styled once, in tokens.css', () => {
    const cases: Array<[string, string, string]> = [
      ['.tag', 'color', 'var(--color-text-muted)'],
      ['.invalid', 'color', 'var(--color-danger)'],
      ['.side-panel', 'border', '1px solid var(--color-line)'],
      ['.setting-row', 'display', 'flex'],
      ['.choice-group', 'border', '0'],
      ['.field-stack', 'flex-direction', 'column'],
      ['.switch-line', 'align-items', 'center'],
      ['.settings-section h4', 'font-size', '13px'],
    ];
    for (const [selector, property, value] of cases) expect(block(CSS, selector).get(property), selector).toBe(value);
    expect(block(CSS, '.secondary,\n.hint').get('color')).toBe('var(--color-text-muted)');
  });

  it('the Plan cards are drawn as Memory draws its groups, and each row wraps its control under its words', () => {
    // Amended January 2026 (user decision; Approved by the user January 2026): one card per group.
    expect(block(PLAN, '.group').get('border')).toBe('1px solid var(--color-line)');
    expect(block(PLAN, '.rows').get('gap')).toBe('0');
    expect(block(PLAN, '.group h4').get('font-size')).toBe('13px');
    // A row is one wrapping flex line whose height follows its content; its control sits at the end.
    expect(block(PLAN, '.row').get('display')).toBe('flex');
    expect(block(PLAN, '.row').get('flex-wrap')).toBe('wrap');
    expect(block(PLAN, '.control').get('margin-inline-start')).toBe('auto');
    // Only tokens are used for colours.
    expect(PLAN.slice(PLAN.indexOf('<style>'))).not.toMatch(/#[0-9a-fA-F]{3,6}\b/);
  });

  it('Settings lists the sections down the side from 700 px and offers a Section menu below that', async () => {
    // Amended January 2026: 700 px of the Settings screen's own width, not the window's.
    expect(SETTINGS).not.toContain('@media (min-width: 700px)');
    expect(SETTINGS).toContain('const WIDE = 700;');
    expect(SETTINGS).toContain('class:wide={width >= WIDE} bind:clientWidth={width}');
    expect(block(SETTINGS, '.settings.wide').get('display')).toBe('grid');
    expect(block(SETTINGS, '.settings.wide .section-list').get('display')).toBe('flex');
    expect(block(SETTINGS, '.settings.wide .section-menu').get('display')).toBe('none');
    expect(block(SETTINGS, '.section-menu').get('display')).toBe('flex');
    expect(block(SETTINGS, '.section-list').get('display')).toBe('none');
  });
});
