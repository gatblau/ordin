<script lang="ts">
  // The fixed first tab of the conversation panel: every waiting item grouped by workspace, the open
  // workspace first, "On [server]" for a server's. Choosing one opens its workspace and item; focus goes
  // to the screen's heading, never to a button. Other modules add lines at the top (the update line,
  // What's new) by registering a slot; this component knows none of them. Each row draws `info` before
  // its words (V8 *Lists*, a Needs you row).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-IC-3
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { BackendState } from '../store/backend-state.svelte';
  import { getApp } from './context';
  import { openTarget, openScreen } from './open-item';
  import { makeKey, needsYouSlots, SCREEN_IDS } from './screens';

  interface Props {
    backends: BackendState[];
  }
  let { backends }: Props = $props();
  const app = getApp();
</script>

<div class="needs-you" role="region" aria-label={t('conv-needs-you')}>
  {#each needsYouSlots() as slot (slot.id)}
    {@const Slot = slot.component}
    <Slot backend={app.local} />
  {/each}
  {#each backends as backend (backend.id)}
    {#each backend.needsYouGroups(app.shell.openWorkspace) as group (group.workspace_id ?? 'none')}
      <section class="needs-group">
        <h3>
          {group.workspace_name ?? ''}
          {#if backend.kind === 'server'}<span class="on-server"> · {t('rail-server-on', { server: backend.displayName })}</span>{/if}
        </h3>
        <ul>
          {#each group.approvals as card (card.approval_id)}
            <li class="needs-item">
              <span class="icon-tile"><Icon name="info" /></span>
              <span class="needs-line">{card.question}</span>
              <button type="button" class="small" onclick={() => openScreen(app, backend.id, card.workspace_id, makeKey(SCREEN_IDS.approval, card.approval_id))}>
                {t('card-open')}
              </button>
            </li>
          {/each}
          {#each group.rows as row (row.opens.kind + row.line + row.at)}
            <li class="needs-item">
              <span class="icon-tile"><Icon name="info" /></span>
              <span class="needs-line">{row.line}</span>
              <button type="button" class="small" onclick={() => openTarget(app, backend.id, row.workspace_id ?? null, row.opens)}>{t('card-open')}</button>
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  {/each}
</div>

<style>
  /* Needs you (cross-cutting/VisualDesign.md V6 "Needs you item"; scene `needsTab`). */
  .needs-you {
    padding: var(--space-14) var(--space-12);
    display: flex;
    flex-direction: column;
    gap: var(--space-10);
  }
  .needs-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  h3 {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
  }
  .needs-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    border: 1px solid var(--color-waiting-line);
    background: var(--color-waiting-soft);
    color: var(--color-waiting-text);
    border-radius: var(--radius-md);
    padding: 7px var(--space-10);
    font-size: 14px;
  }
  .needs-line {
    flex: 1;
    min-width: 0;
  }
</style>
