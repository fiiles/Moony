import { describe, expect, it } from 'vitest';
import type { BudgetGoal, BudgetGoalVersion } from '@shared/schema';
import {
  budgetOverview,
  defaultEditMonth,
  budgetVersionPeriods,
  budgetEditSource,
  intervalLastMonth,
  isUnchangedGoalEdit,
  overwrittenPeriods,
  monthInputValue,
  monthStartFromInput,
  scheduledVersions,
  versionInEffect,
} from './budget-goal-versions';

const JAN = Date.UTC(2026, 0, 1) / 1000;
const JUN = Date.UTC(2026, 5, 1) / 1000;
const SEP = Date.UTC(2026, 8, 1) / 1000;
const OCT = Date.UTC(2026, 9, 1) / 1000;
const DEC = Date.UTC(2026, 11, 1) / 1000;

const v = (effectiveMonth: number, amount: string | null, currency = 'CZK'): BudgetGoalVersion => ({
  effectiveMonth,
  amount,
  currency,
});

describe('versionInEffect', () => {
  const versions = [v(JAN, '1000'), v(JUN, '1500'), v(DEC, '2000')];

  it('picks the latest version starting on or before the month', () => {
    expect(versionInEffect(versions, OCT)?.amount).toBe('1500');
    expect(versionInEffect(versions, JUN)?.amount).toBe('1500');
    expect(versionInEffect(versions, DEC)?.amount).toBe('2000');
  });

  it('has nothing before the first version or after a removal', () => {
    expect(versionInEffect(versions, JAN - 1)).toBeNull();
    expect(versionInEffect([v(JAN, '1000'), v(JUN, null)], OCT)).toBeNull();
  });
});

describe('scheduledVersions', () => {
  it('lists only versions after the given month', () => {
    const versions = [v(JAN, '1000'), v(DEC, '2000')];
    expect(scheduledVersions(versions, OCT)).toEqual([v(DEC, '2000')]);
    expect(scheduledVersions(versions, DEC)).toEqual([]);
  });
});

describe('budgetVersionPeriods', () => {
  it('ends each version immediately before the next and keeps the final interval open', () => {
    expect(budgetVersionPeriods([v(JAN, '1000'), v(JUN, null), v(DEC, '2000', 'EUR')])).toEqual([
      { version: v(JAN, '1000'), end: JUN - 1 },
      { version: v(JUN, null), end: DEC - 1 },
      { version: v(DEC, '2000', 'EUR'), end: null },
    ]);
  });

  it('has no intervals when no versions are configured', () => {
    expect(budgetVersionPeriods([])).toEqual([]);
  });
});

describe('defaultEditMonth', () => {
  it('edits the current month while a limit is in effect', () => {
    expect(defaultEditMonth([v(JAN, '1000'), v(DEC, '2000')], OCT)).toBe(OCT);
  });

  it('edits the first scheduled month of a goal that starts later', () => {
    expect(defaultEditMonth([v(DEC, '2000')], OCT)).toBe(DEC);
    expect(defaultEditMonth([v(JAN, '1000'), v(SEP, null), v(DEC, '2000')], OCT)).toBe(DEC);
  });

  it('falls back to the current month without versions', () => {
    expect(defaultEditMonth([], OCT)).toBe(OCT);
  });
});

describe('budgetEditSource', () => {
  const goal: BudgetGoal = {
    id: 'groceries',
    categoryId: 'cat_groceries',
    timeframe: 'monthly',
    amount: '200',
    currency: 'CZK',
    createdAt: JUN,
    updatedAt: OCT,
    effectiveMonth: OCT,
    versions: [v(JUN, '100.123', 'EUR'), v(OCT, '200', 'CZK')],
  };

  it('takes the selected historical interval amount and currency instead of the current limit', () => {
    expect(budgetEditSource(goal, JUN, OCT)).toEqual({
      month: JUN,
      amount: '100.123',
      currency: 'EUR',
    });
  });

  it('defaults to the current version and preserves native text', () => {
    expect(budgetEditSource(goal, undefined, OCT)).toEqual({
      month: OCT,
      amount: '200',
      currency: 'CZK',
    });
  });

  it('keeps the requested month before the first version and uses the displayed limit as its starting value', () => {
    expect(budgetEditSource(goal, JAN, OCT)).toEqual({
      month: JAN,
      amount: '200',
      currency: 'CZK',
    });
  });
});

