import { describe, expect, it } from 'vitest';
import {
  anyEstimated,
  isEstimatedSource,
  splitEstimated,
  withCumulativeTops,
  withEstimatedSplit,
} from './estimated-series';

describe('isEstimatedSource', () => {
  it('flags only reconstructed rows', () => {
    expect(isEstimatedSource('backfill')).toBe(true);
    expect(isEstimatedSource('live')).toBe(false);
    expect(isEstimatedSource(undefined)).toBe(false);
  });
});

describe('splitEstimated', () => {
  it('keeps a fully live series solid', () => {
    const { solid, dashed } = splitEstimated([1, 2, 3], [false, false, false]);
    expect(solid).toEqual([1, 2, 3]);
    expect(dashed).toEqual([null, null, null]);
  });

  it('draws a fully backfilled series dashed', () => {
    const { solid, dashed } = splitEstimated([1, 2, 3], [true, true, true]);
    expect(solid).toEqual([null, null, null]);
    expect(dashed).toEqual([1, 2, 3]);
  });

  it('shares the boundary points so the line stays connected', () => {
    // live, backfill, backfill, live, live
    const { solid, dashed } = splitEstimated(
      [10, 20, 30, 40, 50],
      [false, true, true, false, false]
    );
    // Solid covers the live days; dashed covers the estimated days plus the
    // live day on either side, so every segment touching an estimate is dashed.
    expect(solid).toEqual([10, null, null, 40, 50]);
    expect(dashed).toEqual([10, 20, 30, 40, null]);
  });

  it('handles a lone estimated point at the start (history before the first live row)', () => {
    const { solid, dashed } = splitEstimated([5, 6, 7], [true, false, false]);
    expect(solid).toEqual([null, 6, 7]);
    expect(dashed).toEqual([5, 6, null]);
  });

  it('handles empty input', () => {
    expect(splitEstimated([], [])).toEqual({ solid: [], dashed: [] });
  });
});

describe('withEstimatedSplit', () => {
  it('adds <key>Solid and <key>Est series and leaves the original value in place', () => {
    const rows = [
      { date: 'a', value: 1, estimated: true },
      { date: 'b', value: 2, estimated: false },
    ];
    const out = withEstimatedSplit(rows, ['value']);
    expect(out[0]).toMatchObject({ date: 'a', value: 1, valueSolid: null, valueEst: 1 });
    expect(out[1]).toMatchObject({ date: 'b', value: 2, valueSolid: 2, valueEst: 2 });
  });

  it('treats a missing flag as live', () => {
    const out = withEstimatedSplit(
      [{ value: 1 } as { value: number; estimated?: boolean }],
      ['value']
    );
    expect(out[0]).toMatchObject({ valueSolid: 1, valueEst: null });
  });
});

describe('withCumulativeTops', () => {
  it('stacks the given keys in order', () => {
    const rows = [{ a: 1, b: 2, c: 3 }];
    const out = withCumulativeTops(rows, ['a', 'b', 'c']);
    expect(out[0]).toMatchObject({ aTop: 1, bTop: 3, cTop: 6 });
  });

  it('skips hidden series', () => {
    const out = withCumulativeTops([{ a: 1, b: 2, c: 3 }], ['a', 'c']);
    expect(out[0]).toMatchObject({ aTop: 1, cTop: 4 });
    expect(out[0]).not.toHaveProperty('bTop');
  });
});

describe('anyEstimated', () => {
  it('is true when at least one row is estimated', () => {
    expect(anyEstimated([{ estimated: false }, { estimated: true }])).toBe(true);
    expect(anyEstimated([{ estimated: false }, {}])).toBe(false);
    expect(anyEstimated([])).toBe(false);
  });
});
