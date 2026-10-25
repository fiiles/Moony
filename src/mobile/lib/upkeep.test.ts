import { describe, expect, it } from 'vitest';
import type { Milestone, MilestoneKind } from '@shared/schema';
import {
  buildUpkeepRows,
  STALE_BALANCE_DAYS,
  STATEMENT_DUE_DAYS,
  type UpkeepAccount,
  type UpkeepInput,
} from './upkeep';

const DAY = 86_400;
/** 16 October 2026, UTC midnight. */
const TODAY = Date.UTC(2026, 9, 16) / 1000;
const ago = (days: number) => TODAY - days * DAY;

function milestone(kind: MilestoneKind, patch: Partial<Milestone> = {}): Milestone {
  return {
    key: `${kind}:test`,
    kind,
    stage: 'now',
    tone: 'action',
    sourceId: null,
    title: '',
    dueDay: null,
    actionDay: null,
    sinceDay: null,
    amount: null,
    currency: null,
    referenceAmount: null,
    direction: null,
    count: null,
    ...patch,
  };
}

function account(id: string, patch: Partial<UpkeepAccount> = {}): UpkeepAccount {
  return {
    id,
    name: `Účet ${id}`,
    balanceAsOf: ago(1),
    updatedAt: ago(1),
    excludeFromBalance: false,
    terminationDate: null,
    institution: null,
    ...patch,
  };
}

function input(patch: Partial<UpkeepInput> = {}): UpkeepInput {
  return {
    todayDay: TODAY,
    milestones: [],
    accounts: [],
    lastImportByAccount: {},
    uncategorized: 0,
    ...patch,
  };
}

const kinds = (rows: { kind: string }[]) => rows.map((r) => r.kind);

