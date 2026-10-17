import { describe, expect, it } from 'vitest';
import { accountPath, loanPath, policyPath, routes, tabForPath } from './routes';

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
