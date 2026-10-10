/**
 * Pure helpers for the bank-account pages (design system §7 List/Detail and §9):
 * flow summaries of a set of transactions, the balance line reconstructed from
 * the current balance and the transactions, and the movements large enough to
 * mark on it. Amounts are in the account's own currency; the caller converts.
 * The monthly flows window of the detail page and its section rules live here too.
 */

import { convert, type FxQuotes } from '@shared/fx';

export interface FlowTransaction {
  id: string;
  currency?: string;
  status?: string;
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

export function summarizeFlow(
  allTransactions: readonly FlowTransaction[],
  currency?: string
): FlowSummary {
  const transactions = allTransactions.filter(
    (tx) => tx.status !== 'pending' && (!currency || tx.currency === currency)
  );
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
 * Absolute floor for a movement to be drawn on the balance line, in euros.
 * `movementFloor` converts it to the account currency before it reaches
 * `movementThreshold`.
 */
export const MOVEMENT_FLOOR_EUR = 400;

/**
 * The movement floor in the account's currency at the current quotes; 0 when the
 * currency cannot be converted (then only the share of the balance applies).
 */
export function movementFloor(fx: FxQuotes | undefined, accountCurrency: string): number {
  return convert(fx, MOVEMENT_FLOOR_EUR, 'EUR', accountCurrency) ?? 0;
}

/**
 * Threshold for a movement to be drawn on the balance line (README §9):
 * the floor (`movementFloor`) in the account currency, or 10 % of the
 * balance, whichever is larger.
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

const WINDOW_MONTHS = 12;

/** UTC month index of a unix timestamp: `year * 12 + monthIndex`. */
export function monthOf(unixSeconds: number): number {
  const d = new Date(unixSeconds * 1000);
  return d.getUTCFullYear() * 12 + d.getUTCMonth();
}

/** First day (UTC, unix seconds) of a month index. */
export function monthStart(month: number): number {
  return Date.UTC(Math.floor(month / 12), month % 12, 1) / 1000;
}

/** A Date on the first day of the month, for `useFormat().month`. */
export function monthLabelDate(month: number): Date {
  return new Date(monthStart(month) * 1000);
}

export interface MonthFlow {
  month: number;
  income: number;
  expense: number;
  count: number;
}

const booked = (transactions: readonly FlowTransaction[]) =>
  transactions.filter((tx) => tx.status !== 'pending');

/** Credits and debits per month for the 12 months ending with `windowEnd` (booked rows only). */
export function monthlyFlows(
  transactions: readonly FlowTransaction[],
  windowEnd: number
): MonthFlow[] {
  const first = windowEnd - (WINDOW_MONTHS - 1);
  const flows: MonthFlow[] = Array.from({ length: WINDOW_MONTHS }, (_, i) => ({
    month: first + i,
    income: 0,
    expense: 0,
    count: 0,
  }));
  for (const tx of booked(transactions)) {
    const i = monthOf(tx.bookingDate) - first;
    if (i < 0 || i >= WINDOW_MONTHS) continue;
    if (tx.type === 'credit') flows[i].income += num(tx.amount);
    else flows[i].expense += num(tx.amount);
    flows[i].count += 1;
  }
  return flows;
}

export function newestMonthWithRows(transactions: readonly FlowTransaction[]): number | null {
  const rows = booked(transactions);
  return rows.length ? Math.max(...rows.map((tx) => monthOf(tx.bookingDate))) : null;
}

export function oldestMonthWithRows(transactions: readonly FlowTransaction[]): number | null {
  const rows = booked(transactions);
  return rows.length ? Math.min(...rows.map((tx) => monthOf(tx.bookingDate))) : null;
}

export interface FlowsWindow {
  end: number;
  months: number[];
  /** Month whose net the flows card shows; null when the window has no rows. */
  selected: number | null;
  canPrev: boolean;
  canNext: boolean;
}

/**
 * The 12-month window ending with `end`, the month shown in it (an override inside
 * the window, else the newest month with rows inside it) and whether the arrows can
 * move: never past `currentMonth`, never before the oldest row.
 */
export function flowsWindow(
  transactions: readonly FlowTransaction[],
  end: number,
  currentMonth: number,
  override?: number | null
): FlowsWindow {
  const first = end - (WINDOW_MONTHS - 1);
  const months = Array.from({ length: WINDOW_MONTHS }, (_, i) => first + i);
  const inside = booked(transactions).filter((tx) => {
    const m = monthOf(tx.bookingDate);
    return m >= first && m <= end;
  });
  const newest = newestMonthWithRows(inside);
  const selected = override != null && override >= first && override <= end ? override : newest;
  const oldest = oldestMonthWithRows(transactions);
  return {
    end,
    months,
    selected,
    canPrev: oldest !== null && first > oldest,
    canNext: end < currentMonth,
  };
}

/** A booked row in the 12 months ending with `currentMonth`. */
export function hasRecentRows(
  transactions: readonly FlowTransaction[],
  currentMonth: number
): boolean {
  const first = currentMonth - (WINDOW_MONTHS - 1);
  return booked(transactions).some((tx) => monthOf(tx.bookingDate) >= first);
}

export interface DetailSections {
  balance: boolean;
  flows: boolean;
  ledger: boolean;
}

/** Which sections the detail page shows (spec "Section combinations"). */
export function detailSections(args: {
  balance: number;
  hasAnyTransaction: boolean;
  hasRecentRows: boolean;
}): DetailSections {
  return {
    balance: args.balance !== 0,
    flows: args.hasRecentRows,
    ledger: args.hasAnyTransaction,
  };
}

/** The ledger preset that shows `month`: a named one for this and last month, else a custom range. */
export function ledgerPresetForMonth(
  month: number,
  currentMonth: number
): 'thisMonth' | 'lastMonth' | 'custom' {
  if (month === currentMonth) return 'thisMonth';
  if (month === currentMonth - 1) return 'lastMonth';
  return 'custom';
}

/** Last day (UTC, unix seconds, 23:59:59) of a month index. */
export function monthEnd(month: number): number {
  return monthStart(month + 1) - 1;
}
