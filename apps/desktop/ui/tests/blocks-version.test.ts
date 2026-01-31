// SM-SF-1, SM-WR-1: the exact version an approval covers. The wiring file is replaced here so the reads can
// answer `ok`, `not_wired` and an error.
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import type { CommandOutcome } from '../src/lib/backend/backend';
import type { NotWired, VersionContent } from '../src/lib/backend/wiring';

const read = vi.fn<(file_version_id: string) => Promise<CommandOutcome<VersionContent> | NotWired>>();
vi.mock('../src/lib/backend/wiring', async (original) => ({
  ...(await original<typeof import('../src/lib/backend/wiring')>()),
  readFileVersion: (_b: unknown, id: string) => read(id),
}));

import ApprovalScreen from '../src/lib/approval/ApprovalScreen.svelte';
import ResolvedView from '../src/lib/blocks/ResolvedView.svelte';
import TableView from '../src/lib/blocks/TableView.svelte';
import { resolveRef } from '../src/lib/blocks/refs';
import type { ApprovalCard } from '../src/lib/service-api/types.gen';
import { approvalCard, err, ok } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
beforeEach(() => read.mockReset());

const VERSION = '0192a1c4-0000-7000-8000-0000000000aa';
const ref = { kind: 'file_version', file_version_id: VERSION } as const;
const text = (markup: string) => markup.replace(/<!--[\s\S]*?-->/g, '').replace(/<style[\s\S]*?<\/style>/g, '').replace(/<[^>]*>/g, ' ').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');
const buttons = (markup: string) => [...markup.matchAll(/<button[^>]*>[\s\S]*?<\/button>/g)].map((m) => m[0]);

describe('Content view: a card whose files hold { kind: file_version } calls readFileVersion and never files.preview', () => {
  it('resolves through readFileVersion on the card\'s backend and sends no files.preview', async () => {
    const { app, transport } = await appWith();
    read.mockResolvedValue(ok({ path: 'Quotes/Quote — Kestrel.md', preview: { kind: 'text', markdown: false, text: 'Rates: £412', truncated: false } }));
    const resolved = await resolveRef(app.local, ref);
    expect(read).toHaveBeenCalledWith(VERSION);
    expect(transport.sentOf('files.preview')).toEqual([]);
    expect(resolved).toEqual({ kind: 'ok', name: 'Quote — Kestrel.md', preview: { kind: 'text', markdown: false, text: 'Rates: £412', truncated: false } });
  });

  it('a place file is read with files.preview (the file as it is); an artefact waits for a backend answering artefacts.get', async () => {
    const { app, transport } = await appWith();
    transport.on('files.preview', ok({ kind: 'text', markdown: false, text: 'hello', truncated: false }));
    const resolved = await resolveRef(app.local, { kind: 'place_file', place_id: uid('p1'), relative_path: 'a/b.txt' });
    expect(resolved).toMatchObject({ kind: 'ok', name: 'b.txt' });
    expect(transport.sentOf('files.preview')[0]?.input).toEqual({ place_id: uid('p1'), path: 'a/b.txt' });
    expect(await resolveRef(app.local, { kind: 'artefact', artefact_id: 'a1' })).toEqual({ kind: 'not_wired' });
    expect(read).not.toHaveBeenCalled();
  });
});

