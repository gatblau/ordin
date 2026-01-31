// Settings: History, Display, Sign-ins, Jira, Slack and Knowledge agents.
import { beforeAll, describe, expect, it } from 'vitest';

import { err, event, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
import { stripIsolates } from '../src/lib/i18n/fluent';
import { html, loadWindowCatalogue } from './support/ssr';
import { loaded, text } from './settings-support';
import Display from '../src/lib/views/Settings/Display.svelte';
import History from '../src/lib/views/Settings/History.svelte';
import Jira from '../src/lib/views/Settings/Jira.svelte';
import KnowledgeAgents from '../src/lib/views/Settings/KnowledgeAgents.svelte';
import SignIns from '../src/lib/views/Settings/SignIns.svelte';
import Slack from '../src/lib/views/Settings/Slack.svelte';
import { DisplayModel, splitAppearance } from '../src/lib/views/Settings/display-model.svelte';
import { HistoryModel } from '../src/lib/views/Settings/history-model.svelte';
import { accountLine, KnowledgeAgentsModel } from '../src/lib/views/Settings/knowledge-agents.svelte';
import { JiraModel, jiraHost, reloadOnSignIn, SignInsModel } from '../src/lib/views/Settings/signins-model.svelte';
import { SlackModel } from '../src/lib/views/Settings/slack-model.svelte';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');

describe('Settings › History', () => {
  const setting = (days: number) => ok({ days, company_max_days: null, last_pruned_at: null });

  it('Edge case — history setting refused in place', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('history.get', setting(90)));
    const model = new HistoryModel(t.handle);
    await model.load();
    expect(model.text).toBe('90');
    const sent = await model.submit('0');
    expect(sent).toBe(false);
    expect(t.transport.sentOf('history.set_days')).toEqual([]);
    expect(model.days).toBe(90);
    const markup = clean(html(History, { backend: t.state, model }));
    expect(text(markup)).toContain('Enter a whole number of days, 1 or more.');
    // The message is tied to the box for screen readers; the box keeps the typed text.
    expect(markup).toMatch(/aria-describedby="history-days-error"/);
    expect(markup).toMatch(/id="history-days-error"/);
    expect(markup).toMatch(/value="0"/);
  });

  it('a valid number is sent and replaces the last value', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('history.get', setting(90));
      tr.on('history.set_days', (i) => setting((i as { days: number }).days));
    });
    const model = new HistoryModel(t.handle);
    await model.load();
    expect(await model.submit('30')).toBe(true);
    expect(t.transport.sentOf('history.set_days')[0]?.input).toEqual({ days: 30 });
    expect(model.days).toBe(30);
    expect(model.invalid).toBe(false);
  });

  it('table: what is a whole number of days, 1 or more', () => {
    for (const [input, expected] of [['1', 1], ['365', 365], [' 7 ', 7], ['0', null], ['-3', null], ['1.5', null], ['abc', null], ['', null], ['12 days', null]] as const) {
      expect(HistoryModel.parse(input), input).toBe(expected);
    }
  });

  it('a refusal keeps the last value and is drawn by the error line', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('history.get', setting(90));
      tr.on('history.set_days', err('JRNL_RETENTION_ABOVE_COMPANY_MAX', 'Your company keeps history for at most 60 days.'));
    });
    const model = new HistoryModel(t.handle);
    await model.load();
    expect(await model.submit('120')).toBe(false);
    expect(model.days).toBe(90);
    expect(model.text).toBe('90');
    expect(text(html(History, { backend: t.state, model }))).toContain('Your company keeps history for at most 60 days.');
  });

  it('says it is the only way earlier conversations are removed, with the box inside the sentence', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('history.get', setting(90)));
    const model = new HistoryModel(t.handle);
    await model.load();
    const out = text(html(History, { backend: t.state, model }));
    expect(out).toContain('Keep conversations and history for');
    expect(out).toContain('days');
    expect(out).toContain('This is the only way earlier conversations are removed.');
  });
});

