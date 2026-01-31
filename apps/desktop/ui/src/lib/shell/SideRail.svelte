<script lang="ts">
  // The left rail: brand, Home with its count, Search or go to…, New workspace, the workspace tree,
  // then Settings at the foot (SM-RAIL-1). Each entry is drawn only when the backend answers its
  // command (SM-UP-1).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-UP-1
  import { onMount, type Snippet } from 'svelte';
  import { answers } from '../backend/groups';
  import { registerActionHandler } from '../search/actions';
  import { i18n, t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import type { ServiceError } from '../service-api/types.gen';
  import { getApp } from './context';
  import { openScreen } from './open-item';
  import { ScreenAreaState } from './screen-area.svelte';
  import { makeKey, parseKey, SCREEN_IDS } from './screens';
  import OpenTeamWorkspaceDialog from '../views/fixed/OpenTeamWorkspaceDialog.svelte';
  import { offersTeamWorkspace } from '../views/fixed/team-workspace.svelte';
  import WorkspaceTree from './WorkspaceTree.svelte';
  import LifecycleFoot from '../views/workspaces/LifecycleFoot.svelte';

  // The pane's collapse button, drawn by the window at the end of the Home and Search row.
  let { toggle }: { toggle?: Snippet | undefined } = $props();

  const app = getApp();
  const local = $derived(app.local);
  const backends = $derived(app.stores.all().sort((a, b) => (a.kind === b.kind ? 0 : a.kind === 'local' ? -1 : 1)));
  const homeCount = $derived(backends.reduce((n, b) => n + b.needsYouCount, 0));
  // Home is drawn pressed while it is the screen in front (scene `home`).
  const inFront = $derived(app.screens.front(ScreenAreaState.scope(app.shell.openBackend, app.shell.openWorkspace)));
  const homeInFront = $derived(inFront === SCREEN_IDS.home);
  // Settings is pressed while it, or one of its sections, is in front (SM-RAIL-1).
  // spec: ordin-desktop/components/ScreensModule#SM-RAIL-1
  const settingsInFront = $derived(inFront !== null && parseKey(inFront).id === SCREEN_IDS.settings);

  let naming = $state(false);
  let newName = $state('');
  let nameField = $state<HTMLInputElement | null>(null);
  // SM-WS-1: the name field takes the cursor as soon as it opens (plan 04 FU-017).
  // spec: ordin-desktop/components/ScreensModule#SM-WS-1
  $effect(() => {
    if (naming) nameField?.focus();
  });
  let createError = $state<ServiceError | null>(null);

  // SM-TW-3: Open a team workspace… under New workspace, on the local backend only (SM-TW-12).
  // spec: ordin-desktop/components/ScreensModule#SM-TW-3
  // spec: ordin-desktop/components/ScreensModule#SM-TW-12
  let openingTeam = $state(false);

  // New workspace and Open a team workspace… live in a menu opened from the icon button beside the
  // Workspaces heading.
  let workspaceMenuOpen = $state(false);
  let workspaceMenuEl = $state<HTMLElement | null>(null);
  let workspaceMenuButton = $state<HTMLElement | null>(null);
  const canCreate = $derived(answers(local, 'workspaces.create'));
  const canOpenTeam = $derived(offersTeamWorkspace(local, 'team_knowledge.workspace_open_preview'));
  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }
  function closeWorkspaceMenu(refocus: boolean): void {
    workspaceMenuOpen = false;
    if (refocus) workspaceMenuButton?.focus();
  }
  function menuKey(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      closeWorkspaceMenu(true);
      return;
    }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    const items = [...(workspaceMenuEl?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    if (items.length === 0) return;
    event.preventDefault();
    const at = items.indexOf(document.activeElement as HTMLElement);
    const to = event.key === 'ArrowDown' ? (at + 1) % items.length : (at - 1 + items.length) % items.length;
    items[to]?.focus();
  }

  /** The new workspace appears in the tree and opens in the centre with focus. */
  async function teamWorkspaceOpened(workspaceId: string): Promise<void> {
    await local.handle.call('workspaces.list', { filter: 'active' }).then((r) => {
      if (r.kind === 'ok') local.workspaces = r.result;
    });
    app.shell.openWorkspaceTab(local.id, workspaceId);
  }

  // The Actions group of *Search or go to…*: New workspace opens the rail's name field (SM-SF-9).
  onMount(() => {
    registerActionHandler('new_workspace', () => (naming = true));
  });

  async function createWorkspace(): Promise<void> {
    const name = newName.trim();
    if (name === '') return;
    const out = await local.handle.call('workspaces.create', { name });
    if (out.kind === 'ok') {
      createError = null;
      naming = false;
      newName = '';
      await local.handle.call('workspaces.list', { filter: 'active' }).then((r) => {
        if (r.kind === 'ok') local.workspaces = r.result;
      });
      app.shell.openWorkspaceTab(local.id, out.result.id);
    } else {
      createError = out.error;
    }
  }
</script>

<svelte:window
  onpointerdown={(e) => {
    if (workspaceMenuOpen && e.target instanceof Node && !workspaceMenuEl?.contains(e.target) && !workspaceMenuButton?.contains(e.target)) closeWorkspaceMenu(false);
  }}
/>

<nav class="side-rail" aria-label={i18n.product}>
  <p class="brand">{i18n.product}</p>
  <!-- Home and Search as icon buttons, in the row of the pane's collapse button. -->
  <div class="find-row">
    {#if answers(local, 'home.view')}
      {@const homeLabel = homeCount > 0 ? t('rail-home-count', { count: homeCount }) : t('rail-home')}
      <button
        type="button"
        class="icon-btn rail-home"
        aria-pressed={homeInFront}
        aria-label={homeLabel}
        title={homeLabel}
        onclick={() => openScreen(app, local.id, null, SCREEN_IDS.home)}
      >
        <Icon name="home" size="small" />{#if homeCount > 0}<span class="count rail-count" aria-hidden="true">{homeCount}</span>{/if}
      </button>
    {/if}
    {#if answers(local, 'search.query')}
      <button type="button" class="icon-btn rail-search" aria-label={t('rail-search')} title={t('rail-search')} onclick={() => (app.shell.searchOpen = true)}><Icon name="search" size="small" /></button>
    {/if}
    <span class="find-gap"></span>
    {@render toggle?.()}
  </div>
  <section class="workspaces" aria-labelledby="rail-workspaces-heading">
    <div class="workspaces-head">
      <h2 id="rail-workspaces-heading">{t('rail-workspaces')}</h2>
      {#if (canCreate && !naming) || canOpenTeam}
        <button
          bind:this={workspaceMenuButton}
          type="button"
          class="icon-btn"
          aria-haspopup="menu"
          aria-expanded={workspaceMenuOpen}
          aria-label={t('more-actions-tip')}
          title={t('more-actions-tip')}
          onclick={() => (workspaceMenuOpen = !workspaceMenuOpen)}
        ><Icon name="plus" size="small" /></button>
      {/if}
      {#if workspaceMenuOpen}
        <div bind:this={workspaceMenuEl} role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('rail-workspaces')} use:focusFirst onkeydown={menuKey}>
          {#if canCreate && !naming}<button type="button" role="menuitem" onclick={() => { closeWorkspaceMenu(false); naming = true; }}><Icon name="plus" />{t('rail-new-workspace')}</button>{/if}
          {#if canOpenTeam}<button type="button" role="menuitem" onclick={() => { closeWorkspaceMenu(false); openingTeam = true; }}><Icon name="open" />{t('rail-open-team-workspace')}</button>{/if}
        </div>
      {/if}
    </div>
    {#if answers(local, 'workspaces.create') && naming}
      <form
        class="new-workspace"
        onsubmit={(e) => {
          e.preventDefault();
          void createWorkspace();
        }}
      >
        <label>
          {t('rail-new-workspace')}
          <input bind:this={nameField} bind:value={newName} />
        </label>
        <div class="new-workspace-buttons">
          <button type="submit" class="small" data-default="true" disabled={newName.trim() === ''}>{t('rail-new-workspace-create')}</button>
          <button type="button" class="small" onclick={() => (naming = false)}>{t('confirm-cancel')}</button>
        </div>
        <ErrorLine error={createError} onretry={() => void createWorkspace()} />
      </form>
    {/if}
    {#if openingTeam}
      <OpenTeamWorkspaceDialog
        handle={local.handle}
        onopened={(id) => void teamWorkspaceOpened(id)}
        onsignin={() => openScreen(app, local.id, null, makeKey(SCREEN_IDS.settings, 'sign-ins'))}
        onclose={() => (openingTeam = false)}
      />
    {/if}
    <WorkspaceTree {backends} />
    <!-- SM-WS-5, SM-WS-7: Archived and the Bin at the foot of the tree. -->
    <LifecycleFoot backend={local} />
  </section>
  <!-- SM-RAIL-1: Settings at the foot of the rail, opened where the person is (SM-OS-1). -->
  {#if answers(local, 'settings.get')}
    <div class="rail-foot">
      <button type="button" class="rail-entry rail-settings" aria-pressed={settingsInFront} onclick={() => openScreen(app, local.id, null, SCREEN_IDS.settings)}>
        {t('rail-settings')}
      </button>
    </div>
  {/if}
</nav>

<style>
  /* The side rail (cross-cutting/VisualDesign.md V5, V6; scene `home`, `tree`). */
  .side-rail {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    min-block-size: 100%;
  }
  .brand {
    font-weight: 700;
    font-size: 18px;
    color: var(--color-text);
  }
  .find-row {
    display: flex;
    gap: var(--space-04);
    align-items: center;
  }
  .find-gap {
    flex: 1;
  }
  /* The editor toolbar's icon button (MarkdownEditor.svelte `.md-tool`). */
  .icon-btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    min-height: 0;
    padding: 0;
  }
  .icon-btn[aria-pressed='true'],
  .icon-btn[aria-expanded='true'] {
    background: var(--color-accent-soft);
    color: var(--color-accent-ink);
  }
  /* Home's count sits on the corner of its button. */
  .rail-count {
    position: absolute;
    inset-block-start: -4px;
    inset-inline-end: -6px;
    font-size: 10px;
  }
  .rail-entry {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    border: 1px solid var(--color-line);
    background: var(--color-surface);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-10);
    min-height: 40px;
    text-align: start;
    font-size: 16px;
  }
  .rail-entry:hover {
    border-color: var(--color-line-strong);
  }
  .rail-entry[aria-pressed='true'] {
    background: var(--color-accent-soft);
    border-color: var(--color-line);
    color: var(--color-text);
    font-weight: 400;
  }
  .rail-foot {
    margin-block-start: auto;
    display: flex;
    flex-direction: column;
  }
  .workspaces {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .workspaces-head {
    position: relative;
    display: flex;
    gap: var(--space-06);
    align-items: center;
    justify-content: space-between;
  }
  .workspaces-head > h2 {
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
  }
  .new-workspace {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    font-size: 14px;
  }
  .new-workspace label {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  .new-workspace-buttons {
    display: flex;
    gap: var(--space-06);
  }
  /* A tool button carries its words beside its icon. */
</style>
