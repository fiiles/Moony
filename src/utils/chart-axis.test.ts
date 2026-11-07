import { describe, expect, it } from 'vitest';
import {
  MAX_AXIS_TICKS,
  dayLabelOptions,
  distinctTickLabels,
  evenTicks,
  tickGutter,
  utcDayFloor,
} from './chart-axis';

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

describe('distinctTickLabels', () => {
  // A stand-in for the compact formatter: millions with at most `digits` decimals
  const millions = (value: number, digits: number) =>
    `${Number((value / 1e6).toFixed(digits))} mil.`;

  it('keeps one decimal when the labels already differ', () => {
    const labels = distinctTickLabels([0, 5e6, 10e6, 15e6], millions);
    expect([...labels.values()]).toEqual(['0 mil.', '5 mil.', '10 mil.', '15 mil.']);
  });

  it('adds decimals until adjacent ticks differ', () => {
    const ticks = [11_210_000, 11_240_000, 11_270_000];
    expect(millions(ticks[0], 1)).toBe(millions(ticks[1], 1));
    const labels = distinctTickLabels(ticks, millions);
    expect([...labels.values()]).toEqual(['11.21 mil.', '11.24 mil.', '11.27 mil.']);
  });

  it('gives up at the limit instead of looping', () => {
    const labels = distinctTickLabels([1_000_001, 1_000_002], millions, 2);
    expect(labels.get(1_000_001)).toBe(labels.get(1_000_002));
  });

  it('ignores a tick listed twice', () => {
    expect(distinctTickLabels([5e6, 5e6], millions).size).toBe(1);
  });
});

describe('tickGutter', () => {
  const nbsp = '\u00a0';

  it('keeps the standard width for labels that fit today', () => {
    expect(tickGutter([`11,2${nbsp}mil.`], 54)).toBe(54);
    expect(tickGutter([`250${nbsp}tis.`], 54)).toBe(54);
    expect(tickGutter([`1,5${nbsp}mld.`], 54)).toBe(54);
  });

  // The measured widths of the label plus the 16 px between plot and label must fit the gutter
  it.each([
    [`11,24${nbsp}mil.`, 43.5],
    [`11,24${nbsp}Mio.`, 47.3],
    [`\u221211,24${nbsp}mill.`, 52.7],
    [`-11,24${nbsp}mil.`, 48.1],
    [`11,24${nbsp}Mln`, 44.2],
    [`112,4${nbsp}mil.`, 43.7],
    [`1,12${nbsp}mld.`, 39.9],
  ])('holds %s (%s px)', (label, measured) => {
    expect(tickGutter([label], 54) - 16).toBeGreaterThanOrEqual(measured);
  });

  it('widens for the extra decimal', () => {
    expect(tickGutter([`11,24${nbsp}mil.`], 54)).toBeGreaterThanOrEqual(60);
    expect(tickGutter([`11,24${nbsp}Mio.`], 54)).toBeGreaterThanOrEqual(64);
    expect(tickGutter([`\u221211,24${nbsp}mill.`], 54)).toBeGreaterThanOrEqual(69);
  });

  it('is the minimum without labels', () => {
    expect(tickGutter([], 54)).toBe(54);
  });
});
