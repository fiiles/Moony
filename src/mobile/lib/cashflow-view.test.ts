import { describe, expect, it } from 'vitest';
import { cashflowViewSearch, viewFromSearch } from './cashflow-view';
import { cashflowSearch, parseCashflowSearch } from './cashflow-period';

describe('viewFromSearch', () => {
  it('reads the plans view and nothing else', () => {
    expect(viewFromSearch('?view=plans&year=2026')).toBe('plans');
    expect(viewFromSearch('')).toBeUndefined();
    expect(viewFromSearch('?tf=yearly')).toBeUndefined();
    expect(viewFromSearch('?view=other')).toBeUndefined();
  });
});

describe('cashflowViewSearch', () => {
  it('opens the plans view with a year', () => {
    expect(cashflowViewSearch('', 'plans', 2026)).toBe('?view=plans&year=2026');
  });

  it('opens the plans view on the current year when it has none', () => {
    expect(cashflowViewSearch('', 'plans')).toBe('?view=plans');
  });

  it('keeps the plans year when no year is given', () => {
    expect(cashflowViewSearch('?view=plans&year=2025', 'plans')).toBe('?view=plans&year=2025');
  });

  it('keeps the period going to the plans and back', () => {
    const period = cashflowSearch({ timeframe: 'yearly', offset: -1, accountIds: ['a', 'b'] });
    const plans = cashflowViewSearch(period, 'plans', 2026);
    expect(viewFromSearch(plans)).toBe('plans');
    expect(parseCashflowSearch(plans)).toEqual(parseCashflowSearch(period));
    const back = cashflowViewSearch(plans, 'report');
    expect(back).toBe(period);
  });

  it('leaves an empty search for the report of the default period', () => {
    expect(cashflowViewSearch('?view=plans&year=2026', 'report')).toBe('');
  });
});

describe('the period parsing', () => {
  it('ignores view and year', () => {
    expect(parseCashflowSearch('?view=plans&year=2026&tf=quarterly')).toEqual({
      timeframe: 'quarterly',
      offset: 0,
      accountIds: undefined,
    });
  });
});
