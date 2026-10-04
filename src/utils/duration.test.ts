import { describe, expect, it } from 'vitest';
import { monthsBetween } from './duration';

const day = (iso: string) => Math.floor(Date.UTC(...splitIso(iso)) / 1000);
function splitIso(iso: string): [number, number, number] {
  const [y, m, d] = iso.split('-').map(Number);
  return [y, m - 1, d];
}

describe('monthsBetween', () => {
  it('counts whole years and months', () => {
    expect(monthsBetween(day('2024-03-12'), day('2026-10-02'))).toEqual({
      years: 2,
      months: 6,
      totalMonths: 30,
    });
  });

  it('counts a month only once its day of month is reached', () => {
    expect(monthsBetween(day('2026-09-14'), day('2026-10-13')).totalMonths).toBe(0);
    expect(monthsBetween(day('2026-09-14'), day('2026-10-14')).totalMonths).toBe(1);
  });

  it('is zero for an empty or inverted range', () => {
    expect(monthsBetween(day('2026-10-02'), day('2026-10-02'))).toEqual({
      years: 0,
      months: 0,
      totalMonths: 0,
    });
    expect(monthsBetween(day('2026-10-02'), day('2025-01-01')).totalMonths).toBe(0);
  });

  it('ignores non-finite input', () => {
    expect(monthsBetween(Number.NaN, day('2026-10-02')).totalMonths).toBe(0);
  });
});
