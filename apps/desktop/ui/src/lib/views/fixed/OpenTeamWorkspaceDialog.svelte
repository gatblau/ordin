<script lang="ts">
  // **Open a team workspace…** (SM-TW-3): one field for the team folder's address as GitHub or GitLab shows
  // it (split by the rule of the share dialog), **Continue**, then the preview card: the name (editable,
  // 1–80 characters), the places with their reach, the agents with their access and whether they are here,
  // the knowledge, the results place and `team-ws-open-intro`; **Open** sends `team_knowledge.open_workspace`.
  // Refusals stay in the dialog through `ErrorLine`. **Cancel** is first in focus; Escape is **Cancel**.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TW-3
  import { onMount } from 'svelte';

  import { formatList } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { ServiceError, TeamPlaceLine, TeamWorkspaceOpenPreview } from '../../service-api/types.gen';
  import { focusableIn, trapIndex } from '../../shell/focus';
  import ErrorLine from '../ErrorLine.svelte';
  import { splitFolderAddress, type ParsedFolder } from './team.svelte';
  import { accessPhrase, FOUND_MESSAGE, nameAllowed, openPreview, openWorkspace, REACH_MESSAGE } from './team-workspace.svelte';
  import type { Handle } from './util';

  interface Props {
    handle: Handle;
    /** Called with the new workspace's id once `open_workspace` answers. */
    onopened: (workspaceId: string) => void;
    /** Opens Settings › Sign-ins (the `sign_in_again` action of `TEAM_SIGN_IN_NEEDED`). */
    onsignin?: () => void;
    onclose: () => void;
  }
  let { handle, onopened, onsignin, onclose }: Props = $props();

  const headingId = `team-ws-open-${Math.random().toString(36).slice(2, 10)}`;
  let root = $state<HTMLElement | null>(null);
  let heading = $state<HTMLElement | null>(null);
  let cancelButton = $state<HTMLElement | null>(null);
  let opener: Element | null = null;

  let address = $state('');
  let addressInvalid = $state(false);
  let folder = $state.raw<ParsedFolder | null>(null);
  let preview = $state.raw<TeamWorkspaceOpenPreview | null>(null);
  let name = $state('');
  let error = $state.raw<ServiceError | null>(null);
  let retry: (() => void) | null = null;
  let busy = $state(false);

  // `name_in_use`: Open stays off until the name changes from the one the preview found taken.
  const nameTaken = $derived(preview !== null && preview.name_in_use && name.trim() === preview.name.trim());
  const canOpen = $derived(preview !== null && !busy && !nameTaken && nameAllowed(name));

  onMount(() => {
    opener = document.activeElement;
    cancelButton?.focus();
    return () => {
      if (opener instanceof HTMLElement) opener.focus();
    };
  });

  async function cont(): Promise<void> {
    const parsed = splitFolderAddress(address);
    addressInvalid = parsed === null;
    if (parsed === null || busy) return;
    busy = true;
    const out = await openPreview(handle, parsed);
    busy = false;
    if (!out.ok) {
      error = out.error;
      retry = () => void cont();
      return;
    }
    error = null;
    folder = parsed;
    preview = out.value;
    name = out.value.name;
  }

  async function open(): Promise<void> {
    if (!canOpen || folder === null) return;
    busy = true;
    const out = await openWorkspace(handle, folder, name.trim());
    busy = false;
    if (!out.ok) {
      error = out.error;
      retry = () => void open();
      return;
    }
    error = null;
    onclose();
    onopened(out.value.workspace_id);
  }

  /** A place with its reach after it, in brackets (chosen here); `will_link` adds nothing. */
  function placeWithReach(place: TeamPlaceLine): string {
    const reach = place.reach ? REACH_MESSAGE[place.reach] : null;
    return reach === null ? place.display_name : `${place.display_name} (${t(reach)})`;
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
      void (preview === null ? cont() : open());
    }}
  >
    <h2 id={headingId} bind:this={heading} tabindex="-1">{t('team-ws-open-title')}</h2>
    {#if preview === null}
      <label>
        {t('team-ws-open-address')}
        <input type="text" bind:value={address} aria-invalid={addressInvalid} aria-describedby={addressInvalid ? `${headingId}-invalid` : undefined} />
      </label>
      {#if addressInvalid}<p id="{headingId}-invalid">{t('team-share-dialog-address-invalid')}</p>{/if}
    {:else}
      <label>
        {t('team-ws-open-name')}
        <input type="text" maxlength="80" bind:value={name} aria-invalid={nameTaken} aria-describedby={nameTaken ? `${headingId}-name` : undefined} />
      </label>
      {#if nameTaken}<p id="{headingId}-name">{t('team-ws-open-name-in-use')}</p>{/if}
      <div class="preview">
        {#if preview.places.length > 0}
          <p>{t('team-ws-preview-places', { places: formatList(preview.places.map(placeWithReach)) })}</p>
        {:else}
          <p>{t('team-ws-preview-no-places')}</p>
        {/if}
        <h3>{t('team-ws-preview-agents')}</h3>
        <ul>
          {#each preview.agents as agent (agent.key)}
            {@const found = agent.found ? FOUND_MESSAGE[agent.found] : null}
            <li>{t('team-ws-agent-access', { agent: agent.name, access: accessPhrase(agent.access) })}{#if found !== null}<span class="secondary">{` · ${t(found)}`}</span>{/if}</li>
          {/each}
        </ul>
        <p>{t('team-ws-preview-knowledge', { facts: preview.knowledge.new_facts, pages: preview.knowledge.new_pages })}</p>
        <p>{t('team-ws-preview-results', { place: preview.results })}</p>
        <p>{t('team-ws-open-intro')}</p>
      </div>
    {/if}

    <ErrorLine {error} onretry={() => retry?.()} {onsignin} />

    <div class="dialog-buttons">
      <button type="button" bind:this={cancelButton} onclick={onclose}>{t('confirm-cancel')}</button>
      {#if preview === null}
        <button type="submit" class="go" disabled={busy || address.trim() === ''}>{t('team-ws-open-continue')}</button>
      {:else}
        <button type="submit" class="go" disabled={!canOpen}>{t('team-ws-open-verb')}</button>
      {/if}
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
  .preview p,
  .preview ul {
    margin: var(--space-04) 0;
  }
  .preview h3 {
    margin: var(--space-06) 0 0;
    font-size: 13px;
  }
  .secondary {
    color: var(--color-text-muted);
  }
</style>
