import { describe, expect, it } from 'vitest';
import { nearestPoint, splitProjected, stepValueAt, toPlotX, toPlotY } from './scrub';

describe('nearestPoint', () => {
  const points = [{ x: 0 }, { x: 10 }, { x: 20 }];

  it('picks the point nearest to the finger', () => {
    expect(nearestPoint(points, 12, 20)).toBe(1);
    expect(nearestPoint(points, 16, 20)).toBe(2);
    expect(nearestPoint(points, 4, 20)).toBe(0);
  });

  it('gives the last point for a finger beyond the end', () => {
    expect(nearestPoint(points, 35, 20)).toBe(2);
  });

  it('gives the first point for a finger before the start', () => {
    expect(nearestPoint(points, -8, 20)).toBe(0);
  });

  it('keeps the earlier point on a tie', () => {
    expect(nearestPoint(points, 5, 20)).toBe(0);
  });

  it('gives null without points', () => {
    expect(nearestPoint([], 12, 20)).toBeNull();
  });

  it('gives the only point of a one-point series', () => {
    expect(nearestPoint([{ x: 7 }], 300, 320)).toBe(0);
  });
});

describe('toPlotX', () => {
  it('maps the time span onto the plot width', () => {
    expect(toPlotX(100, 100, 200, 300)).toBe(0);
    expect(toPlotX(150, 100, 200, 300)).toBe(150);
    expect(toPlotX(200, 100, 200, 300)).toBe(300);
  });

  it('puts a single-day span at the right edge', () => {
    expect(toPlotX(100, 100, 100, 300)).toBe(300);
  });
});

describe('toPlotY', () => {
  it('maps the value domain onto the plot height, top down', () => {
    // domain 0–100 over a 140 px plot starting 14 px from the top
    expect(toPlotY(100, [0, 100], 14, 140)).toBe(14);
    expect(toPlotY(0, [0, 100], 14, 140)).toBe(154);
    expect(toPlotY(50, [0, 100], 14, 140)).toBe(84);
  });

  it('centres a flat domain', () => {
    expect(toPlotY(5, [5, 5], 14, 140)).toBe(84);
  });
});

describe('stepValueAt', () => {
  const points = [
    { t: 0, value: 1 },
    { t: 10, value: 5 },
  ];

  it('holds the latest point at or before the time', () => {
    expect(stepValueAt(points, 7)).toBe(1);
    expect(stepValueAt(points, 10)).toBe(5);
    expect(stepValueAt(points, 30)).toBe(5);
  });

  it('gives the first value before the first point', () => {
    expect(stepValueAt(points, -4)).toBe(1);
  });
});

describe('splitProjected', () => {
  const points = [
    { t: 0, value: 10 },
    { t: 10, value: 8 },
    { t: 20, value: 6 },
  ];

  it('is the plain line without a split: every point actual, none projected', () => {
    const rows = splitProjected(points, undefined);
    expect(rows).toHaveLength(3);
    expect(rows.map((p) => p.actual)).toEqual([10, 8, 6]);
    expect(rows.every((p) => !('projected' in p) || p.projected === undefined)).toBe(true);
  });

  it('keeps the other fields of a point', () => {
    const rows = splitProjected([{ t: 0, value: 1, cost: 4 }], undefined);
    expect(rows[0]).toMatchObject({ t: 0, value: 1, cost: 4, actual: 1 });
  });

  it('draws the points up to the split as actual and from it as projected', () => {
    const rows = splitProjected(points, 10);
    expect(rows).toHaveLength(3);
    expect(rows.map((p) => p.actual)).toEqual([10, 8, undefined]);
    expect(rows.map((p) => p.projected)).toEqual([undefined, 8, 6]);
  });

  it('inserts a point at the split so that both lines meet, at the step value before it', () => {
    const rows = splitProjected(points, 14);
    expect(rows.map((p) => p.t)).toEqual([0, 10, 14, 20]);
    const meet = rows[2];
    expect(meet.value).toBe(8);
    expect(meet.actual).toBe(8);
    expect(meet.projected).toBe(8);
    expect(rows[1].actual).toBe(8);
    expect(rows[1].projected).toBeUndefined();
    expect(rows[3].actual).toBeUndefined();
    expect(rows[3].projected).toBe(6);
  });

  it('adds nothing when the split is outside the range', () => {
    expect(splitProjected(points, -5)).toHaveLength(3);
    expect(splitProjected(points, 99)).toHaveLength(3);
    // Everything is plan before the range, everything actual after it
    expect(splitProjected(points, -5).every((p) => p.projected !== undefined)).toBe(true);
    expect(splitProjected(points, 99).every((p) => p.actual !== undefined)).toBe(true);
  });

  it('does not duplicate a split that sits on a point', () => {
    expect(splitProjected(points, 10).map((p) => p.t)).toEqual([0, 10, 20]);
    expect(splitProjected(points, 0).map((p) => p.t)).toEqual([0, 10, 20]);
    expect(splitProjected(points, 20).map((p) => p.t)).toEqual([0, 10, 20]);
  });

  it('gives no rows for no points', () => {
    expect(splitProjected([], 5)).toEqual([]);
  });
});
