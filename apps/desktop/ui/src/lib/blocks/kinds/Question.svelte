<script lang="ts">
  // `question`: the question, 2 to 4 choice buttons and, when `allow_other`, a box for an answer in words.
  // The answer goes to `ordin.answer_block`; the item then stops waiting.
  import { t } from '../../i18n/fluent';
  import type { ServiceError } from '../../service-api/types.gen';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import { isWaitingForServer } from '../ask';
  import { dismissBlock } from '../dismiss';
  import type { KindProps } from '../types';

  let { backend, workspaceId, blockId, block }: KindProps<'question'> = $props();

  let other = $state('');
  let busy = $state(false);
  let error = $state<ServiceError | null>(null);
  const waiting = $derived(backend.conversation.get(workspaceId)?.find((i) => i.block_id === blockId)?.waiting ?? true);
  const offline = $derived(isWaitingForServer(backend));
  const disabled = $derived(busy || offline || !waiting || !backend.groups.has('ordin'));
  const headingId = $derived(`question-${blockId}`);

  async function answer(body: { kind: 'choice'; choice: string } | { kind: 'other'; text: string }): Promise<void> {
    if (disabled) return;
    busy = true;
    const out = await backend.handle.call('ordin.answer_block', { block_id: blockId, workspace_id: workspaceId, answer: body });
    busy = false;
    if (out.kind === 'ok' && out.result.accepted) {
      error = null;
      backend.markAnswered(workspaceId, blockId);
    } else if (out.kind === 'error') {
      error = out.error;
    }
  }

  // SM-DIS-1: Skip sets the question aside; Ordin carries on without the answer.
  async function skip(): Promise<void> {
    if (disabled) return;
    busy = true;
    error = await dismissBlock(backend, workspaceId, blockId);
    busy = false;
  }
</script>

<div class="block-question" role="group" aria-labelledby={headingId}>
  <p id={headingId} class="question">{block.question}</p>
  <div class="choices">
    {#each block.choices as choice (choice)}
      <button type="button" {disabled} onclick={() => void answer({ kind: 'choice', choice })}>{choice}</button>
    {/each}
  </div>
  {#if block.allow_other}
    <form
      class="other"
      onsubmit={(event) => {
        event.preventDefault();
        if (other.trim() !== '') void answer({ kind: 'other', text: other.trim() });
      }}
    >
      <label>
        <span>{t('block-answer-other')}</span>
        <input type="text" bind:value={other} {disabled} maxlength="300" />
      </label>
      <button type="submit" disabled={disabled || other.trim() === ''}>{t('block-answer-send')}</button>
    </form>
  {/if}
  <!-- A question with its own Not now (the folder card, a pack or language offer) keeps that as its way out: no Skip beside it (SM-DIS-1). -->
  {#if waiting && !block.choices.some((c) => c.trim().toLowerCase() === 'not now')}
    <button type="button" class="skip" {disabled} onclick={() => void skip()}>{t('block-skip')}</button>
  {/if}
  <ErrorLine {error} />
</div>

<style>
  /* Scenes `ambiguous`, `agentAsks`: the question, then one button per choice, one under another. */
  .block-question {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .choices {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-sm);
  }
  .question {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .other {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    align-items: end;
    margin-block-start: var(--space-sm);
  }
  label {
    display: grid;
    gap: var(--space-04);
  }
  .skip {
    align-self: flex-start;
  }
</style>
