<script lang="ts">
  // The *Ready to publish* checklist (SM-SF-8): one tickable row per `ReadyItem`, its file name from `path`
  // and its kind line (`ready-kind-changed` for a changed item). A kind the screens do not know is drawn with
  // its file name and no kind line, and stays tickable, so a newer backend's item is never hidden from the
  // person. **Publish checked items…** raises an approval card; nothing is published from here.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-SF-8
  import { t } from '../i18n/fluent';
  import type { ReadyItem, ServiceError } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { isWaitingForServer } from './ask';
  import { lastSegment } from './refs';

  interface Props {
    backend: BackendState;
    confluenceLinkId: string;
    items: ReadyItem[];
    /** Called with the approval card's id once the card exists, so the caller can open it. */
    onpublished?: (approvalId: string) => void;
  }
  let { backend, confluenceLinkId, items, onpublished }: Props = $props();

  let checked = $state<Record<string, boolean>>({});
  let busy = $state(false);
  let error = $state<ServiceError | null>(null);

  const keyOf = (item: ReadyItem): string => `${item.kind}:${item.path}:${item.from_path ?? ''}`;
  // Only the kinds the catalogue has a line for; an unknown kind gets none.
  const kindLine = (item: ReadyItem): string | null => (item.kind === 'changed' ? t('ready-kind-changed') : null);
  const picked = $derived(items.filter((i) => checked[keyOf(i)] === true));
  const offered = $derived(backend.groups.has('confluence'));

  async function publish(): Promise<void> {
    if (busy || picked.length === 0 || isWaitingForServer(backend)) return;
    busy = true;
    const out = await backend.handle.call('confluence.publish_checked', { confluence_link_id: confluenceLinkId, items: picked });
    busy = false;
    error = out.kind === 'error' ? out.error : null;
    if (out.kind === 'ok') onpublished?.(out.result.approval_id);
  }
</script>

<div class="ready-checklist">
  <ul>
    {#each items as item (keyOf(item))}
      {@const line = kindLine(item)}
      <li>
        <label>
          <input type="checkbox" checked={checked[keyOf(item)] === true} onchange={(e) => (checked[keyOf(item)] = e.currentTarget.checked)} />
          <span class="name">{lastSegment(item.path)}</span>
          {#if line !== null}<span class="kind">{line}</span>{/if}
        </label>
      </li>
    {/each}
  </ul>
  {#if offered}
    <button type="button" disabled={busy || picked.length === 0 || isWaitingForServer(backend)} onclick={() => void publish()}>{t('block-publish-checked')}</button>
  {/if}
  <ErrorLine {error} />
</div>

<style>
  ul {
    list-style: none;
    padding: 0;
    display: grid;
    gap: var(--space-04);
  }
  .kind {
    color: var(--color-text-muted);
  }
</style>
