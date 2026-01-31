<script lang="ts">
  // **Team activity** under a team workspace (SM-TW-7): a row like every group of the tree, drawn only when
  // the backend answers `team_knowledge.activity` and the workspace's `team_knowledge.status` has
  // `team_workspace`, read when the workspace node opens and on `workspace_changed`. The waiting grants are
  // its count, drawn as the workspace node's waiting count is. Never for a connected server (SM-TW-12).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TW-7
  // spec: ordin-desktop/components/ScreensModule#SM-TW-12
  import { onMount } from 'svelte';

  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { TeamWorkspaceStatus } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import { offersTeamWorkspace } from '../views/fixed/team-workspace.svelte';
  import { getApp } from './context';
  import { openScreen } from './open-item';
  import { SCREEN_IDS } from './screens';

  interface Props {
    backend: BackendState;
    workspaceId: string;
  }
  let { backend, workspaceId }: Props = $props();

  const app = getApp();
  // svelte-ignore state_referenced_locally
  const offered = offersTeamWorkspace(backend, 'team_knowledge.activity');
  let team = $state.raw<TeamWorkspaceStatus | null>(null);
  const revision = $derived(backend.workspaceRevision(workspaceId));
  let first = true;

  async function load(): Promise<void> {
    const out = await backend.handle.call('team_knowledge.status', { ws: workspaceId });
    if (out.kind === 'ok') team = out.result.team_workspace ?? null;
  }

  onMount(() => {
    if (offered) void load();
  });
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    if (offered) void load();
  });
</script>

{#if offered && team !== null}
  <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf">
    <button type="button" class="tree-row" onclick={() => openScreen(app, backend.id, workspaceId, SCREEN_IDS.teamActivity)}>
      <span class="icon-tile"><Icon name="history" /></span>{t('tree-team-activity')}
      {#if team.pending_grants > 0}<span class="count" aria-label={t('team-ws-grants-waiting', { count: team.pending_grants })}>{team.pending_grants}</span>{/if}
    </button>
  </div>
{/if}
