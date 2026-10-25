import { describe, expect, it } from 'vitest';
import type { EnrichedStockInvestment } from '@shared/schema';
import {
  amountOf,
  costPerUnit,
  toStockRow,
  topPerformer,
  totalOf,
  tradeCurrency,
  tradePricePrefill,
  unpricedOpenCoins,
  type CryptoRow,
} from './positions';

function stock(overrides: Partial<EnrichedStockInvestment> = {}): EnrichedStockInvestment {
  return {
    id: 's1',
    ticker: 'AAPL',
    companyName: 'Apple',
    quantity: '10',
    averagePrice: '100',
    averagePriceCurrency: 'USD',
    originalPrice: '110',
    currency: 'USD',
    fetchedAt: 1_700_000_000,
    isManualPrice: false,
    originalDividendYield: 1,
    dividendCurrency: 'USD',
    isManualDividend: false,
    price: '110',
    priceCurrency: 'USD',
    value: '1136.36',
    costBasis: '1000.00',
    unrealizedGain: '136.36',
    unrealizedGainPercent: 13.636,
    realizedGain: '0.00',
    dividendPerShare: '1',
    yearlyDividend: '11.36',
    ...overrides,
  };
}

describe('amountOf', () => {
  it('parses decimal text and keeps null', () => {
    expect(amountOf('1136.36')).toBe(1136.36);
    expect(amountOf(null)).toBeNull();
    expect(amountOf('not a number')).toBeNull();
  });
});

describe('totalOf', () => {
  it('reads 0 before the response, null for an unknown total', () => {
    expect(totalOf(undefined)).toBe(0);
    expect(totalOf(null)).toBeNull();
    expect(totalOf('12.50')).toBe(12.5);
  });
});

describe('toStockRow', () => {
  it('takes the backend figures without recomputing them', () => {
    const row = toStockRow(stock());
    expect(row.marketValue).toBe(1136.36);
    expect(row.totalCost).toBe(1000);
    expect(row.gainLoss).toBe(136.36);
    expect(row.originalCurrentPrice).toBe(110);
    expect(row.originalCurrency).toBe('USD');
    expect(row.originalAvgCostCurrency).toBe('USD');
    expect(row.yearlyDividend).toBe(11.36);
  });

  it('keeps unconvertible figures null and an unpriced position at price 0', () => {
    const row = toStockRow(
      stock({ value: null, unrealizedGain: null, unrealizedGainPercent: null, price: null })
    );
    expect(row.marketValue).toBeNull();
    expect(row.gainLoss).toBeNull();
    expect(row.gainLossPercent).toBeNull();
    expect(row.originalCurrentPrice).toBe(0);
  });
});

describe('topPerformer', () => {
  it('picks the open position with the highest known percent', () => {
    const a = toStockRow(stock({ id: 'a', unrealizedGainPercent: 5 }));
    const b = toStockRow(stock({ id: 'b', unrealizedGainPercent: null }));
    const c = toStockRow(stock({ id: 'c', unrealizedGainPercent: 50, quantity: '0' }));
    const d = toStockRow(stock({ id: 'd', unrealizedGainPercent: 9 }));
    expect(topPerformer([a, b, c, d])?.id).toBe('d');
    expect(topPerformer([b, c])).toBeNull();
  });
});

describe('costPerUnit', () => {
  it('divide only known, positive bases', () => {
    expect(costPerUnit(1000, 10)).toBe(100);
    expect(costPerUnit(null, 10)).toBeNull();
    expect(costPerUnit(1000, 0)).toBeNull();
  });
});

describe('trade form defaults', () => {
  it('prefills the price only when it is quoted in the trade currency', () => {
    const same = toStockRow(stock({ price: '0.9531234', priceCurrency: 'USD' }));
    expect(tradeCurrency(same)).toBe('USD');
    expect(tradePricePrefill(same)).toBe(0.953123);
    // average cost in CZK, quote in USD: the USD price must not land in a CZK field
    const foreign = toStockRow(stock({ averagePriceCurrency: 'CZK', priceCurrency: 'USD' }));
    expect(tradeCurrency(foreign)).toBe('CZK');
    expect(tradePricePrefill(foreign)).toBe(0);
  });

  it('falls back to the price currency without an average cost currency', () => {
    const row = toStockRow(stock({ averagePriceCurrency: '', priceCurrency: 'EUR' }));
    expect(tradeCurrency(row)).toBe('EUR');
    expect(tradePricePrefill(row)).toBe(110);
    expect(tradePricePrefill(toStockRow(stock({ price: null })))).toBe(0);
  });
});

describe('unpricedOpenCoins', () => {
  const coin = (id: string, yahooSymbol: string | null, quantity: string): CryptoRow => ({
    ...toStockRow(stock({ id, quantity })),
    name: id,
    yahooSymbol,
  });

  it('keeps open coins without a symbol and skips priced and sold-out ones', () => {
    const rows = [
      coin('sui', null, '5'),
      coin('btc', 'BTC-USD', '1'),
      coin('arb', null, '0'),
      coin('jup', null, '2'),
    ];
    expect(unpricedOpenCoins(rows).map((r) => r.id)).toEqual(['sui', 'jup']);
  });
});
