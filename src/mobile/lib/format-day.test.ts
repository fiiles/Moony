import { describe, expect, it } from 'vitest';
import { shortDayOptions } from './format-day';

const NOW = Date.UTC(2026, 9, 16, 12);

describe('shortDayOptions', () => {
  it('leaves the year out for a day in the current year', () => {
    expect(shortDayOptions(Date.UTC(2026, 0, 2) / 1000, NOW)).toEqual({
      day: 'numeric',
      month: 'numeric',
      year: undefined,
    });
  });
  it('adds the year for any other year', () => {
    expect(shortDayOptions(Date.UTC(2025, 11, 31) / 1000, NOW).year).toBe('numeric');
    expect(shortDayOptions(Date.UTC(2027, 0, 1) / 1000, NOW).year).toBe('numeric');
  });
});
