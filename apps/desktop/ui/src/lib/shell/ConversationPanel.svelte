<script lang="ts">
  // spec: ordin-desktop/components/ScreensModule#SM-CV-1
  // The right panel: *Needs you* as the fixed first tab (never closed), then one tab per opened
  // workspace. Switching a tab switches the whole window to that workspace. Blocks are drawn by the
  // blocks module's view when it has registered one; until then a message is drawn formatted (SM-MSG-1).
  import { onMount, tick, untrack, type Snippet } from 'svelte';
  import { answers } from '../backend/groups';
  import ApprovalCardLine from '../approval/ApprovalCardLine.svelte';
  import MarkdownView from '../blocks/MarkdownView.svelte';
  import { registerActionHandler } from '../search/actions';
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { ServiceError } from '../service-api/types.gen';
  import ConfirmDialog from '../views/ConfirmDialog.svelte';
  import ContinuedFold from '../views/ContinuedFold.svelte';
  import { EarlierListModel, prepareKeyPoints } from '../views/earlier.svelte';
  import ErrorLine from '../views/ErrorLine.svelte';
  import MemoryModelsCard from '../views/memory/MemoryModelsCard.svelte';
  import { getApp } from './context';
  import { ConversationPickerModel } from './conversation-picker.svelte';
  import MessageBox from './MessageBox.svelte';
  import NeedsYouTab from './NeedsYouTab.svelte';
  import { openScreen } from './open-item';
  import { getBlockView, makeKey, SCREEN_IDS } from './screens';
  import { NEEDS_YOU_TAB, ShellState } from './shell-state.svelte';

  // The pane's collapse button, drawn by the window at the start of the panel's top row (absent in a narrow window).
  let { toggle }: { toggle?: Snippet | undefined } = $props();

  const app = getApp();
  const shell = app.shell;
  const stores = $derived(app.stores.all());
  const needsYouTotal = $derived(stores.reduce((n, s) => n + s.needsYouCount, 0));
  const active = $derived(shell.conversationTabs.find((tab) => ShellState.tabKey(tab) === shell.activeConversation) ?? null);
  const activeState = $derived(active === null ? undefined : app.stateFor(active.backend));
  const items = $derived(active === null || activeState === undefined ? [] : (activeState.conversation.get(active.workspaceId) ?? []));
  let confirmClear = $state(false);
  let error = $state<ServiceError | null>(null);
  let box = $state<{ focus(): void } | null>(null);
  const BlockView = $derived(getBlockView());

  // One picker per workspace tab (SM-CC-1); its list also tells the continued fold what the live conversation continues (SM-CC-6).
  const pickers = new Map<string, ConversationPickerModel>();
  const picker = $derived.by(() => {
    if (active === null || activeState === undefined) return null;
    const key = ShellState.tabKey(active);
    let model = pickers.get(key);
    if (model === undefined) {
      model = new ConversationPickerModel(new EarlierListModel(activeState.handle, active.workspaceId), t);
      pickers.set(key, model);
    }
    return model;
  });
  const earlierCount = $derived(picker?.earlier.length ?? 0);
  const hasMessages = $derived(items.length > 0);
  // The live conversation's name and its `continues` are read when the tab is shown, when its first message
  // arrives and after Continue; opening the picker reads the list again.
  $effect(() => {
    void hasMessages;
    void shell.continuedEpoch;
    if (picker !== null && activeState !== undefined && answers(activeState, 'ordin.list_conversations')) void picker.list.load();
  });
  // Load and reload (step 5): a workspace's conversation tab reads `ordin.read_conversation` once its
  // backend has loaded, so the conversation kept before a restart is drawn again.
  const readTabs = new Set<string>();
  $effect(() => {
    if (active === null || activeState === undefined || !activeState.loaded) return;
    const key = ShellState.tabKey(active);
    if (readTabs.has(key) || !answers(activeState, 'ordin.read_conversation')) return;
    readTabs.add(key);
    void activeState.reloadConversation(active.workspaceId);
  });
  // SM-CC-4: after Continue, focus goes to the message box.
  let seenEpoch = shell.continuedEpoch;
  $effect(() => {
    const epoch = shell.continuedEpoch;
    if (epoch === seenEpoch) return;
    seenEpoch = epoch;
    void tick().then(() => box?.focus());
  });

  const workspaceName = (backend: string, workspaceId: string): string =>
    app.stateFor(backend)?.workspaces.find((w) => w.id === workspaceId)?.name ?? '';
  const serverOff = $derived(activeState !== undefined && activeState.kind === 'server' && activeState.connection !== 'connected');

  function select(key: string): void {
    shell.activeConversation = key;
    const tab = shell.conversationTabs.find((x) => ShellState.tabKey(x) === key);
    if (tab !== undefined) shell.openWorkspaceTab(tab.backend, tab.workspaceId);
  }

  function onkeydown(event: KeyboardEvent, index: number, keys: string[]): void {
    const to = event.key === 'ArrowRight' ? (index + 1) % keys.length : event.key === 'ArrowLeft' ? (index - 1 + keys.length) % keys.length : -1;
    const key = keys[to];
    if (key === undefined) return;
    event.preventDefault();
    select(key);
  }

  async function send(text: string): Promise<boolean> {
    if (active === null || activeState === undefined) return false;
    const out = await activeState.handle.call('ordin.send_message', { workspace_id: active.workspaceId, text });
    error = out.kind === 'error' ? out.error : null;
    const accepted = out.kind === 'ok' && out.result.accepted;
    // SM-SCR-1: sending brings the person's message and the working line into view.
    if (accepted) {
      atEnd = true;
      void tick().then(toEnd);
    }
    return accepted;
  }

  async function act(command: 'ordin.summarise'): Promise<void> {
    if (active === null || activeState === undefined) return;
    const out = await activeState.handle.call(command, { workspace_id: active.workspaceId });
    error = out.kind === 'error' ? out.error : null;
  }

  async function clear(): Promise<void> {
    confirmClear = false;
    if (active === null || activeState === undefined) return;
    const out = await activeState.handle.call('ordin.start_fresh', { workspace_id: active.workspaceId, kept_points: [] });
    error = out.kind === 'error' ? out.error : null;
    if (out.kind !== 'ok') return;
    // SM-CF-1: the conversation just ended is not the live one any more; the panel reads the new one (empty),
    // the picker's list and the message box follow, as after Continue (SM-CC-4).
    await activeState.reloadConversation(active.workspaceId);
    shell.afterContinue();
  }

  async function keepPointsOnClear(): Promise<void> {
    confirmClear = false;
    if (active === null || activeState === undefined) return;
    error = await prepareKeyPoints(activeState.handle, active.workspaceId);
  }

  // Approval cards of the workspace in front; a decided one stays as `card-decided`.
  const cards = $derived(
    active === null || activeState === undefined
      ? []
      : [...activeState.seenCards.values()].filter(
          (c) => c.workspace_id === active.workspaceId && (activeState.pendingApprovals.has(c.approval_id) || activeState.decided.get(c.approval_id)?.state === 'approved'),
        ),
  );

  // The Actions group of *Search or go to…* acts on this panel for the workspace in front (SM-SF-9).
  onMount(() => {
    registerActionHandler('summarise', () => void act('ordin.summarise'));
    registerActionHandler('clear', () => (confirmClear = true));
  });

  const tabKeys = $derived([NEEDS_YOU_TAB, ...shell.conversationTabs.map((x) => ShellState.tabKey(x))]);

  // spec: ordin-desktop/components/ScreensModule#SM-WK-1
  // The working line: one line below the last item while Ordin works on a turn; never an item, never stored.
  const working = $derived(active === null || activeState === undefined ? undefined : activeState.ordinWorking.get(active.workspaceId));
  let now = $state(Date.now());
  $effect(() => {
    if (working === undefined) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const seconds = $derived(working === undefined ? 0 : Math.max(0, Math.floor((now - Date.parse(working.since)) / 1000)));
  const workingText = $derived(
    working === undefined
      ? ''
      : working.doing !== undefined
        ? t('ordin-working-doing', { doing: working.doing, seconds })
        : seconds >= 10
          ? t('ordin-working-for', { seconds })
          : t('ordin-working'),
  );

  // spec: ordin-desktop/components/ScreensModule#SM-SCR-1
  // The conversation follows the newest item while the person is at its end (within 48 px); when they have
  // scrolled up to read, it stays put and offers `conv-jump-newest`.
  const END_SLACK_PX = 48;
  let scroller = $state<HTMLElement | null>(null);
  let atEnd = true;
  let newBelow = $state(false);
  const reducedMotion = (): boolean => typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  function onscroll(): void {
    if (scroller === null) return;
    atEnd = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight <= END_SLACK_PX;
    if (atEnd) newBelow = false;
  }
  function toEnd(): void {
    newBelow = false;
    atEnd = true;
    const el = scroller;
    if (el === null) return;
    if (typeof el.scrollTo === 'function') el.scrollTo({ top: el.scrollHeight, behavior: reducedMotion() ? 'auto' : 'smooth' });
    else el.scrollTop = el.scrollHeight;
  }
  const activeKey = $derived(active === null ? '' : ShellState.tabKey(active));
  const newest = $derived(`${items.length}|${cards.length}|${working !== undefined}`);
  let shownKey = '';
  $effect(() => {
    const key = activeKey;
    void newest;
    untrack(() => {
      if (key !== shownKey) {
        // Another conversation in front: start at its end.
        shownKey = key;
        atEnd = true;
      }
      if (atEnd) void tick().then(toEnd);
      else newBelow = true;
    });
  });
</script>

<aside class="conversation-panel" aria-label={t('conv-header-conversation')}>
  <!-- The top row: the collapse button, then Summarise, Clear and Earlier as icon buttons for the workspace in front. -->
  <div class="conversation-head">
    {@render toggle?.()}
    {#if active !== null && activeState !== undefined}
      {@const earlierLabel = earlierCount > 0 ? t('conv-earlier-label', { count: earlierCount }) : t('conv-earlier-name')}
      {#if answers(activeState, 'ordin.summarise')}<button type="button" class="icon-btn" aria-label={t('conv-summarise')} title={t('conv-summarise')} onclick={() => void act('ordin.summarise')}><Icon name="bullets" size="small" /></button>{/if}
      {#if answers(activeState, 'ordin.start_fresh')}<button type="button" class="icon-btn" aria-label={t('conv-clear')} title={t('conv-clear')} onclick={() => (confirmClear = true)}><Icon name="bin" size="small" /></button>{/if}
      {#if answers(activeState, 'ordin.list_conversations')}
        <button type="button" class="icon-btn" aria-label={earlierLabel} title={earlierLabel} onclick={() => openScreen(app, activeState.id, active.workspaceId, SCREEN_IDS.earlierConversations)}><Icon name="history" size="small" /></button>
      {/if}
    {/if}
  </div>
  <div role="tablist" class="conversation-tabs" aria-label={t('conv-header-conversation')}>
    <button
      type="button"
      role="tab"
      id="conv-tab-needs-you"
      aria-selected={shell.activeConversation === NEEDS_YOU_TAB}
      aria-controls="conv-panel"
      tabindex={shell.activeConversation === NEEDS_YOU_TAB ? 0 : -1}
      onclick={() => select(NEEDS_YOU_TAB)}
      onkeydown={(e) => onkeydown(e, 0, tabKeys)}
    >
      <Icon name="info" size="needs-tab" />{t('conv-needs-you')}{#if needsYouTotal > 0}<span class="count tab-count">{needsYouTotal}</span>{/if}
    </button>
    {#each shell.conversationTabs as tab, i (ShellState.tabKey(tab))}
      {@const key = ShellState.tabKey(tab)}
      <button
        type="button"
        role="tab"
        id={`conv-tab-${key}`}
        aria-selected={shell.activeConversation === key}
        aria-controls="conv-panel"
        tabindex={shell.activeConversation === key ? 0 : -1}
        onclick={() => select(key)}
        onkeydown={(e) => onkeydown(e, i + 1, tabKeys)}
      >
        {workspaceName(tab.backend, tab.workspaceId)}
      </button>
    {/each}
  </div>

  <div id="conv-panel" role="tabpanel" class="conversation-body">
    {#if active === null || activeState === undefined}
      <NeedsYouTab backends={stores} />
    {:else}
      {#if activeState.kind === 'local'}
        <!-- SM-MD-1: above the messages of every conversation of the local backend; never a server's. -->
        <MemoryModelsCard model={app.memoryModels} />
      {/if}
      {#if picker?.live?.continues}
        {#key picker.live.conversation_id}
          <ContinuedFold backend={activeState} workspaceId={active.workspaceId} live={picker.live} known={picker.list.all} />
        {/key}
      {/if}
      <div class="conversation-scroll" bind:this={scroller} {onscroll}>
      <ul class="conversation-items">
        {#each items as item (item.block_id)}
          <li class:you={item.from === 'person'}>
            {#if item.from === 'person' && item.block.kind === 'message'}
              <!-- SM-CV-1: the person's own message, drawn as theirs (prototype `.turn.you .bubble`). -->
              <div class="bubble">
                <span class="visually-hidden">{t('conv-you-said')}</span>
                {#each item.block.text.split(/\n\s*\n/) as paragraph, index (index)}<p>{paragraph}</p>{/each}
              </div>
            {:else}
              {#if BlockView !== null}
                <BlockView backend={activeState} workspaceId={item.workspace_id} blockId={item.block_id} block={item.block} />
              {:else if item.block.kind === 'message'}
                <!-- SM-MSG-1: before the block view is registered, Ordin's message is still drawn formatted. -->
                <MarkdownView source={item.block.text} />
              {/if}
            {/if}
          </li>
        {/each}
        {#each cards as card (card.approval_id)}
          <li class="approval-line"><ApprovalCardLine backend={activeState} {card} /></li>
        {/each}
      </ul>
      {#if working !== undefined}
        <p class="working-line" role="status" aria-live="polite">{workingText}</p>
      {/if}
      </div>
      {#if newBelow}
        <button type="button" class="small jump-newest" onclick={toEnd}>{t('conv-jump-newest')}</button>
      {/if}
      <ErrorLine {error} />
      <MessageBox
        bind:this={box}
        onsend={send}
        {...serverOff ? { disabledBecause: t('server-waiting-for', { server: activeState.displayName }) } : {}}
      />
    {/if}
  </div>
</aside>

{#if confirmClear}
  <ConfirmDialog
    title={t('confirm-clear-title')}
    body={t('confirm-clear-body')}
    verb={t('conv-clear')}
    onconfirm={() => void clear()}
    oncancel={() => (confirmClear = false)}
    {...activeState !== undefined && answers(activeState, 'ordin.prepare_start_fresh') ? { extra: { label: t('confirm-clear-keep-points'), onclick: () => void keepPointsOnClear() } } : {}}
  />
{/if}

<style>
  /* The conversation panel (cross-cutting/VisualDesign.md V5, V6 "Tab"; scenes `long`, `direct`, `needsTab`). */
  /* The panel takes the region's height so its items scroll inside it and the message box stays at the foot
     (SM-SCR-1, VisualDesign.md V5-SCR-1). */
  .conversation-panel {
    display: flex;
    flex-direction: column;
    block-size: 100%;
    min-block-size: 0;
  }
  .conversation-tabs {
    display: flex;
    align-items: flex-end;
    gap: var(--space-04);
    padding: var(--space-06) var(--space-sm) 0;
    border-block-end: 1px solid var(--color-line);
    flex: none;
    overflow-x: auto;
  }
  .conversation-tabs > button {
    display: flex;
    align-items: center;
    gap: var(--space-06);
    border: 1px solid transparent;
    border-block-end: 0;
    border-radius: 8px 8px 0 0;
    background: none;
    padding: 5px var(--space-10) 6px;
    min-height: 0;
    max-inline-size: 230px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 13px;
    color: var(--color-text-secondary);
  }
  .conversation-tabs > button:hover {
    background: var(--color-chip);
  }
  .conversation-tabs > button[aria-selected='true'] {
    background: var(--color-surface);
    border-color: var(--color-line);
    color: var(--color-text);
    font-weight: 600;
  }
  .tab-count {
    margin-inline-start: 2px;
  }
  .conversation-body {
    flex: 1;
    min-block-size: 0;
    display: flex;
    flex-direction: column;
  }
  .conversation-head {
    display: flex;
    gap: var(--space-04);
    align-items: center;
    padding: var(--space-04);
    flex: none;
  }
  .conversation-head:empty {
    display: none;
  }
  /* The editor toolbar's icon button (MarkdownEditor.svelte `.md-tool`). */
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    min-height: 0;
    padding: 0;
  }
  .conversation-items > li {
    display: flex;
    flex-direction: column;
    gap: var(--space-06);
    align-items: flex-start;
  }
  .conversation-items > li.you {
    align-items: flex-end;
  }
  .you .bubble {
    max-inline-size: 85%;
    padding: var(--space-06) var(--space-10);
    border-radius: var(--radius-md);
    background: var(--color-accent-soft);
  }
  .you .bubble p {
    margin: 0;
    white-space: pre-wrap;
  }
  .conversation-scroll {
    flex: 1;
    min-block-size: 0;
    overflow: auto;
  }
  .working-line {
    margin: 0;
    padding: 0 var(--space-md) var(--space-md);
    color: var(--color-text-secondary);
    font-size: 13px;
  }
  .jump-newest {
    align-self: center;
    margin-block: var(--space-06);
  }
  /* Approval lines follow the conversation's items, in its column (scenes `typedYes`, `slack`, `mailSend`). */
  .conversation-items > li.approval-line {
    align-items: stretch;
  }
  .conversation-items {
    list-style: none;
    margin: 0;
    padding: var(--space-md);
    display: flex;
    flex-direction: column;
    gap: var(--space-14);
  }
</style>
