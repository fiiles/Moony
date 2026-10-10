import { describe, expect, it } from 'vitest';
import {
  averageMonthlyChange,
  balanceSeries,
  changeThisYear,
  newestRecord,
  previousRecord,
  valueAt,
} from './balance-records';

const DAY = 86400;
const day = (iso: string) =>
  Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10)) / 1000;
const r = (iso: string, amount: number) => ({ asOf: day(iso), amount: String(amount) });
const TODAY = day('2026-10-10');

describe('newestRecord / previousRecord', () => {
  it('orders by day regardless of input order', () => {
    const records = [r('2026-09-01', 437800), r('2026-10-03', 450000), r('2026-08-01', 425600)];
    expect(newestRecord(records)?.asOf).toBe(day('2026-10-03'));
    expect(previousRecord(records)?.asOf).toBe(day('2026-09-01'));
    expect(newestRecord([])).toBeNull();
    expect(previousRecord([r('2026-10-03', 1)])).toBeNull();
  });
});

describe('valueAt', () => {
  it('is the latest record on or before the day, null before the first', () => {
    const records = [r('2026-01-05', 341000), r('2025-12-28', 353600), r('2026-02-01', 353000)];
    expect(valueAt(records, day('2026-01-01'))).toBe(353600);
    expect(valueAt(records, day('2026-01-05'))).toBe(341000);
    expect(valueAt(records, day('2025-12-01'))).toBeNull();
  });
});

describe('balanceSeries', () => {
  it('is empty with fewer than two records', () => {
    expect(balanceSeries([], TODAY)).toEqual([]);
    expect(balanceSeries([r('2026-10-03', 1)], TODAY)).toEqual([]);
  });
  it('covers the last year, enters at the window start and ends at the newest record', () => {
    const records = [r('2025-08-01', 300000), r('2025-12-01', 340000), r('2026-10-03', 450000)];
    const series = balanceSeries(records, TODAY);
    const start = day('2026-10-03') - 365 * DAY; // 2025-10-03, between the first two records
    expect(series[0]).toEqual({ t: start, value: expect.any(Number), record: false });
    // Linear interpolation between 2025-08-01 (300000) and 2025-12-01 (340000)
    const t = (start - day('2025-08-01')) / (day('2025-12-01') - day('2025-08-01'));
    expect(series[0].value).toBe(Math.round(300000 + 40000 * t));
    expect(series.slice(1)).toEqual([
      { t: day('2025-12-01'), value: 340000, record: true },
      { t: day('2026-10-03'), value: 450000, record: true },
    ]);
  });
  it('keeps the year before a stale newest record', () => {
    const records = [r('2024-11-01', 300000), r('2025-03-01', 320000), r('2025-07-01', 340000)];
    expect(balanceSeries(records, TODAY)).toEqual([
      { t: day('2024-11-01'), value: 300000, record: true },
      { t: day('2025-03-01'), value: 320000, record: true },
      { t: day('2025-07-01'), value: 340000, record: true },
    ]);
  });
  it('ignores records dated after today', () => {
    const records = [r('2026-09-01', 1), r('2026-10-03', 2), r('2026-12-01', 3)];
    const series = balanceSeries(records, TODAY);
    expect(series.map((p) => p.t)).toEqual([day('2026-09-01'), day('2026-10-03')]);
    expect(series).toHaveLength(2);
  });
  it('starts at the first record when the history is shorter than a year', () => {
    const records = [r('2026-04-30', 391600), r('2026-09-30', 433560)];
    expect(balanceSeries(records, TODAY).map((p) => p.t)).toEqual([
      day('2026-04-30'),
      day('2026-09-30'),
    ]);
  });
});

describe('changeThisYear', () => {
  it('compares the newest record with the value at 1 January', () => {
    const records = [r('2025-12-28', 353600), r('2026-01-05', 341000), r('2026-10-03', 450000)];
    expect(changeThisYear(records, TODAY)).toEqual({ delta: 96400, baseDay: day('2025-12-28') });
  });
  it('is null without a record on or before 1 January', () => {
    expect(changeThisYear([r('2026-04-30', 1), r('2026-09-30', 2)], TODAY)).toBeNull();
  });
  it('is null when the newest record itself is the base', () => {
    expect(changeThisYear([r('2025-12-28', 353600)], TODAY)).toBeNull();
    expect(changeThisYear([r('2025-06-01', 1), r('2025-12-28', 2)], TODAY)).toBeNull();
  });
});

describe('averageMonthlyChange', () => {
  it('uses the record a year before the newest one when it exists', () => {
    // 2025-10-04 is less than 365 days before the newest record, so it is not the base.
    const records = [r('2025-09-30', 330000), r('2025-10-04', 331000), r('2026-10-03', 450000)];
    const avg = averageMonthlyChange(records);
    expect(avg).toEqual({
      perMonth: 10000,
      months: 12,
      sinceFirst: false,
      baseDay: day('2025-09-30'),
    });
  });
  it('falls back to the oldest record and the elapsed months', () => {
    const records = [r('2025-11-01', 330000), r('2026-10-03', 450000)];
    expect(averageMonthlyChange(records)).toEqual({
      perMonth: Math.round(120000 / 11),
      months: 11,
      sinceFirst: true,
      baseDay: day('2025-11-01'),
    });
    expect(averageMonthlyChange([r('2026-10-03', 1)])).toBeNull();
  });
});
