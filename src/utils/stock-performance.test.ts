import { describe, expect, it } from 'vitest';
import type { StockTag, StockTagGroup } from '@shared/schema';
import {
  ALL_FROM,
  DAY,
  MAX_SEGMENTED_SPLITS,
  MAX_SERIES,
  NO_SPLIT,
  PERIOD_PRESETS,
  SELECTION_KEY,
  bucketKind,
  chooseGroup,
  compareGroups,
  concentration,
  dateAxisTicks,
  dayFromIso,
  drawdown,
  flowBuckets,
  groupParts,
  isoDay,
  maxDrawdownTo,
  noSplitParts,
  parseCustomRange,
  partKey,
  positionCurrency,
  presetRange,
  rebaseReturns,
  resolveRangeInput,
  sameIndexLine,
  seriesSpecs,
  seriesValueAt,
  tagGroupSeriesKey,
  splitControl,
  splitDividers,
  splitParts,
  splitTop,
  utcDay,
  withNoSplit,
  type CompareGroup,
  type PositionLite,
} from './stock-performance';

/** Unix seconds at UTC midnight of an ISO day. */
function d(iso: string): number {
  const [y, m, day] = iso.split('-').map(Number);
  return Date.UTC(y, m - 1, day) / 1000;
}

const TODAY = d('2026-10-11');

describe('constants', () => {
  it('exposes the day length, the presets and the start of "all"', () => {
    expect(DAY).toBe(86_400);
    expect(PERIOD_PRESETS).toEqual(['3m', 'ytd', '1y', '3y', 'all', 'custom']);
    expect(ALL_FROM).toBe(d('2000-01-01'));
  });
});

describe('utcDay / isoDay / dayFromIso', () => {
  it('floors a timestamp to UTC midnight', () => {
    expect(utcDay(d('2026-10-11') + 23 * 3600 + 59)).toBe(d('2026-10-11'));
    expect(utcDay(d('2026-10-11'))).toBe(d('2026-10-11'));
  });

  it('formats a day as YYYY-MM-DD in UTC', () => {
    expect(isoDay(d('2026-01-05'))).toBe('2026-01-05');
    expect(isoDay(d('2026-12-31') + 86_399)).toBe('2026-12-31');
  });

  it('parses a strict YYYY-MM-DD and rejects everything else', () => {
    expect(dayFromIso('2026-10-11')).toBe(d('2026-10-11'));
    expect(dayFromIso('2024-02-29')).toBe(d('2024-02-29'));
    expect(dayFromIso('')).toBeNull();
    expect(dayFromIso('2026-1-5')).toBeNull();
    expect(dayFromIso('2026-10-11T00:00')).toBeNull();
    expect(dayFromIso('2026-13-01')).toBeNull();
    expect(dayFromIso('2026-02-30')).toBeNull();
    expect(dayFromIso('2025-02-29')).toBeNull();
    expect(dayFromIso('11. 10. 2026')).toBeNull();
  });

  it('round-trips', () => {
    expect(dayFromIso(isoDay(TODAY))).toBe(TODAY);
  });

  it('reads a year below 100 as that year, not as 19xx (a half-typed year of a date input)', () => {
    const year2 = dayFromIso('0002-01-05');
    expect(year2).not.toBeNull();
    expect(new Date((year2 as number) * 1000).getUTCFullYear()).toBe(2);
    expect(isoDay(year2 as number)).toBe('0002-01-05');
    expect(isoDay(dayFromIso('0099-12-31') as number)).toBe('0099-12-31');
    expect(dayFromIso('0004-02-29')).not.toBeNull(); // year 4 is a leap year
    expect(dayFromIso('0002-02-29')).toBeNull();
  });
});

describe('presetRange', () => {
  it('computes every preset for 2026-10-11', () => {
    expect(presetRange('3m', TODAY)).toEqual({ from: d('2026-07-11'), to: TODAY });
    expect(presetRange('ytd', TODAY)).toEqual({ from: d('2026-01-01'), to: TODAY });
    expect(presetRange('1y', TODAY)).toEqual({ from: d('2025-10-11'), to: TODAY });
    expect(presetRange('3y', TODAY)).toEqual({ from: d('2023-10-11'), to: TODAY });
    expect(presetRange('all', TODAY)).toEqual({ from: ALL_FROM, to: TODAY });
  });

  it('clamps the day of month when the target month is shorter', () => {
    expect(presetRange('3m', d('2026-05-31')).from).toBe(d('2026-02-28'));
    expect(presetRange('1y', d('2024-02-29')).from).toBe(d('2023-02-28'));
    expect(presetRange('3y', d('2024-02-29')).from).toBe(d('2021-02-28'));
    expect(presetRange('3m', d('2026-03-31')).from).toBe(d('2025-12-31'));
  });

  it('crosses the year boundary for 3m', () => {
    expect(presetRange('3m', d('2026-02-10')).from).toBe(d('2025-11-10'));
  });

  it('keeps from at least one day before to', () => {
    // On Jan 1 the year-to-date start would equal today.
    const jan1 = d('2027-01-01');
    expect(presetRange('ytd', jan1)).toEqual({ from: jan1 - DAY, to: jan1 });
  });

  it('floors a today that is not at midnight', () => {
    expect(presetRange('1y', TODAY + 5 * 3600)).toEqual({ from: d('2025-10-11'), to: TODAY });
  });
});

