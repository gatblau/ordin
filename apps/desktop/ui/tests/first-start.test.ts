// First start (ScreensModule.md, amendment January 2026 "First start, Add to this workspace and removing
// a link", SM-FS-1 to SM-FS-5): one test per first-start scenario and per added setup error row.
import { beforeAll, describe, expect, it } from 'vitest';

import App from '../src/App.svelte';
import type { CheckResult, ProgressStep, SetupProgress, SetupReport } from '../src/lib/service-api/types.gen';
import FirstStart from '../src/lib/views/setup/FirstStart.svelte';
import { harnessNameId, resultMessageId, stateMessageId, stepMessageId } from '../src/lib/views/setup/first-start-model.svelte';
import { EN_GB_FTL, err, event, type FakeTransport, LOCAL, ok, settle } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { text } from './settings-support';

beforeAll(loadWindowCatalogue);

const step = (check: ProgressStep['check'], state: ProgressStep['state']): ProgressStep => ({ check, state, message_id: stepMessageId(check) });
const progress = (steps: ProgressStep[]): SetupProgress => ({
  done: steps.filter((s) => s.state === 'done').length,
  total: steps.length,
  running: true,
  steps,
});
const attachment = (state = 'needs_sign_in', harness = 'claude_code') => ({
  harness, id: 'h1', last_checked_at: '2026-01-28T09:00:00.000Z', location: '/usr/local/bin/claude', signed_in: state === 'ready', state, tested: true, version: '2.1.0',
});
const report = (ready: boolean, results: Partial<CheckResult>[] = []): SetupReport => ({
  started_at: '2026-01-28T09:00:00.000Z',
  ended_at: '2026-01-28T09:01:00.000Z',
  ready,
  results: results.map((r) => ({ check: 'harness_sign_in', checked_at: '2026-01-28T09:01:00.000Z', message_args: {}, outcome: 'failed', ...r }) as CheckResult),
});
const failed = (code: string, action: CheckResult['action'], check: CheckResult['check'] = 'harness_sign_in'): Partial<CheckResult> => ({ check, code, message_id: resultMessageId(code), action });

const SIGNED_OUT = [step('company_settings', 'done'), step('harness', 'done'), step('harness_sign_in', 'waiting'), step('agent_repositories', 'waiting')];

/** A first start: `setup.status` answers `needs_guided: true`; `run_guided` waits until the test answers it. */
function firstStart(t: FakeTransport, steps = SIGNED_OUT, state = 'needs_sign_in'): { answer: (outcome: ReturnType<typeof ok> | ReturnType<typeof err>) => void } {
  let resolve: (o: unknown) => void = () => undefined;
  t.on('setup.status', ok({ needs_guided: true, progress: progress(steps), attachments: [attachment(state)] }));
  t.on('setup.run_guided', () => new Promise((r) => (resolve = r as never)) as never);
  return { answer: (o) => resolve(o) };
}

const draw = (app: Awaited<ReturnType<typeof appWith>>['app']) => text(html(App, { app }, app));

describe('the catalogue (SM-FS-5)', () => {
  it('every step kind and every result code the guided run gives has its words in en-GB', () => {
    // `second_install` comes from the desktop's start checks (SM-FS-8).
    const kinds = ['company_settings', 'harness', 'harness_sign_in', 'tokens', 'agent_repositories', 'agent_folders', 'programs', 'craft_apps', 'second_install', 'harness_choice'] as const;
    const codes = [
      'SETUP_NO_PROFILE_ADDRESS', 'SETUP_PROFILE_UNREACHABLE', 'SETUP_HARNESS_MISSING', 'SETUP_HARNESS_UNTESTED', 'SETUP_HARNESS_CHECK_FAILED',
      'SETUP_NO_HARNESS_ACCESS', 'SETUP_SIGN_IN_TIMEOUT', 'SETUP_SIGN_IN_ON_SERVER', 'SETUP_NO_READY_HARNESS', 'SETUP_CATALOGUE_FAILED',
      'SETUP_AGENT_FOLDER_FAILED', 'SETUP_PROGRAM_REFUSED', 'SETUP_PROGRAM_DAMAGED', 'SETUP_PROGRAM_FAILED', 'SETUP_CRAFT_APP_FAILED',
      'SETUP_SECOND_INSTALL', 'SETUP_NO_CHOICE_PENDING', 'SETUP_HARNESS_NOT_OFFERED', 'SETUP_CHOICE_NOT_SAVED',
    ];
    const ids = [...kinds.map(stepMessageId), ...codes.map(resultMessageId), ...(['waiting', 'running', 'done', 'needs_you', 'failed'] as const).map(stateMessageId)];
    for (const id of ids) expect(EN_GB_FTL, id).toMatch(new RegExp(`^${id} = `, 'm'));
    expect(harnessNameId('claude_code')).toBe('harness-claude-code');
    expect(harnessNameId('copilot_cli')).toBe('harness-copilot');
  });
});

