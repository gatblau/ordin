<script lang="ts">
  // The conversation's one-line card for an approval: *Waiting for you* and **Open** while it waits, then
  // `card-decided` ("Approved: <verb>.") once it is approved. Approval cards are never blocks; they come only
  // from `approval_requested` (02:333).
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { ApprovalCard } from '../service-api/types.gen';
  import { getApp } from '../shell/context';
  import { openScreen } from '../shell/open-item';
  import { makeKey, SCREEN_IDS } from '../shell/screens';
  import type { BackendState } from '../store/backend-state.svelte';

  interface Props {
    backend: BackendState;
    card: ApprovalCard;
  }
  let { backend, card }: Props = $props();
  const app = getApp();
  const decided = $derived(backend.decided.get(card.approval_id));
</script>

<!-- A one-line card (the prototype's `.lcard.wait`, scenes `mailSend`, `slack`): the question, then *Waiting for you* and **Open** on a row below. -->
<div class="approval-line">
  <span class="icon-tile" data-tint="accent-ink"><Icon name="info" /></span>
  <span class="title">{card.question}</span>
  {#if decided === undefined}
    <span class="wait-row">
      <span class="state-label waiting"><span aria-hidden="true">●</span> {t('card-waiting')}</span>
      <button type="button" class="small" onclick={() => openScreen(app, backend.id, card.workspace_id, makeKey(SCREEN_IDS.approval, card.approval_id))}>{t('card-open')}</button>
    </span>
  {:else if decided.state === 'approved'}
    <span class="decided">{t('card-decided', { verb: card.verb })}</span>
  {/if}
</div>

<style>
  .approval-line {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm) var(--space-10);
    align-items: center;
    padding: var(--space-sm) var(--space-10);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    inline-size: 100%;
    max-inline-size: 780px;
    box-sizing: border-box;
  }
  .title {
    font-size: 14px;
    font-weight: 700;
    flex: 1 1 60%;
    overflow-wrap: anywhere;
  }
  .wait-row,
  .decided {
    flex-basis: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    font-size: 12px;
  }
</style>
