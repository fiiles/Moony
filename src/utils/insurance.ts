import type { InsertInsurancePolicy, InsuranceLimit, InsurancePolicy } from '@shared/schema';
import { convertToCzK, type CurrencyCode } from '@shared/currencies';
import {
  annualizeAmount,
  parsePaymentFrequency,
  type PaymentFrequency,
} from '@shared/calculations';
import { dayFloor, dueDay } from '@shared/calculations/loan-amortization';

/** Months between two payments of a frequency; 0 for a one-time policy. */
export const PERIOD_MONTHS: Record<PaymentFrequency, number> = {
  monthly: 1,
  quarterly: 3,
  semi_annually: 6,
  annually: 12,
  one_time: 0,
};

/** Payments guard: never walk more than this many payment periods. */
const MAX_PERIODS = 1_200;

const czk = (amount: string | number | null | undefined, currency: string | null | undefined) =>
  convertToCzK(Number(amount) || 0, (currency || 'CZK') as CurrencyCode);

/** Whole calendar months from `from` to `to` (UTC days), negative when `to` is earlier. */
function monthsDiff(from: number, to: number): number {
  const a = new Date(from * 1000);
  const b = new Date(to * 1000);
  return (b.getUTCFullYear() - a.getUTCFullYear()) * 12 + (b.getUTCMonth() - a.getUTCMonth());
}

/** Inactive, or past its contractual end date. */
export function isEnded(policy: InsurancePolicy, today: number): boolean {
  const day = dayFloor(today);
  return policy.status !== 'active' || (policy.endDate !== null && dayFloor(policy.endDate) < day);
}

function periodOf(policy: InsurancePolicy): number {
  const f = parsePaymentFrequency(policy.paymentFrequency);
  return f ? PERIOD_MONTHS[f] : 0;
}

/**
 * Payment due days of a recurring policy in (from, to]: on the start date's day of
 * month every period from the start, never after the contractual end.
 */
export function paymentDays(policy: InsurancePolicy, from: number, to: number): number[] {
  const period = periodOf(policy);
  if (period === 0) return [];
  const start = dayFloor(policy.startDate);
  const end = policy.endDate !== null ? dayFloor(policy.endDate) : null;
  const last = end !== null ? Math.min(to, end) : to;
  if (last <= from && last < start) return [];
  const out: number[] = [];
  let k = Math.max(0, Math.floor(monthsDiff(start, from) / period) - 1);
  for (let i = 0; i <= MAX_PERIODS; i++, k++) {
    const due = dueDay(start, k * period);
    if (due > last) break;
    if (due > from) out.push(due);
  }
  return out;
}

/** Next payment due after `today`, or null (one-time, ended, or past the end). */
export function nextPaymentDay(policy: InsurancePolicy, today: number): number | null {
  const period = periodOf(policy);
  if (period === 0) return null;
  const day = dayFloor(today);
  const start = dayFloor(policy.startDate);
  const end = policy.endDate !== null ? dayFloor(policy.endDate) : null;
  let k = Math.max(0, Math.floor(monthsDiff(start, day) / period) - 1);
  for (let i = 0; i <= MAX_PERIODS; i++, k++) {
    const due = dueDay(start, k * period);
    if (due > day) return end !== null && due > end ? null : due;
  }
  return null;
}

/** Payments due on or before `today` (the first one on the start day). */
export function paymentsMade(policy: InsurancePolicy, today: number): number {
  const period = periodOf(policy);
  const day = dayFloor(today);
  const start = dayFloor(policy.startDate);
  if (period === 0 || day < start) return 0;
  const end = policy.endDate !== null ? dayFloor(policy.endDate) : null;
  const last = end !== null ? Math.min(day, end) : day;
  if (last < start) return 0;
  const months = monthsDiff(start, last);
  let k = Math.floor(months / period);
  while (k > 0 && dueDay(start, k * period) > last) k--;
  while (dueDay(start, (k + 1) * period) <= last) k++;
  return k + 1;
}

/** Next yearly anniversary of the start after `today`; null when the contract ends before it. */
export function nextAnniversary(policy: InsurancePolicy, today: number): number | null {
  const day = dayFloor(today);
  const start = dayFloor(policy.startDate);
  const end = policy.endDate !== null ? dayFloor(policy.endDate) : null;
  const years = Math.max(0, Math.floor(monthsDiff(start, day) / 12));
  for (let y = years; y <= years + 2; y++) {
    const due = dueDay(start, y * 12);
    if (due > day) return end !== null && due >= end ? null : due;
  }
  return null;
}