describe('First start (SM-FS-1 to SM-FS-4)', () => {
  it('Happy path — a first start signs in and reaches Home (SM-FS-1 to SM-FS-4)', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app, transport } = await appWith((t) => {
      guided = firstStart(t);
      t.on('setup.sign_in_harness', ok(attachment('ready')));
    });
    // setup.status is sent at start-up, after the catalogue and before anything is drawn.
    const sent = transport.commandsSent();
    expect(sent).toContain('setup.status');
    expect(sent.indexOf('localisation.get_catalogue')).toBeLessThan(sent.indexOf('setup.status'));
    expect(app.firstStart.phase).toBe('first_start');

    const out = draw(app);
    expect(out).toContain('Getting you ready…');
    expect(out).toContain('2 of 4 done');
    expect(out).toContain('Signing you in to Claude Code');
    expect(out).not.toContain('Workspaces');
    expect(html(App, { app }, app)).not.toMatch(/data-region="(tree|screen|conversation)"/);

    app.firstStart.begin();
    app.firstStart.begin();
    await settle();
    expect(transport.sentOf('setup.run_guided')).toHaveLength(1);
    expect(transport.sentOf('setup.run_guided')[0]?.input).toEqual({});

    expect(draw(app)).not.toContain('Sign in to Claude Code');
    transport.emit(event(LOCAL, 1, { kind: 'setup_progress', progress: progress([step('company_settings', 'done'), step('harness', 'done'), step('harness_sign_in', 'needs_you'), step('agent_repositories', 'waiting')]) } as never));
    await settle();
    const asking = draw(app);
    expect(asking).toContain('Sign in to Claude Code');
    expect(asking).toContain('Opens Claude Code\'s sign-in page in your browser. This is the only sign-in needed now.');
    expect(asking).toContain('Needs you:');

    await app.firstStart.signIn();
    expect(transport.sentOf('setup.sign_in_harness')[0]?.input).toEqual({ harness: 'claude_code' });

    guided.answer(ok(report(true)));
    await settle();
    expect(app.firstStart.phase).toBe('regions');
    const regions = html(App, { app }, app);
    expect(regions).toContain('data-region="tree"');
    expect(regions).toContain('data-region="screen"');
    expect(regions).not.toContain('Getting you ready…');
  });

  it('the sign-in button is disabled with its waiting words until the answer (SM-FS-3)', async () => {
    let answer: (o: unknown) => void = () => undefined;
    const { app } = await appWith((t) => {
      firstStart(t, [step('harness_sign_in', 'needs_you')]);
      t.on('setup.sign_in_harness', () => new Promise((r) => (answer = r as never)) as never);
    });
    const pending = app.firstStart.signIn();
    const markup = html(FirstStart, { model: app.firstStart }, app);
    expect(text(markup)).toContain('Waiting for you to sign in in your browser…');
    expect(markup).toMatch(/<button[^>]*class="primary[^"]*"[^>]*disabled/);
    answer(ok(attachment('ready')));
    await pending;
    expect(app.firstStart.signingIn).toBe(false);
  });

  it('Error path — a failed sign-in is drawn under the button and the button is enabled again (SM-FS-3)', async () => {
    const { app } = await appWith((t) => {
      firstStart(t, [step('harness_sign_in', 'needs_you')]);
      t.on('setup.sign_in_harness', err('SETUP_SIGN_IN_TIMEOUT', 'The sign-in wasn\'t finished in time.', { action: 'try_again' }));
    });
    await app.firstStart.signIn();
    const markup = html(FirstStart, { model: app.firstStart }, app);
    expect(text(markup)).toContain('The sign-in wasn\'t finished in time.');
    expect(markup).not.toMatch(/<button[^>]*class="primary[^"]*"[^>]*disabled/);
  });

  it('Edge case — already signed in through Claude Code, nothing is asked (scene setupReady)', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const steps = [step('company_settings', 'done'), step('harness', 'done'), step('harness_sign_in', 'running'), step('agent_repositories', 'waiting')];
    const { app, transport } = await appWith((t) => {
      guided = firstStart(t, steps, 'ready');
    });
    app.firstStart.begin();
    transport.emit(event(LOCAL, 1, { kind: 'setup_progress', progress: progress([step('company_settings', 'done'), step('harness', 'done'), step('harness_sign_in', 'done'), step('agent_repositories', 'running')]) } as never));
    await settle();
    expect(draw(app)).not.toContain('Sign in to');
    guided.answer(ok(report(true)));
    await settle();
    expect(app.firstStart.phase).toBe('regions');
    expect(transport.sentOf('setup.sign_in_harness')).toHaveLength(0);
  });

  it('SM-FS-7 — a failed step says why while the run goes on, and only once after it ends', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app, transport } = await appWith((t) => {
      guided = firstStart(t);
    });
    app.firstStart.begin();
    const failedStep = { ...step('company_settings', 'failed'), problem: { message_id: 'setup-no-profile-address', message_args: {} } };
    transport.emit(event(LOCAL, 1, { kind: 'setup_progress', progress: progress([failedStep, step('harness', 'running'), step('harness_sign_in', 'waiting'), step('agent_repositories', 'waiting')]) } as never));
    await settle();
    const during = draw(app);
    expect(during).toContain("ordin can't reach your company's settings.");
    guided.answer(ok(report(false, [failed('SETUP_NO_PROFILE_ADDRESS', null, 'company_settings')])));
    await settle();
    const after = draw(app);
    expect(after.split("ordin can't reach your company's settings.").length - 1).toBe(1);
  });

  it('SM-FS-8 — a failed second-install step from the start checks says where the other copy is', async () => {
    const { app, transport } = await appWith((t) => {
      firstStart(t);
    });
    app.firstStart.begin();
    const secondInstall = { ...step('second_install', 'failed'), problem: { message_id: 'setup-second-install', message_args: { folder: '/Applications' } } };
    transport.emit(event(LOCAL, 1, { kind: 'setup_progress', progress: progress([step('company_settings', 'done'), secondInstall]) } as never));
    await settle();
    const markup = draw(app);
    expect(markup).toContain('Checking for another copy of ordin');
    expect(markup).toContain('ordin is also installed in /Applications. Remove one copy, because updates reach only the copy installed for you.');
  });

  it('Edge case — not a first start (SM-FS-1)', async () => {
    const { app, transport } = await appWith();
    expect(app.firstStart.phase).toBe('regions');
    app.firstStart.begin();
    await settle();
    expect(transport.sentOf('setup.status')).toHaveLength(1);
    expect(transport.sentOf('setup.run_guided')).toHaveLength(0);
    const markup = html(App, { app }, app);
    expect(markup).not.toContain('Getting you ready…');
    expect(markup).toContain('data-region="tree"');
  });

  it('Error path — no Claude access (scene noseat, SM-FS-4)', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app, transport } = await appWith((t) => {
      guided = firstStart(t);
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [failed('SETUP_NO_HARNESS_ACCESS', null)])));
    await settle();
    expect(app.firstStart.phase).toBe('first_start');
    const markup = html(App, { app }, app);
    expect(text(markup)).toContain('You don\'t have Claude access yet.');
    // SM-FS-4: no button to ask anyone; the sentence stands alone.
    expect(text(markup)).not.toContain('Ask IT for Claude access');
    expect(markup).not.toContain('data-region="tree"');
    void transport;
  });

  it('a failed result with no action draws its sentence only; Try again sends the guided run again (SM-FS-4)', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app, transport } = await appWith((t) => {
      guided = firstStart(t);
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [failed('SETUP_PROFILE_UNREACHABLE', null, 'company_settings'), failed('SETUP_CATALOGUE_FAILED', 'try_again', 'agent_repositories')])));
    await settle();
    const out = text(html(FirstStart, { model: app.firstStart }, app));
    expect(out).toContain('Your company\'s settings can\'t be reached just now. ordin goes on with the last ones it has.');
    expect(out).toContain('Can\'t reach the company\'s agent library right now.');
    expect(out.match(/Try again/g)).toHaveLength(1);
    void app.firstStart.runGuided();
    await settle();
    expect(transport.sentOf('setup.run_guided')).toHaveLength(2);
  });

  it('a failed result asking for a sign-in draws the sign-in button (SM-FS-4)', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app } = await appWith((t) => {
      guided = firstStart(t);
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [failed('SETUP_SIGN_IN_TIMEOUT', 'sign_in')])));
    await settle();
    const out = text(html(FirstStart, { model: app.firstStart }, app));
    expect(out).toContain('The sign-in wasn\'t finished in time.');
    expect(out).toContain('Sign in to Claude Code');
  });

  it('a failed result with no action draws its sentence and no help button (SM-FS-4, step 11)', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app } = await appWith((t) => {
      guided = firstStart(t);
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [failed('SETUP_HARNESS_MISSING', null, 'harness')])));
    await settle();
    const out = text(html(FirstStart, { model: app.firstStart }, app));
    expect(out).toContain('Claude Code isn\'t on this computer yet. Install it, then choose Check again.');
    expect(out).not.toContain('Send to IT');
  });

  it('Copilot: the words name GitHub Copilot and its sign-in sends copilot_cli (SM-FS-2, SM-FS-3)', async () => {
    const { app, transport } = await appWith((t) => {
      firstStart(t, [step('harness_sign_in', 'needs_you')]);
      t.on('setup.status', ok({ needs_guided: true, progress: progress([step('harness_sign_in', 'needs_you')]), attachments: [attachment('needs_sign_in', 'copilot_cli')] }));
      t.on('setup.sign_in_harness', ok(attachment('ready', 'copilot_cli')));
    });
    const out = text(html(FirstStart, { model: app.firstStart }, app));
    expect(out).toContain('Sign in to GitHub Copilot');
    expect(out).toContain('To do the work, Ordin sends what it reads to GitHub Copilot.');
    await app.firstStart.signIn();
    expect(transport.sentOf('setup.sign_in_harness')[0]?.input).toEqual({ harness: 'copilot_cli' });
  });

  it('when first start is not drawn, a setup_progress event changes nothing', async () => {
    const { app } = await appWith();
    app.firstStart.receive(event(LOCAL, 1, { kind: 'setup_progress', progress: progress([step('harness', 'done')]) } as never));
    expect(app.firstStart.progress.steps).toEqual([]);
  });
});

