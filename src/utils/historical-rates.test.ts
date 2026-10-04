import { describe, it, expect, vi } from 'vitest';
import {
  dayKeyFor,
  resolveRatesForDay,
  convertBreakdownAtDay,
  convertCzkAtDay,
  type RatesByDay,
  makeDatedConvertFn,
} from './historical-rates';

const DAY = 86400;

describe('dayKeyFor', () => {
  it('floors a timestamp to midnight UTC', () => {
    expect(dayKeyFor(DAY * 100 + 12345)).toBe(DAY * 100);
  });

  it('returns midnight timestamps unchanged', () => {
    expect(dayKeyFor(DAY * 100)).toBe(DAY * 100);
  });
});

describe('resolveRatesForDay', () => {
  const rates: RatesByDay = {
    [DAY * 1]: { CZK: 1, USD: 22 },
    [DAY * 4]: { CZK: 1, USD: 23 },
    [DAY * 10]: { CZK: 1, USD: 24 },
  };

  it('returns undefined when the map is undefined', () => {
    expect(resolveRatesForDay(undefined, DAY * 4)).toBeUndefined();
  });

  it('returns undefined when the map is empty', () => {
    expect(resolveRatesForDay({}, DAY * 4)).toBeUndefined();
  });

  it('returns the exact day entry when present', () => {
    expect(resolveRatesForDay(rates, DAY * 4)).toEqual({ CZK: 1, USD: 23 });
  });

  it('falls back to the closest earlier day when the exact day is missing', () => {
    expect(resolveRatesForDay(rates, DAY * 6)).toEqual({ CZK: 1, USD: 23 });
    expect(resolveRatesForDay(rates, DAY * 3)).toEqual({ CZK: 1, USD: 22 });
    expect(resolveRatesForDay(rates, DAY * 100)).toEqual({ CZK: 1, USD: 24 });
  });

  it('returns undefined when the day is before the first entry', () => {
    expect(resolveRatesForDay(rates, DAY * 0)).toBeUndefined();
  });
});

