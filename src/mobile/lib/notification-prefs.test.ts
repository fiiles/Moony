import { describe, expect, it } from 'vitest';
import {
  DEFAULT_NOTIFICATION_PREFS,
  NOTIFICATION_PREFS_KEY,
  UPKEEP_DAYS,
  readNotificationPrefs,
  writeNotificationPrefs,
  type NotificationPrefs,
} from './notification-prefs';

/** A storage that remembers what was written (the two methods the lib needs). */
function memoryStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

describe('readNotificationPrefs', () => {
  it('gives the defaults on empty storage', () => {
    expect(readNotificationPrefs(memoryStorage())).toEqual(DEFAULT_NOTIFICATION_PREFS);
    expect(DEFAULT_NOTIFICATION_PREFS).toEqual({
      deadlines: true,
      upkeep: true,
      upkeepDay: 1,
      backup: true,
    });
  });

  it('gives the defaults without storage', () => {
    expect(readNotificationPrefs(undefined)).toEqual(DEFAULT_NOTIFICATION_PREFS);
    expect(readNotificationPrefs(null)).toEqual(DEFAULT_NOTIFICATION_PREFS);
  });

  it('falls back to the defaults for unparsable or non-object text', () => {
    for (const raw of ['', 'not json', '[]', 'null', '42', '"text"']) {
      expect(readNotificationPrefs(memoryStorage({ [NOTIFICATION_PREFS_KEY]: raw }))).toEqual(
        DEFAULT_NOTIFICATION_PREFS
      );
    }
  });

  it('falls back field by field: a day outside 1–28 becomes 1, the other fields stay', () => {
    const storage = memoryStorage({
      [NOTIFICATION_PREFS_KEY]: JSON.stringify({
        deadlines: false,
        upkeep: false,
        upkeepDay: 31,
        backup: false,
      }),
    });
    expect(readNotificationPrefs(storage)).toEqual({
      deadlines: false,
      upkeep: false,
      upkeepDay: 1,
      backup: false,
    });
  });

  it('keeps the valid fields next to wrongly typed ones', () => {
    const storage = memoryStorage({
      [NOTIFICATION_PREFS_KEY]: JSON.stringify({
        deadlines: 'yes',
        upkeep: false,
        upkeepDay: 14.5,
        backup: 0,
      }),
    });
    expect(readNotificationPrefs(storage)).toEqual({
      deadlines: true,
      upkeep: false,
      upkeepDay: 1,
      backup: true,
    });
  });

  it('accepts every day from 1 to 28 and rejects 0 and 29', () => {
    expect(UPKEEP_DAYS).toHaveLength(28);
    expect(UPKEEP_DAYS[0]).toBe(1);
    expect(UPKEEP_DAYS[27]).toBe(28);
    const day = (upkeepDay: number) =>
      readNotificationPrefs(
        memoryStorage({ [NOTIFICATION_PREFS_KEY]: JSON.stringify({ upkeepDay }) })
      ).upkeepDay;
    expect(day(28)).toBe(28);
    expect(day(15)).toBe(15);
    expect(day(0)).toBe(1);
    expect(day(29)).toBe(1);
  });

  it('survives storage that throws', () => {
    const throwing = {
      getItem: () => {
        throw new Error('blocked');
      },
    };
    expect(readNotificationPrefs(throwing)).toEqual(DEFAULT_NOTIFICATION_PREFS);
  });
});

describe('writeNotificationPrefs', () => {
  it('round-trips through the storage', () => {
    const storage = memoryStorage();
    const prefs: NotificationPrefs = {
      deadlines: false,
      upkeep: true,
      upkeepDay: 20,
      backup: false,
    };
    writeNotificationPrefs(storage, prefs);
    expect(readNotificationPrefs(storage)).toEqual(prefs);
  });

  it('does not throw on storage that cannot be written', () => {
    const throwing = {
      setItem: () => {
        throw new Error('full');
      },
    };
    expect(() => writeNotificationPrefs(throwing, DEFAULT_NOTIFICATION_PREFS)).not.toThrow();
    expect(() => writeNotificationPrefs(undefined, DEFAULT_NOTIFICATION_PREFS)).not.toThrow();
  });
});
