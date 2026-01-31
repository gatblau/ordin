<script lang="ts">
  // *Add to this workspace* (scenes `newWs`, `addCode`): a screen of the workspace's screen area, one per
  // workspace. It opens on *Where it lives*; SharePoint and Confluence search and add a location, a folder on
  // this computer is chosen with the system picker, Jira issue… opens the Jira dialog, and a successful add
  // draws *Added*. Not in R1 (G-AW-2): GitHub and GitLab entries.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-AW-1
  // spec: ordin-desktop/components/ScreensModule#SM-AW-2
  // spec: ordin-desktop/components/ScreensModule#SM-AW-3
  // spec: ordin-desktop/components/ScreensModule#SM-AW-4
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { IconName } from '../../icons/sprite';
  import { openScreen } from '../../shell/open-item';
  import { ScreenAreaState } from '../../shell/screen-area.svelte';
  import { makeKey, SCREEN_IDS, type ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import FolderAccess from '../fixed/FolderAccess.svelte';
  import JiraAddDialog from '../fixed/JiraAddDialog.svelte';
  import { jiraOffered } from '../fixed/jira.svelte';
  import { appOrNull } from '../fixed/util';
  import { ADD_TO_WORKSPACE_SCREEN, AddToWorkspaceModel, LINK_PROGRESS_EVERY_MS, placeAddress, placeName, type AddEntry } from './add-model.svelte';

  let { backend, workspaceId, model: given }: ScreenProps & { model?: AddToWorkspaceModel } = $props();

  // SM-FA-2: the workspace's agents, read when a link is added, for the access choice of the new folder.
  // spec: ordin-desktop/components/ScreensModule#SM-FA-2
  let addedAgents = $state<{ id: string; name: string }[]>([]);

  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const model = given ?? new AddToWorkspaceModel(backend.handle, workspaceId ?? '');
  const revision = $derived(backend.workspaceRevision(workspaceId ?? ''));

  // The entries of *Where it lives* (SM-AW-1), each drawn only where its commands are answered: Confluence,
  // a folder on this computer and Jira on the desktop's own backend only. Email and Slack have no dialog of
  // their own to open yet, so their entries are not drawn.
  const ENTRIES: ReadonlyArray<{ entry: AddEntry; name: string; line: string; icon: IconName; tint: string; offered: () => boolean }> = [
    { entry: 'sharepoint', name: 'add-sharepoint', line: 'add-sharepoint-line', icon: 'sp', tint: 'sharepoint', offered: () => answers(backend, 'sharepoint.find') },
    { entry: 'confluence', name: 'add-confluence', line: 'add-confluence-line', icon: 'cf', tint: 'confluence', offered: () => backend.kind === 'local' && answers(backend, 'confluence.find') },
    { entry: 'local', name: 'add-local', line: 'add-local-line', icon: 'folder', tint: 'local', offered: () => backend.kind === 'local' && answers(backend, 'app.pick_folder') },
    { entry: 'code', name: 'add-code', line: 'add-code-line', icon: 'code', tint: 'github', offered: () => backend.kind === 'local' && answers(backend, 'app.pick_folder') },
  ];
  const entries = $derived(ENTRIES.filter((e) => e.offered()));
  const jira = $derived(jiraOffered(backend.handle) && answers(backend, 'jira.find_issues'));
  const chosen = $derived(ENTRIES.find((e) => e.entry === model.entry) ?? null);

  // The screen is in front: its workspace is open and its tab is the front one.
  const inFront = $derived(
    app === null ||
      (app.shell.openWorkspace === workspaceId && app.screens.front(ScreenAreaState.scope(backend.id, workspaceId)) === ADD_TO_WORKSPACE_SCREEN),
  );

  onMount(() => {
    void model.loadLinks();
  });
  let first = true;
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    void model.loadLinks();
  });

  $effect(() => {
    const link = model.added;
    if (model.step !== 'added' || link === null || workspaceId === null || !answers(backend, 'agents.list')) {
      addedAgents = [];
      return;
    }
    void backend.handle.call('agents.list', { workspace_id: workspaceId }).then((out) => {
      if (out.kind === 'ok' && model.added?.id === link.id) addedAgents = out.result.map((a) => ({ id: a.instance.id, name: a.instance.display_name }));
    });
  });

  // SM-AW-4: `workspaces.link_progress` every 2 s while the screen is in front, stopping at `done`.
  $effect(() => {
    if (model.step !== 'added' || model.added === null || !inFront) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async (): Promise<void> => {
      const more = await model.pollProgress();
      if (more && !stopped) timer = setTimeout(() => void tick(), LINK_PROGRESS_EVERY_MS);
    };
    void tick();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  });

  function openSettings(section: string): void {
    if (app !== null) openScreen(app, app.local.id, null, makeKey(SCREEN_IDS.settings, section));
  }

  function showDocuments(): void {
    if (app !== null && workspaceId !== null) openScreen(app, backend.id, workspaceId, SCREEN_IDS.documents);
  }
</script>

