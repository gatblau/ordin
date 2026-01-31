// @vitest-environment happy-dom
// A PDF drawn on the page (ScreensModule.md SM-PDF-1, amended January 2026, user decision). happy-dom has no
// canvas and no worker, so pdf.js is replaced by a fake document; the parameters pdf.js is opened with are
// checked as they are.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

const pages = { count: 3 };
const drawnText: number[] = [];
vi.mock('../src/lib/blocks/pdf/pdf-loader', async (original) => {
  const real = await original<typeof import('../src/lib/blocks/pdf/pdf-loader')>();
  return {
    ...real,
    openPdf: vi.fn(async () => ({
      numPages: pages.count,
      destroy: vi.fn(async () => undefined),
      getPage: vi.fn(async (n: number) => ({
        getViewport: ({ scale }: { scale: number }) => ({ width: 600 * scale, height: 800 * scale, scale, n }),
        render: () => ({ promise: Promise.resolve() }),
        streamTextContent: () => null,
        cleanup: () => undefined,
      })),
    })),
    drawTextLayer: vi.fn(async (_page: unknown, container: HTMLElement, viewport: { n: number }) => {
      drawnText.push(viewport.n);
      const span = document.createElement('span');
      span.textContent = `Text of page ${viewport.n}`;
      container.append(span);
    }),
  };
});

import PdfView from '../src/lib/blocks/pdf/PdfView.svelte';
import { bundledData, pdfBytes, pdfOptions, PDF_PAGE_LIMIT } from '../src/lib/blocks/pdf/pdf-loader';
import { settle } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
  drawnText.length = 0;
});

const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
async function flush(): Promise<void> {
  for (let i = 0; i < 8; i++) {
    await settle();
    flushSync();
  }
}
function draw(props: Record<string, unknown>): HTMLElement {
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(PdfView as never, { target, props: { bytes: 'JVBERi0xLjQK', label: 'Quarterly plan.pdf', ...props } as never });
  return target;
}

describe('PdfView (SM-PDF-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-PDF-1
  it('Happy path — each page is a canvas with a selectable text layer, in a scrolling column', async () => {
    pages.count = 3;
    const target = draw({});
    expect(clean(target.textContent)).toContain('Opening the PDF…');
    await flush();
    const slots = [...target.querySelectorAll('.page')];
    expect(slots.map((s) => s.getAttribute('aria-label')).map(clean)).toEqual(['Page 1 of 3', 'Page 2 of 3', 'Page 3 of 3']);
    expect(slots.every((s) => s.querySelector('canvas') !== null && s.querySelector('.textLayer span') !== null)).toBe(true);
    expect(target.querySelector('object, embed, iframe')).toBeNull();
    expect(clean(target.textContent)).not.toContain('Open in its app');
  });

  // SM-PDF-1 (amended January 2026; user decision; Approved by the user January 2026): − and + icon buttons.
  const zoomButton = (target: HTMLElement, name: string): HTMLButtonElement =>
    [...target.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.getAttribute('aria-label') === name)!;

  it('Zoom in (+) draws the pages again larger', async () => {
    pages.count = 1;
    const target = draw({});
    await flush();
    expect((target.querySelector('.page') as HTMLElement).style.width).toBe('600px');
    const zoomIn = zoomButton(target, 'Zoom in');
    expect(clean(zoomIn.textContent)).toBe('+');
    expect(zoomIn.title).toBe('Zoom in');
    zoomIn.click();
    await flush();
    expect((target.querySelector('.page') as HTMLElement).style.width).toBe('750px');
    expect(clean(target.textContent)).toContain('125%');
  });

  it('Edge — Zoom out (−) is a small named button that stops at 50%', async () => {
    pages.count = 1;
    const target = draw({});
    await flush();
    const zoomOut = zoomButton(target, 'Zoom out');
    expect(clean(zoomOut.textContent)).toBe('−');
    expect(zoomOut.title).toBe('Zoom out');
    expect([...target.querySelectorAll('button')].some((b) => clean(b.textContent) === 'Zoom out')).toBe(false);
    for (let i = 0; i < 5; i++) {
      zoomOut.click();
      await flush();
    }
    expect(zoomOut.disabled).toBe(true);
    expect(clean(target.textContent)).toContain('50%');
  });

  it(`Edge — a long PDF draws its first ${PDF_PAGE_LIMIT} pages, then a line with Open in its app`, async () => {
    pages.count = 120;
    let opened = 0;
    const target = draw({ onopen: () => opened++ });
    await flush();
    expect(target.querySelectorAll('.page')).toHaveLength(50);
    expect(clean(target.textContent)).toContain('Only the first 50 of 120 pages are shown.');
    ([...target.querySelectorAll('button')].find((b) => clean(b.textContent) === 'Open in its app') as HTMLButtonElement).click();
    expect(opened).toBe(1);
  });

  it('pdf.js is opened with no network, no XFA, no eval and no WebAssembly, and bundled data only', async () => {
    const options = pdfOptions(new Uint8Array([1]));
    expect(options).toMatchObject({ isEvalSupported: false, enableXfa: false, disableRange: true, disableStream: true, disableAutoFetch: true, useWorkerFetch: false, useWasm: false });
    expect(options).not.toHaveProperty('url');
    expect(Array.from(pdfBytes('AQID'))).toEqual([1, 2, 3]);
    await expect(bundledData('wasmUrl', 'openjpeg.wasm')).rejects.toThrow();
    await expect(bundledData('cMapUrl', '../secret')).rejects.toThrow();
    expect((await bundledData('standardFontDataUrl', 'FoxitSymbol.pfb')).length).toBeGreaterThan(1000);
  });
});
