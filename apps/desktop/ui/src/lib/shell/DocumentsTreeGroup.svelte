<script lang="ts">
  // The Documents row under a workspace: its main action opens the Documents screen and shows, under it, one
  // row per active link grouped by where it lives (SM-RL-1), each opening to its folders and documents (SM-RL-3); its **+** action opens *Add to this workspace*
  // (SM-AW-1, `tree-add-documents`). Each link row's ⋯ menu holds **Remove from this workspace…**, which asks
  // first and then sends `workspaces.remove_link` (SM-RL-2).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-AW-1
  // spec: ordin-desktop/components/ScreensModule#SM-RL-1
  // spec: ordin-desktop/components/ScreensModule#SM-RL-2
  // spec: ordin-desktop/components/ScreensModule#SM-RL-3
  import { answers } from '../backend/groups';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { SourceLink } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import { ADD_TO_WORKSPACE_SCREEN } from '../views/add/add-model.svelte';
  import { DocumentLinksModel, LINK_KIND_ICON } from '../views/add/links-model.svelte';
  import ConfirmDialog from '../views/ConfirmDialog.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import LinkItems from '../views/fixed/LinkItems.svelte';
  import { appOrNull } from '../views/fixed/util';
  import { openScreen } from './open-item';
  import { makeKey, SCREEN_IDS } from './screens';
  import TreeFolder from './TreeFolder.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    model?: DocumentLinksModel;
    initiallyOpen?: boolean;
    /** The link whose ⋯ menu is open when drawn (a preview or a test). */
    initialMenu?: string | null;
  }
  let { backend, workspaceId, model: given, initiallyOpen = false, initialMenu = null }: Props = $props();

  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const model = given ?? new DocumentLinksModel(backend.handle, workspaceId);
  // svelte-ignore state_referenced_locally
  let open = $state(initiallyOpen);
  // svelte-ignore state_referenced_locally
  let menuFor = $state<string | null>(initialMenu);
  /** The links whose items are shown under them (a Jira issue, or a remote link with no place yet). */
  let itemsOpen = $state<string[]>([]);
  const revision = $derived(backend.workspaceRevision(workspaceId));
  const listsLinks = $derived(answers(backend, 'workspaces.links'));
  const canAdd = $derived(answers(backend, 'workspaces.add_link'));
  const canRemove = $derived(answers(backend, 'workspaces.remove_link'));

  $effect(() => {
    void revision;
    if (open && listsLinks) void model.load();
  });

  function openDocuments(): void {
    open = !open;
    if (app !== null) openScreen(app, backend.id, workspaceId, SCREEN_IDS.documents);
  }

  function addDocuments(): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, ADD_TO_WORKSPACE_SCREEN);
  }

  // A link's main action opens its place's documents and shows, under it, the place's folders and documents
  // (SM-RL-3); a Jira issue, or a remote link whose working copy does not exist yet, shows its items under
  // it (SM-WR-5).
  function openLink(link: SourceLink): void {
    menuFor = null;
    if (link.kind !== 'jira_issue' && link.place_id != null && link.place_id !== '') {
      if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.documents, link.place_id), { title: link.display_name });
    }
    itemsOpen = itemsOpen.includes(link.id) ? itemsOpen.filter((id) => id !== link.id) : [...itemsOpen, link.id];
  }

  function askRemove(link: SourceLink): void {
    menuFor = null;
    model.askRemove(link);
  }

  async function confirmRemove(): Promise<void> {
    const name = await model.confirmRemove();
    if (name !== null) app?.shell.announce(t('remove-link-done', { name }));
  }

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  function menuKey(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    menuFor = null;
  }
</script>

<div role="treeitem" aria-selected="false" aria-expanded={listsLinks ? open : undefined} tabindex="-1" class="tree-leaf">
  <div class="tree-line">
    <button type="button" class="tree-row" onclick={openDocuments}><span class="icon-tile"><Icon name="docs" /></span>{t('tree-documents')}</button>
    {#if canAdd}
      <span class="tree-actions">
        <button type="button" class="tree-action" aria-label={t('tree-add-documents')} title={t('tree-add-documents')} onclick={addDocuments}><Icon name="plus" /></button>
      </span>
    {/if}
  </div>
  {#if open && listsLinks}
    <ErrorLine error={model.error} onretry={() => void model.load()} />
    <div role="group" class="tree-group">
      {#each model.links as link (link.id)}
        <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf" aria-expanded={itemsOpen.includes(link.id)}>
          <div class="tree-line">
            <button type="button" class="tree-row leaf tree-link" onclick={() => openLink(link)}><span class="icon-tile" data-tint={LINK_KIND_ICON[link.kind]?.tint}><Icon name={LINK_KIND_ICON[link.kind]?.icon ?? 'docs'} /></span><span class="tree-link-name">{link.display_name}</span></button>
            {#if canRemove}
              <span class="tree-actions" class:shown={menuFor === link.id}>
                <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuFor === link.id} aria-label={t('more-actions-for', { name: link.display_name })} title={t('more-actions-tip')} onclick={() => (menuFor = menuFor === link.id ? null : link.id)}><Icon name="more" /></button>
              </span>
              {#if menuFor === link.id}
                <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: link.display_name })} onkeydown={menuKey} use:focusFirst>
                  <button type="button" role="menuitem" class="danger" onclick={() => askRemove(link)}><Icon name="remove" />{t('tree-remove-link')}</button>
                </div>
              {/if}
            {/if}
          </div>
          {#if itemsOpen.includes(link.id)}
            {#if link.kind !== 'jira_issue' && link.place_id != null && link.place_id !== ''}
              <TreeFolder {backend} {workspaceId} placeId={link.place_id} />
            {:else}
              <LinkItems {backend} {workspaceId} sourceLinkId={link.id} />
            {/if}
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if model.removing !== null}
  {@const link = model.removing}
  <ConfirmDialog
    title={t('remove-link-title', { name: link.display_name })}
    body={t('remove-link-body')}
    verb={t('remove-link-verb')}
    onconfirm={() => void confirmRemove()}
    oncancel={() => model.cancelRemove()}
  >
    {#if link.kind === 'jira_issue'}<p class="dialog-body">{t('jira-remove-line')}</p>{/if}
    <ErrorLine error={model.removeError} />
  </ConfirmDialog>
{/if}

<style>
  /* A link row sits one level below the Documents row (the prototype's `.trow` at `--lv:2`). */
  .tree-link {
    padding-inline-start: calc(2px + 28px + 23px);
  }
  .tree-link-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