describe('Error rows (setup)', () => {
  it('setup.status fails at start: the error is drawn in FirstStart\'s frame with Try again; the regions are not drawn', async () => {
    let fail = true;
    const { app, transport } = await appWith((t) => {
      t.on('setup.status', () => (fail ? err('SVC_INTERNAL', 'Something went wrong getting you ready.') : ok({ needs_guided: false, progress: progress([]), attachments: [] })));
    });
    expect(app.firstStart.phase).toBe('first_start');
    const markup = html(App, { app }, app);
    expect(text(markup)).toContain('Something went wrong getting you ready.');
    expect(text(markup)).toContain('Try again');
    expect(markup).not.toContain('data-region="tree"');
    // The guided run waits for a status that answered.
    app.firstStart.begin();
    expect(transport.sentOf('setup.run_guided')).toHaveLength(0);
    fail = false;
    await app.firstStart.checkStatus();
    expect(transport.sentOf('setup.status')).toHaveLength(2);
    expect(app.firstStart.phase).toBe('regions');
  });

  it('The guided run ends with no ready harness: FirstStart stays, with each failed result and its action', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app } = await appWith((t) => {
      guided = firstStart(t);
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [failed('SETUP_HARNESS_UNTESTED', null, 'harness'), failed('SETUP_NO_READY_HARNESS', null, 'harness')])));
    await settle();
    expect(app.firstStart.phase).toBe('first_start');
    const out = text(html(FirstStart, { model: app.firstStart }, app));
    expect(out).toContain('This version of Claude Code hasn\'t been checked with ordin yet. Install a checked version, then try again.');
    expect(out).toContain('ordin needs Claude Code, signed in, before it can start.');
    expect(out).not.toContain('Send to IT');
  });

  it('an error answer of setup.run_guided is drawn inside the frame, without a help button (SM-FS-4)', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app } = await appWith((t) => {
      guided = firstStart(t);
    });
    app.firstStart.begin();
    guided.answer(err('SETUP_STORE_FAILED', 'ordin couldn\'t save its settings.'));
    await settle();
    expect(app.firstStart.phase).toBe('first_start');
    const out = text(html(App, { app }, app));
    expect(out).toContain('ordin couldn\'t save its settings.');
    expect(out).not.toContain('Send to IT');
  });
});

