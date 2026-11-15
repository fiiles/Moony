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

    it('still asks when a balance was recorded by hand after the import (Skrýt puts it away)', () => {
      const manual = account('a', { balanceAsOf: ago(10) });
      expect(
        kinds(buildUpkeepRows(input({ accounts: [manual], lastImportByAccount: { a: ago(50) } })))
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
            account('a', { institution: { name: 'UniCredit' }, balanceAsOf: ago(38) }),
            // The balance comes from the statement itself, so the account is not kept by hand
            account('b', { name: 'Spořicí účet', balanceAsOf: ago(70) }),
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

    it('is left out where a backup cannot be made (Android for now)', () => {
      const rows = buildUpkeepRows(
        input({ milestones: [milestone('backup_stale')], canBackup: false })
      );
      expect(rows).toEqual([]);
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
          milestones: [milestone('insurance_end'), milestone('watch_target')],
        })
      )
    ).toEqual([]);
  });

  it('carries the milestone behind a row, so the card can hide or snooze it', () => {
    const balances = milestone('balances_stale', { count: 1 });
    const backup = milestone('backup_stale', { sinceDay: ago(50) });
    const valuation = milestone('valuation_stale', { sourceId: 'p', title: 'Byt' });
    const rows = buildUpkeepRows(
      input({
        milestones: [valuation, backup, balances],
        accounts: [account('a', { balanceAsOf: ago(60) })],
        lastImportByAccount: { a: ago(60) },
        uncategorized: 4,
        uncategorizedAccountIds: ['a'],
      })
    );
    expect(rows.map((row) => row.milestone)).toEqual([
      balances,
      undefined,
      undefined,
      backup,
      valuation,
    ]);
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

  describe('loan checks (B3)', () => {
    it('makes one row per loan milestone, after the valuations, titled as on the desktop', () => {
      const rows = buildUpkeepRows(
        input({
          milestones: [
            milestone('loan_fixation_expired', { key: 'fx:l2', sourceId: 'l2', title: 'Auto' }),
            milestone('valuation_stale', { sourceId: 'p', title: 'Byt' }),
            milestone('loan_balance_check', { key: 'bc:l1', sourceId: 'l1', title: 'Hypotéka' }),
          ],
        })
      );
      expect(rows.map((r) => [r.kind, r.sourceId, r.title.key, r.title.params])).toEqual([
        ['valuation_stale', 'p', 'upkeep.valuationStale', { name: 'Byt' }],
        [
          'loan_fixation_expired',
          'l2',
          'milestones:short.loan_fixation_expired',
          { title: 'Auto' },
        ],
        ['loan_balance_check', 'l1', 'milestones:short.loan_balance_check', { title: 'Hypotéka' }],
      ]);
      // The desktop's milestone menu (hide, snooze, mute) comes with them
      expect(rows.slice(1).every((r) => r.milestone !== undefined)).toBe(true);
    });

    it('keys a loan row by its loan, or by the milestone without one', () => {
      const rows = buildUpkeepRows(
        input({
          milestones: [
            milestone('loan_balance_check', { key: 'bc:x', sourceId: null, title: 'X' }),
            milestone('loan_balance_check', { key: 'bc:l1', sourceId: 'l1', title: 'Y' }),
          ],
        })
      );
      expect(rows.map((r) => r.id)).toEqual(['loan_balance_check:bc:x', 'loan_balance_check:l1']);
    });
  });

  it('gives every row its own stable id', () => {
    const rows = buildUpkeepRows(
      input({
        milestones: [
          milestone('valuation_stale', { sourceId: 'p1', title: 'A' }),
          milestone('valuation_stale', { sourceId: 'p2', title: 'B' }),
        ],
        accounts: [account('a', { balanceAsOf: ago(60) }), account('b', { balanceAsOf: ago(60) })],
        lastImportByAccount: { a: ago(60), b: ago(60) },
      })
    );
    const ids = rows.map((r) => r.id);
    expect(ids.filter((id) => id.startsWith('statement_due:'))).toHaveLength(2);
    expect(new Set(ids).size).toBe(ids.length);
  });
  describe('sync_stalled', () => {
    it('is one row when the milestone is there, between backup and valuations', () => {
      const rows = buildUpkeepRows(
        input({
          milestones: [
            milestone('backup_stale', { sinceDay: ago(40) }),
            milestone('valuation_stale', { sourceId: 'p1', title: 'Byt' }),
            milestone('sync_stalled'),
          ],
        })
      );
      expect(rows.map((r) => r.id)).toEqual(['backup_stale', 'sync_stalled', 'valuation_stale:p1']);
      const row = rows[1];
      expect(row.title.key).toBe('upkeep.syncStalled');
      expect(row.milestone?.kind).toBe('sync_stalled');
    });

    it('is absent without the milestone', () => {
      const rows = buildUpkeepRows(input({ milestones: [milestone('backup_stale')] }));
      expect(rows.map((r) => r.kind)).not.toContain('sync_stalled');
    });
  });
  describe('sync_attachments', () => {
    it('is one quiet row without a milestone, after the sync reminder', () => {
      const rows = buildUpkeepRows(
        input({
          milestones: [milestone('sync_stalled')],
          attachmentsProblem: 'noRoom',
        })
      );
      expect(rows.map((r) => r.id)).toEqual(['sync_stalled', 'sync_attachments']);
      const row = rows[1];
      expect(row.title.key).toBe('deviceSync:attachments.problem.noRoom');
      expect(row.milestone).toBeUndefined();
    });

    it('names the problem it is about', () => {
      const rows = buildUpkeepRows(input({ attachmentsProblem: 'serverNeedsUpdate' }));
      expect(rows.map((r) => r.title.key)).toEqual([
        'deviceSync:attachments.problem.serverNeedsUpdate',
      ]);
    });

    it('is absent without a problem', () => {
      expect(buildUpkeepRows(input({}))).toEqual([]);
      expect(buildUpkeepRows(input({ attachmentsProblem: null }))).toEqual([]);
    });
  });
});
