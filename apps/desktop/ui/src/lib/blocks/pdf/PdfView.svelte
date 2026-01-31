<script lang="ts">
  // A PDF drawn in the page (SM-PDF-1, amended January 2026, user decision; Approved by the user January
  // 2026): each page on a canvas with its text selectable over it, the pages one under another in a
  // scrolling column, with Zoom out and Zoom in. The first PDF_PAGE_LIMIT pages are drawn; a longer document
  // ends with `doc-pdf-more-pages` and, where the caller offers it, **Open in its app**. No `object`,
  // `embed` or `iframe` element is drawn and no link or script of the document runs (pdf-loader.ts).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-PDF-1
  import { onDestroy, onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import { drawTextLayer, openPdf, pdfBytes, PDF_PAGE_LIMIT, type PdfDocument } from './pdf-loader';

  interface Props {
    /** The preview's `bytes_b64`. */
    bytes: string;
    /** The file's name, for the pages' accessible names. */
    label: string;
    /** **Open in its app** after the last page drawn of a longer document; absent where it is not offered. */
    onopen?: () => void;
  }
  let { bytes, label, onopen }: Props = $props();

  const ZOOMS = [0.5, 0.75, 1, 1.25, 1.5, 2, 3];
  let zoomAt = $state(2);
  const zoom = $derived(ZOOMS[zoomAt] ?? 1);
  let total = $state(0);
  let phase = $state<'loading' | 'ready' | 'failed'>('loading');
  let pagesEl = $state<HTMLElement | null>(null);
  let doc: PdfDocument | null = null;
  /** Bumped on every redraw, so a redraw that is overtaken stops. */
  let generation = 0;
  let destroyed = false;

  const shown = $derived(Math.min(total, PDF_PAGE_LIMIT));

  onMount(() => {
    void (async () => {
      try {
        doc = await openPdf(pdfBytes(bytes));
        if (destroyed) return void doc.destroy();
        total = doc.numPages;
        phase = 'ready';
      } catch {
        phase = 'failed';
      }
    })();
  });

  onDestroy(() => {
    destroyed = true;
    generation++;
    void doc?.destroy();
  });

  // Draws the pages again whenever they are first ready or the zoom changes.
  $effect(() => {
    const scale = zoom;
    if (phase === 'ready' && pagesEl !== null) void draw(scale, pagesEl);
  });

  async function draw(scale: number, root: HTMLElement): Promise<void> {
    const mine = ++generation;
    const ratio = globalThis.devicePixelRatio || 1;
    for (let number = 1; number <= shown; number++) {
      if (mine !== generation || doc === null) return;
      const slot = root.querySelector<HTMLElement>(`[data-page="${number}"]`);
      if (slot === null) continue;
      try {
        const page = await doc.getPage(number);
        if (mine !== generation) return;
        const viewport = page.getViewport({ scale });
        slot.replaceChildren();
        slot.style.setProperty('--total-scale-factor', String(scale));
        slot.style.width = `${Math.floor(viewport.width)}px`;
        slot.style.height = `${Math.floor(viewport.height)}px`;
        const canvas = document.createElement('canvas');
        canvas.width = Math.floor(viewport.width * ratio);
        canvas.height = Math.floor(viewport.height * ratio);
        canvas.style.width = `${Math.floor(viewport.width)}px`;
        canvas.style.height = `${Math.floor(viewport.height)}px`;
        canvas.setAttribute('aria-hidden', 'true');
        slot.append(canvas);
        // Optional call: a canvas without a 2D context (a test's DOM) draws the text layer only.
        const context = canvas.getContext?.('2d') ?? null;
        if (context !== null) {
          await page.render({ canvas, canvasContext: context, viewport, transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0] } as never).promise;
        }
        const text = document.createElement('div');
        text.className = 'textLayer';
        slot.append(text);
        await drawTextLayer(page, text, viewport);
        page.cleanup();
      } catch {
        // One page that cannot be drawn leaves its slot empty; the others are still drawn.
      }
    }
  }
</script>

