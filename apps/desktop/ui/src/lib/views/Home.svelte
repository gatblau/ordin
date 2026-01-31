<script lang="ts">
  // Home (11 §5.17): Waiting for you (only the oldest keeps a primary Open), Running now (state label and
  // Show progress), Since you last looked (newest first, Open, Run again… on finished work); the filter;
  // `new` marks with a label. A row opens the item in its own workspace. No button approves, sends,
  // posts or publishes. Each section with no row says so in one line, and its rows are grouped by
  // workspace, then Not in a workspace, then Hosted agents (SM-CU-5).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-CU-5
  import { onMount } from 'svelte';

  import { formatDate } from '../format/intl';
  import { t } from '../i18n/fluent';
  import { getApp } from '../shell/context';
  import { openScreen, openTarget } from '../shell/open-item';
  import { makeKey } from '../shell/screens';
  import type { ScreenProps } from '../shell/screens';
  import Icon from '../icons/Icon.svelte';
  import type { IconName } from '../icons/sprite';
  import type { HomeRow } from '../service-api/types.gen';
  import ErrorLine from './ErrorLine.svelte';
  import { groupRows, HOME_REFRESH_EVENTS, HomeModel, rowKey, runStateLabelId, type HomeGroup } from './home-model.svelte';

  interface Props extends ScreenProps {
    model?: HomeModel;
  }
  let { backend, model: given }: Props = $props();
  // svelte-ignore state_referenced_locally
  const app = given === undefined ? getApp() : null;
  // svelte-ignore state_referenced_locally
  const model =
    given ??
    new HomeModel(backend.handle, {
      openRow: (row: HomeRow) => {
        if (app !== null) openTarget(app, backend.id, row.workspace_id ?? null, row.opens);
      },
      openHandover: (id: string) => {
        if (app !== null) openScreen(app, backend.id, null, makeKey('handover', id));
      },
    });
  const view = $derived(model.view);
  const oldest = $derived(model.oldestWaiting);
  const when = (at: string): string => formatDate(at, { dateStyle: 'short', timeStyle: 'short' });

  // spec: ordin-desktop/components/ScreensModule#SM-IC-3
  // A feed row's icon (V8 *Home*, the prototype's `FEEDICO`), from what the row opens: a finished run `task`,
  // a crew run `crew`, a changed document `docs`, an out-of-date knowledge page `know`, anything else
  // `task`. A Slack reply is a finished run with nothing in the row to tell it apart, so it draws `task`.
  // A waiting row draws `info` (V8 *Lists*, a Needs you row); a running row `run`, a crew run `crew`
  // (V8 *Tree and lists*).
  const FEED_ICON: Readonly<Partial<Record<HomeRow['opens']['kind'], IconName>>> = {
    run: 'task',
    crew_run: 'crew',
    document: 'docs',
    knowledge_page: 'know',
  };
  function feedIcon(row: HomeRow): IconName {
    return FEED_ICON[row.opens.kind] ?? 'task';
  }

  onMount(() => {
    void model.load();
    void model.markOpened();
    return backend.subscribe((env) => {
      if (HOME_REFRESH_EVENTS.has(env.kind)) void model.load();
    });
  });
</script>

