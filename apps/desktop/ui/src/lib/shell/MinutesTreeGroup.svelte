<script lang="ts">
  // The Minutes row under a workspace (ScreensModule.md SM-MIN-1, SM-MIN-17 to SM-MIN-21), after Dashboards.
  // Choosing it unfolds or folds it; no screen opens. Its ⋯ and right-click menu holds **Record…** (the
  // reminder, or what is wrong with OBS) and **Make minutes from a file…**. Under it: one row per job still
  // being worked on, with its stage and a menu of **Cancel** / **Try again**, then one row per kept minutes
  // (newest first) with the minutes menu (SM-MIN-13); a minutes row opens `minutes:<minutes id>`. The state
  // is read when the row is drawn, on each `workspace_changed`, when a recording ends, and every 2 seconds
  // while a job moves; the questions a job waits on are listed in *Needs you* (`watches`). Drawn only when
  // the backend answers `minutes.list` (SM-UP-1).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-1
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-13
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-18
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-19
  import { onDestroy } from 'svelte';

  import type { ServiceError } from '../backend/backend';
  import { formatNumber } from '../format/intl';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { MinutesJob, MinutesSummary } from '../service-api/minutes.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { appOrNull } from '../views/fixed/util';
  import MinutesMenu from '../views/minutes/MinutesMenu.svelte';
  import { forOrdin, inProgress, stageLine } from '../views/minutes/minutes-model';
  import { MinutesScreenModel } from '../views/minutes/minutes-screen.svelte';
  import RecordDialog from '../views/minutes/RecordDialog.svelte';
  import { recording } from '../views/minutes/recording.svelte';
  import { MINUTES_PAGE_SCREEN } from '../views/minutes/register-ids';
  import { watchMinutes } from '../views/minutes/watches.svelte';
  import { openScreen } from './open-item';
  import { ScreenAreaState } from './screen-area.svelte';
  import { makeKey } from './screens';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    initiallyOpen?: boolean;
  }
  let { backend, workspaceId, initiallyOpen = false }: Props = $props();

  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  let open = $state(initiallyOpen);
  // svelte-ignore state_referenced_locally
  const model = new MinutesScreenModel(backend.handle, workspaceId);
  model.onready = (title) => app?.shell.announce(t('minutes-ready', { title }));
  // svelte-ignore state_referenced_locally
  const unwatch = watchMinutes(backend.id, workspaceId, model);
  onDestroy(() => {
    unwatch();
    model.dispose();
  });

  let menuFor = $state<string | null>(null);
  let jobMenuFor = $state<string | null>(null);
  let groupMenu = $state(false);
  let actionError = $state<ServiceError | null>(null);
  const revision = $derived(backend.workspaceRevision(workspaceId));
  const working = $derived(model.jobs.filter(inProgress));

  $effect(() => {
    void revision;
    void recording.ended;
    void model.load().then(() => {
      if (model.error === null) app?.screens.closeMissing(ScreenAreaState.scope(backend.id, workspaceId), 'minutes', model.items.map((m) => m.id));
    });
  });

  function openMinutes(m: MinutesSummary): void {
    menuFor = null;
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(MINUTES_PAGE_SCREEN, m.id), { title: m.title });
  }

  /** **Record…** (SM-MIN-20): a fresh status, then the one dialog. */
  async function record(): Promise<void> {
    groupMenu = false;
    await model.readStatus();
    model.askRecord();
  }

  /** **Make minutes from a file…** (SM-MIN-18): the program's file dialog; the row unfolds to show the job. */
  async function importFile(): Promise<void> {
    groupMenu = false;
    await model.importFile();
    if (model.actionError === null) open = true;
  }

  function stage(job: MinutesJob): string {
    const line = stageLine(job.state);
    if (line === null) return t('minutes-job-failed');
    const args = line.args && Object.fromEntries(Object.entries(line.args).map(([k, v]) => [k, typeof v === 'number' ? formatNumber(v) : v]));
    return t(line.id, args);
  }

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  function escapeCloses(close: () => void): (e: KeyboardEvent) => void {
    return (e) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        close();
      }
    };
  }
</script>

