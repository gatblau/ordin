<script lang="ts">
  // The Team knowledge header and panel of a workspace (ADR-103, ADR-110): shown only while it applies,
  // one line each. A refusal is drawn as the owner's sentence through `ErrorLine`.
  //
  // spec: ordin-desktop/components/ScreensModule#ADR-103
  // spec: ordin-desktop/components/ScreensModule#SM-NS-13
  import { onMount } from 'svelte';

  import { answers } from '../../backend/groups';
  import { formatList } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import Icon from '../../icons/Icon.svelte';
  import type { ServiceError, TeamKnowledgeStatus, TeamProblem } from '../../service-api/types.gen';
  import type { BackendState } from '../../store/backend-state.svelte';
  import ConfirmDialog from '../ConfirmDialog.svelte';
  import ErrorLine from '../ErrorLine.svelte';
  import { TeamController, splitFolderAddress, teamErrorMessage } from './team.svelte';

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
  let dialog = $state<'share' | 'read' | null>(null);
  let confirming = $state<'stop' | { reading: string; name: string } | null>(null);
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
    status?.busy === 'checking' ? 'team-busy-checking' : status?.busy === 'getting' ? 'team-busy-getting' : status?.busy === 'sharing' ? 'team-busy-sharing' : status?.busy === 'checking_recall' ? 'team-busy-checking-recall' : null,
  );
  const names = $derived(status?.available ? formatList(status.available.from.slice(0, 5)) : '');

  /** The sentence for one problem of a folder's "Not taken in" list. */
  function problemText(problem: TeamProblem): string {
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

  async function openDialog(kind: 'share' | 'read'): Promise<void> {
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
    const ok = dialog === 'share' ? await c.turnOn(choice) : await c.addReadFolder(choice);
    if (ok) dialog = null;
  }
</script>

{#if status !== null}
  <section class="team-panel" aria-label={t('team-share-with-team')}>
    {#if status.binding}
      <p class="team-header">{t('team-header', { folder: status.header_line ?? status.binding.folder })}</p>
    {:else if status.folders.length > 0}
      <p>{t('team-no-own-folder')}</p>
    {/if}

    {#if busyId !== null}
      <p role="status">{t(busyId)}</p>
    {/if}
    {#if status.last_error && lastError !== null}
      {#if lastError.id === 'team-nothing-to-share'}
        <p>{t('team-nothing-to-share')}</p>
      {:else}
        <ErrorLine error={{ code: status.last_error, message: t(lastError.id), ...(lastError.action ? { action: lastError.action } : {}) }} onretry={() => void c.checkNow()} />
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
      {#if status.binding}
        <button type="button" onclick={() => (confirming = 'stop')}><Icon name="stop" />{t('team-stop')}</button>
        <button type="button" onclick={() => void openDialog('read')}>{t('team-read-another')}</button>
      {:else}
        <button type="button" onclick={() => void openDialog('share')}><Icon name="publish" />{t('team-share-with-team')}</button>
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
        <h3 id="team-dialog-title" tabindex="-1">{dialog === 'share' ? t('team-share-dialog-title') : t('team-read-another')}</h3>
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
        {#if dialog === 'share'}
          <p>{t('team-share-dialog-all')}</p>
          <p>{t('team-share-dialog-history')}</p>
        {:else}
          <p>{t('team-read-dialog-intro', { folder: status.header_line ?? '' })}</p>
        {/if}
        <ErrorLine error={c.error} onretry={() => void c.retry?.()} />
        <button type="submit">{dialog === 'share' ? t('team-share-dialog-verb') : t('team-read-verb')}</button>
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
    {#if confirming === 'stop'}
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