{#snippet rowTitle(row: HomeRow)}
  <span class="grow">
    <b class="line">{row.line}</b>
    <small>
      {#if row.workspace_name}<span class="workspace">{row.workspace_name}</span>{' · '}{/if}<time datetime={row.at}>{when(row.at)}</time>
      {#if row.is_new}{' · '}<span class="new">{t('home-new')}</span>{/if}
    </small>
  </span>
{/snippet}

{#snippet groupHeading(group: HomeGroup)}
  {#if group.headingId !== null}
    <h4 class="group-heading">{t(group.headingId)}</h4>
  {:else if group.name !== null && group.name !== ''}
    <h4 class="group-heading">{group.name}</h4>
  {/if}
{/snippet}

<section class="home" aria-labelledby="home-heading">
  <h2 id="home-heading" tabindex="-1" data-region-focus>{t('rail-home')}</h2>
  <div class="filter" role="group" aria-label={t('home-all-workspaces')}>
    <button type="button" class="small" aria-pressed={model.filter === null} onclick={() => void model.setFilter(null)}>{t('home-all-workspaces')}</button>
    {#each backend.workspaces as ws (ws.id)}
      <button type="button" class="small" aria-pressed={model.filter === ws.id} onclick={() => void model.setFilter(ws.id)}>{ws.name}</button>
    {/each}
  </div>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
  {#if model.following !== null}<p role="status" class="following">{t('home-handover-link-following', { place: model.following })}</p>{/if}

  <section aria-labelledby="home-waiting-heading">
    <h3 id="home-waiting-heading">{t('home-waiting')}{#if view !== null && view.waiting.length > 0}<span class="count">{view.waiting.length}</span>{/if}</h3>
    {#if view === null || view.waiting.length === 0}
      <p class="muted">{t('home-nothing-waiting')}</p>
    {:else}
      {#each groupRows(view.waiting) as group (group.key)}
        {@render groupHeading(group)}
        <ul class="list-rows">
          {#each group.rows as row (rowKey(row))}
            <li class="list-row" class:is-new={row.is_new}>
              <span class="icon-tile"><Icon name="info" /></span>
              {@render rowTitle(row)}
              <button type="button" class="small" data-default={oldest === row ? 'true' : undefined} onclick={() => void model.open(row)}>{t('card-open')}</button>
            </li>
          {/each}
        </ul>
      {/each}
    {/if}
    <div><button type="button" class="small" onclick={() => model.openLink()}>{t('home-open-handover-link')}</button></div>
  </section>

  <section aria-labelledby="home-running-heading">
    <h3 id="home-running-heading">{t('home-running')}{#if view !== null && view.running.length > 0}<span class="count">{view.running.length}</span>{/if}</h3>
    {#if view === null || view.running.length === 0}
      <p class="muted">{t('home-nothing-running')}</p>
    {:else}
      {#each groupRows(view.running) as group (group.key)}
        {@render groupHeading(group)}
        <ul class="list-rows">
          {#each group.rows as row (rowKey(row))}
            {@const label = runStateLabelId(backend, row)}
            <li class="list-row">
              {#if row.opens.kind === 'crew_run'}
                <span class="icon-tile" data-tint="crew"><Icon name="crew" /></span>
              {:else}
                <span class="icon-tile" data-tint="running"><Icon name="run" /></span>
              {/if}
              {@render rowTitle(row)}
              {#if label !== null}<span class="state state-label">{t(label)}</span>{/if}
              <button type="button" class="small" onclick={() => void model.open(row)}><Icon name="eye" />{t('home-show-progress')}</button>
            </li>
          {/each}
        </ul>
      {/each}
    {/if}
  </section>

  <section aria-labelledby="home-since-heading">
    <h3 id="home-since-heading">{t('home-since')}{#if view !== null && view.since_last_looked.length > 0}<span class="count">{view.since_last_looked.length}</span>{/if}</h3>
    {#if view === null || view.since_last_looked.length === 0}
      <p class="muted">{t('home-nothing-since')}</p>
    {:else}
      {#each groupRows(view.since_last_looked) as group (group.key)}
        {@render groupHeading(group)}
        <ul class="list-rows">
          {#each group.rows as row (rowKey(row))}
            <li class="list-row" class:is-new={row.is_new}>
              <span class="icon-tile"><Icon name={feedIcon(row)} /></span>
              {@render rowTitle(row)}
              <button type="button" class="small" onclick={() => void model.open(row)}>{t('card-open')}</button>
              {#if row.run_again}<button type="button" class="small" onclick={() => void model.runAgain(row)}>{t('progress-run-again')}</button>{/if}
            </li>
          {/each}
        </ul>
      {/each}
    {/if}
  </section>
</section>

{#if model.linkOpen}
  <div class="dialog-backdrop">
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="handover-link-heading">
      <h2 id="handover-link-heading" tabindex="-1" data-region-focus>{t('home-open-handover-link')}</h2>
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void model.submitLink();
        }}
      >
        <label>
          <span class="hint">{t('home-open-handover-link-hint')}</span>
          <input type="text" bind:value={model.linkText} autocomplete="off" />
        </label>
        <ErrorLine error={model.linkError} />
        <div class="dialog-buttons">
          <button type="button" onclick={() => model.closeLink()}>{t('confirm-cancel')}</button>
          <button type="submit" data-default="true" disabled={model.linkBusy}>{t('card-open')}</button>
        </div>
      </form>
    </div>
  </div>
{/if}

<style>
  /* Home (scene `home`, `homeGrouped`): the filter as chips, each section a card of rows. */
  .filter {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-06);
  }
  .list-row .new {
    color: var(--color-accent-ink);
    font-weight: 600;
  }
  .list-row > button {
    flex: none;
  }
  .group-heading {
    margin: var(--space-sm) 0 var(--space-04);
    font-size: 12px;
    font-weight: 600;
    color: var(--color-text-muted);
  }
</style>
