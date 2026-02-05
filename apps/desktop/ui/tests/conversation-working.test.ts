// @vitest-environment happy-dom
// The working line (SM-WK-1), following the newest item (SM-SCR-1) and no way to close an item (SM-CL-1).
// Mounted, so the store's events, the seconds and the scrolling run.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import ConversationPanel from '../src/lib/shell/ConversationPanel.svelte';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import { err, event, LOCAL, ok, settle, type FakeTransport } from './support/fake-backend';
import { uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
  vi.useRealTimers();
});

const WS = uid('ws');
const text = (el: Element | null): string => (el?.textContent ?? '').replace(/[⁨⁩]/g, '').trim();
const message = (body: string) => ({ kind: 'message', text: body, attachments: [] }) as never;
const SINCE = '2026-01-10T10:43:50.000Z';

async function draw(): Promise<{ target: HTMLElement; app: AppContext; transport: FakeTransport; seq: () => number }> {
  const { app, transport } = await appWith((t) =>
    t.on('ordin.list_conversations', ok({ items: [], next_cursor: null })).on('ordin.send_message', ok({ accepted: true, position_in_queue: 0 })),
  );
  app.shell.openWorkspaceTab(LOCAL, WS);
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(ConversationPanel as never, { target, props: {} as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await settle(30);
  let n = 0;
  return { target, app, transport, seq: () => ++n };
}

async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

const line = (target: HTMLElement): Element | null => target.querySelector('[role=status].working-line');

/** Gives the scrolling list a size: `height` of content in a 300 px box, scrolled by the person to `top`. */
function size(el: HTMLElement, height: number, top: number): void {
  grow(el, height);
  Object.defineProperty(el, 'clientHeight', { configurable: true, get: () => 300 });
  el.scrollTop = top;
  el.dispatchEvent(new Event('scroll'));
}

/** The content grows to `height` with no scrolling by the person (no scroll event). */
function grow(el: HTMLElement, height: number): void {
  Object.defineProperty(el, 'scrollHeight', { configurable: true, get: () => height });
}

describe('The working line (SM-WK-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-WK-1
  it('Happy path — the working line comes and goes', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] });
    vi.setSystemTime(new Date(SINCE));
    const { target, transport, seq } = await draw();
    transport.emit(event(LOCAL, seq(), { kind: 'ordin_turn_changed', workspace_id: WS, state: 'working', since: SINCE } as never));
    await flush();
    expect(text(line(target))).toBe('Ordin is working…');
    expect(line(target)?.getAttribute('aria-live')).toBe('polite');
    vi.advanceTimersByTime(12_000);
    flushSync();
    expect(text(line(target))).toBe('Ordin is working… 12 s');
    // Ordin's reply ends it.
    transport.emit(event(LOCAL, seq(), { kind: 'block_shown', workspace_id: WS, block_id: uid('reply'), block: message('Done.'), author: 'ordin' } as never));
    await flush();
    expect(line(target)).toBeNull();
  });

  // spec: ordin-desktop/components/ScreensModule#SM-WK-1
  it('Edge case — the working line names what Ordin is doing; idle removes it', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] });
    vi.setSystemTime(new Date(Date.parse(SINCE) + 3_000));
    const { target, transport, seq } = await draw();
    transport.emit(event(LOCAL, seq(), { kind: 'ordin_turn_changed', workspace_id: WS, state: 'working', doing: 'searching the library', since: SINCE } as never));
    await flush();
    expect(text(line(target))).toBe('Ordin is searching the library… 3 s');
    // The person's own message does not end it.
    transport.emit(event(LOCAL, seq(), { kind: 'block_shown', workspace_id: WS, block_id: uid('mine'), block: message('More?'), author: 'person' } as never));
    await flush();
    expect(line(target)).not.toBeNull();
    transport.emit(event(LOCAL, seq(), { kind: 'ordin_turn_changed', workspace_id: WS, state: 'idle', since: SINCE } as never));
    await flush();
    expect(line(target)).toBeNull();
  });

  // spec: ordin-desktop/components/ScreensModule#SM-WK-1
  it('Edge case — another workspace working draws nothing here', async () => {
    const { target, transport, seq } = await draw();
    transport.emit(event(LOCAL, seq(), { kind: 'ordin_turn_changed', workspace_id: uid('other'), state: 'working', since: SINCE } as never));
    await flush();
    expect(line(target)).toBeNull();
  });
});