describe('Settings › Display', () => {
  it('splits the one message into the label and the three choices', () => {
    expect(splitAppearance('Appearance: Same as this computer / Light / Dark')).toEqual({ label: 'Appearance', options: ['Same as this computer', 'Light', 'Dark'] });
  });

  it('reads display.appearance and changes it through settings.change', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('settings.get', ok({ section: 'display', settings: [{ key: 'display.appearance', value: 'light', set_by: 'person', person_may_change: true }] }));
      tr.on('settings.change', (i) => ok({ key: 'display.appearance', value: (i as { value: string }).value, set_by: 'person', person_may_change: true }));
    });
    const model = new DisplayModel(t.handle);
    await model.load();
    expect(t.transport.sentOf('settings.get')[0]?.input).toEqual({ section: 'display' });
    expect(model.appearance).toBe('light');
    await model.choose('dark');
    expect(t.transport.sentOf('settings.change')[0]?.input).toEqual({ key: 'display.appearance', value: 'dark' });
    expect(model.appearance).toBe('dark');
    const out = clean(html(Display, { backend: t.state, model }));
    expect(text(out)).toContain('Appearance');
    expect(out).toMatch(/value="dark" checked/);
  });

  it('a value the company set cannot be changed', async () => {
    const t = await loaded(R1_GROUPS, (tr) =>
      tr.on('settings.get', ok({ section: 'display', settings: [{ key: 'display.appearance', value: 'dark', set_by: 'company', person_may_change: false }] })),
    );
    const model = new DisplayModel(t.handle);
    await model.load();
    expect(html(Display, { backend: t.state, model })).toMatch(/<fieldset[^>]*disabled/);
  });
});

describe('Settings › Sign-ins', () => {
  const credential = (kind: string, state = 'signed_in', id = `c-${kind}`) => ({ id, kind, state, account_label: `${kind} account`, expires_at: null });

  it('Edge case — sign_in_changed with credential_kind', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('identity.list', ok({ items: [credential('gitlab', 'signed_in')] })));
    const model = new SignInsModel(t.handle);
    await model.load();
    const stop = reloadOnSignIn(t.state, () => model.load());
    t.transport.on('identity.list', ok({ items: [credential('gitlab', 'needs_sign_in')] }));
    t.transport.sent.length = 0;
    await t.state.apply(event(LOCAL, t.state.lastSeq + 1, { kind: 'sign_in_changed', credential_id: uid('c-gitlab'), credential_kind: 'gitlab', state: 'needs_sign_in' } as never));
    await settle();
    expect(t.transport.sentOf('identity.list')).toHaveLength(1);
    expect(model.items[0]?.state).toBe('needs_sign_in');
    expect(clean(html(SignIns, { backend: t.state, model }))).toContain('data-kind="gitlab"');
    expect(text(html(SignIns, { backend: t.state, model }))).toContain('Sign in again');
    stop();
  });

  it('CRED_PROVIDER_NOT_CONFIGURED for gitlab switches the row to the token box; the box is cleared after sending', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [credential('gitlab', 'needs_sign_in')] }));
      tr.on('identity.sign_in', err('CRED_PROVIDER_NOT_CONFIGURED', 'Not configured.'));
      tr.on('identity.save_gitlab_token', ok(credential('gitlab')));
    });
    const model = new SignInsModel(t.handle);
    await model.load();
    expect(await model.signIn('gitlab')).toBe('not_configured');
    const out = html(SignIns, { backend: t.state, model });
    expect(text(out)).toContain('Paste a GitLab access token');
    expect(out).toMatch(/type="password"/);
    model.token = 'glpat-secret';
    await model.saveGitlabToken();
    expect(t.transport.sentOf('identity.save_gitlab_token')[0]?.input).toEqual({ token: 'glpat-secret' });
    expect(model.token).toBe('');
    expect(model.tokenBox.has('gitlab')).toBe(false);
  });

  it('a browser sign-in waits for the challenge, then reads the list again', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [] }));
      tr.on('identity.sign_in', ok({ kind: 'browser_opened', challenge_id: uid('ch1'), expires_at: '2026-01-10T11:00:00Z' }));
      tr.on('identity.sign_in_wait', ok(credential('github')));
    });
    const model = new SignInsModel(t.handle);
    expect(await model.signIn('github')).toBe('ok');
    expect(t.transport.commandsSent()).toEqual(['identity.sign_in', 'identity.sign_in_wait', 'identity.list']);
    expect(t.transport.sentOf('identity.sign_in_wait')[0]?.input).toEqual({ challenge_id: uid('ch1') });
  });
});

