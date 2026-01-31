// Focus and keyboard helpers, kept free of the DOM where they can be so the tests need none.

export const REGIONS = ['tree', 'screen', 'conversation'] as const;
export type Region = (typeof REGIONS)[number];

/** F6 moves to the next region, Shift+F6 to the previous; `available` skips a region that is not shown. */
export function nextRegion(current: Region | null, backwards: boolean, available: readonly Region[] = REGIONS): Region | null {
  const order = REGIONS.filter((r) => available.includes(r));
  if (order.length === 0) return null;
  const at = current === null ? -1 : order.indexOf(current);
  if (at === -1) return backwards ? (order[order.length - 1] ?? null) : (order[0] ?? null);
  const step = backwards ? order.length - 1 : 1;
  return order[(at + step) % order.length] ?? null;
}

/** Tab inside a dialog wraps: returns the index to focus next among `count` focusable items. */
export function trapIndex(active: number, count: number, backwards: boolean): number {
  if (count <= 0) return -1;
  if (active < 0) return backwards ? count - 1 : 0;
  return backwards ? (active - 1 + count) % count : (active + 1) % count;
}

export const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function focusableIn(root: ParentNode): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)];
}

/** The region element currently holding focus, from the `data-region` attribute. */
export function regionOf(element: Element | null): Region | null {
  const host = element?.closest('[data-region]') ?? null;
  const value = host?.getAttribute('data-region');
  return REGIONS.find((r) => r === value) ?? null;
}

/** Moves focus into a region: its marked target, else its first focusable item, else the region itself. */
export function focusRegion(region: Region, doc: Document = document): void {
  const host = doc.querySelector<HTMLElement>(`[data-region="${region}"]`);
  if (host === null) return;
  const target = host.querySelector<HTMLElement>('[data-region-focus]') ?? focusableIn(host)[0] ?? host;
  target.focus();
}
