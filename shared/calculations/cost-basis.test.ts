import { describe, expect, test } from 'vitest';
import {
  calculatePositionCostBasis,
  type CostBasisTransaction,
  type DatedConvertFn,
} from './cost-basis';

const DAY = 86_400;
const D1 = 1_700_006_400; // midnight UTC
const D2 = D1 + DAY;

// USD/CZK: 25.0 on D1, 20.0 on D2 — the rate depends on the day, which is
// the whole point.
const USD_CZK: Record<number, number> = { [D1]: 25.0, [D2]: 20.0 };

const convertAt: DatedConvertFn = (amount, from, to, dateTs) => {
  const rate = (ccy: string): number => {
    if (ccy === 'CZK') return 1.0;
    if (ccy === 'USD') return USD_CZK[dateTs] ?? Number.NaN;
    throw new Error(`no rate for ${ccy}`);
  };
  return (amount * rate(from)) / rate(to);
};

function buy(
  quantity: number,
  price: number,
  currency: string,
  date: number
): CostBasisTransaction {
  return {
    type: 'buy',
    quantity: String(quantity),
    pricePerUnit: String(price),
    currency,
    transactionDate: date,
  };
}

function sell(
  quantity: number,
  price: number,
  currency: string,
  date: number
): CostBasisTransaction {
  return { ...buy(quantity, price, currency, date), type: 'sell' };
}

describe('calculatePositionCostBasis', () => {
  test("converts each transaction at its own day's rate", () => {
    // Two identical USD buys on days with different rates: CZK cost must
    // be the per-day sum (45 000), never quantity × average × one rate.
    const txs = [buy(10, 100, 'USD', D1), buy(10, 100, 'USD', D2)];

    const result = calculatePositionCostBasis(txs, convertAt, 'CZK');

    expect(result.quantity).toBeCloseTo(20);
    expect(result.costBasis).toBeCloseTo(45_000);
    expect(result.averageCost).toBeCloseTo(2_250);
  });

  test('sell releases cost proportionally at the running average', () => {
    const txs = [buy(10, 100, 'USD', D1), buy(10, 100, 'USD', D2), sell(10, 150, 'USD', D2)];

    const result = calculatePositionCostBasis(txs, convertAt, 'CZK');

    expect(result.quantity).toBeCloseTo(10);
    expect(result.costBasis).toBeCloseTo(22_500);
  });

  test('sorts by transaction date before replaying', () => {
    const txs = [sell(10, 150, 'USD', D2 + 1), buy(10, 100, 'USD', D2), buy(10, 100, 'USD', D1)];

    const result = calculatePositionCostBasis(txs, convertAt, 'CZK');

    expect(result.costBasis).toBeCloseTo(22_500);
  });

  test('mixed transaction currencies convert per transaction', () => {
    // Crypto case: one CZK buy, one USD buy — the USD buy converts at
    // ITS day's rate (20), not any other.
    const txs = [buy(1, 500_000, 'CZK', D1), buy(1, 30_000, 'USD', D2)];

    const result = calculatePositionCostBasis(txs, convertAt, 'CZK');

    expect(result.quantity).toBeCloseTo(2);
    expect(result.costBasis).toBeCloseTo(1_100_000);
  });

  test("target in the transaction's own currency needs no rate", () => {
    const txs = [buy(20, 100, 'USD', D1)];

    const result = calculatePositionCostBasis(txs, convertAt, 'USD');

    expect(result.costBasis).toBeCloseTo(2_000);
    expect(result.averageCost).toBeCloseTo(100);
  });

  test('empty transactions yield a zero position', () => {
    const result = calculatePositionCostBasis([], convertAt, 'CZK');

    expect(result.quantity).toBe(0);
    expect(result.costBasis).toBe(0);
    expect(result.averageCost).toBe(0);
  });

  test('oversell clamps to zero, never negative', () => {
    const txs = [buy(10, 100, 'USD', D1), sell(15, 100, 'USD', D1)];

    const result = calculatePositionCostBasis(txs, convertAt, 'CZK');

    expect(result.quantity).toBe(0);
    expect(result.costBasis).toBe(0);
    expect(result.averageCost).toBe(0);
  });

  test('same-day sell listed before its buys (API newest-first) still releases cost', () => {
    // The API orders by transaction_date DESC without a tiebreak; all
    // same-day transactions share one UTC-midnight timestamp, so the sell
    // can arrive first. Buys must be processed before sells on a day.
    const txs = [sell(10, 150, 'USD', D1), buy(10, 100, 'USD', D1), buy(10, 100, 'USD', D1)];

    const result = calculatePositionCostBasis(txs, convertAt, 'CZK');

    expect(result.quantity).toBeCloseTo(10);
    expect(result.costBasis).toBeCloseTo(25_000); // half of 2 × 1 000 USD × 25
    expect(result.averageCost).toBeCloseTo(2_500);
  });

  test('createdAt breaks same-day ties between transactions of the same type', () => {
    // Two same-day buys: order between them does not change the result,
    // but a later-created sell must still follow the buys.
    const txs: CostBasisTransaction[] = [
      { ...sell(5, 200, 'USD', D1), createdAt: 300 },
      { ...buy(10, 100, 'USD', D1), createdAt: 100 },
    ];
    const result = calculatePositionCostBasis(txs, convertAt, 'CZK');
    expect(result.quantity).toBeCloseTo(5);
    expect(result.costBasis).toBeCloseTo(12_500);
  });

  test('an oversell only closes the held quantity', () => {
    // buy 10 @ 100, sell 15 @ 120, buy 10 @ 200: the extra 5 sold units do not
    // exist, so the position after the second buy is 10 @ 200, not 5 @ 300.
    const txs = [
      buy(10, 100, 'USD', D1),
      sell(15, 120, 'USD', D1 + 1),
      buy(10, 200, 'USD', D1 + 2),
    ];
    const result = calculatePositionCostBasis(txs, convertAt, 'USD');
    expect(result.quantity).toBeCloseTo(10);
    expect(result.averageCost).toBeCloseTo(200);
  });
});
