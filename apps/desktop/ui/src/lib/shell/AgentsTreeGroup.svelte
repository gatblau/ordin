<script lang="ts">
  // The Agents row under a workspace (the prototype's `ag:` group): its main action opens the Agents screen
  // and shows, under it, one row per installed agent, each opening to its tasks. The group's ⋯ menu holds
  // **Browse the agent library**; an agent's ⋯ menu holds **What it's made of** and **Remove…** (asks first,
  // then `agents.remove`). A task row, drawn by its title, opens the task's form in the centre; it writes
  // nothing to the conversation and sends no command (SM-AT-2).
  //
  // Amended January 2026 (user decision; Approved by the user January 2026): the Agents row opens and closes
  // the list and opens no screen of its own; an agent row opens that agent's page (`agents:<id>`, SM-AG-6)
  // as well as showing its tasks under it. The side tree is the one list of agents.
  //
  // SM-AT-1 (amended January 2026; user decision; Approved by the user January 2026): the Agents row draws no
  // count of agents; a plain count is clutter, and only a count that calls for the person (waiting) is drawn.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-AG-6
  // spec: ordin-desktop/components/ScreensModule#SM-AT-1
  // spec: ordin-desktop/components/ScreensModule#SM-AT-2
  import { answers } from '../backend/groups';
  import type { ServiceError } from '../backend/backend';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { InstalledAgent } from '../service-api/types.gen';
  import type { BackendState } from '../store/backend-state.svelte';
  import ConfirmDialog from '../views/ConfirmDialog.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import { appOrNull } from '../views/fixed/util';
  import { INSTALLED_FILES_SCREEN } from '../views/parts/register';
  import { TASK_FORM_SCREEN } from '../views/tasks/ids';
  import type { TaskForm } from '../service-api/types.gen';
  import { openScreen } from './open-item';
  import { ScreenAreaState } from './screen-area.svelte';
  import { makeKey, SCREEN_IDS } from './screens';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    initiallyOpen?: boolean;
  }
  let { backend, workspaceId, initiallyOpen = false }: Props = $props();

  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  let open = $state(initiallyOpen);
  let agents = $state.raw<InstalledAgent[]>([]);
  let tasksOpen = $state<string[]>([]);
  /** The row whose ⋯ menu is open: `group` or an agent's id. */
  let menuFor = $state<string | null>(null);
  let removing = $state.raw<InstalledAgent | null>(null);
  let error = $state.raw<ServiceError | null>(null);
  const revision = $derived(backend.workspaceRevision(workspaceId));
  const canRemove = $derived(answers(backend, 'agents.remove'));
  const canMadeOf = $derived(answers(backend, 'agents.installed_files'));
  const canStart = $derived(answers(backend, 'runs.start_task'));

  async function load(): Promise<void> {
    const out = await backend.handle.call('agents.list', { workspace_id: workspaceId });
    if (out.kind === 'ok') {
      agents = out.result;
      error = null;
      // SM-OS-3: an agent removed elsewhere closes its tabs once the list no longer has it.
      app?.screens.closeMissing(ScreenAreaState.scope(backend.id, workspaceId), 'agent', agents.map((a) => a.instance.id));
    } else error = out.error;
  }

  $effect(() => {
    void revision;
    if (open) void load();
  });

  function openAgents(): void {
    open = !open;
  }

  function browseLibrary(): void {
    menuFor = null;
    if (app !== null) openScreen(app, backend.id, workspaceId, SCREEN_IDS.library);
  }

  function toggleTasks(agent: InstalledAgent): void {
    menuFor = null;
    const id = agent.instance.id;
    tasksOpen = tasksOpen.includes(id) ? tasksOpen.filter((k) => k !== id) : [...tasksOpen, id];
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.agents, id), { title: agent.instance.display_name });
  }

  function madeOf(agent: InstalledAgent): void {
    menuFor = null;
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(INSTALLED_FILES_SCREEN, agent.instance.id), { title: agent.instance.display_name });
  }

  function askRemove(agent: InstalledAgent): void {
    menuFor = null;
    removing = agent;
    error = null;
  }

  async function confirmRemove(): Promise<void> {
    const agent = removing;
    if (agent === null) return;
    const out = await backend.handle.call('agents.remove', { agent_instance_id: agent.instance.id, workspace_id: workspaceId });
    if (out.kind === 'error') {
      error = out.error;
      return;
    }
    removing = null;
    app?.screens.closeScreensFor('agent', agent.instance.id);
    app?.shell.announce(t('agent-removed', { agent: agent.instance.display_name }));
    await load();
  }

  function openTask(agent: InstalledAgent, form: TaskForm): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(TASK_FORM_SCREEN, `${agent.instance.id}:${form.command_id}`), { title: form.title, inFront: true });
  }

  function focusFirst(node: HTMLElement): void {
    node.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  function menuKey(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    menuFor = null;
  }
</script>

<div role="treeitem" aria-selected="false" aria-expanded={open} tabindex="-1" class="tree-leaf">
  <div class="tree-line">
    <button type="button" class="tree-row" onclick={openAgents}><span class="icon-tile"><Icon name="agent" /></span>{t('tree-agents')}</button>
    <span class="tree-actions" class:shown={menuFor === 'group'}>
      <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuFor === 'group'} aria-label={t('more-actions-for', { name: t('tree-agents') })} title={t('more-actions-tip')} onclick={() => (menuFor = menuFor === 'group' ? null : 'group')}><Icon name="more" /></button>
    </span>
    {#if menuFor === 'group'}
      <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name: t('tree-agents') })} onkeydown={menuKey} use:focusFirst>
        <button type="button" role="menuitem" onclick={browseLibrary}><Icon name="plus" />{t('library-browse')}</button>
      </div>
    {/if}
  </div>
  {#if open}
    <ErrorLine {error} onretry={() => void load()} />
    <div role="group" class="tree-group">
      {#each agents as agent (agent.instance.id)}
        {@const id = agent.instance.id}
        {@const name = agent.instance.display_name}
        <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf" aria-expanded={(agent.task_forms ?? []).length > 0 ? tasksOpen.includes(id) : undefined}>
          <div class="tree-line">
            <button type="button" class="tree-row leaf tree-agent" onclick={() => toggleTasks(agent)} oncontextmenu={(e) => { if (!(canMadeOf || canRemove)) return; e.preventDefault(); menuFor = id; }}><span class="icon-tile" data-tint="agent"><Icon name="agent" /></span><span class="tree-agent-name">{name}</span></button>
            {#if canMadeOf || canRemove}
              <span class="tree-actions" class:shown={menuFor === id}>
                <button type="button" class="tree-action" aria-haspopup="menu" aria-expanded={menuFor === id} aria-label={t('more-actions-for', { name })} title={t('more-actions-tip')} onclick={() => (menuFor = menuFor === id ? null : id)}><Icon name="more" /></button>
              </span>
              {#if menuFor === id}
                <div role="menu" tabindex="-1" class="tree-menu-list" aria-label={t('tree-node-menu', { name })} onkeydown={menuKey} use:focusFirst>
                  {#if canMadeOf}<button type="button" role="menuitem" onclick={() => madeOf(agent)}><Icon name="task" />{t('tree-agent-made-of')}</button>{/if}
                  {#if canRemove}<button type="button" role="menuitem" class="danger" onclick={() => askRemove(agent)}><Icon name="remove" />{t('agent-remove', { agent: name })}</button>{/if}
                </div>
              {/if}
            {/if}
          </div>
          {#if tasksOpen.includes(id)}
            <div role="group" class="tree-group">
              {#each agent.task_forms ?? [] as form (form.command_id)}
                <div role="treeitem" aria-selected="false" tabindex="-1" class="tree-leaf">
                  {#if canStart}
                    <button type="button" class="tree-row leaf tree-task" onclick={() => openTask(agent, form)}><span class="icon-tile"><Icon name="task" /></span><span class="tree-agent-name">{form.title}</span></button>
                  {:else}
                    <span class="tree-row leaf tree-task"><span class="icon-tile"><Icon name="task" /></span><span class="tree-agent-name">{form.title}</span></span>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if removing !== null}
  {@const agent = removing}
  <ConfirmDialog
    title={t('agent-remove-title', { agent: agent.instance.display_name })}
    body={t('agent-remove-body', { agent: agent.instance.display_name })}
    verb={t('agent-remove-verb')}
    onconfirm={() => void confirmRemove()}
    oncancel={() => (removing = null)}
  >
    <ErrorLine {error} />
  </ConfirmDialog>
{/if}

<style>
  .tree-agent {
    padding-inline-start: calc(2px + 28px + 23px);
  }
  .tree-task {
    padding-inline-start: calc(2px + 28px + 23px + 23px);
  }
  .tree-agent-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
