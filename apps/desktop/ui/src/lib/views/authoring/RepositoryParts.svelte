<script lang="ts">
  // Parts of [display name] (SM-NS-7.2; instance = the agent repository id). Drawn only for a repository the
  // person can publish to. Each part draws its kind's icon (V8 `ptIco`).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-7
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import AskButton from '../../blocks/AskButton.svelte';
  import { formatDate, formatNumber } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import Icon from '../../icons/Icon.svelte';
  import { partKindIcon } from '../../icons/kinds';
  import { healthGroups, HEALTH_MESSAGE, RepositoryPartsController, STATE_MESSAGE, type RepoTab, type Show } from './repository.svelte';
  import { NEW_PART_KINDS, openSibling, PART_KIND_MESSAGE } from './support';
  import { appOrNull } from '../fixed/util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in a callback.
  const app = appOrNull();
  const c = new RepositoryPartsController(backend.handle, instance ?? '', (key) => openSibling(app, backend, workspaceId, key));
  const canCreate = $derived(answers(backend, 'authoring.new_part'));

  onMount(() => {
    void c.load();
  });

  const TABS: ReadonlyArray<{ tab: RepoTab; label: string }> = [
    { tab: 'parts', label: 'repo-tab-parts' },
    { tab: 'words', label: 'repo-tab-words' },
    { tab: 'layout', label: 'repo-tab-layout' },
    { tab: 'health', label: 'repo-tab-health' },
  ];
  const SHOWS: ReadonlyArray<{ show: Show; label: string }> = [
    { show: 'in_use', label: 'repo-show-in-use' },
    { show: 'not_for_new', label: 'repo-show-not-for-new' },
    { show: 'withdrawn', label: 'repo-show-withdrawn' },
  ];
</script>

