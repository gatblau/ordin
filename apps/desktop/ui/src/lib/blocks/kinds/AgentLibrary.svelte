<script lang="ts">
  // `agent_library`: the agents the catalogue offers, with a search box. The block names the screen where an
  // agent is added (the library screen, SM-AL-1); here each entry shows its name, what it does and who it is from, with **Ask Ordin about
  // this agent**.
  import { t } from '../../i18n/fluent';
  import type { LibraryEntry, ServiceError } from '../../service-api/types.gen';
  import { getApp } from '../../shell/context';
  import { openScreen } from '../../shell/open-item';
  import { SCREEN_IDS } from '../../shell/screens';
  import Icon from '../../icons/Icon.svelte';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import AskButton from '../AskButton.svelte';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'agent_library'> = $props();
  const app = getApp();

  // svelte-ignore state_referenced_locally
  let query = $state(block.search ?? '');
  let entries = $state<LibraryEntry[]>([]);
  let loaded = $state(false);
  let error = $state<ServiceError | null>(null);

  async function search(): Promise<void> {
    if (!backend.groups.has('catalogue')) {
      loaded = true;
      return;
    }
    const text = query.trim();
    const out = await backend.handle.call('catalogue.search', { kinds: ['WorkspaceProfile'], include_not_for_new: false, repositories: [], ...(text !== '' ? { text } : {}) });
    loaded = true;
    if (out.kind === 'ok') {
      entries = out.result.items;
      error = null;
    } else {
      error = out.error;
    }
  }
  $effect(() => {
    void search();
  });
</script>

<div class="block-agent-library">
  <form
    role="search"
    onsubmit={(e) => {
      e.preventDefault();
      void search();
    }}
  >
    <input type="search" bind:value={query} aria-label={t('block-agent-search')} placeholder={t('block-agent-search')} />
  </form>
  {#if loaded && entries.length === 0 && error === null}<p>{t('block-agent-none')}</p>{/if}
  <ul class="list-rows">
    {#each entries as entry (entry.part.part.id)}
      <li class="list-row">
        <span class="icon-tile" data-tint="accent-ink"><Icon name="agent" /></span>
        <div class="grow">
          <b>{entry.part.part.name}</b>
          <small>{entry.part.summary}</small>
          <small>{entry.from}</small>
          <div class="ask"><AskButton {backend} {workspaceId} labelId="block-ask-about-agent" text={t('block-ask-text-agent', { agent: entry.part.part.name })} /></div>
        </div>
      </li>
    {/each}
  </ul>
  <button type="button" onclick={() => openScreen(app, backend.id, workspaceId, SCREEN_IDS.library)}>{t('card-open')}</button>
  <ErrorLine {error} onretry={() => void search()} />
</div>

<style>
  /* The prototype's `.libl` and `.librow` (scenes `library`, `libraryRepos`): the shared `.list-rows` rows,
     an agent icon in accent ink, the name, then what it does and where it is from in small muted type. */
  .block-agent-library {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    align-items: flex-start;
  }
  .block-agent-library form,
  .block-agent-library input[type='search'],
  .list-rows {
    align-self: stretch;
  }
  .ask {
    margin-top: var(--space-06);
  }
</style>
