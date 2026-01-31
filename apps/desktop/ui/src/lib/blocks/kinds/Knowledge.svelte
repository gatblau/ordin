<script lang="ts">
  // `knowledge`: the workspace's pages with their sources, or the one page the block names. Editing and
  // publishing happen on the knowledge screen, opened with **Open**.
  import { t } from '../../i18n/fluent';
  import type { PageSummary, ServiceError } from '../../service-api/types.gen';
  import { getApp } from '../../shell/context';
  import { openScreen } from '../../shell/open-item';
  import { makeKey, SCREEN_IDS } from '../../shell/screens';
  import Icon from '../../icons/Icon.svelte';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import AskButton from '../AskButton.svelte';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'knowledge'> = $props();
  const app = getApp();

  let pages = $state<PageSummary[]>([]);
  let loaded = $state(false);
  let error = $state<ServiceError | null>(null);

  $effect(() => {
    const only = block.page ?? null;
    loaded = false;
    if (!backend.groups.has('knowledge')) {
      loaded = true;
      return;
    }
    let current = true;
    void backend.handle.call('knowledge.list', { workspace_id: block.workspace_id }).then((out) => {
      if (!current) return;
      if (out.kind === 'ok') {
        pages = only === null ? out.result.this_workspace : out.result.this_workspace.filter((p) => p.id === only);
        error = null;
      } else {
        error = out.error;
      }
      loaded = true;
    });
    return () => {
      current = false;
    };
  });
</script>

<div class="block-knowledge">
  {#if loaded && pages.length === 0 && error === null}<p>{t('block-cannot-show')}</p>{/if}
  <ul class="list-rows">
    {#each pages as page (page.id)}
      <li class="list-row">
        <span class="icon-tile" data-tint="memory"><Icon name="know" /></span>
        <span class="grow">
          <b>{page.title}</b>
          {#if page.description}<small>{page.description}</small>{/if}
        </span>
      </li>
    {/each}
  </ul>
  <div class="actions">
    <button type="button" onclick={() => openScreen(app, backend.id, workspaceId, block.page ? makeKey(SCREEN_IDS.knowledge, block.page) : SCREEN_IDS.knowledge)}>{t('card-open')}</button>
    <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: t('tree-knowledge') })} />
  </div>
  <ErrorLine {error} />
</div>

<style>
  /* The prototype's knowledge page rows (`.libl`, `.librow`; scene `know`): the shared `.list-rows`. */
  .block-knowledge {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .actions {
    display: flex;
    gap: var(--space-sm);
  }
</style>