// SM-FS-9 (amended January 2026, user decision): Claude Code not on this computer is self-service —
// Install Claude Code opens the official install page by its name, and Check again runs the check again.
describe('Claude Code is not on this computer (SM-FS-9)', () => {
  it('Happy path — Install Claude Code opens the install page by name and Check again runs the guided setup again', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app, transport } = await appWith((t) => {
      guided = firstStart(t);
      t.on('app.open_web_page', ok({}));
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [failed('SETUP_HARNESS_MISSING', null, 'harness')])));
    await settle();
    const out = text(html(FirstStart, { model: app.firstStart }, app));
    expect(out).toContain("Claude Code isn't on this computer yet. Install it, then choose Check again.");
    expect(out).toContain('Install Claude Code');
    expect(out).toContain('Check again');
    expect(out).not.toMatch(/\bIT\b/);
    await app.firstStart.installClaudeCode();
    expect(transport.sentOf('app.open_web_page').map((r) => r.input)).toEqual([{ page: 'claude_code_install' }]);
    expect(transport.sentOf('setup.run_guided')).toHaveLength(1);
    void app.firstStart.runGuided();
    await settle();
    expect(transport.sentOf('setup.run_guided')).toHaveLength(2);
  });

  it('Edge case — a missing GitHub Copilot draws no Install Claude Code button', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app } = await appWith((t) => {
      guided = firstStart(t);
      t.on('setup.status', ok({ needs_guided: true, progress: progress(SIGNED_OUT), attachments: [attachment('needs_sign_in', 'copilot_cli')] }));
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [failed('SETUP_HARNESS_MISSING', null, 'harness')])));
    await settle();
    expect(app.firstStart.showInstall).toBe(false);
    expect(text(html(FirstStart, { model: app.firstStart }, app))).not.toContain('Install Claude Code');
  });

  it('Error path — the browser does not open: the sentence and Try again are drawn under the buttons', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const BROWSER = "ordin couldn't open your web browser. Open it yourself and try again.";
    const { app, transport } = await appWith((t) => {
      guided = firstStart(t);
      t.on('app.open_web_page', err('CRED_BROWSER_FAILED', BROWSER, { action: 'try_again' }));
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [failed('SETUP_HARNESS_MISSING', null, 'harness')])));
    await settle();
    await app.firstStart.installClaudeCode();
    const out = text(html(FirstStart, { model: app.firstStart }, app));
    expect(out).toContain(BROWSER);
    expect(out).toContain('Try again');
    await app.firstStart.pages.retry();
    expect(transport.sentOf('app.open_web_page')).toHaveLength(2);
  });
});

