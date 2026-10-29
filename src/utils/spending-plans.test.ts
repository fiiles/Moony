import { describe, expect, it } from 'vitest';
import {
  matchesPlanSearch,
  planMonthAxis,
  planStatus,
  sortPlansByRisk,
  type PlanStatus,
} from './spending-plans';

describe('planStatus', () => {
  it('is over when spent is above the planned amount', () => {
    expect(planStatus(11200, 10000, 9)).toBe('over');
  });

  it('is near from 90 % of the plan up to the plan itself', () => {
    expect(planStatus(9000, 10000, 3)).toBe('near');
    expect(planStatus(10000, 10000, 1)).toBe('near');
  });

  it('is ok below 90 % of the plan', () => {
    expect(planStatus(8999, 10000, 3)).toBe('ok');
  });

  it('is empty when there are no payments, whatever the spent value', () => {
    expect(planStatus(0, 8000, 0)).toBe('empty');
  });

  it('is unknown when spent cannot be computed', () => {
    expect(planStatus(null, 5000, 2)).toBe('unknown');
  });
});

describe('sortPlansByRisk', () => {
  type Row = { name: string; status: PlanStatus; ratio: number; closed: boolean };
  const key = (row: Row) => ({ status: row.status, ratio: row.ratio, closed: row.closed });

  it('orders active plans over, near, unknown, ok, empty and puts closed plans last', () => {
    const rows: Row[] = [
      { name: 'closed-over', status: 'over', ratio: 2, closed: true },
      { name: 'empty', status: 'empty', ratio: 0, closed: false },
      { name: 'ok', status: 'ok', ratio: 0.5, closed: false },
      { name: 'unknown', status: 'unknown', ratio: 0, closed: false },
      { name: 'near', status: 'near', ratio: 0.95, closed: false },
      { name: 'over', status: 'over', ratio: 1.2, closed: false },
    ];

    const sorted = sortPlansByRisk(rows, key).map((row) => row.name);

    expect(sorted).toEqual(['over', 'near', 'unknown', 'ok', 'empty', 'closed-over']);
  });

  it('orders ties by ratio descending and keeps the input order otherwise', () => {
    const rows: Row[] = [
      { name: 'ok-low', status: 'ok', ratio: 0.2, closed: false },
      { name: 'over-small', status: 'over', ratio: 1.1, closed: false },
      { name: 'over-big', status: 'over', ratio: 3, closed: false },
      { name: 'ok-low-2', status: 'ok', ratio: 0.2, closed: false },
    ];

    const sorted = sortPlansByRisk(rows, key).map((row) => row.name);

    expect(sorted).toEqual(['over-big', 'over-small', 'ok-low', 'ok-low-2']);
  });

  it('does not mutate the input array', () => {
    const rows: Row[] = [
      { name: 'ok', status: 'ok', ratio: 0.1, closed: false },
      { name: 'over', status: 'over', ratio: 2, closed: false },
    ];
    sortPlansByRisk(rows, key);
    expect(rows.map((row) => row.name)).toEqual(['ok', 'over']);
  });
});

describe('planMonthAxis', () => {
  it('covers the twelve months of the year when nothing is outside it', () => {
    const axis = planMonthAxis(2026, []);
    expect(axis).toHaveLength(12);
    expect(axis[0]).toBe('2026-01');
    expect(axis[11]).toBe('2026-12');
  });

  it('extends the axis to a later month that has payments', () => {
    const axis = planMonthAxis(2026, ['2027-02']);
    expect(axis).toHaveLength(14);
    expect(axis[axis.length - 1]).toBe('2027-02');
    expect(axis[12]).toBe('2027-01');
  });

  it('extends the axis to an earlier month that has payments', () => {
    const axis = planMonthAxis(2026, ['2025-11', '2026-03']);
    expect(axis[0]).toBe('2025-11');
    expect(axis).toHaveLength(14);
    expect(axis[axis.length - 1]).toBe('2026-12');
  });

  it('returns contiguous YYYY-MM strings', () => {
    const axis = planMonthAxis(2026, ['2025-11']);
    expect(axis.slice(0, 3)).toEqual(['2025-11', '2025-12', '2026-01']);
  });
});

describe('matchesPlanSearch', () => {
  it('matches a prefix of the name, ignoring case and diacritics', () => {
    expect(matchesPlanSearch('Dovolená Jeseníky', 'dovo')).toBe(true);
    expect(matchesPlanSearch('Dovolená Jeseníky', 'DOVOLENA')).toBe(true);
  });

  it('does not match an unrelated name', () => {
    expect(matchesPlanSearch('Zahrada', 'dovo')).toBe(false);
  });
});
