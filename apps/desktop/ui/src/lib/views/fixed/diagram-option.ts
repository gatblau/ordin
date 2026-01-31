// The memory diagram as an ECharts `graph` option (ADR-120 item 2; SM-SF-21), and what a press on it means
// (SM-SF-22). A plain object, no DOM and no ECharts import, so the mapping is tested in Node. Colours come
// from the theme's custom properties, read by the caller once when drawn and again on a theme change.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-21
// spec: ordin-desktop/components/ScreensModule#SM-SF-24
import type { ChartOption } from '../../blocks/chart-option';
import { t } from '../../i18n/fluent';
import type { DiagramView } from '../../service-api/types.gen';
import { objectText } from './facts';

/** The theme's colours the drawing uses (ADR-116). */
export interface DiagramColours {
  accent: string;
  onAccent: string;
  surface: string;
  text: string;
  muted: string;
  line: string;
}

/** The custom property each colour is read from. */
export const DIAGRAM_TOKENS: Readonly<Record<keyof DiagramColours, string>> = {
  accent: '--color-accent',
  onAccent: '--color-on-accent',
  surface: '--color-surface',
  text: '--color-text',
  muted: '--color-text-muted',
  line: '--color-line-strong',
};

export function diagramColours(read: (property: string) => string): DiagramColours {
  return {
    accent: read(DIAGRAM_TOKENS.accent),
    onAccent: read(DIAGRAM_TOKENS.onAccent),
    surface: read(DIAGRAM_TOKENS.surface),
    text: read(DIAGRAM_TOKENS.text),
    muted: read(DIAGRAM_TOKENS.muted),
    line: read(DIAGRAM_TOKENS.line),
  };
}

const THING = 'thing:';
const VALUE = 'value:';

export interface DiagramNode {
  id: string;
  name: string;
  symbol: 'roundRect';
  symbolSize: [number, number];
  itemStyle: { color: string; borderColor: string; borderWidth: number };
  label: { show: true; color: string; fontWeight?: 'bold' };
}

export interface DiagramEdge {
  source: string;
  target: string;
  /** The fact the edge draws; a press opens that fact's line. */
  factId: string;
  label: { show: true; formatter: string };
  lineStyle: { type: 'solid' | 'dashed'; color: string };
}

/** The word on an edge: the word's label, and `memory-diagram-conflicts` for a fact in conflict. */
function edgeLabel(word: string, conflict: boolean): string {
  return conflict ? t('memory-diagram-conflicts', { word }) : word;
}

/** The nodes and edges of a `DiagramView`: one box per thing, one small box per value, one edge per fact. */
export function diagramGraph(view: DiagramView, colours: DiagramColours): { nodes: DiagramNode[]; edges: DiagramEdge[] } {
  const centre = view.centre?.id ?? null;
  const nodes: DiagramNode[] = view.things.map(({ thing }) => {
    const isCentre = thing.id === centre;
    return {
      id: THING + thing.id,
      name: thing.name,
      symbol: 'roundRect',
      symbolSize: isCentre ? [150, 44] : [120, 34],
      itemStyle: { color: isCentre ? colours.accent : colours.surface, borderColor: isCentre ? colours.accent : colours.line, borderWidth: 1 },
      label: isCentre ? { show: true, color: colours.onAccent, fontWeight: 'bold' } : { show: true, color: colours.text },
    };
  });
  const drawn = new Set(view.things.map(({ thing }) => thing.id));
  const edges: DiagramEdge[] = view.joins
    .filter((j) => drawn.has(j.from) && drawn.has(j.to))
    .map((j) => ({
      source: THING + j.from,
      target: THING + j.to,
      factId: j.fact_id,
      label: { show: true, formatter: edgeLabel(j.word.label, j.conflict) },
      lineStyle: { type: j.conflict ? 'dashed' : 'solid', color: colours.line },
    }));
  // A value hangs off the centre; without a centre there is nothing to hang it from (MemoryService G1 step 4).
  if (centre !== null && drawn.has(centre)) {
    for (const v of view.values) {
      nodes.push({
        id: VALUE + v.fact_id,
        name: objectText(v.object),
        symbol: 'roundRect',
        symbolSize: [100, 26],
        itemStyle: { color: colours.surface, borderColor: colours.line, borderWidth: 1 },
        label: { show: true, color: colours.muted },
      });
      edges.push({
        source: THING + centre,
        target: VALUE + v.fact_id,
        factId: v.fact_id,
        label: { show: true, formatter: edgeLabel(v.word.label, v.conflict) },
        lineStyle: { type: v.conflict ? 'dashed' : 'solid', color: colours.line },
      });
    }
  }
  return { nodes, edges };
}

/**
 * The option: a force layout that moves and zooms (`roam`) but cannot be dragged; rounded boxes; an arrow at
 * each edge's end. ECharts' own `aria` is off: the list beside the diagram is its equivalent (SM-SF-22).
 */
export function diagramOption(view: DiagramView, colours: DiagramColours, options: { animate?: boolean } = {}): ChartOption {
  const { nodes, edges } = diagramGraph(view, colours);
  return {
    animation: options.animate ?? true,
    aria: { enabled: false },
    textStyle: { color: colours.text },
    series: [
      {
        type: 'graph',
        layout: 'force',
        roam: true,
        draggable: false,
        force: { repulsion: 320, edgeLength: [90, 160] },
        edgeSymbol: ['none', 'arrow'],
        edgeSymbolSize: 8,
        label: { show: true },
        edgeLabel: { show: true, color: colours.muted, fontSize: 11 },
        data: nodes,
        links: edges,
      },
    ],
  };
}

/** What a press on the drawing means: centre on a thing, or open a fact's line. */
export type DiagramPress = { kind: 'thing'; id: string } | { kind: 'fact'; id: string } | null;

/** Reads ECharts' click parameters (`dataType` `node` or `edge`, `data` the item given above). */
export function diagramPress(params: unknown): DiagramPress {
  if (params === null || typeof params !== 'object') return null;
  const p = params as { dataType?: unknown; data?: unknown };
  const data = p.data as { id?: unknown; factId?: unknown } | undefined;
  if (data === undefined || data === null) return null;
  if (p.dataType === 'edge' && typeof data.factId === 'string') return { kind: 'fact', id: data.factId };
  if (p.dataType === 'node' && typeof data.id === 'string') {
    if (data.id.startsWith(THING)) return { kind: 'thing', id: data.id.slice(THING.length) };
    if (data.id.startsWith(VALUE)) return { kind: 'fact', id: data.id.slice(VALUE.length) };
  }
  return null;
}
