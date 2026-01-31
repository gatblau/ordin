<script lang="ts">
  // A folder's Confluence publishing (SM-SF-7.1, SM-PC-1, SM-PC-2). The workspace's links are read with
  // `workspaces.confluence_links` when the folder opens and on each `workspace_changed`, unless a parent
  // gives them. For a link on this folder in state `proposed`, `active` or `paused`: its automatic-publishing
  // line (SM-SF-7), and for an `active` link its *Ready to publish* list with **Publish checked items…**
  // (SM-SF-8, SM-WR-13); for a `proposed` link, that it waits for the person's approval. The page offers
  // no publishing button: a document is published from its own menu, **Publish to…** (SM-PC-1 as amended,
  // SM-PUB-1).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-PC-2
  import { answers } from '../../backend/groups';
  import LinkedChecklist from '../../blocks/LinkedChecklist.svelte';
  import { ReadyListModel } from '../../blocks/ready-list.svelte';
  import { t } from '../../i18n/fluent';
  import type { ConfluenceLink, ServiceError } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import PublishingLine from './PublishingLine.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    placeId: string;
    /** The folder shown, relative to the place; empty is the place itself. */
    folder?: string;
    links?: ConfluenceLink[];
  }
  let { backend, workspaceId, placeId, folder = '', links: given }: Props = $props();

  let loaded = $state.raw<ConfluenceLink[]>([]);
  let error = $state<ServiceError | null>(null);
  const links = $derived(given ?? loaded);
  const revision = $derived(backend.workspaceRevision(workspaceId));
  // A folder row's path ends with `/` (files.list_folder); a link's folder does not.
  const here = $derived(folder.replace(/\/+$/, ''));
  const shown = $derived(links.filter((l) => l.place_id === placeId && l.folder === here && (l.state === 'proposed' || l.state === 'active' || l.state === 'paused')));
  const ready = new Map<string, ReadyListModel>();
  function readyOf(linkId: string): ReadyListModel {
    let model = ready.get(linkId);
    if (model === undefined) {
      model = new ReadyListModel(backend.handle, linkId);
      ready.set(linkId, model);
    }
    return model;
  }

  async function load(): Promise<void> {
    if (given !== undefined || !answers(backend, 'workspaces.confluence_links')) return;
    const out = await backend.handle.call('workspaces.confluence_links', { workspace_id: workspaceId });
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    error = null;
    loaded = out.result;
    await Promise.all(loaded.filter((l) => l.place_id === placeId && l.state === 'active').map((l) => readyOf(l.id).load()));
  }

  $effect(() => {
    void revision;
    void folder;
    void load();
  });
</script>

{#each shown as link (link.id)}
  {#if link.state === 'proposed'}
    <p class="secondary">{t('publish-confluence-waiting')}</p>
  {:else}
    <PublishingLine {backend} {workspaceId} linkId={link.id} />
  {/if}
  {#if link.state === 'active'}
    {@const model = readyOf(link.id)}
    <section class="ready-to-publish" aria-labelledby={`ready-${link.id}`}>
      <h4 id={`ready-${link.id}`}>{t('publish-confluence-ready-heading')}</h4>
      {#if model.items !== null && model.items.length === 0}
        <p>{t('publish-confluence-ready-none')}</p>
      {:else}
        <LinkedChecklist {backend} {workspaceId} confluenceLinkId={link.id} {model} />
      {/if}
    </section>
  {/if}
{/each}
<ErrorLine {error} onretry={() => void load()} />
