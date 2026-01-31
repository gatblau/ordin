<script lang="ts">
  // *Earlier conversations* under a workspace (SM-WR-7): expanded, it lists the chains and is read again on
  // each `workspace_changed` for the workspace while it is expanded.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WR-7
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { BackendState } from '../store/backend-state.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { appOrNull } from '../views/fixed/util';
  import { EarlierTreeModel, intendOnOpen, type EarlierIntent, type EarlierTreeNode } from '../views/earlier-tree.svelte';
  import { openScreen } from './open-item';
  import { makeKey, SCREEN_IDS } from './screens';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    model?: EarlierTreeModel;
    initiallyOpen?: boolean;
    /** The conversation whose ⋯ menu is open when drawn (a preview or a test). */
    initialMenu?: string | null;
  }
  let { backend, workspaceId, model: given, initiallyOpen = false, initialMenu = null }: Props = $props();

  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const model = given ?? new EarlierTreeModel(backend.handle, workspaceId, t);
  // svelte-ignore state_referenced_locally
  let open = $state(initiallyOpen);
  // svelte-ignore state_referenced_locally
  let menuFor = $state<string | null>(initialMenu);
  const revision = $derived(backend.workspaceRevision(workspaceId));

  $effect(() => {
    void revision;
    if (open) void model.load();
  });

  function read(node: EarlierTreeNode, intent?: EarlierIntent): void {
    menuFor = null;
    if (app === null) return;
    if (node.kind === 'all' || node.conversationId === undefined) {
      openScreen(app, backend.id, workspaceId, SCREEN_IDS.earlierConversations);
      return;
    }
    if (intent !== undefined) intendOnOpen(backend.id, node.conversationId, intent);
    openScreen(app, backend.id, workspaceId, makeKey('earlier-conversation', node.conversationId), { title: node.name });
  }
</script>

<div role="treeitem" aria-selected="false" aria-expanded={open} tabindex="-1" class="tree-leaf">
  <button type="button" class="tree-row" onclick={() => (open = !open)}><span class="icon-tile"><Icon name="history" /></span>{t('conv-earlier-name')}</button>
  {#if open}
    <ErrorLine error={model.list.error} onretry={() => void model.load()} />
    <div role="group" class="tree-group">
      {#each model.nodes as node (node.conversationId ?? 'all')}
        <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf">
          <button type="button" class="tree-row leaf" onclick={() => read(node)}><span class="icon-tile"><Icon name={node.kind === 'all' ? 'history' : 'chat'} /></span>{node.name}</button>
          {#if node.kind === 'conversation' && node.conversationId !== undefined}
            {@const id = node.conversationId}
            <button type="button" class="tree-menu" aria-haspopup="menu" aria-expanded={menuFor === id} aria-label={t('more-actions-for', { name: node.name })} title={t('more-actions-tip')} onclick={() => (menuFor = menuFor === id ? null : id)}><Icon name="more" /></button>
            {#if menuFor === id}
              <div role="menu">
                <button type="button" role="menuitem" onclick={() => read(node)}><Icon name="eye" />{t('conv-read-it')}</button>
                <button type="button" role="menuitem" onclick={() => read(node, 'ask')}><Icon name="chat" />{t('conv-ask-about')}</button>
                <button type="button" role="menuitem" onclick={() => read(node, 'continue')}>{t('conv-continue')}</button>
              </div>
            {/if}
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
