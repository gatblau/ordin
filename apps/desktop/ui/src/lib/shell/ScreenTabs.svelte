<script lang="ts">
  // One row of tabs per workspace (WAI-ARIA tabs): arrows move and show, Delete closes the focused tab.
  // Each tab, Home included, has Close; its context menu (a right press, Shift+F10 or the menu key) holds
  // Close, Close others, Close tabs to the right and Close all (amended January 2026; user decision; Approved
  // by the user January 2026).
  //
  // spec: ordin-desktop/components/ScreensModule#SM-UP-1
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { IconName } from '../icons/sprite';
  import { tabsToClose, type Tab } from './screen-area.svelte';

  interface Props {
    tabs: Tab[];
    front: string | null;
    titleOf: (tab: Tab) => string;
    /** The tab's kind icon (cross-cutting/VisualDesign.md V8 *Screen tab*). */
    iconOf: (tab: Tab) => IconName;
    onselect: (key: string) => void;
    onclose: (key: string) => void;
    /** Closes several tabs at once (the tab menu's Close others, Close tabs to the right, Close all). */
    onclosemany?: (keys: string[]) => void;
  }
  let { tabs, front, titleOf, iconOf, onselect, onclose, onclosemany }: Props = $props();

  /** The tab whose menu is open, and where the menu is drawn (the window's coordinates). */
  let menu = $state<{ key: string; x: number; y: number } | null>(null);
  let menuEl = $state<HTMLElement | null>(null);
  let opener: HTMLElement | null = null;

  function openMenu(key: string, x: number, y: number, from: HTMLElement | null): void {
    opener = from;
    menu = { key, x, y };
    queueMicrotask(() => menuEl?.querySelector<HTMLElement>('[role="menuitem"]:not([disabled])')?.focus());
  }
  function closeMenu(refocus: boolean): void {
    menu = null;
    if (refocus) opener?.focus();
    opener = null;
  }
  function pointerMenu(event: MouseEvent, key: string): void {
    event.preventDefault();
    const tab = (event.currentTarget as HTMLElement | null)?.querySelector<HTMLElement>('[role="tab"]') ?? null;
    openMenu(key, event.clientX, event.clientY, tab);
  }
  function choose(which: 'close' | 'others' | 'right' | 'all'): void {
    const m = menu;
    if (m === null) return;
    const index = tabs.findIndex((tab) => tab.key === m.key);
    closeMenu(false);
    if (index < 0) return;
    if (which === 'close') onclose(m.key);
    else onclosemany?.(tabsToClose(tabs, index, which));
  }
  function menuKey(event: KeyboardEvent): void {
    const items = Array.from(menuEl?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? []);
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault();
      event.stopPropagation();
      closeMenu(true);
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      items[(at + 1) % items.length]?.focus();
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      items[(at - 1 + items.length) % items.length]?.focus();
    } else if (event.key === 'Home') {
      event.preventDefault();
      items[0]?.focus();
    } else if (event.key === 'End') {
      event.preventDefault();
      items[items.length - 1]?.focus();
    }
  }

  function onkeydown(event: KeyboardEvent, index: number): void {
    const last = tabs.length - 1;
    let to = -1;
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') to = index === last ? 0 : index + 1;
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') to = index === 0 ? last : index - 1;
    else if (event.key === 'Home') to = 0;
    else if (event.key === 'End') to = last;
    else if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) {
      event.preventDefault();
      const tab = tabs[index];
      const el = event.currentTarget as HTMLElement;
      const box = el.getBoundingClientRect();
      if (tab !== undefined) openMenu(tab.key, box.left, box.bottom, el);
      return;
    } else if (event.key === 'Delete') {
      event.preventDefault();
      const tab = tabs[index];
      if (tab !== undefined) onclose(tab.key);
      return;
    }
    const target = tabs[to];
    if (target === undefined) return;
    event.preventDefault();
    onselect(target.key);
    queueMicrotask(() => (event.currentTarget as HTMLElement | null)?.parentElement?.querySelectorAll<HTMLElement>('[role="tab"]')[to]?.focus());
  }
</script>

<div class="screen-tabs" role="tablist" aria-orientation="horizontal">
  {#each tabs as tab, index (tab.key)}
    {@const close = t('screen-tab-close', { title: titleOf(tab) })}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <span class="screen-tab" class:front={tab.key === front} oncontextmenu={(e) => pointerMenu(e, tab.key)}>
      <button
        type="button"
        role="tab"
        id={`tab-${tab.key}`}
        aria-selected={tab.key === front}
        aria-controls="screen-panel"
        aria-haspopup="menu"
        tabindex={tab.key === front ? 0 : -1}
        onclick={() => onselect(tab.key)}
        onkeydown={(e) => onkeydown(e, index)}
      >
        <span class="icon-tile"><Icon name={iconOf(tab)} /></span><span class="tab-title">{titleOf(tab)}{#if tab.isNew}<span class="new-mark"> · {t('home-new')}</span>{/if}</span>
      </button>
      <button type="button" class="tab-close" tabindex="-1" aria-label={close} title={close} onclick={() => onclose(tab.key)}><Icon name="remove" size="close" /></button>
    </span>
  {/each}
</div>

<svelte:window
  onpointerdown={(e) => {
    if (menu !== null && e.target instanceof Node && !menuEl?.contains(e.target)) closeMenu(false);
  }}
/>

{#if menu !== null}
  {@const m = menu}
  {@const index = tabs.findIndex((tab) => tab.key === m.key)}
  {@const at = tabs[index]}
  {#if at !== undefined}
    <div
      bind:this={menuEl}
      role="menu"
      tabindex="-1"
      class="tree-menu-list screen-tab-menu"
      style:inset-inline-start={`${m.x}px`}
      style:inset-block-start={`${m.y}px`}
      aria-label={t('screen-tab-menu', { title: titleOf(at) })}
      onkeydown={menuKey}
    >
      <button type="button" role="menuitem" onclick={() => choose('close')}>{t('screen-tab-menu-close')}</button>
      {#if onclosemany}
        <button type="button" role="menuitem" disabled={tabs.length < 2} onclick={() => choose('others')}>{t('screen-tab-menu-close-others')}</button>
        <button type="button" role="menuitem" disabled={index === tabs.length - 1} onclick={() => choose('right')}>{t('screen-tab-menu-close-right')}</button>
        <button type="button" role="menuitem" onclick={() => choose('all')}>{t('screen-tab-menu-close-all')}</button>
      {/if}
    </div>
  {/if}
{/if}

<style>
  /* Drawn at the pointer (or under the tab from the keyboard), above the row, which scrolls sideways. */
  .screen-tab-menu {
    position: fixed;
    inset-inline-end: auto;
  }
  .screen-tab-menu button[disabled] {
    color: var(--color-text-muted);
  }
</style>
