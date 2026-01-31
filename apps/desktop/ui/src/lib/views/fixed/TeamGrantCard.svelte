<script lang="ts">
  // The access card (SM-TW-6, ADR-121 item 5): what a colleague added that waits for this person, with the
  // look of an approval card. Nothing is granted until the person presses **Allow**; **Don't allow** comes
  // first, takes the focus when the card is the one opened, and is never the default button. A refusal is
  // drawn in the card through `ErrorLine`; `TEAM_WS_ALLOW_FAILED` offers **Try again**.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TW-6
  import { onMount } from 'svelte';

  import { formatList } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { TeamPendingGrantView } from '../../service-api/types.gen';
  import ErrorLine from '../ErrorLine.svelte';
  import { grantLine, type TeamGrants } from './team-workspace.svelte';

  interface Props {
    grants: TeamGrants;
    grant: TeamPendingGrantView;
    /** The card opened from *Needs you*, or the first card of Team activity: **Don't allow** takes the focus. */
    focus?: boolean;
  }
  let { grants, grant, focus = false }: Props = $props();

  let decline = $state<HTMLButtonElement | null>(null);
  let busy = $state(false);
  const headingId = $derived(`team-grant-${grant.grant_id}`);
  const error = $derived(grants.errors[grant.grant_id] ?? null);

  onMount(() => {
    if (focus) decline?.focus();
  });

  async function decide(allow: boolean): Promise<void> {
    if (busy) return;
    busy = true;
    await grants.decide(grant, allow);
    busy = false;
  }
</script>

<article class="grant-card" aria-labelledby={headingId} data-grant={grant.grant_id}>
  <p id={headingId} class="grant-line">{grantLine(grant)}</p>
  {#if grant.left_out.length > 0}
    <p class="secondary">{t('team-ws-grant-left-out', { places: formatList(grant.left_out) })}</p>
  {/if}
  <ErrorLine {error} onretry={() => void decide(true)} />
  <div class="grant-buttons">
    <button type="button" bind:this={decline} disabled={busy} onclick={() => void decide(false)}>{t('team-ws-grant-decline')}</button>
    <button type="button" disabled={busy} onclick={() => void decide(true)}>{t('team-ws-grant-allow')}</button>
  </div>
</article>

<style>
  /* The look of an approval card (scene `tkPanel`): a bordered card, the sentence first, the buttons under it. */
  .grant-card {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    padding: var(--space-sm) var(--space-12);
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .grant-line {
    margin: 0;
    font-weight: 600;
  }
  .secondary {
    margin: 0;
    color: var(--color-text-muted);
  }
  .grant-buttons {
    display: flex;
    gap: var(--space-06);
  }
</style>
