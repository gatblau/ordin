// @vitest-environment happy-dom
// The what-you-see Markdown editor (UX guidelines, Writing; ScreensModule.md SM-NS-4.6, amended January 2026,
// user decision): Markdown in and out with nothing lost, the toolbar, the keyboard shortcuts, pasted HTML
// cleaned, and the toolbar's keyboard use and names.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-4
import type { Editor } from '@tiptap/core';
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import MarkdownEditor from '../src/lib/editor/MarkdownEditor.svelte';
import { documentHrefAllowed, escapeText, keptReading, normaliseTypedHref, sanitisePastedHtml, tidyMarkdown } from '../src/lib/editor/markdown';
import { createEditor, getMarkdown } from '../src/lib/editor/setup';
import { settle } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
const made: Editor[] = [];
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  for (const e of made.splice(0)) e.destroy();
  document.body.innerHTML = '';
});

const HOOKS = { label: 'Body', placeholder: 'Start writing…', keptLabel: 'Kept', onlink: () => undefined, onchange: () => undefined, onselection: () => undefined };
function roundTrip(markdown: string): string {
  const host = document.createElement('div');
  document.body.appendChild(host);
  const editor = createEditor(host, markdown, HOOKS);
  made.push(editor);
  return getMarkdown(editor);
}

async function draw(value: string): Promise<{ target: HTMLElement; changes: string[]; editor: () => Editor; surface: () => HTMLElement }> {
  const changes: string[] = [];
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(MarkdownEditor as never, { target, props: { value, label: 'Body', onchange: (md: string) => changes.push(md) } as never });
  await settle();
  flushSync();
  const surface = (): HTMLElement => target.querySelector('.md-surface[contenteditable="true"]') as HTMLElement;
  return { target, changes, surface, editor: () => (surface() as unknown as { editor: Editor }).editor };
}
const button = (target: HTMLElement, tool: string): HTMLButtonElement => target.querySelector(`[data-tool="${tool}"]`) as HTMLButtonElement;
const key = (el: HTMLElement, k: string, mods: { shift?: boolean } = {}): void => {
  const mac = /Mac|iP(hone|ad|od)/.test(navigator.platform);
  el.dispatchEvent(new KeyboardEvent('keydown', { key: k, metaKey: mac, ctrlKey: !mac, shiftKey: mods.shift === true, bubbles: true, cancelable: true }));
  flushSync();
};

describe('Markdown in and out', () => {
  it.each([
    ['headings', '# One\n\n## Two\n\n### Three\n'],
    ['bold, italic, strike and code', 'Some **bold**, *italic*, ~~gone~~ and `code`.\n'],
    ['bulleted and numbered lists, nested', '- one\n- two\n  - nested\n\n1. first\n2. second\n'],
    ['links', 'See [the plan](https://example.com/plan) or [mail us](mailto:a@example.com) or [the folder](Notes/plan.md).\n'],
    ['quotes', '> A quoted line\n'],
    ['fenced code with its language', '```rust\nfn main() {}\n\n\nlet x = 1;\n```\n'],
    ['a horizontal rule', 'Above\n\n---\n\nBelow\n'],
    ['knowledge links, R&D and snake_case as typed', 'See [[Payment terms]] for R&D on snake_case and 5 < 6.\n'],
  ])('%s come back the same', (_name, markdown) => {
    expect(roundTrip(markdown)).toBe(markdown);
  });

  it('a table comes back as the same table', () => {
    const out = roundTrip('| Name | Days |\n| --- | --- |\n| Acme | 30 |\n');
    expect(out.split('\n').map((l) => l.replace(/-+/g, '---').replace(/\s+/g, ' ').trim())).toEqual(['| Name | Days |', '| --- | --- |', '| Acme | 30 |', '']);
  });

  it.each([
    ['raw HTML', '<div align="center">Centred</div>'],
    ['a picture', '![Logo](images/logo.png)'],
    ['a task list', '- [ ] to do\n- [x] done'],
    ['a link definition', '[plan]: https://example.com/plan'],
    ['a link the editor will not follow', 'Run [this](javascript:alert(1)) now.'],
    ['an HTML comment', '<!-- keep me -->'],
  ])('%s it cannot show is kept word for word', (_name, raw) => {
    const markdown = `# Before\n\n${raw}\n\nAfter **this**.\n`;
    expect(roundTrip(markdown)).toBe(markdown);
  });

  it('front matter at the top is kept word for word', () => {
    const markdown = '---\ntitle: Rates\ntags: [a, b]\n---\n\n# Rates\n\nText.\n';
    expect(roundTrip(markdown)).toBe(markdown);
  });

  it('the result is stable: writing it again changes nothing', () => {
    const once = roundTrip('# T\n\nText with a [link](https://a.example) and\na soft break.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n<b>raw</b>\n');
    expect(roundTrip(once)).toBe(once);
  });

  it('an empty text stays empty, and Windows line ends are read', () => {
    expect(roundTrip('')).toBe('');
    expect(roundTrip('# A\r\n\r\nB\r\n')).toBe('# A\n\nB\n');
  });

  it('a kept part is shown as its words, never its codes', () => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    made.push(createEditor(host, '<div align="center">**Centred**</div>\n\n![Logo](logo.png)\n', HOOKS));
    const kept = [...host.querySelectorAll('[data-ordin-kept]')].map((el) => el.textContent);
    expect(kept).toEqual(['Centred', 'Logo']);
    expect(keptReading('---\ntitle: Rates\n---')).toBe('title: Rates');
  });

  it('typed text that would read as formatting is escaped, and only that', () => {
    expect(escapeText('2 * 3 and a_b and _x_', false)).toBe('2 \\* 3 and a_b and \\_x\\_');
    expect(escapeText('# not a heading', true)).toBe('\\# not a heading');
    expect(escapeText('1. not a list', true)).toBe('1\\. not a list');
    expect(escapeText('<script>', false)).toBe('&lt;script>');
    expect(escapeText('&amp; stays', false)).toBe('&amp;amp; stays');
    expect(tidyMarkdown('a\n\n\n\nb\n\n```\nx\n\n\ny\n```\n\n\n')).toBe('a\n\nb\n\n```\nx\n\n\ny\n```\n');
  });
});

