<script lang="ts">
  // The part draft (SM-NS-7.3; instance = the part draft id). The part's own form is not drawn in R1 (G-CU-2).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-7
  // spec: ordin-desktop/components/ScreensModule#SM-CHK-3
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import { makeKey, type ScreenProps } from '../../shell/screens';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import DraftChecks from './DraftChecks.svelte';
  import { PartDraftController } from './draft.svelte';
  import { closeScreen } from './support';
  import { appOrNull } from '../fixed/util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const key = makeKey('part-draft', instance);
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in a callback.
  const app = appOrNull();
  const c = new PartDraftController(backend.handle, instance ?? '', () => closeScreen(app, backend, workspaceId, key));
  let confirmingDiscard = $state(false);

  onMount(() => {
    void c.load();
  });
</script>

<section class="part-draft" aria-labelledby="part-draft-heading">
  <h2 id="part-draft-heading" tabindex="-1" data-region-focus>{t('draft-part-title', { name: c.partName })}</h2>
  <ErrorLine error={c.error} onretry={() => void c.load()} />
  {#if c.view !== null}
    {@const view = c.view}
    <DraftChecks state={view.draft.state} reviewerWords={view.reviewer_words} checks={view.checks} tidied={view.tidied} onfix={(f, fix) => void c.applyFix(f, fix)}>
      {#if view.used_by.length > 0}
        <p class="small">{t('draft-part-used-by', { count: view.used_by.length, names: view.used_by.join(', ') })}</p>
      {/if}
    </DraftChecks>
    {#if view.writing_guide}
      <aside class="guide-box" aria-labelledby="part-draft-guide">
        <h3 id="part-draft-guide">{t('draft-part-how-to-write')}</h3>
        <p class="guide">{view.writing_guide}</p>
      </aside>
    {/if}
    <fieldset class="question">
      <legend>{t('draft-part-changes-what')}</legend>
      <label>
        <input type="radio" name="changes-what" checked={view.draft.changes_what_part_does === false} onchange={() => void c.answerChangesWhat(false)} />
        {t('draft-part-correction')}
      </label>
      <label>
        <input type="radio" name="changes-what" checked={view.draft.changes_what_part_does === true} onchange={() => void c.answerChangesWhat(true)} />
        {t('draft-part-yes')}
      </label>
    </fieldset>
    <div class="foot">
      {#if view.can_publish}
        <button type="button" data-default="true" onclick={() => void c.publish()}>{t('draft-publish')}</button>
      {/if}
      <button type="button" onclick={() => (confirmingDiscard = true)}>{t('draft-discard')}</button>
    </div>
    {#if confirmingDiscard}
      <ConfirmDialog
        title={t('draft-discard-title', { name: c.partName })}
        body={t('draft-discard-body')}
        verb={t('draft-discard')}
        onconfirm={() => {
          confirmingDiscard = false;
          void c.discard();
        }}
        oncancel={() => (confirmingDiscard = false)}
      />
    {/if}
  {/if}
</section>

<style>
  /* The prototype's part draft (scene `partDraft`, `.ptmain`). */
  .part-draft {
    gap: var(--space-14);
  }
  .part-draft > h2 {
    font-size: 19px;
  }
  .small {
    font-size: 13px;
  }
  .guide-box {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    background: var(--color-surface);
    padding: var(--space-10) var(--space-12);
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    max-inline-size: 280px;
  }
  .guide-box > h3 {
    font-size: 14px;
  }
  .guide {
    white-space: pre-wrap;
    font-size: 13px;
  }
  .question {
    border: 0;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    font-size: 13px;
  }
  .question > legend {
    font-weight: 700;
    padding: 0;
    margin-block-end: var(--space-06);
  }
  .question label {
    display: flex;
    gap: var(--space-06);
    align-items: center;
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-06);
  }
</style>
