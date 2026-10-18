import { describe, expect, it } from 'vitest';
import { classTrendPoints, pointsChange, trendChangeBase } from './class-trend';

const DAY = 86_400;
const T = Date.UTC(2026, 9, 16) / 1000;
const point = (recordedAt: number, totalInvestments: string | null) => ({
  recordedAt,
  source: 'live',
  isStale: false,
  totalSavings: null,
  totalLoansPrincipal: null,
  totalInvestments,
  totalCrypto: null,
  totalBonds: null,
  totalRealEstatePersonal: null,
  totalRealEstateInvestment: null,
  totalOtherAssets: null,
  netWorth: null,
});

describe('classTrendPoints', () => {
  it("plots the recorded class, skips unknown days and ends on today's value", () => {
    const history = {
      currency: 'CZK',
      points: [point(T - 2 * DAY, '100'), point(T - DAY, null), point(T, '150')],
    };
    expect(classTrendPoints(history, 'investments', 'CZK', 2_650_000, T + 3_600)).toEqual([
      { t: T - 2 * DAY, value: 100 },
      { t: T, value: 2_650_000 },
    ]);
  });

  it('appends today when the history ends yesterday and leaves today out when its value is unknown', () => {
    const history = { currency: 'CZK', points: [point(T - DAY, '100')] };
    expect(classTrendPoints(history, 'investments', 'CZK', 120, T + 3_600)).toHaveLength(2);
    expect(
      classTrendPoints(
        { currency: 'CZK', points: [point(T - DAY, '100'), point(T, '1')] },
        'investments',
        'CZK',
        null,
        T + 3_600
      )
    ).toEqual([{ t: T - DAY, value: 100 }]);
  });

  it('ignores a history in another currency', () => {
    expect(
      classTrendPoints(
        { currency: 'EUR', points: [point(T - DAY, '1')] },
        'investments',
        'CZK',
        5,
        T
      )
    ).toEqual([{ t: T, value: 5 }]);
  });
});

describe('pointsChange', () => {
  it('is the last point against the first', () => {
    expect(
      pointsChange([
        { t: 1, value: 100 },
        { t: 2, value: 90 },
        { t: 3, value: 125 },
      ])
    ).toEqual({ delta: 25, ratio: 0.25 });
  });

  it('is null with fewer than two points', () => {
    expect(pointsChange([{ t: 1, value: 100 }])).toBeNull();
    expect(pointsChange([])).toBeNull();
  });
});

describe('trendChangeBase', () => {
  const start = T - 30 * DAY;

  it('is the first plotted day when the line starts later than the period', () => {
    expect(
      trendChangeBase(
        [
          { t: T - 10 * DAY + 3_600, value: 100 },
          { t: T, value: 120 },
        ],
        start
      )
    ).toBe(T - 10 * DAY);
  });

  it('is null when the line reaches back to the start of the period', () => {
    expect(
      trendChangeBase(
        [
          { t: start, value: 100 },
          { t: T, value: 120 },
        ],
        start
      )
    ).toBeNull();
    // a first day or so after an arbitrary start still counts as reaching it
    expect(
      trendChangeBase(
        [
          { t: start + DAY, value: 100 },
          { t: T, value: 120 },
        ],
        start
      )
    ).toBeNull();
  });

  it('is null for "All" (no period start) and without points', () => {
    expect(trendChangeBase([{ t: T - 10 * DAY, value: 100 }], undefined)).toBeNull();
    expect(trendChangeBase([], start)).toBeNull();
  });
});
