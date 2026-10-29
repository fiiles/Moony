import { describe, expect, it } from 'vitest';
import {
  UNCATEGORIZED_KEY,
  categoryKey,
  cumulativeSpend,
  monthDate,
  monthOfDay,
  periodMonths,
  periodRange,
  planCategoryRows,
  planMonthSpend,
  signedTotals,
  matchesPlanSearch,
  overviewRows,
  planRow,
  samePlanName,
  takeOverRows,
  planMonthAxis,
  planStatus,
  sortPlansByRisk,
  planAxisLabels,
  planMonthLabel,
  planFlowSegments,
  type PlanStatus,
} from './spending-plans';
import { createFormatters } from '@/lib/format';

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

const DAY = 86400;
/** Unix seconds of a UTC midnight. */
const utc = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;

describe('monthOfDay', () => {
  it('names the UTC month of a booking day', () => {
    expect(monthOfDay(utc(2026, 7, 1))).toBe('2026-07');
    expect(monthOfDay(utc(2026, 12, 31) + DAY - 1)).toBe('2026-12');
  });
});

describe('monthDate', () => {
  it('is the UTC first day of the month', () => {
    expect(monthDate('2026-07').getTime()).toBe(utc(2026, 7, 1) * 1000);
  });
});

describe('categoryKey', () => {
  it('keeps a category id and gives uncategorized payments their own key', () => {
    expect(categoryKey('cat-food')).toBe('cat-food');
    expect(categoryKey(null)).toBe(UNCATEGORIZED_KEY);
  });
});

const detail = {
  months: [
    { month: '2026-06', spent: '1200' },
    { month: '2026-07', spent: '3000.5' },
  ],
  categories: [
    { categoryId: 'lodging', spent: '2800', count: 2 },
    { categoryId: null, spent: '1400.5', count: 3 },
  ],
  cells: [
    { month: '2026-06', categoryId: 'lodging', spent: '1000', count: 1 },
    { month: '2026-06', categoryId: null, spent: '200', count: 1 },
    { month: '2026-07', categoryId: 'lodging', spent: '1800', count: 1 },
    { month: '2026-07', categoryId: null, spent: '1200.5', count: 2 },
  ],
};
const axis = ['2026-05', '2026-06', '2026-07', '2026-08'];

describe('planMonthSpend', () => {
  it('puts the net spend of each month on the axis, zero where nothing was spent', () => {
    expect(planMonthSpend(detail, axis, null)).toEqual([0, 1200, 3000.5, 0]);
  });

  it("with a category sums only that category's cells", () => {
    expect(planMonthSpend(detail, axis, 'lodging')).toEqual([0, 1000, 1800, 0]);
    expect(planMonthSpend(detail, axis, UNCATEGORIZED_KEY)).toEqual([0, 200, 1200.5, 0]);
  });
});

describe('cumulativeSpend', () => {
  it('runs the total up to the last month and leaves later months empty', () => {
    expect(cumulativeSpend([0, 1200, 3000, 500], axis, '2026-07')).toEqual([0, 1200, 4200, null]);
  });

  it('runs over the whole axis for a past year and over nothing for a future one', () => {
    expect(cumulativeSpend([1, 2], ['2025-11', '2025-12'], '2026-10')).toEqual([1, 3]);
    expect(cumulativeSpend([1, 2], ['2027-01', '2027-02'], '2026-10')).toEqual([null, null]);
  });
});

describe('planCategoryRows', () => {
  it('lists the categories of the whole plan, the largest spend first', () => {
    expect(planCategoryRows(detail, null)).toEqual([
      { key: 'lodging', categoryId: 'lodging', spent: 2800, count: 2 },
      { key: UNCATEGORIZED_KEY, categoryId: null, spent: 1400.5, count: 3 },
    ]);
  });

  it('with a month sums the cells of that month only', () => {
    expect(planCategoryRows(detail, '2026-06')).toEqual([
      { key: 'lodging', categoryId: 'lodging', spent: 1000, count: 1 },
      { key: UNCATEGORIZED_KEY, categoryId: null, spent: 200, count: 1 },
    ]);
    expect(planCategoryRows(detail, '2026-08')).toEqual([]);
  });
});

describe('signedTotals', () => {
  it('sums debits as negative and credits as positive per currency, never converting', () => {
    expect(
      signedTotals([
        { amount: '1200', type: 'debit', currency: 'CZK' },
        { amount: '50.5', type: 'debit', currency: 'EUR' },
        { amount: '200', type: 'credit', currency: 'CZK' },
      ])
    ).toEqual([
      { currency: 'CZK', amount: -1000 },
      { currency: 'EUR', amount: -50.5 },
    ]);
  });

  it('is empty for no payments', () => {
    expect(signedTotals([])).toEqual([]);
  });
});

describe('periodMonths', () => {
  it('lists the months of the year up to the current one', () => {
    expect(periodMonths(2026, '2026-03')).toEqual(['2026-01', '2026-02', '2026-03']);
  });

  it('lists all twelve for a past year and none for a future one', () => {
    expect(periodMonths(2025, '2026-03')).toHaveLength(12);
    expect(periodMonths(2027, '2026-03')).toEqual([]);
  });
});

