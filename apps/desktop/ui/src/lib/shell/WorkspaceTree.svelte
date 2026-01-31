<script lang="ts">
  // The person's workspaces as one WAI-ARIA tree, never folded behind a menu. A group appears under a
  // workspace only when the backend answers its commands (SM-UP-1). Each connected server is a group
  // below, under its display name.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-UP-1
  // spec: ordin-desktop/components/ScreensModule#SM-AW-1
  import { onMount } from 'svelte';

  import { answers, answersAll } from '../backend/groups';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { IconName } from '../icons/sprite';
  import { registerActionHandler } from '../search/actions';
  import type { BackendState } from '../store/backend-state.svelte';
  import { ADD_TO_WORKSPACE_SCREEN } from '../views/add/add-model.svelte';
  import { lifecycleOf } from '../views/workspaces/lifecycle.svelte';
  import RenameWorkspace from '../views/workspaces/RenameWorkspace.svelte';
  import { getApp } from './context';
  import DocumentsTreeGroup from './DocumentsTreeGroup.svelte';
  import EarlierTreeGroup from './EarlierTreeGroup.svelte';
  import { openScreen } from './open-item';
  import { SCREEN_IDS } from './screens';

  interface Props {
    /** The stores to list: the local one first, then each server's. */
    backends: BackendState[];
  }
  let { backends }: Props = $props();

  const app = getApp();

  // The groups under a workspace, the commands each one needs (the rail never invents an entry) and the
  // icon each draws (cross-cutting/VisualDesign.md V8; Knowledge and What Ordin remembers in the memory tint).
  // Documents is drawn by DocumentsTreeGroup, before these (SM-RL-1).
  const GROUPS: ReadonlyArray<{ screen: string; label: string; commands: readonly string[]; icon: IconName; tint?: string }> = [
    { screen: SCREEN_IDS.agents, label: 'tree-agents', commands: ['agents.list'], icon: 'agent' },
    { screen: SCREEN_IDS.workflows, label: 'tree-workflows', commands: ['workflows.list'], icon: 'crew' },
    { screen: SCREEN_IDS.dashboards, label: 'tree-dashboards', commands: ['dashboards.list'], icon: 'chart' },
    { screen: SCREEN_IDS.email, label: 'tree-email', commands: ['mail.list_messages'], icon: 'mail' },
    { screen: SCREEN_IDS.telegram, label: 'tree-telegram', commands: ['telegram.links'], icon: 'chat' },
    { screen: SCREEN_IDS.slack, label: 'tree-slack', commands: ['slack.links'], icon: 'chat' },
    { screen: SCREEN_IDS.running, label: 'tree-running', commands: ['workspaces.tree'], icon: 'run' },
    { screen: SCREEN_IDS.knowledge, label: 'tree-knowledge', commands: ['knowledge.list'], icon: 'know', tint: 'memory' },
    { screen: SCREEN_IDS.memory, label: 'tree-memory', commands: ['memory.get'], icon: 'memory', tint: 'memory' },
  ];

  // The workspace row's ⋯ menu (`tree-add`): the workspace whose menu is open.
  let menuFor = $state<string | null>(null);

  // SM-WS-2: Rename…, Archive and Remove workspace… in the same menu, for the local backend's workspaces.
  // spec: ordin-desktop/components/ScreensModule#SM-WS-2
  const lifecycle = (backend: BackendState) => lifecycleOf(backend, app.shell);
  const hasMenu = (backend: BackendState): boolean => {
    const offered = lifecycle(backend).offered;
    return answers(backend, 'workspaces.add_link') || offered.rename || offered.archive || offered.remove;
  };

  function addTo(backend: BackendState, workspaceId: string): void {
    menuFor = null;
    openScreen(app, backend.id, workspaceId, ADD_TO_WORKSPACE_SCREEN);
  }

  // The Actions group of *Search or go to…*: Add documents opens *Add to this workspace* for the workspace in
  // front (step 9, `add_documents`).
  onMount(() => {
    registerActionHandler('add_documents', (workspaceId) => {
      if (workspaceId !== null) openScreen(app, app.shell.openBackend, workspaceId, ADD_TO_WORKSPACE_SCREEN);
    });
  });

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  let expanded = $state<string[]>(app.prefs.expanded(app.shell.openWorkspace === null ? [] : [app.shell.openWorkspace]));
  const nodeKey = (backend: BackendState, workspaceId: string): string => `${backend.id}:${workspaceId}`;

  function toggle(backend: BackendState, workspaceId: string): void {
    const key = nodeKey(backend, workspaceId);
    expanded = expanded.includes(key) ? expanded.filter((k) => k !== key) : [...expanded, key];
    app.prefs.setExpanded(expanded);
    if (expanded.includes(key) && !backend.trees.has(workspaceId)) void backend.loadTree(workspaceId);
  }

  function onkeydown(event: KeyboardEvent, backend: BackendState, workspaceId: string): void {
    const open = expanded.includes(nodeKey(backend, workspaceId));
    if (event.key === 'ArrowRight' && !open) toggle(backend, workspaceId);
    else if (event.key === 'ArrowLeft' && open) toggle(backend, workspaceId);
    else if (event.key === 'Enter') app.shell.openWorkspaceTab(backend.id, workspaceId);
    else return;
    event.preventDefault();
  }

  const heading = (backend: BackendState): string =>
    backend.kind === 'local'
      ? ''
      : backend.connection === 'not_connected'
        ? t('rail-server-not-connected', { time: backend.asOf ?? '' })
        : t('rail-server-on', { server: backend.displayName });

  // The version sentence names the side to update (ServerView.older_side).
  const versionLine = (backend: BackendState): string | null =>
    backend.connection !== 'refused_for_version'
      ? backend.connection === 'disconnected'
        ? t('server-ended', { server: backend.displayName })
        : null
      : backend.olderSide === 'desktop'
        ? t('server-version-desktop-older', { server: backend.displayName })
        : t('server-version-server-older', { server: backend.displayName });
