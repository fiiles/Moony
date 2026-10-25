import type { FlowKind, ReportTransaction } from '@shared/schema';
import { isMovedOut } from '@/utils/report-drilldown';
import { counterpartyLabel, transactionDetails } from '@/utils/transaction-text';

/** Rows shown before "Zobrazit dalších 100" (the desktop dialog pages by the same hundred). */
export const DRILLDOWN_PAGE = 100;

/** One payment of the category drilldown, ready for its row. */
export interface DrilldownRow {
  tx: ReportTransaction;
  /** Who the money went to or came from; without a counterparty what it was for, else the account. */
  title: string;
  /** The day, the account and, under a counterparty, the first detail of the payment. */
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
 * `uncategorized`). The title is the counterparty (name, else account number), then the payment's
 * description, then the account's name; the sub-line adds the first detail only while the title is
 * the counterparty, so the same words never stand twice. `dayLabel` words a booking date.
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
    const counterparty = counterpartyLabel(tx).title;
    const details = transactionDetails(tx);
    const categoryId = changes.has(tx.id) ? (changes.get(tx.id) ?? null) : tx.categoryId;
    const credit = tx.txType === 'credit';
    const flow = flows?.current.get(tx.id);
    return {
      tx,
      title: counterparty ?? details[0] ?? tx.bankAccountName,
      sub: [
        dayLabel(tx.bookingDate),
        tx.bankAccountName,
        ...(counterparty !== null ? details.slice(0, 1) : []),
      ],
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
