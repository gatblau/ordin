// SM-NS-4: the document page and its editor.
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { DocumentController, originWords } from '../src/lib/views/authoring/document.svelte';
import DocumentPage from '../src/lib/views/authoring/DocumentPage.svelte';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));

const PLACE = '0192f200-0000-7000-8000-000000000001';
const PATH = 'Rates/rates.md';
const version = (n: number, origin: unknown = { kind: 'person' }) => ({ id: uid(`v${n}`), version: n, created_at: `2026-10-0${n}T10:00:00Z`, hash: 'h', path: PATH, place_id: PLACE, origin });
const markdown = { kind: 'text', markdown: true, text: '# Rates\n\nOld line', truncated: false };

async function page(over: { preview?: unknown; extra?: (t: import('./support/fake-backend').FakeTransport) => void } = {}) {
  const closed = vi.fn();
  const made = await backendWith({
    setup: (tr) => {
      tr.on('files.preview', ok(over.preview ?? markdown))
        .on('files.versions', ok([version(1), version(3, { kind: 'run', run_id: 'r', tool: 'x' }), version(2, { kind: 'art_craft_edit', app: 'Pages' })]))
        .on('home.document_opened', ok({}))
        .on('files.published_copies', ok([]));
      over.extra?.(tr);
    },
  });
  const c = new DocumentController(made.handle, PLACE, PATH, WS, closed);
  await c.load();
  return { ...made, c, closed };
}

