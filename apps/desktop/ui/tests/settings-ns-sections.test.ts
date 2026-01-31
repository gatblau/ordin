// SM-NS-1: the Settings section bodies (Agent repositories, Email, Telegram, Dictation, How Ordin talks to
// you, Using your plan, Notifications, Code, What Ordin remembers) and the section list of SM-CU-3.
import { beforeAll, describe, expect, it } from 'vitest';

import { err, hello, LOCAL, ok, R1_GROUPS } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { loaded, text } from './settings-support';
import { visibleSections } from '../src/lib/views/Settings/sections';
import Settings from '../src/lib/views/Settings/Settings.svelte';
import AgentRepositories from '../src/lib/views/Settings/AgentRepositories.svelte';
import CodeSection from '../src/lib/views/Settings/CodeSection.svelte';
import Dictation from '../src/lib/views/Settings/Dictation.svelte';
import Email from '../src/lib/views/Settings/Email.svelte';
import Notifications from '../src/lib/views/Settings/Notifications.svelte';
import Plan from '../src/lib/views/Settings/Plan.svelte';
import Telegram from '../src/lib/views/Settings/Telegram.svelte';
import Voice from '../src/lib/views/Settings/Voice.svelte';
import { CodeModel } from '../src/lib/views/Settings/code-model.svelte';
import { EmailModel } from '../src/lib/views/Settings/email-model.svelte';
import { NotificationsModel } from '../src/lib/views/Settings/notifications-model.svelte';
import { applyChange, PlanModel } from '../src/lib/views/Settings/plan-model.svelte';
import { ReposModel } from '../src/lib/views/Settings/repos-model.svelte';
import { TelegramModel } from '../src/lib/views/Settings/telegram-model.svelte';
import { VoiceModel } from '../src/lib/views/Settings/voice-model.svelte';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const GROUPS = [...R1_GROUPS, 'mail', 'telegram', 'voice', 'diagnostics', 'runs'];

describe('Settings sections (SM-CU-3)', () => {
  it('table: the sections that read their own commands are listed exactly when the backend answers them', () => {
    const rows: [string, string][] = [
      ['how-ordin-talks', 'voice'],
      ['code', 'projects'],
      ['what-ordin-remembers', 'memory'],
      ['using-your-plan', 'usage'],
      ['dictation', 'dictation'],
    ];
    for (const [id, group] of rows) {
      expect(visibleSections({ groups: new Set([group]), kind: 'local' }).map((s) => s.id), id).toContain(id);
      expect(visibleSections({ groups: new Set(R1_GROUPS.filter((g) => g !== group)), kind: 'local' }).map((s) => s.id), id).not.toContain(id);
    }
  });
});

