import { describe, expect, it } from 'vitest';
import {
  displayName,
  foldRows,
  positionRows,
  priceStamp,
  searchMatches,
  stocksView,
  ROWS_FOLD,
} from './position-rows';

const T = Date.UTC(2026, 9, 16, 9, 40) / 1000;
const row = (
  ticker: string,
  companyName: string,
  quantity: number,
  marketValue: number | null,
  gainLossPercent: number | null
) => ({ ticker, companyName, quantity, marketValue, gainLossPercent });
const vwce = row('VWCE', 'Vanguard FTSE All-World', 426, 1_182_600, 12.2);
const cez = row('CEZ', 'ČEZ', 100, 168_400, -2.3);
const chf = row('NESN', 'Nestlé', 3, null, null);
const old = row('OLD', 'Closed Co', 0, 0, null);

describe('positionRows', () => {
  it('lists open positions only, sorted by value with unknown values last', () => {
    expect(
      positionRows([cez, chf, old, vwce], { sort: 'value', search: '' }).map((r) => r.ticker)
    ).toEqual(['VWCE', 'CEZ', 'NESN']);
  });

  it('sorts by return and by name', () => {
    expect(
      positionRows([cez, chf, vwce], { sort: 'gain', search: '' }).map((r) => r.ticker)
    ).toEqual(['VWCE', 'CEZ', 'NESN']);
    expect(
      positionRows([vwce, cez, chf], { sort: 'name', search: '' }).map((r) => r.ticker)
    ).toEqual(['CEZ', 'NESN', 'VWCE']);
  });

  it('filters by ticker or name, without case and diacritics', () => {
    expect(
      positionRows([cez, vwce, chf], { sort: 'value', search: 'nestle' }).map((r) => r.ticker)
    ).toEqual(['NESN']);
    expect(searchMatches(cez, ' cez')).toBe(true);
    expect(searchMatches(cez, 'vanguard')).toBe(false);
    expect(searchMatches(cez, '')).toBe(true);
  });
});

describe('displayName', () => {
  it('prefers the company name, then the coin name, then the ticker', () => {
    expect(displayName({ ticker: 'BTC', name: 'Bitcoin' })).toBe('Bitcoin');
    expect(displayName({ ticker: 'AAPL', companyName: 'Apple' })).toBe('Apple');
    expect(displayName({ ticker: 'AAPL', companyName: '' })).toBe('AAPL');
  });
});

describe('positionRows over crypto-shaped rows', () => {
  // A coin has a `name` and no `companyName`; one without a rate has no value and no return
  const coin = (
    ticker: string,
    name: string,
    marketValue: number | null,
    gainLossPercent: number | null
  ) => ({ ticker, name, quantity: 1, marketValue, gainLossPercent });
  const eth = coin('ETH', 'Ethereum', null, null);
  const btc = coin('BTC', 'Bitcoin', 100, 5);

  it('sorts a coin without a rate last by value, by return and by name', () => {
    const tickers = (sort: 'value' | 'gain' | 'name') =>
      positionRows([eth, btc], { sort, search: '' }).map((r) => r.ticker);
    expect(tickers('value')).toEqual(['BTC', 'ETH']);
    expect(tickers('gain')).toEqual(['BTC', 'ETH']);
    expect(tickers('name')).toEqual(['BTC', 'ETH']);
  });

  it('finds a coin by its name', () => {
    expect(
      positionRows([eth, btc], { sort: 'value', search: 'ethe' }).map((r) => r.ticker)
    ).toEqual(['ETH']);
    expect(searchMatches(btc, 'bitco')).toBe(true);
  });
});

describe('foldRows', () => {
  it('shows the first eight and counts the rest until expanded', () => {
    const rows = Array.from({ length: 12 }, (_, i) => i);
    expect(ROWS_FOLD).toBe(8);
    expect(foldRows(rows, false)).toEqual({ shown: rows.slice(0, 8), hidden: 4 });
    expect(foldRows(rows, true)).toEqual({ shown: rows, hidden: 0 });
    expect(foldRows(rows.slice(0, 8), false)).toEqual({ shown: rows.slice(0, 8), hidden: 0 });
  });
});

describe('priceStamp', () => {
  it('tells today from an older day and from never', () => {
    expect(priceStamp(T - 60, T)).toEqual({ kind: 'today', at: T - 60 });
    expect(priceStamp(T - 3 * 86_400, T)).toEqual({ kind: 'day', at: T - 3 * 86_400 });
    expect(priceStamp(null, T)).toEqual({ kind: 'never' });
  });
});

describe('stocksView', () => {
  it('shows the one empty state only when nothing is owned, watched or failed', () => {
    expect(stocksView([], [], false)).toEqual({ kind: 'empty', defaultTab: 'positions' });
  });

  it('says so when the positions failed to load and there is nothing else to show', () => {
    expect(stocksView([], [], true)).toEqual({ kind: 'error', defaultTab: 'positions' });
  });

  it('keeps a watch-only user on the segment, opened on Sledované', () => {
    expect(stocksView([], [{}], false)).toEqual({ kind: 'list', defaultTab: 'watched' });
    // The watched segment stays usable when the positions failed
    expect(stocksView([], [{}], true)).toEqual({ kind: 'list', defaultTab: 'watched' });
  });

  it('opens on Pozice whenever there are positions, whatever else is true', () => {
    expect(stocksView([cez], [], false)).toEqual({ kind: 'list', defaultTab: 'positions' });
    expect(stocksView([cez], [{}], false)).toEqual({ kind: 'list', defaultTab: 'positions' });
    // A refetch that failed keeps the last data
    expect(stocksView([cez], [], true)).toEqual({ kind: 'list', defaultTab: 'positions' });
    // Only closed positions are still positions
    expect(stocksView([old], [], false)).toEqual({ kind: 'list', defaultTab: 'positions' });
  });
});
