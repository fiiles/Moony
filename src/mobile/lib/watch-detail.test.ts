import { describe, expect, it } from 'vitest';
import type { StockPricePoint } from '@shared/schema';
import {
  priceOf,
  watchPoints,
  watchPriceAt,
  watchPriceText,
  watchRange,
  watchSincePeriod,
  watchTargetDirection,
  watchTargetForm,
  watchTargetState,
} from './watch-detail';

const DAY = 86_400;
const NOW = Date.UTC(2026, 9, 24, 12, 0) / 1000;

const bar = (timestamp: number, price: number): StockPricePoint => ({
  timestamp,
  price,
  currency: 'USD',
});

describe('priceOf', () => {
  it('reads a price of the wire and nothing else', () => {
    expect(priceOf('187.4')).toBe(187.4);
    expect(priceOf(null)).toBeNull();
    expect(priceOf('')).toBeNull();
    expect(priceOf('n/a')).toBeNull();
  });
});

describe('watchRange', () => {
  it('maps each phone period onto the Yahoo range that covers it', () => {
    expect(watchRange('30D', NOW)).toEqual({ range: '1M', from: null });
    expect(watchRange('YTD', NOW)).toEqual({ range: 'YTD', from: null });
    expect(watchRange('1Y', NOW)).toEqual({ range: '1Y', from: null });
    expect(watchRange('5Y', NOW)).toEqual({ range: '5Y', from: null });
    expect(watchRange('All', NOW)).toEqual({ range: 'MAX', from: null });
  });

  it('takes 90 days from the six-month range and says where to cut it', () => {
    expect(watchRange('90D', NOW)).toEqual({ range: '6M', from: NOW - 90 * DAY });
  });
});

describe('watchTargetState', () => {
  it('has none without a target', () => {
    expect(watchTargetState(100, null, null)).toBe('none');
    expect(watchTargetState(null, null, null)).toBe('none');
  });

  it('is reached once the price has crossed the target in its direction', () => {
    expect(watchTargetState(120, 110, 'above')).toBe('reached');
    expect(watchTargetState(110, 110, 'above')).toBe('reached');
    expect(watchTargetState(90, 100, 'below')).toBe('reached');
  });

  it('is pending while it has not, or while there is no price to compare', () => {
    expect(watchTargetState(100, 110, 'above')).toBe('pending');
    expect(watchTargetState(110, 100, 'below')).toBe('pending');
    expect(watchTargetState(null, 110, 'above')).toBe('pending');
  });

  it('reads a target without a direction as the old "above"', () => {
    expect(watchTargetState(120, 110, null)).toBe('reached');
    expect(watchTargetState(100, 110, null)).toBe('pending');
  });
});

describe('watchPoints', () => {
  const range = [bar(100, 10), bar(200, 12), bar(300, 11)];

  it('is the range as a line of prices', () => {
    expect(watchPoints(range, null, null, null, null)).toEqual([
      { t: 100, value: 10 },
      { t: 200, value: 12 },
      { t: 300, value: 11 },
    ]);
  });

  it("appends today's price when it was fetched after the last point", () => {
    expect(watchPoints(range, null, 13, 400, null)).toEqual([
      { t: 100, value: 10 },
      { t: 200, value: 12 },
      { t: 300, value: 11 },
      { t: 400, value: 13 },
    ]);
  });

  it('overwrites the last point when the price was fetched on or before it', () => {
    expect(watchPoints(range, null, 13, 300, null).at(-1)).toEqual({ t: 300, value: 13 });
    expect(watchPoints(range, null, 13, 250, null).at(-1)).toEqual({ t: 300, value: 13 });
    expect(watchPoints(range, null, 13, 250, null)).toHaveLength(3);
  });

  it('with no fetch time stamps today with the clock', () => {
    const points = watchPoints(range, null, 13, null, null, 500);
    expect(points.at(-1)).toEqual({ t: 500, value: 13 });
  });

  it('keeps the range as it is without a price', () => {
    expect(watchPoints(range, null, null, 400, null).map((p) => p.value)).toEqual([10, 12, 11]);
  });

  it('drops the points before `from`, keeping the one on it', () => {
    expect(watchPoints(range, 200, null, null, null).map((p) => p.t)).toEqual([200, 300]);
    expect(watchPoints(range, 201, 13, 400, null).map((p) => p.t)).toEqual([300, 400]);
  });

  it('has no points where everything is before `from`, even with a price', () => {
    expect(watchPoints(range, 1000, 13, 2000, null)).toEqual([]);
    expect(watchPoints([], null, 13, 2000, null)).toEqual([]);
  });

  it('carries the target on every point, and only with a target', () => {
    const withTarget = watchPoints(range, null, 13, 400, 15);
    expect(withTarget.every((p) => p.cost === 15)).toBe(true);
    expect(withTarget).toHaveLength(4);
    expect(watchPoints(range, null, 13, 400, null).some((p) => 'cost' in p)).toBe(false);
  });
});

