<script lang="ts">
  // The sharing parts of a workspace's row in the tree (SM-TW-13, amended January 2026, user decision;
  // Approved by the user January 2026). The row holds no share control: **Share with your team…**,
  // **Shared with your team** and **Leave the team workspace** live in the row's ⋯ menu (WorkspaceTree.svelte).
  // This part reads the status when the row is drawn and on every `workspace_changed` for the workspace,
  // draws a team workspace's small team icon (not a control; its accessible name `tree-shared`), the card
  // **Shared with your team** opens, and the dialogs the menu opens.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TW-13
  // spec: ordin-desktop/components/ScreensModule#SM-TW-12
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { BackendState } from '../store/backend-state.svelte';
  import ConfirmDialog from '../views/ConfirmDialog.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import TeamWorkspaceShareDialog from '../views/fixed/TeamWorkspaceShareDialog.svelte';
  import { getApp } from './context';
  import { openScreen } from './open-item';
  import { makeKey, SCREEN_IDS } from './screens';
  import { sharingOffered, type WorkspaceShare } from './workspace-share.svelte';

  interface Props {
    backend: BackendState;
    share: WorkspaceShare;
    /** The workspace's name, for the dialogs' titles. */
    workspaceName: string;
    /** Where the card and the dialogs are drawn: `mark` on the row, `panels` below it. */
    part: 'mark' | 'panels';
  }
  let { backend, share, workspaceName, part }: Props = $props();

  const app = getApp();
  const offered = $derived(sharingOffered(backend));
  const revision = $derived(backend.workspaceRevision(share.controller.workspaceId));
  const c = $derived(share.controller);

  // Read when the row is drawn (or the groups arrive) and again on every `workspace_changed` for it.
  $effect(() => {
    if (part !== 'mark') return;
    void revision;
    if (offered) void c.load();
  });
</script>

{#if offered && part === 'mark'}
  {#if share.shared}
    <span class="team-mark" role="img" aria-label={t('tree-shared')} title={t('tree-shared')}><Icon name="crew" size="small" /></span>
  {/if}
  <span class="visually-hidden" aria-live="polite" role="status">{c.announcer.text}</span>
{:else if offered && part === 'panels'}
  {#if share.showing && share.shared}
    <div class="shared-card" role="group" aria-label={t('tree-shared')} tabindex="-1" onkeydown={(e) => { if (e.key === 'Escape') share.showing = false; }}>
      <p>{t('team-ws-header', { folder: share.folder })}</p>
      <p>{t('team-ws-shared-with', { folder: share.folder })}</p>
    </div>
  {/if}
  {#if share.sharing}
    <TeamWorkspaceShareDialog
      controller={c}
      {workspaceName}
      onsignin={() => openScreen(app, app.local.id, null, makeKey(SCREEN_IDS.settings, 'sign-ins'))}
      onclose={() => (share.sharing = false)}
    />
  {/if}
  {#if share.leaving}
    <ConfirmDialog
      title={t('team-ws-leave-title', { workspace: workspaceName })}
      body={t('team-ws-leave-body')}
      verb={t('team-ws-leave-verb')}
      initialFocus="cancel"
      onconfirm={() => void c.leaveWorkspace().then((ok) => ok && (share.leaving = false))}
      oncancel={() => {
        c.error = null;
        share.leaving = false;
      }}
    >
      <ErrorLine error={c.error} />
    </ConfirmDialog>
  {/if}
{/if}

<style>
  .team-mark {
    display: inline-flex;
    flex: none;
    color: var(--color-text-muted);
  }
  .shared-card {
    padding: var(--space-04) var(--space-10);
    padding-inline-start: calc(2px + 18px + 5px + 22px);
    font-size: 13px;
    color: var(--color-text-muted);
  }
  .shared-card p {
    margin: var(--space-04) 0;
  }
</style>
