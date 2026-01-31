<script lang="ts">
  // The items of one remote link, drawn when its node is expanded (SM-WR-5). An item a Jira check found newer
  // at its origin shows `jira-item-changed` and **Bring in the new version**; nothing changes before the press,
  // and a refusal is drawn beside the item while the notice stays.
  //
  // With `changedOnly` (amended January 2026; user decision): only the items a check found newer are drawn,
  // each with its notice and button. The tree uses it under a Jira issue that has a place, whose documents
  // are listed and opened as a folder's are (SM-RL-3, SM-WR-5 item 2).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WR-5
  import { onMount } from 'svelte';

  import { formatTime } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { LinkItemsController } from './jira.svelte';
  import { lastSegment } from './util';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    sourceLinkId: string;
    controller?: LinkItemsController;
    /** Only the items found newer at their origin (the tree under a Jira issue with a place). */
    changedOnly?: boolean;
  }
  let { backend, workspaceId, sourceLinkId, controller, changedOnly = false }: Props = $props();

  // svelte-ignore state_referenced_locally
  const c = controller ?? new LinkItemsController(backend.handle, sourceLinkId);
  const revision = $derived(backend.workspaceRevision(workspaceId));
  let first = true;

  onMount(() => {
    void c.load();
  });
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    void c.load();
  });
</script>

<ErrorLine error={c.error} onretry={() => void c.load()} />
<ul class="link-items" class:changed-only={changedOnly}>
  {#each changedOnly ? c.items.filter((i) => LinkItemsController.changed(i)) : c.items as item (item.id)}
    <li>
      <span class="item-name">{lastSegment(item.origin_path)}</span>
      {#if LinkItemsController.changed(item)}
        <span class="item-changed">{t('jira-item-changed', { time: formatTime(item.origin_newer_at ?? '') })}</span>
        <button type="button" onclick={() => void c.bringIn(item)}>{t('jira-item-bring-in')}</button>
        <ErrorLine error={c.itemErrors.get(item.origin_path) ?? null} />
      {/if}
    </li>
  {/each}
</ul>

<style>
  /* Under a Jira issue in the tree: one line per changed item, indented with the issue's documents. */
  .link-items.changed-only {
    list-style: none;
    margin: 0;
    padding-inline-start: calc(2px + 28px + 23px + 23px);
  }
</style>
