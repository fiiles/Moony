import type { MenuPreferences } from '@shared/schema';

/** The areas of the app a person can leave out ("Účty a cashflow" is always on). */
export interface TrackedAreas {
  investments: boolean;
  realEstate: boolean;
  liabilities: boolean;
}

/**
 * The three groups from the stored menu preferences. A group is on while any of its flags is on
 * (investments: investments, bonds, crypto; liabilities: loans, insurance); no preferences means
 * everything is tracked.
 */
export function areasFromPreferences(prefs: MenuPreferences | null | undefined): TrackedAreas {
  if (!prefs) return { investments: true, realEstate: true, liabilities: true };
  return {
    investments: prefs.investments || prefs.bonds || (prefs.crypto ?? true),
    realEstate: prefs.realEstate,
    liabilities: prefs.loans || prefs.insurance,
  };
}

/** The menu preferences for the chosen groups; `otherAssets` is not a group and stays as it is (on by default). */
export function preferencesWithAreas(
  current: MenuPreferences | null | undefined,
  areas: TrackedAreas
): MenuPreferences {
  return {
    investments: areas.investments,
    bonds: areas.investments,
    crypto: areas.investments,
    otherAssets: current?.otherAssets ?? true,
    realEstate: areas.realEstate,
    loans: areas.liabilities,
    insurance: areas.liabilities,
  };
}