describe('parseCustomRange', () => {
  const min = d('2024-03-01');
  const max = d('2026-10-11');

  it('accepts a valid range', () => {
    expect(parseCustomRange('2025-01-01', '2025-06-30', min, max)).toEqual({
      range: { from: d('2025-01-01'), to: d('2025-06-30') },
    });
  });

  it('reports a missing or malformed date', () => {
    expect(parseCustomRange('', '2025-06-30', min, max)).toEqual({ error: 'missing' });
    expect(parseCustomRange('2025-01-01', '', min, max)).toEqual({ error: 'missing' });
    expect(parseCustomRange('2025-01-01', '30. 6. 2025', min, max)).toEqual({ error: 'missing' });
  });

  it('reports an order error when from is after to', () => {
    expect(parseCustomRange('2025-06-30', '2025-01-01', min, max)).toEqual({ error: 'order' });
  });

  it('needs more than a one-day span', () => {
    expect(parseCustomRange('2025-01-01', '2025-01-02', min, max)).toEqual({ error: 'order' });
    expect(parseCustomRange('2025-01-01', '2025-01-01', min, max)).toEqual({ error: 'order' });
    expect(parseCustomRange('2025-01-01', '2025-01-03', min, max)).toEqual({
      range: { from: d('2025-01-01'), to: d('2025-01-03') },
    });
  });

  it('clamps to [min, max]', () => {
    expect(parseCustomRange('2020-01-01', '2030-01-01', min, max)).toEqual({
      range: { from: min, to: max },
    });
  });

  it('reports an order error when clamping leaves no room', () => {
    expect(parseCustomRange('2030-01-01', '2031-01-01', min, max)).toEqual({ error: 'order' });
    expect(parseCustomRange('2010-01-01', '2011-01-01', min, max)).toEqual({ error: 'order' });
  });
});

describe('resolveRangeInput', () => {
  const min = d('2024-03-01');
  const max = d('2026-10-11');

  it('commits a complete range inside the bounds at once (the native picker stays instant)', () => {
    expect(resolveRangeInput('2025-01-01', '2025-06-30', min, max, 'from')).toEqual({
      kind: 'commit',
      range: { from: d('2025-01-01'), to: d('2025-06-30') },
    });
    expect(resolveRangeInput('2025-01-01', '2025-06-30', min, max, 'to')).toEqual({
      kind: 'commit',
      range: { from: d('2025-01-01'), to: d('2025-06-30') },
    });
  });

  it('accepts the bounds themselves', () => {
    expect(resolveRangeInput('2024-03-01', '2026-10-11', min, max, 'from')).toEqual({
      kind: 'commit',
      range: { from: min, to: max },
    });
  });

  it('keeps a half-typed year as a draft instead of committing the clamped minimum', () => {
    // Typing 2025 digit by digit: the date input reports 0002-…, 0020-…, 0202-… on the way
    for (const typed of ['0002-01-05', '0020-01-05', '0202-01-05']) {
      expect(resolveRangeInput(typed, '2026-06-30', min, max, 'from')).toEqual({
        kind: 'draft',
        error: null,
      });
    }
    expect(resolveRangeInput('2025-01-05', '2026-06-30', min, max, 'from').kind).toBe('commit');
  });

  it('keeps any date outside the bounds as a draft while typing', () => {
    expect(resolveRangeInput('2024-02-29', '2026-06-30', min, max, 'from')).toEqual({
      kind: 'draft',
      error: null,
    });
    expect(resolveRangeInput('2025-01-01', '2026-10-12', min, max, 'to')).toEqual({
      kind: 'draft',
      error: null,
    });
    expect(resolveRangeInput('2025-01-01', '2030-01-01', min, max, 'to')).toEqual({
      kind: 'draft',
      error: null,
    });
  });

  it('judges only the date being edited, the other one is clamped as before', () => {
    expect(resolveRangeInput('2025-01-01', '2030-01-01', min, max, 'from')).toEqual({
      kind: 'commit',
      range: { from: d('2025-01-01'), to: max },
    });
  });

  it('never commits an incomplete value, typing or settling', () => {
    expect(resolveRangeInput('', '2025-06-30', min, max, 'from')).toEqual({
      kind: 'draft',
      error: 'missing',
    });
    expect(resolveRangeInput('2025-01-01', '', min, max, 'to')).toEqual({
      kind: 'draft',
      error: 'missing',
    });
    expect(resolveRangeInput('2025-01-01', '', min, max, null)).toEqual({
      kind: 'draft',
      error: 'missing',
    });
    expect(resolveRangeInput('2025-01-01', '30. 6. 2025', min, max, 'to')).toEqual({
      kind: 'draft',
      error: 'missing',
    });
  });

  it('reports the order error at once for dates inside the bounds', () => {
    expect(resolveRangeInput('2025-06-30', '2025-01-01', min, max, 'from')).toEqual({
      kind: 'draft',
      error: 'order',
    });
    expect(resolveRangeInput('2025-01-01', '2025-01-02', min, max, 'to')).toEqual({
      kind: 'draft',
      error: 'order',
    });
  });

  it('settles a draft outside the bounds clamped, as parseCustomRange does', () => {
    expect(resolveRangeInput('0202-01-05', '2025-06-30', min, max, null)).toEqual({
      kind: 'commit',
      range: { from: min, to: d('2025-06-30') },
    });
    expect(resolveRangeInput('2020-01-01', '2030-01-01', min, max, null)).toEqual({
      kind: 'commit',
      range: { from: min, to: max },
    });
  });

  it('settles to the order error when clamping leaves no room', () => {
    expect(resolveRangeInput('2030-01-01', '2031-01-01', min, max, null)).toEqual({
      kind: 'draft',
      error: 'order',
    });
    expect(resolveRangeInput('2010-01-01', '2011-01-01', min, max, null)).toEqual({
      kind: 'draft',
      error: 'order',
    });
  });
});

