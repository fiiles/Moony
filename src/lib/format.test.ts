import { describe, expect, it } from 'vitest';
import { createFormatters, getFormatters, type Formatters } from './format';

const NBSP = ' ';
const NNBSP = ' ';
const norm = (s: string) => s.replace(new RegExp(`[${NBSP}${NNBSP}]`, 'g'), ' ');

describe('money', () => {
  // Each amount in the main currency (its narrow symbol); other currencies: see below.
  const asMain = (locale: string) => ({
    money: (amount: number, currency: string, opts?: Parameters<Formatters['money']>[2]) =>
      createFormatters(locale, currency).money(amount, currency, opts),
  });
  const en = asMain('en-US');
  const cs = asMain('cs-CZ');

  it('follows the UI language, not the currency', () => {
    expect(norm(en.money(1234.5, 'EUR'))).toBe('€1,234.50');
    expect(norm(cs.money(1234.5, 'EUR'))).toBe('1 234,50 €');
    expect(norm(en.money(1234, 'CZK'))).toBe('Kč 1,234');
    expect(norm(cs.money(1234, 'CZK'))).toBe('1 234 Kč');
    expect(norm(en.money(1234.56, 'USD'))).toBe('$1,234.56');
    expect(norm(cs.money(1234.56, 'USD'))).toBe('1 234,56 $');
  });

  it('uses the currency decimals and never shows -0', () => {
    expect(norm(en.money(-0, 'CZK'))).toBe('Kč 0');
    expect(norm(en.money(-0.001, 'CZK'))).toBe('Kč 0');
    expect(norm(en.money(1234.567, 'CHF'))).toBe('CHF 1,234.57');
    expect(norm(cs.money(1234.567, 'CHF'))).toBe('1 234,57 CHF');
  });

  it('supports signed and compact variants', () => {
    expect(norm(en.money(1500, 'EUR', { signed: true }))).toBe('+ €1,500.00');
    expect(norm(en.money(-1500, 'EUR', { signed: true }))).toBe('− €1,500.00');
    expect(norm(en.money(1_250_000, 'CZK', { compact: true }))).toBe('Kč 1.3M');
  });

  it('falls back gracefully for unknown codes', () => {
    expect(norm(en.money(10, 'XYZ1'))).toBe('10.00 XYZ1');
    expect(norm(en.money(10, 'notacur'))).toBe('10.00 notacur');
    expect(norm(en.money(10, ''))).toBe('10.00');
  });

  it('lets compact amounts choose the fraction digits', () => {
    expect(norm(en.money(3_120_000_000_000, 'USD', { compact: true, decimals: 2 }))).toBe('$3.12T');
    expect(norm(en.money(3_120_000_000_000, 'USD', { compact: true, decimals: 0 }))).toBe('$3T');
    expect(norm(en.money(3_120_000_000_000, '', { compact: true, decimals: 2 }))).toBe('3.12T');
  });
});

describe('money main-currency symbol', () => {
  it('uses the narrow symbol only for the main currency', () => {
    expect(norm(createFormatters('en-US', 'CZK').money(5, 'USD'))).toBe('USD 5.00');
    expect(norm(createFormatters('en-US', 'USD').money(5, 'USD'))).toBe('$5.00');
    expect(norm(createFormatters('en-US', 'czk').money(1234, 'CZK'))).toBe('Kč 1,234');
  });

  it('shows the ISO code for every currency when no main currency is known', () => {
    expect(norm(createFormatters('en-US', undefined).money(5, 'USD'))).toBe('USD 5.00');
    expect(norm(createFormatters('en-US', undefined).money(1234, 'CZK'))).toBe('CZK 1,234');
  });

  it('memoizes per locale and main currency', () => {
    expect(getFormatters('en-US', 'USD')).toBe(getFormatters('en-US', 'USD'));
    expect(getFormatters('en-US', 'USD')).not.toBe(getFormatters('en-US', 'CZK'));
  });
});

describe('number and percent', () => {
  it('keeps the minimum fraction digits valid when only a minimum is given', () => {
    const en = createFormatters('en-US', undefined);
    expect(norm(en.number(1.5, { minimumFractionDigits: 4 }))).toBe('1.5000');
    expect(norm(en.number(-0))).toBe('0');
    expect(norm(en.number(1234.5678, { maximumFractionDigits: 0 }))).toBe('1,235');
  });

  it('formats with the locale separators', () => {
    expect(norm(createFormatters('en-US', undefined).number(1234567.891))).toBe('1,234,567.89');
    expect(norm(createFormatters('cs-CZ', undefined).number(1234567.891))).toBe('1 234 567,89');
    expect(norm(createFormatters('en-US', undefined).percent(0.1234))).toBe('12.3%');
    expect(norm(createFormatters('cs-CZ', undefined).percent(0.1234))).toBe('12,3 %');
  });

  it('signs percentages without ever showing +0 or -0', () => {
    const en = createFormatters('en-US', undefined);
    expect(norm(en.percent(0.0123, 2, { signed: true }))).toBe('+ 1.23%');
    expect(norm(en.percent(-0.0123, 2, { signed: true }))).toBe('− 1.23%');
    expect(norm(en.percent(-0.00001, 2, { signed: true }))).toBe('0.00%');
    expect(norm(createFormatters('cs-CZ', undefined).percent(0.05, 1, { signed: true }))).toBe(
      '+ 5,0 %'
    );
  });
});

