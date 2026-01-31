<script lang="ts">
  // Search or go to… (11 §5.17): a dialog over the window, Ctrl+K / ⌘K anywhere. A WAI-ARIA combobox with a
  // listbox in three groups; the result count is announced as it changes; a result in another workspace
  // names it and, on use, switches first; Escape returns focus where it was; queries are debounced 120 ms.
  // The live region reads the number of results, or No results (SM-CU-6).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-CU-6
  import { tick } from 'svelte';

  import { answers } from '../backend/groups';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import { fileKindIcon } from '../icons/kinds';
  import type { IconName } from '../icons/sprite';
  import { actionHandler, type SearchActionName } from '../search/actions';
  import { getApp } from '../shell/context';
  import { openScreen, openTarget } from '../shell/open-item';
  import { SCREEN_IDS } from '../shell/screens';
  import { SEARCH_DEBOUNCE_MS, SearchModel, type SearchEntry, type SearchGroup } from './search.svelte';
  import ErrorLine from './ErrorLine.svelte';

  interface Props {
    model?: SearchModel;
  }
  let { model: given }: Props = $props();
  const app = getApp();
  const backend = $derived(app.local);
  // svelte-ignore state_referenced_locally
  const model =
    given ??
    new SearchModel({
      handle: app.local.handle,
      groups: app.local,
      inFront: () => app.shell.openWorkspace,
      diagnostics: app.diagnostics,
    });
  let input = $state<HTMLInputElement | null>(null);
  let opener: Element | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  // Offered only when the backend answers `search.query` (SM-UP-1).
  const open = $derived(app.shell.searchOpen && answers(app.local, 'search.query'));
  const GROUPS: Array<[SearchGroup, string]> = [
    ['go_to', 'search-go-to'],
    ['found', 'search-found'],
    ['actions', 'search-actions'],
  ];
  const groupEntries = (g: SearchGroup): SearchEntry[] => (g === 'go_to' ? model.goTo : g === 'found' ? model.found : model.actions);
  const all = $derived(model.entries);
  const announced = $derived(model.announcement);
  const workspaceName = (id: string | null): string => backend.workspaces.find((w) => w.id === id)?.name ?? '';

  // spec: ordin-desktop/components/ScreensModule#SM-IC-3
  // A result's icon (V8 *Search or go to…*, the prototype's `findAll`), untinted as the prototype draws
  // them. An agent that answers elsewhere would draw `remote`, but a result carries nothing that says so,
  // so every agent draws `agent`. A document draws its file kind's icon, else `word`; a Found result
  // draws the icon of what it opens, and none when that is neither a document nor a knowledge page.
  const GO_TO_ICON: Readonly<Record<string, IconName>> = {
    workspace: 'ws',
    agent: 'agent',
    workflow: 'keptcrew',
    dashboard: 'chart',
    knowledge_page: 'know',
    conversation: 'history',
  };
  const ACTION_ICON: Readonly<Record<SearchActionName, IconName>> = {
    home: 'home',
    new_workspace: 'plus',
    settings: 'key',
    add_documents: 'plus',
    summarise: 'fold',
    clear: 'chat',
    earlier_conversations: 'history',
  };
  function entryIcon(entry: SearchEntry): IconName | null {
    if (entry.action !== null) return ACTION_ICON[entry.action];
    if (entry.go !== null) return entry.go.kind === 'document' ? (fileKindIcon(entry.go.label) ?? 'word') : (GO_TO_ICON[entry.go.kind] ?? null);
    const opens = entry.found?.opens;
    if (opens?.kind === 'document') return fileKindIcon(opens.path) ?? 'word';
    if (opens?.kind === 'knowledge_page') return 'know';
    return null;
  }

  $effect(() => {
    if (open) {
      opener = document.activeElement;
      model.reset();
      void tick().then(() => input?.focus());
    }
  });

  function close(): void {
    clearTimeout(timer);
    app.shell.searchOpen = false;
    if (opener instanceof HTMLElement) opener.focus();
  }

  function oninput(value: string): void {
    model.query = value;
    clearTimeout(timer);
    timer = setTimeout(() => void model.run(), SEARCH_DEBOUNCE_MS);
  }

  function choose(entry: SearchEntry): void {
    const local = app.local.id;
    if (entry.action !== null) {
      const name = entry.action;
      close();
      const front = app.shell.openWorkspace;
      if (name === 'home') openScreen(app, local, null, SCREEN_IDS.home);
      else if (name === 'settings') openScreen(app, local, null, SCREEN_IDS.settings);
      else if (name === 'earlier_conversations') openScreen(app, local, front, SCREEN_IDS.earlierConversations);
      else actionHandler(name)?.(front);
      return;
    }
    const target = entry.go?.opens ?? entry.found?.opens ?? null;
    close();
    // An item (an agent, a workflow, a conversation) takes the result's name as its tab title, as the side tree gives it.
    if (target !== null) openTarget(app, local, entry.workspaceId, target, target.kind === 'item' && entry.label !== null ? entry.label : undefined);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      model.move(1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      model.move(-1);
    } else if (e.key === 'Enter') {
      const entry = all[model.active];
      if (entry !== undefined) {
        e.preventDefault();
        choose(entry);
      }
    }
  }