describe('dateAxisTicks', () => {
  it('uses the 1st and 15th of every month up to 110 days', () => {
    const t0 = d('2026-07-11');
    const t1 = d('2026-10-11'); // 92 days
    const ticks = dateAxisTicks(t0, t1);
    expect(ticks.every((t) => t.kind === 'day')).toBe(true);
    expect(ticks.map((t) => isoDay(t.t))).toEqual([
      '2026-07-15',
      '2026-08-01',
      '2026-08-15',
      '2026-09-01',
      '2026-09-15',
      '2026-10-01',
    ]);
    expect(ticks.every((t) => !t.yearStart)).toBe(true);
  });

  it('keeps ticks strictly inside the span', () => {
    const t0 = d('2026-08-01');
    const t1 = d('2026-09-15');
    const ticks = dateAxisTicks(t0, t1).map((t) => isoDay(t.t));
    expect(ticks).toEqual(['2026-08-15', '2026-09-01']);
  });

  it('flags January 1 as a year start on short spans', () => {
    const ticks = dateAxisTicks(d('2025-12-20'), d('2026-02-20'));
    const jan1 = ticks.find((t) => isoDay(t.t) === '2026-01-01');
    expect(jan1?.yearStart).toBe(true);
    expect(ticks.filter((t) => t.yearStart)).toHaveLength(1);
  });

  it('uses month starts up to 420 days (year to date)', () => {
    const ticks = dateAxisTicks(d('2026-01-01'), d('2026-10-11'));
    expect(ticks.every((t) => t.kind === 'month')).toBe(true);
    expect(ticks.map((t) => isoDay(t.t))).toEqual([
      '2026-02-01',
      '2026-03-01',
      '2026-04-01',
      '2026-05-01',
      '2026-06-01',
      '2026-07-01',
      '2026-08-01',
      '2026-09-01',
      '2026-10-01',
    ]);
    expect(ticks.some((t) => t.yearStart)).toBe(false);
  });

  it('uses quarter starts up to 1300 days', () => {
    const t0 = d('2023-10-11');
    const t1 = d('2026-10-11'); // 1096 days
    const ticks = dateAxisTicks(t0, t1);
    expect(ticks.every((t) => t.kind === 'quarter')).toBe(true);
    expect(ticks.map((t) => isoDay(t.t))).toEqual([
      '2024-01-01',
      '2024-04-01',
      '2024-07-01',
      '2024-10-01',
      '2025-01-01',
      '2025-04-01',
      '2025-07-01',
      '2025-10-01',
      '2026-01-01',
      '2026-04-01',
      '2026-07-01',
      '2026-10-01',
    ]);
    expect(ticks.filter((t) => t.yearStart).map((t) => isoDay(t.t))).toEqual([
      '2024-01-01',
      '2025-01-01',
      '2026-01-01',
    ]);
  });

  it('uses January 1 beyond 1300 days', () => {
    const ticks = dateAxisTicks(d('2021-10-11'), d('2026-10-11'));
    expect(ticks.every((t) => t.kind === 'year' && t.yearStart)).toBe(true);
    expect(ticks.map((t) => isoDay(t.t))).toEqual([
      '2022-01-01',
      '2023-01-01',
      '2024-01-01',
      '2025-01-01',
      '2026-01-01',
    ]);
  });

  it('switches kind exactly at the thresholds', () => {
    const t0 = d('2025-01-01');
    expect(dateAxisTicks(t0, t0 + 110 * DAY)[0]?.kind).toBe('day');
    expect(dateAxisTicks(t0, t0 + 111 * DAY)[0]?.kind).toBe('month');
    expect(dateAxisTicks(t0, t0 + 420 * DAY)[0]?.kind).toBe('month');
    expect(dateAxisTicks(t0, t0 + 421 * DAY)[0]?.kind).toBe('quarter');
    expect(dateAxisTicks(t0, t0 + 1300 * DAY)[0]?.kind).toBe('quarter');
    expect(dateAxisTicks(t0, t0 + 1301 * DAY)[0]?.kind).toBe('year');
  });

  it('returns nothing for an empty span', () => {
    expect(dateAxisTicks(d('2026-01-01'), d('2026-01-01'))).toEqual([]);
    expect(dateAxisTicks(d('2026-02-01'), d('2026-01-01'))).toEqual([]);
  });
});

describe('drawdown', () => {
  it('measures the distance from the running peak of 100 + twr', () => {
    const dd = drawdown([0, 10, 5, 12, null]);
    expect(dd).toHaveLength(5);
    expect(dd[0]).toBeCloseTo(0, 10);
    expect(dd[1]).toBeCloseTo(0, 10);
    expect(dd[2]).toBeCloseTo((105 / 110 - 1) * 100, 10);
    expect(dd[2]).toBeCloseTo(-4.545454, 5);
    expect(dd[3]).toBeCloseTo(0, 10);
    expect(dd[4]).toBeNull();
  });

  it('does not let a null reset the peak', () => {
    const dd = drawdown([0, 20, null, 10]);
    expect(dd[2]).toBeNull();
    expect(dd[3]).toBeCloseTo((110 / 120 - 1) * 100, 10);
  });

  it('is empty for no input and zero for a falling total loss', () => {
    expect(drawdown([])).toEqual([]);
    expect(drawdown([-100, -100])).toEqual([0, 0]);
  });
});

describe('maxDrawdownTo', () => {
  const dd = [0, -2, -1, -6, null, -3];
  it('is the minimum up to and including the index', () => {
    expect(maxDrawdownTo(dd, 0)).toBe(0);
    expect(maxDrawdownTo(dd, 2)).toBe(-2);
    expect(maxDrawdownTo(dd, 3)).toBe(-6);
    expect(maxDrawdownTo(dd, 5)).toBe(-6);
  });

  it('skips nulls and tolerates an index past the end', () => {
    expect(maxDrawdownTo([null, null, -1], 1)).toBe(0);
    expect(maxDrawdownTo(dd, 99)).toBe(-6);
    expect(maxDrawdownTo([], 3)).toBe(0);
  });
});

describe('bucketKind', () => {
  it('follows the thresholds', () => {
    expect(bucketKind(30)).toBe('week');
    expect(bucketKind(100)).toBe('week');
    expect(bucketKind(101)).toBe('month');
    expect(bucketKind(450)).toBe('month');
    expect(bucketKind(451)).toBe('quarter');
    expect(bucketKind(1300)).toBe('quarter');
    expect(bucketKind(1301)).toBe('year');
  });
});

