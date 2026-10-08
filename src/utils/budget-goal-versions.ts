/**
 * Budget limits are versioned by effective calendar month (UTC epoch of the
 * month's first day). These helpers pick the version the editor shows and
 * decide whether a save would change anything.
 */
import type { BudgetGoalVersion } from '@shared/schema';

/** The version that sets the limit for `month`, or null (none yet / removed). */
export function versionInEffect(
  versions: readonly BudgetGoalVersion[],
  month: number
): BudgetGoalVersion | null {
  let found: BudgetGoalVersion | null = null;
  for (const version of versions) {
    if (version.effectiveMonth <= month) found = version;
  }
  return found && found.amount != null ? found : null;
}

/** Versions that start after `month` (scheduled changes), oldest first. */
export function scheduledVersions(
  versions: readonly BudgetGoalVersion[],
  month: number
): BudgetGoalVersion[] {
  return versions.filter((version) => version.effectiveMonth > month);
}

/**
 * Month the editor starts on: the current month while a limit is in effect,
 * otherwise the first scheduled limit (a goal that only starts later).
 */
export function defaultEditMonth(versions: readonly BudgetGoalVersion[], nowMonth: number): number {
  if (versionInEffect(versions, nowMonth)) return nowMonth;
  const next = versions.find((v) => v.effectiveMonth > nowMonth && v.amount != null);
  return next?.effectiveMonth ?? nowMonth;
}

/** Saving `amount` from `month` would leave that month's limit as it is. */
export function isUnchangedGoalEdit(
  versions: readonly BudgetGoalVersion[],
  month: number,
  amount: number,
  currency: string
): boolean {
  const current = versionInEffect(versions, month);
  return current != null && current.currency === currency && Number(current.amount) === amount;
}

/** `YYYY-MM` for an `<input type="month">`, in UTC like the stored months. */
export function monthInputValue(monthStart: number): string {
  return new Date(monthStart * 1000).toISOString().slice(0, 7);
}

/** UTC epoch of the first day of an `<input type="month">` value. */
export function monthStartFromInput(value: string): number | null {
  if (!/^\d{4}-\d{2}$/.test(value)) return null;
  return Date.parse(`${value}-01T00:00:00Z`) / 1000;
}

/** UTC epoch of the first day of the current month. */
export function currentMonthStart(now: Date = new Date()): number {
  return Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1) / 1000;
}
