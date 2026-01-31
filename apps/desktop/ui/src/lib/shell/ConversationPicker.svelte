<script lang="ts">
  // The conversation picker (ADR-108, SM-CC-1): a WAI-ARIA menu button in the conversation panel's header
  // showing the live conversation's name. Its menu lists the live conversation, up to 20 earlier ones
  // (newest first), **All earlier conversations** or **No earlier conversations**, and **Clear**. Choosing an
  // earlier one opens it read-only on its own tab; nothing is sent that changes state.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-CC-1
  import { tick } from 'svelte';

  import Icon from '../icons/Icon.svelte';
  import { buttonKey, ConversationPickerModel, menuKey } from './conversation-picker.svelte';

  interface Props {
    model: ConversationPickerModel;
    /** Opens an earlier conversation read-only on its own tab. */
    onopen: (conversationId: string, title: string) => void;
    /** Opens the Earlier conversations screen. */
    onall: () => void;
    /** Behaves as the Clear tool. */
    onclear: () => void;
  }
  let { model, onopen, onall, onclear }: Props = $props();

  // spec: ordin-desktop/components/ScreensModule#SM-IC-3
  // An earlier conversation draws `chat` (V8 *Lists*); **All earlier conversations** opens Earlier
  // conversations and draws `history` (V8 *Actions*). The live conversation and **Clear** draw none.

  let button = $state<HTMLButtonElement | null>(null);
  let menu = $state<HTMLElement | null>(null);
  const entries = $derived(model.entries);

  async function focusItem(index: number): Promise<void> {
    model.focused = index;
    await tick();
    menu?.querySelectorAll<HTMLElement>('[role^="menuitem"]')[index]?.focus();
  }

  async function show(where: 'first' | 'last'): Promise<void> {
    await model.show(where);
    await focusItem(model.focused);
  }

  function onbuttonkey(event: KeyboardEvent): void {
    if (model.open) return;
    const move = buttonKey(event.key, entries);
    if (!move.open) return;
    event.preventDefault();
    void show(event.key === 'ArrowUp' ? 'last' : 'first');
  }

  function onmenukey(event: KeyboardEvent, index: number): void {
    const move = menuKey(event.key, index, entries);
    if (move === null) return;
    if (move.close) {
      if (!move.restoreButton) {
        model.close();
        return;
      }
      event.preventDefault();
      model.close();
      void tick().then(() => button?.focus());
      return;
    }
    event.preventDefault();
    if (move.focus !== null) void focusItem(move.focus);
  }

  function choose(index: number): void {
    const entry = entries[index];
    if (entry === undefined || entry.disabled) return;
    model.close();
    if (entry.kind === 'earlier' && entry.conversationId !== undefined) onopen(entry.conversationId, entry.label);
    else if (entry.kind === 'all') onall();
    else if (entry.kind === 'clear') onclear();
    else void tick().then(() => button?.focus());
  }
</script>

<div class="conversation-picker">
  <button
    bind:this={button}
    type="button"
    class="small"
    aria-haspopup="menu"
    aria-expanded={model.open}
    aria-label={model.label}
    onclick={() => (model.open ? model.close() : void show('first'))}
    onkeydown={onbuttonkey}
  >
    {model.name}
  </button>
  {#if model.open}
    <ul bind:this={menu} role="menu" aria-label={model.label} class="picker-menu">
      {#each entries as entry, index (index)}
        {#if entry.separatorBefore}<li role="separator"></li>{/if}
        <li role="none">
          <button
            type="button"
            role={entry.kind === 'live' || entry.kind === 'earlier' ? 'menuitemradio' : 'menuitem'}
            aria-checked={entry.kind === 'live' ? true : entry.kind === 'earlier' ? false : undefined}
            aria-label={entry.accessibleName}
            aria-disabled={entry.disabled ? true : undefined}
            tabindex={model.focused === index ? 0 : -1}
            onkeydown={(e) => onmenukey(e, index)}
            onclick={() => choose(index)}
          >
            {#if entry.kind === 'earlier'}<span class="icon-tile"><Icon name="chat" /></span>{:else if entry.kind === 'all'}<Icon name="history" />{/if}
            <span class="entry">{entry.label}</span>
            {#if entry.detail !== undefined}<span class="detail">{entry.detail}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .conversation-picker {
    position: relative;
  }
  .picker-menu {
    position: absolute;
    z-index: 10;
    list-style: none;
    margin: 0;
    padding: var(--space-04);
    background: var(--color-surface, canvas);
    border: 1px solid var(--color-line);
    border-radius: var(--radius-md);
    min-inline-size: 16rem;
  }
  .detail {
    display: block;
    color: var(--color-text-muted);
  }
  [role='separator'] {
    border-block-start: 1px solid var(--color-line);
    margin-block: var(--space-04);
  }
</style>
