import { describe, expect, it } from 'vitest';
import { compareConverted } from './fx-totals';

describe('compareConverted', () => {
  it('orders known amounts and puts unconvertible ones below them', () => {
    const sorted = [3, null, -5, 10, null].sort(compareConverted);
    expect(sorted).toEqual([null, null, -5, 3, 10]);
  });
});
