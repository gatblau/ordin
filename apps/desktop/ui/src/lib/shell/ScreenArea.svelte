<script lang="ts">
  // The centre: one row of tabs for the open workspace and the screen in front. A screen whose commands
  // the backend does not answer is neither listed nor drawn (SM-UP-1).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-UP-1
  import { answersAll } from '../backend/groups';
  import { t } from '../i18n/fluent';
  import { screenTabIcon } from '../icons/kinds';
  import type { IconName } from '../icons/sprite';
  import { getApp } from './context';
  import EmptyCentre from './EmptyCentre.svelte';
  import ScreenPlaceholder from './ScreenPlaceholder.svelte';
  import ScreenTabs from './ScreenTabs.svelte';
  import { ScreenAreaState, type Tab } from './screen-area.svelte';
  import { getScreen, parseKey, SCREEN_IDS, titleIdOf } from './screens';

  const app = getApp();
  const backend = $derived(app.stateFor(app.shell.openBackend) ?? app.local);
  const scope = $derived(ScreenAreaState.scope(backend.id, app.shell.openWorkspace));

  const offered = (key: string): boolean => {
    const commands = getScreen(key)?.commands;
    return commands === undefined || answersAll(backend, commands);
  };
  const tabs = $derived(app.screens.tabs(scope).filter((tab) => offered(tab.key)));
  const front = $derived.by(() => {
    const f = app.screens.front(scope);
    return f !== null && tabs.some((tab) => tab.key === f) ? f : (tabs[0]?.key ?? null);
  });

  // A workspace first shown with nothing open shows Home. Home closes like any tab (amended January 2026; user
  // decision; Approved by the user January 2026): once the person closes every tab the area stays empty, with a
  // faint logo, the version and Check for updates (EmptyCentre), and the side rail's Home opens it again.
  const started = new Set<string>();
  $effect(() => {
    if (started.has(scope)) return;
    started.add(scope);
    if (tabs.length === 0 && offered(SCREEN_IDS.home)) app.screens.open(scope, SCREEN_IDS.home);
  });

  const titleOf = (tab: Tab): string => {
    if (tab.title !== undefined) return tab.title;
    const id = titleIdOf(tab.key);
    return id === null ? tab.key : t(id);
  };

  // A block's tab draws its one-line card's icon when the block is in the conversation's store.
  const iconOf = (tab: Tab): IconName => {
    const { id, instance } = parseKey(tab.key);
    const workspace = app.shell.openWorkspace;
    const kind = workspace === null || instance === null ? undefined : backend.conversation.get(workspace)?.find((i) => i.block_id === instance)?.block.kind;
    return screenTabIcon(id, kind);
  };

  const def = $derived(front === null ? undefined : getScreen(front));
  const parsed = $derived(front === null ? null : parseKey(front));
</script>

<div class="screen-area">
  <ScreenTabs
    {tabs}
    {front}
    {titleOf}
    {iconOf}
    onselect={(key) => app.screens.show(scope, key)}
    onclose={(key) => app.screens.close(scope, key)}
    onclosemany={(keys) => app.screens.closeMany(scope, keys)}
  />
  <div id="screen-panel" class="screen-panel" role="tabpanel" aria-labelledby={front === null ? undefined : `tab-${front}`} data-region-focus-host>
    {#if front === null}
      <EmptyCentre />
    {/if}
    {#key front}
      {#if parsed !== null}
        {#if def !== undefined}
          {@const Screen = def.component}
          <!-- `parsed?.`: when the front tab closes with none after it, the closing screen still reads its props, and
               `parsed` is already null (plan 04: it threw, and main.ts replaced the window with its failure page). -->
          <Screen {backend} workspaceId={app.shell.openWorkspace} instance={parsed?.instance ?? null} />
        {:else}
          <ScreenPlaceholder titleId={titleIdOf(parsed.id) ?? 'screen-unknown'} />
        {/if}
      {/if}
    {/key}
  </div>
</div>
