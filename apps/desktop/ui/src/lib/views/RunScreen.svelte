<script lang="ts">
  // The run screen (SM-RS-1): one run, opened from Home, Running now or a progress card. The agent's name
  // and the run's state; what it was asked; while it runs, its progress (the `plan_progress` block); once
  // it has ended, what it produced: its final message and files (the `produced` result), read from the
  // journal's `run_result` entry, or the first 500 characters the run keeps when the journal cannot be read.
  // A task run that ended interrupted or failed offers **Try again** (SM-RS-8): the same task, given to the
  // same agent again; the new run's screen opens, or the line says the conversation needs the person first.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-RS-1
  // spec: ordin-desktop/components/ScreensModule#SM-RS-8
  import PlanProgress from '../blocks/kinds/PlanProgress.svelte';
  import Result from '../blocks/kinds/Result.svelte';
  import { answers } from '../backend/groups';
  import { t } from '../i18n/fluent';
  import { stateMessageId } from '../i18n/run-state';
  import type { RunView, ServiceError } from '../service-api/types.gen';
  import { openScreen } from '../shell/open-item';
  import { makeKey, SCREEN_IDS, type ScreenProps } from '../shell/screens';
  import ErrorLine from './ErrorLine.svelte';
  import { appOrNull } from './fixed/util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in a handler.
  const app = appOrNull();

  // svelte-ignore state_referenced_locally
  const runId: string | null = instance;
  let view = $state<RunView | null>(null);
  let agentName = $state<string | null>(null);
  let finalMessage = $state<string | null>(null);
  let artefacts = $state<string[]>([]);
  let error = $state<ServiceError | null>(null);

  const LIVE = ['queued', 'running', 'paused'];
  const live = $derived(view !== null && LIVE.includes(view.run.state));
  const brief = $derived(view?.task?.free_text ?? view?.task?.command ?? null);
  // SM-RS-8: the run-state table's next action for these two states.
  const STOPPED = ['interrupted', 'failed'];
  const canTryAgain = $derived(view !== null && view.task != null && STOPPED.includes(view.run.state) && answers(backend, 'runs.try_again'));
  let trying = $state(false);
  let tryError = $state<ServiceError | null>(null);
  let waitingText = $state<string | null>(null);

  async function tryAgain(): Promise<void> {
    if (view === null || runId === null || trying) return;
    const ws = view.workspace_id;
    trying = true;
    tryError = null;
    waitingText = null;
    const out = await backend.handle.call('runs.try_again', { workspace_id: ws, run_id: runId });
    trying = false;
    if (out.kind !== 'ok') {
      tryError = out.error;
      return;
    }
    if (out.result.kind === 'started') {
      if (app !== null) openScreen(app, backend.id, ws, makeKey(SCREEN_IDS.run, out.result.run_id));
    } else {
      waitingText = out.result.text;
    }
  }

  async function load(): Promise<void> {
    if (runId === null) return;
    const out = await backend.handle.call('runs.get', { run_id: runId });
    if (out.kind !== 'ok') {
      error = out.error;
      return;
    }
    error = null;
    view = out.result;
    const agent = out.result.run.agent;
    if (agent.kind === 'agent_instance' && answers(backend, 'agents.get')) {
      const a = await backend.handle.call('agents.get', { agent_instance_id: agent.agent_instance_id });
      if (a.kind === 'ok') agentName = a.result.instance.display_name;
    }
    finalMessage = out.result.run.end_reason ?? null;
    if (!LIVE.includes(out.result.run.state) && answers(backend, 'journal.list_entries')) {
      const j = await backend.handle.call('journal.list_entries', { filter: { run_id: runId, kind: 'run_result' } });
      if (j.kind === 'ok') {
        const data = j.result.items[0]?.data as { final_message?: string; artefacts?: string[] } | undefined;
        if (typeof data?.final_message === 'string') finalMessage = data.final_message;
        if (Array.isArray(data?.artefacts)) artefacts = data.artefacts;
      }
    }
  }
  $effect(() => {
    void load();
  });
</script>

<section class="run-screen" aria-labelledby="run-screen-heading">
  <ErrorLine {error} onretry={() => void load()} />
  {#if view !== null && runId !== null}
    <h2 id="run-screen-heading">{agentName ?? t('screen-run')}</h2>
    <p class="state">{t(stateMessageId(view.run.state))}</p>
    {#if brief !== null}<p>{t('run-screen-asked', { brief })}</p>{/if}
    {#if live && workspaceId !== null}
      <PlanProgress {backend} {workspaceId} blockId={`run:${runId}`} block={{ kind: 'plan_progress', subject: { kind: 'run', run_id: runId } }} />
    {:else if workspaceId !== null && (finalMessage !== null || artefacts.length > 0)}
      <Result
        {backend}
        {workspaceId}
        blockId={`run:${runId}`}
        block={{ kind: 'result', of: 'produced', run_id: runId, summary: finalMessage ?? t('run-screen-no-message'), items: artefacts.map((artefact_id) => ({ kind: 'artefact' as const, artefact_id })) }}
      />
    {:else if !live}
      <p>{t('run-screen-no-message')}</p>
    {/if}
    {#if canTryAgain}
      <div class="try-again">
        <button type="button" disabled={trying} onclick={() => void tryAgain()}>{t('run-screen-try-again')}</button>
        {#if waitingText !== null}
          <p role="status">{t('run-screen-waiting')}</p>
        {/if}
        <ErrorLine error={tryError} onretry={() => void tryAgain()} />
      </div>
    {/if}
  {/if}
</section>

<style>
  .run-screen {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .state {
    color: var(--color-text-muted);
  }
  .try-again {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-04);
  }
</style>