describe('flowBuckets', () => {
  // Month ends of 2026 plus a start point: Jan 1, Jan 31, Feb 28, ... Oct 31 (10 points).
  const dates = [
    '2026-01-31',
    '2026-02-28',
    '2026-03-31',
    '2026-04-30',
    '2026-05-31',
    '2026-06-30',
    '2026-07-31',
    '2026-08-31',
    '2026-09-30',
    '2026-10-31',
  ].map(d);

  it('groups by month with the previous point as base', () => {
    const values = [1000, 1100, 1250, 1230, 1300, 1280, 1400, 1450, 1500, 1520];
    // 100 deposited in March, nothing else
    const flows = [0, 0, 100, 100, 100, 100, 100, 100, 100, 100];
    const buckets = flowBuckets(dates, values, flows, 'month');
    expect(buckets).toHaveLength(9); // point 0 is the base of the first bucket
    expect(buckets[0]).toEqual({
      start: d('2026-02-28'),
      end: d('2026-02-28'),
      deposits: 0,
      market: 100,
    });
    expect(buckets[1]).toEqual({
      start: d('2026-03-31'),
      end: d('2026-03-31'),
      deposits: 100,
      market: 50,
    });
    expect(buckets[2].deposits).toBe(0);
    expect(buckets[2].market).toBe(-20);
    // deposits + market add up to the whole change
    const sum = buckets.reduce((a, b) => a + b.deposits + b.market, 0);
    expect(sum).toBeCloseTo(values[9] - values[0], 10);
  });

  it('merges months into quarters and years', () => {
    const values = [1000, 1100, 1250, 1230, 1300, 1280, 1400, 1450, 1500, 1520];
    const flows = [0, 0, 100, 100, 100, 100, 100, 100, 100, 100];
    const quarters = flowBuckets(dates, values, flows, 'quarter');
    // Feb–Mar (Q1, base Jan), Apr–Jun (Q2), Jul–Sep (Q3), Oct (Q4)
    expect(quarters).toHaveLength(4);
    expect(quarters[0]).toEqual({
      start: d('2026-02-28'),
      end: d('2026-03-31'),
      deposits: 100,
      market: 150,
    });
    expect(quarters[1]).toEqual({
      start: d('2026-04-30'),
      end: d('2026-06-30'),
      deposits: 0,
      market: 30,
    });
    expect(quarters[2].market).toBe(220);
    expect(quarters[3]).toEqual({
      start: d('2026-10-31'),
      end: d('2026-10-31'),
      deposits: 0,
      market: 20,
    });
    const years = flowBuckets(dates, values, flows, 'year');
    expect(years).toEqual([
      { start: d('2026-02-28'), end: d('2026-10-31'), deposits: 100, market: 420 },
    ]);
  });

  it('groups weeks in 7-day blocks from the first date', () => {
    const base = d('2026-03-01');
    const wk = Array.from({ length: 15 }, (_, i) => base + i * DAY);
    const values = wk.map((_, i) => 100 + i);
    const flows = wk.map(() => 0);
    const buckets = flowBuckets(wk, values, flows, 'week');
    // days 1–6 (block 0, base day 0), days 7–13 (block 1), day 14 (block 2)
    expect(buckets.map((b) => [b.start, b.end])).toEqual([
      [wk[1], wk[6]],
      [wk[7], wk[13]],
      [wk[14], wk[14]],
    ]);
    expect(buckets[0].market).toBe(6);
    expect(buckets[1].market).toBe(7);
    expect(buckets[2].market).toBe(1);
  });

  it('drops a bucket whose value is null and the one based on it', () => {
    const values: (number | null)[] = [1000, 1100, null, 1230, 1300, 1280, 1400, 1450, 1500, 1520];
    const flows = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const buckets = flowBuckets(dates, values, flows, 'month');
    // March has a null last value; April's base (March) is null too.
    expect(buckets.map((b) => isoDay(b.end))).toEqual([
      '2026-02-28',
      '2026-05-31',
      '2026-06-30',
      '2026-07-31',
      '2026-08-31',
      '2026-09-30',
      '2026-10-31',
    ]);
  });

  it('drops a bucket whose first point has no value', () => {
    const values: (number | null)[] = [1000, 1100, 1250, 1230, 1300, 1280, 1400, 1450, 1500, 1520];
    const quarterDates = dates;
    const withNullFirst = values.slice();
    withNullFirst[3] = null; // first point of Q2 (April)
    const buckets = flowBuckets(quarterDates, withNullFirst, new Array(10).fill(0), 'quarter');
    expect(buckets.map((b) => isoDay(b.start))).toEqual(['2026-02-28', '2026-07-31', '2026-10-31']);
  });

  it('returns nothing for fewer than two points', () => {
    expect(flowBuckets([], [], [], 'month')).toEqual([]);
    expect(flowBuckets([d('2026-01-01')], [1], [0], 'month')).toEqual([]);
  });
});

// --- comparison groups -----------------------------------------------------

function tagGroup(id: string, name: string, createdAt: number): StockTagGroup {
  return { id, name, description: null, createdAt };
}
function tag(id: string, name: string, groupId: string | null, createdAt = 1): StockTag {
  return { id, name, color: null, groupId, createdAt };
}
function pos(
  id: string,
  opts: { sector?: string | null; currency?: string; tags?: StockTag[] } = {}
): PositionLite {
  return {
    id,
    sector: opts.sector === undefined ? null : opts.sector,
    currency: opts.currency ?? 'USD',
    tags: (opts.tags ?? []).map((t) => ({ id: t.id, groupId: t.groupId })),
  };
}

const strategy = tagGroup('g-strategy', 'Strategie', 100);
const region = tagGroup('g-region', 'Region', 200);
const empty = tagGroup('g-empty', 'Bez štítků', 50);
const growth = tag('t-growth', 'Růst', 'g-strategy');
const dividend = tag('t-div', 'Dividendy', 'g-strategy');
const usa = tag('t-usa', 'USA', 'g-region');
const eu = tag('t-eu', 'Evropa', 'g-region');
const loose = tag('t-loose', 'Osamělý', null);
const allGroups = [region, strategy, empty];
const allTags = [usa, eu, growth, dividend, loose];

