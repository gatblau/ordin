<script lang="ts">
  // spec: ordin-desktop/components/ScreensModule#SM-PDF-1
  // spec: ordin-desktop/components/ScreensModule#SM-PV-1
  // A `Preview` from the core drawn as the document page draws a `files.preview` result: text as text
  // nodes, an image from its bytes, a PDF page by page (SM-PDF-1, amended January 2026, user decision;
  // Approved by the user January 2026), a thumbnail; for `none`, nothing (SM-SF-1.1), or on the document
  // page a line saying so (SM-PV-1). The pane draws content only: actions on the document live in its ⋯ menu.
  import { t } from '../i18n/fluent';
  import type { Preview } from '../service-api/types.gen';
  import MarkdownView from './MarkdownView.svelte';
  import PdfView from './pdf/PdfView.svelte';

  interface Props {
    preview: Preview;
    /** The file's name, used as the image's text alternative. */
    label: string;
    /** Show where each Markdown link points (an approval's content view). */
    showTargets?: boolean;
    /** The document page: `none` says there is no preview (SM-PV-1). */
    onDocumentPage?: boolean;
    /** **Open in its app** after the last page drawn of a long PDF (SM-PDF-1); absent where it is not offered. */
    onopen?: () => void;
  }
  let { preview, label, showTargets = false, onDocumentPage = false, onopen }: Props = $props();

  // Vector images are left out on purpose: only raster types are drawn from bytes.
  const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/webp'];
</script>

{#if preview.kind === 'text'}
  {#if preview.markdown}
    <MarkdownView source={preview.text} {showTargets} />
  {:else}
    <pre class="plain">{preview.text}</pre>
  {/if}
{:else if preview.kind === 'image'}
  {#if IMAGE_TYPES.includes(preview.media_type)}
    <img class="picture" src={`data:${preview.media_type};base64,${preview.bytes_b64}`} alt={label} />
  {:else}
    <p>{t('block-cannot-show')}</p>
  {/if}
{:else if preview.kind === 'thumbnail'}
  <img class="picture" src={`data:image/png;base64,${preview.png_b64}`} alt={label} />
{:else if preview.kind === 'pdf'}
  <!-- SM-PDF-1: pdf.js draws the pages on canvases; no `object`, `embed` or `iframe` (object-src 'none'). -->
  <PdfView bytes={preview.bytes_b64} {label} {onopen} />
{:else if onDocumentPage}
  <!-- SM-PV-1: no preview of this kind (kind `none`): a line, never an empty page. -->
  <p class="secondary">{t('doc-no-preview')}</p>
{/if}

<style>
  .plain {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    margin: 0;
  }
  .picture {
    max-inline-size: 100%;
    block-size: auto;
  }
</style>
