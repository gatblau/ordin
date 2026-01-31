<script lang="ts">
  // The left rail: brand, Home with its count, Search or go to…, New workspace, the workspace tree,
  // then Settings at the foot (SM-RAIL-1). Each entry is drawn only when the backend answers its
  // command (SM-UP-1).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-UP-1
  import { onMount } from 'svelte';
  import { answers } from '../backend/groups';
  import { registerActionHandler } from '../search/actions';
  import { i18n, t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import type { ServiceError } from '../service-api/types.gen';
  import { getApp } from './context';
  import { openScreen } from './open-item';
  import { ScreenAreaState } from './screen-area.svelte';
  import { parseKey, SCREEN_IDS } from './screens';
  import WorkspaceTree from './WorkspaceTree.svelte';
  import LifecycleFoot from '../views/workspaces/LifecycleFoot.svelte';

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

<nav class="side-rail" aria-label={i18n.product}>
  <p class="brand">{i18n.product}</p>
  <div class="find-row">
    {#if answers(local, 'home.view')}
      <button
        type="button"
        class="rail-entry"
        aria-pressed={homeInFront}
        aria-label={homeCount > 0 ? t('rail-home-count', { count: homeCount }) : t('rail-home')}
        onclick={() => openScreen(app, local.id, null, SCREEN_IDS.home)}
      >
        <Icon name="home" />{t('rail-home')}{#if homeCount > 0}<span class="count rail-count">{homeCount}</span>{/if}
      </button>
    {/if}
    {#if answers(local, 'search.query')}
      <button type="button" class="rail-entry find" onclick={() => (app.shell.searchOpen = true)}><Icon name="search" /><span class="find-words">{t('rail-search')}</span></button>
    {/if}
  </div>
  <section class="workspaces" aria-labelledby="rail-workspaces-heading">
    <h2 id="rail-workspaces-heading">
      {t('rail-workspaces')}
      {#if answers(local, 'workspaces.create') && !naming}
        <button type="button" class="keep" onclick={() => (naming = true)}><Icon name="plus" />{t('rail-new-workspace')}</button>
      {/if}
    </h2>
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
    gap: var(--space-06);
    align-items: stretch;
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
  .rail-entry.find {
    flex: 1;
    min-width: 0;
    color: var(--color-text-secondary);
  }
  .find-words {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  .workspaces > h2 {
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
    display: flex;
    gap: var(--space-06);
    align-items: center;
    flex-wrap: wrap;
  }
  .keep {
    margin-inline-start: auto;
    text-transform: none;
    letter-spacing: 0;
    color: var(--color-text);
    border: 1px solid var(--color-line);
    background: none;
    border-radius: var(--radius-xs);
    block-size: 28px;
    min-height: 0;
    padding: 0 var(--space-sm);
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    display: inline-flex;
    align-items: center;
    gap: var(--space-04);
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
</style>
