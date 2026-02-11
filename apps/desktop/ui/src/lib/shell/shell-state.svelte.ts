// What the shell itself holds: which workspace is open, which conversation tab is in front, narrow
// mode, the quit request and the text of the polite live region. Nothing here is in the journal;
// the open workspace and tabs are view state.
import type { BackendId } from '../backend/backend';

export const NEEDS_YOU_TAB = 'needs-you';
/** The narrow mode starts at a window this wide or narrower (prototype `@media (max-width:1100px)`; ScreensModule amendment January 2026, item 3). */
export const NARROW_AT_MOST_PX = 1100;

export interface ConversationTab {
  backend: BackendId;
  workspaceId: string;
}

export class ShellState {
  openBackend = $state<BackendId>('');
  openWorkspace = $state<string | null>(null);
  /** `needs-you` or `<backend id>:<workspace id>`. */
  activeConversation = $state<string>(NEEDS_YOU_TAB);
  conversationTabs = $state.raw<ConversationTab[]>([]);
  narrow = $state(false);
  conversationOverlayOpen = $state(false);
  conversationMark = $state(false);
  quitRequest = $state<{ running_sessions: number } | null>(null);
  /** SM-QT-1: ordin is closing; the window shows one line until it goes. */
  // spec: ordin-desktop/components/ScreensModule#SM-QT-1
  quitting = $state<{ running_sessions: number } | null>(null);
  announcement = $state('');
  /** Bumped after Continue this conversation: the conversation panel reads the new conversation and moves focus to the message box (SM-CC-4). */
  continuedEpoch = $state(0);
  /** *Search or go to…* is open; the search dialog (its own module) draws itself while this is true. */
  searchOpen = $state(false);

  setWidth(px: number): void {
    this.narrow = px <= NARROW_AT_MOST_PX;
    if (!this.narrow) this.conversationOverlayOpen = false;
  }

  /** Opens a workspace: the whole window switches to it, and its conversation tab is added once. */
  openWorkspaceTab(backend: BackendId, workspaceId: string): void {
    this.openBackend = backend;
    this.openWorkspace = workspaceId;
    if (!this.conversationTabs.some((t) => t.backend === backend && t.workspaceId === workspaceId)) {
      this.conversationTabs = [...this.conversationTabs, { backend, workspaceId }];
    }
    this.activeConversation = `${backend}:${workspaceId}`;
  }

  /** The tab key of a conversation tab. */
  static tabKey(tab: ConversationTab): string {
    return `${tab.backend}:${tab.workspaceId}`;
  }

  announce(text: string): void {
    // A repeated sentence must still be announced: clear first.
    this.announcement = '';
    queueMicrotask(() => {
      this.announcement = text;
    });
  }

  /** SM-CC-4: the continued conversation is now the workspace's live one. */
  afterContinue(): void {
    this.continuedEpoch += 1;
  }

  noteNewInConversation(): void {
    if (this.narrow && !this.conversationOverlayOpen) this.conversationMark = true;
  }
}
