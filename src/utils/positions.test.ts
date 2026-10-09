import { describe, expect, it } from 'vitest';
import type { EnrichedStockInvestment } from '@shared/schema';
import { amountOf, costPerUnit, percentOfCost, toStockRow, topPerformer } from './positions';

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

describe('costPerUnit and percentOfCost', () => {
  it('divide only known, positive bases', () => {
    expect(costPerUnit(1000, 10)).toBe(100);
    expect(costPerUnit(null, 10)).toBeNull();
    expect(costPerUnit(1000, 0)).toBeNull();
    expect(percentOfCost(50, 200)).toBe(25);
    expect(percentOfCost(50, 0)).toBe(0);
  });
});