<div role="treeitem" aria-selected="false" aria-expanded={open} tabindex="-1" class="tree-leaf" data-testid="minutes-tree">
  <div class="tree-line">
    <button type="button" class="tree-row" onclick={() => (open = !open)} oncontextmenu={(e) => { e.preventDefault(); groupMenu = true; }}><span class="icon-tile"><Icon name="mic" /></span>{t('minutes-tree')}</button>
    <span class="tree-actions" class:shown={groupMenu}>
      <button type="button" class="tree-action" data-testid="minutes-tree-menu" aria-haspopup="menu" aria-expanded={groupMenu} aria-label={t('more-actions-for', { name: t('minutes-tree') })} title={t('more-actions-tip')} onclick={() => (groupMenu = !groupMenu)}><Icon name="more" /></button>
    </span>
    {#if groupMenu}
      <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: t('minutes-tree') })} use:focusFirst onkeydown={escapeCloses(() => (groupMenu = false))}>
        {#if recording.view === null}<button type="button" role="menuitem" data-testid="minutes-record" onclick={() => void record()}><Icon name="mic" />{t('minutes-record-menu')}</button>{/if}
        <button type="button" role="menuitem" data-testid="minutes-import" disabled={model.busy} onclick={() => void importFile()}><Icon name="plus" />{t('minutes-import')}</button>
      </div>
    {/if}
  </div>
  <ErrorLine error={model.actionError ?? actionError} />
  {#if open}
    <ErrorLine error={model.error} onretry={() => void model.load()} />
    <div role="group" class="tree-group">
      {#each working as job (job.id)}
        {@const failed = job.state.state === 'failed'}
        <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf" data-testid="minutes-job" data-state={job.state.state}>
          <div class="tree-line">
            <span class="tree-row leaf tree-minutes job" title={job.state.state === 'failed' ? job.state.message : undefined} role="presentation" oncontextmenu={(e) => { e.preventDefault(); jobMenuFor = job.id; }}>
              <span class="icon-tile"><Icon name={job.source === 'transcript_file' ? 'page' : 'mic'} /></span>
              <span class="tree-minutes-name">{job.source_name}{#if forOrdin(job)}{` — ${t('minutes-job-for-ordin')}`}{/if}</span>
              {#if failed}<span class="mark"><Icon name="info" size="small" /></span>{/if}
              <span class="stage" data-testid={`minutes-stage-${job.state.state}`}>{stage(job)}</span>
            </span>
            <span class="tree-actions" class:shown={jobMenuFor === job.id}>
              <button type="button" class="tree-action" data-testid="minutes-job-menu" aria-haspopup="menu" aria-expanded={jobMenuFor === job.id} aria-label={t('more-actions-for', { name: job.source_name })} title={t('more-actions-tip')} onclick={() => (jobMenuFor = jobMenuFor === job.id ? null : job.id)}><Icon name="more" /></button>
            </span>
            {#if jobMenuFor === job.id}
              <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: job.source_name })} use:focusFirst onkeydown={escapeCloses(() => (jobMenuFor = null))}>
                {#if job.state.state === 'failed'}
                  <p class="menu-note" data-testid="minutes-job-reason">{job.state.message}</p>
                  <button type="button" role="menuitem" data-testid="minutes-retry" onclick={() => { jobMenuFor = null; void model.retry(job); }}><Icon name="play" />{t('minutes-retry')}</button>
                {/if}
                <button type="button" role="menuitem" data-testid="minutes-cancel" onclick={() => { jobMenuFor = null; void model.cancelJob(job); }}><Icon name="stop" />{t('minutes-cancel')}</button>
              </div>
            {/if}
          </div>
        </div>
      {/each}
      {#each model.items as m (m.id)}
        <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf" data-testid="minutes-item">
          <div class="tree-line">
            <button type="button" class="tree-row leaf tree-minutes" onclick={() => openMinutes(m)} oncontextmenu={(e) => { e.preventDefault(); menuFor = m.id; }}><span class="icon-tile"><Icon name="page" /></span><span class="tree-minutes-name">{m.title}</span></button>
            <MinutesMenu {backend} minutesId={m.id} title={m.title} bind:open={() => menuFor === m.id, (v) => (menuFor = v ? m.id : menuFor === m.id ? null : menuFor)} onerror={(e) => (actionError = e)} onremoved={() => void model.readList()} />
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if model.askingToRecord}
  <RecordDialog {model} />
{/if}

<style>
  .tree-minutes {
    padding-inline-start: calc(2px + 28px + 23px);
  }
  .tree-minutes-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .job {
    display: flex;
    align-items: center;
    gap: var(--space-04);
  }
  .stage {
    flex: none;
    max-inline-size: 55%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .mark {
    display: inline-flex;
    color: var(--color-danger, currentColor);
  }
  .menu-note {
    margin: 0;
    padding: var(--space-04) var(--space-sm);
    font-size: 13px;
    color: var(--color-text-muted);
  }
</style>
