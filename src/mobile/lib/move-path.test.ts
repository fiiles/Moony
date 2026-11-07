import { describe, expect, it } from 'vitest';
import { movePath, transactionFromSearch, transactionPath } from './move-path';

describe('movePath', () => {
  it('opens a bank payment in its account with its sheet', () => {
    expect(movePath({ id: 'bank-tx 1', kind: 'bankOut', href: '/bank-accounts/acc-1' })).toBe(
      '/finance/accounts/acc-1?tx=tx%201'
    );
    expect(movePath({ id: 'bank-t2', kind: 'bankIn', href: '/bank-accounts/acc-2' })).toBe(
      '/finance/accounts/acc-2?tx=t2'
    );
  });

  it('opens a trade on its position or coin', () => {
    expect(movePath({ id: 'stock-1', kind: 'buy', href: '/stocks/inv-1' })).toBe(
      '/wealth/stocks/inv-1'
    );
    expect(movePath({ id: 'crypto-1', kind: 'cryptoSell', href: '/crypto/c-1' })).toBe(
      '/wealth/crypto/c-1'
    );
  });

  it('opens nothing for a shape it does not know', () => {
    expect(movePath({ id: 'x', kind: 'bankOut', href: '/bank-accounts/acc-1' })).toBeNull();
    expect(movePath({ id: 'stock-1', kind: 'sell', href: '/elsewhere/1' })).toBeNull();
  });
});

describe('transaction query', () => {
  it('reads back the payment the path names', () => {
    const path = transactionPath('acc', 'a/b');
    expect(transactionFromSearch(path.slice(path.indexOf('?')))).toBe('a/b');
    expect(transactionFromSearch('?category=uncategorized')).toBeNull();
    expect(transactionFromSearch('?tx=')).toBeNull();
  });
});
