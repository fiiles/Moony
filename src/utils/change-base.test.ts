import { describe, expect, it } from 'vitest';
import { shortHistoryBaseDay, SHORT_HISTORY_TOLERANCE_SECONDS } from './change-base';

const DAY = 86_400;
// 2026-09-30T12:00:00Z
const NOW = 1_790_769_600;

describe('shortHistoryBaseDay', () => {
  it('names the real base day when the history is shorter than the period', () => {
    // "30D" asked for, but the oldest row is 10 days old
    const periodStart = NOW - 30 * DAY;
    const oldest = NOW - 10 * DAY;
    expect(shortHistoryBaseDay(periodStart, oldest)).toBe(Math.floor(oldest / DAY) * DAY);
  });

  it('is null when the history reaches back to the period start', () => {
    const periodStart = NOW - 30 * DAY;
    expect(shortHistoryBaseDay(periodStart, periodStart)).toBeNull();
    // the first row after the start is up to a day or so later
    expect(shortHistoryBaseDay(periodStart, periodStart + DAY + 3_600)).toBeNull();
    expect(shortHistoryBaseDay(periodStart, periodStart - 5 * DAY)).toBeNull();
  });

  it('tolerates a short gap at the start but not a longer one', () => {
    const periodStart = NOW - 30 * DAY;
    expect(
      shortHistoryBaseDay(periodStart, periodStart + SHORT_HISTORY_TOLERANCE_SECONDS)
    ).toBeNull();
    expect(
      shortHistoryBaseDay(periodStart, periodStart + SHORT_HISTORY_TOLERANCE_SECONDS + 1)
    ).not.toBeNull();
  });

  it('has nothing to say without a period start (All) or without any history', () => {
    expect(shortHistoryBaseDay(undefined, NOW - DAY)).toBeNull();
    expect(shortHistoryBaseDay(NOW - 30 * DAY, undefined)).toBeNull();
  });
});
