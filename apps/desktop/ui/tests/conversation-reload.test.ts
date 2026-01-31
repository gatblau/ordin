// @vitest-environment happy-dom
// Load and reload (ScreensModule.md step 5): an open conversation tab reads `ordin.read_conversation`,
// so what was said before a restart is drawn again. Mounted so the panel's reading runs.
import { mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import ConversationPanel from '../src/lib/shell/ConversationPanel.svelte';
import { APP_KEY } from '../src/lib/shell/context';
import { LOCAL, ok, settle, type FakeTransport } from './support/fake-backend';
import { uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const WS = uid('ws');
const LIVE = uid('live');
const BLOCK = uid('block');
const text = (el: Element): string => (el.textContent ?? '').replace(/[⁨⁩]/g, '');

async function draw(setup: (t: FakeTransport) => void): Promise<{ target: HTMLElement; transport: FakeTransport }> {
  const { app, transport } = await appWith(setup);
  app.shell.openWorkspaceTab(LOCAL, WS);
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(ConversationPanel as never, { target, props: {} as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await settle(30);
  return { target, transport };
}

describe('An open conversation tab is read at start (step 5)', () => {
  it('Happy path — the messages kept before a restart are drawn again, read once', async () => {
    const { target, transport } = await draw((t) =>
      t
        .on('ordin.list_conversations', ok({ items: [{ conversation_id: LIVE, first_request: 'Please summarise slowly', message_count: 2, started_at: '2026-01-10T10:00:00Z', state: 'current' }], next_cursor: null }))
        .on('ordin.read_conversation', ok({ items: [{ seq: 1, at: '2026-01-10T10:00:00Z', author: 'ordin', text: 'x', block_id: BLOCK }], next_cursor: null }))
        .on('ordin.block', ok({ conversation_id: LIVE, block: { kind: 'message', text: "I've given the meeting notes to the document agent.", attachments: [] }, block_id: BLOCK, waiting: false, from: 'ordin', at: '2026-01-10T10:00:00Z' })),
    );
    expect(transport.sentOf('ordin.read_conversation').map((e) => e.input)).toEqual([{ conversation_id: LIVE }]);
    expect(text(target)).toContain("I've given the meeting notes to the document agent.");
  });

  it('Edge case — a workspace with no current conversation draws nothing and reads no messages', async () => {
    const { target, transport } = await draw((t) => t.on('ordin.list_conversations', ok({ items: [], next_cursor: null })));
    expect(transport.sentOf('ordin.read_conversation')).toHaveLength(0);
    expect(target.querySelectorAll('[data-region=conversation] li, ul > li')).toHaveLength(0);
  });
});
