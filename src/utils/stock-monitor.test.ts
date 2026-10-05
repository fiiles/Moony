import { describe, expect, it } from 'vitest';
import {
  CHART_PERIODS,
  changeFromReference,
  dayChange,
  formatMarketCap,
  formatNativePrice,
  inferTargetDirection,
  latestFetchedAt,
  percentFromFraction,
  targetDistance,
  targetReached,
} from './stock-monitor';

describe('dayChange', () => {
  it('computes absolute and percent change vs previous close', () => {
    const c = dayChange('230.10', '228.50');
    expect(c).not.toBeNull();
    expect(c!.abs).toBeCloseTo(1.6, 6);
    expect(c!.pct).toBeCloseTo(0.7002, 3);
  });

  it('is negative when price dropped', () => {
    const c = dayChange('95.00', '100.00');
    expect(c!.abs).toBeCloseTo(-5);
    expect(c!.pct).toBeCloseTo(-5);
  });

  it('returns null on missing or invalid inputs', () => {
    expect(dayChange(null, '100')).toBeNull();
    expect(dayChange('100', null)).toBeNull();
    expect(dayChange('abc', '100')).toBeNull();
    expect(dayChange('100', '0')).toBeNull(); // avoid div by zero
  });
});

describe('changeFromReference', () => {
  it('computes change of the current price against a period reference price', () => {
    const c = changeFromReference(189.18, 177.61);
    expect(c!.abs).toBeCloseTo(11.57, 2);
    expect(c!.pct).toBeCloseTo(6.514, 2);
  });

  it('is negative when the price fell over the period', () => {
    const c = changeFromReference(305.59, 333.74);
    expect(c!.abs).toBeCloseTo(-28.15, 2);
    expect(c!.pct).toBeCloseTo(-8.434, 2);
  });

  it('returns null for missing or non-positive references', () => {
    expect(changeFromReference(100, 0)).toBeNull();
    expect(changeFromReference(100, NaN)).toBeNull();
    expect(changeFromReference(NaN, 100)).toBeNull();
  });
});

describe('targetDistance', () => {
  it('is a plain informative percentage from current price to target', () => {
    expect(targetDistance('200', '250')).toBeCloseTo(25);
    expect(targetDistance('230', '200')).toBeCloseTo(-13.043, 2);
    expect(targetDistance('250', '250')).toBeCloseTo(0);
  });

  it('returns null when either side is missing/invalid', () => {
    expect(targetDistance(null, '250')).toBeNull();
    expect(targetDistance('200', null)).toBeNull();
    expect(targetDistance('0', '250')).toBeNull();
  });
});

describe('latestFetchedAt', () => {
  it('returns the newest fetch timestamp across rows', () => {
    expect(
      latestFetchedAt([{ priceFetchedAt: 100 }, { priceFetchedAt: 300 }, { priceFetchedAt: 200 }])
    ).toBe(300);
  });

  it('ignores rows that were never fetched', () => {
    expect(latestFetchedAt([{ priceFetchedAt: null }, { priceFetchedAt: 150 }])).toBe(150);
  });

  it('returns null when nothing has been fetched', () => {
    expect(latestFetchedAt([])).toBeNull();
    expect(latestFetchedAt([{ priceFetchedAt: null }])).toBeNull();
  });
});

