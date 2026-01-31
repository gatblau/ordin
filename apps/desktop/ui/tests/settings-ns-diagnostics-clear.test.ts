// @vitest-environment happy-dom
// SM-NS-3a: Clear logs… beside Save logs for support…, its question, and its two outcomes.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import Diagnostics from '../src/lib/views/Diagnostics.svelte';
import { DiagnosticsModel } from '../src/lib/views/diagnostics-model.svelte';
import { backendWith } from './fixed-support';
import { err, type FakeTransport, ok, R1_GROUPS, settle } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const GROUPS = [...R1_GROUPS, 'diagnostics', 'runs'];

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}
// A button's accessible name: an icon button's aria-label (amended January 2026), else its words.
const buttons = (label: string): HTMLButtonElement[] => [...document.querySelectorAll('button')].filter((b) => (b.getAttribute('aria-label') ?? clean(b.textContent)) === label);
const press = async (label: string): Promise<void> => {
  const found = buttons(label).pop();
  expect(found, `a button named ${label}`).toBeDefined();
  found!.click();
  await flush();
};
const dialog = (): HTMLElement | null => document.querySelector('[role="dialog"]');

async function drawWith(extra: (tr: FakeTransport) => void = () => undefined, now?: () => string) {
  const t = await backendWith({
    groups: GROUPS,
    setup: (tr) => {
      tr.on('diagnostics.lines', ok({ items: [], next_cursor: null }));
      tr.on('runs.list', ok({ items: [], next_cursor: null }));
      extra(tr);
    },
  });
  const model = new DiagnosticsModel(t.handle, now);
  drawn = mount(Diagnostics, { target: document.body, props: { backend: t.state, workspaceId: null, instance: null, model } });
  await flush();
  return { t, model };
}

describe('Clear logs… (SM-NS-3a)', () => {
  it('is drawn beside Save logs for support… on the local backend only', async () => {
    await drawWith();
    const local = [...document.querySelectorAll('.log-actions button')].map((b) => b.getAttribute('aria-label'));
    expect(local).toEqual(['Save logs for support…', 'Clear logs…']);
    // Amended January 2026: icon buttons, each titled by its words. Amended November 2026: the save
    // button's tooltip adds, on a second line, what the file holds.
    for (const b of document.querySelectorAll<HTMLButtonElement>('.log-actions button')) {
      expect(b.title.split('\n')[0]).toBe(b.getAttribute('aria-label'));
      expect(clean(b.textContent)).toBe('');
    }
    void unmount(drawn!);
    document.body.innerHTML = '';
    const server = await backendWith({ groups: GROUPS, kind: 'server', setup: (tr) => tr.on('diagnostics.lines', ok({ items: [], next_cursor: null })) });
    drawn = mount(Diagnostics, { target: document.body, props: { backend: server.state, workspaceId: null, instance: null, model: new DiagnosticsModel(server.handle) } });
    await flush();
    expect(buttons('Clear logs…')).toEqual([]);
    void unmount(drawn);
    document.body.innerHTML = '';
    const noDiagnostics = await backendWith({ groups: R1_GROUPS });
    drawn = mount(Diagnostics, { target: document.body, props: { backend: noDiagnostics.state, workspaceId: null, instance: null, model: new DiagnosticsModel(noDiagnostics.handle) } });
    await flush();
    expect(buttons('Clear logs…')).toEqual([]);
  });

  it('asks first, with its title, its line, Clear logs and Cancel; nothing is sent before the answer', async () => {
    const { t } = await drawWith();
    await press('Clear logs…');
    const asked = dialog();
    expect(asked).not.toBeNull();
    expect(clean(asked!.querySelector('h2')?.textContent)).toBe("Clear ordin's logs?");
    // Amended January 2026: the question says the event list is emptied too.
    expect(clean(asked!.textContent)).toContain("This deletes ordin's log files and empties the list of events below. A file saved for support still includes earlier events.");
    const labels = [...asked!.querySelectorAll('button')].map((b) => clean(b.textContent));
    expect(labels).toEqual(['Clear logs', 'Cancel']);
    // Clear logs removes something, so it is not the default button (the confirmation pattern).
    expect(asked!.querySelector('button.danger')?.textContent?.trim()).toBe('Clear logs');
    expect(t.transport.sentOf('diagnostics.clear_logs')).toEqual([]);
  });

  it('Cancel closes the question and sends nothing', async () => {
    const { t } = await drawWith();
    await press('Clear logs…');
    await press('Cancel');
    expect(dialog()).toBeNull();
    expect(t.transport.sentOf('diagnostics.clear_logs')).toEqual([]);
    expect(clean(document.body.textContent)).not.toContain('Logs cleared.');
  });

  it('Happy path — Clear logs sends diagnostics.clear_logs, closes the question and says so', async () => {
    const { t } = await drawWith((tr) => tr.on('diagnostics.clear_logs', ok({ deleted_files: 13 })));
    await press('Clear logs…');
    await press('Clear logs');
    expect(t.transport.sentOf('diagnostics.clear_logs').map((r) => r.input)).toEqual([{}]);
    expect(dialog()).toBeNull();
    expect(clean(document.body.textContent)).toContain('Logs cleared.');
  });

  it('Error path — DSK_LOGS_CLEAR_FAILED keeps the question open under its sentence, with Try again', async () => {
    const sentence = "ordin couldn't clear all of its logs. Close anything that has a log file open, then try again.";
    const { t } = await drawWith((tr) => tr.on('diagnostics.clear_logs', err('DSK_LOGS_CLEAR_FAILED', sentence, { action: 'try_again' })));
    await press('Clear logs…');
    await press('Clear logs');
    expect(dialog()).not.toBeNull();
    expect(clean(dialog()!.textContent)).toContain(sentence);
    expect(clean(dialog()!.textContent)).not.toContain('DSK_LOGS_CLEAR_FAILED');
    expect(clean(document.body.textContent)).not.toContain('Logs cleared.');
    await press('Try again');
    expect(t.transport.sentOf('diagnostics.clear_logs')).toHaveLength(2);
  });
});

