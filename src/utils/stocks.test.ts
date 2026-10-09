import { describe, it, expect } from 'vitest';
import { getInstrumentIcon } from './stocks';

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
