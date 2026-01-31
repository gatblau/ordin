<script lang="ts">
  // The shell: side rail, screen area, conversation panel. F6 / Shift+F6 cycle the three regions; below
  // 1,024 CSS pixels the conversation slides over from the side, opened from the header.
  import { i18n, t } from './lib/i18n/fluent';
  import Icon from './lib/icons/Icon.svelte';
  import { setApp, type AppContext } from './lib/shell/context';
  import { openNotificationTarget } from './lib/shell/open-item';
  import ConversationPanel from './lib/shell/ConversationPanel.svelte';
  import { focusRegion, nextRegion, regionOf, type Region } from './lib/shell/focus';
  import ScreenArea from './lib/shell/ScreenArea.svelte';
  import { overlays } from './lib/shell/screens';
  import SideRail from './lib/shell/SideRail.svelte';
  import { CONVERSATION, RAIL } from './lib/store/view-prefs';
  import QuitDialog from './lib/views/QuitDialog.svelte';
  import FirstStart from './lib/views/setup/FirstStart.svelte';
  import RecordingIndicator from './lib/views/minutes/RecordingIndicator.svelte';

  interface Props {
    app: AppContext;
  }
  let { app }: Props = $props();
  // The context is read once, at mount: the window is built for one `AppContext`.
  // svelte-ignore state_referenced_locally
  setApp(app);
  // svelte-ignore state_referenced_locally
  const shell = app.shell;
  // A notification's item opens when the window comes forward after it (OrdinDesktop.md DSK-NTF-1).
  // spec: ordin-desktop/components/OrdinDesktop#DSK-NTF-1
  $effect(() => app.onDesktop('open-target', (payload) => void openNotificationTarget(app, payload)));
  // svelte-ignore state_referenced_locally
  const prefs = app.prefs;
  // First start (ScreensModule.md SM-FS-1, SM-FS-4): while the guided setup runs, only `FirstStart` is
  // drawn; the side rail, the screen area and the conversation are not, and take no keys.
  // spec: ordin-desktop/components/ScreensModule#SM-FS-1
  // svelte-ignore state_referenced_locally
  const firstStart = app.firstStart;
  const regions = $derived(firstStart.phase === 'regions');

  let railWidth = $state(prefs.railWidth());
  let conversationWidth = $state(prefs.conversationWidth());
  // The side tree and the conversation collapse to a slim strip holding the restore button (amended January
  // 2026; user decision; Approved by the user January 2026); remembered in the view preferences.
  // spec: ordin-desktop/components/ScreensModule#SM-PC-1
  // A collapsed pane's separator is parked (kept in its grid column, not drawn), never `hidden`: a grid item
  // with `display: none` leaves the five-column grid, and every region after it slides one column to the
  // left — the screen area into the separator's zero-width column, the conversation and its restore button
  // out of sight (amended January 2026; user decision; Approved by the user January 2026).
  let collapsed = $state(prefs.collapsed());
  // The agent-work layout: the side panel collapses to its strip and the width left is split 60% conversation,
  // 40% centre. Leaving it restores the layout before it exactly: the panes' widths are never changed by it,
  // and their collapsed state is kept to restore (amended January 2026; user decision; Approved by the user
  // January 2026).
  let agentWork = $state(prefs.agentWork());
  function toggleAgentWork(): void {
    if (agentWork === null) {
      agentWork = { restore: { ...collapsed } };
      collapsed = { rail: true, conversation: false };
    } else {
      collapsed = { ...agentWork.restore };
      agentWork = null;
    }
    prefs.setAgentWork(agentWork);
    prefs.setCollapsed(collapsed);
    queueMicrotask(() => document.getElementById('agent-work-toggle')?.focus());
  }
  function toggle(which: 'rail' | 'conversation'): void {
    // Collapsing the conversation leaves the agent-work layout, which is drawn around it.
    if (which === 'conversation' && agentWork !== null) {
      agentWork = null;
      prefs.setAgentWork(null);
    }
    collapsed = { ...collapsed, [which]: !collapsed[which] };
    prefs.setCollapsed(collapsed);
    const id = `pane-toggle-${which}`;
    queueMicrotask(() => document.getElementById(id)?.focus());
  }
  const railCollapsed = $derived(collapsed.rail);
  // In a narrow window the conversation slides over and is opened from the header; it does not collapse.
  const conversationCollapsed = $derived(collapsed.conversation && !shell.narrow);
  const agentLayout = $derived(agentWork !== null && !shell.narrow);
  const STEP = 16;

  function onresize(): void {
    shell.setWidth(window.innerWidth);
  }
  $effect(() => {
    onresize();
    window.addEventListener('resize', onresize);
    return () => window.removeEventListener('resize', onresize);
  });

  function onkeydown(event: KeyboardEvent): void {
    if (!regions) return;
    if (event.key === 'F6') {
      event.preventDefault();
      const available: Region[] = shell.narrow && !shell.conversationOverlayOpen ? ['tree', 'screen'] : ['tree', 'screen', 'conversation'];
      const next = nextRegion(regionOf(document.activeElement), event.shiftKey, available);
      if (next !== null) focusRegion(next);
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      shell.searchOpen = true;
    }
  }

  type Which = 'rail' | 'conversation';
  function setWidth(which: Which, px: number): void {
    if (which === 'rail') railWidth = prefs.setRailWidth(px);
    else conversationWidth = prefs.setConversationWidth(px);
  }
  function separatorKey(event: KeyboardEvent, which: Which): void {
    const range = which === 'rail' ? RAIL : CONVERSATION;
    const now = which === 'rail' ? railWidth : conversationWidth;
    // The conversation's edge grows to the left, the rail's to the right.
    const sign = which === 'rail' ? 1 : -1;
    let next: number;
    if (event.key === 'ArrowRight') next = now + sign * STEP;
    else if (event.key === 'ArrowLeft') next = now - sign * STEP;
    else if (event.key === 'Home') next = range.min;
    else if (event.key === 'End') next = range.max;
    else if (event.key === 'Enter') next = range.default;
    else return;
    event.preventDefault();
    setWidth(which, next);
  }
  function drag(event: PointerEvent, which: Which): void {
    const startX = event.clientX;
    const start = which === 'rail' ? railWidth : conversationWidth;
    const sign = which === 'rail' ? 1 : -1;
    const move = (e: PointerEvent): void => setWidth(which, start + sign * (e.clientX - startX));
    const up = (): void => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }

  const showConversation = $derived(!shell.narrow || shell.conversationOverlayOpen);
  const conversationLabel = $derived(t('conv-header-conversation'));
