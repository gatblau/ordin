<script lang="ts">
  // The code-checking question (SM-SF-3.1): a bar above the editor, not a dialog, an approval card or a
  // notification. A region with a level-3 heading; neither button is the default and neither takes focus.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-3
  import { t } from '../../i18n/fluent';
  import type { ServiceError } from '../../service-api/types.gen';
  import ErrorLine from '../ErrorLine.svelte';

  interface Props {
    project: string;
    busy: boolean;
    error: ServiceError | null;
    onanswer: (allowed: boolean) => void;
  }
  let { project, busy, error, onanswer }: Props = $props();

  const headingId = `code-checking-${Math.random().toString(36).slice(2, 8)}`;
</script>

<section class="code-checking-bar" aria-labelledby={headingId}>
  <h3 id={headingId}>{t('code-checking-ask-title', { project })}</h3>
  <p>{t('code-checking-ask-body')}</p>
  <div class="bar-buttons">
    <button type="button" disabled={busy} onclick={() => onanswer(true)}>{t('code-checking-yes')}</button>
    <button type="button" disabled={busy} onclick={() => onanswer(false)}>{t('code-checking-no')}</button>
  </div>
  <ErrorLine {error} />
</section>

<style>
  .code-checking-bar {
    border: 1px solid var(--color-line);
    background: var(--color-surface);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-12);
    margin-block-end: var(--space-sm);
  }
  h3 {
    margin: 0 0 var(--space-04);
    font-size: 1em;
  }
  p {
    margin: 0 0 var(--space-sm);
  }
  .bar-buttons {
    display: flex;
    gap: var(--space-sm);
  }
</style>
