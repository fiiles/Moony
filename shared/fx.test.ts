import { describe, expect, it } from 'vitest';
import vectors from './fx-test-vectors.json';
import { convert, convertOn, makeHistory, MissingTracker } from './fx';

const quotes = (() => {
  const units: Record<string, number> = { ...vectors.quotes.units, [vectors.quotes.reference]: 1 };
  // the backend includes derived codes; mirror that for the vectors
  units.AED = units.USD * 3.6725;
  units.SAR = units.USD * 3.75;
  units.BGN = 1.95583;
  return { reference: vectors.quotes.reference, units };
})();

describe('convert', () => {
  it.each(vectors.cases)('$from -> $to', ({ amount, from, to, expected }) => {
    const got = convert(quotes, amount, from, to);
    if (expected === null) expect(got).toBeNull();
    else expect(got).toBeCloseTo(expected, 9);
  });
  it('returns null without quotes except for same currency', () => {
    expect(convert(undefined, 5, 'EUR', 'USD')).toBeNull();
    expect(convert(undefined, 5, 'EUR', 'eur')).toBe(5);
  });
});

describe('convertOn', () => {
  const DAY = 86_400;
  const d = 20_000 * DAY;
  const h = makeHistory(
    [
      { day: d, units: { CZK: 1, EUR: 1 / 24 } },
      { day: d - DAY, units: { CZK: 1, EUR: 1 / 25, USD: 1 / 20 } },
    ],
    'CZK'
  );
  it('uses a common day for both currencies', () => {
    expect(convertOn(h, 100, 'USD', 'EUR', d + 3600)).toBeCloseTo(80, 9);
  });
  it('gives up after ten days', () => {
    expect(convertOn(h, 1, 'EUR', 'CZK', d + 11 * DAY)).toBeNull();
  });
});

describe('MissingTracker', () => {
  it('collects missing currencies once', () => {
    const t = new MissingTracker();
    t.track('KWD', null);
    t.track('KWD', null);
    t.track('USD', 1);
    expect(t.missing).toEqual(['KWD']);
    expect(t.complete).toBe(false);
  });
});
