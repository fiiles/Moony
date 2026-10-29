import type {
  BankTransaction,
  SpendingPlan,
  SpendingPlanCategory,
  SpendingPlanDetail,
  SpendingPlanSummary,
} from '@shared/schema';
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

// ---- Plan detail (spec §9 "Detail page") -------------------------------------------------------

/** "YYYY-MM" of a booking day (UTC midnight, unix seconds), as the backend buckets months. */
export function monthOfDay(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  return monthKey(date.getUTCFullYear(), date.getUTCMonth() + 1);
}

/** "YYYY-MM" as the UTC date of its first day, for month labels. */
export function monthDate(month: string): Date {
  const [y, m] = month.split('-').map(Number);
  return new Date(Date.UTC(y, m - 1, 1));
}

/** `fmt.monthInSentence`: a UTC date with Intl options, its default year overridable. */
type MonthFormatter = (date: Date, opts: Intl.DateTimeFormatOptions) => string;

/**
 * The month labels under "Útrata v čase": the short month of the plan year alone ("led", "Jan"),
 * a month of another year with its two-digit year ("úno 27").
 */
export function planAxisLabels(axis: string[], planYear: number, format: MonthFormatter): string[] {
  return axis.map((month) => {
    // `year: undefined` drops the year monthInSentence adds by default
    const short = format(monthDate(month), { month: 'short', year: undefined });
    const year = Number(month.slice(0, 4));
    return year === planYear ? short : `${short} ${String(year).slice(2)}`;
  });
}

/** A month in a sentence or a filter badge: "září" in the plan year, "prosinec 2025" outside it. */
export function planMonthLabel(month: string, planYear: number, format: MonthFormatter): string {
  const inYear = Number(month.slice(0, 4)) === planYear;
  return format(monthDate(month), { month: 'long', year: inYear ? undefined : 'numeric' });
}

/** The category filter's key of payments without a category (a category id is never empty). */
export const UNCATEGORIZED_KEY = '';

/** A category id as a filter key; uncategorized payments get `UNCATEGORIZED_KEY`. */
export function categoryKey(categoryId: string | null): string {
  return categoryId ?? UNCATEGORIZED_KEY;
}

/**
 * Net spend of each month on `axis`, in the plan currency (0 where nothing was spent). With a
 * category key only that category's cells count. Display sums of backend-converted values.
 */
export function planMonthSpend(
  detail: Pick<SpendingPlanDetail, 'months' | 'cells'>,
  axis: string[],
  category: string | null
): number[] {
  const byMonth = new Map<string, number>();
  const add = (month: string, spent: string) =>
    byMonth.set(month, (byMonth.get(month) ?? 0) + (parseFloat(spent) || 0));
  if (category === null) {
    for (const m of detail.months) add(m.month, m.spent);
  } else {
    for (const c of detail.cells) if (categoryKey(c.categoryId) === category) add(c.month, c.spent);
  }
  return axis.map((month) => byMonth.get(month) ?? 0);
}

/** Running total of `values` for the months up to `lastMonth`; later months are null. */
export function cumulativeSpend(
  values: number[],
  axis: string[],
  lastMonth: string
): (number | null)[] {
  let total = 0;
  return axis.map((month, i) => {
    if (month > lastMonth) return null;
    total += values[i] ?? 0;
    return total;
  });
}

/** One row of the detail's Kategorie card, in the plan currency. */
export interface PlanCategoryRow {
  key: string;
  categoryId: string | null;
  spent: number;
  count: number;
}

/**
 * The plan's spend per category, the largest first: the whole plan, or with a month only that
 * month's cells.
 */
export function planCategoryRows(
  detail: Pick<SpendingPlanDetail, 'categories' | 'cells'>,
  month: string | null
): PlanCategoryRow[] {
  const rows = new Map<string, PlanCategoryRow>();
  const source = month === null ? detail.categories : detail.cells.filter((c) => c.month === month);
  for (const item of source) {
    const key = categoryKey(item.categoryId);
    const row = rows.get(key) ?? { key, categoryId: item.categoryId, spent: 0, count: 0 };
    row.spent += parseFloat(item.spent) || 0;
    row.count += item.count;
    rows.set(key, row);
  }
  return [...rows.values()].sort((a, b) => b.spent - a.spent);
}

