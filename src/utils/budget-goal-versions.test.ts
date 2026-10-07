import { describe, expect, it } from 'vitest';
import type { BudgetGoalVersion } from '@shared/schema';
import {
  defaultEditMonth,
  isUnchangedGoalEdit,
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

describe('isUnchangedGoalEdit', () => {
  const versions = [v(JAN, '1000.123'), v(DEC, '2000')];

  it('is unchanged when the amount equals the version in effect for that month', () => {
    expect(isUnchangedGoalEdit(versions, OCT, 1000.123, 'CZK')).toBe(true);
    expect(isUnchangedGoalEdit(versions, DEC, 2000, 'CZK')).toBe(true);
  });

  it('is a change for another amount, currency or a month without a limit', () => {
    expect(isUnchangedGoalEdit(versions, OCT, 2000, 'CZK')).toBe(false);
    expect(isUnchangedGoalEdit(versions, OCT, 1000.123, 'EUR')).toBe(false);
    expect(isUnchangedGoalEdit(versions, JAN - 1, 1000.123, 'CZK')).toBe(false);
  });
});

describe('month input helpers', () => {
  it('round-trips a UTC month start through the <input type="month"> value', () => {
    expect(monthInputValue(OCT)).toBe('2026-10');
    expect(monthStartFromInput('2026-10')).toBe(OCT);
    expect(monthStartFromInput('2026-1')).toBeNull();
  });
});
