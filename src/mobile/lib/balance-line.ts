import type { LinePoint } from '@/components/charts/MoonyLineChart';

interface BalanceRecordPoint {
  id: string;
  amount: string | number;
  /** UTC day start (unix seconds). */
  asOf: number;
}

/**
 * The account detail hero's step line (spec §8.4, "the balance-record line with ○ marks"): a balance
 * changes only at a record, so the line steps from record to record and holds the newest one until
 * today. A window `start` enters with the balance held from the record before it (no mark there);
 * `undefined` is the whole history. Records dated after today are ignored. The marks are the
 * records inside the window.
 */
export function balanceLine(
  records: readonly BalanceRecordPoint[],
  todayDay: number,
  start: number | undefined
): { points: LinePoint[]; marks: { id: string; t: number }[] } {
  const sorted = records.filter((r) => r.asOf <= todayDay).sort((a, b) => a.asOf - b.asOf);
  const inside = start === undefined ? sorted : sorted.filter((r) => r.asOf >= start);
  const before = start === undefined ? [] : sorted.filter((r) => r.asOf < start);
  const held = before[before.length - 1];

  const points: LinePoint[] = inside.map((r) => ({ t: r.asOf, value: Number(r.amount) || 0 }));
  if (held && start !== undefined && points[0]?.t !== start) {
    points.unshift({ t: start, value: Number(held.amount) || 0 });
  }
  const last = points[points.length - 1];
  if (last && last.t !== todayDay) points.push({ t: todayDay, value: last.value });

  return { points, marks: inside.map((r) => ({ id: r.id, t: r.asOf })) };
}
