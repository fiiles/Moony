import type { StockPerformance } from '@shared/schema';

/** One sentence of the quality note under the year's return: a translation key and its values. */
export interface PerformanceNote {
  /** Namespaced key (`reports:…`, `common:…`), the desktop's own texts. */
  key: string;
  values: Record<string, string>;
}

/**
 * The quality notes of the return since 1 January, in the desktop's order and words
 * (`PerformanceCard`): currencies the report could not convert, rates of today standing in for
 * the rates of the day, tickers valued at their trade prices, tickers whose dividends are not
 * loaded for the whole period and tickers with a trade priced far from that day's close. Empty
 * when the number can be taken as it is.
 */
export function performanceNotes(
  data: Pick<
    StockPerformance,
    'fxQuality' | 'unpricedTickers' | 'dividendGapTickers' | 'suspectTradeTickers'
  >,
  currency: string,
  /** How a list of currencies or tickers is joined: the language's (`listText`, B9). */
  list: (items: readonly string[]) => string = (items) => items.join(', ')
): PerformanceNote[] {
  const notes: PerformanceNote[] = [];
  const { missingCurrencies, estimated } = data.fxQuality;
  if (missingCurrencies.length > 0) {
    notes.push({
      key: 'common:fxQuality.missing',
      values: { currency, currencies: list(missingCurrencies) },
    });
  }
  if (estimated) notes.push({ key: 'common:fxQuality.estimated', values: {} });
  const lists: [string, readonly string[]][] = [
    ['unpriced', data.unpricedTickers],
    ['dividendGap', data.dividendGapTickers],
    ['suspectTrade', data.suspectTradeTickers],
  ];
  for (const [name, tickers] of lists) {
    if (tickers.length > 0) {
      notes.push({
        key: `reports:stocksAnalysis.performance.${name}`,
        values: { tickers: list(tickers) },
      });
    }
  }
  return notes;
}
