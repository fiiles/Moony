import { describe, expect, it } from 'vitest';
import { flowBridge, flowLabel } from './flows-model';

const utc = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;

describe('flowBridge', () => {
  it('splits the change into deposits and market', () => {
    expect(flowBridge([1000, 1200, 1500], [0, 100, 300])).toEqual({
      start: 1000,
      deposits: 300,
      market: 200,
      end: 1500,
    });
  });

  it('shows a loss of market value against a deposit', () => {
    const bridge = flowBridge([1000, 1400], [0, 600]);
    expect(bridge?.market).toBe(-200);
  });

  it('keeps what is known when a value is missing', () => {
    expect(flowBridge([1000, null], [0, 50])).toEqual({
      start: 1000,
      deposits: 50,
      market: null,
      end: null,
    });
    expect(flowBridge([null, 900], [0, 0])).toMatchObject({ start: null, market: null, end: 900 });
  });

  it('has nothing without points', () => {
    expect(flowBridge([], [])).toBeNull();
  });
});

describe('flowLabel', () => {
  it('labels a week by its first day', () => {
    expect(flowLabel({ start: utc(2026, 7, 7) }, 'week', false)).toEqual({
      kind: 'week',
      ts: utc(2026, 7, 7),
    });
  });

  it('writes the year of a month on January and on the first bucket only', () => {
    expect(flowLabel({ start: utc(2026, 8, 3) }, 'month', true)).toMatchObject({ withYear: true });
    expect(flowLabel({ start: utc(2026, 8, 3) }, 'month', false)).toMatchObject({
      withYear: false,
    });
    expect(flowLabel({ start: utc(2027, 1, 4) }, 'month', false)).toMatchObject({ withYear: true });
  });

  it('numbers a quarter from the month of its first day', () => {
    expect(flowLabel({ start: utc(2025, 7, 1) }, 'quarter', false)).toEqual({
      kind: 'quarter',
      quarter: 3,
      year: 2025,
    });
    expect(flowLabel({ start: utc(2026, 1, 2) }, 'quarter', false)).toMatchObject({ quarter: 1 });
    expect(flowLabel({ start: utc(2026, 12, 15) }, 'quarter', false)).toMatchObject({ quarter: 4 });
  });

  it('labels a year by its number', () => {
    expect(flowLabel({ start: utc(2024, 1, 2) }, 'year', false)).toEqual({
      kind: 'year',
      year: 2024,
    });
  });
});
