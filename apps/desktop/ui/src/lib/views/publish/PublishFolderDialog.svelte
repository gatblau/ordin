<script lang="ts">
  // Publish this folder automatically… (SM-FP-1), from a folder's menu in the side tree: the person finds the
  // Confluence page to publish under (`confluence.find`), chooses one and presses **Publish**, which sends
  // `workspaces.propose_confluence_link` for this folder; the approval card that follows is opened at once.
  // Nothing is published from here.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-FP-1
  import { t } from '../../i18n/fluent';
  import type { ConfluenceLocation, ServiceError } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { foundName } from './destinations';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    placeId: string;
    /** The folder relative to the place, with no trailing `/`; empty is the place itself. */
    folder: string;
    name: string;
    /** Called with the approval id once the link is proposed (empty when none came back), `null` on Cancel. */
    onclose: (approvalId: string | null) => void;
  }
  let { backend, workspaceId, placeId, folder, name, onclose }: Props = $props();

  const FIND_LIMIT = 10;
  let query = $state('');
  let found = $state.raw<ConfluenceLocation[] | null>(null);
  let chosen = $state<number | null>(null);
  let busy = $state(false);
  let needsChoice = $state(false);
  let error = $state<ServiceError | null>(null);

  async function find(): Promise<void> {
    const text = query.trim();
    if (text.length < 2 || busy) return;
    busy = true;
    chosen = null;
    const out = await backend.handle.call('confluence.find', { query: text, limit: FIND_LIMIT });
    busy = false;
    if (out.kind === 'error') {
      error = out.error;
      found = null;
      return;
    }
    error = null;
    found = out.result.filter((l) => l.page_id !== null && l.page_id !== undefined && l.page_id !== '');
  }

  async function publish(): Promise<void> {
    if (busy) return;
    const location = chosen === null ? undefined : found?.[chosen];
    if (location === undefined) {
      needsChoice = true;
      return;
    }
    needsChoice = false;
    busy = true;
    const out = await backend.handle.call('workspaces.propose_confluence_link', { workspace_id: workspaceId, place_id: placeId, folder, location });
    busy = false;
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    error = null;
    onclose(out.result.approval_id ?? '');
  }
</script>

<ConfirmDialog
  title={t('publish-folder-title', { name })}
  body={t('publish-folder-body')}
  verb={t('publish-to-verb')}
  destructive={false}
  onconfirm={() => void publish()}
  oncancel={() => onclose(null)}
>
  <form class="publish-find" onsubmit={(e) => { e.preventDefault(); void find(); }}>
    <label>
      {t('publish-confluence-find')}
      <input type="search" bind:value={query} disabled={busy} />
    </label>
    <button type="submit" disabled={busy || query.trim().length < 2}>{t('publish-confluence-search')}</button>
  </form>
  {#if found !== null}
    {#if found.length === 0}
      <p>{t('publish-confluence-nothing')}</p>
    {:else}
      <div class="publish-found" role="radiogroup" aria-label={t('publish-confluence-find')}>
        {#each found as location, index (index)}
          <label class="found-row"><input type="radio" name="publish-folder-under" checked={chosen === index} onchange={() => { chosen = index; needsChoice = false; }} />{foundName({ kind: 'confluence', location })}</label>
        {/each}
      </div>
    {/if}
  {/if}
  {#if needsChoice}<p class="secondary" role="status">{t('publish-to-choose', { destination: 'Confluence' })}</p>{/if}
  <ErrorLine {error} />
</ConfirmDialog>

<style>
  .publish-find {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    align-items: end;
  }
  .publish-found {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
    max-block-size: 40vh;
    overflow-y: auto;
  }
  .found-row {
    display: flex;
    gap: var(--space-sm);
    align-items: center;
  }
  .secondary {
    color: var(--color-text-muted);
  }
</style>
