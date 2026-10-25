import type { BankAccount, Milestone } from '@shared/schema';
import { daysBetween } from '@/utils/milestones';

/** A balance nobody recorded for longer than this is stale (the backend's own `STALE_BALANCE_DAYS`). */
export const STALE_BALANCE_DAYS = 30;
/** An account whose newest import is older than this is due for the next statement. */
export const STATEMENT_DUE_DAYS = 35;

export type UpkeepKind =
  'balances_stale' | 'uncategorized' | 'statement_due' | 'backup_stale' | 'valuation_stale';

/** An i18n key of the `mobile` namespace and its values; a plural `count` picks the form. */
export interface UpkeepTitle {
  key: string;
  params: Record<string, string | number>;
}

export interface UpkeepRow {
  /** Stable across renders: the kind, plus what it is about when a kind can repeat. */
  id: string;
  kind: UpkeepKind;
  title: UpkeepTitle;
  /** The accounts the row is about: the stale ones, those holding payments, the one due a statement. */
  accountIds?: string[];
  /** UTC day shown in the sub-line of a `statement_due` row: the account's last import. */
  sinceDay?: number;
  /** The property a `valuation_stale` row is about: Přecenit opens it. */
  sourceId?: string;
}

/** What the card needs of an account (`BankAccount`, or one with its institution). */
export type UpkeepAccount = Pick<
  BankAccount,
  'id' | 'name' | 'balanceAsOf' | 'updatedAt' | 'excludeFromBalance' | 'terminationDate'
> & { institution?: { name: string } | null };

export interface UpkeepInput {
  /** UTC midnight of today (unix seconds). */
  todayDay: number;
  /** The milestones the backend lists: its snoozed, done and muted ones are not in it. */
  milestones: readonly Milestone[];
  accounts: readonly UpkeepAccount[];
  /** Unix seconds of each account's newest import; `null` or absent for one never imported into. */
  lastImportByAccount: Readonly<Record<string, number | null | undefined>>;
  /** Payments without a category, all accounts together. */
  uncategorized: number;
  /** The accounts holding them (the card names the account when there is one). */
  uncategorizedAccountIds?: readonly string[];
}

/** A closed account no longer asks for anything (as in the backend's milestones). */
function isClosed(account: UpkeepAccount, todayDay: number): boolean {
  return account.terminationDate !== null && daysBetween(todayDay, account.terminationDate) < 0;
}

/** The day an account's balance dates from: the recorded one, else its last edit. */
function balanceDay(account: UpkeepAccount): number {
  return account.balanceAsOf ?? account.updatedAt;
}

/**
 * The rows of the Údržba card on Přehled (spec §8.3 item 1), in the order they are shown: stale
 * balances, uncategorized payments, statements, backup, valuations. The card is absent when there
 * are none.
 *
 * `balances_stale`, `backup_stale` and `valuation_stale` are the desktop's upkeep milestones, so a
 * reminder snoozed or muted there stays off the card; the milestone only counts the stale
 * accounts, so they are listed here by the backend's rule (not kept out of the balance, not
 * closed, dated more than 30 days ago). `statement_due` and `uncategorized` are computed here.
 */
export function buildUpkeepRows(input: UpkeepInput): UpkeepRow[] {
  const { todayDay, milestones, accounts, lastImportByAccount } = input;
  const open = accounts.filter((account) => !isClosed(account, todayDay));
  const rows: UpkeepRow[] = [];

  if (milestones.some((m) => m.kind === 'balances_stale')) {
    const stale = open
      .filter(
        (account) =>
          !account.excludeFromBalance &&
          daysBetween(balanceDay(account), todayDay) > STALE_BALANCE_DAYS
      )
      .sort((a, b) => balanceDay(a) - balanceDay(b));
    if (stale.length > 0) {
      rows.push({
        id: 'balances_stale',
        kind: 'balances_stale',
        title: { key: 'upkeep.balancesStale', params: { count: stale.length } },
        accountIds: stale.map((account) => account.id),
      });
    }
  }

  if (input.uncategorized > 0) {
    rows.push({
      id: 'uncategorized',
      kind: 'uncategorized',
      title: { key: 'upkeep.uncategorized', params: { count: input.uncategorized } },
      accountIds: [...(input.uncategorizedAccountIds ?? [])],
    });
  }

  const due = open
    .flatMap((account) => {
      const last = lastImportByAccount[account.id];
      return last != null && daysBetween(last, todayDay) > STATEMENT_DUE_DAYS
        ? [{ account, last }]
        : [];
    })
    .sort((a, b) => a.last - b.last);
  for (const { account, last } of due) {
    rows.push({
      id: `statement_due:${account.id}`,
      kind: 'statement_due',
      title: {
        key: 'upkeep.statementDue',
        params: { bank: account.institution?.name ?? account.name },
      },
      accountIds: [account.id],
      sinceDay: last,
    });
  }

  const backup = milestones.find((m) => m.kind === 'backup_stale');
  if (backup) {
    rows.push({
      id: 'backup_stale',
      kind: 'backup_stale',
      title:
        backup.sinceDay === null
          ? { key: 'upkeep.backupNever', params: {} }
          : {
              key: 'upkeep.backupStale',
              params: { count: Math.max(daysBetween(backup.sinceDay, todayDay), 0) },
            },
    });
  }

  for (const m of milestones.filter((m) => m.kind === 'valuation_stale')) {
    rows.push({
      id: `valuation_stale:${m.sourceId ?? m.key}`,
      kind: 'valuation_stale',
      title: { key: 'upkeep.valuationStale', params: { name: m.title } },
      sourceId: m.sourceId ?? undefined,
    });
  }

  return rows;
}