describe('Happy path — an approval shows the exact version it covers', () => {
  it('the heading reads "Quote — Kestrel.md — the version this approval covers", then the note, then the text of that version', async () => {
    const { app } = await appWith();
    const resolved = { kind: 'ok' as const, name: 'Quote — Kestrel.md', preview: { kind: 'text' as const, markdown: false, text: 'Rates: £412', truncated: false } };
    const out = html(ResolvedView, { resolved, bound: true, backend: app.local.handle }, app);
    expect(out.replace(/[\u2068\u2069]/g, '')).toMatch(/<h2[^>]*tabindex="-1"[^>]*>Quote — Kestrel\.md — the version this approval covers<\/h2>/);
    const t = text(out);
    expect(t.indexOf('the version this approval covers')).toBeLessThan(t.indexOf('Changes made to this file after the card was raised are not included.'));
    expect(t.indexOf('Changes made to this file after the card was raised are not included.')).toBeLessThan(t.indexOf('Rates: £412'));
    expect(t).not.toContain('Only the start of this file is shown.');
  });

  it('a text preview with truncated: true ends with "Only the start of this file is shown."', async () => {
    const { app } = await appWith();
    const out = html(ResolvedView, { resolved: { kind: 'ok', name: 'Q.md', preview: { kind: 'text', markdown: false, text: 'start', truncated: true } }, bound: true }, app);
    expect(text(out).endsWith('Only the start of this file is shown. ')).toBe(true);
  });

  it('a preview text holding <img src=x onerror=alert(1)> is drawn literally, plain or as Markdown', async () => {
    const { app } = await appWith();
    for (const markdown of [false, true]) {
      const out = html(ResolvedView, { resolved: { kind: 'ok', name: 'Q.md', preview: { kind: 'text', markdown, text: '<img src=x onerror=alert(1)>', truncated: false } }, bound: true }, app);
      expect(out, String(markdown)).toContain('&lt;img src=x onerror=alert(1)>');
      expect(out, String(markdown)).not.toMatch(/<img/);
    }
  });

  it('an image is drawn from its bytes with the file name as its text alternative; "none" draws nothing', async () => {
    const { app } = await appWith();
    const image = html(ResolvedView, { resolved: { kind: 'ok', name: 'Logo.png', preview: { kind: 'image', media_type: 'image/png', bytes_b64: 'iVBORw0KGgo=' } } }, app);
    expect(image).toMatch(/<img[^>]*src="data:image\/png;base64,iVBORw0KGgo="[^>]*alt="Logo\.png"/);
    const svg = html(ResolvedView, { resolved: { kind: 'ok', name: 'x.svg', preview: { kind: 'image', media_type: 'image/svg+xml', bytes_b64: 'PHN2Zz4=' } } }, app);
    expect(svg).not.toMatch(/<img/);
    expect(text(html(ResolvedView, { resolved: { kind: 'ok', name: 'x', preview: { kind: 'none' } } }, app)).trim()).toBe('');
  });
});

describe('Error path — the version cannot be shown', () => {
  const unavailable = "ordin can't show the exact version this approval covers. Reject the card if you need to see it first.";

  it('not_wired draws the unavailable sentence in place of the preview', async () => {
    const { app } = await appWith();
    read.mockResolvedValue({ kind: 'not_wired' });
    const resolved = await resolveRef(app.local, ref);
    expect(resolved).toEqual({ kind: 'not_wired' });
    expect(text(html(ResolvedView, { resolved, bound: true }, app))).toContain(unavailable);
  });

  it('an error reply draws the same sentence, and its own sentence under it', async () => {
    const { app } = await appWith();
    read.mockResolvedValue(err('FILES_VERSION_NOT_FOUND', 'That version is gone.'));
    const resolved = await resolveRef(app.local, ref);
    const out = text(html(ResolvedView, { resolved, bound: true, backend: app.local.handle }, app));
    expect(out).toContain(unavailable);
    expect(out.indexOf(unavailable)).toBeLessThan(out.indexOf('That version is gone.'));
    expect(out).not.toContain('FILES_VERSION_NOT_FOUND');
  });

  it("Approve is held while the version cannot be shown (SM-SF-1.4); Reject stays enabled", async () => {
    const card = approvalCard({ content: { kind: 'files', label: 'See the files', files: [ref] } }) as unknown as ApprovalCard;
    const { app } = await appWith();
    app.local.pendingApprovals.set(card.approval_id, card);
    const out = html(ApprovalScreen, { backend: app.local, workspaceId: card.workspace_id, instance: card.approval_id }, app);
    const [files, approve, reject] = buttons(out);
    expect(text(files ?? '')).toContain('See the files');
    expect(approve ?? '').toContain('disabled');
    expect(reject ?? '').not.toContain('disabled');
  });
});

describe('a file_version reference outside an approval card (SM-SF-1.3)', () => {
  it('draws the same preview with no heading and no note', async () => {
    const { app } = await appWith();
    const out = html(ResolvedView, { resolved: { kind: 'ok', name: 'Quote.md', preview: { kind: 'text', markdown: false, text: 'Rates', truncated: true } }, bound: false }, app);
    expect(text(out)).toContain('Rates');
    expect(text(out)).not.toContain('the version this approval covers');
    expect(text(out)).not.toContain('Changes made to this file');
    expect(text(out)).not.toContain('Only the start');
  });

  it('a table cell holding one is a button that opens that preview', async () => {
    const { app } = await appWith();
    const out = html(
      TableView,
      {
        backend: app.local,
        title: 'Quotes',
        columns: [{ name: 'File', type: 'document_link' }],
        rows: [[ref]],
        source: { kind: 'earlier_result', run_id: 'r' },
      },
      app,
    );
    expect(buttons(out).some((b) => text(b).includes('A file'))).toBe(true);
  });
});