describe('compareGroups', () => {
  const positions = [
    pos('a', { sector: 'Technologie', currency: 'USD', tags: [growth, usa] }),
    pos('b', { sector: 'Finance', currency: 'EUR', tags: [dividend] }),
  ];

  it('lists tag groups with tags by creation date, then sector, then currency', () => {
    const result = compareGroups(allGroups, allTags, positions);
    expect(result.map((g) => g.key)).toEqual([
      'group:g-strategy',
      'group:g-region',
      'sector',
      'currency',
    ]);
    const first = result[0];
    expect(first.kind).toBe('tags');
    if (first.kind === 'tags') {
      expect(first.group).toBe(strategy);
      expect(first.tags.map((t) => t.id)).toEqual(['t-div', 't-growth']); // by name: Dividendy, Růst
    }
  });

  it('skips a group without tags and tags without a group', () => {
    const result = compareGroups([empty], [loose], positions);
    expect(result.map((g) => g.key)).toEqual(['sector', 'currency']);
  });

  it('hides the sector when all positions share one and none is null', () => {
    const same = [
      pos('a', { sector: 'Technologie' }),
      pos('b', { sector: 'Technologie', currency: 'EUR' }),
    ];
    expect(compareGroups([], [], same).map((g) => g.key)).toEqual(['currency']);
  });

  it('shows the sector for one sector plus an ETF without sector', () => {
    const mixed = [pos('a', { sector: 'Technologie' }), pos('b', { sector: null })];
    expect(compareGroups([], [], mixed).map((g) => g.key)).toEqual(['sector']);
  });

  it('hides the sector when only sector-less positions are held', () => {
    const etfs = [pos('a', { sector: null }), pos('b', { sector: null })];
    expect(compareGroups([], [], etfs)).toEqual([]);
  });

  it('hides the currency with one currency', () => {
    const one = [
      pos('a', { sector: 'X', currency: 'USD' }),
      pos('b', { sector: 'Y', currency: 'USD' }),
    ];
    expect(compareGroups([], [], one).map((g) => g.key)).toEqual(['sector']);
  });

  it('shows nothing for no positions and no groups', () => {
    expect(compareGroups([], [], [])).toEqual([]);
  });
});

describe('partKey', () => {
  const strategyGroup = compareGroups(allGroups, allTags, [])[0];
  const sectorGroup: CompareGroup = { kind: 'sector', key: 'sector' };
  const currencyGroup: CompareGroup = { kind: 'currency', key: 'currency' };

  it('uses the tag of the position in that group, else a none key', () => {
    expect(partKey(pos('a', { tags: [growth, usa] }), strategyGroup)).toBe('tag:t-growth');
    expect(partKey(pos('b', { tags: [usa] }), strategyGroup)).toBe('none:g-strategy');
    expect(partKey(pos('c'), strategyGroup)).toBe('none:g-strategy');
  });

  it('uses the sector name or an empty sector key', () => {
    expect(partKey(pos('a', { sector: 'Finance' }), sectorGroup)).toBe('sector:Finance');
    expect(partKey(pos('b', { sector: null }), sectorGroup)).toBe('sector:');
  });

  it('uses the currency code', () => {
    expect(partKey(pos('a', { currency: 'CZK' }), currencyGroup)).toBe('currency:CZK');
  });

  it('puts every position of "don\'t split" into the selection', () => {
    expect(partKey(pos('a', { tags: [growth] }), NO_SPLIT)).toBe(SELECTION_KEY);
  });
});

describe('groupParts', () => {
  const positions = [
    pos('a', { sector: 'Technologie', currency: 'USD', tags: [growth] }),
    pos('b', { sector: 'Finance', currency: 'EUR', tags: [dividend] }),
    pos('c', { sector: null, currency: 'USD', tags: [growth] }),
    pos('d', { sector: 'Finance', currency: 'CZK', tags: [] }),
  ];
  const groups = compareGroups(allGroups, allTags, positions);
  const byKey = (key: string) => groups.find((g) => g.key === key) as CompareGroup;

  it('orders tag parts by tag name and puts "none" last', () => {
    const parts = groupParts(positions, byKey('group:g-strategy'), allTags);
    expect(parts.map((p) => p.key)).toEqual(['tag:t-div', 'tag:t-growth', 'none:g-strategy']);
    expect(parts.map((p) => p.positionIds)).toEqual([['b'], ['a', 'c'], ['d']]);
    expect(parts[0].tag).toEqual(dividend);
    expect(parts.map((p) => p.isNone)).toEqual([false, false, true]);
    expect(parts[2].tag).toBeNull();
    expect(parts[0].sector).toBeNull();
    expect(parts[0].currency).toBeNull();
  });

  it('leaves out tags that no position carries and the none part when empty', () => {
    const only = [pos('a', { tags: [growth] })];
    const parts = groupParts(only, byKey('group:g-strategy'), allTags);
    expect(parts.map((p) => p.key)).toEqual(['tag:t-growth']);
  });

  it('orders sectors alphabetically with the missing sector last', () => {
    const parts = groupParts(positions, byKey('sector'), allTags);
    expect(parts.map((p) => p.key)).toEqual(['sector:Finance', 'sector:Technologie', 'sector:']);
    expect(parts.map((p) => p.sector)).toEqual(['Finance', 'Technologie', null]);
    expect(parts.map((p) => p.isNone)).toEqual([false, false, true]);
    expect(parts[0].positionIds).toEqual(['b', 'd']);
    expect(parts[0].tag).toBeNull();
  });

  it('orders currencies alphabetically', () => {
    const parts = groupParts(positions, byKey('currency'), allTags);
    expect(parts.map((p) => p.key)).toEqual(['currency:CZK', 'currency:EUR', 'currency:USD']);
    expect(parts.map((p) => p.currency)).toEqual(['CZK', 'EUR', 'USD']);
    expect(parts.every((p) => !p.isNone)).toBe(true);
    expect(parts[2].positionIds).toEqual(['a', 'c']);
  });
});

