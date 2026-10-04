import { describe, expect, it } from 'vitest';
import {
  axisStamps,
  clusterEvents,
  isShortSpan,
  nearestIndex,
  niceStep,
  niceTicks,
  valueAt,
  valueDomain,
  type ChartEvent,
} from './chart-scale';

const DAY = 86400;

describe('niceStep / niceTicks', () => {
  it('picks 1-2-2.5-5-10 steps', () => {
    expect(niceStep(100, 4)).toBe(25);
    expect(niceStep(1000, 4)).toBe(250);
    expect(niceStep(90, 4)).toBe(25);
    expect(niceStep(7, 4)).toBe(2);
    expect(niceStep(2_846_900, 4)).toBe(1_000_000);
  });

  it('never divides into more than about n parts', () => {
    for (const span of [3, 17, 99, 1234, 999_999]) {
      expect(niceTicks(0, span, 4).length).toBeLessThanOrEqual(6);
    }
  });

  it('starts at a multiple of the step inside the range', () => {
    expect(niceTicks(130_000, 420_000, 4)).toEqual([200_000, 300_000, 400_000]);
    expect(niceTicks(0, 100, 4)).toEqual([0, 25, 50, 75, 100]);
  });

  it('degrades gracefully on empty ranges', () => {
    expect(niceStep(0, 4)).toBe(1);
    expect(niceTicks(5, 5)).toEqual([5]);
  });
});

describe('valueDomain (README §8 baseline rule)', () => {
  it('truncates with 15 % headroom below and 8 % above', () => {
    const [lo, hi] = valueDomain([200, 300]);
    expect(lo).toBeCloseTo(185);
    expect(hi).toBeCloseTo(308);
  });

  it('never goes below zero for non-negative data', () => {
    const [lo] = valueDomain([5, 100]);
    expect(lo).toBe(0);
  });

  it('uses zero as the baseline when asked', () => {
    expect(valueDomain([200, 300], { zeroBaseline: true })).toEqual([0, 324]);
  });

  it('gives a flat series a visible band', () => {
    const [lo, hi] = valueDomain([100, 100]);
    expect(lo).toBeLessThan(100);
    expect(hi).toBeGreaterThan(100);
  });

  it('keeps negative data negative', () => {
    const [lo] = valueDomain([-50, 10]);
    expect(lo).toBeLessThan(-50);
  });

  it('ignores NaN and handles empty input', () => {
    expect(valueDomain([])).toEqual([0, 1]);
    expect(valueDomain([NaN, 10, 20])[1]).toBeCloseTo(20.8);
  });
});

describe('axisStamps / isShortSpan', () => {
  it('returns n evenly spaced stamps including both ends', () => {
    expect(axisStamps(0, 400, 5)).toEqual([0, 100, 200, 300, 400]);
  });

  it('collapses to the end for a degenerate range', () => {
    expect(axisStamps(10, 10)).toEqual([10]);
  });

  it('calls 99 days short and 100 days long', () => {
    expect(isShortSpan(0, 99 * DAY)).toBe(true);
    expect(isShortSpan(0, 100 * DAY)).toBe(false);
  });
});

describe('nearestIndex / valueAt', () => {
  const pts = [
    { t: 0, value: 10 },
    { t: 10, value: 20 },
    { t: 20, value: 40 },
  ];

  it('finds the nearest point', () => {
    expect(nearestIndex(pts, -5)).toBe(0);
    expect(nearestIndex(pts, 4)).toBe(0);
    expect(nearestIndex(pts, 6)).toBe(1);
    expect(nearestIndex(pts, 99)).toBe(2);
    expect(nearestIndex([], 1)).toBe(-1);
  });

  it('interpolates between points and clamps at the ends', () => {
    expect(valueAt(pts, 5)).toBe(15);
    expect(valueAt(pts, 15)).toBe(30);
    expect(valueAt(pts, -1)).toBe(10);
    expect(valueAt(pts, 100)).toBe(40);
    expect(valueAt([], 1)).toBe(0);
  });
});

describe('clusterEvents (README §9)', () => {
  const mk = (n: number, type: ChartEvent['type'] = 'buy'): ChartEvent[] =>
    Array.from({ length: n }, (_, i) => ({ id: `e${i}`, t: i * DAY, type }));

  it('keeps one mark per event while there are 24 or fewer in view', () => {
    const out = clusterEvents(mk(24), 0, 30 * DAY);
    expect(out).toHaveLength(24);
    expect(out.every((c) => c.events.length === 1)).toBe(true);
  });

  it('drops events outside the period', () => {
    const out = clusterEvents(mk(10), 3 * DAY, 6 * DAY);
    expect(out.map((c) => c.id)).toEqual(['e3', 'e4', 'e5', 'e6']);
  });

  it('folds neighbours into at most 24 marks when there are more', () => {
    const out = clusterEvents(mk(60), 0, 60 * DAY);
    expect(out.length).toBeLessThanOrEqual(24);
    expect(out.reduce((s, c) => s + c.events.length, 0)).toBe(60);
    expect(out.some((c) => c.events.length > 1)).toBe(true);
  });

  it('labels a cluster with its most frequent type and the first id', () => {
    const events: ChartEvent[] = [
      ...mk(30, 'income'),
      ...Array.from({ length: 5 }, (_, i) => ({ id: `b${i}`, t: i * DAY, type: 'buy' as const })),
    ];
    const out = clusterEvents(events, 0, 30 * DAY, 2);
    expect(out[0].type).toBe('income');
    expect(out[0].id).toBe('e0');
  });
});
