<script lang="ts">
  // The Team knowledge header and panel of a workspace (ADR-103, ADR-110): shown only while it applies,
  // one line each. A refusal is drawn as the owner's sentence through `ErrorLine`.
  //
  // spec: ordin-desktop/components/ScreensModule#ADR-103
  // spec: ordin-desktop/components/ScreensModule#SM-NS-13
  // spec: ordin-desktop/components/ScreensModule#SM-TK-1
  // spec: ordin-desktop/components/ScreensModule#SM-TK-2
  //
  // A team workspace (SM-TW-2, SM-TW-4, SM-TW-5, SM-TW-6, SM-TW-10, SM-TW-11) adds its header, its lines,
  // the waiting access cards and **Leave the team workspace**. Knowledge-only sharing is no longer offered
  // on its own (amended January 2026, user decision): a workspace with nothing shared shows one line that
  // points at the side tree's **Share** link (SM-TK-2), and a workspace that already shares its knowledge
  // keeps working and is offered **Share the whole workspace** (SM-TW-14), which opens the team-workspace
  // share dialog.
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { formatList } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ServiceError, TeamKnowledgeStatus, TeamProblem } from '../../service-api/types.gen';
  import { openScreen } from '../../shell/open-item';
  import { makeKey, SCREEN_IDS } from '../../shell/screens';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { TeamController, splitFolderAddress, teamErrorMessage } from './team.svelte';
  import TeamGrantCard from './TeamGrantCard.svelte';
  import { offersTeamWorkspace, PLACE_STATE_MESSAGE, TeamGrants, teamWorkspaceProblem } from './team-workspace.svelte';
  import TeamWorkspaceShareDialog from './TeamWorkspaceShareDialog.svelte';
  import { appOrNull } from './util';

  interface Props {
    backend: BackendState;
    workspaceId: string;
    /** Choosing the "facts wait for you to check them" line opens To check (or the Facts filter). */
    onopenchecked?: () => void;
    /** A status already read (the screen that holds one, and the tests); it is replaced by the first read. */
    initialStatus?: TeamKnowledgeStatus | null;
  }
  let { backend, workspaceId, onopenchecked, initialStatus = null }: Props = $props();

  const c = new TeamController(backend.handle, workspaceId);
  c.status = initialStatus;
  const revision = $derived(backend.workspaceRevision(workspaceId));
  let first = true;
  let dialog = $state<'read' | null>(null);
  let confirming = $state<'stop' | 'leave' | { reading: string; name: string } | null>(null);
  // SM-TW-2 / SM-TW-5: the share dialog, or its results radio buttons alone (**Change…**).
  let teamDialog = $state<'share' | 'results' | null>(null);
  const grants = new TeamGrants(backend.handle, workspaceId);
  const app = appOrNull();
  let pickedLink = $state('');
  let address = $state('');
  let addressInvalid = $state(false);
  // Describe this folder… (step 4): one text field per description path, the folder itself first.
  let describing = $state(false);
  let texts = $state<Record<string, string>>({});
  let describeError = $state<ServiceError | null>(null);

  // SM-UP-1: nothing is drawn or sent when the backend does not answer the team knowledge group.
  const available = answers(backend, 'team_knowledge.status');

  onMount(() => {
    if (available) void c.load();
  });
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    if (available) void c.load();
  });

  const status = $derived(c.status);
  const ownFolder = $derived(status?.binding != null);
  const lastError = $derived(status?.last_error ? teamErrorMessage(status.last_error) : null);
  const busyId = $derived(
    status?.busy === 'checking'
      ? 'team-busy-checking'
      : status?.busy === 'getting'
        ? 'team-busy-getting'
        : status?.busy === 'sharing'
          ? 'team-busy-sharing'
          : status?.busy === 'checking_recall'
            ? 'team-busy-checking-recall'
            : status?.busy === 'opening'
              ? 'team-ws-busy-opening'
              : null,
  );
  // SM-TW-12: a team workspace is drawn for the local backend only.
  const teamWs = $derived(backend.kind === 'local' ? (status?.team_workspace ?? null) : null);
  // spec: ordin-desktop/components/ScreensModule#SM-TW-4
  // spec: ordin-desktop/components/ScreensModule#SM-TW-5
  // spec: ordin-desktop/components/ScreensModule#SM-TW-10
  const offersShare = $derived(offersTeamWorkspace(backend, 'team_knowledge.share_workspace'));
  const workspaceName = $derived(backend.workspaces.find((w) => w.id === workspaceId)?.name ?? '');
  // SM-TK-2: a workspace with nothing shared (no own folder, no read folder, not a team workspace) shows
  // the one line pointing at the side tree; SM-TW-14: one that shares its knowledge only is offered the upgrade.
  // spec: ordin-desktop/components/ScreensModule#SM-TK-2
  // spec: ordin-desktop/components/ScreensModule#SM-TW-14
  const unshared = $derived(status !== null && status.binding == null && status.folders.length === 0 && teamWs === null);
  const knowledgeOnly = $derived(offersShare && status !== null && status.binding != null && teamWs === null);
  // SM-TW-6: the access cards under `team-ws-grants-waiting`, read whenever the status says some wait.
  $effect(() => {
    if (teamWs !== null && teamWs.pending_grants > 0) void grants.load();
  });

  /** SM-TW-5 **Review**: opens Team activity. */
  function review(): void {
    if (app !== null) openScreen(app, backend.id, workspaceId, makeKey(SCREEN_IDS.teamActivity));
  }

  /** The `sign_in_again` action of a refusal: Settings › Sign-ins. */
  function signIn(): void {
    if (app !== null) openScreen(app, app.local.id, null, makeKey(SCREEN_IDS.settings, 'sign-ins'));
  }
  const names = $derived(status?.available ? formatList(status.available.from.slice(0, 5)) : '');

  /** The sentence for one problem of a folder's "Not taken in" list. */
  function problemText(problem: TeamProblem): string {
    const teamWorkspace = teamWorkspaceProblem(problem);
    if (teamWorkspace !== null) return teamWorkspace;
    switch (problem.code) {
      case 'TEAM_FILE_UNREADABLE':
        return t('team-not-taken-file', { file: problem.path });
      case 'TEAM_ITEM_HELD_ELSEWHERE':
        return t('team-item-elsewhere', { file: problem.path });
      case 'TEAM_DESCRIPTION_NOT_TAKEN':
        return t('team-description-not-taken');
      default:
        return t('team-not-taken-fact');
    }
  }

  const writable = $derived(status?.binding?.access === 'read_write');
  const describeRows = $derived.by(() => {
    const given = status?.descriptions ?? [];
    const root = given.find((d) => d.path === '');
    return [{ path: '', text: root?.text ?? '' }, ...given.filter((d) => d.path !== '')];
  });

  function openDescribe(): void {
    texts = Object.fromEntries(describeRows.map((r) => [r.path, r.text]));
    describeError = null;
    describing = true;
  }

  /** **Save**: sends each description the person changed; an emptied field sends `text: null`. */
  async function saveDescriptions(): Promise<void> {
    for (const row of describeRows) {
      const now = (texts[row.path] ?? '').trim();
      if (now === row.text) continue;
      if (!(await c.setDescription(row.path, now))) {
        describeError = c.error;
        return;
      }
    }
    describeError = null;
    describing = false;
  }

  async function openDialog(kind: 'read'): Promise<void> {
    dialog = kind;
    address = '';
    pickedLink = '';
    addressInvalid = false;
    await c.loadCandidates();
  }

  async function submitDialog(): Promise<void> {
    let choice;
    if (address.trim() !== '') {
      const parsed = splitFolderAddress(address);
      if (parsed === null) {
        addressInvalid = true;
        return;
      }
      choice = { kind: 'new_folder' as const, url: parsed.url, branch: parsed.branch, path: parsed.path };
    } else if (pickedLink !== '') {
      choice = { kind: 'link' as const, source_link_id: pickedLink };
    } else {
      return;
    }
    addressInvalid = false;
    const ok = await c.addReadFolder(choice);
    if (ok) dialog = null;
  }