describe('SM-NS-4 — the document page', () => {
  it('On opening it sends files.preview, files.versions and home.document_opened; the heading is the last segment of the path', async () => {
    const { transport, c } = await page();
    expect(transport.commandsSent()).toEqual(expect.arrayContaining(['files.preview', 'files.versions', 'home.document_opened']));
    expect(transport.sentOf('files.preview')[0]?.input).toEqual({ place_id: PLACE, path: PATH });
    expect(transport.sentOf('home.document_opened')[0]?.input).toEqual({ item: { kind: 'place_file', place_id: PLACE, relative_path: PATH }, workspace_id: WS });
    expect(c.name).toBe('rates.md');
  });

  it('At a glance names the newest version and who changed it; versions run newest first', async () => {
    const { c } = await page();
    expect(c.newest?.version).toBe(3);
    expect(stripIsolates(c.glance ?? '')).toMatch(/^Changed .* · changed by an agent$/);
    expect(c.ordered.map((v) => v.version)).toEqual([3, 2, 1]);
    expect(stripIsolates(originWords({ kind: 'art_craft_edit', app: 'Pages' }))).toBe('changed in Pages');
    expect(originWords({ kind: 'person' })).toBe('saved by you');
  });

  it('Published as is read for a Markdown document only', async () => {
    const md = await page();
    expect(md.transport.sentOf('files.published_copies')[0]?.input).toEqual({ original: { kind: 'place_file', place_id: PLACE, path: PATH } });
    const plain = await page({ preview: { kind: 'text', markdown: false, text: 'x', truncated: false } });
    expect(plain.transport.sentOf('files.published_copies')).toHaveLength(0);
  });

  it('Edit is offered on a Markdown preview that is not cut, and not on a cut one', async () => {
    expect((await page()).c.canEdit).toBe(true);
    expect((await page({ preview: { ...markdown, truncated: true } })).c.canEdit).toBe(false);
    expect((await page({ preview: { kind: 'text', markdown: false, text: 'x', truncated: false } })).c.canEdit).toBe(false);
  });

  it('no_editor asks the question; Open in your usual app sends craft.open_with_system_default', async () => {
    const { c, transport } = await page({ extra: (tr) => tr.on('craft.open', ok({ kind: 'no_editor' })).on('craft.open_with_system_default', ok({ kind: 'opened_with_system_default' })) });
    await c.openInApp();
    expect(c.openState).toBe('no_editor');
    await c.openUsualApp();
    expect(c.openState).toBe('idle');
    expect(transport.sentOf('craft.open_with_system_default')[0]?.input).toEqual({ place_id: PLACE, path: PATH });
  });

  it('Restore sends files.restore_version for the chosen version and reads the page again', async () => {
    const { c, transport } = await page({ extra: (tr) => tr.on('files.restore_version', ok(version(4))) });
    await c.restore(c.ordered[1]!);
    expect(transport.sentOf('files.restore_version')[0]?.input).toEqual({ file_version_id: uid('v2') });
    expect(transport.sentOf('files.versions')).toHaveLength(2);
  });

  it('Happy path — editing a Markdown document (SM-NS-4)', async () => {
    const { c, transport } = await page({ extra: (tr) => tr.on('files.save_edit', ok(version(4))) });
    c.startEdit();
    c.draft = '# Rates\n\nNew line';
    expect(c.dirty).toBe(true);
    expect(await c.save()).toBe(true);
    expect(transport.sentOf('files.save_edit')[0]?.input).toEqual({ place_id: PLACE, path: PATH, text: '# Rates\n\nNew line' });
    expect(c.savedNotSent).toBe(true);
    expect(c.editing).toBe(false);
    expect(t('doc-saved-not-sent')).toBe('Changed by you — not yet sent back');
  });

  it('Error path — the document changed meanwhile (SM-NS-4)', async () => {
    const { c } = await page({ extra: (tr) => tr.on('files.save_edit', err('FILE_CHANGED_MEANWHILE', 'This document changed somewhere else while you were editing it.')) });
    c.startEdit();
    c.draft = 'typed text';
    expect(await c.save()).toBe(false);
    expect(c.saveError?.message).toBe('This document changed somewhere else while you were editing it.');
    expect(c.editing).toBe(true);
    expect(c.draft).toBe('typed text');
  });

  it('Cancel with changes asks on the bar; Keep changing keeps the editor; Discard changes closes it', async () => {
    const { c } = await page();
    c.startEdit();
    c.cancel();
    expect(c.editing).toBe(false);
    c.startEdit();
    c.draft = 'changed';
    c.cancel();
    expect(c.askingDiscard).toBe(true);
    c.keepChanging();
    expect(c.editing && !c.askingDiscard).toBe(true);
    c.cancel();
    c.discardChanges();
    expect(c.editing).toBe(false);
  });

  it('Move to Bin sends files.move_to_bin and closes the tab on ok; a refusal draws its sentence and the tab stays', async () => {
    const good = await page({ extra: (tr) => tr.on('files.move_to_bin', ok({ file_version_id: 'v', path: PATH, place_id: PLACE })) });
    expect(await good.c.moveToBin()).toBe(true);
    expect(good.transport.sentOf('files.move_to_bin')[0]?.input).toEqual({ place_id: PLACE, path: PATH });
    expect(good.closed).toHaveBeenCalledOnce();
    const bad = await page({ extra: (tr) => tr.on('files.move_to_bin', err('FILE_NOT_ON_THIS_COMPUTER', 'This document is not on this computer.')) });
    expect(await bad.c.moveToBin()).toBe(false);
    expect(bad.c.error?.message).toBe('This document is not on this computer.');
    expect(bad.closed).not.toHaveBeenCalled();
  });

  // Amended January 2026 (user decision): Move to Bin… is in the document's ⋯ menu in the tree, not on the page.
  it('the page draws its heading and where it lives, no Move to Bin; Send back… and Publish a copy… are not drawn (G-CU-3)', async () => {
    const { state } = await backendWith({ groups: ['files', 'home', 'ordin', 'craft'] });
    const markup = text(html(DocumentPage, { backend: state, workspaceId: WS, instance: `${PLACE}:${PATH}` }));
    expect(markup).toContain('rates.md');
    expect(markup).toContain('In Rates');
    expect(markup).not.toContain(t('doc-move-to-bin'));
    expect(markup).not.toMatch(/Send back|Publish a copy/);
  });
});
