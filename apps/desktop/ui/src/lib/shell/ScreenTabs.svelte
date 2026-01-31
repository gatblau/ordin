<script lang="ts">
  // One row of tabs per workspace (WAI-ARIA tabs): arrows move and show, Delete closes the focused tab.
  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { IconName } from '../icons/sprite';
  import type { Tab } from './screen-area.svelte';

  interface Props {
    tabs: Tab[];
    front: string | null;
    titleOf: (tab: Tab) => string;
    /** The tab's kind icon (cross-cutting/VisualDesign.md V8 *Screen tab*). */
    iconOf: (tab: Tab) => IconName;
    onselect: (key: string) => void;
    onclose: (key: string) => void;
  }
  let { tabs, front, titleOf, iconOf, onselect, onclose }: Props = $props();

  function onkeydown(event: KeyboardEvent, index: number): void {
    const last = tabs.length - 1;
    let to = -1;
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') to = index === last ? 0 : index + 1;
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') to = index === 0 ? last : index - 1;
    else if (event.key === 'Home') to = 0;
    else if (event.key === 'End') to = last;
    else if (event.key === 'Delete') {
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
    <span class="screen-tab" class:front={tab.key === front}>
      <button
        type="button"
        role="tab"
        id={`tab-${tab.key}`}
        aria-selected={tab.key === front}
        aria-controls="screen-panel"
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
