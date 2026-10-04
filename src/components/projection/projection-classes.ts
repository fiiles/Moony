import type { ProjectionSettings, ProjectionTimelinePoint } from '@shared/schema';
import { defaultRate, parseRate } from '@/utils/projection-assumptions';

export type ProjectionClassKey = ProjectionSettings['assetType'];

export interface ProjectionClass {
  key: Exclude<ProjectionClassKey, 'loans'>;
  /** Key under `projection.categories` in the reports namespace. */
  labelKey: string;
  /** Field of a timeline point. */
  field: keyof Pick<
    ProjectionTimelinePoint,
    'savings' | 'investments' | 'crypto' | 'bonds' | 'realEstate' | 'otherAssets'
  >;
  /** CSS token of the series color. */
  color: string;
  hasContribution: boolean;
  /** Backend default when no setting row exists (savings and bonds derive theirs). */
  defaultRate: number;
}

/** The six projected asset classes, in table order (prototype `projection.html`). */
export const PROJECTION_CLASSES: readonly ProjectionClass[] = [
  {
    key: 'savings',
    labelKey: 'savings',
    field: 'savings',
    color: 'var(--s3)',
    hasContribution: true,
    defaultRate: 0,
  },
  {
    key: 'investments',
    labelKey: 'investments',
    field: 'investments',
    color: 'var(--s1)',
    hasContribution: true,
    defaultRate: 7,
  },
  {
    key: 'crypto',
    labelKey: 'crypto',
    field: 'crypto',
    color: 'var(--s4)',
    hasContribution: true,
    defaultRate: 7,
  },
  {
    key: 'bonds',
    labelKey: 'bonds',
    field: 'bonds',
    color: 'var(--s4)',
    hasContribution: true,
    defaultRate: 0,
  },
  {
    key: 'real_estate',
    labelKey: 'realEstate',
    field: 'realEstate',
    color: 'var(--s2)',
    hasContribution: false,
    defaultRate: 3,
  },
  {
    key: 'other_assets',
    labelKey: 'otherAssets',
    field: 'otherAssets',
    color: 'var(--s-other)',
    hasContribution: true,
    defaultRate: 0,
  },
];

/** Resolved parameters of one class, as the backend applies them. */
export interface ClassParams {
  cls: ProjectionClass;
  /** Annual rate in percent after the savings/bonds fallback to the derived rate. */
  rate: number;
  /** Monthly contribution in CZK (settings store CZK). */
  contribution: number;
  enabled: boolean;
  /** True while the rate is a default (the weighted account/bond rate, or the class constant), not a stored one. */
  derived: boolean;
}

/**
 * Mirrors `get_settings_map` in the backend: a stored rate text wins
 * (an explicit 0 counts), an empty one means the class default — the weighted
 * rate of the holdings for savings and bonds, a constant otherwise. A disabled
 * row keeps its values but marks the class as frozen.
 */
export function resolveClassParams(
  settings: readonly ProjectionSettings[],
  derivedRates: { savingsRate: number; bondsRate: number }
): ClassParams[] {
  return PROJECTION_CLASSES.map((cls) => {
    const saved = settings.find((s) => s.assetType === cls.key);
    const stored = parseRate(saved?.yearlyGrowthRate);
    return {
      cls,
      rate: stored ?? defaultRate(cls.key, derivedRates),
      contribution: saved && cls.hasContribution ? parseFloat(saved.monthlyContribution) || 0 : 0,
      enabled: saved ? saved.enabled : true,
      derived: stored === undefined,
    };
  });
}
