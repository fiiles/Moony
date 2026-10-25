import { describe, expect, it } from 'vitest';
import type { InsurancePolicy } from '@shared/schema';
import { insuranceRow, type InsuranceRow } from '@/utils/insurance';
import { effectiveType, policyTypes, upcomingPremiums } from './upcoming-premiums';

const DAY = 86_400;
const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;
const TODAY = day(2026, 10, 24);
/** A rate that would show at once if an amount were converted. */
const convert = (amount: number) => amount * 25;

/** A quarterly property policy in CZK: payments on the 9th of Jan, Apr, Jul and Oct. */
function policy(partial: Partial<InsurancePolicy> = {}): InsurancePolicy {
  return {
    id: 'p1',
    type: 'property',
    provider: 'Kooperativa',
    policyName: 'Pojištění domácnosti',
    policyNumber: null,
    startDate: day(2024, 1, 9),
    endDate: null,
    paymentFrequency: 'quarterly',
    oneTimePayment: null,
    oneTimePaymentCurrency: null,
    regularPayment: '2150',
    regularPaymentCurrency: 'CZK',
    limits: [],
    notes: null,
    status: 'active',
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

/** A monthly life policy in EUR, due on the 5th. */
const monthly = (partial: Partial<InsurancePolicy> = {}) =>
  policy({
    id: 'p2',
    type: 'life',
    provider: 'Allianz',
    policyName: 'Životní pojištění',
    startDate: day(2024, 3, 5),
    paymentFrequency: 'monthly',
    regularPayment: '80',
    regularPaymentCurrency: 'EUR',
    ...partial,
  });

const rowOf = (p: InsurancePolicy): InsuranceRow => insuranceRow(p, TODAY, convert);
const upcoming = (policies: InsurancePolicy[], days?: number) =>
  upcomingPremiums(policies.map(rowOf), TODAY, days);

describe('upcomingPremiums', () => {
  it('lists a monthly policy three times in 90 days, each in the policy currency', () => {
    expect(upcoming([monthly()])).toEqual([
      {
        policyId: 'p2',
        name: 'Životní pojištění',
        provider: 'Allianz',
        day: day(2026, 11, 5),
        amount: 80,
        currency: 'EUR',
      },
      {
        policyId: 'p2',
        name: 'Životní pojištění',
        provider: 'Allianz',
        day: day(2026, 12, 5),
        amount: 80,
        currency: 'EUR',
      },
      {
        policyId: 'p2',
        name: 'Životní pojištění',
        provider: 'Allianz',
        day: day(2027, 1, 5),
        amount: 80,
        currency: 'EUR',
      },
    ]);
  });

  it('never converts an amount: it is the premium as stored, in its own currency', () => {
    const [first] = upcoming([policy({ startDate: day(2024, 1, 9) })]);
    expect(first).toMatchObject({ day: day(2027, 1, 9), amount: 2150, currency: 'CZK' });
    // A blank currency is CZK, the legacy default (as everywhere else)
    const [legacy] = upcoming([policy({ regularPaymentCurrency: '' })]);
    expect(legacy.currency).toBe('CZK');
  });

  it('counts a payment due today and starts the list there', () => {
    const due = monthly({ startDate: day(2024, 1, 24) });
    const days = upcoming([due]).map((p) => p.day);
    expect(days[0]).toBe(TODAY);
    expect(days).toEqual([TODAY, day(2026, 11, 24), day(2026, 12, 24)]);
  });

  it('leaves out a payment due yesterday', () => {
    const days = upcoming([monthly({ startDate: day(2024, 1, 23) })]).map((p) => p.day);
    expect(days[0]).toBe(day(2026, 11, 23));
  });

  it('reaches to the 90th day, inclusive, and not a day further', () => {
    expect(day(2027, 1, 22)).toBe(TODAY + 90 * DAY);
    const lastDay = upcoming([monthly({ startDate: day(2024, 1, 22) })]).map((p) => p.day);
    expect(lastDay[lastDay.length - 1]).toBe(TODAY + 90 * DAY);
    const dayAfter = upcoming([monthly({ startDate: day(2024, 1, 23) })]).map((p) => p.day);
    expect(dayAfter[dayAfter.length - 1]).toBe(day(2026, 12, 23));
  });

  it('takes the number of days as a parameter', () => {
    expect(upcoming([monthly()], 30).map((p) => p.day)).toEqual([day(2026, 11, 5)]);
    expect(upcoming([monthly()], 0)).toEqual([]);
  });

  it('is sorted by day, then by name, whatever the order of the policies', () => {
    const a = monthly({ id: 'a', policyName: 'Auto' });
    const b = monthly({ id: 'b', policyName: 'Byt' });
    const quarterly = policy({ id: 'q', policyName: 'Domácnost', startDate: day(2024, 1, 5) });
    const order = (list: InsurancePolicy[]) =>
      upcoming(list).map((p) => `${p.day / DAY - TODAY / DAY}:${p.name}`);
    const expected = [
      '12:Auto',
      '12:Byt',
      '42:Auto',
      '42:Byt',
      '73:Auto',
      '73:Byt',
      // The quarterly policy is due on 5. 1. 2027 as well, and sorts by its name there
      '73:Domácnost',
    ];
    // 5. 1. 2027 is 73 days away: the three policies share that day
    expect(order([b, a, quarterly])).toEqual(expected);
    expect(order([quarterly, a, b])).toEqual(expected);
  });

  it('leaves out ended policies: inactive, or past the contractual end', () => {
    expect(upcoming([monthly({ status: 'inactive' })])).toEqual([]);
    expect(upcoming([monthly({ endDate: day(2026, 6, 30) })])).toEqual([]);
  });

  it('never lists a payment after the contractual end', () => {
    const days = upcoming([monthly({ endDate: day(2026, 12, 31) })]).map((p) => p.day);
    expect(days).toEqual([day(2026, 11, 5), day(2026, 12, 5)]);
  });

  it('leaves out a one-time policy and a frequency it does not know', () => {
    expect(
      upcoming([
        policy({
          paymentFrequency: 'one_time',
          regularPayment: '0',
          oneTimePayment: '12000',
          oneTimePaymentCurrency: 'EUR',
        }),
        policy({ id: 'x', paymentFrequency: 'weekly' }),
      ])
    ).toEqual([]);
  });

  it('leaves out a payment of nothing', () => {
    expect(upcoming([monthly({ regularPayment: '0' })])).toEqual([]);
  });

  it('is empty without policies', () => {
    expect(upcomingPremiums([], TODAY)).toEqual([]);
  });

  it('rereads the end of a policy at the day it is asked for, not when its row was made', () => {
    const ending = rowOf(monthly({ endDate: day(2026, 11, 30) }));
    expect(upcomingPremiums([ending], TODAY).map((p) => p.day)).toEqual([day(2026, 11, 5)]);
    expect(upcomingPremiums([ending], day(2026, 12, 15))).toEqual([]);
  });

  it('works through a day that is not a midnight', () => {
    const noon = TODAY + 12 * 3600;
    expect(upcomingPremiums([rowOf(monthly({ startDate: day(2024, 1, 24) }))], noon)[0].day).toBe(
      TODAY
    );
  });
});

describe('policyTypes', () => {
  const typed = (...types: string[]) =>
    types.map((type, i) => rowOf(policy({ id: `p${i}`, type })));

  it('is the distinct types present, in the order of the desktop list', () => {
    expect(
      policyTypes(typed('other', 'vehicle', 'life', 'vehicle', 'accident', 'property'))
    ).toEqual(['life', 'property', 'vehicle', 'accident', 'other']);
  });

  it('names every type of the desktop list in its order', () => {
    expect(
      policyTypes(
        typed('other', 'accident', 'liability', 'travel', 'vehicle', 'property', 'health', 'life')
      )
    ).toEqual([
      'life',
      'health',
      'property',
      'vehicle',
      'travel',
      'liability',
      'accident',
      'other',
    ]);
  });

  it('puts a type the app does not know last, so no policy is out of reach of the filter', () => {
    expect(policyTypes(typed('zebra', 'life', 'alpha', 'other'))).toEqual([
      'life',
      'other',
      'alpha',
      'zebra',
    ]);
  });

  it('is empty without policies', () => {
    expect(policyTypes([])).toEqual([]);
  });
});

describe('effectiveType', () => {
  const present = ['life', 'vehicle', 'other'];

  it('keeps "all", the filter that is off', () => {
    expect(effectiveType(present, 'all')).toBe('all');
    expect(effectiveType([], 'all')).toBe('all');
  });

  it('keeps a remembered type that a policy still has', () => {
    expect(effectiveType(present, 'vehicle')).toBe('vehicle');
    expect(effectiveType(present, 'other')).toBe('other');
  });

  it('is "all" for a remembered type that has gone, so the list is never empty with no way back', () => {
    expect(effectiveType(present, 'property')).toBe('all');
    expect(effectiveType(['life'], 'vehicle')).toBe('all');
    expect(effectiveType([], 'life')).toBe('all');
  });

  it('is "all" for anything that is not a type (a value left behind by another version)', () => {
    expect(effectiveType(present, '')).toBe('all');
    expect(effectiveType(present, 'Life')).toBe('all');
  });
});
