<script lang="ts">
  // The content view the card's content button opens (11 §5.5): the change set, the page, the files or the
  // email, exactly as the card holds them. A reference of kind `file_version` is read with `readFileVersion`
  // on the card's backend and shows the version the approval covers, never the file as it is now (SM-SF-1).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-1
  import { t } from '../i18n/fluent';
  import type { ApprovalContent, ContentRef } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import RefPreview from '../blocks/RefPreview.svelte';
  import { refKey } from '../blocks/refs';

  interface Props {
    backend: BackendState;
    content: Extract<ApprovalContent, { kind: 'changes' | 'page' | 'files' | 'email' }>;
    /** The first heading of the view, so the card can move focus to it when the view opens. */
    heading?: HTMLElement | null;
  }
  let { backend, content, heading = $bindable(null) }: Props = $props();

  const refs = $derived<ContentRef[]>(content.kind === 'changes' ? [content.change_set] : content.kind === 'page' ? [content.page] : content.kind === 'files' ? content.files : []);
</script>

<!-- An item box (the prototype's `.pitem`) holding what the card covers; an email's text in a `.preview` box (scene `mailSend`). -->
<div class="approval-content">
  {#if content.kind === 'email'}
    {@const email = content.email}
    <h2 bind:this={heading} tabindex="-1">{email.subject}</h2>
    <dl>
      <dt>{t('block-email-from')}</dt>
      <dd>{email.from}</dd>
      <dt>{t('block-email-to')}</dt>
      <dd>
        {#each email.to as recipient (recipient.address)}
          <span class="recipient">{recipient.name ?? recipient.address}{#if recipient.outside_company} <span class="state-label waiting"><span aria-hidden="true">↗</span>{t('approval-outside-company')}</span>{/if}</span>
        {/each}
      </dd>
    </dl>
    <p class="body">{email.body}</p>
    {#each email.attachments as attachment (attachment.file_name)}
      <p>{attachment.file_name}</p>
    {/each}
  {:else}
    {#each refs as reference, index (refKey(reference))}
      <RefPreview {backend} {reference} bound={reference.kind === 'file_version'} showTargets bind:heading={() => (index === 0 ? heading : null), (value) => (index === 0 ? (heading = value) : undefined)} />
    {/each}
  {/if}
</div>

<style>
  .approval-content {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    padding: var(--space-sm) var(--space-10);
    background: var(--color-surface);
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .approval-content > :is(h2, p, dl) {
    margin: 0;
  }
  h2 {
    font-size: 16px;
  }
  /* Focus is moved to the heading when the view opens; it is not a control, so it draws no ring. */
  h2:focus {
    outline: none;
  }
  dl {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 2px var(--space-10);
    font-size: 14px;
  }
  dt {
    font-weight: 700;
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .body {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-12);
    background: var(--color-background);
    font-size: 14px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .state-label {
    margin-inline-start: var(--space-04);
  }
</style>