</script>

<!-- SM-TK-2 (amended January 2026; user decision; Approved by the user January 2026): while nothing of the
     workspace is shared the panel draws nothing; sharing is offered on the side tree's row. -->
{#if status !== null && !unshared}
  <section class="team-panel" aria-label={t('team-panel-label')}>
    {#if teamWs !== null}
      <!-- SM-TW-4: the team workspace header. -->
      <p class="team-header">{t('team-ws-header', { folder: status.header_line ?? status.binding?.folder ?? '' })}</p>
      {#if teamWs.state === 'proposed'}<p>{t('team-ws-proposed')}</p>{/if}
    {:else if status.binding}
      <p class="team-header">{t('team-header', { folder: status.header_line ?? status.binding.folder })}</p>
    {:else if status.folders.length > 0}
      <p>{t('team-no-own-folder')}</p>
    {/if}
    {#if knowledgeOnly}
      <!-- SM-TW-14: a workspace that shares its knowledge only is offered the whole workspace, in place. -->
      <p class="team-upgrade">
        {t('team-ws-share-whole-line')}
        <button type="button" onclick={() => (teamDialog = 'share')}><Icon name="publish" />{t('team-ws-share-whole')}</button>
      </p>
    {/if}

    {#if busyId !== null}
      <p role="status">{t(busyId)}</p>
    {/if}
    {#if c.outcome !== null && busyId === null}
      <!-- SM-TK-3: what the person's latest share or get carried. -->
      <p class="team-outcome">
        {c.outcome.kind === 'share' ? t('team-share-outcome', { facts: c.outcome.facts, pages: c.outcome.pages }) : t('team-get-outcome', { facts: c.outcome.facts, pages: c.outcome.pages })}
      </p>
    {/if}
    {#if status.last_error && lastError !== null}
      {#if lastError.id === 'team-nothing-to-share'}
        <p>{t('team-nothing-to-share')}</p>
      {:else}
        <ErrorLine error={{ code: status.last_error, message: t(lastError.id), ...(lastError.action ? { action: lastError.action } : {}) }} onretry={() => void c.checkNow()} />
      {/if}
    {/if}
    {#if teamWs !== null}
      <!-- SM-TW-5: the team panel's lines. -->
      {#each teamWs.places as place (place.key)}
        {@const line = PLACE_STATE_MESSAGE[place.state]}
        {#if line !== null}<p>{t(line, { place: place.display_name })}</p>{/if}
      {/each}
      {#each teamWs.agents_not_here as agent (agent.key)}
        <p>{t('team-ws-agent-not-here', { agent: agent.name })}</p>
      {/each}
      <p>
        {t('team-ws-results-in', { place: teamWs.results })}
        <button type="button" onclick={() => (teamDialog = 'results')}>{t('team-ws-change-results')}</button>
      </p>
      {#if teamWs.results_here_only}<p>{t('team-ws-results-here-only')}</p>{/if}
      {#if teamWs.unsent_results > 0}<p>{t('team-ws-unsent-results', { count: teamWs.unsent_results })}</p>{/if}
      {#if teamWs.pending_grants > 0}
        <p>
          {t('team-ws-grants-waiting', { count: teamWs.pending_grants })}
          <button type="button" onclick={review}>{t('team-ws-grants-review')}</button>
        </p>
        <ErrorLine error={grants.goneError} />
        {#each grants.list as grant (grant.grant_id)}
          <TeamGrantCard {grants} {grant} />
        {/each}
      {/if}
    {/if}
    {#if status.available && !status.busy}
      <p>
        {t('team-available', { facts: status.available.counts.new_facts, pages: status.available.counts.new_pages, people: names })}
        <button type="button" disabled={c.busy} onclick={() => void c.get()}>{t('team-get')}</button>
      </p>
    {/if}
    {#if ownFolder}
      <p>
        <button type="button" disabled={c.busy || !!status.share_unavailable} aria-describedby={status.share_unavailable ? 'team-share-unavailable' : undefined} onclick={() => void c.share()}>
          <Icon name="publish" />
          {t('team-share')}
        </button>
        {#if status.share_unavailable}
          <span id="team-share-unavailable">{t(teamErrorMessage(status.share_unavailable).id)}</span>
        {/if}
      </p>
      {#if teamWs === null}
        <!-- SM-TK-1: what a share carries, and what stays here. -->
        <p class="secondary">{t('team-share-what')}</p>
      {/if}
    {/if}
    {#if status.waiting_for_review}
      <p>{t('team-waiting-review', { repository: status.binding?.repository ?? '' })}</p>
    {/if}
    {#if status.held_for_check > 0}
      <p><button type="button" class="link" onclick={() => onopenchecked?.()}>{t('team-held-for-check', { count: status.held_for_check })}</button></p>
    {/if}
    {#if status.problems.length > 0}
      <section aria-labelledby="team-not-taken">
        <h3 id="team-not-taken">{t('team-not-taken-title')}</h3>
        <ul>
          {#each status.problems as problem (problem.path + problem.code)}
            <li>{problemText(problem)}</li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if status.folder_large}<p>{t('team-folder-large')}</p>{/if}

    {#if status.folders.length > 0}
      <section aria-labelledby="team-also-reads">
        <h3 id="team-also-reads">{t('team-also-reads')}</h3>
        {#each status.folders as folder (folder.folder_id)}
          <div class="team-folder">
            <p>
              {folder.name}
              {#if folder.set_by_company}<span class="tag">{t('team-set-by-company')}</span>{/if}
            </p>
            {#if folder.description}<p class="secondary">{folder.description}</p>{/if}
            <p>{t('team-folder-counts', { facts: folder.held.facts, pages: folder.held.pages })}</p>
            {#if folder.available}
              <p>
                {t('team-available', { facts: folder.available.counts.new_facts, pages: folder.available.counts.new_pages, people: formatList(folder.available.from.slice(0, 5)) })}
                <button type="button" disabled={c.busy} onclick={() => void c.get(folder.folder_id)}>{t('team-get')}</button>
              </p>
            {/if}
            {#if folder.blocked === 'TEAM_COMPANY_FOLDER_NO_ACCESS'}
              <ErrorLine error={{ code: folder.blocked, message: t('team-folder-no-access', { name: folder.name }) }} />
            {:else if folder.blocked === 'TEAM_REVIEW_NOT_REQUIRED'}
              <p>{t('team-folder-review-not-required', { name: folder.name })}</p>
            {:else if folder.blocked === 'TEAM_REVIEW_CANNOT_TELL'}
              <p>{t('team-folder-review-cannot-tell', { name: folder.name })}</p>
            {/if}
            {#if folder.problems.length > 0}
              <h4>{t('team-not-taken-title')}</h4>
              <ul>
                {#each folder.problems as problem (problem.path + problem.code)}<li>{problemText(problem)}</li>{/each}
              </ul>
            {/if}
            {#if folder.folder_large}<p>{t('team-folder-large')}</p>{/if}
            {#if folder.may_stop}
              <button type="button" onclick={() => (confirming = { reading: folder.folder_id, name: folder.name })}>{t('team-stop-reading')}</button>
            {/if}
          </div>
        {/each}
        {#if status.workspace_large}<p>{t('team-workspace-large')}</p>{/if}
      </section>
    {/if}

    {#if status.last_recall_check}
      <ul class="recall">
        {#each status.last_recall_check.folders as folder (folder.folder_id ?? '')}
          <li>{t('team-recall-result', { folder: folder.name, percent: Math.round(folder.recall_at_5_permille / 10), checks: folder.checks })}</li>
        {:else}
          <li>{t('team-recall-no-checks')}</li>
        {/each}
      </ul>
    {/if}

    <p class="team-menu">
      {#if status.binding && writable}
        <button type="button" onclick={openDescribe}>{t('team-describe')}</button>
      {/if}
      {#if status.binding || status.folders.length > 0}
        <button type="button" disabled={c.busy} onclick={() => void c.checkRecall()}>{t('team-check-recall')}</button>
      {/if}
      {#if teamWs !== null}
        <!-- SM-TW-10: Leave the team workspace. -->
        <button type="button" onclick={() => (confirming = 'leave')}><Icon name="stop" />{t('team-ws-leave')}</button>
        <button type="button" onclick={() => void openDialog('read')}>{t('team-read-another')}</button>
      {:else if status.binding}
        <button type="button" onclick={() => (confirming = 'stop')}><Icon name="stop" />{t('team-stop')}</button>
        <button type="button" onclick={() => void openDialog('read')}>{t('team-read-another')}</button>
      {/if}
    </p>

    {#if dialog !== null}
      <form
        class="team-dialog"
        role="group"
        aria-labelledby="team-dialog-title"
        onsubmit={(e) => {
          e.preventDefault();
          void submitDialog();
        }}
      >
        <h3 id="team-dialog-title" tabindex="-1">{t('team-read-another')}</h3>
        <label>
          {t('team-share-dialog-pick')}
          <select bind:value={pickedLink}>
            <option value=""></option>
            {#each c.candidates as link (link.id)}<option value={link.id}>{link.display_name}</option>{/each}
          </select>
        </label>
        <label>
          {t('team-share-dialog-address')}
          <input type="text" bind:value={address} aria-invalid={addressInvalid} aria-describedby={addressInvalid ? 'team-address-invalid' : undefined} />
        </label>
        {#if addressInvalid}<p id="team-address-invalid">{t('team-share-dialog-address-invalid')}</p>{/if}
        <p>{t('team-read-dialog-intro', { folder: status.header_line ?? '' })}</p>
        <ErrorLine error={c.error} onretry={() => void c.retry?.()} />
        <button type="submit">{t('team-read-verb')}</button>
        <button type="button" onclick={() => (dialog = null)}>{t('confirm-cancel')}</button>
      </form>
    {/if}
    {#if describing}
      <form
        class="team-dialog"
        role="group"
        aria-labelledby="team-describe-title"
        onsubmit={(e) => {
          e.preventDefault();
          void saveDescriptions();
        }}
      >
        <h3 id="team-describe-title" tabindex="-1">{t('team-describe')}</h3>
        {#each describeRows as row (row.path)}
          <label>
            {row.path === '' ? (status.header_line ?? status.binding?.folder ?? '') : row.path}
            <textarea rows="2" maxlength="500" bind:value={texts[row.path]} aria-describedby="team-describe-help"></textarea>
          </label>
        {/each}
        <p id="team-describe-help">{t('team-describe-help')}</p>
        <ErrorLine error={describeError} onretry={() => void saveDescriptions()} />
        <button type="submit" data-default="true">{t('engineering-save')}</button>
        <button type="button" onclick={() => (describing = false)}>{t('confirm-cancel')}</button>
      </form>
    {/if}
    {#if teamDialog !== null}
      <TeamWorkspaceShareDialog controller={c} {workspaceName} resultsOnly={teamDialog === 'results'} onsignin={signIn} onclose={() => (teamDialog = null)} />
    {/if}
    {#if confirming === 'leave'}
      <ConfirmDialog
        title={t('team-ws-leave-title', { workspace: workspaceName })}
        body={t('team-ws-leave-body')}
        verb={t('team-ws-leave-verb')}
        initialFocus="cancel"
        onconfirm={() => void c.leaveWorkspace().then((ok) => ok && (confirming = null))}
        oncancel={() => {
          c.error = null;
          confirming = null;
        }}
      >
        <ErrorLine error={c.error} />
      </ConfirmDialog>
    {:else if confirming === 'stop'}
      <ConfirmDialog title={t('team-stop-confirm')} verb={t('team-stop-verb')} initialFocus="cancel" onconfirm={() => void c.turnOff().then(() => (confirming = null))} oncancel={() => (confirming = null)} />
    {:else if confirming !== null}
      {@const target = confirming}
      <ConfirmDialog
        title={t('team-stop-reading-confirm', { folder: target.name })}
        verb={t('team-stop-reading')}
        initialFocus="cancel"
        onconfirm={() => void c.stopReading(target.reading).then(() => (confirming = null))}
        oncancel={() => (confirming = null)}
      />
    {/if}
    <div class="visually-hidden" aria-live="polite" role="status">{c.announcer.text || grants.announcer.text}</div>
  </section>
{/if}

<style>
  .team-panel {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-12);
    margin-block-end: var(--space-12);
  }
  .team-panel p {
    margin: var(--space-04) 0;
  }
  .secondary {
    color: var(--color-text-muted);
  }
  .tag {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding-inline: var(--space-04);
  }
  button.link {
    border: none;
    background: none;
    color: var(--color-accent);
    text-decoration: underline;
    padding: 0;
  }
</style>
