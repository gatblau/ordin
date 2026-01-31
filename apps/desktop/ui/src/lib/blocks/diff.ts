// A line diff for the two-column `changes` view. Pure; no DOM.

export type DiffKind = 'same' | 'removed' | 'added';
export interface DiffRow {
  /** The line on the left (before), or `null` where the right side added a line. */
  left: { text: string; kind: DiffKind } | null;
  /** The line on the right (after), or `null` where the left side lost a line. */
  right: { text: string; kind: DiffKind } | null;
}

/** Past this many lines on either side the middle is shown as one removed block and one added block. */
export const DIFF_LINE_LIMIT = 2000;

export function splitLines(text: string): string[] {
  if (text === '') return [];
  const lines = text.replace(/\r\n?/g, '\n').split('\n');
  if (lines[lines.length - 1] === '') lines.pop();
  return lines;
}

export function diffLines(before: string[], after: string[]): DiffRow[] {
  let start = 0;
  while (start < before.length && start < after.length && before[start] === after[start]) start++;
  let endB = before.length;
  let endA = after.length;
  while (endB > start && endA > start && before[endB - 1] === after[endA - 1]) {
    endB--;
    endA--;
  }
  const rows: DiffRow[] = [];
  for (let i = 0; i < start; i++) rows.push(same(before[i] as string));
  const midB = before.slice(start, endB);
  const midA = after.slice(start, endA);
  if (midB.length > DIFF_LINE_LIMIT || midA.length > DIFF_LINE_LIMIT) {
    pushBlock(rows, midB, midA);
  } else {
    middle(rows, midB, midA);
  }
  for (let i = endB; i < before.length; i++) rows.push(same(before[i] as string));
  return rows;
}

const same = (text: string): DiffRow => ({ left: { text, kind: 'same' }, right: { text, kind: 'same' } });

function pushBlock(rows: DiffRow[], removed: string[], added: string[]): void {
  const n = Math.max(removed.length, added.length);
  for (let i = 0; i < n; i++) {
    const l = removed[i];
    const r = added[i];
    rows.push({
      left: l === undefined ? null : { text: l, kind: 'removed' },
      right: r === undefined ? null : { text: r, kind: 'added' },
    });
  }
}

function middle(rows: DiffRow[], b: string[], a: string[]): void {
  const n = b.length;
  const m = a.length;
  // Longest common subsequence, lengths from the end.
  const width = m + 1;
  const table = new Uint32Array((n + 1) * width);
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      table[i * width + j] =
        b[i] === a[j] ? (table[(i + 1) * width + j + 1] as number) + 1 : Math.max(table[(i + 1) * width + j] as number, table[i * width + j + 1] as number);
    }
  }
  let i = 0;
  let j = 0;
  let removed: string[] = [];
  let added: string[] = [];
  const flush = (): void => {
    pushBlock(rows, removed, added);
    removed = [];
    added = [];
  };
  while (i < n || j < m) {
    if (i < n && j < m && b[i] === a[j]) {
      flush();
      rows.push(same(b[i] as string));
      i++;
      j++;
    } else if (j >= m || (i < n && (table[(i + 1) * width + j] as number) >= (table[i * width + j + 1] as number))) {
      removed.push(b[i++] as string);
    } else {
      added.push(a[j++] as string);
    }
  }
  flush();
}
