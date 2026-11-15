import { describe, expect, it } from 'vitest';
import { transactionSubFacts, transactionTitle } from './transaction-title';

const tx = (over: Partial<Parameters<typeof transactionTitle>[0]> = {}) => ({
  counterpartyName: null,
  counterpartyIban: null,
  counterpartyBankCode: null,
  description: null,
  ...over,
});

describe('transactionTitle', () => {
  it('names the counterparty first', () => {
    expect(
      transactionTitle(
        tx({ counterpartyName: ' Albert ', description: 'Nákup', counterpartyIban: '123' })
      )
    ).toBe('Albert');
  });

  it('falls back to the description before the account number', () => {
    expect(
      transactionTitle(
        tx({ description: 'Nájem', counterpartyIban: '2100123456', counterpartyBankCode: '2010' })
      )
    ).toBe('Nájem');
  });

  it('uses the account number when nothing else names the payment', () => {
    expect(
      transactionTitle(tx({ counterpartyIban: '2100123456', counterpartyBankCode: '2010' }))
    ).toBe('2100123456/2010');
  });

  it('returns null when the payment says nothing', () => {
    expect(transactionTitle(tx({ description: '   ' }))).toBeNull();
  });
});

describe('transactionSubFacts', () => {
  it('keeps day, account and category in that order and drops what the screen already says', () => {
    expect(transactionSubFacts({ day: 'Dnes', account: 'Fio', category: 'Potraviny' })).toEqual([
      'Dnes',
      'Fio',
      'Potraviny',
    ]);
    expect(transactionSubFacts({ day: null, account: 'Fio', category: null })).toEqual(['Fio']);
  });
});
