<script lang="ts">
  // The conversation's card for a block shown on a screen: the item's title, the *Now* sentence for
  // progress (the same words everywhere, 11 §5.4), and **Open**. A waiting item says so and cannot be closed
  // (the conversation panel offers **Close** only to items that do not wait).
  import { formatNumber } from '../format/intl';
  import { t } from '../i18n/fluent';
  import { blockKindIcon } from '../icons/kinds';
  import Icon from '../icons/Icon.svelte';
  import { getApp } from '../shell/context';
  import { openScreen } from '../shell/open-item';
  import { makeKey } from '../shell/screens';
  import type { BlockViewProps } from '../shell/screens';
  import { BLOCK_SCREEN_ID } from './block-screen';
  import { blockTitle } from './titles';

  let { backend, workspaceId, blockId, block }: BlockViewProps = $props();
  const app = getApp();

  const waiting = $derived(backend.conversation.get(workspaceId)?.find((i) => i.block_id === blockId)?.waiting ?? false);
  const title = $derived(blockTitle(block));
  const now = $derived.by(() => {
    if (block.kind !== 'plan_progress' || block.subject.kind !== 'run') return null;
    const step = backend.runs.get(block.subject.run_id)?.plan?.steps.find((s) => s.mark === 'now');
    if (step === undefined) return null;
    const total = backend.runs.get(block.subject.run_id)?.plan?.steps.length ?? 0;
    return t('progress-now', { sentence: step.line, n: formatNumber(step.position), total: formatNumber(total) });
  });

  function open(): void {
    openScreen(app, backend.id, workspaceId, makeKey(BLOCK_SCREEN_ID, blockId), { title });
  }
</script>

<div class="one-line-card">
  <span class="icon-tile" data-tint="accent-ink"><Icon name={blockKindIcon(block.kind)} /></span>
  <span class="title">{title}</span>
  {#if now !== null}<span class="now">{now}</span>{/if}
  {#if waiting}<span class="waiting"><span aria-hidden="true">●</span> {t('card-waiting')}</span>{/if}
  <button type="button" class="small" onclick={open}>{t('card-open')}</button>
</div>

<style>
  .one-line-card {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-04) 10px;
    align-items: center;
    padding: 10px var(--space-12);
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-card);
    inline-size: 100%;
    max-inline-size: 780px;
    box-sizing: border-box;
  }
  .title {
    font-size: 14px;
    font-weight: 700;
    overflow-wrap: anywhere;
    flex: 1 1 10ch;
  }
  .now {
    flex-basis: 100%;
    order: 3;
    font-size: 12px;
    color: var(--color-text-muted);
  }
  .waiting {
    order: 4;
    flex-basis: 100%;
    font-size: 12px;
    font-weight: 600;
    color: var(--color-waiting-text);
  }
</style>
