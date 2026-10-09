import { describe, expect, it } from 'vitest';
import type { InsurancePolicy } from '@shared/schema';
import {
  insuranceMetrics,
  insuranceRow,
  limitsByValue,
  isEnded,
  nextAnniversary,
  nextPaymentDay,
  paidSeries,
  paymentCalendar,
  paymentDays,
  paymentsMade,
  policyToInsert,
} from './insurance';

const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;
const TODAY = day(2026, 10, 3);
/** 1 EUR = 25 CZK, 1 JPY = 0.15 CZK; no rate for KWD. */
const RATES: Record<string, number> = { CZK: 1, EUR: 25, JPY: 0.15 };
const convert = (amount: number, currency: string) =>
  currency in RATES ? amount * RATES[currency] : null;

function policy(partial: Partial<InsurancePolicy>): InsurancePolicy {
  return {
    id: 'p',
    type: 'life',
    provider: 'Kooperativa',
    policyName: 'Život',
    policyNumber: null,
    startDate: day(2019, 4, 1),
    endDate: null,
    paymentFrequency: 'monthly',
    oneTimePayment: null,
    oneTimePaymentCurrency: null,
    regularPayment: '1150',
    regularPaymentCurrency: 'CZK',
    limits: [{ title: 'Smrt', amount: 2_000_000, currency: 'CZK' }],
    notes: null,
    status: 'active',
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

describe('payment days', () => {
  it('falls on the start day of month every period and stops at the end date', () => {
    const quarterly = policy({ paymentFrequency: 'quarterly', startDate: day(2026, 1, 31) });
    expect(paymentDays(quarterly, TODAY, day(2027, 10, 3))).toEqual([
      day(2026, 10, 31),
      day(2027, 1, 31),
      day(2027, 4, 30),
      day(2027, 7, 31),
    ]);
    expect(nextPaymentDay(policy({}), TODAY)).toBe(day(2026, 11, 1));
    expect(nextPaymentDay(policy({ endDate: day(2026, 10, 15) }), TODAY)).toBeNull();
    expect(nextPaymentDay(policy({ paymentFrequency: 'one_time' }), TODAY)).toBeNull();
  });

  it('counts the payments made including the first one on the start day', () => {
    // April 2019 .. October 2026 = 91 monthly payments
    expect(paymentsMade(policy({}), TODAY)).toBe(91);
    expect(paymentsMade(policy({ paymentFrequency: 'annually' }), TODAY)).toBe(8);
    expect(paymentsMade(policy({ startDate: day(2027, 1, 1) }), TODAY)).toBe(0);
  });

  it('finds the next anniversary unless the contract ends first', () => {
    expect(nextAnniversary(policy({}), TODAY)).toBe(day(2027, 4, 1));
    expect(nextAnniversary(policy({ endDate: day(2027, 3, 1) }), TODAY)).toBeNull();
  });
});

describe('insuranceRow / insuranceMetrics', () => {
  it('derives premiums, coverage and status', () => {
    const row = insuranceRow(policy({}), TODAY, convert);
    expect(row.premium).toBe(1150);
    expect(row.yearly).toBe(13_800);
    expect(row.coverage).toBe(2_000_000);
    expect(row.ended).toBe(false);
    expect(row.nextPayment).toBe(day(2026, 11, 1));
    const once = insuranceRow(
      policy({
        paymentFrequency: 'one_time',
        regularPayment: '0',
        oneTimePayment: '2400',
        oneTimePaymentCurrency: 'CZK',
        endDate: day(2026, 6, 30),
      }),
      TODAY,
      convert
    );
    expect(once.premium).toBe(2400);
    expect(once.yearly).toBe(0);
    expect(once.ended).toBe(true);
    expect(isEnded(policy({ status: 'inactive' }), TODAY)).toBe(true);
  });

  it('totals the active policies and finds the next payment and date', () => {
    const life = insuranceRow(policy({ id: 'a' }), TODAY, convert);
    const car = insuranceRow(
      policy({
        id: 'b',
        type: 'vehicle',
        paymentFrequency: 'annually',
        regularPayment: '7800',
        startDate: day(2024, 1, 15),
        limits: [{ title: 'POV', amount: 5_000_000, currency: 'CZK' }],
      }),
      TODAY,
      convert
    );
    const old = insuranceRow(policy({ id: 'c', status: 'inactive' }), TODAY, convert);
    const m = insuranceMetrics([life, car, old], TODAY);
    expect(m.activeCount).toBe(2);
    expect(m.endedCount).toBe(1);
    expect(m.yearlyTotal).toBe(13_800 + 7_800);
    expect(m.coverageTotal).toBe(7_000_000);
    expect(m.coverageByType[0]).toEqual({ type: 'vehicle', value: 5_000_000 });
    expect(m.nextPayment).toEqual({ day: day(2026, 11, 1), amount: 1150, rows: [life] });
    expect(m.nextDate?.day).toBe(day(2027, 1, 15));
    expect(m.nextDate?.kind).toBe('anniversary');
    expect(m.missing).toEqual([]);
  });

  it('excludes unconvertible policies from the totals and reports them', () => {
    const eur = insuranceRow(
      policy({ id: 'a', regularPayment: '10', regularPaymentCurrency: 'EUR', limits: [] }),
      TODAY,
      convert
    );
    const kwd = insuranceRow(
      policy({
        id: 'b',
        regularPayment: '7',
        regularPaymentCurrency: 'KWD',
        limits: [
          { title: 'Smrt', amount: 1_000, currency: 'KWD' },
          { title: 'Úraz', amount: 100_000, currency: 'CZK' },
        ],
      }),
      TODAY,
      convert
    );
    expect(kwd.premium).toBeNull();
    expect(kwd.yearly).toBeNull();
    expect(kwd.premiumNative).toEqual({ amount: 7, currency: 'KWD' });
    expect(kwd.coverage).toBeNull();
    expect(kwd.coverageKnown).toBe(100_000);
    expect(kwd.missing).toEqual(['KWD']);
    // The limit without a rate ranks last
    expect(kwd.topLimit?.title).toBe('Úraz');
    const m = insuranceMetrics([eur, kwd], TODAY);
    expect(m.yearlyTotal).toBe(12 * 250);
    expect(m.coverageTotal).toBe(100_000);
    expect(m.nextPayment?.amount).toBe(250);
    expect(m.missing).toEqual(['KWD']);
    const cal = paymentCalendar([eur, kwd], TODAY);
    expect(cal[0]).toMatchObject({ monthly: 250, other: 0 });
  });
});

describe('insuranceRow limits', () => {
  // The raw amounts rank the JPY limit first; in CZK the EUR limit is the largest.
  const jpy = { title: 'Škoda na majetku', amount: 5_000_000, currency: 'JPY' };
  const crown = { title: 'Smrt', amount: 1_000_000, currency: 'CZK' };
  const eur = { title: 'Odpovědnost', amount: 100_000, currency: 'EUR' };

  it('picks the largest limit after conversion to CZK and counts them all', () => {
    expect(jpy.amount).toBeGreaterThan(eur.amount);
    expect(convert(eur.amount, 'EUR')!).toBeGreaterThan(convert(jpy.amount, 'JPY')!);
    expect(convert(jpy.amount, 'JPY')!).toBeLessThan(crown.amount);

    const row = insuranceRow(policy({ limits: [jpy, crown, eur] }), TODAY, convert);
    expect(row.topLimit).toEqual(eur);
    expect(row.limitCount).toBe(3);
    // the limit keeps its own currency; only the ranking is in CZK
    expect(row.topLimit?.currency).toBe('EUR');
  });

  it('keeps the first of equal limits and reads a blank currency as CZK', () => {
    const row = insuranceRow(
      policy({
        limits: [
          { title: 'První', amount: 500_000, currency: '' },
          { title: 'Druhý', amount: 500_000, currency: 'CZK' },
        ],
      }),
      TODAY,
      convert
    );
    expect(row.topLimit?.title).toBe('První');
    expect(row.limitCount).toBe(2);
  });

  it('orders all limits by their CZK value, ties in their original order', () => {
    const second = { title: 'Druhý', amount: 1_000_000, currency: 'CZK' };
    expect(limitsByValue([jpy, crown, eur, second], convert)).toEqual([eur, crown, second, jpy]);
    expect(limitsByValue(undefined, convert)).toEqual([]);
    const kwd = { title: 'Bez kurzu', amount: 9_000_000, currency: 'KWD' };
    expect(limitsByValue([kwd, jpy, eur], convert)).toEqual([eur, jpy, kwd]);
  });

  it('has no top limit for a policy without limits', () => {
    const row = insuranceRow(policy({ limits: [] }), TODAY, convert);
    expect(row.topLimit).toBeNull();
    expect(row.limitCount).toBe(0);
    expect(row.coverage).toBe(0);
  });
});

describe('paymentCalendar', () => {
  it('splits monthly and other payments per month and marks anniversaries', () => {
    const life = insuranceRow(policy({ id: 'a' }), TODAY, convert);
    const car = insuranceRow(
      policy({
        id: 'b',
        paymentFrequency: 'annually',
        regularPayment: '7800',
        startDate: day(2024, 1, 15),
      }),
      TODAY,
      convert
    );
    const cal = paymentCalendar([life, car], TODAY);
    expect(cal).toHaveLength(12);
    expect(cal[0].month).toBe(day(2026, 11, 1));
    expect(cal[0]).toMatchObject({ monthly: 1150, other: 0 });
    const january = cal.find((c) => c.month === day(2027, 1, 1))!;
    expect(january.other).toBe(7800);
    expect(january.anniversaries.map((r) => r.policy.id)).toEqual(['b']);
    expect(cal.reduce((s, c) => s + c.monthly, 0)).toBe(12 * 1150);
  });
});

describe('paidSeries', () => {
  it('accumulates the premiums and plans the next payments', () => {
    const s = paidSeries(policy({}), TODAY, 3);
    expect(s.paymentsMade).toBe(91);
    expect(s.paidToDay).toBe(91 * 1150);
    expect(s.past[0]).toEqual({ t: day(2019, 4, 1), value: 1150 });
    expect(s.past[s.past.length - 1]).toEqual({ t: TODAY, value: 91 * 1150 });
    expect(s.future).toEqual([
      { t: day(2026, 11, 1), value: 92 * 1150 },
      { t: day(2026, 12, 1), value: 93 * 1150 },
      { t: day(2027, 1, 1), value: 94 * 1150 },
    ]);
  });

  it('starts with the lump sum and has no future for a one-time policy', () => {
    const s = paidSeries(
      policy({ paymentFrequency: 'one_time', regularPayment: '0', oneTimePayment: '2400' }),
      TODAY
    );
    expect(s.paidToDay).toBe(2400);
    expect(s.future).toEqual([]);
  });
});

describe('policyToInsert', () => {
  it('turns nulls into undefined and keeps the status', () => {
    const data = policyToInsert(policy({ status: 'inactive', endDate: day(2026, 1, 1) }));
    expect(data.policyNumber).toBeUndefined();
    expect(data.endDate).toBe(day(2026, 1, 1));
    expect(data.status).toBe('inactive');
    expect(data.paymentFrequency).toBe('monthly');
  });
});
