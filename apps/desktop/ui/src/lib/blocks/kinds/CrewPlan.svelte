<script lang="ts">
  // `crew_plan`: the crew's purpose, agents and limits, its estimated use before it starts (ADR-062, UX-A32)
  // and **Start**. The estimate is the Usage Planner's own result, copied unchanged by the Ordin Host; the
  // rest is the charter, read with `crews.charter` (SM-WR-15). A backend without the command draws the
  // estimate alone.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WR-15
  import { formatNumber, formatPercent } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import { CrewPlanController, limitLines } from '../crew-plan.svelte';
  import type { KindProps } from '../types';

  let { backend, workspaceId, blockId, block }: KindProps<'crew_plan'> = $props();
  // svelte-ignore state_referenced_locally
  const plan = new CrewPlanController(backend, workspaceId, blockId, block.crew_charter_id);
  const estimate = $derived(block.estimate ?? null);

  $effect(() => {
    void plan.load();
  });
</script>

<div class="block-crew-plan">
  {#if plan.charter !== null}
    <p class="purpose">{plan.charter.purpose}</p>
    <h4>{t('block-crew-members-heading')}</h4>
    <ul class="members">
      {#each plan.charter.members as member, at (at)}
        <li class="member">{t('block-crew-member', { agent: plan.agentName(member) ?? t('block-crew-agent-unknown'), role: member.role })}</li>
      {/each}
    </ul>
    <h4>{t('block-crew-limits-heading')}</h4>
    <ul class="limits">
      {#each limitLines(plan.charter, formatNumber) as line (line.id)}
        <li>{t(line.id, line.args)}</li>
      {/each}
    </ul>
  {/if}
  {#if estimate !== null}
    <div class="estimate">
    <p class="band">
      {t(`block-estimate-${estimate.band}`)}
      · {t('block-estimate-turns', { min: formatNumber(estimate.min_turns), max: formatNumber(estimate.max_turns) })}
    </p>
    <p class="muted">{t('block-estimate-day', { percent: formatPercent(estimate.typical_day_permille / 1000) })}</p>
    {#if estimate.ask_first}<p class="note">{t('block-estimate-ask-first')}</p>{/if}
    </div>
  {/if}
  {#if plan.unavailable}<p>{t('block-cannot-show')}</p>{/if}
  {#if plan.error !== null}<ErrorLine error={plan.error} />{/if}
  {#if plan.offersStart}
    <button type="button" class="primary" disabled={!plan.canStart} onclick={() => void plan.start()}>{t('block-crew-start')}</button>
  {:else if plan.startState === 'started'}
    <p>{t('block-crew-started')}</p>
  {:else if plan.startState === 'not-startable'}
    <p>{t('block-crew-not-startable')}</p>
  {/if}
  <ErrorLine error={plan.startError} />
</div>

<style>
  /* The prototype's crew plan (scenes `crew`, `useEstimate`): members as cards in a grid, the limits as
     a compact list, the estimate in a dashed box (`.useest`), Start as the primary button. */
  .block-crew-plan {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    align-items: flex-start;
  }
  .block-crew-plan > :global(*) {
    margin: 0;
  }
  .block-crew-plan h4 {
    font-size: 14px;
    margin-top: var(--space-04);
  }
  .members {
    align-self: stretch;
    list-style: none;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: var(--space-sm);
  }
  .member {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    padding: var(--space-sm) var(--space-10);
    font-size: 14px;
  }
  .limits {
    list-style: none;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    font-size: 14px;
  }
  .estimate {
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: var(--space-02);
    padding: var(--space-sm) var(--space-10);
    border-radius: var(--radius-md);
    border: 1px dashed var(--color-line-strong);
    font-size: 13px;
  }
  .estimate p {
    margin: 0;
  }
  .band {
    font-weight: 600;
  }
  .note {
    color: var(--color-waiting-text);
  }
  .purpose {
    overflow-wrap: anywhere;
  }
</style>
