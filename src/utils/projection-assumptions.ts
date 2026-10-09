/**
 * The assumptions behind a projection, as the page shows them (PRJ-02, PRJ-03).
 *
 * Mirrors how the backend reads `projection_settings` (`get_settings_map` in
 * `services/projection.rs`): only enabled rows count, a stored rate of "0" is an
 * explicit 0 %, an empty rate means "not set" and falls back to the class
 * default (the weighted rate calculated from the user's own accounts / bonds for
 * savings and bonds, a built-in constant for the rest).
 */
import type { CalculatedDefaults, ProjectionTimelinePoint } from '@shared/schema';
import { amountOf } from '@/utils/positions';

/** A projected point with numeric amounts (the wire point carries decimal text). */
export type ProjectionPoint = { [K in keyof ProjectionTimelinePoint]: number };

/** The wire point as numbers, in the projection's currency. */
export function toProjectionPoint(point: ProjectionTimelinePoint): ProjectionPoint {
  const n = (value: string) => amountOf(value) ?? 0;
  return {
    date: point.date,
    totalAssets: n(point.totalAssets),
    totalLiabilities: n(point.totalLiabilities),
    netWorth: n(point.netWorth),
    savings: n(point.savings),
    investments: n(point.investments),
    crypto: n(point.crypto),
    bonds: n(point.bonds),
    realEstate: n(point.realEstate),
    otherAssets: n(point.otherAssets),
    loans: n(point.loans),
  };
}

export type ProjectionAssetKey =
  'savings' | 'investments' | 'crypto' | 'bonds' | 'real_estate' | 'other_assets';

export interface AssetClassDef {
  key: ProjectionAssetKey;
  /** Key under `projection.` in the `reports` namespace. */
  labelKey: string;
  hasContribution: boolean;
  /** Field of the timeline point that holds the class's current value. */
  valueField: keyof Pick<
    ProjectionPoint,
    'savings' | 'investments' | 'crypto' | 'bonds' | 'realEstate' | 'otherAssets'
  >;
}

export const ASSET_CLASSES: readonly AssetClassDef[] = [
  { key: 'savings', labelKey: 'categories.savings', hasContribution: true, valueField: 'savings' },
  {
    key: 'investments',
    labelKey: 'categories.investments',
    hasContribution: true,
    valueField: 'investments',
  },
  { key: 'crypto', labelKey: 'categories.crypto', hasContribution: true, valueField: 'crypto' },
  { key: 'bonds', labelKey: 'categories.bonds', hasContribution: true, valueField: 'bonds' },
  {
    key: 'real_estate',
    labelKey: 'categories.realEstate',
    hasContribution: false,
    valueField: 'realEstate',
  },
  {
    key: 'other_assets',
    labelKey: 'categories.otherAssets',
    hasContribution: true,
    valueField: 'otherAssets',
  },
];

/** Built-in yearly growth (percent) when nothing is stored; savings and bonds use the calculated rates instead. */
export const DEFAULT_RATES: Record<ProjectionAssetKey, number> = {
  savings: 0,
  investments: 7,
  crypto: 7,
  bonds: 0,
  real_estate: 3,
  other_assets: 0,
};

/** A stored rate: undefined for empty or unusable text, otherwise the number (0 stays 0). */
export function parseRate(text: string | undefined): number | undefined {
  if (text === undefined || text.trim() === '') return undefined;
  const value = Number(text);
  return Number.isFinite(value) ? value : undefined;
}

/** The rate used when the user stored none: calculated for savings and bonds, a constant otherwise. */
export function defaultRate(
  key: ProjectionAssetKey,
  calculated: CalculatedDefaults | undefined
): number {
  if (key === 'savings') return calculated?.savingsRate ?? DEFAULT_RATES.savings;
  if (key === 'bonds') return calculated?.bondsRate ?? DEFAULT_RATES.bonds;
  return DEFAULT_RATES[key];
}

/** Fraction digits that show a rate without padding zeros: 7 -> 0, 3.5 -> 1, 4.25 -> 2. */
export function rateDigits(rate: number): 0 | 1 | 2 {
  const near = (v: number) => Math.abs(v - Math.round(v)) < 1e-9;
  if (near(rate)) return 0;
  if (near(rate * 10)) return 1;
  return 2;
}

/** The text of a rate field as the backend stores it: '' = not set, otherwise the number. */
export function rateToStore(text: string): string {
  const value = parseRate(text);
  return value === undefined ? '' : String(value);
}

/** A contribution as stored: the amount as typed and the currency it was typed in. */
export interface StoredContribution {
  monthlyContribution: string;
  contributionCurrency: string;
}

/**
 * What to store for a contribution field. An untouched field keeps the stored amount and its
 * currency (a round trip through the display currency would drift by rounding); an edited one
 * stores the typed amount as it is, in `typedCurrency` (the currency the field showed).
 */
export function contributionToStore(
  editedText: string,
  initialText: string,
  stored: { amount: number; currency: string },
  typedCurrency: string
): StoredContribution {
  if (editedText === initialText) {
    return {
      monthlyContribution: String(stored.amount),
      contributionCurrency: stored.currency || typedCurrency,
    };
  }
  const value = Number(editedText);
  const valid = editedText.trim() !== '' && Number.isFinite(value);
  return {
    monthlyContribution: valid ? String(Math.round(value * 100) / 100) : '0',
    contributionCurrency: typedCurrency,
  };
}

/** "a, b and c" in the given locale; falls back to a comma list where Intl.ListFormat is missing. */
export function joinList(items: string[], locale: string): string {
  try {
    return new Intl.ListFormat(locale, { style: 'long', type: 'conjunction' }).format(items);
  } catch {
    return items.join(', ');
  }
}