/**
 * The limit worth the most in CZK (the first one on a tie), or null without limits. Limits are in
 * their own currencies, so they are ranked by the converted value, never by the raw amount.
 */
function largestLimit(limits: readonly InsuranceLimit[]): InsuranceLimit | null {
  let top: InsuranceLimit | null = null;
  let best = -Infinity;
  for (const limit of limits) {
    const value = czk(limit.amount, limit.currency);
    if (value > best) {
      top = limit;
      best = value;
    }
  }
  return top;
}

/** One policy with the numbers the list and detail show (CZK). */
export interface InsuranceRow {
  policy: InsurancePolicy;
  frequency: PaymentFrequency | null;
  ended: boolean;
  /** One regular payment (the lump sum for a one-time policy), CZK. */
  premiumCzk: number;
  yearlyCzk: number;
  oneTimeCzk: number;
  coverageCzk: number;
  /** The largest limit by its CZK value, in its own currency; null without limits. */
  topLimit: InsuranceLimit | null;
  limitCount: number;
  nextPayment: number | null;
  anniversary: number | null;
  endDay: number | null;
}

export function insuranceRow(policy: InsurancePolicy, today: number): InsuranceRow {
  const frequency = parsePaymentFrequency(policy.paymentFrequency);
  const ended = isEnded(policy, today);
  const regularCzk = czk(policy.regularPayment, policy.regularPaymentCurrency);
  const oneTimeCzk = czk(policy.oneTimePayment, policy.oneTimePaymentCurrency);
  const limits = policy.limits ?? [];
  return {
    policy,
    frequency,
    ended,
    premiumCzk: frequency === 'one_time' ? oneTimeCzk : regularCzk,
    yearlyCzk: annualizeAmount(regularCzk, policy.paymentFrequency),
    oneTimeCzk,
    coverageCzk: limits.reduce((s, l) => s + czk(l.amount, l.currency), 0),
    topLimit: largestLimit(limits),
    limitCount: limits.length,
    nextPayment: ended ? null : nextPaymentDay(policy, today),
    anniversary: ended ? null : nextAnniversary(policy, today),
    endDay: policy.endDate !== null ? dayFloor(policy.endDate) : null,
  };
}

export interface InsuranceMetrics {
  activeCount: number;
  endedCount: number;
  yearlyTotal: number;
  coverageTotal: number;
  /** Coverage per policy type, largest first. */
  coverageByType: { type: string; czk: number }[];
  /** The nearest payment day and everything due on it. */
  nextPayment: { day: number; amountCzk: number; rows: InsuranceRow[] } | null;
  /** The nearest anniversary or contractual end among active policies. */
  nextDate: { row: InsuranceRow; day: number; kind: 'anniversary' | 'end' } | null;
}

export function insuranceMetrics(rows: readonly InsuranceRow[], today: number): InsuranceMetrics {
  const day = dayFloor(today);
  const active = rows.filter((r) => !r.ended);
  const byType = new Map<string, number>();
  for (const r of active)
    byType.set(r.policy.type, (byType.get(r.policy.type) ?? 0) + r.coverageCzk);
  const withNext = active.filter((r) => r.nextPayment !== null);
  const nextDay = withNext.length ? Math.min(...withNext.map((r) => r.nextPayment!)) : null;
  const due = nextDay === null ? [] : withNext.filter((r) => r.nextPayment === nextDay);
  const dates = active
    .flatMap((r) => {
      const out: { row: InsuranceRow; day: number; kind: 'anniversary' | 'end' }[] = [];
      if (r.anniversary !== null) out.push({ row: r, day: r.anniversary, kind: 'anniversary' });
      if (r.endDay !== null && r.endDay >= day) out.push({ row: r, day: r.endDay, kind: 'end' });
      return out;
    })
    .sort((a, b) => a.day - b.day);
  return {
    activeCount: active.length,
    endedCount: rows.length - active.length,
    yearlyTotal: active.reduce((s, r) => s + r.yearlyCzk, 0),
    coverageTotal: active.reduce((s, r) => s + r.coverageCzk, 0),
    coverageByType: [...byType.entries()]
      .map(([type, value]) => ({ type, czk: value }))
      .filter((e) => e.czk > 0)
      .sort((a, b) => b.czk - a.czk),
    nextPayment:
      nextDay === null
        ? null
        : { day: nextDay, amountCzk: due.reduce((s, r) => s + r.premiumCzk, 0), rows: due },
    nextDate: dates[0] ?? null,
  };
}

