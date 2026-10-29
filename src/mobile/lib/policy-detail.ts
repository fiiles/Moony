import type { InsurancePolicy, RealEstate } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import { parsePaymentFrequency, type PaymentFrequency } from '@shared/calculations';
import { dayFloor, dueDay } from '@shared/calculations/loan-amortization';
import { paidSeries, type InsuranceRow, type PaidPoint } from '@/utils/insurance';

/**
 * The policy detail's numbers as pure functions (spec §8.8 "Pojistka"): the contract facts, the
 * cancellation deadline and the premiums line. They are the desktop page's rules
 * (`pages/InsuranceDetail.tsx`) without its strings and formatting; the phone screen words
 * them.
 */

const DAY = 86_400;

/**
 * The next anniversary the milestones count (`services/milestones/insurance.rs`): never the start
 * day itself, so a policy that has not started has its first one a year after it. The desktop's
 * `row.anniversary` is the start day there.
 */
function anniversaryOf(policy: InsurancePolicy, row: InsuranceRow): number | null {
  const start = dayFloor(policy.startDate);
  if (row.anniversary === null || row.anniversary > start) return row.anniversary;
  const first = dueDay(start, 12);
  return row.endDay !== null && first >= row.endDay ? null : first;
}

/**
 * The last day to give notice: the next anniversary less the profile's notice days, the date the
 * Přehled reminder counts to (`insurance_anniversary`). `null` for a one-time policy (nothing
 * renews), an ended one, a contract that ends before its next anniversary, a profile without a
 * notice period (`null`, or 0 days: the reminder is off) and a deadline that has already passed.
 * Today still counts.
 */
export function noticeDeadline(
  policy: InsurancePolicy,
  row: InsuranceRow,
  noticeDays: number | null | undefined,
  today: number
): number | null {
  if (!noticeDays || noticeDays <= 0) return null;
  if (row.ended || row.frequency === 'one_time') return null;
  const anniversary = anniversaryOf(policy, row);
  if (anniversary === null) return null;
  const deadline = anniversary - noticeDays * DAY;
  return deadline >= dayFloor(today) ? deadline : null;
}

/**
 * The one-time amount a recurring policy keeps next to its regular premium, in its own currency
 * (the premium's when none is recorded: the amount cannot be told otherwise); `null` without one.
 * A one-time policy has none of its own: its premium is that amount.
 */
function oneTimeOf(
  policy: InsurancePolicy,
  frequency: PaymentFrequency | null
): { amount: number; currency: string } | null {
  const amount = Number(policy.oneTimePayment) || 0;
  if (frequency === 'one_time' || amount <= 0) return null;
  return {
    amount,
    currency: recordCurrency(policy.oneTimePaymentCurrency || policy.regularPaymentCurrency),
  };
}

/** One line of the contract card; the screen turns it into a label and a value. */
export type PolicyFact =
  | { key: 'number'; number: string }
  | { key: 'provider'; provider: string }
  | { key: 'type'; type: string }
  /** `frequency` is `null` for a one-time premium (and for a frequency the app does not know). */
  | { key: 'premium'; amount: number; currency: string; frequency: PaymentFrequency | null }
  /** The one-time amount a recurring policy also keeps, in its own currency. */
  | { key: 'oneTime'; amount: number; currency: string }
  | { key: 'nextPayment'; day: number }
  /** `end` is the contractual end, `anniversary` the next one of an open-ended policy. */
  | { key: 'term'; start: number; end: number | null; anniversary: number | null }
  | { key: 'notice'; deadline: number }
  | { key: 'property'; id: string; name: string };

/**
 * The facts of the contract card in the order the card shows them: the contract number, the
 * insurer, the type, the premium (a one-time policy's lump sum, in its own currency, with no
 * frequency), the one-time amount a recurring policy also keeps (its own row, in its own currency),
 * the next payment, the term (the start and the end, or the start and the next
 * anniversary of an open-ended policy), the cancellation deadline and the insured property. A
 * fact that is not known is left out: no number, no premium, no payment ahead (an ended or a
 * one-time policy), no deadline, no linked property.
 */
export function policyFacts(
  policy: InsurancePolicy,
  row: InsuranceRow,
  property: Pick<RealEstate, 'id' | 'name'> | null | undefined,
  noticeDays: number | null | undefined,
  today: number
): PolicyFact[] {
  const facts: PolicyFact[] = [];
  const number = policy.policyNumber?.trim();
  if (number) facts.push({ key: 'number', number });
  const provider = policy.provider.trim();
  if (provider) facts.push({ key: 'provider', provider });
  facts.push({ key: 'type', type: policy.type });

  const { amount, currency } = row.premiumNative;
  if (amount > 0) {
    facts.push({
      key: 'premium',
      amount,
      currency,
      frequency: row.frequency === 'one_time' ? null : row.frequency,
    });
  }
  // A recurring policy may keep a one-time amount too; a one-time policy's premium is that amount
  const lump = oneTimeOf(policy, row.frequency);
  if (lump) facts.push({ key: 'oneTime', ...lump });
  if (!row.ended && row.nextPayment !== null) {
    facts.push({ key: 'nextPayment', day: row.nextPayment });
  }
  facts.push({
    key: 'term',
    start: dayFloor(policy.startDate),
    end: row.endDay,
    // A fixed term is told by its end; the anniversary is for the open-ended one that renews
    anniversary:
      row.endDay === null && !row.ended && row.frequency !== 'one_time'
        ? anniversaryOf(policy, row)
        : null,
  });
  const deadline = noticeDeadline(policy, row, noticeDays, today);
  if (deadline !== null) facts.push({ key: 'notice', deadline });
  if (property) facts.push({ key: 'property', id: property.id, name: property.name });
  return facts;
}

export interface PremiumPoints {
  /** Cumulative premiums: the payments made and today, then the next twelve (the plan). */
  points: PaidPoint[];
  paidToDay: number;
  paymentsMade: number;
}

/**
 * The cumulative-premiums line of the detail: `paidSeries` (the payments made, a reading today and
 * the next twelve payments) as one series, in the policy's own currency. A policy that has not
 * started has paid nothing and has no line yet.
 *
 * `paidSeries` adds a one-time amount to the regular premiums as plain numbers. A recurring
 * policy whose one-time amount is in another currency than its premium would be summed 1:1, so the
 * series is built without that amount (the contract card shows it as its own row); a one-time
 * policy's lump sum is its whole premium and stays, in its own currency.
 */
export function premiumPoints(policy: InsurancePolicy, today: number): PremiumPoints {
  const lump = oneTimeOf(policy, parsePaymentFrequency(policy.paymentFrequency));
  const foreign = lump !== null && lump.currency !== recordCurrency(policy.regularPaymentCurrency);
  const series = paidSeries(foreign ? { ...policy, oneTimePayment: null } : policy, today, 12);
  const started = dayFloor(policy.startDate) <= dayFloor(today);
  return {
    points: started ? [...series.past, ...series.future] : [],
    paidToDay: series.paidToDay,
    paymentsMade: series.paymentsMade,
  };
}
