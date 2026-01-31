<script lang="ts">
  // One fact, as What Ordin remembers, a thing's facts, What Ordin used and Remembered during this run
  // draw it (ADR-111 step 2): the fact as its three parts — a thing, a word and a value (SM-SF-13) — or, for
  // an older note, its statement with the mark **Older note**; its kind in words; since when it is true; the
  // fact it replaced; a fading notice with Keep; the facts it conflicts with, with **Keep this one**
  // (SM-SF-14); where it came from (SM-SF-6: an agent's citation reads as the agent's and `origin_ref` is
  // text, never a link); and who shared it (ADR-103, ADR-110); an entry from an agent draws `i-agent` before
  // its statement (V8). **Correct** opens one field for the value by its kind (SM-SF-15). A page section
  // (a passage) is never drawn here (SM-SF-18). Presentational: the actions arrive as callbacks.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-6
  // spec: ordin-desktop/components/ScreensModule#SM-SF-11
  // spec: ordin-desktop/components/ScreensModule#SM-SF-13
  // spec: ordin-desktop/components/ScreensModule#SM-SF-14
  // spec: ordin-desktop/components/ScreensModule#SM-SF-15
  // spec: ordin-desktop/components/ScreensModule#SM-SF-18
  // spec: ordin-desktop/components/ScreensModule#SM-SF-22
  // spec: ordin-desktop/components/ScreensModule#SM-SF-27
  import type { Snippet } from 'svelte';

  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { FactView, ObjectInput, ThingRef } from '../../service-api/types.gen';
  import {
    AgentNames,
    draftOf,
    draftUnchanged,
    FACT_STATEMENT_MAX,
    highlightParts,
    KIND_MESSAGE,
    objectInputOf,
    objectText,
    scopeMessage,
    sourceLine,
    TEXT_VALUE_MAX,
    THING_NAME_MAX,
    tripleLine,
    tripleName,
    truthLine,
    UNIT_MAX,
    type ObjectDraft,
  } from './facts';

  interface Props {
    fact: FactView;
    names: AgentNames;
    /** The statement of the fact this one replaced, when it was read. */
    replaced?: string | undefined;
    onkeep?: (fact: FactView) => void;
    onforget?: (fact: FactView) => void;
    onhistory?: (fact: FactView) => void;
    onopenthing?: (id: string) => void;
    onopenpage?: (fact: FactView) => void;
    onkeepchecked?: (fact: FactView) => void;
    /** Saves the corrected value (SM-SF-15); the line offers **Correct** only when this is given. */
    oncorrect?: (fact: FactView, object: ObjectInput) => void | Promise<void>;
    /** Widens the fact to every workspace (SM-SF-11); offered only for a fact of one workspace. */
    oneverywhere?: (fact: FactView) => void;
    /** Opened on choice: the facts a summary was made from. */
    madeFrom?: FactView[] | undefined;
    onmadefrom?: (fact: FactView) => void;
    /** The facts this one conflicts with (SM-SF-14); `here` when that fact's line is in the same list. */
    conflicting?: { fact: FactView; here: boolean }[] | undefined;
    /** **Keep this one** (SM-SF-14); offered only when given. */
    onkeepside?: (fact: FactView) => void;
    /**
     * Beside the diagram (SM-SF-22): the subject and a thing value centre the diagram instead of opening the
     * thing, and the thing's page is reached from the thing's menu as **Open {thing}** (`onopenthing`).
     */
    oncentre?: (thing: ThingRef) => void;
    /** The fact's history, drawn inside the line under its details while `historyOpen` (SM-SF-13). */
    history?: Snippet;
    /** True while this fact's history is shown. */
    historyOpen?: boolean;
    /** The Adopt actions of the line (`AdoptControls`), drawn after the other actions. */
    adopt?: Snippet;
    /** The starting state: open when true (the panel beside the diagram, SM-SF-22); collapsed by default. */
    expanded?: boolean;
    /**
     * Under **Facts about {thing}** (SM-SF-22, amended January 2026) the heading names the subject, so the line
     * reads word · value; the accessible name keeps all three parts.
     */
    hideSubject?: boolean;
    /** The Facts list's search (SM-SF-27): its matches in the subject, word and value are marked. */
    highlight?: string;
  }
  let {
    fact,
    names,
    replaced,
    onkeep,
    onforget,
    onhistory,
    onopenthing,
    onopenpage,
    onkeepchecked,
    oncorrect,
    oneverywhere,
    madeFrom,
    onmadefrom,
    conflicting,
    onkeepside,
    oncentre,
    adopt,
    history,
    historyOpen = false,
    expanded = false,
    hideSubject = false,
    highlight = '',
  }: Props = $props();

  const marking = $derived(highlight.trim() !== '');
  const truth = $derived(truthLine(fact.true_from, fact.true_until));
  const source = $derived(sourceLine(fact, names));
  const scope = $derived(scopeMessage(fact));
  const triple = $derived(fact.triple ?? null);
  const olderNote = $derived(triple === null && fact.older_note === true);
  const label = $derived(triple === null ? fact.statement : tripleName(triple));
  let editing = $state(false);
  // SM-SF-13 (amended January 2026): a fact is collapsed to its claim and its subject until the person opens it.
  // svelte-ignore state_referenced_locally
  let open = $state(expanded);
  const hasConflict = $derived(conflicting !== undefined && conflicting.length > 0);
  let draft = $state('');
  let value = $state<ObjectDraft>({ kind: 'text', a: '', b: '', yes: false });
  let menuFor = $state<string | null>(null);
  const canSave = $derived.by(() => {
    if (triple !== null) return objectInputOf(value) !== null && !draftUnchanged(value, triple.object);
    return draft.trim() !== '' && draft.trim() !== fact.statement && draft.trim().length <= FACT_STATEMENT_MAX;
  });
  const valueLabel = $derived(triple === null ? '' : t('memory-correct-value-label', { word: triple.predicate.label }));

  function startEdit(): void {
    if (triple !== null) value = draftOf(triple.object);
    else draft = fact.statement;
    editing = true;
  }
  async function save(): Promise<void> {
    if (!canSave || !oncorrect) return;
    // A fact with no three parts that is not an older note keeps SM-SF-11's text box.
    const object = triple !== null ? objectInputOf(value) : { text: draft.trim() };
    if (object === null) return;
    await oncorrect(fact, object);
    editing = false;
  }
  function editKey(e: KeyboardEvent): void {
    if (e.key === 'Enter') void save();
    if (e.key === 'Escape') {
      // Escape closes the box only, not the diagram screen around it (SM-SF-20).
      e.stopPropagation();
      editing = false;
    }
  }
  /** A conflicting fact's line in the same list: scrolled to and focused (SM-SF-14). */
  function focusFact(id: string): void {
    const line = document.getElementById(`fact-line-${id}`);
    line?.scrollIntoView?.({ block: 'nearest' });
    line?.focus();
  }
  function pressThing(thing: ThingRef): void {
    if (oncentre) oncentre(thing);
    else onopenthing?.(thing.id);
  }
  function menuKey(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    menuFor = null;
  }
  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }
  const fromAgent = $derived(fact.came_from.kind === 'agent' || fact.came_from.kind === 'agent_citing');