describe('Settings › Agent repositories (SM-NS-1.1)', () => {
  const repo = (id: string, name: string, over: Record<string, unknown> = {}) => ({
    agent_count: 3,
    repository: { id, display_name: name, format: 'ordin', person_can_publish: true },
    set_by_company: false,
    ...over,
  });
  const probe = (over: Record<string, unknown> = {}) => ({
    probe_id: uid('p1'), agent_count: 4, format: 'apm', person_can_publish: false, suggested_name: 'Supplier agents', expires_at: '2999-01-01T00:00:00Z', ...over,
  });

  it('Happy path — a row per repository in the order given, with its format, count and actions', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('catalogue.list_repositories', ok([repo(uid('r1'), 'Team agents'), repo(uid('r2'), 'Company agents', { set_by_company: true, agent_count: 1 })])));
    const model = new ReposModel(t.handle);
    await model.load();
    const out = text(clean(html(AgentRepositories, { backend: t.state, model })));
    expect(out.indexOf('Team agents')).toBeLessThan(out.indexOf('Company agents'));
    expect(out).toContain('ordin format');
    expect(out).toContain('3 agents');
    expect(out).toContain('1 agent');
    expect(out).toContain("Can't be removed");
    expect(out).not.toContain('Set by your company');
    expect(out).toContain('Add an agent repository…');
    // Rename… and Remove… only on the row that is not set by the company.
    expect(out.match(/Rename…/g)).toHaveLength(1);
    expect(out.match(/Remove…/g)).toHaveLength(1);
  });

  it('Rename sends the new name (1–80 characters) and reads the list again; an empty name is not sent', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('catalogue.list_repositories', ok([repo(uid('r1'), 'Team agents')])).on('catalogue.rename_repository', ok({})));
    const model = new ReposModel(t.handle);
    await model.load();
    model.startRename(model.items[0]!);
    model.renameText = '   ';
    expect(await model.rename()).toBe(false);
    expect(t.transport.sentOf('catalogue.rename_repository')).toEqual([]);
    model.renameText = 'Platform agents';
    expect(await model.rename()).toBe(true);
    expect(t.transport.sentOf('catalogue.rename_repository')[0]?.input).toEqual({ id: uid('r1'), display_name: 'Platform agents' });
  });

  it('Remove sends the id after the confirmation', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('catalogue.list_repositories', ok([repo(uid('r1'), 'Team agents')])).on('catalogue.remove_repository', ok({})));
    const model = new ReposModel(t.handle);
    await model.load();
    model.removing = model.items[0]!;
    await model.remove();
    expect(t.transport.sentOf('catalogue.remove_repository')[0]?.input).toEqual({ id: uid('r1') });
  });

  it('Look it up presets the name and says what was found; Add sends the probe id and the name', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('catalogue.list_repositories', ok([])).on('catalogue.probe_repository', ok(probe())).on('catalogue.add_repository', ok({})),
    );
    const model = new ReposModel(t.handle);
    model.openAdd();
    model.address = 'https://github.com/acme/agents';
    expect(await model.lookUp()).toBe(true);
    expect(t.transport.sentOf('catalogue.probe_repository')[0]?.input).toEqual({ web_address: 'https://github.com/acme/agents' });
    expect(model.name).toBe('Supplier agents');
    const out = text(clean(html(AgentRepositories, { backend: t.state, model })));
    expect(out).toContain('4 agents found · APM format');
    expect(out).toContain("You can add agents from it, but you can't publish to it.");
    expect(await model.add()).toBe(true);
    expect(t.transport.sentOf('catalogue.add_repository')[0]?.input).toEqual({ probe_id: uid('p1'), display_name: 'Supplier agents' });
    expect(model.adding).toBe(false);
  });

  it('Edge case — a probe past its expires_at is looked up again on Add, keeping the typed name', async () => {
    let n = 0;
    const t = await loaded(GROUPS, (tr) =>
      tr
        .on('catalogue.list_repositories', ok([]))
        .on('catalogue.probe_repository', () => ok(probe({ probe_id: uid(`p${++n}`), expires_at: n === 1 ? '2000-01-01T00:00:00Z' : '2999-01-01T00:00:00Z' })))
        .on('catalogue.add_repository', ok({})),
    );
    const model = new ReposModel(t.handle);
    model.openAdd();
    model.address = 'https://github.com/acme/agents';
    await model.lookUp();
    model.name = 'My name';
    await model.add();
    expect(t.transport.sentOf('catalogue.probe_repository')).toHaveLength(2);
    expect(t.transport.sentOf('catalogue.add_repository')[0]?.input).toEqual({ probe_id: uid('p2'), display_name: 'My name' });
  });

  it('Error path — an address over 2,048 bytes is not looked up; a refusal stays in the dialog with the text kept', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('catalogue.probe_repository', err('CAT_NOT_FOUND', 'No agent repository is there.')));
    const model = new ReposModel(t.handle);
    model.openAdd();
    model.address = 'https://x.example/' + 'a'.repeat(2100);
    expect(await model.lookUp()).toBe(false);
    expect(t.transport.sentOf('catalogue.probe_repository')).toEqual([]);
    model.address = 'https://x.example/a';
    expect(await model.lookUp()).toBe(false);
    expect(model.address).toBe('https://x.example/a');
    expect(text(html(AgentRepositories, { backend: t.state, model }))).toContain('No agent repository is there.');
  });
});

