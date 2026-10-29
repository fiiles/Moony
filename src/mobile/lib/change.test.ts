import { describe, expect, it } from 'vitest';
import { changeSinceDayOptions, lineChange, periodChange } from './change';

describe('periodChange', () => {
  it('relates today to the base', () => {
    expect(periodChange(6_775_680, 6_683_580)).toEqual({
      delta: 92_100,
      ratio: 92_100 / 6_683_580,
    });
  });
  it('has no ratio against a zero base and measures against the magnitude of a negative one', () => {
    expect(periodChange(500, 0)).toEqual({ delta: 500, ratio: null });
    expect(periodChange(-50, -100)).toEqual({ delta: 50, ratio: 0.5 });
  });
  it('is null without a base or without today', () => {
    expect(periodChange(500, undefined)).toBeNull();
    expect(periodChange(500, null)).toBeNull();
    expect(periodChange(null, 100)).toBeNull();
  });
});

describe('changeSinceDayOptions', () => {
  const base = Date.UTC(2026, 9, 3) / 1000;
  it('writes the base day short, without the year in the current one', () => {
    expect(changeSinceDayOptions(base, Date.UTC(2026, 9, 17))).toEqual({
      month: 'short',
      day: 'numeric',
    });
  });
  it('adds the year when the base is not in the current one', () => {
    expect(changeSinceDayOptions(base, Date.UTC(2027, 0, 5))).toEqual({
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    });
  });
});

describe('lineChange', () => {
  const DAY = 86_400;
  const start = 1_000 * DAY;

  it('measures the last point against the first', () => {
    const points = [
      { t: start, value: 400 },
      { t: start + 10 * DAY, value: 380 },
      { t: start + 30 * DAY, value: 500 },
    ];
    expect(lineChange(points, start)).toEqual({ delta: 100, ratio: 0.25, sinceDay: null });
  });

  it('names the first day when the line starts well after the period does', () => {
    const points = [
      { t: start + 12 * DAY + 3_600, value: 200 },
      { t: start + 30 * DAY, value: 100 },
    ];
    expect(lineChange(points, start)).toEqual({
      delta: -100,
      ratio: -0.5,
      sinceDay: start + 12 * DAY,
    });
  });

  it('has nothing to say about fewer than two points', () => {
    expect(lineChange([], start)).toBeNull();
    expect(lineChange([{ t: start, value: 5 }], start)).toBeNull();
  });
});
