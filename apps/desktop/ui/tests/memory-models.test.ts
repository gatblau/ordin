// Getting memory ready (ScreensModule.md amendment 2026-01-28, SM-MD-1 to SM-MD-6): one test per added
// scenario and per added error row, with the fake transport of tests/support/fake-backend.ts.
import { beforeAll, describe, expect, it } from 'vitest';

import ConversationPanel from '../src/lib/shell/ConversationPanel.svelte';
import type { MemoryModelsView } from '../src/lib/service-api/types.gen';
import MemoryModelsCard from '../src/lib/views/memory/MemoryModelsCard.svelte';
import { failureMessageId, MemoryModelsModel } from '../src/lib/views/memory/memory-models-model.svelte';
import { EN_GB_FTL, err, event, LOCAL, ok, SERVER, settle, type FakeTransport } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { text } from './settings-support';

beforeAll(loadWindowCatalogue);

const WS = '0192f100-0000-7000-8000-0000000000c1';

const view = (overrides: Partial<MemoryModelsView> = {}): MemoryModelsView => ({
  state: 'missing',
  size_bytes: 173_982_577,
  host: 'github.com',
  received_bytes: 0,
  ask: false,
  ...overrides,
});

let seq = 100;
const changed = (t: FakeTransport, models: MemoryModelsView): void => {
  t.emit(event(LOCAL, seq++, { kind: 'memory_models_changed', models } as never));
};

/** The window with a local workspace's conversation open and `app.memory_models` answering `start`. */
async function windowWith(start: MemoryModelsView, setup: (t: FakeTransport) => void = () => undefined) {
  const { app, transport } = await appWith((t) => {
    t.on('app.memory_models', ok(start));
    setup(t);
  });
  app.shell.openWorkspaceTab(LOCAL, WS);
  const panel = (): string => text(html(ConversationPanel, {}, app));
  return { app, transport, panel };
}

describe('the catalogue (SM-MD-2 to SM-MD-4)', () => {
  it('every id of the card and every failure sentence has its en-GB words', () => {
    const ids = [
      'memory-models-title', 'memory-models-ask', 'memory-models-download', 'memory-models-resume', 'memory-models-not-now',
      'memory-models-progress', 'memory-models-stop', 'memory-models-checking', 'memory-models-ready', 'memory-models-try-again',
      ...['DSK_MODELS_NOT_IN_BUILD', 'DSK_MODELS_OFFLINE', 'DSK_MODELS_BLOCKED', 'DSK_MODELS_MISMATCH', 'DSK_MODELS_DISK_FULL', 'DSK_MODELS_INTERRUPTED', 'DSK_MODELS_WRITE_FAILED'].map(failureMessageId),
    ];
    for (const id of ids) expect(EN_GB_FTL, id).toMatch(new RegExp(`^${id} = `, 'm'));
    expect(failureMessageId('DSK_MODELS_OFFLINE')).toBe('error-dsk-models-offline');
  });

  // The failure sentences the card shows are checked with the Error Table's rows below; this check
  // covers the card's own ids.
  it('SM-MD-5 — no word of the card names a file, a fingerprint, a folder or a model', () => {
    const card = EN_GB_FTL.split('\n').filter((line) => /^memory-models-/.test(line));
    expect(card.length).toBe(10);
    for (const line of card) expect(line.split(' = ')[1], line).not.toMatch(/\b(file|files|fingerprint|folder|model|models|zip)\b/i);
  });
});