describe('the toolbar and the shortcuts', () => {
  it.each([
    ['bold', 'Plan', '**Plan**\n'],
    ['italic', 'Plan', '*Plan*\n'],
    ['bullets', 'Plan', '- Plan\n'],
    ['numbers', 'Plan', '1. Plan\n'],
    ['quote', 'Plan', '> Plan\n'],
  ])('%s changes the selected text and gives out Markdown', async (tool, start, expected) => {
    const { target, changes, editor } = await draw(start);
    editor().commands.selectAll();
    button(target, tool).click();
    flushSync();
    expect(changes.at(-1)).toBe(expected);
    if (tool === 'bold' || tool === 'italic') expect(button(target, tool).getAttribute('aria-pressed')).toBe('true');
  });

  it('a button is shown pressed where the cursor is', async () => {
    const { target, editor } = await draw('**Plan** now\n\n> Quoted\n');
    editor().commands.setTextSelection(2);
    flushSync();
    expect(button(target, 'bold').getAttribute('aria-pressed')).toBe('true');
    expect(button(target, 'italic').getAttribute('aria-pressed')).toBe('false');
    expect(button(target, 'quote').getAttribute('aria-pressed')).toBe('false');
    let quoted = 0;
    editor().state.doc.descendants((node, pos) => {
      if (node.isText && node.text === 'Quoted') quoted = pos + 1;
    });
    editor().commands.setTextSelection(quoted);
    flushSync();
    expect(button(target, 'quote').getAttribute('aria-pressed')).toBe('true');
    expect(button(target, 'bold').getAttribute('aria-pressed')).toBe('false');
  });

  it('Heading opens a small menu: Normal text, Heading 1, 2, 3; the level in force is checked; a choice sets it', async () => {
    const { target, changes, editor } = await draw('## Plan\n');
    editor().commands.setTextSelection(2);
    flushSync();
    const heading = button(target, 'heading');
    expect(heading.getAttribute('aria-haspopup')).toBe('menu');
    expect(heading.getAttribute('aria-expanded')).toBe('false');
    expect(heading.hasAttribute('aria-pressed')).toBe(false);
    heading.click();
    flushSync();
    await settle();
    expect(heading.getAttribute('aria-expanded')).toBe('true');
    const menu = target.querySelector('[role="menu"]') as HTMLElement;
    const items = [...menu.querySelectorAll('[role="menuitemradio"]')] as HTMLButtonElement[];
    expect(items.map((i) => i.textContent)).toEqual(['Normal text', 'Heading 1', 'Heading 2', 'Heading 3']);
    expect(items.map((i) => i.getAttribute('aria-checked'))).toEqual(['false', 'false', 'true', 'false']);
    expect(document.activeElement).toBe(items[2]);
    menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    expect(document.activeElement).toBe(items[3]);
    items[1]?.click();
    flushSync();
    expect(changes.at(-1)).toBe('# Plan\n');
    expect(target.querySelector('[role="menu"]')).toBeNull();
    button(target, 'heading').click();
    flushSync();
    await settle();
    (target.querySelector('[data-level="0"]') as HTMLButtonElement).click();
    flushSync();
    expect(changes.at(-1)).toBe('Plan\n');
  });

  it('Escape closes the Heading menu only (the screen around does not take it as Cancel), and goes back to Heading', async () => {
    const { target } = await draw('Plan');
    button(target, 'heading').click();
    flushSync();
    await settle();
    const menu = target.querySelector('[role="menu"]') as HTMLElement;
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    menu.dispatchEvent(escape);
    flushSync();
    expect(target.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(button(target, 'heading'));
    // The screens leave the editor on an Escape no part of the editor has used (defaultPrevented).
    expect(escape.defaultPrevented).toBe(true);
  });

  it('Table inserts a table with a header row; Undo takes it away and Redo puts it back', async () => {
    const { target, changes } = await draw('Plan');
    expect(button(target, 'undo').disabled).toBe(true);
    button(target, 'table').click();
    flushSync();
    expect(changes.at(-1)).toMatch(/^Plan\n\n\|.*\|\n\| -+ \| -+ \| -+ \|\n/);
    button(target, 'undo').click();
    flushSync();
    expect(changes.at(-1)).toBe('Plan\n');
    button(target, 'redo').click();
    flushSync();
    expect(changes.at(-1)).toMatch(/^Plan\n\n\|/);
  });

  it('Link opens the link box; an address is applied, a script address is refused, Remove link takes it off', async () => {
    const { target, changes, editor } = await draw('Plan');
    editor().commands.selectAll();
    button(target, 'link').click();
    flushSync();
    await settle();
    const input = target.querySelector('.md-link input') as HTMLInputElement;
    input.value = 'javascript:alert(1)';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    ([...target.querySelectorAll('.md-link button')].find((b) => b.textContent === 'Apply') as HTMLButtonElement).click();
    flushSync();
    expect(target.querySelector('[role="alert"]')?.textContent).toBe('Use a web address (https://…) or an email address.');
    input.value = 'example.com/plan';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    ([...target.querySelectorAll('.md-link button')].find((b) => b.textContent === 'Apply') as HTMLButtonElement).click();
    flushSync();
    expect(changes.at(-1)).toBe('[Plan](https://example.com/plan)\n');
    editor().commands.selectAll();
    button(target, 'link').click();
    flushSync();
    await settle();
    ([...target.querySelectorAll('.md-link button')].find((b) => b.textContent === 'Remove link') as HTMLButtonElement).click();
    flushSync();
    expect(changes.at(-1)).toBe('Plan\n');
  });

  it('Cmd/Ctrl+B, I, Z and Shift+Z bold, italicise, undo and redo; Cmd/Ctrl+K opens the link box', async () => {
    const { target, changes, editor, surface } = await draw('Plan');
    editor().commands.selectAll();
    key(surface(), 'b');
    expect(changes.at(-1)).toBe('**Plan**\n');
    key(surface(), 'i');
    expect(changes.at(-1)).toBe('***Plan***\n');
    key(surface(), 'z');
    expect(changes.at(-1)).toBe('**Plan**\n');
    key(surface(), 'z', { shift: true });
    expect(changes.at(-1)).toBe('***Plan***\n');
    key(surface(), 'k');
    await settle();
    expect(target.querySelector('.md-link input')).not.toBeNull();
  });

  it('a value given from outside is loaded and is not a change of its own', async () => {
    const { changes, surface } = await draw('# Rates\n\nOld line\n');
    expect(surface().querySelector('h1')?.textContent).toBe('Rates');
    expect(surface().textContent).not.toContain('#');
    expect(changes).toEqual([]);
  });
});

describe('accessibility', () => {
  it('the toolbar is a toolbar of named icon buttons in groups, one tab stop and arrow keys; the editable area is named', async () => {
    const { target, surface } = await draw('Plan');
    const bar = target.querySelector('[role="toolbar"]') as HTMLElement;
    expect(bar.getAttribute('aria-label')?.replace(/[\u2068\u2069]/g, '')).toBe('Formatting for Body');
    const all = [...bar.querySelectorAll('button')];
    expect(all.map((b) => b.getAttribute('aria-label'))).toEqual(['Heading', 'Bold', 'Italic', 'Bulleted list', 'Numbered list', 'Link', 'Quote', 'Table', 'Undo', 'Redo']);
    // Icons only: no words in the buttons, one drawing each from the sprite.
    expect(all.every((b) => b.textContent?.trim() === '' && b.querySelectorAll('svg.icon use').length === 1)).toBe(true);
    expect(all.map((b) => b.querySelector('use')?.getAttribute('href'))).toEqual(['#i-heading', '#i-bold', '#i-italic', '#i-bullets', '#i-numbers', '#i-link', '#i-quote', '#i-sheet', '#i-undo', '#i-redo']);
    // Groups: Heading, Bold, Italic | Bulleted list, Numbered list | Link, Quote, Table | Undo, Redo.
    const groups = [...bar.children].map((el) => (el.getAttribute('role') === 'separator' ? '|' : el.getAttribute('data-tool'))).join(' ');
    expect(groups).toBe('heading bold italic | bullets numbers | link quote table | undo redo');
    expect(all.filter((b) => b.tabIndex === 0)).toHaveLength(1);
    all[0]?.focus();
    bar.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    flushSync();
    expect(document.activeElement).toBe(all[1]);
    bar.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
    flushSync();
    // Undo and Redo are disabled with nothing to undo, so End and the arrows pass over them.
    expect(document.activeElement).toBe(all[7]);
    bar.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    flushSync();
    expect(document.activeElement).toBe(all[0]);
    expect(surface().getAttribute('aria-label')).toBe('Body');
    expect(surface().getAttribute('role')).toBe('textbox');
  });

  it('each button has a tooltip with its shortcut, as the keyboard writes it', async () => {
    const { target } = await draw('Plan');
    const mac = /Mac|iP(hone|ad|od)/.test(navigator.platform);
    const tips = [...target.querySelectorAll('[role="toolbar"] button')].map((b) => b.getAttribute('title')?.replace(/[\u2068\u2069]/g, ''));
    expect(tips).toEqual(
      mac
        ? ['Heading', 'Bold (⌘B)', 'Italic (⌘I)', 'Bulleted list', 'Numbered list', 'Link (⌘K)', 'Quote', 'Table', 'Undo (⌘Z)', 'Redo (⇧⌘Z)']
        : ['Heading', 'Bold (Ctrl+B)', 'Italic (Ctrl+I)', 'Bulleted list', 'Numbered list', 'Link (Ctrl+K)', 'Quote', 'Table', 'Undo (Ctrl+Z)', 'Redo (Ctrl+Shift+Z)'],
    );
    expect(button(target, 'bold').getAttribute('aria-keyshortcuts')).toBe(mac ? 'Meta+B' : 'Control+B');
  });
});

describe('paste and links', () => {
  it('pasted HTML from Word or a web page loses scripts, styles, comments, handlers, Office tags and unsafe links', () => {
    const word = `<html xmlns:o="urn:schemas-microsoft-com:office:office"><head><style>p.MsoNormal{color:red}</style><script>alert(1)</script></head>
      <body><!--[if gte mso 9]><xml>x</xml><![endif]--><h2 style="color:red" onclick="x()">Terms</h2><p class="MsoNormal"><b>Thirty</b> <span style="font-family:Calibri">days</span><o:p></o:p></p>
      <ul><li><i>one</i></li></ul><a href="javascript:alert(1)">bad</a> <a href='https://example.com' onmouseover="x()">good</a>
      <img src="x" onerror="alert(1)"><table><tr><td colspan="2" style="x">cell</td></tr></table><iframe src="https://e.example"></iframe></body></html>`;
    const out = sanitisePastedHtml(word);
    expect(out).not.toMatch(/script|style|onclick|onmouseover|onerror|javascript|<img|<iframe|class=|o:p|<xml|mso/i);
    expect(out).toContain('<h2>Terms</h2>');
    expect(out).toContain('<b>Thirty</b> <span>days</span>');
    expect(out).toContain('<li><i>one</i></li>');
    expect(out).toContain('<a>bad</a> <a href="https://example.com">good</a>');
    expect(out).toContain('<td colspan="2">cell</td>');
  });

  it('pasted HTML becomes clean Markdown in the editor, keeping what it can', async () => {
    const { changes, editor } = await draw('');
    editor().view.pasteHTML(
      '<html><head><style>h1{color:red}</style></head><body><h1 style="x" onclick="bad()">Title</h1><p class="MsoNormal"><strong>Bold</strong> and <a href="https://a.example">a link</a> and <a href="file:///etc/passwd">a file</a></p><ul><li>one</li></ul><script>bad()</script><table><tr><th>A</th></tr><tr><td>1</td></tr></table></body></html>',
    );
    flushSync();
    const md = changes.at(-1) ?? '';
    expect(md).toContain('# Title\n\n**Bold** and [a link](https://a.example) and a file\n\n- one\n');
    expect(md).toMatch(/\| A +\|\n\| -+ \|\n\| 1 +\|/);
    expect(md).not.toMatch(/bad|script|color|file:/);
  });

  it('only http(s) and mailto are links a person types; a document may also link within its place', () => {
    expect(normaliseTypedHref('https://a.example')).toBe('https://a.example');
    expect(normaliseTypedHref('a@b.example')).toBe('mailto:a@b.example');
    expect(normaliseTypedHref('a.example/x')).toBe('https://a.example/x');
    expect(normaliseTypedHref('javascript:alert(1)')).toBeNull();
    expect(normaliseTypedHref('file:///etc/passwd')).toBeNull();
    expect(documentHrefAllowed('Notes/plan.md')).toBe(true);
    expect(documentHrefAllowed('//evil.example')).toBe(false);
    expect(documentHrefAllowed('data:text/html,x')).toBe(false);
  });
});
