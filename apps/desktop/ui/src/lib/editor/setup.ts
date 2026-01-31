// The what-you-see Markdown editor's engine (TipTap, MIT): the extensions it is built from, the kept part (a
// block the editor cannot show as formatted text, written back word for word), and Markdown in and out.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-4
import { Editor, Extension, Node, type JSONContent } from '@tiptap/core';
import { Markdown } from '@tiptap/markdown';
import { Fragment, Slice, type Node as PmNode } from '@tiptap/pm/model';
import StarterKit from '@tiptap/starter-kit';
import { TableKit } from '@tiptap/extension-table';
import { Placeholder } from '@tiptap/extensions';

import { documentHrefAllowed, escapeText, typedHrefAllowed, keptReading, sanitisePastedHtml, splitParts, tidyMarkdown, type MdToken } from './markdown';

/** A block kept word for word: shown as its words, never as codes, and not changed by the editor. */
export const KeptBlock = Node.create({
  name: 'keptBlock',
  group: 'block',
  atom: true,
  selectable: true,
  draggable: false,
  addOptions() {
    return { label: '' };
  },
  addAttributes() {
    return { raw: { default: '', rendered: false } };
  },
  parseHTML() {
    return [{ tag: 'div[data-ordin-kept]', getAttrs: (el) => ({ raw: (el as HTMLElement).dataset.ordinKept ?? '' }) }];
  },
  renderHTML({ node }) {
    return ['div', { 'data-ordin-kept': node.attrs.raw as string, class: 'kept', contenteditable: 'false', title: this.options.label as string }, keptReading(node.attrs.raw as string)];
  },
  renderMarkdown: (node: JSONContent) => String(node.attrs?.raw ?? ''),
});

/** Pasted content with every link that is not http(s) or mailto turned back into its words. */
export function pastedLinksOnly(fragment: Fragment): Fragment {
  const out: PmNode[] = [];
  fragment.forEach((node) => {
    if (node.isText) out.push(node.mark(node.marks.filter((m) => m.type.name !== 'link' || typedHrefAllowed(String(m.attrs.href ?? '')))));
    else out.push(node.copy(pastedLinksOnly(node.content)));
  });
  return Fragment.from(out);
}

export interface EditorHooks {
  label: string;
  placeholder: string;
  keptLabel: string;
  onlink: () => void;
  onchange: () => void;
  onselection: () => void;
}

/** Cmd/Ctrl+K opens the link box. */
const LinkKey = (onlink: () => void) =>
  Extension.create({
    name: 'ordinLinkKey',
    addKeyboardShortcuts() {
      return {
        'Mod-k': () => {
          onlink();
          return true;
        },
      };
    },
  });

/** The editor in `element`, holding `markdown`. */
export function createEditor(element: HTMLElement, markdown: string, hooks: EditorHooks): Editor {
  const editor = new Editor({
    element,
    injectCSS: false,
    extensions: [
      StarterKit.configure({
        underline: false,
        heading: { levels: [1, 2, 3, 4, 5, 6] },
        link: {
          openOnClick: false,
          autolink: true,
          linkOnPaste: true,
          protocols: [],
          isAllowedUri: (url: string) => documentHrefAllowed(url),
          HTMLAttributes: { rel: 'noopener noreferrer nofollow', target: null },
        },
      }),
      TableKit.configure({ table: { resizable: false } }),
      Placeholder.configure({ placeholder: hooks.placeholder }),
      KeptBlock.configure({ label: hooks.keptLabel }),
      LinkKey(hooks.onlink),
      Markdown,
    ],
    editorProps: {
      attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': hooks.label, class: 'md-surface', spellcheck: 'true' },
      transformPastedHTML: (html) => sanitisePastedHtml(html),
      // A pasted link that is not http(s) or mailto loses its link and keeps its words.
      transformPasted: (slice) => new Slice(pastedLinksOnly(slice.content), slice.openStart, slice.openEnd),
    },
    onUpdate: () => hooks.onchange(),
    onSelectionUpdate: () => hooks.onselection(),
    onTransaction: () => hooks.onselection(),
  });
  // Text is written back escaping only what would read again as formatting (see `escapeText`).
  const manager = editor.markdown as unknown as { encodeTextForMarkdown: (text: string, node: JSONContent, parent?: JSONContent) => string; codeTypes: Set<string> } | undefined;
  if (manager !== undefined) {
    const codeTypes = manager.codeTypes;
    Object.defineProperty(manager, 'encodeTextForMarkdown', {
      configurable: true,
      writable: true,
      value: (text: string, node: JSONContent, parent?: JSONContent): string => {
        if ((parent?.type !== undefined && codeTypes.has(parent.type)) || (node.marks ?? []).some((m) => codeTypes.has(m.type))) return text;
        return escapeText(text, parent?.content?.[0] === node);
      },
    });
  }
  setMarkdown(editor, markdown);
  return editor;
}

/** Replaces the editor's content with `markdown`; blocks it cannot show are kept word for word. */
export function setMarkdown(editor: Editor, markdown: string, emitUpdate = false): void {
  const manager = editor.markdown;
  if (manager === undefined) return;
  const lex = (md: string): MdToken[] => manager.instance.lexer(md) as unknown as MdToken[];
  const content: JSONContent[] = [];
  for (const part of splitParts(markdown.replace(/\r\n/g, '\n'), lex)) {
    if (part.kind === 'kept') content.push({ type: 'keptBlock', attrs: { raw: part.raw } });
    else content.push(...(manager.parse(part.markdown).content ?? []));
  }
  const doc = editor.schema.nodeFromJSON({ type: 'doc', content: content.length > 0 ? content : [{ type: 'paragraph' }] });
  // Loading is not a change a person can undo.
  const tr = editor.state.tr.replaceWith(0, editor.state.doc.content.size, doc.content).setMeta('addToHistory', false).setMeta('preventUpdate', !emitUpdate);
  editor.view.dispatch(tr);
}

/** The editor's content as Markdown. */
export function getMarkdown(editor: Editor): string {
  if (editor.isEmpty) return '';
  return tidyMarkdown(editor.getMarkdown());
}
