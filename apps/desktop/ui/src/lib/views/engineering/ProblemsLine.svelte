<script lang="ts">
  // The problems line of one coding task (SM-SF-2). The icon carries a text label, never colour alone.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-2
  import { t } from '../../i18n/fluent';
  import type { ReviewTask } from '../../service-api/types.gen';
  import { problemsLine } from './problems';

  interface Props {
    review: Pick<ReviewTask, 'task' | 'new_problems'>;
  }
  let { review }: Props = $props();

  const line = $derived(problemsLine(review));
  const glyph = $derived(line.icon === 'tick' ? '✓' : line.icon === 'warning' ? '⚠' : '?');
</script>

<div class="problems-line" data-row={line.row}>
  <p>
    <span aria-hidden="true">{glyph}</span>
    <span>{t(line.messageIds[0] as string, { count: line.count })}</span>
  </p>
  {#if line.problems.length > 0}
    <ul class="problem-list">
      {#each line.problems as problem, i (i)}
        <li>{problem.file}:{problem.line} {problem.message}</li>
      {/each}
    </ul>
  {/if}
  {#if line.messageIds[1] !== undefined}
    <p class="problems-also">{t(line.messageIds[1])}</p>
  {/if}
</div>

<style>
  .problems-line p {
    margin: 0;
    display: flex;
    gap: var(--space-sm);
    flex-wrap: wrap;
  }
  .problem-list {
    margin-block: var(--space-04);
    padding-inline-start: var(--space-md);
    color: var(--color-text-muted);
  }
</style>
