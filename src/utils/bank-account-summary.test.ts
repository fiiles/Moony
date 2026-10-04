import { describe, expect, it } from 'vitest';
import { splitBalancesCzk } from './bank-account-summary';

// 1 EUR = 25 CZK in these tests
const toCzk = (amount: number, currency: string) => (currency === 'EUR' ? amount * 25 : amount);

describe('splitBalancesCzk', () => {
  it('keeps excluded accounts out of the total and reports them separately', () => {
    // audit example: savings 500 000 plus an excluded checking account of 80 000
    const result = splitBalancesCzk(
      [
        { balance: '500000', currency: 'CZK', excludeFromBalance: false },
        { balance: '80000', currency: 'CZK', excludeFromBalance: true },
      ],
      toCzk
    );
    expect(result).toEqual({ included: 500000, excluded: 80000, excludedCount: 1 });
  });

  it('converts each account at its own currency', () => {
    const result = splitBalancesCzk(
      [
        { balance: '100', currency: 'EUR', excludeFromBalance: false },
        { balance: '10', currency: 'EUR', excludeFromBalance: true },
        { balance: '1000', currency: null, excludeFromBalance: false },
      ],
      toCzk
    );
    expect(result).toEqual({ included: 3500, excluded: 250, excludedCount: 1 });
  });

  it('treats a missing flag as included and an unreadable balance as 0', () => {
    const result = splitBalancesCzk([{ balance: '', currency: 'CZK' }, { balance: '5' }], toCzk);
    expect(result).toEqual({ included: 5, excluded: 0, excludedCount: 0 });
  });

  it('handles no accounts', () => {
    expect(splitBalancesCzk([], toCzk)).toEqual({ included: 0, excluded: 0, excludedCount: 0 });
  });
});
