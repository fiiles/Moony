import { describe, expect, it } from 'vitest';
import { parseAmountInput } from './amount-input';
import { emptyManualTxForm, manualTxErrors, manualTxInput } from './manual-transaction';
import { isoToUtcDay } from './property-detail';

const form = emptyManualTxForm('2026-11-07');

describe('emptyManualTxForm', () => {
  it('starts as an outgoing payment for today without a category', () => {
    expect(form).toEqual({
      type: 'debit',
      amountText: '',
      dateIso: '2026-11-07',
      counterparty: '',
      description: '',
      counterpartyIban: '',
      categoryId: null,
    });
  });
});

describe('manualTxErrors', () => {
  it('names a missing, unreadable and non-positive amount', () => {
    expect(manualTxErrors(form)).toEqual({ amount: 'required' });
    expect(manualTxErrors({ ...form, amountText: 'abc' })).toEqual({ amount: 'invalid' });
    expect(manualTxErrors({ ...form, amountText: '0' })).toEqual({ amount: 'notPositive' });
    expect(manualTxErrors({ ...form, amountText: '-5' })).toEqual({ amount: 'notPositive' });
  });

  it('accepts a comma amount with a thousands space', () => {
    expect(manualTxErrors({ ...form, amountText: '1 234,50' })).toEqual({});
  });

  it('requires a date', () => {
    expect(manualTxErrors({ ...form, amountText: '5', dateIso: '' })).toEqual({
      date: 'required',
    });
  });
});

describe('manualTxInput', () => {
  it('writes what the desktop modal writes, empty texts as null', () => {
    expect(
      manualTxInput(
        {
          ...form,
          amountText: '1 234,50',
          counterparty: '  Albert ',
          categoryId: 'c1',
        },
        'acc',
        'CZK'
      )
    ).toEqual({
      bankAccountId: 'acc',
      type: 'debit',
      amount: parseAmountInput('1 234,50'),
      currency: 'CZK',
      bookingDate: isoToUtcDay('2026-11-07'),
      description: null,
      counterpartyName: 'Albert',
      counterpartyIban: null,
      categoryId: 'c1',
      status: 'booked',
    });
    expect(parseAmountInput('1 234,50')).toBe('1234.50');
  });

  it('is null while the form has an error', () => {
    expect(manualTxInput(form, 'acc', 'CZK')).toBeNull();
  });
});
