<script lang="ts">
  // spec: ordin-desktop/components/ScreensModule#SM-EM-1
  // The Knowledge screen (ADR-104): the workspace's pages with a filter box, and a page with its links,
  // "Pages that link here", other names, tags and suggestions. A link whose target is set opens that page;
  // a link with none is drawn muted with a tooltip and opens nothing. The body is drawn as a formatted
  // page from the Markdown reader's tree (SM-KN-1): text only, no markup from data.
  //
  // spec: ordin-desktop/components/ScreensModule#ADR-104
  // spec: ordin-desktop/components/ScreensModule#SM-KN-1
  // spec: ordin-desktop/components/ScreensModule#SM-KN-2
  // spec: ordin-desktop/components/ScreensModule#SM-SF-18
  //
  // The open page's ⋯ menu holds Export as PDF… and Export as Word… (SM-KN-3): on this computer only, the
  // program reads the page by its id and opens the save dialog; a cancelled dialog shows nothing.
  // spec: ordin-desktop/components/ScreensModule#SM-KN-3
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import type { KnowledgeExportFormat, ServiceError } from '../../service-api/types.gen';
  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ScreenProps } from '../../shell/screens';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import MarkdownEditor from '../../editor/MarkdownEditor.svelte';
  import AdoptControls from './AdoptControls.svelte';
  import { KnowledgeController, PAGE_TYPES, parseLines, parseTags } from './knowledge-controller.svelte';
  import { TeamController } from './team.svelte';
  import { appOrNull } from './util';
  import TeamPanel from './TeamPanel.svelte';
  import { pageBlocks, type PageInline } from './wikilinks';

  let { backend, workspaceId, instance }: ScreenProps = $props();

  const c = new KnowledgeController(backend.handle, workspaceId ?? '');
  const team = new TeamController(backend.handle, workspaceId ?? '');
  const app = appOrNull();
  let editing = $state(false);
  let title = $state('');
  let body = $state('');
  let tags = $state('');
  let aliases = $state('');
  let pageType = $state('');
  let description = $state('');
  let confirmingDiscard = $state(false);
  /** The fields as they were when Edit was pressed, to tell whether anything changed. */
  let started = '';
  // The editor gives its Markdown back with a closing line end; that alone is not a change.
  const fields = (): string => JSON.stringify([title, body.replace(/\n$/, ''), tags, aliases, pageType, description]);
  let filterText = $state('');
  let showRenamed = $state(false);
  let showNotAdopted = $state(false);
  let confirmingRemove = $state(false);
  /** The page's details in the edit form (its kind, summary, other names and tags); hidden until asked for. */
  let showDetails = $state(false);

  /** SM-KN-3: Export is offered on this computer only, when the program answers `knowledge.export`. */
  const exportable = $derived(backend.kind === 'local' && answers(backend, 'knowledge.export'));
  let menuOpen = $state(false);
  let exportError = $state<ServiceError | null>(null);

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  /** `knowledge.export { page_id, format }`: saved → announced; the dialog cancelled → nothing; else the error line. */
  async function exportPage(format: KnowledgeExportFormat): Promise<void> {
    if (c.page === null) return;
    const { id, title } = c.page.page;
    menuOpen = false;
    exportError = null;
    const out = await backend.handle.call('knowledge.export', { page_id: id, format });
    if (out.kind === 'ok') app?.shell.announce(t('knowledge-exported', { title }));
    else if (out.error.code !== 'DSK_DIALOG_CANCELLED') exportError = out.error;
  }

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
    pageType = c.page.type ?? '';
    description = c.page.description ?? '';
    started = fields();
    confirmingDiscard = false;
    showDetails = false;
    editing = true;
  }

  /** **Done**: saves, then goes back to the page (amended January 2026, user decision). */
  async function save(): Promise<void> {
    if (await c.save({ title, body_markdown: body, tags: parseTags(tags), aliases: parseLines(aliases), type: pageType, description })) editing = false;
  }

  /** **Cancel** and Escape: back to the page without saving, asking first when something changed. */
  function cancel(): void {
    if (fields() !== started) confirmingDiscard = true;
    else editing = false;
  }

  function discard(): void {
    confirmingDiscard = false;
    editing = false;
    c.editError = null;
  }

  /** The kinds offered: the five, and a kind another tool wrote, kept as written. */
  const typeOptions = $derived(
    pageType !== '' && !PAGE_TYPES.some((p) => p.value === pageType) ? [...PAGE_TYPES.map((p) => ({ value: p.value, shown: t(p.label) })), { value: pageType, shown: pageType }] : PAGE_TYPES.map((p) => ({ value: p.value, shown: t(p.label) })),
  );


  /** SM-SF-18: the first 300 characters of a section. */

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
  {#if c.removed !== null}
    <p role="status" class="secondary">
      {t('knowledge-removed', { title: c.removed.title })}
      <button type="button" class="link" onclick={() => void c.undoRemove()}>{t('knowledge-undo-remove')}</button>
    </p>
  {/if}

  {#if c.page === null}
    <!-- SM-TK-2: a workspace with nothing shared shows one line pointing at the side tree's Share link. -->
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
    <!-- Amended January 2026 (user decision; Approved by the user January 2026): Back, Edit and Remove… are
         small icon buttons in the editor toolbar's style, each named by its word and with it as a tooltip,
         on one line with the page's title. -->
    <div class="page-head">
      <button type="button" class="icon-btn" aria-label={t('fixed-back')} title={t('fixed-back')} onclick={() => c.close()}><Icon name="back" size="small" /></button>
      <h3>{view.page.title}</h3>
      {#if !editing && (!view.read_only || exportable)}
        <span class="page-tools">
          {#if !view.read_only}
            <button type="button" class="icon-btn" aria-label={t('knowledge-edit')} title={t('knowledge-edit')} onclick={startEdit}><Icon name="edit" size="small" /></button>
            <button type="button" class="icon-btn" aria-label={t('knowledge-remove')} title={t('knowledge-remove')} onclick={() => (confirmingRemove = true)}><Icon name="bin" size="small" /></button>
          {/if}
          <!-- SM-KN-3 (amended January 2026; user decision; Approved by the user January 2026): the page's ⋯
               menu, in the tree menu's markup, holding Export as PDF… and Export as Word…. -->
          {#if exportable}
            <button type="button" class="icon-btn" aria-haspopup="menu" aria-expanded={menuOpen} aria-label={t('more-actions-for', { name: view.page.title })} title={t('more-actions-tip')} onclick={() => (menuOpen = !menuOpen)}><Icon name="more" size="small" /></button>
          {/if}
        </span>
        {#if menuOpen && exportable}
          <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: view.page.title })} use:focusFirst onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); menuOpen = false; } }}>
            <button type="button" role="menuitem" onclick={() => void exportPage('pdf')}><Icon name="pdf" />{t('knowledge-export-pdf')}</button>
            <button type="button" role="menuitem" onclick={() => void exportPage('docx')}><Icon name="word" />{t('knowledge-export-docx')}</button>
          </div>
        {/if}
      {/if}
    </div>
    <ErrorLine error={exportError} />
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
      {#if view.adoption_mark}
        <AdoptControls {backend} {team} item={{ kind: 'page', page_id: view.page.id }} mark={view.adoption_mark} adoptedFrom={view.adopted_from} sharedBy={view.shared_by} title={view.page.title} body={view.page.body_markdown} ondone={() => void c.load()} />
      {/if}
    {/if}

    {#if editing}
      <!-- Amended January 2026 (user decision; Approved by the user January 2026): **Done** saves and goes
           back to the page, **Cancel** (and Escape) goes back without saving, asking first when something
           changed. The page's kind and one-sentence summary (Open Knowledge Format `type` and
           `description`) are fields of their own. -->
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void save();
        }}
        onkeydown={(e) => {
          if (e.key === 'Escape' && !e.defaultPrevented) {
            e.preventDefault();
            cancel();
          }
        }}
      >
        <!-- Amended November 2026 (user decision): **Done** and **Cancel** are small icon buttons at the top of
             the form, each with its word as a tooltip, and the details button sits beside the title box. -->
        <div class="edit-tools">
          <button type="submit" class="icon-btn" data-default="true" aria-label={t('edit-done')} title={t('edit-done')}><Icon name="check" size="small" /></button>
          <button type="button" class="icon-btn" aria-label={t('edit-cancel')} title={t('edit-cancel')} onclick={cancel}><Icon name="remove" size="small" /></button>
        </div>
        <div class="title-row">
          <label>{t('knowledge-title')}<input type="text" bind:value={title} /></label>
          <!-- The page's details — its kind, its summary, its other names and its tags (the Open Knowledge
               Format fields) — are hidden until the person asks for them with this button. -->
          <button
            type="button"
            class="icon-btn"
            aria-expanded={showDetails}
            aria-controls={showDetails ? 'knowledge-details' : undefined}
            aria-label={showDetails ? t('knowledge-details-hide') : t('knowledge-details-show')}
            title={showDetails ? t('knowledge-details-hide') : t('knowledge-details-show')}
            onclick={() => (showDetails = !showDetails)}
          ><Icon name="info" size="small" /></button>
        </div>
        {#if showDetails}
        <div id="knowledge-details" class="details">
        <label>
          {t('knowledge-type')}
          <select value={pageType} onchange={(e) => (pageType = e.currentTarget.value)}>
            {#if pageType === ''}<option value=""></option>{/if}
            {#each typeOptions as option (option.value)}<option value={option.value}>{option.shown}</option>{/each}
          </select>
        </label>
        <label>
          {t('knowledge-description')}
          <input type="text" maxlength="200" bind:value={description} aria-describedby="knowledge-description-hint" />
        </label>
        <p id="knowledge-description-hint" class="secondary">{t('knowledge-description-hint')}</p>
        <label>
          {t('knowledge-other-names-hint')}
          <textarea bind:value={aliases} rows="3"></textarea>
        </label>
        <label>
          {t('knowledge-tags-hint')}
          <input type="text" bind:value={tags} aria-describedby={c.editError ? 'knowledge-edit-error' : undefined} />
        </label>
        </div>
        {/if}
        <!-- Amended January 2026 (user decision; Approved by the user January 2026): the body is written in the
             what-you-see editor (UX guidelines, Writing), never as Markdown codes. -->
        <div class="body-field">
          <span class="field-name" aria-hidden="true">{t('knowledge-body')}</span>
          <MarkdownEditor bind:value={body} label={t('knowledge-body')} />
        </div>
        <div id="knowledge-edit-error"><ErrorLine error={c.editError} /></div>
      </form>
      {#if confirmingDiscard}
        <ConfirmDialog title={t('edit-discard-title')} verb={t('edit-discard-verb')} cancelLabel={t('edit-keep-editing')} onconfirm={discard} oncancel={() => (confirmingDiscard = false)} />
      {/if}
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

    <!-- SM-SF-18 (amended January 2026): no What Ordin read section; the page's own sections are its passages, so it only repeated the page. -->

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

{#if confirmingRemove && c.page !== null}
  <ConfirmDialog
    title={t('knowledge-remove-title', { title: c.page.page.title })}
    body={t('knowledge-remove-body')}
    verb={t('knowledge-remove-verb')}
    onconfirm={() => {
      confirmingRemove = false;
      // SM-OS-3: the removed page's tabs close.
      const pageId = c.page?.page.id;
      void c.remove().then((removed) => {
        if (removed && pageId !== undefined) app?.screens.closeScreensFor('knowledge_page', pageId);
      });
    }}
    oncancel={() => (confirmingRemove = false)}
  />
{/if}

<style>
  /* Scenes `know` and `knLinks`: buttons keep their own width, the page's link sections are the
     prototype's `.knsrc` (a small bold heading over small text), page rows the shared `.list-rows`. */
  .knowledge > button,
  .knowledge > form > button {
    align-self: flex-start;
  }
  /* An open page reads tighter than the list: smaller gaps between its lines (amended January 2026). */
  .knowledge:has(> .page-head) {
    gap: var(--space-sm);
  }
  .knowledge:has(> .page-head) > p {
    margin: 0;
  }
  /* The page's head: Back, the title and the page's tools on one line, wrapping on a narrow pane. */
  .page-head {
    position: relative;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-04) var(--space-sm);
  }
  .page-head h3 {
    margin: 0;
    flex: 1;
    min-inline-size: 0;
    overflow-wrap: anywhere;
  }
  .page-tools {
    display: inline-flex;
    gap: var(--space-04);
  }
  /* The editor toolbar's icon button (MarkdownEditor.svelte `.md-tool`). */
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    min-height: 0;
    padding: 0;
  }
  .icon-btn[aria-expanded='true'] {
    background: var(--color-accent-soft);
    color: var(--color-accent-ink);
  }
  .edit-tools {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-04);
  }
  /* The details button sits on the title box's centre line and is as tall as the box: the label lays its
     word and its box out on one line, so its height is the box's height. */
  .title-row {
    display: flex;
    align-items: center;
    gap: var(--space-04);
    margin: 0 0 var(--space-sm);
  }
  .title-row label {
    display: flex;
    align-items: center;
    gap: var(--space-04);
    flex: 1;
    min-inline-size: 0;
  }
  .title-row input {
    flex: 1;
    min-inline-size: 0;
  }
  .title-row .icon-btn {
    flex: none;
    /* The text box's own height: one line of text, its padding and its border (tokens.css, input). */
    block-size: calc(1lh + 2 * var(--space-06) + 2px);
    inline-size: calc(1lh + 2 * var(--space-06) + 2px);
  }
  .details {
    display: flex;
    flex-direction: column;
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
  .body-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
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
