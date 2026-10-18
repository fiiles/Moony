import type { InvestmentTransaction, TickerValueHistoryReport } from '@shared/schema';
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

/** A buy or a sell as a mark on the line; the transaction stays at hand for the callout. */
export interface PositionEvent extends ChartEvent {
  type: 'buy' | 'sell';
  tx: InvestmentTransaction;
}

export type TxFilter = 'all' | 'buy' | 'sell';

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
export function positionEvents(transactions: readonly InvestmentTransaction[]): PositionEvent[] {
  return transactions.map((tx) => ({
    id: tx.id,
    t: tx.transactionDate,
    type: tx.type === 'sell' ? 'sell' : 'buy',
    tx,
  }));
}

/** The ledger as the page lists it: newest first, all of it or only the buys or the sells. */
export function filterTransactions(
  transactions: readonly InvestmentTransaction[],
  filter: TxFilter
): InvestmentTransaction[] {
  const rows = [...transactions].sort((a, b) => b.transactionDate - a.transactionDate);
  return filter === 'all' ? rows : rows.filter((tx) => tx.type === filter);
}

/** What a trade moved in its own currency: quantity × price per unit; 0 for a figure that is not a number. */
export function nativeTotal(tx: Pick<InvestmentTransaction, 'quantity' | 'pricePerUnit'>): number {
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
export function revealRow(
  transactions: readonly InvestmentTransaction[],
  filter: TxFilter,
  id: string,
  fold = ROWS_FOLD
): { filter: TxFilter; expand: boolean } {
  const indexIn = (by: TxFilter) =>
    filterTransactions(transactions, by).findIndex((tx) => tx.id === id);
  const next = indexIn(filter) >= 0 || indexIn('all') < 0 ? filter : 'all';
  return { filter: next, expand: indexIn(next) >= fold };
}
