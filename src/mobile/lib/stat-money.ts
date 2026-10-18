import type { MoneyOptions } from '@/lib/format';

/** From a million on, a stat shows the compact amount ("6,78 mil. Kč") so a third holds it. */
const COMPACT_FROM = 1_000_000;

/**
 * How a stat writes an amount (spec §10.3: a third of the row holds about nine characters): whole
 * under a million, compact with two decimals from there, by magnitude. A signed result spreads
 * `{ signed: true }` over it.
 */
export function statMoneyOptions(value: number): MoneyOptions {
  return Math.abs(value) >= COMPACT_FROM ? { compact: true, decimals: 2 } : { decimals: 0 };
}
