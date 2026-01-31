// Which icon a place draws (cross-cutting/VisualDesign.md V8 *Where icons are drawn*), transcribed from
// the prototype's maps `KIND_ICO`, `ORIGIN_ICO`, `KICO` and the screen tabs' kinds.
//
// spec: ordin-desktop/components/ScreensModule#SM-IC-3
import type { IconName } from './sprite';

/** A one-line block card's icon by block kind; an approval's card `info`; every other kind `docs`. */
const BLOCK_KIND_ICON: Readonly<Record<string, IconName>> = {
  documents: 'docs',
  preview: 'word',
  document: 'md',
  changes: 'edit',
  plan_progress: 'run',
  result: 'task',
  memory: 'memory',
  knowledge: 'know',
  agent_library: 'agent',
  approval: 'info',
};

export function blockKindIcon(kind: string): IconName {
  return BLOCK_KIND_ICON[kind] ?? 'docs';
}

/** A screen tab's icon by screen identifier, for the screens other than a block's (V8 *Screen tab*). */
const SCREEN_ICON: Readonly<Record<string, IconName>> = {
  'earlier-conversations': 'history',
  terminal: 'term',
  'code-editor': 'code',
  'changes-waiting': 'edit',
  workflow: 'keptcrew',
  agents: 'agent',
  'earlier-conversation': 'chat',
  'agent-draft': 'agent',
  'add-to-workspace': 'plus',
  approval: 'info',
};

/**
 * A screen tab's kind icon: a block's screen draws its one-line card's icon (`blockKind`, when the block
 * is known), the screens V8 names theirs, and any other screen `docs`.
 */
export function screenTabIcon(screenId: string, blockKind?: string): IconName {
  if (screenId === 'block') return blockKind === undefined ? 'docs' : blockKindIcon(blockKind);
  return SCREEN_ICON[screenId] ?? 'docs';
}

/**
 * A file's kind icon (`KIND_ICO`), from its name's extension (*chosen here*: the screens hold file names,
 * not the prototype's kind words); `null` when the extension names none of V8's file kinds, so the place
 * applies its own fallback (`i-word`, `i-page`).
 */
export function fileKindIcon(name: string): IconName | null {
  const dot = name.lastIndexOf('.');
  const ext = dot < 0 ? '' : name.slice(dot + 1).toLowerCase();
  switch (ext) {
    case 'doc':
    case 'docx':
    case 'odt':
    case 'rtf':
      return 'word';
    case 'xls':
    case 'xlsx':
    case 'ods':
    case 'csv':
      return 'sheet';
    case 'ppt':
    case 'pptx':
    case 'odp':
      return 'slides';
    case 'pdf':
      return 'pdf';
    case 'md':
    case 'markdown':
      return 'md';
    case 'png':
    case 'jpg':
    case 'jpeg':
    case 'gif':
    case 'webp':
    case 'svg':
      return 'image';
    default:
      return null;
  }
}

/** The tint of a file-kind icon (V8 *Icon tile colour*): its kind's colour, `i-page` Confluence's, `i-md` none. */
export function fileKindTint(icon: IconName): string | undefined {
  switch (icon) {
    case 'word':
    case 'sheet':
    case 'slides':
    case 'pdf':
    case 'image':
      return icon;
    case 'page':
      return 'confluence';
    default:
      return undefined;
  }
}

/** A part of an agent repository by kind (`ptIco`). */
export function partKindIcon(kind: string): IconName {
  switch (kind) {
    case 'WorkspaceProfile':
      return 'agent';
    case 'Guideline':
      return 'md';
    case 'Rule':
    case 'OperatingMode':
      return 'key';
    case 'Memory':
      return 'memory';
    case 'ContextResource':
      return 'docs';
    case 'ToolBinding':
      return 'code';
    case 'Journey':
      return 'run';
    default:
      return 'task';
  }
}
