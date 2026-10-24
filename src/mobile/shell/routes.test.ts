import { describe, expect, it } from 'vitest';
import {
  SETTINGS_SECTIONS,
  accountPath,
  budgetsPath,
  cashflowCategoryPath,
  cryptoPath,
  isSettingsSection,
  loanPath,
  otherAssetPath,
  policyPath,
  positionPath,
  propertyPath,
  routes,
  settingsPath,
  tabForPath,
  watchPath,
} from './routes';

describe('tabForPath', () => {
  it('maps each tab root to its tab', () => {
    expect(tabForPath(routes.overview)).toBe('overview');
    expect(tabForPath(routes.finance)).toBe('finance');
    expect(tabForPath(routes.cashflow)).toBe('cashflow');
    expect(tabForPath(routes.wealth)).toBe('wealth');
  });

  it('keeps a detail pushed on a tab in that tab', () => {
    expect(tabForPath('/finance/accounts/7f3a')).toBe('finance');
    expect(tabForPath('/wealth/stocks')).toBe('wealth');
    expect(tabForPath('/cashflow/')).toBe('cashflow');
    expect(tabForPath(accountPath('7f3a'))).toBe('finance');
    expect(tabForPath(loanPath('7f3a'))).toBe('finance');
    expect(tabForPath(policyPath('7f3a'))).toBe('finance');
    expect(tabForPath(routes.uncategorized)).toBe('finance');
    expect(tabForPath('/cashflow/budgets')).toBe('cashflow');
    expect(tabForPath('/cashflow/category/expense/cat_x')).toBe('cashflow');
  });

  it('keeps the screens pushed on Majetek in Majetek', () => {
    expect(tabForPath(routes.stocks)).toBe('wealth');
    expect(tabForPath(positionPath('7f3a'))).toBe('wealth');
    expect(tabForPath(propertyPath('x'))).toBe('wealth');
    expect(tabForPath(routes.cryptoList)).toBe('wealth');
    expect(tabForPath(cryptoPath('c1'))).toBe('wealth');
    expect(routes.cryptoDetail.replace(':id', 'c1')).toBe(cryptoPath('c1'));
    expect(tabForPath(routes.otherAssetsList)).toBe('wealth');
    expect(tabForPath(otherAssetPath('a1'))).toBe('wealth');
    expect(routes.otherAssetDetail.replace(':id', 'a1')).toBe(otherAssetPath('a1'));
    expect(tabForPath(watchPath('AAPL'))).toBe('wealth');
  });

  it('builds the watch detail path from the ticker, escaped', () => {
    expect(routes.watchDetail).toBe('/wealth/watch/:ticker');
    expect(watchPath('AAPL')).toBe('/wealth/watch/AAPL');
    expect(routes.watchDetail.replace(':ticker', 'AAPL')).toBe(watchPath('AAPL'));
    // An index or a share class is one path segment, not a path
    expect(watchPath('^GSPC')).toBe('/wealth/watch/%5EGSPC');
    expect(watchPath('BRK/B')).toBe('/wealth/watch/BRK%2FB');
    expect(tabForPath(watchPath('BRK/B'))).toBe('wealth');
  });

  it('keeps a screen pushed on Přehled in Přehled', () => {
    expect(tabForPath(routes.agenda)).toBe('overview');
    expect(tabForPath('/agendas')).toBeNull();
  });

  it('matches whole path segments only', () => {
    expect(tabForPath('/financial')).toBeNull();
    expect(tabForPath('/wealthy/x')).toBeNull();
  });

  it('has no tab for the lock screen, settings or an unknown path', () => {
    expect(tabForPath(routes.auth)).toBeNull();
    expect(tabForPath(routes.settings)).toBeNull();
    expect(tabForPath('/unknown')).toBeNull();
    expect(tabForPath('')).toBeNull();
  });
});

describe('cashflow paths', () => {
  it('builds the category detail path from the flow, the key and the period search', () => {
    expect(cashflowCategoryPath('expense', 'cat_x', '?tf=yearly')).toBe(
      '/cashflow/category/expense/cat_x?tf=yearly'
    );
    expect(cashflowCategoryPath('income', 'uncategorized')).toBe(
      '/cashflow/category/income/uncategorized'
    );
  });

  it('builds the budgets path with or without the period search', () => {
    expect(budgetsPath()).toBe('/cashflow/budgets');
    expect(budgetsPath('?o=-1')).toBe('/cashflow/budgets?o=-1');
  });

  it('keeps the route patterns in step with the helpers', () => {
    expect(routes.cashflowCategory).toBe('/cashflow/category/:flow/:key');
    expect(routes.budgets).toBe('/cashflow/budgets');
  });
});

describe('settings sections', () => {
  it('keeps a section page outside the tabs', () => {
    expect(tabForPath('/settings/security')).toBeNull();
    expect(tabForPath(settingsPath('backup'))).toBeNull();
  });

  it('builds the path of each section from the route', () => {
    expect(settingsPath('backup')).toBe('/settings/backup');
    for (const section of SETTINGS_SECTIONS) {
      expect(settingsPath(section)).toBe(routes.settingsSection.replace(':section', section));
    }
  });

  it('knows its sections and nothing else', () => {
    for (const section of SETTINGS_SECTIONS) expect(isSettingsSection(section)).toBe(true);
    expect(isSettingsSection('appearance')).toBe(false);
    expect(isSettingsSection('')).toBe(false);
    expect(isSettingsSection('Security')).toBe(false);
  });
});
