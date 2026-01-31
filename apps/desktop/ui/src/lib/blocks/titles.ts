// The title a block carries in a tab and on its one-line card: the text its data holds where it holds a
// title, otherwise a catalogue word. Titles from data are drawn as text; a title cut from the first line of a
// Markdown text shows its words, not its marks (SM-MSG-1).
import { t } from '../i18n/fluent';
import type { Block } from '../service-api/types.gen';

const FIRST_LINE = 80;

/** A line of Markdown as the words it holds: the heading, list, quote, bold, italic, code and link marks are not shown (SM-MSG-1). */
export function plainLine(line: string): string {
  return line
    .replace(/^\s{0,3}(?:#{1,6}|[-*+]|\d+[.)]|>)\s+/, '')
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/`([^`]*)`/g, '$1')
    .replace(/(\*\*|__)(.+?)\1/g, '$2')
    .replace(/(^|[\s(])[*_](\S(?:.*?\S)?)[*_](?=$|[\s).,;:!?])/g, '$1$2')
    .trim();
}

function firstLine(text: string): string {
  const line = plainLine(text.split('\n')[0] ?? '');
  return line.length > FIRST_LINE ? `${line.slice(0, FIRST_LINE - 1)}…` : line;
}

export function blockTitle(block: Block): string {
  switch (block.kind) {
    case 'message':
      return firstLine(block.text);
    case 'question':
      return firstLine(block.question);
    case 'form':
    case 'table':
    case 'chart':
    case 'report':
      return block.title;
    case 'documents':
      return t('tree-documents');
    case 'preview':
      return t('block-title-preview');
    case 'document':
      return t('block-title-document');
    case 'changes':
      return t('block-title-changes');
    case 'plan_progress':
      return t('screen-run');
    case 'key_figures':
      return t('block-title-key-figures');
    case 'email_list':
    case 'email':
      return t('tree-email');
    case 'telegram_list':
      return t('tree-telegram');
    case 'slack_list':
      return t('tree-slack');
    case 'result':
      return block.of === 'produced' ? firstLine(block.summary) : block.of === 'checklist' ? block.title : t('block-title-offer');
    case 'agent_proposal':
    case 'agent_library':
      return t('tree-agents');
    case 'crew_plan':
      return t('block-title-crew-plan');
    case 'memory':
      return t('tree-memory');
    case 'knowledge':
      return t('tree-knowledge');
    case 'access':
      return firstLine(block.text);
  }
}
