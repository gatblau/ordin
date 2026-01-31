<script lang="ts">
  // What Ordin remembers for a workspace (ADR-111; SM-SF-4, SM-SF-5): a WAI-ARIA tab list of Facts, Things
  // and, when either of its sections is wired, To check, with its count in the tab's accessible name.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-4
  // spec: ordin-desktop/components/ScreensModule#SM-SF-5
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { formatDate, formatPercent } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ScreenProps } from '../../shell/screens';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import AdoptControls from './AdoptControls.svelte';
  import FactLine from './FactLine.svelte';
  import { KIND_MESSAGE, citationLine, heldKindMessage } from './facts';
  import LinkLine from './LinkLine.svelte';
  import { MemoryController, type MemoryTab } from './memory-controller.svelte';
  import { TeamController } from './team.svelte';
  import TeamPanel from './TeamPanel.svelte';

  let { backend, workspaceId }: ScreenProps = $props();

  const c = new MemoryController(backend.handle, workspaceId ?? '');
  // Adopt, Adopt again and Keep as it is need the workspace's own folder (ADR-110).
  const team = new TeamController(backend.handle, workspaceId ?? '');
  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));
  let first = true;
  let correcting = $state<{ id: string; kind: string; from: string; to: string } | null>(null);
  let sameAsPick = $state('');
  let sameAsOpen = $state(false);

  onMount(() => {
    if (answers(backend, 'team_knowledge.status')) void team.load();
    void c.load();
    void c.loadThings();
  });

  // A `workspace_changed` for this workspace reads the To check sections again.
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    void c.loadToCheck();
  });

  const tabs = $derived<MemoryTab[]>(c.toCheckDrawn ? ['facts', 'things', 'to_check'] : ['facts', 'things']);
  const tabLabel = (tab: MemoryTab): string =>
    tab === 'facts' ? t('memory-tab-facts') : tab === 'things' ? t('memory-tab-things') : `${t('memory-tab-to-check')} (${c.toCheckCount})`;

  function onTabKey(event: KeyboardEvent, index: number): void {
    const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    c.tab = tabs[(index + step + tabs.length) % tabs.length] as MemoryTab;
  }

  const pendingTitle = $derived.by(() => {
    const p = c.pending;
    if (p === null) return '';
    if (p.kind === 'fact') return t('memory-forget-confirm');
    if (p.kind === 'link') return t('memory-link-forget-confirm', { from: p.link.from.name, kind: p.link.kind, to: p.link.to.name });
    return t('memory-thing-forget-confirm', { thing: p.name, facts: p.counts.facts, links: p.counts.links });
  });
  const pendingBody = $derived.by(() => {
    const p = c.pending;
    if (p === null) return undefined;
    if (p.kind === 'fact') return p.summaries > 0 ? t('memory-forget-confirm-summaries', { count: p.summaries }) : undefined;
    if (p.kind === 'thing') {
      const lines: string[] = [];
      if (p.changed) lines.push(t('memory-thing-count-changed', { thing: p.name }));
      if (p.counts.summaries > 0) lines.push(t('memory-thing-forget-confirm-summaries', { summaries: p.counts.summaries }));
      return lines.length > 0 ? lines.join(' ') : undefined;
    }
    return undefined;
  });
  const pendingVerb = $derived(c.pending?.kind === 'thing' ? t('memory-thing-forget-verb') : t('memory-link-forget'));
  const holdsSummary = (): boolean => (c.pending?.kind === 'thing' && c.pending.counts.summaries > 0) || false;
  void holdsSummary;
</script>

