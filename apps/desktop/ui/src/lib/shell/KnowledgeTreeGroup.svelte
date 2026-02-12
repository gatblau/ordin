<script lang="ts">
  // The Knowledge row under a workspace (ScreensModule.md SM-KT-1 to SM-KT-3). Choosing its label opens the
  // Knowledge screen, as before; its arrow, and Right/Left on the row, unfold and fold it. Unfolded, it
  // lists the team folders by name with their pages, then the workspace's own pages, each level by title,
  // the 10 most recently changed first when a level holds more, with **Show all…**. A page row opens
  // `knowledge:<page id>`. The list is read when the row unfolds and again whenever the workspace's
  // revision changes (`knowledge_pages_changed`, SM-KT-3). Which rows are unfolded is kept between starts.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-KT-1
  // spec: ordin-desktop/components/ScreensModule#SM-KT-2
  // spec: ordin-desktop/components/ScreensModule#SM-KT-3
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { PageListing, PageSummary, ServiceError } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { appOrNull } from '../views/fixed/util';
  import { knowledgeTree, shownAtLevel } from './knowledge-tree';
  import { openScreen } from './open-item';
  import { makeKey, SCREEN_IDS } from './screens';

  interface Props {
    backend: BackendState;
    workspaceId: string;
  }
  let { backend, workspaceId }: Props = $props();

  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const key = `${backend.id}:${workspaceId}`;
  let open = $state(app?.prefs.knowledgeUnfolded().includes(key) ?? false);
  let listing = $state.raw<PageListing | null>(null);
  let error = $state<ServiceError | null>(null);
  // Levels whose **Show all…** was chosen: '' is the top level, otherwise a folder's name.
  let showingAll = $state<string[]>([]);
  // Team folders unfolded under the row (not kept between starts).
  let openFolders = $state<string[]>([]);

  const tree = $derived(listing === null ? null : knowledgeTree(listing));
  const revision = $derived(backend.workspaceRevision(workspaceId));

  async function load(): Promise<void> {
    const out = await backend.handle.call('knowledge.list', { workspace_id: workspaceId });
    if (out.kind === 'ok') {
      listing = out.result;
      error = null;
    } else {
      error = out.error;
    }
  }

  // Read when unfolded, and again on each revision while unfolded (SM-KT-3).
  $effect(() => {
    void revision;
    if (open) void load();
  });

  function toggle(): void {
    open = !open;
    if (app === null) return;
    const kept = app.prefs.knowledgeUnfolded().filter((k) => k !== key);
    app.prefs.setKnowledgeUnfolded(open ? [...kept, key] : kept);
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowRight' && !open) {
      event.preventDefault();
      toggle();
    } else if (event.key === 'ArrowLeft' && open) {
      event.preventDefault();
      toggle();
    }
  }

  function openKnowledge(): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, SCREEN_IDS.knowledge);
  }

  function openPage(page: PageSummary): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.knowledge, page.id), { title: page.title });
  }

  const toggleFolder = (name: string): void => {
    openFolders = openFolders.includes(name) ? openFolders.filter((n) => n !== name) : [...openFolders, name];
  };
</script>

{#snippet pageRows(pages: PageSummary[], level: string, depth: number)}
  {@const { shown, more } = shownAtLevel(pages, showingAll.includes(level))}
  {#each shown as page (page.id)}
    <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf" data-testid="knowledge-tree-page">
      <button type="button" class="tree-row leaf" style={`padding-inline-start: calc(2px + 28px + ${23 + depth * 14}px)`} onclick={() => openPage(page)}><span class="icon-tile"><Icon name="page" /></span><span class="tree-name">{page.title}</span></button>
    </div>
  {/each}
  {#if more}
    <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf">
      <button type="button" class="tree-row leaf" data-testid="knowledge-tree-show-all" style={`padding-inline-start: calc(2px + 28px + ${23 + depth * 14}px)`} onclick={() => (showingAll = [...showingAll, level])}>{t('tree-show-all')}</button>
    </div>
  {/if}
{/snippet}

<div role="treeitem" aria-selected="false" aria-expanded={open} tabindex="-1" class="tree-leaf" data-testid="knowledge-tree" {onkeydown}>
  <div class="tree-line">
    <button type="button" class="tree-toggle" class:open tabindex="-1" data-testid="knowledge-tree-toggle" aria-label={open ? t('tree-knowledge-hide') : t('tree-knowledge-show')} title={open ? t('tree-knowledge-hide') : t('tree-knowledge-show')} onclick={toggle}>
      <Icon name="chev" size="twist" />
    </button>
    <button type="button" class="tree-row leaf" onclick={openKnowledge}><span class="icon-tile" data-tint="memory"><Icon name="know" /></span>{t('tree-knowledge')}</button>
  </div>
  {#if open}
    <ErrorLine {error} onretry={() => void load()} />
    {#if tree !== null}
      <div role="group" class="tree-group">
        {#if tree.folders.length === 0 && tree.pages.length === 0}
          <p class="none" data-testid="knowledge-tree-none">{t('tree-knowledge-none')}</p>
        {/if}
        {#each tree.folders as folder (folder.name)}
          {@const folderOpen = openFolders.includes(folder.name)}
          <div role="treeitem" aria-selected="false" aria-expanded={folderOpen} tabindex="-1" class="tree-leaf" data-testid="knowledge-tree-folder">
            <button type="button" class="tree-row leaf" style="padding-inline-start: calc(2px + 28px + 23px)" title={folder.description ?? undefined} onclick={() => toggleFolder(folder.name)}><span class="icon-tile"><Icon name="folder" /></span><span class="tree-name">{folder.name}</span></button>
            {#if folderOpen}
              <div role="group" class="tree-group">{@render pageRows(folder.pages, folder.name, 1)}</div>
            {/if}
          </div>
        {/each}
        {@render pageRows(tree.pages, '', 0)}
      </div>
    {/if}
  {/if}
</div>

<style>
  /* The arrow sits in the space the row's padding leaves before its icon (tokens.css `.tree-row`). */
  .tree-toggle {
    position: absolute;
    inset-inline-start: 16px;
    z-index: 1;
    inline-size: 18px;
    block-size: 18px;
    min-height: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    background: none;
    color: var(--color-text-muted);
    border-radius: var(--radius-sm);
    padding: 0;
  }
  .tree-toggle:hover {
    background: var(--color-line);
  }
  .tree-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tree-group {
    display: flex;
    flex-direction: column;
  }
  .none {
    margin: 0;
    padding: 2px 6px 2px calc(2px + 28px + 23px);
    font-size: 13px;
    color: var(--color-text-muted);
  }
</style>
