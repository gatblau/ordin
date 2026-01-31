<script lang="ts">
  // The two questions a minutes job waits on, as lines of *Needs you* (ScreensModule.md SM-MIN-21): the
  // speech model download (SM-MIN-3) and the one-time notice (SM-MIN-4), each with **Open**, which opens its
  // dialog unchanged. Lines come from the workspaces whose Minutes row is drawn (`watches`).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-21
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-3
  // spec: ordin-desktop/components/ScreensModule#SM-MIN-4
  import { formatNumber } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { MinutesJob } from '../../service-api/minutes.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import type { MinutesScreenModel } from './minutes-screen.svelte';
  import { minutesWatches } from './watches.svelte';

  interface Props {
    backend: BackendState;
  }
  // The slot is given the local backend; the lines come from every drawn Minutes row instead.
  const _props: Props = $props();

  type Question = { kind: 'model' | 'notice'; model: MinutesScreenModel; job: MinutesJob };
  const questions = $derived(
    minutesWatches().flatMap((w): Question[] => {
      const out: Question[] = [];
      if (w.model.consentJob !== null) out.push({ kind: 'model', model: w.model, job: w.model.consentJob });
      if (w.model.noticeJob !== null) out.push({ kind: 'notice', model: w.model, job: w.model.noticeJob });
      return out;
    }),
  );
  let open = $state.raw<Question | null>(null);
  const status = $derived(open?.model.status ?? null);

  /** Answers the open question: `yes` is Download or Write the minutes; otherwise Not now cancels the job. */
  function answer(yes: boolean): void {
    const q = open;
    if (q === null) return;
    open = null;
    if (!yes) void q.model.cancelJob(q.job);
    else if (q.kind === 'model') void q.model.giveModelConsent(q.job);
    else void q.model.acceptNotice(q.job);
  }
</script>

{#if questions.length > 0}
  <ul class="minutes-needs" data-testid="minutes-needs-you">
    {#each questions as q (q.kind + q.job.id)}
      <li class="needs-item" data-testid={`minutes-needs-${q.kind}`}>
        <span class="icon-tile"><Icon name="info" /></span>
        <span class="needs-line">{t(q.kind === 'model' ? 'minutes-needs-model' : 'minutes-needs-notice', { file: q.job.source_name })}</span>
        <button type="button" class="small" onclick={() => (open = q)}>{t('card-open')}</button>
      </li>
    {/each}
  </ul>
{/if}

{#if open?.kind === 'model' && status !== null}
  <ConfirmDialog
    title={t('minutes-model-title')}
    body={t('minutes-model-body', { size: formatNumber(status.model.size_mb), folder: status.model.folder })}
    verb={t('minutes-model-download')}
    cancelLabel={t('minutes-model-not-now')}
    destructive={false}
    onconfirm={() => answer(true)}
    oncancel={() => answer(false)}
  />
{:else if open?.kind === 'notice' && status !== null}
  <ConfirmDialog
    title={t('minutes-notice-title')}
    body={t('minutes-notice-body', { service: status.writer_name })}
    verb={t('minutes-notice-accept')}
    cancelLabel={t('minutes-model-not-now')}
    destructive={false}
    onconfirm={() => answer(true)}
    oncancel={() => answer(false)}
  />
{/if}

<style>
  .minutes-needs {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .needs-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
  }
  .needs-line {
    flex: 1;
  }
</style>
