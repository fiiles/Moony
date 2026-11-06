import { describe, expect, it } from 'vitest';
import type {
  BankTransaction,
  SpendingPlan,
  SpendingPlanSummary,
  SpendingPlansOverview,
} from '@shared/schema';
import { UNCATEGORIZED_KEY, planRow } from '@/utils/spending-plans';
import {
  SEARCH_FROM,
  filterPlanPayments,
  pickerGroups,
  planListSections,
  plansCardSummary,
  takeOverInserts,
  yearChoices,
} from './plan-rows';

const QUALITY = { missingCurrencies: [], missingCount: 0, estimated: false };
const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;

function plan(name: string, patch: Partial<SpendingPlan> = {}): SpendingPlan {
  return {
    id: `id_${name}`,
    name,
    year: 2026,
    amount: '1000',
    currency: 'CZK',
    icon: 'wallet',
    color: '#123456',
    note: null,
    closedAt: null,
    createdAt: 0,
    ...patch,
  };
}

function summary(
  name: string,
  spent: string | null,
  patch: Partial<SpendingPlan> = {},
  paymentCount = 1
): SpendingPlanSummary {
  return {
    plan: plan(name, patch),
    spent,
    paymentCount,
    lastPaymentDate: null,
    fxQuality: QUALITY,
  };
}

function overview(
  plans: SpendingPlanSummary[],
  totals: Partial<SpendingPlansOverview['totals']> = {}
): SpendingPlansOverview {
  return {
    year: 2026,
    currency: 'CZK',
    fxQuality: QUALITY,
    plans,
    totals: { planned: '5000', spent: '2000', remainingActive: '3000', ...totals },
    planSpent: [],
    yearsWithPlans: [2026],
  };
}

describe('plansCardSummary', () => {
  it('is null for a year without plans', () => {
    expect(plansCardSummary(overview([]))).toBeNull();
  });

  it('takes the totals and counts active plans by status', () => {
    const result = plansCardSummary(
      overview([
        summary('over', '1500'),
        summary('near', '950'),
        summary('ok', '100'),
        summary('closedOver', '2000', { closedAt: 5 }),
      ])
    );
    expect(result).toEqual({
      year: 2026,
      currency: 'CZK',
      planned: 5000,
      spent: 2000,
      left: 3000,
      over: 1,
      near: 1,
    });
  });

  it('keeps missing totals as null, never 0', () => {
    const result = plansCardSummary(
      overview([summary('x', null)], { planned: null, spent: null, remainingActive: null })
    );
    expect(result).toMatchObject({ planned: null, spent: null, left: null, over: 0, near: 0 });
  });
});

describe('planListSections', () => {
  const rows = [
    summary('Cesta', '100'),
    summary('Auto', '1200'),
    summary('Stará', '10', { closedAt: 5 }),
  ].map(planRow);

  it('splits active and closed, riskiest active first', () => {
    const { active, closed } = planListSections(rows, '', 'cs');
    expect(active.map((r) => r.summary.plan.name)).toEqual(['Auto', 'Cesta']);
    expect(closed.map((r) => r.summary.plan.name)).toEqual(['Stará']);
  });

  it('filters both sections by the name search', () => {
    const { active, closed } = planListSections(rows, 'star', 'cs');
    expect(active).toEqual([]);
    expect(closed.map((r) => r.summary.plan.name)).toEqual(['Stará']);
  });

  it('offers the search from 8 plans', () => {
    expect(SEARCH_FROM).toBe(8);
  });
});

describe('pickerGroups', () => {
  const index = [
    summary('Zima', '0', { year: 2025 }),
    summary('Bydlení', '0', { year: 2026 }),
    summary('Auto', '0', { year: 2026 }),
    summary('Léto', '0', { year: 2027 }),
    summary('Zavřený', '0', { year: 2026, closedAt: 5 }),
    summary('Pozdní', '0', { year: 2024 }),
  ];

  it('puts the booking year first, then others newest first, names sorted', () => {
    const groups = pickerGroups(index, 2026, '', 'cs');
    expect(groups.map((g) => g.year)).toEqual([2026, 2027, 2025, 2024]);
    expect(groups[0].rows.map((r) => r.summary.plan.name)).toEqual(['Auto', 'Bydlení']);
  });

  it('leaves out closed plans', () => {
    const names = pickerGroups(index, 2026, '', 'cs').flatMap((g) =>
      g.rows.map((r) => r.summary.plan.name)
    );
    expect(names).not.toContain('Zavřený');
  });

  it('searches by name and drops empty groups', () => {
    const groups = pickerGroups(index, 2026, 'zim', 'cs');
    expect(groups.map((g) => g.year)).toEqual([2025]);
  });

  it('does not invent a group for the booking year without plans', () => {
    expect(pickerGroups(index, 2030, '', 'cs')[0].year).toBe(2027);
  });
});

describe('filterPlanPayments', () => {
  const tx = (id: string, date: number, categoryId: string | null) =>
    ({ id, bookingDate: date, categoryId }) as BankTransaction;
  const txs = [
    tx('a', day(2026, 3, 5), 'food'),
    tx('b', day(2026, 3, 20), null),
    tx('c', day(2026, 4, 1), 'food'),
  ];

  it('returns everything without filters', () => {
    expect(filterPlanPayments(txs, null, null)).toHaveLength(3);
  });

  it('filters by month', () => {
    expect(filterPlanPayments(txs, '2026-03', null).map((t) => t.id)).toEqual(['a', 'b']);
  });

  it('filters by category and by uncategorized', () => {
    expect(filterPlanPayments(txs, null, 'food').map((t) => t.id)).toEqual(['a', 'c']);
    expect(filterPlanPayments(txs, null, UNCATEGORIZED_KEY).map((t) => t.id)).toEqual(['b']);
  });

  it('combines both filters', () => {
    expect(filterPlanPayments(txs, '2026-04', 'food').map((t) => t.id)).toEqual(['c']);
    expect(filterPlanPayments(txs, '2026-04', UNCATEGORIZED_KEY)).toEqual([]);
  });
});

describe('takeOverInserts', () => {
  const source = [
    plan('Dovolená', { amount: '500', note: 'léto', icon: 'plane', color: '#abcdef' }),
    plan('Zavřený', { closedAt: 9 }),
    plan('Auto'),
  ];

  it('copies open, not yet existing plans into the target year', () => {
    const inserts = takeOverInserts(source, ['  auto '], 2027, 'cs');
    expect(inserts).toEqual([
      {
        name: 'Dovolená',
        year: 2027,
        amount: '500',
        currency: 'CZK',
        icon: 'plane',
        color: '#abcdef',
        note: 'léto',
      },
    ]);
  });

  it('is empty when every plan is closed or exists', () => {
    expect(takeOverInserts(source, ['AUTO', 'dovolená'], 2027, 'cs')).toEqual([]);
  });
});

describe('yearChoices', () => {
  it('is the year with its neighbours', () => {
    expect(yearChoices(2026)).toEqual([2025, 2026, 2027]);
  });
});
