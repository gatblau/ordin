// A small Markdown reader that produces a typed tree, not markup: the components draw the tree with Svelte
// elements, so text is always text (ScreensModule step 6). Links are drawn as their words only: a link is
// never a URL the page can follow (2C §7). Raw HTML in the source is text.

export type Inline =
  | { kind: 'text'; text: string }
  | { kind: 'strong'; text: string }
  | { kind: 'emphasis'; text: string }
  | { kind: 'code'; text: string }
  /** A link: its words and where it points, kept as text. Drawn as words only unless the view asks for targets. */
  | { kind: 'link'; text: string; target: string };

export type MdBlock =
  | { kind: 'heading'; level: 1 | 2 | 3 | 4 | 5 | 6; inlines: Inline[] }
  | { kind: 'paragraph'; inlines: Inline[] }
  | { kind: 'list'; ordered: boolean; items: Inline[][] }
  | { kind: 'quote'; inlines: Inline[] }
  | { kind: 'code'; text: string }
  | { kind: 'rule' };

const INLINE = /(\*\*[^*\n]+\*\*|\*[^*\n]+\*|`[^`\n]+`|\[[^\]\n]*\]\([^)\n]*\))/g;

export function parseInline(source: string): Inline[] {
  const out: Inline[] = [];
  let last = 0;
  for (const m of source.matchAll(INLINE)) {
    const at = m.index ?? 0;
    if (at > last) out.push({ kind: 'text', text: source.slice(last, at) });
    const token = m[0];
    if (token.startsWith('**')) out.push({ kind: 'strong', text: token.slice(2, -2) });
    else if (token.startsWith('`')) out.push({ kind: 'code', text: token.slice(1, -1) });
    else if (token.startsWith('[')) {
      const close = token.indexOf(']');
      out.push({ kind: 'link', text: token.slice(1, close), target: token.slice(close + 2, -1) });
    }
    else out.push({ kind: 'emphasis', text: token.slice(1, -1) });
    last = at + token.length;
  }
  if (last < source.length) out.push({ kind: 'text', text: source.slice(last) });
  return out;
}

export function parseMarkdown(source: string): MdBlock[] {
  const lines = source.replace(/\r\n?/g, '\n').split('\n');
  const blocks: MdBlock[] = [];
  let paragraph: string[] = [];
  let list: { ordered: boolean; items: Inline[][] } | null = null;
  let quote: string[] = [];

  const flushParagraph = (): void => {
    if (paragraph.length > 0) blocks.push({ kind: 'paragraph', inlines: parseInline(paragraph.join(' ')) });
    paragraph = [];
  };
  const flushList = (): void => {
    if (list !== null) blocks.push({ kind: 'list', ordered: list.ordered, items: list.items });
    list = null;
  };
  const flushQuote = (): void => {
    if (quote.length > 0) blocks.push({ kind: 'quote', inlines: parseInline(quote.join(' ')) });
    quote = [];
  };
  const flushAll = (): void => {
    flushParagraph();
    flushList();
    flushQuote();
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] as string;
    if (line.trimStart().startsWith('```')) {
      flushAll();
      const code: string[] = [];
      i++;
      while (i < lines.length && !(lines[i] as string).trimStart().startsWith('```')) code.push(lines[i++] as string);
      blocks.push({ kind: 'code', text: code.join('\n') });
      continue;
    }
    if (line.trim() === '') {
      flushAll();
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading !== null) {
      flushAll();
      blocks.push({ kind: 'heading', level: (heading[1] as string).length as 1 | 2 | 3 | 4 | 5 | 6, inlines: parseInline((heading[2] as string).trim()) });
      continue;
    }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      flushAll();
      blocks.push({ kind: 'rule' });
      continue;
    }
    const bullet = /^\s*[-*+]\s+(.*)$/.exec(line);
    const numbered = /^\s*\d+[.)]\s+(.*)$/.exec(line);
    if (bullet !== null || numbered !== null) {
      flushParagraph();
      flushQuote();
      const ordered = numbered !== null;
      if (list !== null && list.ordered !== ordered) flushList();
      list ??= { ordered, items: [] };
      list.items.push(parseInline(((bullet ?? numbered) as RegExpExecArray)[1] as string));
      continue;
    }
    const quoted = /^\s*>\s?(.*)$/.exec(line);
    if (quoted !== null) {
      flushParagraph();
      flushList();
      quote.push(quoted[1] as string);
      continue;
    }
    flushList();
    flushQuote();
    paragraph.push(line.trim());
  }
  flushAll();
  return blocks;
}