describe('convertBreakdownAtDay', () => {
  const dayRates = { CZK: 1, USD: 20, EUR: 25 };

  it('returns the stored CZK total untouched for CZK display', () => {
    const convertToday = vi.fn();
    expect(convertBreakdownAtDay('{"USD":100}', 1234, dayRates, 'CZK', convertToday)).toBe(1234);
    expect(convertToday).not.toHaveBeenCalled();
  });

  it('converts each native bucket at that day rates via the CZK pivot', () => {
    const convertToday = vi.fn();
    // (100 USD * 20 + 500 CZK * 1) / 25 CZK-per-EUR = 100 EUR
    const result = convertBreakdownAtDay(
      '{"USD":100,"CZK":500}',
      2500,
      dayRates,
      'EUR',
      convertToday
    );
    expect(result).toBeCloseTo(100, 10);
    expect(convertToday).not.toHaveBeenCalled();
  });

  it("falls back per bucket to today's rate when a bucket currency is missing from the day map", () => {
    const convertToday = vi.fn().mockReturnValue(7);
    // USD bucket: 100 * 20 / 25 = 80 EUR; GBP bucket missing -> convertToday(10, 'GBP') = 7
    const result = convertBreakdownAtDay(
      '{"USD":100,"GBP":10}',
      99999,
      dayRates,
      'EUR',
      convertToday
    );
    expect(result).toBeCloseTo(87, 10);
    expect(convertToday).toHaveBeenCalledTimes(1);
    expect(convertToday).toHaveBeenCalledWith(10, 'GBP');
  });

  it('falls back to CZK total at today rate when the display currency is missing from the day map', () => {
    const convertToday = vi.fn().mockReturnValue(42);
    const result = convertBreakdownAtDay(
      '{"USD":100}',
      1000,
      { CZK: 1, USD: 20 },
      'EUR',
      convertToday
    );
    expect(result).toBe(42);
    expect(convertToday).toHaveBeenCalledWith(1000, 'CZK');
  });

  it('falls back to CZK total at today rate when there are no day rates', () => {
    const convertToday = vi.fn().mockReturnValue(42);
    expect(convertBreakdownAtDay('{"USD":100}', 1000, undefined, 'EUR', convertToday)).toBe(42);
    expect(convertToday).toHaveBeenCalledWith(1000, 'CZK');
  });

  it('falls back to CZK total at today rate when the breakdown is empty', () => {
    const convertToday = vi.fn().mockReturnValue(42);
    expect(convertBreakdownAtDay('{}', 1000, dayRates, 'EUR', convertToday)).toBe(42);
    expect(convertToday).toHaveBeenCalledWith(1000, 'CZK');
  });

  it('falls back to CZK total at today rate when the breakdown JSON is invalid', () => {
    const convertToday = vi.fn().mockReturnValue(42);
    expect(convertBreakdownAtDay('not json', 1000, dayRates, 'EUR', convertToday)).toBe(42);
  });

  it('falls back to CZK total at today rate when the breakdown is null or undefined', () => {
    const convertToday = vi.fn().mockReturnValue(42);
    expect(convertBreakdownAtDay(null, 1000, dayRates, 'EUR', convertToday)).toBe(42);
    expect(convertBreakdownAtDay(undefined, 1000, dayRates, 'EUR', convertToday)).toBe(42);
  });

  it('treats a zero or non-finite display rate as missing', () => {
    const convertToday = vi.fn().mockReturnValue(42);
    expect(
      convertBreakdownAtDay('{"USD":100}', 1000, { CZK: 1, EUR: 0 }, 'EUR', convertToday)
    ).toBe(42);
    expect(convertToday).toHaveBeenCalledWith(1000, 'CZK');
  });

  it('ignores non-numeric breakdown entries, falling back when nothing valid remains', () => {
    const convertToday = vi.fn().mockReturnValue(42);
    expect(convertBreakdownAtDay('{"USD":"bad"}', 1000, dayRates, 'EUR', convertToday)).toBe(42);
    expect(convertToday).toHaveBeenCalledWith(1000, 'CZK');
  });

  // Guard against partial/corrupt breakdowns: rows written by the snapshot
  // backfill can carry a breakdown covering only part of the portfolio while
  // the stored CZK total is complete. Such rows must render via the CZK total,
  // not the fragment (they used to render as deep dips in non-CZK display).
  describe('consistency guard (breakdown vs stored CZK total)', () => {
    it('falls back to the CZK total when the breakdown covers only part of it', () => {
      const convertToday = vi.fn().mockReturnValue(42);
      // Breakdown is 100 USD * 20 = 2000 CZK, but the stored total is 10000 CZK
      // (breakdown covers 20% of the portfolio) -> inconsistent -> fallback.
      const result = convertBreakdownAtDay('{"USD":100}', 10000, dayRates, 'EUR', convertToday);
      expect(result).toBe(42);
      expect(convertToday).toHaveBeenCalledWith(10000, 'CZK');
    });

    it('keeps the breakdown conversion when it matches the total within tolerance', () => {
      const convertToday = vi.fn();
      // 100 USD * 20 = 2000 CZK vs stored 2100 CZK (~5% off, within tolerance)
      const result = convertBreakdownAtDay('{"USD":100}', 2100, dayRates, 'EUR', convertToday);
      expect(result).toBeCloseTo(80, 10);
      expect(convertToday).not.toHaveBeenCalled();
    });

    it('keeps the breakdown conversion when the stored total is zero', () => {
      const convertToday = vi.fn();
      const result = convertBreakdownAtDay('{"USD":100}', 0, dayRates, 'EUR', convertToday);
      expect(result).toBeCloseTo(80, 10);
      expect(convertToday).not.toHaveBeenCalled();
    });

    it('skips the check when a bucket currency has no usable day rate', () => {
      const convertToday = vi.fn().mockReturnValue(7);
      // GBP has no day rate, so the breakdown's CZK equivalent cannot be
      // computed like-for-like -> no consistency check, per-bucket fallback only.
      const result = convertBreakdownAtDay(
        '{"USD":100,"GBP":10}',
        99999,
        dayRates,
        'EUR',
        convertToday
      );
      expect(result).toBeCloseTo(87, 10);
      expect(convertToday).toHaveBeenCalledTimes(1);
      expect(convertToday).toHaveBeenCalledWith(10, 'GBP');
    });
  });
});

