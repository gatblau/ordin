<script lang="ts">
  // Publish to [destination] (SM-PUB-2): the person finds the page or folder to publish under, chooses one
  // and presses **Publish**. The screens read the document's newest version (`files.versions`) and send
  // `push.publish_copy`; the Push Service raises an approval card, so nothing leaves Ordin from here. A
  // refusal is drawn in the dialog, which stays open.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-PUB-2
  import { t } from '../../i18n/fluent';
  import type { ServiceError } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { canHold, copyRequest, foundName, PUBLISH_SYSTEMS, type FoundPlace, type PublishSystem } from './destinations';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    placeId: string;
    path: string;
    name: string;
    system: PublishSystem;
    /** Called with `true` once the copy waits for the person's approval, `false` on Cancel. */
    onclose: (sent: boolean) => void;
  }
  let { backend, workspaceId, placeId, path, name, system, onclose }: Props = $props();

  const FIND_LIMIT = 10;
  let query = $state('');
  let found = $state.raw<FoundPlace[] | null>(null);
  let chosen = $state<number | null>(null);
  let busy = $state(false);
  let error = $state<ServiceError | null>(null);
  let needsChoice = $state(false);
  // svelte-ignore state_referenced_locally
  const destination = PUBLISH_SYSTEMS[system].name;

  async function find(): Promise<void> {
    const text = query.trim();
    if (text.length < 2 || busy) return;
    busy = true;
    chosen = null;
    const out =
      system === 'confluence'
        ? await backend.handle.call('confluence.find', { query: text, limit: FIND_LIMIT }).then((o) => (o.kind === 'ok' ? { kind: 'ok' as const, items: o.result.map((location): FoundPlace => ({ kind: 'confluence', location })) } : o))
        : await backend.handle.call('sharepoint.find', { query: text, limit: FIND_LIMIT }).then((o) => (o.kind === 'ok' ? { kind: 'ok' as const, items: o.result.map((location): FoundPlace => ({ kind: 'sharepoint', location })) } : o));
    busy = false;
    if (out.kind === 'error') {
      error = out.error;
      found = null;
      return;
    }
    error = null;
    found = out.items.filter(canHold);
  }

  async function publish(): Promise<void> {
    if (busy) return;
    const place = chosen === null ? undefined : found?.[chosen];
    if (place === undefined) {
      needsChoice = true;
      return;
    }
    needsChoice = false;
    busy = true;
    const versions = await backend.handle.call('files.versions', { place_id: placeId, path });
    if (versions.kind === 'error') {
      busy = false;
      error = versions.error;
      return;
    }
    const newest = versions.result.reduce((n, v) => Math.max(n, v.version), 0);
    const request = newest > 0 ? copyRequest({ workspaceId, placeId, path, name, version: newest, found: place }) : null;
    if (request === null) {
      busy = false;
      needsChoice = true;
      return;
    }
    const out = await backend.handle.call('push.publish_copy', request);
    busy = false;
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    error = null;
    onclose(true);
  }
</script>

<ConfirmDialog
  title={t('publish-to-title', { name, destination })}
  body={t('publish-to-body', { destination })}
  verb={t('publish-to-verb')}
  destructive={false}
  onconfirm={() => void publish()}
  oncancel={() => onclose(false)}
>
  <form class="publish-find" onsubmit={(e) => { e.preventDefault(); void find(); }}>
    <label>
      {t('publish-to-find', { destination })}
      <input type="search" bind:value={query} disabled={busy} />
    </label>
    <button type="submit" disabled={busy || query.trim().length < 2}>{t('publish-confluence-search')}</button>
  </form>
  {#if found !== null}
    {#if found.length === 0}
      <p>{t('publish-to-nothing', { destination })}</p>
    {:else}
      <div class="publish-found" role="radiogroup" aria-label={t('publish-to-find', { destination })}>
        {#each found as place, index (index)}
          <label class="found-row"><input type="radio" name="publish-under" checked={chosen === index} onchange={() => { chosen = index; needsChoice = false; }} />{foundName(place)}</label>
        {/each}
      </div>
    {/if}
  {/if}
  {#if needsChoice}<p class="secondary" role="status">{t('publish-to-choose', { destination })}</p>{/if}
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
