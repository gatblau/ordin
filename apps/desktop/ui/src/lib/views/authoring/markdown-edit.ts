// The five toolbar buttons of the document editor (SM-NS-4.6) as pure text edits over a selection.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-4
export type EditTool = 'heading' | 'bold' | 'italic' | 'bullets' | 'numbers';
export interface Edit {
  text: string;
  start: number;
  end: number;
}

/** The start and end of the lines a selection touches. */
function lineSpan(text: string, start: number, end: number): { from: number; to: number } {
  const from = text.lastIndexOf('\n', Math.max(0, start) - 1) + 1;
  const nl = text.indexOf('\n', Math.max(start, end));
  return { from, to: nl < 0 ? text.length : nl };
}

function wrap(text: string, start: number, end: number, mark: string): Edit {
  const before = text.slice(0, start);
  const chosen = text.slice(start, end);
  const after = text.slice(end);
  if (chosen.startsWith(mark) && chosen.endsWith(mark) && chosen.length >= mark.length * 2) {
    const inner = chosen.slice(mark.length, chosen.length - mark.length);
    return { text: before + inner + after, start, end: start + inner.length };
  }
  return { text: `${before}${mark}${chosen}${mark}${after}`, start: start + mark.length, end: end + mark.length };
}

function prefixLines(text: string, start: number, end: number, prefix: (index: number) => string, pattern: RegExp): Edit {
  const { from, to } = lineSpan(text, start, end);
  const lines = text.slice(from, to).split('\n');
  const allHave = lines.every((l) => pattern.test(l));
  const next = lines.map((l, i) => (allHave ? l.replace(pattern, '') : prefix(i) + l.replace(pattern, '')));
  const block = next.join('\n');
  return { text: text.slice(0, from) + block + text.slice(to), start: from, end: from + block.length };
}

export function applyTool(text: string, start: number, end: number, tool: EditTool): Edit {
  const a = Math.max(0, Math.min(start, end, text.length));
  const b = Math.max(0, Math.min(Math.max(start, end), text.length));
  switch (tool) {
    case 'bold':
      return wrap(text, a, b, '**');
    case 'italic':
      return wrap(text, a, b, '*');
    case 'heading':
      return prefixLines(text, a, b, () => '## ', /^#{1,6} /);
    case 'bullets':
      return prefixLines(text, a, b, () => '- ', /^[-*] /);
    case 'numbers':
      return prefixLines(text, a, b, (i) => `${i + 1}. `, /^\d+\. /);
  }
}
