<script lang="ts">
  // Facts as the memory block draws them: the statement, the kind in words, and the source line. A fact an
  // agent cited reads as the agent's claim (SM-SF-6); `origin_ref` is text, never a link.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-6
  import type { AgentInstance, FactView } from '../service-api/types.gen';
  import { citationLine, kindLabel } from './citation';

  interface Props {
    facts: FactView[];
    agents: ReadonlyArray<Pick<AgentInstance, 'id' | 'display_name'>>;
  }
  let { facts, agents }: Props = $props();
</script>

<ul class="facts">
  {#each facts as fact (fact.id)}
    {@const cited = citationLine(fact, agents)}
    <li>
      <p class="statement">{fact.statement}</p>
      <p class="meta">{kindLabel(fact.kind)}</p>
      {#if cited !== null}<p class="meta source">{cited}</p>{/if}
    </li>
  {/each}
</ul>

<style>
  /* The prototype's `.facts` and `.fact` (scene `memory`): bordered rows 6 px apart, the statement in
     14 px, its kind and source in small muted type. */
  .facts {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .facts > li {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    padding: var(--space-06) var(--space-10);
    font-size: 14px;
  }
  .facts p {
    margin: 0;
  }
  .statement {
    overflow-wrap: anywhere;
  }
  .meta {
    color: var(--color-text-muted);
    font-size: 12px;
  }
</style>
