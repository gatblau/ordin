<script lang="ts">
  // The what-you-see Markdown editor, used wherever a person writes Markdown (UX guidelines, *Writing*;
  // ScreensModule.md SM-NS-4.6 and the knowledge page's Edit). It takes and gives Markdown; the person sees
  // formatted text and never a Markdown code. A part it cannot show (raw HTML, a picture, front matter, a task
  // list, a link definition) is kept word for word and written back unchanged.
  //
  // The toolbar (`role="toolbar"`, one tab stop, arrow keys between buttons) is a row of icon buttons in
  // groups, as a word processor draws it (amended January 2026, user decision; Approved by the user January
  // 2026): Heading (a small menu: Normal text, Heading 1, 2, 3), Bold, Italic | Bulleted list, Numbered list
  // | Link, Quote, Table | Undo, Redo. Each button is named (`aria-label`) and has a tooltip (`title`) with its
  // shortcut, "Bold (⌘B)" on a Mac and "Bold (Ctrl+B)" elsewhere; a format in force is `aria-pressed`.
  // Cmd/Ctrl+B, I, K, Z and Shift+Z. Pasted HTML is cleaned first (`sanitisePastedHtml`): no scripts, styles
  // or event handlers; links http(s) or mailto only.
  //
  // spec: ordin-desktop/components/ScreensModule#SM-NS-4
  import type { Editor } from '@tiptap/core';
  import { onDestroy, onMount, tick } from 'svelte';

  import { t } from '../i18n/fluent';
  import Icon from '../icons/Icon.svelte';
  import type { IconName } from '../icons/sprite';
  import { keptReading, normaliseTypedHref } from './markdown';
  import { createEditor, getMarkdown, setMarkdown } from './setup';

  interface Props {
    /** The text, as Markdown. */
    value: string;
    /** The editable area's accessible name. */
    label: string;
    placeholder?: string;
    disabled?: boolean;
    /** The smallest height of the editable area (a CSS length). */
    minHeight?: string;
    onchange?: (markdown: string) => void;
  }
  let { value = $bindable(''), label, placeholder = '', disabled = false, minHeight = '12rem', onchange }: Props = $props();

  type Tool = 'heading' | 'bold' | 'italic' | 'bullets' | 'numbers' | 'link' | 'quote' | 'table' | 'undo' | 'redo';
  /** The buttons in order; `group` starts a new group (a thin line is drawn before it). */
  const TOOLS: ReadonlyArray<{ tool: Tool; label: string; icon: IconName; group: number; keys?: string }> = [
    { tool: 'heading', label: 'editor-heading', icon: 'heading', group: 1 },
    { tool: 'bold', label: 'editor-bold', icon: 'bold', group: 1, keys: 'Mod+B' },
    { tool: 'italic', label: 'editor-italic', icon: 'italic', group: 1, keys: 'Mod+I' },
    { tool: 'bullets', label: 'editor-bullets', icon: 'bullets', group: 2 },
    { tool: 'numbers', label: 'editor-numbers', icon: 'numbers', group: 2 },
    { tool: 'link', label: 'editor-link', icon: 'link', group: 3, keys: 'Mod+K' },
    { tool: 'quote', label: 'editor-quote', icon: 'quote', group: 3 },
    { tool: 'table', label: 'editor-table', icon: 'sheet', group: 3 },
    { tool: 'undo', label: 'editor-undo', icon: 'undo', group: 4, keys: 'Mod+Z' },
    { tool: 'redo', label: 'editor-redo', icon: 'redo', group: 4, keys: 'Mod+Shift+Z' },
  ];
  /** The Heading menu: 0 is Normal text. */
  const LEVELS: ReadonlyArray<{ level: 0 | 1 | 2 | 3; label: string }> = [
    { level: 0, label: 'editor-heading-normal' },
    { level: 1, label: 'editor-heading-1' },
    { level: 2, label: 'editor-heading-2' },
    { level: 3, label: 'editor-heading-3' },
  ];
  const mac = typeof navigator !== 'undefined' && /Mac|iP(hone|ad|od)/.test(navigator.platform);

  /** A shortcut as the person's keyboard writes it: ⌘B, ⇧⌘Z on a Mac; Ctrl+B, Ctrl+Shift+Z elsewhere. */
  function shownKeys(keys: string): string {
    const parts = keys.split('+');
    const letter = parts.pop() ?? '';
    if (mac) return `${parts.includes('Shift') ? '⇧' : ''}⌘${letter}`;
    return [...parts.map((p) => (p === 'Mod' ? 'Ctrl' : p)), letter].join('+');
  }

  /** The tooltip: the name, and the shortcut when there is one. */
  function tip(item: { label: string; keys?: string }): string {
    return item.keys === undefined ? t(item.label) : t('editor-tip', { name: t(item.label), keys: shownKeys(item.keys) });
  }

  let host = $state<HTMLDivElement | null>(null);
  let editor = $state.raw<Editor | null>(null);
  let focusIndex = $state(0);
  let buttons: HTMLButtonElement[] = $state([]);
  let linkOpen = $state(false);
  let linkText = $state('');
  let linkInvalid = $state(false);
  let linkInput = $state<HTMLInputElement | null>(null);
  let headingOpen = $state(false);
  let headingItems: HTMLButtonElement[] = $state([]);
  let menu = $state<HTMLDivElement | null>(null);
  // Bumped on every transaction so the pressed states are read again.
  let revision = $state(0);
  // The Markdown last given out, so a value that comes back unchanged is not loaded again.
  let given = '';

  onMount(() => {
    if (host === null) return;
    given = value;
    editor = createEditor(host, value, {
      label,
      placeholder: placeholder || t('editor-placeholder'),
      keptLabel: t('editor-kept'),
      onlink: () => void openLink(),
      onchange: () => {
        if (editor === null) return;
        const next = getMarkdown(editor);
        // A transaction that leaves the Markdown as it was (loading, a selection) is not a change.
        if (next === given) return;
        given = next;
        value = given;
        onchange?.(given);
      },
      onselection: () => (revision += 1),
    });
    editor.setEditable(!disabled);
  });
  onDestroy(() => editor?.destroy());

  $effect(() => {
    const next = value;
    if (editor !== null && next !== given) {
      given = next;
      setMarkdown(editor, next);
    }
  });
  $effect(() => {
    editor?.setEditable(!disabled);
  });

  function active(tool: Tool): boolean | undefined {
    void revision;
    const e = editor;
    if (e === null) return undefined;
    switch (tool) {
      case 'bold':
        return e.isActive('bold');
      case 'italic':
        return e.isActive('italic');
      case 'bullets':
        return e.isActive('bulletList');
      case 'numbers':
        return e.isActive('orderedList');
      case 'link':
        return e.isActive('link');
      case 'quote':
        return e.isActive('blockquote');
      default:
        return undefined;
    }
  }

  function canRun(tool: Tool): boolean {
    void revision;
    const e = editor;
    if (e === null || disabled) return false;
    if (tool === 'undo') return e.can().undo();
    if (tool === 'redo') return e.can().redo();
    if (tool === 'table') return !e.isActive('table');
    return true;
  }

  async function openLink(): Promise<void> {
    if (editor === null || disabled) return;
    linkText = (editor.getAttributes('link').href as string | undefined) ?? '';
    linkInvalid = false;
    linkOpen = true;
    await tick();
    linkInput?.focus();
  }

  function applyLink(): void {
    if (editor === null) return;
    const href = normaliseTypedHref(linkText);
    if (href === null) {
      linkInvalid = true;
      return;
    }
    const chain = editor.chain().focus().extendMarkRange('link');
    if (editor.state.selection.empty && !editor.isActive('link')) chain.insertContent({ type: 'text', text: linkText.trim(), marks: [{ type: 'link', attrs: { href } }] }).run();
    else chain.setLink({ href }).run();
    linkOpen = false;
  }

  function removeLink(): void {
    editor?.chain().focus().extendMarkRange('link').unsetLink().run();
    linkOpen = false;
  }

  function closeLink(): void {
    linkOpen = false;
    editor?.commands.focus();
  }

  /** Runs a toolbar button's action. */
  function run(tool: Tool): void {
    const e = editor;
    if (e === null || disabled) return;
    // Heading and Link open their own box; a chain with focus() would give the focus back to the text.
    if (tool === 'heading') {
      if (headingOpen) closeHeading();
      else void openHeading();
      return;
    }
    if (tool === 'link') {
      void openLink();
      return;
    }
    const chain = e.chain().focus();
    switch (tool) {
      case 'bold':
        chain.toggleBold().run();
        break;
      case 'italic':
        chain.toggleItalic().run();
        break;
      case 'bullets':
        chain.toggleBulletList().run();
        break;
      case 'numbers':
        chain.toggleOrderedList().run();
        break;
      case 'quote':
        chain.toggleBlockquote().run();
        break;
      case 'table':
        chain.insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run();
        break;
      case 'undo':
        chain.undo().run();
        break;
      case 'redo':
        chain.redo().run();
        break;
    }
    revision += 1;
  }

  /** The level of the paragraph or heading where the cursor is: 0 for normal text. */
  function currentLevel(): number {
    void revision;
    const e = editor;
    if (e === null) return 0;
    for (const level of [1, 2, 3]) if (e.isActive('heading', { level })) return level;
    return 0;
  }

  async function openHeading(): Promise<void> {
    if (editor === null || disabled) return;
    headingOpen = true;
    await tick();
    headingItems[Math.max(0, LEVELS.findIndex((l) => l.level === currentLevel()))]?.focus();
  }

  function closeHeading(): void {
    headingOpen = false;
    buttons[0]?.focus();
  }

  function setLevel(level: 0 | 1 | 2 | 3): void {
    const e = editor;
    headingOpen = false;
    if (e === null || disabled) return;
    if (level === 0) e.chain().focus().setParagraph().run();
    else e.chain().focus().setHeading({ level }).run();
    revision += 1;
  }

  /** The Heading menu's keys: arrows move, Escape closes it (and only it), Tab closes it. */
  function headingKey(event: KeyboardEvent): void {
    const at = headingItems.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      closeHeading();
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const n = LEVELS.length;
      headingItems[(((at + (event.key === 'ArrowDown' ? 1 : -1)) % n) + n) % n]?.focus();
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      headingItems[event.key === 'Home' ? 0 : LEVELS.length - 1]?.focus();
    } else if (event.key === 'Tab') {
      headingOpen = false;
    }
  }

  /** The next button that can be pressed from `from`, one step at a time in `step`'s direction (disabled ones skipped). */
  function enabledFrom(from: number, step: 1 | -1): number {
    const n = TOOLS.length;
    for (let i = 0; i < n; i++) {
      const at = (((from + step * i) % n) + n) % n;
      if (buttons[at] !== undefined && !buttons[at].disabled) return at;
    }
    return focusIndex;
  }

  function toolbarKey(event: KeyboardEvent): void {
    let next: number;
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') next = enabledFrom(focusIndex + 1, 1);
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') next = enabledFrom(focusIndex - 1, -1);
    else if (event.key === 'Home') next = enabledFrom(0, 1);
    else if (event.key === 'End') next = enabledFrom(TOOLS.length - 1, -1);
    else return;
    event.preventDefault();
    focusIndex = next;
    buttons[next]?.focus();
  }
