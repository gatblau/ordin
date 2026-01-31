// @vitest-environment happy-dom
// Meeting minutes on the screens (ScreensModule.md SM-MIN-1 to SM-MIN-22): the pure rules first, then the
// Minutes row of the tree with its jobs, menus and Record… dialog (there is no Minutes screen, SM-MIN-17),
// the Needs you lines, the recording indicator, a minutes page and Settings › Meeting minutes mounted over
// the fake backend. The build without the switch is minutes-absent.test.ts.
import { flushSync, mount, unmount, type Component } from 'svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { startApp } from '../src/lib/app/bootstrap';
import { i18n } from '../src/lib/i18n/fluent';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import MeetingMinutes from '../src/lib/views/Settings/MeetingMinutes.svelte';
import MinutesTreeGroup from '../src/lib/shell/MinutesTreeGroup.svelte';
import MinutesNeedsYou from '../src/lib/views/minutes/MinutesNeedsYou.svelte';
import MinutesPage from '../src/lib/views/minutes/MinutesPage.svelte';
import { canRecord, elapsed, inProgress, progressOf, stageLine } from '../src/lib/views/minutes/minutes-model';
import RecordingIndicator from '../src/lib/views/minutes/RecordingIndicator.svelte';
import { recording } from '../src/lib/views/minutes/recording.svelte';
import { FakeTransport, ok, R1_GROUPS, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const JOB = uid('job');
const MIN = uid('min');
const REC = uid('rec');
const AT = '2026-01-10T10:43:58.120Z';
const GROUPS = [...R1_GROUPS, 'minutes'];

type ObsState = { state: string; [k: string]: unknown };
const status = (recorder: ObsState, extra: Record<string, unknown> = {}) => ({
  model: { choice: 'larger', file_name: 'ggml-medium.bin', folder: '/Users/sam/Library/Ordin/models', present: false, size_mb: 1533 },
  model_consent_given: false,
  recorder,
  recording: null,
  recommended_model: 'larger',
  settings: { delete_recordings_after_save: false, obs_port: 4455, speech_model: 'recommended' },
  writer_name: 'Claude',
  writer_notice_seen: true,
  ...extra,
});
const READY = { state: 'ready', obs_version: '30.1.0', others_heard: true };
const job = (state: Record<string, unknown>, source_name = 'meeting.vtt') => ({ id: JOB, workspace_id: WS, source: 'transcript_file', source_name, state, created_at: AT });
const minutes = { id: MIN, workspace_id: WS, title: 'Supplier call', body: '## Summary\n\nPrices agreed.', source: 'recording', source_name: 'meeting.mkv', recording: REC, revision: 1, kept_at: AT };

let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
beforeEach(() => recording.reset());
afterEach(() => {
  if (drawn !== null) void unmount(drawn.part);
  drawn?.target.remove();
  drawn = null;
  document.body.innerHTML = '';
  recording.reset();
});

async function flush(): Promise<void> {
  for (let i = 0; i < 6; i++) {
    await settle();
    flushSync();
  }
}
const words = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const byTest = (id: string): HTMLElement | null => document.querySelector(`[data-testid="${id}"]`);
const buttonNamed = (name: string): HTMLButtonElement | undefined =>
  Array.from(document.querySelectorAll('button')).find((b) => words(b.textContent) === name);
const dialogs = (): HTMLElement[] => Array.from(document.querySelectorAll('[role="alertdialog"], [role="dialog"]'));
async function press(el: Element | null | undefined): Promise<void> {
  expect(el, 'the button is drawn').toBeTruthy();
  (el as HTMLElement).click();
  await flush();
}
const sentNamed = (transport: FakeTransport, command: string) => transport.sent.filter((e) => e.command === command);

async function appOn(setup: (tr: FakeTransport) => void, groups = GROUPS): Promise<{ app: AppContext; transport: FakeTransport }> {
  const transport = new FakeTransport({ localGroups: groups });
  setup(transport);
  const data = new Map<string, string>();
  const storage = { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
  const app = await startApp(transport, { document: null, storage, i18n });
  return { app, transport };
}

async function draw(component: Component<never>, props: Record<string, unknown>, setup: (tr: FakeTransport) => void) {
  const { app, transport } = await appOn((tr) => {
    tr.on('minutes.list', ok({ items: [], next_cursor: null }));
    tr.on('minutes.jobs', ok([]));
    setup(tr);
  });
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(component, { target, props: { backend: app.local, ...props } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  drawn = { target, part };
  await flush();
  return { app, transport, target };
}
/** The Minutes row of the tree, unfolded, with the Needs you lines beside it (SM-MIN-17 to SM-MIN-21). */
async function screen(setup: (tr: FakeTransport) => void) {
  const out = await draw(MinutesTreeGroup as Component<never>, { workspaceId: WS, initiallyOpen: true }, setup);
  const needs = document.createElement('div');
  document.body.appendChild(needs);
  mount(MinutesNeedsYou, { target: needs, props: { backend: out.app.local }, context: new Map<unknown, unknown>([[APP_KEY, out.app]]) });
  await flush();
  return out;
}
/** **Record…** from the Minutes row's menu (SM-MIN-18, SM-MIN-20). */
async function chooseRecord(): Promise<void> {
  await press(byTest('minutes-tree-menu'));
  await press(byTest('minutes-record'));
}
/** A job row's menu (SM-MIN-19). */
async function jobMenu(): Promise<void> {
  await press(byTest('minutes-job-menu'));
}
/** The Open of a Needs you line (SM-MIN-21). */
async function openQuestion(kind: 'model' | 'notice'): Promise<void> {
  await press(byTest(`minutes-needs-${kind}`)?.querySelector('button'));
}

describe('the pure rules (SM-MIN-2, SM-MIN-5, SM-MIN-7)', () => {
  it('names each stage, with numbers where the state has them', () => {
    expect(stageLine({ state: 'queued' })).toEqual({ id: 'minutes-stage-queued' });
    expect(stageLine({ state: 'waiting_for_model' })).toEqual({ id: 'minutes-stage-waiting-for-model' });
    expect(stageLine({ state: 'downloading_model', received_mb: 420, total_mb: 1533 })).toEqual({ id: 'minutes-stage-downloading-model', args: { received: 420, total: 1533 } });
    expect(stageLine({ state: 'transcribing', done_ms: 12 * 60_000, total_ms: 60 * 60_000 })).toEqual({ id: 'minutes-stage-transcribing', args: { done: 12, total: 60 } });
    expect(stageLine({ state: 'failed', code: 'MIN_WRITE_FAILED', message: 'x' })).toBeNull();
    expect(progressOf({ state: 'downloading_model', received_mb: 420, total_mb: 1533 })).toEqual({ value: 420, max: 1533 });
    expect(progressOf({ state: 'writing' })).toBeNull();
    expect(inProgress(job({ state: 'done', minutes_id: MIN }) as never)).toBe(false);
    expect(inProgress(job({ state: 'failed', code: 'C', message: 'm' }) as never)).toBe(true);
  });

  it('Record is pressable only when OBS is ready', () => {
    expect(canRecord(READY as never)).toBe(true);
    for (const state of ['not_installed', 'not_reachable', 'password_needed', 'password_refused', 'version_unsupported', 'recording']) {
      expect(canRecord({ state } as never)).toBe(false);
    }
    expect(canRecord(null)).toBe(false);
  });

  it('shows minutes and seconds since the start', () => {
    expect(elapsed('2026-01-10T10:00:00.000Z', Date.parse('2026-01-10T10:12:04.900Z'))).toBe('12:04');
    expect(elapsed('2026-01-10T10:00:00.000Z', Date.parse('2026-01-10T09:59:00.000Z'))).toBe('00:00');
  });
});

describe('Record and the reminder (SM-MIN-5)', () => {
  it('Edge — the reminder shows on every press: three presses cancelled, three reminders, nothing recorded', async () => {
    const { transport } = await screen((tr) => tr.on('minutes.status', ok(status(READY))));
    for (let i = 0; i < 3; i++) {
      await chooseRecord();
      expect(words(dialogs()[0]?.textContent)).toContain("Tell everyone you're recording");
      await press(buttonNamed('Cancel'));
      expect(dialogs()).toHaveLength(0);
    }
    expect(sentNamed(transport, 'minutes.record')).toHaveLength(0);
  });

  it('only the microphone: the reminder says so when OBS cannot hear the others', async () => {
    await screen((tr) => tr.on('minutes.status', ok(status({ ...READY, others_heard: false }))));
    await chooseRecord();
    expect(words(byTest('minutes-record-mic-only')?.textContent)).toContain('Only your microphone will be recorded');
  });
});

describe('the recording indicator (SM-MIN-7)', () => {
  it('Happy path — record and stop: the indicator shows from the record answer to the stop answer, then a job appears', async () => {
    let jobs: unknown[] = [];
    const { transport, app } = await screen((tr) => {
      tr.on('minutes.status', ok(status(READY)));
      tr.on('minutes.jobs', () => ok(jobs));
      tr.on('minutes.record', ok({ id: REC, workspace_id: WS, started_at: new Date().toISOString(), others_heard: true }));
      tr.on('minutes.stop', () => {
        jobs = [job({ state: 'queued' }, 'meeting.mkv')];
        return ok(jobs[0]);
      });
    });
    const header = document.createElement('div');
    document.body.appendChild(header);
    const indicator = mount(RecordingIndicator, { target: header, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    await flush();
    expect(byTest('minutes-recording')).toBeNull();

    await chooseRecord();
    await press(buttonNamed('Start recording'));
    expect(sentNamed(transport, 'minutes.record')[0]?.input).toEqual({ workspace_id: WS });
    expect(words(byTest('minutes-recording')?.textContent)).toMatch(/Recording — 00:0\d/);
    expect(words(app.shell.announcement)).toContain('Recording started.');
    // While a recording runs, the Minutes menu offers no Record… (SM-MIN-18).
    await press(byTest('minutes-tree-menu'));
    expect(byTest('minutes-record')).toBeNull();
    await press(byTest('minutes-tree-menu'));

    await press(byTest('minutes-stop'));
    expect(sentNamed(transport, 'minutes.stop')[0]?.input).toEqual({ recording_id: REC });
    expect(byTest('minutes-recording')).toBeNull();
    expect(words(app.shell.announcement)).toContain('Recording stopped.');
    expect(byTest('minutes-job')?.getAttribute('data-state')).toBe('queued');
    expect(words(byTest('minutes-stage-queued')?.textContent)).toBe('Waiting to start');
    expect(dialogs()).toHaveLength(0);
    void unmount(indicator);
  });

  it('a recording stopped in OBS ends the indicator at the next status read', async () => {
    const { transport, app } = await appOn((tr) => tr.on('minutes.status', ok(status(READY))));
    const target = document.createElement('div');
    document.body.appendChild(target);
    const part = mount(RecordingIndicator, { target, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    recording.started(app.local.handle, { id: REC, workspace_id: WS, started_at: AT, others_heard: true });
    await flush();
    expect(byTest('minutes-recording')).not.toBeNull();
    await recording.check();
    await flush();
    expect(sentNamed(transport, 'minutes.status')[0]?.input).toEqual({ workspace_id: WS });
    expect(byTest('minutes-recording')).toBeNull();
    void unmount(part);
  });
});

describe('three OBS problems (SM-MIN-6)', () => {
  it('Error — Record… says what is wrong with OBS: not installed, not reachable and a refused password each draw their own line (SM-MIN-20)', async () => {
    const cases: Array<[ObsState, string, string]> = [
      [{ state: 'not_installed' }, 'minutes-get-obs', 'Get OBS'],
      [{ state: 'not_reachable' }, 'minutes-open-obs', 'Open OBS'],
      [{ state: 'password_refused' }, 'minutes-obs-password', ''],
    ];
    for (const [recorder, control, name] of cases) {
      const { transport } = await screen((tr) => tr.on('minutes.status', ok(status(recorder))));
      await chooseRecord();
      expect(words(dialogs()[0]?.textContent)).toContain("OBS isn't ready to record");
      expect(buttonNamed('Start recording')).toBeUndefined();
      expect(byTest('minutes-recorder')?.getAttribute('data-state')).toBe(recorder.state);
      expect(byTest(control), control).not.toBeNull();
      if (name !== '') expect(words(byTest(control)?.textContent)).toBe(name);
      for (const other of cases.map((c) => c[1]).filter((c) => c !== control)) expect(byTest(other)).toBeNull();
      if (recorder.state === 'not_installed') {
        await press(byTest('minutes-get-obs'));
        expect(sentNamed(transport, 'app.open_web_page')[0]?.input).toEqual({ page: 'obs_download' });
      }
      if (recorder.state === 'password_refused') {
        transport.on('minutes.save_obs_password', ok({}));
        const field = byTest('minutes-obs-password') as HTMLInputElement;
        expect(field.type).toBe('password');
        field.value = 'fixture-pass';
        field.dispatchEvent(new Event('input'));
        await flush();
        await press(byTest('minutes-obs-save'));
        expect(sentNamed(transport, 'minutes.save_obs_password')[0]?.input).toEqual({ password: 'fixture-pass' });
        expect(field.value).toBe('');
      }
      void unmount(drawn!.part);
      drawn = null;
      document.body.innerHTML = '';
    }
  });
});

describe('the questions a job waits on (SM-MIN-3, SM-MIN-4)', () => {
  it('Edge — the consent prompt names size and folder; Not now cancels the job and downloads nothing', async () => {
    const { transport } = await screen((tr) => {
      tr.on('minutes.status', ok(status(READY)));
      tr.on('minutes.jobs', ok([job({ state: 'waiting_for_model' }, 'speech-60s.m4a')]));
      tr.on('minutes.cancel_job', ok(job({ state: 'cancelled' }, 'speech-60s.m4a')));
    });
    // The question waits in Needs you; nothing opens by itself (SM-MIN-21).
    expect(dialogs()).toHaveLength(0);
    expect(words(byTest('minutes-needs-model')?.textContent)).toContain('Download the speech model for speech-60s.m4a?');
    await openQuestion('model');
    const body = words(dialogs()[0]?.textContent);
    expect(body).toContain("Download Ordin's speech model?");
    expect(body).toContain('1,533 MB');
    expect(body).toContain('/Users/sam/Library/Ordin/models');
    await press(buttonNamed('Not now'));
    expect(sentNamed(transport, 'minutes.cancel_job')[0]?.input).toEqual({ job_id: JOB });
    expect(sentNamed(transport, 'minutes.give_model_consent')).toHaveLength(0);
  });

  it('the one-time notice names the AI service and Write the minutes accepts it', async () => {
    const { transport } = await screen((tr) => {
      tr.on('minutes.status', ok(status(READY, { writer_notice_seen: false })));
      tr.on('minutes.jobs', ok([job({ state: 'waiting_for_notice' })]));
      tr.on('minutes.accept_notice', ok({}));
    });
    expect(words(byTest('minutes-needs-notice')?.textContent)).toContain('Before Ordin writes minutes of meeting.vtt');
    await openQuestion('notice');
    expect(words(dialogs()[0]?.textContent)).toContain('to Claude to write the minutes');
    await press(buttonNamed('Write the minutes'));
    expect(sentNamed(transport, 'minutes.accept_notice')).toHaveLength(1);
  });

  it('Happy path — a transcript becomes minutes: the stage reads, then the minutes are announced ready', async () => {
    let jobs = [job({ state: 'reading' })];
    let items: unknown[] = [];
    const { app } = await screen((tr) => {
      tr.on('minutes.status', ok(status(READY, { writer_notice_seen: true })));
      tr.on('minutes.jobs', () => ok(jobs));
      tr.on('minutes.list', () => ok({ items, next_cursor: null }));
    });
    expect(words(byTest('minutes-stage-reading')?.textContent)).toBe('Reading the transcript');
    jobs = [job({ state: 'done', minutes_id: MIN })];
    items = [{ id: MIN, title: 'Supplier call', source: 'transcript_file', kept_at: AT }];
    await new Promise((r) => setTimeout(r, 2_100));
    await flush();
    expect(words(app.shell.announcement)).toContain('Minutes of Supplier call are ready.');
    expect(words(document.body.textContent)).toContain('Supplier call');
  });
});

describe('a minutes page (SM-MIN-8)', () => {
  it('Happy path — Save waits for an edit, and Export › Word… sends format docx', async () => {
    const { transport } = await draw(MinutesPage as Component<never>, { workspaceId: WS, instance: MIN }, (tr) => {
      tr.on('minutes.get', ok(minutes));
      tr.on('minutes.save_edit', (input) => ok({ ...minutes, body: (input as { body: string }).body, revision: 2 }));
      tr.on('minutes.export', ok({ saved_path: '/Users/sam/Supplier call.docx' }));
    });
    expect(words(byTest('minutes-kept-on')?.textContent)).toContain('Made from meeting.mkv on');
    expect(byTest('minutes-save')?.hasAttribute('disabled')).toBe(true);
    await press(byTest('minutes-menu'));
    expect(byTest('minutes-delete-recording')).not.toBeNull();
    await press(byTest('minutes-export'));
    await press(byTest('minutes-export-word'));
    expect(sentNamed(transport, 'minutes.export')[0]?.input).toEqual({ minutes_id: MIN, format: 'docx' });
  });
});

describe('menus and icon buttons (SM-MIN-13 to SM-MIN-15)', () => {
  const labelOf = (el: Element | null | undefined): string => (el as HTMLElement | null)?.getAttribute('aria-label') ?? '';
  const menuItems = (): string[] => Array.from(document.querySelectorAll('[role="menu"] > [role="menuitem"]')).map((b) => words(b.textContent));

  it('the page header holds icon buttons and a menu of Export, Delete recording… and Remove…', async () => {
    await draw(MinutesPage as Component<never>, { workspaceId: WS, instance: MIN }, (tr) => tr.on('minutes.get', ok(minutes)));
    for (const id of ['minutes-save', 'minutes-show-transcript']) {
      const button = byTest(id);
      expect(words(button?.textContent), id).toBe('');
      expect(labelOf(button), id).not.toBe('');
      expect(button?.getAttribute('title'), id).toBe(labelOf(button));
    }
    expect(labelOf(byTest('minutes-menu'))).toContain('Supplier call');
    await press(byTest('minutes-menu'));
    expect(menuItems()).toEqual(['Export', 'Delete recording…', 'Remove…']);
    expect(document.activeElement).toBe(byTest('minutes-export'));
    byTest('minutes-export')?.closest('[role="menu"]')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await flush();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it('no Delete recording… once the recording is gone', async () => {
    await draw(MinutesPage as Component<never>, { workspaceId: WS, instance: MIN }, (tr) => tr.on('minutes.get', ok({ ...minutes, recording: null })));
    await press(byTest('minutes-menu'));
    expect(menuItems()).toEqual(['Export', 'Remove…']);
  });

  it('a row of the Minutes screen opens its menu on right-click; Export opens to PDF… and Word…', async () => {
    const { transport } = await screen((tr) => {
      tr.on('minutes.status', ok(status(READY)));
      tr.on('minutes.list', ok({ items: [{ id: MIN, title: 'Supplier call', source: 'recording', kept_at: AT }], next_cursor: null }));
      tr.on('minutes.remove', ok({}));
    });
    const row = byTest('minutes-item')?.querySelector('.tree-row');
    row?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await flush();
    expect(menuItems()).toEqual(['Export', 'Remove…']);
    await press(byTest('minutes-export'));
    expect(byTest('minutes-export-pdf')).not.toBeNull();
    expect(byTest('minutes-export-word')).not.toBeNull();
    await press(byTest('minutes-remove'));
    await press(buttonNamed('Remove'));
    expect(sentNamed(transport, 'minutes.remove')[0]?.input).toEqual({ minutes_id: MIN });
  });

  it('the Minutes row opens no screen; its menu holds Record… and Make minutes from a file…; a job row\'s menu holds Try again and Cancel (SM-MIN-17 to SM-MIN-19)', async () => {
    const { transport, app } = await screen((tr) => {
      tr.on('minutes.status', ok(status(READY)));
      tr.on('minutes.jobs', ok([job({ state: 'failed', code: 'TRX_AUDIO_UNDECODABLE', message: "Ordin couldn't play the sound in that file." })]));
      tr.on('minutes.import', ok(job({ state: 'queued' })));
    });
    const row = byTest('minutes-tree')?.querySelector('.tree-row') as HTMLElement;
    row.click();
    await flush();
    expect(app.screens.tabs(ScreenAreaState.scope(app.local.id, WS))).toHaveLength(0);
    row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await flush();
    expect(menuItems()).toEqual(['Record…', 'Make minutes from a file…']);
    await press(byTest('minutes-import'));
    expect(sentNamed(transport, 'minutes.import')[0]?.input).toEqual({ workspace_id: WS });
    const failed = byTest('minutes-job');
    expect(words(failed?.textContent)).toContain("Couldn't finish");
    expect(failed?.querySelector('[title]')?.getAttribute('title')).toBe("Ordin couldn't play the sound in that file.");
    await jobMenu();
    expect(menuItems()).toEqual(['Try again', 'Cancel']);
    expect(words(byTest('minutes-job-reason')?.textContent)).toBe("Ordin couldn't play the sound in that file.");
  });
});

describe('Settings › Meeting minutes (SM-MIN-9)', () => {
  it('each change sends one minutes.change_settings and its answer replaces the view', async () => {
    const { transport } = await draw(MeetingMinutes as Component<never>, { workspaceId: WS }, (tr) => {
      tr.on('minutes.status', ok(status(READY)));
      tr.on('minutes.change_settings', (input) => ok(status(READY, { settings: { delete_recordings_after_save: true, obs_port: 4455, speech_model: 'recommended', ...(input as object) } })));
    });
    expect(words(byTest('minutes-settings-model')?.textContent)).toContain('Not downloaded yet');
    const toggle = byTest('minutes-settings-recordings')?.querySelector('input') as HTMLInputElement;
    expect(toggle.checked).toBe(false);
    toggle.click();
    await flush();
    expect(sentNamed(transport, 'minutes.change_settings').map((e) => e.input)).toEqual([{ delete_recordings_after_save: true }]);
    expect((byTest('minutes-settings-recordings')?.querySelector('input') as HTMLInputElement).checked).toBe(true);
  });
});

describe('the speech model chosen by the machine (SM-MIN-12)', () => {
  const SMALLER = { choice: 'smaller', file_name: 'ggml-small.bin', folder: '/Users/sam/Library/Ordin/models', present: false, size_mb: 488 };

  it('the recommendation is the default and names the model', async () => {
    await draw(MeetingMinutes as Component<never>, { workspaceId: WS }, (tr) => {
      tr.on('minutes.status', ok(status(READY, { recommended_model: 'smaller', model: SMALLER })));
    });
    const card = byTest('minutes-settings-model');
    const options = [...(card?.querySelectorAll('label') ?? [])].map((l) => words(l.textContent));
    expect(options).toEqual(['Recommended for this computer', 'Larger — more accurate, about 1.5 GB', 'Smaller — faster, about 470 MB']);
    const radios = [...(card?.querySelectorAll('input[type=radio]') ?? [])] as HTMLInputElement[];
    expect(radios.map((r) => r.checked)).toEqual([true, false, false]);
    expect(words(byTest('minutes-settings-recommended-line')?.textContent)).toBe('Smaller, about 470 MB — the one this computer handles well');
    expect(byTest('minutes-settings-larger-slow')).toBeNull();
  });

  it('picking the larger model on a computer it does not suit sends one change and shows the warning', async () => {
    const { transport } = await draw(MeetingMinutes as Component<never>, { workspaceId: WS }, (tr) => {
      tr.on('minutes.status', ok(status(READY, { recommended_model: 'smaller', model: SMALLER })));
      tr.on('minutes.change_settings', (input) =>
        ok(status(READY, { recommended_model: 'smaller', settings: { delete_recordings_after_save: false, obs_port: 4455, speech_model: 'recommended', ...(input as object) } })),
      );
    });
    const larger = byTest('minutes-settings-model')?.querySelectorAll('input[type=radio]')[1] as HTMLInputElement;
    larger.click();
    await flush();
    expect(sentNamed(transport, 'minutes.change_settings').map((e) => e.input)).toEqual([{ speech_model: 'larger' }]);
    expect(words(byTest('minutes-settings-larger-slow')?.textContent)).toBe('On this computer the larger model may take more than an hour for each hour of meeting.');
  });

  it('no warning when the larger model is the one the computer suits', async () => {
    await draw(MeetingMinutes as Component<never>, { workspaceId: WS }, (tr) => {
      tr.on('minutes.status', ok(status(READY, { settings: { delete_recordings_after_save: false, obs_port: 4455, speech_model: 'larger' } })));
    });
    expect(byTest('minutes-settings-larger-slow')).toBeNull();
    expect(words(byTest('minutes-settings-recommended-line')?.textContent)).toBe('Larger, about 1.5 GB — the one this computer handles well');
  });
});

describe('text for Ordin (SM-MIN-11)', () => {
  it('a job Ordin started shows "text for Ordin", can be cancelled, and leaves In progress once ready, announcing nothing', async () => {
    const forOrdin = (state: Record<string, unknown>) => ({ ...job(state, 'standup.m4a'), source: 'audio_file', purpose: 'transcript' });
    let jobs = [forOrdin({ state: 'transcribing', done_ms: 60_000, total_ms: 600_000 })];
    const { app, transport } = await screen((tr) => {
      tr.on('minutes.status', ok(status(READY, { model_consent_given: true })));
      tr.on('minutes.jobs', () => ok(jobs));
      tr.on('minutes.cancel_job', ok(forOrdin({ state: 'cancelled' })));
    });
    const row = byTest('minutes-job');
    expect(words(row?.textContent)).toContain('standup.m4a — text for Ordin');
    expect(words(row?.textContent)).toContain('Turning the recording into text — 1 of 10 minutes');
    await jobMenu();
    await press(byTest('minutes-cancel'));
    expect(sentNamed(transport, 'minutes.cancel_job')[0]?.input).toEqual({ job_id: JOB });
    jobs = [forOrdin({ state: 'transcript_ready' })];
    await new Promise((r) => setTimeout(r, 2_100));
    await flush();
    expect(byTest('minutes-job')).toBeNull();
    expect(words(app.shell.announcement)).not.toContain('are ready');
  });

  it('transcript_ready is finished: not in progress and not moving', () => {
    const ready = { ...job({ state: 'transcript_ready' }), purpose: 'transcript' } as never;
    expect(inProgress(ready)).toBe(false);
    expect(stageLine({ state: 'transcript_ready' } as never)).toEqual({ id: 'minutes-stage-transcript-ready' });
  });
});
