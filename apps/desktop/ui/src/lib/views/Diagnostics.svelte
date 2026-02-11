<script lang="ts">
  // Diagnostics: journal diagnostic lines, versions and technical names, for support; the only view that
  // shows codes. Also this window's own diagnostics ring (step 12) and, on this computer, **Save logs for
  // support…**. A line carries no **Send to IT** (SM-NS-3: every feature is self-service).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-3
  // spec: ordin-desktop/components/ScreensModule#SM-NS-3a
  import { onMount } from 'svelte';
  import { LOG_WINDOWS, WINDOW_WORDS } from './diagnostics-model.svelte';

  import { answers } from '../backend/groups';
  import { formatDate } from '../format/intl';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import { getApp } from '../shell/context';
  import type { ScreenProps } from '../shell/screens';
  import { DiagnosticsModel } from './diagnostics-model.svelte';
  import ConfirmDialog from './ConfirmDialog.svelte';
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
  // Clear logs… beside it, on the same terms (SM-NS-3a).
  // svelte-ignore state_referenced_locally
  const clearOffered = backend.kind === 'local' && answers(backend, 'diagnostics.clear_logs');

  onMount(() => void model.load());

  /** The journal rows whose details are open, by row key. */
  let opened = $state.raw<ReadonlySet<string>>(new Set());
  function toggleLine(key: string): void {
    const next = new Set(opened);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    opened = next;
  }
</script>

