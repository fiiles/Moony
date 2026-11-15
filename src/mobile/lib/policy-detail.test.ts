import { describe, expect, it } from 'vitest';
import type { InsurancePolicy } from '@shared/schema';
import { insuranceRow, paidSeries, type InsuranceRow } from '@/utils/insurance';
import { noticeDeadline, policyFacts, policyHero, premiumPoints } from './policy-detail';

const DAY = 86_400;
const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;
const TODAY = day(2026, 10, 24);
const convert = (amount: number) => amount;

/** A quarterly property policy, open-ended, started 9. 1. 2024: payments on the 9th of Jan, Apr, Jul, Oct. */
function policy(partial: Partial<InsurancePolicy> = {}): InsurancePolicy {
  return {
    id: 'p1',
    type: 'property',
    provider: 'Kooperativa',
    policyName: 'Pojištění domácnosti',
    policyNumber: '8540221937',
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

const rowOf = (p: InsurancePolicy): InsuranceRow => insuranceRow(p, TODAY, convert);

const ONE_TIME: Partial<InsurancePolicy> = {
  paymentFrequency: 'one_time',
  regularPayment: '0',
  oneTimePayment: '12000',
  oneTimePaymentCurrency: 'EUR',
};
const ENDED: Partial<InsurancePolicy> = { status: 'inactive', endDate: day(2025, 12, 31) };

/** The next anniversary of the quarterly fixture is 9. 1. 2027: 77 days after TODAY. */
const ANNIVERSARY = day(2027, 1, 9);
const DAYS_TO_ANNIVERSARY = (ANNIVERSARY - TODAY) / DAY;

describe('noticeDeadline', () => {
  const deadline = (p: InsurancePolicy, days: number | null) =>
    noticeDeadline(p, rowOf(p), days, TODAY);

  it('is the next anniversary less the notice days', () => {
    expect(deadline(policy(), 43)).toBe(day(2026, 11, 27));
    expect(deadline(policy(), 43)).toBe(ANNIVERSARY - 43 * DAY);
  });

  it('counts a deadline that falls today, and drops one that has passed', () => {
    expect(DAYS_TO_ANNIVERSARY).toBe(77);
    expect(deadline(policy(), 77)).toBe(TODAY);
    expect(deadline(policy(), 78)).toBeNull();
  });

  it('is null without a notice period: reminder off (null) or 0 days', () => {
    expect(deadline(policy(), null)).toBeNull();
    expect(deadline(policy(), 0)).toBeNull();
  });

  it('is null for a one-time policy, which has nothing to give notice of', () => {
    expect(deadline(policy(ONE_TIME), 43)).toBeNull();
  });

  it('is null for an ended policy, whether set inactive or past its end date', () => {
    expect(deadline(policy(ENDED), 43)).toBeNull();
    expect(deadline(policy({ endDate: day(2026, 6, 30) }), 43)).toBeNull();
  });

  it('is null when the contract ends before the next anniversary', () => {
    expect(deadline(policy({ endDate: day(2026, 12, 31) }), 43)).toBeNull();
  });

  it('counts the first anniversary of a policy that has not started a year after its start, as the milestones do', () => {
    const later = policy({ startDate: day(2026, 12, 1) });
    // 1. 12. 2027 less 43 days; the start day itself is no anniversary
    expect(deadline(later, 43)).toBe(day(2027, 10, 19));
  });
});

describe('policyFacts', () => {
  const property = { id: 're1', name: 'Byt Lipová' };
  const facts = (
    p: InsurancePolicy,
    linked: { id: string; name: string } | null = null,
    notice: number | null = null
  ) => policyFacts(p, rowOf(p), linked, notice, TODAY);
  const keys = (list: ReturnType<typeof facts>) => list.map((fact) => fact.key);

  it('lists every fact the policy has, in the order of the card', () => {
    expect(keys(facts(policy(), property, 43))).toEqual([
      'number',
      'provider',
      'type',
      'premium',
      'nextPayment',
      'term',
      'notice',
      'property',
    ]);
  });

  it('carries the values the screen words', () => {
    expect(facts(policy(), property, 43)).toEqual([
      { key: 'number', number: '8540221937' },
      { key: 'provider', provider: 'Kooperativa' },
      { key: 'type', type: 'property' },
      { key: 'premium', amount: 2150, currency: 'CZK', frequency: 'quarterly' },
      { key: 'nextPayment', day: ANNIVERSARY },
      { key: 'term', start: day(2024, 1, 9), end: null, anniversary: ANNIVERSARY },
      { key: 'notice', deadline: day(2026, 11, 27) },
      { key: 'property', id: 're1', name: 'Byt Lipová' },
    ]);
  });

  it('leaves out the number a policy has not got', () => {
    expect(keys(facts(policy({ policyNumber: null })))).not.toContain('number');
    expect(keys(facts(policy({ policyNumber: '   ' })))).not.toContain('number');
  });

  it('leaves out an insurer that is blank', () => {
    expect(keys(facts(policy({ provider: ' ' })))).not.toContain('provider');
  });

  it('leaves out the property of a policy that is linked to none', () => {
    expect(keys(facts(policy(), null))).not.toContain('property');
  });

  it('leaves out the notice deadline without a notice period or once it has passed', () => {
    expect(keys(facts(policy(), null, null))).not.toContain('notice');
    expect(keys(facts(policy(), null, 0))).not.toContain('notice');
    expect(keys(facts(policy(), null, 78))).not.toContain('notice');
  });

  it('shows a one-time premium as the one-time amount in its own currency, without a frequency', () => {
    const once = facts(policy(ONE_TIME));
    expect(once.find((fact) => fact.key === 'premium')).toEqual({
      key: 'premium',
      amount: 12000,
      currency: 'EUR',
      frequency: null,
    });
    // Nothing is due ahead of a one-time policy, nothing renews, and no notice is given of it
    expect(keys(once)).not.toContain('nextPayment');
    expect(once.find((fact) => fact.key === 'term')).toMatchObject({
      end: null,
      anniversary: null,
    });
    expect(keys(facts(policy(ONE_TIME), null, 43))).not.toContain('notice');
  });

  it('leaves out a premium of nothing', () => {
    expect(keys(facts(policy({ regularPayment: '0' })))).not.toContain('premium');
  });

  it('gives the one-time amount of a recurring policy its own row after the premium, in its own currency', () => {
    const lump = facts(policy({ oneTimePayment: '5000', oneTimePaymentCurrency: 'EUR' }));
    expect(keys(lump)).toEqual([
      'number',
      'provider',
      'type',
      'premium',
      'oneTime',
      'nextPayment',
      'term',
    ]);
    expect(lump.find((fact) => fact.key === 'oneTime')).toEqual({
      key: 'oneTime',
      amount: 5000,
      currency: 'EUR',
    });
  });

  it('reads a one-time amount with no currency recorded in the premium\u2019s currency', () => {
    const lump = facts(
      policy({
        oneTimePayment: '5000',
        oneTimePaymentCurrency: null,
        regularPaymentCurrency: 'EUR',
      })
    );
    expect(lump.find((fact) => fact.key === 'oneTime')).toMatchObject({ currency: 'EUR' });
  });

  it('has no one-time row without an amount, nor for a one-time policy whose premium is that amount', () => {
    expect(keys(facts(policy({ oneTimePayment: null })))).not.toContain('oneTime');
    expect(keys(facts(policy({ oneTimePayment: '0' })))).not.toContain('oneTime');
    expect(keys(facts(policy(ONE_TIME)))).not.toContain('oneTime');
  });

  it('has no next payment for an ended policy, whose term runs from start to end', () => {
    const ended = facts(policy(ENDED), null, 43);
    expect(keys(ended)).toEqual(['number', 'provider', 'type', 'premium', 'term']);
    expect(ended.find((fact) => fact.key === 'term')).toEqual({
      key: 'term',
      start: day(2024, 1, 9),
      end: day(2025, 12, 31),
      anniversary: null,
    });
  });

  it('gives the end of a fixed term and no anniversary: the end stands for it', () => {
    const fixed = facts(policy({ endDate: day(2027, 12, 31) }));
    expect(fixed.find((fact) => fact.key === 'term')).toEqual({
      key: 'term',
      start: day(2024, 1, 9),
      end: day(2027, 12, 31),
      anniversary: null,
    });
  });

  it('gives the anniversary of an open-ended term', () => {
    expect(facts(policy()).find((fact) => fact.key === 'term')).toMatchObject({
      end: null,
      anniversary: ANNIVERSARY,
    });
  });
});

describe('premiumPoints', () => {
  it('is the paid series, the payments so far and a year ahead, as one line', () => {
    const p = policy();
    // Quarterly: the year ahead is four payments
    const series = paidSeries(p, TODAY, 4);
    const { points } = premiumPoints(p, TODAY);
    expect(points).toEqual([...series.past, ...series.future]);
    expect(series.future).toHaveLength(4);
    // 12 payments made by today; the last of the four ahead is the 16th, 9. 10. 2027
    expect(points[points.length - 1]).toEqual({ t: day(2027, 10, 9), value: 16 * 2150 });
    const times = points.map((point) => point.t);
    expect(times).toEqual([...times].sort((a, b) => a - b));
  });

  it('never projects an open-ended policy more than a year ahead (D16)', () => {
    for (const [frequency, ahead] of [
      ['monthly', 12],
      ['semi_annually', 2],
      ['annually', 1],
    ] as const) {
      const { points } = premiumPoints(policy({ paymentFrequency: frequency }), TODAY);
      const future = points.filter((point) => point.t > TODAY);
      expect(future).toHaveLength(ahead);
      expect(future[future.length - 1].t - TODAY).toBeLessThanOrEqual(366 * DAY);
    }
  });

  it('passes what has been paid and the number of payments through', () => {
    const result = premiumPoints(policy(), TODAY);
    expect(result.paidToDay).toBe(12 * 2150);
    expect(result.paymentsMade).toBe(12);
  });

  it('leaves a one-time amount in another currency out of the sum, never adding it 1:1', () => {
    const mixed = policy({ oneTimePayment: '5000', oneTimePaymentCurrency: 'EUR' });
    const result = premiumPoints(mixed, TODAY);
    expect(result).toEqual(premiumPoints(policy(), TODAY));
    expect(result.paidToDay).toBe(12 * 2150);
    // Whichever of the two is the foreign one
    const other = policy({
      oneTimePayment: '5000',
      oneTimePaymentCurrency: 'CZK',
      regularPaymentCurrency: 'EUR',
    });
    expect(premiumPoints(other, TODAY).paidToDay).toBe(12 * 2150);
  });

  it('adds a one-time amount in the premium\u2019s currency, or with none recorded, at the start', () => {
    const same = policy({ oneTimePayment: '5000', oneTimePaymentCurrency: 'CZK' });
    expect(premiumPoints(same, TODAY).paidToDay).toBe(12 * 2150 + 5000);
    const unset = policy({ oneTimePayment: '5000', oneTimePaymentCurrency: null });
    expect(premiumPoints(unset, TODAY).paidToDay).toBe(12 * 2150 + 5000);
  });

  it('keeps the lump sum of a one-time policy, which is its own premium in its own currency', () => {
    // The regular currency is CZK, the lump sum EUR: nothing else is added to it
    const once = premiumPoints(policy(ONE_TIME), TODAY);
    expect(once.paidToDay).toBe(12000);
    expect(once.points.length).toBeGreaterThanOrEqual(2);
  });

  it('has nothing ahead of an ended policy: the line stops today', () => {
    const { points, paymentsMade } = premiumPoints(policy(ENDED), TODAY);
    expect(paymentsMade).toBe(8);
    expect(points[points.length - 1].t).toBe(TODAY);
    expect(points.every((point) => point.t <= TODAY)).toBe(true);
  });

  it('is empty for a policy that has not started, whose payments are all still ahead', () => {
    const result = premiumPoints(policy({ startDate: day(2026, 12, 1) }), TODAY);
    expect(result).toEqual({ points: [], paidToDay: 0, paymentsMade: 0 });
  });
});

describe('policyHero', () => {
  const hero = (p: InsurancePolicy) => policyHero(p, rowOf(p));

  it('leads a recurring policy with its premium and the next payment', () => {
    const p = policy();
    expect(hero(p)).toEqual({
      kind: 'recurring',
      amount: 2150,
      currency: 'CZK',
      frequency: 'quarterly',
      nextPayment: rowOf(p).nextPayment,
      ended: false,
      endDay: null,
    });
    expect(rowOf(p).nextPayment).toBe(day(2027, 1, 9));
  });

  it('names no payment for an ended policy', () => {
    const result = hero(policy(ENDED));
    expect(result).toMatchObject({ kind: 'recurring', ended: true, nextPayment: null });
  });

  it('shows a one-time policy’s lump sum in its own currency and the day it was paid', () => {
    expect(hero(policy(ONE_TIME))).toEqual({
      kind: 'oneTime',
      amount: 12000,
      currency: 'EUR',
      paidOn: day(2024, 1, 9),
      ended: false,
    });
  });

  it('says so when no premium is recorded', () => {
    expect(hero(policy({ regularPayment: '0' }))).toEqual({ kind: 'none' });
  });
});
