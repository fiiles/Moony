import { currencyDecimals, type CurrencyCode } from '@shared/currencies';

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
