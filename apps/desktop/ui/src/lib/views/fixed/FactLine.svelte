<script lang="ts">
  // One fact, as What Ordin remembers, a thing's facts, What Ordin used and Remembered during this run
  // draw it (ADR-111 step 2): the statement; its kind in words; since when it is true; the fact it
  // replaced; a fading notice with Keep; the things it names; where it came from (SM-SF-6: an agent's
  // citation reads as the agent's and `origin_ref` is text, never a link); and who shared it (ADR-103,
  // ADR-110); an entry from an agent draws `i-agent` before its statement (V8). Presentational: the actions
  // arrive as callbacks.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-6
  import type { Snippet } from 'svelte';

  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { FactView } from '../../service-api/types.gen';
  import { AgentNames, KIND_MESSAGE, citationLine, truthLine } from './facts';

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
    /** Opened on choice: the facts a summary was made from. */
    madeFrom?: FactView[] | undefined;
    onmadefrom?: (fact: FactView) => void;
    /** The Adopt actions of the line (`AdoptControls`), drawn after the other actions. */
    adopt?: Snippet;
  }
  let { fact, names, replaced, onkeep, onforget, onhistory, onopenthing, onopenpage, onkeepchecked, madeFrom, onmadefrom, adopt }: Props = $props();

  const truth = $derived(truthLine(fact.true_from, fact.true_until));
  const cite = $derived(citationLine(fact, names));
  const fromAgent = $derived(fact.came_from.kind === 'agent' || fact.came_from.kind === 'agent_citing');
</script>

<li class="fact-line" aria-label={fact.statement}>
  <p class="statement">
    {#if fromAgent}<span class="icon-tile" data-tint="memory"><Icon name="agent" /></span>{/if}{fact.statement}
  </p>
  <p class="secondary">
    <span>{t(KIND_MESSAGE[fact.kind])}</span>
    · <span>{t(truth.id, truth.args)}</span>
  </p>
  {#if replaced !== undefined}<p class="secondary">{t('memory-replaced', { statement: replaced })}</p>{/if}
  {#if fact.fading}
    <p class="notice">
      <span aria-hidden="true">⚠</span>
      {t('memory-fading', { date: formatDate(fact.not_used_since ?? fact.written_at) })}
      {#if onkeep}<button type="button" class="small" onclick={() => onkeep(fact)}>{t('memory-keep')}</button>{/if}
    </p>
  {/if}
  {#if cite !== null}<p class="secondary">{t(cite.id, cite.args)}</p>{/if}
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
  {#if fact.things && fact.things.length > 0}
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
    <button type="button" class="small" onclick={() => onopenpage(fact)}>{t('memory-open-page')}</button>
  {/if}
  <span class="fact-actions">
    {#if onhistory}<button type="button" class="small" onclick={() => onhistory(fact)}>{t('memory-history')}</button>{/if}
    {#if onkeepchecked && fact.needs_check}<button type="button" class="small" onclick={() => onkeepchecked(fact)}>{t('team-check-keep')}</button>{/if}
    {#if onforget}<button type="button" class="small" onclick={() => onforget(fact)}>{t('memory-link-forget')}</button>{/if}
    {@render adopt?.()}
  </span>
</li>

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
    gap: var(--space-sm);
  }
</style>
