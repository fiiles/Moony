import { describe, it, expect } from 'vitest';
import {
  insurancePaymentFields,
  normalizeInsurancePayments,
  insurancePaymentIssues,
} from './insurance-payment';

describe('insurancePaymentFields', () => {
  it('one-time policy: only the one-time amount, and it is required', () => {
    expect(insurancePaymentFields('one_time', false)).toEqual({
      showOneTime: true,
      oneTimeRequired: true,
      showRegular: false,
    });
  });

  it.each(['monthly', 'quarterly', 'semi_annually', 'annually'] as const)(
    '%s policy: only the regular payment',
    (frequency) => {
      expect(insurancePaymentFields(frequency, false)).toEqual({
        showOneTime: false,
        oneTimeRequired: false,
        showRegular: true,
      });
    }
  );

  it('keeps an existing one-time amount of a recurring policy visible (optional) so saving cannot drop it silently', () => {
    expect(insurancePaymentFields('monthly', true)).toEqual({
      showOneTime: true,
      oneTimeRequired: false,
      showRegular: true,
    });
  });
});

describe('normalizeInsurancePayments', () => {
  it('one-time policy: regular payment is zero, one-time amount is kept', () => {
    expect(
      normalizeInsurancePayments({
        frequency: 'one_time',
        oneTimePayment: '5000',
        regularPayment: '120',
        keepOneTime: false,
      })
    ).toEqual({ oneTimePayment: '5000', regularPayment: '0' });
  });

  it('recurring policy: the hidden one-time amount is cleared', () => {
    expect(
      normalizeInsurancePayments({
        frequency: 'monthly',
        oneTimePayment: '5000',
        regularPayment: '120',
        keepOneTime: false,
      })
    ).toEqual({ oneTimePayment: undefined, regularPayment: '120' });
  });

  it('recurring policy with a stored one-time amount: both are kept', () => {
    expect(
      normalizeInsurancePayments({
        frequency: 'annually',
        oneTimePayment: '5000',
        regularPayment: '120',
        keepOneTime: true,
      })
    ).toEqual({ oneTimePayment: '5000', regularPayment: '120' });
  });

  it('an empty one-time field becomes undefined', () => {
    expect(
      normalizeInsurancePayments({
        frequency: 'annually',
        oneTimePayment: '',
        regularPayment: '120',
        keepOneTime: true,
      }).oneTimePayment
    ).toBeUndefined();
  });
});

describe('insurancePaymentIssues', () => {
  it('requires a positive one-time amount for a one-time policy', () => {
    expect(insurancePaymentIssues('one_time', undefined, '0')).toEqual([
      { path: 'oneTimePayment', key: 'validation.amountRequired' },
    ]);
    expect(insurancePaymentIssues('one_time', '', '0')).toEqual([
      { path: 'oneTimePayment', key: 'validation.amountRequired' },
    ]);
    expect(insurancePaymentIssues('one_time', '-1', '0')).toEqual([
      { path: 'oneTimePayment', key: 'validation.paymentNonNegative' },
    ]);
    expect(insurancePaymentIssues('one_time', '0', '0')).toEqual([
      { path: 'oneTimePayment', key: 'validation.amountRequired' },
    ]);
    expect(insurancePaymentIssues('one_time', '5000', '0')).toEqual([]);
  });

  it('does not look at the hidden regular payment of a one-time policy', () => {
    expect(insurancePaymentIssues('one_time', '5000', '')).toEqual([]);
  });

  it('requires a non-negative regular amount for a recurring policy', () => {
    expect(insurancePaymentIssues('monthly', undefined, '')).toEqual([
      { path: 'regularPayment', key: 'validation.amountRequired' },
    ]);
    expect(insurancePaymentIssues('monthly', undefined, '-5')).toEqual([
      { path: 'regularPayment', key: 'validation.paymentNonNegative' },
    ]);
    expect(insurancePaymentIssues('monthly', undefined, '0')).toEqual([]);
    expect(insurancePaymentIssues('monthly', undefined, '120.50')).toEqual([]);
  });

  it('does not require the optional one-time amount of a recurring policy, but rejects a negative one', () => {
    expect(insurancePaymentIssues('annually', '', '100')).toEqual([]);
    expect(insurancePaymentIssues('annually', '-1', '100')).toEqual([
      { path: 'oneTimePayment', key: 'validation.paymentNonNegative' },
    ]);
  });
});
