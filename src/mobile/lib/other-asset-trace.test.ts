import { describe, expect, it } from 'vitest';
import type { AssetValuation, OtherAssetTransaction } from '@shared/schema';
import { otherAssetTrace, trimTrace } from './other-asset-trace';

const day = (month: number, d = 1) => Date.UTC(2026, month - 1, d) / 1000;
const HOUR = 3_600;
/** 24. 10. 2026, mid-morning UTC. */
const NOW = day(10, 24) + 9 * HOUR;

const valuation = (valuedAt: number, value: string, currency: string): AssetValuation => ({
  id: `v-${valuedAt}-${currency}`,
  assetId: 'a1',
  value,
  currency,
  valuedAt,
  note: null,
  createdAt: valuedAt,
});
const trade = (
  type: 'buy' | 'sell',
  transactionDate: number,
  quantity: string
): OtherAssetTransaction => ({
  id: `t-${type}-${transactionDate}`,
  assetId: 'a1',
  type,
  quantity,
  pricePerUnit: '10',
  currency: 'CZK',
  transactionDate,
  createdAt: transactionDate,
});

describe('otherAssetTrace', () => {
  it('counts only the valuations in the price currency (an edited currency)', () => {
    const valuations = [
      valuation(day(3), '10', 'CZK'),
      valuation(day(6), '12', 'CZK'),
      valuation(day(9), '5', 'EUR'),
    ];
    // No trades at all: the quantity known is today's, for every valuation day (ruling P3)
    expect(otherAssetTrace(valuations, [], 'EUR', 5, 3, NOW)).toEqual([
      { t: day(9), value: 15 },
      { t: NOW, value: 15 },
    ]);
  });

  it('steps the line by the quantity held from each buy, on valuation and trade days', () => {
    const valuations = [valuation(day(3), '10', 'CZK'), valuation(day(6), '12', 'CZK')];
    const trades = [trade('buy', day(2), '2'), trade('buy', day(7), '1')];
    // 1. 2.: a buy with no valuation on or before it has no point; 1. 7. takes the 1. 6. price
    expect(otherAssetTrace(valuations, trades, 'CZK', 13, 3, NOW)).toEqual([
      { t: day(3), value: 20 },
      { t: day(6), value: 24 },
      { t: day(7), value: 36 },
      { t: NOW, value: 39 },
    ]);
  });

  it('is today alone without a valuation, whatever the trades', () => {
    expect(otherAssetTrace([], [], 'CZK', 12, 2, NOW)).toEqual([{ t: NOW, value: 24 }]);
    expect(otherAssetTrace([], [trade('buy', day(2), '2')], 'CZK', 12, 2, NOW)).toEqual([
      { t: NOW, value: 24 },
    ]);
  });

  it('is today alone when every valuation is in another currency', () => {
    expect(otherAssetTrace([valuation(day(3), '10', 'CZK')], [], 'EUR', 5, 2, NOW)).toEqual([
      { t: NOW, value: 10 },
    ]);
  });

  it('drops to 0 on the day a sell takes the quantity to 0', () => {
    const valuations = [valuation(day(3), '10', 'CZK'), valuation(day(6), '12', 'CZK')];
    const trades = [trade('buy', day(2), '2'), trade('sell', day(7), '2')];
    expect(otherAssetTrace(valuations, trades, 'CZK', 12, 0, NOW)).toEqual([
      { t: day(3), value: 20 },
      { t: day(6), value: 24 },
      { t: day(7), value: 0 },
      { t: NOW, value: 0 },
    ]);
  });

  it('holds 0 before the first buy', () => {
    const valuations = [valuation(day(3), '10', 'CZK')];
    const trades = [trade('buy', day(5), '4')];
    expect(otherAssetTrace(valuations, trades, 'CZK', 10, 4, NOW)).toEqual([
      { t: day(3), value: 0 },
      { t: day(5), value: 40 },
      { t: NOW, value: 40 },
    ]);
  });

  it('puts a trade on a valuation day into that day, not after it', () => {
    const valuations = [valuation(day(3), '10', 'CZK')];
    const trades = [trade('buy', day(3), '2'), trade('buy', day(3), '1')];
    expect(otherAssetTrace(valuations, trades, 'CZK', 10, 3, NOW)).toEqual([
      { t: day(3), value: 30 },
      { t: NOW, value: 30 },
    ]);
  });

  it("replaces the point of today's UTC day with today's figures", () => {
    const valuations = [valuation(day(10, 24), '11', 'CZK')];
    expect(otherAssetTrace(valuations, [], 'CZK', 12, 2, NOW)).toEqual([
      { t: day(10, 24), value: 24 },
    ]);
  });

  it('leaves out a day after today and orders the points by day whatever the input order', () => {
    const valuations = [
      valuation(day(10, 25), '99', 'CZK'),
      valuation(day(6), '12', 'CZK'),
      valuation(day(3), '10', 'CZK'),
    ];
    expect(otherAssetTrace(valuations, [], 'CZK', 12, 1, NOW)).toEqual([
      { t: day(3), value: 10 },
      { t: day(6), value: 12 },
      { t: NOW, value: 12 },
    ]);
  });

  it('skips a valuation whose value is not a number', () => {
    const valuations = [valuation(day(3), 'NaN', 'CZK'), valuation(day(6), '12', 'CZK')];
    expect(otherAssetTrace(valuations, [], 'CZK', 12, 1, NOW)).toEqual([
      { t: day(6), value: 12 },
      { t: NOW, value: 12 },
    ]);
  });
});

describe('trimTrace', () => {
  const trace = [
    { t: day(3), value: 20 },
    { t: day(6), value: 24 },
    { t: day(7), value: 36 },
    { t: NOW, value: 39 },
  ];

  it("keeps the points at or after the period's start", () => {
    expect(trimTrace(trace, day(6))).toEqual(trace.slice(1));
    expect(trimTrace(trace, day(6) + 1)).toEqual(trace.slice(2));
  });

  it('keeps everything without a start (Vše)', () => {
    expect(trimTrace(trace, undefined)).toEqual(trace);
  });

  it("always keeps today's point, the last one", () => {
    expect(trimTrace(trace, NOW + HOUR)).toEqual([{ t: NOW, value: 39 }]);
    expect(trimTrace([], day(6))).toEqual([]);
  });
});
