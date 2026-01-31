<script lang="ts">
  // Agents of a workspace, and What [agent] is made of with Check it in place and, in a workspace that links
  // code, **Show the installed files** (SM-WR-9, SM-WR-14), and **Browse the agent library** (SM-AL-1).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-WR-9
  // spec: ordin-desktop/components/ScreensModule#SM-AL-1
  // spec: ordin-desktop/components/ScreensModule#SM-FA-1
  // spec: ordin-desktop/components/ScreensModule#SM-AG-1
  // spec: ordin-desktop/components/ScreensModule#SM-AG-2
  // spec: ordin-desktop/components/ScreensModule#SM-AG-3
  import { answers } from '../../backend/groups';
  import { t } from '../../i18n/fluent';
  import type { InstalledAgent, InstalledMadeOf, ServiceError, SourceLink } from '../../service-api/types.gen';
  import { openScreen } from '../../shell/open-item';
  import { makeKey, SCREEN_IDS } from '../../shell/screens';
  import type { ScreenProps } from '../../shell/screens';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import CheckItPanel from './CheckItPanel.svelte';
  import FolderAccess from './FolderAccess.svelte';
  import { readLinksCode } from './links-code';
  import { loadAgents } from './lists.svelte';
  import LoadedList from './LoadedList.svelte';
  import { appOrNull } from './util';

  let { backend, workspaceId }: ScreenProps = $props();
  let chosen = $state<{ id: string; title: string } | null>(null);
  let madeOf = $state<InstalledMadeOf | null>(null);
  let error = $state<ServiceError | null>(null);
  /** SM-AG-2: the workspace's agents as last listed, for the chosen agent's `update`. */
  let installed = $state<InstalledAgent[]>([]);
  /** Bumped to draw the list again after a removal (SM-AG-1). */
  let listVersion = $state(0);
  let confirmRemove = $state(false);
  let removeError = $state<ServiceError | null>(null);
  let updateError = $state<ServiceError | null>(null);
  let busy = $state(false);
  let codeLinked = $state(false);
  /** SM-FA-1: the workspace's links, for the chosen agent's folder access. */
  let links = $state<SourceLink[]>([]);
  // The app context is read while the screen is set up; Svelte's getContext answers nothing inside a click handler.
  const app = appOrNull();
  const update = $derived(chosen === null ? null : (installed.find((a) => a.instance.id === chosen?.id)?.update ?? null));
  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));

  // Read when the screen opens and again on each `workspace_changed` for the workspace (SM-WR-14).
  $effect(() => {
    void revision;
    void readLinksCode(backend.handle, workspaceId).then((v) => (codeLinked = v));
    if (workspaceId !== null && answers(backend, 'workspaces.grants')) {
      void backend.handle.call('workspaces.links', { workspace_id: workspaceId }).then((out) => {
        if (out.kind === 'ok') links = out.result;
      });
    }
  });

  async function choose(row: { id: string; title: string }): Promise<void> {
    chosen = row;
    madeOf = null;
    error = null;
    updateError = null;
    confirmRemove = false;
    const out = await backend.handle.call('agents.made_of', { agent_instance_id: row.id, detail: 'business_user' });
    if (out.kind === 'ok') madeOf = out.result;
    else error = out.error;
  }

  /** SM-AG-1: **Remove** sends `agents.remove`; on `ok` the list is read again and the section closes. */
  async function remove(agent: { id: string; title: string }): Promise<void> {
    if (workspaceId === null) return;
    const out = await backend.handle.call('agents.remove', { agent_instance_id: agent.id, workspace_id: workspaceId });
    if (out.kind === 'error') {
      removeError = out.error;
      return;
    }
    confirmRemove = false;
    removeError = null;
    chosen = null;
    madeOf = null;
    listVersion += 1;
    app?.shell.announce(t('agent-removed', { agent: agent.title }));
  }

  /** SM-AG-2: **Update** and **Not now**; the reply is the agent's new state, drawn in place of the old. */
  async function answerUpdate(agent: { id: string }, accept: boolean): Promise<void> {
    busy = true;
    const out = accept
      ? await backend.handle.call('agents.accept_update', { agent_instance_id: agent.id })
      : await backend.handle.call('agents.decline_update', { agent_instance_id: agent.id });
    busy = false;
    if (out.kind === 'error') {
      updateError = out.error;
      return;
    }
    updateError = null;
    installed = installed.map((a) => (a.instance.id === agent.id ? out.result : a));
  }

  /** SM-AG-3: a change in words; an id the file lacks reads as the general sentence, never the id. */
  function changeText(c: { message_id: string; name: string }): string {
    const words = t(c.message_id, { name: c.name });
    return words === c.message_id ? t('agent-update-change-unknown') : words;
  }

  function browseLibrary(): void {
    if (app !== null) openScreen(app, backend.handle.id, workspaceId, SCREEN_IDS.library);
  }

  function showInstalledFiles(agentInstanceId: string, name: string): void {
    if (app !== null) openScreen(app, backend.handle.id, workspaceId, makeKey('installed-files', agentInstanceId), { title: name });
  }