describe('Settings › Email (SM-NS-1.2)', () => {
  const settings = (over: Record<string, unknown> = {}) => ({ add_signature: false, agent_foot_line: false, agent_foot_line_locked: false, check: 'every15_minutes', signature: '', ...over });
  const mailbox = (kind = 'mail_server_password') => ({ id: uid('c1'), kind, state: 'signed_in', account_label: 'you@company.com', expires_at: null });
  const setup = (over: Record<string, unknown> = {}, kind = 'mail_server_password') => (tr: Parameters<typeof loaded>[1] extends ((t: infer T) => void) | undefined ? T : never) =>
    tr
      .on('mail.settings', ok(settings(over)))
      .on('identity.list', ok({ items: [mailbox(kind)] }))
      .on('mail.verify_sign_in', ok('signed_in'))
      .on('workspaces.list', ok([{ id: uid('w1'), name: 'Suppliers' }, { id: uid('w2'), name: 'Empty' }]))
      .on('workspaces.email_links', (i) => ok((i as { workspace_id: string }).workspace_id === uid('w1') ? [{ id: uid('e1'), display_name: 'Orders inbox' }] : []));

  it('Happy path — Settings › Email shows the mailbox (SM-NS-1.2)', async () => {
    const t = await loaded(GROUPS, setup());
    const model = new EmailModel(t.handle);
    await model.load();
    const out = text(clean(html(Email, { backend: t.state, model })));
    expect(out).toContain("Every email is shown to you before it is sent. This can't be turned off.");
    expect(out).toContain('Your mailbox');
    expect(out).toContain('you@company.com');
    expect(out).toContain("Signed in with your password, kept in this computer's keychain");
    expect(out).toContain('Change password');
    expect(out).toContain('Sign out of email');
    // Email in your workspaces: a workspace with no link is not listed.
    expect(out).toContain('Suppliers');
    expect(out).toContain('Orders inbox');
    expect(out).not.toContain('Empty');
  });

  it('a company-account kind says it is signed in with the account and offers no Change password', async () => {
    const t = await loaded(GROUPS, setup({}, 'microsoft'));
    const model = new EmailModel(t.handle);
    await model.load();
    const out = text(clean(html(Email, { backend: t.state, model })));
    expect(out).toContain('Signed in with your company account');
    expect(out).not.toContain('Change password');
  });

  it('each change sends the whole MailSettingsUpdate', async () => {
    const t = await loaded(GROUPS, (tr) => {
      setup({ signature: 'Sam' })(tr);
      tr.on('mail.set_settings', (i) => ok(settings({ signature: 'Sam', ...(i as object) })));
    });
    const model = new EmailModel(t.handle);
    await model.load();
    expect(await model.setCheck('only_on_workspace_open')).toBe(true);
    expect(t.transport.sentOf('mail.set_settings')[0]?.input).toEqual({ add_signature: false, agent_foot_line: false, check: 'only_on_workspace_open', signature: 'Sam' });
    model.signature = 'Sam Lee';
    await model.saveSignature();
    expect(t.transport.sentOf('mail.set_settings')[1]?.input).toMatchObject({ signature: 'Sam Lee', check: 'only_on_workspace_open' });
  });

  it('Error path — a refused change returns the control to the stored value and draws the sentence', async () => {
    const t = await loaded(GROUPS, (tr) => {
      setup()(tr);
      tr.on('mail.set_settings', err('MAIL_SETTINGS_INVALID', 'The signature is too long.'));
    });
    const model = new EmailModel(t.handle);
    await model.load();
    expect(await model.setAddSignature(true)).toBe(false);
    expect(model.settings?.add_signature).toBe(false);
    expect(text(html(Email, { backend: t.state, model }))).toContain('The signature is too long.');
  });

  it('the footer line is on and disabled, with its reason, when the company requires it', async () => {
    const t = await loaded(GROUPS, setup({ agent_foot_line_locked: true }));
    const model = new EmailModel(t.handle);
    await model.load();
    const markup = clean(html(Email, { backend: t.state, model }));
    expect(text(markup)).toContain('Required by your company');
    expect(markup).toMatch(/<input[^>]*type="checkbox"[^>]*checked[^>]*disabled/);
  });

  it('a refused or insecure check draws its sentence; Sign out of email sends the credential id at once', async () => {
    const t = await loaded(GROUPS, (tr) => {
      setup()(tr);
      tr.on('mail.verify_sign_in', ok('refused')).on('identity.sign_out', ok({}));
    });
    const model = new EmailModel(t.handle);
    await model.load();
    expect(text(html(Email, { backend: t.state, model }))).toContain("Your mail server didn't accept this password. Check it and try again.");
    t.transport.on('mail.verify_sign_in', ok('not_reachable_securely'));
    await model.load();
    expect(text(html(Email, { backend: t.state, model }))).toContain("Your company's mail server can't be reached securely. Ask IT.");
    await model.signOut();
    expect(t.transport.sentOf('identity.sign_out')[0]?.input).toEqual({ credential_id: uid('c1') });
  });
});