describe('The conversation follows the newest item (SM-SCR-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-SCR-1
  it('Happy path — sending scrolls to the newest item', async () => {
    const { target, transport, seq } = await draw();
    const scroller = target.querySelector('.conversation-scroll') as HTMLElement;
    size(scroller, 1000, 0); // scrolled to the top
    const box = target.querySelector('textarea') as HTMLTextAreaElement;
    box.value = 'Summarise the strategy';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();
    const sendButton = [...target.querySelectorAll('button')].find((b) => text(b) === 'Send') as HTMLButtonElement;
    sendButton.click();
    await flush();
    expect(transport.sentOf('ordin.send_message')).toHaveLength(1);
    expect(scroller.scrollTop).toBe(1000);
    // The person's message arrives at the end and stays in view.
    grow(scroller, 1100);
    transport.emit(event(LOCAL, seq(), { kind: 'block_shown', workspace_id: WS, block_id: uid('mine'), block: message('Summarise the strategy'), author: 'person' } as never));
    await flush();
    expect(scroller.scrollTop).toBe(1100);
    expect(target.querySelector('.jump-newest')).toBeNull();
  });

  // spec: ordin-desktop/components/ScreensModule#SM-SCR-1
  it('Edge case — reading earlier items is not interrupted', async () => {
    const { target, transport, seq } = await draw();
    const scroller = target.querySelector('.conversation-scroll') as HTMLElement;
    size(scroller, 1000, 100); // scrolled up to read
    transport.emit(event(LOCAL, seq(), { kind: 'block_shown', workspace_id: WS, block_id: uid('reply'), block: message('Here it is.'), author: 'ordin' } as never));
    await flush();
    expect(scroller.scrollTop).toBe(100);
    const jump = target.querySelector('.jump-newest') as HTMLButtonElement;
    expect(text(jump)).toBe('New messages ↓');
    jump.click();
    await flush();
    expect(scroller.scrollTop).toBe(1000);
    expect(target.querySelector('.jump-newest')).toBeNull();
  });
});

describe('Items cannot be closed (SM-CL-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-CL-1
  it('Happy path — a card carries no close icon, and nothing offers Show again or Show closed items', async () => {
    const { target, app } = await draw();
    const table = { kind: 'table', title: 'Suppliers', columns: [], rows: [] } as never;
    app.local.conversation.set(WS, [
      { block_id: 'm', workspace_id: WS, block: message('Done.'), seq: 1, at: 'x', waiting: false },
      { block_id: 't', workspace_id: WS, block: table, seq: 2, at: 'x', waiting: false },
    ]);
    await flush();
    expect(target.querySelectorAll('button.item-close')).toHaveLength(0);
    expect(target.textContent).not.toContain('Show again');
    expect(target.textContent).not.toContain('Show closed items');
    expect(target.textContent).toContain('Done.');
  });
});

