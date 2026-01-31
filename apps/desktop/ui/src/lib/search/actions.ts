// The Actions group of *Search or go to…* (SM-SF-9). The screens read an Actions entry through one
// function, `actionScreen`; when HomeAndSearch.md gives the group its own result type, only this
// function changes.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-9
import type { GoToResult } from '../service-api/types.gen';

export const SEARCH_ACTION_NAMES = ['home', 'new_workspace', 'add_documents', 'summarise', 'clear', 'earlier_conversations', 'settings'] as const;
export type SearchActionName = (typeof SEARCH_ACTION_NAMES)[number];

/** Catalogue id of the label drawn for each action (the entry's own `label` is never drawn). */
export const ACTION_LABEL_IDS: Readonly<Record<SearchActionName, string>> = {
  home: 'search-action-home',
  new_workspace: 'search-action-new-workspace',
  add_documents: 'search-action-add-documents',
  summarise: 'search-action-summarise',
  clear: 'search-action-clear',
  earlier_conversations: 'search-action-earlier-conversations',
  settings: 'search-action-settings',
};

/** The four actions that act on the workspace in front. */
export const WORKSPACE_ACTIONS: ReadonlySet<SearchActionName> = new Set(['add_documents', 'summarise', 'clear', 'earlier_conversations']);

/**
 * The commands an action sends, for SM-UP-1 (SM-SF-9.5). `settings`, `home` and `earlier_conversations` only
 * open a screen, whose own commands gate it.
 */
export const ACTION_COMMANDS: Readonly<Record<SearchActionName, readonly string[]>> = {
  home: ['home.view'],
  new_workspace: ['workspaces.create'],
  add_documents: ['files.add_place'],
  summarise: ['ordin.summarise'],
  clear: ['ordin.prepare_start_fresh'],
  earlier_conversations: ['ordin.list_conversations'],
  settings: ['settings.get'],
};

/** Today an entry is a `GoToResult` and the name is `opens.name` when `opens.kind` is `screen`. */
export function actionScreen(entry: GoToResult): SearchActionName | null {
  if (entry.opens.kind !== 'screen') return null;
  const name = entry.opens.name;
  return (SEARCH_ACTION_NAMES as readonly string[]).includes(name) ? (name as SearchActionName) : null;
}

/**
 * Handlers other modules register for the actions that act on their own surface (the rail's New workspace,
 * the conversation panel's Summarise and Clear, Add to this workspace). `home`, `settings` and
 * `earlier_conversations` open a screen and need none.
 */
export type ActionHandler = (workspaceId: string | null) => void;
const handlers = new Map<SearchActionName, ActionHandler>();
export function registerActionHandler(name: SearchActionName, handler: ActionHandler): void {
  handlers.set(name, handler);
}
export function actionHandler(name: SearchActionName): ActionHandler | undefined {
  return handlers.get(name);
}
export function clearActionHandlers(): void {
  handlers.clear();
}
