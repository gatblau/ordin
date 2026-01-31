<script lang="ts">
  // What [agent] is made of (SM-NS-11): the agent's files as a tree of their folders (instructions,
  // skills, rules, knowledge…), and beside it, at every width, a reader for the file chosen: two panes, the
  // tree staying in view while the reader scrolls, and the tree can be hidden to give the reader the width
  // (SM-IF-1 to SM-IF-3). Markdown is drawn formatted
  // with the read-only Markdown view; any other text is drawn as it is, in monospace. Size and checksum
  // are under the reader's details. Nothing here changes the agent.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-11
  // spec: ordin-desktop/components/ScreensModule#SM-IF-1
  // spec: ordin-desktop/components/ScreensModule#SM-IF-2
  import MarkdownView from '../../blocks/MarkdownView.svelte';
  import { formatNumber } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { buildTree, folderPaths, isMarkdown, type InstalledFilesController, type TreeNode } from './installed-files.svelte';

  interface Props {
    backend: BackendState;
    c: InstalledFilesController;
    /** The agent's name, held by *What [agent] is made of* and passed as the tab title. */
    agent: string;
  }
  let { backend, c, agent }: Props = $props();
  const tree = $derived(buildTree(c.files));
  const allFolders = $derived(folderPaths(tree));
  /** Folders the person closed; every folder starts open, as an agent has few files. */
  let closed = $state<string[]>([]);
  let copied = $state(false);
  /** SM-IF-2: the tree is hidden, so the reader has the whole width. */
  let treeHidden = $state(false);
  const entry = $derived(c.files.find((f) => f.path === c.selected) ?? null);

  function toggle(path: string): void {
    closed = closed.includes(path) ? closed.filter((p) => p !== path) : [...closed, path];
  }

  async function copy(): Promise<void> {
    if (c.file === null) return;
    try {
      await navigator.clipboard.writeText(c.file.text);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // The clipboard is not available: the text stays on screen to select.
    }
  }
</script>

