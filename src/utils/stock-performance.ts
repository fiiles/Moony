/**
 * Pure helpers for the stocks analysis page: period bounds, custom range validation, date axis
 * ticks, drawdown, flow buckets, comparison groups (and the "don't split" choice), chart series
 * specs and concentration.
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
  // Not `Date.UTC(year, …)`: it reads the years 0–99 as 1900–1999
  const date = new Date(0);
  date.setUTCFullYear(year, month - 1, day);
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day
  ) {
    return null;
  }
  return date.getTime() / 1000;
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

/** What the custom range does with the date inputs after an edit (see `resolveRangeInput`). */
export type RangeInputDecision =
  { kind: 'commit'; range: DayRange } | { kind: 'draft'; error: RangeError | null };

/**
 * Decide what an edit of the two custom date inputs does. A date input reports every digit typed
 * into its year, so `0002-01-05`, `0020-01-05` and `0202-01-05` precede `2025-01-05`; clamping
 * and committing those would overwrite the field mid-typing and request the whole history.
 *
 * - While typing (`typing` names the input just edited), the range commits at once only when that
 *   date lies inside `[min, max]` as typed, which keeps the native picker instant. Outside it the
 *   inputs stay a draft without an error: the person is probably still typing.
 * - Settling (`typing` is `null`: blur or Enter) commits whatever is complete, clamped to
 *   `[min, max]` as `parseCustomRange` does.
 * - An incomplete value (`missing`) or a wrong order (`order`) never commits; it stays a draft
 *   with that error, typing or settling.
 */
export function resolveRangeInput(
  fromIso: string,
  toIso: string,
  min: number,
  max: number,
  typing: 'from' | 'to' | null
): RangeInputDecision {
  const edited = dayFromIso(typing === 'to' ? toIso : fromIso);
  const complete = dayFromIso(fromIso) !== null && dayFromIso(toIso) !== null;
  if (complete && typing !== null && edited !== null && (edited < min || edited > max)) {
    return { kind: 'draft', error: null };
  }
  const parsed = parseCustomRange(fromIso, toIso, min, max);
  return 'error' in parsed
    ? { kind: 'draft', error: parsed.error }
    : { kind: 'commit', range: parsed.range };
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

/**
 * A way to split the selected positions into lines and rows. `none` ("Nerozdělovat") splits
 * nothing: without a filter the chart shows the whole portfolio only, with a filter the selection
 * is the one part. It is a choice of the control, not an option `compareGroups` finds in the data.
 */
export type CompareGroup =
  | { kind: 'none'; key: 'none' }
  | { kind: 'tags'; key: string; group: StockTagGroup; tags: StockTag[] }
  | { kind: 'sector'; key: 'sector' }
  | { kind: 'currency'; key: 'currency' };

/** The "don't split" choice; its key is what the page remembers. */
export const NO_SPLIT: CompareGroup = { kind: 'none', key: 'none' };

/** Key of the one part of "don't split" with a filter on: the selected positions together. */
export const SELECTION_KEY = 'selection';

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

/**
 * What the "Rozdělit podle" control offers: "don't split" first, then the real options. Without a
 * real option there is nothing to choose from, so no control at all.
 */
export function withNoSplit(options: readonly CompareGroup[]): CompareGroup[] {
  return options.length > 0 ? [NO_SPLIT, ...options] : [];
}

/**
 * The chosen way to split. `key` is the remembered choice: "none" is always there (once any real
 * option exists), a key that no longer exists falls back to the first real option, and with no
 * real option there is no group (`null`: the whole portfolio only, no control).
 */
export function chooseGroup(
  options: readonly CompareGroup[],
  key: string | null
): CompareGroup | null {
  if (options.length === 0) return null;
  if (key === NO_SPLIT.key) return NO_SPLIT;
  return options.find((g) => g.key === key) ?? options[0];
}

/**
 * Whether a divider precedes each choice: where the kind of choice changes, which is "don't
 * split", then the tag groups, then the automatic groups.
 */
export function splitDividers(choices: readonly CompareGroup[]): boolean[] {
  const family = (g: CompareGroup) => (g.kind === 'none' ? 0 : g.kind === 'tags' ? 1 : 2);
  return choices.map((g, i) => i > 0 && family(g) !== family(choices[i - 1]));
}

/** Most real options that fit a segmented control; more turn it into a select. */
export const MAX_SEGMENTED_SPLITS = 4;

/**
 * How the control for `choices` (see `withNoSplit`) is drawn: not at all without a real option,
 * a segmented control for up to four real options, else a select. "Don't split" does not count.
 */
export function splitControl(choices: readonly CompareGroup[]): 'hidden' | 'segmented' | 'select' {
  const real = choices.filter((g) => g.kind !== 'none').length;
  if (real === 0) return 'hidden';
  return real > MAX_SEGMENTED_SPLITS ? 'select' : 'segmented';
}

/** The part of `g` a position belongs to. */
export function partKey(p: PositionLite, g: CompareGroup): string {
  if (g.kind === 'none') return SELECTION_KEY;
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
 * alphabetical; the part without a tag or sector last. "Don't split" has no parts of its own
 * (see `noSplitParts`).
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
  } else if (g.kind === 'currency') {
    const codes = [...new Set(positions.map((p) => p.currency))].sort((a, b) => a.localeCompare(b));
    for (const currency of codes) {
      add(`currency:${currency}`, { tag: null, sector: null, currency, isNone: false });
    }
  }
  return parts;
}

