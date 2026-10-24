import type { PositionRowBase } from '@/utils/positions';

/** What the sheet needs of a trade, stock or crypto: whose it is, buy or sell, and the day. */
export interface PositionTrade {
  investmentId: string;
  type: string;
  /** The trade's UTC day, unix seconds. */
  transactionDate: number;
}

/**
 * The day of the last sell of each position (`investmentId` → UTC day, unix seconds). A position
 * that was only bought has no entry. Buys are ignored: a position that is closed was last sold,
 * whatever it was bought before.
 */
export function lastSellDays(trades: readonly PositionTrade[]): Map<string, number> {
  const days = new Map<string, number>();
  for (const trade of trades) {
    if (trade.type !== 'sell') continue;
    const known = days.get(trade.investmentId);
    if (known === undefined || trade.transactionDate > known) {
      days.set(trade.investmentId, trade.transactionDate);
    }
  }
  return days;
}

/**
 * The closed positions of a class (nothing held any more) with the day each was closed, the
 * newest close first. A position whose last sell is not known (`null`) goes last; equal days keep
 * the order of `rows`.
 */
export function closedPositions<R extends Pick<PositionRowBase, 'id' | 'quantity'>>(
  rows: readonly R[],
  lastSellDay: (id: string) => number | null
): { row: R; closedAt: number | null }[] {
  return rows
    .filter((row) => row.quantity <= 0)
    .map((row) => ({ row, closedAt: lastSellDay(row.id) }))
    .sort((a, b) => {
      if (a.closedAt === null || b.closedAt === null) {
        return a.closedAt === b.closedAt ? 0 : a.closedAt === null ? 1 : -1;
      }
      return b.closedAt - a.closedAt;
    });
}
