import type { SpendingPlan, SpendingPlanSummary } from '@shared/schema';
import { matchesSearch } from './holdings-search';

/** A plan counts as "near" once spent reaches this share of the planned amount (and is not over). */
export const NEAR_THRESHOLD = 0.9;

export type PlanStatus = 'over' | 'near' | 'ok' | 'empty' | 'unknown';

/**
 * Status of one plan. `paymentCount === 0` is `empty` even when spent is 0; `spent === null`
 * (a value that could not be converted) is `unknown`, never a guessed 0.
 */
export function planStatus(
  spent: number | null,
  planned: number,
  paymentCount: number
): PlanStatus {
  if (paymentCount === 0) return 'empty';
  if (spent === null) return 'unknown';
  if (spent > planned) return 'over';
  if (planned > 0 && spent / planned >= NEAR_THRESHOLD) return 'near';
  return 'ok';
}

/** Lower rank comes first among active plans: the riskiest status leads. */
const STATUS_RANK: Record<PlanStatus, number> = {
  over: 0,
  near: 1,
  unknown: 2,
  ok: 3,
  empty: 4,
};

/**
 * Orders plans for the list: active before closed, then by status risk (over, near, unknown,
 * ok, empty), then by ratio descending. Ties keep their input order, so the caller's name sort
 * decides the rest. Returns a new array.
 */
export function sortPlansByRisk<T>(
  rows: T[],
  key: (row: T) => { status: PlanStatus; ratio: number; closed: boolean }
): T[] {
  return rows
    .map((row, index) => ({ row, index, info: key(row) }))
    .sort((a, b) => {
      if (a.info.closed !== b.info.closed) return a.info.closed ? 1 : -1;
      const byStatus = STATUS_RANK[a.info.status] - STATUS_RANK[b.info.status];
      if (byStatus !== 0) return byStatus;
      if (a.info.ratio !== b.info.ratio) return b.info.ratio - a.info.ratio;
      return a.index - b.index;
    })
    .map(({ row }) => row);
}

function monthKey(year: number, month: number): string {
  return `${year}-${String(month).padStart(2, '0')}`;
}

/**
 * Contiguous "YYYY-MM" months for the plan timeline: all twelve months of `year`, widened to
 * include any earlier or later month that already has payments.
 */
export function planMonthAxis(year: number, monthsWithPayments: string[]): string[] {
  let start = `${year}-01`;
  let end = `${year}-12`;
  for (const month of monthsWithPayments) {
    if (month < start) start = month;
    if (month > end) end = month;
  }

  const [startYear, startMonth] = start.split('-').map(Number);
  const [endYear, endMonth] = end.split('-').map(Number);
  const axis: string[] = [];
  let y = startYear;
  let m = startMonth;
  while (y < endYear || (y === endYear && m <= endMonth)) {
    axis.push(monthKey(y, m));
    m += 1;
    if (m > 12) {
      m = 1;
      y += 1;
    }
  }
  return axis;
}

/** Plan-name search: case- and diacritic-insensitive, every word of the query must match. */
export function matchesPlanSearch(name: string, query: string): boolean {
  return matchesSearch(query, [name]);
}

/** Plan-name equality as the backend checks it: trimmed and Unicode lower-cased (diacritics kept). */
export function samePlanName(a: string, b: string): boolean {
  return a.trim().toLowerCase() === b.trim().toLowerCase();
}

/** One overview row: the summary with its numbers parsed, all in the plan currency. */
export interface PlanRow {
  summary: SpendingPlanSummary;
  planned: number;
  /** Null when no payment could be converted (never a guessed 0). */
  spent: number | null;
  /** spent / planned; null without a spend. */
  ratio: number | null;
  /** planned − spent (negative when over); null without a spend. */
  left: number | null;
  status: PlanStatus;
  closed: boolean;
}

export function planRow(summary: SpendingPlanSummary): PlanRow {
  const planned = parseFloat(summary.plan.amount) || 0;
  const spent = summary.spent === null ? null : parseFloat(summary.spent);
  return {
    summary,
    planned,
    spent,
    ratio: spent === null ? null : planned > 0 ? spent / planned : 0,
    left: spent === null ? null : planned - spent,
    status: planStatus(spent, planned, summary.paymentCount),
    closed: summary.plan.closedAt !== null,
  };
}

export type PlanSegment = 'active' | 'closed' | 'all';

/**
 * The overview table's rows: the segment, the name search, then the risk order (closed last,
 * ties by name in `locale`).
 */
export function overviewRows(
  rows: PlanRow[],
  segment: PlanSegment,
  query: string,
  locale: string
): PlanRow[] {
  const shown = rows.filter(
    (row) =>
      (segment === 'all' || row.closed === (segment === 'closed')) &&
      matchesPlanSearch(row.summary.plan.name, query)
  );
  const byName = [...shown].sort((a, b) =>
    a.summary.plan.name.localeCompare(b.summary.plan.name, locale)
  );
  return sortPlansByRisk(byName, (row) => ({
    status: row.status,
    ratio: row.ratio ?? 0,
    closed: row.closed,
  }));
}

/** One plan offered for the take-over into the next year. */
export interface TakeOverRow {
  plan: SpendingPlan;
  /** A plan of the same name already exists in the target year (spec C6: cannot be chosen). */
  exists: boolean;
  /** Preselected: open and not existing yet. */
  selected: boolean;
}

/** The source year's plans for the take-over: open ones first, then by name in `locale`. */
export function takeOverRows(
  source: SpendingPlan[],
  targetNames: string[],
  locale: string
): TakeOverRow[] {
  return [...source]
    .sort(
      (a, b) =>
        Number(a.closedAt !== null) - Number(b.closedAt !== null) ||
        a.name.localeCompare(b.name, locale)
    )
    .map((plan) => {
      const exists = targetNames.some((name) => samePlanName(name, plan.name));
      return { plan, exists, selected: !exists && plan.closedAt === null };
    });
}
