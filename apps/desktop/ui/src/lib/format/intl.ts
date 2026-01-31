// Numbers, money, percentages, dates, lists and plurals, all through Intl in the active locale.
// The locale is set once from the active locale (`setFormatLocale`), never read from the browser.

let current = 'en-GB';

export function setFormatLocale(locale: string): void {
  try {
    new Intl.NumberFormat(locale);
    current = locale;
  } catch {
    current = 'en-GB';
  }
}

export function formatLocale(): string {
  return current;
}

export function formatNumber(value: number, options?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(current, options).format(value);
}

export function formatMoney(value: number, currency: string): string {
  return new Intl.NumberFormat(current, { style: 'currency', currency }).format(value);
}

/** `fraction` 0.256 is 26 %. */
export function formatPercent(fraction: number, maximumFractionDigits = 0): string {
  return new Intl.NumberFormat(current, { style: 'percent', maximumFractionDigits }).format(fraction);
}

export function formatDate(value: string | number | Date, options: Intl.DateTimeFormatOptions = { dateStyle: 'medium' }): string {
  return new Intl.DateTimeFormat(current, options).format(new Date(value));
}

export function formatTime(value: string | number | Date): string {
  return new Intl.DateTimeFormat(current, { timeStyle: 'short' }).format(new Date(value));
}

export function formatList(items: string[], type: 'conjunction' | 'disjunction' = 'conjunction'): string {
  return new Intl.ListFormat(current, { style: 'long', type }).format(items);
}

export function pluralCategory(count: number): Intl.LDMLPluralRule {
  return new Intl.PluralRules(current).select(count);
}