</script>

<svelte:window {onkeydown} />

{#snippet toggleButton(which: 'rail' | 'conversation', isCollapsed: boolean)}
  {@const label = t(which === 'rail' ? (isCollapsed ? 'pane-rail-restore' : 'pane-rail-collapse') : isCollapsed ? 'pane-conversation-restore' : 'pane-conversation-collapse')}
  <button
    type="button"
    id={`pane-toggle-${which}`}
    class="pane-toggle"
    class:points-start={which === 'rail' ? !isCollapsed : isCollapsed}
    aria-expanded={!isCollapsed}
    aria-label={label}
    title={label}
    data-region-focus={isCollapsed ? '' : undefined}
    onclick={() => toggle(which)}
  ><Icon name="chev" size="small" /></button>
{/snippet}

{#snippet agentWorkButton()}
  {@const label = t('pane-agent-work')}
  <button
    type="button"
    id="agent-work-toggle"
    class="pane-toggle agent-work"
    aria-pressed={agentWork !== null}
    aria-label={label}
    title={label}
    onclick={toggleAgentWork}
  ><Icon name="agent" size="small" /></button>
{/snippet}

{#snippet paneToggle(which: 'rail' | 'conversation', isCollapsed: boolean)}
  <div class="pane-strip" class:end={which === 'conversation'}>{@render toggleButton(which, isCollapsed)}{#if which === 'conversation'}{@render agentWorkButton()}{/if}</div>
{/snippet}

<!-- An open pane draws its collapse button in its own top row: the side panel beside Home and Search, the
     conversation beside its Summarise, Clear and Earlier buttons. -->
{#snippet railToggle()}{@render toggleButton('rail', false)}{/snippet}
{#snippet conversationToggle()}{@render toggleButton('conversation', false)}{@render agentWorkButton()}{/snippet}

{#if !regions}
  {#if firstStart.phase === 'first_start'}<FirstStart model={firstStart} />{/if}
{:else}
<div
  class="shell"
  class:narrow={shell.narrow}
  class:agent-work={agentLayout}
  style:--rail-width={railCollapsed ? 'var(--pane-strip)' : `${railWidth}px`}
  style:--conversation-width={conversationCollapsed ? 'var(--pane-strip)' : agentLayout ? 'calc((100% - var(--rail-width)) * 0.6)' : `${conversationWidth}px`}
  lang={i18n.locale}
>
  <div class="region rail" class:collapsed={railCollapsed} data-region="tree">
    {#if railCollapsed}{@render paneToggle('rail', true)}{/if}
    <div class="pane-body" hidden={railCollapsed}><SideRail toggle={railCollapsed ? undefined : railToggle} /></div>
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (a window splitter is focusable by WAI-ARIA) -->
  <div
    class="separator separator-rail"
    class:parked={railCollapsed}
    role="separator"
    aria-orientation="vertical"
    aria-valuemin={RAIL.min}
    aria-valuemax={RAIL.max}
    aria-valuenow={railWidth}
    aria-label={t('rail-resize')}
    tabindex={railCollapsed ? -1 : 0}
    aria-hidden={railCollapsed ? 'true' : undefined}
    onkeydown={(e) => separatorKey(e, 'rail')}
    onpointerdown={(e) => drag(e, 'rail')}
  ></div>

  <main class="region centre" data-region="screen">
    <!-- SM-MIN-7: a running recording is shown above every screen; nothing is drawn while there is none. -->
    <RecordingIndicator />
    {#if shell.narrow}
      <header class="narrow-header">
        <button type="button" aria-expanded={shell.conversationOverlayOpen} onclick={() => { shell.conversationOverlayOpen = !shell.conversationOverlayOpen; shell.conversationMark = false; }}>
          <Icon name="chat" />{conversationLabel}{#if shell.conversationMark}<span class="new-mark"> · {t('home-new')}</span>{/if}
        </button>
      </header>
    {/if}
    <ScreenArea />
  </main>

  {#if showConversation}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (a window splitter is focusable by WAI-ARIA) -->
    <div
      class="separator separator-conversation"
      class:parked={conversationCollapsed || agentLayout}
      role="separator"
      aria-orientation="vertical"
      aria-valuemin={CONVERSATION.min}
      aria-valuemax={CONVERSATION.max}
      aria-valuenow={conversationWidth}
      aria-label={t('conv-resize')}
      tabindex={conversationCollapsed || agentLayout ? -1 : 0}
      aria-hidden={conversationCollapsed || agentLayout ? 'true' : undefined}
      onkeydown={(e) => separatorKey(e, 'conversation')}
      onpointerdown={(e) => drag(e, 'conversation')}
    ></div>
    <div class="region conversation" class:overlay={shell.narrow} class:collapsed={conversationCollapsed} data-region="conversation">
      {#if conversationCollapsed}{@render paneToggle('conversation', true)}{/if}
      <div class="pane-body" hidden={conversationCollapsed}><ConversationPanel toggle={shell.narrow || conversationCollapsed ? undefined : conversationToggle} /></div>
    </div>
  {/if}
</div>

<div class="visually-hidden" aria-live="polite" role="status">{shell.announcement}</div>

{#each overlays() as overlay (overlay.id)}
  {@const Overlay = overlay.component}
  <Overlay />
{/each}
{/if}

<style>
  /* The collapse button in the editor toolbar's style (MarkdownEditor.svelte `.md-tool`). */
  .pane-strip {
    display: flex;
    justify-content: flex-end;
  }
  .pane-strip.end {
    justify-content: flex-start;
  }
  .conversation .pane-strip {
    padding: var(--space-04);
  }
  .collapsed .pane-strip {
    justify-content: center;
  }
  .pane-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    /* The same size as the icon buttons beside it (Home, Search, Summarise): not the 36 px of a button. */
    min-height: 0;
    padding: 0;
  }
  .pane-toggle.agent-work[aria-pressed='true'] {
    background: var(--color-accent-soft);
    color: var(--color-accent-ink);
  }
  .collapsed .pane-strip {
    flex-direction: column;
    align-items: center;
    gap: var(--space-04);
  }
  /* The chevron points to where the pane goes: towards the edge to collapse, away from it to restore. */
  .pane-toggle.points-start :global(.icon) {
    transform: scaleX(-1);
  }
  .rail.collapsed {
    padding: var(--space-04) 0;
    overflow: hidden;
  }
  .conversation.collapsed {
    overflow: hidden;
  }
  .conversation .pane-body {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  /* After the rule above, so a collapsed pane's body stays hidden: nothing of it shows through the strip. */
  .pane-body[hidden] {
    display: none;
  }
  .shell {
    --pane-strip: 2.5rem;
  }
</style>

{#if shell.quitRequest !== null}
  <QuitDialog count={shell.quitRequest.running_sessions} backend={app.local.handle} oncancel={() => (shell.quitRequest = null)} />
{/if}