describe('day', () => {
  // 2026-09-30T00:00:00Z — how booking dates are stored (ADR 0008)
  const SEPT_30 = 1_790_726_400;

  it('formats the calendar day in UTC regardless of the machine time zone', () => {
    expect(norm(createFormatters('en-US', undefined).day(SEPT_30))).toBe('9/30/2026');
    expect(norm(createFormatters('cs-CZ', undefined).day(SEPT_30))).toBe('30. 9. 2026');
    expect(norm(createFormatters('en-GB', undefined).day(SEPT_30))).toBe('30/09/2026');
  });

  it('accepts formatting options', () => {
    expect(
      norm(createFormatters('en-US', undefined).day(SEPT_30, { month: 'short', day: 'numeric' }))
    ).toBe('Sep 30');
  });
});

describe('time', () => {
  it('formats the time of day without a date', () => {
    const at = Math.floor(new Date(2026, 8, 30, 14, 5).getTime() / 1000);
    expect(norm(createFormatters('cs-CZ', undefined).time(at))).toBe('14:05');
    expect(norm(createFormatters('en-US', undefined).time(at))).toBe('02:05 PM');
  });
});

describe('date', () => {
  // A local-time Date, e.g. a period range built in the UI
  const local = new Date(2026, 8, 30, 14, 5);

  it('formats local dates with the UI locale and numeric defaults', () => {
    expect(norm(createFormatters('en-US', undefined).date(local))).toBe('9/30/2026');
    expect(norm(createFormatters('cs-CZ', undefined).date(local))).toBe('30. 9. 2026');
  });

  it('keeps the requested parts only (like toLocaleDateString)', () => {
    expect(
      norm(createFormatters('en-US', undefined).date(local, { month: 'short', day: 'numeric' }))
    ).toBe('Sep 30');
    expect(
      norm(createFormatters('cs-CZ', undefined).date(local, { month: 'long', year: 'numeric' }))
    ).toBe('září 2026');
  });

  it('accepts ISO strings and returns an empty string for invalid input', () => {
    expect(norm(createFormatters('en-US', undefined).date('2026-09-30T12:00:00'))).toBe(
      '9/30/2026'
    );
    expect(createFormatters('en-US', undefined).date('nonsense')).toBe('');
    expect(createFormatters('en-US', undefined).date(new Date(NaN))).toBe('');
  });
});

describe('month', () => {
  it('capitalizes period labels in both languages', () => {
    const d = new Date(Date.UTC(2026, 8, 1));
    expect(createFormatters('en-US', undefined).month(d)).toBe('September 2026');
    expect(createFormatters('cs-CZ', undefined).month(d)).toBe('Září 2026');
  });
});

describe('monthInSentence', () => {
  const september = new Date(Date.UTC(2026, 8, 1));

  it.each([
    ['en-US', 'September 2026'],
    ['cs-CZ', 'září 2026'],
    ['de-DE', 'September 2026'],
    ['es-ES', 'septiembre de 2026'],
    ['it-IT', 'settembre 2026'],
    ['nb-NO', 'september 2026'],
    ['pl-PL', 'wrzesień 2026'],
    ['sv-SE', 'september 2026'],
    ['ja-JP', '2026年9月'],
    ['zh-CN', '2026年9月'],
  ])('%s keeps the casing Intl uses inside a sentence', (locale, expected) => {
    expect(createFormatters(locale, undefined).monthInSentence(september)).toBe(expected);
  });

  it('leaves the line-start form capitalised', () => {
    expect(createFormatters('es-ES', undefined).month(september)).toBe('Septiembre de 2026');
  });
});

describe('getFormatters', () => {
  it('memoizes per locale', () => {
    expect(getFormatters('en-US', undefined)).toBe(getFormatters('en-US', undefined));
    expect(getFormatters('en-US', undefined)).not.toBe(getFormatters('cs-CZ', undefined));
  });
});

// Imported legacy data must not crash an entire transaction list.
describe('invalid stored dates', () => {
  it('renders malformed Unix dates as an empty value', () => {
    const fmt = createFormatters('cs-CZ', 'CZK');
    for (const value of [
      NaN,
      Infinity,
      -Infinity,
      Number.MAX_VALUE,
      Number('9223372036854775807'),
    ]) {
      expect(fmt.day(value)).toBe('');
      expect(fmt.dateTime(value)).toBe('');
      expect(fmt.time(value)).toBe('');
    }
  });
});