describe('Settings › Telegram (SM-NS-1.3)', () => {
  it('table: a section not offered says why and nothing more', async () => {
    const rows: [string, string][] = [
      ['not_allowed_by_company', "Your company doesn't allow Telegram here."],
      ['tools_turned_off', "Telegram isn't available here."],
      ['app_not_on_computer', "Telegram isn't on this computer. Install it to link Telegram groups here."],
      ['no_bot', 'No Telegram bot is set up for ordin.'],
    ];
    for (const [reason, sentence] of rows) {
      const t = await loaded(GROUPS, (tr) => tr.on('telegram.status', ok({ offered: false, connected: false, reason_not_offered: reason })));
      const model = new TelegramModel(t.handle);
      await model.load();
      const out = text(clean(html(Telegram, { backend: t.state, model })));
      expect(out, reason).toContain(sentence);
      expect(out, reason).not.toContain('Set up Telegram');
    }
  });

  it('Connect sends the key and clears the field; the reply draws the bot with the administrator note', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('telegram.status', ok({ offered: true, connected: false })).on('telegram.connect_bot', ok({ display_name: 'Ordin bot', username: 'ordin_bot', can_read_all_group_messages: false })),
    );
    const model = new TelegramModel(t.handle);
    await model.load();
    expect(text(clean(html(Telegram, { backend: t.state, model })))).toContain('Set up Telegram');
    expect(html(Telegram, { backend: t.state, model })).toMatch(/type="password"/);
    model.key = '123:secret';
    await model.connect();
    expect(t.transport.sentOf('telegram.connect_bot')[0]?.input).toEqual({ key: '123:secret' });
    expect(model.key).toBe('');
    const out = text(clean(html(Telegram, { backend: t.state, model })));
    expect(out).toContain('Your bot: Ordin bot (@ordin_bot)');
    expect(out).toContain("Your bot sees only messages addressed to it in groups where it isn't an administrator.");
    expect(out).toContain('Remove Telegram');
  });

  it('Remove Telegram sends telegram.remove at once', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('telegram.status', ok({ offered: true, connected: true, bot: { display_name: 'B', username: 'b', can_read_all_group_messages: true } })).on('telegram.remove', ok({})),
    );
    const model = new TelegramModel(t.handle);
    await model.load();
    await model.remove();
    expect(t.transport.sentOf('telegram.remove')[0]?.input).toEqual({});
  });
});

describe('Settings › Dictation (SM-NS-1.4)', () => {
  it('table: available names the language, needs_assets says it gets ready, unavailable shows its message; no control', async () => {
    const t = await loaded(GROUPS);
    const rows: [object, string][] = [
      [{ state: 'available', locale: 'en-GB' }, 'Dictation is ready in'],
      [{ state: 'needs_assets', locale: 'en-GB' }, 'Dictation gets ready the first time you use it.'],
      [{ state: 'unavailable', reason: 'os_too_old', message: 'Dictation needs a newer macOS.' }, 'Dictation needs a newer macOS.'],
    ];
    for (const [initial, sentence] of rows) {
      const markup = clean(html(Dictation, { backend: t.state, initial }));
      expect(text(markup)).toContain(sentence);
      expect(markup).not.toMatch(/<(button|input|select)/);
    }
    expect(text(html(Dictation, { backend: t.state, initial: { state: 'available', locale: 'en-GB' } }))).toContain('English');
  });
});

