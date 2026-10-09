import { describe, expect, it } from 'vitest';
import { accountTotals } from './bank-account-summary';

/** 1 EUR = 25 CZK; no rate for KWD. */
const convert = (amount: number, currency: string) =>
  currency === 'KWD' ? null : currency === 'EUR' ? amount * 25 : amount;

describe('accountTotals', () => {
  it('keeps excluded accounts out of the total and reports them separately', () => {
    // audit example: savings 500 000 plus an excluded checking account of 80 000
    const result = accountTotals(
      [
        { balance: '500000', currency: 'CZK', excludeFromBalance: false },
        { balance: '80000', currency: 'CZK', excludeFromBalance: true },
      ],
      convert
    );
    expect(result).toEqual({
      included: 500000,
      excluded: 80000,
      excludedCount: 1,
      income: 0,
      expense: 0,
      missing: [],
    });
  });

  it('converts each account and its 30-day flow at its own currency', () => {
    const result = accountTotals(
      [
        {
          balance: '100',
          currency: 'EUR',
          excludeFromBalance: false,
          last30: { income: 10, expense: 4 },
        },
        {
          balance: '10',
          currency: 'EUR',
          excludeFromBalance: true,
          last30: { income: 99, expense: 0 },
        },
        { balance: '1000', currency: null, excludeFromBalance: false },
      ],
      convert
    );
    expect(result).toMatchObject({ included: 3500, excluded: 250, excludedCount: 1 });
    // Excluded accounts stay out of the flow too
    expect(result).toMatchObject({ income: 250, expense: 100 });
  });

  it('treats a missing flag as included and an unreadable balance as 0', () => {
    const result = accountTotals([{ balance: '', currency: 'CZK' }, { balance: '5' }], convert);
    expect(result).toMatchObject({ included: 5, excluded: 0, excludedCount: 0 });
  });

  it('excludes unconvertible accounts from the totals and reports them', () => {
    const result = accountTotals(
      [
        { balance: '100', currency: 'EUR', last30: { income: 1, expense: 2 } },
        { balance: '7', currency: 'KWD', last30: { income: 3, expense: 1 } },
        { balance: '9', currency: 'KWD', excludeFromBalance: true },
      ],
      convert
    );
    expect(result).toMatchObject({ included: 2500, excluded: 0, income: 25, expense: 50 });
    expect(result.excludedCount).toBe(1);
    expect(result.missing).toEqual(['KWD']);
  });

  it('handles no accounts', () => {
    expect(accountTotals([], convert)).toMatchObject({ included: 0, excluded: 0, missing: [] });
  });
});
