/**
 * Pure helpers for the bank-account pages (design system §7 List/Detail and §9):
 * flow summaries of a set of transactions, the balance line reconstructed from
 * the current balance and the transactions, and the movements large enough to
 * mark on it. Amounts are in the account's own currency; the caller converts.
 */

export interface FlowTransaction {
  id: string;
  type: 'credit' | 'debit';
  /** Positive amount as stored. */
  amount: string | number;
  /** Unix seconds (UTC calendar day). */
  bookingDate: number;
  categoryId?: string | null;
  counterpartyName?: string | null;
  description?: string | null;
}

export interface FlowSummary {
  income: number;
  expense: number;
  /** income − expense */
  net: number;
  count: number;
  incomeCount: number;
  expenseCount: number;
  uncategorized: number;
  /** Category with the largest expense total, with its share of all expenses. */
  topExpenseCategory: { id: string; amount: number; share: number } | null;
}

const num = (v: string | number) => Math.abs(Number(v) || 0);

export function summarizeFlow(transactions: readonly FlowTransaction[]): FlowSummary {
  let income = 0;
  let expense = 0;
  let incomeCount = 0;
  let expenseCount = 0;
  let uncategorized = 0;
  const byCategory = new Map<string, number>();
  for (const tx of transactions) {
    const amount = num(tx.amount);
    if (tx.type === 'credit') {
      income += amount;
      incomeCount++;
    } else {
      expense += amount;
      expenseCount++;
      if (tx.categoryId)
        byCategory.set(tx.categoryId, (byCategory.get(tx.categoryId) ?? 0) + amount);
    }
    if (!tx.categoryId) uncategorized++;
  }
  let top: FlowSummary['topExpenseCategory'] = null;
  for (const [id, amount] of byCategory) {
    if (!top || amount > top.amount)
      top = { id, amount, share: expense > 0 ? amount / expense : 0 };
  }
  return {
    income,
    expense,
    net: income - expense,
    count: transactions.length,
    incomeCount,
    expenseCount,
    uncategorized,
    topExpenseCategory: top,
  };
}

export interface BalancePoint {
  t: number;
  value: number;
}

const DAY = 86400;

/**
 * Daily balance from `fromDay` to `toDay` (UTC day starts, inclusive), walking
 * back from the current balance through the transactions of that span. Days
 * before the first known transaction keep the balance the account had then.
 * Transactions newer than `toDay` are undone first so the line ends on the
 * balance of `toDay`.
 */
export function reconstructBalance(
  currentBalance: number,
  transactions: readonly FlowTransaction[],
  fromDay: number,
  toDay: number
): BalancePoint[] {
  if (toDay < fromDay) return [];
  const signed = (tx: FlowTransaction) => (tx.type === 'credit' ? 1 : -1) * num(tx.amount);
  // Net movement per UTC day
  const perDay = new Map<number, number>();
  let afterEnd = 0;
  for (const tx of transactions) {
    const day = Math.floor(tx.bookingDate / DAY) * DAY;
    if (day > toDay) afterEnd += signed(tx);
    else perDay.set(day, (perDay.get(day) ?? 0) + signed(tx));
  }
  let balance = currentBalance - afterEnd;
  const points: BalancePoint[] = [];
  for (let day = toDay; day >= fromDay; day -= DAY) {
    points.push({ t: day, value: balance });
    balance -= perDay.get(day) ?? 0;
  }
  return points.reverse();
}

export interface MovementEvent {
  id: string;
  t: number;
  type: 'income' | 'expense';
  tx: FlowTransaction;
}

/**
 * Absolute floor for a movement to be drawn on the balance line, in the base
 * currency (CZK, roughly 400 EUR). The caller converts it to the account
 * currency before passing it to `movementThreshold`.
 */
export const MOVEMENT_FLOOR_BASE = 10_000;

/**
 * Threshold for a movement to be drawn on the balance line (README §9):
 * the base-currency floor (MOVEMENT_FLOOR_BASE) in the account currency, or
 * 10 % of the balance, whichever is larger.
 */
export function movementThreshold(balance: number, floorInCurrency: number): number {
  return Math.max(floorInCurrency, Math.abs(balance) * 0.1);
}

export function significantMovements(
  transactions: readonly FlowTransaction[],
  threshold: number
): MovementEvent[] {
  return transactions
    .filter((tx) => num(tx.amount) >= threshold)
    .map((tx) => ({
      id: tx.id,
      t: tx.bookingDate,
      type: tx.type === 'credit' ? ('income' as const) : ('expense' as const),
      tx,
    }))
    .sort((a, b) => a.t - b.t);
}
