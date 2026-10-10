import { describe, expect, it } from 'vitest';
import { counterpartyLabel, transactionDetails } from './transaction-text';

const tx = (
  counterpartyName: string | null,
  counterpartyIban: string | null,
  description: string | null = null,
  remittanceInfo: string | null = null
) => ({
  counterpartyName,
  counterpartyIban,
  counterpartyBankCode: null,
  description,
  remittanceInfo,
});

describe('counterpartyLabel', () => {
  it('shows the name with the full account number underneath', () => {
    expect(counterpartyLabel(tx('Albert', 'CZ6508000000192000145399'))).toEqual({
      title: 'Albert',
      subtitle: '19-2000145399/0800',
    });
  });

  it('promotes the account number when the name is unknown', () => {
    expect(counterpartyLabel(tx('  ', 'CZ6508000000192000145399'))).toEqual({
      title: '19-2000145399/0800',
      subtitle: null,
    });
  });

  it('never borrows the description', () => {
    expect(counterpartyLabel(tx(null, null, 'Platba kartou'))).toEqual({
      title: null,
      subtitle: null,
    });
  });
});

describe('transactionDetails', () => {
  it('lists the description and the remittance info', () => {
    expect(transactionDetails(tx('Albert', null, 'Platba kartou', 'Nákup 42'))).toEqual([
      'Platba kartou',
      'Nákup 42',
    ]);
  });

  it('keeps the description even without a counterparty name', () => {
    expect(transactionDetails(tx(null, null, 'Platba kartou'))).toEqual(['Platba kartou']);
  });

  it('skips duplicates regardless of case and spacing', () => {
    expect(transactionDetails(tx('Albert', null, 'Nájem  říjen', 'nájem říjen'))).toEqual([
      'Nájem  říjen',
    ]);
  });

  it('ignores blank values', () => {
    expect(transactionDetails(tx('Albert', null, ' ', null))).toEqual([]);
  });
});
