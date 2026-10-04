/**
 * The sidebar's navigation tree as data (design system README §5).
 *
 * One source for the sidebar, the top-bar breadcrumb *and* the Settings →
 * menu-visibility toggles, so the labels of the three can never diverge
 *. Label keys live in the `common` namespace under `nav.*`.
 */
import {
  BarChart3,
  Bitcoin,
  Building,
  Calculator,
  ChartCandlestick,
  CreditCard,
  FileText,
  Gem,
  Home as HomeIcon,
  Landmark,
  LayoutDashboard,
  LineChart,
  PiggyBank,
  Repeat,
  Settings,
  Shield,
  Tag,
  TrendingUp,
  type LucideIcon,
} from 'lucide-react';
import type { MenuPreferences } from '@shared/schema';

/** `menuPreferences` flags that can hide a sidebar item. */
export type MenuPrefKey = keyof MenuPreferences;

export interface NavItem {
  id: string;
  /** Key in the `common` namespace, e.g. `nav.bankAccounts`. */
  labelKey: string;
  url: string;
  icon: LucideIcon;
  /** Set when the user can hide the item in Settings → Menu visibility. */
  prefKey?: MenuPrefKey;
}

export interface NavGroup {
  id: string;
  labelKey: string;
  items: NavItem[];
}

export const OVERVIEW_ITEM: NavItem = {
  id: 'overview',
  labelKey: 'nav.overview',
  url: '/',
  icon: LayoutDashboard,
};

export const NAV_GROUPS: NavGroup[] = [
  {
    id: 'money',
    labelKey: 'nav.groupMoney',
    items: [
      { id: 'bankAccounts', labelKey: 'nav.bankAccounts', url: '/bank-accounts', icon: Landmark },
      { id: 'budgets', labelKey: 'nav.budgets', url: '/reports/budgeting', icon: PiggyBank },
      { id: 'cashflow', labelKey: 'nav.cashflow', url: '/reports/cashflow', icon: BarChart3 },
      {
        id: 'cashflowPlanning',
        labelKey: 'nav.cashflowPlanning',
        url: '/reports/cashflow-planning',
        icon: Repeat,
      },
    ],
  },
  {
    id: 'investments',
    labelKey: 'nav.groupInvestments',
    items: [
      {
        id: 'stocks',
        labelKey: 'nav.stocks',
        url: '/stocks',
        icon: TrendingUp,
        prefKey: 'investments',
      },
      { id: 'crypto', labelKey: 'nav.crypto', url: '/crypto', icon: Bitcoin, prefKey: 'crypto' },
      { id: 'bonds', labelKey: 'nav.bonds', url: '/bonds', icon: FileText, prefKey: 'bonds' },
      {
        id: 'realEstate',
        labelKey: 'nav.realEstate',
        url: '/real-estate',
        icon: HomeIcon,
        prefKey: 'realEstate',
      },
      {
        id: 'otherAssets',
        labelKey: 'nav.otherAssets',
        url: '/other-assets',
        icon: Gem,
        prefKey: 'otherAssets',
      },
    ],
  },
  {
    id: 'liabilities',
    labelKey: 'nav.groupLiabilities',
    items: [
      { id: 'loans', labelKey: 'nav.loans', url: '/loans', icon: CreditCard, prefKey: 'loans' },
      {
        id: 'insurance',
        labelKey: 'nav.insurance',
        url: '/insurance',
        icon: Shield,
        prefKey: 'insurance',
      },
    ],
  },
  {
    id: 'tools',
    labelKey: 'nav.groupTools',
    items: [
      { id: 'projection', labelKey: 'nav.projection', url: '/reports/projection', icon: LineChart },
      {
        id: 'stocksAnalysis',
        labelKey: 'nav.stocksAnalysis',
        url: '/reports/stocks-analysis',
        icon: Tag,
      },
      {
        id: 'stockMonitor',
        labelKey: 'nav.stockMonitor',
        url: '/stock-monitor',
        icon: ChartCandlestick,
      },
      {
        id: 'annuityCalculator',
        labelKey: 'nav.annuityCalculator',
        url: '/calculators/annuity',
        icon: Calculator,
      },
      {
        id: 'estateCalculator',
        labelKey: 'nav.estateCalculator',
        url: '/calculators/estate',
        icon: Building,
      },
    ],
  },
];

