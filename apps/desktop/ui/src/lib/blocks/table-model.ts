// Table drawing without the DOM: typed cell text, sorting, search and the visible window (A-34: 10,000 rows
// by 30 columns; sorting or searching ≤ 200 ms with only the visible rows, ±50, in the document).
import { formatLocale } from '../format/intl';
import type { Cell, TableColumn } from '../service-api/types.gen';
import { refName } from './refs';

/** Rows kept in the document either side of the visible ones. */
export const WINDOW_MARGIN = 50;

// Formatters are made once per locale and options: building an `Intl` formatter per cell would cost seconds
// on 10,000 rows by 30 columns.
const formatters = new Map<string, Intl.NumberFormat | Intl.DateTimeFormat>();
function numberFormat(key: string, options: Intl.NumberFormatOptions): Intl.NumberFormat {
  const id = `${formatLocale()}|n|${key}`;
  let f = formatters.get(id) as Intl.NumberFormat | undefined;
  if (f === undefined) {
    f = new Intl.NumberFormat(formatLocale(), options);
    formatters.set(id, f);
  }
  return f;
}
function dateFormat(): Intl.DateTimeFormat {
  const id = `${formatLocale()}|d`;
  let f = formatters.get(id) as Intl.DateTimeFormat | undefined;
  if (f === undefined) {
    f = new Intl.DateTimeFormat(formatLocale(), { dateStyle: 'medium' });
    formatters.set(id, f);
  }
  return f;
}

/** The text a cell shows. A string is text, even when it starts with `=` or looks like markup. */
export function cellText(column: TableColumn, cell: Cell): string {
  if (cell === null) return '';
  if (typeof cell === 'object') return refName(cell) ?? '';
  if (typeof cell === 'string') return column.type === 'date' ? safeDate(cell) : cell;
  if (!Number.isFinite(cell)) return '';
  switch (column.type) {
    case 'money':
      try {
        return numberFormat(`m${column.currency}`, { style: 'currency', currency: column.currency }).format(cell);
      } catch {
        return `${numberFormat('d', {}).format(cell)} ${column.currency}`;
      }
    case 'percentage':
      return numberFormat('p', { style: 'percent', maximumFractionDigits: 1 }).format(cell);
    case 'number':
      return column.decimals === undefined || column.decimals === null
        ? numberFormat('d', {}).format(cell)
        : numberFormat(`d${column.decimals}`, { minimumFractionDigits: column.decimals, maximumFractionDigits: column.decimals }).format(cell);
    default:
      return numberFormat('d', {}).format(cell);
  }
}

function safeDate(value: string): string {
  return /^\d{4}-\d{2}-\d{2}$/.test(value) ? dateFormat().format(new Date(`${value}T00:00:00`)) : value;
}

export type SortDirection = 'ascending' | 'descending';

export function compareCells(column: TableColumn, a: Cell, b: Cell, collator: Intl.Collator): number {
  const aEmpty = a === null || (typeof a === 'number' && !Number.isFinite(a));
  const bEmpty = b === null || (typeof b === 'number' && !Number.isFinite(b));
  if (aEmpty || bEmpty) return aEmpty === bEmpty ? 0 : aEmpty ? 1 : -1;
  if (typeof a === 'number' && typeof b === 'number') return a - b;
  const left = typeof a === 'object' ? (refName(a) ?? '') : String(a);
  const right = typeof b === 'object' ? (refName(b) ?? '') : String(b);
  // ISO dates compare as text; everything else by the active locale's collation.
  return column.type === 'date' ? (left < right ? -1 : left > right ? 1 : 0) : collator.compare(left, right);
}

/** The row indexes in display order: filtered by `query` (every word must appear in some cell), then sorted. */
// The searchable text of each row, made once per table and locale.
const haystacks = new WeakMap<Cell[][], { locale: string; text: string[] }>();
function haystackOf(columns: TableColumn[], rows: Cell[][]): string[] {
  const cached = haystacks.get(rows);
  if (cached !== undefined && cached.locale === formatLocale()) return cached.text;
  const text = rows.map((row) => columns.map((c, k) => cellText(c, row[k] ?? null)).join('\u0000').toLocaleLowerCase(formatLocale()));
  haystacks.set(rows, { locale: formatLocale(), text });
  return text;
}

export function arrangeRows(columns: TableColumn[], rows: Cell[][], query: string, sort: { column: number; direction: SortDirection } | null): number[] {
  const words = query.toLocaleLowerCase(formatLocale()).split(/\s+/).filter((w) => w !== '');
  let indexes: number[] = [];
  if (words.length === 0) {
    indexes = rows.map((_, i) => i);
  } else {
    haystackOf(columns, rows).forEach((haystack, i) => {
      if (words.every((w) => haystack.includes(w))) indexes.push(i);
    });
  }
  if (sort !== null) {
    const column = columns[sort.column];
    if (column !== undefined) {
      const collator = new Intl.Collator(formatLocale(), { numeric: true });
      const sign = sort.direction === 'ascending' ? 1 : -1;
      indexes.sort((x, y) => {
        const a = rows[x]?.[sort.column] ?? null;
        const b = rows[y]?.[sort.column] ?? null;
        const aEmpty = a === null;
        const bEmpty = b === null;
        if (aEmpty || bEmpty) return aEmpty === bEmpty ? x - y : aEmpty ? 1 : -1;
        return sign * compareCells(column, a, b, collator) || x - y;
      });
    }
  }
  return indexes;
}

/** The slice of `total` rows to put in the document for a scroll position. */
export function visibleWindow(total: number, scrollTop: number, viewportHeight: number, rowHeight: number): { start: number; end: number } {
  const first = Math.max(0, Math.floor(scrollTop / rowHeight));
  const count = Math.max(1, Math.ceil(viewportHeight / rowHeight));
  return { start: Math.max(0, first - WINDOW_MARGIN), end: Math.min(total, first + count + WINDOW_MARGIN) };
}
