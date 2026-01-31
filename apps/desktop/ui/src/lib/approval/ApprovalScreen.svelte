<script lang="ts">
  // The fixed approval card (11 §5.5, UX-A53, ADR-080). It is drawn in this order: the question as heading;
  // `approval-from-ordin`; Where, Affects, Changes; the content (a button, or open on the card for a message
  // or a command); the approve button and the reject button; `approval-last-line`. The fixed lines come only
  // from the catalogue: no field of the card can change them. The approve button is never the default
  // button, so Enter on the card does not approve (UX-A2), and focus never lands on a button when the card
  // opens. Pressing sends `approval.decide { card_ref, decision }` to the card's backend.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-1
  import { onMount, tick, untrack } from 'svelte';

  import { formatTime } from '../format/intl';
  import { t } from '../i18n/fluent';
  import type { ApprovalCard, ContentRef, PressDecision, ServiceError } from '../service-api/types.gen';
  import type { ScreenProps } from '../shell/screens';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { isWaitingForServer } from '../blocks/ask';
  import { resolveRef } from '../blocks/refs';
  import ContentView from './ContentView.svelte';
  import { decide } from './decide';
  import { foldMessage } from './message-fold';

  let { backend, instance }: ScreenProps = $props();

  // The card as it was when the screen opened, kept after the decision (the store drops it from `pending`).
  // svelte-ignore state_referenced_locally
  let card = $state<ApprovalCard | null>(instance === null ? null : (backend.pendingApprovals.get(instance) ?? null));
  let decidedLocal = $state<{ state: string; decided_on: string; at: string } | null>(null);
  let heading = $state<HTMLElement | null>(null);
  let contentHeading = $state<HTMLElement | null>(null);
  let contentOpen = $state(false);
  let wholeOpen = $state(false);
  let busy = $state(false);
  let closed = $state(false);
  let error = $state<ServiceError | null>(null);

  $effect(() => {
    const id = instance;
    if (id === null) return;
    const held = backend.pendingApprovals.get(id);
    if (held !== undefined) {
      card = held;
      return;
    }
    // Read without subscribing: this effect writes `card`, and a card whose id differs from the one asked for
    // would otherwise ask again for ever (plan 04, found by the dead-control sweep).
    const shown = untrack(() => card);
    if (shown !== null && shown.approval_id === id) return;
    let current = true;
    void backend.handle.call('approval.get', { approval_id: id }).then((out) => {
      if (!current) return;
      if (out.kind === 'ok') {
        card = out.result.card;
        if (out.result.state !== 'pending') decidedLocal = { state: out.result.state, decided_on: out.result.decided_on ?? '', at: out.result.decided_at ?? '' };
      } else {
        error = out.error;
      }
    });
    return () => {
      current = false;
    };
  });

  const decided = $derived(instance === null ? undefined : (backend.decided.get(instance) ?? decidedLocal ?? undefined));
  const offline = $derived(isWaitingForServer(backend));
  const inline = $derived(card?.content?.kind === 'message' || card?.content?.kind === 'command');
  const message = $derived(card?.content?.kind === 'message' ? foldMessage(card.content.text, wholeOpen) : null);
  // The approve button reads "… you've read the start" until a long message has been opened (11 §5.5).
  const mustReadFirst = $derived(message !== null && message.folded);
  const disabled = $derived(busy || offline || decided !== undefined || closed);

  // Nothing is approved unseen (SM-SF-1.4): every exact-version reference of the card is read when the card
  // opens, and Approve stays off until all of them have been read. Reject is never held back.
  const boundRefs = $derived<ContentRef[]>(
    (card?.content?.kind === 'changes' ? [card.content.change_set] : card?.content?.kind === 'page' ? [card.content.page] : card?.content?.kind === 'files' ? card.content.files : []).filter((r) => r.kind === 'file_version'),
  );
  // `null` while being read; `true` when every version could be shown; `false` otherwise.
  let boundShown = $state<boolean | null>(null);
  $effect(() => {
    const refs = boundRefs;
    if (refs.length === 0) return;
    boundShown = null;
    let current = true;
    void Promise.all(refs.map((r) => resolveRef(backend, r))).then((all) => {
      if (current) boundShown = all.every((r) => r.kind === 'ok');
    });
    return () => {
      current = false;
    };
  });
  const approveHeld = $derived(boundRefs.length > 0 && boundShown !== true);

  onMount(() => {
    // Focus goes to the heading, never to a button (UX-A2).
    heading?.focus();
  });

  async function press(decision: PressDecision): Promise<void> {
    if (card === null || disabled || (decision === 'approve' && approveHeld)) return;
    busy = true;
    const result = await decide(backend, card, decision);
    busy = false;
    if (result.kind === 'decided') {
      error = null;
      decidedLocal = { state: result.state, decided_on: result.decided_on, at: backend.now() };
    } else {
      error = result.error;
      if (result.kind === 'closed') closed = true;
    }
  }

  async function openContent(): Promise<void> {
    contentOpen = !contentOpen;
    if (contentOpen) {
      await tick();
      contentHeading?.focus();
    }
  }
</script>

