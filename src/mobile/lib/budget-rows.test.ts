import { describe, expect, it } from 'vitest';
import type { BudgetGoal, BudgetGoalVersion } from '@shared/schema';
import { budgetedCategoryIds, budgetRows, reportRowLimit, unbudgetedRows } from './budget-rows';

const month = (year: number, index: number) => Date.UTC(year, index, 1) / 1000;
/** October 2026, the month the tests run in. */
const NOW = month(2026, 9);

function version(effectiveMonth: number, amount: string | null): BudgetGoalVersion {
  return { effectiveMonth, amount, currency: 'CZK' };
}

function goal(
  categoryId: string,
  versions: BudgetGoalVersion[],
  patch: Partial<BudgetGoal> = {}
): BudgetGoal {
  return {
    id: `goal_${categoryId}`,
    categoryId,
    timeframe: 'monthly',
    amount: versions[0]?.amount ?? '0',
    currency: 'CZK',
    createdAt: 0,
    updatedAt: 0,
    effectiveMonth: versions[0]?.effectiveMonth ?? null,
    versions,
    ...patch,
  };
}

/** Names the tests' categories by their id, upper-cased: the order is the name's, not the id's. */
const nameOf = (id: string) => id.toUpperCase();

const ACTIVE = goal('cat_b', [version(month(2026, 0), '3000')]);
const SCHEDULED = goal('cat_a', [version(month(2026, 10), '1500')]);
const ENDED = goal('cat_c', [version(month(2026, 0), '800'), version(month(2026, 5), null)]);

describe('budgetRows', () => {
  it('lists active and scheduled budgets first and the ended one last', () => {
    // The input order is the reverse of the expected one
    const rows = budgetRows([ENDED, SCHEDULED, ACTIVE], NOW, nameOf, 'en');
    expect(rows.map((row) => [row.categoryId, row.status])).toEqual([
      ['cat_a', 'scheduled'],
      ['cat_b', 'active'],
      ['cat_c', 'ended'],
    ]);
  });

  it('sorts by name within the live budgets and within the ended ones', () => {
    // The ended budgets' names come first in the alphabet: the status still outranks the name
    const rows = budgetRows(
      [
        goal('cat_b', [version(month(2026, 0), '100'), version(month(2026, 2), null)]),
        goal('cat_z', [version(month(2026, 0), '100')]),
        goal('cat_a', [version(month(2026, 0), '100'), version(month(2026, 2), null)]),
        goal('cat_y', [version(month(2026, 0), '100')]),
      ],
      NOW,
      nameOf,
      'en'
    );
    expect(rows.map((row) => row.categoryId)).toEqual(['cat_y', 'cat_z', 'cat_a', 'cat_b']);
  });

  it('sorts names by the locale ("Ch" follows "H" in Czech, it does not in English)', () => {
    const names: Record<string, string> = { cat_1: 'Chleba', cat_2: 'Cukr', cat_3: 'Hotel' };
    const list = ['cat_1', 'cat_2', 'cat_3'].map((id) => goal(id, [version(month(2026, 0), '1')]));
    const order = (locale: string) =>
      budgetRows(list, NOW, (id) => names[id], locale).map((row) => row.name);
    expect(order('cs')).toEqual(['Cukr', 'Hotel', 'Chleba']);
    expect(order('en')).toEqual(['Chleba', 'Cukr', 'Hotel']);
  });

  it('carries the goal, the row key, the name and the interval that stands for the budget', () => {
    const [row] = budgetRows([ACTIVE], NOW, nameOf, 'en');
    expect(row.goal).toBe(ACTIVE);
    expect(row.key).toBe('cat_b:monthly');
    expect(row.name).toBe('CAT_B');
    expect(row.main.version.amount).toBe('3000');
    expect(row.main.end).toBeNull();
  });

  it('keeps a quarterly and a monthly budget of one category apart', () => {
    const rows = budgetRows(
      [
        goal('cat_a', [version(month(2026, 0), '100')]),
        goal('cat_a', [version(month(2026, 0), '300')], { timeframe: 'quarterly' }),
      ],
      NOW,
      nameOf,
      'en'
    );
    expect(rows.map((row) => row.key).sort()).toEqual(['cat_a:monthly', 'cat_a:quarterly']);
  });

  it('lists what is scheduled after the limit in effect', () => {
    const [row] = budgetRows(
      [goal('cat_a', [version(month(2026, 0), '3000'), version(month(2026, 11), '3500')])],
      NOW,
      nameOf,
      'en'
    );
    expect(row.status).toBe('active');
    expect(row.upcoming.map((v) => v.amount)).toEqual(['3500']);
  });

  it('lets an interval be deleted only when there is another limit than the one shown', () => {
    const rows = budgetRows(
      [
        // A single interval: deleting it would be deleting the budget
        goal('cat_single', [version(month(2026, 0), '3000')]),
        // An earlier limit behind the one in effect
        goal('cat_two', [version(month(2025, 0), '2500'), version(month(2026, 0), '3000')]),
        // A limit scheduled after the one in effect
        goal('cat_next', [version(month(2026, 0), '3000'), version(month(2026, 11), '3500')]),
        // A scheduled removal is not a limit
        goal('cat_removal', [version(month(2026, 0), '3000'), version(month(2026, 11), null)]),
        // An ended budget lists no upcoming changes and has no earlier limit
        ENDED,
      ],
      NOW,
      nameOf,
      'en'
    );
    const byId = Object.fromEntries(rows.map((row) => [row.categoryId, row.canDeleteInterval]));
    expect(byId).toEqual({
      cat_single: false,
      cat_two: true,
      cat_next: true,
      cat_removal: false,
      cat_c: false,
    });
  });

  it('can delete an interval of an active budget with two limited versions', () => {
    const [row] = budgetRows(
      [goal('cat_a', [version(month(2026, 0), '3000'), version(month(2026, 11), '3500')])],
      NOW,
      nameOf,
      'en'
    );
    expect(row.canDeleteInterval).toBe(true);
  });

  it('skips a goal that never had a limit', () => {
    expect(budgetRows([goal('cat_a', [version(month(2026, 0), null)])], NOW, nameOf, 'en')).toEqual(
      []
    );
  });
});