// SM-CPX-1 (ADR-123): GitHub Copilot is the person's own install; ordin opens no install page for it.
describe('GitHub Copilot is not on this computer (SM-CPX-1)', () => {
  it('Error path — GitHub Copilot is not installed: its sentence and Check again, no install button', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app, transport } = await appWith((t) => {
      guided = firstStart(t);
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [{ ...failed('SETUP_HARNESS_MISSING', null, 'harness'), message_id: 'setup-harness-missing-copilot' }])));
    await settle();
    const out = text(html(FirstStart, { model: app.firstStart }, app));
    expect(out).toContain("GitHub Copilot isn't on this computer yet. Install it, then choose Check again.");
    expect(out).toContain('Check again');
    expect(out).not.toContain('Install Claude Code');
    expect(out).not.toMatch(/Install GitHub Copilot/);
    expect(app.firstStart.showInstall).toBe(false);
    expect(app.firstStart.showCheckAgain).toBe(true);
    void app.firstStart.runGuided();
    await settle();
    expect(transport.sentOf('setup.run_guided')).toHaveLength(2);
    expect(transport.sentOf('app.open_web_page')).toHaveLength(0);
  });

  it('an untested GitHub Copilot draws its own sentence', async () => {
    let guided!: ReturnType<typeof firstStart>;
    const { app } = await appWith((t) => {
      guided = firstStart(t);
    });
    app.firstStart.begin();
    guided.answer(ok(report(false, [{ ...failed('SETUP_HARNESS_UNTESTED', null, 'harness'), message_id: 'setup-harness-untested-copilot' }])));
    await settle();
    const out = text(html(FirstStart, { model: app.firstStart }, app));
    expect(out).toContain("This version of GitHub Copilot hasn't been checked with ordin yet. Install a checked version, then try again.");
  });
});