{#if card !== null}
  <!-- The question is the screen's heading, above the card's frame (scenes `reject`, `typedYes`). -->
  <h2 id="approval-heading" class="approval-heading" bind:this={heading} tabindex="-1">{card.question}</h2>
  <article class="approval-card" aria-labelledby="approval-heading">
    <p class="from-ordin">{t('approval-from-ordin')}</p>

    <dl class="facts">
      <dt>{t('approval-where')}</dt>
      <dd>{card.where_line}</dd>
      <dt>{t('approval-affects')}</dt>
      <dd>
        <ul>
          {#each card.affects as item, index (index)}
            <li>
              {#if item.kind === 'file'}
                {item.label}{#if item.version}{' · '}{t('approval-affects-version', { version: item.version })}{/if}
              {:else if item.kind === 'record'}
                {item.label}
              {:else}
                {item.name} ({item.address}){#if item.outside_company} <span class="state-label waiting"><span aria-hidden="true">↗</span>{t('approval-outside-company')}</span>{/if}
              {/if}
            </li>
          {/each}
        </ul>
      </dd>
      <dt>{t('approval-changes')}</dt>
      <dd>{card.changes}</dd>
    </dl>

    {#if card.content !== undefined && card.content !== null}
      {#if card.content.kind === 'message'}
        <div class="inline-content">
          <p class="message">{message?.text ?? card.content.text}</p>
          {#if card.content.destination_note}<p class="note">{card.content.destination_note}</p>{/if}
          {#if message?.folded}
            <button type="button" onclick={() => (wholeOpen = true)}>{t('approval-show-whole')}</button>
          {/if}
        </div>
      {:else if card.content.kind === 'command'}
        <div class="inline-content">
          <p class="note">{card.content.runs_in}</p>
          <pre><code>{card.content.command}</code></pre>
        </div>
      {:else if contentOpen}
        <ContentView {backend} content={card.content} bind:heading={contentHeading} />
      {/if}
    {/if}

    {#if card.content !== undefined && card.content !== null && !inline && (decided !== undefined || closed)}
      <button type="button" aria-expanded={contentOpen} onclick={() => void openContent()}>{card.content.label}</button>
    {/if}

    {#if decided !== undefined}
      <p class="decided" role="status">{t('approval-decided-on', { place: decided.decided_on, time: decided.at === '' ? '' : formatTime(decided.at) })}</p>
    {:else if !closed}
      <!-- The content button, then approve and reject, in one row (the prototype's `.approval .row`, scenes `slack`, `typedYes`). -->
      <div class="buttons">
        {#if card.content !== undefined && card.content !== null && !inline}
          <button type="button" aria-expanded={contentOpen} onclick={() => void openContent()}>{card.content.label}</button>
        {/if}
        <!-- Neither button is the default button, and there is no form: Enter on the card approves nothing. -->
        <button type="button" class="go" disabled={disabled || approveHeld} onclick={() => void press('approve')}>
          {mustReadFirst ? t('approval-verb-read-start', { verb: card.verb }) : card.verb}
        </button>
        <button type="button" {disabled} onclick={() => void press('reject')}>
          {card.reject_style === 'not_now' ? t('approval-not-now') : t('approval-reject')}
        </button>
      </div>
      {#if boundRefs.length > 0 && boundShown === false}<p class="note">{t('approval-approve-needs-version')}</p>{/if}
      {#if offline}<p class="note">{t('server-waiting-for', { server: backend.displayName })}</p>{/if}
    {/if}
    <ErrorLine {error} />
    <p class="last-line">{t('approval-last-line')}</p>
  </article>
{:else}
  <ErrorLine {error} />
{/if}

<style>
  /* The approval card (cross-cutting/VisualDesign.md V6 "Approval card"; scenes `reject`, `typedYes`). */
  .approval-card {
    background: var(--color-approval-background);
    border: 2px solid var(--color-approval-line);
    border-radius: var(--radius-lg);
    padding: var(--space-14);
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-inline-size: 780px;
    align-items: flex-start;
  }
  .approval-card > :is(.facts, .inline-content, :global(.approval-content)) {
    align-self: stretch;
  }
  .approval-heading {
    font-size: 19px;
    margin: 0 0 var(--space-12);
  }
  /* Focus is moved to the heading when the card opens; it is not a control, so it draws no ring. */
  .approval-heading:focus {
    outline: none;
  }
  .from-ordin {
    display: flex;
    align-items: center;
    gap: var(--space-06);
    margin: 0;
    font-size: 12px;
    font-weight: 700;
    color: var(--color-accent-ink);
  }
  .from-ordin::before {
    content: '';
    inline-size: 10px;
    block-size: 10px;
    border-radius: 3px;
    background: var(--color-approval-line);
  }
  .note {
    color: var(--color-text-muted);
    font-size: 13px;
    margin: 0;
  }
  .facts {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 2px 10px;
    margin: 0;
    font-size: 14px;
  }
  .facts dt {
    font-weight: 700;
  }
  .facts dd {
    margin-inline-start: 0;
    overflow-wrap: anywhere;
  }
  /* Affects: one line per item, without bullets (the prototype's `.anat dd`). */
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .state-label {
    margin-inline-start: var(--space-04);
  }
  /* A message or command shown open on the card: an item box (`.pitem`) holding the exact text (`.preview`). */
  .inline-content {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-list);
    padding: var(--space-sm) var(--space-10);
    background: var(--color-surface);
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    align-items: flex-start;
  }
  .inline-content > :is(.message, pre) {
    align-self: stretch;
  }
  .message {
    margin: 0;
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-12);
    background: var(--color-background);
    font-size: 14px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  pre {
    overflow: auto;
    padding: var(--space-sm);
    background: var(--color-surface);
    border-radius: var(--radius-md);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }
  .last-line {
    color: var(--color-text-muted);
    font-size: 12px;
    margin: 0;
  }
</style>
