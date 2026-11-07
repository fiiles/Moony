import type { UpkeepRow } from './upkeep';

const DAY = 86_400;

/** How long "Připomenout za měsíc" puts a row away (the desktop's upkeep snooze is a month too). */
export const UPKEEP_SNOOZE_DAYS = 30;

/**
 * A phone-only Údržba row put away on this device: until a UTC day (a snooze) or for good (`null`:
 * hidden). A hidden "Nezařazené" row keeps the count it was hidden at, so it comes back once more
 * payments wait than then.
 */
export interface UpkeepDismissal {
  until: number | null;
  count?: number;
}

export type UpkeepDismissals = Readonly<Record<string, UpkeepDismissal>>;

/**
 * The key a phone-only row is put away under, or `null` for a row that stands for a desktop
 * milestone (those are hidden and snoozed through the backend, as on the desktop). A statement row's
 * key carries the account and its last import, so hiding it lasts until the next import; the
 * uncategorized row has one key.
 */
export function dismissKey(
  row: Pick<UpkeepRow, 'kind' | 'accountIds' | 'sinceDay'>
): string | null {
  switch (row.kind) {
    case 'statement_due':
      return `statement_due:${row.accountIds?.[0] ?? ''}:${row.sinceDay ?? ''}`;
    case 'uncategorized':
      return 'uncategorized';
    default:
      return null;
  }
}

/** The count a row stands for (the payments of the uncategorized row), 0 for the others. */
function countOf(row: Pick<UpkeepRow, 'title'>): number {
  const count = row.title.params.count;
  return typeof count === 'number' ? count : 0;
}

/**
 * Whether the row is put away on `todayDay`: a snooze until it runs out, a hidden statement row
 * for good (its key changes with the next import), a hidden uncategorized row while no more
 * payments wait than when it was hidden.
 */
export function isDismissed(
  row: Pick<UpkeepRow, 'kind' | 'accountIds' | 'sinceDay' | 'title'>,
  dismissals: UpkeepDismissals,
  todayDay: number
): boolean {
  const key = dismissKey(row);
  const dismissal = key === null ? undefined : dismissals[key];
  if (!dismissal) return false;
  if (dismissal.until !== null) return todayDay < dismissal.until;
  if (dismissal.count !== undefined) return countOf(row) <= dismissal.count;
  return true;
}

/** "Skrýt": the row is put away for good (an uncategorized one until more payments wait). */
export function hideRow(
  dismissals: UpkeepDismissals,
  row: Pick<UpkeepRow, 'kind' | 'accountIds' | 'sinceDay' | 'title'>
): UpkeepDismissals {
  const key = dismissKey(row);
  if (key === null) return dismissals;
  return {
    ...dismissals,
    [key]: row.kind === 'uncategorized' ? { until: null, count: countOf(row) } : { until: null },
  };
}

/** "Připomenout za měsíc": the row is put away for `UPKEEP_SNOOZE_DAYS` days from `todayDay`. */
export function snoozeRow(
  dismissals: UpkeepDismissals,
  row: Pick<UpkeepRow, 'kind' | 'accountIds' | 'sinceDay'>,
  todayDay: number
): UpkeepDismissals {
  const key = dismissKey(row);
  if (key === null) return dismissals;
  return { ...dismissals, [key]: { until: todayDay + UPKEEP_SNOOZE_DAYS * DAY } };
}

/** "Vrátit": the row's own entry goes, whatever it was. */
export function restoreRow(
  dismissals: UpkeepDismissals,
  row: Pick<UpkeepRow, 'kind' | 'accountIds' | 'sinceDay'>
): UpkeepDismissals {
  const key = dismissKey(row);
  if (key === null || !(key in dismissals)) return dismissals;
  const next = { ...dismissals };
  delete next[key];
  return next;
}

/**
 * The stored map without the snoozes that ran out, so the device's entry stays small. A hidden
 * statement row stays: its key names an import that may still be the account's newest.
 */
export function pruneDismissals(dismissals: UpkeepDismissals, todayDay: number): UpkeepDismissals {
  const next: Record<string, UpkeepDismissal> = {};
  for (const [key, dismissal] of Object.entries(dismissals)) {
    if (dismissal.until !== null && todayDay >= dismissal.until) continue;
    next[key] = dismissal;
  }
  return next;
}

/** The map as stored on the device; anything unreadable is an empty map. */
export function parseDismissals(text: string | null): UpkeepDismissals {
  if (!text) return {};
  try {
    const value: unknown = JSON.parse(text);
    if (typeof value !== 'object' || value === null || Array.isArray(value)) return {};
    const out: Record<string, UpkeepDismissal> = {};
    for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
      if (typeof entry !== 'object' || entry === null) continue;
      const { until, count } = entry as { until?: unknown; count?: unknown };
      if (until !== null && typeof until !== 'number') continue;
      out[key] = typeof count === 'number' ? { until, count } : { until };
    }
    return out;
  } catch {
    return {};
  }
}
