import { describe, expect, it } from 'vitest';
import {
  activeFilterCount,
  dayLabel,
  groupByDay,
  ledgerFilters,
  nextOffset,
  periodForMonth,
  type LedgerState,
} from './ledger';
const DAY = 86_400;
const TODAY = Date.UTC(2026, 9, 16) / 1000;
const state: LedgerState = {
  kind: 'all',
  search: '',
  categoryId: null,
  preset: 'allTime',
  from: null,
  to: null,
  month: null,
};

describe('ledgerFilters', () => {
  it('is newest first, by date, unfiltered for the defaults', () => {
    expect(ledgerFilters(state, TODAY)).toEqual({ sortBy: 'date', ascending: false });
  });
  it('maps the kind, the category, the search and this month', () => {
    const f = ledgerFilters(
      { ...state, kind: 'in', categoryId: 'uncategorized', search: ' lidl ', preset: 'thisMonth' },
      TODAY
    );
    expect(f).toMatchObject({
      txType: 'credit',
      categoryId: 'uncategorized',
      search: 'lidl',
      dateFrom: Date.UTC(2026, 9, 1) / 1000,
      dateTo: Date.UTC(2026, 10, 1) / 1000 - 1,
    });
  });
  it('prefers a selected flows month over the preset', () => {
    const f = ledgerFilters({ ...state, preset: 'thisMonth', month: 2026 * 12 + 7 }, TODAY);
    expect(f.dateFrom).toBe(Date.UTC(2026, 7, 1) / 1000);
    expect(f.dateTo).toBe(Date.UTC(2026, 8, 1) / 1000 - 1);
  });
  it('reads a custom ISO range', () => {
    const f = ledgerFilters(
      { ...state, preset: 'custom', from: '2026-01-05', to: '2026-01-07' },
      TODAY
    );
    expect(f.dateFrom).toBe(Date.UTC(2026, 0, 5) / 1000);
    expect(f.dateTo).toBe(Date.UTC(2026, 0, 8) / 1000 - 1);
  });
});

describe('periodForMonth', () => {
  it('spans the UTC month', () => {
    expect(periodForMonth(2026 * 12 + 1)).toEqual({ from: '2026-02-01', to: '2026-02-28' });
  });
});

describe('groupByDay', () => {
  it('keeps the input order and merges rows of one day across pages', () => {
    const rows = [
      { id: 'a', bookingDate: TODAY },
      { id: 'b', bookingDate: TODAY - DAY },
      { id: 'c', bookingDate: TODAY - DAY },
    ];
    expect(groupByDay(rows)).toEqual([
      { day: TODAY, rows: [rows[0]] },
      { day: TODAY - DAY, rows: [rows[1], rows[2]] },
    ]);
  });
});

describe('dayLabel', () => {
  it('names today and yesterday, dates the rest with the year only when it differs', () => {
    expect(dayLabel(TODAY, TODAY)).toEqual({ key: 'today' });
    expect(dayLabel(TODAY - DAY, TODAY)).toEqual({ key: 'yesterday' });
    expect(dayLabel(TODAY - 2 * DAY, TODAY)).toEqual({ date: TODAY - 2 * DAY, withYear: false });
    expect(dayLabel(Date.UTC(2025, 11, 31) / 1000, TODAY)).toEqual({
      date: Date.UTC(2025, 11, 31) / 1000,
      withYear: true,
    });
  });
});

describe('nextOffset', () => {
  it('is the loaded count while rows remain', () => {
    expect(nextOffset(100, 250)).toBe(100);
    expect(nextOffset(250, 250)).toBeUndefined();
    expect(nextOffset(0, 0)).toBeUndefined();
  });
});

describe('activeFilterCount', () => {
  it('counts a category and a period, not the kind or the search', () => {
    expect(activeFilterCount(state)).toBe(0);
    expect(
      activeFilterCount({
        ...state,
        kind: 'out',
        search: 'x',
        categoryId: 'c1',
        preset: 'lastMonth',
      })
    ).toBe(2);
  });
});