describe('Settings › Jira', () => {
  const user = { account_id: 'a1', display_name: 'Sam Lee', email: 'sam.lee@acme.co.uk' };
  const connected = { id: uid('j1'), kind: 'jira', state: 'signed_in', account_label: 'Sam Lee', expires_at: null };

  it('Happy path — connecting Jira with an API token', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [] }));
      tr.on('jira.connect_token', ok(user));
    });
    const model = new JiraModel(t.handle);
    await model.load();
    model.site = 'acme.atlassian.net';
    model.email = 'sam.lee@acme.co.uk';
    model.token = 'secret-token';
    t.transport.on('identity.list', ok({ items: [connected] }));
    expect(await model.connect()).toBe(true);
    expect(t.transport.sentOf('jira.connect_token')[0]?.input).toEqual({ site: 'acme.atlassian.net', email: 'sam.lee@acme.co.uk', token: 'secret-token' });
    expect(model.token).toBe('');
    expect(text(html(Jira, { backend: t.state, model }))).toContain('Connected to acme.atlassian.net as Sam Lee');
  });

  it('Edge case — no company Atlassian sign-in', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [] }));
      tr.on('identity.sign_in', err('CRED_PROVIDER_NOT_CONFIGURED', 'No.'));
    });
    const model = new JiraModel(t.handle);
    model.site = 'acme.atlassian.net';
    await model.signInWithAtlassian();
    expect(t.transport.sentOf('identity.sign_in')[0]?.input).toEqual({ kind: 'jira', site: 'https://acme.atlassian.net' });
    const out = text(html(Jira, { backend: t.state, model }));
    expect(out).toContain("Your company hasn't set up Atlassian sign-in. Use an API token instead.");
    expect(out).not.toContain('Sign in with Atlassian…');
  });

  it('Error path — a token Atlassian refuses', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [] }));
      tr.on('jira.connect_token', err('JIRA_TOKEN_REFUSED', 'Jira did not accept that email address and token.'));
    });
    const model = new JiraModel(t.handle);
    model.site = 'acme.atlassian.net';
    model.email = 'sam@acme.co.uk';
    model.token = 'bad';
    expect(await model.connect()).toBe(false);
    expect(model.token).toBe('');
    const out = text(html(Jira, { backend: t.state, model }));
    expect(out).toContain('Jira did not accept that email address and token.');
    // The form is still there, so focus stays in it.
    expect(html(Jira, { backend: t.state, model })).toMatch(/type="password"/);
  });

  it('CRED_DUPLICATE is drawn as the other-site sentence', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [] }));
      tr.on('jira.connect_token', err('CRED_DUPLICATE', 'Already.'));
    });
    const model = new JiraModel(t.handle);
    model.site = 'other.atlassian.net';
    model.email = 'sam.lee@acme.co.uk';
    model.token = 't';
    await model.connect();
    expect(text(html(Jira, { backend: t.state, model }))).toContain('ordin is already connected to another Jira site. Disconnect it first.');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-JS-1
  it('Happy path — a pasted address is sent as its host (SM-JS-1)', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [] }));
      tr.on('jira.connect_token', ok(user));
    });
    const model = new JiraModel(t.handle);
    model.site = 'https://Acme.atlassian.net/jira/your-work?x=1';
    model.email = 'sam.lee@acme.co.uk';
    model.token = 'secret-token';
    expect(await model.connect()).toBe(true);
    expect(t.transport.sentOf('jira.connect_token')[0]?.input).toEqual({ site: 'acme.atlassian.net', email: 'sam.lee@acme.co.uk', token: 'secret-token' });
    expect(model.site).toBe('https://Acme.atlassian.net/jira/your-work?x=1');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-JS-1
  it('Edge case — Sign in with Atlassian uses the same host (SM-JS-1)', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [] }));
      tr.on('identity.sign_in', err('CRED_PROVIDER_NOT_CONFIGURED', 'No.'));
    });
    const model = new JiraModel(t.handle);
    model.site = 'https://acme.atlassian.net/';
    await model.signInWithAtlassian();
    expect(t.transport.sentOf('identity.sign_in')[0]?.input).toEqual({ kind: 'jira', site: 'https://acme.atlassian.net' });
  });

  it('jiraHost reads the forms a person types or pastes', () => {
    for (const [typed, host] of [
      ['acme.atlassian.net', 'acme.atlassian.net'],
      ['  acme.atlassian.net  ', 'acme.atlassian.net'],
      ['HTTPS://ACME.atlassian.net', 'acme.atlassian.net'],
      ['http://acme.atlassian.net/browse/ABC-1', 'acme.atlassian.net'],
      ['acme.atlassian.net.', 'acme.atlassian.net'],
      ['acme.atlassian.net#top', 'acme.atlassian.net'],
      ['https://', ''],
      ['', ''],
    ] as const) {
      expect(jiraHost(typed), typed).toBe(host);
    }
  });

  // spec: ordin-desktop/components/ScreensModule#SM-JS-2
  it('Error path — no site (SM-JS-2)', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [] }));
    });
    const model = new JiraModel(t.handle);
    model.site = '  ';
    model.email = 'sam.lee@acme.co.uk';
    model.token = 'secret-token';
    expect(await model.connect()).toBe(false);
    expect(t.transport.sentOf('jira.connect_token')).toEqual([]);
    expect(model.token).toBe('secret-token');
    expect(text(html(Jira, { backend: t.state, model }))).toContain('Type your Jira site, for example acme.atlassian.net.');
  });

  it('JIRA_UNAVAILABLE offers Try again, which re-sends the same input', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [] }));
      tr.on('jira.connect_token', err('JIRA_UNAVAILABLE', 'Jira did not answer.', { action: 'try_again' }));
    });
    const model = new JiraModel(t.handle);
    model.site = 'acme.atlassian.net';
    model.email = 'a@b.c';
    model.token = 'tok';
    await model.connect();
    expect(text(html(Jira, { backend: t.state, model }))).toContain('Try again');
    await model.connect(true);
    expect(t.transport.sentOf('jira.connect_token').map((e) => e.input)).toEqual([
      { site: 'acme.atlassian.net', email: 'a@b.c', token: 'tok' },
      { site: 'acme.atlassian.net', email: 'a@b.c', token: 'tok' },
    ]);
  });

  it('Disconnect Jira… asks first, then signs out', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('identity.list', ok({ items: [connected] }));
      tr.on('identity.sign_out', ok({}));
    });
    const model = new JiraModel(t.handle);
    await model.load();
    model.confirmDisconnect = true;
    expect(text(html(Jira, { backend: t.state, model }))).toContain('Disconnect Jira…');
    t.transport.on('identity.list', ok({ items: [] }));
    await model.disconnect();
    expect(t.transport.sentOf('identity.sign_out')[0]?.input).toEqual({ credential_id: uid('j1') });
    expect(model.connection).toBeNull();
  });
});