describe('periodRange', () => {
  it('covers the whole year up to the last second of 31 December', () => {
    expect(periodRange(2026, 'year')).toEqual({
      dateFrom: utc(2026, 1, 1),
      dateTo: utc(2026, 12, 31) + DAY - 1,
    });
  });

  it('covers one month up to the last second of its last day', () => {
    expect(periodRange(2026, '2026-02')).toEqual({
      dateFrom: utc(2026, 2, 1),
      dateTo: utc(2026, 2, 28) + DAY - 1,
    });
  });

  it('has no bounds for all periods', () => {
    expect(periodRange(2026, 'all')).toEqual({ dateFrom: null, dateTo: null });
  });
});

describe('plan month labels', () => {
  const cs = createFormatters('cs-CZ', undefined).monthInSentence;
  const en = createFormatters('en-US', undefined).monthInSentence;

  it('writes the axis months of the plan year without the year', () => {
    expect(planAxisLabels(['2026-01', '2026-02', '2026-12'], 2026, cs)).toEqual([
      'led',
      'úno',
      'pro',
    ]);
    expect(planAxisLabels(['2026-01', '2026-09'], 2026, en)).toEqual(['Jan', 'Sep']);
  });

  it('adds a two-digit year to axis months outside the plan year', () => {
    expect(planAxisLabels(['2025-12', '2026-01', '2027-02'], 2026, cs)).toEqual([
      'pro 25',
      'led',
      'úno 27',
    ]);
  });

  it('names a month of the plan year without the year and other months with it', () => {
    expect(planMonthLabel('2026-09', 2026, cs)).toBe('září');
    expect(planMonthLabel('2025-12', 2026, cs)).toBe('prosinec 2025');
    expect(planMonthLabel('2026-09', 2026, en)).toBe('September');
  });
});

describe('planFlowSegments', () => {
  const labels = {
    category: (id: string | null) => (id === null ? 'Bez kategorie' : `cat ${id}`),
    other: 'Ostatní',
    rest: 'Zbývá v plánech',
  };
  const cat = (categoryId: string | null, spent: string) => ({ categoryId, spent });
  const brief = (segments: ReturnType<typeof planFlowSegments>) =>
    segments.map((s) => [s.key, s.label, s.value, s.share, s.kind ?? null]);

  it('shows the four largest categories, then Ostatní, then the hatched rest of the plans', () => {
    const segments = planFlowSegments(
      {
        planned: 1000,
        spent: 600,
        categories: [
          cat('a', '100'),
          cat('b', '250'),
          cat('c', '50'),
          cat('d', '80'),
          cat('e', '70'),
          cat('f', '50'),
        ],
      },
      labels
    );
    expect(brief(segments)).toEqual([
      ['cat:b', 'cat b', 250, 0.25, null],
      ['cat:a', 'cat a', 100, 0.1, null],
      ['cat:d', 'cat d', 80, 0.08, null],
      ['cat:e', 'cat e', 70, 0.07, null],
      ['other', 'Ostatní', 100, 0.1, null],
      ['rest', 'Zbývá v plánech', 400, 0.4, 'rest'],
    ]);
  });

  it('has no Ostatní with four categories or fewer', () => {
    const segments = planFlowSegments(
      { planned: 100, spent: 30, categories: [cat('a', '20'), cat('b', '10')] },
      labels
    );
    expect(segments.map((s) => s.key)).toEqual(['cat:a', 'cat:b', 'rest']);
  });

  it('measures an overspent year against the spend and leaves no rest', () => {
    const segments = planFlowSegments(
      { planned: 100, spent: 200, categories: [cat('a', '150'), cat('b', '50')] },
      labels
    );
    expect(brief(segments)).toEqual([
      ['cat:a', 'cat a', 150, 0.75, null],
      ['cat:b', 'cat b', 50, 0.25, null],
    ]);
  });

  it('leaves no rest when the plans are exactly used up', () => {
    const segments = planFlowSegments(
      { planned: 100, spent: 100, categories: [cat('a', '100')] },
      labels
    );
    expect(segments.map((s) => s.key)).toEqual(['cat:a']);
  });

  it('names payments without a category "Bez kategorie"', () => {
    const [segment] = planFlowSegments(
      { planned: 100, spent: 40, categories: [cat(null, '40')] },
      labels
    );
    expect([segment.key, segment.label]).toEqual(['uncategorized', 'Bez kategorie']);
  });

  it('leaves out categories with zero or negative spend', () => {
    const segments = planFlowSegments(
      {
        planned: 100,
        spent: 30,
        categories: [cat('a', '40'), cat('b', '0'), cat('c', '-10'), cat('d', 'x')],
      },
      labels
    );
    expect(segments.map((s) => s.key)).toEqual(['cat:a', 'rest']);
  });

  it('is empty when nothing is planned or spent', () => {
    expect(planFlowSegments({ planned: 0, spent: 0, categories: [] }, labels)).toEqual([]);
  });
});
