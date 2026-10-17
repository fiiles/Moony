/**
 * Paths of the phone shell (spec §6): four tab roots, Settings (a sheet from the avatar on
 * Přehled) and the lock screen. Details are pushed under their tab's root (`/finance/…`);
 * Přehled's root is "/", so its details are listed in `OVERVIEW_DETAILS`.
 */
export const routes = {
  overview: '/',
  /** "Co vás čeká" in full, pushed on Přehled. */
  agenda: '/agenda',
  finance: '/finance',
  cashflow: '/cashflow',
  wealth: '/wealth',
  /**
   * Pushed on Majetek: the Akcie class page, a position (`:id` is the position's id), the class
   * placeholders of the next Phase 4 part.
   */
  stocks: '/wealth/stocks',
  positionDetail: '/wealth/stocks/:id',
  cryptoList: '/wealth/crypto',
  bondsList: '/wealth/bonds',
  otherAssetsList: '/wealth/other-assets',
  realEstateList: '/wealth/real-estate',
  propertyDetail: '/wealth/real-estate/:id',
  settings: '/settings',
  auth: '/auth',
} as const;

/** Paths of the details pushed on Majetek, for a `Link` or `setLocation`. */
export const positionPath = (id: string) => `/wealth/stocks/${id}`;
export const propertyPath = (id: string) => `/wealth/real-estate/${id}`;

/** Screens pushed on Přehled (its root "/" cannot prefix them). */
const OVERVIEW_DETAILS: readonly string[] = [routes.agenda];

/** The four tabs, in tab bar order. */
export const TABS = ['overview', 'finance', 'cashflow', 'wealth'] as const;

export type TabId = (typeof TABS)[number];

/**
 * The tab a path belongs to: a tab root or any path under it (a detail pushed on that tab).
 * `null` for paths outside the tabs (lock screen, settings, unknown), which show no tab bar.
 */
export function tabForPath(path: string): TabId | null {
  // Přehled's root is "/", which every path starts with: only the exact path and its listed
  // details belong to it.
  if (
    path === routes.overview ||
    OVERVIEW_DETAILS.some((detail) => path === detail || path.startsWith(`${detail}/`))
  ) {
    return 'overview';
  }
  const tab = TABS.find(
    (id) => id !== 'overview' && (path === routes[id] || path.startsWith(`${routes[id]}/`))
  );
  return tab ?? null;
}
