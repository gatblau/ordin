<script lang="ts">
  // One reference as a one-line card: the file's name (from its path) and **Open** when the reference names a
  // place's file. A reference is never a link: Open asks the shell to open the document screen. Open on a
  // Markdown document draws `md` (V8 *Actions*).
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import { fileKindIcon } from '../icons/kinds';
  import type { ContentRef } from '../service-api/types.gen';
  import { getApp } from '../shell/context';
  import { openTarget } from '../shell/open-item';
  import type { BackendState } from '../store/backend-state.svelte';
  import { refName } from './refs';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    reference: ContentRef;
  }
  let { backend, workspaceId, reference }: Props = $props();
  const app = getApp();
  const name = $derived(refName(reference));
  const markdown = $derived(name !== null && fileKindIcon(name) === 'md');

  function open(): void {
    if (reference.kind !== 'place_file') return;
    openTarget(app, backend.id, workspaceId, { kind: 'document', place_id: reference.place_id, path: reference.relative_path, workspace_id: workspaceId });
  }
</script>

<span class="ref-line">
  <span class="ref-name">{name ?? t('block-file-unnamed')}</span>
  {#if reference.kind === 'place_file'}<button type="button" class="small" onclick={open}>{#if markdown}<Icon name="md" />{/if}{t('card-open')}</button>{/if}
</span>

<style>
  .ref-line {
    display: flex;
    gap: var(--space-sm);
    align-items: center;
    inline-size: 100%;
  }
  .ref-name {
    flex: 1;
    min-inline-size: 0;
    overflow-wrap: anywhere;
  }
</style>
