/**
 * Budget limits are versioned by effective calendar month (UTC epoch of the
 * month's first day). These helpers pick the version the editor shows and
 * decide whether a save would change anything.
 */
import type { BudgetGoal, BudgetGoalVersion } from '@shared/schema';

/** Validity of each stored version (ordered by effective month by the API). */
export function budgetVersionPeriods(versions: readonly BudgetGoalVersion[]) {
  return versions.map((version, index) => ({
    version,
    end: versions[index + 1] ? versions[index + 1].effectiveMonth - 1 : null,
  }));
}

export type BudgetOverviewStatus = 'active' | 'scheduled' | 'ended';

export interface BudgetLimitPeriod {
  version: BudgetGoalVersion;
  /** Last second of the interval, or null while it runs on. */
  end: number | null;
}

export interface BudgetOverviewRow {
  goal: BudgetGoal;
  status: BudgetOverviewStatus;
  /** The interval that represents the goal: in effect now, the first scheduled, or the last one. */
  main: BudgetLimitPeriod;
  /** Changes after `main` starts (a null amount ends the limit); empty for an ended goal. */
  upcoming: BudgetGoalVersion[];
  /** Earlier intervals with a limit, newest first. Gaps without a limit are left out. */
  history: BudgetLimitPeriod[];
}

/**
 * One row per goal for managing budgets: the limit in effect now (or the
 * first scheduled one, or the last one of an ended goal) with the earlier
 * limits behind it. Goals that never had a limit are skipped.
 */
export function budgetOverview(
  goals: readonly BudgetGoal[],
  nowMonth: number
): BudgetOverviewRow[] {
  const rows: BudgetOverviewRow[] = [];
  for (const goal of goals) {
    const limited = budgetVersionPeriods(goal.versions).filter((p) => p.version.amount != null);
    if (limited.length === 0) continue;
    const currentIndex = limited.findIndex(
      (p) => p.version.effectiveMonth <= nowMonth && (p.end == null || p.end >= nowMonth)
    );
    const scheduledIndex = limited.findIndex((p) => p.version.effectiveMonth > nowMonth);
    const status: BudgetOverviewStatus =
      currentIndex >= 0 ? 'active' : scheduledIndex >= 0 ? 'scheduled' : 'ended';
    const mainIndex =
      status === 'active'
        ? currentIndex
        : status === 'scheduled'
          ? scheduledIndex
          : limited.length - 1;
    const main = limited[mainIndex];
    rows.push({
      goal,
      status,
      main,
      upcoming:
        status === 'ended' ? [] : scheduledVersions(goal.versions, main.version.effectiveMonth),
      history: limited.slice(0, mainIndex).reverse(),
    });
  }
  return rows;
}

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
 * Changes after `month` that remain once `amount` is saved: the leading ones
 * that repeat the saved limit are merged into it by the backend.
 */
export function upcomingAfterSave(
  versions: readonly BudgetGoalVersion[],
  month: number,
  amount: number,
  currency: string
): BudgetGoalVersion[] {
  const upcoming = scheduledVersions(versions, month);
  const firstDifferent = upcoming.findIndex(
    (version) =>
      version.amount == null || version.currency !== currency || Number(version.amount) !== amount
  );
  return firstDifferent < 0 ? [] : upcoming.slice(firstDifferent);
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

/** Snapshot the selected interval when editing; changing its date must not change its value. */
export function budgetEditSource(
  goal: BudgetGoal,
  selectedMonth: number | undefined,
  nowMonth: number
) {
  const month = selectedMonth ?? defaultEditMonth(goal.versions, nowMonth);
  const version = versionInEffect(goal.versions, month);
  return {
    month,
    amount: version?.amount ?? goal.amount,
    currency: version?.currency ?? goal.currency,
  };
}

/**
 * Saving `amount` from `month` through `until` (or, open-ended, for good)
 * would leave every month of that range with the limit it already has.
 */
export function isUnchangedGoalEdit(
  versions: readonly BudgetGoalVersion[],
  month: number,
  amount: number,
  currency: string,
  until: number | null = null
): boolean {
  const current = versionInEffect(versions, month);
  if (current == null || current.currency !== currency || Number(current.amount) !== amount)
    return false;
  const last = intervalLastMonth(versions, month);
  return until == null ? last == null : last == null || last >= until;
}

/**
 * Limits that saving `amount` over `from`..`until` (null: for good) replaces,
 * so the user can confirm first. Equal limits merge instead and are not
 * listed; nor is anything while the range stays inside the edited interval.
 */
export function overwrittenPeriods(
  versions: readonly BudgetGoalVersion[],
  from: number,
  until: number | null,
  amount: number,
  currency: string,
  edited: { start: number; end: number | null } | null
): BudgetLimitPeriod[] {
  const insideEdited =
    edited != null &&
    from >= edited.start &&
    (edited.end == null || (until != null && until <= edited.end));
  if (insideEdited) return [];
  return budgetVersionPeriods(versions).filter(
    ({ version, end }) =>
      version.amount != null &&
      (version.currency !== currency || Number(version.amount) !== amount) &&
      (until == null || version.effectiveMonth <= until) &&
      (end == null || end >= from)
  );
}

/** First day of the last month of the interval containing `month`; null while it runs on. */
export function intervalLastMonth(
  versions: readonly BudgetGoalVersion[],
  month: number
): number | null {
  const next = versions.find((version) => version.effectiveMonth > month);
  if (!next) return null;
  const date = new Date(next.effectiveMonth * 1000);
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth() - 1, 1) / 1000;
}

/** `YYYY-MM` for an `<input type="month">`, in UTC like the stored months. */
export function monthInputValue(monthStart: number): string {
  return new Date(monthStart * 1000).toISOString().slice(0, 7);
}

/** UTC epoch of the first day of an `<input type="month">` value. */
export function monthStartFromInput(value: string): number | null {
  if (!/^\d{4}-\d{2}$/.test(value)) return null;
  const year = Number(value.slice(0, 4));
  const month = Number(value.slice(5));
  if (year < 1 || month < 1 || month > 12) return null;
  const timestamp = Date.parse(`${value}-01T00:00:00Z`) / 1000;
  return Number.isFinite(timestamp) ? timestamp : null;
}

/** UTC epoch of the first day of the current month. */
export function currentMonthStart(now: Date = new Date()): number {
  return Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1) / 1000;
}