<section class="repository-parts" aria-labelledby="repo-parts-heading">
  <h2 id="repo-parts-heading" tabindex="-1" data-region-focus>{t('repo-parts-title', { name: c.current?.repository.display_name ?? '' })}</h2>
  <ErrorLine error={c.error} onretry={() => void c.load()} />
  {#if c.current !== null}
    <label class="switch">
      {t('repo-parts-switch')}
      <select value={c.repoId} onchange={(e) => void c.switchTo(e.currentTarget.value)}>
        {#each c.publishable as repo (repo.repository.id)}<option value={repo.repository.id}>{repo.repository.display_name}</option>{/each}
      </select>
    </label>
    {#if c.isApm}
      <p>{t('repo-apm-note', { name: c.current.repository.display_name })}</p>
    {:else}
      <div class="tabs" role="tablist" aria-label={t('repo-parts-title', { name: c.current.repository.display_name })}>
        {#each TABS as item (item.tab)}
          <button type="button" role="tab" aria-selected={c.tab === item.tab} onclick={() => void c.openTab(item.tab)}>{t(item.label)}</button>
        {/each}
      </div>
    {/if}

    {#if c.tab === 'parts' || c.isApm}
      <div class="search">
        <input type="search" aria-label={t('repo-parts-search')} placeholder={t('repo-parts-search')} bind:value={c.query} onchange={() => void c.search()} />
        <fieldset class="show">
          {#each SHOWS as item (item.show)}
            <label>
              <input
                type="radio"
                name="repo-show"
                checked={c.show === item.show}
                onchange={() => {
                  c.show = item.show;
                  void c.search();
                }}
              />
              {t(item.label)}
            </label>
          {/each}
        </fieldset>
      </div>
      {#if canCreate && !c.isApm}
        <button type="button" class="small new-part" onclick={() => (c.newPartOpen = !c.newPartOpen)}>{t('repo-new-part')}</button>
        {#if c.newPartOpen}
          <div class="panel new-part-panel" role="group" aria-label={t('repo-new-part')}>
            <label>
              {t('import-part-kind')}
              <select bind:value={c.newKind}>
                {#each NEW_PART_KINDS as kind (kind)}<option value={kind}>{t(PART_KIND_MESSAGE[kind])}</option>{/each}
              </select>
            </label>
            <input type="text" aria-label={t('repo-new-part')} bind:value={c.newName} />
            <button type="button" data-default="true" disabled={c.newName.trim() === ''} onclick={() => void c.newPart(false)}>{t('repo-new-part')}</button>
            <button type="button" onclick={() => (c.newPartOpen = false)}>{t('confirm-cancel')}</button>
          </div>
        {/if}
      {/if}
      <div class="lib">
      <ul class="list-rows">
        {#each c.shown as entry (entry.part.part.id)}
          <li class="list-row part-row">
            <span class="icon-tile" data-tint={partKindIcon(entry.part.part.kind) === 'agent' ? 'accent-ink' : undefined}><Icon name={partKindIcon(entry.part.part.kind)} /></span>
            <button type="button" class="link" aria-pressed={c.selected?.part.id === entry.part.part.id} onclick={() => c.choose(entry)}>{entry.part.part.name}</button>
            <span class="kind">{t(PART_KIND_MESSAGE[entry.part.part.kind])}</span>
            <span class="state-label" class:done={entry.part.state_label === 'current'} class:waiting={entry.part.state_label === 'not_for_new_agents'} class:failed={entry.part.state_label === 'withdrawn'}>{t(STATE_MESSAGE[entry.part.state_label])}</span>
          </li>
        {/each}
      </ul>
      {#if c.selected !== null}
        {@const part = c.selected}
        <section class="detail" aria-label={part.part.name}>
          <p>{part.summary}</p>
          <div class="part-actions">
            <button type="button" class="small" onclick={() => c.readIt()}><Icon name="eye" />{t('repo-read-it')}</button>
            {#if workspaceId !== null}
              <AskButton {backend} {workspaceId} labelId="repo-ask-about-part" text={t('block-ask-text-this', { title: part.part.name })} />
            {/if}
          </div>
          <details class="fold" ontoggle={(e) => e.currentTarget.open && void c.loadUsedBy()}>
            <summary>{t('repo-used-by')}</summary>
            <ul>
              {#each [...(c.usedBy?.agents ?? []), ...(c.usedBy?.parts ?? [])] as user (user.part.id)}<li><span class="icon-tile" data-tint={partKindIcon(user.part.kind) === 'agent' ? 'accent-ink' : undefined}><Icon name={partKindIcon(user.part.kind)} /></span>{user.part.name}</li>{/each}
            </ul>
          </details>
          <details class="fold" ontoggle={(e) => e.currentTarget.open && void c.loadHistory()}>
            <summary>{t('repo-history')}</summary>
            <ul>
              {#each c.history?.versions ?? [] as line (line.commit)}<li>{line.version} · {formatDate(line.merged_at)}</li>{/each}
            </ul>
          </details>
          {#if answers(backend, 'authoring.withdraw')}
            <details class="more">
              <summary>{t('repo-more')}</summary>
              <button type="button" class="small" onclick={() => c.withdraw()}>{t('repo-withdraw')}</button>
            </details>
          {/if}
        </section>
      {/if}
      </div>
    {:else if c.tab === 'words'}
      <ul>
        {#each c.vocabulary as term (term.id)}<li><strong>{term.label}</strong> · {term.description}</li>{/each}
      </ul>
    {:else if c.tab === 'layout'}
      <ul>
        {#each c.layout as line (line.kind)}<li>{t(PART_KIND_MESSAGE[line.kind])} · {formatNumber(line.count)}</li>{/each}
      </ul>
    {:else if c.health !== null}
      <p class="secondary">{t('repo-health-checked', { time: formatDate(c.health.computed_at) })}</p>
      <ul>
        {#each healthGroups(c.health) as group (group.kind)}<li>{t(HEALTH_MESSAGE[group.kind])} · {formatNumber(group.count)}</li>{/each}
      </ul>
      <p class="secondary">{t('repo-health-line')}</p>
    {/if}
  {/if}
</section>

<style>
  /* The prototype's Parts of [repository] (scene `partsBiz`). */
  .repository-parts {
    gap: var(--space-10);
  }
  .repository-parts > h2 {
    font-size: 19px;
  }
  .secondary {
    color: var(--color-text-muted);
  }
  .switch {
    display: flex;
    gap: var(--space-sm);
    align-items: center;
    font-size: 13px;
    font-weight: 700;
  }
  .switch select {
    font-size: 16px;
    font-weight: 400;
    inline-size: auto;
  }
  .tabs {
    display: flex;
    gap: 2px;
    border-block-end: 1px solid var(--color-line);
    overflow-x: auto;
  }
  .tabs > button {
    border: 1px solid transparent;
    border-block-end: 0;
    border-radius: 8px 8px 0 0;
    padding: var(--space-06) var(--space-12);
    min-height: 0;
    background: none;
    font-size: 13px;
    color: var(--color-text-secondary);
    margin-block-end: -1px;
    white-space: nowrap;
  }
  .tabs > button:hover {
    background: var(--color-chip);
  }
  .tabs > button[aria-selected='true'] {
    background: var(--color-surface);
    border-color: var(--color-line);
    color: var(--color-text);
    font-weight: 600;
  }
  .search {
    display: flex;
    flex-direction: column;
    gap: var(--space-10);
  }
  .search input[type='search'] {
    inline-size: 100%;
    border-color: var(--color-line-strong);
  }
  .show {
    border: 0;
    margin: 0;
    padding: 0;
    display: flex;
    gap: var(--space-sm);
    align-items: center;
    flex-wrap: wrap;
    font-size: 13px;
  }
  .show label {
    display: flex;
    gap: var(--space-06);
    align-items: center;
  }
  .new-part {
    align-self: flex-start;
  }
  .new-part-panel {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    align-items: center;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-10);
    background: var(--color-background);
  }
  .lib {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 300px;
    gap: var(--space-md);
    align-items: start;
  }
  @media (max-width: 900px) {
    .lib {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .part-row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    column-gap: var(--space-10);
    row-gap: 2px;
  }
  .part-row > :global(.icon-tile) {
    grid-row: span 2;
  }
  .part-row > .state-label {
    grid-column: 3;
    grid-row: 1 / span 2;
  }
  .part-row:hover {
    border-color: var(--color-line-strong);
  }
  .part-row:has(> button[aria-pressed='true']) {
    border-color: var(--color-accent);
    background: var(--color-accent-soft);
  }
  .part-row .kind {
    grid-column: 2;
    font-size: 13px;
    color: var(--color-text-muted);
  }
  button.link {
    border: none;
    background: none;
    color: var(--color-text);
    text-decoration: none;
    font-weight: 700;
    font-size: 16px;
    padding: 0;
    min-height: 0;
    text-align: start;
  }
  button.link[aria-pressed='true'] {
    background: none;
    border: none;
    color: var(--color-text);
  }
  .detail {
    position: sticky;
    inset-block-start: 0;
  }
  .part-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
  .part-actions :global(button) {
    padding: 3px var(--space-sm);
    min-height: 28px;
    font-size: 13px;
  }
  .fold {
    border-block-start: 1px solid var(--color-line);
    padding-block-start: var(--space-10);
  }
  .fold summary {
    cursor: pointer;
    font-weight: 600;
    font-size: 14px;
  }
  .fold ul {
    margin: var(--space-sm) 0 0;
    padding-inline-start: 18px;
    font-size: 14px;
  }
  .more {
    align-self: flex-start;
  }
  .more summary {
    list-style: none;
    border: 1px solid var(--color-border-control);
    border-radius: var(--radius-md);
    padding: 3px var(--space-sm);
    font-size: 13px;
    cursor: pointer;
  }
  .more summary::-webkit-details-marker {
    display: none;
  }
  .more[open] > button {
    margin-block-start: var(--space-04);
  }
</style>