describe('Settings › Slack', () => {
  const status = (over: Record<string, unknown> = {}) => ({
    direct_messages_allowed: true, names_allowed: true, offered: true, own_app: false, person_name: 'Asha', posting_allowed: true, profile_app: true,
    rules_allowed: true, scope_needed: false, signed_in: true, workspace_name: 'Northwind', manifest: null, reason_not_offered: null, ...over,
  });
  const rule = { rule: { id: uid('r1'), channel_id: 'C1', channel_name: 'supplier-onboarding', created_at: '2026-01-10T10:00:00Z', daily_cap: 20, kind: 'handover_notices', scope: { kind: 'link', slack_link_id: uid('l1') }, state: 'on' }, sent_today: 1 };

  it('Edge case — Settings › Slack opens with its fixed sentence', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('slack.status', ok(status()));
      tr.on('slack.rules', ok([rule]));
    });
    const model = new SlackModel(t.handle);
    await model.load();
    const out = clean(html(Slack, { backend: t.state, model }));
    expect(text(out)).toContain('Every post is shown to you before it is posted, unless you set a rule for it.');
    expect(out.indexOf('Every post is shown')).toBeLessThan(out.indexOf('Signed in to'));
    expect(text(out)).toContain('Signed in to Northwind as Asha.');
    expect(text(out)).toContain('supplier-onboarding');
    expect(text(out)).toContain('Turn off');
    await model.turnOff(uid('r1'));
    expect(t.transport.sentOf('slack.turn_off_rule')[0]?.input).toEqual({ rule_id: uid('r1') });
  });

  it('a company that turned rules off says so instead of the list', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('slack.status', ok(status({ rules_allowed: false }))));
    const model = new SlackModel(t.handle);
    await model.load();
    expect(t.transport.sentOf('slack.rules')).toEqual([]);
    expect(text(html(Slack, { backend: t.state, model }))).toContain('Your company asks that every Slack post is shown to you first.');
  });

  it('not signed in: Sign in to Slack…, and Connect your own Slack app… when the profile names none', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('slack.status', ok(status({ signed_in: false, profile_app: false })));
      tr.on('slack.rules', ok([]));
      tr.on('slack.connect_own_app', ok(status({ own_app: true })));
    });
    const model = new SlackModel(t.handle);
    await model.load();
    const out = text(html(Slack, { backend: t.state, model }));
    expect(out).toContain('Sign in to Slack…');
    expect(out).toContain('Connect your own Slack app…');
    model.clientId = '123.456';
    expect(await model.connectOwnApp()).toBe(true);
    expect(t.transport.sentOf('slack.connect_own_app')[0]?.input).toEqual({ client_id: '123.456' });
  });

  it('an administrator-approval refusal says handovers still go by email', async () => {
    const t = await loaded(R1_GROUPS, (tr) => tr.on('slack.status', ok(status({ signed_in: false, offered: false, reason_not_offered: 'app_not_approved' }))));
    const model = new SlackModel(t.handle);
    await model.load();
    expect(text(html(Slack, { backend: t.state, model }))).toContain("Handovers still go by email.");
  });

  it('the more-permissions line is shown when the scope was needed', async () => {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('slack.status', ok(status({ scope_needed: true })));
      tr.on('slack.rules', ok([]));
    });
    const model = new SlackModel(t.handle);
    await model.load();
    expect(text(html(Slack, { backend: t.state, model }))).toContain('ordin needs two more Slack permissions');
  });
});