describe('budgetedCategoryIds', () => {
  it('has the categories whose monthly limit is in effect or scheduled', () => {
    expect([...budgetedCategoryIds([ACTIVE, SCHEDULED], NOW)].sort()).toEqual(['cat_a', 'cat_b']);
  });

  it('leaves out a budget that has ended: the category may get a new one', () => {
    expect([...budgetedCategoryIds([ACTIVE, ENDED], NOW)]).toEqual(['cat_b']);
  });

  it('leaves out a quarterly budget: only the monthly limits are managed here', () => {
    const quarterly = goal('cat_q', [version(month(2026, 0), '9000')], { timeframe: 'quarterly' });
    expect([...budgetedCategoryIds([ACTIVE, quarterly], NOW)]).toEqual(['cat_b']);
  });
});

describe('unbudgetedRows', () => {
  const averages = new Map([
    ['cat_a', { average: 4120, months: 6 }],
    ['cat_b', { average: 0, months: 6 }],
    ['cat_c', { average: 900, months: 6 }],
    ['cat_x', { average: 5000, months: 6 }],
    ['cat_d', { average: 1800, months: 6 }],
  ]);
  const counts = new Map([
    ['cat_a', 31],
    ['cat_b', 0],
    ['cat_c', 12],
    ['cat_x', 4],
    ['cat_d', 9],
  ]);
  // cat_x is an income category: it has an average, but no limit can be set on it
  const expenses = new Set(['cat_a', 'cat_b', 'cat_c', 'cat_d']);

  it('lists the expense categories that spend and have no budget, as average and count', () => {
    const rows = unbudgetedRows(
      averages,
      counts,
      new Set(['cat_c', 'cat_d']),
      new Set(['cat_a', 'cat_b', 'cat_c', 'cat_d'])
    );
    expect(rows).toEqual([{ categoryId: 'cat_a', average: 4120, count: 31 }]);
  });

  it('leaves out a category with no spending, a budgeted one and a category that is not an expense', () => {
    const rows = unbudgetedRows(averages, counts, new Set(['cat_c']), expenses);
    expect(rows.map((row) => row.categoryId)).not.toContain('cat_b');
    expect(rows.map((row) => row.categoryId)).not.toContain('cat_c');
    expect(rows.map((row) => row.categoryId)).not.toContain('cat_x');
  });

  it('sorts by the average, largest first', () => {
    const rows = unbudgetedRows(averages, counts, new Set(['cat_c']), expenses);
    expect(rows.map((row) => row.categoryId)).toEqual(['cat_a', 'cat_d']);
  });

  it('counts a category the report has no count for as zero payments', () => {
    const rows = unbudgetedRows(averages, new Map(), new Set(), new Set(['cat_a']));
    expect(rows).toEqual([{ categoryId: 'cat_a', average: 4120, count: 0 }]);
  });

  it('has no rows without averages (the report is missing or cannot be trusted)', () => {
    expect(unbudgetedRows(new Map(), counts, new Set(), expenses)).toEqual([]);
  });
});

describe('reportRowLimit', () => {
  const stored = goal('cat_a', [version(month(2026, 0), '3000'), version(month(2026, 11), '3500')]);
  const periodStart = month(2026, 9);

  it('edits the stored budget from the month the report shows when the period has one', () => {
    // The report's own copy of the goal carries no versions: the stored one does
    const inPeriod = goal('cat_a', []);
    expect(reportRowLimit('cat_a', inPeriod, [ACTIVE, stored], periodStart)).toEqual({
      hasLimit: true,
      mode: { kind: 'edit', goal: stored, effectiveMonth: periodStart },
    });
  });

  it('adds a limit for the category from the month the report shows when the period has none', () => {
    expect(reportRowLimit('cat_a', null, [stored], periodStart)).toEqual({
      hasLimit: false,
      mode: { kind: 'add', categoryId: 'cat_a', effectiveMonth: periodStart },
    });
    expect(reportRowLimit('cat_a', undefined, [], periodStart).hasLimit).toBe(false);
  });

  it('knows there is a budget but cannot edit it while the stored goals are missing', () => {
    expect(reportRowLimit('cat_a', goal('cat_a', []), [], periodStart)).toEqual({
      hasLimit: true,
      mode: null,
    });
  });

  it('matches the stored budget by timeframe as well as category', () => {
    const quarterly = goal('cat_a', [version(month(2026, 0), '9000')], { timeframe: 'quarterly' });
    const inPeriod = goal('cat_a', [], { timeframe: 'quarterly' });
    expect(reportRowLimit('cat_a', inPeriod, [stored, quarterly], periodStart).mode).toEqual({
      kind: 'edit',
      goal: quarterly,
      effectiveMonth: periodStart,
    });
  });
});
