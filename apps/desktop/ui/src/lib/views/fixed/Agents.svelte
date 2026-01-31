<script lang="ts">
  // The page of one agent (SM-AG-7; instance = the agent instance id), opened from its row under Agents in
  // the side tree (SM-AG-6): what it does, a waiting update (SM-AG-2), its recent tasks, its choices, what
  // it can use (SM-FA-1) and Check it in place (SM-WR-9). Its actions — **Show the installed files**
  // (SM-WR-14) and **Remove [agent]…** (SM-AG-1) — are in the page's ⋯ menu (SM-AG-8). With no instance
  // the page lists nothing: the side tree is the one list of agents (SM-AG-6).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WR-9
  // spec: ordin-desktop/components/ScreensModule#SM-FA-1
  // spec: ordin-desktop/components/ScreensModule#SM-AG-1
  // spec: ordin-desktop/components/ScreensModule#SM-AG-2
  // spec: ordin-desktop/components/ScreensModule#SM-AG-3
  // spec: ordin-desktop/components/ScreensModule#SM-AG-7
  // spec: ordin-desktop/components/ScreensModule#SM-AG-8
  import { answers } from '../../backend/groups';
  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import { stateMessageId } from '../../i18n/run-state';
  import Icon from '../../icons/Icon.svelte';
  import type { ServiceError, SourceLink } from '../../service-api/types.gen';
  import { openScreen } from '../../shell/open-item';
  import { makeKey, SCREEN_IDS } from '../../shell/screens';
  import type { ScreenProps } from '../../shell/screens';
  import { PART_KIND_MESSAGE } from '../authoring/support';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { AgentPage } from './agent-page.svelte';
  import CheckItPanel from './CheckItPanel.svelte';
  import FolderAccess from './FolderAccess.svelte';
  import { readLinksCode } from './links-code';
  import { appOrNull } from './util';

  let { backend, workspaceId, instance }: ScreenProps = $props();
  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const page = instance === null || workspaceId === null ? null : new AgentPage(backend.handle, workspaceId, instance, answers(backend, 'runs.list'));
  const name = $derived(page?.agent?.instance.display_name ?? page?.madeOf?.display_name ?? '');
  const update = $derived(page?.agent?.update ?? null);
  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));

  let menuOpen = $state(false);
  let confirmRemove = $state(false);
  let removeError = $state<ServiceError | null>(null);
  let updateError = $state<ServiceError | null>(null);
  let busy = $state(false);
  let codeLinked = $state(false);
  /** SM-FA-1: the workspace's links, for the agent's folder access. */
  let links = $state<SourceLink[]>([]);

  // Read when the page opens and again on each `workspace_changed` for the workspace (SM-WR-14).
  $effect(() => {
    void revision;
    if (page === null) return;
    void page.load();
    void readLinksCode(backend.handle, workspaceId).then((v) => (codeLinked = v));
    if (workspaceId !== null && answers(backend, 'workspaces.grants')) {
      void backend.handle.call('workspaces.links', { workspace_id: workspaceId }).then((out) => {
        if (out.kind === 'ok') links = out.result;
      });
    }
  });

  const canShowFiles = $derived(codeLinked && answers(backend, 'agents.installed_files'));
  const canRemove = $derived(workspaceId !== null && answers(backend, 'agents.remove'));

  /** SM-AG-1: **Remove** sends `agents.remove`; on `ok` the page closes and the tree reads its agents again. */
  async function remove(): Promise<void> {
    if (workspaceId === null || page === null) return;
    const out = await backend.handle.call('agents.remove', { agent_instance_id: page.agentInstanceId, workspace_id: workspaceId });
    if (out.kind === 'error') {
      removeError = out.error;
      return;
    }
    confirmRemove = false;
    removeError = null;
    app?.shell.announce(t('agent-removed', { agent: name }));
    void backend.loadTree(workspaceId);
    // SM-OS-3: the agent's page and its other tabs (What it's made of, its task forms) close.
    app?.screens.closeScreensFor('agent', page.agentInstanceId);
  }

  /** SM-AG-2: **Update** and **Not now**; the reply is the agent's new state, drawn in place of the old. */
  async function answerUpdate(accept: boolean): Promise<void> {
    if (page === null) return;
    busy = true;
    const id = { agent_instance_id: page.agentInstanceId };
    const out = accept ? await backend.handle.call('agents.accept_update', id) : await backend.handle.call('agents.decline_update', id);
    busy = false;
    if (out.kind === 'error') {
      updateError = out.error;
      return;
    }
    updateError = null;
    page.replace(out.result);
  }

  /** SM-AG-3: a change in words; an id the file lacks reads as the general sentence, never the id. */
  function changeText(c: { message_id: string; name: string }): string {
    const words = t(c.message_id, { name: c.name });
    return words === c.message_id ? t('agent-update-change-unknown') : words;
  }

  function showInstalledFiles(): void {
    menuOpen = false;
    if (app !== null && page !== null) openScreen(app, backend.handle.id, workspaceId, makeKey('installed-files', page.agentInstanceId), { title: name });
  }

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }
</script>

