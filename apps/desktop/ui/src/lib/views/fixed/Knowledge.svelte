<script lang="ts">
  // spec: ordin-desktop/components/ScreensModule#SM-EM-1
  // The Knowledge screen (ADR-104): the workspace's pages with a filter box, and a page with its links,
  // "Pages that link here", other names, tags and suggestions. A link whose target is set opens that page;
  // a link with none is drawn muted with a tooltip and opens nothing. The body is drawn as a formatted
  // page from the Markdown reader's tree (SM-KN-1): text only, no markup from data.
  //
  // spec: ordin-desktop/components/ScreensModule#ADR-104
  // spec: ordin-desktop/components/ScreensModule#SM-KN-1
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import AdoptControls from './AdoptControls.svelte';
  import { KnowledgeController, parseLines, parseTags } from './knowledge-controller.svelte';
  import { TeamController } from './team.svelte';
  import TeamPanel from './TeamPanel.svelte';
  import { pageBlocks, type PageInline } from './wikilinks';

  let { backend, workspaceId, instance }: ScreenProps = $props();

  const c = new KnowledgeController(backend.handle, workspaceId ?? '');
  const team = new TeamController(backend.handle, workspaceId ?? '');
  let editing = $state(false);
  let title = $state('');
  let body = $state('');
  let tags = $state('');
  let aliases = $state('');
  let filterText = $state('');
  let showRenamed = $state(false);
  let showNotAdopted = $state(false);

  onMount(() => {
    if (answers(backend, 'team_knowledge.status')) void team.load();
    void c.load();
    if (instance) void c.open(instance);
  });

  function startEdit(): void {
    if (c.page === null) return;
    title = c.page.page.title;
    body = c.page.page.body_markdown;
    tags = c.page.tags.join(', ');
    aliases = c.page.aliases.join('\n');
    editing = true;
  }

  async function save(): Promise<void> {
    if (await c.save({ title, body_markdown: body, tags: parseTags(tags), aliases: parseLines(aliases) })) editing = false;
  }


  const notAdoptedReason = (reason: string): string =>
    reason === 'page_limit'
      ? t('knowledge-not-adopted-limit')
      : reason === 'too_large'
        ? t('knowledge-not-adopted-large')
        : reason === 'header_invalid'
          ? t('knowledge-not-adopted-header')
          : t('knowledge-not-adopted-other-workspace');
</script>

