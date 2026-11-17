import type { BankAccount, Milestone, SyncAttachmentsProblem } from '@shared/schema';
import { daysBetween } from '@/utils/milestones';

/**
 * A balance nobody recorded for longer than this is stale: the backend's own `STALE_BALANCE_DAYS`
 * (`services/milestones/accounts.rs`), which `upkeep-backend-pin.test.ts` keeps this copy equal to.
 */
export const STALE_BALANCE_DAYS = 30;
/** An account whose newest import is older than this is due for the next statement. */
export const STATEMENT_DUE_DAYS = 35;

export type UpkeepKind =
  | 'balances_stale'
  | 'uncategorized'
  | 'statement_due'
  | 'backup_stale'
  | 'sync_stalled'
  | 'sync_attachments'
  | 'valuation_stale'
  | 'loan_balance_check'
  | 'loan_fixation_expired';

/** The loan reminders of the desktop's upkeep (`UPKEEP_KINDS`): one row per loan. */
const LOAN_KINDS = ['loan_fixation_expired', 'loan_balance_check'] as const;

/**
 * An i18n key of the `mobile` namespace (or another one named with its prefix, `milestones:…`) and
 * its values; a plural `count` picks the form.
 */
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
  /** The property a `valuation_stale` row is about (Přecenit opens it), the loan of a loan row. */
  sourceId?: string;
  /**
   * The desktop's upkeep milestone behind the row (`balances_stale`, `backup_stale`,
   * `valuation_stale` and the loan checks): the card hides or snoozes it as the desktop does. The
   * rows the phone works out itself have none.
   */
  milestone?: Milestone;
  /** A second line the row brings itself (the backup row with the sync flag on). */
  sub?: UpkeepTitle;
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
  /**
   * Whether a backup can be made on this device (`exportsOnThisDevice`); without it there is no
   * backup row, which would lead to a button that cannot work (audit C7). Default true.
   */
  canBackup?: boolean;
  /**
   * Why documents and photos do not sync while the rest does (no room, or the sync server too old),
   * with sync on; `null` or absent otherwise. A quiet row, never a reminder.
   */
  attachmentsProblem?: SyncAttachmentsProblem | null;
  /**
   * Sync is on (`syncIsOn`): the backup row names the newest safe copy (a backup or a complete
   * sync) and both ways out. Default false: the row reads as before.
   */
  syncOn?: boolean;
  /**
   * The sync flag is on and sync is off: the backup row's second line offers sync first, then a
   * backup (owner, 2026-11-17). `syncOn` wins. Default false.
   */
  offerSync?: boolean;
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
 * balances, uncategorized payments, statements, backup, stalled sync, valuations, then the loans' fixations and
 * balance checks. The card is absent when there are none.
 *
 * `balances_stale`, `backup_stale`, `valuation_stale` and the two loan checks are the desktop's
 * upkeep milestones (`UPKEEP_KINDS`), so a
 * reminder snoozed or muted there stays off the card; the milestone only counts the stale
 * accounts, so they are listed here by the backend's rule (not kept out of the balance, not
 * closed, dated more than 30 days ago). `statement_due` and `uncategorized` are computed here.
 */
export function buildUpkeepRows(input: UpkeepInput): UpkeepRow[] {
  const { todayDay, milestones, accounts, lastImportByAccount, syncOn = false } = input;
  const open = accounts.filter((account) => !isClosed(account, todayDay));
  const rows: UpkeepRow[] = [];

  const balances = milestones.find((m) => m.kind === 'balances_stale');
  if (balances) {
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
        milestone: balances,
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
  if (backup && input.canBackup !== false) {
    rows.push({
      id: 'backup_stale',
      kind: 'backup_stale',
      title:
        backup.sinceDay === null
          ? { key: syncOn ? 'upkeep.safeCopyNever' : 'upkeep.backupNever', params: {} }
          : {
              key: syncOn ? 'upkeep.safeCopyStale' : 'upkeep.backupStale',
              params: { count: Math.max(daysBetween(backup.sinceDay, todayDay), 0) },
            },
      ...(syncOn
        ? { sub: { key: 'upkeep.bothWaysOut', params: {} } }
        : input.offerSync
          ? { sub: { key: 'upkeep.syncOrBackUp', params: {} } }
          : {}),
      milestone: backup,
    });
  }

  // Sync stopped for good (the desktop's `sync_stalled` reminder): absent while healthy or merely
  // out of reach
  const syncStalled = milestones.find((m) => m.kind === 'sync_stalled');
  if (syncStalled) {
    rows.push({
      id: 'sync_stalled',
      kind: 'sync_stalled',
      title: { key: 'upkeep.syncStalled', params: {} },
      milestone: syncStalled,
    });
  }

  // Documents and photos wait for room or a newer sync server: the phone's own row, no desktop
  // milestone behind it (so no reminder and no menu); it leaves when the problem does
  if (input.attachmentsProblem) {
    rows.push({
      id: 'sync_attachments',
      kind: 'sync_attachments',
      title: { key: `deviceSync:attachments.problem.${input.attachmentsProblem}`, params: {} },
    });
  }

  for (const m of milestones.filter((m) => m.kind === 'valuation_stale')) {
    rows.push({
      id: `valuation_stale:${m.sourceId ?? m.key}`,
      kind: 'valuation_stale',
      title: { key: 'upkeep.valuationStale', params: { name: m.title } },
      sourceId: m.sourceId ?? undefined,
      milestone: m,
    });
  }

  // The loans' upkeep (B3): the balance to enter and the rate to fill in after a fixation ran out,
  // titled with the desktop's folded-line wording ("Hypotéka zadat aktuální zůstatek")
  for (const kind of LOAN_KINDS) {
    for (const m of milestones.filter((m) => m.kind === kind)) {
      rows.push({
        id: `${kind}:${m.sourceId ?? m.key}`,
        kind,
        title: { key: `milestones:short.${kind}`, params: { title: m.title } },
        sourceId: m.sourceId ?? undefined,
        milestone: m,
      });
    }
  }

  return rows;
}
