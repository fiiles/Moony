import { describe, expect, it } from 'vitest';
import { cashTrendPoints } from './cash-trend';
const DAY = 86_400;
const TODAY = Date.UTC(2026, 9, 16) / 1000;
const point = (recordedAt: number, totalSavings: string | null) => ({
  recordedAt,
  totalSavings,
  source: 'live',
  isStale: false,
  totalLoansPrincipal: null,
  totalInvestments: null,
  totalCrypto: null,
  totalBonds: null,
  totalRealEstatePersonal: null,
  totalRealEstateInvestment: null,
  totalOtherAssets: null,
  netWorth: null,
});

describe('cashTrendPoints', () => {
  it("plots the recorded cash and ends on today's total", () => {
    const history = {
      currency: 'CZK',
      points: [point(TODAY - 2 * DAY, '100'), point(TODAY - DAY, null), point(TODAY, '150')],
    };
    expect(cashTrendPoints(history, 'CZK', 214_380, TODAY + 3_600)).toEqual([
      { t: TODAY - 2 * DAY, value: 100 },
      { t: TODAY, value: 214_380 },
    ]);
  });
  it('appends today when the history ends yesterday', () => {
    const history = { currency: 'CZK', points: [point(TODAY - DAY, '100')] };
    const out = cashTrendPoints(history, 'CZK', 120, TODAY + 3_600);
    expect(out).toHaveLength(2);
    expect(out[1].value).toBe(120);
  });
  it('ignores a history in another currency and a null total', () => {
    expect(
      cashTrendPoints({ currency: 'EUR', points: [point(TODAY - DAY, '1')] }, 'CZK', 5, TODAY)
    ).toEqual([{ t: TODAY, value: 5 }]);
    expect(
      cashTrendPoints({ currency: 'CZK', points: [point(TODAY, '1')] }, 'CZK', null, TODAY)
    ).toEqual([]);
  });
});