{#snippet inlines(list: PageInline[])}
  {#each list as part, i (i)}
    {#if part.kind === 'page_link'}
      {#if part.link?.target}
        <button type="button" class="link" onclick={() => part.link && void c.openLink(part.link)}>{part.shown}</button>
      {:else}
        <span class="muted-link" title={t('knowledge-link-missing', { name: part.link?.name ?? part.shown })}>{part.shown}</span>
      {/if}
    {:else if part.kind === 'strong'}<strong>{part.text}</strong>{:else if part.kind === 'emphasis'}<em>{part.text}</em>{:else if part.kind === 'code'}<code>{part.text}</code>{:else}{part.text}{/if}
  {/each}
{/snippet}

<section class="knowledge" aria-labelledby="knowledge-heading">
  <h2 id="knowledge-heading" tabindex="-1" data-region-focus>{t('tree-knowledge')}</h2>
  <ErrorLine error={c.error} onretry={() => void c.load()} />

  {#if c.page === null}
    <TeamPanel {backend} workspaceId={workspaceId ?? ''} />
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void c.load(filterText);
      }}
    >
      <input type="search" bind:value={filterText} placeholder={t('knowledge-filter-hint')} aria-label={t('knowledge-filter-hint')} />
    </form>
    <!-- SM-EM-1: an empty Knowledge says what to do. -->
    {#if c.listing !== null && c.listing.this_workspace.length === 0 && c.listing.from_other_teams.length === 0 && filterText.trim() === ''}
      <p class="secondary">{t('knowledge-none-yet')}</p>
    {/if}
    {#if c.listing !== null}
      <ul class="page-list list-rows">
        {#each c.listing.this_workspace as summary (summary.id)}
          <li class="list-row">
            <span class="icon-tile" data-tint="memory"><Icon name="know" /></span>
            <button type="button" class="link" onclick={() => void c.open(summary.id)}>{summary.title}</button>
            {#if summary.shared_by}<span class="secondary"> · {t('team-shared-by', { name: summary.shared_by, date: formatDate(summary.shared_at ?? summary.last_changed_at) })}</span>{/if}
            {#if summary.cites_other_computer}<span class="secondary"> · {t('team-cites-their-files', { name: summary.shared_by ?? '' })}</span>{/if}
            {#each summary.tags as tag (tag)}
              <button type="button" class="chip" onclick={() => void c.filterByTag(tag)}>{tag}</button>
            {/each}
          </li>
        {/each}
      </ul>
      {#each [...new Set(c.listing.from_other_teams.map((p) => p.folder ?? ''))] as folder (folder)}
        <h3>{folder}</h3>
        <ul>
          {#each c.listing.from_other_teams.filter((p) => (p.folder ?? '') === folder) as summary (summary.id)}
            <li>
              <span class="icon-tile" data-tint="memory"><Icon name="know" /></span>
              <button type="button" class="link" onclick={() => void c.open(summary.id)}>{summary.title}</button>
              {#if summary.set_by_company}<span class="chip">{t('team-set-by-company')}</span>{/if}
            </li>
          {/each}
        </ul>
      {/each}
      {#if c.listing.not_adopted.length > 0}
        <p>
          <button type="button" class="link" aria-expanded={showNotAdopted} onclick={() => (showNotAdopted = !showNotAdopted)}>
            {t('knowledge-not-adopted', { count: c.listing.not_adopted.length })}
          </button>
        </p>
        {#if showNotAdopted}
          <ul>
            {#each c.listing.not_adopted as file, i (i)}
              <li>{file.relative_path} · {notAdoptedReason(String(file.reason))}</li>
            {/each}
          </ul>
        {/if}
      {/if}
    {/if}
  {:else}
    {@const view = c.page}
    <button type="button" class="small" onclick={() => c.close()}>{t('fixed-back')}</button>
    <h3>{view.page.title}</h3>
    {#if view.aliases.length > 0}<p class="secondary">{t('knowledge-other-names', { names: view.aliases.join(', ') })}</p>{/if}
    {#if view.tags.length > 0}
      <p>{#each view.tags as tag (tag)}<span class="chip">{tag}</span>{/each}</p>
    {/if}
    {#if view.unregistered_aliases.length > 0}
      {#each view.unregistered_aliases as name (name)}<p>{t('knowledge-name-in-use', { name })}</p>{/each}
    {/if}
    {#if view.title_refused}<p>{t('knowledge-title-in-use', { title: view.title_refused })}</p>{/if}
    {#if view.shared_by}<p class="secondary">{t('team-shared-by', { name: view.shared_by, date: formatDate(view.shared_at ?? view.page.last_changed_at) })}</p>{/if}
    {#if view.cites_other_computer}<p class="secondary">{t('team-cites-their-files', { name: view.shared_by ?? '' })}</p>{/if}
    {#if view.folder}<p class="secondary" title={view.folder_description ?? undefined}>{t('team-from-folder', { folder: view.folder })}</p>{/if}

    {#if c.renamed !== null}
      <p>
        {t('knowledge-renamed-links', { count: c.renamed.length })}
        <button type="button" class="link" onclick={() => (showRenamed = !showRenamed)}>{t('knowledge-show-them')}</button>
      </p>
      {#if showRenamed}<ul>{#each c.renamed as ref (ref.page_id)}<li>{ref.title}</li>{/each}</ul>{/if}
    {/if}

    {#if view.read_only}
      <p>{t('team-read-only-page')}</p>
      <AdoptControls {backend} {team} item={{ kind: 'page', page_id: view.page.id }} folder={view.folder} sharedBy={view.shared_by} title={view.page.title} body={view.page.body_markdown} ondone={() => void c.load()} />
    {:else if !editing}
      <button type="button" class="small primary" onclick={startEdit}>{t('knowledge-edit')}</button>
      {#if view.adoption_mark}
        <AdoptControls {backend} {team} item={{ kind: 'page', page_id: view.page.id }} mark={view.adoption_mark} adoptedFrom={view.adopted_from} sharedBy={view.shared_by} title={view.page.title} body={view.page.body_markdown} ondone={() => void c.load()} />
      {/if}
    {/if}

    {#if editing}
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <label>{t('knowledge-title')}<input type="text" bind:value={title} /></label>
        <label>{t('knowledge-body')}<textarea bind:value={body} rows="12"></textarea></label>
        <label>
          {t('knowledge-other-names-hint')}
          <textarea bind:value={aliases} rows="3"></textarea>
        </label>
        <label>
          {t('knowledge-tags-hint')}
          <input type="text" bind:value={tags} aria-describedby={c.editError ? 'knowledge-edit-error' : undefined} />
        </label>
        <div id="knowledge-edit-error"><ErrorLine error={c.editError} /></div>
        <button type="submit">{t('engineering-save')}</button>
        <button type="button" onclick={() => (editing = false)}>{t('confirm-cancel')}</button>
      </form>
    {:else}
      <!-- SM-KN-1: the body as a formatted page, never its Markdown source. -->
      <div class="page-body">
        {#each pageBlocks(view.page.body_markdown, view.links_out, view.relative_path) as block, b (b)}
          {#if block.kind === 'heading'}
            <svelte:element this={`h${Math.min(6, block.level + 2)}`}>{@render inlines(block.inlines)}</svelte:element>
          {:else if block.kind === 'paragraph'}
            <p>{@render inlines(block.inlines)}</p>
          {:else if block.kind === 'list'}
            <svelte:element this={block.ordered ? 'ol' : 'ul'}>
              {#each block.items as item, k (k)}<li>{@render inlines(item)}</li>{/each}
            </svelte:element>
          {:else if block.kind === 'quote'}
            <blockquote>{@render inlines(block.inlines)}</blockquote>
          {:else if block.kind === 'code'}
            <pre><code>{block.text}</code></pre>
          {:else}
            <hr />
          {/if}
        {/each}
      </div>
    {/if}

    {#if view.suggestions.length > 0}
      <section aria-labelledby="knowledge-suggestions">
        <h4 id="knowledge-suggestions">{t('knowledge-suggestions-title')}</h4>
        <ul>
          {#each view.suggestions as s (s.id)}
            <li>
              {#if s.kind === 'link'}
                {t('knowledge-suggest-link', { words: s.matched_text ?? '', page: s.other.title })}
                {#if s.context}<span class="secondary"> {s.context}</span>{/if}
                <button type="button" onclick={() => void c.acceptSuggestion(s)}>{t('knowledge-link-it')}</button>
              {:else}
                {t(s.reason === 'text' ? 'knowledge-suggest-duplicate-text' : 'knowledge-suggest-duplicate', { page: s.other.title })}
                <button type="button" onclick={() => void c.open(s.other.page_id)}>{t('card-open')}</button>
              {/if}
              <button type="button" onclick={() => void c.dismissSuggestion(s)}>{t('knowledge-dismiss')}</button>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    <section aria-labelledby="knowledge-links-in">
      <h4 id="knowledge-links-in">{t('knowledge-links-in-title')}</h4>
      {#if view.links_in.length === 0}
        <p>{t('knowledge-links-in-none')}</p>
      {:else}
        <ul>
          {#each view.links_in as ref (ref.page_id)}
            <li><span class="icon-tile" data-tint="memory"><Icon name="know" /></span><button type="button" class="link" onclick={() => void c.open(ref.page_id)}>{ref.title}</button></li>
          {/each}
        </ul>
      {/if}
    </section>
    <section aria-labelledby="knowledge-links-out">
      <h4 id="knowledge-links-out">{t('knowledge-links-out-title')}</h4>
      <ul>
        {#each [...new Map(view.links_out.map((l) => [l.target?.page_id ?? l.name, l])).values()] as link (link.target?.page_id ?? link.name)}
          <li>
            {#if link.target}
              <span class="icon-tile" data-tint="memory"><Icon name="know" /></span>
              <button type="button" class="link" onclick={() => void c.openLink(link)}>{link.target.title}</button>
            {:else}
              <span class="muted-link" title={t('knowledge-link-missing', { name: link.name })}>{link.name}</span>
            {/if}
          </li>
        {/each}
      </ul>
      {#if view.links_truncated > 0}<p>{t('knowledge-links-truncated', { count: view.links_truncated })}</p>{/if}
    </section>
  {/if}
  <div class="visually-hidden" aria-live="polite" role="status">{c.announcer.text || team.announcer.text}</div>
</section>

<style>
  /* Scenes `know` and `knLinks`: buttons keep their own width, the page's link sections are the
     prototype's `.knsrc` (a small bold heading over small text), page rows the shared `.list-rows`. */
  .knowledge > button,
  .knowledge > form > button {
    align-self: flex-start;
  }
  .knowledge section h4 {
    margin: 0;
    font-size: 13px;
  }
  .knowledge section p,
  .knowledge section ul {
    margin: 0;
    font-size: 13px;
  }
  .knowledge section ul {
    list-style: none;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  /* The page itself reads on a card (the prototype's `.mdv.mdread`). */
  .page-body {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-lg);
    padding: 18px;
    line-height: 1.5;
  }
  .secondary {
    color: var(--color-text-muted);
  }
  .chip {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding-inline: var(--space-04);
    margin-inline-end: var(--space-04);
    background: var(--color-surface);
  }
  .icon-tile {
    vertical-align: middle;
    margin-inline-end: var(--space-04);
  }
  .muted-link {
    color: var(--color-text-muted);
    text-decoration: underline dotted;
  }
  button.link {
    border: none;
    background: none;
    color: var(--color-accent);
    text-decoration: underline;
    padding: 0;
  }
  .page-body blockquote {
    margin-inline: 0;
    padding-inline-start: var(--space-12);
    border-inline-start: 3px solid var(--color-line);
    color: var(--color-text-muted);
  }
  .page-body pre {
    overflow: auto;
    padding: var(--space-sm);
    background: var(--color-surface);
    border-radius: var(--radius-md);
  }
  .page-list {
    padding: 0;
  }
</style>
