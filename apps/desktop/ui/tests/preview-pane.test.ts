// A `Preview` as the document page draws it (SM-NS-4.3), and a PDF read in its own app (SM-PDF-1): the
// window's security policy has no embedded viewer, so a PDF is never drawn in the page.
import { beforeAll, describe, expect, it } from 'vitest';

import PreviewPane from '../src/lib/blocks/PreviewPane.svelte';
import { html, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const text = (markup: string): string => markup.replace(/<[^>]*>/g, ' ').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');

describe('PreviewPane', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-PDF-1
  it('Edge case — a PDF says it opens in its own app and draws no embedded viewer (SM-PDF-1)', () => {
    const out = html(PreviewPane, { preview: { kind: 'pdf', bytes_b64: 'JVBERi0xLjQK' }, label: 'Quarterly plan.pdf' });
    expect(text(out)).toContain('PDFs open in their own app. Use Open in its app to read this one.');
    expect(out).not.toMatch(/<(object|embed|iframe)\b/);
    expect(out).not.toContain('JVBERi0xLjQK');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-PV-1
  it('Edge case — a kind with no preview says so on the document page, never an empty page (SM-PV-1)', () => {
    const out = html(PreviewPane, { preview: { kind: 'none' }, label: 'Old terms.doc', onDocumentPage: true });
    expect(text(out)).toContain("There's no preview of this kind of file. It opens in its own app.");
  });

  // spec: ordin-desktop/components/ScreensModule#SM-PV-1
  it('Edge case — elsewhere, a kind with no preview still draws nothing (SM-SF-1.1, SM-PV-1)', () => {
    const out = html(PreviewPane, { preview: { kind: 'none' }, label: 'Old terms.doc' });
    expect(text(out).trim()).toBe('');
  });

  it('text is drawn as text', () => {
    const out = html(PreviewPane, { preview: { kind: 'text', text: 'Rates go up in Q3.', markdown: false, truncated: false }, label: 'notes.txt' });
    expect(text(out)).toContain('Rates go up in Q3.');
  });
});