</script>

{#snippet marked(text: string)}{#each highlightParts(text, highlight) as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}{/snippet}

{#snippet thingButton(thing: ThingRef)}
  {#if marking}<button type="button" class="link" onclick={() => pressThing(thing)}>{@render marked(thing.name)}</button>{:else}<button type="button" class="link" onclick={() => pressThing(thing)}>{thing.name}</button>{/if}
  {#if oncentre && onopenthing}
    <button
      type="button"
      class="thing-menu"
      aria-haspopup="menu"
      aria-expanded={menuFor === thing.id}
      title={t('more-actions-tip')}
      onclick={() => (menuFor = menuFor === thing.id ? null : thing.id)}
    ><Icon name="more" size="small" /><span class="visually-hidden">{t('more-actions-for', { name: thing.name })}</span></button>
    {#if menuFor === thing.id}
      <span role="menu" tabindex="-1" class="thing-menu-list" aria-label={t('tree-node-menu', { name: thing.name })} onkeydown={menuKey} use:focusFirst>
        <button type="button" role="menuitem" onclick={() => { menuFor = null; onopenthing?.(thing.id); }}>{t('memory-diagram-open-thing', { thing: thing.name })}</button>
      </span>
    {/if}
  {/if}
{/snippet}

{#if fact.passage !== true}
<li class="fact-line" id={`fact-line-${fact.id}`} tabindex="-1" aria-label={label}>
  {#if editing && triple !== null}
    <fieldset class="statement edit">
      <legend>{valueLabel}</legend>
      {#if value.kind === 'thing'}
        <input type="text" bind:value={value.a} maxlength={THING_NAME_MAX} aria-label={valueLabel} onkeydown={editKey} />
      {:else if value.kind === 'text'}
        <input type="text" bind:value={value.a} maxlength={TEXT_VALUE_MAX} aria-label={valueLabel} onkeydown={editKey} />
      {:else if value.kind === 'number'}
        <!-- SPEC-GAP-1: SM-SF-15 gives no words for the unit box (nor the currency box below); both are named by the group's legend. -->
        <input type="text" inputmode="decimal" bind:value={value.a} aria-label={valueLabel} onkeydown={editKey} />
        <input type="text" class="short" bind:value={value.b} maxlength={UNIT_MAX} onkeydown={editKey} />
      {:else if value.kind === 'money'}
        <input type="text" inputmode="decimal" bind:value={value.a} aria-label={valueLabel} onkeydown={editKey} />
        <input
          type="text"
          class="short"
          value={value.b}
          maxlength="3"
          oninput={(e) => {
            const upper = e.currentTarget.value.toUpperCase();
            e.currentTarget.value = upper;
            value.b = upper;
          }}
          onkeydown={editKey}
        />
      {:else if value.kind === 'date'}
        <input type="text" bind:value={value.a} maxlength="10" aria-label={valueLabel} aria-describedby={`date-hint-${fact.id}`} onkeydown={editKey} />
        <span class="secondary" id={`date-hint-${fact.id}`}>{t('memory-correct-date-hint')}</span>
      {:else}
        <span role="radiogroup" aria-label={valueLabel}>
          <label><input type="radio" name={`yes-no-${fact.id}`} checked={value.yes} onchange={() => (value.yes = true)} />{t('memory-value-yes')}</label>
          <label><input type="radio" name={`yes-no-${fact.id}`} checked={!value.yes} onchange={() => (value.yes = false)} />{t('memory-value-no')}</label>
        </span>
      {/if}
    </fieldset>
  {:else if editing}
    <p class="statement edit">
      <input type="text" bind:value={draft} maxlength={FACT_STATEMENT_MAX} aria-label={t('memory-correct-label')} onkeydown={editKey} />
    </p>
  {:else if triple !== null}
    <!-- SM-SF-13 (amended January 2026): one line that reads as a sentence, subject · word · value; the
         evidence line follows under Details. -->
    <p class="statement claim">
      {#if fromAgent}<span class="icon-tile" data-tint="memory"><Icon name="agent" /></span>{/if}{#if !hideSubject}<span class="subject">{@render thingButton(triple.subject)}</span>
      ·{' '}{/if}{#if marking}<span class="word">{@render marked(triple.predicate.label)}</span>{:else}<span class="word">{triple.predicate.label}</span>{/if}
      · {#if triple.object.kind === 'thing'}{@render thingButton(triple.object.thing)}{:else if marking}<span class="value">{@render marked(objectText(triple.object))}</span>{:else}<span class="value">{objectText(triple.object)}</span>{/if}
    </p>
  {:else}
    <p class="statement">
      {#if fromAgent}<span class="icon-tile" data-tint="memory"><Icon name="agent" /></span>{/if}{#if marking}{@render marked(fact.statement)}{:else}{fact.statement}{/if}
    </p>
  {/if}
  {#if !open && hasConflict}
    <!-- A conflict needs the person's attention, so its mark stays while the line is collapsed (SM-SF-14). -->
    <p class="conflict-mark"><Icon name="info" size="small" /><span>{t('memory-fact-conflict-mark')}</span></p>
  {/if}
  {#if open}
  <p class="secondary" id={`fact-details-${fact.id}`}>
    {#if triple !== null}
      <!-- A fact in three parts leads its evidence line with since when it is true (SM-SF-13). -->
      <span>{t(truth.id, truth.args)}</span>
      · {#if source !== null}<span class="source">{t(source.id, source.args)}</span> · {/if}<span>{t(KIND_MESSAGE[fact.kind])}</span>
    {:else}
      {#if source !== null}<span class="source">{t(source.id, source.args)}</span> · {/if}<span>{t(KIND_MESSAGE[fact.kind])}</span>
      {#if olderNote}<span class="tag" title={t('memory-older-note-help')}>{t('memory-older-note')}</span>{/if}
      · <span>{t(truth.id, truth.args)}</span>
    {/if}
    · <span class="scope">{t(scope)}</span>
    {#if fact.may_be_out_of_date}· <b class="stale">{t('memory-out-of-date')}</b>{/if}
  </p>
  {#if hasConflict && conflicting !== undefined}
    <p class="conflict">
      <Icon name="info" size="small" />
      <span>{t('memory-conflicts-with')}</span>
      {#each conflicting as other (other.fact.id)}
        {#if other.here}
          <button type="button" class="link" onclick={() => focusFact(other.fact.id)}>{tripleLine(other.fact)}</button>
        {:else}
          <span>{tripleLine(other.fact)}</span>
        {/if}
      {/each}
      {#if onkeepside}<button type="button" class="small" onclick={() => onkeepside(fact)}>{t('memory-keep-this')}</button>{/if}
    </p>
  {/if}
  {#if replaced !== undefined}<p class="secondary">{t('memory-replaced', { statement: replaced })}</p>{/if}
  {#if fact.fading}
    <p class="notice">
      <span aria-hidden="true">⚠</span>
      {t('memory-fading', { date: formatDate(fact.not_used_since ?? fact.written_at) })}
      {#if onkeep}<button type="button" class="small" onclick={() => onkeep(fact)}>{t('memory-keep')}</button>{/if}
    </p>
  {/if}
  {#if fact.shared_by}<p class="secondary">{t('team-shared-by', { name: fact.shared_by, date: formatDate(fact.shared_at ?? fact.written_at) })}</p>{/if}
  {#if fact.folder}
    <p class="secondary" title={fact.folder_description ?? undefined}>
      {t('team-from-folder', { folder: fact.folder })}
      {#if fact.set_by_company}<span class="tag">{t('team-set-by-company')}</span>{/if}
    </p>
  {/if}
  {#if fact.adopted_from}<p class="secondary">{t('team-adopted-from', { folder: fact.adopted_from })}</p>{/if}
  {#if fact.adoption_mark}
    <p class="notice">{t(fact.adoption_mark === 'changed_at_source' ? 'team-changed-at-source' : 'team-removed-at-source')}</p>
  {/if}
  {#if triple === null && fact.things && fact.things.length > 0}
    <ul class="thing-links">
      {#each fact.things as thing (thing.id)}
        <li><button type="button" class="link" onclick={() => onopenthing?.(thing.id)}>{thing.name}</button></li>
      {/each}
    </ul>
  {/if}
  {#if fact.kind === 'summary' && fact.made_from_count != null}
    <p class="secondary">
      {#if onmadefrom}
        <button type="button" class="link" aria-expanded={madeFrom !== undefined} onclick={() => onmadefrom(fact)}>
          {t('memory-made-from-count', { count: fact.made_from_count })}
        </button>
      {:else}
        {t('memory-made-from-count', { count: fact.made_from_count })}
      {/if}
    </p>
    {#if madeFrom !== undefined}
      <ul class="made-from">
        {#each madeFrom as origin (origin.id)}<li>{origin.statement}</li>{/each}
      </ul>
    {/if}
  {/if}
  {#if fact.page_id && onopenpage}
    <button type="button" class="icon-btn" aria-label={t('memory-open-page')} title={t('memory-open-page')} onclick={() => onopenpage(fact)}><Icon name="open" size="small" /></button>
  {/if}
  {/if}
  <!-- SM-SF-13 (amended January 2026; user decision; Approved by the user January 2026): the line's
       secondary actions are small icon buttons in the editor toolbar's style, each named by its verb and
       with that verb as its tooltip. Save and Cancel of a correction are icon buttons too (amended January
       2026; user decision; Approved by the user January 2026). -->
  <span class="fact-actions">
    <button
      type="button"
      class="icon-btn disclose"
      aria-expanded={open}
      aria-controls={open ? `fact-details-${fact.id}` : undefined}
      aria-label={open ? t('memory-fact-less') : t('memory-fact-more')}
      title={open ? t('memory-fact-less') : t('memory-fact-more')}
      onclick={() => {
        open = !open;
        if (!open) editing = false;
      }}
    ><Icon name="chev" size="small" /></button>
    {#if !open}
    {:else if editing}
      <button type="button" class="icon-btn primary" disabled={!canSave} aria-label={t('memory-correct-save')} title={t('memory-correct-save')} onclick={() => void save()}><Icon name="check" size="small" /></button>
      <button type="button" class="icon-btn" aria-label={t('memory-correct-cancel')} title={t('memory-correct-cancel')} onclick={() => (editing = false)}><Icon name="remove" size="small" /></button>
    {:else}
    {#if oncorrect && !olderNote}<button type="button" class="icon-btn" aria-label={t('memory-correct')} title={t('memory-correct')} onclick={startEdit}><Icon name="edit" size="small" /></button>{/if}
    {#if onhistory}<button type="button" class="icon-btn" aria-expanded={historyOpen} aria-label={t('memory-history')} title={t('memory-history')} onclick={() => onhistory(fact)}><Icon name="history" size="small" /></button>{/if}
    {#if onkeepchecked && fact.needs_check}<button type="button" class="icon-btn" aria-label={t('team-check-keep')} title={t('team-check-keep')} onclick={() => onkeepchecked(fact)}><Icon name="eye" size="small" /></button>{/if}
    {#if oneverywhere && fact.workspace_id}<button type="button" class="icon-btn" aria-label={t('memory-everywhere')} title={t('memory-everywhere')} onclick={() => oneverywhere(fact)}><Icon name="ws" size="small" /></button>{/if}
    {#if onforget}<button type="button" class="icon-btn" aria-label={t('memory-link-forget')} title={t('memory-link-forget')} onclick={() => onforget(fact)}><Icon name="bin" size="small" /></button>{/if}
    {/if}
    {#if open}{@render adopt?.()}{/if}
  </span>
  {#if open && historyOpen && history}
    <!-- SM-SF-13 (amended January 2026; user decision; Approved by the user January 2026): History opens
         inside the fact's own line, under its details, never at the foot of the list. -->
    <div class="fact-history">{@render history()}</div>
  {/if}
</li>
{/if}

<style>
  /* The prototype's `.fact` (scene `memory`): a bordered row, the statement and its small muted lines on
     the start side, the actions on the end side. */
  .fact-line {
    list-style: none;
    display: grid;
    grid-template-columns: minmax(200px, 1fr) auto;
    column-gap: var(--space-sm);
    align-items: center;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    padding: var(--space-06) var(--space-10);
    font-size: 14px;
  }
  .fact-line > :global(*) {
    grid-column: 1;
  }
  .fact-line > .fact-actions {
    grid-column: 2;
    grid-row: 1 / span 12;
  }
  .statement {
    margin: 0;
  }
  .icon-tile {
    vertical-align: middle;
    margin-inline-end: var(--space-04);
  }
  .secondary {
    margin: 0;
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .notice {
    margin: var(--space-04) 0;
    color: var(--color-waiting-text);
  }
  .edit input[type='text'] {
    width: 100%;
    box-sizing: border-box;
  }
  .edit input.short {
    width: 8em;
  }
  fieldset.edit {
    border: none;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-04);
    align-items: center;
  }
  .conflict {
    margin: var(--space-04) 0 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-04);
    align-items: center;
    font-size: 12px;
  }
  .word {
    color: var(--color-text-secondary);
  }
  /* One line read as a sentence: the subject and the value in the text colour, the word quieter between them. */
  .claim {
    font-weight: 400;
  }
  .claim .value {
    font-weight: 600;
  }
  .conflict-mark {
    margin: 0;
    display: inline-flex;
    gap: var(--space-04);
    align-items: center;
    font-size: 12px;
    color: var(--color-waiting-text);
  }
  .thing-menu {
    border: none;
    background: none;
    padding: 0 var(--space-04);
  }
  .thing-menu-list {
    display: inline-flex;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    background: var(--color-surface);
  }
  .stale {
    color: var(--color-waiting-text);
  }
  .tag {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding-inline: var(--space-04);
  }
  .thing-links {
    display: flex;
    gap: var(--space-sm);
    padding: 0;
    margin: 0;
  }
  .thing-links li {
    list-style: none;
  }
  button.link {
    border: none;
    background: none;
    color: var(--color-accent);
    text-decoration: underline;
    padding: 0;
  }
  .fact-actions {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-04);
    align-items: center;
  }
  /* The editor toolbar's icon button (MarkdownEditor.svelte `.md-tool`), a little smaller for a list line. */
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    padding: 0;
  }
  .icon-btn[aria-expanded='true'] {
    background: var(--color-accent-soft);
    color: var(--color-accent-ink);
  }
  .disclose[aria-expanded='true'] :global(.icon) {
    transform: rotate(90deg);
  }
  .fact-line > .fact-history {
    grid-column: 1 / -1;
    margin-block-start: var(--space-04);
    border-block-start: 1px solid var(--color-line);
    padding-block-start: var(--space-04);
    font-size: 12px;
  }
</style>