describe('Settings › How Ordin talks to you (SM-NS-1.5)', () => {
  const voice = (over: Record<string, unknown> = {}) => ({
    tone: { set_by: 'person', value: 'friendly' },
    register: { set_by: 'person', value: 'everyday' },
    length: { set_by: 'company', value: 'brief' },
    name_to_use: { set_by: 'person', value: null },
    notes: { set_by: 'person', value: null },
    updated_at: '2026-01-10T10:00:00Z',
    ...over,
  });

  it('Happy path — draws the fixed line, the choices and the company default beside the company choice', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('voice.get', ok(voice())));
    const model = new VoiceModel(t.handle);
    await model.load();
    const markup = clean(html(Voice, { backend: t.state, model }));
    const out = text(markup);
    expect(out).toContain('Ordin always uses plain words and keeps it short. Choose how it sounds.');
    for (const word of ['Tone', 'Friendly', 'Neutral', 'Warm', 'Direct', 'Formality', 'Everyday', 'Formal', 'Length', 'Brief', 'Shortest', 'Call me', 'Notes for Ordin']) expect(out).toContain(word);
    expect(out.match(/Your company's default/g)).toHaveLength(1);
    expect(markup).toMatch(/value="friendly" checked/);
  });

  it('each change sends voice.update with the one field changed; Call me empty sends ""', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('voice.get', ok(voice({ name_to_use: { set_by: 'person', value: 'Sam' } }))).on('voice.update', ok(voice({ name_to_use: { set_by: 'person', value: 'Sam' } }))),
    );
    const model = new VoiceModel(t.handle);
    await model.load();
    await model.update({ tone: 'warm' });
    expect(t.transport.sentOf('voice.update')[0]?.input).toEqual({ tone: 'warm' });
    model.nameToUse = '';
    await model.saveName();
    expect(t.transport.sentOf('voice.update')[1]?.input).toEqual({ name_to_use: '' });
  });

  it('Error path — a refusal returns the choice to the stored value', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('voice.get', ok(voice())).on('voice.update', err('VOICE_INVALID', 'That choice is not available.')));
    const model = new VoiceModel(t.handle);
    await model.load();
    expect(await model.update({ tone: 'direct' })).toBe(false);
    expect(model.settings?.tone.value).toBe('friendly');
    expect(text(html(Voice, { backend: t.state, model }))).toContain('That choice is not available.');
  });
});

describe('Settings › Using your plan (SM-NS-1.6)', () => {
  const sv = <T>(value: T, set_by = 'person') => ({ set_by, value });
  const usage = (over: Record<string, unknown> = {}) => ({
    agents_at_same_time: sv(3),
    answer_simple_requests_without_model: sv(true),
    ask_before_estimated_requests: sv('heavy'),
    choice: sv('balanced'),
    continue_from_summary_after_break: sv(false),
    daily_pause_requests: { set_by: 'person' },
    give_agents_only_needed_parts: sv(true),
    model_level_per_kind: sv({}),
    reuse_answers: sv(true),
    show_usage_on_each_run: sv(false),
    summarise_long_conversations: sv('when_nearly_full'),
    thinking: sv('automatic'),
    updated_at: '2026-01-10T10:00:00Z',
    ...over,
  });

  it('Happy path — Using your plan shows today (SM-NS-1.6)', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('usage.today', ok({ typical_day_permille: 200, model_turns: 12, smaller_share_permille: 450, answered_without_model: 5 })).on('usage.get_settings', ok(usage())),
    );
    const model = new PlanModel(t.handle);
    await model.load();
    const out = text(clean(html(Plan, { backend: t.state, model })));
    expect(out).toContain("Today you've used about 20% of a typical day's plan.");
    expect(out).toContain('12 model turns · 45% on smaller models · 5 answered by ordin without the model');
    for (const word of ['How Ordin uses your plan', 'Save my plan', 'Balanced', 'Best results', 'Custom', 'Summarise long conversations', 'Ask before work estimated as', 'Agents working at the same time', 'More choices']) expect(out).toContain(word);
  });

  it('a change sends usage.update_settings with the one field; custom is drawn but never chosen', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('usage.today', ok({ typical_day_permille: 0, model_turns: 0, smaller_share_permille: 0, answered_without_model: 0 })).on('usage.get_settings', ok(usage({ choice: sv('custom') }))).on('usage.update_settings', ok(usage())),
    );
    const model = new PlanModel(t.handle);
    await model.load();
    await model.update({ choice: 'best_results' });
    expect(t.transport.sentOf('usage.update_settings')[0]?.input).toEqual({ choice: 'best_results' });
    expect(applyChange(usage() as never, { reuse_answers: false }).reuse_answers.value).toBe(false);
    const markup = clean(html(Plan, { backend: t.state, model: Object.assign(new PlanModel(t.handle), { settings: usage({ choice: sv('custom') }), today: null }) }));
    expect(markup).toMatch(/<input[^>]*value="custom"[^>]*checked[^>]*disabled/);
  });

  it('the agents count steps by one and stays at 1 or more; a refusal returns it', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('usage.today', ok({})).on('usage.get_settings', ok(usage({ agents_at_same_time: sv(1) }))).on('usage.update_settings', err('USAGE_INVALID', 'Not that many.')));
    const model = new PlanModel(t.handle);
    await model.load();
    await model.stepAgents(-1);
    expect(t.transport.sentOf('usage.update_settings')).toEqual([]);
    expect(await model.stepAgents(1)).toBe(false);
    expect(t.transport.sentOf('usage.update_settings')[0]?.input).toEqual({ agents_at_same_time: 2 });
    expect(model.settings?.agents_at_same_time.value).toBe(1);
  });
});

