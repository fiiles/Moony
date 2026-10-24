import { describe, expect, it } from 'vitest';
import {
  filterTransactions,
  nativeTotal,
  positionEvents,
  positionPoints,
  revealRow,
  ytdRange,
} from './position-series';

const DAY = 86_400;
const T = Date.UTC(2026, 9, 16) / 1000;
const h = (recordedAt: number, value: string | null, costBasis: string | null) => ({
  ticker: 'VWCE',
  recordedAt,
  quantity: '1',
  price: '1',
  currency: 'EUR',
  value,
  costBasis,
  isStale: false,
});
const cryptoTx = (id: string, type: string, date: number) => ({
  id,
  investmentId: 'c',
  type,
  ticker: 'BTC',
  name: 'Bitcoin',
  quantity: '0.5',
  pricePerUnit: '60000',
  currency: 'USD',
  transactionDate: date,
  createdAt: date,
});
const tx = (id: string, type: string, date: number, quantity = '10', pricePerUnit = '110.4') => ({
  id,
  investmentId: 'i',
  type,
  ticker: 'VWCE',
  companyName: 'V',
  quantity,
  pricePerUnit,
  currency: 'EUR',
  transactionDate: date,
  createdAt: date,
});

describe('positionPoints', () => {
  it("carries value and cost per day and ends on today's figures", () => {
    const report = {
      currency: 'CZK',
      fxQuality: { missingCurrencies: [] },
      points: [h(T - 2 * DAY, '1000', '900'), h(T - DAY, null, '900'), h(T, '1100', '950')],
    };
    expect(positionPoints(report, 'CZK', 1_182_600, 1_054_300, T + 3_600)).toEqual([
      { t: T - 2 * DAY, value: 1000, cost: 900 },
      { t: T, value: 1_182_600, cost: 1_054_300 },
    ]);
  });

  it('appends today, leaves it out without a value, and ignores a report in another currency', () => {
    const report = {
      currency: 'CZK',
      fxQuality: { missingCurrencies: [] },
      points: [h(T - DAY, '1000', null)],
    };
    expect(positionPoints(report, 'CZK', 1200, null, T + 3_600)).toEqual([
      { t: T - DAY, value: 1000, cost: undefined },
      { t: T + 3_600, value: 1200, cost: undefined },
    ]);
    expect(positionPoints(report, 'CZK', null, null, T + 3_600)).toEqual([
      { t: T - DAY, value: 1000, cost: undefined },
    ]);
    expect(positionPoints({ ...report, currency: 'EUR' }, 'CZK', 5, null, T)).toEqual([
      { t: T, value: 5, cost: undefined },
    ]);
  });
});

describe('positionEvents and filterTransactions', () => {
  const txs = [
    tx('a', 'buy', T - 30 * DAY),
    tx('b', 'sell', T - 10 * DAY),
    tx('c', 'buy', T - DAY),
  ];

  it('marks every trade by its day and direction', () => {
    expect(positionEvents(txs).map((e) => [e.id, e.t, e.type])).toEqual([
      ['a', T - 30 * DAY, 'buy'],
      ['b', T - 10 * DAY, 'sell'],
      ['c', T - DAY, 'buy'],
    ]);
  });

  it('filters newest first', () => {
    expect(filterTransactions(txs, 'all').map((t) => t.id)).toEqual(['c', 'b', 'a']);
    expect(filterTransactions(txs, 'buy').map((t) => t.id)).toEqual(['c', 'a']);
    expect(filterTransactions(txs, 'sell').map((t) => t.id)).toEqual(['b']);
  });

  it('takes a crypto trade as it does a stock trade, keeping the same object', () => {
    const buy = cryptoTx('x', 'buy', T - 5 * DAY);
    const sell = cryptoTx('y', 'sell', T - DAY);
    const events = positionEvents([buy, sell]);
    expect(events.map((e) => [e.id, e.t, e.type])).toEqual([
      ['x', T - 5 * DAY, 'buy'],
      ['y', T - DAY, 'sell'],
    ]);
    expect(events[0].tx).toBe(buy);
    expect(events[0].tx.name).toBe('Bitcoin');
    const rows = filterTransactions([buy, sell], 'all');
    expect(rows[0]).toBe(sell);
    expect(rows[1]).toBe(buy);
    expect(filterTransactions([buy, sell], 'buy')[0]).toBe(buy);
  });

  it('totals a trade natively', () => {
    expect(nativeTotal(tx('x', 'buy', T, '12', '110.4'))).toBeCloseTo(1_324.8);
    expect(nativeTotal(tx('x', 'buy', T, 'abc', '1'))).toBe(0);
  });
});

describe('ytdRange', () => {
  it("runs from 1 January UTC to today's UTC day", () => {
    expect(ytdRange(T + 3_600)).toEqual({ fromTs: Date.UTC(2026, 0, 1) / 1000, toTs: T });
  });
});

describe('revealRow', () => {
  // Newest first the list reads n10 … n1: ten buys, and the oldest of them a sell
  const many = Array.from({ length: 10 }, (_, i) =>
    tx(`n${i + 1}`, i === 0 ? 'sell' : 'buy', T - (10 - i) * DAY)
  );

  it('leaves a row alone that is already in the first eight', () => {
    expect(revealRow(many, 'all', 'n10')).toEqual({ filter: 'all', expand: false });
  });

  it('opens the fold for a row it hides', () => {
    expect(revealRow(many, 'all', 'n1')).toEqual({ filter: 'all', expand: true });
  });

  it('shows everything when the filter leaves the row out, and opens the fold if it then hides it', () => {
    expect(revealRow(many, 'sell', 'n10')).toEqual({ filter: 'all', expand: false });
    expect(revealRow(many, 'buy', 'n1')).toEqual({ filter: 'all', expand: true });
  });

  it('does nothing for a row that is not there', () => {
    expect(revealRow(many, 'buy', 'gone')).toEqual({ filter: 'buy', expand: false });
  });
});