</script>

{#if open}
  <div class="dialog-backdrop">
    <div class="dialog search" role="dialog" aria-modal="true" aria-label={t('rail-search')}>
      <input
        bind:this={input}
        type="text"
        role="combobox"
        aria-expanded={all.length > 0}
        aria-controls="search-results"
        aria-autocomplete="list"
        aria-activedescendant={all[model.active] === undefined ? undefined : `search-${all[model.active]?.id}`}
        aria-label={t('rail-search')}
        value={model.query}
        oninput={(e) => oninput(e.currentTarget.value)}
        {onkeydown}
      />
      <div id="search-results" role="listbox" aria-label={t('rail-search')}>
        {#each GROUPS as [group, titleId] (group)}
          {@const entries = groupEntries(group)}
          {#if entries.length > 0}
            <div role="group" aria-label={t(titleId)}>
              <p class="group-title">{t(titleId)}</p>
              {#each entries as entry (entry.id)}
                {@const icon = entryIcon(entry)}
                <div id={`search-${entry.id}`} role="option" tabindex="-1" aria-selected={all[model.active]?.id === entry.id} onclick={() => choose(entry)} onkeydown={() => undefined}>
                  {#if icon !== null}<span class="icon-tile"><Icon name={icon} /></span>{/if}
                  <span class="label">{entry.labelId !== null ? t(entry.labelId) : entry.label}</span>
                  {#if entry.detail}<span class="detail">{entry.detail}</span>{/if}
                  {#if entry.elsewhere}<span class="workspace">{workspaceName(entry.workspaceId)}</span>{/if}
                  {#if entry.elsewhere && entry.go !== null && ['agent', 'workflow', 'dashboard'].includes(entry.go.kind)}
                    <span class="note">{t('search-other-workspace')}</span>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        {/each}
      </div>
      <ErrorLine error={model.error} />
      <div class="visually-hidden" role="status" aria-live="polite">{announced === null ? '' : announced.id === 'search-result-count' ? t(announced.id, { count: announced.count }) : t(announced.id)}</div>
      <p class="footer">{t('search-footer')}</p>
    </div>
  </div>
{/if}

<style>
  /* Search or go to… (scenes `findAll`, `findCount`, the prototype's `.find`): a 640 px dialog near the top
     of the window; the box across its top with a divider under it; groups headed in Label text, muted; each
     result its icon, its words over a muted line, and the workspace it belongs to in a bordered pill; the
     keys line under a divider. */
  .dialog-backdrop {
    align-items: flex-start;
    padding-block-start: 12vh;
  }
  .search {
    inline-size: min(640px, 100%);
    max-block-size: 72vh;
    padding: 0;
    gap: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--color-surface);
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-lg);
  }
  .search > input {
    border: 0;
    border-block-end: 1px solid var(--color-line);
    border-radius: 0;
    padding: 14px var(--space-md);
    font-size: 16px;
    background: transparent;
  }
  .search > input:focus-visible {
    outline-offset: -3px;
  }
  #search-results {
    flex: 1;
    min-block-size: 0;
    overflow: auto;
    padding: var(--space-06) 0;
  }
  .group-title {
    margin: 0;
    padding: var(--space-sm) var(--space-md) var(--space-04);
    font-size: 12px;
    font-weight: 600;
    color: var(--color-text-muted);
  }
  [role='option'] {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    column-gap: var(--space-10);
    align-items: center;
    padding: 7px var(--space-md);
    cursor: pointer;
  }
  [role='option']:hover,
  [role='option'][aria-selected='true'] {
    background: var(--color-accent-soft);
  }
  [role='option'] > .icon-tile {
    grid-column: 1;
    grid-row: 1 / span 2;
  }
  .label,
  .detail,
  .note {
    grid-column: 2;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .label {
    font-weight: 600;
  }
  .detail,
  .note {
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .workspace {
    grid-column: 3;
    grid-row: 1 / span 2;
    max-inline-size: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--color-text-secondary);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-pill);
    padding: 1px var(--space-sm);
  }
  .footer {
    margin: 0;
    padding: var(--space-sm) var(--space-md);
    border-block-start: 1px solid var(--color-line);
    font-size: 12px;
    color: var(--color-text-muted);
  }
</style>
