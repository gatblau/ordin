<script lang="ts">
  // spec: ordin-desktop/components/ScreensModule#SM-PDF-1
  // spec: ordin-desktop/components/ScreensModule#SM-PV-1
  // A `Preview` from the core drawn as the document page draws a `files.preview` result: text as text
  // nodes, an image from its bytes, a PDF's line, a thumbnail; for `none`, nothing (SM-SF-1.1), or on the
  // document page, which offers Open in its app, a line saying so (SM-PV-1).
  import { t } from '../i18n/fluent';
  import type { Preview } from '../service-api/types.gen';
  import MarkdownView from './MarkdownView.svelte';

  interface Props {
    preview: Preview;
    /** The file's name, used as the image's text alternative. */
    label: string;
    /** Show where each Markdown link points (an approval's content view). */
    showTargets?: boolean;
    /** The document page: `none` says there is no preview, beside the page's Open in its app (SM-PV-1). */
    onDocumentPage?: boolean;
  }
  let { preview, label, showTargets = false, onDocumentPage = false }: Props = $props();

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
  <!-- SM-PDF-1: the window's security policy has no embedded viewer (object-src 'none'), so a PDF is never
       drawn in the page; the person reads it in its own app. -->
  <p class="secondary">{t('doc-pdf-opens-in-app')}</p>
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
