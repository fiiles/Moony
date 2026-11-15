import { describe, expect, it } from 'vitest';
import { flowColumnWidth, FLOW_COLUMN_MIN } from './flow-strip';

describe('flowColumnWidth', () => {
  it('shares the width out evenly between the whole columns that fit', () => {
    // 296 pt (a 360 pt phone's card) holds six 44 pt columns: each takes a sixth
    expect(flowColumnWidth(296, 12)).toBeCloseTo(296 / 6);
  });

  it('never makes a column narrower than the minimum', () => {
    expect(flowColumnWidth(43, 12)).toBe(FLOW_COLUMN_MIN);
    expect(flowColumnWidth(0, 12)).toBe(FLOW_COLUMN_MIN);
  });

  it('spreads a strip of few months over the whole width', () => {
    expect(flowColumnWidth(300, 3)).toBe(100);
  });

  it('a width that is a whole number of columns keeps the minimum', () => {
    expect(flowColumnWidth(44 * 7, 12)).toBe(44);
  });

  it('falls back to the minimum without months or a usable width', () => {
    expect(flowColumnWidth(300, 0)).toBe(FLOW_COLUMN_MIN);
    expect(flowColumnWidth(Number.NaN, 12)).toBe(FLOW_COLUMN_MIN);
  });
});
