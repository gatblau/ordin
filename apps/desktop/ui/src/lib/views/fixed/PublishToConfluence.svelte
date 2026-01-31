<script lang="ts">
  // SM-PC-1 Publish this folder to Confluence: the person finds the page to publish under
  // (`confluence.find`) and chooses **Publish here**, which sends `workspaces.propose_confluence_link`
  // for this folder; the approval card that follows is opened at once. Nothing is published from here.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-PC-1
  import { t } from '../../i18n/fluent';
  import type { ConfluenceLocation, ServiceError } from '../../service-api/types.gen';
  import { openScreen } from '../../shell/open-item';
  import { makeKey, SCREEN_IDS } from '../../shell/screens';
  import type { BackendState } from '../../store/backend-state.svelte';
  import { placeName } from '../add/add-model.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { appOrNull } from './util';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    placeId: string;
    /** The folder relative to the place; empty is the place itself. */
    folder: string;
    /** Called once the link is proposed, so the screen reads its links again. */
    onproposed?: () => void;
  }
  let { backend, workspaceId, placeId, folder, onproposed }: Props = $props();
  const app = appOrNull();

  const FIND_LIMIT = 10;
  let open = $state(false);
  let query = $state('');
  let results = $state.raw<ConfluenceLocation[] | null>(null);
  let busy = $state(false);
  let error = $state<ServiceError | null>(null);

  async function find(): Promise<void> {
    const text = query.trim();
    if (text.length < 2 || busy) return;
    busy = true;
    const out = await backend.handle.call('confluence.find', { query: text, limit: FIND_LIMIT });
    busy = false;
    if (out.kind === 'error') {
      error = out.error;
      results = null;
      return;
    }
    error = null;
    results = out.result.filter((l) => l.page_id !== null && l.page_id !== undefined);
  }

  async function publishHere(location: ConfluenceLocation): Promise<void> {
    if (busy) return;
    busy = true;
    const out = await backend.handle.call('workspaces.propose_confluence_link', { workspace_id: workspaceId, place_id: placeId, folder, location });
    busy = false;
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    error = null;
    open = false;
    results = null;
    onproposed?.();
    const approval = out.result.approval_id;
    if (approval !== null && approval !== undefined && app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.approval, approval));
  }
</script>

<section class="publish-confluence" aria-labelledby="publish-confluence-heading">
  {#if !open}
    <button type="button" onclick={() => (open = true)}>{t('publish-confluence-open')}</button>
  {:else}
    <h4 id="publish-confluence-heading">{t('publish-confluence-heading')}</h4>
    <form onsubmit={(e) => { e.preventDefault(); void find(); }}>
      <label>
        {t('publish-confluence-find')}
        <input type="search" bind:value={query} disabled={busy} />
      </label>
      <button type="submit" disabled={busy || query.trim().length < 2}>{t('publish-confluence-search')}</button>
      <button type="button" onclick={() => { open = false; results = null; error = null; }}>{t('confirm-cancel')}</button>
    </form>
    {#if results !== null}
      {#if results.length === 0}
        <p>{t('publish-confluence-nothing')}</p>
      {:else}
        <ul class="list-rows">
          {#each results as location (location.page_id)}
            <li class="list-row">
              <span>{placeName({ kind: 'confluence', location })}</span>
              <button type="button" disabled={busy} onclick={() => void publishHere(location)}>{t('publish-confluence-here')}</button>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  {/if}
  <ErrorLine {error} />
</section>
