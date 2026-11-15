import type { MoneyOptions } from '@/lib/format';
import type { StatItem } from '@/mobile/components/StatRow';

/**
 * From seven whole digits on, a summary shows the compact amount ("6,78 mil. Kč", "¥1.54億") so a
 * third of a row holds it: one rule for every currency, counted on the digits the whole amount
 * would show (audit 2026-11-15 A11), the one place the phone decides it.
 */
const COMPACT_DIGITS = 7;

/** Whether `value` is written compact in a summary (`COMPACT_DIGITS`). */
export function isCompactAmount(value: number): boolean {
  if (!Number.isFinite(value)) return false;
  return String(Math.round(Math.abs(value))).length >= COMPACT_DIGITS;
}

/**
 * How a stat writes an amount (spec §10.3: a third of the row holds about nine characters): whole
 * under a million, compact with two decimals from there, by magnitude. A signed result spreads
 * `{ signed: true }` over it.
 */
export function statMoneyOptions(value: number): MoneyOptions {
  return isCompactAmount(value) ? { compact: true, decimals: 2 } : { decimals: 0 };
}

/** The tone of a signed result: green above zero, red under it, none at zero or unknown. */
export const statTone = (value: number | null): StatItem['tone'] =>
  value === null || value === 0 ? undefined : value > 0 ? 'gain' : 'loss';
