import type { ReportTransaction } from '@shared/schema';
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
  /** Changed here to a category that is not the list's: the row stays, dimmed. */
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
 */
export function drilldownRows(
  rows: readonly ReportTransaction[],
  changes: ReadonlyMap<string, string | null>,
  targetKey: string,
  dayLabel: (bookingDate: number) => string
): DrilldownRow[] {
  return rows.map((tx) => {
    const counterparty = counterpartyLabel(tx).title;
    const details = transactionDetails(tx);
    const categoryId = changes.has(tx.id) ? (changes.get(tx.id) ?? null) : tx.categoryId;
    return {
      tx,
      title: counterparty ?? details[0] ?? tx.bankAccountName,
      sub: [
        dayLabel(tx.bookingDate),
        tx.bankAccountName,
        ...(counterparty !== null ? details.slice(0, 1) : []),
      ],
      credit: tx.txType === 'credit',
      moved: changes.has(tx.id) && isMovedOut(targetKey, categoryId),
      categoryId,
    };
  });
}