describe('isUnchangedGoalEdit', () => {
  const versions = [v(JAN, '1000.123'), v(DEC, '2000')];

  it('is unchanged when the amount equals the version in effect for that month', () => {
    // Through November; an open save would also replace December's change.
    expect(isUnchangedGoalEdit(versions, OCT, 1000.123, 'CZK', OCT)).toBe(true);
    expect(isUnchangedGoalEdit(versions, DEC, 2000, 'CZK')).toBe(true);
  });

  it('is a change for another amount, currency or a month without a limit', () => {
    expect(isUnchangedGoalEdit(versions, OCT, 2000, 'CZK')).toBe(false);
    expect(isUnchangedGoalEdit(versions, OCT, 1000.123, 'EUR')).toBe(false);
    expect(isUnchangedGoalEdit(versions, JAN - 1, 1000.123, 'CZK')).toBe(false);
  });
});

describe('isUnchangedGoalEdit with an end month', () => {
  const versions = [v(JAN, '1000'), v(DEC, '2000')];
  const NOV = Date.UTC(2026, 10, 1) / 1000;

  it('is unchanged when the whole range already has that limit', () => {
    expect(isUnchangedGoalEdit(versions, OCT, 1000, 'CZK', NOV)).toBe(true);
    expect(isUnchangedGoalEdit(versions, OCT, 1000, 'CZK', OCT)).toBe(true);
  });

  it('is a change when the range reaches into a different limit', () => {
    expect(isUnchangedGoalEdit(versions, OCT, 1000, 'CZK', DEC)).toBe(false);
  });
});

describe('isUnchangedGoalEdit without an end month', () => {
  it('is a change when later months differ, e.g. extending an interval back over a gap', () => {
    const versions = [v(JAN, '5000'), v(JUN, null), v(SEP, '5000')];
    expect(isUnchangedGoalEdit(versions, JAN, 5000, 'CZK')).toBe(false);
    expect(isUnchangedGoalEdit([v(JAN, '5000'), v(DEC, '6000')], JAN, 5000, 'CZK')).toBe(false);
  });

  it('is unchanged only when the limit already runs on', () => {
    expect(isUnchangedGoalEdit([v(JAN, '5000')], OCT, 5000, 'CZK')).toBe(true);
  });
});

describe('overwrittenPeriods', () => {
  const Y2025_JUL = Date.UTC(2025, 6, 1) / 1000;
  const Y2027_JAN = Date.UTC(2027, 0, 1) / 1000;
  const year2026 = [v(JAN, '5000'), v(Y2027_JAN, null)];

  it('lists the limits a wider range replaces, the edited one included', () => {
    expect(
      overwrittenPeriods(year2026, Y2025_JUL, null, 7000, 'CZK', { start: JAN, end: DEC })
    ).toEqual([{ version: v(JAN, '5000'), end: Y2027_JAN - 1 }]);
  });

  it('stays quiet while the range keeps inside the edited interval', () => {
    expect(overwrittenPeriods(year2026, OCT, DEC, 7000, 'CZK', { start: JAN, end: DEC })).toEqual(
      []
    );
  });

  it('lists overlapping limits for a new budget but not equal ones or limits outside', () => {
    const versions = [v(JAN, '100'), v(JUN, '200'), v(OCT, '300')];
    expect(overwrittenPeriods(versions, JUN, SEP, 100, 'CZK', null)).toEqual([
      { version: v(JUN, '200'), end: OCT - 1 },
    ]);
    expect(overwrittenPeriods(versions, JUN, SEP, 200, 'CZK', null)).toEqual([]);
  });
});