<section class="memory" aria-labelledby="memory-heading">
  <h2 id="memory-heading" tabindex="-1" data-region-focus>{t('tree-memory')}</h2>

  <TeamPanel {backend} workspaceId={workspaceId ?? ''} onopenchecked={() => (c.toCheckDrawn ? (c.tab = 'to_check') : (c.tab = 'facts'))} />

  <div role="tablist" aria-label={t('tree-memory')}>
    {#each tabs as tab, i (tab)}
      <button
        type="button"
        role="tab"
        id={`memory-tab-${tab}`}
        aria-selected={c.tab === tab}
        aria-controls={`memory-panel-${tab}`}
        tabindex={c.tab === tab ? 0 : -1}
        onclick={() => (c.tab = tab)}
        onkeydown={(e) => onTabKey(e, i)}
      >
        {tabLabel(tab)}
      </button>
    {/each}
  </div>

  <ErrorLine error={c.errorOn('')} />

  {#if c.tab === 'facts'}
    <div role="tabpanel" id="memory-panel-facts" aria-labelledby="memory-tab-facts">
      {#if c.toCheckDrawn && c.toCheckCount > 0}
        <p><button type="button" class="link" onclick={() => (c.tab = 'to_check')}>{t('memory-to-check-line', { count: c.toCheckCount })}</button></p>
      {/if}
      <ul class="fact-list">
        {#each c.facts as fact (fact.id)}
          <FactLine
            {fact}
            names={c.names}
            replaced={fact.replaces ? c.replaced[fact.replaces] : undefined}
            onkeep={(f) => void c.keep(f)}
            onforget={(f) => void c.askForgetFact(f.id)}
            onhistory={(f) => void c.openHistory(f)}
            onopenthing={(id) => void c.openThing(id)}
            onkeepchecked={(f) => void c.keepChecked(f)}
            onmadefrom={(f) => void c.openOrigins(f.id)}
            madeFrom={c.origins[fact.id]?.items.filter((o) => o.state === 'current').map((o) => o.fact)}
          >
            {#snippet adopt()}
              <AdoptControls
                {backend}
                {team}
                item={{ kind: 'fact', id: fact.id }}
                folder={fact.folder}
                mark={fact.adoption_mark}
                adoptedFrom={fact.adopted_from}
                sharedBy={fact.shared_by}
                statement={fact.statement}
                ondone={() => void c.load()}
              />
            {/snippet}
          </FactLine>
          {#if c.errorOn(fact.id)}<ErrorLine error={c.errorOn(fact.id)} />{/if}
        {/each}
      </ul>
      {#if c.factsCursor !== null}<button type="button" onclick={() => void c.moreFacts()}>{t('fixed-show-more')}</button>{/if}
      {#if c.history !== null}
        <div class="history" role="dialog" aria-labelledby="memory-history-heading">
          <h3 id="memory-history-heading">{t('memory-history-title')}</h3>
          <ol>
            {#each c.history.view.versions as version (version.id)}
              <li>
                {t('memory-history-version', {
                  statement: version.statement,
                  when: version.true_until ? t('memory-true-between', { from: formatDate(version.true_from), until: formatDate(version.true_until) }) : t('memory-true-since', { date: formatDate(version.true_from) }),
                })}
              </li>
            {/each}
          </ol>
          <ul>
            {#each c.history.view.links as link (link.id)}<LinkLine {link} names={c.names} pageTitles={c.pages.titles} />{/each}
          </ul>
          <button type="button" onclick={() => (c.history = null)}>{t('confirm-cancel')}</button>
        </div>
      {/if}
    </div>
  {:else if c.tab === 'things'}
    <div role="tabpanel" id="memory-panel-things" aria-labelledby="memory-tab-things">
      {#if c.detail === null}
        <label>
          {t('memory-things-search')}
          <input type="search" value={c.thingSearch} oninput={(e) => c.searchThings(e.currentTarget.value)} />
        </label>
        {#if c.things.length === 0}
          <p>{t('memory-things-empty')}</p>
        {:else}
          <ul class="thing-list">
            {#each c.things as thing (thing.id)}
              <li>
                <button type="button" class="link" onclick={() => void c.openThing(thing.id)}>{thing.name}</button>
                {#if thing.other_names.length > 0}<span> · {t('memory-thing-also-called', { names: thing.other_names.join(', ') })}</span>{/if}
                <span> · {t('memory-thing-counts', { facts: thing.fact_count, links: thing.link_count })}</span>
                {#if thing.explained_in}
                  <span> · {t('memory-explained-in', { page: thing.explained_in.title })}</span>
                {/if}
              </li>
            {/each}
          </ul>
          {#if c.thingsCursor !== null}<button type="button" onclick={() => void c.moreThings()}>{t('fixed-show-more')}</button>{/if}
        {/if}
      {:else}
        <button type="button" onclick={() => c.closeThing()}>{t('fixed-back')}</button>
        <h3>{c.detail.thing.name}</h3>
        {#if c.detail.thing.other_names.length > 0}<p>{t('memory-thing-also-called', { names: c.detail.thing.other_names.join(', ') })}</p>{/if}
        {#if c.detail.thing.explained_in}<p>{t('memory-explained-in', { page: c.detail.thing.explained_in.title })}</p>{/if}
        <ul>
          {#each c.detail.facts as fact (fact.id)}
            <FactLine {fact} names={c.names} onforget={(f) => void c.askForgetFact(f.id)} onopenthing={(id) => void c.openThing(id)} />
          {/each}
        </ul>
        <ul>
          {#each c.detail.links as link (link.id)}
            <LinkLine
              {link}
              names={c.names}
              pageTitles={c.pages.titles}
              onopenthing={(id) => void c.openThing(id)}
              oncorrect={(l) => (correcting = { id: l.id, kind: l.kind, from: l.from.id, to: l.to.id })}
              onforget={(l) => c.askForgetLink(l)}
            />
          {/each}
        </ul>
        {#if correcting !== null}
          <form
            class="correct-link"
            aria-labelledby="memory-correct-heading"
            onsubmit={(e) => {
              e.preventDefault();
              const link = c.detail?.links.find((l) => l.id === correcting?.id);
              if (link && correcting) void c.correctLink(link, correcting).then((ok) => ok && (correcting = null));
            }}
          >
            <h4 id="memory-correct-heading">{t('memory-link-correct-title')}</h4>
            <p>{t('memory-link-correct-help')}</p>
            <select bind:value={correcting.from} aria-label={t('memory-tab-things')}>
              {#each c.things as thing (thing.id)}<option value={thing.id}>{thing.name}</option>{/each}
            </select>
            <input type="text" bind:value={correcting.kind} minlength="1" maxlength="64" aria-label={t('memory-link-correct')} />
            <select bind:value={correcting.to} aria-label={t('memory-tab-things')}>
              {#each c.things as thing (thing.id)}<option value={thing.id}>{thing.name}</option>{/each}
            </select>
            <button type="submit">{t('engineering-save')}</button>
            <button type="button" onclick={() => (correcting = null)}>{t('confirm-cancel')}</button>
          </form>
        {/if}
        <button type="button" class="danger" onclick={() => c.detail && void c.askForgetThing(c.detail.thing.id, c.detail.thing.name)}>
          {t('memory-thing-forget')}
        </button>
        <button type="button" onclick={() => (sameAsOpen = !sameAsOpen)}>{t('memory-same-as')}</button>
        {#if sameAsOpen}
          <form
            class="same-as"
            aria-labelledby="memory-same-as-heading"
            onsubmit={(e) => {
              e.preventDefault();
              const keep = c.things.find((x) => x.id === sameAsPick);
              if (keep && c.detail) void c.sameAs({ id: keep.id, name: keep.name }, { id: c.detail.thing.id, name: c.detail.thing.name }).then(() => (sameAsOpen = false));
            }}
          >
            <h4 id="memory-same-as-heading">{t('memory-same-as-title', { thing: c.detail.thing.name })}</h4>
            <p>{t('memory-same-as-help', { thing: c.detail.thing.name })}</p>
            <select bind:value={sameAsPick} aria-label={t('memory-same-as')}>
              {#each c.things.filter((x) => x.id !== c.detail?.thing.id) as thing (thing.id)}<option value={thing.id}>{thing.name}</option>{/each}
            </select>
            <button type="submit" disabled={sameAsPick === ''}>{t('memory-same-as-verb')}</button>
          </form>
        {/if}
      {/if}
    </div>
  {:else}
    <div role="tabpanel" id="memory-panel-to_check" aria-labelledby="memory-tab-to_check">
      {#if c.held !== null}
        <section aria-labelledby="memory-held-title">
          <h3 id="memory-held-title" tabindex="-1">{t('memory-held-title')}</h3>
          <p>{t('memory-held-intro')}</p>
          {#if c.held.length === 0}
            <p>{t('memory-held-empty')}</p>
          {:else}
            <ul class="held-list">
              {#each c.held as entry (entry.id + entry.content_hash)}
                {@const kind = heldKindMessage(entry.fact_class)}
                {@const agent = c.names.nameOf(entry.written_by)}
                <li id={entry.id} class="held" aria-label={entry.statement}>
                  <p>
                    {#if entry.written_by != null}<span class="icon-tile" data-tint="memory"><Icon name="agent" /></span>{/if}{entry.statement}
                  </p>
                  {#if entry.about}<p>{t('memory-held-about', { about: entry.about })}</p>{/if}
                  {#if kind !== null}<p class="secondary">{t(kind)}</p>{/if}
                  <p class="secondary">
                    {#if entry.written_by == null}{t('memory-held-from-colleague')}
                    {:else if agent !== null}{t('memory-held-by-agent', { agent })}
                    {:else}{t('memory-held-by-an-agent')}{/if}
                  </p>
                  {#if entry.cited}<p class="secondary">{t('memory-held-cites', { source: entry.cited })}</p>{/if}
                  {#if entry.things.length > 0}
                    <p>{t('memory-held-names')}</p>
                    <ul>
                      {#each entry.things as thing (thing.name)}
                        <li>
                          {thing.name}
                          {#if thing.other_names.length > 0}· {t('memory-thing-also-called', { names: thing.other_names.join(', ') })}{/if}
                        </li>
                      {/each}
                    </ul>
                  {/if}
                  {#if entry.links.length > 0}
                    <p>{t('memory-held-links')}</p>
                    <ul>
                      {#each entry.links as link, i (i)}
                        <li>
                          {t(link.ended ? 'memory-held-link-ended' : 'memory-held-link', { from: link.from, kind: link.kind, to: link.to })}
                          {#if link.confidence_permille != null}· {t('memory-held-link-sure', { percent: formatPercent(link.confidence_permille / 1000) })}{/if}
                        </li>
                      {/each}
                    </ul>
                  {/if}
                  {#if entry.needs_check}<p class="notice">{t('memory-held-needs-check')}</p>{/if}
                  <p class="secondary">{formatDate(entry.held_at)}</p>
                  <ErrorLine error={c.errorOn(entry.id)} />
                  <button type="button" onclick={() => void c.keepHeldWrite(entry)}>{t('memory-keep')}</button>
                  <button type="button" onclick={() => void c.discardHeldWrite(entry)}>{t('memory-held-discard')}</button>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/if}
      {#if c.citations !== null}
        <section aria-labelledby="memory-citations-title">
          <h3 id="memory-citations-title" tabindex="-1">{t('memory-citations-title')}</h3>
          <p>{t('memory-citations-intro')}</p>
          {#if c.citations.length === 0}
            <p>{t('memory-citations-empty')}</p>
          {:else}
            <ul class="citation-list">
              {#each c.citations as fact (fact.id)}
                <FactLine {fact} names={c.names} />
                <li class="citation-actions">
                  <ErrorLine error={c.errorOn(fact.id)} />
                  <button type="button" onclick={() => void c.confirmSource(fact)}>{t('memory-citation-confirm')}</button>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/if}
    </div>
  {/if}

  {#if c.pending !== null}
    <ConfirmDialog
      title={pendingTitle}
      body={pendingBody}
      verb={pendingVerb}
      initialFocus={c.pending.kind === 'thing' ? 'cancel' : 'heading'}
      onconfirm={() => void c.confirmPending()}
      oncancel={() => c.cancelPending()}
    />
  {/if}
  <div class="visually-hidden" aria-live="polite" role="status">{c.announcer.text || team.announcer.text}</div>
</section>

<style>
  [role='tablist'] {
    display: flex;
    gap: var(--space-04);
    border-block-end: 1px solid var(--color-line);
    margin-block: var(--space-12);
  }
  [role='tab'][aria-selected='true'] {
    border-block-end: 2px solid var(--color-accent);
    font-weight: 600;
  }
  .fact-list,
  .thing-list,
  .held-list {
    padding: 0;
  }
  /* The prototype's `.facts`: rows 6 px apart. */
  .fact-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .held {
    list-style: none;
    border-block-end: 1px solid var(--color-line);
    padding-block: var(--space-sm);
  }
  .secondary {
    color: var(--color-text-muted);
    margin: 0;
  }
  .icon-tile {
    vertical-align: middle;
    margin-inline-end: var(--space-04);
  }
  .notice {
    color: var(--color-waiting-text);
  }
  button.link {
    border: none;
    background: none;
    color: var(--color-accent);
    text-decoration: underline;
    padding: 0;
  }
</style>
