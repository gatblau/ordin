<script lang="ts">
  // The inside of a linked location in the tree: its sub-folders and documents, read with `files.list_folder`
  // when the row is opened, a folder one level at a time (SM-RL-3). A folder row opens or closes its own
  // contents; a document row opens the document page.
  //
  // A document's actions live in its ⋯ menu and on a right-click of its row (amended January 2026, user
  // decision; Approved by the user January 2026; 11-ux-guidelines.md "Actions in menus, content in panes"):
  // **Ask Ordin about this** (the same message and attachment as SM-ASK-1), **Open in its app**
  // (`craft.open`, then the `no_editor` question under the row as SM-NS-4.2 asks it) and **Move to Bin…**
  // (SM-RL-4). The document page shows the document only.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-ASK-1
  // spec: ordin-desktop/components/ScreensModule#SM-NS-4
  // spec: ordin-desktop/components/ScreensModule#SM-RL-3
  // spec: ordin-desktop/components/ScreensModule#SM-RL-4
  //
  // The document menu also holds **Publish to…** with the destinations the workspace is connected to
  // (SM-PUB-1, SM-PUB-2), and a folder of the person's own folders holds **Publish this folder
  // automatically…** (SM-FP-1) (amended January 2026; user decision; Approved by the user January 2026).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-PUB-1
  // spec: ordin-desktop/components/ScreensModule#SM-FP-1
  import { answers, answersAll } from '../backend/groups';
  import { askOrdin, isWaitingForServer, menuOf, type MenuText } from '../blocks/ask';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import { fileKindIcon, fileKindTint } from '../icons/kinds';
  import type { IconName } from '../icons/sprite';
  import type { BinTicket, FileEntry, ServiceError } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import ConfirmDialog from '../views/ConfirmDialog.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { appOrNull } from '../views/fixed/util';
  import { loadDestinations, type PublishSystem } from '../views/publish/destinations';
  import PublishFolderDialog from '../views/publish/PublishFolderDialog.svelte';
  import PublishToDialog from '../views/publish/PublishToDialog.svelte';
  import PublishToItems from '../views/publish/PublishToItems.svelte';
  import TreeFolder from './TreeFolder.svelte';
  import { openScreen } from './open-item';
  import { itemOfTab, ScreenAreaState } from './screen-area.svelte';
  import { makeKey, SCREEN_IDS } from './screens';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    placeId: string;
    /** The folder's path inside the place, `/`-separated, empty for the place itself. */
    folder?: string;
    /** How deep the rows sit, for their indent. */
    depth?: number;
  }
  // SM-IC-5: a file's tree row shows its kind's icon and tint, as its screen tab does; a kind with none is `page`.
  // spec: ordin-desktop/components/ScreensModule#SM-IC-5
  const fileIcon = (name: string): IconName => fileKindIcon(name) ?? 'page';

  let { backend, workspaceId, placeId, folder = '', depth = 1 }: Props = $props();

  const app = appOrNull();
  let entries = $state<FileEntry[]>([]);
  let error = $state<ServiceError | null>(null);
  let loaded = $state(false);
  let next = $state<string | null>(null);
  let opened = $state<string[]>([]);

  async function read(cursor: string | null): Promise<void> {
    const out = await backend.handle.call('files.list_folder', {
      place_id: placeId,
      folder,
      sort: 'name_asc',
      ...(cursor === null ? {} : { cursor }),
    });
    loaded = true;
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    error = null;
    entries = cursor === null ? out.result.items : [...entries, ...out.result.items];
    next = out.result.next_cursor ?? null;
    // SM-OS-3: once the folder is read whole, a tab on a document of this folder that is gone closes.
    if (next === null) closeGoneDocuments();
  }

  $effect(() => {
    void backend.workspaceRevision(workspaceId);
    void read(null);
  });

  function closeGoneDocuments(): void {
    const prefix = `${placeId}:`;
    const here = new Set(entries.map((e) => `${placeId}:${e.path}`));
    const inThisFolder = (item: string): boolean => {
      if (!item.startsWith(prefix)) return false;
      const path = item.slice(prefix.length);
      const at = path.lastIndexOf('/');
      return (at < 0 ? '' : path.slice(0, at)) === folder.replace(/\/+$/, '');
    };
    app?.screens.closeWhere((key) => {
      const item = itemOfTab('document', key);
      return item !== null && inThisFolder(item) && !here.has(item);
    }, ScreenAreaState.scope(backend.id, workspaceId));
  }

  const indent = $derived(`padding-inline-start: calc(2px + 28px + ${23 + depth * 14}px)`);

  function openFile(entry: FileEntry): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.document, `${placeId}:${entry.path}`), { title: entry.name });
  }

  // A document's ⋯ menu: Move to Bin… asks first, then `files.move_to_bin` (SM-RL-3), and closes the
  // document's tab if it is open, as the document page's Move to Bin did (SM-NS-4.7).
  const canBin = $derived(answers(backend, 'files.move_to_bin'));
  const canAsk = $derived(backend.groups.has('ordin') && answers(backend, 'ordin.send_message'));
  const canOpen = $derived(answers(backend, 'craft.open'));
  const hasMenu = $derived(canBin || canAsk || canOpen);
  /** A document whose Open in its app answered `no_editor`: the question is asked under its row. */
  let noEditor = $state<string | null>(null);
  let actionError = $state<ServiceError | null>(null);

  // Add to knowledge and Add to memory (amended January 2026, user decision; Approved by the user January
  // 2026): no command makes a knowledge page or memory from a document (`knowledge.write` takes a finished
  // body, `memory.*` keeps what is already remembered), so each asks Ordin in the workspace's conversation
  // with the entry attached, the same path as Ask Ordin about this; the conversation is brought into view.
  type AskText = MenuText;
  async function ask(entry: FileEntry, text: AskText = 'block-ask-text-this'): Promise<void> {
    menuFor = null;
    if (text !== 'block-ask-text-this' && app !== null) {
      app.shell.openWorkspaceTab(backend.id, workspaceId);
      if (app.shell.narrow) app.shell.conversationOverlayOpen = true;
    }
    const out = await askOrdin(backend, workspaceId, t(text, { title: entry.name }), [{ kind: 'place_file', place_id: placeId, relative_path: entry.path }], menuOf(text));
    actionError = out.kind === 'error' ? out.error : null;
  }

  async function openInApp(entry: FileEntry): Promise<void> {
    menuFor = null;
    const out = await backend.handle.call('craft.open', { place_id: placeId, path: entry.path });
    if (out.kind === 'error') return void (actionError = out.error);
    actionError = null;
    noEditor = out.result.kind === 'no_editor' ? entry.path : null;
  }

  async function openUsualApp(path: string): Promise<void> {
    const out = await backend.handle.call('craft.open_with_system_default', { place_id: placeId, path });
    if (out.kind === 'error') return void (actionError = out.error);
    actionError = null;
    noEditor = null;
  }

  // SM-PUB-1: the destinations of the document whose menu is open, read when it opens.
  let destinations = $state.raw<PublishSystem[]>([]);
  let publishing = $state<{ entry: FileEntry; system: PublishSystem } | null>(null);
  // SM-FP-1: only the person's own folders publish (WorkspaceService.md), and a folder already linked
  // offers no second link.
  const ownPlace = $derived((backend.trees.get(workspaceId)?.places ?? []).some((p) => p.id === placeId && (p.kind === 'persons_folder' || p.kind === 'workspace_folder')));
  const canPublishFolder = $derived(ownPlace && answersAll(backend, ['confluence.find', 'workspaces.propose_confluence_link']));
  let linkedFolders = $state.raw<string[]>([]);
  let folderPublishing = $state<FileEntry | null>(null);
  const folderPath = (entry: FileEntry): string => entry.path.replace(/\/+$/, '');

  $effect(() => {
    const path = menuFor;
    const entry = entries.find((e) => e.path === path);
    if (entry === undefined) return;
    if (!entry.is_folder) {
      destinations = [];
      void loadDestinations(backend, workspaceId, placeId, entry.name).then((d) => {
        if (menuFor === entry.path) destinations = d;
      });
    } else if (canPublishFolder && answers(backend, 'workspaces.confluence_links')) {
      void backend.handle.call('workspaces.confluence_links', { workspace_id: workspaceId }).then((out) => {
        if (out.kind === 'ok') linkedFolders = out.result.filter((l) => l.place_id === placeId).map((l) => l.folder);
      });
    }
  });

  function publishedFolder(approvalId: string | null): void {
    const entry = folderPublishing;
    folderPublishing = null;
    if (approvalId === null || entry === null) return;
    app?.shell.announce(t('publish-folder-sent', { name: entry.name }));
    if (approvalId !== '' && app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.approval, approvalId));
  }

  function contextMenu(event: MouseEvent, entry: FileEntry): void {
    if (entry.is_folder ? !(canAsk || canPublishFolder) : !hasMenu) return;
    event.preventDefault();
    event.stopPropagation();
    menuFor = entry.path;
  }
  let menuFor = $state<string | null>(null);
  let binning = $state<FileEntry | null>(null);
  let binError = $state<ServiceError | null>(null);
  /** The last move to the Bin, kept so **Undo** can send `files.restore_from_bin` with its ticket (SM-RL-4). */
  let moved = $state<{ ticket: BinTicket; name: string } | null>(null);

  function askBin(entry: FileEntry): void {
    menuFor = null;
    binError = null;
    binning = entry;
  }

  async function confirmBin(): Promise<void> {
    const entry = binning;
    if (entry === null) return;
    const out = await backend.handle.call('files.move_to_bin', { place_id: placeId, path: entry.path });
    if (out.kind === 'error') {
      binError = out.error;
      return;
    }
    binning = null;
    moved = { ticket: out.result, name: entry.name };
    app?.screens.closeScreensFor('document', `${placeId}:${entry.path}`);
    app?.shell.announce(t('tree-bin-done', { name: entry.name }));
    await read(null);
  }

  async function undoBin(): Promise<void> {
    if (moved === null) return;
    const out = await backend.handle.call('files.restore_from_bin', { ticket: moved.ticket });
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    moved = null;
    await read(null);
  }

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  function menuKey(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    menuFor = null;
  }

  function toggle(entry: FileEntry): void {
    opened = opened.includes(entry.path) ? opened.filter((p) => p !== entry.path) : [...opened, entry.path];
  }
