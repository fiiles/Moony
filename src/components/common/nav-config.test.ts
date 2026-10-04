import { describe, expect, it } from 'vitest';
import {
  DEFAULT_COLLAPSED_GROUPS,
  NAV_GROUPS,
  OVERVIEW_ITEM,
  SETTINGS_ITEM,
  SIDEBAR_GROUPS_STORAGE_KEY,
  findNavLocation,
  isGroupOpen,
  isNavItemActive,
  menuToggleItems,
  readGroupState,
  visibleItems,
  writeGroupState,
} from './nav-config';

function fakeStorage(initial?: string) {
  const data = new Map<string, string>();
  if (initial !== undefined) data.set(SIDEBAR_GROUPS_STORAGE_KEY, initial);
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

describe('nav tree (design system §5)', () => {
  it('has the four groups in the agreed order', () => {
    expect(NAV_GROUPS.map((g) => g.id)).toEqual(['money', 'investments', 'liabilities', 'tools']);
  });

  it('keeps every existing route path and adds cashflow planning after cashflow', () => {
    const urls = [OVERVIEW_ITEM, ...NAV_GROUPS.flatMap((g) => g.items), SETTINGS_ITEM].map(
      (i) => i.url
    );
    expect(urls).toEqual([
      '/',
      '/bank-accounts',
      '/reports/budgeting',
      '/reports/cashflow',
      '/reports/cashflow-planning',
      '/stocks',
      '/crypto',
      '/bonds',
      '/real-estate',
      '/other-assets',
      '/loans',
      '/insurance',
      '/reports/projection',
      '/reports/stocks-analysis',
      '/stock-monitor',
      '/calculators/annuity',
      '/calculators/estate',
      '/settings',
    ]);
  });

  it('never repeats a url or label key', () => {
    const items = [OVERVIEW_ITEM, ...NAV_GROUPS.flatMap((g) => g.items), SETTINGS_ITEM];
    expect(new Set(items.map((i) => i.url)).size).toBe(items.length);
    expect(new Set(items.map((i) => i.labelKey)).size).toBe(items.length);
  });

  it('uses nav.* label keys everywhere', () => {
    const keys = [
      OVERVIEW_ITEM.labelKey,
      SETTINGS_ITEM.labelKey,
      ...NAV_GROUPS.flatMap((g) => [g.labelKey, ...g.items.map((i) => i.labelKey)]),
    ];
    for (const key of keys) expect(key).toMatch(/^nav\.[a-zA-Z]+$/);
  });
});

describe('isNavItemActive', () => {
  it('matches the overview only on "/"', () => {
    expect(isNavItemActive('/', '/')).toBe(true);
    expect(isNavItemActive('/stocks', '/')).toBe(false);
  });

  it('matches the page and its detail routes', () => {
    expect(isNavItemActive('/bank-accounts', '/bank-accounts')).toBe(true);
    expect(isNavItemActive('/bank-accounts/abc', '/bank-accounts')).toBe(true);
    expect(isNavItemActive('/stocks/abc', '/stocks')).toBe(true);
    expect(isNavItemActive('/stock-monitor/AAPL', '/stock-monitor')).toBe(true);
  });

  it('does not match a sibling that only shares a prefix', () => {
    expect(isNavItemActive('/stock-monitor', '/stocks')).toBe(false);
    expect(isNavItemActive('/stocks-extra', '/stocks')).toBe(false);
    expect(isNavItemActive('/reports/cashflow-planning', '/reports/cashflow')).toBe(false);
  });

  it('keeps Settings active on every /settings* route', () => {
    expect(isNavItemActive('/settings', SETTINGS_ITEM.url)).toBe(true);
    expect(isNavItemActive('/settings/security', SETTINGS_ITEM.url)).toBe(true);
    expect(isNavItemActive('/settings/categorization-rules', SETTINGS_ITEM.url)).toBe(true);
    expect(isNavItemActive('/settingsfoo', SETTINGS_ITEM.url)).toBe(false);
  });
});

describe('findNavLocation (breadcrumb)', () => {
  it('resolves the overview and settings without a group', () => {
    expect(findNavLocation('/')).toEqual({ item: OVERVIEW_ITEM, isDetail: false });
    expect(findNavLocation('/settings')).toEqual({ item: SETTINGS_ITEM, isDetail: false });
    expect(findNavLocation('/settings/security')?.isDetail).toBe(true);
  });

  it('resolves a list page to its group and item', () => {
    const loc = findNavLocation('/stocks');
    expect(loc?.group?.id).toBe('investments');
    expect(loc?.item.id).toBe('stocks');
    expect(loc?.isDetail).toBe(false);
  });

  it('marks detail routes and ignores query strings', () => {
    const loc = findNavLocation('/bank-accounts/abc?tab=1');
    expect(loc?.item.id).toBe('bankAccounts');
    expect(loc?.isDetail).toBe(true);
  });

  it('tells cashflow and cashflow planning apart', () => {
    expect(findNavLocation('/reports/cashflow')?.item.id).toBe('cashflow');
    expect(findNavLocation('/reports/cashflow-planning')?.item.id).toBe('cashflowPlanning');
  });

  it('returns undefined for routes outside the tree', () => {
    expect(findNavLocation('/auth')).toBeUndefined();
    expect(findNavLocation('/nowhere')).toBeUndefined();
  });
});

describe('visibleItems', () => {
  const investments = NAV_GROUPS.find((g) => g.id === 'investments')!;
  const liabilities = NAV_GROUPS.find((g) => g.id === 'liabilities')!;

  it('shows everything when no preferences are loaded', () => {
    expect(visibleItems(investments, undefined)).toHaveLength(investments.items.length);
    expect(visibleItems(investments, null)).toHaveLength(investments.items.length);
  });

  it('hides items switched off in menuPreferences', () => {
    const prefs = { crypto: false, bonds: false };
    expect(visibleItems(investments, prefs).map((i) => i.id)).toEqual([
      'stocks',
      'realEstate',
      'otherAssets',
    ]);
  });

  it('treats missing optional flags as visible', () => {
    expect(visibleItems(investments, { investments: true }).length).toBe(5);
  });

  it('can empty a group', () => {
    expect(visibleItems(liabilities, { loans: false, insurance: false })).toEqual([]);
  });

  it('never hides items without a preference flag', () => {
    const money = NAV_GROUPS.find((g) => g.id === 'money')!;
    const prefs = { loans: false, insurance: false, investments: false };
    expect(visibleItems(money, prefs)).toHaveLength(money.items.length);
  });
});

describe('menuToggleItems', () => {
  it('lists exactly the sidebar items that can be hidden, in sidebar order', () => {
    expect(menuToggleItems().map((i) => i.prefKey)).toEqual([
      'investments',
      'crypto',
      'bonds',
      'realEstate',
      'otherAssets',
      'loans',
      'insurance',
    ]);
  });
});

describe('sidebar group state', () => {
  it('defaults to nothing stored', () => {
    expect(readGroupState(fakeStorage())).toEqual({});
  });

  it('round-trips through storage', () => {
    const storage = fakeStorage();
    writeGroupState({ money: false, tools: true }, storage);
    expect(readGroupState(storage)).toEqual({ money: false, tools: true });
  });

  it('ignores corrupt or foreign values', () => {
    expect(readGroupState(fakeStorage('not json'))).toEqual({});
    expect(readGroupState(fakeStorage('[1,2]'))).toEqual({});
    expect(readGroupState(fakeStorage('{"money":false,"tools":"yes","x":1}'))).toEqual({
      money: false,
    });
  });

  it('survives a storage that throws', () => {
    const broken = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(readGroupState(broken)).toEqual({});
    expect(() => writeGroupState({ money: true }, broken)).not.toThrow();
  });

  it('collapses only Tools by default (design system §5)', () => {
    expect([...DEFAULT_COLLAPSED_GROUPS]).toEqual(['tools']);
    expect(isGroupOpen({}, 'money', false)).toBe(true);
    expect(isGroupOpen({}, 'tools', false)).toBe(false);
  });

  it('respects the stored choice over the default', () => {
    expect(isGroupOpen({ tools: true }, 'tools', false)).toBe(true);
    expect(isGroupOpen({ money: false }, 'money', false)).toBe(false);
  });

  it('always opens the group that holds the active page', () => {
    expect(isGroupOpen({}, 'tools', true)).toBe(true);
    expect(isGroupOpen({ money: false }, 'money', true)).toBe(true);
  });
});