<div class="pdf" aria-label={label} role="document">
  {#if phase === 'loading'}
    <p class="secondary" role="status">{t('doc-pdf-loading')}</p>
  {:else if phase === 'failed'}
    <p class="secondary">{t('doc-pdf-failed')}</p>
  {:else}
    <div class="zoom" role="group" aria-label={t('doc-pdf-zoom', { percent: Math.round(zoom * 100) })}>
      <!-- SM-PDF-1 (amended January 2026; user decision; Approved by the user January 2026): small − and +
           buttons in the editor toolbar's style, named Zoom out and Zoom in. -->
      <button type="button" class="zoom-btn" aria-label={t('doc-pdf-zoom-out')} title={t('doc-pdf-zoom-out')} disabled={zoomAt === 0} onclick={() => (zoomAt = Math.max(0, zoomAt - 1))}><span aria-hidden="true">−</span></button>
      <span aria-live="polite">{t('doc-pdf-zoom', { percent: Math.round(zoom * 100) })}</span>
      <button type="button" class="zoom-btn" aria-label={t('doc-pdf-zoom-in')} title={t('doc-pdf-zoom-in')} disabled={zoomAt === ZOOMS.length - 1} onclick={() => (zoomAt = Math.min(ZOOMS.length - 1, zoomAt + 1))}><span aria-hidden="true">+</span></button>
    </div>
    <div class="pages" bind:this={pagesEl}>
      {#each { length: shown } as _, index (index)}
        <div class="page" data-page={index + 1} role="region" aria-label={t('doc-pdf-page', { page: index + 1, total })}></div>
      {/each}
    </div>
    {#if total > PDF_PAGE_LIMIT}
      <p class="secondary more">
        {t('doc-pdf-more-pages', { shown: PDF_PAGE_LIMIT, total })}
        {#if onopen !== undefined}<button type="button" class="link" onclick={() => onopen?.()}>{t('doc-open-in-app')}</button>{/if}
      </p>
    {/if}
  {/if}
</div>

<style>
  .pdf {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .secondary {
    color: var(--color-text-muted);
  }
  .zoom {
    display: flex;
    gap: var(--space-04);
    align-items: center;
  }
  /* The editor toolbar's icon button (MarkdownEditor.svelte `.md-tool`). */
  .zoom-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    padding: 0;
    font-size: 1.125rem;
    line-height: 1;
  }
  .pages {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-12);
    max-block-size: 75vh;
    overflow: auto;
    padding: var(--space-sm);
    background: var(--color-chip);
  }
  .page {
    position: relative;
    flex: none;
    background: white;
    box-shadow: 0 0 0 1px var(--color-line);
    --scale-round-x: 1px;
    --scale-round-y: 1px;
  }
  .page :global(canvas) {
    display: block;
  }
  /* pdf.js's text layer (pdfjs-dist web/pdf_viewer.css, `.textLayer`): transparent text placed over the
     canvas so it can be selected and copied. */
  .page :global(.textLayer) {
    position: absolute;
    text-align: initial;
    inset: 0;
    overflow: clip;
    opacity: 1;
    line-height: 1;
    letter-spacing: normal;
    word-spacing: normal;
    text-size-adjust: none;
    forced-color-adjust: none;
    transform-origin: 0 0;
    caret-color: CanvasText;
    z-index: 0;
    --min-font-size: 1;
    --text-scale-factor: calc(var(--total-scale-factor) * var(--min-font-size));
    --min-font-size-inv: calc(1 / var(--min-font-size));
  }
  .page :global(.textLayer :is(span, br)) {
    color: transparent;
    position: absolute;
    white-space: pre;
    cursor: text;
    transform-origin: 0% 0%;
    user-select: text;
  }
  .page :global(.textLayer > :not(.markedContent)),
  .page :global(.textLayer .markedContent span:not(.markedContent)) {
    z-index: 1;
    --font-height: 0;
    font-size: calc(var(--text-scale-factor) * var(--font-height));
    --scale-x: 1;
    --rotate: 0deg;
    transform: rotate(var(--rotate)) scaleX(var(--scale-x)) scale(var(--min-font-size-inv));
  }
  .page :global(.textLayer .markedContent) {
    display: contents;
  }
  .page :global(.textLayer ::selection) {
    background: color-mix(in srgb, Highlight, transparent 50%);
    color: transparent;
  }
  .page :global(.textLayer br::selection) {
    background: transparent;
  }
  .page :global(.textLayer .endOfContent) {
    display: block;
    position: absolute;
    inset: 100% 0 0;
    z-index: 0;
    cursor: default;
    user-select: none;
  }
  .more {
    display: flex;
    gap: var(--space-sm);
    align-items: baseline;
    flex-wrap: wrap;
  }
</style>
