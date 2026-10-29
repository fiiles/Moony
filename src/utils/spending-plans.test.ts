import { describe, expect, it } from 'vitest';
import {
  matchesPlanSearch,
  overviewRows,
  planRow,
  samePlanName,
  takeOverRows,
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

function summary(
  id: string,
  over: Partial<{
    name: string;
    amount: string;
    currency: string;
    spent: string | null;
    paymentCount: number;
    closed: boolean;
    year: number;
  }> = {}
) {
  return {
    plan: {
      id,
      name: over.name ?? id,
      year: over.year ?? 2026,
      amount: over.amount ?? '1000',
      currency: over.currency ?? 'CZK',
      icon: 'wallet',
      color: '#6366F1',
      note: null,
      closedAt: over.closed ? 1 : null,
      createdAt: 0,
    },
    spent: over.spent === undefined ? '0' : over.spent,
    paymentCount: over.paymentCount ?? 0,
    lastPaymentDate: null,
    fxQuality: { missingCurrencies: [], missingCount: 0, estimated: false },
  };
}

describe('planRow', () => {
  it('derives status, ratio and what is left in the plan currency', () => {
    const row = planRow(summary('a', { amount: '1000', spent: '1200', paymentCount: 3 }));
    expect(row.status).toBe('over');
    expect(row.ratio).toBeCloseTo(1.2);
    expect(row.left).toBe(-200);
    expect(row.closed).toBe(false);
  });

  it('keeps an unconvertible spend unknown, never 0', () => {
    const row = planRow(summary('a', { spent: null, paymentCount: 2 }));
    expect(row.status).toBe('unknown');
    expect(row.spent).toBeNull();
    expect(row.ratio).toBeNull();
    expect(row.left).toBeNull();
  });

  it('marks closed plans', () => {
    expect(planRow(summary('a', { closed: true })).closed).toBe(true);
  });
});

describe('overviewRows', () => {
  const rows = [
    summary('ok', { name: 'Ok', spent: '100', paymentCount: 1 }),
    summary('closed', { name: 'Closed', spent: '5000', paymentCount: 1, closed: true }),
    summary('over', { name: 'Over', spent: '1100', paymentCount: 4 }),
    summary('near', { name: 'Dovolená', spent: '950', paymentCount: 2 }),
    summary('empty', { name: 'Empty' }),
  ].map(planRow);

  it('shows active plans by risk', () => {
    expect(overviewRows(rows, 'active', '', 'cs').map((r) => r.summary.plan.id)).toEqual([
      'over',
      'near',
      'ok',
      'empty',
    ]);
  });

  it('puts closed plans last in "all" and alone in "closed"', () => {
    expect(overviewRows(rows, 'all', '', 'cs').map((r) => r.summary.plan.id)).toEqual([
      'over',
      'near',
      'ok',
      'empty',
      'closed',
    ]);
    expect(overviewRows(rows, 'closed', '', 'cs').map((r) => r.summary.plan.id)).toEqual([
      'closed',
    ]);
  });

  it('filters by name without case or diacritics', () => {
    expect(overviewRows(rows, 'all', 'dovo', 'cs').map((r) => r.summary.plan.id)).toEqual(['near']);
  });

  it('orders equal risk by name', () => {
    const same = [summary('b', { name: 'Beta' }), summary('a', { name: 'Alfa' })].map(planRow);
    expect(overviewRows(same, 'active', '', 'cs').map((r) => r.summary.plan.id)).toEqual([
      'a',
      'b',
    ]);
  });
});

describe('takeOverRows', () => {
  it('preselects open plans whose name is free in the target year', () => {
    const source = [
      summary('free', { name: 'Fyzioterapie', year: 2025 }),
      summary('closed', { name: 'Kolo', year: 2025, closed: true }),
      summary('taken', { name: 'Dárky Vánoce ', year: 2025 }),
    ].map((s) => s.plan);
    const rows = takeOverRows(source, ['dárky vánoce'], 'cs');
    expect(rows.map((r) => [r.plan.id, r.exists, r.selected])).toEqual([
      ['taken', true, false],
      ['free', false, true],
      ['closed', false, false],
    ]);
  });
});

describe('samePlanName', () => {
  it('compares trimmed and lower-cased like the backend', () => {
    expect(samePlanName(' ŠKODA ', 'škoda')).toBe(true);
    expect(samePlanName('Skoda', 'Škoda')).toBe(false);
  });
});
