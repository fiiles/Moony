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
export function nativeBookedTransactions<T extends { currency: string; status: string }>(
  rows: readonly T[],
  currency: string
): T[] {
  return rows.filter((row) => row.status === 'booked' && row.currency === currency);
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