export interface CalendarMonth {
  /** UTC first day of the month. */
  month: number;
  /** Payments of monthly policies, CZK. */
  monthly: number;
  /** Payments of quarterly, semi-annual and annual policies, CZK. */
  other: number;
  anniversaries: InsuranceRow[];
}

/** The next twelve calendar months of payments, from the month after `today`. */
export function paymentCalendar(rows: readonly InsuranceRow[], today: number): CalendarMonth[] {
  const d = new Date(dayFloor(today) * 1000);
  const out: CalendarMonth[] = [];
  for (let m = 1; m <= 12; m++) {
    const monthStart = Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + m, 1) / 1000;
    const monthEnd = Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + m + 1, 1) / 1000 - 1;
    const entry: CalendarMonth = { month: monthStart, monthly: 0, other: 0, anniversaries: [] };
    for (const r of rows) {
      if (r.ended) continue;
      const count = paymentDays(r.policy, monthStart - 1, monthEnd).length;
      if (count > 0) {
        if (r.frequency === 'monthly') entry.monthly += count * r.premiumCzk;
        else entry.other += count * r.premiumCzk;
      }
      if (r.anniversary !== null && r.anniversary >= monthStart && r.anniversary <= monthEnd) {
        entry.anniversaries.push(r);
      }
    }
    out.push(entry);
  }
  return out;
}

export interface PaidPoint {
  t: number;
  value: number;
}

export interface PaidSeries {
  /** Cumulative premiums after each payment up to `today`, plus a reading today. */
  past: PaidPoint[];
  /** Cumulative total after each of the next payments (dashed in the chart). */
  future: PaidPoint[];
  paidToDay: number;
  paymentsMade: number;
}

/** Cumulative premiums in the policy's own currency: the lump sum at the start, then each payment. */
export function paidSeries(policy: InsurancePolicy, today: number, futureCount = 12): PaidSeries {
  const day = dayFloor(today);
  const start = dayFloor(policy.startDate);
  const regular = Number(policy.regularPayment) || 0;
  const oneTime = Number(policy.oneTimePayment) || 0;
  const past: PaidPoint[] = [];
  let total = 0;
  if (oneTime > 0 && start <= day) {
    total += oneTime;
    past.push({ t: start, value: total });
  }
  const made = paymentsMade(policy, day);
  const period = periodOf(policy);
  for (let k = 0; k < made; k++) {
    total += regular;
    const t = dueDay(start, k * period);
    const prev = past[past.length - 1];
    if (prev && prev.t === t) prev.value = total;
    else past.push({ t, value: total });
  }
  if (past.length === 0 || past[past.length - 1].t < day) past.push({ t: day, value: total });
  const future: PaidPoint[] = [];
  if (period > 0 && !isEnded(policy, day)) {
    const end = policy.endDate !== null ? dayFloor(policy.endDate) : null;
    let running = total;
    for (let k = made; k < made + futureCount; k++) {
      const t = dueDay(start, k * period);
      if (end !== null && t > end) break;
      running += regular;
      future.push({ t, value: running });
    }
  }
  return { past, future, paidToDay: total, paymentsMade: made };
}

/** The full update payload of a stored policy (the update command replaces every field). */
export function policyToInsert(policy: InsurancePolicy): InsertInsurancePolicy {
  return {
    type: policy.type,
    provider: policy.provider,
    policyName: policy.policyName,
    policyNumber: policy.policyNumber ?? undefined,
    startDate: policy.startDate,
    endDate: policy.endDate ?? undefined,
    paymentFrequency: parsePaymentFrequency(policy.paymentFrequency) ?? 'monthly',
    oneTimePayment: policy.oneTimePayment ?? undefined,
    oneTimePaymentCurrency: policy.oneTimePaymentCurrency ?? undefined,
    regularPayment: policy.regularPayment,
    regularPaymentCurrency: policy.regularPaymentCurrency,
    limits: policy.limits ?? [],
    notes: policy.notes ?? undefined,
    status: policy.status,
  };
}
