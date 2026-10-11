/**
 * Pure helpers for the stocks analysis page: period bounds, custom range validation, date axis
 * ticks, drawdown, flow buckets, comparison groups, chart series specs and concentration.
 *
 * All timestamps are unix seconds at UTC midnight (ADR 0008); calendar maths uses the UTC
 * accessors only, never local time.
 */
import type { StockTag, StockTagGroup } from '@shared/schema';

export const DAY = 86_400;

export type PeriodPreset = '3m' | 'ytd' | '1y' | '3y' | 'all' | 'custom';
export const PERIOD_PRESETS: readonly PeriodPreset[] = ['3m', 'ytd', '1y', '3y', 'all', 'custom'];

/** Start of "Vše": the backend clamps it to the first recorded day. */
export const ALL_FROM = Date.UTC(2000, 0, 1) / 1000;

export interface DayRange {
  from: number;
  to: number;
}

// ---------------------------------------------------------------------------
// Days and periods
// ---------------------------------------------------------------------------

/** Floor a timestamp to UTC midnight. */
export function utcDay(ts: number): number {
  return Math.floor(ts / DAY) * DAY;
}

/** `YYYY-MM-DD` of the UTC day containing `ts`. */
export function isoDay(ts: number): string {
  const date = new Date(ts * 1000);
  const year = String(date.getUTCFullYear()).padStart(4, '0');
  const month = String(date.getUTCMonth() + 1).padStart(2, '0');
  const day = String(date.getUTCDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

const ISO_DAY = /^(\d{4})-(\d{2})-(\d{2})$/;

/** Strict `YYYY-MM-DD` to UTC midnight; `null` for anything else (including 2026-02-30). */
export function dayFromIso(iso: string): number | null {
  const match = ISO_DAY.exec(iso);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const ms = Date.UTC(year, month - 1, day);
  const date = new Date(ms);
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day
  ) {
    return null;
  }
  return ms / 1000;
}

/** Same calendar day `months` months earlier (UTC); a day that does not exist there is clamped. */
function monthsBack(ts: number, months: number): number {
  const date = new Date(ts * 1000);
  const year = date.getUTCFullYear();
  const month = date.getUTCMonth() - months;
  const lastDay = new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
  return Date.UTC(year, month, Math.min(date.getUTCDate(), lastDay)) / 1000;
}

/** Bounds of a preset period ending today; `from` is at most `to − DAY`. */
export function presetRange(preset: Exclude<PeriodPreset, 'custom'>, today: number): DayRange {
  const to = utcDay(today);
  let from: number;
  switch (preset) {
    case '3m':
      from = monthsBack(to, 3);
      break;
    case 'ytd':
      from = Date.UTC(new Date(to * 1000).getUTCFullYear(), 0, 1) / 1000;
      break;
    case '1y':
      from = monthsBack(to, 12);
      break;
    case '3y':
      from = monthsBack(to, 36);
      break;
    case 'all':
      from = ALL_FROM;
      break;
  }
  return { from: Math.min(from, to - DAY), to };
}

export type RangeError = 'missing' | 'order';

/**
 * Validate the two custom date inputs. Both are clamped to `[min, max]`; the range must then span
 * at least two days (`from ≤ to − 2·DAY`).
 */
export function parseCustomRange(
  fromIso: string,
  toIso: string,
  min: number,
  max: number
): { range: DayRange } | { error: RangeError } {
  const rawFrom = dayFromIso(fromIso);
  const rawTo = dayFromIso(toIso);
  if (rawFrom === null || rawTo === null) return { error: 'missing' };
  const from = Math.min(Math.max(rawFrom, min), max);
  const to = Math.min(Math.max(rawTo, min), max);
  if (from > to - 2 * DAY) return { error: 'order' };
  return { range: { from, to } };
}

// ---------------------------------------------------------------------------
// Date axis
// ---------------------------------------------------------------------------

export type AxisTickKind = 'day' | 'month' | 'quarter' | 'year';

export interface AxisTick {
  t: number;
  kind: AxisTickKind;
  /** The tick is January 1. */
  yearStart: boolean;
}

/**
 * Ticks strictly inside `(t0, t1)`: the 1st and 15th of every month up to 110 days, month starts
 * up to 420 days, quarter starts (Jan/Apr/Jul/Oct) up to 1300 days, January 1 beyond.
 */
export function dateAxisTicks(t0: number, t1: number): AxisTick[] {
  if (!(t1 > t0)) return [];
  const spanDays = (t1 - t0) / DAY;
  const kind: AxisTickKind =
    spanDays <= 110 ? 'day' : spanDays <= 420 ? 'month' : spanDays <= 1300 ? 'quarter' : 'year';
  const firstYear = new Date(t0 * 1000).getUTCFullYear();
  const lastYear = new Date(t1 * 1000).getUTCFullYear();
  const out: AxisTick[] = [];
  const push = (year: number, month: number, day: number) => {
    const t = Date.UTC(year, month, day) / 1000;
    if (t > t0 && t < t1) out.push({ t, kind, yearStart: month === 0 && day === 1 });
  };
  for (let year = firstYear; year <= lastYear; year++) {
    if (kind === 'year') {
      push(year, 0, 1);
      continue;
    }
    for (let month = 0; month < 12; month++) {
      if (kind === 'day') {
        push(year, month, 1);
        push(year, month, 15);
      } else if (kind === 'month') {
        push(year, month, 1);
      } else if (month % 3 === 0) {
        push(year, month, 1);
      }
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Drawdown
// ---------------------------------------------------------------------------

/**
 * Drawdown in percent from the running peak of `100 + twr` (`(index / peak − 1) · 100`);
 * `null` stays `null` and does not touch the peak.
 */
export function drawdown(twr: readonly (number | null)[]): (number | null)[] {
  let peak = -Infinity;
  return twr.map((value) => {
    if (value === null) return null;
    const index = 100 + value;
    peak = Math.max(peak, index);
    return peak > 0 ? (index / peak - 1) * 100 : 0;
  });
}

/** Deepest drawdown up to and including `index` (≤ 0). */
export function maxDrawdownTo(dd: readonly (number | null)[], index: number): number {
  let min = 0;
  const last = Math.min(index, dd.length - 1);
  for (let i = 0; i <= last; i++) {
    const value = dd[i];
    if (value !== null && value < min) min = value;
  }
  return min;
}

// ---------------------------------------------------------------------------
// Deposits versus market buckets
// ---------------------------------------------------------------------------

export type BucketKind = 'week' | 'month' | 'quarter' | 'year';

export function bucketKind(days: number): BucketKind {
  if (days <= 100) return 'week';
  if (days <= 450) return 'month';
  if (days <= 1300) return 'quarter';
  return 'year';
}

export interface FlowBucket {
  /** Date of the bucket's first point (labels a month, quarter or year by it). */
  start: number;
  /** Date of the bucket's last point. */
  end: number;
  deposits: number;
  market: number;
}

function bucketKey(ts: number, first: number, kind: BucketKind): number {
  if (kind === 'week') return Math.floor((ts - first) / (7 * DAY));
  const date = new Date(ts * 1000);
  const year = date.getUTCFullYear();
  if (kind === 'month') return year * 12 + date.getUTCMonth();
  if (kind === 'quarter') return year * 4 + Math.floor(date.getUTCMonth() / 3);
  return year;
}

/**
 * Split the value change of a series into net deposits and market movement per bucket.
 *
 * Points `1…n−1` are grouped by 7-day block from `dates[0]` (week) or by calendar month, quarter
 * or year; point 0 only serves as the base of the first bucket. A bucket's base is the point
 * immediately before its first point. `deposits = netFlows[last] − netFlows[base]` and
 * `market = values[last] − values[base] − deposits`. A bucket whose first, last or base value is
 * `null` is dropped.
 */
export function flowBuckets(
  dates: readonly number[],
  values: readonly (number | null)[],
  netFlows: readonly number[],
  kind: BucketKind
): FlowBucket[] {
  const n = dates.length;
  const out: FlowBucket[] = [];
  let i = 1;
  while (i < n) {
    const key = bucketKey(dates[i], dates[0], kind);
    const first = i;
    let last = i;
    while (last + 1 < n && bucketKey(dates[last + 1], dates[0], kind) === key) last++;
    const base = first - 1;
    const firstValue = values[first];
    const lastValue = values[last];
    const baseValue = values[base];
    if (firstValue != null && lastValue != null && baseValue != null) {
      const deposits = netFlows[last] - netFlows[base];
      out.push({
        start: dates[first],
        end: dates[last],
        deposits,
        market: lastValue - baseValue - deposits,
      });
    }
    i = last + 1;
  }
  return out;
}

// ---------------------------------------------------------------------------
// Comparison groups
// ---------------------------------------------------------------------------

export interface PositionLite {
  id: string;
  sector: string | null;
  currency: string;
  tags: readonly Pick<StockTag, 'id' | 'groupId'>[];
}

export type CompareGroup =
  | { kind: 'tags'; key: string; group: StockTagGroup; tags: StockTag[] }
  | { kind: 'sector'; key: 'sector' }
  | { kind: 'currency'; key: 'currency' };

function byName(a: { name: string }, b: { name: string }): number {
  return a.name.localeCompare(b.name);
}

/**
 * Ways to split the portfolio: tag groups that have at least one tag (by creation date), the
 * sector when the held positions have two or more distinct sector values (a missing sector counts
 * as one value), the currency when they have two or more currencies.
 */
export function compareGroups(
  groups: readonly StockTagGroup[],
  tags: readonly StockTag[],
  positions: readonly PositionLite[]
): CompareGroup[] {
  const out: CompareGroup[] = [];
  const ordered = [...groups].sort((a, b) => a.createdAt - b.createdAt || byName(a, b));
  for (const group of ordered) {
    const groupTags = tags.filter((t) => t.groupId === group.id).sort(byName);
    if (groupTags.length > 0) {
      out.push({ kind: 'tags', key: `group:${group.id}`, group, tags: groupTags });
    }
  }
  if (new Set(positions.map((p) => p.sector)).size >= 2)
    out.push({ kind: 'sector', key: 'sector' });
  if (new Set(positions.map((p) => p.currency)).size >= 2) {
    out.push({ kind: 'currency', key: 'currency' });
  }
  return out;
}

/** The part of `g` a position belongs to. */
export function partKey(p: PositionLite, g: CompareGroup): string {
  if (g.kind === 'tags') {
    const tag = p.tags.find((t) => t.groupId === g.group.id);
    return tag ? `tag:${tag.id}` : `none:${g.group.id}`;
  }
  if (g.kind === 'sector') return `sector:${p.sector ?? ''}`;
  return `currency:${p.currency}`;
}

export interface Part {
  key: string;
  positionIds: string[];
  tag: StockTag | null;
  sector: string | null;
  currency: string | null;
  isNone: boolean;
}

/**
 * The non-empty parts of `g`: tags in group order (by name), sectors and currencies
 * alphabetical; the part without a tag or sector last.
 */
export function groupParts(
  positions: readonly PositionLite[],
  g: CompareGroup,
  tags: readonly StockTag[]
): Part[] {
  const members = new Map<string, string[]>();
  for (const p of positions) {
    const key = partKey(p, g);
    const ids = members.get(key);
    if (ids) ids.push(p.id);
    else members.set(key, [p.id]);
  }
  const parts: Part[] = [];
  const add = (key: string, fields: Pick<Part, 'tag' | 'sector' | 'currency' | 'isNone'>) => {
    const positionIds = members.get(key);
    if (positionIds) parts.push({ key, positionIds, ...fields });
  };

  if (g.kind === 'tags') {
    for (const groupTag of g.tags) {
      const tag = tags.find((t) => t.id === groupTag.id) ?? groupTag;
      add(`tag:${tag.id}`, { tag, sector: null, currency: null, isNone: false });
    }
    add(`none:${g.group.id}`, { tag: null, sector: null, currency: null, isNone: true });
  } else if (g.kind === 'sector') {
    const names = [
      ...new Set(positions.map((p) => p.sector).filter((s): s is string => s !== null)),
    ];
    names.sort((a, b) => a.localeCompare(b));
    for (const sector of names) {
      add(`sector:${sector}`, { tag: null, sector, currency: null, isNone: false });
    }
    add('sector:', { tag: null, sector: null, currency: null, isNone: true });
  } else {
    const codes = [...new Set(positions.map((p) => p.currency))].sort((a, b) => a.localeCompare(b));
    for (const currency of codes) {
      add(`currency:${currency}`, { tag: null, sector: null, currency, isNone: false });
    }
  }
  return parts;
}

export interface SeriesSpecLite {
  key: string;
  investmentIds: string[];
  currency?: string | null;
}

/**
 * Series to request from the backend: one per tag carried by a selected position (`tag:<id>`),
 * plus the parts of `g` that are not tag parts (`none:…`, `sector:…`, `currency:…`), and for a
 * currency part whose code differs from `reportCurrency` also `<key>:native` computed in that
 * currency. Keys are unique, the order is stable.
 */
export function seriesSpecs(
  selected: readonly PositionLite[],
  tags: readonly StockTag[],
  g: CompareGroup | null,
  reportCurrency: string
): SeriesSpecLite[] {
  const specs = new Map<string, SeriesSpecLite>();
  const add = (spec: SeriesSpecLite) => {
    if (!specs.has(spec.key)) specs.set(spec.key, spec);
  };

  for (const tag of tags) {
    const investmentIds = selected
      .filter((p) => p.tags.some((t) => t.id === tag.id))
      .map((p) => p.id);
    if (investmentIds.length > 0) add({ key: `tag:${tag.id}`, investmentIds });
  }

  if (g) {
    for (const part of groupParts(selected, g, tags)) {
      if (part.key.startsWith('tag:')) continue;
      add({ key: part.key, investmentIds: part.positionIds });
      if (part.currency !== null && part.currency !== reportCurrency) {
        add({
          key: `${part.key}:native`,
          investmentIds: part.positionIds,
          currency: part.currency,
        });
      }
    }
  }
  return [...specs.values()];
}

/** The currency a position is quoted in: its price currency, else its average price currency. */
export function positionCurrency(item: {
  priceCurrency: string | null;
  averagePriceCurrency: string;
}): string {
  return item.priceCurrency || item.averagePriceCurrency;
}

// ---------------------------------------------------------------------------
// Concentration, long tails, benchmark alignment
// ---------------------------------------------------------------------------

/** Share of the five largest values in the total, and the effective number of positions `1 / Σ w²`. */
export function concentration(values: readonly number[]): { top5Share: number; effective: number } {
  const total = values.reduce((sum, v) => sum + v, 0);
  if (!(total > 0)) return { top5Share: 0, effective: 0 };
  const top5 = [...values]
    .sort((a, b) => b - a)
    .slice(0, 5)
    .reduce((sum, v) => sum + v, 0);
  const squares = values.reduce((sum, v) => sum + (v / total) ** 2, 0);
  return { top5Share: top5 / total, effective: squares > 0 ? 1 / squares : 0 };
}

/**
 * Everything when there are at most `limit` items, else the `keep` largest by `|size|` in
 * `shown` and the remainder in `rest`; both keep the original order of `items`.
 */
export function splitTop<T>(
  items: readonly T[],
  size: (t: T) => number,
  keep = 8,
  limit = 9
): { shown: T[]; rest: T[] } {
  if (items.length <= limit) return { shown: [...items], rest: [] };
  const ranked = items
    .map((item, index) => ({ index, magnitude: Math.abs(size(item)) }))
    .sort((a, b) => b.magnitude - a.magnitude || a.index - b.index);
  const kept = new Set(ranked.slice(0, keep).map((r) => r.index));
  const shown: T[] = [];
  const rest: T[] = [];
  items.forEach((item, index) => (kept.has(index) ? shown : rest).push(item));
  return { shown, rest };
}

/**
 * Map a daily benchmark onto the performance dates: the last benchmark point on or before each
 * date, `null` before the first. Both date lists must be ascending.
 */
export function seriesValueAt(
  dates: readonly number[],
  benchDates: readonly number[],
  bench: readonly number[]
): (number | null)[] {
  const count = Math.min(benchDates.length, bench.length);
  const out: (number | null)[] = [];
  let j = -1;
  for (const date of dates) {
    while (j + 1 < count && benchDates[j + 1] <= date) j++;
    out.push(j >= 0 ? bench[j] : null);
  }
  return out;
}
