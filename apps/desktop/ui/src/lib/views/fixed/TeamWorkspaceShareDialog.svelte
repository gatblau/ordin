<script lang="ts">
  // The share dialog (SM-TW-2), opened by the tree's **Share** (SM-TW-13) or by **Share the whole
  // workspace** (SM-TW-14): what is shared is said before it is shared. Step one is the folder: the
  // workspace's own team folder when it already shares knowledge (shown as `team-header`, not
  // changeable, and upgraded in place), otherwise a linked folder or the folder's address. Choosing a folder sends
  // `team_knowledge.workspace_share_preview`; the places, what stays here, the agents and their access, the
  // knowledge and the results place are drawn before **Share** is offered. With `resultsOnly` the dialog
  // is **Change…** of the panel (SM-TW-5): the results radio buttons alone, read from
  // `team_knowledge.results_choices` with the current place checked, and sent with `set_results_place`.
  // A refusal stays in the dialog through `ErrorLine`. **Cancel** is first in focus; Escape is **Cancel**.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TW-2
  // spec: ordin-desktop/components/ScreensModule#SM-TW-5
  import { onMount } from 'svelte';

  import { formatList } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { TeamFolderChoice, TeamResultsChoice, TeamResultsChoices, TeamWorkspaceSharePreview } from '../../service-api/types.gen';
  import { focusableIn, trapIndex } from '../../shell/focus';
  import ErrorLine from '../ErrorLine.svelte';
  import { splitFolderAddress, type TeamController } from './team.svelte';
  import { accessPhrase } from './team-workspace.svelte';

  interface Props {
    controller: TeamController;
    /** The workspace's name (`team-ws-share-title`). */
    workspaceName: string;
    /** **Change…** of the panel: the results radio buttons alone (SM-TW-5). */
    resultsOnly?: boolean;
    /** Opens Settings › Sign-ins (the `sign_in_again` action of `TEAM_SIGN_IN_NEEDED`). */
    onsignin?: () => void;
    onclose: () => void;
  }
  let { controller: c, workspaceName, resultsOnly = false, onsignin, onclose }: Props = $props();

  const headingId = `team-ws-share-${Math.random().toString(36).slice(2, 10)}`;
  let root = $state<HTMLElement | null>(null);
  let heading = $state<HTMLElement | null>(null);
  let cancelButton = $state<HTMLElement | null>(null);
  let opener: Element | null = null;

  let pickedLink = $state('');
  let address = $state('');
  let addressInvalid = $state(false);
  let choice = $state.raw<TeamFolderChoice | null>(null);
  let preview = $state.raw<TeamWorkspaceSharePreview | null>(null);
  let resultsChoices = $state.raw<TeamResultsChoices | null>(null);
  let resultsAt = $state(0);
  let busy = $state(false);

  // The workspace already shares knowledge: its own folder is the folder, shown and not changeable; the
  // own binding's link names it (TeamKnowledgeService.md TW-A step 2: "a `Link` to the bound link").
  // spec: ordin-desktop/components/TeamKnowledgeService#TW-A
  const ownLink = $derived(c.status?.binding?.source_link_id ?? null);
  const ownFolderName = $derived(c.status?.header_line ?? c.status?.binding?.folder ?? '');
  const options = $derived(resultsOnly ? (resultsChoices?.choices ?? null) : (preview?.results_choices ?? null));
  const chosen = $derived(options?.[resultsAt] ?? null);

  function same(a: TeamResultsChoice, b: TeamResultsChoice): boolean {
    if (a.kind === 'team_folder' || b.kind === 'team_folder') return a.kind === b.kind;
    return a.source_link_id === b.source_link_id;
  }

  onMount(() => {
    opener = document.activeElement;
    cancelButton?.focus();
    c.error = null;
    if (resultsOnly) void loadChoices();
    else if (ownLink !== null) void previewFor({ kind: 'link', source_link_id: ownLink });
    else void c.loadCandidates();
    return () => {
      if (opener instanceof HTMLElement) opener.focus();
    };
  });

  /** **Change…**: the choices with the current place checked (`current` absent → the first). */
  async function loadChoices(): Promise<void> {
    resultsChoices = null;
    const read = await c.resultsChoices();
    const current = read?.current ?? null;
    const at = current === null ? -1 : (read?.choices.findIndex((o) => same(o.choice, current)) ?? -1);
    resultsAt = at < 0 ? 0 : at;
    resultsChoices = read;
  }

  async function previewFor(next: TeamFolderChoice): Promise<void> {
    choice = next;
    preview = null;
    resultsAt = 0;
    preview = await c.sharePreview(next);
  }

  /** The value is read from the list itself: its `change` can reach this handler before `bind:value` has. */
  function pick(value: string): void {
    pickedLink = value;
    if (pickedLink === '') return;
    address = '';
    addressInvalid = false;
    void previewFor({ kind: 'link', source_link_id: pickedLink });
  }

  function addressChosen(): void {
    if (address.trim() === '') return;
    const parsed = splitFolderAddress(address);
    addressInvalid = parsed === null;
    if (parsed === null) return;
    pickedLink = '';
    void previewFor({ kind: 'new_folder', url: parsed.url, branch: parsed.branch, path: parsed.path });
  }

  async function submit(): Promise<void> {
    if (busy || chosen === null || (!resultsOnly && choice === null)) return;
    busy = true;
    const ok = resultsOnly || choice === null ? await c.setResultsPlace(chosen.choice) : await c.shareWorkspace(choice, chosen.choice);
    busy = false;
    if (ok) onclose();
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      onclose();
    } else if (event.key === 'Tab' && root !== null) {
      const items = [heading, ...focusableIn(root)].filter((x): x is HTMLElement => x !== null);
      const next = trapIndex(items.indexOf(document.activeElement as HTMLElement), items.length, event.shiftKey);
      if (next >= 0) {
        event.preventDefault();
        items[next]?.focus();
      }
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="dialog-backdrop">
  <div bind:this={root} class="dialog" role="dialog" aria-modal="true" aria-labelledby={headingId}>
  <form
    class="team-ws-dialog"
    onsubmit={(e) => {
      e.preventDefault();
      void submit();
    }}
  >
    <h2 id={headingId} bind:this={heading} tabindex="-1">{resultsOnly ? t('team-ws-results-label') : t('team-ws-share-title', { workspace: workspaceName })}</h2>

    {#if !resultsOnly}
      {#if ownLink !== null}
        <p class="team-header">{t('team-header', { folder: ownFolderName })}</p>
      {:else}
        <label>
          {t('team-share-dialog-pick')}
          <select bind:value={pickedLink} onchange={(e) => pick(e.currentTarget.value)}>
            <option value=""></option>
            {#each c.candidates as link (link.id)}<option value={link.id}>{link.display_name}</option>{/each}
          </select>
        </label>
        <label>
          {t('team-share-dialog-address')}
          <input type="text" bind:value={address} onchange={addressChosen} aria-invalid={addressInvalid} aria-describedby={addressInvalid ? `${headingId}-invalid` : undefined} />
        </label>
        {#if addressInvalid}<p id="{headingId}-invalid">{t('team-share-dialog-address-invalid')}</p>{/if}
      {/if}
    {/if}

    {#if options !== null}
      <fieldset class="results">
        <legend>{t('team-ws-results-label')}</legend>
        {#each options as option, i (i)}
          <label class="radio">
            <input type="radio" name="{headingId}-results" value={i} checked={resultsAt === i} onchange={() => (resultsAt = i)} />
            {option.choice.kind === 'team_folder' ? t('team-ws-results-team-folder') : option.name}
          </label>
        {/each}
      </fieldset>

      {#if !resultsOnly && preview !== null}
        <div class="preview">
          {#if preview.places.length > 0}
            <p>{t('team-ws-preview-places', { places: formatList(preview.places.map((p) => p.display_name)) })}</p>
          {:else}
            <p>{t('team-ws-preview-no-places')}</p>
          {/if}
          {#if preview.stays_here.length > 0}
            <p>{t('team-ws-preview-stays', { names: formatList(preview.stays_here) })}</p>
          {/if}
          <h3>{t('team-ws-preview-agents')}</h3>
          <ul>
            {#each preview.agents as agent (agent.key)}
              <li>{t('team-ws-agent-access', { agent: agent.name, access: accessPhrase(agent.access) })}</li>
            {/each}
          </ul>
          <p>{t('team-ws-preview-knowledge', { facts: preview.knowledge.new_facts, pages: preview.knowledge.new_pages })}</p>
          {#if chosen !== null}<p>{t('team-ws-preview-results', { place: chosen.name })}</p>{/if}
          <p>{t('team-ws-share-intro')}</p>
          <p>{t('team-share-dialog-history')}</p>
        </div>
      {/if}
    {/if}

    <ErrorLine error={c.error} onretry={() => void c.retry?.()} {onsignin} />

    <div class="dialog-buttons">
      <button type="button" bind:this={cancelButton} onclick={onclose}>{t('confirm-cancel')}</button>
      <button type="submit" class="go" disabled={busy || options === null || chosen === null}>{resultsOnly ? t('engineering-save') : t('team-ws-share-verb')}</button>
    </div>
  </form>
  </div>
</div>

<style>
  .team-ws-dialog {
    display: flex;
    flex-direction: column;
    gap: var(--space-10);
  }
  .team-ws-dialog label {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  .team-ws-dialog label.radio {
    flex-direction: row;
    align-items: center;
  }
  .results {
    border: 0;
    padding: 0;
    margin: 0;
  }
  .preview p,
  .preview ul {
    margin: var(--space-04) 0;
  }
  .preview h3 {
    margin: var(--space-06) 0 0;
    font-size: 13px;
  }
</style>
