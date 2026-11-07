import type { StockDividend, TickerValueHistoryReport } from '@shared/schema';
import type { LinePoint } from '@/components/charts/MoonyLineChart';
import { utcDayFloor } from '@/utils/chart-axis';
import type { ChartEvent } from '@/utils/chart-scale';
import { chartPeriodStart } from '@/utils/period';
import { amountOf } from '@/utils/positions';
import { ROWS_FOLD } from './position-rows';

/** A day of the position's line: its value and, when known, the cost basis ("Vloženo") of that day. */
export interface CostPoint extends LinePoint {
  cost?: number;
}

/**
 * What a position's ledger needs of a trade, stock or crypto: a stock's `InvestmentTransaction`
 * and a coin's `CryptoTransaction` both have it (they differ in `companyName` and `name`).
 */
export interface LedgerTx {
  id: string;
  type: string;
  ticker: string;
  quantity: string;
  pricePerUnit: string;
  currency: string;
  transactionDate: number;
}

/** A buy or a sell as a mark on the line; the transaction stays at hand for the callout. */
export interface PositionEvent<T extends LedgerTx = LedgerTx> extends ChartEvent {
  type: 'buy' | 'sell';
  tx: T;
}

export type LedgerFilter = 'all' | 'buy' | 'sell' | 'dividend';
/** The filter a coin's ledger takes (no dividends); kept for the crypto code. */
export type TxFilter = LedgerFilter;

/** A row of the ledger: a trade or, for a stock, a received dividend (dated by its ex-date). */
export type LedgerEntry<T extends LedgerTx = LedgerTx> =
  | { kind: 'trade'; id: string; date: number; tx: T }
  | { kind: 'dividend'; id: string; date: number; dividend: StockDividend };

/**
 * The line of a position (`PositionHero`'s `points` as a pure function): the recorded days with
 * their value and cost basis, a day without a value left out, and today's figures as the last
 * point — the day's own when the history already has it, a new one otherwise. A report in another
 * currency than `reportCurrency` (the previous one, while a currency change settles) must not
 * meet today's value, so it gives no days. An unknown `currentValue` leaves today out: the line
 * then ends at the last recorded day.
 */
export function positionPoints(
  report: Pick<TickerValueHistoryReport, 'currency' | 'points'> | undefined,
  reportCurrency: string,
  currentValue: number | null,
  currentCost: number | null,
  nowSec: number
): CostPoint[] {
  const days = report !== undefined && report.currency === reportCurrency ? report.points : [];
  const points = days.flatMap((day) => {
    const value = amountOf(day.value);
    return value === null
      ? []
      : [{ t: day.recordedAt, value, cost: amountOf(day.costBasis) ?? undefined }];
  });
  if (currentValue === null) return points;
  const today: CostPoint = {
    t: Math.floor(nowSec),
    value: currentValue,
    cost: currentCost ?? undefined,
  };
  const last = points[points.length - 1];
  if (last !== undefined && utcDayFloor(last.t) === utcDayFloor(nowSec)) {
    return [...points.slice(0, -1), { ...today, t: last.t }];
  }
  return [...points, today];
}

/** Every trade as a mark on its day: ▲ for a buy, ▽ for a sell. */
export function positionEvents<T extends LedgerTx>(transactions: readonly T[]): PositionEvent<T>[] {
  return transactions.map((tx) => ({
    id: tx.id,
    t: tx.transactionDate,
    type: tx.type === 'sell' ? 'sell' : 'buy',
    tx,
  }));
}

/** The ledger as the page lists it: newest first, all of it or only the buys or the sells. */
export function filterTransactions<T extends Pick<LedgerTx, 'type' | 'transactionDate'>>(
  transactions: readonly T[],
  filter: Exclude<LedgerFilter, 'dividend'>
): T[] {
  const rows = [...transactions].sort((a, b) => b.transactionDate - a.transactionDate);
  return filter === 'all' ? rows : rows.filter((tx) => tx.type === filter);
}

/**
 * The ledger as the page lists it: the trades and the received dividends merged, newest first.
 * Stable for the same day (trades before dividends, each in its input order, as the desktop does).
 * Buys and sells drop the dividends, the dividend filter drops the trades.
 */
export function ledgerEntries<T extends LedgerTx>(
  transactions: readonly T[],
  dividends: readonly StockDividend[],
  filter: LedgerFilter
): LedgerEntry<T>[] {
  const trades: LedgerEntry<T>[] =
    filter === 'dividend'
      ? []
      : transactions
          .filter((tx) => filter === 'all' || tx.type === filter)
          .map((tx) => ({ kind: 'trade', id: tx.id, date: tx.transactionDate, tx }));
  const paid: LedgerEntry<T>[] =
    filter === 'all' || filter === 'dividend'
      ? dividends.map((dividend) => ({
          kind: 'dividend',
          id: dividend.id,
          date: dividend.exDate,
          dividend,
        }))
      : [];
  return [...trades, ...paid].sort((a, b) => b.date - a.date);
}

/** What a trade moved in its own currency: quantity × price per unit; 0 for a figure that is not a number. */
export function nativeTotal(tx: Pick<LedgerTx, 'quantity' | 'pricePerUnit'>): number {
  return (parseFloat(tx.quantity) || 0) * (parseFloat(tx.pricePerUnit) || 0);
}

/** The return of "this year": from 1 January (UTC) to today's UTC day, the performance report's range. */
export function ytdRange(nowSec: number): { fromTs: number; toTs: number } {
  return { fromTs: chartPeriodStart('YTD', nowSec)!, toTs: utcDayFloor(nowSec) };
}

/**
 * What a tap on a mark must change for its row to be in view: the filter that holds the row (all of
 * them when the current one leaves it out) and whether the list must unfold because the fold hides
 * it. A row that is not in the ledger at all changes nothing.
 */
export function revealRow<T extends LedgerTx>(
  transactions: readonly T[],
  dividends: readonly StockDividend[],
  filter: LedgerFilter,
  id: string,
  fold = ROWS_FOLD
): { filter: LedgerFilter; expand: boolean } {
  const indexIn = (by: LedgerFilter) =>
    ledgerEntries(transactions, dividends, by).findIndex((entry) => entry.id === id);
  const next = indexIn(filter) >= 0 || indexIn('all') < 0 ? filter : 'all';
  return { filter: next, expand: indexIn(next) >= fold };
}
