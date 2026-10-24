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
  quantity: string,
  pricePerUnit = '10',
  currency = 'CZK'
): OtherAssetTransaction => ({
  id: `t-${type}-${transactionDate}-${currency}`,
  assetId: 'a1',
  type,
  quantity,
  pricePerUnit,
  currency,
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
    const trades = [trade('buy', day(2), '2', '9'), trade('buy', day(7), '1')];
    // 1. 2.: no valuation on or before it, so the buy is priced by its own 9; 1. 7. takes the 1. 6. price
    expect(otherAssetTrace(valuations, trades, 'CZK', 13, 3, NOW)).toEqual([
      { t: day(2), value: 18 },
      { t: day(3), value: 20 },
      { t: day(6), value: 24 },
      { t: day(7), value: 36 },
      { t: NOW, value: 39 },
    ]);
  });

  it('is today alone without a valuation or a trade priced in the price currency', () => {
    expect(otherAssetTrace([], [], 'CZK', 12, 2, NOW)).toEqual([{ t: NOW, value: 24 }]);
    expect(otherAssetTrace([], [trade('buy', day(2), '2', '5', 'EUR')], 'CZK', 12, 2, NOW)).toEqual(
      [{ t: NOW, value: 24 }]
    );
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
      { t: day(2), value: 20 },
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

describe('otherAssetTrace: a trade as a price observation', () => {
  const day2025 = (month: number, d = 1) => Date.UTC(2025, month - 1, d) / 1000;

  it('starts a fresh asset with a first purchase and no valuation at that purchase', () => {
    // "Zlato 1 oz": 2 × 65 000 Kč bought on 1. 3. 2025, today's price 72 000 Kč, no valuation
    const trades = [trade('buy', day2025(3), '2', '65000')];
    expect(otherAssetTrace([], trades, 'CZK', 72_000, 2, NOW)).toEqual([
      { t: day2025(3), value: 130_000 },
      { t: NOW, value: 144_000 },
    ]);
  });

  it('prices a trade by its own price only until a valuation is on or before its day', () => {
    const valuations = [valuation(day(3), '10', 'CZK')];
    const trades = [trade('buy', day(2), '2', '9'), trade('buy', day(4), '1', '99')];
    // 1. 2.: the trade's 9; 1. 4.: the 1. 3. valuation wins over the trade's 99
    expect(otherAssetTrace(valuations, trades, 'CZK', 10, 3, NOW)).toEqual([
      { t: day(2), value: 18 },
      { t: day(3), value: 20 },
      { t: day(4), value: 30 },
      { t: NOW, value: 30 },
    ]);
  });

  it('lets a valuation on the trade day win over the trade price', () => {
    const valuations = [valuation(day(3), '10', 'CZK')];
    expect(
      otherAssetTrace(valuations, [trade('buy', day(3), '2', '99')], 'CZK', 10, 2, NOW)
    ).toEqual([
      { t: day(3), value: 20 },
      { t: NOW, value: 20 },
    ]);
  });

  it("reads the trade's currency without regard to case", () => {
    const trades = [trade('buy', day(2), '2', '9', 'czk')];
    expect(otherAssetTrace([], trades, 'CZK', 10, 2, NOW)).toEqual([
      { t: day(2), value: 18 },
      { t: NOW, value: 20 },
    ]);
  });

  it('gives a trade in another currency no point before the first valuation', () => {
    const valuations = [valuation(day(3), '10', 'CZK')];
    const trades = [trade('buy', day(2), '2', '5', 'EUR')];
    expect(otherAssetTrace(valuations, trades, 'CZK', 10, 2, NOW)).toEqual([
      { t: day(3), value: 20 },
      { t: NOW, value: 20 },
    ]);
  });

  it('keeps an edited currency apart: CZK then EUR valuations with buys and sells in both', () => {
    const valuations = [
      valuation(day(1), '10', 'CZK'),
      valuation(day(6), '12', 'CZK'),
      valuation(day(9), '5', 'EUR'),
    ];
    const trades = [
      // in CZK before the first EUR valuation: no price to value them at, but they move the quantity
      trade('buy', day(2), '2', '10', 'CZK'),
      trade('buy', day(7), '1', '12', 'CZK'),
      // an EUR trade the day after the EUR valuation steps the quantity at that valuation's price
      trade('buy', day(10), '1', '6', 'EUR'),
      trade('sell', day(10, 5), '2', '6', 'EUR'),
    ];
    expect(otherAssetTrace(valuations, trades, 'EUR', 6, 2, NOW)).toEqual([
      { t: day(9), value: 15 },
      { t: day(10), value: 20 },
      { t: day(10, 5), value: 10 },
      { t: NOW, value: 12 },
    ]);
  });

  it('prices an EUR trade before the first EUR valuation by itself, ignoring older CZK estimates', () => {
    const valuations = [valuation(day(1), '10', 'CZK'), valuation(day(9), '5', 'EUR')];
    const trades = [trade('buy', day(4), '3', '4', 'EUR'), trade('buy', day(5), '1', '10', 'CZK')];
    // The CZK buy has no price of its own here but moves the quantity at the newest EUR estimate (4)
    expect(otherAssetTrace(valuations, trades, 'EUR', 5, 4, NOW)).toEqual([
      { t: day(4), value: 12 },
      { t: day(5), value: 16 },
      { t: day(9), value: 20 },
      { t: NOW, value: 20 },
    ]);
  });

  it('steps down on a partial sell (3 to 2)', () => {
    const valuations = [valuation(day(1), '10', 'CZK')];
    const trades = [trade('buy', day(2), '3'), trade('sell', day(5), '1')];
    expect(otherAssetTrace(valuations, trades, 'CZK', 10, 2, NOW)).toEqual([
      { t: day(1), value: 0 },
      { t: day(2), value: 30 },
      { t: day(5), value: 20 },
      { t: NOW, value: 20 },
    ]);
  });

  it('clamps an over-sell at 0 instead of going negative', () => {
    const valuations = [valuation(day(1), '10', 'CZK')];
    const trades = [trade('buy', day(2), '1'), trade('sell', day(5), '3')];
    expect(otherAssetTrace(valuations, trades, 'CZK', 10, 0, NOW)).toEqual([
      { t: day(1), value: 0 },
      { t: day(2), value: 10 },
      { t: day(5), value: 0 },
      { t: NOW, value: 0 },
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

  it("keeps the points from the period's start on, a point on the start itself included", () => {
    expect(trimTrace(trace, day(6))).toEqual(trace.slice(1));
    expect(trimTrace(trace, day(3))).toEqual(trace);
  });

  it('carries the estimate in force at the start onto the start (a step line holds its value)', () => {
    expect(trimTrace(trace, day(6) + 1)).toEqual([
      { t: day(6) + 1, value: 24 },
      { t: day(7), value: 36 },
      { t: NOW, value: 39 },
    ]);
  });

  it('draws the held value across a period with no point of its own', () => {
    // Not revalued or traded since 1. 7.: the line is that value from the start to today
    expect(trimTrace(trace, day(8))).toEqual([
      { t: day(8), value: 36 },
      { t: NOW, value: 39 },
    ]);
  });

  it('carries a point with its cost along', () => {
    const costed = [
      { t: day(3), value: 20, cost: 18 },
      { t: NOW, value: 30, cost: 18 },
    ];
    expect(trimTrace(costed, day(5))).toEqual([
      { t: day(5), value: 20, cost: 18 },
      { t: NOW, value: 30, cost: 18 },
    ]);
  });

  it('keeps everything without a start (Vše)', () => {
    expect(trimTrace(trace, undefined)).toEqual(trace);
  });

  it("is today alone when nothing is earlier, and always keeps today's point", () => {
    expect(trimTrace([{ t: NOW, value: 39 }], day(8))).toEqual([{ t: NOW, value: 39 }]);
    expect(trimTrace(trace, NOW + HOUR)).toEqual([{ t: NOW, value: 39 }]);
    expect(trimTrace([], day(6))).toEqual([]);
  });
});
