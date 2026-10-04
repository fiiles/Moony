import { describe, expect, it } from 'vitest';
import { firstTradeDay, tradeDayEvents, type TradeLike } from './trade-events';
import { utcDayStart } from './period';

const DAY = 86_400;
const D1 = utcDayStart('2026-09-29');
const D2 = utcDayStart('2026-09-30');
const D3 = utcDayStart('2026-10-01');

function trade(partial: Partial<TradeLike> & Pick<TradeLike, 'transactionDate'>): TradeLike {
  return {
    type: 'buy',
    ticker: 'AAPL',
    quantity: '1',
    pricePerUnit: '100',
    currency: 'CZK',
    ...partial,
  };
}

// 1 USD = 25 CZK, 1 EUR = 24 CZK regardless of the day
const FLAT_RATES: Record<string, number> = { CZK: 1, USD: 25, EUR: 24 };
const toCzk = (amount: number, currency: string) => amount * (FLAT_RATES[currency] ?? 1);

describe('tradeDayEvents', () => {
  it('folds the trades of one day and direction into one event', () => {
    const events = tradeDayEvents(
      [
        trade({ ticker: 'AAPL', quantity: '2', pricePerUnit: '100', transactionDate: D2 }),
        trade({ ticker: 'MSFT', quantity: '1', pricePerUnit: '300', transactionDate: D2 }),
      ],
      toCzk
    );
    expect(events).toEqual([
      { id: `buy-${D2}`, t: D2, type: 'buy', tickers: ['AAPL', 'MSFT'], amountCzk: 500 },
    ]);
  });

  it('keeps buys and sells of the same day apart, buy first', () => {
    const events = tradeDayEvents(
      [
        trade({ type: 'sell', ticker: 'AAPL', transactionDate: D2 }),
        trade({ type: 'buy', ticker: 'MSFT', transactionDate: D2 }),
      ],
      toCzk
    );
    expect(events.map((e) => [e.id, e.type, e.tickers])).toEqual([
      [`buy-${D2}`, 'buy', ['MSFT']],
      [`sell-${D2}`, 'sell', ['AAPL']],
    ]);
  });

  it('lists every ticker once, in the order its first trade appears', () => {
    const events = tradeDayEvents(
      [
        trade({ ticker: 'MSFT', transactionDate: D2 }),
        trade({ ticker: 'AAPL', transactionDate: D2 }),
        trade({ ticker: 'MSFT', transactionDate: D2 }),
        trade({ ticker: 'AAPL', transactionDate: D2 }),
        trade({ ticker: 'NVDA', transactionDate: D2 }),
      ],
      toCzk
    );
    expect(events).toHaveLength(1);
    expect(events[0].tickers).toEqual(['MSFT', 'AAPL', 'NVDA']);
    expect(events[0].amountCzk).toBe(500);
  });

  it('treats a sell as a sell and every other type as a buy', () => {
    const events = tradeDayEvents(
      [
        trade({ type: 'sell', transactionDate: D1 }),
        trade({ type: 'buy', transactionDate: D2 }),
        trade({ type: 'transfer-in', transactionDate: D3 }),
      ],
      toCzk
    );
    expect(events.map((e) => e.type)).toEqual(['sell', 'buy', 'buy']);
  });

  it('groups by UTC calendar day, not by a 24-hour window', () => {
    const events = tradeDayEvents(
      [
        trade({ ticker: 'AAPL', transactionDate: D2 + 23 * 3600 + 3599 }), // 23:59:59 UTC
        trade({ ticker: 'MSFT', transactionDate: D2 + DAY }), // 00:00:00 the next day
        trade({ ticker: 'NVDA', transactionDate: D2 + 12 * 3600 }), // midday, same day as the first
      ],
      toCzk
    );
    expect(events.map((e) => [e.t, e.tickers])).toEqual([
      [D2, ['AAPL', 'NVDA']],
      [D3, ['MSFT']],
    ]);
  });

  it('sorts by day whatever the order of the input', () => {
    const events = tradeDayEvents(
      [
        trade({ type: 'sell', transactionDate: D3 }),
        trade({ type: 'buy', transactionDate: D3 }),
        trade({ type: 'buy', transactionDate: D1 }),
        trade({ type: 'sell', transactionDate: D2 }),
      ],
      toCzk
    );
    expect(events.map((e) => e.id)).toEqual([`buy-${D1}`, `sell-${D2}`, `buy-${D3}`, `sell-${D3}`]);
  });

  it('converts each trade from its own currency and sums in CZK', () => {
    const events = tradeDayEvents(
      [
        trade({ quantity: '2', pricePerUnit: '10', currency: 'USD', transactionDate: D2 }), // 20 USD
        trade({ quantity: '5', pricePerUnit: '10', currency: 'EUR', transactionDate: D2 }), // 50 EUR
        trade({ quantity: '1', pricePerUnit: '70', currency: 'CZK', transactionDate: D2 }),
      ],
      toCzk
    );
    expect(events[0].amountCzk).toBe(20 * 25 + 50 * 24 + 70);
  });

  it('hands the converter the amount, the currency and the date of the trade', () => {
    const calls: Array<[number, string, number]> = [];
    tradeDayEvents(
      [
        trade({ quantity: '3', pricePerUnit: '1.5', currency: 'USD', transactionDate: D1 }),
        trade({ quantity: '4', pricePerUnit: '2', currency: 'EUR', transactionDate: D3 + 3600 }),
      ],
      (amount, currency, date) => {
        calls.push([amount, currency, date]);
        return amount;
      }
    );
    expect(calls).toEqual([
      [4.5, 'USD', D1],
      [8, 'EUR', D3 + 3600],
    ]);
  });

  it('converts at the rate of the trade date, not of another day', () => {
    // USD/CZK: 25 on D1, 20 on D2: the same 100 USD is worth different CZK on each day
    const rateOn: Record<number, number> = { [D1]: 25, [D2]: 20 };
    const events = tradeDayEvents(
      [
        trade({ quantity: '1', pricePerUnit: '100', currency: 'USD', transactionDate: D1 }),
        trade({ quantity: '1', pricePerUnit: '100', currency: 'USD', transactionDate: D2 }),
      ],
      (amount, _currency, date) => amount * (rateOn[Math.floor(date / DAY) * DAY] ?? Number.NaN)
    );
    expect(events.map((e) => e.amountCzk)).toEqual([2500, 2000]);
  });

  it('reads a missing, null or empty currency as CZK', () => {
    const currencies: string[] = [];
    tradeDayEvents(
      [
        trade({ currency: undefined, transactionDate: D1 }),
        trade({ currency: null, transactionDate: D2 }),
        trade({ currency: '', transactionDate: D3 }),
      ],
      (amount, currency) => {
        currencies.push(currency);
        return amount;
      }
    );
    expect(currencies).toEqual(['CZK', 'CZK', 'CZK']);
  });

  it('counts an unreadable quantity or price as zero instead of poisoning the day', () => {
    const events = tradeDayEvents(
      [
        trade({ quantity: 'n/a', pricePerUnit: '100', transactionDate: D2 }),
        trade({ quantity: '2', pricePerUnit: '', transactionDate: D2 }),
        trade({ quantity: '1', pricePerUnit: '40', transactionDate: D2 }),
      ],
      toCzk
    );
    expect(events[0].amountCzk).toBe(40);
  });

  it('returns nothing for no trades', () => {
    expect(tradeDayEvents([], toCzk)).toEqual([]);
  });
});

describe('firstTradeDay', () => {
  it('is the UTC day of the earliest trade', () => {
    expect(
      firstTradeDay([
        trade({ transactionDate: D3 }),
        trade({ transactionDate: D1 + 15 * 3600 }),
        trade({ transactionDate: D2 }),
      ])
    ).toBe(D1);
  });

  it('is undefined without trades', () => {
    expect(firstTradeDay([])).toBeUndefined();
  });
});
