<script lang="ts">
  // A document's ⋯ menu in the side tree and on the Documents overview (SM-PUB-1, SM-DS-4): **Open** and
  // **Publish to…**. The destinations are read when the menu opens; publishing opens the Publish to
  // dialog, which outlives the menu.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-PUB-1
  import { answers } from '../../backend/groups';
  import { askOrdin, isWaitingForServer } from '../../blocks/ask';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { BackendState } from '../../store/backend-state.svelte';
  import { appOrNull } from '../fixed/util';
  import { loadDestinations, type PublishSystem } from './destinations';
  import PublishToDialog from './PublishToDialog.svelte';
  import PublishToItems from './PublishToItems.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    placeId: string;
    path: string;
    name: string;
    onopen: () => void;
  }
  let { backend, workspaceId, placeId, path, name, onopen }: Props = $props();

  const app = appOrNull();
  let open = $state(false);
  let destinations = $state.raw<PublishSystem[]>([]);
  let publishing = $state<PublishSystem | null>(null);

  // The same Ask Ordin items as the tree's document menu (TreeFolder.svelte, SM-ASK-1).
  const canAsk = $derived(backend.groups.has('ordin') && answers(backend, 'ordin.send_message'));
  async function ask(text: 'block-ask-text-this' | 'tree-ask-text-knowledge' | 'tree-ask-text-memory'): Promise<void> {
    open = false;
    if (text !== 'block-ask-text-this' && app !== null) app.shell.openWorkspaceTab(backend.id, workspaceId);
    await askOrdin(backend, workspaceId, t(text, { title: name }), [{ kind: 'place_file', place_id: placeId, relative_path: path }]);
  }

  async function toggle(): Promise<void> {
    open = !open;
    if (open) destinations = await loadDestinations(backend, workspaceId, placeId, name);
  }

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  function closed(sent: boolean): void {
    publishing = null;
    if (sent) app?.shell.announce(t('publish-to-sent', { name }));
  }
</script>

<span class="tree-actions" class:shown={open}>
  <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={open} aria-label={t('more-actions-for', { name })} title={t('more-actions-tip')} onclick={(e) => { e.stopPropagation(); void toggle(); }}><Icon name="more" /></button>
</span>
{#if open}
  <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name })} use:focusFirst onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); open = false; } }}>
    <button type="button" role="menuitem" onclick={() => { open = false; onopen(); }}><Icon name="open" />{t('doc-menu-open')}</button>
    {#if canAsk}<button type="button" role="menuitem" disabled={isWaitingForServer(backend)} onclick={() => void ask('block-ask-text-this')}><Icon name="chat" />{t('block-ask-about-this')}</button>{/if}
    {#if canAsk}<button type="button" role="menuitem" disabled={isWaitingForServer(backend)} onclick={() => void ask('tree-ask-text-knowledge')}><Icon name="know" />{t('tree-add-to-knowledge')}</button>{/if}
    {#if canAsk}<button type="button" role="menuitem" disabled={isWaitingForServer(backend)} onclick={() => void ask('tree-ask-text-memory')}><Icon name="memory" />{t('tree-add-to-memory')}</button>{/if}
    <PublishToItems {destinations} onchoose={(system) => { open = false; publishing = system; }} />
  </div>
{/if}
{#if publishing !== null}
  <PublishToDialog {backend} {workspaceId} {placeId} {path} {name} system={publishing} onclose={closed} />
{/if}
