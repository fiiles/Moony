import { describe, it, expect } from 'vitest';
import { groupHoldingsByTicker, getInstrumentIcon } from './stocks';
import type { HoldingData } from './stocks';

function makeHolding(overrides: Partial<HoldingData> = {}): HoldingData {
  return {
    id: 'test-id',
    ticker: 'AAPL',
    companyName: 'Apple Inc.',
    quantity: 10,
    avgCost: 150,
    totalCost: 1500,
    currentPrice: 180,
    marketValue: 1800,
    gainLoss: 300,
    gainLossPercent: 20,
    ...overrides,
  };
}

describe('groupHoldingsByTicker', () => {
  it('returns holdings unchanged when all tickers are unique', () => {
    const holdings = [
      makeHolding({ ticker: 'AAPL', id: '1' }),
      makeHolding({ ticker: 'MSFT', id: '2' }),
    ];
    const result = groupHoldingsByTicker(holdings);
    expect(result).toHaveLength(2);
  });

  it('merges two holdings with the same ticker', () => {
    const holdings = [
      makeHolding({ ticker: 'AAPL', id: '1', quantity: 10, totalCost: 1500, marketValue: 1800 }),
      makeHolding({ ticker: 'AAPL', id: '2', quantity: 5, totalCost: 750, marketValue: 900 }),
    ];
    const result = groupHoldingsByTicker(holdings);
    expect(result).toHaveLength(1);
    expect(result[0].quantity).toBe(15);
    expect(result[0].totalCost).toBe(2250);
    expect(result[0].marketValue).toBe(2700);
  });

  it('calculates gainLoss correctly for merged group', () => {
    const holdings = [
      makeHolding({ ticker: 'TSLA', id: '1', quantity: 2, totalCost: 400, marketValue: 600 }),
      makeHolding({ ticker: 'TSLA', id: '2', quantity: 3, totalCost: 600, marketValue: 750 }),
    ];
    const result = groupHoldingsByTicker(holdings);
    expect(result[0].gainLoss).toBeCloseTo(350, 2); // 1350 - 1000
  });

  it('calculates avgCost per unit for merged group', () => {
    const holdings = [
      makeHolding({ ticker: 'GOOG', id: '1', quantity: 4, totalCost: 800, marketValue: 1000 }),
      makeHolding({ ticker: 'GOOG', id: '2', quantity: 6, totalCost: 1200, marketValue: 1500 }),
    ];
    const result = groupHoldingsByTicker(holdings);
    expect(result[0].avgCost).toBeCloseTo(200, 2); // 2000 / 10
  });

  it('uses synthetic id for merged group', () => {
    const holdings = [
      makeHolding({ ticker: 'NVDA', id: 'a' }),
      makeHolding({ ticker: 'NVDA', id: 'b' }),
    ];
    const result = groupHoldingsByTicker(holdings);
    expect(result[0].id).toBe('group-NVDA');
  });

  it('returns empty array for empty input', () => {
    expect(groupHoldingsByTicker([])).toHaveLength(0);
  });
});

describe('getInstrumentIcon', () => {
  it('returns a valid Tailwind color class', () => {
    const result = getInstrumentIcon('AAPL');
    expect(result).toMatch(/^bg-/);
  });

  it('returns consistent result for the same ticker', () => {
    expect(getInstrumentIcon('TSLA')).toBe(getInstrumentIcon('TSLA'));
  });

  it('returns different colors for different tickers', () => {
    const colors = new Set(
      ['AAPL', 'MSFT', 'TSLA', 'GOOG', 'AMZN', 'META', 'NVDA'].map(getInstrumentIcon)
    );
    expect(colors.size).toBeGreaterThan(1);
  });
});
