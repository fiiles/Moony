import type { FlowKind, ReportTransaction } from '@shared/schema';
import { isMovedOut } from '@/utils/report-drilldown';
import { transactionSubFacts, transactionTitle } from './transaction-title';

/** Rows shown before "Zobrazit dalších 100" (the desktop dialog pages by the same hundred). */
export const DRILLDOWN_PAGE = 100;

/** One payment of the category drilldown, ready for its row. */
export interface DrilldownRow {
  tx: ReportTransaction;
  /** `transactionTitle` (counterparty, description, account number), else the account's name. */
  title: string;
  /** The day and the account (`transactionSubFacts`; the category is the page). */
  sub: string[];
  credit: boolean;
  /**
   * The payment's worth in the report currency, with the row's own sign (a debit is negative, a
   * credit positive) — `signedAmount` is its contribution to the report line, so in an expense
   * list a debit is positive there. `null` without a conversion (`signedAmount` is `null`).
   */
  reportAmount: number | null;
  /**
   * Left the list here: changed to a category that is not the list's, or given a meaning that is
   * not the list's flow. The row stays, dimmed.
   */
  moved: boolean;
  /** The category the row has now: the change made here, else the one it was loaded with. */
  categoryId: string | null;
}

/**
 * The rows of a category's drilldown. The list is the snapshot the page opened with; `changes`
 * holds what was recategorized since (`isMovedOut`: against `targetKey`, a category id or
 * `uncategorized`). The title is every payment row's (`transactionTitle`: the named counterparty,
 * the description, the account number), then the account's name; the sub-line is the day and the
 * account, the bank's note being the transaction sheet's (README §13, "Transactions", C4). `dayLabel` words a booking date.
 *
 * `flows` is the other way a payment leaves the list: `current` holds the effective flow the
 * transaction sheet read for the rows it has been opened on, and a row whose flow there is not
 * the list's `flowKind` (a meaning chosen in the sheet, or the category's role) is `moved` too, a
 * row that is in `current` with the list's own flow or not in it at all is not. Without `flows`
 * only the category rule applies.
 */
export function drilldownRows(
  rows: readonly ReportTransaction[],
  changes: ReadonlyMap<string, string | null>,
  targetKey: string,
  dayLabel: (bookingDate: number) => string,
  flows?: { flowKind: FlowKind; current: ReadonlyMap<string, FlowKind> }
): DrilldownRow[] {
  return rows.map((tx) => {
    const categoryId = changes.has(tx.id) ? (changes.get(tx.id) ?? null) : tx.categoryId;
    const credit = tx.txType === 'credit';
    const flow = flows?.current.get(tx.id);
    return {
      tx,
      title: transactionTitle(tx) ?? tx.bankAccountName,
      sub: transactionSubFacts({
        day: dayLabel(tx.bookingDate),
        account: tx.bankAccountName,
        category: null,
      }),
      credit,
      reportAmount:
        tx.signedAmount === null ? null : Math.abs(Number(tx.signedAmount)) * (credit ? 1 : -1),
      moved:
        (changes.has(tx.id) && isMovedOut(targetKey, categoryId)) ||
        (flow !== undefined && flow !== flows?.flowKind),
      categoryId,
    };
  });
}

/** Months of the category trend on the drilldown (audit 2026-11-15 D19). */
export const TREND_MONTHS = 6;

/**
 * The UTC month starts of the trend: `count` months, oldest first, the last one holding
 * `periodEnd` (a month, the last month of a quarter or a year).
 */
export function trendMonths(periodEnd: number, count: number): number[] {
  const end = new Date(periodEnd * 1000);
  const year = end.getUTCFullYear();
  const month = end.getUTCMonth();
  return Array.from({ length: count }, (_, i) => Date.UTC(year, month - (count - 1 - i), 1) / 1000);
}

/**
 * What a category came to in each month of `months` (UTC month starts, oldest first): the sum of
 * the rows' report amounts (`signedAmount`: an expense debit-positive, refunds lowering it, every
 * other role credit-positive). A row without a conversion, or outside the months, is left out.
 */
export function monthlyTotals(
  rows: readonly Pick<ReportTransaction, 'bookingDate' | 'signedAmount'>[],
  months: readonly number[]
): number[] {
  const totals = months.map(() => 0);
  for (const row of rows) {
    if (row.signedAmount === null) continue;
    const amount = Number(row.signedAmount);
    if (!Number.isFinite(amount)) continue;
    const date = new Date(row.bookingDate * 1000);
    const start = Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), 1) / 1000;
    const index = months.indexOf(start);
    if (index >= 0) totals[index] += amount;
  }
  return totals.map((total) => Math.round(total * 100) / 100);
}
