<script lang="ts">
  // `changes`: `text_differences` as a two-column diff; `before_and_after_pages` and `_images` side by side.
  // Both sides are read through their references (a version through its own read, SM-SF-1.3).
  import { t } from '../../i18n/fluent';
  import AskButton from '../AskButton.svelte';
  import { diffLines, splitLines } from '../diff';
  import PreviewPane from '../PreviewPane.svelte';
  import ResolvedView from '../ResolvedView.svelte';
  import { resolveRef, type Resolved } from '../refs';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'changes'> = $props();

  let before = $state<Resolved | null>(null);
  let after = $state<Resolved | null>(null);
  $effect(() => {
    const { before: b, after: a } = block;
    before = null;
    after = null;
    let current = true;
    void Promise.all([resolveRef(backend, b), resolveRef(backend, a)]).then(([x, y]) => {
      if (current) {
        before = x;
        after = y;
      }
    });
    return () => {
      current = false;
    };
  });

  const rows = $derived.by(() => {
    if (before?.kind !== 'ok' || after?.kind !== 'ok') return null;
    if (before.preview.kind !== 'text' || after.preview.kind !== 'text') return null;
    return diffLines(splitLines(before.preview.text), splitLines(after.preview.text));
  });
  const mark = (kind: 'same' | 'removed' | 'added'): string => (kind === 'removed' ? '−' : kind === 'added' ? '+' : '');
</script>

<div class="block-changes">
  {#if block.view === 'text_differences'}
    {#if before === null || after === null}
      <p role="status" aria-busy="true"></p>
    {:else if rows === null}
      <ResolvedView resolved={before.kind === 'ok' && after.kind === 'ok' ? { kind: 'unresolvable' } : before.kind === 'ok' ? after : before} backend={backend.handle} />
    {:else}
      <table class="diff">
        <caption class="visually-hidden">{t('block-title-changes')}</caption>
        <thead>
          <tr><th scope="col">{t('block-changes-before')}</th><th scope="col">{t('block-changes-after')}</th></tr>
        </thead>
        <tbody>
          {#each rows as row, index (index)}
            <tr>
              <td class={row.left?.kind ?? 'gap'}><span aria-hidden="true">{row.left === null ? '' : mark(row.left.kind)}</span><span class="text">{row.left?.text ?? ''}</span>{#if row.left?.kind === 'removed'}<span class="visually-hidden">{t('block-changes-removed')}</span>{/if}</td>
              <td class={row.right?.kind ?? 'gap'}><span aria-hidden="true">{row.right === null ? '' : mark(row.right.kind)}</span><span class="text">{row.right?.text ?? ''}</span>{#if row.right?.kind === 'added'}<span class="visually-hidden">{t('block-changes-added')}</span>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else}
    <div class="sides">
      {#each [{ id: 'before', resolved: before }, { id: 'after', resolved: after }] as side (side.id)}
        <section aria-label={side.id === 'before' ? t('block-changes-before') : t('block-changes-after')}>
          <h4>{side.id === 'before' ? t('block-changes-before') : t('block-changes-after')}</h4>
          {#if side.resolved?.kind === 'ok'}
            <PreviewPane preview={side.resolved.preview} label={side.resolved.name} />
          {:else}
            <ResolvedView resolved={side.resolved} backend={backend.handle} />
          {/if}
        </section>
      {/each}
    </div>
  {/if}
  <AskButton {backend} {workspaceId} labelId="block-ask-more-changes" text={t('block-ask-text-more-changes')} attachments={[block.before, block.after]} />
</div>

<style>
  /* The prototype's changes (scene `changes`): the difference in a rounded, bordered box (`.diff`), the
     before-and-after sides as boxes, and the actions below with a gap. */
  .block-changes {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    align-items: flex-start;
  }
  .block-changes > .diff,
  .block-changes > .sides {
    align-self: stretch;
  }
  .diff {
    inline-size: 100%;
    border-collapse: separate;
    border-spacing: 0;
    overflow: hidden;
    table-layout: fixed;
    font-family: var(--font-mono);
    font-size: 13px;
    line-height: 1.5;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
  }
  th {
    padding: var(--space-02) var(--space-sm);
    text-align: start;
    font-family: var(--font-body);
    font-size: 12px;
    color: var(--color-text-muted);
    background: var(--color-surface);
    border-block-end: 1px solid var(--color-line);
  }
  td {
    vertical-align: top;
    padding: 0 var(--space-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    border-inline-end: 1px solid var(--color-line);
  }
  td.removed {
    background: var(--color-diff-removed);
  }
  td.added {
    background: var(--color-diff-added);
  }
  td.gap {
    background: var(--color-surface);
  }
  .sides {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-12);
  }
  .sides > section {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-10);
  }
  .sides h4 {
    margin: 0 0 var(--space-04);
    font-size: 13px;
  }
</style>
