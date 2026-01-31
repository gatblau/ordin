// The PDF preview's reader (SM-PDF-1, amended January 2026, user decision; Approved by the user January
// 2026). pdf.js (`pdfjs-dist`, Apache-2.0) parses the document in a bundled, same-origin module worker,
// which the window's policy allows (`worker-src 'self' blob:`); no other rule of the policy is changed.
//
// What it is told not to do:
// - no network: the bytes are handed over (`data`); range requests, streaming and auto-fetch are off, and
//   the character maps and standard fonts are read from this bundle (`BundledDataFactory`), never fetched
//   (the policy's `connect-src` names the IPC bridge only);
// - no scripting: no scripting manager is ever created and no annotation layer is drawn, so a PDF's
//   JavaScript and links never run;
// - no XFA forms (`enableXfa: false`);
// - no evaluated code: `isEvalSupported: false` (pdf.js 6 has no such path left; the option is kept as a guard);
// - the legacy build, whose polyfills cover the newer built-ins (`Map.prototype.getOrInsertComputed`) the
//   system WebViews may lack; it loads under the window's frozen `Object.prototype` (pdf-frozen-prototype test);
// - no WebAssembly (`useWasm: false`): the policy has no `wasm-unsafe-eval`, so the image decoders run their
//   JavaScript fallbacks or the image is left out.
//
// spec: ordin-desktop/components/ScreensModule#SM-PDF-1
import type { PDFDocumentProxy, PDFPageProxy, PageViewport } from 'pdfjs-dist/legacy/build/pdf.mjs';

/** Pages drawn at most; a longer document ends with a line and **Open in its app**. */
export const PDF_PAGE_LIMIT = 50;

/** A preview's `bytes_b64`, decoded the way the other previews' bytes are (base64 from `files.preview`). */
export function pdfBytes(base64: string): Uint8Array {
  return Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
}

// The character maps and standard fonts, each its own lazily loaded chunk holding the file as a data URL
// (vite.config.ts `assetsInclude`), decoded here without a fetch.
const CMAPS = import.meta.glob('../../../../node_modules/pdfjs-dist/cmaps/*.bcmap', { query: '?inline', import: 'default' }) as Record<string, () => Promise<string>>;
const FONTS = import.meta.glob('../../../../node_modules/pdfjs-dist/standard_fonts/*.{pfb,ttf}', { query: '?inline', import: 'default' }) as Record<string, () => Promise<string>>;

function dataUrlBytes(url: string): Uint8Array {
  const comma = url.indexOf(',');
  return pdfBytes(url.slice(comma + 1));
}

/** Finds a bundled file by its name; anything else is refused, so no other kind of data is ever loaded. */
export async function bundledData(kind: string, filename: string): Promise<Uint8Array> {
  const table = kind === 'cMapUrl' ? CMAPS : kind === 'standardFontDataUrl' ? FONTS : null;
  if (table === null || !/^[A-Za-z0-9._-]+$/.test(filename)) throw new Error(`pdf: no bundled ${kind} data`);
  const entry = Object.entries(table).find(([path]) => path.endsWith(`/${filename}`));
  if (entry === undefined) throw new Error(`pdf: no bundled ${kind} data for ${filename}`);
  return dataUrlBytes(await entry[1]());
}

/**
 * pdf.js's data factory, reading from the bundle (the `BinaryDataFactory` parameter). pdf.js calls its method
 * by the name `fetch`; it is written as a computed name because nothing here makes a network request.
 */
class BundledDataFactory {
  constructor(_options: unknown) {}
  ['fetch']({ kind, filename }: { kind: string; filename: string }): Promise<Uint8Array> {
    return bundledData(kind, filename);
  }
}

/** The parameters every document is opened with. */
export function pdfOptions(data: Uint8Array): Record<string, unknown> {
  return {
    data,
    isEvalSupported: false,
    enableXfa: false,
    disableRange: true,
    disableStream: true,
    disableAutoFetch: true,
    useWorkerFetch: false,
    useWasm: false,
    useSystemFonts: false,
    cMapPacked: true,
    BinaryDataFactory: BundledDataFactory,
    stopAtErrors: false,
  };
}

type PdfJs = typeof import('pdfjs-dist/legacy/build/pdf.mjs');
let loaded: Promise<PdfJs> | null = null;

/** pdf.js, loaded on first use, with its worker started once. */
function pdfjs(): Promise<PdfJs> {
  loaded ??= import('pdfjs-dist/legacy/build/pdf.mjs').then((lib) => {
    const worker = new Worker(new URL('pdfjs-dist/legacy/build/pdf.worker.min.mjs', import.meta.url), { type: 'module' });
    lib.GlobalWorkerOptions.workerPort = worker;
    return lib;
  });
  return loaded;
}

export type PdfPage = Pick<PDFPageProxy, 'getViewport' | 'render' | 'streamTextContent' | 'cleanup'>;
export interface PdfDocument {
  numPages: number;
  getPage(number: number): Promise<PdfPage>;
  /** Ends the document's work in the worker. */
  destroy(): Promise<void>;
}

export async function openPdf(data: Uint8Array): Promise<PdfDocument> {
  const lib = await pdfjs();
  const task = lib.getDocument(pdfOptions(data) as never);
  const doc: PDFDocumentProxy = await task.promise;
  return { numPages: doc.numPages, getPage: (n) => doc.getPage(n), destroy: () => task.destroy() };
}

/** The selectable text over a drawn page. */
export async function drawTextLayer(page: PdfPage, container: HTMLElement, viewport: PageViewport): Promise<void> {
  const lib = await pdfjs();
  await new lib.TextLayer({ textContentSource: page.streamTextContent(), container, viewport }).render();
}