</script>

{#if workspaceId !== null && answers(backend, 'agents.add')}
  <button type="button" onclick={browseLibrary}>{t('library-browse')}</button>
{/if}
{#key listVersion}
<LoadedList {backend} {workspaceId} heading={t('tree-agents')} load={() => loadAgents(backend.handle, workspaceId ?? '', (agents) => (installed = agents))} onopen={(row) => void choose(row)} icon="agent" tint="accent-ink" emptyId="agents-none-yet" />
{/key}
{#if chosen !== null}
  {@const agent = chosen}
  <section aria-labelledby="made-of-heading">
    <h3 id="made-of-heading">{t('agents-made-of', { agent: agent.title })}</h3>
    <ErrorLine {error} />
    {#if madeOf !== null}<p>{madeOf.what_it_does}</p>{/if}
    {#if codeLinked && answers(backend, 'agents.installed_files')}
      <button type="button" onclick={() => showInstalledFiles(agent.id, madeOf?.display_name ?? agent.title)}>{t('installed-files-show')}</button>
    {/if}
    {#if workspaceId !== null}
      <FolderAccess {backend} {workspaceId} agents={[{ id: agent.id, name: agent.title }]} {links} by="folder" />
    {/if}
    <CheckItPanel {backend} {workspaceId} subjectName={agent.title} subject={{ kind: 'installed', agentInstanceId: agent.id }} />
    {#if update !== null && answers(backend, 'agents.accept_update') && answers(backend, 'agents.decline_update')}
      <div class="update">
        <p>{t('agent-update-heading', { agent: agent.title })}</p>
        <ul>
          {#each update.may_do_changes as change (change.kind + change.name)}<li>{changeText(change)}</li>{/each}
        </ul>
        <ErrorLine error={updateError} />
        <button type="button" disabled={busy} onclick={() => void answerUpdate(agent, true)}>{t('agent-update')}</button>
        <button type="button" disabled={busy} onclick={() => void answerUpdate(agent, false)}>{t('agent-update-decline')}</button>
      </div>
    {/if}
    {#if workspaceId !== null && answers(backend, 'agents.remove')}
      <button type="button" onclick={() => { removeError = null; confirmRemove = true; }}>{t('agent-remove', { agent: agent.title })}</button>
    {/if}
  </section>
{/if}

{#if confirmRemove && chosen !== null}
  {@const agent = chosen}
  <ConfirmDialog
    title={t('agent-remove-title', { agent: agent.title })}
    body={t('agent-remove-body', { agent: agent.title })}
    verb={t('agent-remove-verb')}
    destructive
    initialFocus="cancel"
    onconfirm={() => void remove(agent)}
    oncancel={() => (confirmRemove = false)}
  >
    <ErrorLine error={removeError} />
  </ConfirmDialog>
{/if}