// SM-NS-3a (amended January 2026): what the screen shows is cleared with the logs.
const line = (at: string, summary: string) => ({ at, level: 'info', code: 'RUN_STARTED', summary, reference: 'abcd1234' });
const rows = (): string[] => [...document.querySelectorAll('.journal tbody tr:not(.line-details) .summary')].map((c) => clean(c.textContent));

describe('Clear logs… empties what is on screen (SM-NS-3a, amended January 2026)', () => {
  const CLEARED = '2026-01-10T12:00:00.000Z';

  it('Happy path — the shown lines are emptied and the list is read again', async () => {
    let reads = 0;
    const { t } = await drawWith((tr) => {
      tr.on('diagnostics.lines', (() => {
        reads += 1;
        // The backend lists only what was recorded after the clear (JA-DIAG-CLEAR-1).
        return ok({ items: reads === 1 ? [line('2026-01-10T11:00:00Z', 'Older')] : [line('2026-01-10T12:00:05Z', 'Newer')], next_cursor: null });
      }) as never);
      tr.on('diagnostics.clear_logs', ok({ deleted_files: 2 }));
    }, () => CLEARED);
    expect(rows()).toEqual(['Older']);
    await press('Clear logs…');
    await press('Clear logs');
    expect(t.transport.sentOf('diagnostics.lines')).toHaveLength(2);
    expect(rows()).toEqual(['Newer']);
    expect(clean(document.body.textContent)).toContain('Logs cleared.');
  });

  it('Edge — with nothing written since, the table gives way to "No events to show."', async () => {
    let cleared = false;
    await drawWith((tr) => {
      tr.on('diagnostics.lines', (() => (cleared ? ok({ items: [], next_cursor: null }) : ok({ items: [line('2026-01-10T11:00:00Z', 'Older')], next_cursor: 'c2' }))) as never);
      tr.on('diagnostics.clear_logs', (() => {
        cleared = true;
        return ok({ deleted_files: 0 });
      }) as never);
    }, () => CLEARED);
    expect(buttons('Show more')).toHaveLength(1);
    await press('Clear logs…');
    await press('Clear logs');
    expect(document.querySelector('.journal')).toBeNull();
    expect(clean(document.body.textContent)).toContain('No events to show.');
    expect(buttons('Show more')).toEqual([]);
  });

  it('Error path — a refused clear leaves the shown lines as they were', async () => {
    await drawWith((tr) => {
      tr.on('diagnostics.lines', ok({ items: [line('2026-01-10T11:00:00Z', 'Older')], next_cursor: null }));
      tr.on('diagnostics.clear_logs', err('DSK_LOGS_CLEAR_FAILED', "ordin couldn't clear all of its logs.", { action: 'try_again' }));
    }, () => CLEARED);
    await press('Clear logs…');
    await press('Clear logs');
    expect(rows()).toEqual(['Older']);
  });
});

describe('Journal events as a compact table (SM-NS-3, amended January 2026)', () => {
  it('Happy path — a row shows its reference and opens its code on request, and closes it again', async () => {
    await drawWith((tr) => tr.on('diagnostics.lines', ok({ items: [line('2026-01-10T11:00:00Z', 'A run started')], next_cursor: null })));
    expect([...document.querySelectorAll('.journal th')].map((h) => clean(h.textContent))).toEqual(['Reference', 'Time', 'Level', 'What happened', 'Details']);
    expect(rows()).toEqual(['A run started']);
    expect(clean(document.querySelector('.journal .reference')?.textContent)).toContain('abcd1234');
    expect(clean(document.querySelector('.journal')?.textContent)).not.toContain('RUN_STARTED');
    const more = buttons('Show the details')[0]!;
    expect(more.getAttribute('aria-expanded')).toBe('false');
    expect(more.title).toBe('Show the details');
    more.click();
    await flush();
    const details = clean(document.querySelector('.line-details')?.textContent);
    expect(details).toContain('RUN_STARTED');
    expect(buttons('Hide the details')[0]!.getAttribute('aria-expanded')).toBe('true');
    await press('Hide the details');
    expect(document.querySelector('.line-details')).toBeNull();
  });

  it('Edge — an empty journal says so instead of drawing an empty table', async () => {
    await drawWith();
    expect(document.querySelector('.journal')).toBeNull();
    expect(clean(document.body.textContent)).toContain('No events to show.');
  });
});
