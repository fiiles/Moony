import { describe, expect, it } from 'vitest';
import { isoDayToUtcSeconds, isoMonthToUtcDate } from './picker-value';

describe('isoDayToUtcSeconds', () => {
  it('reads an ISO day as its UTC midnight', () => {
    expect(isoDayToUtcSeconds('2026-11-15')).toBe(Date.UTC(2026, 10, 15) / 1000);
    expect(isoDayToUtcSeconds('2024-02-29')).toBe(Date.UTC(2024, 1, 29) / 1000);
  });

  it('is null for an empty, half-typed or impossible day', () => {
    expect(isoDayToUtcSeconds('')).toBeNull();
    expect(isoDayToUtcSeconds('2026-11')).toBeNull();
    expect(isoDayToUtcSeconds('2026-02-30')).toBeNull();
    expect(isoDayToUtcSeconds('2026-13-01')).toBeNull();
  });
});

describe('isoMonthToUtcDate', () => {
  it('reads an ISO month as the UTC date of its first day', () => {
    expect(isoMonthToUtcDate('2026-11')?.toISOString()).toBe('2026-11-01T00:00:00.000Z');
  });

  it('is null for an empty or impossible month', () => {
    expect(isoMonthToUtcDate('')).toBeNull();
    expect(isoMonthToUtcDate('2026-13')).toBeNull();
    expect(isoMonthToUtcDate('2026-00')).toBeNull();
    expect(isoMonthToUtcDate('2026-11-01')).toBeNull();
  });
});
