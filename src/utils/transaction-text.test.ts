import { describe, expect, it } from 'vitest';
import { transactionDetails, transactionTitle } from './transaction-text';

const tx = (
  counterpartyName: string | null,
  description: string | null,
  remittanceInfo: string | null
) => ({ counterpartyName, description, remittanceInfo });

describe('transactionTitle', () => {
  it('prefers the counterparty name', () => {
    expect(transactionTitle(tx('Albert', 'Platba kartou', null))).toBe('Albert');
  });

  it('falls back to the description when the name is blank', () => {
    expect(transactionTitle(tx('  ', 'Platba kartou', null))).toBe('Platba kartou');
  });

  it('is null without any text', () => {
    expect(transactionTitle(tx(null, null, null))).toBeNull();
  });
});

describe('transactionDetails', () => {
  it('lists the description and the remittance info', () => {
    expect(transactionDetails(tx('Albert', 'Platba kartou', 'Nákup 42'))).toEqual([
      'Platba kartou',
      'Nákup 42',
    ]);
  });

  it('skips text already shown as the title', () => {
    expect(transactionDetails(tx(null, 'Platba kartou', 'Nákup 42'))).toEqual(['Nákup 42']);
  });

  it('skips duplicates regardless of case and spacing', () => {
    expect(transactionDetails(tx('Albert', 'Nájem  říjen', 'nájem říjen'))).toEqual([
      'Nájem  říjen',
    ]);
  });

  it('ignores blank values', () => {
    expect(transactionDetails(tx('Albert', ' ', null))).toEqual([]);
  });
});
