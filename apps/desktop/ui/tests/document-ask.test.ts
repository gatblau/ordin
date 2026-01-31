// @vitest-environment happy-dom
// Ask Ordin about this on an opened document (SM-ASK-1): the message carries the file, so Ordin's turn
// can name its folder and path. The page is mounted so its button runs.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import DocumentPage from '../src/lib/views/authoring/DocumentPage.svelte';
import { backendWith, WS } from './fixed-support';
import { ok, settle } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const PLACE = '0192f200-0000-7000-8000-000000000001';
const PATH = 'Quarterly plan.pdf';

async function settled(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

describe('SM-ASK-1 — Ask Ordin about an opened document', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-ASK-1
  it('Happy path — sends the text and the file as one place_file attachment', async () => {
    const { state, transport } = await backendWith({
      groups: ['files', 'home', 'ordin', 'craft'],
      setup: (tr) => {
        tr.on('files.preview', ok({ kind: 'none' }))
          .on('files.versions', ok([]))
          .on('home.document_opened', ok({}))
          .on('files.published_copies', ok([]))
          .on('ordin.send_message', ok({ accepted: true, position_in_queue: 0 }));
      },
    });
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(DocumentPage as never, { target, props: { backend: state, workspaceId: WS, instance: `${PLACE}:${PATH}` } as never });
    await settled();
    const ask = [...target.querySelectorAll('button')].find((b) => (b.textContent ?? '').includes('Ask')) as HTMLButtonElement | undefined;
    expect(ask).toBeDefined();
    ask?.click();
    await settled();
    const sent = transport.sentOf('ordin.send_message').map((s) => s.input) as { workspace_id: string; text: string; attachments?: unknown[] }[];
    expect(sent).toHaveLength(1);
    expect(sent[0]?.workspace_id).toBe(WS);
    expect(sent[0]?.text).toContain('Quarterly plan.pdf');
    expect(sent[0]?.attachments).toEqual([{ kind: 'place_file', place_id: PLACE, relative_path: PATH }]);
  });
});