describe('Clear leaves the panel empty (SM-CF-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-CF-1
  it('Happy path — confirming Clear sends start_fresh and the old messages are no longer drawn', async () => {
    const { target, app, transport, seq } = await draw();
    transport.on('ordin.start_fresh', ok({ conversation_id: uid('fresh') }));
    transport.emit(event(LOCAL, seq(), { kind: 'block_shown', workspace_id: WS, block_id: uid('old'), block: message('An old answer.'), author: 'ordin' } as never));
    await flush();
    expect(text(target)).toContain('An old answer.');
    const press = (label: string, within: ParentNode): void => {
      // Clear above the conversation is an icon button named by its label; Clear in the dialog has words.
      const button = [...within.querySelectorAll('button')].find((b) => text(b) === label || b.getAttribute('aria-label') === label);
      expect(button, `a button named ${label}`).toBeDefined();
      button?.click();
    };
    press('Clear', target);
    await flush();
    press('Clear', document.body.querySelector('[role=dialog], dialog') ?? document.body);
    await flush();
    expect(transport.commandsSent()).toContain('ordin.start_fresh');
    expect(text(target)).not.toContain('An old answer.');
    expect(app.local.conversation.get(WS) ?? []).toHaveLength(0);
  });

  it('Error path — a refused Clear keeps the messages and shows the error', async () => {
    const { target, transport, seq } = await draw();
    transport.on('ordin.start_fresh', err('OH_CREW_RUNNING', 'A crew is running.'));
    transport.emit(event(LOCAL, seq(), { kind: 'block_shown', workspace_id: WS, block_id: uid('keep'), block: message('Keep me.'), author: 'ordin' } as never));
    await flush();
    const press = (label: string, within: ParentNode): void => [...within.querySelectorAll('button')].find((b) => text(b) === label || b.getAttribute('aria-label') === label)?.click();
    press('Clear', target);
    await flush();
    press('Clear', document.body.querySelector('[role=dialog], dialog') ?? document.body);
    await flush();
    expect(text(target)).toContain('Keep me.');
    expect(text(target)).toContain('A crew is running.');
  });
});

// A turn that fails before any reply says so (ScreensModule.md, amendment 2026-02-05, SM-RF-1).
describe('A turn that fails before any reply (SM-RF-1)', () => {
  async function sendText(target: HTMLElement, body: string): Promise<void> {
    const box = target.querySelector('textarea') as HTMLTextAreaElement;
    box.value = body;
    box.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    const sendButton = [...target.querySelectorAll('button')].find((b) => text(b) === 'Send') as HTMLButtonElement;
    sendButton.click();
    await flush();
  }
  const failed = (target: HTMLElement) => target.querySelector('.error-line');

  // spec: ordin-desktop/components/ScreensModule#SM-RF-1
  it('Error path — the run fails before a reply', async () => {
    const { target, transport, seq } = await draw();
    await sendText(target, 'who are you?');
    transport.emit(event(LOCAL, seq(), { kind: 'block_shown', workspace_id: WS, block_id: uid('mine'), block: message('who are you?'), author: 'person' } as never));
    transport.emit(event(LOCAL, seq(), { kind: 'ordin_turn_changed', workspace_id: WS, state: 'working', since: SINCE } as never));
    await flush();
    expect(failed(target)).toBeNull();
    transport.emit(event(LOCAL, seq(), { kind: 'ordin_turn_changed', workspace_id: WS, state: 'idle', since: SINCE } as never));
    await flush();
    expect(line(target)).toBeNull();
    expect(text(failed(target))).toContain("Ordin couldn't get an answer this time.");
    const again = [...(failed(target)?.querySelectorAll('button') ?? [])].find((b) => text(b) === 'Try again') as HTMLButtonElement;
    expect(again).toBeDefined();
    again.click();
    await flush();
    expect(transport.sentOf('ordin.send_message').map((c) => (c.input as { text: string }).text)).toEqual(['who are you?', 'who are you?']);
    expect(failed(target)).toBeNull();
  });

  // spec: ordin-desktop/components/ScreensModule#SM-RF-1
  it('Edge case — a failure after a reply draws nothing', async () => {
    const { target, transport, seq } = await draw();
    await sendText(target, 'who are you?');
    transport.emit(event(LOCAL, seq(), { kind: 'ordin_turn_changed', workspace_id: WS, state: 'working', since: SINCE } as never));
    await flush();
    transport.emit(event(LOCAL, seq(), { kind: 'block_shown', workspace_id: WS, block_id: uid('reply'), block: message('I am Ordin.'), author: 'ordin' } as never));
    transport.emit(event(LOCAL, seq(), { kind: 'ordin_turn_changed', workspace_id: WS, state: 'idle', since: SINCE } as never));
    await flush();
    expect(failed(target)).toBeNull();
  });
});