/** A segment of the overview's 100 % bar; fits `FlowSegment` of the cashflow `FlowBar`. */
export interface PlanFlowSegment {
  key: string;
  label: string;
  value: number;
  /** Share of the bar, 0–1. */
  share: number;
  kind?: 'rest';
}

/** Categories shown on their own in the overview bar; the others fold into "Ostatní". */
const FLOW_HEAD = 4;

/**
 * "Kam jdou peníze z plánů": the year's spend by category across all plans, on a bar where
 * 100 % is the larger of planned and spent (report currency). The four categories with the
 * largest positive spend, "Ostatní" for the rest of the positive spend, then the hatched
 * remainder (planned − spent) while the plans are not used up. Zero or negative categories
 * (refunds outweighing payments) are left out.
 */
export function planFlowSegments(
  input: {
    planned: number;
    spent: number;
    categories: readonly Pick<SpendingPlanCategory, 'categoryId' | 'spent'>[];
  },
  labels: { category: (categoryId: string | null) => string; other: string; rest: string }
): PlanFlowSegment[] {
  const base = Math.max(input.planned, input.spent);
  if (!(base > 0)) return [];
  const positive = input.categories
    .map((c) => ({ categoryId: c.categoryId, value: parseFloat(c.spent) }))
    .filter((c) => Number.isFinite(c.value) && c.value > 0)
    .sort((a, b) => b.value - a.value);
  const segments: PlanFlowSegment[] = positive.slice(0, FLOW_HEAD).map((c) => ({
    key: c.categoryId === null ? 'uncategorized' : `cat:${c.categoryId}`,
    label: labels.category(c.categoryId),
    value: c.value,
    share: c.value / base,
  }));
  const tail = positive.slice(FLOW_HEAD);
  if (tail.length > 0) {
    const value = tail.reduce((sum, c) => sum + c.value, 0);
    segments.push({ key: 'other', label: labels.other, value, share: value / base });
  }
  const left = input.planned - input.spent;
  if (left > 0) {
    segments.push({
      key: 'rest',
      label: labels.rest,
      value: left,
      share: left / base,
      kind: 'rest',
    });
  }
  return segments;
}

/**
 * Signed sums of payments per currency (credits plus, debits minus), in the order the currencies
 * first appear. Native amounts are never converted on the client.
 */
export function signedTotals(
  transactions: readonly Pick<BankTransaction, 'amount' | 'type' | 'currency'>[]
): { currency: string; amount: number }[] {
  const totals = new Map<string, number>();
  for (const tx of transactions) {
    const value = Math.abs(parseFloat(tx.amount) || 0);
    const signed = tx.type === 'credit' ? value : -value;
    totals.set(tx.currency, (totals.get(tx.currency) ?? 0) + signed);
  }
  return [...totals].map(([currency, amount]) => ({ currency, amount }));
}

/** The months of `year` a period select offers: up to `currentMonth`, none for a future year. */
export function periodMonths(year: number, currentMonth: string): string[] {
  return Array.from({ length: 12 }, (_, i) => monthKey(year, i + 1)).filter(
    (month) => month <= currentMonth
  );
}

/** A period of the add-payments dialog: the plan year, one "YYYY-MM" month, or everything. */
export type PlanPeriod = 'year' | 'all' | string;

/**
 * Booking-date bounds of a period in unix seconds. `dateTo` is inclusive on the raw value, so it
 * is the last second of the last day (booking days are UTC midnights).
 */
export function periodRange(
  year: number,
  period: PlanPeriod
): { dateFrom: number | null; dateTo: number | null } {
  if (period === 'all') return { dateFrom: null, dateTo: null };
  const [y, m] = period === 'year' ? [year, null] : period.split('-').map(Number);
  const from = m === null ? Date.UTC(y, 0, 1) : Date.UTC(y, m - 1, 1);
  const next = m === null ? Date.UTC(y + 1, 0, 1) : Date.UTC(y, m, 1);
  return { dateFrom: from / 1000, dateTo: next / 1000 - 1 };
}
