<script lang="ts">
  // Check it, in place on What [agent] is made of and on every part page (SA-CLOSE-01). "Every check
  // passed." is drawn only for a report with no finding; an error reply shows its sentence and no
  // result. A finding reads as its own sentence (SM-CHK-1); **Ask Ordin to fix it** sends Ordin the problem in
  // the open workspace (SM-CHK-2); a fix with no action on this page is not drawn (SM-CHK-3).
  //
  // spec: ordin-desktop/components/ScreensModule#SA-CLOSE-01
  // spec: ordin-desktop/components/ScreensModule#SM-CHK-1
  // spec: ordin-desktop/components/ScreensModule#SM-CHK-2
  // spec: ordin-desktop/components/ScreensModule#SM-CHK-3
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import type { ServiceError } from '../../backend/backend';
  import { askOrdin, canAsk } from '../../blocks/ask';
  import { formatTime } from '../../format/intl';
  import { stripIsolates, t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { Finding, Fix } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { openScreen } from '../../shell/open-item';
  import { CheckItController, orderedFindings, partPageKeyFor, type CheckSubjectRef } from './check-it.svelte';
  import { appOrNull } from './util';

  interface Props {
    backend: BackendState;
    subject: CheckSubjectRef;
    /** A fix the person chose; the draft that applies it belongs to the authoring screens. */
    onfix?: (finding: Finding, fix: Fix) => void;
    /** The workspace the page is open in; a finding's part page opens there. */
    workspaceId?: string | null;
    /** The part the page is already about, so its own findings do not link back to it. */
    currentPart?: string | null;
    /** The agent's or part's name shown on the page, for **Ask Ordin to fix it** (SM-CHK-2). */
    subjectName?: string;
  }
  let { backend, subject, onfix, workspaceId = null, currentPart = null, subjectName = '' }: Props = $props();

  const c = new CheckItController(backend.handle, subject);
  // The app context is read while the panel is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();

  onMount(() => {
    void c.reopen();
  });

  function openPart(finding: Finding): void {
    const key = partPageKeyFor(subject, finding, currentPart);
    if (key !== null && app !== null) openScreen(app, backend.id, workspaceId ?? app.shell.openWorkspace, key);
  }

  /** SM-CHK-1: the finding's sentence, from its own `args` (the part's name), never its identifier. */
  const sentence = (finding: Finding): string => t(finding.message_id, { ...finding.args });

  /** The workspace an ask goes to: the page's, else the open one. */
  const askWorkspace = $derived(workspaceId ?? app?.shell.openWorkspace ?? null);
  let asking = $state<number | null>(null);
  let asked = $state<number | null>(null);
  let askError = $state.raw<ServiceError | null>(null);

  // SM-CHK-2: Ordin is sent the problem as the person's message, in the workspace the page is open in.
  async function ask(finding: Finding, index: number): Promise<void> {
    if (askWorkspace === null || asking !== null) return;
    asking = index;
    askError = null;
    const subjectLabel = subjectName !== '' ? subjectName : (finding.args['part'] ?? '');
    // Text sent to Ordin carries no direction marks (they are for drawing, not for the model).
    const out = await askOrdin(backend, askWorkspace, stripIsolates(t('check-ask-ordin-text', { subject: subjectLabel, finding: sentence(finding) })));
    asking = null;
    if (out.kind === 'error') {
      askError = out.error;
      return;
    }
    asked = index;
  }

  /** SM-CHK-3: a fix is drawn only when this page can act on it. */
  const fixLabel = (fix: Fix): string | null => {
    if (fix.fix === 'ask_ordin') return askWorkspace !== null && canAsk(backend) ? t('check-ask-ordin-to-fix') : null;
    if (onfix === undefined) return null;
    return fix.fix === 'fix_it' ? t('check-fix-it') : fix.fix === 'change_it_yourself' ? t('check-change-it-yourself') : null;
  };
</script>

{#if answers(backend, 'checker.report')}
<section class="check-it" aria-label={t('check-it')}>
  <button type="button" class="small" disabled={c.running} onclick={() => void c.check()}>{t('check-it')}</button>
  {#if c.running}<p role="status" class="quiet">{t('check-running')}</p>{/if}
  <ErrorLine error={c.error} onretry={() => void c.check()} />
  {#if c.report !== null}
    <div class="check-result">
      <p class="checked">{t('check-checked-by-ordin')} · {t('check-checked-at', { time: formatTime(c.report.checked_at) })}</p>
      {#if c.report.findings.length === 0}
        <p>{t('check-all-passed')}</p>
      {:else}
        <ul>
          {#each orderedFindings(c.report.findings) as finding, i (finding.code + i)}
            <li data-severity={finding.severity}>
              <span class="glyph" aria-hidden="true">{finding.severity === 'failure' ? '✗' : '⚠'}</span>
              {sentence(finding)}
              {#if partPageKeyFor(subject, finding, currentPart) !== null}<button type="button" class="small" onclick={() => openPart(finding)}><Icon name="eye" />{t('repo-read-it')}</button>{/if}
              {#each finding.fixes as fix, j (j)}
                {@const label = fixLabel(fix)}
                {#if label !== null && fix.fix === 'ask_ordin'}<button type="button" class="small" disabled={asking !== null} onclick={() => void ask(finding, i)}>{label}</button>
                {:else if label !== null}<button type="button" class="small" onclick={() => onfix?.(finding, fix)}>{label}</button>{/if}
              {/each}
              {#if asked === i}<span role="status" class="quiet">{t('check-asked-ordin')}</span>{/if}
            </li>
          {/each}
        </ul>
        <ErrorLine error={askError} />
      {/if}
    </div>
  {/if}
</section>
{/if}

<style>
  /* The prototype's *Check it* (scene `checkAgent`, `.cgrp`, `.agck`): no card of its own, a small
     button, one line per finding with its glyph in a pill and its fixes as small buttons. */
  .check-it {
    background: none;
    border: 0;
    border-radius: 0;
    padding: 0;
    gap: var(--space-sm);
    align-items: flex-start;
  }
  .check-result {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .check-result p {
    margin: 0;
  }
  .checked {
    font-size: 13px;
    color: var(--color-text-muted);
  }
  .check-result ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .check-result li {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    align-items: center;
  }
  .glyph {
    border-radius: var(--radius-pill);
    padding: 1px var(--space-sm);
    font-size: 12px;
    font-weight: 600;
    background: var(--color-danger-soft);
    color: var(--color-danger);
  }
  [data-severity='warning'] > .glyph {
    background: var(--color-waiting-soft);
    color: var(--color-waiting-text);
  }
  .quiet {
    color: var(--color-text-muted);
  }
</style>
