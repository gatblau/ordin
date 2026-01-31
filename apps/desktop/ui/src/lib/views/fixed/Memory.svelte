<script lang="ts">
  // What Ordin remembers for a workspace (ADR-111; SM-SF-4, SM-SF-5): a WAI-ARIA tab list of Facts, Things
  // and, when either of its sections is wired, To check, with its count in the tab's accessible name.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-4
  // spec: ordin-desktop/components/ScreensModule#SM-SF-5
  // spec: ordin-desktop/components/ScreensModule#SM-SF-14
  // spec: ordin-desktop/components/ScreensModule#SM-SF-17
  // spec: ordin-desktop/components/ScreensModule#SM-SF-19
  // spec: ordin-desktop/components/ScreensModule#SM-SF-20
  // spec: ordin-desktop/components/ScreensModule#SM-SF-22
  // spec: ordin-desktop/components/ScreensModule#SM-SF-23
  // spec: ordin-desktop/components/ScreensModule#SM-SF-27
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { formatDate, formatList, formatPercent } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ScreenProps } from '../../shell/screens';
  import type { FactView, ServiceError, TermView } from '../../service-api/types.gen';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import AdoptControls from './AdoptControls.svelte';
  import FactLine from './FactLine.svelte';
  import {
    GROUP_BY,
    GROUP_BY_MESSAGE,
    isGroupBy,
    KIND_MESSAGE,
    TOPIC_MAX,
    WORD_LABEL_MAX,
    WORD_OBJECT_MESSAGE,
    addedByLine,
    citationLine,
    heldKindMessage,
    otherNamesOf,
    thingTitle,
    tripleLine,
    wordLabel,
    wordMeaning,
  } from './facts';
  import LinkLine from './LinkLine.svelte';
  import MemoryDiagram from './MemoryDiagram.svelte';
  import { MemoryController, type MemoryTab } from './memory-controller.svelte';
  import { TeamController } from './team.svelte';
  import { appOrNull } from './util';
  import TeamPanel from './TeamPanel.svelte';

  // `embedded`: drawn inside Settings › Memory, whose card carries the heading (amended January 2026 (user decision)).
  let { backend, workspaceId, embedded = false }: ScreenProps & { embedded?: boolean } = $props();
  // The window's diagnostics, where a diagram that cannot be drawn is recorded (SM-SF-21).
  const app = appOrNull();

  const c = new MemoryController(backend.handle, workspaceId ?? '', app?.prefs);
  // Adopt, Adopt again and Keep as it is need the workspace's own folder (ADR-110).
  const team = new TeamController(backend.handle, workspaceId ?? '');
  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));
  let first = true;
  let correcting = $state<{ id: string; kind: string; from: string; to: string } | null>(null);
  let sameAsPick = $state('');
  let sameAsOpen = $state(false);
  // Words (SM-SF-17): the one renamed in place, and the merge dialog.
  let renaming = $state<string | null>(null);
  let renameDraft = $state('');
  let merging = $state<{ term: TermView; into: string; error: ServiceError | null } | null>(null);
  let diagramSearch = $state('');
  // SM-SF-27: the groups drawn, and **Change topic…**.
  const grouping = $derived(c.grouping);
  let topicEdit = $state<{ id: string; name: string; choice: string; draft: string } | null>(null);
  /** The choice that means "a new topic" in **Change topic…** (never a topic: topics are 1–40 characters). */
  const NEW_TOPIC = '';
  const NO_TOPIC = '\u0000none';
  const groupByOffered = $derived(GROUP_BY.filter((by) => by !== 'topic' || c.topicsWired));

  function openTopic(id: string, name: string): void {
    const now = c.topicOfThing(id);
    topicEdit = { id, name, choice: now ?? (c.topicNames.length > 0 ? NO_TOPIC : NEW_TOPIC), draft: '' };
  }
  /** The topic **Change topic…** saves: the chosen one, the new one typed, or `null` for none; `undefined` while not ready. */
  const topicChosen = $derived.by((): string | null | undefined => {
    const e = topicEdit;
    if (e === null) return undefined;
    if (e.choice === NO_TOPIC) return null;
    if (e.choice !== NEW_TOPIC) return e.choice;
    const typed = e.draft.trim();
    return typed.length >= 1 && typed.length <= TOPIC_MAX ? typed : undefined;
  });
  async function saveTopic(): Promise<void> {
    const e = topicEdit;
    if (e === null || topicChosen === undefined) return;
    if (await c.setTopic(e.id, topicChosen)) topicEdit = null;
  }

  /** **Show as diagram** is drawn while the backend answers `memory.diagram` (SM-SF-23). */
  const diagramOffered = $derived(c.diagramWired && answers(backend, 'memory.diagram'));

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
    // Topics an agent gave settle once their things exist (SM-SF-27, amended January 2026).
    if (c.topicsWired) void c.loadTopics();
  });

  // The Words tab is read when first shown and after each change (SM-SF-17).
  $effect(() => {
    if (c.tab === 'words' && c.words === null) void c.loadWords();
  });

  const tabs = $derived<MemoryTab[]>(c.toCheckDrawn ? ['facts', 'things', 'words', 'to_check'] : ['facts', 'things', 'words']);
  const tabLabel = (tab: MemoryTab): string =>
    tab === 'facts'
      ? t('memory-tab-facts')
      : tab === 'things'
        ? t('memory-tab-things')
        : tab === 'words'
          ? t('memory-tab-words')
          : `${t('memory-tab-to-check')} (${c.toCheckCount})`;

  /** The facts a line conflicts with, each marked when its own line is in the same list (SM-SF-14). */
  const conflictsIn = (fact: FactView, list: readonly FactView[]): { fact: FactView; here: boolean }[] =>
    c.conflictsOf(fact).map((other) => ({ fact: other, here: list.some((x) => x.id === other.id) }));

  /** What **Keep this one** names: the facts Ordin stops using when the chosen one is kept. */
  const keepBody = $derived.by(() => {
    const p = c.keepPending;
    if (p === null) return undefined;
    const group = [p.fact, ...p.others];
    const kept = group.find((f) => f.id === p.choice) ?? p.fact;
    const stopped = kept.id === p.fact.id ? p.others : group.filter((f) => (kept.conflicts_with ?? []).includes(f.id));
    return t('memory-keep-this-body', { other: formatList(stopped.map((f) => tripleLine(f))) });
  });

  function startRename(term: TermView): void {
    renaming = term.key;
    renameDraft = wordLabel(term);
  }
  const renameReady = (term: TermView): boolean => {
    const label = renameDraft.trim();
    return label.length >= 1 && label.length <= WORD_LABEL_MAX && label !== wordLabel(term);
  };
  async function saveRename(term: TermView): Promise<void> {
    if (!renameReady(term)) return;
    if (await c.renameWord(term, renameDraft.trim())) renaming = null;
  }
  function startMerge(term: TermView): void {
    merging = { term, into: c.mergeTargets(term)[0]?.key ?? '', error: null };
  }
  async function confirmMerge(): Promise<void> {
    const m = merging;
    if (m === null || m.into === '') return;
    const error = await c.mergeWord(m.term, m.into);
    merging = error === null ? null : { ...m, error };
  }
  /** **Open {thing}** from a line beside the diagram: the thing's page, the diagram closed. */
  function openFromDiagram(id: string): void {
    c.closeDiagram();
    void c.openThing(id);
  }
  /**
   * **Facts about {thing}** (SM-SF-22, amended January 2026): the centre's facts grouped by word, in the
   * order each word is first met — the groups whose value is written (a text, a number, a date) first, then
   * the groups that link to another thing.
   */
  function factGroups(facts: FactView[], centreId: string): { key: string; word: string; facts: FactView[] }[] {
    const groups = new Map<string, { key: string; word: string; facts: FactView[]; links: boolean }>();
    for (const fact of facts) {
      const triple = fact.triple ?? null;
      const links = triple === null || triple.object.kind === 'thing' || triple.subject.id !== centreId;
      const key = `${links ? 'l' : 'v'}:${triple?.predicate.key ?? fact.id}`;
      const group = groups.get(key) ?? { key, word: triple?.predicate.label ?? '', facts: [], links };
      group.facts.push(fact);
      groups.set(key, group);
    }
    const all = [...groups.values()];
    return [...all.filter((g) => !g.links), ...all.filter((g) => g.links)];
  }
  /** The panel opened from an arrow or a value takes the focus to its title, so the person sees where it opened. */
  function focusTitle(node: HTMLElement): void {
    node.focus();
  }
  function diagramKey(event: KeyboardEvent): void {
    if (event.key !== 'Escape' || event.defaultPrevented) return;
    event.preventDefault();
    c.closeDiagram();
  }

  /** WAI-ARIA tabs (amended January 2026; user decision; Approved by the user January 2026): the arrows move
   * and show, Home and End go to the first and last tab; the focus follows the tab shown. */
  function onTabKey(event: KeyboardEvent, index: number): void {
    const last = tabs.length - 1;
    let to = -1;
    if (event.key === 'ArrowRight') to = index === last ? 0 : index + 1;
    else if (event.key === 'ArrowLeft') to = index === 0 ? last : index - 1;
    else if (event.key === 'Home') to = 0;
    else if (event.key === 'End') to = last;
    const target = tabs[to];
    if (target === undefined) return;
    event.preventDefault();
    c.tab = target;
    const list = (event.currentTarget as HTMLElement | null)?.parentElement;
    queueMicrotask(() => list?.querySelector<HTMLElement>(`#memory-tab-${target}`)?.focus());
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

{#snippet historyOf()}
  <!-- SM-SF-13 (amended January 2026; user decision; Approved by the user January 2026): the history of a
       fact, drawn inside that fact's line; its History button closes it again. -->
  {#if c.history !== null}
    <section class="history" aria-labelledby={`memory-history-heading-${c.history.fact.id}`}>
      <h4 id={`memory-history-heading-${c.history.fact.id}`}>{t('memory-history-title')}</h4>
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
      {#if c.history.view.links.length > 0}
        <ul>
          {#each c.history.view.links as link (link.id)}<LinkLine {link} names={c.names} pageTitles={c.pages.titles} />{/each}
        </ul>
      {/if}
    </section>
  {/if}
{/snippet}

<section class="memory" aria-labelledby={embedded ? undefined : 'memory-heading'}>
  {#if !embedded}<h2 id="memory-heading" tabindex="-1" data-region-focus>{t('tree-memory')}</h2>{/if}

  <TeamPanel {backend} workspaceId={workspaceId ?? ''} onopenchecked={() => (c.toCheckDrawn ? (c.tab = 'to_check') : (c.tab = 'facts'))} />

  {#if c.diagramPlace !== null}
    <!-- The diagram screen (SM-SF-20 to SM-SF-24), in place of the tabs; Escape is Back to the list. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <section class="diagram" aria-labelledby="memory-diagram-heading" onkeydown={diagramKey}>
      <h3 id="memory-diagram-heading" tabindex="-1">{t('memory-diagram-title')}</h3>
      <p>{t('memory-diagram-intro')}</p>
      <ErrorLine error={c.diagramError} actionLabel={t('memory-diagram-retry')} onretry={() => void c.reloadDiagram()} />
      {#if c.diagram !== null}
        {@const view = c.diagram}
        {#if view.left_out > 0}
          <p>{t('memory-diagram-left-out', { shown: view.things.length, total: view.total_things })}</p>
          <label>
            {t('memory-things-search')}
            <input
              type="search"
              bind:value={diagramSearch}
              oninput={() => void c.searchDiagram(diagramSearch)}
            />
          </label>
          {#if c.diagramMatches.length > 0}
            <ul class="diagram-matches">
              {#each c.diagramMatches as match (match.id)}
                <li><button type="button" class="link" onclick={() => { diagramSearch = ''; c.diagramMatches = []; void c.centre(match.id); }}>{thingTitle(match)}</button></li>
              {/each}
            </ul>
          {/if}
        {/if}
        <!-- SM-SF-22 (amended January 2026; user decision; Approved by the user January 2026): the drawing spans
             the pane; under it the things of the drawing as its keyboard equivalent, then only what the
             person selected: the fact pressed, or the facts of the thing pressed. -->
        <div class="diagram-body">
          <MemoryDiagram
            {view}
            diagnostics={app?.diagnostics}
            onpress={(press) => void (press.kind === 'thing' ? c.centre(press.id) : c.openDiagramFact(press.id))}
          />
          <h4 id="memory-diagram-things">{t('memory-diagram-things-title')}</h4>
          <ul class="diagram-things" aria-labelledby="memory-diagram-things">
            {#each view.things as entry (entry.thing.id)}
              <li><button type="button" aria-current={view.centre?.id === entry.thing.id ? 'true' : undefined} onclick={() => void c.centre(entry.thing.id)}>{entry.thing.name}</button></li>
            {/each}
          </ul>
          <div class="diagram-lists">
            {#if c.diagramFact !== null}
              {@const fact = c.diagramFact}
              <!-- SM-SF-22 (amended January 2026): the fact pressed on the drawing opens at the top of the lists,
                   beside the drawing (under it in a narrow window), with a title and Close. -->
              <section class="diagram-panel" aria-labelledby="memory-diagram-fact-heading">
                <div class="panel-head">
                  <h4 id="memory-diagram-fact-heading" tabindex="-1" use:focusTitle>{t('memory-diagram-fact-title')}</h4>
                  <button type="button" class="small" onclick={() => (c.diagramFact = null)}>{t('memory-diagram-fact-close')}</button>
                </div>
                <ul class="fact-list">
                  <FactLine
                    {fact}
                    expanded
                    names={c.names}
                    conflicting={conflictsIn(fact, [fact])}
                    oncentre={(thing) => void c.centre(thing.id)}
                    onopenthing={openFromDiagram}
                    onkeepside={(f) => c.askKeepSide(f)}
                    oncorrect={(f, object) => c.correct(f, object)}
                    onforget={(f) => void c.askForgetFact(f.id)}
                    onhistory={(f) => void c.openHistory(f)}
                    history={historyOf}
                    historyOpen={c.history?.fact.id === fact.id}
                  />
                  {#if c.errorOn(fact.id)}<ErrorLine error={c.errorOn(fact.id)} />{/if}
                </ul>
              </section>
            {/if}
            {#if view.centre && c.diagramDetail !== null}
              {@const facts = c.diagramDetail.facts}
              {@const centreId = view.centre.id}
              <h4>{t('memory-diagram-facts-title', { thing: view.centre.name })}</h4>
              <ul class="fact-groups">
                {#each factGroups(facts, centreId) as group (group.key)}
                <li><ul class="fact-list" aria-label={group.word || undefined}>
                {#each group.facts as fact (fact.id)}
                  <FactLine
                    {fact}
                    hideSubject={fact.triple?.subject.id === centreId}
                    names={c.names}
                    conflicting={conflictsIn(fact, facts)}
                    oncentre={(thing) => void c.centre(thing.id)}
                    onopenthing={openFromDiagram}
                    onkeepside={(f) => c.askKeepSide(f)}
                    oncorrect={(f, object) => c.correct(f, object)}
                    onforget={(f) => void c.askForgetFact(f.id)}
                    onhistory={(f) => void c.openHistory(f)}
                    history={historyOf}
                    historyOpen={c.history?.fact.id === fact.id}
                  />
                  {#if c.errorOn(fact.id)}<ErrorLine error={c.errorOn(fact.id)} />{/if}
                {/each}
                </ul></li>
                {/each}
              </ul>
            {/if}
            {#if c.diagramFact === null && !(view.centre && c.diagramDetail !== null)}
              <p class="secondary diagram-hint">{t('memory-diagram-select-hint')}</p>
            {/if}
          </div>
        </div>
      {/if}
      <button type="button" onclick={() => c.closeDiagram()}>{t('memory-diagram-back')}</button>
    </section>
  {:else}
  <div class="memory-tabs" role="tablist" aria-label={t('tree-memory')} aria-orientation="horizontal">
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
      <!-- SM-SF-27: Group by, one search over the three parts, and the filter chips. -->
      <div class="fact-tools">
        <label>
          {t('memory-group-by')}
          <select value={c.effectiveGroupBy} onchange={(e) => { const v = e.currentTarget.value; if (isGroupBy(v)) c.setGroupBy(v); }}>
            {#each groupByOffered as by (by)}<option value={by}>{t(GROUP_BY_MESSAGE[by])}</option>{/each}
          </select>
        </label>
        <label>
          {t('memory-facts-search')}
          <input type="search" value={c.factSearch} oninput={(e) => (c.factSearch = e.currentTarget.value)} />
        </label>
      </div>
      <div class="chips" role="group" aria-label={t('memory-filters')}>
        <button type="button" class="chip" aria-pressed={c.filters.conflicts} onclick={() => c.setFilter({ conflicts: !c.filters.conflicts })}>{t('memory-filter-conflicts')}</button>
        <button type="button" class="chip" aria-pressed={c.filters.outOfDate} onclick={() => c.setFilter({ outOfDate: !c.filters.outOfDate })}>{t('memory-filter-out-of-date')}</button>
        <button type="button" class="chip" aria-pressed={c.filters.older} onclick={() => c.setFilter({ older: !c.filters.older })}>{t('memory-filter-older')}</button>
        <button type="button" class="chip" aria-pressed={c.filters.scope === 'workspace'} onclick={() => c.setFilter({ scope: c.filters.scope === 'workspace' ? 'all' : 'workspace' })}>{t('memory-filter-workspace')}</button>
        <button type="button" class="chip" aria-pressed={c.filters.scope === 'everywhere'} onclick={() => c.setFilter({ scope: c.filters.scope === 'everywhere' ? 'all' : 'everywhere' })}>{t('memory-filter-everywhere')}</button>
        {#if c.topicsWired}
          <label class="chip">
            {t('memory-filter-topic')}
            <select value={c.filters.topic ?? NO_TOPIC + 'all'} onchange={(e) => { const v = e.currentTarget.value; c.setFilter({ topic: v === NO_TOPIC + 'all' ? null : v === NO_TOPIC ? '' : v }); }}>
              <option value={NO_TOPIC + 'all'}>{t('memory-filter-topic-all')}</option>
              {#each c.topicNames as topic (topic)}<option value={topic}>{topic}</option>{/each}
              <option value={NO_TOPIC}>{t('memory-group-no-topic')}</option>
            </select>
          </label>
        {/if}
      </div>
      {#if c.factsCursor !== null && (c.factSearch.trim() !== '' || grouping.facts < c.facts.length)}
        <p class="secondary">{t('memory-facts-search-loaded', { count: c.facts.length })}</p>
      {/if}
      {#if grouping.groups.length === 0 && c.facts.length > 0}
        <p>{t('memory-facts-none-match')}</p>
      {/if}
      <ul class="fact-groups">
        {#each grouping.groups as group (group.key)}
          {@const headingId = `memory-group-${group.key}`}
          <li class="fact-group">
            <h3 class="group-head" id={headingId}>
              <button type="button" class="group-toggle" aria-expanded={group.open} onclick={() => c.toggleGroup(group.key)}>
                <Icon name="chev" size="twist" />
                {t('memory-group-title', { name: group.name, count: group.facts.length })}
              </button>
            </h3>
            {#if group.open}
            <ul class="fact-list" aria-labelledby={headingId}>
        {#each group.shown as fact (fact.id)}
          <FactLine
            {fact}
            names={c.names}
            highlight={c.factSearch}
            hideSubject={c.effectiveGroupBy === 'thing' && fact.triple != null}
            expanded={c.detailsOpen}
            replaced={fact.replaces ? c.replaced[fact.replaces] : undefined}
            onkeep={(f) => void c.keep(f)}
            onforget={(f) => void c.askForgetFact(f.id)}
            onhistory={(f) => void c.openHistory(f)}
                    history={historyOf}
                    historyOpen={c.history?.fact.id === fact.id}
            onopenthing={(id) => void c.openThing(id)}
            onkeepchecked={(f) => void c.keepChecked(f)}
            conflicting={conflictsIn(fact, c.facts)}
            onkeepside={(f) => c.askKeepSide(f)}
            oncorrect={(f, object) => c.correct(f, object)}
            oneverywhere={(f) => void c.rememberEverywhere(f)}
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
            {#if group.more > 0}
              <button type="button" class="small" onclick={() => c.showMore(group.key)}>{t('memory-group-show-more', { count: Math.min(group.more, c.pageSize) })}</button>
            {/if}
            {/if}
          </li>
        {/each}
      </ul>
      {#if grouping.total > grouping.groups.length}
        <button type="button" onclick={() => c.showMoreGroups()}>{t('memory-groups-show-more')}</button>
      {/if}
      {#if c.factsCursor !== null}<button type="button" onclick={() => void c.moreFacts()}>{t('memory-facts-load-more')}</button>{/if}
    </div>
  {:else if c.tab === 'things'}
    <div role="tabpanel" id="memory-panel-things" aria-labelledby="memory-tab-things">
      {#if c.detail === null}
        <label>
          {t('memory-things-search')}
          <input type="search" value={c.thingSearch} oninput={(e) => c.searchThings(e.currentTarget.value)} />
        </label>
        {#if diagramOffered}
          <button type="button" onclick={() => void c.openDiagram(null)}>{t('memory-diagram-show')}</button>
        {/if}
        {#if c.things.length === 0}
          <p>{t('memory-things-empty')}</p>
        {:else}
          <!-- Amended November 2026 (user decision): one table row per thing, its icon buttons on the right. -->
          <table class="thing-list">
            <tbody>
              {#each c.things as thing (thing.id)}
                <tr>
                  <td>
                    <button type="button" class="link" onclick={() => void c.openThing(thing.id)}>{thingTitle(thing)}</button>
                    {#if otherNamesOf(thing).length > 0}<span> · {t('memory-thing-also-called', { names: otherNamesOf(thing).join(', ') })}</span>{/if}
                    <span> · {t('memory-thing-counts', { facts: thing.fact_count, links: thing.link_count })}</span>
                    {#if thing.explained_in}
                      <span> · {t('memory-explained-in', { page: thing.explained_in.title })}</span>
                    {/if}
                    {#if c.topicOfThing(thing.id)}<span class="thing-topic"> · {t('memory-topic', { topic: c.topicOfThing(thing.id) ?? '' })}</span>{/if}
                  </td>
                  <td class="row-actions">
                    {#if c.topicsWired}
                      <!-- SM-SF-27: Change topic…, now an icon button on the thing's row. -->
                      <button type="button" class="icon-btn" aria-label={t('memory-topic-change')} title={t('memory-topic-change')} onclick={() => openTopic(thing.id, thingTitle(thing))}><Icon name="edit" size="small" /></button>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if c.thingsCursor !== null}<button type="button" onclick={() => void c.moreThings()}>{t('fixed-show-more')}</button>{/if}
        {/if}
      {:else}
        {@const detail = c.detail}
        <button type="button" onclick={() => c.closeThing()}>{t('fixed-back')}</button>
        <h3>{thingTitle(c.detail.thing)}</h3>
        {#if diagramOffered}
          <button type="button" onclick={() => void c.openDiagram({ id: detail.thing.id, name: detail.thing.name })}>{t('memory-diagram-show')}</button>
        {/if}
        {#if otherNamesOf(c.detail.thing).length > 0}<p>{t('memory-thing-also-called', { names: otherNamesOf(c.detail.thing).join(', ') })}</p>{/if}
        {#if c.detail.thing.explained_in}<p>{t('memory-explained-in', { page: c.detail.thing.explained_in.title })}</p>{/if}
        {#if c.topicsWired}
          <!-- SM-SF-27: the thing's topic, and Change topic…. -->
          <p class="thing-topic-line">
            {c.topicOfThing(detail.thing.id) ? t('memory-topic', { topic: c.topicOfThing(detail.thing.id) ?? '' }) : t('memory-topic-none')}
            <button type="button" class="small" onclick={() => openTopic(detail.thing.id, thingTitle(detail.thing))}>{t('memory-topic-change')}</button>
          </p>
        {/if}
        <ul>
          {#each detail.facts as fact (fact.id)}
            <FactLine
              {fact}
              names={c.names}
              conflicting={conflictsIn(fact, detail.facts)}
              onkeepside={(f) => c.askKeepSide(f)}
              onforget={(f) => void c.askForgetFact(f.id)}
              onopenthing={(id) => void c.openThing(id)}
            />
            {#if c.errorOn(fact.id)}<ErrorLine error={c.errorOn(fact.id)} />{/if}
          {/each}
        </ul>
        {#if (detail.summaries ?? []).length > 0}
          <!-- SM-SF-19: Ordin's summaries naming the thing, with their date, what they were made from, and Forget. -->
          <section aria-labelledby="memory-summaries-heading">
            <h4 id="memory-summaries-heading">{t('memory-summaries-title')}</h4>
            <ul class="summary-list">
              {#each detail.summaries ?? [] as summary (summary.id)}
                <li>
                  <p>{summary.statement}</p>
                  <p class="secondary">
                    {formatDate(summary.written_at)}{#if summary.made_from_count != null}
                      · {t('memory-made-from-count', { count: summary.made_from_count })}{/if}
                  </p>
                  <button type="button" class="small" onclick={() => void c.askForgetFact(summary.id)}>{t('memory-link-forget')}</button>
                  {#if c.errorOn(summary.id)}<ErrorLine error={c.errorOn(summary.id)} />{/if}
                </li>
              {/each}
            </ul>
          </section>
        {/if}
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
  {:else if c.tab === 'words'}
    <div role="tabpanel" id="memory-panel-words" aria-labelledby="memory-tab-words">
      <p>{t('memory-words-intro')}</p>
      <ErrorLine error={c.errorOn('words')} onretry={() => void c.loadWords()} />
      {#if c.words !== null}
        <!-- Amended November 2026 (user decision): each kind of fact is a table row, with Rename and Merge
             into… as small icon buttons at its right end, each with its word as a tooltip. -->
        <table class="word-table">
          <tbody>
            {#each c.words as term (term.key)}
              {@const by = addedByLine(term.added_by, c.names)}
              {@const targets = c.mergeTargets(term)}
              <tr class="word">
                <td>
                  {#if renaming === term.key}
                    <p class="edit">
                      <input
                        type="text"
                        bind:value={renameDraft}
                        maxlength={WORD_LABEL_MAX}
                        aria-label={t('memory-word-rename')}
                        onkeydown={(e) => {
                          if (e.key === 'Enter') void saveRename(term);
                          if (e.key === 'Escape') renaming = null;
                        }}
                      />
                      <button type="button" class="small primary" disabled={!renameReady(term)} onclick={() => void saveRename(term)}>{t('memory-correct-save')}</button>
                      <button type="button" class="small" onclick={() => (renaming = null)}>{t('memory-correct-cancel')}</button>
                    </p>
                    <ErrorLine error={c.errorOn(`word:${term.key}`)} />
                  {:else}
                    <p><strong>{wordLabel(term)}</strong> · {wordMeaning(term)}</p>
                    {#if c.notAKind(term)}<p class="secondary not-a-kind">{t('memory-word-not-a-kind')}</p>{/if}
                  {/if}
                  <p class="secondary">
                    {t(WORD_OBJECT_MESSAGE[term.object])} · {t(term.single_valued ? 'memory-word-one' : 'memory-word-several')} · {t(by.id, by.args)} · {t('memory-word-count', { count: term.fact_count })}
                  </p>
                </td>
                <td class="row-actions">
                  <button type="button" class="icon-btn" aria-label={t('memory-word-rename')} title={t('memory-word-rename')} onclick={() => startRename(term)}><Icon name="edit" size="small" /></button>
                  {#if targets.length > 0}
                    <button type="button" class="icon-btn" aria-label={t('memory-word-merge')} title={t('memory-word-merge')} onclick={() => startMerge(term)}><Icon name="fold" size="small" /></button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
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
  {/if}


  {#if c.keepPending !== null}
    {@const keep = c.keepPending}
    <ConfirmDialog
      title={t('memory-keep-this-title')}
      body={keepBody}
      verb={t('memory-keep-this-verb')}
      onconfirm={() => void c.confirmKeepSide()}
      oncancel={() => c.cancelKeepSide()}
    >
      {#if keep.others.length >= 2}
        <div role="radiogroup" aria-label={t('memory-keep-this-title')} class="keep-choices">
          {#each [keep.fact, ...keep.others] as choice (choice.id)}
            <label><input type="radio" name="memory-keep-side" checked={keep.choice === choice.id} onchange={() => c.chooseKeep(choice.id)} />{tripleLine(choice)}</label>
          {/each}
        </div>
      {/if}
    </ConfirmDialog>
  {/if}

  {#if merging !== null}
    {@const merge = merging}
    <ConfirmDialog
      title={t('memory-word-merge-title')}
      body={t('memory-word-merge-body', { count: merge.term.fact_count })}
      verb={t('memory-word-merge-verb')}
      onconfirm={() => void confirmMerge()}
      oncancel={() => (merging = null)}
    >
      <div role="radiogroup" aria-label={t('memory-word-merge-title')} class="merge-choices">
        {#each c.mergeTargets(merge.term) as target (target.key)}
          <label><input type="radio" name="memory-merge-into" checked={merge.into === target.key} onchange={() => merging && (merging = { ...merging, into: target.key })} />{wordLabel(target)}</label>
        {/each}
      </div>
      <ErrorLine error={merge.error} />
    </ConfirmDialog>
  {/if}

  {#if topicEdit !== null}
    {@const edit = topicEdit}
    <!-- SM-SF-27: Change topic… — the place's topics, a new one, or none. -->
    <ConfirmDialog
      title={t('memory-topic-title', { thing: edit.name })}
      body={t('memory-topic-help')}
      verb={t('memory-topic-save')}
      destructive={false}
      onconfirm={() => void saveTopic()}
      oncancel={() => (topicEdit = null)}
    >
      <div role="radiogroup" aria-label={t('memory-topic-title', { thing: edit.name })} class="topic-choices">
        {#each c.topicNames as topic (topic)}
          <label><input type="radio" name="memory-topic" checked={edit.choice === topic} onchange={() => topicEdit && (topicEdit = { ...topicEdit, choice: topic })} />{topic}</label>
        {/each}
        <label><input type="radio" name="memory-topic" checked={edit.choice === NEW_TOPIC} onchange={() => topicEdit && (topicEdit = { ...topicEdit, choice: NEW_TOPIC })} />{t('memory-topic-new')}</label>
        {#if edit.choice === NEW_TOPIC}
          <input
            type="text"
            maxlength={TOPIC_MAX}
            aria-label={t('memory-topic-new')}
            value={edit.draft}
            oninput={(e) => topicEdit && (topicEdit = { ...topicEdit, draft: e.currentTarget.value })}
          />
        {/if}
        <label><input type="radio" name="memory-topic" checked={edit.choice === NO_TOPIC} onchange={() => topicEdit && (topicEdit = { ...topicEdit, choice: NO_TOPIC })} />{t('memory-topic-clear')}</label>
      </div>
      <ErrorLine error={c.errorOn(`topic:${edit.id}`)} />
    </ConfirmDialog>
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
  /* Real tabs, not a row of buttons (amended January 2026; user decision; Approved by the user January
     2026): words on the pane's line, the shown tab underlined in the accent colour. */
  .memory-tabs {
    display: flex;
    gap: var(--space-md);
    border-block-end: 1px solid var(--color-line);
    margin-block: var(--space-12);
  }
  .memory-tabs [role='tab'] {
    border: none;
    border-block-end: 2px solid transparent;
    border-radius: 0;
    background: none;
    margin-block-end: -1px;
    padding: var(--space-06) var(--space-02);
    color: var(--color-text-secondary);
  }
  .memory-tabs [role='tab']:hover {
    color: var(--color-text);
  }
  .memory-tabs [role='tab'][aria-selected='true'] {
    border-block-end-color: var(--color-accent);
    color: var(--color-text);
    font-weight: 600;
  }
  .fact-list,
  .held-list,
  .summary-list,
  .diagram-things,
  .diagram-matches {
    padding: 0;
  }
  .summary-list li,
  .diagram-things li,
  .diagram-matches li {
    list-style: none;
  }
  .word {
    border-block-end: 1px solid var(--color-line);
    padding-block: var(--space-sm);
  }
  .word p {
    margin: 0;
  }
  /* Kinds of fact and things: one table row each, its icon buttons at the right end. */
  .word-table,
  .thing-list {
    inline-size: 100%;
    border-collapse: collapse;
  }
  .thing-list td {
    border-block-end: 1px solid var(--color-line);
    padding-block: var(--space-04);
  }
  .word td {
    padding-block: var(--space-sm);
  }
  .row-actions {
    inline-size: 1%;
    white-space: nowrap;
    text-align: end;
    vertical-align: top;
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
  /* The diagram across the pane, and under it what the person selected. */
  .diagram-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    margin-block: var(--space-12);
    min-width: 0;
  }
  /* Things in the diagram: a compact run of small buttons that wraps. */
  .diagram-things {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-04);
    margin: 0;
  }
  .diagram-things button {
    padding: var(--space-02) var(--space-sm);
    border-radius: var(--radius-pill);
  }
  .diagram-things button[aria-current='true'] {
    font-weight: 600;
    border-color: var(--color-accent);
  }
  .diagram-lists {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    min-width: 0;
  }
  .diagram-lists h4 {
    margin: 0;
  }
  /* One group per word, a little more space between groups than between lines. */
  .fact-groups {
    display: flex;
    flex-direction: column;
    gap: var(--space-12);
    padding: 0;
    margin: 0;
  }
  .fact-groups > li {
    list-style: none;
  }
  .diagram-panel {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    padding: var(--space-12);
  }
  .panel-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-sm);
    margin-block-end: var(--space-sm);
  }
  .keep-choices,
  .merge-choices {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  /* SM-SF-27: Group by and the search on one row, the chips under them, each group a section. */
  /* Group by and the search on one row, label beside its control, controls of one height (amended January
     2026; user decision; Approved by the user January 2026). */
  .fact-tools {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-md);
    align-items: center;
  }
  .fact-tools label {
    display: inline-flex;
    align-items: center;
    gap: var(--space-sm);
    margin: 0;
  }
  .fact-tools select,
  .fact-tools input {
    box-sizing: border-box;
    block-size: 2rem;
    margin: 0;
    font: inherit;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-04);
    margin-block: var(--space-sm);
  }
  .chip {
    border-radius: var(--radius-pill);
    padding: var(--space-02) var(--space-sm);
  }
  button.chip[aria-pressed='true'] {
    border-color: var(--color-accent);
    font-weight: 600;
  }
  .group-head {
    margin: 0;
    font-size: inherit;
  }
  .group-toggle {
    border: none;
    background: none;
    font-weight: 600;
    padding: 0;
  }
  .group-toggle[aria-expanded='true'] :global(.icon) {
    transform: rotate(90deg);
  }
  .thing-topic {
    color: var(--color-text-muted);
  }
  .topic-choices {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
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
  /* A tool button carries its words beside its icon. */
</style>
