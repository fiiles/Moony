import { describe, expect, it } from 'vitest';
import { performanceNotes } from './performance-notes';

const clean = {
  fxQuality: { missingCurrencies: [], estimated: false },
  unpricedTickers: [],
  dividendGapTickers: [],
  suspectTradeTickers: [],
};

describe('performanceNotes', () => {
  it('is empty for a clean report', () => {
    expect(performanceNotes(clean as never, 'CZK')).toEqual([]);
  });

  it('lists every note in the desktop order', () => {
    const notes = performanceNotes(
      {
        fxQuality: { missingCurrencies: ['JPY', 'NOK'], estimated: true },
        unpricedTickers: ['AAA'],
        dividendGapTickers: ['BBB', 'CCC'],
        suspectTradeTickers: ['DDD'],
      } as never,
      'CZK'
    );
    expect(notes.map((n) => n.key)).toEqual([
      'common:fxQuality.missing',
      'common:fxQuality.estimated',
      'reports:stocksAnalysis.performance.unpriced',
      'reports:stocksAnalysis.performance.dividendGap',
      'reports:stocksAnalysis.performance.suspectTrade',
    ]);
    expect(notes[0].values).toEqual({ currency: 'CZK', currencies: 'JPY, NOK' });
    expect(notes[3].values).toEqual({ tickers: 'BBB, CCC' });
  });
});
