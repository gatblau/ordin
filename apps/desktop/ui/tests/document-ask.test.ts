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

describe('The document page shows content only (SM-NS-4, SM-ASK-1; amended January 2026, user decision)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-NS-4
  it('draws the name, where it lives and the preview, and no action on the file', async () => {
    const { state, transport } = await backendWith({
      groups: ['files', 'home', 'ordin', 'craft', 'workspaces'],
      setup: (tr) => {
        tr.on('files.preview', ok({ kind: 'none' }))
          .on('files.versions', ok([]))
          .on('home.document_opened', ok({}))
          .on('files.published_copies', ok([]))
          .on('workspaces.links', ok([{ id: 'l1', kind: 'local_folder', display_name: 'Sample documents', state: 'active', workspace_id: WS, added_at: '2026-01-06T09:00:00Z', origin_location: 'Sample documents', place_id: PLACE }]));
      },
    });
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(DocumentPage as never, { target, props: { backend: state, workspaceId: WS, instance: `${PLACE}:Q3/${PATH}` } as never });
    await settled();
    const words = (target.textContent ?? '').replace(/[⁨⁩]/g, '');
    expect(target.querySelector('h2')?.textContent).toBe(PATH);
    expect(words).toContain('In Sample documents › Q3');
    expect(words).toContain("There's no preview of this kind of file.");
    const labels = [...target.querySelectorAll('button, summary')].map((b) => (b.textContent ?? '') + (b.getAttribute('aria-label') ?? ''));
    for (const action of ['Ask', 'Open in its app', 'Move to Bin', 'More actions']) expect(labels.filter((l) => l.includes(action)), action).toEqual([]);
    expect(transport.sentOf('workspaces.links')[0]?.input).toEqual({ workspace_id: WS });
  });
});
