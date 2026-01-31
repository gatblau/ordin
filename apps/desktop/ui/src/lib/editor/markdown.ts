// The what-you-see Markdown editor's text rules (ScreensModule.md, *Writing*; UX guidelines, *Writing*):
// which parts of a Markdown text the editor can show as formatted text, how the parts it cannot show are kept
// word for word, how plain text is written back so it reads the same when read again, and how pasted HTML is
// cleaned before it reaches the editor. No editor library is imported here, so the rules are tested alone.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-4

/** A token as the Markdown reader (marked) gives it; only the fields read here. */
export interface MdToken {
  type: string;
  raw: string;
  href?: string;
  task?: boolean;
  tokens?: MdToken[];
  items?: MdToken[];
  header?: { tokens?: MdToken[] }[];
  rows?: { tokens?: MdToken[] }[][];
}

const BLOCKS = new Set(['paragraph', 'heading', 'list', 'list_item', 'blockquote', 'code', 'hr', 'table', 'space', 'text']);
const INLINES = new Set(['text', 'strong', 'em', 'del', 'link', 'codespan', 'br', 'escape']);

/** A link the editor keeps as a link: http(s), mailto, or a path within the same place (no scheme). */
export function documentHrefAllowed(href: string): boolean {
  const h = href.trim();
  if (/^(https?:|mailto:)/i.test(h)) return true;
  if (h.startsWith('//')) return false;
  return !/^[a-z][a-z0-9+.-]*:/i.test(h);
}

/** A link a person types or pastes: http(s) or mailto only. */
export function typedHrefAllowed(href: string): boolean {
  return /^(https?:\/\/[^\s]+|mailto:[^\s]+)$/i.test(href.trim());
}

/** What a person typed into the link box as a link, or `null` when it cannot be one. A bare address gets `https://`. */
export function normaliseTypedHref(text: string): string | null {
  const v = text.trim();
  if (v === '') return null;
  if (typedHrefAllowed(v)) return v;
  if (/^[^\s:/@]+@[^\s:/@]+\.[^\s:/@]+$/.test(v)) return `mailto:${v}`;
  if (/^[a-z0-9-]+(\.[a-z0-9-]+)+(\/\S*)?$/i.test(v)) return `https://${v}`;
  return null;
}

function inlineOk(tokens: MdToken[] | undefined): boolean {
  return (tokens ?? []).every((tk) => {
    if (!INLINES.has(tk.type)) return false;
    if (tk.type === 'link' && !documentHrefAllowed(tk.href ?? '')) return false;
    return inlineOk(tk.tokens);
  });
}

/** Whether a top-level block can be shown and edited as formatted text without losing anything. */
export function blockSupported(token: MdToken): boolean {
  if (!BLOCKS.has(token.type)) return false;
  switch (token.type) {
    case 'list':
      return (token.items ?? []).every((item) => item.task !== true && (item.tokens ?? []).every((tk) => (tk.type === 'text' ? inlineOk(tk.tokens) : blockSupported(tk))));
    case 'blockquote':
      return (token.tokens ?? []).every(blockSupported);
    case 'table':
      return (token.header ?? []).every((cell) => inlineOk(cell.tokens)) && (token.rows ?? []).every((row) => row.every((cell) => inlineOk(cell.tokens)));
    case 'code':
    case 'hr':
    case 'space':
      return true;
    default:
      return inlineOk(token.tokens);
  }
}

/** A part of the text: shown as formatted text, or kept word for word. */
export type Part = { kind: 'formatted'; markdown: string } | { kind: 'kept'; raw: string };

const FRONT_MATTER = /^---\r?\n[\s\S]*?\r?\n---[ \t]*(\r?\n|$)/;

/** Splits a Markdown text into the parts the editor shows formatted and the parts it keeps as written. */
export function splitParts(markdown: string, lex: (md: string) => MdToken[]): Part[] {
  const parts: Part[] = [];
  let rest = markdown;
  const front = FRONT_MATTER.exec(rest);
  if (front !== null) {
    parts.push({ kind: 'kept', raw: front[0].replace(/\s+$/, '') });
    rest = rest.slice(front[0].length);
  }
  let run = '';
  for (const token of lex(rest)) {
    if (blockSupported(token)) {
      run += token.raw;
      continue;
    }
    if (run.trim() !== '') parts.push({ kind: 'formatted', markdown: run });
    run = '';
    const raw = token.raw.replace(/\s+$/, '');
    if (raw !== '') parts.push({ kind: 'kept', raw });
  }
  if (run.trim() !== '') parts.push({ kind: 'formatted', markdown: run });
  return parts;
}

