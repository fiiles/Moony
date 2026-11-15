/**
 * How the phone writes money (design system README §13, "Money and currencies"): a record's amount
 * in its own currency, the converted amount under it behind "≈" when that currency is not the main
 * one, summaries whole and a single payment exact. The wording is `useApprox()`'s; this decides.
 */
import { currencyDecimals, isoMinorUnits } from '@shared/currencies';
import type { MoneyOptions } from '@/lib/format';

/** A summary (hero, stat, section number, category row, limit, axis): no decimals. */
export const WHOLE: MoneyOptions = { decimals: 0 };
/** A summary with its sign ("+ 86 420 Kč"). */
export const SIGNED_WHOLE: MoneyOptions = { signed: true, decimals: 0 };

/**
 * A single payment or trade, exactly: the currency's ISO 4217 minor units ("− 129,90 Kč",
 * "− 2,06 €"), and none on an amount that is whole at them ("− 500 Kč", "− 80 €").
 */
export function exactMoneyOptions(amount: number, currency: string): { decimals: number } {
  const units = isoMinorUnits(currency);
  const scale = 10 ** units;
  const minor = Math.round(Math.abs(Number.isFinite(amount) ? amount : 0) * scale);
  return { decimals: minor % scale === 0 ? 0 : units };
}

/**
 * The "≈" amount: whole, as every summary is; under one unit it keeps the currency's own decimals
 * ("≈ $0.21"), so a small payment does not read "≈ $0".
 */
export function approxMoneyOptions(value: number, currency: string): { decimals: number } {
  const decimals = currencyDecimals(currency);
  const scale = 10 ** decimals;
  const rounded = Math.round(Math.abs(value) * scale) / scale;
  return rounded >= 1 || decimals === 0 ? { decimals: 0 } : { decimals };
}

/** What stands under a native amount: the converted amount, "no rate", or nothing. */
export type Approx = { kind: 'none' } | { kind: 'approx'; value: number } | { kind: 'noRate' };

/**
 * The line under a record's native amount. Nothing for a record in the main currency, for a zero
 * amount, while the rates load (`pending`) and for a converted amount that rounds to nothing;
 * "no rate" when the currency cannot be converted (`converted` is null); else the converted amount.
 */
export function approxOf(args: {
  amount: number;
  currency: string;
  mainCurrency: string;
  /** `amount` in the main currency; null without a rate (or while the rates load). */
  converted: number | null;
  pending?: boolean;
}): Approx {
  const { amount, currency, mainCurrency, converted, pending = false } = args;
  if (currency === mainCurrency || amount === 0) return { kind: 'none' };
  if (converted === null) return pending ? { kind: 'none' } : { kind: 'noRate' };
  const scale = 10 ** approxMoneyOptions(converted, mainCurrency).decimals;
  if (Math.round(Math.abs(converted) * scale) === 0) return { kind: 'none' };
  return { kind: 'approx', value: converted };
}
