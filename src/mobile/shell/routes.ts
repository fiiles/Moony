import type { FlowKind } from '@shared/schema';

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
  /** One category of the period, pushed on Cashflow: `/cashflow/category/<flow>/<key>`. */
  cashflowCategory: '/cashflow/category/:flow/:key',
  /** The budgets of the period, pushed on Cashflow. */
  budgets: '/cashflow/budgets',
  /**
   * The old path of the plans list: it redirects to the plans view of the Cashflow root
   * (`plansPath`). Its detail still lives under it.
   */
  plans: '/cashflow/plans',
  /** One spending plan (`:id` is the plan's id). */
  planDetail: '/cashflow/plans/:id',
  wealth: '/wealth',
  /**
   * Pushed on Majetek: the class pages (Akcie, Krypto, Dluhopisy, Ostatní aktiva) and their
   * details; a property is pushed straight from Majetek's list (no class page of its own).
   */
  stocks: '/wealth/stocks',
  positionDetail: '/wealth/stocks/:id',
  cryptoList: '/wealth/crypto',
  /** A coin of Krypto (`:id` is the position's id). */
  cryptoDetail: '/wealth/crypto/:id',
  bondsList: '/wealth/bonds',
  otherAssetsList: '/wealth/other-assets',
  /** An other asset (`:id` is the asset's id). */
  otherAssetDetail: '/wealth/other-assets/:id',
  /** A watched ticker of Akcie, followed or not (`:ticker` is the Yahoo symbol, escaped by `watchPath`). */
  watchDetail: '/wealth/watch/:ticker',
  propertyDetail: '/wealth/real-estate/:id',
  settings: '/settings',
  /** A section page pushed on Nastavení (`:section` is one of `SETTINGS_SECTIONS`). */
  settingsSection: '/settings/:section',
  auth: '/auth',
} as const;

/** Paths of the details pushed on Finance, for a `Link` or `setLocation`. */
export const accountPath = (id: string) => `/finance/accounts/${id}`;
export const loanPath = (id: string) => `/finance/loans/${id}`;
export const policyPath = (id: string) => `/finance/policies/${id}`;

/**
 * The plans view of the Cashflow root (`?view=plans`); without a year it opens on the current one.
 * It carries no period: the Cashflow tab keeps its own while the plans are shown.
 */
export const plansPath = (year?: number) =>
  year === undefined
    ? `${routes.cashflow}?view=plans`
    : `${routes.cashflow}?view=plans&year=${year}`;
export const planPath = (id: string) => `${routes.plans}/${id}`;

/** Paths of the details pushed on Majetek, for a `Link` or `setLocation`. */
export const positionPath = (id: string) => `/wealth/stocks/${id}`;
export const cryptoPath = (id: string) => `/wealth/crypto/${id}`;
export const otherAssetPath = (id: string) => `/wealth/other-assets/${id}`;
/** A symbol can hold characters a path cannot (`^GSPC`, `BRK/B`), so it is escaped. */
export const watchPath = (ticker: string) => `/wealth/watch/${encodeURIComponent(ticker)}`;
export const propertyPath = (id: string) => `/wealth/real-estate/${id}`;

/** The pushed pages of Nastavení, in root order. */
export const SETTINGS_SECTIONS = [
  'profile',
  'tracking',
  'security',
  'reminders',
  'notifications',
  'backup',
  'sync',
  'categories',
  'categorization',
  'import',
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
 * The path of a category's transactions in the selected period. `key` is a category id or
 * `uncategorized` (neither needs escaping); `search` is the period search of the Cashflow tab
 * (`cashflowSearch`), so the way back keeps the period.
 */
export function cashflowCategoryPath(flow: FlowKind, key: string, search = ''): string {
  return `/cashflow/category/${flow}/${key}${search}`;
}

/** The path of the budgets screen; `search` is the period search of the Cashflow tab. */
export function budgetsPath(search = ''): string {
  return `${routes.budgets}${search}`;
}

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

/**
 * Whether `value` is a path of the shell a screen can be opened at: a tab, a screen pushed on one,
 * Nastavení or one of its sections. Not the lock, an unknown page or anything with an origin. A
 * notification's target is checked with it before the app goes there.
 */
export function isShellPath(value: unknown): value is string {
  if (typeof value !== 'string' || !value.startsWith('/') || value.startsWith('//')) return false;
  const cut = value.search(/[?#]/);
  const path = cut === -1 ? value : value.slice(0, cut);
  if (path === routes.settings) return true;
  if (path.startsWith(`${routes.settings}/`)) {
    return isSettingsSection(path.slice(routes.settings.length + 1));
  }
  return tabForPath(path) !== null;
}
