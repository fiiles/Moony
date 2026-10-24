import { utcDayFloor } from '@/utils/chart-axis';
import { compareKnown } from '@/utils/positions';

export type PositionSort = 'value' | 'gain' | 'name';

/** Rows of the positions list shown before "Zobrazit všech N". */
export const ROWS_FOLD = 8;

interface Sortable {
  ticker: string;
  /** A stock's name; a coin has `name` instead. */
  companyName?: string;
  name?: string;
  quantity: number;
  marketValue: number | null;
  gainLossPercent: number | null;
}

/** What a row is called: the company name or the coin name, the ticker when there is neither. */
export function displayName(row: { ticker: string; companyName?: string; name?: string }): string {
  return row.companyName || row.name || row.ticker;
}

/** Lower case without diacritics: "Nestlé" is found by "nestle". */
const plain = (text: string) => text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();

/** Whether a position's ticker or name contains the query; an empty query matches everything. */
export function searchMatches(
  row: Pick<Sortable, 'ticker' | 'companyName' | 'name'>,
  query: string
): boolean {
  const needle = plain(query.trim());
  if (needle === '') return true;
  return plain(row.ticker).includes(needle) || plain(displayName(row)).includes(needle);
}

/**
 * The positions list as the page shows it: closed positions (quantity 0) only on request, the
 * search applied, then sorted — by value or return highest first with the unknown ones last, or by
 * name. Equal rows keep their order.
 */
export function positionRows<T extends Sortable>(
  rows: readonly T[],
  opts: { sort: PositionSort; search: string; showClosed: boolean }
): T[] {
  const { sort, search, showClosed } = opts;
  const shown = rows.filter(
    (row) => (showClosed || row.quantity > 0) && searchMatches(row, search)
  );
  switch (sort) {
    case 'value':
      return shown.sort((a, b) => compareKnown(b.marketValue, a.marketValue));
    case 'gain':
      return shown.sort((a, b) => compareKnown(b.gainLossPercent, a.gainLossPercent));
    case 'name':
      return shown.sort((a, b) =>
        displayName(a).localeCompare(displayName(b), undefined, { sensitivity: 'base' })
      );
  }
}

/** The first `fold` rows and how many are left out, until the list is expanded. */
export function foldRows<T>(
  rows: readonly T[],
  expanded: boolean,
  fold = ROWS_FOLD
): { shown: T[]; hidden: number } {
  if (expanded || rows.length <= fold) return { shown: [...rows], hidden: 0 };
  return { shown: rows.slice(0, fold), hidden: rows.length - fold };
}

/**
 * When the prices of the list were fetched, as the sub-line says it: "ceny 9:40" for today,
 * "ceny 12. 10." for an older day, "ceny nenačteny" for never (`null`).
 */
export function priceStamp(
  fetchedAt: number | null,
  nowSec: number
): { kind: 'today'; at: number } | { kind: 'day'; at: number } | { kind: 'never' } {
  if (fetchedAt === null) return { kind: 'never' };
  return { kind: utcDayFloor(fetchedAt) === utcDayFloor(nowSec) ? 'today' : 'day', at: fetchedAt };
}

export type StocksTab = 'positions' | 'watched';

/**
 * What the Akcie page makes of the two lists it can show. `empty`: nothing owned, nothing
 * watched and nothing failed — the page's one empty state. `error`: the positions failed to load
 * and there is nothing watched to show instead. `list`: the segment Pozice · Sledované, opened on
 * `defaultTab` until the user picks one — Sledované when there are no positions but watched ones,
 * so a user who only watches lands on something. Positions that are there win over a failed
 * refetch (the query keeps the last data).
 */
export function stocksView(
  rows: readonly unknown[],
  watched: readonly unknown[],
  positionsFailed: boolean
): { kind: 'empty' | 'error' | 'list'; defaultTab: StocksTab } {
  if (rows.length > 0) return { kind: 'list', defaultTab: 'positions' };
  if (watched.length > 0) return { kind: 'list', defaultTab: 'watched' };
  return { kind: positionsFailed ? 'error' : 'empty', defaultTab: 'positions' };
}