describe('buildUpkeepRows', () => {
  it('is empty for empty input', () => {
    expect(buildUpkeepRows(input())).toEqual([]);
  });

  it('is empty when everything is fresh', () => {
    expect(
      buildUpkeepRows(
        input({
          accounts: [account('a')],
          lastImportByAccount: { a: ago(3) },
          milestones: [],
        })
      )
    ).toEqual([]);
  });

  describe('balances_stale', () => {
    const stale = milestone('balances_stale', { count: 1, sinceDay: ago(31) });

    it('thresholds: 30 days old is current, 31 is stale', () => {
      expect(STALE_BALANCE_DAYS).toBe(30);
      const at30 = buildUpkeepRows(
        input({
          milestones: [stale],
          accounts: [account('a', { balanceAsOf: ago(STALE_BALANCE_DAYS) })],
        })
      );
      expect(at30).toEqual([]);
      const at31 = buildUpkeepRows(
        input({
          milestones: [stale],
          accounts: [account('a', { balanceAsOf: ago(STALE_BALANCE_DAYS + 1) })],
        })
      );
      expect(kinds(at31)).toEqual(['balances_stale']);
    });

    it('lists the stale accounts, oldest first, with their count in the title', () => {
      const rows = buildUpkeepRows(
        input({
          milestones: [stale],
          accounts: [
            account('fresh', { balanceAsOf: ago(2) }),
            account('b', { balanceAsOf: ago(40) }),
            account('a', { balanceAsOf: ago(90) }),
          ],
        })
      );
      expect(rows).toHaveLength(1);
      expect(rows[0].accountIds).toEqual(['a', 'b']);
      expect(rows[0].title).toEqual({ key: 'upkeep.balancesStale', params: { count: 2 } });
    });

    it('dates an account without a recorded balance by its last edit', () => {
      const rows = buildUpkeepRows(
        input({
          milestones: [stale],
          accounts: [account('a', { balanceAsOf: null, updatedAt: ago(45) })],
        })
      );
      expect(rows[0].accountIds).toEqual(['a']);
    });

    it('leaves out accounts kept out of the balance and closed accounts', () => {
      const rows = buildUpkeepRows(
        input({
          milestones: [stale],
          accounts: [
            account('excluded', { balanceAsOf: ago(60), excludeFromBalance: true }),
            account('closed', { balanceAsOf: ago(60), terminationDate: ago(5) }),
            account('open', { balanceAsOf: ago(60), terminationDate: TODAY + 20 * DAY }),
          ],
        })
      );
      expect(rows[0].accountIds).toEqual(['open']);
    });

    it('needs the milestone: a snoozed or muted reminder stays off the card', () => {
      expect(
        buildUpkeepRows(input({ accounts: [account('a', { balanceAsOf: ago(90) })] }))
      ).toEqual([]);
    });

    it('is no row when the milestone is there but no account can be listed', () => {
      expect(buildUpkeepRows(input({ milestones: [stale], accounts: [] }))).toEqual([]);
    });
  });

  describe('uncategorized', () => {
    it('is a row for any positive count', () => {
      const rows = buildUpkeepRows(
        input({ uncategorized: 12, uncategorizedAccountIds: ['a', 'b'] })
      );
      expect(rows).toHaveLength(1);
      expect(rows[0].kind).toBe('uncategorized');
      expect(rows[0].title).toEqual({ key: 'upkeep.uncategorized', params: { count: 12 } });
      expect(rows[0].accountIds).toEqual(['a', 'b']);
    });

    it('is absent at zero', () => {
      expect(buildUpkeepRows(input({ uncategorized: 0 }))).toEqual([]);
    });

    it('works without the accounts that hold the payments', () => {
      expect(buildUpkeepRows(input({ uncategorized: 3 }))[0].accountIds).toEqual([]);
    });
  });

  describe('statement_due', () => {
    it('thresholds: an import 35 days old is current, 36 is due', () => {
      expect(STATEMENT_DUE_DAYS).toBe(35);
      const accounts = [account('a')];
      expect(
        buildUpkeepRows(input({ accounts, lastImportByAccount: { a: ago(STATEMENT_DUE_DAYS) } }))
      ).toEqual([]);
      expect(
        kinds(
          buildUpkeepRows(
            input({ accounts, lastImportByAccount: { a: ago(STATEMENT_DUE_DAYS + 1) } })
          )
        )
      ).toEqual(['statement_due']);
    });

    it('counts whole UTC days, whatever the hour of the import', () => {
      const accounts = [account('a')];
      // 35 days and 23 hours ago is still "35 days" on the calendar
      expect(
        buildUpkeepRows(input({ accounts, lastImportByAccount: { a: ago(35) + 3_600 } }))
      ).toEqual([]);
      expect(
        kinds(buildUpkeepRows(input({ accounts, lastImportByAccount: { a: ago(36) + 3_600 } })))
      ).toEqual(['statement_due']);
    });

    it('an account with imports newer than 35 days produces no row', () => {
      expect(
        buildUpkeepRows(input({ accounts: [account('a')], lastImportByAccount: { a: ago(10) } }))
      ).toEqual([]);
    });

    it('an account that was never imported into is not due', () => {
      expect(
        buildUpkeepRows(input({ accounts: [account('a')], lastImportByAccount: { a: null } }))
      ).toEqual([]);
      expect(buildUpkeepRows(input({ accounts: [account('a')] }))).toEqual([]);
    });

    it('names the bank, or the account without an institution, and carries the import day', () => {
      const rows = buildUpkeepRows(
        input({
          accounts: [
            account('a', { institution: { name: 'UniCredit' } }),
            account('b', { name: 'Spořicí účet' }),
          ],
          lastImportByAccount: { a: ago(38), b: ago(70) },
        })
      );
      expect(rows.map((r) => r.title)).toEqual([
        { key: 'upkeep.statementDue', params: { bank: 'Spořicí účet' } },
        { key: 'upkeep.statementDue', params: { bank: 'UniCredit' } },
      ]);
      expect(rows.map((r) => r.accountIds)).toEqual([['b'], ['a']]);
      expect(rows.map((r) => r.sinceDay)).toEqual([ago(70), ago(38)]);
    });

    it('skips closed accounts', () => {
      expect(
        buildUpkeepRows(
          input({
            accounts: [account('a', { terminationDate: ago(10) })],
            lastImportByAccount: { a: ago(80) },
          })
        )
      ).toEqual([]);
    });
  });

  describe('backup_stale', () => {
    it('counts the days since the last backup', () => {
      const rows = buildUpkeepRows(
        input({ milestones: [milestone('backup_stale', { sinceDay: ago(41) })] })
      );
      expect(rows).toHaveLength(1);
      expect(rows[0].title).toEqual({ key: 'upkeep.backupStale', params: { count: 41 } });
    });

    it('says there has been no backup when the milestone has no day', () => {
      const rows = buildUpkeepRows(input({ milestones: [milestone('backup_stale')] }));
      expect(rows[0].title).toEqual({ key: 'upkeep.backupNever', params: {} });
    });
  });

  describe('valuation_stale', () => {
    it('is one row per property, named', () => {
      const rows = buildUpkeepRows(
        input({
          milestones: [
            milestone('valuation_stale', { sourceId: 'p1', title: 'Byt Lipová' }),
            milestone('valuation_stale', { sourceId: 'p2', title: 'Chata' }),
          ],
        })
      );
      expect(rows.map((r) => r.title)).toEqual([
        { key: 'upkeep.valuationStale', params: { name: 'Byt Lipová' } },
        { key: 'upkeep.valuationStale', params: { name: 'Chata' } },
      ]);
    });

    it('keeps the property it is about, for Přecenit', () => {
      const rows = buildUpkeepRows(
        input({
          milestones: [
            milestone('valuation_stale', { sourceId: 'p1', title: 'Byt Lipová' }),
            milestone('valuation_stale', { title: 'Bez id' }),
          ],
        })
      );
      expect(rows.map((r) => r.sourceId)).toEqual(['p1', undefined]);
    });
  });

  it('ignores milestones that are not upkeep of this card', () => {
    expect(
      buildUpkeepRows(
        input({
          milestones: [
            milestone('insurance_end'),
            milestone('loan_balance_check'),
            milestone('loan_fixation_expired'),
          ],
        })
      )
    ).toEqual([]);
  });

  it('orders balances, payments, statements, backup, valuations', () => {
    const rows = buildUpkeepRows(
      input({
        milestones: [
          milestone('valuation_stale', { sourceId: 'p', title: 'Byt' }),
          milestone('backup_stale', { sinceDay: ago(50) }),
          milestone('balances_stale', { count: 1 }),
        ],
        accounts: [account('a', { balanceAsOf: ago(60) })],
        lastImportByAccount: { a: ago(60) },
        uncategorized: 4,
        uncategorizedAccountIds: ['a'],
      })
    );
    expect(kinds(rows)).toEqual([
      'balances_stale',
      'uncategorized',
      'statement_due',
      'backup_stale',
      'valuation_stale',
    ]);
  });

  it('gives every row its own stable id', () => {
    const rows = buildUpkeepRows(
      input({
        milestones: [
          milestone('valuation_stale', { sourceId: 'p1', title: 'A' }),
          milestone('valuation_stale', { sourceId: 'p2', title: 'B' }),
        ],
        accounts: [account('a'), account('b')],
        lastImportByAccount: { a: ago(60), b: ago(60) },
      })
    );
    const ids = rows.map((r) => r.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
