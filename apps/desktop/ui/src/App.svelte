<script lang="ts">
  // The shell: side rail, screen area, conversation panel. F6 / Shift+F6 cycle the three regions; below
  // 1,024 CSS pixels the conversation slides over from the side, opened from the header.
  import { i18n, t } from './lib/i18n/fluent';
  import Icon from './lib/icons/Icon.svelte';
  import { setApp, type AppContext } from './lib/shell/context';
  import ConversationPanel from './lib/shell/ConversationPanel.svelte';
  import { focusRegion, nextRegion, regionOf, type Region } from './lib/shell/focus';
  import ScreenArea from './lib/shell/ScreenArea.svelte';
  import { overlays } from './lib/shell/screens';
  import SideRail from './lib/shell/SideRail.svelte';
  import { CONVERSATION, RAIL } from './lib/store/view-prefs';
  import QuitDialog from './lib/views/QuitDialog.svelte';
  import FirstStart from './lib/views/setup/FirstStart.svelte';

  interface Props {
    app: AppContext;
  }
  let { app }: Props = $props();
  // The context is read once, at mount: the window is built for one `AppContext`.
  // svelte-ignore state_referenced_locally
  setApp(app);
  // svelte-ignore state_referenced_locally
  const shell = app.shell;
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

{#if !regions}
  {#if firstStart.phase === 'first_start'}<FirstStart model={firstStart} />{/if}
{:else}
<div
  class="shell"
  class:narrow={shell.narrow}
  style:--rail-width={`${railWidth}px`}
  style:--conversation-width={`${conversationWidth}px`}
  lang={i18n.locale}
>
  <div class="region rail" data-region="tree">
    <SideRail />
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (a window splitter is focusable by WAI-ARIA) -->
  <div
    class="separator"
    role="separator"
    aria-orientation="vertical"
    aria-valuemin={RAIL.min}
    aria-valuemax={RAIL.max}
    aria-valuenow={railWidth}
    aria-label={t('rail-resize')}
    tabindex="0"
    onkeydown={(e) => separatorKey(e, 'rail')}
    onpointerdown={(e) => drag(e, 'rail')}
  ></div>

  <main class="region centre" data-region="screen">
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
      class="separator"
      role="separator"
      aria-orientation="vertical"
      aria-valuemin={CONVERSATION.min}
      aria-valuemax={CONVERSATION.max}
      aria-valuenow={conversationWidth}
      aria-label={t('conv-resize')}
      tabindex="0"
      onkeydown={(e) => separatorKey(e, 'conversation')}
      onpointerdown={(e) => drag(e, 'conversation')}
    ></div>
    <div class="region conversation" class:overlay={shell.narrow} data-region="conversation">
      <ConversationPanel />
    </div>
  {/if}
</div>

<div class="visually-hidden" aria-live="polite" role="status">{shell.announcement}</div>

{#each overlays() as overlay (overlay.id)}
  {@const Overlay = overlay.component}
  <Overlay />
{/each}
{/if}

{#if shell.quitRequest !== null}
  <QuitDialog count={shell.quitRequest.running_sessions} backend={app.local.handle} oncancel={() => (shell.quitRequest = null)} />
{/if}