describe('Settings › Knowledge agents', () => {
  const connection = (over: Record<string, unknown> = {}) => ({
    connection_id: uid('k1'), name: 'Supplier knowledge', url: 'https://knowledge.example.org', added_by: 'person', last_answered_at: null,
    sources_record: { enabled: true, retention_days: 365 }, ...over,
  });
  const about = (over: Record<string, unknown> = {}) => ({ agent: 'Supplier knowledge', issuer: 'x', repository_count: 3, token_audience: 'y', sources_record: { enabled: true, retention_days: 365 }, as_of: null, accounts: [], ...over });

  async function model(setup: Parameters<typeof loaded>[1] = () => undefined, announce: string[] = []) {
    const t = await loaded(R1_GROUPS, (tr) => {
      tr.on('knowledge_agents.list', ok({ connections: [] }));
      setup(tr);
    });
    const m = new KnowledgeAgentsModel(t.handle, WS, (x) => announce.push(x), t.diagnostics);
    return { ...t, m };
  }

  it('Happy path — connecting a front door shows what it keeps', async () => {
    const said: string[] = [];
    const t = await model((tr) => tr.on('knowledge_agents.add', ok({ connection_id: uid('k1'), about: about() })), said);
    t.m.openConnect();
    t.m.address = 'https://knowledge.example.org';
    expect(await t.m.connect()).toBe(true);
    expect(t.transport.sentOf('knowledge_agents.add')[0]?.input).toEqual({ workspace_id: WS, url: 'https://knowledge.example.org' });
    const out = text(html(KnowledgeAgents, { backend: t.state, model: t.m }));
    expect(out).toContain('This knowledge agent keeps a record of which sources each answer used, for 365 days. The record never holds your question or your name.');
    t.m.keepIt();
    expect(said.map(stripIsolates)).toEqual(["Connected. Ordin will ask Supplier knowledge when its own memory isn't enough."]);
    expect(t.m.connectOpen).toBe(false);
  });

  it('the second stage Remove sends knowledge_agents.remove at once, with no further confirmation', async () => {
    const t = await model((tr) => {
      tr.on('knowledge_agents.add', ok({ connection_id: uid('k9'), about: about() }));
      tr.on('knowledge_agents.remove', ok({}));
    });
    t.m.openConnect();
    t.m.address = 'https://knowledge.example.org';
    await t.m.connect();
    await t.m.removeJustAdded();
    expect(t.transport.sentOf('knowledge_agents.remove')[0]?.input).toEqual({ connection_id: uid('k9') });
  });

  it('Edge case — a workspace with no front door', async () => {
    const t = await model();
    await t.m.load();
    expect(text(html(KnowledgeAgents, { backend: t.state, model: t.m }))).toContain("This workspace isn't connected to a knowledge agent. It works as it always has.");
  });

  it('UI_KNOWLEDGE_ADDRESS_NOT_HTTPS — Error path — an address that is not https', async () => {
    const t = await model();
    t.m.openConnect();
    t.m.address = 'http://knowledge.example.org';
    expect(await t.m.connect()).toBe(false);
    expect(t.transport.sentOf('knowledge_agents.add')).toEqual([]);
    expect(t.diagnostics.codes()).toContain('UI_KNOWLEDGE_ADDRESS_NOT_HTTPS');
    const out = text(html(KnowledgeAgents, { backend: t.state, model: t.m }));
    expect(out).toContain('Use an address that starts with https://.');
    expect(out).not.toContain('UI_KNOWLEDGE_ADDRESS_NOT_HTTPS');
  });

  it('Error path — the person is outside the agent’s audience', async () => {
    const t = await model((tr) => tr.on('knowledge_agents.add', err('KQ_NOT_IN_AUDIENCE', 'This knowledge agent is not open to your account.')));
    t.m.openConnect();
    t.m.address = 'https://knowledge.example.org';
    await t.m.connect();
    const out = text(html(KnowledgeAgents, { backend: t.state, model: t.m }));
    expect(out).toContain('This knowledge agent is not open to your account.');
    expect(t.m.address).toBe('https://knowledge.example.org');
    expect(out).not.toContain('Sign in again');
  });

  it('Happy path — removing a front door asks first', async () => {
    const said: string[] = [];
    const t = await model((tr) => {
      tr.on('knowledge_agents.list', ok({ connections: [connection({ added_by: 'company' })] }));
      tr.on('knowledge_agents.remove', ok({}));
    }, said);
    await t.m.load();
    const target = t.m.connections[0]!;
    expect(text(html(KnowledgeAgents, { backend: t.state, model: t.m }))).toContain('Set by your company');
    t.m.confirmRemove = target;
    const dialog = html(KnowledgeAgents, { backend: t.state, model: t.m });
    expect(text(dialog)).toContain("Stop asking Supplier knowledge? Ordin will answer from this workspace's own memory only.");
    expect(dialog).toMatch(/<button[^>]*data-default="true"[^>]*>\s*Cancel/);
    expect(t.transport.sentOf('knowledge_agents.remove')).toEqual([]);
    await t.m.remove(target);
    expect(t.transport.sentOf('knowledge_agents.remove')[0]?.input).toEqual({ connection_id: uid('k1') });
    expect(said.map(stripIsolates)).toEqual(["Removed. Ordin won't ask Supplier knowledge again."]);
  });

  it('a row says when it last answered, or that it has not been asked', async () => {
    const t = await model((tr) => tr.on('knowledge_agents.list', ok({ connections: [connection(), connection({ connection_id: uid('k2'), name: 'Finance knowledge', last_answered_at: '2026-01-10T10:00:00Z', sources_record: null })] })));
    await t.m.load();
    const out = text(html(KnowledgeAgents, { backend: t.state, model: t.m }));
    expect(out).toContain('Not asked yet');
    expect(out).toContain('Last answered');
  });

  it('Details: places, up to date, the record and the address; accounts per ADR-115', async () => {
    const t = await model((tr) =>
      tr.on('knowledge_agents.details', ok({
        connection: connection(),
        about: about({ as_of: '2026-01-10T10:00:00Z', accounts: [{ system: 'confluence', host: 'c' }, { system: 'github', host: 'g' }, { system: 'sharepoint', host: 's' }] }),
        accounts: [{ system: 'confluence', state: 'not_signed_in' }, { system: 'github', state: 'linked' }, { system: 'sharepoint', state: 'linked' }],
      })),
    );
    await t.m.openDetails(uid('k1'));
    const out = text(html(KnowledgeAgents, { backend: t.state, model: t.m }));
    expect(out).toContain('Reads 3 places');
    expect(out).toContain('Up to date as of');
    expect(out).toContain('https://knowledge.example.org');
    expect(out).toContain('Sign in to Confluence in ordin to include what you can read there.');
    expect(out).toContain('GitHub: your account is linked.');
    expect(out).not.toMatch(/[Ss]hare[Pp]oint/);
  });

  it('Error — a system the catalogue has no name for is not drawn', () => {
    expect(accountLine({ system: 'sharepoint', state: 'linked' })).toBeNull();
    expect(accountLine({ system: 'github', state: 'refused' })?.messageId).toBe('knowledge-agents-account-refused');
    expect(accountLine({ system: 'github', state: 'not_signed_in' })?.signIn).toBe(true);
    expect(accountLine({ system: 'github', state: 'later' } as never)).toBeNull();
  });

  it('a knowledge_agents_changed event for the chosen workspace re-reads the list', async () => {
    const t = await model();
    await t.m.load();
    t.transport.sent.length = 0;
    await t.state.apply(event(LOCAL, t.state.lastSeq + 1, { kind: 'knowledge_agents_changed', workspace_id: WS }));
    expect(t.state.workspaceRevision(WS)).toBe(1);
  });
});
