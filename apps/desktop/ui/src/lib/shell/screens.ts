// The screen registry. Every screen has an identifier; the shell opens a screen by identifier and the
// module that owns the screen registers it, so two workers never edit the same file:
//
//   // lib/views/<area>/register.ts  (picked up by lib/shell/load-registrations.ts)
//   registerScreen({ id: 'home', titleId: 'rail-home', component: Home, commands: ['home.view'] });
//
// A screen key is `<id>` or `<id>:<instance>` (for example `document:<place id>`); at most 6 are
// open per workspace (Data Model, `ordin.view.tabs`).
import type { Component } from 'svelte';

import type { BackendState } from '../store/backend-state.svelte';

export interface ScreenProps {
  backend: BackendState;
  /** The workspace the screen is open in; `null` for a screen with no workspace (Settings). */
  workspaceId: string | null;
  /** The part of the key after the first `:`, or `null`. */
  instance: string | null;
}

export interface ScreenDef {
  id: string;
  /** Message id of the tab title when the opener gives no text title. */
  titleId: string;
  component: Component<ScreenProps>;
  /** Commands the screen sends; the screen is offered only when the backend answers every one (SM-UP-1). */
  commands?: readonly string[];
}

/** The identifiers of the fixed screens (11 §4, §5.15) and of the tree's groups. Other modules register under these. */
export const SCREEN_IDS = {
  home: 'home',
  settings: 'settings',
  diagnostics: 'diagnostics',
  earlierConversations: 'earlier-conversations',
  documents: 'documents',
  agents: 'agents',
  library: 'library',
  workflows: 'workflows',
  dashboards: 'dashboards',
  email: 'email',
  telegram: 'telegram',
  slack: 'slack',
  running: 'running',
  knowledge: 'knowledge',
  memory: 'memory',
  approval: 'approval',
  document: 'document',
  run: 'run',
  crew: 'crew',
} as const;

/** Title message ids of the fixed screens, used by the placeholder until a module registers the real one. */
export const FIXED_TITLES: Readonly<Record<string, string>> = {
  [SCREEN_IDS.home]: 'rail-home',
  [SCREEN_IDS.settings]: 'screen-settings',
  [SCREEN_IDS.diagnostics]: 'screen-diagnostics',
  [SCREEN_IDS.earlierConversations]: 'conv-earlier-name',
  [SCREEN_IDS.documents]: 'tree-documents',
  [SCREEN_IDS.agents]: 'tree-agents',
  [SCREEN_IDS.library]: 'library-title',
  [SCREEN_IDS.workflows]: 'tree-workflows',
  [SCREEN_IDS.dashboards]: 'tree-dashboards',
  [SCREEN_IDS.email]: 'tree-email',
  [SCREEN_IDS.telegram]: 'tree-telegram',
  [SCREEN_IDS.slack]: 'tree-slack',
  [SCREEN_IDS.running]: 'tree-running',
  [SCREEN_IDS.knowledge]: 'tree-knowledge',
  [SCREEN_IDS.memory]: 'tree-memory',
  [SCREEN_IDS.approval]: 'screen-approval',
  [SCREEN_IDS.document]: 'screen-document',
  [SCREEN_IDS.run]: 'screen-run',
  [SCREEN_IDS.crew]: 'screen-crew',
};

const registry = new Map<string, ScreenDef>();

/** Registers a screen under its identifier; a second registration of the same identifier replaces the first. */
export function registerScreen(def: ScreenDef): void {
  registry.set(def.id, def);
}

export function makeKey(id: string, instance?: string | null): string {
  return instance === undefined || instance === null || instance === '' ? id : `${id}:${instance}`;
}

export function parseKey(key: string): { id: string; instance: string | null } {
  const at = key.indexOf(':');
  return at < 0 ? { id: key, instance: null } : { id: key.slice(0, at), instance: key.slice(at + 1) };
}

export function getScreen(keyOrId: string): ScreenDef | undefined {
  return registry.get(parseKey(keyOrId).id);
}

export function registeredScreens(): ScreenDef[] {
  return [...registry.values()];
}

/** The message id of a screen's title: the registered one, else the fixed one, else `null`. */
export function titleIdOf(key: string): string | null {
  const id = parseKey(key).id;
  return registry.get(id)?.titleId ?? FIXED_TITLES[id] ?? null;
}

/** Test support: clears the registry. */
export function clearScreens(): void {
  registry.clear();
}

// ---- the *Needs you* tab's extra lines (the update line, What's new) -----------------------------

export interface NeedsYouSlot {
  id: string;
  component: Component<{ backend: BackendState }>;
}
const slots = new Map<string, NeedsYouSlot>();
export function registerNeedsYouSlot(slot: NeedsYouSlot): void {
  slots.set(slot.id, slot);
}
export function needsYouSlots(): NeedsYouSlot[] {
  return [...slots.values()];
}

// ---- how the conversation draws a block (registered by the blocks module) -------------------------

export interface BlockViewProps {
  backend: BackendState;
  workspaceId: string;
  blockId: string;
  block: import('../service-api/types.gen').Block;
}
let blockView: Component<BlockViewProps> | null = null;
export function registerBlockView(component: Component<BlockViewProps> | null): void {
  blockView = component;
}
export function getBlockView(): Component<BlockViewProps> | null {
  return blockView;
}

/**
 * The screen key an `OpenTarget` opens (*Needs you*, Home, notifications); `null` when it opens no screen
 * of its own (a workspace, or a block in the conversation).
 */
export function screenKeyFor(target: import('../service-api/types.gen').OpenTarget): string | null {
  switch (target.kind) {
    case 'approval':
    case 'hosted_approval':
      return makeKey(SCREEN_IDS.approval, target.approval_id);
    case 'document':
      // spec: ordin-desktop/components/ScreensModule#SM-CU-2
      return makeKey(SCREEN_IDS.document, `${target.place_id}:${target.path}`);
    case 'run':
      return makeKey(SCREEN_IDS.run, target.run_id);
    case 'crew_run':
      return makeKey(SCREEN_IDS.crew, target.crew_run_id);
    default:
      return null;
  }
}

// ---- overlays drawn over the whole window (Search or go to…) -----------------------------------------

export interface OverlayDef {
  id: string;
  /** The component draws itself only while its own condition holds (for example `shell.searchOpen`). */
  component: Component<Record<string, never>>;
}
const overlayRegistry = new Map<string, OverlayDef>();
export function registerOverlay(def: OverlayDef): void {
  overlayRegistry.set(def.id, def);
}
export function overlays(): OverlayDef[] {
  return [...overlayRegistry.values()];
}
