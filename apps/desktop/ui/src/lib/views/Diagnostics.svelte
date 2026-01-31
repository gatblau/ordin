<script lang="ts">
  // Diagnostics: journal diagnostic lines, versions and technical names, for support; the only view that
  // shows codes. Also this window's own diagnostics ring (step 12) and, on this computer, **Save logs for
  // support…**. A line carries no **Send to IT** (SM-NS-3: every feature is self-service).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-3
  import { onMount } from 'svelte';

  import { answers } from '../backend/groups';
  import { formatDate } from '../format/intl';
  import { t } from '../i18n/fluent';
  import { getApp } from '../shell/context';
  import type { ScreenProps } from '../shell/screens';
  import { DiagnosticsModel } from './diagnostics-model.svelte';
  import ErrorLine from './ErrorLine.svelte';

  interface Props extends ScreenProps {
    model?: DiagnosticsModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const app = given === undefined ? getApp() : null;
  // svelte-ignore state_referenced_locally
  const model = given ?? new DiagnosticsModel(backend.handle);
  const ring = $derived(app?.diagnostics.entries ?? []);
  // Local backend only (scope `Local`), and only when the build answers both commands (SM-UP-1).
  // svelte-ignore state_referenced_locally
  const exportOffered = backend.kind === 'local' && answers(backend, 'runs.list') && answers(backend, 'diagnostics.support_export');

  onMount(() => void model.load());
</script>

<section class="diagnostics" aria-labelledby="diagnostics-heading">
  <h2 id="diagnostics-heading" tabindex="-1" data-region-focus>{t('screen-diagnostics')}</h2>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
  {#if exportOffered}
    <p><button type="button" onclick={() => void model.openPanel()}>{t('diagnostics-support-export')}</button></p>
    {#if model.panelOpen}
      <section class="support-export" aria-labelledby="diagnostics-support-pick">
        <p id="diagnostics-support-pick">{t('diagnostics-support-pick')}</p>
        <ul class="runs">
          {#each model.runs as run (run.id)}
            <li>
              <label>
                <input type="checkbox" checked={model.ticked.has(run.id)} onchange={() => model.toggle(run.id)} />
                <code>{run.id}</code>
                <span class="state">{run.state}</span>
                <time datetime={run.queued_at}>{formatDate(run.queued_at, { dateStyle: 'short', timeStyle: 'medium' })}</time>
              </label>
            </li>
          {/each}
        </ul>
        {#if model.runsNext !== null}<button type="button" onclick={() => void model.moreRuns()}>{t('settings-show-more')}</button>{/if}
        {#if model.limitShown}<p role="status">{t('diagnostics-support-limit')}</p>{/if}
        <p>{t('diagnostics-support-note')}</p>
        <button type="button" data-default="true" disabled={model.ticked.size === 0} onclick={() => void model.exportSupport()}>{t('diagnostics-support-save')}</button>
        {#if model.savedPath !== null}<p role="status">{t('diagnostics-support-saved', { path: model.savedPath })}</p>{/if}
        <ErrorLine error={model.exportError} />
      </section>
    {/if}
  {/if}
  <h3>{t('diagnostics-journal-title')}</h3>
  <ul class="journal">
    {#each model.lines as line, i (`${line.at}-${line.code}-${i}`)}
      <li>
        <time datetime={line.at}>{formatDate(line.at, { dateStyle: 'short', timeStyle: 'medium' })}</time>
        <span class="level">{line.level}</span>
        <code>{line.code}</code>
        <span class="summary">{line.summary}</span>
        <code class="reference">{line.reference}</code>
      </li>
    {/each}
  </ul>
  {#if model.next !== null}<button type="button" onclick={() => void model.more()}>{t('settings-show-more')}</button>{/if}
  <h3>{t('diagnostics-window-title')}</h3>
  <ul class="window-ring">
    {#each ring as entry, i (i)}
      <li><time datetime={entry.at}>{entry.at}</time> <code>{entry.code}</code> {#if entry.detail}<span>{entry.detail}</span>{/if}</li>
    {/each}
  </ul>
</section>