</script>

<ErrorLine {error} onretry={() => void read(null)} />
<ErrorLine error={actionError} />
{#if moved !== null}
  <p class="tree-empty" role="status" style={indent}>
    {t('tree-bin-done', { name: moved.name })}
    <button type="button" class="link" onclick={() => void undoBin()}>{t('tree-bin-undo')}</button>
  </p>
{/if}
<div role="group" class="tree-group">
  {#each entries as entry (entry.path)}
    {#if entry.is_folder}
      <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf" aria-expanded={opened.includes(entry.path)}>
        <div class="tree-line">
          <button type="button" class="tree-row leaf" style={indent} onclick={() => toggle(entry)} oncontextmenu={(e) => contextMenu(e, entry)}><span class="icon-tile"><Icon name="folder" /></span><span class="tree-name">{entry.name}</span></button>
          {#if canAsk || canPublishFolder}
            <span class="tree-actions" class:shown={menuFor === entry.path}>
              <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuFor === entry.path} aria-label={t('more-actions-for', { name: entry.name })} title={t('more-actions-tip')} onclick={() => (menuFor = menuFor === entry.path ? null : entry.path)}><Icon name="more" /></button>
            </span>
            {#if menuFor === entry.path}
              <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: entry.name })} onkeydown={menuKey} use:focusFirst>
                {#if canAsk}<button type="button" role="menuitem" disabled={isWaitingForServer(backend)} onclick={() => void ask(entry, 'tree-ask-text-knowledge')}><Icon name="know" />{t('tree-add-to-knowledge')}</button>{/if}
                {#if canPublishFolder && !linkedFolders.includes(folderPath(entry))}<button type="button" role="menuitem" onclick={() => { menuFor = null; folderPublishing = entry; }}><Icon name="publish" />{t('publish-folder')}</button>{/if}
              </div>
            {/if}
          {/if}
        </div>
        {#if opened.includes(entry.path)}
          <TreeFolder {backend} {workspaceId} {placeId} folder={entry.path} depth={depth + 1} />
        {/if}
      </div>
    {:else}
      <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf">
        <div class="tree-line">
          <button type="button" class="tree-row leaf" style={indent} onclick={() => openFile(entry)} oncontextmenu={(e) => contextMenu(e, entry)}><span class="icon-tile" data-tint={fileKindTint(fileIcon(entry.name))}><Icon name={fileIcon(entry.name)} /></span><span class="tree-name">{entry.name}</span></button>
          {#if hasMenu}
            <span class="tree-actions" class:shown={menuFor === entry.path}>
              <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuFor === entry.path} aria-label={t('more-actions-for', { name: entry.name })} title={t('more-actions-tip')} onclick={() => (menuFor = menuFor === entry.path ? null : entry.path)}><Icon name="more" /></button>
            </span>
            {#if menuFor === entry.path}
              <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: entry.name })} onkeydown={menuKey} use:focusFirst>
                {#if canAsk}<button type="button" role="menuitem" disabled={isWaitingForServer(backend)} onclick={() => void ask(entry)}><Icon name="chat" />{t('block-ask-about-this')}</button>{/if}
                {#if canAsk}<button type="button" role="menuitem" disabled={isWaitingForServer(backend)} onclick={() => void ask(entry, 'tree-ask-text-knowledge')}><Icon name="know" />{t('tree-add-to-knowledge')}</button>{/if}
                {#if canAsk}<button type="button" role="menuitem" disabled={isWaitingForServer(backend)} onclick={() => void ask(entry, 'tree-ask-text-memory')}><Icon name="memory" />{t('tree-add-to-memory')}</button>{/if}
                {#if canOpen}<button type="button" role="menuitem" onclick={() => void openInApp(entry)}><Icon name="open" />{t('doc-open-in-app')}</button>{/if}
                <PublishToItems {destinations} onchoose={(system) => { menuFor = null; publishing = { entry, system }; }} />
                {#if canBin}<button type="button" role="menuitem" class="danger" onclick={() => askBin(entry)}><Icon name="remove" />{t('doc-move-to-bin')}</button>{/if}
              </div>
            {/if}
          {/if}
        </div>
        {#if noEditor === entry.path}
          <p class="tree-empty" role="status" style={indent}>
            {t('doc-no-editor')}
            <button type="button" class="link" onclick={() => void openUsualApp(entry.path)}>{t('doc-open-usual-app')}</button>
          </p>
        {/if}
      </div>
    {/if}
  {/each}
  {#if loaded && error === null && entries.length === 0}
    <p class="tree-empty" style={indent}>{t('tree-folder-empty')}</p>
  {/if}
  {#if next !== null}
    <button type="button" class="tree-row leaf" style={indent} onclick={() => void read(next)}>{t('settings-show-more')}</button>
  {/if}
</div>
{#if publishing !== null}
  {@const doc = publishing.entry}
  <PublishToDialog
    {backend}
    {workspaceId}
    {placeId}
    path={doc.path}
    name={doc.name}
    system={publishing.system}
    onclose={(sent) => {
      publishing = null;
      if (sent) app?.shell.announce(t('publish-to-sent', { name: doc.name }));
    }}
  />
{/if}
{#if folderPublishing !== null}
  <PublishFolderDialog {backend} {workspaceId} {placeId} folder={folderPath(folderPublishing)} name={folderPublishing.name} onclose={publishedFolder} />
{/if}
{#if binning !== null}
  <ConfirmDialog title={t('doc-bin-title', { name: binning.name })} body={t('doc-bin-body')} verb={t('doc-bin-verb')} onconfirm={() => void confirmBin()} oncancel={() => (binning = null)}>
    <ErrorLine error={binError} />
  </ConfirmDialog>
{/if}

<style>
  .tree-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tree-empty {
    margin: 0;
    color: var(--color-text-muted);
    font-size: 12px;
  }
</style>