describe("don't split", () => {
  const a = pos('a', { sector: 'Technologie', tags: [growth] });
  const b = pos('b', { currency: 'EUR', tags: [dividend] });
  const real = compareGroups(allGroups, allTags, [a, b]); // strategy, region, sector, currency

  it('is the key "none" with its own kind', () => {
    expect(NO_SPLIT).toEqual({ kind: 'none', key: 'none' });
    expect(SELECTION_KEY).toBe('selection');
  });

  describe('withNoSplit', () => {
    it('puts "don\'t split" before the real options', () => {
      const choices = withNoSplit(real);
      expect(choices.map((g) => g.key)).toEqual([
        'none',
        'group:g-strategy',
        'group:g-region',
        'sector',
        'currency',
      ]);
      expect(choices[0]).toBe(NO_SPLIT);
    });

    it('offers nothing without a real option', () => {
      expect(withNoSplit([])).toEqual([]);
    });
  });

  describe('chooseGroup', () => {
    it('defaults to the first real option, not to "don\'t split"', () => {
      expect(chooseGroup(real, null)?.key).toBe('group:g-strategy');
    });

    it('keeps the remembered real option', () => {
      expect(chooseGroup(real, 'sector')?.key).toBe('sector');
      expect(chooseGroup(real, 'group:g-region')?.key).toBe('group:g-region');
    });

    it('keeps "none" whenever a real option exists', () => {
      expect(chooseGroup(real, 'none')).toBe(NO_SPLIT);
      expect(chooseGroup(real.slice(2), 'none')).toBe(NO_SPLIT);
    });

    it('falls back to the first real option for a key that no longer exists', () => {
      expect(chooseGroup(real, 'group:deleted')?.key).toBe('group:g-strategy');
      expect(chooseGroup(real.slice(2), 'group:g-strategy')?.key).toBe('sector');
    });

    it('is no group at all without a real option, even when "none" is remembered', () => {
      expect(chooseGroup([], null)).toBeNull();
      expect(chooseGroup([], 'none')).toBeNull();
      expect(chooseGroup([], 'sector')).toBeNull();
    });
  });

  describe('splitDividers', () => {
    it('divides "don\'t split", the tag groups and the automatic groups', () => {
      expect(splitDividers(withNoSplit(real))).toEqual([false, true, false, true, false]);
    });

    it('has one divider less where a kind is missing', () => {
      const auto = real.filter((g) => g.kind !== 'tags');
      const tagsOnly = real.filter((g) => g.kind === 'tags');
      expect(splitDividers(withNoSplit(auto))).toEqual([false, true, false]);
      expect(splitDividers(withNoSplit(tagsOnly))).toEqual([false, true, false]);
      expect(splitDividers(withNoSplit(real.slice(2, 3)))).toEqual([false, true]);
    });

    it('is empty without choices', () => {
      expect(splitDividers([])).toEqual([]);
    });
  });

  describe('splitControl', () => {
    const many = (count: number) =>
      Array.from({ length: count }, (_, i): CompareGroup => ({
        kind: 'tags',
        key: `group:${i}`,
        group: tagGroup(`${i}`, `G${i}`, i),
        tags: [],
      }));

    it('is hidden without a real option', () => {
      expect(splitControl([])).toBe('hidden');
      expect(splitControl([NO_SPLIT])).toBe('hidden');
    });

    it('is segmented up to four real options and a select beyond', () => {
      expect(MAX_SEGMENTED_SPLITS).toBe(4);
      expect(splitControl(withNoSplit(many(1)))).toBe('segmented');
      expect(splitControl(withNoSplit(many(4)))).toBe('segmented');
      expect(splitControl(withNoSplit(many(5)))).toBe('select');
    });

    it('does not count "don\'t split" (four real options plus it are five buttons)', () => {
      const choices = withNoSplit(many(4));
      expect(choices).toHaveLength(5);
      expect(splitControl(choices)).toBe('segmented');
    });
  });

  describe('noSplitParts', () => {
    const selected = [a, b];

    it('has no part while no filter is on', () => {
      expect(noSplitParts(selected, false)).toEqual([]);
    });

    it('is one part, the selection, with a filter on', () => {
      expect(noSplitParts(selected, true)).toEqual([
        {
          key: 'selection',
          positionIds: ['a', 'b'],
          tag: null,
          sector: null,
          currency: null,
          isNone: false,
        },
      ]);
    });

    it('has no part when the filter selects nothing', () => {
      expect(noSplitParts([], true)).toEqual([]);
    });
  });

  describe('splitParts', () => {
    it('gives "don\'t split" the selection only with a filter on', () => {
      expect(splitParts([a, b], NO_SPLIT, allTags, false)).toEqual([]);
      expect(splitParts([a, b], NO_SPLIT, allTags, true).map((p) => p.key)).toEqual(['selection']);
    });

    it('gives a real group its own parts whatever the filter', () => {
      const strategyGroup = real[0];
      expect(splitParts([a, b], strategyGroup, allTags, false)).toEqual(
        groupParts([a, b], strategyGroup, allTags)
      );
      expect(splitParts([a, b], strategyGroup, allTags, true).map((p) => p.key)).toEqual([
        'tag:t-div',
        'tag:t-growth',
      ]);
    });

    it('leaves groupParts without parts for "don\'t split" (it is not a currency group)', () => {
      expect(groupParts([a, b], NO_SPLIT, allTags)).toEqual([]);
    });
  });
});

