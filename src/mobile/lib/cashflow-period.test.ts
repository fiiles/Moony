import { describe, expect, it } from 'vitest';
import {
  cashflowSearch,
  DEFAULT_CASHFLOW_STATE,
  isPartialPeriod,
  parseCashflowSearch,
  selectMonth,
  type CashflowState,
} from './cashflow-period';

describe('parseCashflowSearch', () => {
  it('reads nothing as the default state', () => {
    expect(parseCashflowSearch('')).toEqual(DEFAULT_CASHFLOW_STATE);
    expect(parseCashflowSearch('?')).toEqual(DEFAULT_CASHFLOW_STATE);
  });

  it('reads the timeframe, the offset and the accounts', () => {
    expect(parseCashflowSearch('?tf=quarterly&o=-2&acc=a,b')).toEqual({
      timeframe: 'quarterly',
      offset: -2,
      accountIds: ['a', 'b'],
    });
  });

  it('reads the search wouter hands over, without its leading question mark', () => {
    expect(parseCashflowSearch('tf=yearly&o=-1')).toEqual({
      timeframe: 'yearly',
      offset: -1,
      accountIds: undefined,
    });
  });

  it('falls back to the default for every field it cannot read', () => {
    expect(parseCashflowSearch('?tf=weekly&o=3&acc=')).toEqual(DEFAULT_CASHFLOW_STATE);
    expect(parseCashflowSearch('?o=abc').offset).toBe(0);
    expect(parseCashflowSearch('?o=-1.5').offset).toBe(0);
    expect(parseCashflowSearch('?o=-0').offset).toBe(0);
    expect(parseCashflowSearch('?tf=yearly&o=oops')).toEqual({
      timeframe: 'yearly',
      offset: 0,
      accountIds: undefined,
    });
  });

  it('never looks forward in time', () => {
    expect(parseCashflowSearch('?o=1').offset).toBe(0);
  });

  it('drops empty account ids', () => {
    expect(parseCashflowSearch('?acc=a,,b,').accountIds).toEqual(['a', 'b']);
    expect(parseCashflowSearch('?acc=,').accountIds).toBeUndefined();
  });
});

describe('cashflowSearch', () => {
  it('is empty for the default state', () => {
    expect(cashflowSearch(DEFAULT_CASHFLOW_STATE)).toBe('');
  });

  it('carries only the fields that differ from the default', () => {
    expect(cashflowSearch({ timeframe: 'yearly', offset: 0, accountIds: undefined })).toBe(
      '?tf=yearly'
    );
    expect(cashflowSearch({ timeframe: 'monthly', offset: -3, accountIds: undefined })).toBe(
      '?o=-3'
    );
  });

  it('survives a round trip through parseCashflowSearch', () => {
    const states: CashflowState[] = [
      { timeframe: 'yearly', offset: -1, accountIds: ['x'] },
      { timeframe: 'quarterly', offset: 0, accountIds: ['a', 'b', 'c'] },
      { timeframe: 'monthly', offset: -12, accountIds: undefined },
      DEFAULT_CASHFLOW_STATE,
    ];
    for (const state of states) {
      expect(parseCashflowSearch(cashflowSearch(state))).toEqual(state);
    }
  });

  it('leaves an empty account list out', () => {
    expect(cashflowSearch({ timeframe: 'monthly', offset: 0, accountIds: [] })).toBe('');
  });
});

describe('selectMonth', () => {
  it('switches to the monthly view of the month, counted back from now', () => {
    const now = new Date('2026-10-17T10:00:00Z');
    expect(selectMonth(Date.UTC(2026, 8, 1) / 1000, now)).toEqual({
      timeframe: 'monthly',
      offset: -1,
    });
    expect(selectMonth(Date.UTC(2026, 9, 1) / 1000, now)).toEqual({
      timeframe: 'monthly',
      offset: 0,
    });
    expect(selectMonth(Date.UTC(2025, 9, 1) / 1000, now)).toEqual({
      timeframe: 'monthly',
      offset: -12,
    });
  });
});

describe('isPartialPeriod', () => {
  const now = Date.UTC(2026, 9, 17) / 1000;

  it('is true while the last second of the period is still ahead', () => {
    expect(isPartialPeriod(Date.UTC(2026, 9, 31, 23, 59, 59) / 1000, now)).toBe(true);
  });

  it('is false for a period that is over', () => {
    expect(isPartialPeriod(Date.UTC(2026, 8, 30, 23, 59, 59) / 1000, now)).toBe(false);
  });
});
