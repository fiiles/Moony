/**
 * The account detail's ledger (spec §8.4): its filter state, the backend filters it asks for, the
 * day groups it shows and the paging of its infinite scroll. Pure; days are UTC day starts in unix
 * seconds (ADR 0008).
 */
import type { TransactionFilters } from '@shared/schema';
import { monthEnd, monthStart } from '@/utils/bank-activity';
import { utcDayFloor } from '@/utils/chart-axis';
import { getUtcPeriodRange, isoDateFromUtcTimestamp, utcDayEnd, utcDayStart } from '@/utils/period';

const DAY = 86_400;

/** The segment: every movement, credits only, debits only. */
export type LedgerKind = 'all' | 'in' | 'out';

/** The filter sheet's periods (`bank_accounts:filters.*`). */
export type PeriodPreset = 'thisMonth' | 'lastMonth' | 'thisYear' | 'allTime' | 'custom';

export interface LedgerState {
  kind: LedgerKind;
  search: string;
  /** A category id, `'uncategorized'`, or `null` for every category. */
  categoryId: string | null;
  preset: PeriodPreset;
  /** The custom range, ISO `YYYY-MM-DD` (either end may be open). */
  from: string | null;
  to: string | null;
  /** A month tapped in the flows strip (`year * 12 + monthIndex`); it wins over the preset. */
  month: number | null;
}

/** Every movement, newest first: the ledger as it opens. */
export const DEFAULT_LEDGER: LedgerState = {
  kind: 'all',
  search: '',
  categoryId: null,
  preset: 'allTime',
  from: null,
  to: null,
  month: null,
};

const TX_TYPES = { in: 'credit', out: 'debit' } as const;

/** The date range of a state: the flows month, else the preset (`now` picks this and last month). */
function dateRange(state: LedgerState, nowSec: number): { from?: number; to?: number } {
  if (state.month !== null) return { from: monthStart(state.month), to: monthEnd(state.month) };
  const now = new Date(nowSec * 1000);
  switch (state.preset) {
    case 'thisMonth':
    case 'lastMonth': {
      const range = getUtcPeriodRange('monthly', state.preset === 'thisMonth' ? 0 : -1, now);
      return { from: range.start, to: range.end };
    }
    case 'thisYear': {
      const range = getUtcPeriodRange('yearly', 0, now);
      return { from: range.start, to: range.end };
    }
    case 'custom':
      return {
        from: state.from ? utcDayStart(state.from) : undefined,
        to: state.to ? utcDayEnd(state.to) : undefined,
      };
    case 'allTime':
      return {};
  }
}

/** The backend filters of a state, newest first (paging is added by the caller). */
export function ledgerFilters(state: LedgerState, nowSec: number): TransactionFilters {
  const filters: TransactionFilters = { sortBy: 'date', ascending: false };
  if (state.kind !== 'all') filters.txType = TX_TYPES[state.kind];
  if (state.categoryId !== null) filters.categoryId = state.categoryId;
  const search = state.search.trim();
  if (search) filters.search = search;
  const { from, to } = dateRange(state, nowSec);
  if (from !== undefined) filters.dateFrom = from;
  if (to !== undefined) filters.dateTo = to;
  return filters;
}

/** The custom range that covers a flows month, as the filter sheet's two date fields hold it. */
export function periodForMonth(month: number): { from: string; to: string } {
  return {
    from: isoDateFromUtcTimestamp(monthStart(month)),
    to: isoDateFromUtcTimestamp(monthEnd(month)),
  };
}

/**
 * Rows grouped by their UTC day, in the order the days first appear (the ledger is newest first);
 * a day split across two loaded pages is one group.
 */
export function groupByDay<T extends { bookingDate: number }>(
  rows: readonly T[]
): { day: number; rows: T[] }[] {
  const groups = new Map<number, T[]>();
  for (const row of rows) {
    const day = utcDayFloor(row.bookingDate);
    const group = groups.get(day);
    if (group) group.push(row);
    else groups.set(day, [row]);
  }
  return [...groups].map(([day, dayRows]) => ({ day, rows: dayRows }));
}

/** A day head: today and yesterday by name, other days as a date with the year when it is not this one. */
export function dayLabel(
  day: number,
  todayDay: number
): { key: 'today' | 'yesterday' } | { date: number; withYear: boolean } {
  if (day === todayDay) return { key: 'today' };
  if (day === todayDay - DAY) return { key: 'yesterday' };
  const year = (t: number) => new Date(t * 1000).getUTCFullYear();
  return { date: day, withYear: year(day) !== year(todayDay) };
}

/** Offset of the next ledger page: the number of rows loaded, while the filter has more. */
export function nextOffset(loaded: number, total: number): number | undefined {
  return loaded < total ? loaded : undefined;
}

/** Filters the sheet holds (a category, a period) for the dot on its button; the segment and the search are on screen. */
export function activeFilterCount(state: LedgerState): number {
  const period =
    state.month !== null ||
    (state.preset === 'custom'
      ? state.from !== null || state.to !== null
      : state.preset !== 'allTime');
  return (state.categoryId !== null ? 1 : 0) + (period ? 1 : 0);
}