</script>

<div role="tree" aria-label={t('rail-workspaces')} class="workspace-tree">
  {#each backends as backend (backend.id)}
    {#if backend.kind === 'server'}
      <!-- A connected server's group draws `remote` before its words (V8, scene `srvTree`). -->
      <p class="server-heading"><span class="icon-tile"><Icon name="remote" /></span>{heading(backend)}</p>
      {@const line = versionLine(backend)}
      {#if line !== null}<p class="server-line">{line}</p>{/if}
    {/if}
    {#each backend.workspaces as workspace (workspace.id)}
      {@const open = expanded.includes(nodeKey(backend, workspace.id))}
      {@const waiting = backend.needsYouCountFor(workspace.id)}
      {@const current = app.shell.openWorkspace === workspace.id && app.shell.openBackend === backend.id}
      <div
        role="treeitem"
        aria-selected={current}
        aria-expanded={open}
        tabindex="0"
        class="tree-node"
        class:current
        class:on-server={backend.kind === 'server'}
        onkeydown={(e) => onkeydown(e, backend, workspace.id)}
        onclick={() => app.shell.openWorkspaceTab(backend.id, workspace.id)}
      >
        <button type="button" class="tree-toggle" class:open tabindex="-1" aria-label={open ? t('rail-collapse-all') : t('rail-expand')} title={open ? t('rail-collapse-all') : t('rail-expand')} onclick={(e) => { e.stopPropagation(); toggle(backend, workspace.id); }}>
          <Icon name="chev" size="twist" />
        </button>
        <span class="icon-tile"><Icon name="ws" /></span>
        <span class="tree-name">{workspace.name}</span>
        {#if waiting > 0}<span class="count" aria-label={t('card-waiting')}>{waiting}</span>{/if}
        {#if hasMenu(backend)}
          {@const key = nodeKey(backend, workspace.id)}
          {@const offered = lifecycle(backend).offered}
          <span class="tree-actions" class:shown={menuFor === key}>
            <button type="button" class="tree-action" tabindex="-1" aria-haspopup="menu" aria-expanded={menuFor === key} aria-label={t('more-actions-for', { name: workspace.name })} title={t('more-actions-tip')} onclick={(e) => { e.stopPropagation(); menuFor = menuFor === key ? null : key; }}><Icon name="more" /></button>
          </span>
          {#if menuFor === key}
            <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: workspace.name })} use:focusFirst onclick={(e) => e.stopPropagation()} onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Escape') menuFor = null; }}>
              {#if answers(backend, 'workspaces.add_link')}<button type="button" role="menuitem" onclick={() => addTo(backend, workspace.id)}><Icon name="plus" />{t('tree-add')}</button>{/if}
              {#if offered.rename}<button type="button" role="menuitem" onclick={() => { menuFor = null; lifecycle(backend).startRename(workspace.id); }}><Icon name="edit" />{t('ws-rename')}</button>{/if}
              {#if offered.archive}<button type="button" role="menuitem" onclick={() => { menuFor = null; void lifecycle(backend).archive(workspace); }}><Icon name="fold" />{t('ws-archive')}</button>{/if}
              {#if offered.remove}<button type="button" role="menuitem" onclick={() => { menuFor = null; void lifecycle(backend).askBin(workspace, 'rail'); }}><Icon name="bin" />{t('ws-remove')}</button>{/if}
            </div>
          {/if}
        {/if}
      </div>
      {#if lifecycle(backend).renaming === workspace.id}<RenameWorkspace model={lifecycle(backend)} {workspace} />{/if}
      {#if open}
        <div role="group" class="tree-group" class:on-server={backend.kind === 'server'}>
          {#if answers(backend, 'files.list_folder')}<DocumentsTreeGroup {backend} workspaceId={workspace.id} />{/if}
          {#each GROUPS.filter((g) => answersAll(backend, g.commands)) as group (group.screen)}
            <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf">
              <button type="button" class="tree-row" class:leaf={group.screen === SCREEN_IDS.knowledge || group.screen === SCREEN_IDS.memory} onclick={() => openScreen(app, backend.id, workspace.id, group.screen)}><span class="icon-tile" data-tint={group.tint}><Icon name={group.icon} /></span>{t(group.label)}</button>
            </div>
          {/each}
          {#if answers(backend, 'ordin.list_conversations')}<EarlierTreeGroup {backend} workspaceId={workspace.id} />{/if}
        </div>
      {/if}
    {/each}
  {/each}
</div>

<style>
  /* The tree (cross-cutting/VisualDesign.md V6 "Tree row"; scenes `tree`, `catchup`, `srvTree`). */
  .workspace-tree {
    display: flex;
    flex-direction: column;
    font-size: 14px;
  }
  .tree-node {
    display: flex;
    align-items: center;
    gap: 5px;
    min-height: 30px;
    padding: 2px 6px 2px 2px;
    border-radius: var(--radius-row);
    border: 1px solid transparent;
    cursor: pointer;
    outline: none;
    position: relative;
  }
  .tree-node:hover {
    background: var(--color-chip);
  }
  .tree-node.current {
    border-color: var(--color-accent);
    background: var(--color-accent-soft);
    font-weight: 700;
  }
  .tree-node:focus-visible {
    outline: 3px solid var(--color-focus);
    outline-offset: -2px;
  }
  .tree-toggle {
    inline-size: 18px;
    block-size: 18px;
    min-height: 0;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    background: none;
    color: var(--color-text-muted);
    border-radius: var(--radius-sm);
    padding: 0;
    font-size: 14px;
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
  /* A server is a row of the tree at the workspaces' level, its workspaces one level in (scene `srvTree`). */
  .server-heading {
    display: flex;
    align-items: center;
    gap: 5px;
    min-height: 30px;
    padding: 2px 6px 2px calc(2px + 18px + 5px);
    font-size: 14px;
    color: var(--color-text);
  }
  .tree-node.on-server {
    padding-inline-start: calc(2px + 14px);
  }
  .tree-group.on-server :global(.tree-row) {
    padding-inline-start: calc(2px + 28px + 23px);
  }
  .server-line {
    font-size: 12px;
    color: var(--color-text-muted);
  }
</style>