/**
 * The parts of "don't split": none while no filter is on (the chart shows the whole portfolio
 * only), else one part, the selection, holding every selected position. Nothing selected is no
 * part either.
 */
export function noSplitParts(selected: readonly PositionLite[], filterActive: boolean): Part[] {
  if (!filterActive || selected.length === 0) return [];
  return [
    {
      key: SELECTION_KEY,
      positionIds: selected.map((p) => p.id),
      tag: null,
      sector: null,
      currency: null,
      isNone: false,
    },
  ];
}

/** The parts of the chosen way to split, "don't split" included (it needs `filterActive`). */
export function splitParts(
  selected: readonly PositionLite[],
  g: CompareGroup,
  tags: readonly StockTag[],
  filterActive: boolean
): Part[] {
  return g.kind === 'none' ? noSplitParts(selected, filterActive) : groupParts(selected, g, tags);
}

export interface SeriesSpecLite {
  key: string;
  investmentIds: string[];
  currency?: string | null;
}

/** Most series one performance request may ask for (the backend's `validation.tooManySeries`). */
export const MAX_SERIES = 256;

/** Key of the series of every position carrying a tag of the tag group `groupId`. */
export function tagGroupSeriesKey(groupId: string): string {
  return `tag-group:${groupId}`;
}

/**
 * Series to request from the backend: one per tag carried by a selected position (`tag:<id>`),
 * plus the parts of `g` that are not tag parts (`none:…`, `sector:…`, `currency:…`, or the
 * `selection` of "don't split" while `filterActive`), and for a currency part whose code differs
 * from `reportCurrency` also `<key>:native` computed in that currency. Last, one per tag group
 * (`tag-group:<id>`) of the selected positions carrying any of its tags, for the group's subtotal
 * in the tags table. Keys are unique, the order is stable.
 *
 * The request never holds more than `MAX_SERIES`. The group series come only when everything
 * fits. Past the limit the tag series of tags outside the chosen group (all of them without a
 * tag group) are dropped first: only the chosen split is charted, the others only feed the tags
 * table's period return. Should the chosen group alone be too big, the first `MAX_SERIES` stay.
 */
export function seriesSpecs(
  selected: readonly PositionLite[],
  tags: readonly StockTag[],
  g: CompareGroup | null,
  reportCurrency: string,
  filterActive = false
): SeriesSpecLite[] {
  const specs = new Map<string, SeriesSpecLite>();
  const outside = new Set<string>();
  const add = (spec: SeriesSpecLite) => {
    if (!specs.has(spec.key)) specs.set(spec.key, spec);
  };
  const chosenGroupId = g?.kind === 'tags' ? g.group.id : null;

  for (const tag of tags) {
    const investmentIds = selected
      .filter((p) => p.tags.some((t) => t.id === tag.id))
      .map((p) => p.id);
    if (investmentIds.length === 0) continue;
    add({ key: `tag:${tag.id}`, investmentIds });
    if (chosenGroupId === null || tag.groupId !== chosenGroupId) outside.add(`tag:${tag.id}`);
  }

  if (g) {
    for (const part of splitParts(selected, g, tags, filterActive)) {
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

  const groupIds = [...new Set(tags.flatMap((tag) => (tag.groupId ? [tag.groupId] : [])))];
  const groupSpecs = groupIds.flatMap((groupId) => {
    const investmentIds = selected
      .filter((p) => p.tags.some((t) => t.groupId === groupId))
      .map((p) => p.id);
    return investmentIds.length > 0 ? [{ key: tagGroupSeriesKey(groupId), investmentIds }] : [];
  });

  const all = [...specs.values()];
  if (all.length + groupSpecs.length <= MAX_SERIES) return [...all, ...groupSpecs];
  if (all.length <= MAX_SERIES) return all;
  return all.filter((spec) => !outside.has(spec.key)).slice(0, MAX_SERIES);
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

/**
 * Cumulative returns (percent) re-based to start at 0 on their first entry: `(1 + r) / (1 + r₀) − 1`.
 * An index line fetched for a longer period read on a shorter one then still starts at 0. All
 * `null` when the first entry is missing or a total loss, as there is nothing to re-base from.
 */
export function rebaseReturns(values: readonly (number | null)[]): (number | null)[] {
  const first = values[0];
  if (first === null || first === undefined || first <= -100) return values.map(() => null);
  // Already based at zero (a line fetched for this very period): no float noise
  if (first === 0) return [...values];
  const base = 100 + first;
  return values.map((value) => (value === null ? null : ((100 + value) / base - 1) * 100));
}

/** What identifies an index line apart from its period: the ticker and the report currency. */
export interface IndexLineId {
  ticker: string;
  reportCurrency: string;
}

/**
 * Whether the previous index request's response may stand in for the next one while it loads:
 * the same index in the same currency, only the period moved. Another index would show its
 * predecessor's returns under the new name.
 */
export function sameIndexLine(
  previous: IndexLineId | null | undefined,
  next: IndexLineId | null | undefined
): boolean {
  return (
    !!previous &&
    !!next &&
    previous.ticker === next.ticker &&
    previous.reportCurrency === next.reportCurrency
  );
}
