<script lang="ts">
  // `result`: what a task produced with where it is; an offer that waits for an answer; or a list to tick,
  // which is a linked folder's Ready to publish list when it names the folder (SM-WR-13).
  import { t } from '../../i18n/fluent';
  import type { ServiceError } from '../../service-api/types.gen';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import AskButton from '../AskButton.svelte';
  import { isWaitingForServer } from '../ask';
  import ContentRefLine from '../ContentRefLine.svelte';
  import LinkedChecklist from '../LinkedChecklist.svelte';
  import MarkdownView from '../MarkdownView.svelte';
  import { refKey } from '../refs';
  import type { KindProps } from '../types';

  let { backend, workspaceId, blockId, block }: KindProps<'result'> = $props();

  let busy = $state(false);
  let error = $state<ServiceError | null>(null);
  let ticked = $state<Record<string, boolean>>({});
  const waiting = $derived(backend.conversation.get(workspaceId)?.find((i) => i.block_id === blockId)?.waiting ?? true);
  const disabled = $derived(busy || !waiting || isWaitingForServer(backend) || !backend.groups.has('ordin'));

  async function answer(accept: boolean): Promise<void> {
    if (disabled) return;
    busy = true;
    const out = await backend.handle.call('ordin.answer_block', { block_id: blockId, workspace_id: workspaceId, answer: { kind: 'offer', accept } });
    busy = false;
    if (out.kind === 'ok' && out.result.accepted) {
      error = null;
      backend.markAnswered(workspaceId, blockId);
    } else if (out.kind === 'error') {
      error = out.error;
    }
  }
</script>

<div class="block-result">
  {#if block.of === 'produced'}
    <!-- SM-RES-1: what the agent said at the end is its own Markdown; it is drawn formatted, like Ordin's messages. -->
    <div class="summary"><MarkdownView source={block.summary} /></div>
    <ul>
      {#each block.items as reference (refKey(reference))}
        <li><ContentRefLine {backend} {workspaceId} {reference} /></li>
      {/each}
    </ul>
  {:else if block.of === 'offer'}
    <p class="summary">{block.offer === 'save_as_workflow' ? t('block-offer-save-question') : t('block-offer-update-question')}</p>
    <div class="actions">
      <button type="button" {disabled} onclick={() => void answer(true)}>{block.offer === 'save_as_workflow' ? t('block-offer-save') : t('block-offer-update')}</button>
      <button type="button" {disabled} onclick={() => void answer(false)}>{t('block-not-now')}</button>
    </div>
  {:else if block.confluence_link_id}
    <h4>{block.title}</h4>
    <LinkedChecklist {backend} {workspaceId} confluenceLinkId={block.confluence_link_id} />
  {:else}
    <h4>{block.title}</h4>
    <ul>
      {#each block.items as reference (refKey(reference))}
        <li>
          <label>
            <input type="checkbox" checked={ticked[refKey(reference)] === true} onchange={(e) => (ticked[refKey(reference)] = e.currentTarget.checked)} />
            <ContentRefLine {backend} {workspaceId} {reference} />
          </label>
        </li>
      {/each}
    </ul>
  {/if}
  {#if block.of !== 'offer'}
    <AskButton {backend} {workspaceId} labelId="block-ask-about-this" text={t('block-ask-text-this', { title: block.of === 'produced' ? block.summary : block.title })} attachments={block.items} />
  {/if}
  <ErrorLine {error} />
</div>

<style>
  .block-result {
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-items: flex-start;
  }
  .block-result > :is(p, h4, ul) {
    margin: 0;
  }
  .block-result > h4 {
    font-size: 15px;
  }
  .summary {
    overflow-wrap: anywhere;
  }
  ul {
    list-style: none;
    padding: 0;
    display: grid;
    gap: var(--space-04);
  }
  .actions {
    display: flex;
    gap: var(--space-sm);
  }
</style>
