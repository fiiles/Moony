/**
 * Paths of the phone shell (spec §6): four tab roots, Settings (from the avatar on Přehled, its
 * sections pushed under it) and the lock screen. Details are pushed under their tab's root
 * (`/finance/…`); Přehled's root is "/", so its details are listed in `OVERVIEW_DETAILS`.
 */
export const routes = {
  overview: '/',
  /** "Co vás čeká" in full, pushed on Přehled. */
  agenda: '/agenda',
  finance: '/finance',
  /** Pushed on Finance: an account, a loan and a policy (the `:id` is the record's id), and the categorize queue. */
  accountDetail: '/finance/accounts/:id',
  loanDetail: '/finance/loans/:id',
  policyDetail: '/finance/policies/:id',
  uncategorized: '/finance/uncategorized',
  cashflow: '/cashflow',
  wealth: '/wealth',
  settings: '/settings',
  /** A section page pushed on Nastavení (`:section` is one of `SETTINGS_SECTIONS`). */
  settingsSection: '/settings/:section',
  auth: '/auth',
} as const;

/** Paths of the details pushed on Finance, for a `Link` or `setLocation`. */
export const accountPath = (id: string) => `/finance/accounts/${id}`;
export const loanPath = (id: string) => `/finance/loans/${id}`;
export const policyPath = (id: string) => `/finance/policies/${id}`;

/** The pushed pages of Nastavení, in root order. */
export const SETTINGS_SECTIONS = [
  'profile',
  'security',
  'reminders',
  'notifications',
  'backup',
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

export const settingsPath = (section: SettingsSection) => `/settings/${section}`;

export function isSettingsSection(value: string): value is SettingsSection {
  return (SETTINGS_SECTIONS as readonly string[]).includes(value);
}

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
