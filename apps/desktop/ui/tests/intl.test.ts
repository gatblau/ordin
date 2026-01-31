import { afterEach, describe, expect, it } from 'vitest';

import { formatLocale, formatMoney, formatNumber, formatPercent, formatList, pluralCategory, setFormatLocale } from '../src/lib/format/intl';

afterEach(() => setFormatLocale('en-GB'));

describe('intl', () => {
  it('formats through the active locale', () => {
    expect(formatNumber(1234.5)).toBe('1,234.5');
    setFormatLocale('de-DE');
    expect(formatNumber(1234.5)).toBe('1.234,5');
    expect(formatPercent(0.25)).toBe('25 %');
    expect(formatMoney(3, 'EUR')).toBe('3,00 €');
  });
  it('falls back to en-GB for a tag Intl refuses', () => {
    setFormatLocale('not a locale');
    expect(formatLocale()).toBe('en-GB');
  });
  it('lists and plurals', () => {
    expect(formatList(['a', 'b', 'c'])).toBe('a, b and c');
    expect(pluralCategory(1)).toBe('one');
    expect(pluralCategory(2)).toBe('other');
  });
});
