import { describe, expect, it } from 'vitest';
import { MAX_AXIS_TICKS, dayLabelOptions, evenTicks, utcDayFloor } from './chart-axis';

describe('evenTicks', () => {
  it('returns everything for short series', () => {
    expect(evenTicks(['a', 'b', 'c'])).toEqual(['a', 'b', 'c']);
    expect(evenTicks([])).toEqual([]);
    expect(evenTicks(['only'])).toEqual(['only']);
  });

  it('caps the number of ticks and keeps first and last', () => {
    const labels = Array.from({ length: 90 }, (_, i) => `d${i}`);
    const ticks = evenTicks(labels);
    expect(ticks).toHaveLength(MAX_AXIS_TICKS);
    expect(ticks[0]).toBe('d0');
    expect(ticks[ticks.length - 1]).toBe('d89');
  });

  it('spaces the picks evenly (gaps differ by at most one point)', () => {
    const labels = Array.from({ length: 31 }, (_, i) => i);
    const ticks = evenTicks(labels, 8);
    const gaps = ticks.slice(1).map((v, i) => v - ticks[i]);
    expect(Math.max(...gaps) - Math.min(...gaps)).toBeLessThanOrEqual(1);
  });

  it('collapses repeated labels', () => {
    expect(evenTicks(['x', 'x', 'x', 'y'], 4)).toEqual(['x', 'y']);
  });
});

describe('dayLabelOptions', () => {
  it('adds the year only for multi-year periods', () => {
    expect(dayLabelOptions('30D')).toEqual({ month: 'short', day: 'numeric' });
    expect(dayLabelOptions('YTD')).toEqual({ month: 'short', day: 'numeric' });
    expect(dayLabelOptions('1Y')).toEqual({ month: 'short', day: 'numeric', year: '2-digit' });
    expect(dayLabelOptions('All')).toEqual({ month: 'short', day: 'numeric', year: '2-digit' });
  });
});

describe('utcDayFloor', () => {
  it('floors to UTC midnight', () => {
    // 2026-09-30T23:59:59Z -> 2026-09-30T00:00:00Z
    expect(utcDayFloor(1_790_726_400 + 86399)).toBe(1_790_726_400);
    expect(utcDayFloor(1_790_726_400)).toBe(1_790_726_400);
  });
});