{#if page === null}
  <p>{t('agents-choose-in-tree')}</p>
{:else}
  <section class="agent-page" aria-labelledby="agent-heading">
    <div class="heading tree-line">
      <span class="icon-tile" data-tint="accent-ink"><Icon name="agent" size="heading" /></span>
      <h2 id="agent-heading" tabindex="-1" data-region-focus>{name}</h2>
      {#if canShowFiles || canRemove}
        <span class="tree-actions shown">
          <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuOpen} aria-label={t('more-actions-for', { name })} title={t('more-actions-tip')} onclick={() => (menuOpen = !menuOpen)}><Icon name="more" /></button>
        </span>
        {#if menuOpen}
          <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name })} use:focusFirst onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); menuOpen = false; } }}>
            {#if canShowFiles}<button type="button" role="menuitem" onclick={showInstalledFiles}><Icon name="open" />{t('installed-files-show')}</button>{/if}
            {#if canRemove}<button type="button" role="menuitem" class="danger" onclick={() => { menuOpen = false; removeError = null; confirmRemove = true; }}><Icon name="remove" />{t('agent-remove', { agent: name })}</button>{/if}
          </div>
        {/if}
      {/if}
    </div>
    <ErrorLine error={page.error} onretry={() => void page.load()} />
    {#if page.loaded && page.agent === null && page.error === null}<p>{t('agent-page-unknown')}</p>{/if}

    {#if page.madeOf !== null}
      <h3>{t('agent-page-what')}</h3>
      <p>{page.madeOf.what_it_does}</p>
    {/if}

    {#if update !== null && answers(backend, 'agents.accept_update') && answers(backend, 'agents.decline_update')}
      <div class="update" role="group" aria-label={t('agent-update-heading', { agent: name })}>
        <p>{t('agent-update-heading', { agent: name })}</p>
        <ul>
          {#each update.may_do_changes as change (change.kind + change.name)}<li>{changeText(change)}</li>{/each}
        </ul>
        <ErrorLine error={updateError} />
        <button type="button" disabled={busy} onclick={() => void answerUpdate(true)}>{t('agent-update')}</button>
        <button type="button" disabled={busy} onclick={() => void answerUpdate(false)}>{t('agent-update-decline')}</button>
      </div>
    {/if}

    {#if page.agent !== null && answers(backend, 'runs.list')}
      <h3>{t('agent-page-tasks')}</h3>
      {#if page.tasks.length === 0}
        <p class="secondary">{t('agent-page-no-tasks')}</p>
      {:else}
        <ul class="plain">
          {#each page.tasks as run (run.id)}
            <li><button type="button" class="link" onclick={() => { if (app !== null) openScreen(app, backend.handle.id, workspaceId, makeKey(SCREEN_IDS.run, run.id)); }}>{t('agent-page-task-line', { date: formatDate(run.queued_at), state: t(stateMessageId(run.state)) })}</button></li>
          {/each}
        </ul>
      {/if}
    {/if}

    {#if page.agent !== null}
      <h3>{t('agent-page-choices')}</h3>
      {#if page.agent.uses.length === 0}
        <p class="secondary">{t('agent-page-no-choices')}</p>
      {:else}
        <ul class="plain">
          {#each page.agent.uses as use (use.identifier)}
            <li>{t(use.changeable ? 'agent-page-choice-line' : 'agent-page-choice-fixed', { kind: t(PART_KIND_MESSAGE[use.kind]), label: use.label })}</li>
          {/each}
        </ul>
      {/if}
    {/if}

    {#if workspaceId !== null && page.agent !== null}
      <FolderAccess {backend} {workspaceId} agents={[{ id: page.agentInstanceId, name }]} {links} by="folder" />
      <CheckItPanel {backend} {workspaceId} subjectName={name} subject={{ kind: 'installed', agentInstanceId: page.agentInstanceId }} />
    {/if}
  </section>

  {#if confirmRemove}
    <ConfirmDialog
      title={t('agent-remove-title', { agent: name })}
      body={t('agent-remove-body', { agent: name })}
      verb={t('agent-remove-verb')}
      destructive
      initialFocus="cancel"
      onconfirm={() => void remove()}
      oncancel={() => (confirmRemove = false)}
    >
      <ErrorLine error={removeError} />
    </ConfirmDialog>
  {/if}
{/if}

<style>
  .agent-page {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .heading {
    display: flex;
    align-items: center;
    gap: var(--space-12);
    position: relative;
  }
  .heading h2 {
    flex: 1;
    margin: 0;
  }
  h3 {
    margin: var(--space-sm) 0 0;
    font-size: 15px;
  }
  .plain {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    min-height: 0;
    text-align: start;
    color: var(--color-accent-ink, inherit);
  }
  .secondary {
    color: var(--color-text-muted);
  }
</style>
