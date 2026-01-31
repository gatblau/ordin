// A part's kind in plain words: the Fluent id AgentModel.md's kind table gives each `PartKind`.
// `part-kind-not-offered` kinds are never offered to the person for choosing.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-10
import type { PartKind } from '../../service-api/types.gen';

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
  SourceWorkflowOverlay: 'part-kind-code-workflow',
  StyleOverlay: 'part-kind-writing-style',
  LanguageOverlay: 'part-kind-language',
  HandoffOverlay: 'part-kind-hand-over',
  Evaluation: 'part-kind-rehearsal-checks',
  OutputContract: 'part-kind-result-must-hold',
  Vocabulary: 'part-kind-words',
  Guideline: 'part-kind-writing-guide',
  Memory: 'part-kind-memory-rules',
  ControlLoop: 'part-kind-way-of-working',
};

/** The kinds a person can choose when importing one part. */
export const OFFERED_PART_KINDS: readonly PartKind[] = (Object.keys(PART_KIND_MESSAGE) as PartKind[]).filter((k) => PART_KIND_MESSAGE[k] !== 'part-kind-not-offered');