<section class="screen add-to-workspace" aria-labelledby="add-title">
  <h2 id="add-title" tabindex="-1" data-region-focus>{t('add-title')}</h2>

  {#if model.step === 'where'}
    <p class="muted add-intro">{t('add-intro')}</p>
    <ul class="list-rows" aria-labelledby="add-title">
      {#each entries as e (e.entry)}
        <li>
          <button type="button" class="list-row add-entry" onclick={() => model.choose(e.entry)}>
            <span class="icon-tile" data-tint={e.tint}><Icon name={e.icon} /></span>
            <span class="grow"><b>{t(e.name)}</b><small>{t(e.line)}</small></span>
            {#if model.linked(e.entry)}<span class="state-label done"><span aria-hidden="true">✓</span>{t('add-linked')}</span>{/if}
          </button>
        </li>
      {/each}
      {#if jira}
        <li>
          <button type="button" class="list-row add-entry" onclick={() => model.openJira()}>
            <span class="icon-tile"><Icon name="task" /></span>
            <span class="grow"><b>{t('jira-add-to-workspace')}</b></span>
            {#if model.linked('jira')}<span class="state-label done"><span aria-hidden="true">✓</span>{t('add-linked')}</span>{/if}
          </button>
        </li>
      {/if}
    </ul>
  {:else if model.step === 'search'}
    <label class="add-search">
      {t('add-search-label')}
      <input type="search" value={model.query} oninput={(e) => model.type(e.currentTarget.value)} />
    </label>
    {#if model.errorAt === 'field'}<ErrorLine error={model.error} onretry={() => void model.search()} onsignin={() => openSettings('sign-ins')} />{/if}
    {#if model.results.length > 0}
      <ul class="list-rows" aria-label={t('add-search-label')}>
        {#each model.results as place (placeAddress(place))}
          <li class="list-row">
            <span class="icon-tile" data-tint={place.kind}><Icon name={place.kind === 'sharepoint' ? 'sp' : 'cf'} /></span>
            <span class="grow"><b>{placeName(place)}</b><small>{placeAddress(place)}</small></span>
            <button type="button" class="small" onclick={() => void model.addPlace(place)}>{t('add-add')}</button>
          </li>
        {/each}
      </ul>
    {/if}
    {#if model.errorAt === 'add'}<ErrorLine error={model.error} onsignin={() => openSettings('sign-ins')} />{/if}
    <div class="add-row"><button type="button" class="small" onclick={() => model.back()}>{t('add-back')}</button></div>
  {:else if model.step === 'sign_in'}
    {@const heading = model.signInKind === 'confluence' ? t('add-sign-in-confluence') : t('add-sign-in-microsoft')}
    <div class="add-panel">
      <h3>{heading}</h3>
      <p>{t('add-sign-in-line')}</p>
      <div class="add-row">
        <button type="button" class="primary" disabled={model.signingIn} onclick={() => void model.signIn(t('add-sign-in-reason'))}>{heading}</button>
        <button type="button" class="small" onclick={() => model.back()}>{t('add-back')}</button>
      </div>
      <ErrorLine error={model.error} onretry={() => void model.signIn(t('add-sign-in-reason'))} />
    </div>
  {:else if model.step === 'not_configured'}
    <div class="add-panel">
      <p>{t('add-confluence-not-configured')}</p>
      <div class="add-row">
        <button type="button" onclick={() => openSettings('sign-ins')}>{t('add-open-settings')}</button>
        <button type="button" class="small" onclick={() => model.back()}>{t('add-back')}</button>
      </div>
    </div>
  {:else if model.step === 'folder' && chosen !== null}
    <div class="add-panel">
      <h3><span class="icon-tile" data-tint={chosen.tint}><Icon name={chosen.icon} /></span>{t(chosen.name)}</h3>
      <p class="muted small">{t(chosen.line)}</p>
      <div class="add-row">
        <button type="button" class="primary" onclick={() => void model.chooseFolder()}>{t('add-choose-folder')}</button>
        <button type="button" class="small" onclick={() => model.back()}>{t('add-back')}</button>
      </div>
      <ErrorLine error={model.error} />
    </div>
  {:else if model.step === 'jira' && workspaceId !== null}
    <div class="add-panel">
      <JiraAddDialog {backend} {workspaceId} onadded={(link) => model.jiraAdded(link)} onclose={() => model.back()} onopensettings={() => openSettings('jira')} />
    </div>
  {:else if model.step === 'added' && model.added !== null}
    <div class="add-panel">
      <h3>{t('add-added')}</h3>
      <p>{t('add-added-line', { name: model.added.display_name })}</p>
      {#if model.progress !== null && !model.progress.done}
        <p class="muted small" role="status">{t('add-reading', { read: model.progress.read, listed: model.progress.listed })}</p>
      {/if}
      <!-- SM-FA-2: who may use the new folder, for the agents already in the workspace. -->
      {#if workspaceId !== null && addedAgents.length > 0}
        <FolderAccess {backend} {workspaceId} agents={addedAgents} links={[model.added]} by="agent" />
      {/if}
      <div class="add-row">
        <button type="button" onclick={showDocuments}>{t('add-show-documents')}</button>
        <button type="button" onclick={() => model.back()}>{t('add-more')}</button>
      </div>
    </div>
  {/if}
</section>

<style>
  /* Add to this workspace (scenes `newWs`, `addCode`): the prototype's `.libl` rows and `.libd` panel. */
  .add-intro {
    font-size: 16px;
  }
  .add-entry {
    inline-size: 100%;
    min-height: 0;
    text-align: start;
    font-size: 14px;
  }
  .add-entry:hover {
    border-color: var(--color-line-strong);
  }
  .add-entry b {
    font-size: 16px;
  }
  .add-entry .icon-tile {
    padding-block-start: 3px;
  }
  .add-entry .state-label {
    margin-inline-start: auto;
  }
  .add-search {
    display: flex;
    flex-direction: column;
    gap: var(--space-04);
  }
  .add-panel {
    background: var(--color-surface);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-lg);
    padding: var(--space-14);
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .add-panel h3 {
    display: flex;
    align-items: center;
    gap: var(--space-06);
    font-size: 16px;
    margin: 0;
  }
  .add-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-sm);
  }
</style>