describe('watchTargetDirection', () => {
  it('follows the typed price until the person picks', () => {
    expect(watchTargetDirection({ chosen: null, draft: 90, current: 100, stored: null })).toBe(
      'below'
    );
    expect(watchTargetDirection({ chosen: null, draft: 110, current: 100, stored: null })).toBe(
      'above'
    );
    expect(watchTargetDirection({ chosen: null, draft: 90, current: null, stored: null })).toBe(
      'above'
    );
  });

  it('keeps what the person picked whatever is typed', () => {
    expect(watchTargetDirection({ chosen: 'above', draft: 90, current: 100, stored: null })).toBe(
      'above'
    );
    expect(watchTargetDirection({ chosen: 'below', draft: 110, current: 100, stored: null })).toBe(
      'below'
    );
  });

  it('keeps the stored direction of an unchanged target, and with nothing typed', () => {
    const stored = { price: 90, direction: 'below' as const };
    expect(watchTargetDirection({ chosen: null, draft: 90, current: 80, stored })).toBe('below');
    expect(watchTargetDirection({ chosen: null, draft: null, current: 80, stored })).toBe('below');
    expect(watchTargetDirection({ chosen: null, draft: null, current: 80, stored: null })).toBe(
      'above'
    );
  });
});

describe('watchSincePeriod', () => {
  it('names the shortest range that surely holds the day the stock was followed', () => {
    expect(watchSincePeriod(NOW - 3 * DAY, NOW)).toBe('1M');
    expect(watchSincePeriod(NOW - 28 * DAY, NOW)).toBe('1M');
    expect(watchSincePeriod(NOW - 90 * DAY, NOW)).toBe('6M');
    expect(watchSincePeriod(NOW - 300 * DAY, NOW)).toBe('1Y');
    expect(watchSincePeriod(NOW - 3 * 365 * DAY, NOW)).toBe('5Y');
    expect(watchSincePeriod(NOW - 9 * 365 * DAY, NOW)).toBe('MAX');
  });
});

describe('watchPriceAt', () => {
  const series = [bar(100, 10), bar(200, 12), bar(300, 11)];

  it('is the price of the point nearest the day', () => {
    expect(watchPriceAt(series, 190)).toBe(12);
    expect(watchPriceAt(series, 90)).toBe(10);
    expect(watchPriceAt(series, 1000)).toBe(11);
  });

  it('is nothing for a series without points', () => {
    expect(watchPriceAt([], 100)).toBeNull();
  });
});

describe('watchTargetForm', () => {
  it('reads a positive number as the keypad of either language types it', () => {
    expect(watchTargetForm('120.5')).toEqual({ price: 120.5, text: '120.5' });
    expect(watchTargetForm(' 1 250,50 ')).toEqual({ price: 1250.5, text: '1250.50' });
  });

  it('asks for a price when the field is empty', () => {
    expect(watchTargetForm('')).toEqual({ price: null, text: null, error: 'required' });
    expect(watchTargetForm('   ').error).toBe('required');
  });

  it('refuses text that is not a number and numbers that are not positive', () => {
    expect(watchTargetForm('abc').error).toBe('invalid');
    expect(watchTargetForm('12..5').error).toBe('invalid');
    expect(watchTargetForm('0').error).toBe('positive');
    expect(watchTargetForm('-5').error).toBe('positive');
  });
});

describe('watchPriceText', () => {
  const money = (
    amount: number,
    currency: string,
    opts?: { decimals?: number; signed?: boolean }
  ) => `${opts?.signed && amount > 0 ? '+' : ''}${amount.toFixed(opts?.decimals ?? 2)} ${currency}`;

  it("writes a price in the stock's own currency with the decimals of a price", () => {
    expect(watchPriceText({ money }, 187.4, 'USD')).toBe('187.40 USD');
    // From 1000 on the fraction is noise
    expect(watchPriceText({ money }, 1234.5, 'USD')).toBe('1235 USD');
  });

  it('writes the plus of a change when asked', () => {
    expect(watchPriceText({ money }, 2.15, 'USD', true)).toBe('+2.15 USD');
    expect(watchPriceText({ money }, 2.15, 'USD')).toBe('2.15 USD');
  });

  it('has no currency to name for a stock without one', () => {
    expect(watchPriceText({ money }, 5, null)).toBe('5.00 ');
  });
});
