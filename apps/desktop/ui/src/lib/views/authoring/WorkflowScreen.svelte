<script lang="ts">
  // The workflow screen and its editor (SM-NS-5; instance = the workflow id). Icons per V8.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-5
  import { onMount } from 'svelte';

  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { WorkflowFileStep } from '../../service-api/types.gen';
  import { makeKey, type ScreenProps } from '../../shell/screens';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import Icon from '../../icons/Icon.svelte';
  import { closeScreen } from './support';
  import { appOrNull } from '../fixed/util';
  import { currentVersion, WorkflowController } from './workflow.svelte';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const workflowId = instance ?? '';
  const key = makeKey('workflow', instance);
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in a callback.
  const app = appOrNull();
  const c = new WorkflowController(backend.handle, workflowId, workspaceId, () => closeScreen(app, backend, workspaceId, key));
  const local = $derived(backend.kind === 'local');

  onMount(() => {
    void c.load();
  });

  const presence = (agentKey: string) => c.view?.agents.find((a) => a.key === agentKey)?.presence;
  const agentName = (agentKey: string): string => c.view?.file.agents.find((a) => a.key === agentKey)?.name ?? agentKey;
</script>

{#snippet stepList(steps: WorkflowFileStep[])}
  <ol class="steps">
    {#each steps as step, i (i)}
      {#if step.kind === 'agent'}
        <li class="step"><span class="number">{step.position}</span><span><b>{agentName(step.agent)}</b>: {step.line}</span></li>
      {:else if step.kind === 'for_each' || step.kind === 'repeat_until'}
        <li>{@render stepList(step.steps)}</li>
      {/if}
    {/each}
  </ol>
{/snippet}

<section class="workflow" aria-labelledby="workflow-heading">
  <h2 id="workflow-heading" tabindex="-1" data-region-focus>{c.view?.file.name ?? ''}</h2>
  <ErrorLine error={c.error} onretry={() => void c.load()} />
  {#if c.view !== null}
    {@const view = c.view}
    {#if view.file.purpose !== ''}<p>{view.file.purpose}</p>{/if}
    <p class="secondary small">{view.file.origin}</p>
    {#if c.renamedFrom !== null}<p class="secondary" role="status">{t('workflow-renamed-from', { name: c.renamedFrom })}</p>{/if}
    {#if c.savedAs !== null}<p class="secondary" role="status">{t('workflow-saved-as', { version: c.savedAs })}</p>{/if}
    {#if c.imported}
      <div class="band">
        <p>{t('workflow-imported-band')}</p>
        <button type="button" data-default="true" onclick={() => void c.saveImported()}>{t('workflow-save-to-workspace')}</button>
        <button type="button" onclick={() => void c.discardImported()}>{t('workflow-discard')}</button>
      </div>
    {/if}

    {#if c.editing && c.draft !== null}
      {#each c.notices as notice, i (i)}<p class="notice">{t(notice.message_id, notice.args)}</p>{/each}
      {#each c.unplacedProblems as problem, i (i)}<p class="problem" role="alert">{t(problem.message_id, problem.args)}</p>{/each}
    {/if}

    {#if (c.editing ? c.draft?.inputs : view.file.inputs)?.length}
      <h3>{t('workflow-asks-for')}</h3>
      <ul class="inputs" class:editing={c.editing}>
        {#if c.editing && c.draft !== null}
          {#each c.draft.inputs as input, i (i)}
            <li><input type="text" value={input.label} aria-label={input.label} oninput={(e) => c.setInputLabel(i, e.currentTarget.value)} /></li>
          {/each}
        {:else}
          {#each view.file.inputs as input, i (i)}<li>{input.label}</li>{/each}
        {/if}
      </ul>
    {/if}

    {#if view.file.agents.length > 0}
      <h3>{t('workflow-agents')}</h3>
      <ul class="members">
        {#each view.file.agents as agent (agent.key)}
          <li class="member">
            <span class="icon-tile" data-tint="accent-ink"><Icon name="agent" /></span>
            {agent.name}
            {#if presence(agent.key) === 'missing'}<span class="mark">{t('workflow-agent-missing')}</span>{/if}
            {#if presence(agent.key) === 'other_version'}<span class="mark">{t('workflow-agent-other-version')}</span>{/if}
          </li>
        {/each}
      </ul>
    {/if}

    <h3>{t('workflow-steps')}</h3>
    {#if c.editing && c.draft !== null}
      <ol class="steps">
        {#each c.stepIndexes as index, n (index)}
          {@const step = c.draft.steps[index]}
          {#if step !== undefined && step.kind === 'agent'}
            <li class="step editing" class:invalid={c.problemsAt(step.position).length > 0}>
              <span class="number">{step.position}</span>
              <span class="step-edit">
              <input type="text" class="line" value={step.line} aria-label={`${step.position} · ${agentName(step.agent)}`} oninput={(e) => c.setStepLine(index, e.currentTarget.value)} />
              {#each c.problemsAt(step.position) as problem, i (i)}<p class="problem" role="alert">{t(problem.message_id, problem.args)}</p>{/each}
              <span class="step-buttons">
                <button type="button" class="small" disabled={n === 0} onclick={() => c.moveStep(index, -1)}>{t('workflow-move-up')}</button>
                <button type="button" class="small" disabled={n === c.stepIndexes.length - 1} onclick={() => c.moveStep(index, 1)}>{t('workflow-move-down')}</button>
                <button type="button" class="small danger" onclick={() => c.removeStep(index)}>{t('workflow-remove-step')}</button>
              </span>
              </span>
            </li>
          {/if}
        {/each}
      </ol>
      <label class="note">
        {t('workflow-change-note')}
        <input type="text" bind:value={c.changeNote} required />
      </label>
      <div class="actions">
        <button type="button" data-default="true" disabled={c.changeNote.trim() === ''} onclick={() => void c.save()}>{t('doc-save')}</button>
        <button type="button" onclick={() => c.cancel()}>{t('doc-cancel')}</button>
        {#if c.askingDiscard}
          <span class="question" role="group" aria-label={t('doc-discard-question')}>
            {t('doc-discard-question')}
            <button type="button" onclick={() => c.discardChanges()}>{t('doc-discard-verb')}</button>
            <button type="button" onclick={() => c.keepChanging()}>{t('doc-keep-changing')}</button>
          </span>
        {/if}
      </div>
    {:else}
      {@render stepList(view.file.steps)}

      {#if view.versions.length > 0}
        <h3>{t('workflow-versions')}</h3>
        <ul class="list-rows">
          {#each view.versions as line (line.version)}
            <li class="list-row">
              <span class="grow">{t('workflow-version-line', { version: line.version, date: formatDate(line.saved_at), note: line.change_note })}</span>
              {#if line.version !== currentVersion(view)}<button type="button" class="small" onclick={() => void c.restore(line.version)}>{t('workflow-restore')}</button>{/if}
            </li>
          {/each}
        </ul>
      {/if}

      <div class="actions">
        <button type="button" class="primary" disabled={c.missingAgent} onclick={() => void c.run()}><Icon name="play" />{t('workflow-run')}</button>
        <button type="button" onclick={() => c.startEdit()}><Icon name="edit" />{t('workflow-change')}</button>
        <button type="button" onclick={() => c.openPanel('duplicate')}><Icon name="copy" />{t('workflow-duplicate')}</button>
        {#if local}<button type="button" onclick={() => c.openPanel('export')}><Icon name="open" />{t('workflow-export')}</button>{/if}
        <button type="button" onclick={() => c.openPanel('rename')}><Icon name="edit" />{t('workflow-rename')}</button>
        <button type="button" class="danger" onclick={() => c.openPanel('remove')}><Icon name="remove" />{t('workflow-remove')}</button>
      </div>
      {#if c.missingAgent}<p class="secondary">{t('workflow-run-missing-agent')}</p>{/if}

      {#if c.panel === 'rename'}
        <div class="panel" role="group" aria-label={t('workflow-rename')}>
          <label>
            {t('workflow-rename-field')}
            <input type="text" bind:value={c.renameName} minlength="1" maxlength="80" />
          </label>
          {#if c.renameEmpty}<p class="problem" role="alert">{t('workflow-rename-empty')}</p>{/if}
          <ErrorLine error={c.panelError} />
          <button type="button" data-default="true" onclick={() => void c.rename()}>{t('workflow-rename-verb')}</button>
          <button type="button" onclick={() => c.closePanel()}>{t('confirm-cancel')}</button>
        </div>
      {:else if c.panel === 'duplicate'}
        <div class="panel" role="group" aria-label={t('workflow-duplicate')}>
          <label>
            {t('workflow-rename-field')}
            <input type="text" bind:value={c.duplicateName} maxlength="80" />
          </label>
          <label>
            {t('workflow-duplicate-to')}
            <select bind:value={c.duplicateTo}>
              {#each c.workspaceChoices as space (space.id)}<option value={space.id}>{space.name}</option>{/each}
            </select>
          </label>
          <ErrorLine error={c.panelError} />
          <button type="button" data-default="true" disabled={c.duplicateName.trim() === ''} onclick={() => void c.duplicate()}>{t('workflow-duplicate-verb')}</button>
          <button type="button" onclick={() => c.closePanel()}>{t('confirm-cancel')}</button>
        </div>
      {:else if c.panel === 'export'}
        <div class="panel" role="group" aria-label={t('workflow-export')}>
          <p>{t('workflow-export-note')}</p>
          <ErrorLine error={c.panelError} />
          <button type="button" data-default="true" onclick={() => void c.export()}>{t('workflow-export')}</button>
          <button type="button" onclick={() => c.closePanel()}>{t('confirm-cancel')}</button>
        </div>
      {:else if c.panel === 'remove'}
        <ConfirmDialog
          title={t('workflow-remove-title', { name: view.file.name })}
          body={t('workflow-remove-body')}
          verb={t('workflow-remove')}
          onconfirm={() => void c.remove()}
          oncancel={() => c.closePanel()}
        />
      {/if}
    {/if}
  {/if}
  <div class="live" aria-live="polite">{c.announcer.text}</div>
</section>

<style>
  /* The prototype's workflow block on its screen (scene `flow`, `flowEdit`): a tight column whose
     section names are small bold words, not headings of a larger step. */
  .workflow {
    gap: var(--space-06);
  }
  .workflow > h2 {
    font-size: 19px;
    margin-block-end: var(--space-04);
  }
  .workflow h3 {
    font-size: 13px;
    margin-block-start: var(--space-04);
  }
  .secondary,
  .mark {
    color: var(--color-text-muted);
  }
  .small {
    font-size: 13px;
  }
  .mark {
    font-weight: 400;
    font-size: 13px;
  }
  .problem {
    color: var(--color-danger);
    font-size: 13px;
  }
  .inputs {
    margin: 0;
    padding-inline-start: 18px;
    font-size: 14px;
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  .inputs.editing {
    list-style: none;
    padding-inline-start: 0;
  }
  .members {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: var(--space-sm);
  }
  .member {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    padding: var(--space-sm) var(--space-10);
    font-size: 14px;
    font-weight: 700;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-04);
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .step {
    display: flex;
    gap: var(--space-10);
    align-items: flex-start;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    padding: var(--space-sm) var(--space-10);
    font-size: 14px;
  }
  .step.invalid {
    border-color: var(--color-danger);
  }
  .number {
    min-inline-size: 2.2em;
    block-size: 1.9em;
    border-radius: var(--radius-pill);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    font-weight: 600;
    font-size: 13px;
    background: var(--color-background);
    border: 1px solid var(--color-line-strong);
  }
  .step-edit {
    flex: 1;
    min-inline-size: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  .step-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-06);
  }
  .note {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
    font-size: 13px;
  }
  .note input {
    font-size: 16px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-06);
    align-items: center;
  }
  .list-rows {
    margin: 0;
  }
  .list-row {
    font-size: 14px;
  }
  .line {
    inline-size: 100%;
  }
  .live {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
  }
</style>
