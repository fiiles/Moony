import { currencyDecimals, type CurrencyCode } from '@shared/currencies';
import type { EffectiveTransactionFlow, FlowKind } from '@shared/schema';

export function preserveMoneyText(
  original: string | null,
  originalCurrency: string | null,
  value: number,
  currency: string
): string {
  if (!Number.isFinite(value)) throw new Error('validation.amountInvalid');
  if (original != null && originalCurrency === currency && Number(original) === value)
    return original;
  return value.toFixed(currencyDecimals(currency as CurrencyCode));
}
/** Converts an amount from `from` into the account currency at the rates of `timestamp`'s day. */
export type AccountConvertAt = (amount: number, from: string, timestamp: number) => number | null;

export interface AccountCurrencyRows<T> {
  /** Booked rows with `amount` and `currency` in the account currency. */
  rows: T[];
  /** Currencies whose rows were converted, sorted. */
  converted: string[];
  /** Currencies whose rows had no rate for their day and were left out, sorted. */
  missing: string[];
  /** Number of rows left out for lack of a rate. */
  missingCount: number;
}

/**
 * Booked rows expressed in the account currency, for the account's totals and balance
 * line: rows in the account currency pass through, rows in another currency (a pocket of
 * a multi-currency account) are converted at the rate of their booking day, and rows
 * that cannot be converted are left out and their currency reported. Pending rows are
 * never counted.
 */
export function bookedInAccountCurrency<
  T extends { currency: string; status: string; amount: string | number; bookingDate: number },
>(rows: readonly T[], currency: string, convertAt: AccountConvertAt): AccountCurrencyRows<T> {
  const converted = new Set<string>();
  const missing = new Set<string>();
  const result: T[] = [];
  let missingCount = 0;
  for (const row of rows) {
    if (row.status !== 'booked') continue;
    if (row.currency === currency) {
      result.push(row);
      continue;
    }
    const value = convertAt(Math.abs(Number(row.amount) || 0), row.currency, row.bookingDate);
    if (value === null) {
      missing.add(row.currency);
      missingCount++;
      continue;
    }
    converted.add(row.currency);
    result.push({ ...row, amount: String(value), currency });
  }
  return {
    rows: result,
    converted: [...converted].sort(),
    missing: [...missing].sort(),
    missingCount,
  };
}
export function absoluteMovementShares(amounts: readonly number[]): number[] {
  const total = amounts.reduce((sum, value) => sum + Math.abs(value), 0);
  return amounts.map((value) => (total ? Math.abs(value) / total : 0));
}

/**
 * The financial meaning worth printing on a ledger row, or null when the row
 * already says it: the sign covers an outgoing expense or incoming income, and
 * a category-derived meaning is visible in the category column. An expense
 * coming back reads "refund"; an explicit choice and an own-account transfer
 * without a category are named.
 */
export function rowFlowLabel(
  flow: Pick<EffectiveTransactionFlow, 'effectiveFlowKind' | 'flowSource'>,
  isCredit: boolean,
  hasCategory: boolean
): FlowKind | 'refund' | null {
  const kind = flow.effectiveFlowKind;
  if (kind === 'expense' && isCredit) return 'refund';
  if (flow.flowSource === 'explicit') return kind;
  if (flow.flowSource === 'ownAccount' && !hasCategory) return kind;
  return null;
}
