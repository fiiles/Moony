import { describe, expect, it } from 'vitest';
import { closedPositions, lastSellDays } from './closed-positions';

const row = (id: string, ticker: string, quantity: number) => ({
  id,
  ticker,
  quantity,
});
const day = (month: number, d: number, year = 2026) => Date.UTC(year, month - 1, d) / 1000;

describe('closedPositions', () => {
  const vwce = row('a', 'VWCE', 426);
  const cez = row('b', 'CEZ', 0);
  const old = row('c', 'OLD', 0);
  const nesn = row('d', 'NESN', 0);
  const sells: Record<string, number | null> = {
    b: day(3, 14),
    c: day(10, 2),
    d: null,
  };

  it('leaves the open positions out and puts the newest close first', () => {
    const closed = closedPositions([vwce, cez, old, nesn], (id) => sells[id] ?? null);
    expect(closed.map(({ row }) => row.ticker)).toEqual(['OLD', 'CEZ', 'NESN']);
    expect(closed.map(({ closedAt }) => closedAt)).toEqual([day(10, 2), day(3, 14), null]);
  });

  it('puts the positions without a known sell last, in the order they came', () => {
    const closed = closedPositions([nesn, cez, row('e', 'ABC', 0), old], (id) => sells[id] ?? null);
    expect(closed.map(({ row }) => row.ticker)).toEqual(['OLD', 'CEZ', 'NESN', 'ABC']);
  });

  it('keeps the order of positions closed on the same day', () => {
    const closed = closedPositions([cez, old], () => day(5, 5));
    expect(closed.map(({ row }) => row.ticker)).toEqual(['CEZ', 'OLD']);
  });

  it('asks for the last sell of each closed position by its id, and of no open one', () => {
    const asked: string[] = [];
    closedPositions([vwce, cez, old], (id) => {
      asked.push(id);
      return null;
    });
    expect(asked).toEqual(['b', 'c']);
  });

  it('is empty when nothing is closed', () => {
    expect(closedPositions([vwce], () => null)).toEqual([]);
    expect(closedPositions([], () => null)).toEqual([]);
  });

  it('hands back the very row it was given', () => {
    expect(closedPositions([cez], () => null)[0].row).toBe(cez);
  });
});

describe('lastSellDays', () => {
  const trade = (investmentId: string, type: string, transactionDate: number) => ({
    investmentId,
    type,
    transactionDate,
  });

  it('keeps the newest sell of each position, whatever the order of the trades', () => {
    const days = lastSellDays([
      trade('a', 'sell', day(3, 1)),
      trade('a', 'sell', day(9, 1)),
      trade('a', 'sell', day(6, 1)),
      trade('b', 'sell', day(2, 1)),
    ]);
    expect(days.get('a')).toBe(day(9, 1));
    expect(days.get('b')).toBe(day(2, 1));
  });

  it('ignores buys: a position bought after its last sell is not closed by them', () => {
    const days = lastSellDays([trade('a', 'sell', day(3, 1)), trade('a', 'buy', day(9, 1))]);
    expect(days.get('a')).toBe(day(3, 1));
  });

  it('has no entry for a position that was only bought', () => {
    const days = lastSellDays([trade('a', 'buy', day(3, 1))]);
    expect(days.has('a')).toBe(false);
    expect(lastSellDays([]).size).toBe(0);
  });
});
