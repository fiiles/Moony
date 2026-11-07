import {
  amortizationSchedule,
  dayFloor,
  loanAnchor,
  paymentsDueThrough,
} from '@shared/calculations/loan-amortization';
import { isEnded, paymentDays, type InsuranceRow } from '@/utils/insurance';
import type { LoanRow } from '@/utils/loans';

const DAY = 86_400;

/** How far ahead "Nejbližší platby" looks (spec §8.8's planning ruling). */
export const UPCOMING_DAYS = 90;

/** One premium falling due, in the currency of its policy. */
export interface UpcomingPremium {
  policyId: string;
  name: string;
  provider: string;
  /** The due day (UTC midnight, unix seconds). */
  day: number;
  /** The policy's regular premium as stored: never converted. */
  amount: number;
  currency: string;
}

/**
 * The regular premiums of the running recurring policies that fall due from `today` (a payment
 * due today is in) to `days` days after it (inclusive), nearest first and by policy name within a
 * day. Ended policies and one-time premiums have no payment ahead and stay out, so do policies
 * whose premium is nothing. Every amount is the policy's own (`premiumNative`): the list names
 * what leaves the account, in the currency it leaves it in, and no total is worked out from it.
 * `paymentDays` takes the days after its `from`, so it starts the day before.
 */
export function upcomingPremiums(
  rows: readonly InsuranceRow[],
  today: number,
  days = UPCOMING_DAYS
): UpcomingPremium[] {
  const from = dayFloor(today);
  const to = from + days * DAY;
  const out: UpcomingPremium[] = [];
  for (const row of rows) {
    if (row.ended || isEnded(row.policy, today)) continue;
    if (row.frequency === null || row.frequency === 'one_time') continue;
    const { amount, currency } = row.premiumNative;
    if (amount <= 0) continue;
    for (const day of paymentDays(row.policy, from - DAY, to)) {
      out.push({
        policyId: row.policy.id,
        name: row.policy.policyName,
        provider: row.policy.provider,
        day,
        amount,
        currency,
      });
    }
  }
  return out.sort(
    (a, b) => a.day - b.day || a.name.localeCompare(b.name) || a.policyId.localeCompare(b.policyId)
  );
}

/** One loan payment falling due, in the loan's currency. */
export interface UpcomingLoanPayment {
  loanId: string;
  name: string;
  /** The due day (UTC midnight, unix seconds). */
  day: number;
  /** The scheduled payment (the last one may be smaller): never converted. */
  amount: number;
  currency: string;
}

/**
 * The scheduled payments of the running loans that fall due from `today` (a payment due today is
 * in) to `days` days after it, nearest first and by loan name within a day: the plan ahead of
 * today (`row.plan`, the desktop's schedule from today's balance), so a manual balance and the
 * events are in it. Paid-off loans, loans without a payment and those whose plan has ended have
 * none. Amounts are the loan's own, as the premiums' are.
 */
export function upcomingLoanPayments(
  rows: readonly LoanRow[],
  today: number,
  days = UPCOMING_DAYS
): UpcomingLoanPayment[] {
  const from = dayFloor(today);
  const to = from + days * DAY;
  const out: UpcomingLoanPayment[] = [];
  for (const row of rows) {
    if (row.matured) continue;
    // The schedule runs from the plan's anchor (the loan's start for an estimated balance): it is
    // walked as far as the window's last payment, then cut to the window
    const maxRows = paymentsDueThrough(loanAnchor(row.plan).day, to) + 1;
    for (const step of amortizationSchedule(row.plan, maxRows).rows) {
      if (step.dueDay < from || step.dueDay > to || step.payment <= 0) continue;
      out.push({
        loanId: row.loan.id,
        name: row.loan.name,
        day: step.dueDay,
        amount: step.payment,
        currency: row.currency,
      });
    }
  }
  return out.sort(
    (a, b) => a.day - b.day || a.name.localeCompare(b.name) || a.loanId.localeCompare(b.loanId)
  );
}

/** The policy types of the desktop's list (`insurance:types`), in its order. */
const TYPE_ORDER: readonly string[] = [
  'life',
  'health',
  'property',
  'vehicle',
  'travel',
  'liability',
  'accident',
  'other',
];

/**
 * The distinct types among `rows`, in the order of `insurance:types`; a type the list does not
 * name (a policy stored by another version) follows them alphabetically, so the filter reaches
 * every policy.
 */
export function policyTypes(rows: readonly InsuranceRow[]): string[] {
  const present = new Set(rows.map((row) => row.policy.type));
  const known = TYPE_ORDER.filter((type) => present.has(type));
  const unknown = [...present].filter((type) => !TYPE_ORDER.includes(type)).sort();
  return [...known, ...unknown];
}

/**
 * The type filter in force: the remembered type while a policy still has it, else "all". A type
 * whose last policy ended or was deleted must not leave the list empty with no menu to undo it (the
 * group is offered only among two or more types); "all" is the filter that is off.
 */
export function effectiveType(types: readonly string[], remembered: string): string {
  return types.includes(remembered) ? remembered : 'all';
}
