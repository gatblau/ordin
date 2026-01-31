<script lang="ts">
  // One email draft in full (SM-NS-12.3), as the `email` block draws it: who drafted it, To, the body,
  // and while it may still change **Send…** (which raises the approval card; this screen never sends),
  // **Edit**, **Save to mail drafts** and **Discard** after a confirmation. By state: sent, not sure it was
  // sent, or the reason it was not sent.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-12
  import { t } from '../../i18n/fluent';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { draftIsEditable, paragraphs, type EmailDraftController } from './mail-controller.svelte';

  interface Props {
    backend: BackendState;
    c: EmailDraftController;
    /** The agent's name when an agent drafted it (the screen resolves it); else the general words. */
    agentName?: string | null;
    ondiscarded?: () => void;
  }
  let { backend, c, agentName = null, ondiscarded }: Props = $props();

  let editing = $state(false);
  let to = $state('');
  let subject = $state('');
  let body = $state('');
  let confirmDiscard = $state(false);

  const draft = $derived(c.draft);
  const draftedBy = $derived.by(() => {
    if (draft === null || draft.drafted_by.kind === 'person') return null;
    return agentName !== null ? t('block-email-drafted-by', { agent: agentName }) : t('block-email-drafted-by-an-agent');
  });

  function startEdit(): void {
    if (draft === null) return;
    to = draft.to.join(', ');
    subject = draft.subject;
    body = draft.body;
    editing = true;
  }
  async function save(): Promise<void> {
    const list = to.split(',').map((a) => a.trim()).filter((a) => a !== '');
    if (await c.edit({ to: list, subject, body })) editing = false;
  }
</script>

<section class="email-draft" aria-labelledby="email-draft-heading">
  {#if draft !== null}
    <h2 id="email-draft-heading" tabindex="-1" data-region-focus>{draft.subject}</h2>
    {#if draftedBy !== null}<p class="quiet">{draftedBy}</p>{/if}
    {#if editing}
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <label>{t('block-email-to')} <input type="text" bind:value={to} /></label>
        <input type="text" aria-label={t('block-email-edit')} bind:value={subject} />
        <textarea rows="10" aria-label={t('block-email-edit')} bind:value={body}></textarea>
        <button type="submit" disabled={c.busy}>{t('doc-save')}</button>
        <button type="button" onclick={() => (editing = false)}>{t('doc-cancel')}</button>
      </form>
    {:else}
      <p>{t('block-email-to')}: {draft.to.join(', ')}</p>
      {#if draft.cc.length > 0}<p>{t('email-cc')}: {draft.cc.join(', ')}</p>{/if}
      {#each paragraphs(draft.body) as paragraph, i (i)}<p class="text">{paragraph}</p>{/each}
    {/if}
    {#if draft.state === 'sent'}
      <p role="status">{t('email-sent')}</p>
    {:else if draft.state === 'not_sure_it_was_sent'}
      <p role="status">{t('email-not-sure')}</p>
    {:else if draft.state === 'not_sent' && draft.failure_reason}
      <p role="status">{draft.failure_reason}</p>
    {/if}
    {#if draftIsEditable(draft) && !editing}
      <div class="actions">
        <button type="button" disabled={c.busy} onclick={() => void c.send()}>{t('block-email-send')}</button>
        <button type="button" onclick={startEdit}>{t('block-email-edit')}</button>
        <button type="button" disabled={c.busy} onclick={() => void c.saveToMailDrafts()}>{t('block-email-save-to-drafts')}</button>
        <button type="button" class="danger" disabled={c.busy} onclick={() => (confirmDiscard = true)}>{t('block-email-discard')}</button>
      </div>
    {/if}
    {#if c.notice === 'saved_to_drafts'}<p role="status">{t('block-email-saved-to-drafts')}</p>{/if}
  {/if}
  <ErrorLine error={c.error} onretry={() => void c.load()} />
</section>

{#if confirmDiscard}
  <ConfirmDialog
    title={t('block-email-discard-title')}
    body={t('email-discard-body')}
    verb={t('block-email-discard')}
    onconfirm={() => {
      confirmDiscard = false;
      void c.discard().then((done) => done && ondiscarded?.());
    }}
    oncancel={() => (confirmDiscard = false)}
  />
{/if}

<style>
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .quiet {
    color: var(--color-text-muted);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
</style>