describe('Settings › Notifications (SM-NS-1.7)', () => {
  const view = (key: string, value: boolean, over: Record<string, unknown> = {}) => ({ key, value, set_by: 'person', person_may_change: true, ...over });
  const all = [
    view('notifications.handovers', true, { person_may_change: false }),
    view('notifications.approvals', true),
    view('notifications.other', true),
    view('notifications.questions', false),
    view('notifications.finished_work', true),
    view('notifications.draft_reviews', false),
  ];

  it('Edge case — a notification choice set by the company (SM-NS-1.7)', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('settings.get', ok({ section: 'notifications', settings: all })));
    const model = new NotificationsModel(t.handle);
    await model.load();
    expect(t.transport.sentOf('settings.get')[0]?.input).toEqual({ section: 'notifications' });
    const markup = clean(html(Notifications, { backend: t.state, model }));
    const out = text(markup);
    expect(out).toContain('ordin tells you outside its window only about these.');
    // Order is the spec's; a key outside the five is not drawn.
    const order = ['Approvals waiting', 'Finished tasks', 'Questions that stop', 'Reviews of your drafts', 'Handovers to you'].map((w) => out.indexOf(w));
    expect(order).toEqual([...order].sort((a, b) => a - b));
    expect(order.every((i) => i >= 0)).toBe(true);
    expect(markup).not.toContain('notifications.other');
    expect(markup).toMatch(/data-key="notifications.handovers"[\s\S]*?<input[^>]*disabled/);
    expect(out).toContain('Set by your company');
  });

  it('a change sends settings.change with a boolean', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('settings.get', ok({ section: 'notifications', settings: all })).on('settings.change', (i) => ok(view((i as { key: string }).key, (i as { value: boolean }).value))),
    );
    const model = new NotificationsModel(t.handle);
    await model.load();
    expect(await model.change('notifications.questions', true)).toBe(true);
    expect(t.transport.sentOf('settings.change')[0]?.input).toEqual({ key: 'notifications.questions', value: true });
    expect(model.isOn('notifications.questions')).toBe(true);
    // A locked switch is never sent.
    expect(await model.change('notifications.handovers', false)).toBe(false);
    expect(t.transport.sentOf('settings.change')).toHaveLength(1);
  });

  it('Error path — DSK_PREFERENCES_WRITE_FAILED returns the switch and draws its sentence with Try again', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('settings.get', ok({ section: 'notifications', settings: all })).on('settings.change', err('DSK_PREFERENCES_WRITE_FAILED', 'ordin could not save this choice.')),
    );
    const model = new NotificationsModel(t.handle);
    await model.load();
    expect(await model.change('notifications.questions', true)).toBe(false);
    expect(model.isOn('notifications.questions')).toBe(false);
    const out = text(clean(html(Notifications, { backend: t.state, model })));
    expect(out).toContain('ordin could not save this choice.');
    expect(out).toContain('Try again');
  });
});

