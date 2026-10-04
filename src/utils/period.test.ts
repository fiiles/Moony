import { describe, it, expect } from 'vitest';
import { getUtcPeriodRange, utcDayStart, utcDayEnd, isoDateFromUtcTimestamp } from './period';

const utc = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;

describe('getUtcPeriodRange', () => {
  // 2026-09-15T12:00:00Z — mid month, any timezone
  const now = new Date(Date.UTC(2026, 8, 15, 12));

  it('monthly: covers the whole month including the last day at UTC midnight', () => {
    const { start, end } = getUtcPeriodRange('monthly', 0, now);
    expect(start).toBe(utc(2026, 9, 1));
    expect(end).toBe(utc(2026, 10, 1) - 1);
    const sept30 = utc(2026, 9, 30); // booking_date as stored by CSV import and manual entry
    expect(sept30 >= start && sept30 <= end).toBe(true);
    const oct1 = utc(2026, 10, 1);
    expect(oct1 <= end).toBe(false);
  });

  it('monthly: 31-day month and previous-month offset', () => {
    const { start, end } = getUtcPeriodRange('monthly', 1, now); // October
    expect(start).toBe(utc(2026, 10, 1));
    expect(end).toBe(utc(2026, 11, 1) - 1);
    expect(utc(2026, 10, 31) <= end).toBe(true);

    const prev = getUtcPeriodRange('monthly', -1, now); // August
    expect(prev.start).toBe(utc(2026, 8, 1));
    expect(utc(2026, 8, 31) <= prev.end).toBe(true);
  });

  it('monthly: offsets cross year boundaries', () => {
    const { start, end } = getUtcPeriodRange('monthly', -9, now); // December 2025
    expect(start).toBe(utc(2025, 12, 1));
    expect(end).toBe(utc(2026, 1, 1) - 1);
    expect(utc(2025, 12, 31) <= end).toBe(true);
  });

  it('quarterly: Q3 contains 30 September, previous quarter is Q2', () => {
    const q3 = getUtcPeriodRange('quarterly', 0, now);
    expect(q3.start).toBe(utc(2026, 7, 1));
    expect(q3.end).toBe(utc(2026, 10, 1) - 1);
    expect(utc(2026, 9, 30) <= q3.end).toBe(true);

    const q2 = getUtcPeriodRange('quarterly', -1, now);
    expect(q2.start).toBe(utc(2026, 4, 1));
    expect(q2.end).toBe(utc(2026, 7, 1) - 1);

    const q4prev = getUtcPeriodRange('quarterly', -3, now); // Q4 2025
    expect(q4prev.start).toBe(utc(2025, 10, 1));
    expect(q4prev.end).toBe(utc(2026, 1, 1) - 1);
  });

  it('yearly: contains 31 December', () => {
    const { start, end } = getUtcPeriodRange('yearly', 0, now);
    expect(start).toBe(utc(2026, 1, 1));
    expect(end).toBe(utc(2027, 1, 1) - 1);
    expect(utc(2026, 12, 31) <= end).toBe(true);
  });

  it('returns the period start as a Date usable for UTC-formatted labels', () => {
    const { startDate } = getUtcPeriodRange('monthly', 0, now);
    expect(startDate.toISOString()).toBe('2026-09-01T00:00:00.000Z');
    expect(startDate.getUTCMonth()).toBe(8);
  });

  it('does not depend on local-time Date getters (only Date.UTC is used)', () => {
    // A local-midnight construction differs from UTC midnight in every
    // non-UTC zone; the helper must never produce such a value.
    const { start } = getUtcPeriodRange('monthly', 0, now);
    expect(start % 86_400).toBe(0);
    expect(new Date(start * 1000).getUTCDate()).toBe(1);
    expect(new Date(start * 1000).getUTCHours()).toBe(0);
  });
});

describe('utcDayStart / utcDayEnd', () => {
  it('maps an ISO date to the UTC day bounds', () => {
    expect(utcDayStart('2026-09-30')).toBe(utc(2026, 9, 30));
    expect(utcDayEnd('2026-09-30')).toBe(utc(2026, 10, 1) - 1);
  });

  it('the stored booking_date of that day falls inside the bounds', () => {
    const bookingDate = Math.floor(new Date('2026-09-30').getTime() / 1000);
    expect(bookingDate).toBeGreaterThanOrEqual(utcDayStart('2026-09-30'));
    expect(bookingDate).toBeLessThanOrEqual(utcDayEnd('2026-09-30'));
  });
});

describe('isoDateFromUtcTimestamp', () => {
  it('formats a UTC-midnight timestamp as its calendar day', () => {
    expect(isoDateFromUtcTimestamp(utc(2026, 9, 30))).toBe('2026-09-30');
    expect(isoDateFromUtcTimestamp(utc(2026, 12, 31) + 3600)).toBe('2026-12-31');
  });
});