// Which assistant, asked at first start (ScreensModule.md, amendment 2026-02-04, SM-FS-10 to SM-FS-12).
describe('Which assistant, asked at first start (SM-FS-10 to SM-FS-12)', () => {
  const CHOICE = [step('company_settings', 'done'), step('harness', 'done'), step('harness_choice', 'needs_you'), step('harness_sign_in', 'waiting')];
  const both = (copilot = 'ready') => [attachment('ready'), { ...attachment(copilot, 'copilot_cli'), id: 'h2', location: '/usr/local/bin/copilot' }];

  /** A first start whose `setup.status` already holds the question. */
  function asking(t: FakeTransport, copilot = 'ready'): void {
    t.on('setup.status', ok({ needs_guided: true, progress: progress(CHOICE), attachments: both(copilot) }));
    t.on('setup.run_guided', () => new Promise(() => undefined) as never);
  }
  const screen = (app: Awaited<ReturnType<typeof appWith>>['app']) => html(FirstStart, { model: app.firstStart }, app);

  // spec: ordin-desktop/components/ScreensModule#SM-FS-10
  // spec: ordin-desktop/components/ScreensModule#SM-FS-11
  it('Happy path — the person picks Copilot (SM-FS-10, SM-FS-11)', async () => {
    const { app, transport } = await appWith((t) => {
      asking(t);
      t.on('setup.choose_harness', ok({ restarting: true }));
    });
    const before = screen(app);
    const words = text(before);
    expect(words).toContain('Which assistant should ordin use?');
    expect(words).toContain('Both are on this computer. You can change this later in Settings › Agents and models.');
    expect(words.indexOf('Claude Code')).toBeLessThan(words.indexOf('GitHub Copilot'));
    expect(words.split('Signed in').length - 1).toBe(2);
    expect(before).toContain('role="radiogroup"');
    expect(before).not.toMatch(/<input[^>]*checked/);
    expect(words).toContain('Choose an assistant');
    expect(before).toMatch(/<button[^>]*class="primary[^"]*"[^>]*disabled[^>]*>\s*Choose an assistant/);

    app.firstStart.pick('copilot_cli');
    const picked = screen(app);
    expect(text(picked)).toContain('Use GitHub Copilot');
    expect(picked).not.toMatch(/<button[^>]*class="primary[^"]*"[^>]*disabled/);

    await app.firstStart.choose();
    expect(transport.sentOf('setup.choose_harness')[0]?.input).toEqual({ harness: 'copilot_cli' });
    const after = screen(app);
    expect(text(after)).toContain('Starting ordin again with GitHub Copilot…');
    expect(text(after)).not.toContain('Which assistant should ordin use?');
    expect(after).not.toContain('<button');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-FS-10
  it('the rows come from setup.status asked again when the question appears during the run (SM-FS-10)', async () => {
    let calls = 0;
    const { app, transport } = await appWith((t) => {
      t.on('setup.status', () => {
        calls += 1;
        // At start-up nothing has been looked for yet; the guided run finds both.
        return ok({ needs_guided: true, progress: progress(SIGNED_OUT), attachments: calls === 1 ? [] : both() }) as never;
      });
      t.on('setup.run_guided', () => new Promise(() => undefined) as never);
    });
    app.firstStart.begin();
    transport.emit(event(LOCAL, 1, { kind: 'setup_progress', progress: progress(CHOICE) } as never));
    await settle();
    transport.emit(event(LOCAL, 2, { kind: 'setup_progress', progress: progress(CHOICE) } as never));
    await settle();
    expect(transport.sentOf('setup.status')).toHaveLength(2);
    const words = text(screen(app));
    expect(words).toContain('Which assistant should ordin use?');
    expect(words).toContain('Claude Code');
    expect(words).toContain('GitHub Copilot');
    app.firstStart.pick('copilot_cli');
    expect(text(screen(app))).toContain('Use GitHub Copilot');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-FS-10
  it('Edge case — an assistant that needs signing in is marked (SM-FS-10)', async () => {
    const { app } = await appWith((t) => asking(t, 'needs_sign_in'));
    const words = text(screen(app));
    expect(words).toContain("You'll sign in next");
    expect(words.indexOf('GitHub Copilot')).toBeLessThan(words.indexOf("You'll sign in next"));
    expect(words.split('Signed in').length - 1).toBe(1);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-FS-11
  it('Edge case — choosing the assistant already in use goes on in place (SM-FS-11)', async () => {
    const { app, transport } = await appWith((t) => {
      asking(t);
      t.on('setup.choose_harness', ok({ restarting: false }));
    });
    app.firstStart.pick('claude_code');
    await app.firstStart.choose();
    expect(text(screen(app))).not.toContain('Which assistant should ordin use?');
    transport.emit(event(LOCAL, 1, { kind: 'setup_progress', progress: progress([step('company_settings', 'done'), step('harness', 'done'), step('harness_choice', 'done'), step('harness_sign_in', 'running')]) } as never));
    await settle();
    const words = text(screen(app));
    expect(words).toContain('3 of 4 done');
    expect(words).toContain('Signing you in to Claude Code');
    expect(words).not.toContain('Starting ordin again');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-FS-10
  it('Edge case — no question without the step (SM-FS-10)', async () => {
    const { app } = await appWith((t) => {
      t.on('setup.status', ok({ needs_guided: true, progress: progress(SIGNED_OUT), attachments: both() }));
      t.on('setup.run_guided', () => new Promise(() => undefined) as never);
    });
    const markup = screen(app);
    expect(text(markup)).not.toContain('Which assistant should ordin use?');
    expect(markup).not.toContain('radiogroup');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-FS-11
  it('Error path — the choice could not be saved (SM-FS-11)', async () => {
    const { app, transport } = await appWith((t) => {
      asking(t);
      t.on('setup.choose_harness', err('SETUP_CHOICE_NOT_SAVED', "ordin couldn't save your choice. Try again.", { action: 'try_again' }));
    });
    app.firstStart.pick('copilot_cli');
    await app.firstStart.choose();
    const markup = screen(app);
    expect(text(markup)).toContain("ordin couldn't save your choice. Try again.");
    expect(text(markup)).toContain('Use GitHub Copilot');
    expect(markup).not.toMatch(/<button[^>]*class="primary[^"]*"[^>]*disabled/);
    // One main action: the error line draws no Try again of its own (11 §4).
    expect(text(markup)).not.toContain('Try again Try again');
    expect(markup.match(/<button/g)).toHaveLength(1);
    expect(app.firstStart.chosen).toBe('copilot_cli');
    await app.firstStart.choose();
    expect(transport.sentOf('setup.choose_harness').map((c) => c.input)).toEqual([{ harness: 'copilot_cli' }, { harness: 'copilot_cli' }]);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-FS-11
  it('Error path — an assistant no longer offered redraws from setup.status (SM-FS-11)', async () => {
    let calls = 0;
    const { app, transport } = await appWith((t) => {
      t.on('setup.status', () => {
        calls += 1;
        const attachments = calls === 1 ? both() : [attachment('ready')];
        return ok({ needs_guided: true, progress: progress(CHOICE), attachments }) as never;
      });
      t.on('setup.run_guided', () => new Promise(() => undefined) as never);
      t.on('setup.choose_harness', err('SETUP_HARNESS_NOT_OFFERED', "That assistant isn't available any more. Choose again."));
    });
    app.firstStart.pick('copilot_cli');
    await app.firstStart.choose();
    expect(transport.sentOf('setup.status')).toHaveLength(2);
    expect(app.firstStart.chosen).toBeNull();
    const words = text(screen(app));
    expect(words).toContain("That assistant isn't available any more. Choose again.");
    expect(words).not.toContain('GitHub Copilot');
  });
});