{#snippet nodes(list: TreeNode[], depth: number)}
  {#each list as node (node.path)}
    {#if node.kind === 'folder'}
      {@const open = !closed.includes(node.path)}
      <li role="treeitem" aria-expanded={open} aria-selected="false">
        <button type="button" class="node" style:--depth={depth} onclick={() => toggle(node.path)}>
          <span class="chev" class:open><Icon name="chev" size="twist" /></span><Icon name="folder" /><span class="name">{node.name}</span>
        </button>
        {#if open}<ul role="group">{@render nodes(node.children, depth + 1)}</ul>{/if}
      </li>
    {:else}
      <li role="treeitem" aria-selected={c.selected === node.path}>
        <button type="button" class="node file" class:selected={c.selected === node.path} style:--depth={depth} aria-current={c.selected === node.path ? 'true' : undefined} onclick={() => void c.open(node.path)}>
          <span class="chev"></span><Icon name={isMarkdown(node.path) ? 'md' : 'page'} /><span class="name">{node.name}</span>
        </button>
      </li>
    {/if}
  {/each}
{/snippet}

<section class="installed-files" aria-labelledby="installed-files-heading">
  <h2 id="installed-files-heading" tabindex="-1" data-region-focus>{t('installed-files-title', { agent })}</h2>
  <p class="quiet">{t('installed-files-note')}</p>
  <ErrorLine error={c.error} onretry={() => void c.load()} />
  {#if tree.length > 0}
    <div class="split" class:tree-hidden={treeHidden}>
      {#if !treeHidden}
      <nav class="tree-pane" data-testid="installed-files-tree" aria-label={t('installed-files-tree', { agent })}>
          <div class="tools" role="toolbar" aria-label={t('installed-files-tree', { agent })}>
            <button type="button" class="md-tool" data-testid="installed-files-hide-tree" aria-label={t('installed-files-hide-tree')} title={t('installed-files-hide-tree')} onclick={() => (treeHidden = true)}><Icon name="back" /></button>
            {#if allFolders.length > 0}
            <button type="button" class="md-tool" disabled={closed.length === 0} aria-label={t('installed-files-open-all')} title={t('installed-files-open-all')} onclick={() => (closed = [])}><Icon name="unfold" /></button>
            <button type="button" class="md-tool" disabled={allFolders.every((f) => closed.includes(f))} aria-label={t('installed-files-close-all')} title={t('installed-files-close-all')} onclick={() => (closed = [...allFolders])}><Icon name="fold" /></button>
            {/if}
          </div>
        <ul class="tree" role="tree">{@render nodes(tree, 0)}</ul>
      </nav>
      {/if}
      <div class="reader" data-testid="installed-files-reader" aria-live="polite">
        {#if treeHidden}
          <div class="tools">
            <button type="button" class="md-tool" data-testid="installed-files-show-tree" aria-label={t('installed-files-show-tree')} title={t('installed-files-show-tree')} onclick={() => (treeHidden = false)}><Icon name="folder" /></button>
          </div>
        {/if}
        {#if c.selected === null}
          <p class="quiet">{t('installed-files-choose')}</p>
        {:else}
          <div class="reader-head">
            <h3>{c.selected}</h3>
            {#if c.file !== null}
              <button type="button" class="md-tool" aria-label={copied ? t('installed-files-copied') : t('installed-files-copy')} title={copied ? t('installed-files-copied') : t('installed-files-copy')} onclick={() => void copy()}><Icon name={copied ? 'check' : 'copy'} /></button>
            {/if}
          </div>
          <ErrorLine error={c.fileError} onretry={() => { if (c.selected !== null) void c.open(c.selected); }} />
          {#if c.file !== null}
            <div class="page">
              {#if isMarkdown(c.file.path)}<MarkdownView source={c.file.text} />{:else}<pre class="mono">{c.file.text}</pre>{/if}
            </div>
            {#if c.file.cut}<p class="quiet">{t('installed-files-cut')}</p>{/if}
            {#if entry !== null}
              <details>
                <summary>{t('installed-files-details')}</summary>
                <p class="quiet">{formatNumber(entry.size, { style: 'unit', unit: 'byte', unitDisplay: 'narrow' })} · {entry.sha256.slice(0, 12)}</p>
              </details>
            {/if}
          {/if}
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .installed-files {
    gap: var(--space-sm);
    container-type: inline-size;
  }
  .installed-files > h2 {
    font-size: 19px;
  }
  .quiet {
    color: var(--color-text-muted);
  }
  /* SM-IF-1: the tree beside the reader at every width; a narrow screen gives the tree a narrower column.
     The tree stays in view while the reader scrolls (SM-IF-3). */
  .split {
    display: grid;
    grid-template-columns: minmax(12rem, 18rem) minmax(0, 1fr);
    gap: var(--space-md);
    align-items: start;
  }
  .split.tree-hidden {
    grid-template-columns: minmax(0, 1fr);
  }
  @container (max-width: 40rem) {
    .split:not(.tree-hidden) {
      grid-template-columns: minmax(8rem, 40%) minmax(0, 1fr);
    }
  }
  /* spec: ordin-desktop/components/ScreensModule#SM-IF-3 */
  .tree-pane {
    position: sticky;
    inset-block-start: 0;
    max-block-size: calc(100dvh - 10rem);
    overflow: auto;
    padding-inline-end: var(--space-04);
    border-inline-end: 1px solid var(--color-line);
  }
  .tools {
    display: flex;
    gap: var(--space-04);
    margin-block-end: var(--space-04);
  }
  /* The editor toolbar's icon buttons (MarkdownEditor `.md-tool`). */
  .md-tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    min-height: 0;
    padding: 0;
  }
  .tree,
  .tree :global(ul) {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .node {
    display: flex;
    align-items: center;
    gap: var(--space-04);
    inline-size: 100%;
    min-height: 0;
    padding: 2px var(--space-04);
    padding-inline-start: calc(var(--depth) * 1rem + var(--space-04));
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    text-align: start;
    font-size: 14px;
  }
  .node:hover {
    background: var(--color-accent-soft);
  }
  .node.selected {
    background: var(--color-accent-soft);
    color: var(--color-accent-ink);
    font-weight: 700;
  }
  /* The twist turns when open (tokens.css `.open > .icon-twist`). */
  .chev {
    display: inline-flex;
    justify-content: center;
    inline-size: 1rem;
    flex: none;
  }
  .name {
    overflow-wrap: anywhere;
  }
  .reader {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    min-inline-size: 0;
  }
  .reader-head {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }
  .reader-head h3 {
    flex: 1;
    margin: 0;
    font-family: var(--font-mono);
    font-size: 14px;
    overflow-wrap: anywhere;
  }
  /* The file's words in one surface, as a part page draws them. */
  .page {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-lg);
    padding: var(--space-md) 18px;
    overflow: auto;
  }
  .mono {
    margin: 0;
    font-family: var(--font-mono);
    font-size: 13px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  /* A tool button carries its words beside its icon. */
</style>
