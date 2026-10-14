/**
 * Pure helpers for the balance section of the bank account detail (spec
 * `docs/specs/2026-10-10-bank-account-detail-redesign-design.md`): the line
 * through dated balance records and the three balance stats. Days are UTC day
 * starts in unix seconds; amounts are in the account currency.
 */

export interface BalanceRecordLike {
  amount: string | number;
  /** UTC day start (unix seconds). */
  asOf: number;
}

export interface BalanceSeriesPoint {
  t: number;
  value: number;
  /** True on a record, false on the interpolated window-start point. */
  record: boolean;
}

const DAY = 86400;
const YEAR = 365 * DAY;
const AVG_MONTH_DAYS = 30.44;

const num = (v: string | number) => Number(v) || 0;

function ascending<T extends BalanceRecordLike>(records: readonly T[]): T[] {
  return [...records].sort((a, b) => a.asOf - b.asOf);
}

export function newestRecord<T extends BalanceRecordLike>(records: readonly T[]): T | null {
  const sorted = ascending(records);
  return sorted.length ? sorted[sorted.length - 1] : null;
}

export function previousRecord<T extends BalanceRecordLike>(records: readonly T[]): T | null {
  const sorted = ascending(records);
  return sorted.length > 1 ? sorted[sorted.length - 2] : null;
}

/** Latest record on or before `day` (step semantics), null before the first record. */
export function valueAt(records: readonly BalanceRecordLike[], day: number): number | null {
  let best: BalanceRecordLike | null = null;
  for (const r of records) if (r.asOf <= day && (!best || r.asOf > best.asOf)) best = r;
  return best ? num(best.amount) : null;
}

/**
 * Points of the balance line: the year ending at the newest record on or before `today`
 * (from the first record when the history is shorter), entered at the window start by
 * the balance held from the record before it (a balance changes only at a record, so
 * the caller draws a step line). The axis the caller draws may run
 * on to `today`, so a stale newest record shows as a gap. Records dated after `today`
 * are ignored. Empty with fewer than two such records.
 */
export function balanceSeries(
  records: readonly BalanceRecordLike[],
  today: number
): BalanceSeriesPoint[] {
  const sorted = ascending(records).filter((r) => r.asOf <= today);
  if (sorted.length < 2) return [];
  const newest = sorted[sorted.length - 1];
  const start = Math.max(sorted[0].asOf, newest.asOf - YEAR);
  const inside = sorted.filter((r) => r.asOf >= start);
  const before = [...sorted].reverse().find((r) => r.asOf < start) ?? null;
  const points = inside.map((r) => ({ t: r.asOf, value: num(r.amount), record: true }));
  if (before && points.length && points[0].t !== start) {
    points.unshift({ t: start, value: num(before.amount), record: false });
  }
  return points;
}

/**
 * Newest record minus the value at 1 January of `today`'s UTC year; null without a base
 * (including when the newest record itself is the base: there is no change to show).
 */
export function changeThisYear(
  records: readonly BalanceRecordLike[],
  today: number
): { delta: number; baseDay: number } | null {
  const newest = newestRecord(records);
  if (!newest) return null;
  const jan1 = Date.UTC(new Date(today * 1000).getUTCFullYear(), 0, 1) / 1000;
  let base: BalanceRecordLike | null = null;
  for (const r of records) if (r.asOf <= jan1 && (!base || r.asOf > base.asOf)) base = r;
  if (!base || base === newest) return null;
  return { delta: num(newest.amount) - num(base.amount), baseDay: base.asOf };
}

/**
 * Average change per month: against the latest record at least a year before the
 * newest one, else against the oldest record over the months elapsed (at least one).
 * Null with fewer than two records.
 */
export function averageMonthlyChange(
  records: readonly BalanceRecordLike[]
): { perMonth: number; months: number; sinceFirst: boolean; baseDay: number } | null {
  const sorted = ascending(records);
  if (sorted.length < 2) return null;
  const newest = sorted[sorted.length - 1];
  const cutoff = newest.asOf - YEAR;
  let base: BalanceRecordLike | null = null;
  for (const r of sorted) if (r.asOf <= cutoff) base = r;
  const sinceFirst = base === null;
  if (!base) base = sorted[0];
  if (base === newest) return null;
  const months = Math.max(1, Math.round((newest.asOf - base.asOf) / DAY / AVG_MONTH_DAYS));
  return {
    perMonth: Math.round((num(newest.amount) - num(base.amount)) / months),
    months,
    sinceFirst,
    baseDay: base.asOf,
  };
}
