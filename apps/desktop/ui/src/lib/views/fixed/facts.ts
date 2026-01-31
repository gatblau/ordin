// Pure helpers for drawing a fact: the kind in words, the agent's name, the citation line (SM-SF-6).
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-6
import { formatDate } from '../../format/intl';
import type { FactView, InstalledAgent, MemoryKind } from '../../service-api/types.gen';

export const KIND_MESSAGE: Readonly<Record<MemoryKind, string>> = {
  happened: 'memory-kind-happened',
  known: 'memory-kind-known',
  summary: 'memory-kind-summary',
};

/** The kind label of a held write by its `fact_class`; no label for any other value (SM-SF-4.2). */
export function heldKindMessage(factClass: string | null | undefined): string | null {
  switch (factClass) {
    case 'episodic':
      return KIND_MESSAGE.happened;
    case 'semantic':
      return KIND_MESSAGE.known;
    case 'consolidated':
      return KIND_MESSAGE.summary;
    default:
      return null;
  }
}

/** Agent names by agent instance id, from an `agents.list` result (SA-UI-SF-05). */
export class AgentNames {
  readonly #by: Map<string, string>;
  constructor(agents: readonly InstalledAgent[] = []) {
    this.#by = new Map(agents.map((a) => [a.instance.id, a.instance.display_name]));
  }
  nameOf(id: string | null | undefined): string | null {
    return id === null || id === undefined ? null : (this.#by.get(id) ?? null);
  }
}

export interface MessageRef {
  id: string;
  args: Record<string, string>;
}

/**
 * The source line of an agent's citation (SM-SF-6): the agent's name, or "an agent" when none matches;
 * never a plain document source. `null` for any other origin. `origin_ref` is text, not a link.
 */
export function citationLine(fact: Pick<FactView, 'came_from'>, names: AgentNames): MessageRef | null {
  const from = fact.came_from;
  if (from.kind !== 'agent_citing') return null;
  const agent = names.nameOf(from.agent_instance_id);
  const args = { source: from.origin_ref, version: from.version };
  return agent === null ? { id: 'memory-cited-by-an-agent', args } : { id: 'memory-cited-by-agent', args: { ...args, agent } };
}

/** `true_from` / `true_until` as the line the fact shows (`memory-true-since` or `memory-true-between`). */
export function truthLine(from: string, until: string | null | undefined): MessageRef {
  return until === null || until === undefined
    ? { id: 'memory-true-since', args: { date: formatDate(from) } }
    : { id: 'memory-true-between', args: { from: formatDate(from), until: formatDate(until) } };
}
