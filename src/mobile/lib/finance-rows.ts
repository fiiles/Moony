import { recordCurrency } from '@shared/currencies';
import { compareConverted } from '@/utils/fx-totals';
import { daysBetween } from '@/utils/milestones';
import { STALE_BALANCE_DAYS } from './upkeep';

/** A statement or balance older than this many days is called stale (the Údržba card's threshold). */
export const STALE_DAYS = STALE_BALANCE_DAYS;

/** How an account row's sub-line is made: the bank, the currency and the date it stands on. */
export interface AccountSub {
  institution: string | null;
  currency: string;
  /** The newest imported statement, else the date of the balance; `null` for a new, empty account. */
  stamp: { kind: 'statement' | 'balance'; day: number } | null;
}

/** The parts of an account row's sub-line ("Fio · CZK · výpis 10. 10."); the component words them. */
export function accountSub(
  account: {
    institution: { name: string } | null;
    currency: string;
    balanceAsOf: number | null;
  },
  lastImport: number | null
): AccountSub {
  const stamp: AccountSub['stamp'] =
    lastImport != null
      ? { kind: 'statement', day: lastImport }
      : account.balanceAsOf != null
        ? { kind: 'balance', day: account.balanceAsOf }
        : null;
  return {
    institution: account.institution?.name || null,
    currency: recordCurrency(account.currency),
    stamp,
  };
}

/** What stands under an account's balance on the right (prototype frame 03). */
export type AccountTrail =
  | { kind: 'change'; net: number }
  | { kind: 'converted'; value: number | null }
  | { kind: 'stale'; what: 'statement' | 'balance'; days: number }
  | { kind: 'none' };

/**
 * The trailing sub of an account row, by precedence: the 30-day change when the account moved,
 * else the main-currency equivalent of a foreign balance (`null` when its currency has no rate),
 * else how old a stale statement or balance is, else nothing.
 */
export function accountTrail(args: {
  currency: string;
  mainCurrency: string;
  last30: { net: number; count: number } | undefined;
  stamp: AccountSub['stamp'];
  todayDay: number;
  converted: number | null;
}): AccountTrail {
  const { currency, mainCurrency, last30, stamp, todayDay, converted } = args;
  if (last30 && last30.count > 0) return { kind: 'change', net: last30.net };
  if (currency !== mainCurrency) return { kind: 'converted', value: converted };
  if (stamp) {
    const days = daysBetween(stamp.day, todayDay);
    if (days > STALE_DAYS) return { kind: 'stale', what: stamp.kind, days };
  }
  return { kind: 'none' };
}

/**
 * The hero's change line: the 30-day net against the balance before it. `null` when there is
 * nothing to show — no movement, or no balance before it to relate it to.
 */
export function cashChange(
  balance: number,
  net30: number
): { delta: number; ratio: number } | null {
  const before = balance - net30;
  if (net30 === 0 || before <= 0) return null;
  return { delta: net30, ratio: net30 / before };
}

export type AccountSort = 'balance' | 'name' | 'change' | 'last';

/**
 * Accounts in the order the `···` menu picked: balance, 30-day change and last movement largest
 * first, name A to Z. Balances and changes compare in the main currency; an account whose
 * currency has no rate (`null`) sorts after every other. Ties keep their order.
 */
export function sortAccounts<
  T extends { id: string; name: string; currency: string; balance: string },
>(
  accounts: readonly T[],
  sort: AccountSort,
  ctx: {
    convert: (amount: number, currency: string) => number | null;
    activity: (id: string) => { net: number; lastDay: number | null } | undefined;
  }
): T[] {
  const balance = (a: T) =>
    ctx.convert(parseFloat(a.balance || '0') || 0, recordCurrency(a.currency));
  const change = (a: T) => ctx.convert(ctx.activity(a.id)?.net ?? 0, recordCurrency(a.currency));
  const lastDay = (a: T) => ctx.activity(a.id)?.lastDay ?? 0;
  const compare = (a: T, b: T): number => {
    switch (sort) {
      case 'name':
        return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
      case 'balance':
        return compareConverted(balance(b), balance(a));
      case 'change':
        return compareConverted(change(b), change(a));
      case 'last':
        return lastDay(b) - lastDay(a);
    }
  };
  return [...accounts].sort(compare);
}