describe('seriesSpecs', () => {
  const selected = [
    pos('a', { sector: 'Technologie', currency: 'USD', tags: [growth, usa] }),
    pos('b', { sector: 'Finance', currency: 'EUR', tags: [dividend, usa] }),
    pos('c', { sector: null, currency: 'CZK', tags: [] }),
  ];
  const groups = compareGroups(allGroups, allTags, selected);
  const byKey = (key: string) => groups.find((g) => g.key === key) as CompareGroup;

  it('gives one series per tag carried by a selected position', () => {
    const specs = seriesSpecs(selected, allTags, null, 'CZK');
    expect(specs.filter((s) => s.key.startsWith('tag:'))).toEqual([
      { key: 'tag:t-usa', investmentIds: ['a', 'b'] },
      { key: 'tag:t-growth', investmentIds: ['a'] },
      { key: 'tag:t-div', investmentIds: ['b'] },
    ]);
  });

  it('gives one series per tag group, of the positions carrying any of its tags', () => {
    const withEu = [...selected, pos('d', { tags: [eu, loose] })];
    const specs = seriesSpecs(withEu, allTags, null, 'CZK');
    expect(specs.filter((s) => !s.key.startsWith('tag:'))).toEqual([
      { key: 'tag-group:g-region', investmentIds: ['a', 'b', 'd'] },
      { key: 'tag-group:g-strategy', investmentIds: ['a', 'b'] },
    ]);
    expect(tagGroupSeriesKey('g-region')).toBe('tag-group:g-region');
  });

  it('does not repeat a tag series for tag parts of the chosen group', () => {
    const specs = seriesSpecs(selected, allTags, byKey('group:g-strategy'), 'CZK');
    const keys = specs.map((s) => s.key);
    expect(new Set(keys).size).toBe(keys.length);
    expect(keys).toEqual([
      'tag:t-usa',
      'tag:t-growth',
      'tag:t-div',
      'none:g-strategy',
      'tag-group:g-region',
      'tag-group:g-strategy',
    ]);
    expect(specs[3].investmentIds).toEqual(['c']);
  });

  it('adds the sector parts that are not tag parts', () => {
    const keys = seriesSpecs(selected, allTags, byKey('sector'), 'CZK').map((s) => s.key);
    expect(keys).toEqual([
      'tag:t-usa',
      'tag:t-growth',
      'tag:t-div',
      'sector:Finance',
      'sector:Technologie',
      'sector:',
      'tag-group:g-region',
      'tag-group:g-strategy',
    ]);
  });

  it('adds native series only for currencies other than the report currency', () => {
    const specs = seriesSpecs(selected, allTags, byKey('currency'), 'CZK');
    const currencyKeys = specs.filter((s) => s.key.startsWith('currency:')).map((s) => s.key);
    expect(currencyKeys).toEqual([
      'currency:CZK',
      'currency:EUR',
      'currency:EUR:native',
      'currency:USD',
      'currency:USD:native',
    ]);
    const eurNative = specs.find((s) => s.key === 'currency:EUR:native');
    expect(eurNative).toEqual({
      key: 'currency:EUR:native',
      investmentIds: ['b'],
      currency: 'EUR',
    });
    const czk = specs.find((s) => s.key === 'currency:CZK');
    expect(czk?.currency).toBeUndefined();
  });

  it('has no native series when every position is in the report currency', () => {
    const czkOnly = [pos('a', { currency: 'CZK' }), pos('b', { currency: 'CZK' })];
    const specs = seriesSpecs(czkOnly, [], { kind: 'currency', key: 'currency' }, 'CZK');
    expect(specs).toEqual([{ key: 'currency:CZK', investmentIds: ['a', 'b'] }]);
  });

  it('is empty for no selected positions', () => {
    expect(seriesSpecs([], allTags, byKey('sector'), 'CZK')).toEqual([]);
  });

  it('adds the selection series of "don\'t split" with a filter on', () => {
    const specs = seriesSpecs(selected, allTags, NO_SPLIT, 'CZK', true);
    expect(specs).toEqual([
      { key: 'tag:t-usa', investmentIds: ['a', 'b'] },
      { key: 'tag:t-growth', investmentIds: ['a'] },
      { key: 'tag:t-div', investmentIds: ['b'] },
      { key: 'selection', investmentIds: ['a', 'b', 'c'] },
      { key: 'tag-group:g-region', investmentIds: ['a', 'b'] },
      { key: 'tag-group:g-strategy', investmentIds: ['a', 'b'] },
    ]);
  });

  it('asks for no selection series of "don\'t split" without a filter', () => {
    const expected = seriesSpecs(selected, allTags, null, 'CZK');
    expect(seriesSpecs(selected, allTags, NO_SPLIT, 'CZK', false)).toEqual(expected);
    expect(seriesSpecs(selected, allTags, NO_SPLIT, 'CZK')).toEqual(expected);
    expect(expected.some((s) => s.key === 'selection')).toBe(false);
  });

  it('never adds the selection series for a real group, filter or not', () => {
    const keys = seriesSpecs(selected, allTags, byKey('sector'), 'CZK', true).map((s) => s.key);
    expect(keys).not.toContain('selection');
  });

  it('asks for no selection series when the filter selects nothing', () => {
    expect(seriesSpecs([], allTags, NO_SPLIT, 'CZK', true)).toEqual([]);
  });
});

describe('seriesSpecs limit', () => {
  /** `perGroup` tags in each of two groups, every position carrying one tag of both. */
  function crowd(perGroup: number) {
    const groups = [tagGroup('g-a', 'A', 1), tagGroup('g-b', 'B', 2)];
    const tags = groups.flatMap((g) =>
      Array.from({ length: perGroup }, (_, i) => tag(`${g.id}-${i}`, `${g.name}${i}`, g.id))
    );
    const positions = Array.from({ length: perGroup }, (_, i) =>
      pos(`p${i}`, { tags: [tags[i], tags[perGroup + i]] })
    );
    return { groups, tags, positions };
  }

  it('is the backend limit', () => {
    expect(MAX_SERIES).toBe(256);
  });

  it('keeps every tag series while they fit, and the two group series', () => {
    const { groups, tags, positions } = crowd(100);
    const g = compareGroups(groups, tags, positions)[0];
    expect(seriesSpecs(positions, tags, g, 'CZK')).toHaveLength(202);
  });

  it('leaves the group series out when they would not fit', () => {
    const { groups, tags, positions } = crowd(128);
    const g = compareGroups(groups, tags, positions)[0];
    const specs = seriesSpecs(positions, tags, g, 'CZK');
    expect(specs).toHaveLength(MAX_SERIES);
    expect(specs.some((s) => s.key.startsWith('tag-group:'))).toBe(false);
  });

  it('drops the tag series outside the chosen group first when there are too many', () => {
    const { groups, tags, positions } = crowd(150);
    const options = compareGroups(groups, tags, positions);
    const specs = seriesSpecs(positions, tags, options[1], 'CZK');
    expect(specs.length).toBeLessThanOrEqual(MAX_SERIES);
    const keys = specs.map((s) => s.key);
    // all 150 tags of the chosen group B stay, none of group A
    expect(keys.filter((k) => k.startsWith('tag:g-b-'))).toHaveLength(150);
    expect(keys.some((k) => k.startsWith('tag:g-a-'))).toBe(false);
  });

  it('drops every tag series when the chosen split is not a tag group', () => {
    const { groups, tags, positions } = crowd(150);
    const sectors = positions.map((p, i) => ({ ...p, sector: i < 75 ? 'X' : 'Y' }));
    const g = compareGroups(groups, tags, sectors).find((o) => o.key === 'sector') as CompareGroup;
    expect(seriesSpecs(sectors, tags, g, 'CZK').map((s) => s.key)).toEqual([
      'sector:X',
      'sector:Y',
    ]);
  });

  it('drops them all without a chosen split too (the portfolio line needs no series)', () => {
    const { tags, positions } = crowd(150);
    expect(seriesSpecs(positions, tags, null, 'CZK')).toEqual([]);
  });

  it('keeps the selection series of "don\'t split" when the tag series are dropped', () => {
    const { tags, positions } = crowd(150);
    const specs = seriesSpecs(positions, tags, NO_SPLIT, 'CZK', true);
    expect(specs.map((s) => s.key)).toEqual(['selection']);
    expect(specs[0].investmentIds).toHaveLength(150);
  });

  it('never exceeds the limit, even for one oversized group', () => {
    const { groups, tags, positions } = crowd(300);
    const specs = seriesSpecs(positions, tags, compareGroups(groups, tags, positions)[0], 'CZK');
    expect(specs).toHaveLength(MAX_SERIES);
    expect(new Set(specs.map((s) => s.key)).size).toBe(MAX_SERIES);
    expect(specs.every((s) => s.key.startsWith('tag:g-a-'))).toBe(true);
  });
});