<section class="diagnostics" aria-labelledby="diagnostics-heading">
  <h2 id="diagnostics-heading" tabindex="-1" data-region-focus>{t('screen-diagnostics')}</h2>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
  {#if exportOffered || clearOffered}
    <!-- Amended January 2026 (user decision; Approved by the user January 2026): the screen's buttons are small
         icon buttons in the editor toolbar's style, each named by its words and with them as a tooltip. -->
    <p class="log-actions">
      <!-- SM-NS-3b: how far back the support file goes; 5 minutes by default (DSK-LOG-6). -->
      {#if exportOffered}<select class="log-window" aria-label={t('diagnostics-support-window')} title={t('diagnostics-support-window')} bind:value={model.lastMinutes}>
          {#each LOG_WINDOWS as minutes (minutes)}<option value={minutes}>{t(WINDOW_WORDS[minutes])}</option>{/each}
        </select>{/if}
      {#if exportOffered}<button type="button" class="icon-btn" disabled={model.saving} aria-label={t('diagnostics-support-export')} title={`${t('diagnostics-support-export')}\n${t('diagnostics-support-note')}`} onclick={() => void model.saveForSupport()}><Icon name="save" size="small" /></button>{/if}
      {#if clearOffered}<button type="button" class="icon-btn" aria-label={t('diagnostics-clear')} title={t('diagnostics-clear')} onclick={() => model.askClear()}><Icon name="bin" size="small" /></button>{/if}
    </p>
    {#if model.cleared}<p role="status">{t('diagnostics-clear-done')}</p>{/if}
  {/if}
  {#if exportOffered}
    <!-- Amended November 2026 (user decision): one action saves the newest runs, up to 20, and the last 3 days
         of logs; there is nothing to tick first. -->
    {#if model.nothingToSave}<p role="status">{t('diagnostics-support-nothing')}</p>{/if}
    {#if model.savedPath !== null}<p role="status">{t('diagnostics-support-saved', { path: model.savedPath })}</p>{/if}
    <ErrorLine error={model.exportError} />
  {/if}
  <h3>{t('diagnostics-journal-title')}</h3>
  <!-- Amended January 2026 (user decision; Approved by the user January 2026): the journal's events are one
       compact table — reference, time, level and summary — and each row's code is shown on request. The
       reference is the start of the event's own id, for quoting to support; it is muted on info rows. -->
  {#if model.lines.length === 0}
    <p class="secondary">{t('diagnostics-journal-none')}</p>
  {:else}
    <table class="journal">
      <thead>
        <tr>
          <th scope="col" title={t('diagnostics-col-reference-hint')}>{t('diagnostics-col-reference')}</th>
          <th scope="col">{t('diagnostics-col-time')}</th>
          <th scope="col">{t('diagnostics-col-level')}</th>
          <th scope="col">{t('diagnostics-col-summary')}</th>
          <th scope="col"><span class="visually-hidden">{t('diagnostics-col-details')}</span></th>
        </tr>
      </thead>
      <tbody>
        {#each model.lines as line, i (`${line.at}-${line.code}-${i}`)}
          {@const key = `${line.at}-${line.code}-${i}`}
          {@const open = opened.has(key)}
          <tr>
            <td class="reference" class:quiet={line.level === 'info'} title={t('diagnostics-col-reference-hint')}><code>{line.reference}</code></td>
            <td class="when"><time datetime={line.at}>{formatDate(line.at, { dateStyle: 'short', timeStyle: 'medium' })}</time></td>
            <td><span class="level {line.level}">{line.level}</span></td>
            <td class="summary">{line.summary}</td>
            <td class="tools">
              <button
                type="button"
                class="icon-btn disclose"
                aria-expanded={open}
                aria-controls={open ? `diagnostics-line-${i}` : undefined}
                aria-label={open ? t('diagnostics-line-less') : t('diagnostics-line-more')}
                title={open ? t('diagnostics-line-less') : t('diagnostics-line-more')}
                onclick={() => toggleLine(key)}
              ><Icon name="chev" size="small" /></button>
            </td>
          </tr>
          {#if open}
            <tr class="line-details" id={`diagnostics-line-${i}`}>
              <td colspan="5"><code>{line.code}</code></td>
            </tr>
          {/if}
        {/each}
      </tbody>
    </table>
  {/if}
  {#if model.next !== null}<button type="button" class="icon-btn" aria-label={t('settings-show-more')} title={t('settings-show-more')} onclick={() => void model.more()}><Icon name="unfold" size="small" /></button>{/if}
  <h3>{t('diagnostics-window-title')}</h3>
  <ul class="window-ring">
    {#each ring.filter((entry) => model.shownAfterClear(entry.at)) as entry, i (i)}
      <li><time datetime={entry.at}>{entry.at}</time> <code>{entry.code}</code> {#if entry.detail}<span>{entry.detail}</span>{/if}</li>
    {/each}
  </ul>
</section>

{#if model.clearAsked}
  <ConfirmDialog
    title={t('diagnostics-clear-title')}
    body={t('diagnostics-clear-body')}
    verb={t('diagnostics-clear-verb')}
    onconfirm={() => void model.clearLogs()}
    oncancel={() => model.cancelClear()}
  >
    <ErrorLine error={model.clearError} onretry={() => void model.clearLogs()} />
  </ConfirmDialog>
{/if}

<style>
  /* Save logs for support… and Clear logs… side by side (SM-NS-3a). */
  .log-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-04);
    margin: 0;
  }
  /* The editor toolbar's icon button (MarkdownEditor.svelte `.md-tool`). */
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    min-height: 0;
    padding: 0;
  }
  .icon-btn[aria-expanded='true'] {
    background: var(--color-accent-soft);
    color: var(--color-accent-ink);
  }
  .disclose[aria-expanded='true'] :global(.icon) {
    transform: rotate(90deg);
  }
  /* A compact table: one line per event, the summary taking the room that is left. */
  .journal {
    inline-size: 100%;
    border-collapse: collapse;
    font-size: 13px;
    table-layout: auto;
  }
  .journal th {
    text-align: start;
    font-size: 12px;
    font-weight: 600;
    color: var(--color-text-muted);
    padding: var(--space-04) var(--space-06);
    border-block-end: 1px solid var(--color-line);
  }
  .journal td {
    padding: var(--space-04) var(--space-06);
    border-block-end: 1px solid var(--color-line);
    vertical-align: middle;
  }
  .journal .reference {
    white-space: nowrap;
    font-size: 12px;
    color: var(--color-text-secondary);
  }
  /* An info row's reference is rarely needed, so it is drawn quieter. */
  .journal .reference.quiet {
    color: var(--color-text-muted);
    opacity: 0.7;
  }
  .journal .when {
    white-space: nowrap;
    color: var(--color-text-secondary);
  }
  .journal .summary {
    inline-size: 100%;
    overflow-wrap: anywhere;
  }
  .journal .tools {
    text-align: end;
  }
  .level {
    font-size: 12px;
    font-weight: 600;
  }
  .level.error {
    color: var(--color-danger);
  }
  .level.warning {
    color: var(--color-waiting-text);
  }
  .line-details td {
    font-size: 12px;
    color: var(--color-text-secondary);
    background: var(--color-background);
  }
</style>
