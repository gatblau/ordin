<script lang="ts">
  // Who may use which folder (SM-FA-1): for each agent and each linked folder of the workspace, one choice
  // of three — can't use, can read, can read and change (11 vocabulary). Read from `workspaces.grants`;
  // a change sends `workspaces.set_grant` at once (no access is `access: null`, which removes the grant).
  // A Jira issue is read-only, so it offers can't use and can read only.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-FA-1
  import { t } from '../../i18n/fluent';
  import type { FolderAccess, FolderGrant, ServiceError, SourceLink } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ErrorLine from '../ErrorLine.svelte';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    agents: readonly { id: string; name: string }[];
    /** The folders to offer: active links with a place. */
    links: readonly SourceLink[];
    /** Which name each row shows: the folder's (one agent, its folders) or the agent's (one folder, its agents). */
    by: 'folder' | 'agent';
  }
  let { backend, workspaceId, agents, links, by }: Props = $props();

  let grants = $state<FolderGrant[]>([]);
  let error = $state<ServiceError | null>(null);
  let busy = $state<string | null>(null);

  const places = $derived(links.filter((l) => l.state === 'active' && typeof l.place_id === 'string'));

  async function load(): Promise<void> {
    const out = await backend.handle.call('workspaces.grants', { workspace_id: workspaceId });
    if (out.kind === 'ok') {
      grants = out.result;
      error = null;
    } else {
      error = out.error;
    }
  }
  $effect(() => {
    void load();
  });

  type Choice = 'none' | FolderAccess;
  function current(agentId: string, placeId: string): Choice {
    const g = grants.find((x) => x.place_id === placeId && x.grantee.kind === 'agent_instance' && x.grantee.agent_instance_id === agentId);
    return g?.access ?? 'none';
  }

  async function choose(agentId: string, placeId: string, choice: Choice): Promise<void> {
    busy = `${agentId}:${placeId}`;
    const out = await backend.handle.call('workspaces.set_grant', {
      workspace_id: workspaceId,
      grantee: { kind: 'agent_instance', agent_instance_id: agentId },
      place_id: placeId,
      access: choice === 'none' ? null : choice,
    });
    busy = null;
    if (out.kind === 'ok') {
      error = null;
      grants = [
        ...grants.filter((g) => !(g.place_id === placeId && g.grantee.kind === 'agent_instance' && g.grantee.agent_instance_id === agentId)),
        ...(out.result === null || out.result === undefined ? [] : [out.result]),
      ];
    } else {
      error = out.error;
    }
  }
</script>

{#if places.length > 0 && agents.length > 0}
  <section class="folder-access" aria-labelledby="folder-access-heading">
    <h4 id="folder-access-heading">{t(by === 'folder' ? 'folder-access-heading' : 'folder-access-agents-heading')}</h4>
    <ul class="list-rows">
      {#each agents as agent (agent.id)}
        {#each places as link (link.id)}
          {@const placeId = link.place_id as string}
          <li class="list-row">
            <label>
              <span>{by === 'folder' ? link.display_name : agent.name}</span>
              <select
                value={current(agent.id, placeId)}
                disabled={busy === `${agent.id}:${placeId}`}
                onchange={(e) => void choose(agent.id, placeId, (e.currentTarget as HTMLSelectElement).value as Choice)}
              >
                <option value="none">{t('folder-access-none')}</option>
                <option value="read">{t('folder-access-read')}</option>
                {#if link.kind !== 'jira_issue'}<option value="read_and_change">{t('folder-access-read-and-change')}</option>{/if}
              </select>
            </label>
          </li>
        {/each}
      {/each}
    </ul>
    <ErrorLine {error} onretry={() => void load()} />
  </section>
{/if}

<style>
  .folder-access label {
    display: flex;
    gap: var(--space-sm);
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    width: 100%;
  }
</style>
