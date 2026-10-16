import { describe, expect, it } from 'vitest';
import { nearestPoint, toPlotX, toPlotY } from './scrub';

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
