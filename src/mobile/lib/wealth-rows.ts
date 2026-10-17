import type { AllocationSegment } from '@/utils/allocation-percent';
import { amountOf, compareKnown } from '@/utils/positions';
import { tickerSummary } from '@/utils/trade-events';
import { periodChange } from './change';

/** The classes of the Investice section, in the order the spec lists them (§8.8). */
export const INVESTMENT_CLASSES = ['investments', 'crypto', 'bonds', 'otherAssets'] as const;
export type InvestmentClass = (typeof INVESTMENT_CLASSES)[number];
/** Every class Majetek counts: the investment classes, real estate and the cash on accounts. */
export type WealthClass = InvestmentClass | 'realEstate' | 'savings';
/** The fixed order of the allocation bar and its legend. */
export const ALLOCATION_ORDER: readonly WealthClass[] = [
  'realEstate',
  'investments',
  'bonds',
  'crypto',
  'otherAssets',
  'savings',
];

/**
 * The investment classes the menu preference leaves in, in spec order. `crypto` and
 * `otherAssets` are optional in the stored preference and default to shown.
 */
export function visibleClasses(prefs: {
  investments: boolean;
  bonds: boolean;
  crypto?: boolean;
  otherAssets?: boolean;
}): InvestmentClass[] {
  const shown: Record<InvestmentClass, boolean> = {
    investments: prefs.investments,
    crypto: prefs.crypto ?? true,
    bonds: prefs.bonds,
    otherAssets: prefs.otherAssets ?? true,
  };
  return INVESTMENT_CLASSES.filter((cls) => shown[cls]);
}

/**
 * The tickers of a class row's sub-line: the open positions (quantity above zero) by value, a
 * position without a known value after the valued ones, ties by ticker, at most `max` named and
 * the rest counted ("VWCE, CSPX, AAPL +2").
 */
export function topTickers(
  rows: readonly { ticker: string; quantity: number; value: number | null }[],
  max: number
): string {
  const open = rows
    .filter((row) => row.quantity > 0)
    .sort((a, b) => compareKnown(b.value, a.value) || a.ticker.localeCompare(b.ticker));
  return tickerSummary(
    open.map((row) => row.ticker),
    max
  );
}

/**
 * The change a property's row shows: its last revaluation against the one before it, dated by
 * the last. The ratio is `null` with a single valuation or a zero one before. `null` without any
 * valuation. The input may come in any order.
 */
export function propertyChange(
  valuations: readonly { valuedAt: number; value: string }[]
): { ratio: number | null; day: number } | null {
  const ordered = [...valuations].sort((a, b) => a.valuedAt - b.valuedAt);
  const last = ordered[ordered.length - 1];
  if (!last) return null;
  const previous = ordered[ordered.length - 2];
  return {
    ratio: periodChange(amountOf(last.value), amountOf(previous?.value))?.ratio ?? null,
    day: last.valuedAt,
  };
}

/**
 * The allocation bar's segments: the six classes in the fixed order, a class whose value is
 * unknown (held, nothing converted) or empty left out. The bar counts every class, also one the
 * menu preference hides from the rows, because it shows the hero's total.
 */
export function allocationInput(
  display: Record<WealthClass, number>,
  unknown: Record<WealthClass, boolean>,
  labels: Record<WealthClass, string>
): AllocationSegment[] {
  return ALLOCATION_ORDER.filter((cls) => !unknown[cls] && display[cls] > 0).map((cls) => ({
    key: cls,
    label: labels[cls],
    value: display[cls],
  }));
}

/**
 * What a class row of Investice shows: `unknown` (held, nothing converted: "—"), `empty` (no
 * record of the class at all and no value: a row that leads to adding the first one) or `valued`.
 * Emptiness is by records, not by the open ones: a class whose bonds have all matured or whose
 * coins are all sold keeps its row, which opens the list that still holds them.
 */
export function classRowState(input: {
  records: number;
  value: number;
  unknown: boolean;
}): 'unknown' | 'empty' | 'valued' {
  if (input.unknown) return 'unknown';
  return input.records === 0 && input.value === 0 ? 'empty' : 'valued';
}