/** Standalone item at the bottom of the sidebar content; active for every `/settings*` route. */
export const SETTINGS_ITEM: NavItem = {
  id: 'settings',
  labelKey: 'nav.settings',
  url: '/settings',
  icon: Settings,
};

/** True for the item's own page and everything below it (detail pages, sub-pages). */
export function isNavItemActive(location: string, url: string): boolean {
  if (url === '/') return location === '/';
  return location === url || location.startsWith(`${url}/`);
}

export function isItemVisible(
  item: NavItem,
  prefs: Partial<MenuPreferences> | null | undefined
): boolean {
  if (!item.prefKey) return true;
  return prefs?.[item.prefKey] ?? true;
}

export function visibleItems(
  group: NavGroup,
  prefs: Partial<MenuPreferences> | null | undefined
): NavItem[] {
  return group.items.filter((item) => isItemVisible(item, prefs));
}

/** Items that have a visibility toggle in Settings, in sidebar order. */
export function menuToggleItems(): (NavItem & { prefKey: MenuPrefKey })[] {
  return NAV_GROUPS.flatMap((g) => g.items).filter(
    (item): item is NavItem & { prefKey: MenuPrefKey } => item.prefKey !== undefined
  );
}

// ---------------------------------------------------------------------------
// Where a location sits in the tree (top-bar breadcrumb)
// ---------------------------------------------------------------------------

export interface NavLocation {
  /** The group the active item belongs to; absent for Overview and Settings. */
  group?: NavGroup;
  item: NavItem;
  /** True when the location is below the item's own page (a detail or sub-page). */
  isDetail: boolean;
}

/** Resolves a location to its sidebar item; `undefined` for routes the sidebar does not know. */
export function findNavLocation(location: string): NavLocation | undefined {
  const path = location.split(/[?#]/)[0];
  if (isNavItemActive(path, OVERVIEW_ITEM.url)) {
    return { item: OVERVIEW_ITEM, isDetail: false };
  }
  if (isNavItemActive(path, SETTINGS_ITEM.url)) {
    return { item: SETTINGS_ITEM, isDetail: path !== SETTINGS_ITEM.url };
  }
  for (const group of NAV_GROUPS) {
    for (const item of group.items) {
      if (isNavItemActive(path, item.url)) {
        return { group, item, isDetail: path !== item.url };
      }
    }
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// Persisted open/closed state of the collapsible groups
// ---------------------------------------------------------------------------

export const SIDEBAR_GROUPS_STORAGE_KEY = 'moony-sidebar-groups';

/**
 * Groups closed until the user opens them (README §5: Tools is collapsed by
 * default so the 17 items fit a 900 px window). A group that contains the
 * active page always opens.
 */
export const DEFAULT_COLLAPSED_GROUPS: ReadonlySet<string> = new Set(['tools']);

/** Open state of a group: stored choice, else the default, overridden by an active item inside. */
export function isGroupOpen(
  state: Record<string, boolean>,
  groupId: string,
  containsActive: boolean
): boolean {
  if (containsActive) return true;
  return state[groupId] ?? !DEFAULT_COLLAPSED_GROUPS.has(groupId);
}

type ReadableStorage = Pick<Storage, 'getItem'>;
type WritableStorage = Pick<Storage, 'setItem'>;

function defaultStorage(): Storage | undefined {
  try {
    return typeof localStorage === 'undefined' ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

/** `{ [groupId]: open }`; groups that were never toggled are absent (= default). */
export function readGroupState(
  storage: ReadableStorage | undefined = defaultStorage()
): Record<string, boolean> {
  try {
    const raw = storage?.getItem(SIDEBAR_GROUPS_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return {};
    const state: Record<string, boolean> = {};
    for (const [id, open] of Object.entries(parsed)) {
      if (typeof open === 'boolean') state[id] = open;
    }
    return state;
  } catch {
    return {};
  }
}

export function writeGroupState(
  state: Record<string, boolean>,
  storage: WritableStorage | undefined = defaultStorage()
): void {
  try {
    storage?.setItem(SIDEBAR_GROUPS_STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Storage can be blocked or full; the sidebar then just forgets its state.
  }
}