describe('positionCurrency', () => {
  it('prefers the price currency and falls back to the average price currency', () => {
    expect(positionCurrency({ priceCurrency: 'USD', averagePriceCurrency: 'EUR' })).toBe('USD');
    expect(positionCurrency({ priceCurrency: null, averagePriceCurrency: 'EUR' })).toBe('EUR');
  });
});

describe('concentration', () => {
  it('computes the top-five share and the effective number of positions', () => {
    const c = concentration([50, 30, 20]);
    expect(c.top5Share).toBeCloseTo(1, 10);
    expect(c.effective).toBeCloseTo(1 / (0.25 + 0.09 + 0.04), 10);
    expect(c.effective).toBeCloseTo(2.63, 2);
  });

  it('counts only the five largest in the share', () => {
    const c = concentration([30, 25, 15, 10, 8, 7, 5]);
    expect(c.top5Share).toBeCloseTo(0.88, 10);
  });

  it('is zero for an empty or zero total', () => {
    expect(concentration([])).toEqual({ top5Share: 0, effective: 0 });
    expect(concentration([0, 0])).toEqual({ top5Share: 0, effective: 0 });
  });

  it('treats equal weights as that many effective positions', () => {
    expect(concentration([10, 10, 10, 10]).effective).toBeCloseTo(4, 10);
  });
});

describe('splitTop', () => {
  const size = (n: number) => n;

  it('keeps everything up to the limit', () => {
    const items = Array.from({ length: 9 }, (_, i) => i + 1);
    expect(splitTop(items, size)).toEqual({ shown: items, rest: [] });
    expect(splitTop([], size)).toEqual({ shown: [], rest: [] });
  });

  it('keeps the eight largest by absolute size from ten items', () => {
    const items = [5, -1, 9, 2, -8, 7, 3, 6, -4, 10]; // smallest |size|: -1 and 2
    const { shown, rest } = splitTop(items, size);
    expect(shown).toHaveLength(8);
    expect(rest).toEqual([-1, 2]);
    expect(shown).toEqual([5, 9, -8, 7, 3, 6, -4, 10]); // original order kept
  });

  it('honours custom keep and limit', () => {
    const { shown, rest } = splitTop([1, 2, 3, 4, 5, 6], size, 2, 4);
    expect(shown).toEqual([5, 6]);
    expect(rest).toEqual([1, 2, 3, 4]);
    expect(splitTop([1, 2, 3, 4], size, 2, 4).rest).toEqual([]);
  });
});

describe('seriesValueAt', () => {
  it('carries the last benchmark point forward and is null before the first', () => {
    const dates = [
      d('2026-01-01'),
      d('2026-01-05'),
      d('2026-01-06'),
      d('2026-01-09'),
      d('2026-01-20'),
    ];
    const benchDates = [d('2026-01-03'), d('2026-01-06'), d('2026-01-10')];
    const bench = [1, 2, 3];
    expect(seriesValueAt(dates, benchDates, bench)).toEqual([null, 1, 2, 2, 3]);
  });

  it('matches a benchmark point on the same day', () => {
    expect(seriesValueAt([10, 20], [10, 20], [5, 6])).toEqual([5, 6]);
  });

  it('is all null for an empty benchmark', () => {
    expect(seriesValueAt([1, 2], [], [])).toEqual([null, null]);
  });
});

describe('rebaseReturns', () => {
  it('moves the zero point to the first value (returns compound, they do not subtract)', () => {
    // +10 % at the start, +21 % later: 1.21 / 1.10 − 1 = +10 %
    const rebased = rebaseReturns([10, 21, 0]);
    expect(rebased[0]).toBeCloseTo(0, 10);
    expect(rebased[1]).toBeCloseTo(10, 10);
    expect(rebased[2]).toBeCloseTo((1 / 1.1 - 1) * 100, 10);
  });

  it('leaves a series that already starts at zero as it is', () => {
    expect(rebaseReturns([0, 5, -2])).toEqual([0, 5, -2]);
  });

  it('keeps gaps as gaps', () => {
    expect(rebaseReturns([10, null, 21])[1]).toBeNull();
  });

  it('has nothing to rebase from when the first day has no value', () => {
    expect(rebaseReturns([null, 5, 6])).toEqual([null, null, null]);
    expect(rebaseReturns([])).toEqual([]);
  });

  it('is all gaps for a total loss at the start (no base to divide by)', () => {
    expect(rebaseReturns([-100, 5])).toEqual([null, null]);
  });
});

describe('sameIndexLine', () => {
  const request = { ticker: 'SPY', reportCurrency: 'CZK' };

  it('lets the previous line stand in for the same index in the same currency', () => {
    expect(sameIndexLine({ ...request }, request)).toBe(true);
  });

  it('does not show another index under the new name', () => {
    expect(sameIndexLine({ ticker: 'QQQ', reportCurrency: 'CZK' }, request)).toBe(false);
  });

  it('does not carry returns over to another currency', () => {
    expect(sameIndexLine({ ticker: 'SPY', reportCurrency: 'EUR' }, request)).toBe(false);
  });

  it('has nothing to keep without a previous or a next request', () => {
    expect(sameIndexLine(null, request)).toBe(false);
    expect(sameIndexLine(undefined, request)).toBe(false);
    expect(sameIndexLine(request, null)).toBe(false);
  });
});