describe('formatNativePrice', () => {
  it('formats using the currency of the stock', () => {
    expect(formatNativePrice(230.1, 'USD', 'en-US')).toBe('$230.10');
    expect(formatNativePrice(85.2, 'EUR', 'en-US')).toBe('€85.20');
  });

  it('falls back gracefully for unknown currency codes', () => {
    // GBp: Intl case-folds to GBP on current ICU (renders £), while other ICUs may reject it
    expect(formatNativePrice(85.2, 'GBp', 'en-US')).toMatch(/^(£85\.20|85\.20\sGBp)$/);
    expect(formatNativePrice(85.2, null, 'en-US')).toBe('85.20');
    // Locale-aware formatting: Czech uses comma as decimal separator
    expect(formatNativePrice(85.2, null, 'cs-CZ')).toBe('85,20');
  });

  it('falls back to plain format for invalid currency codes', () => {
    // 'notacur' triggers the catch block; fallback is plain number + currency code
    expect(formatNativePrice(85.2, 'notacur', 'en-US')).toBe('85.20 notacur');
    expect(formatNativePrice(85.2, 'notacur', 'cs-CZ')).toBe('85,20 notacur');
  });

  it('omits decimals for currencies that do not use them', () => {
    // en-US renders the narrow CZK symbol with a non-breaking space before the amount
    expect(formatNativePrice(1234.56, 'CZK', 'en-US')).toBe('K\u010d\u00a01,235');
    expect(formatNativePrice(1234.56, 'JPY', 'en-US')).toBe('¥1,235');
  });

  it('can show an explicit plus for changes and never a signed zero', () => {
    expect(formatNativePrice(1.5, 'USD', 'en-US', undefined, true)).toBe('+\u00a0$1.50');
    expect(formatNativePrice(-1.5, 'USD', 'en-US', undefined, true)).toBe('\u2212\u00a0$1.50');
    expect(formatNativePrice(-0.001, 'USD', 'en-US', undefined, true)).toBe('$0.00');
  });

  it('returns em dash for non-finite values', () => {
    expect(formatNativePrice(NaN, 'USD', 'en-US')).toBe('—');
  });

  it('drops decimals when the formatted value exceeds maxLength', () => {
    // cs-CZ: "1 234,56 $" (10 chars) and "1 234,6 $" (9) are too wide for an 8-char card
    expect(formatNativePrice(1234.56, 'USD', 'cs-CZ', 8)).toBe('1\u00A0235\u00A0$');
  });

  it('keeps decimals when the value already fits maxLength', () => {
    expect(formatNativePrice(65.08, 'USD', 'cs-CZ', 10)).toBe('65,08\u00A0$');
  });
});

describe('percentFromFraction', () => {
  it('converts a stored fraction string to a percent number', () => {
    expect(percentFromFraction('0.004400')).toBeCloseTo(0.44);
    expect(percentFromFraction(null)).toBeNull();
    expect(percentFromFraction('abc')).toBeNull();
  });
});

describe('formatMarketCap', () => {
  it('compacts large values', () => {
    expect(formatMarketCap('3120000000000', 'USD', 'en-US')).toBe('$3.12T');
    expect(formatMarketCap(null, 'USD', 'en-US')).toBeNull();
  });

  it('keeps an unknown currency code as a suffix of the compact number', () => {
    expect(formatMarketCap('3120000000000', 'notacur', 'en-US')).toBe('3.12T notacur');
  });

  it('uses plain compact notation when currency is null', () => {
    expect(formatMarketCap('3120000000000', null, 'en-US')).toBe('3.12T');
  });

  it('drops decimals when the compact value exceeds maxLength', () => {
    // cs-CZ: "327,06 mld. $" (13 chars) and "327,1 mld. $" (12) overflow a 10-char card
    expect(formatMarketCap('327060000000', 'USD', 'cs-CZ', 10)).toBe('327\u00A0mld.\u00A0$');
  });

  it('keeps decimals when the compact value already fits maxLength', () => {
    expect(formatMarketCap('3120000000000', 'USD', 'en-US', 12)).toBe('$3.12T');
  });

  it('steps down one decimal at a time, keeping as much precision as fits', () => {
    // cs-CZ 4.46e12: "4,46 bil. $" (11) is too long for 10, but "4,5 bil. $" (10)
    // fits — so it must not fall all the way to "4 bil. $"
    expect(formatMarketCap('4459835424768', 'USD', 'cs-CZ', 10)).toBe('4,5\u00A0bil.\u00A0$');
  });
});

describe('CHART_PERIODS', () => {
  it('matches spec D9 order', () => {
    expect(CHART_PERIODS).toEqual(['1D', '5D', '1M', '6M', 'YTD', '1Y', '5Y', 'MAX']);
  });
});

describe('inferTargetDirection', () => {
  it('waits for a dip when the target is under the price', () => {
    expect(inferTargetDirection(600, 845)).toBe('below');
  });
  it('waits for a rise when the target is above the price or no price is known', () => {
    expect(inferTargetDirection(900, 845)).toBe('above');
    expect(inferTargetDirection(600, null)).toBe('above');
  });
});

describe('targetReached', () => {
  it('a dip target is reached at or under the target', () => {
    expect(targetReached(845, 600, 'below')).toBe(false);
    expect(targetReached(600, 600, 'below')).toBe(true);
    expect(targetReached(598, 600, 'below')).toBe(true);
  });
  it('a rise target is reached at or over the target', () => {
    expect(targetReached(845, 900, 'above')).toBe(false);
    expect(targetReached(905, 900, 'above')).toBe(true);
  });
  it('a target without a direction keeps the old meaning (at or above)', () => {
    expect(targetReached(845, 600, null)).toBe(true);
  });
  it('is never reached without both numbers', () => {
    expect(targetReached(null, 600, 'below')).toBe(false);
    expect(targetReached(600, null, 'below')).toBe(false);
  });
});
