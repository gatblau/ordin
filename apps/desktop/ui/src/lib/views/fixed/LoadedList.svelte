<script lang="ts">
  // A fixed list that loads its rows when it opens and again on each `workspace_changed` for its workspace.
  import { onMount } from 'svelte';

  import type { IconName } from '../../icons/sprite';
  import type { BackendState } from '../../store/backend-state.svelte';
  import FixedList, { type Row } from './FixedList.svelte';
  import type { Loaded } from './lists.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string | null;
    heading: string;
    load: () => Promise<Loaded>;
    onopen?: (row: Row) => void;
    icon?: IconName | undefined;
    tint?: string | undefined;
    emptyId?: string;
  }
  let { backend, workspaceId, heading, load, onopen, icon, tint, emptyId }: Props = $props();

  let loaded = $state<Loaded>({ rows: [], error: null });
  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));
  let first = true;

  async function reload(): Promise<void> {
    loaded = await load();
  }
  onMount(() => {
    void reload();
  });
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    void reload();
  });
</script>

<FixedList {backend} {heading} rows={loaded.rows} error={loaded.error} {onopen} {icon} {tint} {...emptyId !== undefined ? { emptyId } : {}} onretry={() => void reload()} />