describe('intervalLastMonth', () => {
  const NOV = Date.UTC(2026, 10, 1) / 1000;

  it('is the month before the next change, whatever that change is', () => {
    expect(intervalLastMonth([v(JAN, '1000'), v(DEC, '2000')], OCT)).toBe(NOV);
    expect(intervalLastMonth([v(JAN, '1000'), v(NOV, null)], JAN)).toBe(OCT);
  });

  it('is null for an open-ended interval', () => {
    expect(intervalLastMonth([v(JAN, '1000'), v(JUN, '1500')], OCT)).toBeNull();
    expect(intervalLastMonth([], OCT)).toBeNull();
  });

  it('crosses a year boundary', () => {
    expect(intervalLastMonth([v(OCT, '1'), v(JAN + 365 * 86_400, '2')], OCT)).toBe(DEC);
  });
});

describe('month input helpers', () => {
  it('round-trips a UTC month start through the <input type="month"> value', () => {
    expect(monthInputValue(OCT)).toBe('2026-10');
    expect(monthStartFromInput('2026-10')).toBe(OCT);
    expect(monthStartFromInput('2026-1')).toBeNull();
  });

  it('rejects out-of-range months rather than returning NaN or a normalized date', () => {
    for (const value of ['2026-00', '2026-13', '0000-01', '', '2026-02-01']) {
      expect(monthStartFromInput(value)).toBeNull();
    }
  });
});

describe('budgetOverview', () => {
  const MAR = Date.UTC(2026, 2, 1) / 1000;
  const NOV = Date.UTC(2026, 10, 1) / 1000;
  const goal = (categoryId: string, versions: BudgetGoalVersion[]): BudgetGoal => ({
    id: categoryId,
    categoryId,
    timeframe: 'monthly',
    amount: '0',
    currency: 'CZK',
    createdAt: JAN,
    updatedAt: JAN,
    effectiveMonth: null,
    versions,
  });

  it('shows the limit in effect with its upcoming changes and earlier limits newest first', () => {
    const [row] = budgetOverview(
      [goal('a', [v(JAN, '700'), v(MAR, '800'), v(JUN, null), v(SEP, '900'), v(NOV, '950')])],
      OCT
    );
    expect(row.status).toBe('active');
    expect(row.main).toEqual({ version: v(SEP, '900'), end: NOV - 1 });
    expect(row.upcoming).toEqual([v(NOV, '950')]);
    expect(row.history).toEqual([
      { version: v(MAR, '800'), end: JUN - 1 },
      { version: v(JAN, '700'), end: MAR - 1 },
    ]);
  });

  it('lists a scheduled removal as an upcoming change', () => {
    const [row] = budgetOverview([goal('a', [v(JAN, '700'), v(DEC, null)])], OCT);
    expect(row.status).toBe('active');
    expect(row.upcoming).toEqual([v(DEC, null)]);
  });

  it('shows a goal that only starts later by its first scheduled limit', () => {
    const [row] = budgetOverview([goal('a', [v(JAN, '500'), v(MAR, null), v(NOV, '600')])], OCT);
    expect(row.status).toBe('scheduled');
    expect(row.main).toEqual({ version: v(NOV, '600'), end: null });
    expect(row.upcoming).toEqual([]);
    expect(row.history).toEqual([{ version: v(JAN, '500'), end: MAR - 1 }]);
  });

  it('shows an ended goal by its last limit and skips goals without any limit', () => {
    const rows = budgetOverview(
      [goal('a', [v(JAN, '500'), v(MAR, '550'), v(JUN, null)]), goal('b', [v(JAN, null)])],
      OCT
    );
    expect(rows).toHaveLength(1);
    expect(rows[0].status).toBe('ended');
    expect(rows[0].main).toEqual({ version: v(MAR, '550'), end: JUN - 1 });
    expect(rows[0].upcoming).toEqual([]);
    expect(rows[0].history).toEqual([{ version: v(JAN, '500'), end: MAR - 1 }]);
  });
});
