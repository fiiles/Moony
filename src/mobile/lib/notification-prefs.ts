/**
 * What the phone notifies about (Nastavení → Oznámení, spec §8.9): three switches and the day of
 * the monthly upkeep. A device setting like the background lock, so it lives in `localStorage` as
 * one JSON object and is read field by field: a missing, mistyped or out-of-range field is its
 * default, whatever else is stored. The reminders' own groups are not here: those are the
 * milestone mute list, which the desktop shares.
 */

/** localStorage: the JSON of `NotificationPrefs`. */
export const NOTIFICATION_PREFS_KEY = 'moony-notifications';

export interface NotificationPrefs {
  /** The 14-days-ahead and the day-before notice of every deadline. */
  deadlines: boolean;
  /** The monthly "time for data upkeep". */
  upkeep: boolean;
  /** Day of the month of the upkeep reminder (1–28, so every month has it). */
  upkeepDay: number;
  /** The reminder to make a backup while the last one is older than 30 days (or there is none). */
  backup: boolean;
}

export const DEFAULT_NOTIFICATION_PREFS: NotificationPrefs = {
  deadlines: true,
  upkeep: true,
  upkeepDay: 1,
  backup: true,
};

/** The days of the month the upkeep reminder can fall on: 1 to 28. */
export const UPKEEP_DAYS: readonly number[] = Array.from({ length: 28 }, (_, i) => i + 1);

function isUpkeepDay(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && UPKEEP_DAYS.includes(value);
}

/** The stored preferences; every field that is missing, unreadable or out of range is its default. */
export function readNotificationPrefs(
  storage: Pick<Storage, 'getItem'> | null | undefined
): NotificationPrefs {
  let stored: unknown;
  try {
    const raw = storage?.getItem(NOTIFICATION_PREFS_KEY);
    stored = raw ? JSON.parse(raw) : undefined;
  } catch {
    // Blocked storage or text that is not JSON: the defaults.
  }
  const fields =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  const flag = (name: 'deadlines' | 'upkeep' | 'backup') =>
    typeof fields[name] === 'boolean' ? fields[name] : DEFAULT_NOTIFICATION_PREFS[name];
  return {
    deadlines: flag('deadlines'),
    upkeep: flag('upkeep'),
    upkeepDay: isUpkeepDay(fields.upkeepDay)
      ? fields.upkeepDay
      : DEFAULT_NOTIFICATION_PREFS.upkeepDay,
    backup: flag('backup'),
  };
}

/** Stores the preferences. Storage that cannot be written leaves the previous ones in force. */
export function writeNotificationPrefs(
  storage: Pick<Storage, 'setItem'> | null | undefined,
  prefs: NotificationPrefs
): void {
  try {
    storage?.setItem(NOTIFICATION_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // Storage that cannot be written: the previous choice (or the default) stays.
  }
}
