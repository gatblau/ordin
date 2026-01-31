<script lang="ts">
  // `access` (SM-FA-3, OrdinHost.md OH-ACC-1): Ordin's sentence, then a tick box per folder —
  // "[agent] can read and change [folder]", or "can read" for a folder that is read only. Ticking grants
  // that access at once with `workspaces.set_grant`; unticking removes it. The person's tick is the
  // decision: nothing goes to *Needs you* (11 §5.x *Missing access is shown, not assumed*).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-FA-3
  import { t } from '../../i18n/fluent';
  import type { FolderGrant, ServiceError } from '../../service-api/types.gen';
  import ErrorLine from '../../views/ErrorLine.svelte';
  import type { KindProps } from '../types';

  let { backend, workspaceId, block }: KindProps<'access'> = $props();

  let grants = $state<FolderGrant[]>([]);
  let error = $state<ServiceError | null>(null);
  let busy = $state<string | null>(null);

  async function load(): Promise<void> {
    const out = await backend.handle.call('workspaces.grants', { workspace_id: workspaceId });
    if (out.kind === 'ok') grants = out.result;
    else error = out.error;
  }
  $effect(() => {
    void load();
  });

  const mine = (placeId: string): FolderGrant | undefined =>
    grants.find((g) => g.place_id === placeId && g.grantee.kind === 'agent_instance' && g.grantee.agent_instance_id === block.agent_instance_id);

  async function toggle(placeId: string, readOnly: boolean, on: boolean): Promise<void> {
    busy = placeId;
    const out = await backend.handle.call('workspaces.set_grant', {
      workspace_id: workspaceId,
      grantee: { kind: 'agent_instance', agent_instance_id: block.agent_instance_id },
      place_id: placeId,
      access: on ? (readOnly ? 'read' : 'read_and_change') : null,
    });
    busy = null;
    if (out.kind === 'ok') {
      error = null;
      grants = [...grants.filter((g) => g !== mine(placeId)), ...(out.result === null || out.result === undefined ? [] : [out.result])];
    } else {
      error = out.error;
    }
  }
</script>

<div class="block-access">
  <p>{block.text}</p>
  <ul class="access-places">
    {#each block.places as place (place.place_id)}
      <li>
        <label>
          <input
            type="checkbox"
            checked={mine(place.place_id) !== undefined}
            disabled={busy === place.place_id}
            onchange={(e) => void toggle(place.place_id, place.read_only, (e.currentTarget as HTMLInputElement).checked)}
          />
          {t(place.read_only ? 'access-can-read' : 'access-can-read-and-change', { agent: block.agent_name, folder: place.name })}
        </label>
      </li>
    {/each}
  </ul>
  <ErrorLine {error} onretry={() => void load()} />
</div>

<style>
  .access-places {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .access-places label {
    display: flex;
    gap: var(--space-sm);
    align-items: center;
  }
</style>
