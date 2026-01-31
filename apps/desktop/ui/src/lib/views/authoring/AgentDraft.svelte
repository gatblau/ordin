<script lang="ts">
  // The agent draft (SM-NS-6; instance = the agent draft id).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-6
  import { onMount } from 'svelte';

  import { t } from '../../i18n/fluent';
  import { makeKey, type ScreenProps } from '../../shell/screens';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import DraftChecks from './DraftChecks.svelte';
  import { AgentDraftController, failureCount } from './draft.svelte';
  import { closeScreen } from './support';
  import { appOrNull } from '../fixed/util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  const key = makeKey('agent-draft', instance);
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in a callback.
  const app = appOrNull();
  const c = new AgentDraftController(backend.handle, instance ?? '', workspaceId, () => closeScreen(app, backend, workspaceId, key));
  let confirmingDiscard = $state(false);

  onMount(() => {
    void c.load();
  });

  function focusChecks(): void {
    document.getElementById('draft-checks-heading')?.focus();
  }
</script>

<section class="agent-draft" aria-labelledby="agent-draft-heading">
  <h2 id="agent-draft-heading" tabindex="-1" data-region-focus>{t('draft-agent-title', { name: c.view?.draft.name ?? '' })}</h2>
  <ErrorLine error={c.error} onretry={() => void c.load()} />
  {#if c.view !== null}
    {@const view = c.view}
    <DraftChecks state={view.draft.state} reviewerWords={view.reviewer_words} checks={view.checks} tidied={view.tidied} onfix={(f, fix) => void c.applyFix(f, fix)}>
      <div class="next" role="group" aria-label={t('draft-next')}>
        {#if c.next === 'fix'}
          <button type="button" data-default="true" onclick={focusChecks}>{t('draft-next-fix', { count: failureCount(view.checks) })}</button>
        {:else if c.next === 'rehearse'}
          <button type="button" data-default="true" disabled={c.busy || workspaceId === null} onclick={() => void c.rehearse()}>{t('draft-next-rehearse')}</button>
        {:else if c.next === 'try'}
          <button type="button" data-default="true" disabled={c.busy || workspaceId === null} onclick={() => void c.tryIt()}>{t('draft-try')}</button>
        {:else if c.next === 'publish'}
          <button type="button" data-default="true" onclick={() => void c.publish()}>{t('draft-publish')}</button>
        {/if}
        {#if view.draft.state === 'changes_asked_for'}
          <button type="button" onclick={() => void c.sendChange()}>{t('draft-send-change')}</button>
        {/if}
      </div>
      {#if view.draft.purpose !== ''}<p>{view.draft.purpose}</p>{/if}
    </DraftChecks>

    {#if c.rehearsal !== null}
      {@const r = c.rehearsal}
      <section aria-label={t('draft-next-rehearse')}>
        <p>
          {t('draft-rehearsal-result', {
            passed: r.situations.filter((s) => s.expectations.every((e) => e.outcome === 'met')).length,
            total: r.situations.length,
          })}
        </p>
        {#each r.situations as situation, i (i)}
          <h4>{situation.description}</h4>
          <ul>
            {#each situation.expectations as expectation, j (j)}
              <li>{expectation.text} · {expectation.outcome === 'met' ? t('draft-expectation-met') : t('draft-expectation-not-met')}</li>
            {/each}
          </ul>
          {#if situation.reviewer_opinion !== ''}
            <p class="secondary">{t('draft-reviewer-opinion')}</p>
            <blockquote>{situation.reviewer_opinion}</blockquote>
          {/if}
        {/each}
      </section>
    {/if}

    <h3>{t('draft-what-it-is')}</h3>
    {#each [['draft-will', view.draft.will], ['draft-wont', view.draft.wont], ['draft-needs', view.draft.needs], ['draft-tasks', view.draft.tasks]] as [id, items] (id)}
      {#if (items as string[]).length > 0}
        <h4>{t(id as string)}</h4>
        <ul>
          {#each items as string[] as item, i (i)}<li>{item}</li>{/each}
        </ul>
      {/if}
    {/each}

    {#if view.publish_to.length > 0}
      <fieldset>
        <legend>{t('draft-publish-to')}</legend>
        {#each view.publish_to as id (id)}
          <label>
            <input type="radio" name="publish-to" value={id} checked={id === view.draft.agent_repository_id} onchange={() => void c.setPublishTo(id)} />
            {c.repositoryName(id)}
          </label>
        {/each}
      </fieldset>
    {/if}

    <div class="foot">
      <button type="button" onclick={() => (confirmingDiscard = true)}>{t('draft-discard')}</button>
    </div>
    {#if confirmingDiscard}
      <ConfirmDialog
        title={t('draft-discard-title', { name: view.draft.name })}
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
  .secondary {
    color: var(--color-text-muted);
  }
  /* The prototype's Next bar (`.agnext`; scene `makeDraft`): an accent-bordered strip, a small primary button. */
  .next {
    display: flex;
    gap: var(--space-sm);
    align-items: center;
    padding: var(--space-sm) var(--space-10);
    border: 1px solid var(--color-accent);
    border-radius: var(--radius-md);
    background: var(--color-surface);
  }
  .next:empty {
    display: none;
  }
  .next > button {
    padding: 3px var(--space-sm);
    min-height: 28px;
    font-size: 13px;
  }
  /* *What it is*: a 15 px heading, small bold sub-headings and lists without the default margins. */
  .agent-draft > h3 {
    font-size: 15px;
    margin: 0;
  }
  .agent-draft > h4 {
    font-size: 13px;
    margin: 0;
  }
  .agent-draft > ul {
    margin: 0;
    padding-inline-start: 18px;
  }
  .agent-draft > p {
    margin: 0;
  }
</style>
