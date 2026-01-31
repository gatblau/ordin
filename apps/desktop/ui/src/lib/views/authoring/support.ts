// Small helpers the authoring screens share: the instance of a document screen, the plain words of a part
// kind, opening a sibling screen and closing the screen the person is on.
//
// spec: ordin-desktop/components/ScreensModule#SM-CU-2
// spec: ordin-desktop/components/ScreensModule#SM-NS-7
import type { PartKind } from '../../service-api/types.gen';
import { openScreen } from '../../shell/open-item';
import { ScreenAreaState } from '../../shell/screen-area.svelte';
import type { AppContext } from '../../shell/context';
import type { BackendState } from '../../store/backend-state.svelte';

/** `<place id>:<path>`, split at the first `:` (a place id holds no `:`; a path may). */
export function splitDocumentInstance(instance: string | null): { placeId: string; path: string } {
  const value = instance ?? '';
  const at = value.indexOf(':');
  return at < 0 ? { placeId: value, path: '' } : { placeId: value.slice(0, at), path: value.slice(at + 1) };
}

/**
 * The message id of a part kind in plain words, from the kind table of AgentModel.md
 * (`crates/ordin-agent-model/src/kinds.rs`, field `plain_kind_message_id`). The kinds without a row of
 * their own in that table read `part-kind-not-offered`.
 */
export const PART_KIND_MESSAGE: Readonly<Record<PartKind, string>> = {
  SystemRole: 'part-kind-role',
  Rule: 'part-kind-rule',
  OperatingMode: 'part-kind-working-mode',
  Skill: 'part-kind-ability',
  Command: 'part-kind-task',
  ContextResource: 'part-kind-reference-material',
  ToolBinding: 'part-kind-tool',
  AgentIdentity: 'part-kind-not-offered',
  Capability: 'part-kind-group-of-abilities',
  WorkspaceProfile: 'part-kind-agent',
  CrewProfile: 'part-kind-not-offered',
  Journey: 'part-kind-lifecycle',
  PracticePack: 'part-kind-working-practice',
  BrandOverlay: 'part-kind-not-offered',
  JurisdictionOverlay: 'part-kind-not-offered',
  LanguageOverlay: 'part-kind-language',
  SourceWorkflowOverlay: 'part-kind-code-workflow',
  StyleOverlay: 'part-kind-writing-style',
  HandoffOverlay: 'part-kind-hand-over',
  Evaluation: 'part-kind-rehearsal-checks',
  OutputContract: 'part-kind-result-must-hold',
  Vocabulary: 'part-kind-words',
  Guideline: 'part-kind-writing-guide',
  Memory: 'part-kind-memory-rules',
  ControlLoop: 'part-kind-way-of-working',
};

/** The kinds a person can start a new part of: the rows of the kind table whose `offered` column is true. */
export const NEW_PART_KINDS: readonly PartKind[] = [
  'SystemRole', 'Rule', 'OperatingMode', 'Skill', 'Command', 'ContextResource', 'ToolBinding', 'Capability', 'WorkspaceProfile',
  'PracticePack', 'LanguageOverlay', 'SourceWorkflowOverlay', 'StyleOverlay', 'HandoffOverlay', 'Evaluation', 'OutputContract',
  'Vocabulary', 'Guideline', 'Memory', 'ControlLoop',
];

/**
 * Opens a screen in the workspace in front (or the backend, without a workspace); a no-op with no app
 * context. `app` is read by the calling component while it is set up (`appOrNull()` at the top of its
 * script): Svelte's getContext answers nothing later, in a click handler or a callback (plan 04 FU-005).
 */
export function openSibling(app: AppContext | null, backend: BackendState, workspaceId: string | null, key: string, title?: string): void {
  if (app === null) return;
  openScreen(app, backend.handle.id, workspaceId, key, title === undefined ? {} : { title });
}

/** Closes the screen with `key` in the scope of the backend and workspace; a no-op with no app context. */
export function closeScreen(app: AppContext | null, backend: BackendState, workspaceId: string | null, key: string): void {
  app?.screens.close(ScreenAreaState.scope(backend.handle.id, workspaceId), key);
}
