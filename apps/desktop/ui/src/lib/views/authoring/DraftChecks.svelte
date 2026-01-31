<script lang="ts">
  // The band by state, the reviewer's words, the Next bar's place and **Checked by ordin** that the agent draft
  // and the part draft share (SM-NS-6.1, SM-NS-7.3). A finding's text is a message of the catalogue filled from
  // its arguments; the reviewer's words are text.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-6
  import type { Snippet } from 'svelte';

  import { t } from '../../i18n/fluent';
  import type { Finding, FixChoice, ReviewDraftState, WordingChange } from '../../service-api/types.gen';
  import { bandMessage, findingText, offeredFixes, orderedChecks } from './draft.svelte';

  interface Props {
    state: ReviewDraftState;
    reviewerWords: readonly string[];
    checks: readonly Finding[];
    tidied?: readonly WordingChange[];
    onfix?: (finding: Finding, fix: FixChoice) => void;
    /** Drawn between the reviewer's words and **Checked by ordin**: the Next bar and the purpose. */
    children?: Snippet;
  }
  let { state, reviewerWords, checks, tidied = [], onfix, children }: Props = $props();
  const band = $derived(bandMessage(state));
</script>

{#if band !== null}<p class="band">{t(band)}</p>{/if}
{#each reviewerWords as words, i (i)}<blockquote>{words}</blockquote>{/each}
{@render children?.()}
<section class="checks" aria-labelledby="draft-checks-heading">
  <h3 id="draft-checks-heading" tabindex="-1">{t('check-checked-by-ordin')}</h3>
  <p class="secondary">{t('draft-checked-line')}</p>
  <ul class="findings">
    {#each orderedChecks(checks) as finding, i (finding.code + i)}
      <li data-severity={finding.severity}>
        <span class="glyph" aria-hidden="true">{finding.severity === 'failure' ? '✗' : '⚠'}</span>
        {findingText(finding)}
        {#each offeredFixes(finding) as fix (fix.choice)}
          <button type="button" class="small" onclick={() => onfix?.(finding, fix.choice)}>{t(fix.label)}</button>
        {/each}
      </li>
    {/each}
  </ul>
  {#if tidied.length > 0}
    <details>
      <summary>{t('draft-tidied', { count: tidied.length })}</summary>
      <ul>
        {#each tidied as change, i (i)}<li>{change.before} → {change.after}</li>{/each}
      </ul>
    </details>
  {/if}
</section>

<style>
  /* The prototype's band (`.engnote`) and *Checked by ordin* (`.cgrp`, `.agck`): scenes `partDraft`, `agDraft`. */
  .band {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-10);
    background: var(--color-background);
    font-weight: 700;
  }
  .checks {
    background: none;
    border: 0;
    border-radius: 0;
    padding: 0;
    gap: var(--space-sm);
  }
  .checks > h3 {
    font-size: 14px;
  }
  .secondary {
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .findings {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .findings > li {
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
  blockquote {
    margin-inline: 0;
    padding-inline-start: var(--space-12);
    border-inline-start: 3px solid var(--color-line);
  }
</style>