describe('convertCzkAtDay', () => {
  it('returns the CZK amount untouched for CZK display', () => {
    const convertToday = vi.fn();
    expect(convertCzkAtDay(1234, { CZK: 1, EUR: 25 }, 'CZK', convertToday)).toBe(1234);
    expect(convertToday).not.toHaveBeenCalled();
  });

  it('divides by the day display rate when available', () => {
    const convertToday = vi.fn();
    expect(convertCzkAtDay(2500, { CZK: 1, EUR: 25 }, 'EUR', convertToday)).toBeCloseTo(100, 10);
    expect(convertToday).not.toHaveBeenCalled();
  });

  it("falls back to today's rate when the display rate is missing", () => {
    const convertToday = vi.fn().mockReturnValue(42);
    expect(convertCzkAtDay(2500, { CZK: 1 }, 'EUR', convertToday)).toBe(42);
    expect(convertToday).toHaveBeenCalledWith(2500, 'CZK');
  });

  it("falls back to today's rate when there are no day rates", () => {
    const convertToday = vi.fn().mockReturnValue(42);
    expect(convertCzkAtDay(2500, undefined, 'EUR', convertToday)).toBe(42);
    expect(convertToday).toHaveBeenCalledWith(2500, 'CZK');
  });

  it("falls back to today's rate when the display rate is zero", () => {
    const convertToday = vi.fn().mockReturnValue(42);
    expect(convertCzkAtDay(2500, { CZK: 1, EUR: 0 }, 'EUR', convertToday)).toBe(42);
  });
});

describe('makeDatedConvertFn', () => {
  const DAY = 86400;
  const D1 = 1700006400;
  const rates = { [D1]: { CZK: 1, USD: 25, EUR: 25 } };

  it("converts at the day's cross rate, not today's", () => {
    const convertToday = vi.fn().mockReturnValue(-1);
    const convertAt = makeDatedConvertFn(rates, convertToday);

    // 100 USD at day rate 25 -> 2500 CZK
    expect(convertAt(100, 'USD', 'CZK', D1)).toBeCloseTo(2500);
    // 100 USD -> EUR via cross rate 25/25 = 1
    expect(convertAt(100, 'USD', 'EUR', D1)).toBeCloseTo(100);
    expect(convertToday).not.toHaveBeenCalled();
  });

  it('resolves the closest earlier day for weekend dates', () => {
    const convertToday = vi.fn().mockReturnValue(-1);
    const convertAt = makeDatedConvertFn(rates, convertToday);

    expect(convertAt(100, 'USD', 'CZK', D1 + 2 * DAY)).toBeCloseTo(2500);
    expect(convertToday).not.toHaveBeenCalled();
  });

  it("falls back to today's conversion when the day has no usable rate", () => {
    const convertToday = vi.fn().mockReturnValue(42);
    const convertAt = makeDatedConvertFn(rates, convertToday);

    // GBP missing from the snapshot -> fallback.
    expect(convertAt(100, 'GBP', 'CZK', D1)).toBe(42);
    expect(convertToday).toHaveBeenCalledWith(100, 'GBP', 'CZK');
    // No rates at all (day before the range) -> fallback.
    expect(convertAt(100, 'USD', 'CZK', D1 - 30 * DAY)).toBe(42);
  });

  it('is identity when both currencies match', () => {
    const convertToday = vi.fn().mockReturnValue(-1);
    const convertAt = makeDatedConvertFn(undefined, convertToday);

    expect(convertAt(123, 'CZK', 'CZK', D1)).toBe(123);
    expect(convertToday).not.toHaveBeenCalled();
  });
});
