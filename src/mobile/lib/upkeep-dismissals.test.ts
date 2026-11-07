import { describe, expect, it } from 'vitest';
import {
  UPKEEP_SNOOZE_DAYS,
  dismissKey,
  hideRow,
  isDismissed,
  parseDismissals,
  pruneDismissals,
  restoreRow,
  snoozeRow,
} from './upkeep-dismissals';
import type { UpkeepRow } from './upkeep';

const DAY = 86_400;
const TODAY = Date.UTC(2026, 10, 7) / 1000;

const statement = (accountId: string, sinceDay: number): UpkeepRow => ({
  id: `statement_due:${accountId}`,
  kind: 'statement_due',
  title: { key: 'upkeep.statementDue', params: { bank: 'Fio' } },
  accountIds: [accountId],
  sinceDay,
});
const uncategorized = (count: number): UpkeepRow => ({
  id: 'uncategorized',
  kind: 'uncategorized',
  title: { key: 'upkeep.uncategorized', params: { count } },
  accountIds: ['a'],
});

describe('dismissKey', () => {
  it('keys a statement row by its account and last import, the uncategorized row by itself', () => {
    expect(dismissKey(statement('a', 100))).toBe('statement_due:a:100');
    expect(dismissKey(uncategorized(3))).toBe('uncategorized');
  });

  it('has no key for a row that stands for a desktop milestone', () => {
    expect(dismissKey({ kind: 'backup_stale' })).toBeNull();
    expect(dismissKey({ kind: 'loan_balance_check' })).toBeNull();
  });
});

describe('hiding and snoozing', () => {
  it('hides a statement row until the account is imported into again', () => {
    const hidden = hideRow({}, statement('a', 100));
    expect(isDismissed(statement('a', 100), hidden, TODAY)).toBe(true);
    expect(isDismissed(statement('a', 200), hidden, TODAY)).toBe(false);
    expect(isDismissed(statement('b', 100), hidden, TODAY)).toBe(false);
  });

  it('hides the uncategorized row until more payments wait than when it was hidden', () => {
    const hidden = hideRow({}, uncategorized(5));
    expect(isDismissed(uncategorized(5), hidden, TODAY)).toBe(true);
    expect(isDismissed(uncategorized(2), hidden, TODAY)).toBe(true);
    expect(isDismissed(uncategorized(6), hidden, TODAY)).toBe(false);
  });

  it('snoozes for a month, then the row is back', () => {
    const snoozed = snoozeRow({}, uncategorized(5), TODAY);
    expect(isDismissed(uncategorized(50), snoozed, TODAY)).toBe(true);
    expect(isDismissed(uncategorized(50), snoozed, TODAY + (UPKEEP_SNOOZE_DAYS - 1) * DAY)).toBe(
      true
    );
    expect(isDismissed(uncategorized(50), snoozed, TODAY + UPKEEP_SNOOZE_DAYS * DAY)).toBe(false);
  });

  it('brings a row back with Vrátit, and leaves the others alone', () => {
    const both = snoozeRow(hideRow({}, statement('a', 100)), uncategorized(1), TODAY);
    const restored = restoreRow(both, uncategorized(1));
    expect(isDismissed(uncategorized(1), restored, TODAY)).toBe(false);
    expect(isDismissed(statement('a', 100), restored, TODAY)).toBe(true);
  });

  it('never puts away a desktop milestone row', () => {
    const row = { kind: 'backup_stale', title: { key: 'k', params: {} } } as const;
    expect(hideRow({}, row)).toEqual({});
    expect(isDismissed(row, { uncategorized: { until: null } }, TODAY)).toBe(false);
  });
});

describe('storage', () => {
  it('drops snoozes that ran out and keeps the rest', () => {
    const stored = {
      old: { until: TODAY - DAY },
      running: { until: TODAY + DAY },
      'statement_due:a:1': { until: null },
    };
    expect(pruneDismissals(stored, TODAY)).toEqual({
      running: { until: TODAY + DAY },
      'statement_due:a:1': { until: null },
    });
  });

  it('reads back what it stored and treats anything else as nothing put away', () => {
    const stored = hideRow(snoozeRow({}, statement('a', 1), TODAY), uncategorized(4));
    expect(parseDismissals(JSON.stringify(stored))).toEqual(stored);
    expect(parseDismissals(null)).toEqual({});
    expect(parseDismissals('not json')).toEqual({});
    expect(parseDismissals('[1]')).toEqual({});
    expect(parseDismissals('{"a":{"until":"x"},"b":3}')).toEqual({});
  });
});
