import type { InsurancePolicy, RealEstate } from '@shared/schema';
import type { PaymentFrequency } from '@shared/calculations';
import { dayFloor, dueDay } from '@shared/calculations/loan-amortization';
import { paidSeries, type InsuranceRow, type PaidPoint } from '@/utils/insurance';

/**
 * The policy detail's numbers as pure functions (spec §8.8 "Pojistka"): the contract facts, the
 * cancellation deadline and the premiums line. They are the desktop page's rules
 * (`pages/InsuranceDetail.tsx`) without its strings and formatting; the phone screen words and
 * masks them.
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

/** One line of the contract card; the screen turns it into a label and a value. */
export type PolicyFact =
  | { key: 'number'; number: string }
  | { key: 'provider'; provider: string }
  | { key: 'type'; type: string }
  /** `frequency` is `null` for a one-time premium (and for a frequency the app does not know). */
  | { key: 'premium'; amount: number; currency: string; frequency: PaymentFrequency | null }
  | { key: 'nextPayment'; day: number }
  /** `end` is the contractual end, `anniversary` the next one of an open-ended policy. */
  | { key: 'term'; start: number; end: number | null; anniversary: number | null }
  | { key: 'notice'; deadline: number }
  | { key: 'property'; id: string; name: string };

/**
 * The facts of the contract card in the order the card shows them: the contract number, the
 * insurer, the type, the premium (a one-time policy's lump sum, in its own currency, with no
 * frequency), the next payment, the term (the start and the end, or the start and the next
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
 */
export function premiumPoints(policy: InsurancePolicy, today: number): PremiumPoints {
  const series = paidSeries(policy, today, 12);
  const started = dayFloor(policy.startDate) <= dayFloor(today);
  return {
    points: started ? [...series.past, ...series.future] : [],
    paidToDay: series.paidToDay,
    paymentsMade: series.paymentsMade,
  };
}