describe('Settings › Code (SM-NS-1.8)', () => {
  const plugin = (over: Record<string, unknown> = {}) => ({
    id: 'p1', language: 'rust', enabled: true, default_server: 'rust-analyzer', chosen_server: null, approved_servers: ['rust-analyzer'],
    pinned_version: null, source: 'agent_package', status: 'running', server_settings: { a: 1 }, ...over,
  });

  it('Happy path — IDE switches, and a row per language with its server, source and Restart', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('projects.ides', ok([{ id: 'i1', name: 'VS Code', enabled: true }])).on('projects.language_plugins', ok([plugin({ pinned_version: '1.2', source: 'organisation_profile', approved_servers: ['rust-analyzer', 'other'] })])),
    );
    const model = new CodeModel(t.handle);
    await model.load();
    const markup = clean(html(CodeSection, { backend: t.state, model }));
    const out = text(markup);
    expect(out).toContain('Open code in');
    expect(out).toContain('VS Code');
    expect(out).toContain('Code checking by language');
    expect(out).toContain('rust-analyzer');
    expect(out).toContain('version 1.2');
    expect(out).toContain('From your company');
    expect(out).toContain('Restart');
    expect(markup).toContain('<select');
  });

  it('Edge case — no IDE found says so', async () => {
    const t = await loaded(GROUPS, (tr) => tr.on('projects.ides', ok([])).on('projects.language_plugins', ok([])));
    const model = new CodeModel(t.handle);
    await model.load();
    expect(text(html(CodeSection, { backend: t.state, model }))).toContain('No IDE was found on this computer.');
  });

  it('switches send their commands; the language switch sends the row\'s chosen_server and server_settings unchanged', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr
        .on('projects.ides', ok([{ id: 'i1', name: 'VS Code', enabled: true }]))
        .on('projects.language_plugins', ok([plugin({ chosen_server: 'x' })]))
        .on('projects.set_ide_enabled', ok({}))
        .on('projects.configure_language', ok(plugin({ enabled: false, chosen_server: 'x' })))
        .on('projects.restart_language_server', ok({})),
    );
    const model = new CodeModel(t.handle);
    await model.load();
    await model.setIde(model.ides[0]!, false);
    expect(t.transport.sentOf('projects.set_ide_enabled')[0]?.input).toEqual({ ide_id: 'i1', enabled: false });
    await model.configure(model.plugins[0]!, { enabled: false });
    expect(t.transport.sentOf('projects.configure_language')[0]?.input).toEqual({ language: 'rust', enabled: false, chosen_server: 'x', server_settings: { a: 1 } });
    await model.restart(model.plugins[0]!);
    expect(t.transport.sentOf('projects.restart_language_server')[0]?.input).toEqual({ language: 'rust' });
  });

  it('Error path — a refused IDE switch returns to the stored value', async () => {
    const t = await loaded(GROUPS, (tr) =>
      tr.on('projects.ides', ok([{ id: 'i1', name: 'VS Code', enabled: true }])).on('projects.language_plugins', ok([])).on('projects.set_ide_enabled', err('PROJ_IDE_UNKNOWN', 'That editor is not installed.')),
    );
    const model = new CodeModel(t.handle);
    await model.load();
    expect(await model.setIde(model.ides[0]!, false)).toBe(false);
    expect(model.ides[0]?.enabled).toBe(true);
  });
});

describe('Settings › What Ordin remembers (SM-NS-1.9)', () => {
  it('Edge case — with no workspace in front it says so and draws nothing else', async () => {
    const { app } = await appWith((tr) => tr.on('service.hello', ok(hello(LOCAL, GROUPS))));
    const out = text(clean(html(Settings, { backend: app.local, workspaceId: null, instance: null, section: 'what-ordin-remembers' }, app)));
    expect(out).toContain('Open a workspace to see what Ordin remembers in it.');
    expect(out).not.toContain('In the workspace');
  });
});
