// SM-SF-6: an agent's citation reads as the agent's. Wherever a `FactView` is drawn, a fact whose
// `came_from.kind` is `agent_citing` shows its source line as the agent's claim, never as a plain document
// source, and `origin_ref` is text, not a link.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-6
import { t } from '../i18n/fluent';
import type { AgentInstance, FactView } from '../service-api/types.gen';

/** The source line of a fact that an agent cited; `null` for any other origin. */
export function citationLine(fact: FactView, agents: ReadonlyArray<Pick<AgentInstance, 'id' | 'display_name'>>): string | null {
  const from = fact.came_from;
  if (from.kind !== 'agent_citing') return null;
  const agent = agents.find((a) => a.id === from.agent_instance_id);
  return agent === undefined
    ? t('memory-cited-by-an-agent', { source: from.origin_ref, version: from.version })
    : t('memory-cited-by-agent', { agent: agent.display_name, source: from.origin_ref, version: from.version });
}

/** The kind of a fact in words (`memory-kind-*`). */
export function kindLabel(kind: FactView['kind']): string {
  return kind === 'happened' ? t('memory-kind-happened') : kind === 'summary' ? t('memory-kind-summary') : t('memory-kind-known');
}