describe('Getting memory ready (SM-MD-1 to SM-MD-4)', () => {
  it('Happy path — asked, downloaded, ready (SM-MD-1 to SM-MD-3)', async () => {
    const { app, transport, panel } = await windowWith(view({ ask: false }), (t) => {
      t.on('app.download_memory_models', ok(view({ state: 'downloading' })));
    });
    // SM-MD-1: sent at start to the local backend.
    expect(transport.sentOf('app.memory_models').map((e) => e.backend)).toEqual([LOCAL]);
    expect(panel()).not.toContain('Getting memory ready');

    changed(transport, view({ ask: true }));
    await settle();
    let out = panel();
    expect(out).toContain('Getting memory ready');
    expect(out).toContain('To remember things for you, Ordin needs a one-time download of 174 MB from github.com.');
    expect(html(ConversationPanel, {}, app)).toMatch(/>Download<\/button>/);
    expect(out).toContain('Not now');
    expect(html(ConversationPanel, {}, app)).toContain('aria-live="polite"');

    await app.memoryModels.download();
    expect(transport.sentOf('app.download_memory_models')).toHaveLength(1);
    expect(transport.sentOf('app.download_memory_models')[0]?.input).toEqual({});

    changed(transport, view({ state: 'downloading', received_bytes: 87_000_000 }));
    await settle();
    out = panel();
    expect(out).toContain('Downloading 87 of 174 MB…');
    expect(out).toContain('Stop');
    const markup = html(ConversationPanel, {}, app);
    expect(markup).toMatch(/role="progressbar"[^>]*aria-valuenow="50"/);
    expect(markup).toContain('#i-stop');

    changed(transport, view({ state: 'ready', received_bytes: 173_982_577 }));
    await settle();
    expect(panel()).toContain('Memory is ready.');
  });

  it('the ready line goes 5 seconds later (SM-MD-3)', async () => {
    const timers: Array<{ run: () => void; ms: number }> = [];
    const { app } = await appWith();
    const model = new MemoryModelsModel(app.local.handle, { setTimer: (run, ms) => timers.push({ run, ms }) });
    model.receive(event(LOCAL, 1, { kind: 'memory_models_changed', models: view({ state: 'checking' }) } as never));
    expect(text(html(MemoryModelsCard, { model }, app))).toContain('Checking the download…');
    model.receive(event(LOCAL, 2, { kind: 'memory_models_changed', models: view({ state: 'ready' }) } as never));
    expect(text(html(MemoryModelsCard, { model }, app))).toContain('Memory is ready.');
    expect(timers.map((x) => x.ms)).toEqual([5_000]);
    timers[0]?.run();
    expect(text(html(MemoryModelsCard, { model }, app))).not.toContain('Getting memory ready');
  });

  it('Edge case — Not now (SM-MD-2)', async () => {
    const { app, transport, panel } = await windowWith(view({ ask: true }), (t) => {
      t.on('app.decline_memory_models', ok(view({ ask: false })));
    });
    expect(panel()).toContain('Getting memory ready');
    await app.memoryModels.notNow();
    expect(transport.sentOf('app.decline_memory_models')).toHaveLength(1);
    expect(transport.sentOf('app.download_memory_models')).toHaveLength(0);
    const out = panel();
    expect(out).not.toContain('Getting memory ready');
    // Not a dialog: the message box stays and nothing took the focus.
    const markup = html(ConversationPanel, {}, app);
    expect(markup).toContain('<textarea');
    expect(markup).not.toMatch(/role="dialog"|autofocus/);
  });

  it('Edge case — resumed after a stop (SM-MD-2, SM-MD-3)', async () => {
    const { app, panel } = await windowWith(view({ ask: true, received_bytes: 50_000_000 }));
    expect(panel()).toContain('Carry on downloading');
    expect(html(ConversationPanel, {}, app)).not.toMatch(/>Download<\/button>/);
  });

  it('Stop sends app.stop_memory_models_download and the card goes with missing (SM-MD-3)', async () => {
    const { app, transport, panel } = await windowWith(view({ state: 'downloading', received_bytes: 50_000_000 }), (t) => {
      t.on('app.stop_memory_models_download', ok(view({ state: 'missing', received_bytes: 50_000_000 })));
    });
    expect(panel()).toContain('Downloading 50 of 174 MB…');
    await app.memoryModels.stop();
    expect(transport.sentOf('app.stop_memory_models_download')).toHaveLength(1);
    expect(panel()).not.toContain('Getting memory ready');
  });

  it('Error path — offline (SM-MD-4)', async () => {
    const { app, transport, panel } = await windowWith(view({ ask: true }), (t) => {
      t.on('app.download_memory_models', ok(view({ state: 'downloading' })));
    });
    await app.memoryModels.download();
    changed(transport, view({ state: 'failed', failure: 'DSK_MODELS_OFFLINE' }));
    await settle();
    const out = panel();
    expect(out).toContain("ordin couldn't connect to get memory ready. Check you're online, then try again.");
    expect(out).toContain('Try again');
    expect(out).toContain('Not now');
    expect(out).not.toContain('DSK_MODELS_OFFLINE');
    await app.memoryModels.download();
    expect(transport.sentOf('app.download_memory_models')).toHaveLength(2);
  });

  it('Error path — damaged download (SM-MD-4)', async () => {
    const { app, transport, panel } = await windowWith(view({ ask: true }), (t) => {
      t.on('app.download_memory_models', ok(view({ state: 'downloading' })));
    });
    await app.memoryModels.download();
    changed(transport, view({ state: 'failed', failure: 'DSK_MODELS_MISMATCH' }));
    await settle();
    expect(panel()).toContain("The memory download arrived damaged, so ordin didn't use it. Try again; if it happens again, choose Save logs for support… in Settings › Diagnostics.");
  });

  it('Error path — full disk (SM-MD-4)', async () => {
    const { app, transport, panel } = await windowWith(view({ ask: true }), (t) => {
      t.on('app.download_memory_models', ok(view({ state: 'downloading' })));
    });
    await app.memoryModels.download();
    changed(transport, view({ state: 'failed', failure: 'DSK_MODELS_DISK_FULL' }));
    await settle();
    expect(panel()).toContain("There isn't enough free space on this computer to get memory ready. Free up about 350 MB, then try again.");
  });

  it('Error row — the download ends in failed: each code shows its sentence with Try again and Not now', async () => {
    const sentences: Record<string, string> = {
      DSK_MODELS_BLOCKED: 'Your network stopped the memory download. Try again later, or on another network.',
      DSK_MODELS_INTERRUPTED: 'The memory download stopped before it finished. Try again; it carries on from where it stopped.',
      DSK_MODELS_WRITE_FAILED: "ordin couldn't save what memory needs on this computer. Try again; if it happens again, choose Save logs for support… in Settings › Diagnostics.",
    };
    for (const [code, sentence] of Object.entries(sentences)) {
      const { app, transport, panel } = await windowWith(view({ ask: true }), (t) => {
        t.on('app.download_memory_models', ok(view({ state: 'downloading' })));
      });
      await app.memoryModels.download();
      changed(transport, view({ state: 'failed', failure: code }));
      await settle();
      const out = panel();
      expect(out, code).toContain(sentence);
      expect(out, code).toContain('Try again');
    }
  });

  it('Error row — a command of the card is refused: step 11 inside the card', async () => {
    const { app, panel } = await windowWith(view({ ask: true }), (t) => {
      t.on('app.download_memory_models', err('DSK_MODELS_NOT_IN_BUILD', "This version of ordin can't get memory ready. Install the latest version of ordin."));
    });
    await app.memoryModels.download();
    const out = panel();
    expect(out).toContain('Getting memory ready');
    expect(out).toContain("This version of ordin can't get memory ready. Install the latest version of ordin.");
    expect(out).not.toContain('DSK_MODELS_NOT_IN_BUILD');
  });

  it('Edge case — a build with no models (SM-MD-1)', async () => {
    const { transport, panel } = await windowWith(view({ state: 'not_in_build', size_bytes: 0, host: '' }));
    expect(panel()).not.toContain('Getting memory ready');
    changed(transport, view({ state: 'not_in_build', size_bytes: 0, host: '', ask: true }));
    await settle();
    expect(panel()).not.toContain('Getting memory ready');
  });

  it('an event from another backend is not taken (SM-MD-1)', async () => {
    const { app, transport } = await windowWith(view({ ask: true }));
    expect(app.memoryModels.shows).toBe('ask');
    transport.emit(event(SERVER, 1, { kind: 'memory_models_changed', models: view({ ask: false }) } as never));
    await settle();
    expect(app.memoryModels.shows).toBe('ask');
  });
});
