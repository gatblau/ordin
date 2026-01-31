// @vitest-environment happy-dom
// An earlier conversation can be deleted from the list and from its own screen (SM-WR-16, SM-WR-17).
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { APP_KEY } from '../src/lib/shell/context';
import EarlierConversation from '../src/lib/views/EarlierConversation.svelte';
import EarlierConversations from '../src/lib/views/EarlierConversations.svelte';
import { err, ok, R1_GROUPS, hello, LOCAL, settle } from './support/fake-backend';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const A = '0192a1c2-3344-7556-8677-8899aabbccd1';
const B = '0192a1c2-3344-7556-8677-8899aabbccd2';
const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const conv = (id: string, first: string) => ({ conversation_id: id, first_request: first, message_count: 2, started_at: '2026-01-10T10:00:00Z', state: 'earlier' });

async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}
const press = (label: string): void => {
  const button = [...document.querySelectorAll('button')].filter((b) => clean(b.textContent) === label).pop();
  expect(button, `a button named ${label}`).toBeDefined();
  button?.click();
};

async function draw(component: unknown, props: Record<string, unknown>, del: unknown = ok({})) {
  let listed = 0;
  const { app, transport } = await appWith((t) =>
    t
      .on('service.hello', ok(hello(LOCAL, R1_GROUPS)))
      .on('ordin.list_conversations', (() => (listed++ === 0 ? ok({ items: [conv(A, 'Q3 supplier rates'), conv(B, 'Account brief')], next_cursor: null }) : ok({ items: [conv(B, 'Account brief')], next_cursor: null }))) as never)
      .on('ordin.read_conversation', ok({ items: [], next_cursor: null }))
      .on('ordin.delete_conversation', del as never)
      .on('ordin.ask_about_conversation', ok({}))
      .on('ordin.continue_conversation', ok({ conversation_id: B })),
  );
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(component as never, { target, props: { backend: app.local, workspaceId: WS, ...props } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await flush();
  return { app, target, transport };
}

describe('Delete… on the Earlier conversations list (SM-WR-16)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-WR-16
  it('Happy path — ⋯, Delete…, confirm: ordin.delete_conversation is sent with the row\'s id and the list is read again', async () => {
    const { target, transport } = await draw(EarlierConversations, { instance: null });
    expect(clean(target.textContent)).toContain('Q3 supplier rates');
    document.querySelector<HTMLElement>('button[aria-haspopup="menu"][aria-label*="Q3 supplier rates"]')?.click();
    await flush();
    press('Delete…');
    await flush();
    expect(clean(document.body.textContent)).toContain('Delete “Q3 supplier rates”?');
    expect(transport.sentOf('ordin.delete_conversation')).toHaveLength(0);
    press('Delete');
    await flush();
    expect(transport.sentOf('ordin.delete_conversation').map((c) => c.input)).toEqual([{ conversation_id: A }]);
    expect(clean(target.textContent)).not.toContain('Q3 supplier rates');
    expect(clean(target.textContent)).toContain('Account brief');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-WR-16
  it('Edge case — Cancel sends nothing', async () => {
    const { target, transport } = await draw(EarlierConversations, { instance: null });
    document.querySelector<HTMLElement>('button[aria-haspopup="menu"][aria-label*="Account brief"]')?.click();
    await flush();
    press('Delete…');
    await flush();
    press('Cancel');
    await flush();
    expect(transport.sentOf('ordin.delete_conversation')).toHaveLength(0);
    expect(clean(target.textContent)).toContain('Account brief');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-WR-16
  it('Error path — a refusal is drawn in the dialog, which stays open', async () => {
    const { target } = await draw(EarlierConversations, { instance: null }, err('OH_CONVERSATION_NOT_DELETABLE', 'Only an earlier conversation that no later one continues can be deleted.', {}));
    document.querySelector<HTMLElement>('button[aria-haspopup="menu"][aria-label*="Q3 supplier rates"]')?.click();
    await flush();
    press('Delete…');
    await flush();
    press('Delete');
    await flush();
    expect(clean(document.body.textContent)).toContain('Only an earlier conversation that no later one continues can be deleted.');
    expect(clean(document.body.textContent)).toContain('Delete “Q3 supplier rates”?');
    expect(clean(target.textContent)).toContain('Q3 supplier rates');
  });
});

describe('the row\'s menu on the Earlier conversations list', () => {
  const menuItems = (): string[] => [...document.querySelectorAll('[role="menu"] [role="menuitem"]')].map((b) => clean(b.textContent));

  it('⋯ and right-click open the same menu: Ask Ordin about it, Continue this conversation, Delete…', async () => {
    await draw(EarlierConversations, { instance: null });
    document.querySelector<HTMLElement>('button[aria-haspopup="menu"][aria-label*="Q3 supplier rates"]')?.click();
    await flush();
    expect(menuItems()).toEqual(['Ask Ordin about it', 'Continue this conversation', 'Delete…']);
    document.querySelector<HTMLElement>('button[aria-haspopup="menu"][aria-label*="Q3 supplier rates"]')?.click();
    await flush();
    expect(menuItems()).toEqual([]);
    const row = [...document.querySelectorAll<HTMLElement>('button.row')].find((b) => clean(b.textContent).includes('Account brief'));
    row?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await flush();
    expect(menuItems()).toEqual(['Ask Ordin about it', 'Continue this conversation', 'Delete…']);
  });

  it('Ask Ordin about it asks under the row and sends the question without opening a page', async () => {
    const { app, transport } = await draw(EarlierConversations, { instance: null });
    document.querySelector<HTMLElement>('button[aria-haspopup="menu"][aria-label*="Q3 supplier rates"]')?.click();
    await flush();
    press('Ask Ordin about it');
    await flush();
    const input = document.querySelector<HTMLInputElement>('form.ask input');
    expect(input).not.toBeNull();
    if (input !== null) {
      input.value = 'What rates did we agree?';
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    document.querySelector<HTMLFormElement>('form.ask')?.requestSubmit();
    await flush();
    expect(transport.sentOf('ordin.ask_about_conversation').map((c) => c.input)).toEqual([{ workspace_id: WS, conversation_id: A, question: 'What rates did we agree?' }]);
    expect(document.querySelector('form.ask')).toBeNull();
    expect(app.screens.tabs(ScreenAreaState.scope(app.local.id, WS)).some((tab) => tab.key.startsWith('earlier-conversation'))).toBe(false);
  });

  it('Continue this conversation sends at once when the live conversation is empty', async () => {
    const { transport } = await draw(EarlierConversations, { instance: null });
    document.querySelector<HTMLElement>('button[aria-haspopup="menu"][aria-label*="Account brief"]')?.click();
    await flush();
    press('Continue this conversation');
    await flush();
    expect(transport.sentOf('ordin.continue_conversation').map((c) => c.input)).toEqual([{ workspace_id: WS, conversation_id: B, kept_points: [] }]);
  });
});

describe('Delete… on the open earlier conversation (SM-WR-17)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-WR-17
  it('Happy path — confirming sends the command and closes the tab', async () => {
    const { app, transport } = await draw(EarlierConversation, { instance: A });
    const scope = ScreenAreaState.scope(app.local.id, WS);
    app.screens.open(scope, `earlier-conversation:${A}`);
    press('Delete…');
    await flush();
    press('Delete');
    await flush();
    expect(transport.sentOf('ordin.delete_conversation').map((c) => c.input)).toEqual([{ conversation_id: A }]);
    expect(app.screens.tabs(scope).some((tab) => tab.key.startsWith('earlier-conversation'))).toBe(false);
  });
});
