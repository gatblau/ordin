<script lang="ts">
  // First start (ScreensModule.md, amendment January 2026; scenes `setup`, `setupReady`, `noseat`): drawn
  // in place of the window's three regions while the guided setup runs. The brand tile and heading, the
  // count, one row per step with its run-state dot, the two small lines, then the failed results with
  // their actions and the sign-in button.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-FS-1
  // spec: ordin-desktop/components/ScreensModule#SM-FS-2
  // spec: ordin-desktop/components/ScreensModule#SM-FS-3
  // spec: ordin-desktop/components/ScreensModule#SM-FS-4
  // spec: ordin-desktop/components/ScreensModule#SM-FS-5
  // spec: ordin-desktop/components/ScreensModule#SM-FS-7
  // spec: ordin-desktop/components/ScreensModule#SM-FS-9
  import { i18n, t } from '../../i18n/fluent';
  import type { CheckResult, ServiceError } from '../../service-api/types.gen';
  import ErrorLine from '../ErrorLine.svelte';
  import { DOT_GLYPH, DOT_OF_STATE, resultButton, resultMessageId, stateMessageId, type FirstStartModel } from './first-start-model.svelte';

  let { model }: { model: FirstStartModel } = $props();

  // The guided run is sent once, when first start is drawn (SM-FS-2).
  $effect(() => {
    model.begin();
  });

  const harness = $derived(model.harnessName);

  /** A failed result as step 11's error: its sentence, and its action as the one button (SM-FS-4). */
  function asError(result: CheckResult): ServiceError {
    const button = resultButton(result.action);
    const code = result.code ?? '';
    const message = t(result.message_id ?? resultMessageId(code), result.message_args);
    if (button?.kind === 'try_again') return { code, message, action: 'try_again' };
    return { code, message };
  }
  function labelOf(result: CheckResult): string | undefined {
    const button = resultButton(result.action);
    if (button?.kind === 'try_again') return t('setup-try-again');
    return undefined;
  }
</script>

<div class="first-start" lang={i18n.locale}>
  <section class="card" aria-labelledby="first-start-heading">
    <div class="brand">
      <span class="logo" aria-hidden="true"></span>
      <span class="word">{i18n.product}</span>
    </div>
    <h1 id="first-start-heading">{t('setup-getting-ready')}</h1>

    {#if model.statusError !== null}
      <ErrorLine error={model.statusError} onretry={() => void model.checkStatus()} />
    {:else}
      <p class="muted">{t('setup-progress-count', { done: model.progress.done, total: model.progress.total })}</p>
      <ul class="steps">
        {#each model.progress.steps as step (step.check)}
          {@const tone = DOT_OF_STATE[step.state]}
          <li data-state={step.state}>
            <span class="dot" data-tone={tone} aria-hidden="true">{DOT_GLYPH[tone]}</span>
            <span class="visually-hidden">{t(stateMessageId(step.state))} </span>{t(step.message_id, { harness })}
            {#if step.state === 'failed' && step.problem && model.failedResults.length === 0}
              <span class="problem">{t(step.problem.message_id, { harness, ...step.problem.message_args })}</span>
            {/if}
          </li>
        {/each}
      </ul>
      <p class="small">{t('setup-sends-to', { harness })}</p>

      {#each model.failedResults as result, index (`${result.check}-${index}`)}
        <ErrorLine error={asError(result)} actionLabel={labelOf(result)} onretry={() => void model.runGuided()} />
      {/each}
      <ErrorLine error={model.runError} onretry={() => void model.runGuided()} />

      {#if model.showInstall}
        <div class="install">
          <button type="button" class="primary" onclick={() => void model.installClaudeCode()}>{t('setup-install-claude-code')}</button>
          <button type="button" onclick={() => void model.runGuided()}>{t('setup-check-again')}</button>
        </div>
        <ErrorLine error={model.pages.error} onretry={() => void model.pages.retry()} />
      {/if}

      {#if model.showSignIn}
        <div class="sign-in">
          <button type="button" class="primary" disabled={model.signingIn} onclick={() => void model.signIn()}>
            {model.signingIn ? t('setup-signing-in') : t('setup-sign-in', { harness })}
          </button>
          <p class="muted small">{t('setup-sign-in-note', { harness })}</p>
          <ErrorLine error={model.signInError} onretry={() => void model.signIn()} />
        </div>
      {/if}
    {/if}
  </section>
</div>

<style>
  /* First start (prototype `#setup`, `.setup .card`; cross-cutting/VisualDesign.md V6 *Brand*, *Run-state dot*). */
  .first-start {
    min-block-size: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 100px var(--space-md) var(--space-md);
    background: var(--color-background);
  }
  .card {
    inline-size: 100%;
    max-inline-size: 440px;
    display: flex;
    flex-direction: column;
    gap: var(--space-14);
    padding: 28px;
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-card);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-10);
    font-weight: 700;
    font-size: 18px;
    color: var(--color-text);
  }
  .logo {
    inline-size: 28px;
    block-size: 28px;
    flex: none;
    border-radius: var(--radius-md);
    background: var(--color-primary);
  }
  h1 {
    font-size: 22px;
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
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm);
    font-size: 14px;
  }
  /* Why a step failed, under it, as soon as it fails (SM-FS-7). */
  .problem {
    flex-basis: 100%;
    padding-inline-start: calc(14px + var(--space-sm));
    color: var(--color-text-muted);
    font-size: 13px;
  }
  .dot {
    inline-size: 14px;
    block-size: 14px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    background: var(--color-line-strong);
    color: var(--color-on-status);
    font-size: 10px;
    line-height: 1;
  }
  .dot[data-tone='now'] {
    background: var(--color-accent);
  }
  .dot[data-tone='done'] {
    background: var(--color-done);
  }
  .dot[data-tone='waiting'] {
    background: var(--color-waiting);
  }
  .dot[data-tone='failed'] {
    background: var(--color-danger);
  }
  /* Install Claude Code, then Check again (SM-FS-9). */
  .install {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
  .sign-in {
    display: flex;
    flex-direction: column;
    gap: var(--space-14);
  }
  /* The prototype's sign-in button spans the card, in body-size text. */
  .sign-in > button {
    font-size: 16px;
  }
</style>
