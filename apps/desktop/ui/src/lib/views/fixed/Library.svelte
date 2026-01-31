<script lang="ts">
  // The agent library (SM-AL-1 to SM-AL-4): the agents the catalogue offers, with a search box; the agent
  // Ordin proposed (the screen's instance) first and chosen; the chosen agent's detail from
  // `agents.preview_add`, with its one action, **Add it**, beside its name. No IT action is offered.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-AL-1
  // spec: ordin-desktop/components/ScreensModule#SM-AL-2
  // spec: ordin-desktop/components/ScreensModule#SM-AL-3
  // spec: ordin-desktop/components/ScreensModule#SM-AL-4
  // spec: ordin-desktop/components/ScreensModule#SM-AL-6
  import AskButton from '../../blocks/AskButton.svelte';
  import { i18n, t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { AddAgentInput, AddPreview, Availability, LibraryEntry, ServiceError } from '../../service-api/types.gen';
  import type { ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';

  let { backend, workspaceId, instance }: ScreenProps = $props();

  /** SM-AL-6: ordin's own agents (Ordin and the agent maker, PackageCatalogue.md PKG-OWN-1), in the bundled
   *  catalogue, are never offered to add. */
  const BUNDLED_REPOSITORY_ID = '01900000-0000-7000-8000-000000000001';
  const OWN_AGENTS = ['urn:ordin:default:workspaces:coordinator', 'urn:ordin:default:workspaces:advisor'];
  const isOwnAgent = (e: LibraryEntry): boolean => e.part.part.agent_repository_id === BUNDLED_REPOSITORY_ID && OWN_AGENTS.includes(e.part.part.identifier);

  // svelte-ignore state_referenced_locally
  const suggested: string | null = instance;
  let query = $state('');
  let entries = $state<LibraryEntry[]>([]);
  let loaded = $state(false);
  let listError = $state<ServiceError | null>(null);

  let chosen = $state<LibraryEntry | null>(null);
  let preview = $state<AddPreview | null>(null);
  let alreadyAdded = $state(false);
  let previewError = $state<ServiceError | null>(null);
  /** `idle` before Add it, `adding` while it is in flight, `done` once an outcome says the action is gone. */
  let adding = $state<'idle' | 'adding' | 'done'>('idle');
  let outcomeId = $state<string | null>(null);
  let addError = $state<ServiceError | null>(null);

  /** The suggested agent first; the others in the catalogue's order. */
  const ordered = $derived(
    suggested === null ? entries : [...entries.filter((e) => e.part.part.identifier === suggested), ...entries.filter((e) => e.part.part.identifier !== suggested)],
  );

  async function search(): Promise<void> {
    const text = query.trim();
    const out = await backend.handle.call('catalogue.search', { kinds: ['WorkspaceProfile'], include_not_for_new: false, repositories: [], ...(text !== '' ? { text } : {}) });
    loaded = true;
    if (out.kind === 'ok') {
      entries = out.result.items.filter((e) => !isOwnAgent(e));
      listError = null;
      if (chosen === null && suggested !== null) {
        const wanted = entries.find((e) => e.part.part.identifier === suggested);
        if (wanted !== undefined) void choose(wanted);
      }
    } else {
      listError = out.error;
    }
  }
  $effect(() => {
    void search();
  });

  function input(entry: LibraryEntry): AddAgentInput {
    return { workspace_id: workspaceId ?? '', agent: entry.part.part.id, choices: [], interface_locale: i18n.locale };
  }

  /** The one sentence for an agent that cannot be added (SM-AL-2); `null` when it can. */
  function unavailableId(available: Availability): string | null {
    switch (available.kind) {
      case 'available':
        return null;
      case 'pack_missing':
        return 'library-unavailable';
      case 'needs_it_approval':
        return 'library-needs-approval';
      case 'unsafe':
        return 'library-unsafe';
    }
  }

  async function choose(entry: LibraryEntry): Promise<void> {
    chosen = entry;
    preview = null;
    alreadyAdded = false;
    previewError = null;
    adding = 'idle';
    outcomeId = null;
    addError = null;
    if (workspaceId === null) return;
    const out = await backend.handle.call('agents.preview_add', input(entry));
    if (chosen !== entry) return;
    if (out.kind === 'ok') preview = out.result;
    else if (out.error.code === 'PKG_ALREADY_ADDED') alreadyAdded = true;
    else previewError = out.error;
  }

  async function add(entry: LibraryEntry): Promise<void> {
    adding = 'adding';
    addError = null;
    const out = await backend.handle.call('agents.add', input(entry));
    if (out.kind !== 'ok') {
      adding = 'idle';
      addError = out.error;
      return;
    }
    adding = 'done';
    outcomeId = out.result.kind === 'ready' ? 'library-added' : unavailableId(out.result);
  }
</script>

<div class="library">
  <form
    role="search"
    onsubmit={(e) => {
      e.preventDefault();
      void search();
    }}
  >
    <input type="search" bind:value={query} aria-label={t('block-agent-search')} placeholder={t('block-agent-search')} />
  </form>
  <ErrorLine error={listError} onretry={() => void search()} />
  {#if loaded && ordered.length === 0 && listError === null}<p>{t('block-agent-none')}</p>{/if}
  <ul class="list-rows">
    {#each ordered as entry (entry.part.part.id)}
      <li class="list-row" class:chosen={chosen === entry}>
        <button type="button" class="row" aria-pressed={chosen === entry} onclick={() => void choose(entry)}>
          <span class="icon-tile" data-tint="accent-ink"><Icon name="agent" /></span>
          <span class="grow">
            <b>{entry.part.part.name}</b>
            {#if entry.part.part.identifier === suggested}<small class="suggested">{t('library-suggested')}</small>{/if}
            <small>{entry.part.summary}</small>
            <small>{t('library-from', { from: entry.from })}</small>
          </span>
        </button>
      </li>
    {/each}
  </ul>

  {#if chosen !== null}
    {@const entry = chosen}
    <section class="detail" aria-labelledby="library-chosen-heading">
      <div class="head">
        <h3 id="library-chosen-heading">{entry.part.part.name}</h3>
        {#if preview !== null && adding !== 'done' && unavailableId(preview.available) === null}
          <button type="button" class="primary" disabled={adding === 'adding'} onclick={() => void add(entry)}>
            {adding === 'adding' ? t('library-adding', { agent: entry.part.part.name }) : t('library-add')}
          </button>
        {/if}
      </div>
      <p>{entry.part.summary}</p>
      <p>{t('library-from', { from: entry.from })}</p>
      {#if preview !== null && preview.uses.length > 0}
        <p>{t('library-uses', { uses: preview.uses.map((u) => u.label).join(', ') })}</p>
      {/if}
      {#if alreadyAdded}<p role="status">{t('library-already-added')}</p>{/if}
      {#if preview !== null && adding !== 'done' && unavailableId(preview.available) !== null}
        <p role="status">{t(unavailableId(preview.available) as string)}</p>
      {/if}
      {#if outcomeId !== null}<p role="status">{t(outcomeId, { agent: entry.part.part.name })}</p>{/if}
      <ErrorLine error={previewError} onretry={() => void choose(entry)} />
      <ErrorLine error={addError} />
      {#if workspaceId !== null}
        <AskButton {backend} {workspaceId} labelId="block-ask-about-agent" text={t('block-ask-text-agent', { agent: entry.part.part.name })} />
      {/if}
    </section>
  {/if}
</div>

<style>
  .library {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .row {
    display: flex;
    gap: var(--space-sm);
    align-items: flex-start;
    width: 100%;
    text-align: start;
    background: none;
    border: 0;
    padding: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .row .grow {
    display: flex;
    flex-direction: column;
  }
  .list-row.chosen {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .suggested {
    font-weight: 600;
  }
  .detail .head {
    display: flex;
    gap: var(--space-sm);
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
  }
</style>
