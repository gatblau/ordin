// The what-you-see Markdown editor (TipTap, ProseMirror, marked) under the window's frozen
// `Object.prototype` (OrdinDesktop `tauri.conf.json`: `freezePrototype`). As for ECharts and pdf.js
// (echarts-frozen-prototype.test.ts, pdf-frozen-prototype.test.ts), a Node process freezes the prototype
// first, as the window does, then loads the editor's packages and edits a text: Markdown in, the toolbar's
// commands, undo, a table, and Markdown out. Checked January 2026: no rewrite of the packages is needed.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-4
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const UI = fileURLToPath(new URL('..', import.meta.url));

function edit(frozen: boolean): string {
  const script = `
    const { Window } = await import('happy-dom');
    const w = new Window();
    for (const k of ['window', 'document', 'Node', 'HTMLElement', 'Element', 'DOMParser', 'getComputedStyle', 'MutationObserver', 'Text', 'DocumentFragment', 'KeyboardEvent', 'ClipboardEvent', 'Event', 'requestAnimationFrame', 'cancelAnimationFrame', 'getSelection']) {
      Object.defineProperty(globalThis, k, { value: w[k] ?? w, configurable: true, writable: true });
    }
    if (${frozen}) Object.freeze(Object.prototype);
    try {
      const { Editor } = await import('@tiptap/core');
      const { default: StarterKit } = await import('@tiptap/starter-kit');
      const { TableKit } = await import('@tiptap/extension-table');
      const { Placeholder } = await import('@tiptap/extensions');
      const { Markdown } = await import('@tiptap/markdown');
      const element = document.createElement('div');
      document.body.appendChild(element);
      const e = new Editor({ element, injectCSS: false, extensions: [StarterKit.configure({ underline: false }), TableKit, Placeholder.configure({ placeholder: 'x' }), Markdown] });
      e.commands.setContent('# Rates\\n\\nOld [line](https://a.example)', { contentType: 'markdown' });
      e.chain().selectAll().toggleBold().run();
      e.chain().undo().run();
      e.chain().focus('end').toggleBulletList().insertTable({ rows: 2, cols: 2, withHeaderRow: true }).run();
      e.commands.insertContent('<p><em>pasted</em></p>');
      console.log(e.getMarkdown().split('\\n')[0] + '|' + e.isActive('table') + '|' + (e.getMarkdown().includes('*pasted*')));
      e.destroy();
    } catch (error) {
      console.log('failed: ' + error.message);
    }
  `;
  return execFileSync(process.execPath, ['--input-type=module', '-e', script], { cwd: UI, encoding: 'utf8' }).trim();
}

describe('the Markdown editor loads and edits under the frozen Object.prototype', () => {
  it('edits a text with the prototype as it ships', () => {
    expect(edit(false)).toBe('# Rates|true|true');
  });

  it('edits the same text with Object.prototype frozen, as the window freezes it', () => {
    expect(edit(true)).toBe('# Rates|true|true');
  });
});
