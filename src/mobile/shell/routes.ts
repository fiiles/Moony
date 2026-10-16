/**
 * Paths of the phone shell (spec §6): four tab roots, Settings (a sheet from the avatar on
 * Přehled) and the lock screen. Details are pushed under their tab's root (`/finance/…`).
 */
export const routes = {
  overview: '/',
  finance: '/finance',
  cashflow: '/cashflow',
  wealth: '/wealth',
  settings: '/settings',
  auth: '/auth',
} as const;

/** The four tabs, in tab bar order. */
export const TABS = ['overview', 'finance', 'cashflow', 'wealth'] as const;

export type TabId = (typeof TABS)[number];

/**
 * The tab a path belongs to: a tab root or any path under it (a detail pushed on that tab).
 * `null` for paths outside the tabs (lock screen, settings, unknown), which show no tab bar.
 */
export function tabForPath(path: string): TabId | null {
  // Přehled's root is "/", which every path starts with: only the exact path belongs to it.
  if (path === routes.overview) return 'overview';
  const tab = TABS.find(
    (id) => id !== 'overview' && (path === routes[id] || path.startsWith(`${routes[id]}/`))
  );
  return tab ?? null;
}