</script>

<svelte:window
  onpointerdown={(e) => {
    // A press anywhere but the Heading menu and its button closes the menu.
    if (headingOpen && e.target instanceof Node && !menu?.contains(e.target) && !buttons[0]?.contains(e.target)) headingOpen = false;
  }}
/>

<div class="md-editor" class:disabled>
  <div class="md-toolbar" role="toolbar" tabindex="-1" aria-label={t('editor-toolbar', { name: label })} onkeydown={toolbarKey}>
    {#each TOOLS as item, index (item.tool)}
      {@const pressed = active(item.tool)}
      {#if index > 0 && TOOLS[index - 1]?.group !== item.group}<span class="md-sep" role="separator" aria-orientation="vertical"></span>{/if}
      <button
        bind:this={buttons[index]}
        type="button"
        class="md-tool"
        data-tool={item.tool}
        tabindex={index === focusIndex ? 0 : -1}
        aria-label={t(item.label)}
        title={tip(item)}
        aria-pressed={pressed === undefined ? undefined : pressed ? 'true' : 'false'}
        aria-haspopup={item.tool === 'heading' ? 'menu' : undefined}
        aria-expanded={item.tool === 'heading' ? (headingOpen ? 'true' : 'false') : undefined}
        aria-keyshortcuts={item.keys?.replace('Mod', mac ? 'Meta' : 'Control')}
        disabled={!canRun(item.tool)}
        onfocus={() => (focusIndex = index)}
        onmousedown={(e) => e.preventDefault()}
        onclick={() => run(item.tool)}><Icon name={item.icon} /></button>
    {/each}
  </div>
  {#if headingOpen}
    <div bind:this={menu} class="md-menu" role="menu" tabindex="-1" aria-label={t('editor-heading')} onkeydown={headingKey}
>
      {#each LEVELS as item, index (item.level)}
        <button
          bind:this={headingItems[index]}
          type="button"
          role="menuitemradio"
          class="md-level-{item.level}"
          data-level={item.level}
          tabindex="-1"
          aria-checked={currentLevel() === item.level ? 'true' : 'false'}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => setLevel(item.level)}>{t(item.label)}</button
        >
      {/each}
    </div>
  {/if}
  {#if linkOpen}
    <div
      class="md-link"
      role="group"
      aria-label={t('editor-link')}
      onkeydown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          e.stopPropagation();
          closeLink();
        } else if (e.key === 'Enter') {
          e.preventDefault();
          applyLink();
        }
      }}
    >
      <label>{t('editor-link-address')}<input bind:this={linkInput} type="text" inputmode="url" bind:value={linkText} aria-invalid={linkInvalid ? 'true' : undefined} aria-describedby={linkInvalid ? 'md-link-invalid' : undefined} /></label>
      <button type="button" onclick={applyLink}>{t('editor-link-apply')}</button>
      {#if active('link')}<button type="button" onclick={removeLink}>{t('editor-link-remove')}</button>{/if}
      <button type="button" onclick={closeLink}>{t('editor-link-cancel')}</button>
      {#if linkInvalid}<p id="md-link-invalid" class="invalid" role="alert">{t('editor-link-invalid')}</p>{/if}
    </div>
  {/if}
  {#if editor === null}
    <!-- Until the editor starts (and when drawn on the server for a test): the words, without codes. -->
    <div class="md-host"><div class="md-surface" aria-hidden="true">{keptReading(value)}</div></div>
  {/if}
  <div class="md-host" bind:this={host} style:--md-min-height={minHeight}></div>
</div>

<style>
  .md-editor {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    inline-size: 100%;
  }
  .md-toolbar,
  .md-link {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-04);
    align-items: center;
  }
  .md-tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    inline-size: 1.75rem;
    block-size: 1.75rem;
    padding: 0;
  }
  .md-toolbar button[aria-pressed='true'],
  .md-toolbar button[aria-expanded='true'] {
    background: var(--color-accent-soft);
    color: var(--color-accent-ink);
  }
  .md-sep {
    inline-size: 1px;
    block-size: 1.125rem;
    margin-inline: var(--space-04);
    background: var(--color-line-strong);
  }
  .md-menu {
    position: absolute;
    inset-block-start: 2rem;
    inset-inline-start: 0;
    z-index: 5;
    display: flex;
    flex-direction: column;
    min-inline-size: 11rem;
    padding: var(--space-04);
    border: 1px solid var(--color-line-strong);
    border-radius: var(--radius-md);
    background: var(--color-surface);
  }
  .md-menu button {
    justify-content: flex-start;
    border: 0;
    background: none;
    text-align: start;
  }
  .md-menu button[aria-checked='true'] {
    background: var(--color-accent-soft);
    color: var(--color-accent-ink);
  }
  .md-menu .md-level-1 {
    font-size: 1.25em;
    font-weight: 700;
  }
  .md-menu .md-level-2 {
    font-size: 1.125em;
    font-weight: 700;
  }
  .md-menu .md-level-3 {
    font-weight: 700;
  }
  .md-link .invalid {
    flex-basis: 100%;
    margin: 0;
    color: var(--color-danger);
  }
  .md-host :global(.md-surface) {
    min-block-size: var(--md-min-height);
    padding: var(--space-sm) var(--space-12);
    border: 1px solid var(--color-border-control);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-text);
    font: inherit;
    line-height: 1.5;
    outline: none;
    overflow-wrap: anywhere;
  }
  .md-host :global(.md-surface:focus-visible) {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
  }
  .md-host :global(.md-surface blockquote) {
    margin-inline: 0;
    padding-inline-start: var(--space-12);
    border-inline-start: 3px solid var(--color-line-strong);
    color: var(--color-text-secondary);
  }
  .md-host :global(.md-surface pre) {
    padding: var(--space-sm);
    border-radius: var(--radius-sm);
    background: var(--color-chip);
    font-family: var(--font-mono);
    white-space: pre-wrap;
  }
  .md-host :global(.md-surface code) {
    font-family: var(--font-mono);
  }
  .md-host :global(.md-surface a) {
    color: var(--color-accent-ink);
  }
  .md-host :global(.md-surface table) {
    border-collapse: collapse;
  }
  .md-host :global(.md-surface th),
  .md-host :global(.md-surface td) {
    min-inline-size: 4em;
    padding: var(--space-04) var(--space-sm);
    border: 1px solid var(--color-line-strong);
    vertical-align: top;
  }
  .md-host :global(.md-surface .selectedCell) {
    background: var(--color-accent-soft);
  }
  .md-host :global(.md-surface .kept) {
    margin-block: var(--space-sm);
    padding: var(--space-sm);
    border: 1px dashed var(--color-line-strong);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    white-space: pre-wrap;
  }
  .md-host :global(.md-surface p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    float: inline-start;
    block-size: 0;
    color: var(--color-text-muted);
    pointer-events: none;
  }
  .disabled .md-host :global(.md-surface) {
    opacity: 0.7;
  }
</style>
