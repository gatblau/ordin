<script lang="ts">
  // Changes waiting for a project (engineering mode): each coding task with its request, test outcome
  // and problems line (SM-SF-2), its entries accepted or rejected one by one, and the engineer's commit
  // and push. Attempts at one request sit side by side, each with Keep this one.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-2
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import { ChangesController } from './changes.svelte';
  import OpenInIde from './OpenInIde.svelte';
  import ProblemsLine from './ProblemsLine.svelte';
  import { projectOf } from './projects';

  let { backend, instance }: ScreenProps = $props();

  const controller = new ChangesController(backend.handle, instance ?? '');
  let message = $state('');
  const groups = $derived.by(() => {
    const by = new Map<string, typeof controller.tasks>();
    for (const review of controller.tasks) {
      const key = review.task.attempt_group_id ?? review.task.id;
      by.set(key, [...(by.get(key) ?? []), review]);
    }
    return [...by.values()];
  });

  onMount(() => {
    void controller.load();
  });
</script>

<section class="changes-waiting" aria-labelledby="changes-heading">
  <h2 id="changes-heading" tabindex="-1" data-region-focus>{t('engineering-changes-title')}</h2>
  <!-- Open in [IDE] for the repository (UX-A25); the Code group of the side tree is not drawn in this build. -->
  {#if (instance ?? '') !== ''}
    <OpenInIde {backend} projectId={instance ?? ''} name={projectOf(backend.handle.id, instance ?? '')?.display_name ?? t('engineering-changes-title')} />
  {/if}
  <ErrorLine error={controller.error} onretry={() => void controller.load()} />
  {#each groups as group, g (g)}
    <div class="attempts" class:side-by-side={group.length > 1}>
      {#each group as review (review.task.id)}
        <article class="task" aria-label={review.task.request}>
          <h3>{review.task.request}</h3>
          <p class="branch">{review.task.branch}</p>
          {#if review.task.test_outcome}
            <p>{t(review.task.test_outcome === 'passed' ? 'engineering-tests-passed' : 'engineering-tests-failed')}</p>
          {/if}
          <ProblemsLine {review} />
          {#if group.length > 1}
            <button type="button" class="small keep" onclick={() => void controller.keepAttempt(review.task.id)}>{t('engineering-keep-this-one')}</button>
          {/if}
          {#if review.change_set}
            <ul class="entries">
              {#each review.change_set.entries as entry (entry.id)}
                <li>
                  <span class="entry-file">{entry.file}</span>
                  <pre class="entry-hunk">{#each entry.hunk.split('\n') as line, i (i)}{#if i > 0}{'\n'}{/if}<span class:added={line.startsWith('+')} class:removed={line.startsWith('-')}>{line}</span>{/each}</pre>
                  {#if entry.state === 'pending'}
                    <button type="button" class="small" disabled={controller.busy} onclick={() => void controller.decide(entry.id, { kind: 'accept' })}>
                      {t('engineering-entry-accept')}
                    </button>
                    <button type="button" class="small" disabled={controller.busy} onclick={() => void controller.decide(entry.id, { kind: 'reject' })}>
                      {t('engineering-entry-reject')}
                    </button>
                  {/if}
                  {#if controller.conflicts[entry.id]}
                    <div class="conflict">
                      <button type="button" class="small" onclick={() => void controller.decide(entry.id, { kind: 'keep_yours' })}>{t('engineering-keep-yours')}</button>
                      <button type="button" class="small" onclick={() => void controller.decide(entry.id, { kind: 'use_the_agents' })}>
                        {t('engineering-use-the-agents')}
                      </button>
                    </div>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </article>
      {/each}
    </div>
  {/each}
  {#if controller.own.length > 0}
    <ul class="own-changes">
      {#each controller.own as entry (entry.path)}<li>{entry.path}</li>{/each}
    </ul>
  {/if}
  <form
    class="commit"
    onsubmit={(e) => {
      e.preventDefault();
      if (message.trim() !== '') void controller.commitAndPush(message.trim());
    }}
  >
    <label>
      {t('engineering-commit-message')}
      <input type="text" bind:value={message} />
    </label>
    <button type="submit" class="primary" disabled={controller.busy || message.trim() === ''}>{t('engineering-commit-and-push')}</button>
  </form>
</section>

<style>
  /* Changes waiting (scenes `eng`, `engTwo`: the prototype's `review` and `compareHTML`): a task's lines in
     Small text, each change in its own bordered part with Small buttons on its first row, a diff in Code
     text with added and removed lines filled; attempts side by side as two pages on the ground. */
  .changes-waiting h3 {
    font-size: 15px;
  }
  .task {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    margin-block-end: var(--space-12);
  }
  .task > p,
  .task :global(.problems-line) {
    font-size: 13px;
  }
  .branch {
    color: var(--color-text-muted);
  }
  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .entries > li {
    display: grid;
    grid-template-columns: 1fr auto auto;
    align-items: center;
    gap: var(--space-06);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    padding: var(--space-sm) var(--space-10);
    background: var(--color-surface);
  }
  .entry-file {
    grid-row: 1;
    font-weight: 700;
  }
  .entries > li > button {
    grid-row: 1;
  }
  .entry-hunk,
  .conflict {
    grid-column: 1 / -1;
  }
  .conflict {
    display: flex;
    gap: var(--space-sm);
  }
  .entry-hunk {
    margin: 0;
    overflow: auto;
    white-space: pre-wrap;
    font: 13px/1.5 var(--font-mono);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    background: var(--color-surface);
  }
  .entry-hunk > span {
    display: inline-block;
    box-sizing: border-box;
    min-inline-size: 100%;
    padding: 0 var(--space-sm);
  }
  .entry-hunk > .added {
    background: var(--color-diff-added);
  }
  .entry-hunk > .removed {
    background: var(--color-diff-removed);
    text-decoration: line-through;
  }
  .side-by-side {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-sm);
    margin-block-end: var(--space-12);
  }
  .side-by-side > .task {
    margin: 0;
    min-width: 0;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-10);
    background: var(--color-background);
    font-size: 13px;
  }
  .side-by-side > .task > h3 {
    font-size: 13px;
    font-weight: 400;
  }
  /* In a page, Keep this one is the last row, after the change (the prototype's order). */
  .side-by-side > .task > .keep {
    order: 1;
    align-self: flex-start;
  }
  .side-by-side .entries > li {
    border: 0;
    padding: 0;
    background: none;
  }
  .own-changes {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .own-changes > li {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-06) var(--space-10);
    background: none;
    font-size: 14px;
    font-weight: 700;
  }
  .commit {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    align-items: flex-start;
  }
  .commit label {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
    align-self: stretch;
    font-size: 13px;
  }
  .commit input {
    font: 13px var(--font-mono);
  }
</style>
