// ADR-104 step 1: wiki links in a page body drawn as links. `[[target|shown]]` shows `shown`,
// `[[target#section]]` shows `target › section`, `[[target]]` shows `target`, and `![[target]]` is drawn as
// a link (no embedding). The i-th link of the body matches the i-th entry of `links_out`.
//
// spec: ordin-desktop/components/ScreensModule#ADR-104
import { parseMarkdown, type Inline, type MdBlock } from '../../blocks/markdown';
import type { PageLinkView } from '../../service-api/types.gen';

export type BodySegment = { kind: 'text'; text: string } | { kind: 'link'; shown: string; link: PageLinkView | null };

const WIKI = /!?\[\[([^\]\n]+)\]\]/g;

export function linkShown(inner: string): string {
  const bar = inner.indexOf('|');
  if (bar >= 0) return inner.slice(bar + 1);
  const hash = inner.indexOf('#');
  if (hash >= 0) return `${inner.slice(0, hash)} › ${inner.slice(hash + 1)}`;
  return inner;
}

export function splitWikiLinks(body: string, linksOut: readonly PageLinkView[]): BodySegment[] {
  const out: BodySegment[] = [];
  let last = 0;
  let index = 0;
  for (const m of body.matchAll(WIKI)) {
    const at = m.index ?? 0;
    if (at > last) out.push({ kind: 'text', text: body.slice(last, at) });
    out.push({ kind: 'link', shown: linkShown(m[1] as string), link: linksOut[index] ?? null });
    index++;
    last = at + m[0].length;
  }
  if (last < body.length) out.push({ kind: 'text', text: body.slice(last) });
  return out;
}

/** The tag a chip puts in the filter box. */
export const tagFilter = (tag: string): string => `tag:${tag}`;

// ---- the page body as a formatted page (SM-KN-1) ------------------------------------------------------

/**
 * A piece of a page's text: Markdown's own pieces, a Markdown link that is no page link (its words only,
 * ScreensModule *Where a link goes*), or a page link with its `links_out` entry.
 */
export type PageInline =
  | { kind: 'text' | 'strong' | 'emphasis' | 'code'; text: string }
  | { kind: 'words'; text: string }
  | { kind: 'page_link'; shown: string; link: PageLinkView | null };

export type PageBlock =
  | { kind: 'heading'; level: 1 | 2 | 3 | 4 | 5 | 6; inlines: PageInline[] }
  | { kind: 'paragraph'; inlines: PageInline[] }
  | { kind: 'list'; ordered: boolean; items: PageInline[][] }
  | { kind: 'quote'; inlines: PageInline[] }
  | { kind: 'code'; text: string }
  | { kind: 'rule' };

const stripMd = (name: string): string => (name.length >= 3 && name.slice(-3).toLowerCase() === '.md' ? name.slice(0, -3) : name);

/** A target naming an extension other than `.md` (`image.png`) is no page link (KnowledgeService.md §P.2). */
function namesOtherExtension(target: string): boolean {
  const last = target.split('/').pop() ?? target;
  const dot = last.lastIndexOf('.');
  if (dot <= 0) return false;
  const ext = last.slice(dot + 1);
  return ext.length >= 1 && ext.length <= 8 && /^[A-Za-z0-9]+$/.test(ext) && /[A-Za-z]/.test(ext);
}

/** Whether the service keeps `[[inner]]` as a link (KnowledgeService.md §P.2): a non-empty target with no other extension. */
function keptWiki(inner: string): boolean {
  if (inner.includes('[[')) return false;
  const bar = inner.indexOf('|');
  const targetPart = bar >= 0 ? inner.slice(0, bar) : inner;
  const hash = targetPart.lastIndexOf('#');
  const target = (hash >= 0 ? targetPart.slice(0, hash) : targetPart).trim();
  return target !== '' && !namesOtherExtension(stripMd(target));
}

/**
 * Whether the service keeps a Markdown link to `dest` (KnowledgeService.md §P.3): a path ending in `.md`, with
 * no scheme, that stays under `knowledge/` when resolved against the linking page's folder.
 */
export function keptMarkdownLink(dest: string, pageRelativePath: string): boolean {
  let d = (dest.split(' "')[0] ?? dest).trim();
  if (d.startsWith('<') && d.endsWith('>')) d = d.slice(1, -1);
  const hash = d.indexOf('#');
  const pathRaw = hash >= 0 ? d.slice(0, hash) : d;
  if (pathRaw === '' || !pathRaw.toLowerCase().endsWith('.md')) return false;
  const firstSlash = pathRaw.indexOf('/');
  if ((firstSlash >= 0 ? pathRaw.slice(0, firstSlash) : pathRaw).includes(':')) return false;
  let path: string;
  try {
    path = decodeURIComponent(pathRaw);
  } catch {
    return false;
  }
  const below = pageRelativePath.startsWith('knowledge/') ? pageRelativePath.slice('knowledge/'.length) : pageRelativePath;
  const folder = below.includes('/') ? below.slice(0, below.lastIndexOf('/')) : '';
  const full = path.startsWith('/') ? path.slice(1) : folder === '' ? path : `${folder}/${path}`;
  const parts: string[] = [];
  for (const seg of full.split('/')) {
    if (seg === '' || seg === '.') continue;
    if (seg === '..') {
      if (parts.pop() === undefined) return false;
    } else parts.push(seg);
  }
  return parts.length > 0;
}

/**
 * The page body as Markdown blocks (the reader every Markdown document is drawn with), with each link the
 * service keeps in `links_out` paired with its entry in body order: wiki links and Markdown links to a page
 * file. Code holds no links. Any other Markdown link is its words only.
 */
// spec: ordin-desktop/components/ScreensModule#SM-KN-1
export function pageBlocks(body: string, linksOut: readonly PageLinkView[], pageRelativePath: string): PageBlock[] {
  let index = 0;
  const next = (): PageLinkView | null => linksOut[index++] ?? null;
  const inlines = (list: Inline[]): PageInline[] => {
    const out: PageInline[] = [];
    for (const part of list) {
      if (part.kind === 'code') out.push(part);
      else if (part.kind === 'link') {
        if (keptMarkdownLink(part.target, pageRelativePath)) out.push({ kind: 'page_link', shown: part.text, link: next() });
        else out.push({ kind: 'words', text: part.text });
      } else {
        let last = 0;
        for (const m of part.text.matchAll(WIKI)) {
          const inner = m[1] as string;
          if (!keptWiki(inner)) continue;
          const at = m.index ?? 0;
          if (at > last) out.push({ kind: part.kind, text: part.text.slice(last, at) });
          out.push({ kind: 'page_link', shown: linkShown(inner), link: next() });
          last = at + m[0].length;
        }
        if (last < part.text.length) out.push({ kind: part.kind, text: part.text.slice(last) });
      }
    }
    return out;
  };
  return parseMarkdown(body).map((block: MdBlock): PageBlock => {
    switch (block.kind) {
      case 'heading':
        return { kind: 'heading', level: block.level, inlines: inlines(block.inlines) };
      case 'paragraph':
        return { kind: 'paragraph', inlines: inlines(block.inlines) };
      case 'quote':
        return { kind: 'quote', inlines: inlines(block.inlines) };
      case 'list':
        return { kind: 'list', ordered: block.ordered, items: block.items.map(inlines) };
      default:
        return block;
    }
  });
}
