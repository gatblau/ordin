<script lang="ts">
  // `plan_progress`: three layers (11 §5.4) from the store's run or crew: *Now*, the steps, and *What
  // happened*. The same words are used everywhere progress is shown. A step is a symbol and a word, never
  // colour alone.
  //
  // spec: ordin-desktop/components/ScreensModule#ADR-111
  // spec: ordin-desktop/components/ScreensModule#SM-PG-2
  // spec: ordin-desktop/components/ScreensModule#SM-PG-3
  import { formatNumber } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import { stateMessageId } from '../../i18n/run-state';
  import type { ServiceError, StepMark } from '../../service-api/types.gen';
  import { getApp } from '../../shell/context';
  import { answers } from '../../backend/groups';
  import { openScreen } from '../../shell/open-item';
  import { ScreenAreaState } from '../../shell/screen-area.svelte';
  import { makeKey, SCREEN_IDS } from '../../shell/screens';
  import Icon from '../../icons/Icon.svelte';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import RememberedInRun from '../../views/fixed/RememberedInRun.svelte';
  import ActivityDot from '../ActivityDot.svelte';
  import AskButton from '../AskButton.svelte';
  import { isWaitingForServer } from '../ask';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'plan_progress'> = $props();
  const app = getApp();

  let showHappened = $state(false);
  let busy = $state(false);
  let error = $state<ServiceError | null>(null);

  const run = $derived(block.subject.kind === 'run' ? backend.runs.get(block.subject.run_id) : undefined);
  const crew = $derived(block.subject.kind === 'crew_run' ? backend.crews.get(block.subject.crew_run_id) : undefined);
  const stoppableGroup = $derived(block.subject.kind === 'run' ? 'runs' : 'crews');
  // SM-PG-2: a run that completed has finished its plan, so a step the agent never marked is drawn as done.
  const steps = $derived(
    (run?.plan?.steps.slice().sort((a, b) => a.position - b.position) ?? []).map((s) =>
      run?.state === 'completed' && (s.mark === 'next' || s.mark === 'now') ? { ...s, mark: 'done' as StepMark } : s,
    ),
  );
  const nowStep = $derived(steps.find((s) => s.mark === 'now'));
  const runState = $derived(run?.state ?? crew?.state);
  const stoppable = $derived(run !== undefined ? ['queued', 'running', 'paused'].includes(run.state) : crew !== undefined && ['starting', 'running', 'paused'].includes(crew.state));
  const lines = $derived(run?.progressLines.flatMap((l) => (l.kind === 'line' ? [l.line] : [])) ?? []);

  // The colour of a run state's label, as the prototype's run states draw them (scene `runs`, V6).
  const stateTone = (state: string): string =>
    state === 'completed' || state === 'finished'
      ? 'done'
      : ['failed', 'interrupted', 'killed'].includes(state)
        ? 'failed'
        : ['paused', 'stopped_at_limit', 'no_reply', 'abandoned', 'escalated'].includes(state)
          ? 'waiting'
          : '';
  const symbol: Record<StepMark, string> = { done: '✓', now: '▶', next: '○', skipped: '↷' };
  const word = (mark: StepMark): string =>
    mark === 'done' ? t('progress-step-done') : mark === 'now' ? t('progress-step-now') : mark === 'skipped' ? t('progress-step-skipped') : t('progress-step-next');
  const countLine = $derived.by(() => {
    const c = run?.count;
    if (c === undefined || c === null) return null;
    return c.total === undefined || c.total === null
      ? t('progress-count-so-far', { done: formatNumber(c.done) })
      : t('progress-count-of', { done: formatNumber(c.done), total: formatNumber(c.total) });
  });

  // SM-PG-3: Review opens the run's screen; on that screen it would open itself, so it is not drawn there.
  const runScreenKey = $derived(block.subject.kind === 'run' ? makeKey(SCREEN_IDS.run, block.subject.run_id) : block.subject.kind === 'crew_run' ? makeKey(SCREEN_IDS.crew, block.subject.crew_run_id) : null);
  const onItsScreen = $derived(runScreenKey !== null && app.screens.front(ScreenAreaState.scope(backend.id, workspaceId)) === runScreenKey);

  async function stop(): Promise<void> {
    if (busy || isWaitingForServer(backend)) return;
    busy = true;
    const subject = block.subject;
    const out =
      subject.kind === 'run'
        ? await backend.handle.call('runs.stop', { run_id: subject.run_id })
        : subject.kind === 'crew_run'
          ? await backend.handle.call('crews.stop', { crew_run_id: subject.crew_run_id })
          : null;
    busy = false;
    if (out === null) return;
    error = out.kind === 'error' ? out.error : null;
  }

  function review(): void {
    const subject = block.subject;
    if (subject.kind === 'run') openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.run, subject.run_id));
    else if (subject.kind === 'crew_run') openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.crew, subject.crew_run_id));
  }
</script>

<div class="block-plan-progress">
  <p class="now" aria-live="polite">
    {#if runState === 'running'}<ActivityDot />{/if}
    {#if nowStep !== undefined}
      {t('progress-now', { sentence: nowStep.line, n: formatNumber(nowStep.position), total: formatNumber(steps.length) })}
    {:else if runState !== undefined}
      <span class="state-label {stateTone(runState)}">{t(stateMessageId(runState))}</span>
    {/if}
  </p>
  {#if countLine !== null}<p class="count-line">{countLine}</p>{/if}
  {#if steps.length > 0}
    <ol class="steps">
      {#each steps as step (step.id)}
        <li class={step.mark}><span class="state-label" class:done={step.mark === 'done'}><span aria-hidden="true">{symbol[step.mark]}</span> {word(step.mark)}</span> <span class="line">{step.line}</span></li>
      {/each}
    </ol>
  {/if}
  <div class="actions">
    {#if stoppable && backend.groups.has(stoppableGroup)}
      <button type="button" disabled={busy || isWaitingForServer(backend)} onclick={() => void stop()}><Icon name="stop" />{t('progress-stop-note')}</button>
    {/if}
    {#if !onItsScreen}<button type="button" onclick={review}>{t('block-review')}</button>{/if}
    <button type="button" aria-expanded={showHappened} onclick={() => (showHappened = !showHappened)}>{t('progress-show-what-happened')}</button>
    <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-progress')} />
  </div>
  {#if showHappened}
    {#if lines.length > 0}
      <ul class="happened">
        {#each lines as line, index (index)}<li>{line}</li>{/each}
      </ul>
    {:else}
      <p>{t('progress-no-activity')}</p>
    {/if}
    {#if block.subject.kind === 'run' && answers(backend, 'memory.remembered_in_run')}
      <RememberedInRun {backend} runId={block.subject.run_id} {workspaceId} running={run?.state === 'running'} refreshKey={run?.progressLines.length ?? 0} />
    {/if}
  {/if}
  <ErrorLine {error} />
</div>

<style>
  /* Three layers (scenes `progress`, `runs`): *Now* first, then one row per step with its state label. */
  .block-plan-progress {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .block-plan-progress > p {
    margin: 0;
  }
  .now {
    font-weight: 600;
  }
  .count-line {
    font-size: 13px;
    color: var(--color-text-muted);
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .steps li {
    display: flex;
    gap: var(--space-sm);
    align-items: center;
    flex-wrap: wrap;
    font-size: 14px;
  }
  .steps li.now .line {
    font-weight: 600;
  }
  .steps li:is(.next, .skipped) .line {
    color: var(--color-text-muted);
  }
  .happened {
    margin: 0;
    padding-inline-start: 18px;
    font-size: 13px;
    color: var(--color-text-secondary);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
</style>