/** What a kept part reads as, without its Markdown or HTML codes (the editor shows this, never the codes). */
export function keptReading(raw: string): string {
  return raw
    .replace(FRONT_MATTER, (m) => m.replace(/^---\r?\n|\r?\n---[ \t]*\s*$/g, ''))
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<[^>]*>/g, '')
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/^\s*\[[^\]]+\]:\s*\S+.*$/gm, '')
    .replace(/^\s*[-*+] \[[ xX]\] /gm, '')
    .replace(/[*_`~#>]/g, '')
    .replace(/&nbsp;/g, ' ')
    .replace(/\n{2,}/g, '\n')
    .trim();
}

/**
 * Plain text written back as Markdown. Only what would otherwise be read again as formatting is escaped, so
 * `R&D`, `snake_case` and `[[Page]]` (a knowledge link) are written as typed.
 */
export function escapeText(text: string, atLineStart: boolean): string {
  let out = text
    .replace(/\\/g, '\\\\')
    .replace(/[`*~]/g, (c) => `\\${c}`)
    .replace(/(^|[^A-Za-z0-9])_|_(?=$|[^A-Za-z0-9])/g, (m) => m.replace('_', '\\_'))
    .replace(/&(?=#?[A-Za-z0-9]+;)/g, '&amp;')
    .replace(/<(?=[A-Za-z/!?])/g, '&lt;');
  // `[` and `]` are escaped except in a knowledge link (`[[Page]]`), which is written as typed.
  out = out
    .split(/(\[\[[^\]\n]+\]\])/)
    .map((piece, i) => (i % 2 === 1 ? piece : piece.replace(/[[\]]/g, (c) => `\\${c}`)))
    .join('');
  if (atLineStart) {
    out = out.replace(/^(\s*)(#{1,6}(?=\s|$)|>|[-+](?=\s)|=+\s*$)/, '$1\\$2').replace(/^(\s*\d+)([.)])(?=\s)/, '$1\\$2');
  }
  return out;
}

/** Collapses the extra blank lines the writer leaves around some blocks, outside fenced code. */
export function tidyMarkdown(markdown: string): string {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  const out: string[] = [];
  let fence: string | null = null;
  let blank = 0;
  for (const line of lines) {
    const opener = /^\s*(`{3,}|~{3,})/.exec(line);
    if (fence === null && opener !== null) fence = opener[1] ?? null;
    else if (fence !== null && opener !== null && line.trim().startsWith(fence)) {
      out.push(line);
      fence = null;
      blank = 0;
      continue;
    }
    if (fence === null && line.trim() === '') {
      blank += 1;
      if (blank > 1) continue;
    } else blank = 0;
    out.push(line);
  }
  const text = out.join('\n').replace(/^\n+/, '').replace(/\s+$/, '');
  return text === '' ? '' : `${text}\n`;
}

// ---- Paste ----------------------------------------------------------------------------------------------

const DROP_WITH_CONTENT = /<(script|style|title|head|iframe|object|embed|noscript|template|svg|math|xml|textarea|select|button)\b[\s\S]*?<\/\1\s*>/gi;
const DROP_TAG = /<\/?(script|style|iframe|object|embed|img|picture|source|video|audio|canvas|meta|link|base|input|form|frame|frameset)\b[^>]*>/gi;
const ATTRIBUTE = /\s([A-Za-z_:][-A-Za-z0-9_:.]*)(\s*=\s*("[^"]*"|'[^']*'|[^\s"'>]+))?/g;
const KEPT_ATTRIBUTES = new Set(['href', 'colspan', 'rowspan']);

/**
 * HTML pasted from Word, a web page or a mail, cleaned as text before the editor reads it: no scripts, no
 * styles, no frames or embedded objects, no comments (Word's conditional ones included), no Office tags
 * (`<o:p>`), and on the elements left no attribute but `href` (kept only when http(s) or mailto) and a table
 * cell's `colspan` and `rowspan`. The editor then reads what is left against its own schema, in a detached
 * document, which keeps headings, emphasis, lists, quotes, code, links and tables and drops everything else.
 */
export function sanitisePastedHtml(html: string): string {
  return html
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<!\[[\s\S]*?\]>/g, '')
    .replace(DROP_WITH_CONTENT, '')
    .replace(DROP_TAG, '')
    .replace(/<\/?[A-Za-z]+:[^>]*>/g, '')
    .replace(/<([A-Za-z][A-Za-z0-9]*)((?:\s+[^>]*?)?)(\/?)>/g, (_m, tag: string, attrs: string, close: string) => {
      const kept: string[] = [];
      for (const a of attrs.matchAll(ATTRIBUTE)) {
        const name = (a[1] ?? '').toLowerCase();
        const raw = a[3] ?? '';
        const value = raw.replace(/^["']|["']$/g, '').trim();
        if (!KEPT_ATTRIBUTES.has(name)) continue;
        if (name === 'href' && !typedHrefAllowed(value)) continue;
        if (name !== 'href' && !/^\d{1,2}$/.test(value)) continue;
        kept.push(` ${name}="${value.replace(/"/g, '&quot;').replace(/</g, '&lt;')}"`);
      }
      return `<${tag.toLowerCase()}${kept.join('')}${close}>`;
    });
}
