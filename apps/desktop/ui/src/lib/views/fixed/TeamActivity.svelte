<script lang="ts">
  // The Team activity screen (SM-TW-8): the access cards first (SM-TW-6), then the entries of
  // `team_knowledge.activity`, newest first, each one line with its time in the person's locale, its
  // outcome told by text, and each result a link that opens its file as any document of the workspace
  // opens. Only the fields of `TeamActivityView` are drawn: no conversation, input or approval of a run
  // (SM-TW-9). Both lists are read again on `workspace_changed`; a workspace that stops being a team
  // workspace closes the screen. Opened from *Needs you* with a grant (HomeAndSearch.md HS-TW-2), the
  // instance is that grant: its card takes the focus, or `needs-you-team-grant-gone` says it was answered.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-TW-8
  // spec: ordin-desktop/components/ScreensModule#SM-TW-9
  import { onMount } from 'svelte';

  import { formatDate } from '../../format/intl';
  import { t } from '../../i18n/fluent';
  import type { TeamActivityView, TeamResultRef } from '../../service-api/types.gen';
  import { openTarget } from '../../shell/open-item';
  import { ScreenAreaState } from '../../shell/screen-area.svelte';
  import { makeKey, SCREEN_IDS, type ScreenProps } from '../../shell/screens';
  import ErrorLine from '../ErrorLine.svelte';
  import TeamGrantCard from './TeamGrantCard.svelte';
  import { activityLine, notSharedYet, OUTCOME_MESSAGE, TeamActivityList, TeamGrants } from './team-workspace.svelte';
  import { appOrNull } from './util';

  let { backend, workspaceId, instance }: ScreenProps = $props();

  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const ws = workspaceId ?? '';
  // svelte-ignore state_referenced_locally
  const grants = new TeamGrants(backend.handle, ws);
  // svelte-ignore state_referenced_locally
  const activity = new TeamActivityList(backend.handle, ws);
  const revision = $derived(backend.workspaceRevision(ws));
  let first = true;

  /** HS-TW-2: the grant was answered or removed before it was opened. The id is the Home and Search amendment's. */
  const GRANT_GONE = 'needs-you-team-grant-gone';

  async function load(): Promise<void> {
    await Promise.all([grants.load(), activity.load(), stillTeam()]);
  }

  /** Closes the screen when the workspace is no longer a team workspace. */
  async function stillTeam(): Promise<void> {
    const out = await backend.handle.call('team_knowledge.status', { ws });
    if (out.kind === 'ok' && out.result.team_workspace == null && app !== null) {
      app.screens.close(ScreenAreaState.scope(backend.id, ws), makeKey(SCREEN_IDS.teamActivity, instance));
    }
  }

  onMount(() => {
    void load();
  });
  $effect(() => {
    void revision;
    if (first) {
      first = false;
      return;
    }
    void load();
  });

  const focusId = $derived(instance ?? grants.list[0]?.grant_id ?? null);
  const gone = $derived(instance !== null && grants.loaded && !grants.list.some((g) => g.grant_id === instance));

  const when = (at: string): string => formatDate(at, { dateStyle: 'medium', timeStyle: 'short' });

  function openResult(result: TeamResultRef): void {
    if (app === null || result.file == null || result.file.kind !== 'place_file') return;
    openTarget(app, backend.id, ws, { kind: 'document', place_id: result.file.place_id, path: result.file.relative_path, workspace_id: ws });
  }

  const linkable = (result: TeamResultRef): boolean => result.file != null && result.file.kind === 'place_file';
  const resultTipId = (entry: TeamActivityView, i: number): string => `team-result-${entry.id}-${i}`;
</script>

<section class="team-activity" aria-labelledby="team-activity-heading">
  <h2 id="team-activity-heading" tabindex="-1" data-region-focus>{t('screen-team-activity')}</h2>

  {#if gone}<p role="status">{t(GRANT_GONE)}</p>{/if}
  <ErrorLine error={grants.goneError} />
  {#if grants.list.length > 0}
    <div class="cards">
      {#each grants.list as grant (grant.grant_id)}
        <TeamGrantCard {grants} {grant} focus={grant.grant_id === focusId} />
      {/each}
    </div>
  {/if}

  <ErrorLine error={activity.error} onretry={() => void activity.load()} />
  {#if activity.loaded && activity.error === null && activity.entries.length === 0}
    <p class="secondary">{t('team-ws-activity-empty')}</p>
  {:else}
    <ul class="entries list-rows">
      {#each activity.entries as entry (entry.id)}
        <li class="list-row entry">
          <span class="line">{activityLine(entry)}</span>
          <time class="secondary" datetime={entry.at}>{when(entry.at)}</time>
          {#if entry.kind === 'run' && entry.outcome}<span class="tag">{t(OUTCOME_MESSAGE[entry.outcome])}</span>{/if}
          {#if notSharedYet(entry)}<span class="tag">{t('team-ws-activity-not-shared')}</span>{/if}
          {#each entry.results as result, i (i)}
            {#if linkable(result)}
              <button type="button" class="link" onclick={() => openResult(result)}>{result.name}</button>
            {:else}
              <span class="muted-link" title={t('team-ws-result-not-here')} aria-describedby={resultTipId(entry, i)}>{result.name}</span>
              <span id={resultTipId(entry, i)} class="visually-hidden">{t('team-ws-result-not-here')}</span>
            {/if}
          {/each}
        </li>
      {/each}
    </ul>
    {#if activity.next !== null}
      <button type="button" class="small" onclick={() => void activity.more()}>{t('team-ws-activity-more')}</button>
    {/if}
  {/if}
  <div class="visually-hidden" aria-live="polite" role="status">{grants.announcer.text}</div>
</section>

<style>
  .team-activity > button {
    align-self: flex-start;
  }
  .cards {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }
  .entries {
    padding: 0;
  }
  .entry {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-06);
    align-items: baseline;
  }
  .secondary {
    color: var(--color-text-muted);
  }
  .tag {
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    padding-inline: var(--space-04);
  }
  .muted-link {
    color: var(--color-text-muted);
    text-decoration: underline dotted;
  }
  button.link {
    border: none;
    background: none;
    color: var(--color-accent);
    text-decoration: underline;
    padding: 0;
  }
</style>
