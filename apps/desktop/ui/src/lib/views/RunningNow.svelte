<script lang="ts">
  // Running now for one workspace (SM-RN-1): the workspace's running work as Home draws it (`home.view` with
  // the workspace filter), each row with its state and **Show progress**; "Nothing running now" when none.
  // It reads Home's rows but never marks Home opened, so Home's new marks stay.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-RN-1
  import { onMount } from 'svelte';

  import { formatDate } from '../format/intl';
  import ActivityDot from '../blocks/ActivityDot.svelte';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { HomeRow } from '../service-api/types.gen';
  import { openTarget } from '../shell/open-item';
  import type { ScreenProps } from '../shell/screens';
  import ErrorLine from './ErrorLine.svelte';
  import { appOrNull } from './fixed/util';
  import { HOME_REFRESH_EVENTS, HomeModel, rowKey, runStateLabelId } from './home-model.svelte';

  let { backend, workspaceId }: ScreenProps = $props();
  // The app context is read while the screen is set up; Svelte's getContext answers nothing in a handler.
  const app = appOrNull();
  // svelte-ignore state_referenced_locally
  const model = new HomeModel(backend.handle, {
    openRow: (row: HomeRow) => {
      if (app !== null) openTarget(app, backend.id, row.workspace_id ?? null, row.opens);
    },
    openHandover: () => undefined,
  });
  // svelte-ignore state_referenced_locally
  model.filter = workspaceId;
  const rows = $derived(model.view?.running ?? []);
  const when = (at: string): string => formatDate(at, { dateStyle: 'short', timeStyle: 'short' });

  onMount(() => {
    void model.load();
    return backend.subscribe((env) => {
      if (HOME_REFRESH_EVENTS.has(env.kind)) void model.load();
    });
  });
</script>

<section class="running-now" aria-labelledby="running-now-heading">
  <h2 id="running-now-heading" tabindex="-1" data-region-focus>{t('tree-running')}</h2>
  <ErrorLine error={model.error} onretry={() => void model.load()} />
  {#if rows.length === 0}
    <p class="muted">{t('home-nothing-running')}</p>
  {:else}
    <ul class="list-rows">
      {#each rows as row (rowKey(row))}
        {@const label = runStateLabelId(backend, row)}
        <li class="list-row">
          <span class="icon-tile" data-tint={row.opens.kind === 'crew_run' ? 'crew' : 'running'}><Icon name={row.opens.kind === 'crew_run' ? 'crew' : 'run'} /></span>
          <span class="grow">
            <b class="line">{row.line}</b>
            <small><time datetime={row.at}>{when(row.at)}</time></small>
          </span>
          {#if label !== null}<span class="state state-label">{#if label === 'state-running'}<ActivityDot />{/if}{t(label)}</span>{/if}
          <button type="button" class="small" onclick={() => void model.open(row)}><Icon name="eye" />{t('home-show-progress')}</button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .grow {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
</style>
