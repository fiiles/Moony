import { describe, expect, test } from 'vitest';
import { calculateRealizedGains, type RealizedGainTransaction } from './realized-gains';
import type { DatedConvertFn } from './cost-basis';

const DAY = 86_400;
const D1 = 1_700_006_400;
const D2 = D1 + DAY;

// USD/CZK: 25.0 on D1, 20.0 on D2.
const USD_CZK: Record<number, number> = { [D1]: 25.0, [D2]: 20.0 };

const convertAt: DatedConvertFn = (amount, from, to, dateTs) => {
  const rate = (ccy: string): number => {
    if (ccy === 'CZK') return 1.0;
    if (ccy === 'USD') return USD_CZK[dateTs] ?? Number.NaN;
    throw new Error(`no rate for ${ccy}`);
  };
  return (amount * rate(from)) / rate(to);
};

function tx(
  type: string,
  ticker: string,
  quantity: number,
  price: number,
  currency: string,
  date: number
): RealizedGainTransaction {
  return {
    type,
    ticker,
    quantity: String(quantity),
    pricePerUnit: String(price),
    currency,
    transactionDate: date,
  };
}

describe('calculateRealizedGains', () => {
  test('uses the buy-day and sell-day rates, not one rate for both', () => {
    // Buy 10 @ 100 USD when USD=25 (cost 25 000 CZK), sell 10 @ 100 USD
    // when USD=20 (proceeds 20 000 CZK). Same USD price — the realized
    // result in CZK is a 5 000 loss, purely from the currency move.
    // Converting both legs at either single rate would report 0.
    const gain = calculateRealizedGains(
      [tx('buy', 'AAA', 10, 100, 'USD', D1), tx('sell', 'AAA', 10, 100, 'USD', D2)],
      convertAt,
      'CZK'
    );

    expect(gain).toBeCloseTo(-5_000);
  });

  test('weighted average cost per ticker, in the target currency', () => {
    // Buy 10 @ 100 (rate 25 -> 25 000), buy 10 @ 200 (rate 20 -> 40 000):
    // avg cost 3 250 CZK/unit. Sell 5 @ 250 at rate 20 -> proceeds 25 000,
    // cost released 16 250 -> gain 8 750.
    const gain = calculateRealizedGains(
      [
        tx('buy', 'AAA', 10, 100, 'USD', D1),
        tx('buy', 'AAA', 10, 200, 'USD', D2),
        tx('sell', 'AAA', 5, 250, 'USD', D2),
      ],
      convertAt,
      'CZK'
    );

    expect(gain).toBeCloseTo(8_750);
  });

  test('tickers are independent', () => {
    const gain = calculateRealizedGains(
      [
        tx('buy', 'AAA', 10, 100, 'USD', D2),
        tx('sell', 'AAA', 10, 110, 'USD', D2),
        tx('buy', 'BBB', 1, 1_000, 'CZK', D1),
        tx('sell', 'BBB', 1, 900, 'CZK', D1),
      ],
      convertAt,
      'CZK'
    );

    // AAA: (110-100)*10*20 = 2 000; BBB: -100.
    expect(gain).toBeCloseTo(1_900);
  });

  test('same-day sell listed before its buy is realized against that buy', () => {
    const txs = [tx('sell', 'AAPL', 10, 150, 'USD', D1), tx('buy', 'AAPL', 10, 100, 'USD', D1)];
    // (150 − 100) × 10 USD × 25 CZK/USD
    expect(calculateRealizedGains(txs, convertAt, 'CZK')).toBeCloseTo(12_500);
  });

  test('an oversell realizes a gain only on the held quantity', () => {
    const txs = [tx('buy', 'AAPL', 10, 100, 'USD', D1), tx('sell', 'AAPL', 15, 120, 'USD', D2)];
    // buy leg at D1 (25 CZK/USD), sell leg at D2 (20 CZK/USD), 10 held units — not 15:
    // (120 × 20 − 100 × 25) × 10
    expect(calculateRealizedGains(txs, convertAt, 'CZK')).toBeCloseTo((120 * 20 - 100 * 25) * 10);
  });
});
