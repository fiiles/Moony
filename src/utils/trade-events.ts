/**
 * Buy and sell marks for the aggregate trend of the stocks and crypto lists
 * (design system §9: an aggregate line folds a day's trades into one mark).
 * Pure, no DOM and no locale: the pages format the tooltips.
 */

import { utcDayFloor } from './chart-axis';
import type { ChartEvent } from './chart-scale';

/** What a stock or crypto transaction has to offer for its mark. */
export interface TradeLike {
  type: string;
  ticker: string;
  quantity: string;
  pricePerUnit: string;
  /** Currency of `pricePerUnit`; CZK when missing. */
  currency?: string | null;
  /** Unix seconds. */
  transactionDate: number;
}

/** One buy or sell day on the aggregate trend; several tickers on one day share a mark. */
export interface TradeDayEvent extends ChartEvent {
  type: 'buy' | 'sell';
  /** Tickers traded that day in this direction, in the order their first trade appears. */
  tickers: string[];
  /** Total of that day's trades in this direction, in CZK. */
  amountCzk: number;
}

/**
 * One event per UTC day and direction, sorted by day with the buy before the
 * sell. Every trade converts to CZK on its own date (`toCzk` gets that trade's
 * `transactionDate`), so a day's amount is what it was worth then; anything
 * that is not a sell counts as a buy.
 */
export function tradeDayEvents(
  trades: readonly TradeLike[],
  toCzk: (amount: number, currency: string, date: number) => number
): TradeDayEvent[] {
  const byDayAndType = new Map<string, TradeDayEvent>();
  for (const trade of trades) {
    const type = trade.type === 'sell' ? 'sell' : 'buy';
    const day = utcDayFloor(trade.transactionDate);
    const key = `${type}-${day}`;
    const amount = (parseFloat(trade.quantity) || 0) * (parseFloat(trade.pricePerUnit) || 0);
    const amountCzk = toCzk(amount, trade.currency || 'CZK', trade.transactionDate);
    const event = byDayAndType.get(key);
    if (event) {
      event.amountCzk += amountCzk;
      if (!event.tickers.includes(trade.ticker)) event.tickers.push(trade.ticker);
    } else {
      byDayAndType.set(key, { id: key, t: day, type, tickers: [trade.ticker], amountCzk });
    }
  }
  return [...byDayAndType.values()].sort(
    (a, b) => a.t - b.t || (a.type === b.type ? 0 : a.type === 'buy' ? -1 : 1)
  );
}

/** UTC day of the earliest trade: the first day a position chart has anything to show. */
export function firstTradeDay(trades: readonly { transactionDate: number }[]): number | undefined {
  let earliest: number | undefined;
  for (const trade of trades) {
    if (earliest === undefined || trade.transactionDate < earliest) {
      earliest = trade.transactionDate;
    }
  }
  return earliest === undefined ? undefined : utcDayFloor(earliest);
}

/**
 * Up to `max` tickers joined by ", " and the rest counted as "+N", so the tooltip of a day with
 * many trades (an imported history) stays one short line.
 */
export function tickerSummary(tickers: readonly string[], max = 4): string {
  if (tickers.length <= max) return tickers.join(', ');
  return `${tickers.slice(0, max).join(', ')} +${tickers.length - max}`;
}
