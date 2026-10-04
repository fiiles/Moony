import { describe, expect, test } from 'vitest';
import {
  PAYMENT_FREQUENCIES,
  annualizeAmount,
  parsePaymentFrequency,
  paymentsPerYear,
} from './payment-frequency';

describe('payment frequency', () => {
  test('canonical values parse to themselves', () => {
    for (const f of PAYMENT_FREQUENCIES) expect(parsePaymentFrequency(f)).toBe(f);
  });

  test('legacy spellings are accepted', () => {
    expect(parsePaymentFrequency('yearly')).toBe('annually');
    expect(parsePaymentFrequency('Annual')).toBe('annually');
    expect(parsePaymentFrequency('semi-annually')).toBe('semi_annually');
    expect(parsePaymentFrequency('one-time')).toBe('one_time');
    expect(parsePaymentFrequency('weekly')).toBeNull();
    expect(parsePaymentFrequency(null)).toBeNull();
  });

  test('annualizes each frequency; unknown and one-time count as 0', () => {
    expect(annualizeAmount(3219, 'yearly')).toBe(3219); // the dropped Hyundai policy
    expect(annualizeAmount(100, 'monthly')).toBe(1200);
    expect(annualizeAmount(100, 'quarterly')).toBe(400);
    expect(annualizeAmount(10_000, 'semi_annually')).toBe(20_000);
    expect(annualizeAmount(50_000, 'one_time')).toBe(0);
    expect(annualizeAmount(50_000, 'weekly')).toBe(0);
    expect(paymentsPerYear('annually')).toBe(1);
  });
});
